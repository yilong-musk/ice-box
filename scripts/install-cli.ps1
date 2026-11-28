# SPDX-License-Identifier: GPL-3.0-or-later
# Install or uninstall the cross-platform headless ice-box CLI (`ice-box-cli`)
# on Windows (x64) from GitHub Releases.
#
# Quick install (PowerShell):
#   irm https://raw.githubusercontent.com/yilong-musk/ice-box/main/scripts/install-cli.ps1 | iex
#
# Customize via parameters or ICE_BOX_* environment variables:
#   $env:ICE_BOX_VERSION = "0.4.2"; irm ... | iex
#   .\install-cli.ps1 -Version 0.4.2 -InstallDir "$env:LOCALAPPDATA\ice-box-cli"
#   .\install-cli.ps1 -Uninstall

[CmdletBinding()]
param(
    [string]$Version = $env:ICE_BOX_VERSION,
    [string]$InstallDir = $(if ($env:ICE_BOX_INSTALL_DIR) { $env:ICE_BOX_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA "ice-box-cli" }),
    [string]$Repo = $(if ($env:ICE_BOX_REPO) { $env:ICE_BOX_REPO } else { "yilong-musk/ice-box" }),
    [switch]$NoVerify = ($env:ICE_BOX_NO_VERIFY -eq "1"),
    [switch]$NoPathUpdate = ($env:ICE_BOX_NO_PATH_UPDATE -eq "1"),
    [switch]$SkipSmoke = ($env:ICE_BOX_SKIP_SMOKE -eq "1"),
    [switch]$Uninstall = ($env:ICE_BOX_UNINSTALL -eq "1"),
    # Offline fixtures set this to exercise activation under pwsh on non-Windows
    # hosts. Published archives are Windows x64 only.
    [switch]$SkipArchCheck = ($env:ICE_BOX_SKIP_ARCH_CHECK -eq "1")
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
} catch {
    # PowerShell 7+ on modern .NET negotiates TLS automatically.
}

function Normalize-PathEntry {
    param([string]$Entry)
    return $Entry.Trim().TrimEnd('\', '/')
}

function Test-PlatformNotSupported {
    param($ErrorRecord)
    $current = $ErrorRecord.Exception
    while ($null -ne $current) {
        if ($current -is [System.PlatformNotSupportedException]) {
            return $true
        }
        $current = $current.InnerException
    }
    return $false
}

function Get-UserPathVariable {
    try {
        return [Environment]::GetEnvironmentVariable("Path", "User")
    } catch {
        # pwsh on macOS/Linux has no user-scope environment block. The call is
        # often wrapped in MethodInvocationException, so match the inner type.
        if (Test-PlatformNotSupported $_) {
            return $null
        }
        throw
    }
}

function Set-UserPathVariable {
    param([string]$Value)
    try {
        [Environment]::SetEnvironmentVariable("Path", $Value, "User")
        return $true
    } catch {
        if (Test-PlatformNotSupported $_) {
            return $false
        }
        throw
    }
}

function Add-ToUserPath {
    param([string]$Dir)
    $normalizedTarget = Normalize-PathEntry $Dir
    $currentUserPath = Get-UserPathVariable
    $entries = @()
    if (-not [string]::IsNullOrWhiteSpace($currentUserPath)) {
        $entries = @($currentUserPath.Split(';') | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    }
    $alreadyPresent = $false
    foreach ($entry in $entries) {
        if ((Normalize-PathEntry $entry).Equals($normalizedTarget, [System.StringComparison]::OrdinalIgnoreCase)) {
            $alreadyPresent = $true
            break
        }
    }
    if (-not $alreadyPresent) {
        $updated = ($entries + @($Dir)) -join ';'
        if (Set-UserPathVariable $updated) {
            Write-Host "Added $Dir to user PATH."
        }
    }

    $sessionEntries = @()
    if (-not [string]::IsNullOrWhiteSpace($env:Path)) {
        $sessionEntries = @($env:Path.Split(';') | Where-Object { -not [string]::IsNullOrWhiteSpace($_) })
    }
    $inSession = $false
    foreach ($entry in $sessionEntries) {
        if ((Normalize-PathEntry $entry).Equals($normalizedTarget, [System.StringComparison]::OrdinalIgnoreCase)) {
            $inSession = $true
            break
        }
    }
    if (-not $inSession) {
        $env:Path = "$Dir;$env:Path"
    }
}

function Remove-FromUserPath {
    param([string]$Dir)
    $normalizedTarget = Normalize-PathEntry $Dir
    $currentUserPath = Get-UserPathVariable
    if ([string]::IsNullOrWhiteSpace($currentUserPath)) {
        return
    }
    $kept = @(
        $currentUserPath.Split(';') |
            Where-Object {
                (-not [string]::IsNullOrWhiteSpace($_)) -and
                (-not (Normalize-PathEntry $_).Equals($normalizedTarget, [System.StringComparison]::OrdinalIgnoreCase))
            }
    )
    $updated = $kept -join ';'
    if ($updated -ne $currentUserPath) {
        if (Set-UserPathVariable $updated) {
            Write-Host "Removed $Dir from user PATH."
        }
    }
}

$currentDir = Join-Path $InstallDir "current"

if ($Uninstall) {
    Remove-FromUserPath $currentDir
    if (Test-Path -LiteralPath $InstallDir) {
        Remove-Item -LiteralPath $InstallDir -Recurse -Force
        Write-Host "Removed $InstallDir."
    } else {
        Write-Host "Nothing to remove under $InstallDir."
    }
    return
}

if ($Repo -notmatch '^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$') {
    throw "install-cli: invalid repository name: $Repo"
}

if (-not $SkipArchCheck) {
    if (-not [Environment]::Is64BitOperatingSystem) {
        throw "install-cli: prebuilt Windows CLI archives require 64-bit Windows (x64)."
    }

    $arch = $env:PROCESSOR_ARCHITECTURE
    if ($env:PROCESSOR_ARCHITEW6432) {
        $arch = $env:PROCESSOR_ARCHITEW6432
    }
    if ($arch -and $arch -notin @("AMD64", "x86_64")) {
        throw "install-cli: prebuilt Windows CLI archives currently ship for x64 only (detected: $arch)."
    }
}

function Resolve-LatestTag {
    param([string]$Repository)
    $latestUrl = if ($env:ICE_BOX_LATEST_URL) { $env:ICE_BOX_LATEST_URL } else { "https://github.com/$Repository/releases/latest" }
    try {
        $request = [System.Net.HttpWebRequest]::Create($latestUrl)
        $request.Method = "HEAD"
        $request.AllowAutoRedirect = $false
        $request.UserAgent = "ice-box-cli-installer"
        $response = $request.GetResponse()
        try {
            $location = $response.Headers["Location"]
        } finally {
            $response.Close()
        }
        if ($location -and $location -match '/releases/tag/([^/?#]+)') {
            return $Matches[1]
        }
    } catch [System.Net.WebException] {
        if ($_.Exception.Response) {
            $location = $_.Exception.Response.Headers["Location"]
            $_.Exception.Response.Close()
            if ($location -and $location -match '/releases/tag/([^/?#]+)') {
                return $Matches[1]
            }
        }
    }

    $apiUrl = "https://api.github.com/repos/$Repository/releases/latest"
    $release = Invoke-RestMethod -Uri $apiUrl -Headers @{ "User-Agent" = "ice-box-cli-installer"; "Accept" = "application/vnd.github+json" }
    if ($release -and $release.tag_name) {
        return [string]$release.tag_name
    }
    throw "install-cli: failed to resolve latest release tag from $latestUrl; pass -Version <vX.Y.Z> explicitly."
}

$tag = if ([string]::IsNullOrWhiteSpace($Version)) {
    Resolve-LatestTag -Repository $Repo
} else {
    "v" + ($Version.Trim().TrimStart('v', 'V'))
}

$bareVersion = $tag.Substring(1)
if ($bareVersion -notmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$') {
    throw "install-cli: invalid release version: $tag"
}

$stem = "ice-box-cli_${bareVersion}_windows_x86_64"
$archiveName = "${stem}.zip"
$baseUrl = if ($env:ICE_BOX_RELEASE_BASE_URL) {
    $env:ICE_BOX_RELEASE_BASE_URL.TrimEnd('/')
} else {
    "https://github.com/$Repo/releases/download/$tag"
}

$workDir = Join-Path ([IO.Path]::GetTempPath()) ("ice-box-cli-install-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $workDir -Force | Out-Null

try {
    $archivePath = Join-Path $workDir $archiveName
    $archiveUrl = "$baseUrl/$archiveName"
    Write-Host "Downloading $archiveName ($tag)..."
    try {
        Invoke-WebRequest -Uri $archiveUrl -OutFile $archivePath -UseBasicParsing -Headers @{ "User-Agent" = "ice-box-cli-installer" }
    } catch {
        throw "install-cli: failed to download $archiveUrl (CLI archives are published for releases after v0.4.1): $_"
    }

    if (-not $NoVerify) {
        Write-Host "Verifying SHA256SUMS..."
        $sumsPath = Join-Path $workDir "SHA256SUMS"
        $sumsUrl = "$baseUrl/SHA256SUMS"
        try {
            Invoke-WebRequest -Uri $sumsUrl -OutFile $sumsPath -UseBasicParsing -Headers @{ "User-Agent" = "ice-box-cli-installer" }
        } catch {
            throw "install-cli: failed to download $sumsUrl (pass -NoVerify to skip checksum check): $_"
        }

        $expectedSum = $null
        foreach ($line in Get-Content -LiteralPath $sumsPath) {
            if ($line -match '^([0-9a-fA-F]{64})\s+\*?(.+)$') {
                if ($Matches[2].Trim() -eq $archiveName) {
                    $expectedSum = $Matches[1].ToLowerInvariant()
                    break
                }
            }
        }
        if (-not $expectedSum) {
            throw "install-cli: SHA256SUMS does not contain an entry for $archiveName"
        }

        $actualSum = (Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash.ToLowerInvariant()
        if ($actualSum -ne $expectedSum) {
            throw "install-cli: SHA-256 mismatch for $archiveName (expected $expectedSum, got $actualSum)"
        }
    }

    $extractRoot = Join-Path $workDir "extract"
    Expand-Archive -LiteralPath $archivePath -DestinationPath $extractRoot -Force
    $extracted = Join-Path $extractRoot $stem
    $cliExe = Join-Path $extracted "ice-box-cli.exe"
    $resourcesDir = Join-Path $extracted "resources"
    if (-not (Test-Path -LiteralPath $cliExe) -or -not (Test-Path -LiteralPath $resourcesDir)) {
        throw "install-cli: archive $archiveName does not contain expected '$stem\ice-box-cli.exe' and '$stem\resources\'"
    }

    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    $stagingDir = Join-Path $InstallDir (".staging-" + [Guid]::NewGuid().ToString("N"))
    $backupDir = Join-Path $InstallDir (".previous-" + [Guid]::NewGuid().ToString("N"))

    Copy-Item -LiteralPath $extracted -Destination $stagingDir -Recurse -Force
    $stagedExe = Join-Path $stagingDir "ice-box-cli.exe"
    # Offline fixtures run under pwsh on macOS/Linux and ship a shebang script
    # named ice-box-cli.exe. Windows CreateProcess does not need this bit.
    if ($env:OS -ne "Windows_NT") {
        & chmod +x $stagedExe
        if ($LASTEXITCODE -ne 0) {
            Remove-Item -LiteralPath $stagingDir -Recurse -Force -ErrorAction SilentlyContinue
            throw "install-cli: failed to mark $stagedExe executable"
        }
    }
    # Prove the new tree runs before replacing the previous install.
    if (-not $SkipSmoke) {
        & $stagedExe --version
        if ($LASTEXITCODE -ne 0) {
            Remove-Item -LiteralPath $stagingDir -Recurse -Force -ErrorAction SilentlyContinue
            throw "install-cli: staged binary failed smoke check (exit $LASTEXITCODE)"
        }
    }

    $movedBackup = $false
    try {
        if (Test-Path -LiteralPath $currentDir) {
            Move-Item -LiteralPath $currentDir -Destination $backupDir -Force
            $movedBackup = $true
        }
        Move-Item -LiteralPath $stagingDir -Destination $currentDir -Force
        if (-not $NoPathUpdate) {
            Add-ToUserPath $currentDir
        }
        if ($movedBackup -and (Test-Path -LiteralPath $backupDir)) {
            Remove-Item -LiteralPath $backupDir -Recurse -Force
        }
    } catch {
        if (Test-Path -LiteralPath $stagingDir) {
            Remove-Item -LiteralPath $stagingDir -Recurse -Force -ErrorAction SilentlyContinue
        }
        if ($movedBackup -and (Test-Path -LiteralPath $backupDir)) {
            if (Test-Path -LiteralPath $currentDir) {
                Remove-Item -LiteralPath $currentDir -Recurse -Force -ErrorAction SilentlyContinue
            }
            if (-not (Test-Path -LiteralPath $currentDir)) {
                Move-Item -LiteralPath $backupDir -Destination $currentDir -Force -ErrorAction SilentlyContinue
            }
        }
        throw "install-cli: failed to activate installation at $currentDir (is ice-box-cli.exe still running?): $_"
    }

    Write-Host "Installed ice-box-cli ($tag) to $currentDir"
    if (-not $NoPathUpdate) {
        Write-Host "Open a new terminal if 'ice-box-cli' is not immediately found in existing shells."
    }
} finally {
    if (Test-Path -LiteralPath $workDir) {
        Remove-Item -LiteralPath $workDir -Recurse -Force -ErrorAction SilentlyContinue
    }
}

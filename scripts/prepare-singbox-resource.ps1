# SPDX-License-Identifier: GPL-3.0-or-later

# Ensure src-tauri/resources has the sing-box binary for the given platform.
# PowerShell mirror of scripts/prepare-singbox-resource.sh for Windows hosts
# without Git Bash. Usage: scripts/prepare-singbox-resource.ps1 [-Platform win]
param([string]$Platform = "win")

$ErrorActionPreference = "Stop"

$Root = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
$St = Join-Path $Root "apps/desktop/src-tauri"
$Version = (Get-Content (Join-Path $Root "third_party/sing-box/VERSION") -Raw).Trim()

switch ($Platform) {
  "win" { $TargetDir = "windows-x86_64"; $Bin = "sing-box.exe" }
  "mac-arm64" { $TargetDir = "darwin-aarch64"; $Bin = "sing-box" }
  "mac-x64" { $TargetDir = "darwin-x86_64"; $Bin = "sing-box" }
  default { throw "unknown platform: $Platform" }
}

$Src = Join-Path $Root "third_party/sing-box/$TargetDir/$Bin"
$DestDir = Join-Path $St "resources"
$Dest = Join-Path $DestDir $Bin

# A binary that cannot run on this host (cross-arch / cross-OS) prints nothing.
function Get-CoreVersion([string]$Path) {
  try {
    $line = & $Path version 2>$null | Select-Object -First 1
    if ($null -eq $line) { return "" }
    return [string]$line
  } catch {
    return ""
  }
}

function Get-SingBoxAsset([string]$Platform) {
  switch ($Platform) {
    "win" { return "sing-box-$Version-windows-amd64.zip" }
    "mac-arm64" { return "sing-box-$Version-darwin-arm64.tar.gz" }
    "mac-x64" { return "sing-box-$Version-darwin-amd64.tar.gz" }
    default { throw "unknown platform: $Platform" }
  }
}

# Mirror scripts/fetch-singbox.sh: download the pinned release, verify its
# checksum, and install it (plus the Windows archive companions) before
# staging. A stale core would otherwise fail later with an "unknown field"
# decode error because the generated config uses the pinned schema.
function Invoke-SingBoxFetch([string]$Platform) {
  $Asset = Get-SingBoxAsset $Platform
  $Url = "https://github.com/SagerNet/sing-box/releases/download/v$Version/$Asset"
  $Checksums = Join-Path $Root "third_party/sing-box/CHECKSUMS.sha256"
  if (-not (Test-Path $Checksums)) {
    Write-Error "Missing $Checksums (expected pinned SHA-256 list)"
    exit 1
  }
  $Expected = Get-Content $Checksums |
    ForEach-Object {
      $Parts = $_ -split '\s+'
      if ($Parts.Count -ge 2 -and $Parts[1] -eq $Asset) { $Parts[0] }
    } |
    Select-Object -First 1
  if (-not $Expected) {
    Write-Error "No SHA-256 entry for $Asset in $Checksums"
    exit 1
  }

  $Tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("ice-singbox-" + [guid]::NewGuid().ToString("N"))
  New-Item -ItemType Directory -Force -Path $Tmp | Out-Null
  try {
    $Archive = Join-Path $Tmp $Asset
    Write-Host "Fetching ${Platform}: $Url"
    # Windows PowerShell 5.1 defaults to older TLS versions on some hosts;
    # GitHub release downloads require TLS 1.2.
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    Invoke-WebRequest -Uri $Url -OutFile $Archive -UseBasicParsing
    $Actual = (Get-FileHash -Algorithm SHA256 -Path $Archive).Hash.ToLowerInvariant()
    if ($Actual -ne $Expected.ToLowerInvariant()) {
      Write-Error "SHA-256 mismatch for ${Asset}: expected $Expected, got $Actual"
      exit 1
    }
    Write-Host "SHA-256 ok: $Asset"

    $Extract = Join-Path $Tmp "extract"
    New-Item -ItemType Directory -Force -Path $Extract | Out-Null
    if ($Asset.EndsWith(".zip")) {
      Expand-Archive -Path $Archive -DestinationPath $Extract -Force
    } else {
      & tar -xzf $Archive -C $Extract
      if ($LASTEXITCODE -ne 0) {
        Write-Error "Failed to extract $Asset"
        exit 1
      }
    }
    $Found = Get-ChildItem -Path $Extract -Recurse -File -Filter $Bin | Select-Object -First 1
    if ($null -eq $Found) {
      Write-Error "Extracted binary not found in $Asset"
      exit 1
    }
    $DestDir = Split-Path -Parent $Src
    New-Item -ItemType Directory -Force -Path $DestDir | Out-Null
    Copy-Item $Found.FullName $Src -Force
    Write-Host "Installed $Src"

    if ($Platform -eq "win") {
      $Companion = Get-ChildItem -Path $Extract -Recurse -File -Filter "libcronet.dll" | Select-Object -First 1
      if ($null -ne $Companion) {
        Copy-Item $Companion.FullName (Join-Path $DestDir "libcronet.dll") -Force
        Write-Host "Installed $(Join-Path $DestDir 'libcronet.dll')"
      } else {
        Write-Warning "libcronet.dll not found in $Asset (NaiveProxy outbound will fail at runtime)"
      }
    }
  } finally {
    Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
  }
}

$NeedsFetch = $false
if (-not (Test-Path $Src)) {
  Write-Host "Missing $Src - fetching pinned $Version"
  $NeedsFetch = $true
} else {
  $SrcVersion = Get-CoreVersion $Src
  if (-not $SrcVersion) {
    Write-Host "note: cannot execute $Src on this host; skipping version check"
  } elseif ($SrcVersion -notmatch [regex]::Escape($Version)) {
    Write-Host "Stale $Src ($SrcVersion) - fetching pinned $Version"
    $NeedsFetch = $true
  }
}

if ($NeedsFetch) {
  Invoke-SingBoxFetch -Platform $Platform
}

if (-not (Test-Path $Src)) {
  Write-Error "Missing $Src after fetch - pin $Version"
  exit 1
}

# Never stage a core that does not match the pin: the generated config uses the
# pinned core's schema, so a stale binary fails at startup with an "unknown
# field" decode error.
$SrcVersion = Get-CoreVersion $Src
if ($SrcVersion -and $SrcVersion -notmatch [regex]::Escape($Version)) {
  Write-Error "Stale $Src ($SrcVersion) after fetch - expected pin $Version"
  exit 1
}

New-Item -ItemType Directory -Force -Path $DestDir | Out-Null
Copy-Item $Src $Dest -Force
Write-Host "Prepared resource $Dest for $Platform ($SrcVersion)"

# The shared tauri.conf.json also bundles `resources/sing-box` (unused at runtime on
# Windows, where the binary is resolved as sing-box.exe), so keep it present or the
# bundler aborts on the missing resource.
if ($Platform -eq "win") {
  $Bare = Join-Path $DestDir "sing-box"
  Copy-Item $Src $Bare -Force
  Write-Host "Prepared resource $Bare for $Platform (bundle entry compatibility)"

  # Plan B (scheduled-task elevation): build + bundle the TUN task launcher.
  # The workspace gate only touches an empty resource marker (build.rs) so
  # clippy/test do not pay for a release compile; this is the sole release
  # build of the launcher for the bundle.
  $LauncherExe = Join-Path $Root "target/release/ice-tun-launcher.exe"
  & cargo build --release -p ice-tun-launcher
  if (-not (Test-Path $LauncherExe)) {
    Write-Error "ice-tun-launcher build failed; the TUN scheduled-task elevation would be missing"
    exit 1
  }
  Copy-Item $LauncherExe (Join-Path $DestDir "ice-tun-launcher.exe") -Force
  Write-Host "Prepared resource ice-tun-launcher.exe for $Platform (TUN task elevation)"

  # Windows archive companion (Windows TUN packaging): the
  # NaiveProxy outbound needs libcronet.dll next to sing-box.exe. wintun.dll
  # is embedded in the pinned binary; the T0 spike re-verifies this before
  # the Windows TUN gate flips.
  $Cronet = Join-Path $DestDir "libcronet.dll"
  $CronetSrc = Join-Path $Root "third_party/sing-box/$TargetDir/libcronet.dll"
  if (Test-Path $CronetSrc) {
    Copy-Item $CronetSrc $Cronet -Force
    Write-Host "Prepared resource $Cronet for $Platform (Windows archive companion)"
  } else {
    Write-Warning "missing $CronetSrc - run scripts/fetch-singbox.sh win (NaiveProxy outbound will fail at runtime)"
  }
}

$GeoipSrc = Join-Path $Root "third_party/sing-geoip/rule-set"
$GeoipDest = Join-Path $DestDir "geoip"
if (Test-Path $GeoipSrc) {
  New-Item -ItemType Directory -Force -Path $GeoipDest | Out-Null
  Copy-Item (Join-Path $GeoipSrc "*.srs") $GeoipDest -Force
  $Count = (Get-ChildItem $GeoipDest -Filter *.srs).Count
  Write-Host "Prepared $GeoipDest ($Count geoip rule-sets)"
} else {
  Write-Error "missing $GeoipSrc - run scripts/fetch-geoip.sh (packaged app would silently drop GEOIP rules)"
  exit 1
}
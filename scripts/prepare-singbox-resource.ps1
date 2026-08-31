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

if (-not (Test-Path $Src)) {
  Write-Error "Missing $Src - run: npm run fetch-singbox -- $Platform (pin $Version)"
  exit 1
}

New-Item -ItemType Directory -Force -Path $DestDir | Out-Null
Copy-Item $Src $Dest -Force
Write-Host "Prepared resource $Dest for $Platform (pin $Version)"

# The shared tauri.conf.json also bundles `resources/sing-box` (unused at runtime on
# Windows, where the binary is resolved as sing-box.exe), so keep it present or the
# bundler aborts on the missing resource.
if ($Platform -eq "win") {
  $Bare = Join-Path $DestDir "sing-box"
  Copy-Item $Src $Bare -Force
  Write-Host "Prepared resource $Bare for $Platform (bundle entry compatibility)"

  # Windows archive companion (Windows TUN packaging, plan §5 T5): the
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
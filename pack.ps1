<#
.SYNOPSIS
  Packs the Windows launcher this fork builds: a folder and a zip with horizon-launcher.exe and
  install.ps1 (the same asset name install.ps1 downloads from a release).

.DESCRIPTION
  Run from the repository's folder:
    powershell -ExecutionPolicy Bypass -File pack.ps1

  It builds launcher/target/release/horizon-launcher.exe first (-SkipBuild reuses the last build),
  then writes dist/horizon-launcher-windows-x64.zip and the folder it was packed from. The zip is
  what a release's "horizon-launcher-windows-x64.zip" asset is, and install.ps1 -Exe inside the
  folder installs it without downloading anything.
#>
param(
    # Pack the last release build instead of building again.
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$Root = $PSScriptRoot
function Say($text) { Write-Host "  $text" }

$version = (Select-String -Path (Join-Path $Root "launcher\Cargo.toml") -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
$exe = Join-Path $Root "launcher\target\release\horizon-launcher.exe"

if (-not $SkipBuild) {
    $cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    if (-not (Test-Path $cargo)) { $cargo = (Get-Command cargo -ErrorAction Stop).Source }
    Say "Building the launcher (release)"
    Push-Location (Join-Path $Root "launcher")
    try {
        & $cargo build --release
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    } finally {
        Pop-Location
    }
}
if (-not (Test-Path $exe)) { throw "No $exe - build it first (no -SkipBuild)." }

$name = "Horizon-Offline-Java-Bridge-$version-win-x64"
$dist = Join-Path $Root "dist"
$dir = Join-Path $dist $name
Remove-Item $dir -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $dir | Out-Null

Copy-Item $exe (Join-Path $dir "horizon-launcher.exe")
Copy-Item (Join-Path $Root "install.ps1") $dir
Copy-Item (Join-Path $Root "packaging\windows\README-FIRST.txt") $dir

$zip = Join-Path $dist "horizon-launcher-windows-x64.zip"
Remove-Item $zip -Force -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $dir "*") -DestinationPath $zip

Say "Packed $name"
Say "Folder: $dir"
Say "Zip:    $zip"
Say "Install from the folder: powershell -ExecutionPolicy Bypass -File `"$dir\install.ps1`" -Exe `"$dir\horizon-launcher.exe`""

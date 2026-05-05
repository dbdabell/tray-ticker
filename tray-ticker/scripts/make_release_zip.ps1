# Run from the `tray-ticker` crate directory: .\scripts\make_release_zip.ps1
$ErrorActionPreference = "Stop"
$crate = Split-Path $PSScriptRoot
$exe = Join-Path $crate "target\release\tray-ticker.exe"

if (-not (Test-Path $exe)) {
    Write-Error "Missing $exe — run .\scripts\build-release.ps1 first (plain `cargo build` may send output to Cursor's sandbox target dir)."
}
$outDir = Join-Path $crate "dist"
New-Item -ItemType Directory -Force -Path $outDir | Out-Null
$zip = Join-Path $outDir "tray-ticker-windows-amd64.zip"
if (Test-Path $zip) { Remove-Item $zip }
Compress-Archive -Path @(
    $exe,
    (Join-Path $crate "README.md"),
    (Join-Path $crate "LICENSE")
) -DestinationPath $zip -Force
Write-Host "Wrote $zip"

# Always writes to <crate>\target\release\tray-ticker.exe (never a sandbox cache).
$ErrorActionPreference = "Stop"
$cargo = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
if (-not (Test-Path $cargo)) {
    Write-Error "cargo not found at $cargo — install Rust from rustup.rs"
}
$crateRoot = Split-Path $PSScriptRoot
$targetDir = Join-Path $crateRoot "target"

# Cursor / agent shells often set CARGO_TARGET_DIR — that bypasses this repo's target\.
Remove-Item Env:\CARGO_TARGET_DIR -ErrorAction SilentlyContinue

Push-Location $crateRoot
try {
    Write-Host "Building release into: $targetDir"
    & $cargo build --release --target-dir $targetDir
} finally {
    Pop-Location
}

$exe = Join-Path $targetDir "release\tray-ticker.exe"
if (-not (Test-Path $exe)) {
    Write-Error "Expected exe missing: $exe"
}
Write-Host ""
Get-Item $exe | Format-List FullName, Length, LastWriteTime

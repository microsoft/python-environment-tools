#!/usr/bin/env pwsh
# Rust pre-commit checks for PET workspace.
# Runs `cargo fmt --all` and `cargo clippy --all -- -D warnings`.
# Exits non-zero on any failure so callers can gate commits.

[CmdletBinding()]
param(
    [string]$WorkspacePath = (Get-Location).Path
)

$ErrorActionPreference = 'Stop'
Set-Location -Path $WorkspacePath

Write-Host '=== RUST PRE-COMMIT CHECKS ===' -ForegroundColor Cyan

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Error 'FAIL: cargo not found in PATH'
    exit 1
}

Write-Host ''
Write-Host '--- Step 1: cargo fmt --all ---' -ForegroundColor Cyan
& cargo fmt --all
if ($LASTEXITCODE -ne 0) {
    Write-Error 'FAIL: cargo fmt failed.'
    exit 1
}
Write-Host 'PASS: Formatting complete.' -ForegroundColor Green

Write-Host ''
Write-Host '--- Step 2: cargo clippy --all -- -D warnings ---' -ForegroundColor Cyan
& cargo clippy --all -- -D warnings
if ($LASTEXITCODE -ne 0) {
    Write-Error 'FAIL: clippy reported errors. Fix them, then re-run.'
    exit 1
}
Write-Host 'PASS: Clippy clean.' -ForegroundColor Green

Write-Host ''
Write-Host '=== ALL PRE-COMMIT CHECKS PASSED ===' -ForegroundColor Green
Write-Host 'Safe to commit.'

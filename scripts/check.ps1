#!/usr/bin/env pwsh
# Runs every check this project enforces.
#
# Usage:
#   ./scripts/check.ps1            # full check
#   ./scripts/check.ps1 -Fast      # skip the release build

[CmdletBinding()]
param(
    [switch]$Fast
)

$ErrorActionPreference = 'Stop'
Push-Location (Split-Path -Parent $PSScriptRoot)

$failed = [System.Collections.Generic.List[string]]::new()

function Invoke-Step {
    param(
        [string]$Name,
        [scriptblock]$Action
    )

    Write-Host ""
    Write-Host "==> $Name" -ForegroundColor Cyan
    & $Action
    if ($LASTEXITCODE -ne 0) {
        $failed.Add($Name)
        Write-Host "FAILED: $Name" -ForegroundColor Red
    }
}

try {
    # Formatting is not a lint, but a diff here means the tree is not canonical.
    Invoke-Step 'cargo fmt --check' { cargo fmt --all --check }

    # Clippy is the linter: all + pedantic + cargo groups, plus restriction
    # lints, configured in Cargo.toml and clippy.toml.
    Invoke-Step 'cargo clippy' { cargo clippy --all-targets -- -D warnings }

    Invoke-Step 'cargo test' { cargo test }

    # Dependency policy: advisories, licenses, banned crates, sources.
    if (Get-Command cargo-deny -ErrorAction SilentlyContinue) {
        Invoke-Step 'cargo deny' { cargo deny check }
    } else {
        Write-Host ""
        Write-Host "skipping cargo deny (not installed: cargo install --locked cargo-deny)" -ForegroundColor Yellow
    }

    if (-not $Fast) {
        Invoke-Step 'cargo build --release' { cargo build --release }
    }
} finally {
    Pop-Location
}

Write-Host ""
if ($failed.Count -eq 0) {
    Write-Host "all checks passed" -ForegroundColor Green
    exit 0
}

Write-Host "failed checks:" -ForegroundColor Red
$failed | ForEach-Object { Write-Host "  - $_" -ForegroundColor Red }
exit 1

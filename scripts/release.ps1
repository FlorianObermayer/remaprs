#!/usr/bin/env pwsh
# Builds the archive a release ships, plus its SHA256.
#
# Usage:
#   ./scripts/release.ps1            # cargo build --release --locked, then package
#   ./scripts/release.ps1 -SkipBuild # package target\release\remaprs.exe as it is
#
# The archive name comes from Cargo.toml, so bump the version there first. This
# script does not tag, commit or upload anything.

[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [string]$OutputDirectory = 'dist'
)

$ErrorActionPreference = 'Stop'
Push-Location (Split-Path -Parent $PSScriptRoot)

try {
    $manifest = cargo metadata --no-deps --format-version 1 | ConvertFrom-Json
    $package = $manifest.packages | Where-Object name -eq 'remaprs'
    if (-not $package) {
        throw 'cargo metadata did not report a remaprs package'
    }

    $version = $package.version
    $archiveName = "remaprs-$version-x86_64-pc-windows-gnu"

    if (-not $SkipBuild) {
        Write-Host "==> cargo build --release --locked" -ForegroundColor Cyan
        cargo build --release --locked
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed with exit code $LASTEXITCODE"
        }
    }

    # Stage exactly what a user needs to run the tool: the binary, the docs that
    # state the limitations, and one commented example config. The zip must not
    # depend on the rest of the repository being present.
    #
    # The example deliberately stays under examples\ and is *not* named
    # remaprs.yaml next to the exe: that name is auto-loaded, so unzipping and
    # double-clicking would silently start the demo bindings. `remaprs config
    # init` is the supported way to create one.
    $stage = Join-Path $OutputDirectory $archiveName
    if (Test-Path $stage) {
        Remove-Item -Recurse -Force $stage
    }
    New-Item -ItemType Directory -Force -Path (Join-Path $stage 'examples') | Out-Null

    Copy-Item 'target\release\remaprs.exe' $stage
    Copy-Item 'README.md', 'LICENSE' $stage
    Copy-Item 'examples\remaprs.yaml' (Join-Path $stage 'examples')

    $zipPath = Join-Path $OutputDirectory "$archiveName.zip"
    if (Test-Path $zipPath) {
        Remove-Item -Force $zipPath
    }
    # Archive the folder itself so unzipping gives one directory rather than
    # scattering four files into whatever the user extracted into.
    Compress-Archive -Path $stage -DestinationPath $zipPath -CompressionLevel Optimal

    # Written in sha256sum format so it can be verified on any platform.
    $hash = (Get-FileHash -Path $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $checksumPath = "$zipPath.sha256"
    [IO.File]::WriteAllText(
        (Join-Path (Resolve-Path -LiteralPath $OutputDirectory).Path "$archiveName.zip.sha256"),
        "$hash  $archiveName.zip`n",
        [Text.UTF8Encoding]::new($false)
    )

    $sizeMb = [math]::Round((Get-Item $zipPath).Length / 1MB, 2)
    Remove-Item -Recurse -Force $stage
    Write-Host ""
    Write-Host "packaged $archiveName ($sizeMb MB)" -ForegroundColor Green
    Write-Host "  archive  $zipPath"
    Write-Host "  SHA256   $hash"
    Write-Host "  also in  $checksumPath"
} finally {
    Pop-Location
}

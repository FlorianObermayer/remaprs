#!/usr/bin/env pwsh
# Writes the body of a GitHub release: what the archive is, how to verify it,
# and the changelog git-cliff derives from the commit history.
#
# Usage:
#   ./scripts/release-notes.ps1 -Version 0.1.0 -Build 0.1.0-42 -Commit <sha>
#   ./scripts/release-notes.ps1 -Version 0.1.0 -Build 0.1.0-42 -Kind nightly
#
# The version comes from Cargo.toml and the kind is decided by the workflow;
# this script only renders the text. git-cliff must be on PATH
# (`cargo install --locked git-cliff`) and the checkout must have the tags,
# otherwise the changelog covers the wrong range.

[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Version,
    [Parameter(Mandatory)][string]$Build,
    [string]$Commit = '',
    [ValidateSet('stable', 'nightly')][string]$Kind = 'stable',
    [string]$OutputPath = 'release-body.md',
    [string]$Repository = 'FlorianObermayer/remaprs'
)

$ErrorActionPreference = 'Stop'
Push-Location (Split-Path -Parent $PSScriptRoot)

try {
    if (-not (Get-Command git-cliff -ErrorAction SilentlyContinue)) {
        throw 'git-cliff is not installed: cargo install --locked git-cliff'
    }

    $archive = "remaprs-$Version-x86_64-pc-windows-gnu.zip"
    $provenance = "Base version: ``$Version`` | Build: ``$Build``"
    if ($Commit) {
        $short = $Commit.Substring(0, [Math]::Min(7, $Commit.Length))
        $provenance += " | Commit: [``$short``](https://github.com/$Repository/commit/$Commit)"
    }

    # A stable release names the version it ships; the nightly documents the
    # commits made since the last stable release, which is what "unreleased"
    # means once the stable tag exists.
    if ($Kind -eq 'stable') {
        $lines = @(
            'Windows x64, single executable, no runtime dependencies.'
            ''
            "Download ``$archive``, unzip it anywhere and run ``remaprs.exe``."
            'Verify the download against the attached `.zip.sha256`.'
            ''
            'The archive holds the executable, the README, the license and one commented example'
            'config; `scripts/release.ps1` builds the same layout locally. The binary is built'
            'from this commit by GitHub Actions on a clean Windows runner.'
            ''
            'remaprs is developed with AI assistance; see the "AI assistance" section of the'
            'README.'
            ''
            'Read the "Limitations" section before using it in a game: `SendInput` is'
            'foreground-only, elevation must match, and anti-cheat may block synthetic input.'
            ''
            $provenance
            ''
            "## What's Changed"
            ''
        )
        $changelogArgs = @('--unreleased', '--tag', "v$Version", '--strip', 'header')
    } else {
        $lines = @(
            'Automated build from `master`; **not a stable release**.'
            ''
            "Download ``$archive`` and verify it against the attached ``.zip.sha256``."
            'The archive holds the executable, the README, the license and one commented example'
            'config; `scripts/release.ps1` builds the same layout locally. The binary is built'
            'from this commit by GitHub Actions on a clean Windows runner.'
            ''
            $provenance
            ''
            "## Changes since v$Version"
            ''
        )
        $changelogArgs = @('--unreleased', '--strip', 'header')
    }

    # Captured rather than piped into the file so a failed run cannot leave a
    # body that the release step would then publish without a changelog.
    #
    # Without a token git-cliff would try to enrich commits with GitHub
    # metadata anonymously, which fails outright for a private repository. The
    # workflow supplies one; a local run falls back to the local history, which
    # is all the changelog needs.
    $changelogArgs += if ($env:GITHUB_TOKEN) { '--github-token', $env:GITHUB_TOKEN } else { '--offline' }

    # The template falls back to RELEASE_VERSION for the compare link when
    # git-cliff has no version to compare against, which is the nightly case:
    # there is no tag for "the next release" yet.
    $previousReleaseVersion = $env:RELEASE_VERSION
    $env:RELEASE_VERSION = if ($Kind -eq 'stable') { "v$Version" } else { 'nightly' }
    try {
        $changelog = & git-cliff @changelogArgs
        $cliffExit = $LASTEXITCODE
    } finally {
        $env:RELEASE_VERSION = $previousReleaseVersion
    }
    if ($cliffExit -ne 0) {
        throw "git-cliff failed with exit code $cliffExit"
    }

    # git-cliff opens with its own "## [<version>]" or "## [Unreleased]"
    # heading. The release body already has a section heading, and two stacked
    # headings read like a mistake, so drop the generated heading and the blank
    # lines immediately after it, keeping everything else.
    $changelog = @($changelog)
    if ($changelog.Count -gt 0 -and $changelog[0] -match '^##\s') {
        $index = 1
        while ($index -lt $changelog.Count -and $changelog[$index] -eq '') {
            $index++
        }
        $changelog = @($changelog | Select-Object -Skip $index)
    }

    # Set-Content would truncate correctly but writes a BOM by default on
    # Windows PowerShell, and `-Encoding utf8NoBOM` does not exist there. The
    # .NET call keeps the script working on both and matches release.ps1.
    $resolved = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputPath)
    [IO.File]::WriteAllText(
        $resolved,
        (($lines + $changelog) -join "`n") + "`n",
        [Text.UTF8Encoding]::new($false)
    )

    Write-Host "wrote $Kind release notes for $Version to $OutputPath" -ForegroundColor Green
} finally {
    Pop-Location
}

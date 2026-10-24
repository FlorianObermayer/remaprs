#!/usr/bin/env pwsh
# Bumps the crate version and refreshes Cargo.lock, so the only remaining step
# is committing and pushing.
#
# Usage:
#   ./scripts/bump-version.ps1          # patch bump: 0.1.0 -> 0.1.1
#   ./scripts/bump-version.ps1 minor    # 0.1.0 -> 0.2.0
#   ./scripts/bump-version.ps1 major    # 0.1.0 -> 1.0.0
#   ./scripts/bump-version.ps1 -DryRun  # print the change without writing it
#
# Pushing the bump commit to master is what publishes a release; see the
# "Releasing" section of README.md. This script does not commit, tag, push or
# upload anything.

[CmdletBinding()]
param(
    [Parameter(Position = 0)]
    [ValidateSet('patch', 'minor', 'major')]
    [string]$Part = 'patch',
    [switch]$DryRun
)

$ErrorActionPreference = 'Stop'
Push-Location (Split-Path -Parent $PSScriptRoot)

# Matches exactly one top-level `version = "x.y.z"` line: dependency entries are
# either `name = "1.2.3"` or inline tables, so this cannot hit the wrong line.
$versionPattern = [regex]'(?m)^version = "(?<major>\d+)\.(?<minor>\d+)\.(?<patch>\d+)"$'

function Get-ManifestVersion {
    param([string]$Text)
    # Not `$matches`: that is an automatic variable PowerShell populates for
    # `-match`, and shadowing it makes later matching behave unpredictably.
    $found = $versionPattern.Matches($Text)
    if ($found.Count -ne 1) {
        throw "expected exactly one version line in Cargo.toml, found $($found.Count)"
    }
    $match = $found[0]
    @(
        [int]$match.Groups['major'].Value
        [int]$match.Groups['minor'].Value
        [int]$match.Groups['patch'].Value
    )
}

try {
    $manifestPath = Join-Path (Get-Location) 'Cargo.toml'
    $text = [IO.File]::ReadAllText($manifestPath)
    $current = Get-ManifestVersion -Text $text
    $oldVersion = '{0}.{1}.{2}' -f $current[0], $current[1], $current[2]

    # `switch` would unroll the arrays into separate pipeline items, and the
    # comma operator binds before `+`, so each component gets its own variable
    # rather than one comma-separated expression.
    $major = $current[0]
    $minor = $current[1]
    $patch = $current[2]
    switch ($Part) {
        'major' { $major = $major + 1; $minor = 0; $patch = 0 }
        'minor' { $minor = $minor + 1; $patch = 0 }
        default { $patch = $patch + 1 }
    }
    $newVersion = "$major.$minor.$patch"

    # A bump mixed with unrelated work is a bump nobody can revert on its own.
    # Untracked files are ignored: scratch output is normal, staged work is not.
    # `git diff --quiet` exits 1 for "has changes" and above 1 for a real error
    # (not a repository, unreadable index), which must not be read as dirty.
    if (-not $DryRun) {
        foreach ($cached in @($false, $true)) {
            $gitArgs = if ($cached) { @('diff', '--cached', '--quiet') } else { @('diff', '--quiet') }
            git @gitArgs
            if ($LASTEXITCODE -eq 1) {
                throw 'the working tree has uncommitted changes; commit or stash them first'
            }
            if ($LASTEXITCODE -ne 0) {
                throw "git $($gitArgs -join ' ') failed with exit code $LASTEXITCODE"
            }
        }
    }

    Write-Host "version $oldVersion -> $newVersion ($Part)" -ForegroundColor Cyan

    if ($DryRun) {
        Write-Host '[dry-run] would rewrite the Cargo.toml version line'
        Write-Host '[dry-run] would run cargo update --workspace to refresh Cargo.lock'
        Write-Host ''
        Write-Host "next: commit, then push to master to release v$newVersion"
        return
    }

    $updated = $versionPattern.Replace($text, "version = `"$newVersion`"", 1)
    [IO.File]::WriteAllText($manifestPath, $updated, [Text.UTF8Encoding]::new($false))

    # Cargo only rewrites the lock entry for the crate itself when it re-resolves
    # the workspace, and a stale lock makes `--locked` builds fail.
    cargo update --workspace --quiet
    if ($LASTEXITCODE -ne 0) {
        throw "cargo update --workspace failed with exit code $LASTEXITCODE"
    }

    Write-Host ''
    git --no-pager diff --stat -- Cargo.toml Cargo.lock
    Write-Host ''
    Write-Host "bumped to $newVersion" -ForegroundColor Green
    Write-Host "  next: git commit -am 'chore: bump version to $newVersion'"
    Write-Host "  then: git push origin master   # publishes v$newVersion"
} finally {
    Pop-Location
}

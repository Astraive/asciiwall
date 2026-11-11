<#
.SYNOPSIS
Opt-in local Git hook installer for Astraive/asciiwall.

.DESCRIPTION
Sets ONLY the repository-local Git configuration `core.hooksPath` to `.githooks`.
Never modifies global Git configuration, never writes `user.name` or `user.email`,
never configures automatic sign-off or history rewriting, and never alters `.git/hooks/` files.
#>
[CmdletBinding()]
param(
    [switch]$Status,
    [switch]$Uninstall
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    throw 'Git CLI (`git`) is required on PATH.'
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$gitTopRaw = & git -C $repoRoot rev-parse --show-toplevel 2>$null
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($gitTopRaw)) {
    throw "Directory '$repoRoot' is not inside a Git working tree."
}

$gitTop = (Resolve-Path $gitTopRaw.Trim()).Path
if ($gitTop -ne $repoRoot) {
    throw "Expected repository root '$repoRoot', got Git top-level '$gitTop'."
}

$currentHooksPath = (& git -C $repoRoot config --local --get core.hooksPath 2>$null)
if ($LASTEXITCODE -ne 0) {
    $currentHooksPath = $null
}
else {
    $currentHooksPath = $currentHooksPath.Trim()
}

if ($Status) {
    if ([string]::IsNullOrEmpty($currentHooksPath)) {
        Write-Output 'Local core.hooksPath is not configured (using Git default).'
    }
    else {
        Write-Output "Local core.hooksPath = $currentHooksPath"
    }
    exit 0
}

if ($Uninstall) {
    if ($currentHooksPath -eq '.githooks') {
        & git -C $repoRoot config --local --unset core.hooksPath
        if ($LASTEXITCODE -ne 0) {
            throw 'Failed to unset local core.hooksPath.'
        }
        Write-Output 'Unset repository-local core.hooksPath.'
    }
    elseif ([string]::IsNullOrEmpty($currentHooksPath)) {
        Write-Output 'Repository-local core.hooksPath is already unset.'
    }
    else {
        Write-Output "Preserved existing custom local core.hooksPath '$currentHooksPath' (not '.githooks')."
    }
    exit 0
}

if (-not [string]::IsNullOrEmpty($currentHooksPath) -and $currentHooksPath -ne '.githooks') {
    throw "Existing local core.hooksPath '$currentHooksPath' was preserved. Remove it explicitly before opting into .githooks."
}

& git -C $repoRoot config --local core.hooksPath .githooks
if ($LASTEXITCODE -ne 0) {
    throw 'Failed to configure repository-local core.hooksPath.'
}

Write-Output 'Configured repository-local core.hooksPath = .githooks'
Write-Output 'No global Git settings or author identity (user.name / user.email) were modified.'
Write-Output 'Active hooks: .githooks/pre-commit and .githooks/commit-msg'

if ($IsLinux -or $IsMacOS) {
    Write-Output 'On POSIX systems, ensure hook scripts are executable (chmod +x .githooks/pre-commit .githooks/commit-msg).'
}

[CmdletBinding(SupportsShouldProcess = $true)]
param(
    [Parameter(Mandatory = $true)]
    [string]$Version,
    [string]$CommitMessage,
    [string]$Remote = "origin"
)

$ErrorActionPreference = "Stop"

function Invoke-Git {
    param(
        [Parameter(Mandatory = $true)]
        [string[]]$Arguments,
        [Parameter(Mandatory = $true)]
        [string]$Action
    )

    $commandText = "git " + ($Arguments -join " ")
    if ($PSCmdlet.ShouldProcess($commandText, $Action)) {
        & git @Arguments
        if ($LASTEXITCODE -ne 0) {
            throw "Git command failed: $commandText"
        }
    }
}

function Get-GitOutput {
    param(
        [Parameter(Mandatory = $true)]
        [string[]]$Arguments
    )

    $output = & git @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "Git command failed: git $($Arguments -join ' ')"
    }

    return ($output | Out-String).Trim()
}

$normalizedVersion = $Version.Trim()
if (-not $normalizedVersion.StartsWith("v")) {
    $normalizedVersion = "v$normalizedVersion"
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path

Push-Location $repoRoot
try {
    $currentBranch = Get-GitOutput @("rev-parse", "--abbrev-ref", "HEAD")
    if ($currentBranch -eq "HEAD") {
        throw "Cannot publish from a detached HEAD. Check out a branch first."
    }

    .\scripts\Assert-ReleaseTagMatchesVersion.ps1 -Tag $normalizedVersion

    $existingLocalTag = Get-GitOutput @("tag", "--list", $normalizedVersion)
    if (-not [string]::IsNullOrWhiteSpace($existingLocalTag)) {
        throw "Local tag '$normalizedVersion' already exists."
    }

    $existingRemoteTag = Get-GitOutput @("ls-remote", "--tags", $Remote, "refs/tags/$normalizedVersion")
    if (-not [string]::IsNullOrWhiteSpace($existingRemoteTag)) {
        throw "Remote tag '$normalizedVersion' already exists on '$Remote'."
    }

    if (-not [string]::IsNullOrWhiteSpace($CommitMessage)) {
        $worktreeStatus = Get-GitOutput @("status", "--porcelain")
        if ([string]::IsNullOrWhiteSpace($worktreeStatus)) {
            Write-Host "No local changes to commit before publishing." -ForegroundColor Yellow
        }
        else {
            Invoke-Git @("add", "-A") "Stage all repository changes"

            $stagedStatus = Get-GitOutput @("diff", "--cached", "--name-only")
            if ([string]::IsNullOrWhiteSpace($stagedStatus)) {
                Write-Host "No staged changes were produced by 'git add -A'." -ForegroundColor Yellow
            }
            else {
                Invoke-Git @("commit", "-m", $CommitMessage) "Create release preparation commit"
            }
        }
    }

    if (-not $WhatIfPreference) {
        $remainingStatus = Get-GitOutput @("status", "--porcelain")
        if (-not [string]::IsNullOrWhiteSpace($remainingStatus)) {
            throw "Worktree is not clean. Commit or stash changes before publishing the release tag."
        }
    }

    Invoke-Git @("push", $Remote, "HEAD") "Push current branch '$currentBranch' to '$Remote'"
    Invoke-Git @("tag", $normalizedVersion) "Create local release tag '$normalizedVersion'"
    Invoke-Git @("push", $Remote, $normalizedVersion) "Push release tag '$normalizedVersion' to '$Remote'"

    if ($WhatIfPreference) {
        Write-Host "Dry run completed for branch '$currentBranch' and tag '$normalizedVersion' on '$Remote'." -ForegroundColor Yellow
    }
    else {
        Write-Host "Published branch '$currentBranch' and tag '$normalizedVersion' to '$Remote'." -ForegroundColor Green
    }
}
finally {
    Pop-Location
}

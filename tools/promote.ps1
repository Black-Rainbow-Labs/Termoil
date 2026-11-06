<#
.SYNOPSIS
  Promotes a nightly Termoil to stable: the same files, published again on the stable channel.

.DESCRIPTION
  task-2524 (docs/decisions/updates.md). Every release tools/release.ps1 cuts is a nightly: its GitHub
  release is a prerelease and unluminous.com lists it in /termoil/nightly.json only, so nobody following
  the stable channel, which is everybody unless they chose otherwise, is offered it. Once a nightly has
  been used and has held up, this makes it the stable version. Nothing is built: the files the nightly
  published are published again with the sizes and SHA-256 recorded when they were, so the bytes a
  stable Termoil installs are the bytes somebody already ran.

  What it does:

    1. Finds the version: -Version, or the newest on https://unluminous.com/termoil/nightly.json. A
       version that is not on the nightly channel is refused.
    2. Runs scripts/termoil/publish-termoil.ps1 -Channel stable -Version x.y.z in the sites checkout:
       /termoil/stable.json and /termoil/latest.json name the version and the page offers it.
    3. Marks the GitHub release as a full release and the latest one, so GitHub's releases/latest, which
       a stable Termoil asks when unluminous.com does not answer, names it too.
    4. Asks unluminous.com and GitHub what they say, and exits 1 when one of them is wrong.

  Running it twice is harmless. An agent does not run it unless Jason asks.

.PARAMETER Version
  The version to promote. The newest on the nightly channel when left out.

.PARAMETER SkipSite
  Leave unluminous.com alone and only change the GitHub release.

.PARAMETER SkipGitHub
  Leave the GitHub release alone and only publish the site.

.PARAMETER WhatIf
  Say what would happen and change nothing.

.EXAMPLE
  pwsh tools/promote.ps1
  pwsh tools/promote.ps1 -Version 1.5.0
#>
[CmdletBinding()]
param(
    [string] $Version,
    [switch] $SkipSite,
    [switch] $SkipGitHub,
    [switch] $WhatIf
)

$ErrorActionPreference = 'Stop'
$Here = Split-Path -Parent $MyInvocation.MyCommand.Path
. (Join-Path $Here 'github.ps1')
$Origin = 'https://unluminous.com'

<#
.SYNOPSIS
  Prints a step heading.
.PARAMETER Message
  The heading.
#>
function Write-Step([string] $Message) {
    Write-Host ''
    Write-Host "==> $Message" -ForegroundColor Cyan
}

<#
.SYNOPSIS
  Reads a channel's manifest from unluminous.com, bypassing caches.
.PARAMETER Channel
  stable, nightly or latest.
#>
function Get-Manifest([string] $Channel) {
    return Invoke-RestMethod -Method GET "$Origin/termoil/$Channel.json" -TimeoutSec 15 -Headers @{ 'cache-control' = 'no-cache' }
}

<#
.SYNOPSIS
  The version to promote, refusing one that is not on the nightly channel.
.PARAMETER Nightly
  The nightly manifest.
#>
function Resolve-Version($Nightly) {
    $listed = @($Nightly.versions | ForEach-Object { $_.version })
    $wanted = if ($Version) { $Version.TrimStart('v') } else { $Nightly.version }
    if ($listed -notcontains $wanted) {
        throw "Termoil $wanted is not on the nightly channel ($($listed -join ', ')), so it cannot be promoted."
    }
    return $wanted
}

<#
.SYNOPSIS
  Asks unluminous.com and GitHub whether they name the version as stable, and answers the problems.
.PARAMETER Wanted
  The promoted version.
.PARAMETER Gh
  The GitHub CLI, or null when GitHub was skipped.
#>
function Test-Promoted([string] $Wanted, $Gh) {
    $problems = @()
    if (-not $SkipSite) {
        foreach ($name in 'stable', 'latest') {
            $m = Get-Manifest $name
            if ($m.version -ne $Wanted) { $problems += "/termoil/$name.json says $($m.version)" }
        }
    }
    if ($Gh) {
        $view = & $Gh release view "v$Wanted" -R $TermoilRepository --json isPrerelease | ConvertFrom-Json
        if ($view.isPrerelease) { $problems += "the GitHub release v$Wanted is still a prerelease" }
        $latest = Invoke-RestMethod "https://api.github.com/repos/$TermoilRepository/releases/latest" -TimeoutSec 15
        if ($latest.tag_name -ne "v$Wanted") { $problems += "GitHub's latest release is $($latest.tag_name)" }
    }
    return $problems
}

$nightly = Get-Manifest 'nightly'
$wanted = Resolve-Version $nightly
$site = Get-TermoilSite

if ($WhatIf) {
    Write-Host "Termoil promotion plan (nothing will be changed)" -ForegroundColor Yellow
    Write-Host "  Version:  $wanted (newest nightly is $($nightly.version))"
    Write-Host "  Site:     $(if ($SkipSite) { 'skipped' } elseif ($site) { "pwsh $site/scripts/termoil/publish-termoil.ps1 -Channel stable -Version $wanted" } else { 'the sites checkout was not found (set TERMOIL_SITE)' })"
    Write-Host "  GitHub:   $(if ($SkipGitHub) { 'skipped' } else { "gh release edit v$wanted --prerelease=false --latest" })"
    exit 0
}

$gh = if ($SkipGitHub) { $null } else { Get-GitHubCli }

if (-not $SkipSite) {
    if (-not $site) { throw 'The black-rainbow-labs-sites checkout was not found. Set TERMOIL_SITE or pass -SkipSite.' }
    Write-Step "Publishing Termoil $wanted on the stable channel of unluminous.com"
    & pwsh -NoProfile -File (Join-Path $site 'scripts/termoil/publish-termoil.ps1') -Channel stable -Version $wanted | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "publish-termoil.ps1 failed with exit code $LASTEXITCODE." }
}

if ($gh) {
    Write-Step "Marking the GitHub release v$wanted as the latest full release"
    & $gh release edit "v$wanted" -R $TermoilRepository --prerelease=false --latest | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "gh release edit failed with exit code $LASTEXITCODE." }
}

Write-Step 'Asking every destination what it says'
$problems = Test-Promoted $wanted $gh
if ($problems.Count -gt 0) {
    $problems | ForEach-Object { Write-Host "  $_" -ForegroundColor Red }
    exit 1
}
Write-Host "Termoil $wanted is stable." -ForegroundColor Green

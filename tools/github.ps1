<#
.SYNOPSIS
  The GitHub sign-in that tools/release.ps1 and tools/promote.ps1 share, and where the unluminous.com
  checkout is. Dot-source it: `. (Join-Path $PSScriptRoot 'github.ps1')`.
#>

# The public repository the releases are published on.
$TermoilRepository = 'Black-Rainbow-Labs/Termoil'

<#
.SYNOPSIS
  Returns the GitHub token: GH_TOKEN or GITHUB_TOKEN when set, otherwise the github.com credential
  git already uses to push (the credential manager), so gh needs no separate login.
#>
function Get-GitHubToken {
    if ($env:GH_TOKEN) { return $env:GH_TOKEN }
    if ($env:GITHUB_TOKEN) { return $env:GITHUB_TOKEN }
    # The request goes in through a file: a piped string does not reach git credential in a form it
    # accepts from every PowerShell. The file holds the protocol and host only; the answer, which holds
    # the secret, is only ever in memory.
    $ask = Join-Path ([System.IO.Path]::GetTempPath()) ('termoil-credential-' + [guid]::NewGuid().ToString('N') + '.txt')
    try {
        Set-Content -Path $ask -Value "protocol=https`nhost=github.com`n" -NoNewline -Encoding ascii
        $answer = & cmd /c "git credential fill < `"$ask`"" 2>$null
    } finally {
        Remove-Item -LiteralPath $ask -Force -ErrorAction SilentlyContinue
    }
    $line = $answer | Where-Object { $_ -like 'password=*' } | Select-Object -First 1
    if (-not $line) { throw 'No GitHub credential is stored for github.com. Run `gh auth login` (or set GH_TOKEN) and run this again.' }
    return $line.Substring('password='.Length)
}

<#
.SYNOPSIS
  Returns the GitHub CLI, authenticated with the token from Get-GitHubToken, failing early when it is
  missing or the token is refused.
#>
function Get-GitHubCli {
    $gh = Get-Command 'gh' -ErrorAction SilentlyContinue
    if (-not $gh) { throw 'The GitHub CLI (gh) is not installed. Install it from https://cli.github.com and run this again.' }
    $env:GH_TOKEN = Get-GitHubToken
    & $gh.Source auth status 2>&1 | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'gh auth status failed: the GitHub token was refused.' }
    return $gh.Source
}

<#
.SYNOPSIS
  The unluminous.com site folder in the black-rainbow-labs-sites checkout: TERMOIL_SITE when set,
  otherwise C:/jason/dev/black-rainbow-labs-sites/sites/unluminous. Returns null when it has no
  scripts/termoil/publish-termoil.ps1.
#>
function Get-TermoilSite {
    $site = if ($env:TERMOIL_SITE) { $env:TERMOIL_SITE } else { 'C:/jason/dev/black-rainbow-labs-sites/sites/unluminous' }
    if (Test-Path -LiteralPath (Join-Path $site 'scripts/termoil/publish-termoil.ps1')) { return $site }
    return $null
}

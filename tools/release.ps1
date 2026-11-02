<#
.SYNOPSIS
  Releases Terminus: test, bump the version, build, package, install on this machine, tag, push, and
  publish a GitHub release with the installers attached.

.DESCRIPTION
  Modelled on Unluminous's tools/release.ps1. Everything a person would otherwise have to remember is
  here, in the order it has to happen, stopping at the first thing that goes wrong.

    1. Refuses to run on a dirty checkout.
    2. Runs `cargo test --workspace --release`.
    3. Bumps `version` under `[workspace.package]` in Cargo.toml, the one place the version is written.
    4. Builds terminus.exe and terminus-gui.exe in release mode. TERMINUS_BUILD_DATE is set for the build
       so `terminus +version` and the About dialog can print the build date.
    5. Fetches the bundled ConPTY pair when installer/windows/conpty is missing.
    6. Compiles the Inno Setup installer into installer/dist/.
    7. Installs it silently on this machine and checks that the installed `terminus.exe +version`
       prints the new version.
    8. Builds the Linux AppImage and .deb in WSL (the macOS dmg comes from installer/macos/build-on-windows.ps1 when its preflight passes).
    9. Commits Cargo.toml and Cargo.lock as `Release v<version>`, tags `v<version>`, and pushes the
       branch and the tag.
   10. Creates the GitHub release with the installers attached, then downloads each asset back and
       compares its SHA256 with the local file.

  Every release is a prerelease until promoted. -Stable makes a full release.

.PARAMETER Part
  Which part of the version goes up: patch (the default), minor or major.

.PARAMETER Stable
  Create the GitHub release as a full release. Without it the release is a prerelease.

.PARAMETER Prerelease
  Create the GitHub release as a prerelease. This is the default; the switch exists so a command line
  can say so. -Stable overrides it.

.PARAMETER Notes
  The body of the GitHub release. Defaults to a line naming the version.

.PARAMETER SkipTests
  Do not run the test suite. For a release whose tests were just run by hand.

.PARAMETER SkipInstall
  Build the installer but do not install it on this machine.

.PARAMETER SkipPublish
  Stop after the tag: do not push and do not touch GitHub.

.PARAMETER WhatIf
  Print the plan and change nothing.

.EXAMPLE
  pwsh tools/release.ps1 -WhatIf
  pwsh tools/release.ps1 -Part minor -Notes "M2: renderer"
#>
[CmdletBinding()]
param(
    [ValidateSet('patch', 'minor', 'major')]
    [string] $Part = 'patch',
    [switch] $Prerelease = $true,
    [switch] $Stable,
    [string] $Notes,
    [switch] $SkipTests,
    [switch] $SkipInstall,
    [switch] $SkipPublish,
    [switch] $WhatIf
)

$ErrorActionPreference = 'Stop'

$Here = Split-Path -Parent $MyInvocation.MyCommand.Path
$Repo = (Resolve-Path (Join-Path $Here '..')).Path
$Manifest = Join-Path $Repo 'Cargo.toml'
$InstallerDir = Join-Path $Repo 'installer/windows'
$DistDir = Join-Path $Repo 'installer/dist'
$ConptyDir = Join-Path $InstallerDir 'conpty'
$ScratchDir = Join-Path $Repo '_agent_output/task-2458-installer'
$InstallDir = Join-Path $env:LOCALAPPDATA 'Programs/Terminus'

$script:Writing = $false

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
  Stops the release when the previous native command exited with a failure code.
.PARAMETER What
  Names the command in the error message.
.PARAMETER Body
  The command to run.
#>
function Invoke-Checked([string] $What, [scriptblock] $Body) {
    # The command's output goes to the console, not into the caller's return value: a function that
    # runs ISCC and then returns a path would otherwise return ISCC's whole log with the path.
    & $Body | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE." }
}

<#
.SYNOPSIS
  Runs a block with the CC environment variable removed, then puts it back.
.DESCRIPTION
  A CC variable naming cl.exe with no INCLUDE or LIB makes the cc crate skip its own Visual Studio
  lookup, so any crate with C code fails to build. The variable is the person's and is left alone;
  only the environment handed to cargo changes.
.PARAMETER Body
  The block to run.
#>
function Invoke-WithoutCc([scriptblock] $Body) {
    $had = Test-Path env:CC
    $was = if ($had) { $env:CC } else { $null }
    if ($had) { Remove-Item env:CC }
    try { & $Body } finally { if ($had) { $env:CC = $was } }
}

<#
.SYNOPSIS
  Returns the version written under [workspace.package] in Cargo.toml.
#>
function Get-CurrentVersion {
    $line = Select-String -Path $Manifest -Pattern '^\s*version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"' | Select-Object -First 1
    if (-not $line) { throw "No version = 'x.y.z' in $Manifest." }
    return $line.Matches[0].Groups[1].Value
}

<#
.SYNOPSIS
  Returns the next version for the part that is going up. A minor bump zeroes the patch and a major
  bump zeroes both.
.PARAMETER Current
  The current version, x.y.z.
.PARAMETER Which
  patch, minor or major.
#>
function Get-NextVersion([string] $Current, [string] $Which) {
    $parts = $Current.Split('.') | ForEach-Object { [int] $_ }
    switch ($Which) {
        'major' { return "$($parts[0] + 1).0.0" }
        'minor' { return "$($parts[0]).$($parts[1] + 1).0" }
        default { return "$($parts[0]).$($parts[1]).$($parts[2] + 1)" }
    }
}

<#
.SYNOPSIS
  Writes the new version into the first version line of Cargo.toml and refreshes Cargo.lock.
.PARAMETER New
  The new version, x.y.z.
#>
function Set-Version([string] $New) {
    $text = Get-Content -Raw -Path $Manifest
    $pattern = '(?m)^(\s*version\s*=\s*")([0-9]+\.[0-9]+\.[0-9]+)(")'
    $replaced = [regex]::new($pattern).Replace($text, "`${1}$New`${3}", 1)
    if ($replaced -eq $text) { throw "Could not write the version into $Manifest." }
    Set-Content -Path $Manifest -Value $replaced -NoNewline -Encoding utf8
    Invoke-Checked 'cargo metadata' { & cargo metadata --no-deps --format-version 1 --manifest-path $Manifest | Out-Null }
}

<#
.SYNOPSIS
  Returns the list of changed paths in the checkout, empty when it is clean.
#>
function Get-DirtyPaths {
    return @(& git -C $Repo status --porcelain)
}

<#
.SYNOPSIS
  Finds ISCC.exe, the Inno Setup 6 compiler, installing Inno Setup with winget when it is missing.
#>
function Get-InnoCompiler {
    $candidates = @(
        (Join-Path $env:LOCALAPPDATA 'Programs/Inno Setup 6/ISCC.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6/ISCC.exe'),
        (Join-Path $env:ProgramFiles 'Inno Setup 6/ISCC.exe')
    )
    foreach ($candidate in $candidates) { if (Test-Path -LiteralPath $candidate) { return $candidate } }
    $onPath = Get-Command 'ISCC.exe' -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    Write-Host 'Inno Setup 6 is not installed. Installing it with winget.'
    & winget install --id JRSoftware.InnoSetup -e --accept-package-agreements --accept-source-agreements --disable-interactivity
    foreach ($candidate in $candidates) { if (Test-Path -LiteralPath $candidate) { return $candidate } }
    throw 'Inno Setup was installed but ISCC.exe could not be found afterwards.'
}

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
    $ask = Join-Path ([System.IO.Path]::GetTempPath()) ('terminus-credential-' + [guid]::NewGuid().ToString('N') + '.txt')
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
    Invoke-Checked 'gh auth status' { & $gh.Source auth status 2>&1 | Out-Null }
    return $gh.Source
}

<#
.SYNOPSIS
  Runs the whole workspace test suite in release mode.
#>
function Invoke-TestSuite {
    Write-Step 'Running cargo test --workspace --release'
    Invoke-WithoutCc { Invoke-Checked 'cargo test' { & cargo test --workspace --release --manifest-path $Manifest } }
}

<#
.SYNOPSIS
  Builds terminus.exe and terminus-gui.exe in release mode with the build date in the environment.
.PARAMETER BuildDate
  The date text that build.rs reads from TERMINUS_BUILD_DATE.
#>
function Build-WindowsBinaries([string] $BuildDate) {
    Write-Step 'Building terminus.exe and terminus-gui.exe'
    $env:TERMINUS_BUILD_DATE = $BuildDate
    Invoke-WithoutCc {
        Invoke-Checked 'cargo build' { & cargo build --release -p terminus-cli -p terminus-app --manifest-path $Manifest }
    }
    foreach ($name in 'terminus.exe', 'terminus-gui.exe') {
        if (-not (Test-Path -LiteralPath (Join-Path $Repo "target/release/$name"))) { throw "The build did not produce $name." }
    }
}

<#
.SYNOPSIS
  Fetches the bundled ConPTY pair when either file is missing.
#>
function Ensure-Conpty {
    $missing = -not (Test-Path -LiteralPath (Join-Path $ConptyDir 'conpty.dll')) -or -not (Test-Path -LiteralPath (Join-Path $ConptyDir 'OpenConsole.exe'))
    if (-not $missing) { return }
    Write-Step 'Fetching the bundled ConPTY'
    Invoke-Checked 'fetch-conpty' { & pwsh -NoProfile -File (Join-Path $Here 'fetch-conpty.ps1') }
}

<#
.SYNOPSIS
  Compiles the Inno Setup installer and returns its path.
.PARAMETER Version
  The version baked into the installer.
#>
function Build-WindowsInstaller([string] $Version) {
    Write-Step 'Compiling the Windows installer'
    Ensure-Conpty
    $iscc = Get-InnoCompiler
    New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
    Invoke-Checked 'ISCC' { & $iscc "/DAppVersion=$Version" "/DOutDir=$DistDir" (Join-Path $InstallerDir 'terminus.iss') }
    $installer = Join-Path $DistDir "TerminusSetup-$Version-x64.exe"
    if (-not (Test-Path -LiteralPath $installer)) { throw "ISCC did not produce $installer." }
    return $installer
}

<#
.SYNOPSIS
  Installs the installer silently for the current user and checks the installed terminus.exe prints
  the new version.
.PARAMETER Installer
  Path of the installer.
.PARAMETER Version
  The version terminus.exe +version must print.
#>
function Install-OnThisMachine([string] $Installer, [string] $Version) {
    Write-Step 'Installing on this machine'
    # Setup hands the work to a second process and returns at once, so wait for the whole tree.
    $proc = Start-Process -FilePath $Installer -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CURRENTUSER' -Wait -PassThru
    if ($proc.ExitCode -ne 0) { throw "The installer exited with code $($proc.ExitCode)." }
    $exe = Join-Path $InstallDir 'terminus.exe'
    if (-not (Test-Path -LiteralPath $exe)) { throw "$exe is not there after the install." }
    $printed = (& $exe +version) -join "`n"
    Write-Host "terminus.exe +version: $printed"
    if ($printed -notmatch [regex]::Escape($Version)) { throw "The installed terminus.exe printed '$printed', which does not contain $Version." }
}

<#
.SYNOPSIS
  Builds the macOS dmg with installer/macos/build-on-windows.ps1 and returns its path.
.DESCRIPTION
  The script builds both Apple architectures with the cross toolchain, signs with the Developer ID,
  notarises, staples and writes a universal dmg to installer/dist. It runs after the version bump,
  so it reads the new version from Cargo.toml. When its -Preflight reports that something is missing
  (the SDK, the cross tools, the signing identity, the notary credential or the dmg tools in WSL)
  the step is skipped with the reason and returns nothing.
#>
function Build-MacosPackage {
    $script = Join-Path $Repo 'installer/macos/build-on-windows.ps1'
    $problems = (& pwsh -NoProfile -File $script -Preflight) -join ' '
    if ($LASTEXITCODE -ne 0) {
        Write-Host "macOS dmg: skipped, because $problems"
        return @()
    }
    Write-Step 'Building, signing and notarising the macOS dmg'
    # Every line goes to the console and is kept, because the script's last line names the dmg.
    $printed = @(& pwsh -NoProfile -File $script -Notarize 2>&1 | ForEach-Object { Write-Host $_; "$_" })
    if ($LASTEXITCODE -ne 0) { throw "installer/macos/build-on-windows.ps1 failed with exit code $LASTEXITCODE." }
    $dmgLine = $printed | Where-Object { $_ -like 'DMG: *' } | Select-Object -Last 1
    if (-not $dmgLine) { throw 'installer/macos/build-on-windows.ps1 did not print the dmg path.' }
    $dmg = $dmgLine.Substring(5)
    if (-not (Test-Path -LiteralPath $dmg)) { throw "The macOS build did not produce $dmg." }
    return @($dmg)
}

<#
.SYNOPSIS
  Turns a Windows path on a drive into the path WSL sees, such as C:\a\b into /mnt/c/a/b.
.PARAMETER Path
  The Windows path.
#>
function ConvertTo-WslPath([string] $Path) {
    $full = [System.IO.Path]::GetFullPath($Path)
    return '/mnt/' + $full.Substring(0, 1).ToLower() + $full.Substring(2).Replace('\', '/')
}

<#
.SYNOPSIS
  Builds the Linux AppImage and .deb in WSL with installer/linux/build.sh and returns their paths.
.DESCRIPTION
  The script runs under wsl.exe from the Windows checkout (as /mnt/c/...), builds the release
  binaries with the Linux toolchain into a target folder on the Linux file system, and writes both
  packages to installer/dist. Without WSL the step is skipped with a message and returns nothing.
.PARAMETER Version
  The version being released, which names the two packages.
.PARAMETER BuildDate
  The date text the About box shows, passed to the build as TERMINUS_BUILD_DATE.
#>
function Build-LinuxPackages([string] $Version, [string] $BuildDate) {
    if (-not (Get-Command wsl.exe -ErrorAction SilentlyContinue)) {
        Write-Host 'Linux AppImage and .deb: wsl.exe is not available, skipping.'
        return @()
    }
    Write-Step 'Building the Linux AppImage and .deb in WSL'
    $script = ConvertTo-WslPath (Join-Path $Repo 'installer/linux/build.sh')
    $env:TERMINUS_BUILD_DATE = $BuildDate
    $env:TERMINUS_VERSION = $Version
    $env:WSLENV = 'TERMINUS_BUILD_DATE:TERMINUS_VERSION'
    Invoke-Checked 'installer/linux/build.sh' { & wsl.exe -e bash $script }
    $packages = @("terminus-$Version-x86_64.AppImage", "terminus_${Version}_amd64.deb") | ForEach-Object { Join-Path $DistDir $_ }
    foreach ($p in $packages) { if (-not (Test-Path -LiteralPath $p)) { throw "The Linux build did not produce $p." } }
    return $packages
}

<#
.SYNOPSIS
  Commits the version bump, tags it, and pushes the branch and the tag.
.PARAMETER Version
  The released version.
#>
function Publish-Tag([string] $Version) {
    Write-Step "Committing, tagging v$Version and pushing"
    Invoke-Checked 'git add' { & git -C $Repo add -- Cargo.toml Cargo.lock }
    Invoke-Checked 'git commit' { & git -C $Repo commit -m "Release v$Version" }
    $script:Writing = $false
    Invoke-Checked 'git tag' { & git -C $Repo tag "v$Version" }
    if ($SkipPublish) { return }
    Invoke-Checked 'git push' { & git -C $Repo push origin HEAD }
    Invoke-Checked 'git push tag' { & git -C $Repo push origin "v$Version" }
}

<#
.SYNOPSIS
  Creates the GitHub release with the assets attached, then downloads each asset back and compares
  its SHA256 with the local file.
.PARAMETER Gh
  Path of the GitHub CLI.
.PARAMETER Version
  The released version.
.PARAMETER Assets
  Paths of the files to attach.
#>
function Publish-GitHubRelease([string] $Gh, [string] $Version, [string[]] $Assets) {
    Write-Step "Creating the GitHub release v$Version"
    # Not named $body: inside Invoke-Checked that name is its own scriptblock parameter, which
    # PowerShell would pass to gh as an encoded command.
    $releaseNotes = if ($Notes) { $Notes } else { "Terminus $Version" }
    # @() around the if: an if expression unrolls a one item array to a string, and splatting a
    # string passes it one character at a time.
    $releaseFlags = @(if (-not $Stable) { '--prerelease' })
    Invoke-Checked 'gh release create' { & $Gh release create "v$Version" @Assets --title "Terminus $Version" --notes $releaseNotes @releaseFlags }
    Test-DownloadedAssets $Gh $Version $Assets
}

<#
.SYNOPSIS
  Downloads each release asset and fails when its SHA256 differs from the local file.
.PARAMETER Gh
  Path of the GitHub CLI.
.PARAMETER Version
  The released version.
.PARAMETER Assets
  Paths of the local files that were uploaded.
#>
function Test-DownloadedAssets([string] $Gh, [string] $Version, [string[]] $Assets) {
    Write-Step 'Verifying the downloaded assets'
    $folder = Join-Path $ScratchDir "verify-$Version"
    New-Item -ItemType Directory -Force -Path $folder | Out-Null
    foreach ($asset in $Assets) {
        $name = Split-Path -Leaf $asset
        Invoke-Checked "gh release download $name" { & $Gh release download "v$Version" --pattern $name --dir $folder --clobber }
        $local = (Get-FileHash -Algorithm SHA256 -LiteralPath $asset).Hash
        $remote = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $folder $name)).Hash
        if ($local -ne $remote) { throw "$name downloaded from GitHub has SHA256 $remote but the local file has $local." }
        Write-Host "  $name  SHA256 $local  matches"
    }
}

$current = Get-CurrentVersion
$next = Get-NextVersion $current $Part
$buildDate = (Get-Date).ToUniversalTime().ToString('yyyy-MM-dd')
$kind = if ($Stable) { 'stable release' } else { 'prerelease' }

if ($WhatIf) {
    Write-Host "Terminus release plan (nothing will be changed)" -ForegroundColor Yellow
    Write-Host "  Version:      $current -> $next ($Part)"
    Write-Host "  GitHub:       v$next as a $kind"
    Write-Host "  Build date:   $buildDate (passed to cargo as TERMINUS_BUILD_DATE)"
    $dirty = Get-DirtyPaths
    if ($dirty.Count -gt 0) { Write-Host "  Checkout:     DIRTY ($($dirty.Count) changed paths), a real run would refuse" -ForegroundColor Red } else { Write-Host '  Checkout:     clean' }
    $iscc = @(
        (Join-Path $env:LOCALAPPDATA 'Programs/Inno Setup 6/ISCC.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6/ISCC.exe'),
        (Join-Path $env:ProgramFiles 'Inno Setup 6/ISCC.exe')
    ) | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
    if ($iscc) { Write-Host "  Inno Setup:   $iscc" } else { Write-Host '  Inno Setup:   not found, a real run installs it with winget' }
    $conptyThere = (Test-Path -LiteralPath (Join-Path $ConptyDir 'conpty.dll')) -and (Test-Path -LiteralPath (Join-Path $ConptyDir 'OpenConsole.exe'))
    Write-Host "  ConPTY:       $(if ($conptyThere) { 'present' } else { 'missing, a real run runs tools/fetch-conpty.ps1' })"
    Write-Host '  Steps:'
    Write-Host '    1. refuse a dirty checkout'
    Write-Host "    2. $(if ($SkipTests) { 'skip' } else { 'run' }) cargo test --workspace --release"
    Write-Host "    3. bump [workspace.package].version to $next in Cargo.toml"
    Write-Host '    4. cargo build --release -p terminus-cli -p terminus-app'
    Write-Host '    5. fetch ConPTY if missing, compile installer/windows/terminus.iss with ISCC'
    Write-Host "    6. $(if ($SkipInstall) { 'skip the install on this machine' } else { "install silently into $InstallDir and check terminus.exe +version prints $next" })"
    Write-Host '    7. macOS dmg: installer/macos/build-on-windows.ps1 -Notarize when its -Preflight passes, otherwise skipped with the reason; Linux AppImage and .deb: built in WSL by installer/linux/build.sh when wsl.exe exists'
    Write-Host "    8. commit 'Release v$next', tag v$next, push the branch and the tag"
    Write-Host "    9. gh release create v$next with $(Join-Path $DistDir "TerminusSetup-$next-x64.exe") ($kind)"
    Write-Host '   10. download each asset back and compare SHA256 with the local file'
    exit 0
}

# From here on a failure after the version bump puts Cargo.toml and Cargo.lock back.
trap {
    if ($script:Writing) {
        & git -C $Repo checkout -- Cargo.toml Cargo.lock
        Write-Host ''
        Write-Host 'The release stopped, and Cargo.toml and Cargo.lock were put back as they were.' -ForegroundColor Yellow
        $script:Writing = $false
    }
    break
}

Write-Step 'Checking the checkout'
$dirty = Get-DirtyPaths
if ($dirty.Count -gt 0) {
    $dirty | ForEach-Object { Write-Host "  $_" }
    throw 'The checkout has uncommitted changes. Commit them first: a release built from a dirty tree cannot be rebuilt.'
}
$gh = $null
if (-not $SkipPublish) { $gh = Get-GitHubCli; Write-Host "GitHub CLI: $gh (signed in)" }

if (-not $SkipTests) { Invoke-TestSuite }

Write-Step "Bumping $current to $next"
$script:Writing = $true
Set-Version $next

Build-WindowsBinaries $buildDate
$installer = Build-WindowsInstaller $next
if (-not $SkipInstall) { Install-OnThisMachine $installer $next }

$assets = @($installer) + (Build-MacosPackage) + (Build-LinuxPackages $next $buildDate)

Publish-Tag $next
if ($SkipPublish) { Write-Host "Tagged v$next locally. Skipped the push and GitHub because of -SkipPublish."; exit 0 }
Publish-GitHubRelease $gh $next $assets
Write-Host ''
Write-Host "Released Terminus v$next ($kind)." -ForegroundColor Green

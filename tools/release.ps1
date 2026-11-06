<#
.SYNOPSIS
  Releases Termoil: test, bump the version, build, package, install on this machine, tag, push, and
  publish a GitHub release with the installers attached.

.DESCRIPTION
  Modelled on Unluminous's tools/release.ps1. Everything a person would otherwise have to remember is
  here, in the order it has to happen, stopping at the first thing that goes wrong.

    1. Refuses to run on a dirty checkout.
    2. Runs `cargo test --workspace --release`.
    3. Bumps `version` under `[workspace.package]` in Cargo.toml, the one place the version is written.
    4. Builds termoil.exe and termoil-gui.exe in release mode. TERMOIL_BUILD_DATE is set for the build
       so `termoil +version` and the About dialog can print the build date.
    5. Fetches the bundled ConPTY pair when installer/windows/conpty is missing.
    6. Compiles the Inno Setup installer into installer/dist/.
    7. Installs it silently on this machine and checks that the installed `termoil.exe +version`
       prints the new version.
    8. Builds the Linux AppImage and .deb in WSL (the macOS dmg comes from installer/macos/build-on-windows.ps1 when its preflight passes).
    9. Commits Cargo.toml and Cargo.lock as `Release v<version>`, tags `v<version>`, and pushes the
       branch and the tag.
   10. Creates the GitHub release with the installers attached, then downloads each asset back and
       compares its SHA256 with the local file.
   11. Writes the winget manifest for the release into installer/winget/manifests. It is not committed or
       submitted; installer/winget/README.md says how a person publishes it.
   12. Publishes unluminous.com/termoil with scripts/termoil/publish-termoil.ps1 in the
       black-rainbow-labs-sites checkout (TERMOIL_SITE, or C:/jason/dev/black-rainbow-labs-sites/sites/unluminous):
       the version goes on the nightly channel, /termoil/nightly.json, and the page names it beside the
       stable downloads. A failure here exits 1 with the command to run again; the GitHub release stays.

  Every release is a nightly: a GitHub prerelease on the nightly channel (docs/decisions/updates.md).
  tools/promote.ps1 makes a nightly stable after it has been used. -Stable releases and promotes at once.

.PARAMETER Part
  Which part of the version goes up: patch (the default), minor or major.

.PARAMETER Stable
  Create the GitHub release as a full release and publish it on the stable channel of unluminous.com.
  Without it the release is a prerelease on the nightly channel.

.PARAMETER SkipSite
  Leave unluminous.com alone.

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
    [switch] $SkipSite,
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
$InstallDir = Join-Path $env:LOCALAPPDATA 'Programs/Termoil'

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

# Get-GitHubToken, Get-GitHubCli and Get-TermoilSite, shared with tools/promote.ps1.
. (Join-Path $Here 'github.ps1')

<#
.SYNOPSIS
  Publishes the release on unluminous.com/termoil with the site's scripts/termoil/publish-termoil.ps1,
  and answers whether it worked. The GitHub release already exists when this runs, so a failure is
  reported with the command to run again rather than thrown.
.PARAMETER Version
  The released version.
.PARAMETER Channel
  nightly, or stable for a release made with -Stable, which publishes nightly first and then stable.
#>
function Publish-Site([string] $Version, [string] $Channel) {
    $site = Get-TermoilSite
    if (-not $site) {
        Write-Host 'unluminous.com: the black-rainbow-labs-sites checkout was not found (set TERMOIL_SITE), skipping.' -ForegroundColor Yellow
        return $false
    }
    $script = Join-Path $site 'scripts/termoil/publish-termoil.ps1'
    Write-Step "Publishing Termoil $Version on the $Channel channel of unluminous.com"
    & pwsh -NoProfile -File $script -Channel $Channel -Version $Version | Out-Host
    if ($LASTEXITCODE -eq 0) { return $true }
    Write-Host "unluminous.com was not updated. Run this again: pwsh $script -Channel $Channel -Version $Version" -ForegroundColor Red
    return $false
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
  Builds termoil.exe and termoil-gui.exe in release mode with the build date in the environment.
.PARAMETER BuildDate
  The date text that build.rs reads from TERMOIL_BUILD_DATE.
#>
function Build-WindowsBinaries([string] $BuildDate) {
    Write-Step 'Building termoil.exe and termoil-gui.exe'
    $env:TERMOIL_BUILD_DATE = $BuildDate
    Invoke-WithoutCc {
        Invoke-Checked 'cargo build' { & cargo build --release -p termoil-cli -p termoil-app --manifest-path $Manifest }
    }
    foreach ($name in 'termoil.exe', 'termoil-gui.exe') {
        if (-not (Test-Path -LiteralPath (Join-Path (Get-BinDir) $name))) { throw "The build did not produce $name in $(Get-BinDir)." }
    }
}

<#
.SYNOPSIS
  The folder cargo writes release binaries to: cargo's own target directory, which a worktree's
  .cargo/config.toml moves out of the checkout (task-2468), with release on the end.
#>
function Get-BinDir {
    if (-not $script:BinDir) {
        $meta = & cargo metadata --format-version 1 --no-deps --manifest-path $Manifest | ConvertFrom-Json
        $script:BinDir = Join-Path $meta.target_directory 'release'
    }
    return $script:BinDir
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
    Invoke-Checked 'ISCC' { & $iscc "/DAppVersion=$Version" "/DOutDir=$DistDir" "/DBinDir=$(Get-BinDir)" (Join-Path $InstallerDir 'termoil.iss') }
    $installer = Join-Path $DistDir "TermoilSetup-$Version-x64.exe"
    if (-not (Test-Path -LiteralPath $installer)) { throw "ISCC did not produce $installer." }
    return $installer
}

<#
.SYNOPSIS
  Installs the installer silently for the current user and checks the installed termoil.exe prints
  the new version.
.PARAMETER Installer
  Path of the installer.
.PARAMETER Version
  The version termoil.exe +version must print.
#>
function Install-OnThisMachine([string] $Installer, [string] $Version) {
    Write-Step 'Installing on this machine'
    # Setup hands the work to a second process and returns at once, so wait for the whole tree.
    $proc = Start-Process -FilePath $Installer -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/CURRENTUSER' -Wait -PassThru
    if ($proc.ExitCode -ne 0) { throw "The installer exited with code $($proc.ExitCode)." }
    $exe = Join-Path $InstallDir 'termoil.exe'
    if (-not (Test-Path -LiteralPath $exe)) { throw "$exe is not there after the install." }
    $printed = (& $exe +version) -join "`n"
    Write-Host "termoil.exe +version: $printed"
    if ($printed -notmatch [regex]::Escape($Version)) { throw "The installed termoil.exe printed '$printed', which does not contain $Version." }
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
  The date text the About box shows, passed to the build as TERMOIL_BUILD_DATE.
#>
function Build-LinuxPackages([string] $Version, [string] $BuildDate) {
    if (-not (Get-Command wsl.exe -ErrorAction SilentlyContinue)) {
        Write-Host 'Linux AppImage and .deb: wsl.exe is not available, skipping.'
        return @()
    }
    Write-Step 'Building the Linux AppImage and .deb in WSL'
    $script = ConvertTo-WslPath (Join-Path $Repo 'installer/linux/build.sh')
    $env:TERMOIL_BUILD_DATE = $BuildDate
    $env:TERMOIL_VERSION = $Version
    $env:WSLENV = 'TERMOIL_BUILD_DATE:TERMOIL_VERSION'
    Invoke-Checked 'installer/linux/build.sh' { & wsl.exe -e bash $script }
    $packages = @("termoil-$Version-x86_64.AppImage", "termoil_${Version}_amd64.deb") | ForEach-Object { Join-Path $DistDir $_ }
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
    $releaseNotes = if ($Notes) { $Notes } else { "Termoil $Version" }
    # @() around the if: an if expression unrolls a one item array to a string, and splatting a
    # string passes it one character at a time.
    $releaseFlags = @(if (-not $Stable) { '--prerelease' })
    Invoke-Checked 'gh release create' { & $Gh release create "v$Version" @Assets -R $TermoilRepository --title "Termoil $Version" --notes $releaseNotes @releaseFlags }
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
        Invoke-Checked "gh release download $name" { & $Gh release download "v$Version" -R $TermoilRepository --pattern $name --dir $folder --clobber }
        $local = (Get-FileHash -Algorithm SHA256 -LiteralPath $asset).Hash
        $remote = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $folder $name)).Hash
        if ($local -ne $remote) { throw "$name downloaded from GitHub has SHA256 $remote but the local file has $local." }
        Write-Host "  $name  SHA256 $local  matches"
    }
}

<#
.SYNOPSIS
  Writes the winget multi file manifest (version, default locale and installer files) for a release
  into installer/winget/manifests/t/Termoil/Termoil/<version>/ and returns that folder.
.DESCRIPTION
  The package identifier Termoil.Termoil matches the AppUserModelID in installer/windows/termoil.iss.
  The scope is user because termoil.iss sets PrivilegesRequired=lowest. winget adds the Inno silent
  switches itself, so none are written here. ProductCode is the AppId from termoil.iss followed by _is1,
  which is the key Inno Setup writes under Uninstall.
.PARAMETER Version
  The released version, x.y.z.
.PARAMETER InstallerUrl
  The URL of the installer asset on the GitHub release.
.PARAMETER Sha256
  The SHA256 of the installer, in any letter case.
.PARAMETER OutRoot
  The manifests folder to write under. Defaults to installer/winget/manifests in the checkout.
#>
function Write-WingetManifest([string] $Version, [string] $InstallerUrl, [string] $Sha256, [string] $OutRoot = (Join-Path $Repo 'installer/winget/manifests')) {
    $id = 'Termoil.Termoil'
    $schema = '1.10.0'
    $schemaBase = 'https://aka.ms/winget-manifest'
    $folder = Join-Path $OutRoot "t/Termoil/Termoil/$Version"
    New-Item -ItemType Directory -Force -Path $folder | Out-Null
    $hash = $Sha256.ToUpper()
    $date = (Get-Date).ToUniversalTime().ToString('yyyy-MM-dd')
    $repoUrl = 'https://github.com/Black-Rainbow-Labs/Termoil'
    $versionYaml = @"
# yaml-language-server: `$schema=$schemaBase.version.$schema.schema.json
PackageIdentifier: $id
PackageVersion: $Version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: $schema
"@
    $localeYaml = @"
# yaml-language-server: `$schema=$schemaBase.defaultLocale.$schema.schema.json
PackageIdentifier: $id
PackageVersion: $Version
PackageLocale: en-US
Publisher: Termoil
PublisherUrl: $repoUrl
PublisherSupportUrl: $repoUrl/issues
PackageName: Termoil
PackageUrl: $repoUrl
License: MIT OR Apache-2.0
LicenseUrl: $repoUrl/blob/main/LICENSE-MIT
ShortDescription: A GPU accelerated terminal emulator written in Rust.
Moniker: termoil
Tags:
- terminal
- terminal-emulator
- rust
ReleaseNotesUrl: $repoUrl/releases/tag/v$Version
ManifestType: defaultLocale
ManifestVersion: $schema
"@
    $installerYaml = @"
# yaml-language-server: `$schema=$schemaBase.installer.$schema.schema.json
PackageIdentifier: $id
PackageVersion: $Version
InstallerType: inno
Scope: user
InstallModes:
- interactive
- silent
- silentWithProgress
UpgradeBehavior: install
Commands:
- termoil
ReleaseDate: $date
Installers:
- Architecture: x64
  InstallerUrl: $InstallerUrl
  InstallerSha256: $hash
  ProductCode: '{9B3E6D52-4C1A-4F0E-8A57-2D6C1E7B94A3}_is1'
ManifestType: installer
ManifestVersion: $schema
"@
    $files = @{ "$id.yaml" = $versionYaml; "$id.locale.en-US.yaml" = $localeYaml; "$id.installer.yaml" = $installerYaml }
    foreach ($name in $files.Keys) {
        Set-Content -Path (Join-Path $folder $name) -Value (($files[$name] -replace "`r`n", "`n") + "`n") -NoNewline -Encoding utf8
    }
    Write-Host "winget manifest written to $folder"
    return $folder
}

$current = Get-CurrentVersion
$next = Get-NextVersion $current $Part
$buildDate = (Get-Date).ToUniversalTime().ToString('yyyy-MM-dd')
$kind = if ($Stable) { 'stable release' } else { 'prerelease' }

if ($WhatIf) {
    Write-Host "Termoil release plan (nothing will be changed)" -ForegroundColor Yellow
    Write-Host "  Version:      $current -> $next ($Part)"
    Write-Host "  GitHub:       v$next as a $kind"
    Write-Host "  Build date:   $buildDate (passed to cargo as TERMOIL_BUILD_DATE)"
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
    Write-Host '    4. cargo build --release -p termoil-cli -p termoil-app'
    Write-Host '    5. fetch ConPTY if missing, compile installer/windows/termoil.iss with ISCC'
    Write-Host "    6. $(if ($SkipInstall) { 'skip the install on this machine' } else { "install silently into $InstallDir and check termoil.exe +version prints $next" })"
    Write-Host '    7. macOS dmg: installer/macos/build-on-windows.ps1 -Notarize when its -Preflight passes, otherwise skipped with the reason; Linux AppImage and .deb: built in WSL by installer/linux/build.sh when wsl.exe exists'
    Write-Host "    8. commit 'Release v$next', tag v$next, push the branch and the tag"
    Write-Host "    9. gh release create v$next with $(Join-Path $DistDir "TermoilSetup-$next-x64.exe") ($kind)"
    Write-Host '   10. download each asset back and compare SHA256 with the local file'
    Write-Host '   11. write the winget manifest into installer/winget/manifests'
    $site = Get-TermoilSite
    $channel = if ($Stable) { 'stable' } else { 'nightly' }
    $siteStep = if ($SkipSite -or $SkipPublish) { 'skip unluminous.com' } elseif ($site) { "publish unluminous.com/termoil on the $channel channel with $site/scripts/termoil/publish-termoil.ps1" } else { 'unluminous.com: the sites checkout was not found, a real run skips it' }
    Write-Host "   12. $siteStep"
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
Write-Step 'Writing the winget manifest'
$installerUrl = "https://github.com/Black-Rainbow-Labs/Termoil/releases/download/v$next/$(Split-Path -Leaf $installer)"
Write-WingetManifest $next $installerUrl (Get-FileHash -Algorithm SHA256 -LiteralPath $installer).Hash | Out-Null
$siteOk = $true
# The site promotes to stable only a version already on the nightly channel, so -Stable publishes
# the nightly first and then promotes it, the same two steps tools/promote.ps1 relies on.
if (-not $SkipSite) {
    $siteOk = Publish-Site $next 'nightly'
    if ($siteOk -and $Stable) { $siteOk = Publish-Site $next 'stable' }
}
Write-Host ''
Write-Host "Released Termoil v$next ($kind)." -ForegroundColor Green
if (-not $siteOk) { exit 1 }

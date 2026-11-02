<#
.SYNOPSIS
    Builds, signs and notarises Termoil.app and packages it as a .dmg, all on the Windows machine.

.DESCRIPTION
    The same arrangement as Unluminous's installer/macos/build-on-windows.ps1, which this follows. Every
    Apple program has a replacement that runs here:

        lipo        -> rcodesign macho-universal-create
        codesign    -> rcodesign sign
        notarytool  -> rcodesign notary-submit
        stapler     -> rcodesign staple
        hdiutil     -> installer/macos/make-dmg.sh in WSL (mkfs.hfsplus, hfsplus and dmg)
        iconutil    -> not needed; installer/macos/termoil.icns is committed (tools/make-icns.ps1)

    The steps:
      1. cargo zigbuild for aarch64-apple-darwin and x86_64-apple-darwin, at the same time.
      2. A universal binary for each of termoil-gui and termoil.
      3. Termoil.app: termoil-gui is the main executable, termoil sits beside it in Contents/MacOS,
         the shell-integration folder is in Contents/Resources, and Info.plist has the version.
      4. Sign `termoil`, then the bundle, with the Developer ID and the hardened runtime.
      5. Zip the bundle, submit the zip to Apple, wait, and staple the ticket to the bundle.
      6. Write the .dmg holding the stapled bundle, sign it, notarise it and staple it.
      The result is installer/dist/termoil-<version>-macos-universal.dmg.

    WHERE THE INGREDIENTS ARE, all of them outside this repository and none of them printed:
      * rcodesign, cargo-zigbuild and zig: tools/cross/bin here when present, otherwise the copy
        Unluminous fetched at C:\jason\dev\unluminous\tools\cross\bin (override with
        $env:TERMOIL_CROSS_BIN). tools/cross/fetch-toolchain.ps1 in the Unluminous checkout fetches them.
      * The Developer ID key, its certificate and the notary key: installer/macos/notarize.env here when
        present, otherwise the one in the Unluminous checkout (override with $env:TERMOIL_MACOS_ENV_FILE).
        It holds paths to files sealed with DPAPI under this Windows account, and SDKROOT.
      * The macOS SDK: -Sdk, then $env:SDKROOT (set by notarize.env), then tools/cross/sdk/MacOSX.sdk.
      * The dmg tools in WSL: installer/macos/fetch-dmg-tools.ps1 installs them once.

.PARAMETER Version
    The version the bundle claims. Defaults to the workspace version.

.PARAMETER Sdk
    The macOS SDK directory.

.PARAMETER SkipBuild
    Assemble, sign and package what is already built.

.PARAMETER SelfSigned
    Sign with a certificate generated on the spot. Proves every step except the ones that depend on
    Apple's opinion of the certificate. Implies no notarisation.

.PARAMETER Notarize
    Submit the bundle and the dmg to Apple, wait, and staple the tickets.

.PARAMETER Preflight
    Say whether a macOS build could run here, and exit. Nothing is built and nothing is written. Exit
    code 0 means it could run; 1 means it could not, and the reasons are on stdout.

.EXAMPLE
    pwsh installer\macos\build-on-windows.ps1 -Preflight
    pwsh installer\macos\build-on-windows.ps1 -Notarize
#>
[CmdletBinding()]
param(
    [string] $Version,
    [string] $Sdk,
    [switch] $SkipBuild,
    [switch] $SelfSigned,
    [switch] $Notarize,
    [switch] $Preflight
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$dist = Join-Path $repo 'installer/dist'
$stage = Join-Path $dist 'macos-stage'
$app = Join-Path $stage 'Termoil.app'
$unluminous = 'C:\jason\dev\unluminous'
$appleTargets = @('aarch64-apple-darwin', 'x86_64-apple-darwin')
# The two programs: the Cargo package, the binary name, and the file name inside Contents/MacOS.
$programs = @(
    @{ Package = 'termoil-app'; Bin = 'termoil-gui' },
    @{ Package = 'termoil-cli'; Bin = 'termoil' }
)

<#
.SYNOPSIS
    Returns the first existing path from a list, or $null.
.PARAMETER Candidates
    The paths to try, in order. Empty entries are skipped.
#>
function Find-FirstPath([string[]] $Candidates) {
    foreach ($candidate in $Candidates) {
        if ($candidate -and (Test-Path -LiteralPath $candidate)) { return $candidate }
    }
    return $null
}

<#
.SYNOPSIS
    The folder holding rcodesign.exe, cargo-zigbuild.exe and zig, or $null when none has them.
#>
function Find-CrossBin {
    foreach ($folder in @($env:TERMOIL_CROSS_BIN, (Join-Path $repo 'tools/cross/bin'), (Join-Path $unluminous 'tools/cross/bin'))) {
        if ($folder -and (Test-Path -LiteralPath (Join-Path $folder 'rcodesign.exe')) -and (Test-Path -LiteralPath (Join-Path $folder 'cargo-zigbuild.exe'))) { return $folder }
    }
    return $null
}

<#
.SYNOPSIS
    Reads a notarize.env file into environment variables, leaving any variable already set alone.
.PARAMETER Path
    The file. Lines look like NAME=value; comments start with #.
#>
function Import-EnvFile([string] $Path) {
    foreach ($line in Get-Content -Path $Path) {
        if ($line -notmatch '^\s*([A-Z_][A-Z0-9_]*)\s*=\s*(.*?)\s*$') { continue }
        if (Get-Item "env:$($Matches[1])" -ErrorAction SilentlyContinue) { continue }
        Set-Item "env:$($Matches[1])" ($Matches[2].Trim('"').Trim("'"))
    }
}

<#
.SYNOPSIS
    Runs rcodesign and throws if it failed, so no step is skipped silently.
.PARAMETER Arguments
    The whole command line.
.PARAMETER Quiet
    Discard its output.
#>
function Invoke-Rcodesign([string[]] $Arguments, [switch] $Quiet) {
    if ($Quiet) { & $script:rcodesign @Arguments *> $null } else { & $script:rcodesign @Arguments }
    if ($LASTEXITCODE -ne 0) { throw "rcodesign $($Arguments[0]) failed with $LASTEXITCODE" }
}

<#
.SYNOPSIS
    Reads the version out of Cargo.toml's [workspace.package].
#>
function Get-WorkspaceVersion {
    $manifest = Get-Content -Path (Join-Path $repo 'Cargo.toml') -Raw
    if ($manifest -match '(?ms)\[workspace\.package\].*?version\s*=\s*"([^"]+)"') { return $Matches[1] }
    throw 'Cargo.toml does not declare [workspace.package] version'
}

<#
.SYNOPSIS
    Finds the macOS SDK, or says exactly what is missing. Nothing here downloads one: Apple's licence
    for the SDK says Apple branded hardware, which is a decision for the person who accepted it.
#>
function Resolve-Sdk {
    $found = Find-FirstPath @($Sdk, $env:SDKROOT, (Join-Path $repo 'tools/cross/sdk/MacOSX.sdk'))
    if (-not $found) {
        throw 'No macOS SDK. Name one with -Sdk or $env:SDKROOT, or put it at tools/cross/sdk/MacOSX.sdk. winit, wgpu and arboard link AppKit, Metal, QuartzCore and more, and zig ships a stub for libSystem and nothing else.'
    }
    if (-not (Test-Path -LiteralPath (Join-Path $found 'System/Library/Frameworks/AppKit.framework'))) {
        throw "$found does not hold System/Library/Frameworks/AppKit.framework, so it is not a macOS SDK"
    }
    return (Resolve-Path -LiteralPath $found).Path
}

<#
.SYNOPSIS
    Says what is missing for a macOS build on this machine, as a list of problems (empty when ready).
#>
function Get-PreflightProblems {
    $problems = @()
    try { $null = Resolve-Sdk } catch { $problems += 'no macOS SDK (see -Sdk)' }
    if (-not $script:crossBin) { $problems += 'rcodesign, cargo-zigbuild and zig are not in tools/cross/bin or the Unluminous checkout' }
    $haveKey = $env:CODESIGN_KEY_SEALED -and $env:CODESIGN_CERT -and
        (Test-Path -LiteralPath $env:CODESIGN_KEY_SEALED) -and (Test-Path -LiteralPath $env:CODESIGN_CERT)
    $haveP12 = $env:CODESIGN_P12 -and (Test-Path -LiteralPath $env:CODESIGN_P12)
    if (-not $haveKey -and -not $haveP12) { $problems += 'no Developer ID identity (CODESIGN_KEY_SEALED with CODESIGN_CERT, or CODESIGN_P12)' }
    $haveNotary = ($env:NOTARY_KEY_SEALED -and (Test-Path -LiteralPath $env:NOTARY_KEY_SEALED)) -or
        ($env:NOTARY_KEY -and $env:NOTARY_KEY_ID -and $env:NOTARY_ISSUER)
    if (-not $haveNotary) { $problems += 'no notary credential (NOTARY_KEY_SEALED, or the NOTARY_KEY trio)' }
    $problems += Get-DmgToolProblems
    return $problems
}

<#
.SYNOPSIS
    Says whether the dmg tools are installed in WSL: wsl.exe, hfsprogs and the two libdmg programs.
#>
function Get-DmgToolProblems {
    if (-not (Get-Command wsl.exe -ErrorAction SilentlyContinue)) { return @('wsl.exe is not available, so the dmg cannot be written') }
    & wsl.exe -e bash -c 'command -v mkfs.hfsplus && command -v fsck.hfsplus && test -x /opt/libdmg/pb/build/hfs/hfsplus && test -x /opt/libdmg/src/build/dmg/dmg' *> $null
    if ($LASTEXITCODE -ne 0) { return @('the dmg tools are missing in WSL; run installer/macos/fetch-dmg-tools.ps1') }
    return @()
}

<#
.SYNOPSIS
    The target directory one Apple architecture builds into: its own, under the cargo target directory,
    so two architectures do not wait for each other's lock.
.PARAMETER Target
    The triple.
#>
function Get-AppleTargetDir([string] $Target) {
    $base = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $repo 'target' }
    return (Join-Path $base "mac-$Target")
}

<#
.SYNOPSIS
    Starts one architecture's cargo-zigbuild with its output written to its own log.
.PARAMETER Build
    The build: its triple, its log and the try it is on, which this counts.
.PARAMETER SdkRoot
    The macOS SDK.
#>
function Start-AppleBuild($Build, [string] $SdkRoot) {
    $target = $Build.Target
    $arguments = @('zigbuild', '--manifest-path', (Join-Path $repo 'Cargo.toml'), '--release', '--locked', '--target', $target,
        '-p', 'termoil-app', '--bin', 'termoil-gui', '-p', 'termoil-cli', '--bin', 'termoil')
    $line = '"' + (Join-Path $script:crossBin 'cargo-zigbuild.exe') + '" ' + (($arguments | ForEach-Object { '"' + $_ + '"' }) -join ' ') + ' > "' + $Build.Log + '" 2>&1'
    $start = [System.Diagnostics.ProcessStartInfo]::new($env:ComSpec)
    $start.Arguments = '/d /s /c "' + $line + '"'
    $start.UseShellExecute = $false
    $start.Environment['PATH'] = "$(Join-Path $script:crossBin 'zig');$env:PATH"
    $start.Environment['CARGO_TARGET_DIR'] = Get-AppleTargetDir $target
    # Room for the code signature load command; rcodesign refuses a Mach-O linked by zig without it.
    $start.Environment["CARGO_TARGET_$($target.ToUpperInvariant().Replace('-', '_'))_RUSTFLAGS"] = '-C link-arg=-Wl,-headerpad_max_install_names'
    $start.Environment['SDKROOT'] = $SdkRoot
    $Build.Tries += 1
    Write-Host "==> $target, into $(Get-AppleTargetDir $target), try $($Build.Tries), log $($Build.Log)"
    $Build.Process = [System.Diagnostics.Process]::Start($start)
    $null = $Build.Process.Handle
}

<#
.SYNOPSIS
    Waits, for at most 30 seconds, until a build has printed its first cargo line, so the next
    architecture does not start while cargo-zigbuild is still writing its shared files.
.PARAMETER Build
    The build that was just started.
#>
function Wait-UntilCompiling($Build) {
    $until = (Get-Date).AddSeconds(30)
    while ((Get-Date) -lt $until -and -not $Build.Process.HasExited) {
        $text = Get-Content -LiteralPath $Build.Log -Raw -ErrorAction SilentlyContinue
        if ($text -match '(Compiling|Fresh|Finished|Updating|Downloaded)') { return }
        Start-Sleep -Milliseconds 500
    }
}

<#
.SYNOPSIS
    Cross compiles both Apple architectures at the same time and throws if either failed. A build
    that failed because another process held a file is started again, up to three times.
.PARAMETER SdkRoot
    The macOS SDK.
#>
function Build-AppleTargets([string] $SdkRoot) {
    $logs = Join-Path $repo '_agent_output/task-2458-macos'
    New-Item -ItemType Directory -Force -Path $logs | Out-Null
    $builds = @()
    foreach ($target in $appleTargets) {
        $build = [pscustomobject]@{ Target = $target; Log = (Join-Path $logs "zigbuild-$target.log"); Process = $null; Tries = 0 }
        Start-AppleBuild $build $SdkRoot
        Wait-UntilCompiling $build
        $builds += $build
    }
    $failed = @()
    foreach ($build in $builds) {
        while ($true) {
            $build.Process.WaitForExit()
            if ($build.Process.ExitCode -eq 0) { break }
            $text = Get-Content -LiteralPath $build.Log -Raw -ErrorAction SilentlyContinue
            if ($build.Tries -lt 3 -and $text -match 'os error 32|os error 33|being used by another process') {
                Start-Sleep -Seconds 5
                Start-AppleBuild $build $SdkRoot
                continue
            }
            Get-Content -LiteralPath $build.Log -ErrorAction SilentlyContinue | Select-Object -Last 40 | ForEach-Object { Write-Host "  $_" }
            $failed += "$($build.Target) with $($build.Process.ExitCode) (log: $($build.Log))"
            break
        }
    }
    if ($failed.Count -gt 0) { throw "cargo-zigbuild failed for $($failed -join ', ')" }
}

<#
.SYNOPSIS
    Joins the two architectures of one program into a universal binary and checks both slices are in it.
.PARAMETER Name
    The program's file name.
.PARAMETER Into
    The directory to write it to.
#>
function New-UniversalBinary([string] $Name, [string] $Into) {
    $slices = $appleTargets | ForEach-Object { Join-Path (Get-AppleTargetDir $_) "$_/release/$Name" }
    foreach ($slice in $slices) {
        if (-not (Test-Path -LiteralPath $slice)) { throw "the build did not produce $slice" }
    }
    $output = Join-Path $Into $Name
    Invoke-Rcodesign -Quiet -Arguments (@('macho-universal-create', '--output', $output) + $slices)
    $seen = @()
    foreach ($index in 0, 1) {
        $header = & $script:rcodesign extract macho-header --universal-index $index $output 2>&1
        if ($LASTEXITCODE -ne 0) { throw "$output has no slice at index $index" }
        $seen += ($header | Select-String -Pattern 'cputype: (\d+)').Matches.Groups[1].Value
    }
    # 16777228 is arm64 and 16777223 is x86-64, as Mach-O numbers them.
    foreach ($wanted in '16777228', '16777223') {
        if ($seen -notcontains $wanted) { throw "$output is missing the cputype $wanted slice" }
    }
}

<#
.SYNOPSIS
    Assembles Termoil.app: the two programs, the icon, the shell integration scripts, Info.plist and PkgInfo.
.PARAMETER Version
    The version written into Info.plist.
#>
function New-AppBundle([string] $Version) {
    if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath $stage -Recurse -Force -Confirm:$false }
    $macos = Join-Path $app 'Contents/MacOS'
    $resources = Join-Path $app 'Contents/Resources'
    New-Item -ItemType Directory -Force -Path $macos, $resources | Out-Null
    foreach ($program in $programs) { New-UniversalBinary -Name $program.Bin -Into $macos }

    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'termoil.icns') -Destination (Join-Path $resources 'Termoil.icns') -Force
    # The app finds this folder at Resources/shell-integration (shell_integration::source_dir_from).
    Copy-Item -LiteralPath (Join-Path $repo 'shell-integration') -Destination (Join-Path $resources 'shell-integration') -Recurse -Force

    $plist = (Get-Content -Path (Join-Path $PSScriptRoot 'Info.plist') -Raw).Replace('__VERSION__', $Version)
    $plistPath = Join-Path $app 'Contents/Info.plist'
    [System.IO.File]::WriteAllText($plistPath, $plist)
    # What plutil -lint does on a Mac: refuse a manifest that is not well formed XML.
    $document = New-Object System.Xml.XmlDocument
    $document.XmlResolver = $null
    $document.Load($plistPath)
    [System.IO.File]::WriteAllText((Join-Path $app 'Contents/PkgInfo'), 'APPL????')
}

<#
.SYNOPSIS
    Reads a file sealed with DPAPI under this Windows account.
.PARAMETER Path
    The sealed file.
#>
function Unprotect-Secret([string] $Path) {
    $secure = Get-Content -Path $Path -Raw | ConvertTo-SecureString
    return [System.Net.NetworkCredential]::new('', $secure).Password
}

<#
.SYNOPSIS
    Returns a folder for the short lived files that hold unsealed keys: the RAM disk when there is one.
#>
function Get-SecretScratch {
    $scratch = if (Test-Path -LiteralPath 'R:\') { 'R:\termoil-release' } else { Join-Path $env:TEMP 'termoil-release' }
    New-Item -ItemType Directory -Force -Path $scratch | Out-Null
    return $scratch
}

<#
.SYNOPSIS
    Works out how to sign and returns the rcodesign arguments plus the files to delete afterwards.
    The Developer ID key is unsealed to a scratch file for the length of the build and never printed.
#>
function New-SigningSession {
    $scratch = Get-SecretScratch
    $stamp = [guid]::NewGuid().ToString('N')
    if ($SelfSigned) {
        $pem = Join-Path $scratch "selfsigned-$stamp.pem"
        Invoke-Rcodesign -Quiet -Arguments @('generate-self-signed-certificate', '--algorithm', 'rsa', '--profile', 'developer-id-application',
            '--team-id', 'SELFSIGNED', '--person-name', 'Termoil self-signed build', '--validity-days', '30', '--pem-unified-file', $pem)
        return [pscustomobject]@{ Arguments = @('--pem-file', $pem); Scratch = @($pem); Kind = 'self-signed' }
    }
    if ($env:CODESIGN_KEY_SEALED -and $env:CODESIGN_CERT) {
        foreach ($needed in $env:CODESIGN_KEY_SEALED, $env:CODESIGN_CERT) {
            if (-not (Test-Path -LiteralPath $needed)) { throw "$needed is not there" }
        }
        $keyFile = Join-Path $scratch "key-$stamp.pem"
        Set-Content -Path $keyFile -Value (Unprotect-Secret -Path $env:CODESIGN_KEY_SEALED) -NoNewline
        return [pscustomobject]@{ Arguments = @('--pem-file', $keyFile, '--certificate-der-file', $env:CODESIGN_CERT); Scratch = @($keyFile); Kind = 'Developer ID' }
    }
    throw 'CODESIGN_KEY_SEALED with CODESIGN_CERT is not set, so there is no Developer ID identity to sign with. Or pass -SelfSigned to prove the pipeline.'
}

<#
.SYNOPSIS
    Deletes the files New-SigningSession unsealed or generated.
.PARAMETER Session
    The object it returned; a null session is accepted.
#>
function Remove-SigningSession($Session) {
    if ($null -eq $Session) { return }
    foreach ($path in $Session.Scratch) {
        if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force -Confirm:$false }
    }
}

<#
.SYNOPSIS
    Signs the nested termoil program, then the bundle, with the hardened runtime, and reads the results back.
.PARAMETER Session
    The signing identity's rcodesign arguments.
#>
function Invoke-BundleSigning($Session) {
    # termoil is signed first: a second Mach-O in Contents/MacOS is a nested entity, and an unsigned one
    # inside a signed bundle makes verification fail on a bundle that looked fine.
    Invoke-Rcodesign -Quiet -Arguments (@('sign') + $Session.Arguments + @(
            '--binary-identifier', 'termoil', '--code-signature-flags', 'runtime', (Join-Path $app 'Contents/MacOS/termoil')))
    Invoke-Rcodesign -Quiet -Arguments (@('sign') + $Session.Arguments + @('--code-signature-flags', 'runtime', $app))
    $info = & $script:rcodesign print-signature-info $app 2>&1
    if ($LASTEXITCODE -ne 0) { throw 'the signed bundle has no readable signature' }
    if (-not (Test-Path -LiteralPath (Join-Path $app 'Contents/_CodeSignature/CodeResources'))) {
        throw 'the bundle carries no _CodeSignature/CodeResources, so nothing sealed its resources'
    }
    # Two programs with two architectures each: all four signatures need the hardened runtime.
    $runtime = ($info | Select-String -Pattern 'CodeSignatureFlags\(RUNTIME\)').Count
    if ($runtime -lt 4) { throw "only $runtime of the four signatures carry the hardened runtime flag" }
    # rcodesign verify reads a Mach-O and refuses a bundle path, so each program is verified.
    foreach ($program in $programs) { Invoke-Rcodesign -Quiet -Arguments @('verify', (Join-Path $app "Contents/MacOS/$($program.Bin)")) }
}

<#
.SYNOPSIS
    Writes a Unix mode onto every entry of a zip so the bundle keeps its executable bits on a Mac.
    A zip written on Windows carries no Unix permissions, and unzip on macOS reads the mode only when
    the host byte says Unix.
.PARAMETER Path
    The zip to patch.
.PARAMETER ExecutablePrefix
    Entries under this path get 0755; everything else gets 0644.
#>
function Set-ZipUnixModes([string] $Path, [string] $ExecutablePrefix) {
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    for ($at = 0; $at -lt $bytes.Length - 46; $at++) {
        if ($bytes[$at] -ne 0x50 -or $bytes[$at + 1] -ne 0x4B -or $bytes[$at + 2] -ne 0x01 -or $bytes[$at + 3] -ne 0x02) { continue }
        $nameLength = [BitConverter]::ToUInt16($bytes, $at + 28)
        $name = [System.Text.Encoding]::UTF8.GetString($bytes, $at + 46, $nameLength)
        # 33261 is 0100755 and 33188 is 0100644, in decimal because PowerShell has no octal literal.
        $mode = if ($name.StartsWith($ExecutablePrefix)) { 33261 } else { 33188 }
        $bytes[$at + 5] = 3
        [Array]::Copy([BitConverter]::GetBytes([uint32] ([int64] $mode * 65536)), 0, $bytes, $at + 38, 4)
    }
    [System.IO.File]::WriteAllBytes($Path, $bytes)
}

<#
.SYNOPSIS
    Zips the bundle, with Termoil.app as the top entry, for the notary, and returns the path.
.PARAMETER Version
    The version in the file's name.
#>
function New-BundleZip([string] $Version) {
    $zip = Join-Path $dist "termoil-$Version-macos-bundle.zip"
    if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip -Force -Confirm:$false }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [System.IO.Compression.ZipFile]::CreateFromDirectory($app, $zip, [System.IO.Compression.CompressionLevel]::Optimal, $true)
    Set-ZipUnixModes -Path $zip -ExecutablePrefix 'Termoil.app/Contents/MacOS/'
    return $zip
}

<#
.SYNOPSIS
    Returns the App Store Connect key file rcodesign notarises with: the unsealed JSON, in the scratch
    folder, to be deleted by the caller.
#>
function Get-NotaryKeyFile {
    $scratch = Get-SecretScratch
    $file = Join-Path $scratch ('notary-' + [guid]::NewGuid().ToString('N') + '.json')
    if ($env:NOTARY_KEY_SEALED) {
        if (-not (Test-Path -LiteralPath $env:NOTARY_KEY_SEALED)) { throw "$($env:NOTARY_KEY_SEALED) is not there" }
        Set-Content -Path $file -Value (Unprotect-Secret -Path $env:NOTARY_KEY_SEALED) -NoNewline
        return $file
    }
    foreach ($needed in 'NOTARY_KEY', 'NOTARY_KEY_ID', 'NOTARY_ISSUER') {
        if (-not (Get-Item "env:$needed" -ErrorAction SilentlyContinue)) { throw "$needed is not set, and neither is NOTARY_KEY_SEALED." }
    }
    Invoke-Rcodesign -Quiet -Arguments @('encode-app-store-connect-api-key', '-o', $file, $env:NOTARY_ISSUER, $env:NOTARY_KEY_ID, $env:NOTARY_KEY)
    return $file
}

<#
.SYNOPSIS
    Submits one file to Apple's notary service and waits for the verdict, using an unsealed key file
    that is deleted afterwards.
.PARAMETER Path
    The zip or dmg.
#>
function Submit-ToNotary([string] $Path) {
    $keyFile = Get-NotaryKeyFile
    try {
        Write-Host "==> notarising $(Split-Path -Leaf $Path); Apple usually answers in two to fifteen minutes"
        Invoke-Rcodesign -Arguments @('notary-submit', '--api-key-file', $keyFile, '--wait', '--max-wait-seconds', '3600', $Path)
    } finally {
        if (Test-Path -LiteralPath $keyFile) { Remove-Item -LiteralPath $keyFile -Force -Confirm:$false }
    }
}

<#
.SYNOPSIS
    Converts a Windows path into the path WSL sees, such as C:\a\b into /mnt/c/a/b.
.PARAMETER Path
    The Windows path.
#>
function ConvertTo-WslPath([string] $Path) {
    $full = [System.IO.Path]::GetFullPath($Path)
    return '/mnt/' + $full.Substring(0, 1).ToLower() + $full.Substring(2).Replace('\', '/')
}

<#
.SYNOPSIS
    Writes the .dmg holding the staged bundle by running installer/macos/make-dmg.sh in WSL.
.PARAMETER Output
    The .dmg to write.
#>
function New-DiskImage([string] $Output) {
    $arguments = @('-e', 'bash', (ConvertTo-WslPath (Join-Path $PSScriptRoot 'make-dmg.sh')), (ConvertTo-WslPath $stage), (ConvertTo-WslPath $Output), 'Termoil')
    $arguments += $programs | ForEach-Object { "Termoil.app/Contents/MacOS/$($_.Bin)" }
    & wsl.exe @arguments
    if ($LASTEXITCODE -ne 0) { throw "make-dmg.sh failed with $LASTEXITCODE" }
    if (-not (Test-Path -LiteralPath $Output)) { throw "make-dmg.sh did not write $Output" }
}

<#
.SYNOPSIS
    Notarises and staples the bundle: submits its zip, waits, staples the ticket to the bundle and
    checks that the ticket file is there.
.PARAMETER Version
    The version in the zip's name.
#>
function Complete-BundleNotarisation([string] $Version) {
    $zip = New-BundleZip -Version $Version
    Submit-ToNotary $zip
    Invoke-Rcodesign -Arguments @('staple', $app)
    if (-not (Test-Path -LiteralPath (Join-Path $app 'Contents/CodeResources'))) { throw 'staple finished but the bundle has no Contents/CodeResources ticket' }
}

# ---------------------------------------------------------------------------
# The build itself.
# ---------------------------------------------------------------------------

foreach ($file in @($env:TERMOIL_MACOS_ENV_FILE, (Join-Path $PSScriptRoot 'notarize.env'), (Join-Path $unluminous 'installer/macos/notarize.env'))) {
    if ($file -and (Test-Path -LiteralPath $file)) { Import-EnvFile $file }
}
$script:crossBin = Find-CrossBin
$script:rcodesign = if ($script:crossBin) { Join-Path $script:crossBin 'rcodesign.exe' } else { $null }

if ($Preflight) {
    $problems = Get-PreflightProblems
    if ($problems.Count -gt 0) { Write-Host ($problems -join '; '); exit 1 }
    Write-Host 'ready: SDK, cross tools, Developer ID identity, notary credential and dmg tools are all present'
    exit 0
}

if (-not $Version) { $Version = Get-WorkspaceVersion }
$problems = Get-PreflightProblems
if ($SelfSigned) { $problems = $problems | Where-Object { $_ -notmatch 'Developer ID|notary' } }
if ($SkipBuild) { $problems = $problems | Where-Object { $_ -notmatch 'SDK' } }
if (@($problems).Count -gt 0) { throw "a macOS build cannot run here: $(@($problems) -join '; ')" }
$sdkRoot = if ($SkipBuild) { '' } else { Resolve-Sdk }
Write-Host "Termoil $Version - macOS, built on Windows"
if ($sdkRoot) { Write-Host "  SDK: $sdkRoot" }
if (-not $SkipBuild) { Build-AppleTargets -SdkRoot $sdkRoot }

Write-Host '==> Termoil.app'
New-AppBundle -Version $Version
Write-Host "  $app"

$session = $null
try {
    $session = New-SigningSession
    Write-Host "==> signing ($($session.Kind))"
    Invoke-BundleSigning -Session $session
    if ($Notarize) {
        if ($SelfSigned) { throw 'a self-signed certificate cannot be notarised' }
        Complete-BundleNotarisation -Version $Version
    }
    $dmg = Join-Path $dist "termoil-$Version-macos-universal.dmg"
    Write-Host '==> dmg'
    New-DiskImage -Output $dmg
    Invoke-Rcodesign -Quiet -Arguments (@('sign') + $session.Arguments + @($dmg))
    if ($Notarize) {
        Submit-ToNotary $dmg
        Invoke-Rcodesign -Arguments @('staple', $dmg)
    }
} finally {
    Remove-SigningSession -Session $session
}

Write-Host ''
Write-Host 'checked here:'
Write-Host '  both architectures in both programs; hardened runtime on all four signatures; rcodesign verify on each program'
if ($Notarize) { Write-Host '  Apple notarised the bundle and the dmg, and both carry a stapled ticket' } else { Write-Host '  NOT notarised, so Gatekeeper will warn about it' }
Write-Host 'not checked here, because a Mach-O only runs on macOS: that the application starts, that spctl accepts it, and that the dmg mounts.'
if ($SelfSigned) { Write-Warning 'signed with a throwaway certificate. Gatekeeper rejects these; this proves the pipeline, not a release.' }
Write-Host "DMG: $dmg"

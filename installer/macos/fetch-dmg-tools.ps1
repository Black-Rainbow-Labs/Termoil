<#
.SYNOPSIS
    Installs, in WSL, the programs installer/macos/make-dmg.sh needs to write a .dmg without a Mac.

.DESCRIPTION
    Runs as root inside the Ubuntu WSL distribution:
      * apt installs hfsprogs (mkfs.hfsplus and fsck.hfsplus) and a C toolchain.
      * two small programs are built from source into /opt/libdmg, each pinned to a commit:
          planetbeing/libdmg-hfsplus  builds `hfsplus`, which copies files into an HFS+ image
          fanquake/libdmg-hfsplus     builds `dmg`, which wraps an image in a compressed UDIF .dmg
        Both are GPL 3 programs used here as build tools. Nothing from them is shipped in the app.
    Run it once per machine. `build-on-windows.ps1 -Preflight` says whether it is needed.

.PARAMETER Distribution
    The WSL distribution. Defaults to Ubuntu.
#>
param([string] $Distribution = 'Ubuntu')
$ErrorActionPreference = 'Stop'

$PlanetbeingCommit = '7ac55ec64c96f7800d9818ce64c79670e7f02b67'
$FanquakeCommit = '1cc791e4173da9cb0b0cc16c5a1aaa25d5eb5efa'

$script = @"
set -euo pipefail
apt-get update -qq
apt-get install -y -qq hfsprogs build-essential cmake git zlib1g-dev libbz2-dev libssl-dev
mkdir -p /opt/libdmg
cd /opt/libdmg
[ -d pb ] || git clone -q https://github.com/planetbeing/libdmg-hfsplus pb
git -C pb checkout -q $PlanetbeingCommit
[ -d src ] || git clone -q https://github.com/fanquake/libdmg-hfsplus src
git -C src checkout -q $FanquakeCommit
cmake -S pb -B pb/build -DCMAKE_BUILD_TYPE=Release -DCMAKE_POLICY_VERSION_MINIMUM=3.5 >/dev/null
cmake --build pb/build --target hfsplus -j4
cmake -S src -B src/build -DCMAKE_BUILD_TYPE=Release >/dev/null
cmake --build src/build --target dmg-bin -j4
chmod -R a+rX /opt/libdmg
ls -l /opt/libdmg/pb/build/hfs/hfsplus /opt/libdmg/src/build/dmg/dmg
"@
$file = Join-Path $env:TEMP 'terminus-fetch-dmg-tools.sh'
[System.IO.File]::WriteAllText($file, $script.Replace("`r`n", "`n"))
$wslPath = '/mnt/' + $file.Substring(0, 1).ToLower() + $file.Substring(2).Replace('\', '/')
& wsl.exe -d $Distribution -u root -e bash $wslPath
if ($LASTEXITCODE -ne 0) { throw "installing the dmg tools failed with exit code $LASTEXITCODE" }
Remove-Item -LiteralPath $file -Force

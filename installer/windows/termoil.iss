; Termoil installer (Inno Setup 6).
;
; Build:   ISCC.exe /DAppVersion=0.1.1 installer\windows\termoil.iss
; Optional defines: /DBinDir=<folder holding termoil.exe and termoil-gui.exe>  (default ..\..\target\release)
;                   /DOutDir=<folder for the installer>                          (default ..\dist)
;
; Installs for the current user by default under %LOCALAPPDATA%\Programs\Termoil, with the option of
; an install for all users under Program Files. Uninstalling removes everything the installer wrote
; and leaves the person's settings folder alone.

#ifndef AppVersion
  #error Pass the version with /DAppVersion=x.y.z
#endif
#ifndef BinDir
  #define BinDir "..\..\target\release"
#endif
#ifndef OutDir
  #define OutDir "..\dist"
#endif
#define AppName "Termoil"
#define GuiExe "termoil-gui.exe"
#define CliExe "termoil.exe"

[Setup]
AppId={{7D7EA3DF-6A1C-4346-ADDC-4512927709B9}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Black Rainbow Labs
AppPublisherURL=https://github.com/Black-Rainbow-Labs/Termoil
AppSupportURL=https://github.com/Black-Rainbow-Labs/Termoil/issues
VersionInfoVersion={#AppVersion}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutDir}
OutputBaseFilename=TermoilSetup-{#AppVersion}-x64
SetupIconFile=termoil.ico
UninstallDisplayIcon={app}\{#GuiExe}
UninstallDisplayName={#AppName}
LicenseFile=..\..\LICENSE-MIT
Compression=lzma2/ultra
SolidCompression=yes
WizardStyle=modern
ChangesEnvironment=yes
; Upgrading while Termoil is open asks Windows to close it through the Restart Manager, and the
; [Code] section below ends any termoil-gui.exe still running from the install folder.
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked
Name: "addtopath"; Description: "Add Termoil to the PATH, so that ""termoil +command"" works from any terminal"; GroupDescription: "Other:"
Name: "contextmenu"; Description: "Add ""Open Termoil here"" to the Explorer context menu for folders"; GroupDescription: "Other:"

[Files]
Source: "{#BinDir}\{#GuiExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#BinDir}\{#CliExe}"; DestDir: "{app}"; Flags: ignoreversion
; The bundled ConPTY pair sits beside termoil-gui.exe, which loads conpty.dll from its own folder.
Source: "conpty\conpty.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "conpty\OpenConsole.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\shell-integration\*"; DestDir: "{app}\shell-integration"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\..\LICENSE-MIT"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "..\..\LICENSE-APACHE"; DestDir: "{app}\licenses"; Flags: ignoreversion
#ifexist "..\..\THIRD-PARTY-NOTICES.md"
Source: "..\..\THIRD-PARTY-NOTICES.md"; DestDir: "{app}\licenses"; Flags: ignoreversion
#endif

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#GuiExe}"; AppUserModelID: "Termoil.Termoil"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#GuiExe}"; Tasks: desktopicon

[Registry]
; App Paths lets Start and the Run box find termoil-gui.exe by name.
Root: HKA; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\{#GuiExe}"; ValueType: string; ValueName: ""; ValueData: "{app}\{#GuiExe}"; Flags: uninsdeletekey
; The context menu entries are written under HKA\Software\Classes, which is HKCU for a per user install
; and HKLM for an all users install. %V is the folder the person clicked.
Root: HKA; Subkey: "Software\Classes\Directory\shell\OpenTermoilHere"; ValueType: string; ValueName: ""; ValueData: "Open Termoil here"; Flags: uninsdeletekey; Tasks: contextmenu
Root: HKA; Subkey: "Software\Classes\Directory\shell\OpenTermoilHere"; ValueType: string; ValueName: "Icon"; ValueData: "{app}\{#GuiExe}"; Tasks: contextmenu
Root: HKA; Subkey: "Software\Classes\Directory\shell\OpenTermoilHere\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#GuiExe}"" --cwd ""%V"""; Tasks: contextmenu
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\OpenTermoilHere"; ValueType: string; ValueName: ""; ValueData: "Open Termoil here"; Flags: uninsdeletekey; Tasks: contextmenu
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\OpenTermoilHere"; ValueType: string; ValueName: "Icon"; ValueData: "{app}\{#GuiExe}"; Tasks: contextmenu
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\OpenTermoilHere\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#GuiExe}"" --cwd ""%V"""; Tasks: contextmenu

[Run]
Filename: "{app}\{#GuiExe}"; Description: "Start {#AppName}"; Flags: nowait postinstall skipifsilent

[Code]
const
  EnvironmentKeyUser = 'Environment';
  EnvironmentKeyMachine = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment';

{ Returns the registry root that holds the PATH this install writes to. }
function PathRoot: Integer;
begin
  if IsAdminInstallMode then Result := HKEY_LOCAL_MACHINE else Result := HKEY_CURRENT_USER;
end;

{ Returns the registry key under PathRoot that holds the PATH value. }
function PathKey: String;
begin
  if IsAdminInstallMode then Result := EnvironmentKeyMachine else Result := EnvironmentKeyUser;
end;

{ True when Dir is already one of the semicolon separated entries of Path. }
function PathContains(Path, Dir: String): Boolean;
begin
  Result := Pos(';' + Lowercase(Dir) + ';', ';' + Lowercase(Path) + ';') > 0;
end;

{ Appends the install folder to PATH unless it is already there. }
procedure AddInstallFolderToPath;
var
  Current: String;
begin
  if not RegQueryStringValue(PathRoot, PathKey, 'Path', Current) then Current := '';
  if PathContains(Current, ExpandConstant('{app}')) then exit;
  if (Current <> '') and (Copy(Current, Length(Current), 1) <> ';') then Current := Current + ';';
  RegWriteExpandStringValue(PathRoot, PathKey, 'Path', Current + ExpandConstant('{app}'));
end;

{ Removes every entry equal to the install folder from PATH. }
procedure RemoveInstallFolderFromPath;
var
  Current, Dir, Rest: String;
  P: Integer;
begin
  if not RegQueryStringValue(PathRoot, PathKey, 'Path', Current) then exit;
  Dir := Lowercase(ExpandConstant('{app}'));
  Rest := Current;
  Current := '';
  while Rest <> '' do
  begin
    P := Pos(';', Rest);
    if P = 0 then P := Length(Rest) + 1;
    if Lowercase(Copy(Rest, 1, P - 1)) <> Dir then
    begin
      if Current <> '' then Current := Current + ';';
      Current := Current + Copy(Rest, 1, P - 1);
    end;
    Delete(Rest, 1, P);
  end;
  RegWriteExpandStringValue(PathRoot, PathKey, 'Path', Current);
end;

{ Ends termoil-gui.exe processes that run from the install folder, and no other process. }
procedure StopRunningTermoil;
var
  ResultCode: Integer;
  Command: String;
begin
  Command := '-NoProfile -NonInteractive -Command "Get-Process -Name termoil-gui -ErrorAction SilentlyContinue | ' +
    'Where-Object { $_.Path -like (''' + ExpandConstant('{app}') + '\*'') } | Stop-Process -Force"';
  Exec('powershell.exe', Command, '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

{ Runs before files are copied: stops a running Termoil so its files can be replaced. }
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopRunningTermoil;
  Result := '';
end;

{ Adds the PATH entry after the files are installed, when the task was chosen. }
procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('addtopath') then AddInstallFolderToPath;
end;

{ Stops Termoil and removes the PATH entry when uninstalling. }
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
  begin
    StopRunningTermoil;
    RemoveInstallFolderFromPath;
  end;
end;

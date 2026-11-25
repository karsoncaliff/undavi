; The Windows installer, made with Inno Setup 6 or 7 (https://jrsoftware.org/isinfo.php):
;
;   cargo build --release --locked
;   iscc packaging\windows\undavi.iss
;
; writes target\installer\undavi-<version>-x86_64-windows-setup.exe.
;
; It puts the same files in the same folder as install.ps1, but comes with an uninstaller
; of its own (unins000.exe). Updates go by that file to tell the two apart (see
; src/update/windows.rs), and install.ps1 leaves an Undavi with it alone.

#define Root AddBackslash(SourcePath) + "..\.."
#define Program Root + "\target\release\undavi.exe"
#define Version GetStringFileInfo(Program, "ProductVersion")
#if Version == ""
  #error There is no undavi.exe in target\release. Run: cargo build --release --locked
#endif
#define Page "https://github.com/karsoncaliff/undavi"

[Setup]
AppId=Undavi
AppName=Undavi
AppVersion={#Version}
AppVerName=Undavi {#Version}
AppPublisher=Karson Califf
AppPublisherURL={#Page}
AppSupportURL={#Page}/issues
AppUpdatesURL={#Page}/releases
AppCopyright=Copyright (C) 2026 Karson Califf
VersionInfoVersion={#GetVersionNumbersString(Program)}
; For the current user, without administrator rights: updates couldn't install
; themselves otherwise.
PrivilegesRequired=lowest
DefaultDirName={userpf}\Undavi
DisableDirPage=no
DisableProgramGroupPage=yes
; What single apps as sources need: Windows 10 version 2004.
MinVersion=10.0.19041
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; A running Undavi is moved out of the way instead (see MoveAside).
CloseApplications=no
OutputDir={#Root}\target\installer
OutputBaseFilename=undavi-{#Version}-x86_64-windows-setup
SetupIconFile={#Root}\assets\icons\windows\undavi.ico
UninstallDisplayName=Undavi
UninstallDisplayIcon={app}\bin\undavi.exe
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes

[Tasks]
Name: desktopicon; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[InstallDelete]
; Versions that were replaced while they were running.
Type: files; Name: "{app}\bin\undavi.exe.old*"
; If install.ps1 was here first: it's uninstalled from here now.
Type: files; Name: "{app}\install.ps1"

[Files]
Source: "{#Program}"; DestDir: "{app}\bin"; Flags: ignoreversion; BeforeInstall: MoveAside
; Undavi's license, the bundled font's and icons' licenses, and the notices of the Rust
; crates compiled into the program, which their licenses ask to travel with it.
Source: "{#Root}\LICENSE"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "{#Root}\NOTICE"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "{#Root}\assets\fonts\OFL-Inter.txt"; DestDir: "{app}\licenses"; Flags: ignoreversion
Source: "{#Root}\packaging\THIRD-PARTY-LICENSES.txt"; DestDir: "{app}\licenses"; Flags: ignoreversion
; Under another name: it's also called LICENSE.
Source: "{#Root}\assets\icons\ui\LICENSE"; DestDir: "{app}\licenses"; DestName: "Apache-Material-Symbols.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Undavi"; Filename: "{app}\bin\undavi.exe"; Comment: "Music visualizer with synced lyrics"
Name: "{autodesktop}\Undavi"; Filename: "{app}\bin\undavi.exe"; Comment: "Music visualizer with synced lyrics"; Tasks: desktopicon

[Registry]
; install.ps1's entry under "Installed apps", if it was here first.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Uninstall\Undavi"; ValueType: none; Flags: deletekey dontcreatekey; Check: ScriptWasHere

[Run]
Filename: "{app}\bin\undavi.exe"; Description: "{cm:LaunchProgram,Undavi}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: files; Name: "{app}\bin\undavi.exe.old*"
; These go by themselves, except where install.ps1 made them.
Type: dirifempty; Name: "{app}\bin"
Type: dirifempty; Name: "{app}\licenses"
Type: dirifempty; Name: "{app}"

[Code]
var
  Moved: String;

function UndaviExe: String;
begin
  Result := ExpandConstant('{app}\bin\undavi.exe');
end;

// A running program can't be overwritten, but it can be renamed. Without one running,
// it's removed again below; else by a later install, or the next time Undavi starts.
procedure MoveAside;
var
  Name: String;
begin
  Name := UndaviExe + '.old-' + GetDateTimeString('yyyymmddhhnnss', #0, #0);
  if FileExists(UndaviExe) and RenameFile(UndaviExe, Name) then
    Moved := Name;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and (Moved <> '') then
    DeleteFile(Moved);
end;

// Put the old one back if the new one didn't get there.
procedure DeinitializeSetup;
begin
  if (Moved <> '') and FileExists(Moved) and not FileExists(UndaviExe) then
    RenameFile(Moved, UndaviExe);
end;

function ScriptWasHere: Boolean;
var
  Prefix: String;
begin
  Result := RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Undavi', 'InstallLocation', Prefix)
    and (CompareText(RemoveBackslashUnlessRoot(Prefix), ExpandConstant('{app}')) = 0);
end;

// Whether Undavi is running: then its file can't be written to.
function IsRunning: Boolean;
var
  Stream: TFileStream;
begin
  Result := False;
  if FileExists(UndaviExe) then
  try
    Stream := TFileStream.Create(UndaviExe, fmOpenReadWrite or fmShareExclusive);
    Stream.Free;
  except
    Result := True;
  end;
end;

function InitializeUninstall: Boolean;
begin
  Result := True;
  while Result and IsRunning do
    Result := SuppressibleMsgBox('Undavi is running. Close it, then try again.', mbError, MB_RETRYCANCEL, IDCANCEL) = IDRETRY;
end;

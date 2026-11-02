#ifndef SourceDir
  #error SourceDir is required
#endif
#ifndef OutputDir
  #error OutputDir is required
#endif
#ifndef PackageName
  #error PackageName is required
#endif
#ifndef DisplayVersion
  #error DisplayVersion is required
#endif

[Setup]
; Stable per-user identity, separate from older preview installations.
AppId=WorkBuddyProxy
AppName=WorkBuddy Proxy
AppVersion={#DisplayVersion}
UninstallDisplayName=WorkBuddy Proxy
AppPublisher=gloryhui
AppPublisherURL=https://github.com/gloryhui/workbuddy-proxy
DefaultDirName={localappdata}\Programs\WorkBuddy Proxy
DefaultGroupName=WorkBuddy Proxy
DisableDirPage=no
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
WizardStyle=modern
SetupIconFile=..\..\assets\app-icon.ico
UninstallDisplayIcon={app}\workbuddy-proxy.exe
OutputDir={#OutputDir}
OutputBaseFilename={#PackageName}-setup
Compression=lzma2
SolidCompression=yes
CloseApplications=yes
CloseApplicationsFilter=*.exe
RestartApplications=no
UsePreviousAppDir=yes
Uninstallable=yes
ShowLanguageDialog=no
UsePreviousLanguage=no
UsePreviousTasks=no

[Languages]
Name: "chinesesimplified"; MessagesFile: "ChineseSimplified.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[InstallDelete]
; Remove only the legacy helper name from a previous portable layout.
Type: files; Name: "{app}\workbuddy-proxy-gpui.exe"

[UninstallDelete]
; Never sweep {app}: unrelated files and user data must remain untouched.
Type: dirifempty; Name: "{app}\internal"

[Icons]
Name: "{group}\WorkBuddy Proxy"; Filename: "{app}\workbuddy-proxy.exe"; WorkingDir: "{app}"
Name: "{group}\{cm:UninstallProgram,WorkBuddy Proxy}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\WorkBuddy Proxy"; Filename: "{app}\workbuddy-proxy.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\workbuddy-proxy.exe"; Description: "{cm:LaunchProgram,WorkBuddy Proxy}"; Flags: nowait postinstall skipifsilent

[Registry]
; The application opts in. Installation never creates/enables this value.
; Uninstall removes only our own Run value, retaining other programs' entries.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueName: "WorkBuddyProxy"; Flags: dontcreatekey uninsdeletevalue

; Configuration/logs are outside {app}; no user-data cleanup.

[Code]
function UninstallGetTopWindow(Parent: HWND): HWND;
  external 'GetTopWindow@user32.dll stdcall';
function UninstallGetNextWindow(Wnd: HWND; Command: Cardinal): HWND;
  external 'GetWindow@user32.dll stdcall';
function UninstallWindowProcessId(Wnd: HWND; var ProcessId: Cardinal): Cardinal;
  external 'GetWindowThreadProcessId@user32.dll stdcall';
function UninstallOpenProcess(Access: Cardinal; Inherit: BOOL; ProcessId: Cardinal): THandle;
  external 'OpenProcess@kernel32.dll stdcall';
function UninstallProcessImage(Process: THandle; Flags: Cardinal; Buffer: String; var Size: Cardinal): BOOL;
  external 'QueryFullProcessImageNameW@kernel32.dll stdcall';
function UninstallLongPath(Path: String; Buffer: String; Size: Cardinal): Cardinal;
  external 'GetLongPathNameW@kernel32.dll stdcall';
function UninstallOpenFile(Path: String; Access, Share: Cardinal; Security: UINT_PTR;
  Creation, Attributes: Cardinal; Template: THandle): THandle;
  external 'CreateFileW@kernel32.dll stdcall';
function UninstallCloseHandle(Handle: THandle): BOOL;
  external 'CloseHandle@kernel32.dll stdcall';

function CanonicalProgramPath(Path: String): String;
var
  Buffer: String;
  Size: Cardinal;
begin
  Result := '';
  Buffer := StringOfChar(#0, 32768);
  Size := UninstallLongPath(ExpandFileName(Path), Buffer, Length(Buffer));
  if (Size > 0) and (Size < Cardinal(Length(Buffer))) then
    Result := Copy(Buffer, 1, Size);
end;

function InstalledWindow(Wnd: HWND): Boolean;
var
  ProcessId, Size: Cardinal;
  Process: THandle;
  Image, MainPath, HelperPath, LegacyPath: String;
begin
  Result := False;
  ProcessId := 0;
  UninstallWindowProcessId(Wnd, ProcessId);
  if ProcessId = 0 then Exit;
  Process := UninstallOpenProcess($1000, False, ProcessId);
  if Process = 0 then Exit;
  try
    Image := StringOfChar(#0, 32768);
    Size := Length(Image);
    if not UninstallProcessImage(Process, 0, Image, Size) then Exit;
    Image := CanonicalProgramPath(Copy(Image, 1, Size));
    if Image = '' then Exit;
    MainPath := CanonicalProgramPath(ExpandConstant('{app}\workbuddy-proxy.exe'));
    HelperPath := CanonicalProgramPath(ExpandConstant('{app}\internal\workbuddy-settings.exe'));
    LegacyPath := CanonicalProgramPath(ExpandConstant('{app}\workbuddy-proxy-gpui.exe'));
    Result := ((MainPath <> '') and (CompareText(Image, MainPath) = 0)) or
      ((HelperPath <> '') and (CompareText(Image, HelperPath) = 0)) or
      ((LegacyPath <> '') and (CompareText(Image, LegacyPath) = 0));
  finally
    UninstallCloseHandle(Process);
  end;
end;

procedure CloseInstalledWindows;
var
  Wnd, Next: HWND;
begin
  Wnd := UninstallGetTopWindow(0);
  while Wnd <> 0 do
  begin
    Next := UninstallGetNextWindow(Wnd, 2); { GW_HWNDNEXT }
    if InstalledWindow(Wnd) then
      { The hidden tray's WM_CLOSE runs hostQuit: stop settings, then flush core. }
      PostMessage(Wnd, $0010, 0, 0); { WM_CLOSE }
    Wnd := Next;
  end;
end;

function ProgramFileReleased(Path: String): Boolean;
var
  Handle: THandle;
begin
  Result := True;
  if not FileExists(Path) then Exit;
  { OPEN_EXISTING without truncation or writing: a mapped executable rejects
    write access until it exits. Also stop safely on inaccessible files. }
  Handle := UninstallOpenFile(Path, $40000000, 7, 0, 3, $80, 0);
  Result := Handle <> THandle(-1);
  if Result then UninstallCloseHandle(Handle);
end;

function InitializeUninstall: Boolean;
var
  Attempt: Integer;
begin
  Log('Closing only processes whose executable matches this installation.');
  CloseInstalledWindows;
  for Attempt := 1 to 150 do
  begin
    Result := ProgramFileReleased(ExpandConstant('{app}\workbuddy-proxy.exe')) and
      ProgramFileReleased(ExpandConstant('{app}\internal\workbuddy-settings.exe')) and
      ProgramFileReleased(ExpandConstant('{app}\workbuddy-proxy-gpui.exe'));
    if Result then Exit;
    Sleep(100);
  end;
  Log('Uninstall aborted: an installed executable is still in use or inaccessible.');
  SuppressibleMsgBox('程序仍在运行或程序文件无法访问。请从托盘退出 WorkBuddy Proxy 后重试卸载。',
    mbError, MB_OK, IDOK);
  Result := False;
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssInstall then
  begin
    Log('Installer language: ' + ActiveLanguage);
    Log('Desktop task label: ' + CustomMessage('CreateDesktopIcon'));
    if WizardIsTaskSelected('desktopicon') then
      Log('Desktop shortcut selected: yes')
    else
      Log('Desktop shortcut selected: no');
  end;
end;

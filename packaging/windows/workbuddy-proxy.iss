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
  #define DisplayVersion "0.5.5"
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

; Configuration/logs are outside {app}. No UninstallDelete or user-data cleanup.

[Code]
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

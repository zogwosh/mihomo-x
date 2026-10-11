#ifndef AppVersion
  #error AppVersion is required
#endif
#ifndef SourceDirectory
  #error SourceDirectory is required
#endif
#ifndef OutputDirectory
  #error OutputDirectory is required
#endif
#ifndef Architecture
  #define Architecture "x64"
#endif

[Setup]
AppId={{9C9B7808-AB1C-43B6-A90D-5A736DE9E645}
AppName=mihomo-x
AppVersion={#AppVersion}
AppPublisher=zogwosh
AppPublisherURL=https://github.com/zogwosh/mihomo-x
AppSupportURL=https://github.com/zogwosh/mihomo-x/issues
DefaultDirName={autopf}\mihomo-x
DefaultGroupName=mihomo-x
DisableProgramGroupPage=yes
PrivilegesRequired=admin
#if Architecture == "x64"
ArchitecturesAllowed=x64os
ArchitecturesInstallIn64BitMode=x64os
#else
ArchitecturesAllowed=arm64
ArchitecturesInstallIn64BitMode=arm64
#endif
MinVersion=10.0.17763
OutputDir={#OutputDirectory}
OutputBaseFilename=mihomo-x-{#AppVersion}-{#Architecture}-setup
SetupIconFile=..\assets\mihomo-x.ico
UninstallDisplayIcon={app}\mihomo-x.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=no
RestartApplications=no
SetupLogging=yes

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "{#SourceDirectory}\*"; DestDir: "{app}"; Excludes: "AppX\*,Package.appxmanifest"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\mihomo-x"; Filename: "{app}\mihomo-x.exe"; WorkingDir: "{app}"
Name: "{commondesktop}\mihomo-x"; Filename: "{app}\mihomo-x.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[Run]
Filename: "{app}\mihomo-x.exe"; Description: "Launch mihomo-x"; Flags: nowait postinstall skipifsilent

[Code]
function ApplicationClosed: Boolean;
begin
  Result := FindWindowByWindowName('mihomo-x · Mihomo') = 0;
  if not Result then
    MsgBox('Please exit mihomo-x from its tray menu before installing or uninstalling.', mbError, MB_OK);
end;

function InitializeSetup: Boolean;
begin
  Result := ApplicationClosed;
end;

function InitializeUninstall: Boolean;
begin
  Result := ApplicationClosed;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  ResultCode: Integer;
begin
  if CurUninstallStep <> usUninstall then exit;
  if not Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    '-NoProfile -NonInteractive -Command "$ErrorActionPreference=''Stop''; Get-ScheduledTask | Where-Object TaskName -Like ''mihomo-x-S-1-5-*'' | Unregister-ScheduledTask -Confirm:$false"',
    '', SW_HIDE, ewWaitUntilTerminated, ResultCode) then
    MsgBox('Could not remove mihomo-x startup tasks. Please remove them in Task Scheduler.', mbError, MB_OK)
  else if ResultCode <> 0 then
    MsgBox('Removing mihomo-x startup tasks failed. Please check Task Scheduler.', mbError, MB_OK);
end;

; Inno Setup script for PerifeMngr (formerly Joystick Manager).
; Build with scripts\build-installer.ps1, which passes the version and the signing command.

#define AppName "PerifeMngr"
; Name used before the rename, for cleaning up older installs.
#define LegacyAppName "Joystick Manager"
#define LegacyAppExeName "JoystickManager.exe"
#define AppPublisher "NikiCoding"
#define AppPublisherUrl "https://nikicoding.com"
#define AppRepositoryUrl "https://github.com/NNKTV28/JoistickMngr"
#define AppExeName "PerifeMngr.exe"

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{8F3C2A61-5B7E-4D2A-9C41-2E6F7A9B1D35}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppPublisherUrl}
AppSupportURL={#AppRepositoryUrl}/issues
AppUpdatesURL={#AppRepositoryUrl}/releases
DefaultDirName={autopf}\NikiCoding\PerifeMngr
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
OutputDir=..\dist
OutputBaseFilename=PerifeMngr-Setup-{#AppVersion}
SetupIconFile=..\assets\app.ico
UninstallDisplayIcon={app}\{#AppExeName}
UninstallDisplayName={#AppName}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=force
RestartApplications=no
VersionInfoCompany={#AppPublisher}
VersionInfoDescription={#AppName} Setup
VersionInfoProductName={#AppName}
VersionInfoVersion={#AppVersion}
#ifdef UseSignTool
SignTool=nikicoding
SignedUninstaller=yes
#endif

[Tasks]
Name: "autostart"; Description: "Start {#AppName} when I sign in to Windows"; GroupDescription: "Start-up:"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[InstallDelete]
; Updating from a version before the rename: remove the old program file and shortcuts.
Type: files; Name: "{app}\{#LegacyAppExeName}"
Type: files; Name: "{autoprograms}\{#LegacyAppName}.lnk"
Type: files; Name: "{autodesktop}\{#LegacyAppName}.lnk"

[Files]
Source: "..\target\release\{#AppExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\fonts\OFL-*.txt"; DestDir: "{app}\licenses"; Flags: ignoreversion

[Icons]
; The Start menu entry is what makes the app show up in Windows search.
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExeName}"; AppUserModelID: "NikiCoding.PerifeMngr"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExeName}"; Parameters: "--register-autostart"; Flags: runhidden waituntilterminated; Tasks: autostart; StatusMsg: "Setting up start-up..."
Filename: "{app}\{#AppExeName}"; Parameters: "--unregister-autostart"; Flags: runhidden waituntilterminated; Tasks: not autostart
; postinstall entries run as the original (non-elevated) user by default; the app needs administrator rights.
Filename: "{app}\{#AppExeName}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent runascurrentuser
; Silent installs come from the in-app updater: start the updated app again.
Filename: "{app}\{#AppExeName}"; Flags: nowait runascurrentuser; Check: WizardSilent

[UninstallDelete]
; Installers downloaded by the in-app updater.
Type: filesandordirs; Name: "{app}\updates"

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM {#AppExeName}"; Flags: runhidden; RunOnceId: "StopApplication"
Filename: "{app}\{#AppExeName}"; Parameters: "--unregister-autostart"; Flags: runhidden waituntilterminated; RunOnceId: "RemoveAutostart"

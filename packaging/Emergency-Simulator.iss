; Emergency-Simulator Windows installer
; Inno Setup 6 script. Per-user installation; no administrator rights are required.

#define AppVersion "1.1.0"

[Setup]
AppId={{9A31E6AC-1D0B-4F40-9C44-4A7F76DB6B13}
AppName=Emergency-Simulator
AppVersion={#AppVersion}
AppVerName=Emergency-Simulator {#AppVersion}
AppPublisher=Emergency-Simulator project
AppPublisherURL=https://github.com/tirodz/Emergency-Simulator-
AppSupportURL=https://github.com/tirodz/Emergency-Simulator-/issues
DefaultDirName={localappdata}\Programs\Emergency-Simulator
DefaultGroupName=Emergency-Simulator
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=installer-output
OutputBaseFilename=Emergency-Simulator-Setup-{#AppVersion}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\Emergency-Simulator.exe
CloseApplications=yes
RestartApplications=no
VersionInfoVersion={#AppVersion}.0
VersionInfoCompany=Emergency-Simulator project
VersionInfoDescription=Emergency-Simulator Windows installer
VersionInfoProductName=Emergency-Simulator
VersionInfoProductVersion={#AppVersion}.0

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
Source: "..\dist\Emergency-Simulator.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: isreadme

[Icons]
Name: "{autoprograms}\Emergency-Simulator"; Filename: "{app}\Emergency-Simulator.exe"; WorkingDir: "{app}"
Name: "{userdesktop}\Emergency-Simulator"; Filename: "{app}\Emergency-Simulator.exe"; WorkingDir: "{app}"

[Run]
Filename: "{app}\Emergency-Simulator.exe"; Description: "Launch Emergency-Simulator"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: filesandordirs; Name: "{localappdata}\Emergency-Simulator"

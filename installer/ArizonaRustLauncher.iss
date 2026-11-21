#ifndef AppVersion
  #define AppVersion "1.0.0"
#endif

[Setup]
AppId={{F05895BA-7110-4EE8-9F90-C74B59DB73AF}
AppName=Arizona Rust Launcher
AppVersion={#AppVersion}
AppVerName=Arizona Rust Launcher {#AppVersion}
AppComments=Unofficial Rust launcher for Arizona Games
DefaultDirName={userpf}\Arizona Rust Launcher
DefaultGroupName=Arizona Rust Launcher
DisableProgramGroupPage=yes
UsePreviousAppDir=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes
RestartApplications=no
UninstallDisplayIcon={app}\ArizonaRustLauncher.exe
OutputDir=..\target\installer
OutputBaseFilename=ArizonaRustLauncher-{#AppVersion}-Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Создать ярлык на рабочем столе"; GroupDescription: "Ярлыки:"; Flags: unchecked

[Files]
Source: "..\target\release\arizona-rust-launcher.exe"; DestDir: "{app}"; DestName: "ArizonaRustLauncher.exe"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\CONTRIBUTORS.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\docs\screenshot.png"; DestDir: "{app}\docs"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Arizona Rust Launcher"; Filename: "{app}\ArizonaRustLauncher.exe"
Name: "{autodesktop}\Arizona Rust Launcher"; Filename: "{app}\ArizonaRustLauncher.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\ArizonaRustLauncher.exe"; Description: "Запустить Arizona Rust Launcher"; Flags: nowait postinstall skipifsilent

; Instalador do Snapnab (Inno Setup 6) — empacota dist\Snapnab\ gerado pelo build.bat
;   "%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe" packaging\installer\Snapnab.iss
; Ao mudar a versão no CMakeLists.txt, atualize AppVersion abaixo.

#define AppName      "Snapnab"
#define AppVersion   "0.1.0"
#define AppPublisher "Antonio Rezende"
#define SrcDir       "..\..\dist\Snapnab"

[Setup]
AppId={{C59F0FFC-FF4A-4107-8483-9934772DA4EB}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppCopyright=Copyright © 2026 {#AppPublisher}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
OutputDir=..\..\dist
OutputBaseFilename=Snapnab-{#AppVersion}-setup
SetupIconFile=..\..\src\resources\Snapnab.ico
UninstallDisplayIcon={app}\Snapnab.exe
LicenseFile=..\..\LICENSE
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.19041
; instala só para o usuário atual (sem UAC); o diálogo deixa escolher "todos os usuários"
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
; o instalador/desinstalador pede para fechar o Snapnab se ele estiver aberto
AppMutex=Snapnab-Capture-{{41F1BF9C-5A4E-4B6F-9C33-1D2E3F405A61},Snapnab-Editor-{{800F2EB3-7C1D-4E2A-A3B4-C5D6E7F80912}
CloseApplications=yes

[Languages]
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "startup"; Description: "Iniciar o Snapnab com o Windows"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#SrcDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs

[Icons]
Name: "{autoprograms}\Snapnab"; Filename: "{app}\Snapnab.exe"
Name: "{autoprograms}\Snapnab Editor"; Filename: "{app}\SnapnabEditor.exe"
Name: "{autodesktop}\Snapnab"; Filename: "{app}\Snapnab.exe"; Tasks: desktopicon

[Registry]
; mesmo valor que Opções > "Iniciar com o Windows" grava; removido na desinstalação
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Snapnab"; \
    ValueData: """{app}\Snapnab.exe"" /hide"; Tasks: startup; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "Snapnab"; \
    Flags: dontcreatekey uninsdeletevalue

[Run]
Filename: "{app}\Snapnab.exe"; Description: "{cm:LaunchProgram,Snapnab}"; Flags: nowait postinstall skipifsilent

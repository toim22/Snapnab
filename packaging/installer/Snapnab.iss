; Instalador do Snapnab (Inno Setup 6.5 ou mais novo) — empacota dist\Snapnab\ gerado pelo build.bat.
; O build.bat compila sozinho quando acha o Inno Setup e passa a versão do CMakeLists.txt:
;   ISCC.exe /DAppVersion=0.2.0 packaging\installer\Snapnab.iss   →   dist\Snapnab-0.2.0-setup.exe
;
; Instala só para o usuário atual (sem pedir administrador, em %LOCALAPPDATA%\Programs\Snapnab) ou, se a pessoa
; escolher, para todos os usuários (em Arquivos de Programas). As capturas, presets e configurações ficam no perfil
; do usuário e continuam lá depois de desinstalar.

#define AppName      "Snapnab"
#ifndef AppVersion
  #define AppVersion "0.2.0"
#endif
#define AppPublisher "Antonio Rezende"
#define AppURL       "https://github.com/toim22/Snapnab"
#define SrcDir       "..\..\dist\Snapnab"

[Setup]
AppId={{C59F0FFC-FF4A-4107-8483-9934772DA4EB}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppURL}
AppSupportURL={#AppURL}/issues
AppUpdatesURL={#AppURL}/releases
AppCopyright=Copyright © 2026 {#AppPublisher}
VersionInfoVersion={#AppVersion}
VersionInfoCompany={#AppPublisher}
VersionInfoDescription={#AppName} Setup
VersionInfoProductName={#AppName}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
OutputDir=..\..\dist
OutputBaseFilename=Snapnab-{#AppVersion}-setup
SetupIconFile=..\..\src\resources\Snapnab.ico
UninstallDisplayIcon={app}\Snapnab.exe
UninstallDisplayName={#AppName}
LicenseFile=..\..\LICENSE
Compression=lzma2/max
SolidCompression=yes
; assistente moderno que acompanha o tema claro/escuro do Windows, com o ícone do Snapnab
WizardStyle=modern dynamic
WizardImageFile=WizardLarge.png
WizardSmallImageFile=WizardSmall.png
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.19041
; instala só para o usuário atual (sem UAC); o diálogo deixa escolher "todos os usuários". Se a pessoa escolher
; uma pasta protegida (como Arquivos de Programas) no modo do usuário, o [Code] oferece reiniciar como administrador
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=commandline dialog
; o instalador/desinstalador fecha o Snapnab se ele estiver aberto (e o reabre depois de atualizar)
AppMutex=Snapnab-Capture-{{41F1BF9C-5A4E-4B6F-9C33-1D2E3F405A61},Snapnab-Editor-{{800F2EB3-7C1D-4E2A-A3B4-C5D6E7F80912}
CloseApplications=yes
; o idioma do assistente segue o do Windows; a lista só aparece se ele não estiver entre os 12
ShowLanguageDialog=auto

[Languages]
; os mesmos 12 idiomas do programa: 7 com tradução oficial do Inno Setup e 5 em Languages\ (feitos para o Snapnab)
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "chinesesimplified"; MessagesFile: "Languages\ChineseSimplified.isl"
Name: "hindi"; MessagesFile: "Languages\Hindi.isl"
Name: "spanish"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "arabic"; MessagesFile: "compiler:Languages\Arabic.isl"
Name: "french"; MessagesFile: "compiler:Languages\French.isl"
Name: "bengali"; MessagesFile: "Languages\Bengali.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "indonesian"; MessagesFile: "Languages\Indonesian.isl"
Name: "urdu"; MessagesFile: "Languages\Urdu.isl"
Name: "german"; MessagesFile: "compiler:Languages\German.isl"

[CustomMessages]
; pasta escolhida exige administrador (modo "só para mim") / pasta sem permissão de escrita mesmo como administrador
brazilianportuguese.DirNeedsAdmin=A pasta escolhida exige permissão de administrador:%n%n%1%n%nPara instalar nela, o Snapnab precisa ser instalado para todos os usuários. Deseja reiniciar a instalação como administrador?
brazilianportuguese.DirNotWritable=Não é possível gravar na pasta escolhida:%n%n%1%n%nEscolha outra pasta.
english.DirNeedsAdmin=The selected folder requires administrator permission:%n%n%1%n%nTo install there, Snapnab must be installed for all users. Restart Setup as administrator?
english.DirNotWritable=Setup cannot write to the selected folder:%n%n%1%n%nPlease choose another folder.
chinesesimplified.DirNeedsAdmin=所选文件夹需要管理员权限：%n%n%1%n%n要安装到此处，必须为所有用户安装 Snapnab。是否以管理员身份重新启动安装程序？
chinesesimplified.DirNotWritable=无法写入所选文件夹：%n%n%1%n%n请选择其他文件夹。
hindi.DirNeedsAdmin=चुने गए फ़ोल्डर के लिए व्यवस्थापक अनुमति आवश्यक है:%n%n%1%n%nवहाँ इंस्टॉल करने के लिए Snapnab को सभी उपयोगकर्ताओं के लिए इंस्टॉल करना होगा। क्या सेटअप को व्यवस्थापक के रूप में फिर से शुरू करें?
hindi.DirNotWritable=चुने गए फ़ोल्डर में लिखा नहीं जा सकता:%n%n%1%n%nकृपया कोई दूसरा फ़ोल्डर चुनें।
spanish.DirNeedsAdmin=La carpeta seleccionada requiere permisos de administrador:%n%n%1%n%nPara instalar allí, Snapnab debe instalarse para todos los usuarios. ¿Desea reiniciar la instalación como administrador?
spanish.DirNotWritable=No se puede escribir en la carpeta seleccionada:%n%n%1%n%nElija otra carpeta.
arabic.DirNeedsAdmin=يتطلب المجلد المحدد صلاحيات المسؤول:%n%n%1%n%nللتثبيت فيه، يجب تثبيت Snapnab لجميع المستخدمين. هل تريد إعادة تشغيل الإعداد كمسؤول؟
arabic.DirNotWritable=تتعذّر الكتابة في المجلد المحدد:%n%n%1%n%nيُرجى اختيار مجلد آخر.
french.DirNeedsAdmin=Le dossier choisi nécessite les droits d'administrateur :%n%n%1%n%nPour l'installer à cet emplacement, Snapnab doit être installé pour tous les utilisateurs. Redémarrer l'installation en tant qu'administrateur ?
french.DirNotWritable=Impossible d'écrire dans le dossier choisi :%n%n%1%n%nVeuillez choisir un autre dossier.
bengali.DirNeedsAdmin=নির্বাচিত ফোল্ডারের জন্য প্রশাসকের অনুমতি প্রয়োজন:%n%n%1%n%nসেখানে ইনস্টল করতে Snapnab সব ব্যবহারকারীর জন্য ইনস্টল করতে হবে। প্রশাসক হিসেবে সেটআপ আবার শুরু করবেন?
bengali.DirNotWritable=নির্বাচিত ফোল্ডারে লেখা যাচ্ছে না:%n%n%1%n%nঅন্য একটি ফোল্ডার বেছে নিন।
russian.DirNeedsAdmin=Для выбранной папки требуются права администратора:%n%n%1%n%nЧтобы установить Snapnab в эту папку, его нужно установить для всех пользователей. Перезапустить установку от имени администратора?
russian.DirNotWritable=Невозможно выполнить запись в выбранную папку:%n%n%1%n%nВыберите другую папку.
indonesian.DirNeedsAdmin=Folder yang dipilih memerlukan izin administrator:%n%n%1%n%nUntuk memasang di sana, Snapnab harus dipasang untuk semua pengguna. Mulai ulang penyiapan sebagai administrator?
indonesian.DirNotWritable=Tidak dapat menulis ke folder yang dipilih:%n%n%1%n%nSilakan pilih folder lain.
urdu.DirNeedsAdmin=منتخب فولڈر کے لیے منتظم کی اجازت درکار ہے:%n%n%1%n%nوہاں انسٹال کرنے کے لیے Snapnab کو تمام صارفین کے لیے انسٹال کرنا ہوگا۔ کیا سیٹ اپ کو بطور منتظم دوبارہ شروع کریں؟
urdu.DirNotWritable=منتخب فولڈر میں لکھا نہیں جا سکتا:%n%n%1%n%nبراہ کرم کوئی دوسرا فولڈر منتخب کریں۔
german.DirNeedsAdmin=Der ausgewählte Ordner erfordert Administratorrechte:%n%n%1%n%nUm Snapnab dort zu installieren, muss es für alle Benutzer installiert werden. Setup als Administrator neu starten?
german.DirNotWritable=In den ausgewählten Ordner kann nicht geschrieben werden:%n%n%1%n%nBitte wählen Sie einen anderen Ordner.

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "startup"; Description: "{cm:AutoStartProgram,{#AppName}}"; GroupDescription: "{cm:AutoStartProgramGroupDescription}"

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

[Code]
// No modo "só para mim" o Setup roda sem administrador; se a pessoa trocar a pasta para uma protegida (Arquivos de
// Programas, raiz do C:\…), a cópia falharia com "acesso negado". Ao sair da página da pasta, o Setup testa se
// consegue gravar nela e, se não, oferece reiniciar como administrador (/ALLUSERS) já com a pasta escolhida.
var
  Relaunching: Boolean;

// verdadeiro na instância reiniciada como administrador (já aceitou a licença)
function IsRelaunch: Boolean;
var
  I: Integer;
begin
  Result := False;
  for I := 1 to ParamCount do
    if CompareText(ParamStr(I), '/RELAUNCHED') = 0 then
      Result := True;
end;

// pasta mais próxima que já existe (a de instalação ainda pode não existir)
function ExistingAncestor(Dir: String): String;
var
  Parent: String;
begin
  Result := RemoveBackslashUnlessRoot(Dir);
  while not DirExists(Result) do
  begin
    Parent := ExtractFileDir(Result);
    if (Parent = '') or (Parent = Result) then
    begin
      Result := '';
      Exit;
    end;
    Result := Parent;
  end;
end;

function CanWriteTo(Dir: String): Boolean;
var
  Probe: String;
begin
  Dir := ExistingAncestor(Dir);
  Result := Dir <> '';
  if not Result then
    Exit;
  Probe := AddBackslash(Dir) + '~snapnab-setup-' + GetDateTimeString('hhnnsszzz', #0, #0) + '.tmp';
  Result := SaveStringToFile(Probe, '', False);
  if Result then
    DeleteFile(Probe);
end;

function ShouldSkipPage(PageID: Integer): Boolean;
begin
  Result := (PageID = wpLicense) and IsRelaunch;
end;

function NextButtonClick(CurPageID: Integer): Boolean;
var
  Dir: String;
  ErrorCode: Integer;
begin
  Result := True;
  if (CurPageID <> wpSelectDir) or WizardSilent then
    Exit;
  Dir := WizardDirValue;
  if CanWriteTo(Dir) then
    Exit;
  Result := False;
  if IsAdminInstallMode then
  begin
    MsgBox(FmtMessage(CustomMessage('DirNotWritable'), [Dir]), mbError, MB_OK);
    Exit;
  end;
  if MsgBox(FmtMessage(CustomMessage('DirNeedsAdmin'), [Dir]), mbConfirmation, MB_YESNO) <> IDYES then
    Exit;
  if ShellExec('runas', ExpandConstant('{srcexe}'),
               '/ALLUSERS /RELAUNCHED /LANG=' + ActiveLanguage + ' /DIR="' + Dir + '"',
               '', SW_SHOW, ewNoWait, ErrorCode) then
  begin
    Relaunching := True;
    WizardForm.Close;
  end
  else if ErrorCode <> 1223 then  // 1223: a pessoa recusou o UAC; continua nesta página
    MsgBox(SysErrorMessage(ErrorCode), mbError, MB_OK);
end;

// fecha esta instância sem perguntar "Deseja sair da instalação?" quando a elevada já abriu
procedure CancelButtonClick(CurPageID: Integer; var Cancel, Confirm: Boolean);
begin
  if Relaunching then
    Confirm := False;
end;

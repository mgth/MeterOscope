; Installeur Windows de MeterOscope (Inno Setup 6).
; Compilation : iscc /DAppVersion=1.0.0 /DSourceDir=<dossier contenant meteroscope.exe> installer\meteroscope.iss
; Le fichier produit est installer\Output\MeterOscope-<version>-setup-x64.exe.

#ifndef AppVersion
  #define AppVersion "1.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\target\release"
#endif

#define AppName "MeterOscope"
#define AppExe "MeterOscope.exe"
#define ProgId "MeterOscope.Project"

[Setup]
; Ne jamais changer cet identifiant : il relie les mises à jour à l'installation existante.
AppId={{6F81631B-0696-4EB7-A6BA-83F41EBC8AB0}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Mathieu GRENET
AppPublisherURL=https://github.com/mgth/MeterOscope
AppSupportURL=https://github.com/mgth/MeterOscope/issues
AppUpdatesURL=https://github.com/mgth/MeterOscope/releases
VersionInfoVersion={#AppVersion}
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
; Installation pour tous (admin) ou pour l'utilisateur seul, au choix.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
LicenseFile=..\LICENSE
SetupIconFile=..\assets\icon\meteroscope.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
OutputDir=Output
OutputBaseFilename={#AppName}-{#AppVersion}-setup-x64
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=yes
CloseApplications=yes

[Languages]
; Mêmes langues que l'application (et que LittleBigMouse) ; l'assistant suit la langue du système.
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"
Name: "de"; MessagesFile: "compiler:Languages\German.isl"
Name: "es"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "it"; MessagesFile: "compiler:Languages\Italian.isl"
Name: "pl"; MessagesFile: "compiler:Languages\Polish.isl"
Name: "ptbr"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "ja"; MessagesFile: "compiler:Languages\Japanese.isl"
Name: "ko"; MessagesFile: "compiler:Languages\Korean.isl"
Name: "zhcn"; MessagesFile: "languages\ChineseSimplified.isl"

[CustomMessages]
en.AssocProject=Associate .meteroscope project files with MeterOscope
fr.AssocProject=Associer les fichiers projet .meteroscope à MeterOscope
de.AssocProject=Projektdateien (.meteroscope) mit MeterOscope verknüpfen
es.AssocProject=Asociar los archivos de proyecto .meteroscope con MeterOscope
it.AssocProject=Associa i file di progetto .meteroscope a MeterOscope
pl.AssocProject=Skojarz pliki projektów .meteroscope z MeterOscope
ptbr.AssocProject=Associar os arquivos de projeto .meteroscope ao MeterOscope
ja.AssocProject=プロジェクト ファイル (.meteroscope) を MeterOscope に関連付ける
ko.AssocProject=프로젝트 파일(.meteroscope)을 MeterOscope에 연결
zhcn.AssocProject=将 .meteroscope 项目文件关联到 MeterOscope
en.ProjectType=MeterOscope project
fr.ProjectType=Projet MeterOscope
de.ProjectType=MeterOscope-Projekt
es.ProjectType=Proyecto de MeterOscope
it.ProjectType=Progetto MeterOscope
pl.ProjectType=Projekt MeterOscope
ptbr.ProjectType=Projeto do MeterOscope
ja.ProjectType=MeterOscope プロジェクト
ko.ProjectType=MeterOscope 프로젝트
zhcn.ProjectType=MeterOscope 项目

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "assoc"; Description: "{cm:AssocProject}"

[Files]
Source: "{#SourceDir}\meteroscope.exe"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"
Source: "..\README.md"; DestDir: "{app}"

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
; Type de fichier « projet MeterOscope ».
Root: HKA; Subkey: "Software\Classes\.meteroscope"; ValueType: string; ValueName: ""; ValueData: "{#ProgId}"; Flags: uninsdeletevalue; Tasks: assoc
Root: HKA; Subkey: "Software\Classes\.meteroscope\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\{#ProgId}"; ValueType: string; ValueName: ""; ValueData: "{cm:ProjectType}"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\{#ProgId}\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#AppExe},0"
Root: HKA; Subkey: "Software\Classes\{#ProgId}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""
; « Ouvrir avec » pour les images prises en charge (sans voler l'association par défaut).
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "{#AppName}"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\SupportedTypes"; ValueType: string; ValueName: ".meteroscope"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\SupportedTypes"; ValueType: string; ValueName: ".png"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\SupportedTypes"; ValueType: string; ValueName: ".jpg"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\SupportedTypes"; ValueType: string; ValueName: ".jpeg"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\SupportedTypes"; ValueType: string; ValueName: ".bmp"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\SupportedTypes"; ValueType: string; ValueName: ".tif"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\SupportedTypes"; ValueType: string; ValueName: ".tiff"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\SupportedTypes"; ValueType: string; ValueName: ".webp"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\SupportedTypes"; ValueType: string; ValueName: ".gif"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.png\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}.Image"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.jpg\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}.Image"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.jpeg\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}.Image"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.bmp\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}.Image"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.tif\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}.Image"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.tiff\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}.Image"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.webp\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}.Image"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\.gif\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}.Image"; ValueData: ""; Flags: uninsdeletevalue
Root: HKA; Subkey: "Software\Classes\{#ProgId}.Image"; ValueType: string; ValueName: ""; ValueData: "Image ({#AppName})"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\{#ProgId}.Image\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#AppExe},0"
Root: HKA; Subkey: "Software\Classes\{#ProgId}.Image\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

#define AppName "Resonate"
#define AppUserModelId "org.resonate.Resonate"
#define ProgramId "Resonate.Audio"
#define Executable "resonate.exe"
#define LinkScheme "resonate"

[Setup]
AppId={{7052C132-1EB4-4DC5-9358-0DEEB4F1654F}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher=Resonate
DefaultDirName={autopf}\{#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed={#InstallArchitecture}
ArchitecturesInstallIn64BitMode={#InstallArchitecture}
LicenseFile={#RepositoryRoot}\LICENSE
SetupIconFile=resonate.ico
UninstallDisplayIcon={app}\{#Executable}
OutputDir={#OutputDirectory}
OutputBaseFilename={#OutputName}
VersionInfoVersion={#NumericVersion}
VersionInfoProductName={#AppName}
VersionInfoDescription=Resonate setup
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
ChangesAssociations=yes
CloseApplications=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Files]
Source: "{#SourceExecutable}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceSignIn}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepositoryRoot}\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#RepositoryRoot}\COPYRIGHT"; DestDir: "{app}"; Flags: ignoreversion
Source: "README.txt"; DestDir: "{app}"; Flags: ignoreversion isreadme

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#Executable}"; AppUserModelID: "{#AppUserModelId}"

[Run]
Filename: "{app}\{#Executable}"; Description: "Open {#AppName}"; Flags: nowait postinstall skipifsilent

[Registry]
Root: HKCU; Subkey: "Software\Classes\{#ProgramId}"; ValueType: string; ValueData: "Resonate audio file"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\{#ProgramId}\DefaultIcon"; ValueType: string; ValueData: "{app}\{#Executable},0"
Root: HKCU; Subkey: "Software\Classes\{#ProgramId}\shell\open\command"; ValueType: string; ValueData: """{app}\{#Executable}"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\{#LinkScheme}"; ValueType: string; ValueData: "URL:{#AppName}"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\{#LinkScheme}"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\{#LinkScheme}\DefaultIcon"; ValueType: string; ValueData: "{app}\{#Executable},0"
Root: HKCU; Subkey: "Software\Classes\{#LinkScheme}\shell\open\command"; ValueType: string; ValueData: """{app}\{#Executable}"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "{#AppName}"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}\shell\open\command"; ValueType: string; ValueData: """{app}\{#Executable}"" ""%1"""
Root: HKCU; Subkey: "Software\{#AppName}"; ValueType: none; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\{#AppName}\Capabilities"; ValueType: string; ValueName: "ApplicationName"; ValueData: "{#AppName}"
Root: HKCU; Subkey: "Software\{#AppName}\Capabilities"; ValueType: string; ValueName: "ApplicationDescription"; ValueData: "A music player for audio enthusiasts"
Root: HKCU; Subkey: "Software\RegisteredApplications"; ValueType: string; ValueName: "{#AppName}"; ValueData: "Software\{#AppName}\Capabilities"; Flags: uninsdeletevalue uninsdeletekeyifempty

Root: HKCU; Subkey: "Software\Classes\.aac\OpenWithProgids"; ValueType: string; ValueName: "{#ProgramId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKCU; Subkey: "Software\Classes\.aif\OpenWithProgids"; ValueType: string; ValueName: "{#ProgramId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKCU; Subkey: "Software\Classes\.aiff\OpenWithProgids"; ValueType: string; ValueName: "{#ProgramId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKCU; Subkey: "Software\Classes\.ape\OpenWithProgids"; ValueType: string; ValueName: "{#ProgramId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKCU; Subkey: "Software\Classes\.caf\OpenWithProgids"; ValueType: string; ValueName: "{#ProgramId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKCU; Subkey: "Software\Classes\.dff\OpenWithProgids"; ValueType: string; ValueName: "{#ProgramId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKCU; Subkey: "Software\Classes\.dsf\OpenWithProgids"; ValueType: string; ValueName: "{#ProgramId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKCU; Subkey: "Software\Classes\.flac\OpenWithProgids"; ValueType: string; ValueName: "{#ProgramId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKCU; Subkey: "Software\Classes\.m4a\OpenWithProgids"; ValueType: string; ValueName: "{#ProgramId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKCU; Subkey: "Software\Classes\.m4b\OpenWithProgids"; ValueType: string; ValueName: "{#ProgramId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKCU; Subkey: "Software\Classes\.mka\OpenWithProgids"; ValueType: string; ValueName: "{#ProgramId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKCU; Subkey: "Software\Classes\.mp3\OpenWithProgids"; ValueType: string; ValueName: "{#ProgramId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKCU; Subkey: "Software\Classes\.mp4\OpenWithProgids"; ValueType: string; ValueName: "{#ProgramId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKCU; Subkey: "Software\Classes\.oga\OpenWithProgids"; ValueType: string; ValueName: "{#ProgramId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKCU; Subkey: "Software\Classes\.ogg\OpenWithProgids"; ValueType: string; ValueName: "{#ProgramId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKCU; Subkey: "Software\Classes\.opus\OpenWithProgids"; ValueType: string; ValueName: "{#ProgramId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKCU; Subkey: "Software\Classes\.rf64\OpenWithProgids"; ValueType: string; ValueName: "{#ProgramId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKCU; Subkey: "Software\Classes\.w64\OpenWithProgids"; ValueType: string; ValueName: "{#ProgramId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKCU; Subkey: "Software\Classes\.wav\OpenWithProgids"; ValueType: string; ValueName: "{#ProgramId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKCU; Subkey: "Software\Classes\.wave\OpenWithProgids"; ValueType: string; ValueName: "{#ProgramId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty
Root: HKCU; Subkey: "Software\Classes\.wv\OpenWithProgids"; ValueType: string; ValueName: "{#ProgramId}"; ValueData: ""; Flags: uninsdeletevalue uninsdeletekeyifempty

Root: HKCU; Subkey: "Software\{#AppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".aac"; ValueData: "{#ProgramId}"
Root: HKCU; Subkey: "Software\{#AppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".aif"; ValueData: "{#ProgramId}"
Root: HKCU; Subkey: "Software\{#AppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".aiff"; ValueData: "{#ProgramId}"
Root: HKCU; Subkey: "Software\{#AppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".ape"; ValueData: "{#ProgramId}"
Root: HKCU; Subkey: "Software\{#AppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".caf"; ValueData: "{#ProgramId}"
Root: HKCU; Subkey: "Software\{#AppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".dff"; ValueData: "{#ProgramId}"
Root: HKCU; Subkey: "Software\{#AppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".dsf"; ValueData: "{#ProgramId}"
Root: HKCU; Subkey: "Software\{#AppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".flac"; ValueData: "{#ProgramId}"
Root: HKCU; Subkey: "Software\{#AppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".m4a"; ValueData: "{#ProgramId}"
Root: HKCU; Subkey: "Software\{#AppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".m4b"; ValueData: "{#ProgramId}"
Root: HKCU; Subkey: "Software\{#AppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".mka"; ValueData: "{#ProgramId}"
Root: HKCU; Subkey: "Software\{#AppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".mp3"; ValueData: "{#ProgramId}"
Root: HKCU; Subkey: "Software\{#AppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".mp4"; ValueData: "{#ProgramId}"
Root: HKCU; Subkey: "Software\{#AppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".oga"; ValueData: "{#ProgramId}"
Root: HKCU; Subkey: "Software\{#AppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".ogg"; ValueData: "{#ProgramId}"
Root: HKCU; Subkey: "Software\{#AppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".opus"; ValueData: "{#ProgramId}"
Root: HKCU; Subkey: "Software\{#AppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".rf64"; ValueData: "{#ProgramId}"
Root: HKCU; Subkey: "Software\{#AppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".w64"; ValueData: "{#ProgramId}"
Root: HKCU; Subkey: "Software\{#AppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".wav"; ValueData: "{#ProgramId}"
Root: HKCU; Subkey: "Software\{#AppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".wave"; ValueData: "{#ProgramId}"
Root: HKCU; Subkey: "Software\{#AppName}\Capabilities\FileAssociations"; ValueType: string; ValueName: ".wv"; ValueData: "{#ProgramId}"

Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}\SupportedTypes"; ValueType: string; ValueName: ".aac"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}\SupportedTypes"; ValueType: string; ValueName: ".aif"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}\SupportedTypes"; ValueType: string; ValueName: ".aiff"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}\SupportedTypes"; ValueType: string; ValueName: ".ape"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}\SupportedTypes"; ValueType: string; ValueName: ".caf"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}\SupportedTypes"; ValueType: string; ValueName: ".dff"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}\SupportedTypes"; ValueType: string; ValueName: ".dsf"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}\SupportedTypes"; ValueType: string; ValueName: ".flac"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}\SupportedTypes"; ValueType: string; ValueName: ".m4a"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}\SupportedTypes"; ValueType: string; ValueName: ".m4b"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}\SupportedTypes"; ValueType: string; ValueName: ".mka"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}\SupportedTypes"; ValueType: string; ValueName: ".mp3"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}\SupportedTypes"; ValueType: string; ValueName: ".mp4"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}\SupportedTypes"; ValueType: string; ValueName: ".oga"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}\SupportedTypes"; ValueType: string; ValueName: ".ogg"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}\SupportedTypes"; ValueType: string; ValueName: ".opus"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}\SupportedTypes"; ValueType: string; ValueName: ".rf64"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}\SupportedTypes"; ValueType: string; ValueName: ".w64"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}\SupportedTypes"; ValueType: string; ValueName: ".wav"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}\SupportedTypes"; ValueType: string; ValueName: ".wave"; ValueData: ""
Root: HKCU; Subkey: "Software\Classes\Applications\{#Executable}\SupportedTypes"; ValueType: string; ValueName: ".wv"; ValueData: ""

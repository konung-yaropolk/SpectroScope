; Inno Setup script for the SpectroScope Windows installer.
;
; Built in CI (the build-windows job in .github/workflows/release.yml) with:
;     iscc /DMyAppVersion=<x.y.z> packaging\windows\spectroscope.iss
; It packages the already-built SpectroScope.exe (the wgpu release the job
; stages at the repo root) and emits SpectroScope-setup.exe there. No rebuild
; happens here -- the exe must exist before iscc runs.
;
; Per-user install (no UAC): `PrivilegesRequired=lowest` puts the app under the
; user's Programs folder, which is all SpectroScope needs — it writes nothing
; outside its own install folder and claims no machine-wide state.
;
; Paths below are relative to THIS script's directory (Inno's default
; SourceDir), so the repo layout — not the CI working directory — is what they
; resolve against. SpectroScope is a single crate at the repo root, so there is
; one directory level less to climb than in a workspace layout:
;   ..\..  -> repo root
;
; Deliberately absent: a [Registry] section. SpectroScope is an instrument, not
; a document viewer — it opens no file type from Explorer, so there is no ProgID
; to register, nothing to add to "Open with", and no reason to set
; ChangesAssociations (which exists only to make the shell refresh file icons).
; Recordings and TIFF exports are files SpectroScope *writes*; the user opens
; those in ImageJ or whatever else they prefer.

#ifndef MyAppVersion
  #define MyAppVersion "0.0.0"
#endif
#define MyAppName "SpectroScope"
; Lowercase, because the crate's [[bin]] name is `spectroscope` and cargo names
; the executable after it verbatim.
#define MyAppExeName "spectroscope.exe"
#define MyAppPublisher "konung_yaropolk"
#define MyAppURL "https://github.com/konung-yaropolk/SpectroScope"

[Setup]
; AppId uniquely identifies the app for upgrades/uninstall — never change it
; once released, or upgrades install side-by-side instead of replacing.
AppId={{CE5F3905-476C-44DD-B9C9-EEB721893F3E}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
; One-liner shown in Add/Remove Programs next to the entry.
AppComments=Spectrum analyzer for multiple SDR platforms
DefaultDirName={autopf}\SpectroScope
DefaultGroupName=SpectroScope
DisableProgramGroupPage=yes
UninstallDisplayIcon={app}\{#MyAppExeName}
; 64-bit only (the app has no 32-bit build).
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Per-user, no administrator prompt. With `lowest`, {autopf} above expands to
; {localappdata}\Programs rather than C:\Program Files.
PrivilegesRequired=lowest
OutputDir=..\..
; Stable (version-less) filename so the README can link to it via GitHub's
; releases/latest/download/ redirect. The version still shows in Add/Remove
; Programs (AppVersion above).
OutputBaseFilename=SpectroScope-setup
SetupIconFile=..\..\icon\icon.ico
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
; The single file this installer carries. The exe is self-contained: the only
; renderer is wgpu, which talks to the DX12/Vulkan driver already on the
; machine, and nothing else is needed to launch. The optional SDR helper tools
; (soapy_power, hackrf_sweep, rtl_power, rtl_power_fftw, rx_power) are external
; programs the user installs separately if they want those backends — the
; built-in rtl_tcp and demo backends work without any of them, so none of this
; is a setup-time dependency.
; `DestName` is not redundant. The release job stages the build output at
; the repo root as SpectroScope.exe -- capitalised, because the asset is a
; product download -- while cargo and everything below call the binary
; spectroscope.exe. Windows finds it either way, but without DestName the
; installed file would take whatever case the staged one happened to have,
; and `MyAppExeName` would be right only by the filesystem's indulgence.
Source: "..\..\SpectroScope.exe"; DestDir: "{app}"; DestName: "{#MyAppExeName}"; Flags: ignoreversion

[Icons]
Name: "{group}\SpectroScope"; Filename: "{app}\{#MyAppExeName}"
Name: "{group}\{cm:UninstallProgram,SpectroScope}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\SpectroScope"; Filename: "{app}\{#MyAppExeName}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,SpectroScope}"; Flags: nowait postinstall skipifsilent

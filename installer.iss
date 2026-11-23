#ifndef AppVersion
  #error AppVersion is required
#endif
#ifndef AppExe
  #error AppExe is required
#endif
#ifndef OutputFolder
  #error OutputFolder is required
#endif

[Setup]
AppId={{41D603A5-E847-4039-B439-F667504F9A10}
AppName=MMO Mouse Control
AppVersion={#AppVersion}
AppPublisher=nekomimiyks
AppPublisherURL=https://github.com/nekomimiyks/mmo-mouse-control
AppSupportURL=https://github.com/nekomimiyks/mmo-mouse-control/issues
DefaultDirName={localappdata}\Programs\MMO Mouse Control
DefaultGroupName=MMO Mouse Control
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.19041
UninstallDisplayIcon={app}\MMOMouseControl.exe
SetupIconFile=source\assets\icon.ico
OutputDir={#OutputFolder}
OutputBaseFilename=MMOMouseControl-Setup-{#AppVersion}-x64
Compression=lzma2
SolidCompression=yes
CloseApplications=yes
RestartApplications=no
WizardStyle=modern

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "ja"; MessagesFile: "compiler:Languages\Japanese.isl"

[CustomMessages]
en.LegacyTitle=Keep settings from the portable version
en.LegacyDescription=Choose the previous app folder, if you used the standalone EXE.
en.LegacyHelp=The installer copies saved drafts and shortcuts from that folder when no copy is already saved. Leave the field empty for a new installation or to keep the current settings.
en.LaunchQuestion=Start MMO Mouse Control now?
en.LaunchFailed=The app could not be started. Open it from the Start menu.
en.CopyFailed=An old setting could not be copied. The original files are still in the previous app folder.
ja.LegacyTitle=ポータブル版から設定を引き継ぐ
ja.LegacyDescription=旧版のEXEを使っていた場合は、そのフォルダーを選べます。
ja.LegacyHelp=移行先に同じファイルがなければ、保存済みの下書きとショートカットをコピーします。新規インストールや現在の設定を使う場合は空欄のまま進んでください。
ja.LaunchQuestion=今すぐアプリを起動しますか？
ja.LaunchFailed=アプリを起動できませんでした。スタートメニューから開いてください。
ja.CopyFailed=一部の設定をコピーできませんでした。元のファイルは旧アプリのフォルダーにあります。

[Files]
Source: "{#AppExe}"; DestDir: "{app}"; DestName: "MMOMouseControl.exe"; Flags: ignoreversion
Source: "LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "THIRD_PARTY.md"; DestDir: "{app}"; Flags: ignoreversion

[INI]
Filename: "{app}\.mmo-installed"; Section: "Installation"; Key: "Managed"; String: "1"

[UninstallDelete]
Type: files; Name: "{app}\.mmo-installed"

[Icons]
Name: "{autoprograms}\MMO Mouse Control"; Filename: "{app}\MMOMouseControl.exe"

[Code]
var
  LegacyPage: TInputDirWizardPage;

procedure InitializeWizard;
var
  SourceDir: String;
begin
  LegacyPage := CreateInputDirPage(wpSelectDir, CustomMessage('LegacyTitle'),
    CustomMessage('LegacyDescription'), CustomMessage('LegacyHelp'), False, '');
  LegacyPage.Add('');
  SourceDir := ExpandConstant('{src}');
  if FileExists(AddBackslash(SourceDir) + 'm908.draft') or
     FileExists(AddBackslash(SourceDir) + 'm908-custom.txt') or
     FileExists(AddBackslash(SourceDir) + 'g600-custom.txt') then
    LegacyPage.Values[0] := SourceDir;
end;

function NextButtonClick(CurPageID: Integer): Boolean;
begin
  Result := True;
  if (CurPageID = LegacyPage.ID) and (Trim(LegacyPage.Values[0]) <> '') and
     not DirExists(Trim(LegacyPage.Values[0])) then begin
    MsgBox(SetupMessage(msgInvalidPath), mbError, MB_OK);
    Result := False;
  end;
end;

procedure CopySetting(const SourceDir, DataDir, FileName: String);
var
  FromFile, ToFile: String;
begin
  FromFile := AddBackslash(SourceDir) + FileName;
  ToFile := AddBackslash(DataDir) + FileName;
  if FileExists(FromFile) and not FileExists(ToFile) then
    if not ForceDirectories(DataDir) or not FileCopy(FromFile, ToFile, True) then
      SuppressibleMsgBox(CustomMessage('CopyFailed'), mbInformation, MB_OK, IDOK);
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  SourceDir, DataRoot: String;
  ResultCode: Integer;
begin
  if CurStep = ssPostInstall then begin
    SourceDir := Trim(LegacyPage.Values[0]);
    if SourceDir <> '' then begin
      DataRoot := GetEnv('LOCALAPPDATA');
      if DataRoot = '' then
        DataRoot := ExpandConstant('{localappdata}');
      CopySetting(SourceDir, AddBackslash(DataRoot) + 'MMOMouseControl', 'm908.draft');
      CopySetting(SourceDir, AddBackslash(DataRoot) + 'MMOMouseControl', 'm908-custom.txt');
      CopySetting(SourceDir, AddBackslash(DataRoot) + 'G600rControl', 'g600-custom.txt');
    end;
  end;
  if (CurStep = ssDone) and not WizardSilent then begin
    if MsgBox(CustomMessage('LaunchQuestion'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
      if not Exec(ExpandConstant('{app}\MMOMouseControl.exe'), '',
                  ExpandConstant('{app}'), SW_SHOWNORMAL, ewNoWait, ResultCode) then
        MsgBox(CustomMessage('LaunchFailed'), mbError, MB_OK);
  end;
end;

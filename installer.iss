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
en.LegacyHelp=The installer copies saved drafts and shortcuts if they are found in the selected folder and no copy is already saved. For a new installation, keep the suggested folder.
en.LaunchQuestion=Start MMO Mouse Control now?
en.LaunchFailed=The app could not be started. Open it from the Start menu.
en.CopyFailed=An old setting could not be copied. The original files are still in the previous app folder.
en.CloseFailed=MMO Mouse Control could not finish closing. Save your changes or respond to any prompt in the app, then close it and try again.
ja.LegacyTitle=ポータブル版から設定を引き継ぐ
ja.LegacyDescription=旧版のEXEを使っていた場合は、そのフォルダーを選べます。
ja.LegacyHelp=選んだフォルダーに保存済みの下書きやショートカットがあり、移行先に同じファイルがなければコピーします。新規インストールの場合は初期値のまま進んでください。
ja.LaunchQuestion=今すぐアプリを起動しますか？
ja.LaunchFailed=アプリを起動できませんでした。スタートメニューから開いてください。
ja.CopyFailed=一部の設定をコピーできませんでした。元のファイルは旧アプリのフォルダーにあります。
ja.CloseFailed=MMO Mouse Controlを終了できませんでした。アプリの確認画面に応答するか、設定を保存して終了してから、もう一度更新してください。

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
Name: "{autodesktop}\MMO Mouse Control"; Filename: "{app}\MMOMouseControl.exe"

[Code]
const
  ProcessQueryLimitedInformation = $1000;
  ProcessSynchronize = $100000;
  CloseWindowMessage = $0010;
  WaitSignaled = 0;
  CloseTimeoutMs = 20000;

var
  LegacyPage: TInputDirWizardPage;

function GetWindowThreadProcessId(Window: HWND; var ProcessId: Cardinal): Cardinal;
  external 'GetWindowThreadProcessId@user32.dll stdcall';
function OpenProcess(Access: Cardinal; InheritHandle: Integer; ProcessId: Cardinal): THandle;
  external 'OpenProcess@kernel32.dll stdcall';
function QueryFullProcessImageName(Process: THandle; Flags: Cardinal; ImageName: String; var Size: Cardinal): Integer;
  external 'QueryFullProcessImageNameW@kernel32.dll stdcall';
function PostWindowMessage(Window: HWND; Message: Cardinal; WParam, LParam: NativeUInt): Integer;
  external 'PostMessageW@user32.dll stdcall';
function WaitForSingleObject(Handle: THandle; Milliseconds: Cardinal): Cardinal;
  external 'WaitForSingleObject@kernel32.dll stdcall';
function CloseHandle(Handle: THandle): Integer;
  external 'CloseHandle@kernel32.dll stdcall';

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  TargetPath, ImageName: String;
  Window: HWND;
  ProcessId, CurrentProcessId, Size: Cardinal;
  Process: THandle;
begin
  Result := '';
  TargetPath := ExpandConstant('{app}\MMOMouseControl.exe');
  if not FileExists(TargetPath) then
    Exit;
  Window := FindWindowByWindowName('MMO Mouse Control');
  if Window = 0 then
    Exit;
  if GetWindowThreadProcessId(Window, ProcessId) = 0 then
    Exit;
  Process := OpenProcess(ProcessQueryLimitedInformation or ProcessSynchronize, 0, ProcessId);
  if Process = 0 then
    Exit; { Restart Manager handles any file that is still in use. }
  try
    Size := 32768;
    SetLength(ImageName, Size);
    if QueryFullProcessImageName(Process, 0, ImageName, Size) = 0 then
      Exit;
    SetLength(ImageName, Size);
    if CompareText(ImageName, TargetPath) <> 0 then
      Exit; { Leave the Store and portable copies running. }
    if WaitForSingleObject(Process, 0) = WaitSignaled then
      Exit;
    if (GetWindowThreadProcessId(Window, CurrentProcessId) <> 0) and
       (CurrentProcessId = ProcessId) and
       (PostWindowMessage(Window, CloseWindowMessage, 0, 0) <> 0) then begin
      Log('Requested a normal shutdown of the installed MMO Mouse Control.');
      if WaitForSingleObject(Process, CloseTimeoutMs) = WaitSignaled then begin
        Log('The installed MMO Mouse Control exited before the update.');
        Exit;
      end;
    end;
    Result := CustomMessage('CloseFailed');
  finally
    CloseHandle(Process);
  end;
end;

procedure InitializeWizard;
var
  PreviousFolder: String;
begin
  LegacyPage := CreateInputDirPage(wpSelectDir, CustomMessage('LegacyTitle'),
    CustomMessage('LegacyDescription'), CustomMessage('LegacyHelp'), False, '');
  LegacyPage.Add('');
  PreviousFolder := ExpandConstant('{param:LEGACYDIR|}');
  if not DirExists(PreviousFolder) then
    PreviousFolder := ExpandConstant('{src}');
  LegacyPage.Values[0] := PreviousFolder;
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

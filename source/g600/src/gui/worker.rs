use std::{ffi::CString, path::PathBuf, sync::mpsc::{self, Receiver, Sender}};
use eframe::egui;
use g600ctl::{backup, device, operations, protocol::{Active, Profile, PRODUCT_G600}};
use super::inputlog::{self, InputDevice};

pub struct DisplaySettings {
    pub active: Active,
    pub profiles: Vec<Profile>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceIdentity {
    pub path: CString,
    pub serial: Option<String>,
}

impl DeviceIdentity {
    pub fn same_device(&self, other: &Self) -> bool {
        match (&self.serial, &other.serial) {
            (Some(a), Some(b)) => a == b,
            (None, None) => self.path == other.path,
            _ => false,
        }
    }
}

pub struct DeviceSettings {
    pub identity: DeviceIdentity,
    pub settings: DisplaySettings,
    pub(super) inputs: Vec<InputDevice>,
    pub(super) continued_handle: bool,
}

// 取得済みのハンドルは抜き差しで無効になる。同じHIDパスを再オープンすると
// 別個体を指しうるため、読み込みから書き込みまで同じハンドルを使う。
struct BoundDevice {
    identity: DeviceIdentity,
    handle: hidapi::HidDevice,
}

fn bound_handle(bound: Option<&BoundDevice>) -> Result<&hidapi::HidDevice, String> {
    bound.map(|device| &device.handle).ok_or_else(|| "G600rの実機設定を先に読み込んでください。".into())
}

pub enum Command {
    Reload,
    Rebind,
    Connect(Option<DeviceIdentity>),
    RefreshInputs(u64),
    Save(PathBuf),
    Open(PathBuf),
    Apply { profile: u8, data: Vec<u8> },
    Use { profile: u8, backup: bool },
    RestoreSession(PathBuf),
    RestoreBackup(DisplaySettings),
    /// 即時反映: backup が true なら書き込み前に自動バックアップを取る（起動後の最初の 1 回）
    AutoApply { profile: u8, expected: Vec<u8>, data: Vec<u8>, backup: bool },
}

pub enum WriteKind {
    BackupProfile(usize),
    BackupFile,
    SessionRestore,
}

pub enum Response {
    Reload(Result<DeviceSettings, String>),
    Connected(Result<DeviceSettings, String>),
    Inputs(u64, Vec<InputDevice>),
    Save(Result<PathBuf, String>),
    Open(Result<(PathBuf, Option<DisplaySettings>, bool), String>),
    Write(WriteKind, Result<(String, DisplaySettings), (String, Option<String>)>),
    Use(Result<(Option<String>, DisplaySettings), (String, Option<String>)>),
    /// 即時反映の結果: (プロファイル, 書いた内容, 結果)。成功時は (取ったバックアップのパス, 読み直した設定)
    AutoApply(u8, Vec<u8>, Result<(Option<String>, DisplaySettings), (String, Option<String>)>),
}

pub fn start(ctx: egui::Context) -> (Sender<Command>, Receiver<Response>) {
    let (commands, incoming) = mpsc::channel();
    let (outgoing, responses) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bound = None;
        while let Ok(command) = incoming.recv() {
            let response = match command {
                Command::Apply { profile, data } => Response::Write(WriteKind::BackupProfile(profile as usize), (|| {
                    let dev = bound_handle(bound.as_ref()).map_err(|e| (e, None))?;
                    let dir = operations::backup_dir().map_err(|e| (e, None))?;
                    let edits = [(profile, data)];
                    let path = operations::write_profiles(dev, &edits, &dir, |_, _| {})
                        .map_err(|e| (e.message, e.backup.map(|p| p.display().to_string())))?.display().to_string();
                    let settings = device::read_settings(dev).map_err(|e| (format!("書き込み後の再読み込みに失敗: {e}"), Some(path.clone())))?;
                    operations::verify_readback(&settings, edits.iter().map(|(slot, data)| (*slot, data.as_slice())), None)
                        .map_err(|e| (e, Some(path.clone())))?;
                    Ok((path, DisplaySettings { active: settings.active, profiles: settings.profiles }))
                })()),
                Command::AutoApply { profile, expected, data, backup } => {
                    let result = (|| {
                        let dev = bound_handle(bound.as_ref()).map_err(|e| (e, None))?;
                        let dir = backup.then(operations::backup_dir).transpose().map_err(|e| (e, None))?;
                        let saved = operations::write_automatic(dev, profile, &expected, &data, dir.as_deref())
                            .map_err(|e| (e.message, e.backup.map(|p| p.display().to_string())))?
                            .map(|p| p.display().to_string());
                        let settings = device::read_settings(dev).map_err(|e| (format!("書き込み後の再読み込みに失敗: {e}"), saved.clone()))?;
                        operations::verify_readback(&settings, [(profile, data.as_slice())], None)
                            .map_err(|e| (e, saved.clone()))?;
                        Ok((saved, DisplaySettings { active: settings.active, profiles: settings.profiles }))
                    })();
                    Response::AutoApply(profile, data, result)
                }
                Command::Use { profile, backup } => Response::Use((|| {
                    let dev = bound_handle(bound.as_ref()).map_err(|e| (e, None))?;
                    let mut settings = device::read_settings(dev).map_err(|e| (e, None))?;
                    let mut saved = None;
                    if settings.active.profile != profile {
                        if backup {
                            let dir = operations::backup_dir().map_err(|e| (e, None))?;
                            let path = operations::save_before_write(&settings, &dir).map_err(|e| (e.message, None))?;
                            saved = Some(path.display().to_string());
                        }
                        device::select_profile(dev, profile).map_err(|e| (e, saved.clone()))?;
                        settings = device::read_settings(dev).map_err(|e| (e, saved.clone()))?;
                    }
                    operations::verify_readback(&settings, [], Some(profile)).map_err(|e| (e, saved.clone()))?;
                    Ok((saved, DisplaySettings { active: settings.active, profiles: settings.profiles }))
                })()),
                Command::RestoreSession(_) | Command::RestoreBackup(_) => Response::Write(
                    if matches!(&command, Command::RestoreSession(_)) { WriteKind::SessionRestore }
                    else { WriteKind::BackupFile }, (|| {
                    let (active, profiles) = match command {
                        Command::RestoreSession(path) => backup::read_backup_profiles(&path).map_err(|e| (e, None))?,
                        Command::RestoreBackup(settings) => (settings.active, settings.profiles),
                        _ => unreachable!(),
                    };
                    let selected: Vec<_> = profiles.into_iter().enumerate()
                        .map(|(i, p)| (i as u8, p.raw)).collect();
                    let dev = bound_handle(bound.as_ref()).map_err(|e| (e, None))?;
                    let dir = operations::backup_dir().map_err(|e| (e, None))?;
                    let saved = operations::write_profiles(dev, &selected, &dir, |_, _| {})
                        .map_err(|e| (e.message, e.backup.map(|p| p.display().to_string())))?.display().to_string();
                    device::select_profile(dev, active.profile).map_err(|e| (e, Some(saved.clone())))?;
                    let settings = device::read_settings(dev).map_err(|e| (format!("復元後の再読み込みに失敗: {e}"), Some(saved.clone())))?;
                    operations::verify_readback(
                        &settings, selected.iter().map(|(slot, data)| (*slot, data.as_slice())),
                        Some(active.profile),
                    ).map_err(|e| (e, Some(saved.clone())))?;
                    Ok((saved, DisplaySettings { active: settings.active, profiles: settings.profiles }))
                })()),
                Command::Reload | Command::Rebind | Command::Connect(_) => {
                    let reconnect = matches!(&command, Command::Connect(_));
                    let new_handle = matches!(&command, Command::Rebind);
                    let preferred = match command {
                        Command::Connect(preferred) => preferred,
                        _ => None,
                    };
                    let result = read_device(&mut bound, preferred.as_ref(), new_handle);
                    if reconnect {
                        Response::Connected(result)
                    } else {
                        Response::Reload(result)
                    }
                }
                Command::RefreshInputs(generation) => {
                    let devices = bound.as_ref().and_then(|device| {
                        let inputs = inputlog::open_inputs(&device.identity);
                        device::get_report(&device.handle, g600ctl::protocol::REPORT_ACTIVE, g600ctl::protocol::ACTIVE_LEN)
                            .ok().map(|_| inputs)
                    }).unwrap_or_default();
                    Response::Inputs(generation, devices)
                }
                Command::Save(path) => Response::Save((|| {
                    let settings = device::read_settings(bound_handle(bound.as_ref())?)?;
                    g600ctl::mouse_backup::Mouse::G600.save(&path, &backup::encode_settings(&settings), None)?;
                    Ok(path)
                })()),
                Command::Open(path) => Response::Open((|| {
                    let archive = g600ctl::mouse_backup::Mouse::G600.load(&path)?;
                    let settings = archive.payload.as_deref().map(|text| {
                        let (active, profiles) = backup::parse_backup_profiles(text)?;
                        Ok::<_, String>(DisplaySettings { active, profiles })
                    }).transpose()?;
                    let staged = archive.stage_settings()?;
                    Ok((path, settings, staged))
                })()),
            };
            if outgoing.send(response).is_err() { break; }
            ctx.request_repaint();
        }
    });
    (commands, responses)
}

fn read_device(bound: &mut Option<BoundDevice>, preferred: Option<&DeviceIdentity>, new_handle: bool) -> Result<DeviceSettings, String> {
    let mut discovery = None;
    if let Some(previous) = bound.as_ref().filter(|old| !new_handle && preferred.is_none_or(|id| old.identity.same_device(id))) {
        match read_bound_settings(previous, true) {
            Ok(settings) => return Ok(settings),
            Err(error) if preferred.is_some_and(|id| id.serial.is_none()) => {
                return Err(format!("編集中のG600rを読み込めません。接続中なら再接続を試してください。切断後はシリアル番号がないため個体を確認できません。{error}"));
            }
            Err(error) if preferred.is_none() => {
                let api = device::g600_interfaces()?;
                if api.device_list().any(|info| info.path() == previous.identity.path.as_c_str()) {
                    return Err(format!("読み込んだG600rと通信できません。再読み込みを試してください。別個体なら接続中の実機を読み込んでください。{error}"));
                }
                discovery = Some(api);
            }
            Err(_) => {}
        }
    }
    let device = if let Some(identity) = preferred {
        let serial = identity.serial.as_deref().ok_or(
            "シリアル番号がないため、切断後の個体を確認できません。編集を破棄して実機設定を再読み込みしてください。",
        )?;
        find_by_serial(serial).ok_or("編集中のG600rに再接続できません。元のマウスを接続してください。")?
    } else {
        let api = match discovery { Some(api) => api, None => device::g600_interfaces()? };
        let (handle, path) = device::open_g600_with_path(&api, PRODUCT_G600)?;
        let serial = handle.get_serial_number_string().map_err(|e| e.to_string())?.filter(|serial| !serial.is_empty());
        BoundDevice { handle, identity: DeviceIdentity { path, serial } }
    };
    let settings = read_bound_settings(&device, false)?;
    *bound = Some(device);
    Ok(settings)
}

fn read_bound_settings(device: &BoundDevice, continued_handle: bool) -> Result<DeviceSettings, String> {
    // 入力ハンドルを開いた後で設定ハンドルの生存を確認する。途中で同じパスに
    // 別個体が現れても、失効した設定ハンドルの読み込みが失敗して両方破棄される。
    let inputs = inputlog::open_inputs(&device.identity);
    let settings = device::read_settings(&device.handle)?;
    Ok(DeviceSettings {
        identity: device.identity.clone(),
        settings: DisplaySettings { active: settings.active, profiles: settings.profiles },
        inputs,
        continued_handle,
    })
}

fn find_by_serial(serial: &str) -> Option<BoundDevice> {
    let api = device::g600_interfaces().ok()?;
    let mut matched = None;
    for info in api.device_list().filter(|info| info.serial_number() == Some(serial)) {
        let Ok(handle) = info.open_device(&api) else { continue };
        if handle.get_serial_number_string().ok().flatten().as_deref() == Some(serial)
            && device::get_report(&handle, g600ctl::protocol::REPORT_ACTIVE, g600ctl::protocol::ACTIVE_LEN).is_ok()
        {
            if matched.is_some() { return None; }
            let identity = DeviceIdentity { path: info.path().to_owned(), serial: Some(serial.into()) };
            matched = Some(BoundDevice { handle, identity });
        }
    }
    matched
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_without_a_serial_or_live_handle_does_not_open_the_saved_path() {
        let identity = DeviceIdentity { path: CString::new("reusable path").unwrap(), serial: None };
        let mut bound = None;
        let result = read_device(&mut bound, Some(&identity), false);
        assert!(matches!(result, Err(error) if error.contains("シリアル番号がない")));
        assert!(bound.is_none());
    }
}

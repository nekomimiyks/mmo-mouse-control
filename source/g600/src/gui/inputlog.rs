//! G600 のベンダー定義コレクション（0xFF80）の入力レポートを読み続ける（読むだけ）。
//! - 押されているボタンのビットを共有し、GUI の「番号・名称」を光らせる（buttons.rs）
//! - 診断用に、最初の MAX_LINES 行を input-log.txt に追記する

use std::ffi::CStr;
use g600ctl::protocol::{hex, PRODUCT_G600, VENDOR_LOGITECH};
use std::io::Write;
use super::buttons::{mask_from_report, PressState};
use super::worker::{Command, DeviceIdentity};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use eframe::egui;

pub const MAX_LINES: usize = 500;

/// ログに書く 1 行（純粋関数）
pub fn format_line(elapsed_ms: u128, usage_page: u16, usage: u16, data: &[u8]) -> String {
    format!("{:>8} ms  UP={usage_page:04X} U={usage:04X}  {}\n", elapsed_ms, hex(data))
}

/// G600の物理ボタンの入力が届くコレクション
pub fn is_target(usage_page: u16) -> bool {
    usage_page == 0xFF80
}

#[derive(Default)]
pub struct Monitor {
    shared: Arc<Mutex<InputState>>,
    commands: Option<mpsc::Sender<Option<Target>>>,
}

#[derive(Default)]
struct InputState {
    generation: u64,
    presses: PressState,
}

impl InputState {
    fn report(&mut self, generation: u64, mask: u32, now: Instant) -> bool {
        self.generation == generation && self.presses.report(mask, now)
    }
}

struct Target {
    generation: u64,
    devices: Vec<InputDevice>,
}

pub(super) struct InputDevice(u16, u16, hidapi::HidDevice);

impl Monitor {
    pub fn start(ctx: egui::Context, worker: mpsc::Sender<Command>) -> Self {
        let (commands, incoming) = mpsc::channel::<Option<Target>>();
        let shared = Arc::new(Mutex::new(InputState::default()));
        let state = shared.clone();
        std::thread::spawn(move || {
            let mut next = incoming.recv().ok();
            while let Some(target) = next {
                let Some(target) = target else { next = incoming.recv().ok(); continue };
                if !target.devices.is_empty() { let _ = run(&state, &ctx, &target); }
                {
                    let mut state = state.lock().unwrap();
                    if state.generation != target.generation {
                        drop(state);
                        next = incoming.recv().ok();
                        continue;
                    }
                    state.presses = PressState::default();
                }
                ctx.request_repaint();
                next = match incoming.recv_timeout(Duration::from_secs(3)) {
                    Ok(target) => Some(target),
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if worker.send(Command::RefreshInputs(target.generation)).is_err() { break; }
                        incoming.recv().ok()
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
            }
        });
        Self { shared, commands: Some(commands) }
    }

    pub(super) fn bind(&self, devices: Option<Vec<InputDevice>>) {
        let generation = {
            let mut state = self.shared.lock().unwrap();
            state.generation += 1;
            state.presses = PressState::default();
            state.generation
        };
        if let Some(commands) = &self.commands {
            let _ = commands.send(devices.map(|devices| Target { generation, devices }));
        }
    }

    pub(super) fn refresh(&self, generation: u64, devices: Vec<InputDevice>) {
        if self.shared.lock().unwrap().generation == generation {
            if let Some(commands) = &self.commands {
                let _ = commands.send(Some(Target { generation, devices }));
            }
        }
    }

    pub fn display(&self, now: Instant) -> (u32, u32, bool) {
        self.shared.lock().unwrap().presses.display(now)
    }

    #[cfg(test)]
    pub fn set_test_press(&self, mask: u32, now: Instant) {
        let mut state = self.shared.lock().unwrap();
        state.presses = PressState::default();
        state.presses.report(mask, now);
    }
}

impl Drop for Monitor {
    fn drop(&mut self) { self.bind(None); }
}

pub(super) fn open_inputs(identity: &DeviceIdentity) -> Vec<InputDevice> {
    let Ok(api) = g600ctl::device::g600_interfaces() else { return Vec::new() };
    let container = container_id(&identity.path);
    api.device_list()
        .filter(|info| info.vendor_id() == VENDOR_LOGITECH && info.product_id() == PRODUCT_G600
            && is_target(info.usage_page()) && matches_target(identity, container, info))
        .filter_map(|info| info.open_device(&api).ok().map(|dev| InputDevice(info.usage_page(), info.usage(), dev)))
        .collect()
}

fn run(state: &Mutex<InputState>, ctx: &egui::Context, target: &Target) -> Result<(), String> {
    let current = || state.lock().unwrap().generation == target.generation;
    if !current() { return Ok(()); }
    let mut file = g600ctl::storage::file("input-log.txt").ok().and_then(|path|
        std::fs::OpenOptions::new().create(true).append(true).open(path).ok()
    );
    let mut log = |s: &str| { if let Some(f) = file.as_mut() { let _ = f.write_all(s.as_bytes()); let _ = f.flush(); } };
    let mut header = format!("==== 接続 {} ====\n", g600ctl::backup::timestamp());
    for InputDevice(page, usage, _) in &target.devices {
        header += &format!("開けた: UP={page:04X} U={usage:04X}\n");
    }
    log(&header);
    let start = std::time::Instant::now();
    let mut lines = 0;
    let mut buf = [0u8; 256];
    while current() {
        let mut got = false;
        for InputDevice(page, usage, dev) in &target.devices {
            match dev.read_timeout(&mut buf, 20) {
                Ok(n) if n > 0 => {
                    got = true;
                    if let Some(mask) = mask_from_report(&buf[..n]) {
                        if state.lock().unwrap().report(target.generation, mask, Instant::now()) {
                            ctx.request_repaint();
                        }
                    }
                    if lines < MAX_LINES {
                        log(&format_line(start.elapsed().as_millis(), *page, *usage, &buf[..n]));
                        lines += 1;
                        if lines == MAX_LINES { log(&format!("（{MAX_LINES} 行に達したので記録を止めました。ボタンの読み取りは続けます）\n")); }
                    }
                }
                Ok(_) => {}
                Err(e) => { log(&format!("読み取りエラー UP={page:04X}: {e}\n")); return Err(e.to_string()); }
            }
        }
        if !got { std::thread::sleep(Duration::from_millis(2)); }
    }
    Ok(())
}

fn matches_target(identity: &DeviceIdentity, container: Option<[u8; 16]>, info: &hidapi::DeviceInfo) -> bool {
    if info.path() == identity.path.as_c_str() { return true; }
    if cfg!(windows) {
        return container.is_some_and(|id| container_id(info.path()) == Some(id));
    }
    identity.serial.as_deref().is_some_and(|serial| info.serial_number() == Some(serial))
}

#[cfg(windows)]
fn container_id(path: &CStr) -> Option<[u8; 16]> {
    use windows_sys::Win32::Devices::{
        DeviceAndDriverInstallation::{CM_Get_DevNode_PropertyW, CM_Get_Device_Interface_PropertyW, CM_Locate_DevNodeW, CR_SUCCESS},
        Properties::{DEVPKEY_Device_ContainerId, DEVPKEY_Device_InstanceId, DEVPROP_TYPE_GUID, DEVPROP_TYPE_STRING},
    };

    let path = super::single::wide(path.to_str().ok()?);
    let mut instance = [0u16; 200]; // MAX_DEVICE_ID_LEN
    let mut bytes = std::mem::size_of_val(&instance) as u32;
    let mut kind = 0;
    let mut node = 0;
    let mut id = [0; 16];
    // 入出力ポインターはそれぞれ呼び出し中に有効な固定長のバッファを指す。
    unsafe {
        if CM_Get_Device_Interface_PropertyW(path.as_ptr(), &DEVPKEY_Device_InstanceId, &mut kind,
            instance.as_mut_ptr().cast(), &mut bytes, 0) != CR_SUCCESS || kind != DEVPROP_TYPE_STRING
            || CM_Locate_DevNodeW(&mut node, instance.as_ptr(), 0) != CR_SUCCESS
        {
            return None;
        }
        bytes = id.len() as u32;
        if CM_Get_DevNode_PropertyW(node, &DEVPKEY_Device_ContainerId, &mut kind, id.as_mut_ptr(),
            &mut bytes, 0) != CR_SUCCESS || kind != DEVPROP_TYPE_GUID || bytes != id.len() as u32
        {
            return None;
        }
    }
    (id != [0; 16]).then_some(id)
}

#[cfg(not(windows))]
fn container_id(_: &CStr) -> Option<[u8; 16]> { None }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formats_and_filters() {
        assert_eq!(format_line(12, 0xFF80, 0x80, &[0x80, 1, 2]), "      12 ms  UP=FF80 U=0080  80 01 02\n");
        assert!(is_target(0xFF80));
        assert!(!is_target(0x0001));
        assert!(!is_target(0xFFA0));
    }

    #[test]
    fn reports_from_the_previous_binding_are_ignored() {
        let monitor = Monitor::default();
        let now = Instant::now();
        assert!(monitor.shared.lock().unwrap().report(0, 1, now));
        monitor.bind(None);
        assert!(!monitor.shared.lock().unwrap().report(0, 1, now));
        assert_eq!(monitor.display(now).0, 0);
        assert!(monitor.shared.lock().unwrap().report(1, 2, now));
        assert_eq!(monitor.display(now).0, 2);
    }
}

//! M908のRaw Inputだけを受ける。別キーボードのキー内容は読み取らず、入力を抑止しない。
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
#[derive(Clone, Debug)]
pub enum Event {
    Mouse { flags: u16, wheel: i16, raw: u32 },
    Key { scan: u16, flags: u16, vkey: u16 },
    Hid(Vec<u8>),
    Reset,
    Blur,
}
pub struct Monitor {
    pub events: Receiver<Event>,
    hwnd: isize,
}
impl Drop for Monitor {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(
                self.hwnd as _,
                windows_sys::Win32::UI::WindowsAndMessaging::WM_CLOSE,
                0,
                0,
            );
        }
    }
}
static SENDER: OnceLock<Mutex<Option<Sender<Event>>>> = OnceLock::new();
fn emit(event: Event) {
    if let Ok(sender) = SENDER.get_or_init(|| Mutex::new(None)).lock() {
        if let Some(tx) = sender.as_ref() {
            let _ = tx.send(event);
        }
    }
}
#[cfg(windows)]
mod native {
    use super::*;
    use std::{cell::RefCell, collections::HashMap};
    use windows_sys::Win32::{
        Foundation::*,
        System::LibraryLoader::GetModuleHandleW,
        UI::{Accessibility::*, Input::*, WindowsAndMessaging::*},
    };
    thread_local! {static SOURCES:RefCell<HashMap<usize,bool>>=RefCell::new(HashMap::new());static LAST_RAW:RefCell<HashMap<usize,u32>>=RefCell::new(HashMap::new());}
    thread_local! {static OWNER:std::cell::Cell<u32>=const {std::cell::Cell::new(0)};}
    unsafe extern "system" fn foreground_changed(
        _: HWINEVENTHOOK,
        _: u32,
        hwnd: HWND,
        _: i32,
        _: i32,
        _: u32,
        _: u32,
    ) {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        if OWNER.with(|owner| owner.get() != 0 && owner.get() != pid) {
            emit(Event::Blur);
        }
    }
    unsafe fn is_target(handle: HANDLE) -> bool {
        SOURCES.with(|cache| {
            if let Some(value) = cache.borrow().get(&(handle as usize)) {
                return *value;
            }
            let mut size = 0;
            GetRawInputDeviceInfoW(handle, RIDI_DEVICENAME, std::ptr::null_mut(), &mut size);
            if size == 0 || size > 4096 {
                return false;
            }
            let mut name = vec![0u16; size as usize + 1];
            if GetRawInputDeviceInfoW(handle, RIDI_DEVICENAME, name.as_mut_ptr().cast(), &mut size)
                == u32::MAX
            {
                return false;
            }
            let text = String::from_utf16_lossy(&name[..size as usize]).to_ascii_uppercase();
            let target = text.contains("VID_04D9&PID_FC4D");
            cache.borrow_mut().insert(handle as usize, target);
            target
        })
    }
    unsafe fn receive(handle: HRAWINPUT) {
        let header_size = std::mem::size_of::<RAWINPUTHEADER>() as u32;
        let mut header: RAWINPUTHEADER = std::mem::zeroed();
        let mut size = header_size;
        if GetRawInputData(
            handle,
            RID_HEADER,
            (&mut header as *mut RAWINPUTHEADER).cast(),
            &mut size,
            header_size,
        ) == u32::MAX
            || !is_target(header.hDevice)
        {
            return;
        }
        if header.dwSize < header_size || header.dwSize > 65536 {
            return;
        }
        let mut data = vec![
            0u64;
            (header.dwSize as usize)
                .max(std::mem::size_of::<RAWINPUT>())
                .div_ceil(8)
        ];
        size = header.dwSize;
        if GetRawInputData(
            handle,
            RID_INPUT,
            data.as_mut_ptr().cast(),
            &mut size,
            header_size,
        ) == u32::MAX
        {
            return;
        }
        let raw = &*(data.as_ptr() as *const RAWINPUT);
        match header.dwType {
            RIM_TYPEMOUSE => {
                let m = raw.data.mouse;
                let buttons = m.Anonymous.Anonymous;
                let changed = LAST_RAW.with(|last| {
                    let mut last = last.borrow_mut();
                    let old = last
                        .insert(header.hDevice as usize, m.ulRawButtons)
                        .unwrap_or(0);
                    old != m.ulRawButtons
                });
                if buttons.usButtonFlags != 0 || changed {
                    emit(Event::Mouse {
                        flags: buttons.usButtonFlags,
                        wheel: buttons.usButtonData as i16,
                        raw: m.ulRawButtons,
                    });
                }
            }
            RIM_TYPEKEYBOARD => {
                let k = raw.data.keyboard;
                emit(Event::Key {
                    scan: k.MakeCode,
                    flags: k.Flags,
                    vkey: k.VKey,
                });
            }
            RIM_TYPEHID => {
                let hid = &raw.data.hid;
                let bytes = (hid.dwSizeHid as usize).checked_mul(hid.dwCount as usize);
                let offset = std::ptr::addr_of!(hid.bRawData) as usize - data.as_ptr() as usize;
                if let Some(bytes) = bytes {
                    if offset + bytes <= size as usize && hid.dwSizeHid > 0 {
                        let reports = std::slice::from_raw_parts(
                            std::ptr::addr_of!(hid.bRawData).cast::<u8>(),
                            bytes,
                        );
                        for report in reports.chunks(hid.dwSizeHid as usize) {
                            emit(Event::Hid(report.to_vec()));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    unsafe extern "system" fn procedure(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        match msg {
            WM_INPUT => receive(l as HRAWINPUT),
            WM_INPUT_DEVICE_CHANGE => {
                if w == GIDC_REMOVAL as usize {
                    let target =
                        SOURCES.with(|s| s.borrow_mut().remove(&(l as usize)).unwrap_or(false));
                    if target {
                        emit(Event::Reset);
                    }
                }
            }
            WM_CLOSE => {
                DestroyWindow(hwnd);
                return 0;
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                return 0;
            }
            _ => {}
        }
        DefWindowProcW(hwnd, msg, w, l)
    }
    pub unsafe fn run(ready: Sender<Result<isize, String>>, owner: u32) {
        let class: Vec<u16> = "M908InputObserver".encode_utf16().chain(Some(0)).collect();
        let instance = GetModuleHandleW(std::ptr::null());
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.hInstance = instance;
        wc.lpfnWndProc = Some(procedure);
        wc.lpszClassName = class.as_ptr();
        if RegisterClassW(&wc) == 0 && GetLastError() != 1410 {
            let _ = ready.send(Err(std::io::Error::last_os_error().to_string()));
            return;
        }
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            let _ = ready.send(Err(std::io::Error::last_os_error().to_string()));
            return;
        }
        let registrations =
            [(1, 2), (1, 6), (0x0c, 1), (0xffa0, 1)].map(|(page, usage)| RAWINPUTDEVICE {
                usUsagePage: page,
                usUsage: usage,
                dwFlags: RIDEV_INPUTSINK | RIDEV_DEVNOTIFY,
                hwndTarget: hwnd,
            });
        if RegisterRawInputDevices(
            registrations.as_ptr(),
            registrations.len() as u32,
            std::mem::size_of::<RAWINPUTDEVICE>() as u32,
        ) == 0
        {
            let _ = ready.send(Err(std::io::Error::last_os_error().to_string()));
            DestroyWindow(hwnd);
            return;
        }
        OWNER.with(|pid| pid.set(owner));
        let hook = if owner != 0 {
            SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                std::ptr::null_mut(),
                Some(foreground_changed),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            )
        } else {
            std::ptr::null_mut()
        };
        let _ = ready.send(Ok(hwnd as isize));
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if !hook.is_null() {
            UnhookWinEvent(hook);
        }
        if let Ok(mut sender) = SENDER.get_or_init(|| Mutex::new(None)).lock() {
            *sender = None;
        }
    }
}
pub fn start() -> Result<Monitor, String> {
    start_with_owner(0)
}
pub(crate) fn start_with_owner(owner: u32) -> Result<Monitor, String> {
    let (tx, events) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::channel();
    {
        let mut slot = SENDER
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|e| e.to_string())?;
        if slot.is_some() {
            return Err("押下監視はすでに起動しています".into());
        }
        *slot = Some(tx);
    }
    #[cfg(windows)]
    std::thread::spawn(move || unsafe { native::run(ready_tx, owner) });
    #[cfg(not(windows))]
    let _ = ready_tx.send(Err("Windows専用です".into()));
    let hwnd = ready_rx
        .recv_timeout(Duration::from_secs(3))
        .map_err(|e| e.to_string())??;
    Ok(Monitor { events, hwnd })
}
pub fn watch(seconds: u64, mut print: impl FnMut(&str)) -> Result<(), String> {
    let monitor = start()?;
    let begin = Instant::now();
    print("M908のRaw Input監視を開始しました（他のキーボードの入力は記録しません）。");
    let mut count = 0;
    while begin.elapsed() < Duration::from_secs(seconds.min(300)) {
        if let Ok(event) = monitor.events.recv_timeout(Duration::from_millis(50)) {
            count += 1;
            print(&format!("{:>6} ms: {event:?}", begin.elapsed().as_millis()));
        }
    }
    print(&format!("監視終了: {count}件"));
    Ok(())
}

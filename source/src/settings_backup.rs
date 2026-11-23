pub fn apply_pending() -> Result<(), String> {
    g600ctl::mouse_backup::apply_pending(&crate::ui::settings_directory())
}

#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> { text.encode_utf16().chain(std::iter::once(0)).collect() }

pub fn show_startup_error(error: &str) {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        MessageBoxW(std::ptr::null_mut(), wide(error).as_ptr(),
            wide("MMO Mouse Control — Settings restore").as_ptr(), MB_OK | MB_ICONERROR);
    }
    #[cfg(not(windows))]
    eprintln!("{error}");
}

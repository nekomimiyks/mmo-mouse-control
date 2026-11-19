//! 多重起動の防止（Windows のみ）。
//! 名前付きミューテックスで2つ目の起動を検出し、既存のウィンドウを前面に出して終了する。

pub const MUTEX_NAME: &str = "Local\\MmoMouseControl";
pub const WINDOW_TITLE: &str = "MMO Mouse Control";

/// null 終端の UTF-16 に変換する
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// プロセスが終わるまで保持するミューテックスのハンドル
pub struct InstanceGuard(#[allow(dead_code)] isize);

#[cfg(target_os = "windows")]
mod sys {
    use std::ffi::c_void;
    pub const ERROR_ALREADY_EXISTS: u32 = 183;
    pub const SW_RESTORE: i32 = 9;
    #[link(name = "kernel32")]
    extern "system" {
        pub fn CreateMutexW(attrs: *mut c_void, initial_owner: i32, name: *const u16) -> isize;
        pub fn GetLastError() -> u32;
    }
    #[link(name = "user32")]
    extern "system" {
        pub fn FindWindowW(class: *const u16, title: *const u16) -> isize;
        pub fn IsIconic(hwnd: isize) -> i32;
        pub fn ShowWindow(hwnd: isize, cmd: i32) -> i32;
        pub fn SetForegroundWindow(hwnd: isize) -> i32;
    }
}

/// 最初の起動なら Some(ガード) を返す。すでに起動していれば既存のウィンドウを前面に出して None を返す。
#[cfg(target_os = "windows")]
pub fn acquire() -> Option<InstanceGuard> {
    let name = wide(MUTEX_NAME);
    let handle = unsafe { sys::CreateMutexW(std::ptr::null_mut(), 0, name.as_ptr()) };
    let already = unsafe { sys::GetLastError() } == sys::ERROR_ALREADY_EXISTS;
    if handle == 0 {
        // ミューテックスを作れない場合は多重起動の判定をせずに起動する
        return Some(InstanceGuard(0));
    }
    if !already { return Some(InstanceGuard(handle)); }
    let title = wide(WINDOW_TITLE);
    // 既存のプロセスがまだウィンドウを作っている最中のこともあるので少し待って探す
    for _ in 0..20 {
        let hwnd = unsafe { sys::FindWindowW(std::ptr::null(), title.as_ptr()) };
        if hwnd != 0 {
            unsafe {
                if sys::IsIconic(hwnd) != 0 { sys::ShowWindow(hwnd, sys::SW_RESTORE); }
                sys::SetForegroundWindow(hwnd);
            }
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    None
}

#[cfg(not(target_os = "windows"))]
pub fn acquire() -> Option<InstanceGuard> { Some(InstanceGuard(0)) }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wide_is_null_terminated() {
        let w = wide("G600r 制御ソフト");
        assert_eq!(*w.last().unwrap(), 0);
        assert_eq!(String::from_utf16(&w[..w.len() - 1]).unwrap(), "G600r 制御ソフト");
    }
}

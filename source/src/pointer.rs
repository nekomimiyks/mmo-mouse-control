//! Windows のポインター・ホイール設定（マウス本体ではなく、この PC の全マウス共通の設定）。
//! 「マウスのプロパティ」のポインター オプション／ホイールと同じ項目を扱う。

/// カーソルの速さ（Windows の目盛り 1〜20、標準は 10）。
pub const SPEED_MIN: u32 = 1;
pub const SPEED_MAX: u32 = 20;
pub const SPEED_DEFAULT: u32 = 10;
/// ホイール 1 目盛りでスクロールする行数（Windows の設定画面と同じ 1〜100、標準は 3）。
pub const LINES_MIN: u32 = 1;
pub const LINES_MAX: u32 = 100;
pub const LINES_DEFAULT: u32 = 3;
/// 横スクロール（チルト）1 回の文字数（1〜100、標準は 3）。
pub const CHARS_MIN: u32 = 1;
pub const CHARS_MAX: u32 = 100;
pub const CHARS_DEFAULT: u32 = 3;
/// SPI_SETWHEELSCROLLLINES でこの値を指定すると「1 目盛りで 1 画面分」になる。
pub const WHEEL_PAGESCROLL: u32 = 0xFFFF_FFFF;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerSettings {
    /// カーソルの速さ 1〜20
    pub speed: u32,
    /// 「ポインターの精度を高める」（速く動かすとカーソルが加速する）
    pub acceleration: bool,
    /// ホイール 1 目盛りの行数 1〜100（wheel_page が true のときは使わない）
    pub wheel_lines: u32,
    /// 1 目盛りで 1 画面分スクロールする
    pub wheel_page: bool,
    /// 横スクロール 1 回の文字数 1〜100
    pub wheel_chars: u32,
}

/// SPI_SETMOUSE に渡す [しきい値1, しきい値2, 加速]。Windows 標準のオン/オフの値。
pub fn mouse_params(acceleration: bool) -> [i32; 3] {
    if acceleration {
        [6, 10, 1]
    } else {
        [0, 0, 0]
    }
}

/// SPI_GETMOUSE の値から加速の有無を判定する。
pub fn acceleration_from_params(params: [i32; 3]) -> bool {
    params[2] != 0
}

/// SPI_GETWHEELSCROLLLINES の値 → (行数, 1画面ずつか)。0（スクロールしない）などの範囲外は範囲内に丸める。
pub fn decode_wheel_lines(value: u32) -> (u32, bool) {
    if value == WHEEL_PAGESCROLL {
        (LINES_DEFAULT, true)
    } else {
        (value.clamp(LINES_MIN, LINES_MAX), false)
    }
}

/// (行数, 1画面ずつか) → SPI_SETWHEELSCROLLLINES に渡す値。
pub fn encode_wheel_lines(lines: u32, page: bool) -> u32 {
    if page {
        WHEEL_PAGESCROLL
    } else {
        lines.clamp(LINES_MIN, LINES_MAX)
    }
}

/// 書き込む前に一度だけ範囲を検証する（範囲外は丸める）。
pub fn validated(s: PointerSettings) -> PointerSettings {
    PointerSettings {
        speed: s.speed.clamp(SPEED_MIN, SPEED_MAX),
        acceleration: s.acceleration,
        wheel_lines: s.wheel_lines.clamp(LINES_MIN, LINES_MAX),
        wheel_page: s.wheel_page,
        wheel_chars: s.wheel_chars.clamp(CHARS_MIN, CHARS_MAX),
    }
}

#[cfg(target_os = "windows")]
mod sys {
    use std::ffi::c_void;
    pub const SPI_GETMOUSE: u32 = 0x0003;
    pub const SPI_SETMOUSE: u32 = 0x0004;
    pub const SPI_GETWHEELSCROLLLINES: u32 = 0x0068;
    pub const SPI_SETWHEELSCROLLLINES: u32 = 0x0069;
    pub const SPI_GETWHEELSCROLLCHARS: u32 = 0x006C;
    pub const SPI_SETWHEELSCROLLCHARS: u32 = 0x006D;
    pub const SPI_GETMOUSESPEED: u32 = 0x0070;
    pub const SPI_SETMOUSESPEED: u32 = 0x0071;
    /// 設定を保存し（次回ログオン後も有効）、他のアプリへ変更を通知する。
    pub const SPIF_UPDATE_AND_SEND: u32 = 0x0001 | 0x0002;
    #[link(name = "user32")]
    extern "system" {
        pub fn SystemParametersInfoW(action: u32, param: u32, pv: *mut c_void, win_ini: u32)
            -> i32;
    }
}

#[cfg(target_os = "windows")]
fn spi(
    action: u32,
    param: u32,
    pv: *mut std::ffi::c_void,
    win_ini: u32,
    what: &str,
) -> Result<(), String> {
    if unsafe { sys::SystemParametersInfoW(action, param, pv, win_ini) } == 0 {
        Err(format!("{what}: {}", std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

#[cfg(target_os = "windows")]
pub fn read() -> Result<PointerSettings, String> {
    use std::ffi::c_void;
    let mut speed: u32 = 0;
    let mut params = [0i32; 3];
    let mut lines: u32 = 0;
    let mut chars: u32 = 0;
    spi(
        sys::SPI_GETMOUSESPEED,
        0,
        &mut speed as *mut u32 as *mut c_void,
        0,
        "カーソルの速さを読めません",
    )?;
    spi(
        sys::SPI_GETMOUSE,
        0,
        params.as_mut_ptr() as *mut c_void,
        0,
        "加速の設定を読めません",
    )?;
    spi(
        sys::SPI_GETWHEELSCROLLLINES,
        0,
        &mut lines as *mut u32 as *mut c_void,
        0,
        "スクロール行数を読めません",
    )?;
    spi(
        sys::SPI_GETWHEELSCROLLCHARS,
        0,
        &mut chars as *mut u32 as *mut c_void,
        0,
        "横スクロールの文字数を読めません",
    )?;
    let (wheel_lines, wheel_page) = decode_wheel_lines(lines);
    Ok(PointerSettings {
        speed,
        acceleration: acceleration_from_params(params),
        wheel_lines,
        wheel_page,
        wheel_chars: chars.clamp(CHARS_MIN, CHARS_MAX),
    })
}

#[cfg(target_os = "windows")]
/// persist が false のときは今のセッションにだけ即時反映（ドラッグ中用）。
/// true のときは保存して他のアプリへ通知する（ドラッグを離したとき・確定時）。
pub fn write(settings: PointerSettings, persist: bool) -> Result<(), String> {
    use std::ffi::c_void;
    let s = validated(settings);
    let mut params = mouse_params(s.acceleration);
    let flags = if persist {
        sys::SPIF_UPDATE_AND_SEND
    } else {
        0
    };
    // SPI_SETMOUSESPEED は値そのものを pvParam に入れる。
    spi(
        sys::SPI_SETMOUSESPEED,
        0,
        s.speed as usize as *mut c_void,
        flags,
        "カーソルの速さを設定できません",
    )?;
    spi(
        sys::SPI_SETMOUSE,
        0,
        params.as_mut_ptr() as *mut c_void,
        flags,
        "加速の設定を変更できません",
    )?;
    // ホイールの 2 項目は uiParam に値を入れる。
    spi(
        sys::SPI_SETWHEELSCROLLLINES,
        encode_wheel_lines(s.wheel_lines, s.wheel_page),
        std::ptr::null_mut(),
        flags,
        "スクロール行数を設定できません",
    )?;
    spi(
        sys::SPI_SETWHEELSCROLLCHARS,
        s.wheel_chars,
        std::ptr::null_mut(),
        flags,
        "横スクロールの文字数を設定できません",
    )?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn read() -> Result<PointerSettings, String> {
    Err("この OS では未対応です".into())
}

#[cfg(not(target_os = "windows"))]
pub fn write(_: PointerSettings, _: bool) -> Result<(), String> {
    Err("この OS では未対応です".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn acceleration_params_roundtrip() {
        assert!(acceleration_from_params(mouse_params(true)));
        assert!(!acceleration_from_params(mouse_params(false)));
        assert_eq!(mouse_params(true), [6, 10, 1]);
    }
    #[test]
    fn wheel_lines_encoding() {
        assert_eq!(decode_wheel_lines(3), (3, false));
        assert_eq!(decode_wheel_lines(WHEEL_PAGESCROLL), (LINES_DEFAULT, true));
        assert_eq!(decode_wheel_lines(0), (1, false));
        assert_eq!(decode_wheel_lines(500), (100, false));
        assert_eq!(encode_wheel_lines(5, false), 5);
        assert_eq!(encode_wheel_lines(5, true), WHEEL_PAGESCROLL);
        assert_eq!(encode_wheel_lines(0, false), 1);
    }
    #[test]
    fn validation_clamps_once() {
        let s = validated(PointerSettings {
            speed: 0,
            acceleration: true,
            wheel_lines: 0,
            wheel_page: false,
            wheel_chars: 999,
        });
        assert_eq!((s.speed, s.wheel_lines, s.wheel_chars), (1, 1, 100));
        let s = validated(PointerSettings {
            speed: 25,
            acceleration: false,
            wheel_lines: 3,
            wheel_page: true,
            wheel_chars: 3,
        });
        assert_eq!(s.speed, 20);
        assert!(s.wheel_page);
    }
    #[cfg(target_os = "windows")]
    #[test]
    fn reads_current_settings() {
        // 読むだけ（設定は変更しない）
        let s = read().unwrap();
        assert!((SPEED_MIN..=SPEED_MAX).contains(&s.speed));
        assert!((LINES_MIN..=LINES_MAX).contains(&s.wheel_lines));
    }
}

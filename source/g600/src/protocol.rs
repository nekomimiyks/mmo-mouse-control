//! G600 / G600r on-board memory protocol.
//!
//! The G600 does not speak HID++. It is configured through plain HID feature
//! reports on its vendor interface:
//!
//! * `0xF0` (4 bytes)   : currently active profile and DPI slot
//! * `0xF3/0xF4/0xF5`   : profile 1/2/3, 154 bytes each
//!
//! Layout knowledge comes from public documentation of the device (e.g. the
//! MIT-licensed libratbag project). This file is an independent implementation.

pub const VENDOR_LOGITECH: u16 = 0x046d;
/// G600 and G600r share this product ID (confirmed on device 2026-11-11).
pub const PRODUCT_G600: u16 = 0xc24a;

pub const REPORT_ACTIVE: u8 = 0xF0;
pub const REPORT_PROFILES: [u8; 3] = [0xF3, 0xF4, 0xF5];
pub const ACTIVE_LEN: usize = 4;
pub const PROFILE_LEN: usize = 154;
pub const BUTTONS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Active {
    /// 0-based profile index.
    pub profile: u8,
    /// 0-based DPI slot index.
    pub dpi_slot: u8,
}

impl Active {
    pub fn parse(buf: &[u8]) -> Result<Self, String> {
        if buf.len() != ACTIVE_LEN || buf[0] != REPORT_ACTIVE || buf[1] >> 4 > 2 {
            return Err(format!("unexpected active report: {}", hex(buf)));
        }
        Ok(Active {
            profile: (buf[1] >> 4) & 0x0f,
            dpi_slot: (buf[1] >> 1) & 0x03,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedEffect {
    Solid,
    Breathe,
    Cycle,
    Unknown(u8),
}

impl From<u8> for LedEffect {
    fn from(v: u8) -> Self {
        match v {
            0 => LedEffect::Solid,
            1 => LedEffect::Breathe,
            2 => LedEffect::Cycle,
            x => LedEffect::Unknown(x),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Mouse(u8),
    DpiUp,
    DpiDown,
    DpiCycle,
    ProfileCycle,
    DpiShift,
    GShift,
    Key { modifiers: u8, usage: u8 },
    Unknown { code: u8, modifiers: u8, usage: u8 },
}

impl Action {
    pub fn parse(b: &[u8]) -> Self {
        let (code, modifiers, usage) = (b[0], b[1], b[2]);
        match code {
            0x00 if modifiers == 0 && usage == 0 => Action::None,
            0x00 => Action::Key { modifiers, usage },
            0x01..=0x05 => Action::Mouse(code),
            0x11 => Action::DpiUp,
            0x12 => Action::DpiDown,
            0x13 => Action::DpiCycle,
            0x14 => Action::ProfileCycle,
            0x15 => Action::DpiShift,
            0x17 => Action::GShift,
            _ => Action::Unknown { code, modifiers, usage },
        }
    }

    pub fn describe(&self) -> String {
        match *self {
            Action::None => "(なし)".into(),
            Action::Mouse(1) => "左クリック".into(),
            Action::Mouse(2) => "右クリック".into(),
            Action::Mouse(3) => "中クリック".into(),
            Action::Mouse(4) => "戻る".into(),
            Action::Mouse(5) => "進む".into(),
            Action::Mouse(n) => format!("マウスボタン{n}"),
            Action::DpiUp => "DPIアップ".into(),
            Action::DpiDown => "DPIダウン".into(),
            Action::DpiCycle => "DPI切り替え（循環）".into(),
            Action::ProfileCycle => "プロファイル切り替え".into(),
            Action::DpiShift => "DPIシフト（押している間）".into(),
            Action::GShift => "Gシフト".into(),
            Action::Key { modifiers, usage } => {
                let mut parts = modifier_names(modifiers);
                if usage != 0 {
                    parts.push(key_name(usage));
                }
                format!("キー {}", parts.join("+"))
            }
            Action::Unknown { code, modifiers, usage } => {
                format!("不明 (code=0x{code:02X} mod=0x{modifiers:02X} key=0x{usage:02X})")
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Profile {
    pub raw: Vec<u8>,
    pub led_rgb: [u8; 3],
    pub led_effect: LedEffect,
    pub led_seconds: u8,
    pub poll_hz: u32,
    /// 1-based default DPI slot as stored on the device.
    pub dpi_default: u8,
    pub dpi_shift: u32,
    /// DPI values of the 4 slots; 0 means the slot is disabled.
    pub dpi: [u32; 4],
    pub buttons: [Action; BUTTONS],
    pub gshift_rgb: [u8; 3],
    pub gshift_buttons: [Action; BUTTONS],
}

impl Profile {
    pub fn parse(buf: &[u8]) -> Result<Self, String> {
        if buf.len() < PROFILE_LEN {
            return Err(format!("profile report too short ({} bytes)", buf.len()));
        }
        if !REPORT_PROFILES.contains(&buf[0]) {
            return Err(format!("unexpected profile report id 0x{:02X}", buf[0]));
        }
        let action_at = |off: usize| Action::parse(&buf[off..off + 3]);
        let mut buttons = [Action::None; BUTTONS];
        let mut gshift_buttons = [Action::None; BUTTONS];
        for i in 0..BUTTONS {
            buttons[i] = action_at(31 + i * 3);
            gshift_buttons[i] = action_at(94 + i * 3);
        }
        Ok(Profile {
            raw: buf[..PROFILE_LEN].to_vec(),
            led_rgb: [buf[1], buf[2], buf[3]],
            led_effect: buf[4].into(),
            led_seconds: buf[5],
            poll_hz: 1000 / (buf[11] as u32 + 1),
            dpi_shift: buf[12] as u32 * 50,
            dpi_default: buf[13],
            dpi: [
                buf[14] as u32 * 50,
                buf[15] as u32 * 50,
                buf[16] as u32 * 50,
                buf[17] as u32 * 50,
            ],
            buttons,
            gshift_rgb: [buf[91], buf[92], buf[93]],
            gshift_buttons,
        })
    }
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

pub const POLL_RATES: [u32; 8] = [1000, 500, 333, 250, 200, 166, 142, 125];
pub const DPI_MIN: u32 = 200;
pub const DPI_MAX: u32 = 8200;

/// Physical name of button slot `i` (0-based). Slots 4-20 match the G numbers
/// printed in Logicool's software.
pub fn button_label(i: usize) -> String {
    match i {
        0 => "左ボタン".into(),
        1 => "右ボタン".into(),
        2 => "ホイールクリック".into(),
        3 => "G4 ホイール左チルト".into(),
        4 => "G5 ホイール右チルト".into(),
        5 => "G6 薬指ボタン".into(),
        6 => "G7".into(),
        7 => "G8".into(),
        n => format!("G{} サイド", n + 1),
    }
}

/// Parse a button name: 1-20 (slot number), "left"/"right"/"middle", or "G4".."G20".
pub fn parse_button(s: &str) -> Result<usize, String> {
    let l = s.to_ascii_lowercase();
    let n = match l.as_str() {
        "left" | "l" => 1,
        "right" | "r" => 2,
        "middle" | "wheel" | "m" => 3,
        _ => l
            .trim_start_matches('g')
            .trim_start_matches('#')
            .parse::<usize>()
            .map_err(|_| format!("ボタン名が不正です: {s}（1〜20、G4〜G20、left/right/middle）"))?,
    };
    if !(1..=BUTTONS).contains(&n) {
        return Err(format!("ボタン番号は1〜20です: {s}"));
    }
    Ok(n - 1)
}

impl Action {
    pub fn encode(&self) -> [u8; 3] {
        match *self {
            Action::None => [0, 0, 0],
            Action::Mouse(n) => [n, 0, 0],
            Action::DpiUp => [0x11, 0, 0],
            Action::DpiDown => [0x12, 0, 0],
            Action::DpiCycle => [0x13, 0, 0],
            Action::ProfileCycle => [0x14, 0, 0],
            Action::DpiShift => [0x15, 0, 0],
            Action::GShift => [0x17, 0, 0],
            Action::Key { modifiers, usage } => [0, modifiers, usage],
            Action::Unknown { code, modifiers, usage } => [code, modifiers, usage],
        }
    }

    /// Parse an action such as `left`, `dpi-up`, `ctrl+shift+a`, `f5`, `raw:16:00:00`.
    pub fn from_str(s: &str) -> Result<Self, String> {
        let l = s.to_ascii_lowercase();
        let simple = match l.as_str() {
            "none" | "off" | "disable" => Some(Action::None),
            "left" => Some(Action::Mouse(1)),
            "right" => Some(Action::Mouse(2)),
            "middle" => Some(Action::Mouse(3)),
            "back" => Some(Action::Mouse(4)),
            "forward" => Some(Action::Mouse(5)),
            "dpi-up" => Some(Action::DpiUp),
            "dpi-down" => Some(Action::DpiDown),
            "dpi-cycle" => Some(Action::DpiCycle),
            "dpi-shift" => Some(Action::DpiShift),
            "profile-cycle" => Some(Action::ProfileCycle),
            "gshift" | "g-shift" => Some(Action::GShift),
            _ => None,
        };
        if let Some(a) = simple {
            return Ok(a);
        }
        if let Some(rest) = l.strip_prefix("raw:") {
            let b: Vec<u8> = rest
                .split(':')
                .map(|x| u8::from_str_radix(x, 16))
                .collect::<Result<_, _>>()
                .map_err(|_| format!("raw指定が不正です: {s}（例 raw:16:00:00）"))?;
            if b.len() != 3 {
                return Err(format!("raw指定は3バイトです: {s}"));
            }
            return Ok(Action::parse(&b));
        }
        // keyboard: modifiers joined with '+', last part a key
        let mut modifiers = 0u8;
        let mut usage = 0u8;
        for part in l.split('+') {
            let bit = match part {
                "ctrl" | "lctrl" => Some(0),
                "shift" | "lshift" => Some(1),
                "alt" | "lalt" => Some(2),
                "win" | "lwin" | "meta" => Some(3),
                "rctrl" => Some(4),
                "rshift" => Some(5),
                "ralt" => Some(6),
                "rwin" => Some(7),
                _ => None,
            };
            match bit {
                Some(b) => modifiers |= 1 << b,
                None => {
                    if usage != 0 {
                        return Err(format!("キーは1つだけ指定できます: {s}"));
                    }
                    usage = key_usage(part).ok_or_else(|| {
                        format!("不明なキーまたは動作です: {part}（`g600ctl help` を参照）")
                    })?;
                }
            }
        }
        if modifiers == 0 && usage == 0 {
            return Err(format!("動作が空です: {s}"));
        }
        Ok(Action::Key { modifiers, usage })
    }
}

/// Reverse of [`key_name`], case-insensitive.
pub fn key_usage(name: &str) -> Option<u8> {
    let aliases = [("esc", 0x29u8), ("escape", 0x29), ("return", 0x28), ("del", 0x4c),
        ("pgup", 0x4b), ("pgdn", 0x4e), ("ins", 0x49), ("minus", 0x2d), ("equal", 0x2e),
        ("space", 0x2c), ("tab", 0x2b), ("numplus", 0x57), ("numminus", 0x56)];
    if let Some(&(_, u)) = aliases.iter().find(|(a, _)| *a == name) {
        return Some(u);
    }
    (1u8..=0xff).find(|&u| key_name(u).eq_ignore_ascii_case(name) && !key_name(u).starts_with("0x"))
}

/// Mutable view over a raw 154-byte profile report. Unknown bytes are kept.
pub struct ProfileEdit<'a>(pub &'a mut [u8]);

impl ProfileEdit<'_> {
    /// `slots`: up to 4 DPI values, 0 = disabled. `default`: 1-based slot.
    pub fn set_dpi(&mut self, slots: &[u32], default: Option<u8>) -> Result<(), String> {
        if slots.is_empty() || slots.len() > 4 {
            return Err("DPIは1〜4個指定してください".into());
        }
        if slots[0] == 0 {
            return Err("1つ目のDPIは無効にできません".into());
        }
        let mut raw = [0u8; 4];
        for (i, &d) in slots.iter().enumerate() {
            if d == 0 {
                continue;
            }
            if !(DPI_MIN..=DPI_MAX).contains(&d) {
                return Err(format!("DPIは{DPI_MIN}〜{DPI_MAX}です: {d}"));
            }
            raw[i] = ((d + 25) / 50) as u8;
        }
        self.0[14..18].copy_from_slice(&raw);
        let def = default.unwrap_or(self.0[13]);
        let def = if (1..=4).contains(&def) && raw[def as usize - 1] != 0 { def } else { 1 };
        self.0[13] = def;
        Ok(())
    }

    pub fn set_dpi_shift(&mut self, dpi: u32) -> Result<(), String> {
        if dpi != 0 && !(DPI_MIN..=DPI_MAX).contains(&dpi) {
            return Err(format!("DPIシフトは0または{DPI_MIN}〜{DPI_MAX}です: {dpi}"));
        }
        self.0[12] = ((dpi + 25) / 50) as u8;
        Ok(())
    }

    pub fn set_led(&mut self, rgb: [u8; 3], effect: LedEffect, seconds: u8) -> Result<(), String> {
        let e = match effect {
            LedEffect::Solid => 0,
            LedEffect::Breathe => 1,
            LedEffect::Cycle => 2,
            LedEffect::Unknown(x) => x,
        };
        if effect != LedEffect::Solid && !(1..=15).contains(&seconds) {
            return Err("効果の秒数は1〜15です".into());
        }
        self.0[1..4].copy_from_slice(&rgb);
        self.0[4] = e;
        self.0[5] = if effect == LedEffect::Solid { self.0[5] } else { seconds };
        Ok(())
    }

    pub fn set_gshift_led(&mut self, rgb: [u8; 3]) {
        self.0[91..94].copy_from_slice(&rgb);
    }

    pub fn set_poll(&mut self, hz: u32) -> Result<(), String> {
        let v = POLL_RATES
            .iter()
            .position(|&r| r == hz)
            .map(|_| (1000 / hz - 1) as u8)
            .ok_or_else(|| format!("ポーリングレートは {:?} のどれかです: {hz}", POLL_RATES))?;
        self.0[11] = v;
        Ok(())
    }

    pub fn set_button(&mut self, slot: usize, gshift: bool, action: Action) {
        let off = if gshift { 94 } else { 31 } + slot * 3;
        self.0[off..off + 3].copy_from_slice(&action.encode());
    }
}

pub fn parse_rgb(s: &str) -> Result<[u8; 3], String> {
    let h = s.trim_start_matches('#');
    if h.len() != 6 {
        return Err(format!("色は RRGGBB 形式です: {s}"));
    }
    let v = u32::from_str_radix(h, 16).map_err(|_| format!("色は RRGGBB 形式です: {s}"))?;
    Ok([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

pub fn parse_hex_line(line: &str) -> Result<Vec<u8>, String> {
    line.split_whitespace()
        .map(|x| u8::from_str_radix(x, 16).map_err(|_| format!("16進データが不正です: {x}")))
        .collect()
}

pub fn active_set_profile(index: u8) -> [u8; ACTIVE_LEN] {
    [REPORT_ACTIVE, 0x80 | (index << 4), 0, 0]
}

pub fn modifier_names(m: u8) -> Vec<String> {
    const NAMES: [&str; 8] = ["LCtrl", "LShift", "LAlt", "LWin", "RCtrl", "RShift", "RAlt", "RWin"];
    (0..8)
        .filter(|i| m & (1 << i) != 0)
        .map(|i| NAMES[i].to_string())
        .collect()
}

/// Name of a USB HID keyboard usage (Usage Page 0x07).
pub fn key_name(u: u8) -> String {
    match u {
        0x04..=0x1d => ((b'A' + (u - 0x04)) as char).to_string(),
        0x1e..=0x26 => ((b'1' + (u - 0x1e)) as char).to_string(),
        0x27 => "0".into(),
        0x28 => "Enter".into(),
        0x29 => "Esc".into(),
        0x2a => "Backspace".into(),
        0x2b => "Tab".into(),
        0x2c => "Space".into(),
        0x2d => "-".into(),
        0x2e => "=".into(),
        0x2f => "[".into(),
        0x30 => "]".into(),
        0x31 => "\\".into(),
        0x33 => ";".into(),
        0x34 => "'".into(),
        0x35 => "`".into(),
        0x36 => ",".into(),
        0x37 => ".".into(),
        0x38 => "/".into(),
        0x39 => "CapsLock".into(),
        0x3a..=0x45 => format!("F{}", u - 0x3a + 1),
        0x46 => "PrintScreen".into(),
        0x47 => "ScrollLock".into(),
        0x48 => "Pause".into(),
        0x49 => "Insert".into(),
        0x4a => "Home".into(),
        0x4b => "PageUp".into(),
        0x4c => "Delete".into(),
        0x4d => "End".into(),
        0x4e => "PageDown".into(),
        0x4f => "Right".into(),
        0x50 => "Left".into(),
        0x51 => "Down".into(),
        0x52 => "Up".into(),
        0x53 => "NumLock".into(),
        0x54 => "Num/".into(),
        0x55 => "Num*".into(),
        0x56 => "Num-".into(),
        0x57 => "Num+".into(),
        0x58 => "NumEnter".into(),
        0x59..=0x61 => format!("Num{}", u - 0x59 + 1),
        0x62 => "Num0".into(),
        0x63 => "Num.".into(),
        0x65 => "Menu".into(),
        0x68..=0x73 => format!("F{}", u - 0x68 + 13),
        _ => format!("0x{u:02X}"),
    }
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_active() {
        // profile 2 (index 1), dpi slot 3 (index 2)
        let a = Active::parse(&[0xF0, (1 << 4) | (2 << 1), 0, 0]).unwrap();
        assert_eq!(a, Active { profile: 1, dpi_slot: 2 });
        assert!(Active::parse(&[0xF0, 0, 0]).is_err());
        assert!(Active::parse(&[0xF0, 0, 0, 0, 0]).is_err());
        assert!(Active::parse(&[0xF0, 0x30, 0, 0]).is_err());
    }

    #[test]
    fn parses_profile() {
        let mut b = vec![0u8; PROFILE_LEN];
        b[0] = 0xF3;
        b[1..4].copy_from_slice(&[0xff, 0x80, 0x00]);
        b[4] = 1;
        b[5] = 4;
        b[11] = 0; // 1000 Hz
        b[13] = 2;
        b[14..18].copy_from_slice(&[0x08, 0x10, 0x20, 0x00]);
        b[31..34].copy_from_slice(&[0x01, 0, 0]);
        b[34..37].copy_from_slice(&[0x00, 0x01, 0x06]); // LCtrl+C
        b[37..40].copy_from_slice(&[0x17, 0, 0]);
        let p = Profile::parse(&b).unwrap();
        assert_eq!(p.led_rgb, [0xff, 0x80, 0x00]);
        assert_eq!(p.led_effect, LedEffect::Breathe);
        assert_eq!(p.poll_hz, 1000);
        assert_eq!(p.dpi, [400, 800, 1600, 0]);
        assert_eq!(p.buttons[0], Action::Mouse(1));
        assert_eq!(p.buttons[1].describe(), "キー LCtrl+C");
        assert_eq!(p.buttons[2], Action::GShift);
        assert_eq!(p.buttons[3], Action::None);
    }

    #[test]
    fn action_roundtrip() {
        for s in ["left", "dpi-up", "gshift", "ctrl+shift+a", "f5", "num1", "alt+tab", "raw:16:00:00", "none"] {
            let a = Action::from_str(s).unwrap();
            assert_eq!(Action::parse(&a.encode()), a, "{s}");
        }
        assert_eq!(Action::from_str("ctrl+c").unwrap(), Action::Key { modifiers: 1, usage: 0x06 });
        assert!(Action::from_str("a+b").is_err());
        assert!(Action::from_str("bogus").is_err());
    }

    #[test]
    fn edits_profile() {
        let mut b = vec![0u8; PROFILE_LEN];
        b[0] = 0xF3;
        b[7] = 0xAB; // unknown byte must survive
        let mut e = ProfileEdit(&mut b);
        e.set_dpi(&[400, 1200, 0, 3200], Some(2)).unwrap();
        e.set_led([1, 2, 3], LedEffect::Cycle, 5).unwrap();
        e.set_poll(500).unwrap();
        e.set_button(parse_button("G9").unwrap(), true, Action::from_str("ctrl+1").unwrap());
        assert!(e.set_dpi(&[100], None).is_err());
        assert!(e.set_poll(300).is_err());
        let p = Profile::parse(&b).unwrap();
        assert_eq!(p.dpi, [400, 1200, 0, 3200]);
        assert_eq!(p.dpi_default, 2);
        assert_eq!(p.led_rgb, [1, 2, 3]);
        assert_eq!(p.led_effect, LedEffect::Cycle);
        assert_eq!(p.poll_hz, 500);
        assert_eq!(p.gshift_buttons[8].describe(), "キー LCtrl+1");
        assert_eq!(b[7], 0xAB);
    }

    #[test]
    fn parses_buttons() {
        assert_eq!(parse_button("left").unwrap(), 0);
        assert_eq!(parse_button("G20").unwrap(), 19);
        assert_eq!(parse_button("7").unwrap(), 6);
        assert!(parse_button("G21").is_err());
    }

    #[test]
    fn rejects_short_profile() {
        assert!(Profile::parse(&[0xF3, 0, 0]).is_err());
    }
}

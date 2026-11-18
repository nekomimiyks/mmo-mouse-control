//! いま押されているキー・マウスボタンを GetAsyncKeyState で調べる（Windows のみ）。
//! egui のキーイベントは IME 有効時などに届かないことがあるため、こちらで直接読む。
//! 仮想キー → スキャンコード（MapVirtualKeyW、キーボード配列に依存しない）→ HID usage の順に変換する。

/// いま押されている入力
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Held {
    /// 修飾キー以外で押されているキーの HID usage
    pub usages: Vec<u8>,
    /// 修飾キー（HID の並び: bit0 左Ctrl, 1 左Shift, 2 左Alt, 3 左Win, 4 右Ctrl, 5 右Shift, 6 右Alt, 7 右Win）
    pub modifiers: u8,
    /// 押されているマウスボタン 1=左 2=右 3=中 4=戻る 5=進む
    pub mouse: Vec<u8>,
}

/// 修飾キーの仮想キーと HID の修飾ビット
pub const MODIFIER_VKS: [(u8, u8); 8] = [
    (0xA2, 0x01), (0xA0, 0x02), (0xA4, 0x04), (0x5B, 0x08), // 左 Ctrl Shift Alt Win
    (0xA3, 0x10), (0xA1, 0x20), (0xA5, 0x40), (0x5C, 0x80), // 右 Ctrl Shift Alt Win
];

/// マウスボタンの仮想キー → 番号
pub const MOUSE_VKS: [(u8, u8); 5] = [(0x01, 1), (0x02, 2), (0x04, 3), (0x05, 4), (0x06, 5)];

/// スキャンコードを使わずに直接決める仮想キー
pub fn vk_special(vk: u8) -> Option<u8> {
    match vk {
        0x2C => Some(0x46), // PrintScreen
        0x13 => Some(0x48), // Pause
        0x91 => Some(0x47), // ScrollLock
        0x90 => Some(0x53), // NumLock
        _ => None,
    }
}

/// 拡張スキャンコード（拡張キーは 0xE0xx。MapVirtualKeyW の MAPVK_VK_TO_VSC_EX の値）→ HID usage
pub fn scan_to_usage(scan: u32) -> Option<u8> {
    let u = match scan {
        0x01 => 0x29, // Esc
        0x02..=0x0A => 0x1E + (scan - 0x02) as u8, // 1〜9
        0x0B => 0x27, // 0
        0x0C => 0x2D, 0x0D => 0x2E, 0x0E => 0x2A, 0x0F => 0x2B,
        0x10 => 0x14, 0x11 => 0x1A, 0x12 => 0x08, 0x13 => 0x15, 0x14 => 0x17, // Q W E R T
        0x15 => 0x1C, 0x16 => 0x18, 0x17 => 0x0C, 0x18 => 0x12, 0x19 => 0x13, // Y U I O P
        0x1A => 0x2F, 0x1B => 0x30, 0x1C => 0x28,
        0x1E => 0x04, 0x1F => 0x16, 0x20 => 0x07, 0x21 => 0x09, 0x22 => 0x0A, // A S D F G
        0x23 => 0x0B, 0x24 => 0x0D, 0x25 => 0x0E, 0x26 => 0x0F, // H J K L
        0x27 => 0x33, 0x28 => 0x34, 0x29 => 0x35, 0x2B => 0x31,
        0x2C => 0x1D, 0x2D => 0x1B, 0x2E => 0x06, 0x2F => 0x19, 0x30 => 0x05, 0x31 => 0x11, 0x32 => 0x10, // Z X C V B N M
        0x33 => 0x36, 0x34 => 0x37, 0x35 => 0x38,
        0x37 => 0x55, // Num*
        0x39 => 0x2C, // Space
        0x3A => 0x39, // CapsLock
        0x3B..=0x44 => 0x3A + (scan - 0x3B) as u8, // F1〜F10
        0x47 => 0x5F, 0x48 => 0x60, 0x49 => 0x61, 0x4A => 0x56, // Num7 Num8 Num9 Num-
        0x4B => 0x5C, 0x4C => 0x5D, 0x4D => 0x5E, 0x4E => 0x57, // Num4 Num5 Num6 Num+
        0x4F => 0x59, 0x50 => 0x5A, 0x51 => 0x5B, 0x52 => 0x62, 0x53 => 0x63, // Num1 Num2 Num3 Num0 Num.
        0x57 => 0x44, 0x58 => 0x45, // F11 F12
        0x64..=0x6E => 0x68 + (scan - 0x64) as u8, // F13〜F23
        0x76 => 0x73, // F24
        0x70 => 0x88, 0x73 => 0x87, 0x79 => 0x8A, 0x7B => 0x8B, 0x7D => 0x89, // 日本語キーボードの追加キー
        0xE01C => 0x58, // NumEnter
        0xE035 => 0x54, // Num/
        0xE047 => 0x4A, 0xE048 => 0x52, 0xE049 => 0x4B, // Home Up PageUp
        0xE04B => 0x50, 0xE04D => 0x4F, // Left Right
        0xE04F => 0x4D, 0xE050 => 0x51, 0xE051 => 0x4E, // End Down PageDown
        0xE052 => 0x49, 0xE053 => 0x4C, // Insert Delete
        0xE05D => 0x65, // Menu
        _ => return None,
    };
    Some(u)
}

/// 調べない仮想キー（マウス・修飾キーは別に扱う。0x10〜0x12 は左右をまとめた Shift/Ctrl/Alt）
pub fn skip_vk(vk: u8) -> bool {
    matches!(vk, 0x01..=0x07 | 0x10..=0x12 | 0xA0..=0xA5 | 0x5B | 0x5C)
}

#[cfg(target_os = "windows")]
mod sys {
    #[link(name = "user32")]
    extern "system" {
        pub fn GetAsyncKeyState(vk: i32) -> i16;
        pub fn MapVirtualKeyW(code: u32, map_type: u32) -> u32;
        pub fn GetSystemMetrics(index: i32) -> i32;
    }
    pub const MAPVK_VK_TO_VSC_EX: u32 = 4;
    pub const SM_SWAPBUTTON: i32 = 23;
}

#[cfg(target_os = "windows")]
fn down(vk: u8) -> bool {
    unsafe { sys::GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0 }
}

/// いま押されている入力を読む
#[cfg(target_os = "windows")]
pub fn read_held() -> Held {
    let mut held = Held::default();
    for (vk, bit) in MODIFIER_VKS { if down(vk) { held.modifiers |= bit; } }
    // 左右ボタンを入れ替えている設定では VK_LBUTTON が物理的な右ボタンになる
    let swapped = unsafe { sys::GetSystemMetrics(sys::SM_SWAPBUTTON) } != 0;
    for (vk, n) in MOUSE_VKS {
        if down(vk) {
            let n = match (swapped, n) { (true, 1) => 2, (true, 2) => 1, _ => n };
            held.mouse.push(n);
        }
    }
    for vk in 0x08u8..=0xFE {
        if skip_vk(vk) || !down(vk) { continue; }
        let usage = vk_special(vk).or_else(|| {
            let scan = unsafe { sys::MapVirtualKeyW(vk as u32, sys::MAPVK_VK_TO_VSC_EX) };
            scan_to_usage(scan)
        });
        if let Some(u) = usage { if !held.usages.contains(&u) { held.usages.push(u); } }
    }
    held
}

#[cfg(not(target_os = "windows"))]
pub fn read_held() -> Held { Held::default() }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scan_codes_map_to_hid_usages() {
        assert_eq!(scan_to_usage(0x02), Some(0x1E)); // 1
        assert_eq!(scan_to_usage(0x0B), Some(0x27)); // 0
        assert_eq!(scan_to_usage(0x30), Some(0x05)); // B
        assert_eq!(scan_to_usage(0x4F), Some(0x59)); // Num1（通常の 1 と区別する）
        assert_eq!(scan_to_usage(0x52), Some(0x62)); // Num0
        assert_eq!(scan_to_usage(0xE04F), Some(0x4D)); // End（Num1 とは別）
        assert_eq!(scan_to_usage(0xE01C), Some(0x58)); // NumEnter
        assert_eq!(scan_to_usage(0x1C), Some(0x28)); // Enter
        assert_eq!(scan_to_usage(0x3B), Some(0x3A)); // F1
        assert_eq!(scan_to_usage(0x58), Some(0x45)); // F12
        assert_eq!(scan_to_usage(0x76), Some(0x73)); // F24
        assert_eq!(scan_to_usage(0x00), None);
    }
    #[test]
    fn letters_cover_a_to_z() {
        let scans = [0x1E, 0x30, 0x2E, 0x20, 0x12, 0x21, 0x22, 0x23, 0x17, 0x24, 0x25, 0x26, 0x32,
                     0x31, 0x18, 0x19, 0x10, 0x13, 0x1F, 0x14, 0x16, 0x2F, 0x11, 0x2D, 0x15, 0x2C];
        for (i, scan) in scans.iter().enumerate() { assert_eq!(scan_to_usage(*scan), Some(0x04 + i as u8)); }
    }
    #[test]
    fn modifiers_and_mouse_are_skipped_in_key_scan() {
        assert!(skip_vk(0x01) && skip_vk(0x10) && skip_vk(0xA0) && skip_vk(0x5B));
        assert!(!skip_vk(0x31) && !skip_vk(0x61));
        assert_eq!(vk_special(0x2C), Some(0x46));
    }
}

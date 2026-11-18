//! M908の4バイト割当。復号不能な値もraw表現で往復させ、別項目の編集で壊さない。
use crate::protocol::{ACTION_CODES, KEY_CODES};
const LABELS: [(&str, &str); 15] = [
    ("左クリック", "left"),
    ("右クリック", "right"),
    ("ホイール押下", "middle"),
    ("戻る", "backward"),
    ("進む", "forward"),
    ("DPI ＋", "dpi+"),
    ("DPI －", "dpi-"),
    ("DPI 切替", "dpi-cycle"),
    ("プロファイル切替", "profile_switch"),
    ("ホイール上", "scroll_up"),
    ("ホイール下", "scroll_down"),
    ("無効", "none"),
    ("プロファイル ＋", "profile+"),
    ("プロファイル －", "profile-"),
    ("LED切替", "led_mode_switch"),
];
pub fn encode(text: &str) -> Result<[u8; 4], String> {
    let text = text.trim();
    let text = shortcut(text).map_or(text, |(_, _, keys)| keys);
    if let Some(raw) = text
        .strip_prefix("raw:")
        .or_else(|| text.strip_prefix("0x"))
    {
        if raw.len() != 8 || !raw.is_ascii() {
            return Err("raw割当は8桁の16進数です".into());
        }
        let mut bytes = [0; 4];
        for i in 0..4 {
            bytes[i] = u8::from_str_radix(&raw[i * 2..i * 2 + 2], 16)
                .map_err(|_| "raw割当の16進数が不正です")?;
        }
        return Ok(bytes);
    }
    let action = LABELS
        .iter()
        .find(|(label, _)| *label == text)
        .map(|(_, v)| *v)
        .unwrap_or(text);
    if let Some((_, code)) = ACTION_CODES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(action))
    {
        return Ok(*code);
    }
    if let Some(args) = text.strip_prefix("fire:") {
        let parts: Vec<_> = args.split(':').collect();
        if parts.len() != 3 {
            return Err("連射はfire:mouse_left:回数:間隔です".into());
        }
        let key = match parts[0] {
            "mouse_left" => 0x81,
            "mouse_right" => 0x82,
            "mouse_middle" => 0x84,
            v => key_code(v)?,
        };
        let repeats = parts[1].parse::<u8>().map_err(|_| "連射回数が不正です")?;
        if repeats == 0 {
            return Err("連射回数は1以上です".into());
        }
        let delay = parts[2].parse::<u8>().map_err(|_| "連射間隔が不正です")?;
        return Ok([0x99, key, repeats, delay]);
    }
    if let Some(rest) = text.strip_prefix("macro") {
        let mut args = rest.split(':');
        let slot = args
            .next()
            .unwrap_or("")
            .parse::<u8>()
            .map_err(|_| "マクロ番号が不正です")?;
        if !(1..=15).contains(&slot) {
            return Err("マクロ番号は1〜15です".into());
        }
        let variant = args.next().unwrap_or("1");
        if args.next().is_some() {
            return Err("マクロ指定が不正です".into());
        }
        return match variant {
            "until" => Ok([0x91, slot + 0x3f, 0xff, 0xff]),
            "while" => Ok([0x91, slot + 0x7f, 0xff, 0xff]),
            v => {
                let n = v.parse::<u8>().map_err(|_| "マクロ回数が不正です")?;
                if n == 0 {
                    return Err("マクロ回数は1以上です".into());
                }
                Ok([0x91, slot - 1, n, 0])
            }
        };
    }
    let text = text.strip_prefix("キー ").unwrap_or(text);
    let parts: Vec<_> = text.split('+').map(str::trim).collect();
    let mut modifiers = 0u8;
    for modifier in &parts[..parts.len() - 1] {
        let flag = match modifier.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "ctrl_l" => 1,
            "shift" | "shift_l" => 2,
            "alt" | "alt_l" => 4,
            "win" | "super" | "super_l" => 8,
            "ctrl_r" => 16,
            "shift_r" => 32,
            "alt_r" => 64,
            "super_r" | "win_r" => 128,
            _ => return Err(format!("不明な修飾キー: {modifier}")),
        };
        if modifiers & flag != 0 {
            return Err("修飾キーが重複しています".into());
        }
        modifiers |= flag;
    }
    let mut key = parts[parts.len() - 1];
    if key == "Plus" {
        modifiers |= 2;
        key = "Equals";
    }
    let usage = key_code(key)?;
    Ok([
        if modifiers == 0 { 0x90 } else { 0x8f },
        modifiers,
        usage,
        0,
    ])
}
/// 選んだ機能名をPC上に保持する。本体には従来と同じ4バイトだけを書き込む。
pub fn shortcut(text: &str) -> Option<(&'static str, &'static str, &'static str)> {
    let (family, name) = text.split_once(": ")?;
    let list = match family {
        "Chrome" => crate::presets::CHROME,
        "Windows" => crate::presets::WINDOWS,
        _ => return None,
    };
    let &(name, keys) = list.iter().find(|(label, _)| *label == name)?;
    Some((
        if family == "Chrome" {
            "Chrome"
        } else {
            "Windows"
        },
        name,
        keys,
    ))
}
pub fn display(text: &str) -> String {
    match shortcut(text) {
        Some((family, name, keys)) => {
            format!(
                "{family}: {name}（{}）",
                decode(encode(keys).expect("検証済みプリセット"))
            )
        }
        None => text.into(),
    }
}
fn key_code(key: &str) -> Result<u8, String> {
    let lower = key.to_ascii_lowercase();
    let key = match lower.as_str() {
        "escape" => "Esc",
        "enter" => "Return",
        "space" => "Space",
        "backspace" => "Backspace",
        "arrowup" => "Up",
        "arrowdown" => "Down",
        "arrowleft" => "Left",
        "arrowright" => "Right",
        "pageup" => "PgUp",
        "pagedown" => "PgDown",
        "printscreen" => "PrtSc",
        "scrolllock" => "ScrLk",
        "." => "Period",
        "," => "Comma",
        "/" => "Slash",
        "\\" => "Backslash",
        ";" => "Semicolon",
        "=" => "Equals",
        "-" => "Minus",
        "#" => "Hash",
        "numenter" => "Num_Return",
        "numplus" => "Num_Plus",
        "numminus" => "Num_Minus",
        "insert" => "Insert",
        "openbracket" | "[" => "Bracket_l",
        "closebracket" | "]" => "Bracket_r",
        "backtick" | "`" => "Tilde",
        "quote" | "'" => "Apostrophe",
        v => v,
    };
    KEY_CODES
        .iter()
        .find(|(name, _)| {
            name.eq_ignore_ascii_case(key)
                || name
                    .replace('_', "")
                    .eq_ignore_ascii_case(&key.replace('_', ""))
        })
        .map(|(_, code)| *code)
        .ok_or_else(|| format!("未対応のキー: {key}"))
}
pub fn decode(bytes: [u8; 4]) -> String {
    let raw = || {
        format!(
            "raw:{:02X}{:02X}{:02X}{:02X}",
            bytes[0], bytes[1], bytes[2], bytes[3]
        )
    };
    let candidate = if let Some((name, _)) = ACTION_CODES.iter().find(|(_, code)| *code == bytes) {
        LABELS
            .iter()
            .find(|(_, v)| v == name)
            .map(|(l, _)| l.to_string())
            .unwrap_or_else(|| name.to_string())
    } else if (bytes[0] == 0x90 && bytes[1] == 0 || bytes[0] == 0x8f) && bytes[3] == 0 {
        if let Some((key, _)) = KEY_CODES.iter().find(|(_, code)| *code == bytes[2]) {
            let mut parts = Vec::new();
            for (mask, label) in [
                (1, "Ctrl"),
                (2, "Shift"),
                (4, "Alt"),
                (8, "Win"),
                (16, "Ctrl_R"),
                (32, "Shift_R"),
                (64, "Alt_R"),
                (128, "Win_R"),
            ] {
                if bytes[1] & mask != 0 {
                    parts.push(label.to_string());
                }
            }
            parts.push(if key.len() == 1 {
                key.to_ascii_uppercase()
            } else {
                key.to_string()
            });
            parts.join(" + ")
        } else {
            return raw();
        }
    } else if bytes[0] == 0x99 {
        let key = match bytes[1] {
            0x81 => "mouse_left".into(),
            0x82 => "mouse_right".into(),
            0x84 => "mouse_middle".into(),
            v => KEY_CODES
                .iter()
                .find(|(_, code)| *code == v)
                .map(|(name, _)| name.to_string())
                .unwrap_or_default(),
        };
        format!("fire:{key}:{}:{}", bytes[2], bytes[3])
    } else if bytes[0] == 0x91 && bytes[1] < 15 && bytes[3] == 0 {
        format!("macro{}:{}", bytes[1] + 1, bytes[2])
    } else {
        return raw();
    };
    if encode(&candidate).ok() == Some(bytes) {
        candidate
    } else {
        raw()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn named_presets_keep_identical_device_bytes_and_show_function_and_keys() {
        for (family, list) in [
            ("Chrome", crate::presets::CHROME),
            ("Windows", crate::presets::WINDOWS),
        ] {
            for (name, keys) in list {
                let named = format!("{family}: {name}");
                assert_eq!(encode(&named).unwrap(), encode(keys).unwrap());
                let label = display(&named);
                assert!(label.contains(name));
                assert!(label.contains(&decode(encode(keys).unwrap())));
            }
        }
        assert!(encode("Chrome: 存在しない機能").is_err());
    }
    #[test]
    fn shortcuts_and_unknowns() {
        for s in [
            "Ctrl + C",
            "Ctrl + Shift + N",
            "Alt + Tab",
            "Win + E",
            "Ctrl + L",
            "F12",
            "1",
            "左クリック",
            "DPI ＋",
            "Ctrl + OpenBracket",
            "Shift + Quote",
        ] {
            let b = encode(s).unwrap();
            assert_eq!(encode(&decode(b)).unwrap(), b);
        }
        for b in [
            [0x99, 0x81, 3, 1],
            [0x91, 2, 1, 0],
            [0xde, 0xad, 0xbe, 0xef],
            [0x90, 1, 6, 7],
            [0x8f, 0, 6, 0],
        ] {
            assert_eq!(encode(&decode(b)).unwrap(), b);
        }
        assert!(encode("Ctrl + NoSuchKey").is_err());
        assert!(encode("macro0").is_err());
        assert!(encode("raw:あいうえ").is_err());
    }
    #[test]
    fn reject_accidental_zero_key() {
        assert!(encode("Ctrl + ").is_err());
        assert_eq!(encode("Ctrl + C").unwrap(), [0x8f, 1, 6, 0]);
        assert_eq!(encode("C").unwrap(), [0x90, 0, 6, 0]);
    }
}

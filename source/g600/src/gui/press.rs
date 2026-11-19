//! PC が受け取った入力（キー＋修飾キー・マウスボタン）の表示用（egui に依存しない部分）。

use g600ctl::protocol::key_name;
use super::winkeys::Held;

/// 修飾キー（左右をまとめた 4 ビット: Ctrl=1, Shift=2, Alt=4, Win=8）
pub fn collapse_modifiers(m: u8) -> u8 {
    (m & 0x0f) | (m >> 4)
}

/// 「Ctrl+1」「戻る」のような表示
pub fn describe_held(held: &Held) -> String {
    let mut parts: Vec<String> = Vec::new();
    for (bit, name) in [(0x01, "Ctrl"), (0x02, "Shift"), (0x04, "Alt"), (0x08, "Win")] {
        if collapse_modifiers(held.modifiers) & bit != 0 { parts.push(name.into()); }
    }
    for u in &held.usages { parts.push(key_name(*u)); }
    for m in &held.mouse {
        parts.push(match m { 1 => "左クリック", 2 => "右クリック", 3 => "中クリック", 4 => "戻る", _ => "進む" }.into());
    }
    parts.join("+")
}

/// 何か押されているか（修飾キーだけでも true）
pub fn is_pressed(held: &Held) -> bool {
    held.modifiers != 0 || !held.usages.is_empty() || !held.mouse.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn keys(usages: &[u8], modifiers: u8) -> Held { Held { usages: usages.to_vec(), modifiers, mouse: vec![] } }
    #[test]
    fn modifiers_collapse_left_and_right() {
        assert_eq!(collapse_modifiers(0x01), 1); // 左Ctrl
        assert_eq!(collapse_modifiers(0x10), 1); // 右Ctrl
        assert_eq!(collapse_modifiers(0x22), 2); // 左右Shift
        assert_eq!(collapse_modifiers(0x81), 9); // 左Ctrl + 右Win
    }
    #[test]
    fn describes_input() {
        assert_eq!(describe_held(&keys(&[0x1e], 0x10)), "Ctrl+1");
        assert_eq!(describe_held(&keys(&[0x59], 0)), "Num1");
        assert_eq!(describe_held(&Held { usages: vec![], modifiers: 0, mouse: vec![4] }), "戻る");
        assert!(!is_pressed(&Held::default()));
        assert!(is_pressed(&keys(&[], 0x02)));
    }
}

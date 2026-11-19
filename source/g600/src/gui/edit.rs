use g600ctl::protocol::{key_name, Action, LedEffect, Profile, ProfileEdit};

pub enum Change {
    Led([u8; 3], LedEffect, u8),
    GshiftLed([u8; 3]),
    Poll(u32),
    Dpi([u32; 4], u8),
    DpiShift(u32),
    Button(usize, bool, Action),
}

pub fn apply(raw: &mut [u8], change: Change) -> Result<(), String> {
    let mut edit = ProfileEdit(raw);
    match change {
        Change::Led(rgb, effect, seconds) => edit.set_led(rgb, effect, seconds),
        Change::GshiftLed(rgb) => { edit.set_gshift_led(rgb); Ok(()) },
        Change::Poll(hz) => edit.set_poll(hz),
        Change::Dpi(slots, default) => edit.set_dpi(&slots, Some(default)),
        Change::DpiShift(dpi) => edit.set_dpi_shift(dpi),
        Change::Button(slot, shifted, action) => { edit.set_button(slot, shifted, action); Ok(()) },
    }
}

pub fn changed(original: &Profile, raw: &[u8]) -> bool { original.raw != raw }

/// LED の効果の選択肢（「消灯」は 色 000000 ＋ 点灯 で表す）
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LedChoice { Off, Effect(LedEffect) }

/// 今の色と効果 → 画面に出す選択肢
pub fn led_choice(rgb: [u8; 3], effect: LedEffect) -> LedChoice {
    if rgb == [0, 0, 0] && effect == LedEffect::Solid { LedChoice::Off } else { LedChoice::Effect(effect) }
}

/// 選択肢を選んだときの (色, 効果)。消灯なら黒・点灯。ほかの効果を選んだとき、今が黒なら覚えておいた色（無ければ白）に戻す
pub fn apply_led_choice(choice: LedChoice, current: [u8; 3], remembered: Option<[u8; 3]>) -> ([u8; 3], LedEffect) {
    match choice {
        LedChoice::Off => ([0, 0, 0], LedEffect::Solid),
        LedChoice::Effect(effect) => (if current == [0, 0, 0] { remembered.unwrap_or([0xFF, 0xFF, 0xFF]) } else { current }, effect),
    }
}

pub fn diff_summary(before: &Profile, after: &Profile) -> Vec<String> {
    let mut lines = Vec::new();
    macro_rules! diff { ($name:expr, $a:expr, $b:expr) => { if $a != $b { lines.push(format!("{}: {:?} → {:?}", $name, $a, $b)); } }; }
    diff!("LED色", before.led_rgb, after.led_rgb);
    diff!("LED効果", before.led_effect, after.led_effect);
    diff!("LED秒数", before.led_seconds, after.led_seconds);
    diff!("GシフトLED色", before.gshift_rgb, after.gshift_rgb);
    diff!("ポーリングレート", before.poll_hz, after.poll_hz);
    diff!("既定DPIスロット", before.dpi_default, after.dpi_default);
    diff!("DPIシフト", before.dpi_shift, after.dpi_shift);
    for i in 0..4 { diff!(format!("DPI{}", i + 1), before.dpi[i], after.dpi[i]); }
    for i in 0..g600ctl::protocol::BUTTONS {
        diff!(format!("{} 通常", g600ctl::protocol::button_label(i)), before.buttons[i].describe(), after.buttons[i].describe());
        diff!(format!("{} Gシフト", g600ctl::protocol::button_label(i)), before.gshift_buttons[i].describe(), after.gshift_buttons[i].describe());
    }
    lines
}

pub fn keys() -> Vec<(u8, String)> {
    (1..=u8::MAX).filter_map(|usage| {
        let name = key_name(usage);
        (!name.starts_with("0x")).then_some((usage, name))
    }).collect()
}


/// Windows のショートカット（G600 に保存できる「キー1つ＋修飾キー」で表せるもの）
pub fn windows_shortcuts() -> Vec<(&'static str, Action)> {
    to_actions(&[
        ("コピー", "ctrl+c"), ("貼り付け", "ctrl+v"), ("切り取り", "ctrl+x"), ("元に戻す", "ctrl+z"),
        ("やり直し", "ctrl+y"), ("すべて選択", "ctrl+a"), ("保存", "ctrl+s"), ("検索", "ctrl+f"),
        ("印刷", "ctrl+p"), ("新規作成", "ctrl+n"), ("開く", "ctrl+o"), ("名前の変更", "f2"),
        ("最新の情報に更新", "f5"), ("削除", "delete"), ("完全に削除", "shift+delete"), ("プロパティ", "alt+enter"),
        ("デスクトップ表示", "win+d"), ("すべて最小化", "win+m"), ("エクスプローラー", "win+e"), ("設定", "win+i"),
        ("ロック", "win+l"), ("ファイル名を指定して実行", "win+r"), ("Windows 検索", "win+s"), ("クイックリンク", "win+x"),
        ("通知", "win+n"), ("クリップボード履歴", "win+v"), ("絵文字", "win+."), ("音声入力", "win+h"),
        ("ゲームバー", "win+g"), ("タスクビュー", "win+tab"), ("ウィンドウ切替", "alt+tab"), ("ウィンドウを閉じる", "alt+f4"),
        ("最大化", "win+up"), ("最小化・元に戻す", "win+down"), ("左にスナップ", "win+left"), ("右にスナップ", "win+right"),
        ("左のモニターへ移動", "win+shift+left"), ("右のモニターへ移動", "win+shift+right"),
        ("仮想デスクトップを追加", "win+ctrl+d"), ("左の仮想デスクトップ", "win+ctrl+left"),
        ("右の仮想デスクトップ", "win+ctrl+right"), ("仮想デスクトップを閉じる", "win+ctrl+f4"),
        ("スクリーンショット（範囲）", "win+shift+s"), ("画面全体を保存", "win+printscreen"),
        ("タスクマネージャー", "ctrl+shift+esc"),
    ])
}

/// Google Chrome のショートカット
pub fn chrome_shortcuts() -> Vec<(&'static str, Action)> {
    to_actions(&[
        ("新しいタブ", "ctrl+t"), ("タブを閉じる", "ctrl+w"), ("閉じたタブを開く", "ctrl+shift+t"), ("新しいウィンドウ", "ctrl+n"),
        ("シークレットウィンドウ", "ctrl+shift+n"), ("ウィンドウを閉じる", "ctrl+shift+w"), ("次のタブ", "ctrl+tab"), ("前のタブ", "ctrl+shift+tab"),
        ("タブ1", "ctrl+1"), ("タブ2", "ctrl+2"), ("タブ3", "ctrl+3"), ("タブ4", "ctrl+4"),
        ("タブ5", "ctrl+5"), ("タブ6", "ctrl+6"), ("タブ7", "ctrl+7"), ("タブ8", "ctrl+8"), ("最後のタブ", "ctrl+9"),
        ("再読み込み", "f5"), ("強制再読み込み", "ctrl+shift+r"), ("戻る", "alt+left"), ("進む", "alt+right"), ("ホーム", "alt+home"),
        ("アドレスバー", "ctrl+l"), ("ページ内検索", "ctrl+f"), ("ブックマークに追加", "ctrl+d"), ("ブックマークバー表示", "ctrl+shift+b"),
        ("ブックマークマネージャー", "ctrl+shift+o"), ("履歴", "ctrl+h"), ("ダウンロード", "ctrl+j"), ("閲覧履歴を削除", "ctrl+shift+delete"),
        ("開発者ツール", "f12"), ("ページのソース", "ctrl+u"), ("拡大", "ctrl+numplus"), ("縮小", "ctrl+minus"),
        ("等倍に戻す", "ctrl+0"), ("全画面", "f11"), ("ページを保存", "ctrl+s"), ("印刷", "ctrl+p"),
        ("Chrome のタスクマネージャー", "shift+esc"), ("プロフィール切替", "ctrl+shift+m"),
    ])
}

/// キーの検索。英語のキー名（大文字小文字を区別しない）でも、日本語の別名でも引ける。
/// keys は (usage, キー名) の一覧（keys() の戻り値）。一致したものを一覧の順で返す。
pub fn search_keys(query: &str, keys: &[(u8, String)]) -> Vec<(u8, String)> {
    let q = query.trim().to_lowercase();
    if q.is_empty() { return Vec::new(); }
    let is_fn = |n: &str| n.len() >= 2 && n.starts_with('F') && n[1..].chars().all(|c| c.is_ascii_digit());
    // 日本語の別名 → そのキーかどうか
    let aliases: [(&[&str], &dyn Fn(&str) -> bool); 18] = [
        (&["エンター", "改行", "えんたー"], &|n| n == "Enter" || n == "NumEnter"),
        (&["テンキー", "てんきー", "数字パッド"], &|n| n.starts_with("Num")),
        (&["矢印", "やじるし", "カーソルキー"], &|n| matches!(n, "Up" | "Down" | "Left" | "Right")),
        (&["上", "うえ"], &|n| n == "Up"),
        (&["下", "した"], &|n| n == "Down"),
        (&["左", "ひだり"], &|n| n == "Left"),
        (&["右", "みぎ"], &|n| n == "Right"),
        (&["スペース", "空白"], &|n| n == "Space"),
        (&["タブ"], &|n| n == "Tab"),
        (&["エスケープ", "エスケ"], &|n| n == "Esc"),
        (&["バックスペース", "後退"], &|n| n == "Backspace"),
        (&["削除", "デリート"], &|n| n == "Delete"),
        (&["挿入", "インサート"], &|n| n == "Insert"),
        (&["ホーム", "エンド", "先頭", "末尾"], &|n| n == "Home" || n == "End"),
        (&["ページ"], &|n| n == "PageUp" || n == "PageDown"),
        (&["ファンクション", "ふぁんくしょん"], &|n| n.len() >= 2 && n.starts_with('F') && n[1..].chars().all(|c| c.is_ascii_digit())),
        (&["プリントスクリーン", "スクショ", "画面印刷"], &|n| n == "PrintScreen"),
        (&["数字"], &|n| n.len() == 1 && n.chars().all(|c| c.is_ascii_digit())),
    ];
    keys.iter().filter(|(_, name)| {
        let lower = name.to_lowercase();
        // 「f1」と打ったら F1 だけ（F10〜F19 は出さない）、それ以外は部分一致
        let by_name = if is_fn(name) && q.starts_with('f') && q[1..].chars().all(|c| c.is_ascii_digit()) && q.len() > 1 { lower == q } else { lower.contains(&q) };
        by_name || aliases.iter().any(|(words, pred)| words.iter().any(|w| w.contains(q.as_str()) || q.contains(*w)) && pred(name))
    }).cloned().collect()
}

/// 「自分で登録」の名前欄: 空か、前回自動で入れた名前のままなら、新しい登録内容に書き替える。
/// ユーザーが別の名前を書いていればそのまま（全部消して空にすると、また自動に戻る）。
pub fn auto_name(current: &str, last_auto: &str, new_auto: &str) -> String {
    if current.trim().is_empty() || current == last_auto { new_auto.to_string() } else { current.to_string() }
}

/// 「自分で登録」: 同じ組み合わせがあればその位置を返し（追加しない）、無ければ末尾に追加する。戻り値は (位置, 追加したか)
pub fn register_custom(list: &mut Vec<(String, Action)>, name: &str, action: Action) -> (usize, bool) {
    if let Some(i) = list.iter().position(|(_, a)| *a == action) { return (i, false); }
    list.push((name.trim().to_string(), action));
    (list.len() - 1, true)
}

fn to_actions(list: &[(&'static str, &str)]) -> Vec<(&'static str, Action)> {
    list.iter().map(|&(name, value)| (name, Action::from_str(value).unwrap())).collect()
}

#[cfg(test)]
pub fn presets() -> Vec<(&'static str, Action)> {
    [
        ("コピー", "ctrl+c"), ("貼り付け", "ctrl+v"), ("切り取り", "ctrl+x"),
        ("元に戻す", "ctrl+z"), ("やり直し", "ctrl+y"), ("すべて選択", "ctrl+a"),
        ("保存", "ctrl+s"), ("検索", "ctrl+f"), ("タブを閉じる", "ctrl+w"),
        ("新しいタブ", "ctrl+t"), ("閉じたタブを開く", "ctrl+shift+t"),
        ("次のタブ", "ctrl+tab"), ("前のタブ", "ctrl+shift+tab"),
        ("ウィンドウ切替", "alt+tab"), ("ウィンドウを閉じる", "alt+f4"),
        ("デスクトップ表示", "win+d"), ("エクスプローラー", "win+e"),
        ("スクリーンショット", "win+shift+s"), ("タスクマネージャー", "ctrl+shift+esc"),
        ("ロック", "win+l"), ("更新", "f5"), ("全画面", "f11"),
    ].into_iter().map(|(name, value)| (name, Action::from_str(value).unwrap())).collect()
}

pub fn parse_action(value: &str) -> Result<Action, String> {
    let action = Action::from_str(value)?;
    if value.get(..4).is_some_and(|prefix| prefix.eq_ignore_ascii_case("raw:")) {
        let raw = &value[4..];
        let bytes: Vec<u8> = raw.split(':').map(|v| u8::from_str_radix(v, 16).unwrap()).collect();
        if action.encode() != bytes.as_slice() {
            return Ok(Action::Unknown { code: bytes[0], modifiers: bytes[1], usage: bytes[2] });
        }
    }
    Ok(action)
}

pub fn parse_custom(text: &str) -> (Vec<(String, Action)>, usize) {
    let mut entries = Vec::new();
    let mut invalid = 0;
    for line in text.lines() {
        let entry = line.split_once('\t').and_then(|(name, value)| {
            if name.trim().is_empty() || name.contains('\t') { return None; }
            parse_action(value).ok().map(|action| (name.to_string(), action))
        });
        if let Some(entry) = entry { entries.push(entry); } else { invalid += 1; }
    }
    (entries, invalid)
}

pub fn parse_custom_bytes(bytes: &[u8]) -> (Vec<(String, Action)>, usize) {
    let mut entries = Vec::new();
    let mut invalid = 0;
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    if bytes.is_empty() { return (entries, invalid); }
    for line in bytes.split(|b| *b == b'\n') {
        match std::str::from_utf8(line) {
            Ok(line) => { let line = line.trim_end_matches('\r'); if line.is_empty() { invalid += 1; } else { let (mut parsed, count) = parse_custom(line); entries.append(&mut parsed); invalid += count; } }
            Err(_) => invalid += 1,
        }
    }
    (entries, invalid)
}

pub fn format_custom(entries: &[(String, Action)]) -> String {
    entries.iter().map(|(name, action)| {
        let [a, b, c] = action.encode();
        format!("{name}\traw:{a:02x}:{b:02x}:{c:02x}\n")
    }).collect()
}

// egui の Key::name() を HID usage に変換する（egui で区別できない記号キーは None）
#[cfg(test)]
pub fn key_usage_for_event(name: &str) -> Option<u8> {
    let name = match name {
        "Escape" => "Esc", "Comma" => ",", "Backslash" => "\\", "Slash" => "/",
        "OpenBracket" => "[", "CloseBracket" => "]", "Backtick" => "`",
        "Minus" => "-", "Period" => ".", "Equals" => "=", "Semicolon" => ";", "Quote" => "'",
        "Colon" | "Pipe" | "Questionmark" | "Exclamationmark" | "OpenCurlyBracket" | "CloseCurlyBracket" | "Plus" => return None,
        other => other,
    };
    (1..=u8::MAX).find(|&usage| { let n = key_name(usage); !n.starts_with("0x") && n.eq_ignore_ascii_case(name) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use g600ctl::protocol::{PROFILE_LEN, REPORT_PROFILES};
    #[test]
    fn edits_preserve_other_bytes_and_detect_revert() {
        let mut raw = vec![0; PROFILE_LEN]; raw[0] = REPORT_PROFILES[0]; raw[7] = 0xab;
        raw[31..34].copy_from_slice(&[0x16, 2, 3]);
        let original = Profile::parse(&raw).unwrap();
        apply(&mut raw, Change::Dpi([400, 800, 0, 0], 2)).unwrap();
        apply(&mut raw, Change::Button(1, true, Action::Key { modifiers: 1, usage: 4 })).unwrap();
        assert!(changed(&original, &raw));
        assert_eq!(&raw[31..34], &[0x16, 2, 3]); assert_eq!(raw[7], 0xab);
        let p = Profile::parse(&raw).unwrap();
        assert_eq!(p.dpi, [400, 800, 0, 0]); assert_eq!(p.gshift_buttons[1], Action::Key { modifiers: 1, usage: 4 });
        raw = original.raw.clone(); assert!(!changed(&original, &raw));
    }
    #[test]
    fn summary_lists_changes() {
        let mut raw = vec![0; PROFILE_LEN]; raw[0] = REPORT_PROFILES[0];
        let before = Profile::parse(&raw).unwrap();
        apply(&mut raw, Change::Poll(500)).unwrap();
        apply(&mut raw, Change::DpiShift(800)).unwrap();
        let after = Profile::parse(&raw).unwrap();
        assert_eq!(diff_summary(&before, &after), vec!["ポーリングレート: 1000 → 500", "DPIシフト: 0 → 800"]);
        assert!(diff_summary(&before, &before).is_empty());
    }
    #[test]
    fn presets_and_custom_roundtrip() {
        let p = presets();
        assert_eq!(p.len(), 22);
        assert_eq!(p[0].1, Action::from_str("ctrl+c").unwrap());
        assert_eq!(p[0].0, "コピー");
        assert!(p.iter().all(|(name, _)| !name.contains('?')));
        let entries = vec![("すべて保存".to_string(), Action::from_str("ctrl+shift+a").unwrap()), ("raw".into(), Action::from_str("raw:16:01:02").unwrap())];
        assert_eq!(parse_custom(&format_custom(&entries)), (entries, 0));
        assert_eq!(parse_action("raw:01:02:03").unwrap().encode(), [1, 2, 3]);
        assert_eq!(parse_custom_bytes(b"good\tctrl+a\n\xff\n").1, 1);
        assert_eq!(parse_custom("bad\n\tctrl+a\n名前\tbad-key\n").1, 3);
    }
    #[test]
    fn led_off_and_back() {
        assert_eq!(led_choice([0, 0, 0], LedEffect::Solid), LedChoice::Off);
        assert_eq!(led_choice([0, 0, 0], LedEffect::Breathe), LedChoice::Effect(LedEffect::Breathe));
        assert_eq!(led_choice([255, 0, 0], LedEffect::Solid), LedChoice::Effect(LedEffect::Solid));
        assert_eq!(apply_led_choice(LedChoice::Off, [255, 0, 0], None), ([0, 0, 0], LedEffect::Solid));
        assert_eq!(apply_led_choice(LedChoice::Effect(LedEffect::Cycle), [0, 0, 0], Some([255, 0, 0])), ([255, 0, 0], LedEffect::Cycle)); // 覚えていた色に戻す
        assert_eq!(apply_led_choice(LedChoice::Effect(LedEffect::Solid), [0, 0, 0], None), ([255, 255, 255], LedEffect::Solid)); // 覚えていなければ白
        assert_eq!(apply_led_choice(LedChoice::Effect(LedEffect::Breathe), [0, 255, 0], Some([1, 2, 3])), ([0, 255, 0], LedEffect::Breathe)); // 黒でなければそのまま
    }
    #[test]
    fn auto_name_follows_until_user_edits() {
        assert_eq!(auto_name("", "", "LWin+F6"), "LWin+F6");              // 開いた直後（空）
        assert_eq!(auto_name("LWin+F6", "LWin+F6", "LWin+F7"), "LWin+F7"); // 自動のまま → 追従
        assert_eq!(auto_name("スキル1", "LWin+F7", "LCtrl+A"), "スキル1"); // 書き換えた → そのまま
        assert_eq!(auto_name("", "LWin+F7", "LCtrl+A"), "LCtrl+A");       // 全部消した → 自動に戻る
        assert_eq!(auto_name("LWin+F6", "LWin+F6", ""), "");              // 組み合わせが空になった
    }
    #[test]
    fn register_custom_avoids_duplicates() {
        let mut list = Vec::new();
        let a = Action::from_str("ctrl+f1").unwrap();
        assert_eq!(register_custom(&mut list, " Ctrl+F1 ", a), (0, true));
        assert_eq!(list[0].0, "Ctrl+F1");
        assert_eq!(register_custom(&mut list, "別の名前", a), (0, false));
        assert_eq!(list.len(), 1);
        assert_eq!(register_custom(&mut list, "F2", Action::from_str("f2").unwrap()), (1, true));
    }
    #[test]
    fn key_search_by_english_and_japanese() {
        let k = keys();
        let names = |q: &str| search_keys(q, &k).into_iter().map(|(_, n)| n).collect::<Vec<_>>();
        assert_eq!(names("F1"), vec!["F1"]);
        assert_eq!(names("f12"), vec!["F12"]);
        assert!(names("enter").contains(&"Enter".to_string()) && names("enter").contains(&"NumEnter".to_string()));
        assert_eq!(names("エンター"), vec!["Enter", "NumEnter"]);
        assert!(names("テンキー").iter().all(|n| n.starts_with("Num")) && names("テンキー").len() >= 15);
        assert_eq!(names("矢印").len(), 4);
        assert_eq!(names("上"), vec!["Up"]);
        assert_eq!(names("ファンクション").len(), 24);
        assert!(names("").is_empty());
        assert!(names("存在しないキー").is_empty());
    }
    #[test]
    fn shortcut_lists_are_single_key_combos() {
        let w = windows_shortcuts(); let c = chrome_shortcuts();
        assert!(w.len() >= 40 && c.len() >= 35);
        for (name, action) in w.iter().chain(c.iter()) {
            assert!(matches!(action, Action::Key { usage, .. } if *usage != 0), "{name}");
            assert!(!name.contains('?'));
        }
        assert_eq!(w[0].1, Action::from_str("ctrl+c").unwrap());
        assert_eq!(c.iter().find(|(n, _)| *n == "閉じたタブを開く").unwrap().1, Action::from_str("ctrl+shift+t").unwrap());
    }
    #[test]
    fn event_key_mapping() {
        assert_eq!(key_usage_for_event("A"), Some(4));
        assert_eq!(key_usage_for_event("Escape"), Some(0x29));
        assert_eq!(key_usage_for_event("Comma"), Some(0x36));
        assert_eq!(key_usage_for_event("F24"), Some(0x73));
        assert_eq!(key_usage_for_event("F25"), None);
        assert_eq!(key_usage_for_event("Copy"), None);
    }
    #[test]
    fn key_list_contains_only_named_usages() {
        let list = keys(); assert!(list.contains(&(4, "A".into())));
        assert!(list.iter().all(|(u, name)| *name == key_name(*u) && !name.starts_with("0x")));
        assert!(!list.iter().any(|(u, _)| *u == 0));
    }
}

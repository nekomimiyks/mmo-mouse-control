//! Presentation-only translations. Device data and saved user names stay unchanged.
use std::sync::{atomic::{AtomicUsize, Ordering}, OnceLock};

pub const LANGUAGES: &[(&str, &str)] = &[
    ("auto", "自動 / Windows"), ("ja", "日本語"), ("en", "English"),
    ("zh-Hans", "简体中文"), ("zh-Hant", "繁體中文"), ("ko", "한국어"),
    ("es", "Español"), ("fr", "Français"), ("de", "Deutsch"), ("pt", "Português"),
];
static CURRENT: AtomicUsize = AtomicUsize::new(1);

pub fn resolve(tag: &str) -> usize {
    let tag = tag.replace('_', "-").to_ascii_lowercase();
    if tag.starts_with("zh") {
        if tag.contains("hant") || tag.ends_with("-tw") || tag.ends_with("-hk") || tag.ends_with("-mo") { 4 } else { 3 }
    } else {
        LANGUAGES.iter().position(|(code, _)| *code != "auto" && tag.split('-').next() == Some(code)).unwrap_or(2)
    }
}

#[cfg(windows)]
fn windows_language() -> String {
    #[link(name = "kernel32")]
    extern "system" { fn GetUserPreferredUILanguages(flags: u32, count: *mut u32, buffer: *mut u16, size: *mut u32) -> i32; }
    let mut count = 0; let mut size = 0;
    unsafe { GetUserPreferredUILanguages(8, &mut count, std::ptr::null_mut(), &mut size); }
    if size == 0 || size > 65536 { return "en".into(); }
    let mut buffer = vec![0u16; size as usize];
    if unsafe { GetUserPreferredUILanguages(8, &mut count, buffer.as_mut_ptr(), &mut size) } == 0 { return "en".into(); }
    String::from_utf16_lossy(buffer.split(|c| *c == 0).next().unwrap_or_default())
}
#[cfg(not(windows))]
fn windows_language() -> String { std::env::var("LANG").unwrap_or_else(|_| "en".into()) }

pub fn initialize() -> String {
    let code = g600ctl::storage::file("language.txt").map(|p| load_selection(&p)).unwrap_or_else(|_| "auto".into());
    activate(&code);
    code
}
fn load_selection(path: &std::path::Path) -> String {
    let value = std::fs::read_to_string(path).unwrap_or_else(|_| "auto".into());
    let code = value.trim();
    if LANGUAGES.iter().any(|(c, _)| *c == code) { code.to_string() } else { "auto".into() }
}
pub fn activate(code: &str) {
    CURRENT.store(if code == "auto" { resolve(&windows_language()) } else { resolve(code) }, Ordering::Relaxed);
}
pub fn save(code: &str) -> Result<(), String> {
    g600ctl::storage::write_atomic(&g600ctl::storage::file("language.txt")?, code.as_bytes()).map_err(|e| format!("保存できません: {e}"))?;
    activate(code);
    Ok(())
}

struct Entry { source: String, translations: Vec<String> }
fn catalog() -> &'static Vec<Entry> {
    static ENTRIES: OnceLock<Vec<Entry>> = OnceLock::new();
    ENTRIES.get_or_init(|| {
        let mut entries: Vec<_> = include_str!("../../locales/messages.tsv").lines().filter(|l| !l.is_empty() && !l.starts_with('#')).map(|l| {
        let mut fields = l.split('\t');
        Entry { source: fields.next().unwrap().replace("\\n", "\n"), translations: fields.map(|v| v.replace("\\n", "\n")).collect() }
        }).collect();
        entries.sort_by_key(|e| std::cmp::Reverse(e.source.replace("{}", "").len()));
        entries
    })
}

pub fn tr(text: impl AsRef<str>) -> String { translate(text.as_ref(), CURRENT.load(Ordering::Relaxed), 0) }

fn translate(text: &str, language: usize, depth: usize) -> String {
    if language == 1 || depth > 6 || text.is_empty() { return text.to_string(); }
    if let Some(inner) = text.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        return format!("\"{}\"", translate(inner, language, depth + 1));
    }
    let column = language.saturating_sub(2);
    for entry in catalog() {
        if entry.source == text { return entry.translations.get(column).unwrap_or(&entry.translations[0]).to_string(); }
    }
    for prefix in ["▼ ", "▶ ", "✓ "] {
        if let Some(rest) = text.strip_prefix(prefix) { return format!("{prefix}{}", translate(rest, language, depth + 1)); }
    }
    // Match complete format templates, never replace fragments in paths or user text.
    for entry in catalog().iter().filter(|e| e.source.contains("{}")) {
        if let Some(values) = captures(&entry.source, text) {
            let target = entry.translations.get(column).unwrap_or(&entry.translations[0]);
            let mut result = String::new();
            let mut pieces = target.split("{}");
            result.push_str(pieces.next().unwrap_or_default());
            for (value, suffix) in values.iter().zip(pieces) {
                result.push_str(&translate(value, language, depth + 1));
                result.push_str(suffix);
            }
            return result;
        }
    }
    if text.contains('+') {
        return text.split('+').map(|part| translate(part, language, depth + 1)).collect::<Vec<_>>().join("+");
    }
    if text.contains(", ") {
        return text.split(", ").map(|part| translate(part, language, depth + 1)).collect::<Vec<_>>().join(", ");
    }
    if let Some((number, label)) = text.split_once(' ') {
        if number.parse::<u8>().is_ok_and(|n| (1..=20).contains(&n)) {
            return format!("{number} {}", translate(label, language, depth + 1));
        }
    }
    text.to_string()
}

fn captures<'a>(template: &str, text: &'a str) -> Option<Vec<&'a str>> {
    let parts: Vec<_> = template.split("{}").collect();
    let mut rest = text.strip_prefix(parts[0])?;
    let mut values = Vec::new();
    for (i, part) in parts.iter().enumerate().skip(1) {
        if i == parts.len() - 1 {
            values.push(rest.strip_suffix(part)?);
        } else {
            let position = rest.find(part)?;
            values.push(&rest[..position]);
            rest = &rest[position + part.len()..];
        }
    }
    Some(values)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resolves_windows_language_variants() {
        for (tag, expected) in [("ja-JP",1),("en-GB",2),("zh-CN",3),("zh-TW",4),("zh-HK",4),("zh-Hant",4),("ko-KR",5),("es-MX",6),("fr-CA",7),("de-DE",8),("pt-BR",9),("it-IT",2)] { assert_eq!(resolve(tag), expected); }
    }
    #[test]
    fn language_selection_survives_reload_and_invalid_values_use_auto() {
        let base = std::env::temp_dir().join(format!("g600-language-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap(); let path = base.join("language.txt");
        assert_eq!(load_selection(&path), "auto");
        for (code, _) in LANGUAGES {
            g600ctl::storage::write_atomic(&path, code.as_bytes()).unwrap();
            assert_eq!(load_selection(&path), *code);
        }
        g600ctl::storage::write_atomic(&path, b"invalid").unwrap();
        assert_eq!(load_selection(&path), "auto");
        std::fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn every_translation_has_the_same_placeholders() {
        let mut seen = std::collections::HashSet::new();
        for e in catalog() {
            assert!(seen.insert(&e.source), "duplicate: {}", e.source);
            assert_eq!(e.translations.len(), 8, "{}", e.source);
            for t in &e.translations {
                assert!(!t.is_empty(), "{}", e.source);
                assert_eq!(e.source.matches("{}").count(), t.matches("{}").count(), "{} -> {}", e.source, t);
            }
        }
    }
    #[test]
    fn templates_preserve_paths_and_user_names() {
        assert_eq!(translate("プロファイル2", 2, 0), "Profile 2");
        assert_eq!(translate("バックアップ: C:\\名前\\backup.txt", 2, 0), "Backup: C:\\名前\\backup.txt");
        assert_eq!(translate("my custom name", 5, 0), "my custom name");
        assert_eq!(translate("プロファイル2・使用中", 2, 0), "Profile 2 · active");
        assert_eq!(translate("使用中: プロファイル2 / DPIスロット1", 2, 0), "Active: profile 2 / DPI slot 1");
        for language in 2..=9 {
            assert_ne!(translate("キーコンフィグ", language, 0), "キーコンフィグ");
            assert_ne!(translate("自分で登録", language, 0), "自分で登録");
            assert_ne!(translate("読み込み失敗: HIDの初期化に失敗: test", language, 0), "読み込み失敗: HIDの初期化に失敗: test");
        }
    }
}

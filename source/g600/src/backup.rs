use hidapi::HidDevice;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::protocol::*;

pub fn save_settings(settings: &crate::device::ReadSettings, path: &std::path::Path) -> Result<(), String> {
    let mut out = String::from("# g600ctl backup v1\n");
    out += &format!("{}\n", hex(&settings.active_raw));
    for profile in &settings.profiles {
        out += &format!("{}\n", hex(&profile.raw));
    }
    crate::storage::write_atomic(path, out.as_bytes()).map_err(|e| format!("{} に保存できません: {e}", path.display()))
}

pub fn read_test_profile(path: &str) -> Result<Vec<u8>, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path} を読めません: {e}"))?;
    let mut profile = None;
    for line in text.lines().skip(3).map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let bytes = parse_hex_line(line)?;
        if bytes.first() == Some(&REPORT_PROFILES[2]) {
            if bytes.len() != PROFILE_LEN || profile.is_some() {
                return Err("F5 のデータが不正または重複しています".into());
            }
            profile = Some(bytes);
        }
    }
    profile.ok_or("F5 のデータが見つかりません".into())
}

pub fn read_backup(path: &str) -> Result<(Active, Vec<Vec<u8>>), String> {
    let (active, profiles) = read_backup_profiles(path)?;
    Ok((active, profiles.into_iter().map(|p| p.raw).collect()))
}

pub fn read_backup_profiles(path: &str) -> Result<(Active, Vec<Profile>), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{path} を読めません: {e}"))?;
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#'));
    let active = Active::parse(&parse_hex_line(lines.next().ok_or("バックアップが空です")?)?)?;
    let mut profiles = Vec::new();
    for &id in &REPORT_PROFILES {
        let b = parse_hex_line(lines.next().ok_or("プロファイルのデータが足りません")?)?;
        if b.len() != PROFILE_LEN || b[0] != id {
            return Err(format!("プロファイル 0x{id:02X} のデータが不正です"));
        }
        profiles.push(Profile::parse(&b)?);
    }
    Ok((active, profiles))
}

pub fn auto_backup(dev: &HidDevice) -> Result<(), String> {
    let dir = crate::storage::backup_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("バックアップフォルダを作れません: {e}"))?;
    let path = new_path(&dir, "auto");
    backup(dev, &path.to_string_lossy())
}

pub fn backup(dev: &HidDevice, path: &str) -> Result<(), String> {
    let settings = crate::device::read_settings(dev)?;
    save_settings(&settings, Path::new(path))?;
    println!("バックアップを保存しました: {path}");
    Ok(())
}

pub fn new_path(dir: &Path, kind: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
    dir.join(format!("{kind}-{}-{:09}-{}-{sequence}.txt", timestamp(), now.subsec_nanos(), std::process::id()))
}

/// Local-ish timestamp (JST, the user's zone) like 20261111-231500, no extra deps.
pub fn timestamp() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
        + 9 * 3600;
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // civil-from-days (Howard Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_backups_do_not_reuse_a_path() {
        let paths: std::collections::HashSet<_> =
            (0..128).map(|_| new_path(Path::new("backups"), "auto")).collect();
        assert_eq!(paths.len(), 128);
    }
}

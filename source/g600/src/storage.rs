//! All mutable data lives outside the application installation directory.
use std::path::{Path, PathBuf};
use std::io::Write;

pub fn data_path() -> Result<PathBuf, String> {
    let root = std::env::var_os("LOCALAPPDATA").filter(|p| !p.is_empty()).map(PathBuf::from)
        .or_else(|| std::env::var_os("XDG_DATA_HOME").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")))
        .ok_or("ユーザーデータの保存先が見つかりません")?;
    Ok(root.join("G600rControl"))
}

pub fn data_dir() -> Result<PathBuf, String> {
    let path = data_path()?;
    std::fs::create_dir_all(&path).map_err(|e| format!("ユーザーデータの保存先を作れません: {e}"))?;
    Ok(path)
}

pub fn file(name: &str) -> Result<PathBuf, String> { Ok(data_dir()?.join(name)) }

/// Publish complete settings in one rename, leaving the previous file intact on failure.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let temporary = path.with_extension(format!("tmp-{}-{}", std::process::id(), NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let result = (|| {
        let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        output.write_all(bytes)?; output.sync_all()?; drop(output);
        std::fs::rename(&temporary, path)
    })();
    if result.is_err() { let _ = std::fs::remove_file(&temporary); }
    result
}

pub fn backup_dir() -> Result<PathBuf, String> {
    let path = data_dir()?.join("backups");
    std::fs::create_dir_all(&path).map_err(|e| format!("バックアップフォルダを作れません: {e}"))?;
    Ok(path)
}

/// Copy portable-version data once. Never overwrite existing new-version data,
/// remove the originals, follow directory links, or copy executables.
pub fn migrate_from(legacy: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(destination)?;
    let marker = destination.join("migration-v1.done");
    if marker.exists() { return Ok(()); }
    if !legacy.join("g600-custom.txt").is_file() && !legacy.join("backups").is_dir() { return Ok(()); }
    copy_if_missing(&legacy.join("g600-custom.txt"), &destination.join("g600-custom.txt"))?;
    let backups = legacy.join("backups");
    if backups.is_dir() && !backups.symlink_metadata()?.file_type().is_symlink() {
        for entry in std::fs::read_dir(backups)? {
            let entry = entry?;
            if entry.file_type()?.is_file() && entry.path().extension().is_some_and(|e| e == "txt") {
                std::fs::create_dir_all(destination.join("backups"))?;
                copy_if_missing(&entry.path(), &destination.join("backups").join(entry.file_name()))?;
            }
        }
    }
    std::fs::write(marker, b"1")
}

fn copy_if_missing(source: &Path, destination: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(source) {
        Ok(m) if m.is_file() && !m.file_type().is_symlink() => {
            match std::fs::OpenOptions::new().write(true).create_new(true).open(destination) {
                Ok(mut output) => { let mut input = std::fs::File::open(source)?; std::io::copy(&mut input, &mut output)?; }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {},
                Err(e) => return Err(e),
            }
            Ok(())
        }
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

pub fn initialize() -> Result<(), String> {
    let destination = data_dir()?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if let Some(parent) = exe.parent() {
        let migrate = |source: &Path| {
            migrate_from(source, &destination).map_err(|e| format!("以前の設定を引き継げません: {e}"))
        };
        migrate(parent)?;
        if let Some(workspace) = parent.parent().and_then(Path::parent) {
            migrate(&workspace.join("G600r制御ソフト").join("実行ファイル"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn migration_preserves_existing_data_and_does_not_repeat() {
        let base = std::env::temp_dir().join(format!("g600-migrate-{}", std::process::id()));
        let old = base.join("old"); let new = base.join("new");
        std::fs::create_dir_all(old.join("backups")).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(old.join("g600-custom.txt"), "old").unwrap();
        std::fs::write(new.join("g600-custom.txt"), "new").unwrap();
        std::fs::write(old.join("backups/a.txt"), "backup").unwrap();
        std::fs::write(old.join("backups/a.exe"), "excluded").unwrap();
        migrate_from(&old, &new).unwrap();
        assert_eq!(std::fs::read_to_string(new.join("g600-custom.txt")).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(new.join("backups/a.txt")).unwrap(), "backup");
        assert!(!new.join("backups/a.exe").exists());
        std::fs::write(old.join("backups/b.txt"), "later").unwrap();
        migrate_from(&old, &new).unwrap();
        assert!(!new.join("backups/b.txt").exists());
        assert!(old.join("backups/a.txt").exists());
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn missing_legacy_data_does_not_block_later_migration() {
        let base = std::env::temp_dir().join(format!("g600-later-{}", std::process::id()));
        let old = base.join("old"); let new = base.join("new");
        std::fs::create_dir_all(&old).unwrap();
        migrate_from(&old, &new).unwrap();
        assert!(!new.join("migration-v1.done").exists());
        std::fs::write(old.join("g600-custom.txt"), "保存\tctrl+s\n").unwrap();
        migrate_from(&old, &new).unwrap();
        assert_eq!(std::fs::read(new.join("g600-custom.txt")).unwrap(), std::fs::read(old.join("g600-custom.txt")).unwrap());
        std::fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn atomic_settings_replace_and_failure_cleanup() {
        let base = std::env::temp_dir().join(format!("g600-atomic-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let path = base.join("language.txt");
        write_atomic(&path, b"en").unwrap(); write_atomic(&path, b"ko").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"ko");
        let blocked = base.join("blocked"); std::fs::create_dir_all(&blocked).unwrap();
        assert!(write_atomic(&blocked, b"fr").is_err());
        assert!(blocked.is_dir());
        assert_eq!(std::fs::read_dir(&base).unwrap().count(), 2);
        std::fs::remove_dir_all(base).unwrap();
    }
}

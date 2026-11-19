use std::path::{Path, PathBuf};
use hidapi::HidDevice;
use crate::{backup, device};

pub trait ProfileDevice {
    fn read_settings(&self) -> Result<device::ReadSettings, String>;
    fn write_profile(&self, index: u8, data: &[u8]) -> Result<(), String>;
}

impl ProfileDevice for HidDevice {
    fn read_settings(&self) -> Result<device::ReadSettings, String> { device::read_settings(self) }
    fn write_profile(&self, index: u8, data: &[u8]) -> Result<(), String> { device::write_profile(self, index, data) }
}

#[derive(Debug)]
pub struct WriteError { pub backup: Option<PathBuf>, pub message: String }

pub fn write_profiles<D: ProfileDevice>(dev: &D, profiles: &[(u8, Vec<u8>)], dir: &Path, mut progress: impl FnMut(Option<u8>, &Path)) -> Result<PathBuf, WriteError> {
    let settings = dev.read_settings().map_err(without_backup)?;
    let path = save_before_write(&settings, dir)?;
    progress(None, &path);
    for (index, data) in profiles {
        dev.write_profile(*index, data).map_err(|message| WriteError { backup: Some(path.clone()), message })?;
        progress(Some(*index), &path);
    }
    Ok(path)
}

fn without_backup(message: String) -> WriteError {
    WriteError { backup: None, message }
}

pub(crate) fn save_before_write(settings: &device::ReadSettings, dir: &Path) -> Result<PathBuf, WriteError> {
    std::fs::create_dir_all(dir).map_err(|e| without_backup(format!("バックアップフォルダを作れません: {e}")))?;
    let path = backup::new_path(dir, "auto");
    backup::save_settings(settings, &path).map_err(without_backup)?;
    Ok(path)
}

pub fn verify_readback<'a>(
    settings: &device::ReadSettings,
    requested: impl IntoIterator<Item = (u8, &'a [u8])>,
    active: Option<u8>,
) -> Result<(), String> {
    for (slot, data) in requested {
        if settings.profiles.get(slot as usize).is_none_or(|profile| profile.raw != data) {
            return Err(format!("最終読み戻しでプロファイル{}が一致しません", slot + 1));
        }
    }
    if let Some(expected) = active {
        if settings.active.profile != expected {
            return Err("最終読み戻しで使用中のプロファイルが一致しません".into());
        }
    }
    Ok(())
}

/// 自動反映の直前に、編集中のプロファイルが実機で変更されていないか確認する。
pub fn write_automatic<D: ProfileDevice>(
    dev: &D,
    index: u8,
    expected: &[u8],
    data: &[u8],
    backup_dir: Option<&Path>,
) -> Result<Option<PathBuf>, WriteError> {
    let settings = dev.read_settings().map_err(without_backup)?;
    let current = settings.profiles.get(index as usize)
        .ok_or_else(|| without_backup("プロファイル番号が不正です".into()))?;
    if current.raw == data {
        return Ok(None);
    }
    if current.raw != expected {
        return Err(without_backup(format!(
            "プロファイル{}が実機で変更されています。実機設定を再読み込みしてください。", index + 1
        )));
    }
    let backup = backup_dir.map(|dir| save_before_write(&settings, dir)).transpose()?;
    dev.write_profile(index, data)
        .map_err(|message| WriteError { backup: backup.clone(), message })?;
    Ok(backup)
}

/// 自動バックアップを取らずに書き込む（GUI の即時反映で、起動後の最初の書き込みで
/// write_profiles によるバックアップを取り済みのときだけ使う）。1 件ずつ write_profile
/// （SET → 1 秒待つ → GET で照合）を完了させてから次へ進み、失敗したらそこで止める。
pub fn write_profiles_no_backup<D: ProfileDevice>(dev: &D, profiles: &[(u8, Vec<u8>)]) -> Result<(), String> {
    for (index, data) in profiles {
        dev.write_profile(*index, data)?;
    }
    Ok(())
}

pub fn backup_dir() -> Result<PathBuf, String> {
    crate::storage::backup_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use crate::protocol::{Active, Profile, PROFILE_LEN, REPORT_PROFILES};

    struct Mock { writes: RefCell<Vec<u8>>, fail: Option<u8> }
    impl ProfileDevice for Mock {
        fn read_settings(&self) -> Result<device::ReadSettings, String> {
            let profiles = REPORT_PROFILES.iter().map(|id| {
                let mut raw = vec![0; PROFILE_LEN]; raw[0] = *id;
                Profile::parse(&raw).unwrap()
            }).collect();
            Ok(device::ReadSettings { active: Active { profile: 0, dpi_slot: 0 }, active_raw: vec![0xf0, 0, 0, 0], profiles })
        }
        fn write_profile(&self, index: u8, _: &[u8]) -> Result<(), String> {
            self.writes.borrow_mut().push(index);
            if self.fail == Some(index) { Err("読み戻し不一致".into()) } else { Ok(()) }
        }
    }

    #[test]
    fn backup_failure_prevents_writes() {
        let dev = Mock { writes: RefCell::new(vec![]), fail: None };
        let file = Path::new("Cargo.toml");
        let err = write_profiles(&dev, &[(0, vec![])], file, |_, _| {}).unwrap_err();
        assert!(err.backup.is_none());
        assert!(dev.writes.borrow().is_empty());
    }

    #[test]
    fn final_readback_rejects_a_profile_changed_after_its_individual_check() {
        let dev = Mock { writes: RefCell::new(vec![]), fail: None };
        let mut actual = dev.read_settings().unwrap();
        let requested = actual.profiles.iter().map(|p| p.raw.clone()).collect::<Vec<_>>();
        let profiles = || requested.iter().enumerate().map(|(slot, data)| (slot as u8, data.as_slice()));
        assert!(verify_readback(&actual, profiles(), Some(0)).is_ok());
        actual.profiles[0].raw[1] = 11;
        assert!(verify_readback(&actual, profiles(), Some(0)).is_err());
        actual.profiles[0].raw.clone_from(&requested[0]);
        actual.active.profile = 1;
        assert!(verify_readback(&actual, profiles(), Some(0)).is_err());
    }

    #[test]
    fn mismatch_stops_later_writes() {
        let dev = Mock { writes: RefCell::new(vec![]), fail: Some(1) };
        let dir = Path::new("target/operations-test-mismatch");
        let err = write_profiles(&dev, &[(0, vec![]), (1, vec![]), (2, vec![])], dir, |_, _| {}).unwrap_err();
        assert!(err.backup.is_some());
        assert_eq!(*dev.writes.borrow(), vec![0, 1]);
        std::fs::remove_file(err.backup.unwrap()).unwrap();
    }

    #[test]
    fn automatic_write_checks_the_device_baseline_before_writing() {
        let dev = Mock { writes: RefCell::new(vec![]), fail: None };
        let mut base = dev.read_settings().unwrap().profiles[0].raw.clone();
        let mut new = base.clone();
        new[1] = 9;

        base[1] = 2;
        assert!(write_automatic(&dev, 0, &base, &new, None).is_err());
        assert!(dev.writes.borrow().is_empty());
        assert!(write_automatic(&dev, 0, &new, &base, None).is_err());
        assert!(dev.writes.borrow().is_empty());

        base[1] = 0;
        assert!(write_automatic(&dev, 0, &new, &base, None).unwrap().is_none());
        assert!(dev.writes.borrow().is_empty());
        write_automatic(&dev, 0, &base, &new, None).unwrap();
        assert_eq!(*dev.writes.borrow(), [0]);
    }

    #[test]
    fn no_backup_writes_in_order_and_stops_on_mismatch() {
        let dev = Mock { writes: RefCell::new(vec![]), fail: None };
        write_profiles_no_backup(&dev, &[(2, vec![]), (0, vec![])]).unwrap();
        assert_eq!(*dev.writes.borrow(), vec![2, 0]);
        let dev = Mock { writes: RefCell::new(vec![]), fail: Some(0) };
        assert!(write_profiles_no_backup(&dev, &[(0, vec![]), (1, vec![])]).is_err());
        assert_eq!(*dev.writes.borrow(), vec![0]);
    }
}

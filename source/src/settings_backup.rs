use eframe::egui;
use g600ctl::{gui::i18n::tr, storage};
use std::{fs, io::{self, Read}, path::{Path, PathBuf}};

const HEADER: &[u8] = b"MMOMOUSE-SETTINGS\x01";
const FILES: [&str; 4] = ["m908.draft", "m908-custom.txt", "g600-custom.txt", "language.txt"];
const MAX_FILE: usize = 4 * 1024 * 1024;
const MAX_ARCHIVE: usize = HEADER.len() + 4 + FILES.len() * (5 + MAX_FILE);
const EXTENSION: &str = "mmo-settings";

fn paths() -> Result<[PathBuf; 4], String> {
    let m908 = crate::ui::settings_directory();
    let g600 = storage::data_dir()?;
    Ok([m908.join(FILES[0]), m908.join(FILES[1]), g600.join(FILES[2]), g600.join(FILES[3])])
}

fn read_limited(path: &Path, max: usize) -> io::Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Expected a regular file."));
    }
    let mut bytes = Vec::new();
    file.take((max + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > max {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "File exceeds the backup size limit."));
    }
    Ok(bytes)
}

fn archive(paths: &[PathBuf; 4]) -> io::Result<Vec<u8>> {
    let mut result = HEADER.to_vec();
    for (index, path) in paths.iter().enumerate() {
        let bytes = match read_limited(path, MAX_FILE) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        result.push(index as u8);
        result.extend((bytes.len() as u32).to_le_bytes());
        result.extend(bytes);
    }
    if result.len() == HEADER.len() {
        return Err(io::Error::new(io::ErrorKind::NotFound, "No saved settings to back up."));
    }
    result.extend(crc32fast::hash(&result).to_le_bytes());
    Ok(result)
}

fn unpack(bytes: &[u8]) -> Option<[Option<&[u8]>; 4]> {
    let (body, checksum) = bytes.split_at_checked(bytes.len().checked_sub(4)?)?;
    if crc32fast::hash(body).to_le_bytes() != checksum { return None; }
    let mut remaining = body.strip_prefix(HEADER)?;
    let mut entries = [None; 4];
    while !remaining.is_empty() {
        let (meta, content) = remaining.split_at_checked(5)?;
        let index = usize::from(meta[0]);
        let length = u32::from_le_bytes(meta[1..].try_into().ok()?) as usize;
        let entry = entries.get_mut(index)?;
        if entry.is_some() || length > MAX_FILE { return None; }
        let (value, rest) = content.split_at_checked(length)?;
        *entry = Some(value);
        remaining = rest;
    }
    entries.iter().any(Option::is_some).then_some(entries)
}

fn stage_path() -> Result<PathBuf, String> {
    Ok(storage::data_dir()?.join("settings-restore.pending"))
}

fn restore(path: &Path, paths: &[PathBuf; 4]) -> Result<bool, String> {
    let bytes = match read_limited(path, MAX_ARCHIVE) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.to_string()),
    };
    let entries = unpack(&bytes).ok_or("Invalid settings backup.")?;
    for (entry, target) in entries.into_iter().zip(paths) {
        if let Some(bytes) = entry {
            storage::write_atomic(target, bytes).map_err(|e| format!("{}: {e}", target.display()))?;
        }
    }
    fs::remove_file(path).map_err(|e| e.to_string())?;
    Ok(true)
}

pub fn apply_pending() -> Result<(), String> {
    let path = stage_path()?;
    if path.is_file() {
        // The pending file is retained until all writes complete. Startup never loads a partial restore.
        restore(&path, &paths()?).map_err(|error| format!("{error}\n{}", path.display()))?;
    }
    Ok(())
}

#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> { text.encode_utf16().chain(std::iter::once(0)).collect() }

pub fn show_startup_error(error: &str) {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
        MessageBoxW(std::ptr::null_mut(), wide(error).as_ptr(),
            wide("MMO Mouse Control — Settings restore").as_ptr(), MB_OK | MB_ICONERROR);
    }
    #[cfg(not(windows))]
    eprintln!("{error}");
}

#[cfg(windows)]
fn choose_file(save: bool) -> Result<Option<PathBuf>, String> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::UI::{Controls::Dialogs::*, Input::KeyboardAndMouse::GetActiveWindow};
    let mut file = [0u16; 32768];
    if save {
        let suggested = wide("MMOMouseControl.mmo-settings");
        file[..suggested.len()].copy_from_slice(&suggested);
    }
    let filter = wide("MMO Mouse Control (*.mmo-settings)\0*.mmo-settings\0");
    let extension = wide(EXTENSION);
    let mut dialog: OPENFILENAMEW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
    dialog.hwndOwner = unsafe { GetActiveWindow() };
    dialog.lpstrFilter = filter.as_ptr();
    dialog.lpstrDefExt = extension.as_ptr();
    dialog.lpstrFile = file.as_mut_ptr();
    dialog.nMaxFile = file.len() as u32;
    dialog.Flags = OFN_EXPLORER | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR | OFN_DONTADDTORECENT
        | if save { OFN_OVERWRITEPROMPT } else { OFN_FILEMUSTEXIST };
    let result = unsafe { if save { GetSaveFileNameW(&mut dialog) } else { GetOpenFileNameW(&mut dialog) } };
    if result == 0 {
        let error = unsafe { CommDlgExtendedError() };
        return if error == 0 { Ok(None) } else { Err(format!("Windows file dialog: 0x{error:x}")) };
    }
    let length = file.iter().position(|&unit| unit == 0).unwrap_or(file.len());
    let path = PathBuf::from(std::ffi::OsString::from_wide(&file[..length]));
    if !path.extension().and_then(|s| s.to_str()).is_some_and(|s| s.eq_ignore_ascii_case(EXTENSION)) {
        return Err(tr("バックアップの拡張子は .mmo-settings です。"));
    }
    Ok(Some(path))
}

#[cfg(not(windows))]
fn choose_file(_: bool) -> Result<Option<PathBuf>, String> { Ok(None) }

#[derive(Default)]
pub struct Panel {
    open: bool,
    confirmation: Option<Vec<u8>>,
    status: Option<(bool, String)>,
}

impl Panel {
    pub fn sample(open: bool) -> Self { Self { open, ..Self::default() } }

    pub fn button(&mut self, ui: &mut egui::Ui) {
        if ui.button(tr("アプリ設定のバックアップ")).clicked() { self.open = true; }
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        let mut open = self.open;
        if open {
            egui::Window::new(tr("アプリ設定のバックアップ"))
                .id(egui::Id::new("app-settings-backup"))
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.set_max_width(440.0);
                    ui.label(tr("保存済みの下書き、登録したショートカット、表示言語を1つのファイルにまとめます。"));
                    ui.label(tr("マウス本体の設定は各マウスのバックアップを使ってください。"));
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(tr("バックアップを保存")).clicked() {
                            match self.export() {
                                Ok(Some(())) => self.status = Some((true, tr("バックアップを保存しました。"))),
                                Ok(None) => {}
                                Err(error) => self.status = Some((false, error)),
                            }
                        }
                        if ui.button(tr("バックアップから復元")).clicked() {
                            match Self::import() {
                                Ok(Some(bytes)) => self.confirmation = Some(bytes),
                                Ok(None) => {}
                                Err(error) => self.status = Some((false, error)),
                            }
                        }
                    });
                    if let Some((success, status)) = &self.status {
                        if *success { ui.label(status); } else { ui.colored_label(egui::Color32::LIGHT_RED, status); }
                    }
                });
        }
        self.open = open;
        if self.confirmation.is_some() {
            egui::Modal::new(egui::Id::new("app-settings-restore")).show(ctx, |ui| {
                ui.heading(tr("バックアップから復元"));
                ui.label(tr("次回の起動時に、バックアップに含まれるアプリ設定を上書きします。"));
                ui.horizontal_wrapped(|ui| {
                    if ui.button(tr("キャンセル")).clicked() { self.confirmation = None; }
                    if ui.button(tr("次回起動時に復元")).clicked() {
                        if let Some(bytes) = self.confirmation.take() {
                            match stage_path().and_then(|path| storage::write_atomic(&path, &bytes).map_err(|e| e.to_string())) {
                                Ok(()) => self.status = Some((true, tr("アプリを終了して起動し直すと復元されます。"))),
                                Err(error) => self.status = Some((false, error)),
                            }
                        }
                    }
                });
            });
        }
    }

    fn export(&self) -> Result<Option<()>, String> {
        let bytes = archive(&paths()?).map_err(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                tr("バックアップできる保存済み設定がありません。")
            } else { error.to_string() }
        })?;
        if let Some(path) = choose_file(true)? {
            storage::write_atomic(&path, &bytes).map_err(|e| e.to_string())?;
            return Ok(Some(()));
        }
        Ok(None)
    }

    fn import() -> Result<Option<Vec<u8>>, String> {
        let Some(path) = choose_file(false)? else { return Ok(None) };
        let bytes = read_limited(&path, MAX_ARCHIVE).map_err(|e| e.to_string())?;
        unpack(&bytes).ok_or_else(|| tr("このファイルは有効なアプリ設定のバックアップではありません。"))?;
        Ok(Some(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_present_entries_and_keeps_settings_not_in_backup() {
        let dir = std::env::temp_dir().join(format!("mmo-settings-backup-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let paths = FILES.map(|name| dir.join(name));
        fs::write(&paths[1], b"shortcut").unwrap();
        fs::write(&paths[3], b"ja").unwrap();
        let archive = archive(&paths).unwrap();
        let stage = dir.join("pending");
        fs::write(&stage, &archive).unwrap();
        fs::write(&paths[0], b"leave this intact").unwrap();
        fs::write(&paths[1], b"changed").unwrap();
        assert!(restore(&stage, &paths).unwrap());
        assert_eq!(fs::read(&paths[0]).unwrap(), b"leave this intact");
        assert_eq!(fs::read(&paths[1]).unwrap(), b"shortcut");
        assert!(!stage.exists());
        let mut damaged = archive;
        damaged[HEADER.len()] ^= 1;
        fs::write(&stage, damaged).unwrap();
        assert!(restore(&stage, &paths).is_err());
        assert_eq!(fs::read(&paths[1]).unwrap(), b"shortcut");
        fs::remove_dir_all(dir).unwrap();
    }
}

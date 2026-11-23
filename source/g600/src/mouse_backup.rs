use std::{fs, io::{self, Read}, path::{Path, PathBuf}};

const HEADER: &[u8] = b"MMOMOUSE-SETTINGS\x01";
const FILES: [&str; 4] = ["m908.draft", "m908-custom.txt", "g600-custom.txt", "language.txt"];
const MAX_FILE: usize = 4 * 1024 * 1024;
const MAX_ARCHIVE: usize = HEADER.len() + 4 + FILES.len() * (5 + MAX_FILE);
const MOUSE_HEADER: &[u8] = b"MMOMOUSE-BACKUP\x01";
const MAX_MOUSE_BACKUP: usize = MOUSE_HEADER.len() + 9 + MAX_FILE + MAX_ARCHIVE;
use crate::storage;

fn paths(m908: &Path) -> Result<[PathBuf; 4], String> {
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

fn archive(entries: [Option<&[u8]>; 4]) -> Vec<u8> {
    let mut result = HEADER.to_vec();
    for (index, bytes) in entries.into_iter().enumerate() {
        if let Some(bytes) = bytes {
            result.push(index as u8);
            result.extend((bytes.len() as u32).to_le_bytes());
            result.extend(bytes);
        }
    }
    result.extend(crc32fast::hash(&result).to_le_bytes());
    result
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

pub fn apply_pending(m908: &Path) -> Result<(), String> {
    let path = stage_path()?;
    if path.is_file() {
        // The pending file is retained until all writes complete. Startup never loads a partial restore.
        restore(&path, &paths(m908)?).map_err(|error| format!("{error}\n{}", path.display()))?;
    }
    Ok(())
}


#[derive(Clone, Copy)]
#[repr(u8)]
pub enum Mouse {
    M908 = 0,
    G600 = 1,
}

impl Mouse {
    fn includes(self, index: usize) -> bool {
        index == 3 || match self {
            Self::M908 => index < 2,
            Self::G600 => index == 2,
        }
    }

    pub fn save(self, destination: &Path, payload: &str, m908: Option<&Path>) -> Result<(), String> {
        let paths = paths(m908.unwrap_or_else(|| Path::new("")))?;
        if payload.len() > MAX_FILE {
            return Err("Mouse settings exceed the backup size limit.".into());
        }
        let mut entries = [None, None, None, None];
        for (index, path) in paths.iter().enumerate().filter(|(index, _)| self.includes(*index)) {
            entries[index] = read_optional(path, MAX_FILE)?;
        }
        let bytes = pack(self, payload, entries.each_ref().map(Option::as_deref));
        storage::write_atomic(destination, &bytes).map_err(|error| error.to_string())
    }

    pub fn load(self, path: &Path) -> Result<Loaded, String> {
        let bytes = read_limited(path, MAX_MOUSE_BACKUP).map_err(|error| error.to_string())?;
        if bytes.starts_with(HEADER) {
            unpack(&bytes).ok_or("Invalid app settings backup.")?;
            return Ok(Loaded { payload: None, settings: Some(bytes) });
        }
        if bytes.starts_with(MOUSE_HEADER) {
            return decode(self, &bytes).ok_or_else(|| "Invalid backup or a backup for a different mouse.".into());
        }
        Ok(Loaded { payload: Some(String::from_utf8(bytes).map_err(|error| error.to_string())?), settings: None })
    }
}

fn read_optional(path: &Path, limit: usize) -> Result<Option<Vec<u8>>, String> {
    match read_limited(path, limit) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn pack(mouse: Mouse, payload: &str, entries: [Option<&[u8]>; 4]) -> Vec<u8> {
    let mut bytes = MOUSE_HEADER.to_vec();
    bytes.push(mouse as u8);
    bytes.extend((payload.len() as u32).to_le_bytes());
    bytes.extend(payload.as_bytes());
    if entries.iter().any(Option::is_some) {
        bytes.extend(archive(entries));
    }
    bytes.extend(crc32fast::hash(&bytes).to_le_bytes());
    bytes
}

fn decode(mouse: Mouse, bytes: &[u8]) -> Option<Loaded> {
    let (body, checksum) = bytes.split_at_checked(bytes.len().checked_sub(4)?)?;
    if crc32fast::hash(body).to_le_bytes() != checksum { return None; }
    let (metadata, content) = body.strip_prefix(MOUSE_HEADER)?.split_at_checked(5)?;
    let length = u32::from_le_bytes(metadata[1..].try_into().ok()?) as usize;
    if metadata[0] != mouse as u8 || length > MAX_FILE { return None; }
    let (payload, settings) = content.split_at_checked(length)?;
    let settings = if settings.is_empty() {
        None
    } else {
        let entries = unpack(settings)?;
        if entries.iter().enumerate().any(|(index, value)| value.is_some() && !mouse.includes(index)) {
            return None;
        }
        Some(settings.to_vec())
    };
    Some(Loaded { payload: Some(std::str::from_utf8(payload).ok()?.to_owned()), settings })
}

pub struct Loaded {
    pub payload: Option<String>,
    settings: Option<Vec<u8>>,
}

impl Loaded {
    pub fn stage_settings(&self) -> Result<bool, String> {
        let Some(settings) = &self.settings else { return Ok(false); };
        let target = stage_path()?;
        stage(&target, settings)?;
        Ok(true)
    }
}

fn stage(target: &Path, settings: &[u8]) -> Result<(), String> {
    static STAGING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _lock = STAGING.lock().map_err(|error| error.to_string())?;
    let incoming = unpack(settings).ok_or("Invalid app settings backup.")?;
    let pending = read_optional(target, MAX_ARCHIVE)?;
    let previous = match pending.as_deref() {
        Some(bytes) => unpack(bytes).ok_or("Invalid pending app settings backup.")?,
        None => [None; 4],
    };
    let merged = archive(std::array::from_fn(|index| incoming[index].or(previous[index])));
    storage::write_atomic(target, &merged).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_model_scope_and_checksum() {
        let bytes = pack(Mouse::M908, "mouse data", [Some(b"draft"), None, None, Some(b"ja")]);
        assert_eq!(decode(Mouse::M908, &bytes).unwrap().payload.as_deref(), Some("mouse data"));
        assert!(decode(Mouse::G600, &bytes).is_none());
        assert!(decode(Mouse::M908, &bytes[..bytes.len() - 1]).is_none());
        let wrong_scope = pack(Mouse::M908, "mouse data", [None, None, Some(b"shortcut"), None]);
        assert!(decode(Mouse::M908, &wrong_scope).is_none());
    }

    #[test]
    fn merges_pending_backups_and_leaves_other_files_in_place() {
        let dir = std::env::temp_dir().join(format!("mmo-bundle-restore-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let paths = FILES.map(|name| dir.join(name));
        let pending = dir.join("pending");
        stage(&pending, &archive([None, Some(b"first"), None, Some(b"ja")])).unwrap();
        stage(&pending, &archive([None, None, Some(b"second"), Some(b"en")])).unwrap();
        fs::write(&paths[0], b"keep").unwrap();
        assert!(restore(&pending, &paths).unwrap());
        for (path, value) in paths.iter().zip([b"keep".as_slice(), b"first", b"second", b"en"]) {
            assert_eq!(fs::read(path).unwrap(), value);
        }
        assert!(!pending.exists());
        fs::remove_dir_all(dir).unwrap();
    }
}

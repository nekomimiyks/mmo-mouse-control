use crate::{
    model::Profile,
    protocol::{self, READ_1, READ_3},
    settings::{self, Snapshot},
};
use hidapi::{HidApi, HidDevice};
use std::{
    ffi::CString,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const DISPLAY_NAME: &str = "M908 / Pusat V8";
pub const VID: u16 = 0x04d9;
// Monster Pusat V8 MMOなど、USB IDと設定用HIDが共通の別名製品も対象。
pub const PID: u16 = 0xfc4d;
/// アプリの最初に呼び、HidApi生成のたびの全HID列挙を停止する。
/// M908系とG600系のバックエンドがそれぞれ対象VID/PIDだけを明示的に列挙する。
pub fn initialize_hid() {
    HidApi::disable_device_discovery();
}
#[derive(Clone, Debug)]
pub struct Device {
    pub path: CString,
    pub name: String,
    pub interface: i32,
    pub usage_page: u16,
    pub usage: u16,
}
impl Device {
    pub fn is_config(&self) -> bool {
        self.interface == 2 && self.usage_page == 0xffa0 && self.usage == 1
    }
    pub fn label(&self) -> String {
        format!(
            "{} / IF {} / Usage {:04X}:{:04X}",
            self.name, self.interface, self.usage_page, self.usage
        )
    }
}
pub fn enumerate() -> Result<Vec<Device>, String> {
    let mut api = HidApi::new().map_err(|e| e.to_string())?;
    api.reset_devices().map_err(|e| e.to_string())?;
    api.add_devices(VID, PID).map_err(|e| e.to_string())?;
    Ok(api
        .device_list()
        .filter(|d| d.vendor_id() == VID && d.product_id() == PID)
        .map(|d| Device {
            path: d.path().to_owned(),
            name: d.product_string().unwrap_or("M908互換マウス").into(),
            interface: d.interface_number(),
            usage_page: d.usage_page(),
            usage: d.usage(),
        })
        .collect())
}
pub fn find() -> Result<Device, String> {
    let ds = enumerate()?;
    let config: Vec<_> = ds.into_iter().filter(Device::is_config).collect();
    match config.len() {
        0 => Err("M908の設定用HIDが見つかりません。接続とUSBアクセス権を確認してください。".into()),
        1 => Ok(config[0].clone()),
        _ => Err("M908が複数見つかりました。デバイス情報で対象を選択してください。".into()),
    }
}
/// ベンダー定義の入力だけを短時間記録する。通常のキーボード入力は扱わない。
pub fn watch_input(seconds: u64, mut emit: impl FnMut(&str)) -> Result<(), String> {
    watch_input_until(seconds, None, &mut emit)
}
/// 観測クライアントだけを終了する停止ファイル。設定レポートは一切送信しない。
pub fn watch_input_until(
    seconds: u64,
    stop: Option<&Path>,
    mut emit: impl FnMut(&str),
) -> Result<(), String> {
    let device = find()?;
    let api = HidApi::new().map_err(|e| e.to_string())?;
    let dev = api.open_path(&device.path).map_err(|e| e.to_string())?;
    emit("M908の専用入力を監視中です。");
    let started = std::time::Instant::now();
    let mut buf = [0u8; 64];
    let mut previous = Vec::new();
    let mut count = 0;
    while started.elapsed() < Duration::from_secs(seconds.min(180))
        && !stop.is_some_and(Path::exists)
    {
        let n = dev.read_timeout(&mut buf, 20).map_err(|e| e.to_string())?;
        if n > 0 && previous != buf[..n] {
            previous = buf[..n].to_vec();
            count += 1;
            emit(&format!(
                "{:>6} ms: {}",
                started.elapsed().as_millis(),
                buf[..n]
                    .iter()
                    .map(|b| format!("{b:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
        }
    }
    emit(&format!("監視終了: {count}件"));
    Ok(())
}

/// 別プロセスも含めてM908のコマンド列が混ざらないよう排他する。
pub(crate) struct DeviceLock(isize);
#[cfg(windows)]
mod mutex {
    #[link(name = "kernel32")]
    extern "system" {
        pub fn CreateMutexW(attr: *const std::ffi::c_void, owner: i32, name: *const u16) -> isize;
        pub fn WaitForSingleObject(handle: isize, ms: u32) -> u32;
        pub fn ReleaseMutex(handle: isize) -> i32;
        pub fn CloseHandle(handle: isize) -> i32;
        pub fn MoveFileExW(existing: *const u16, destination: *const u16, flags: u32) -> i32;
    }
}
impl DeviceLock {
    pub(crate) fn acquire() -> Result<Self, String> {
        Self::acquire_named("Local\\M908Ctl.Device04D9FC4D", 0)
    }
    pub(crate) fn acquire_named(lock_name: &str, timeout: u32) -> Result<Self, String> {
        #[cfg(windows)]
        unsafe {
            let name: Vec<u16> = lock_name.encode_utf16().chain(Some(0)).collect();
            let h = mutex::CreateMutexW(std::ptr::null(), 0, name.as_ptr());
            if h == 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            let result = mutex::WaitForSingleObject(h, timeout);
            if result == 0 || result == 0x80 {
                return Ok(Self(h));
            }
            mutex::CloseHandle(h);
            Err("別のM908制御ソフトが通信中です。終了後に再試行してください。".into())
        }
        #[cfg(not(windows))]
        {
            Ok(Self(0))
        }
    }
}
impl Drop for DeviceLock {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            mutex::ReleaseMutex(self.0);
            mutex::CloseHandle(self.0);
        }
    }
}

pub trait Transport {
    fn send(&mut self, row: &[u8]) -> Result<(), String>;
    fn get(&mut self, id: u8, len: usize) -> Result<Vec<u8>, String>;
}
pub(crate) struct HidTransport(HidDevice, Duration);
impl Transport for HidTransport {
    fn send(&mut self, row: &[u8]) -> Result<(), String> {
        self.0
            .send_feature_report(row)
            .map_err(|e| format!("HID送信失敗: {e}"))?;
        if !self.1.is_zero() {
            std::thread::sleep(self.1);
        }
        Ok(())
    }
    fn get(&mut self, id: u8, len: usize) -> Result<Vec<u8>, String> {
        for attempt in 0..4 {
            let mut buf = vec![0; len];
            buf[0] = id;
            let n = self
                .0
                .get_feature_report(&mut buf)
                .map_err(|e| format!("HID応答取得失敗: {e}"))?;
            if n > buf.len() {
                return Err("HIDの応答長がバッファを超えています".into());
            }
            if n == 8 && len > 8 && attempt < 3 {
                // ヘッダーだけなら応答だけを再取得する。書込みは決して再送しない。
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            buf.truncate(n);
            return Ok(buf);
        }
        unreachable!()
    }
}
pub(crate) fn open(device: &Device) -> Result<HidTransport, String> {
    if !device.is_config() {
        return Err("M908の設定用コレクション（IF 2 / FFA0:0001）を選択してください。".into());
    }
    let api = HidApi::new().map_err(|e| e.to_string())?;
    let dev = api
        .open_path(&device.path)
        .map_err(|e| format!("選択したM908を開けません: {e}"))?;
    let actual = dev.get_device_info().map_err(|e| e.to_string())?;
    if actual.vendor_id() != VID
        || actual.product_id() != PID
        || actual.usage_page() != 0xffa0
        || actual.usage() != 1
    {
        return Err("接続先がM908の設定用HIDではありません。書き込みを停止しました。".into());
    }
    Ok(HidTransport(dev, Duration::ZERO))
}

/// 読取り専用で固定待機の必要性を検証する。設定を書き込むコマンドは送らない。
pub fn benchmark_transport(device: &Device) -> Result<String, String> {
    let _lock = DeviceLock::acquire()?;
    let mut io = open(device)?;
    let original = read_controls_transport(&mut io)?;
    let mut lines = Vec::new();
    for millis in [5, 2, 1, 0] {
        println!("測定開始: 待機{millis}ms");
        io.1 = Duration::from_millis(millis);
        let mut samples = Vec::new();
        for _ in 0..8 {
            let start = std::time::Instant::now();
            let actual = read_controls_transport(&mut io)
                .map_err(|e| format!("待機{millis}ms・{}回目: {e}", samples.len() + 1))?;
            samples.push(start.elapsed().as_secs_f64());
            if !original.same_controls(&actual, false) {
                return Err(format!("待機{millis}msの読み取りが一致しません"));
            }
        }
        lines.push(format!(
            "待機{millis}ms: 8回一致 / 平均{:.4}秒 / 最小{:.4}秒 / 最大{:.4}秒",
            samples.iter().sum::<f64>() / samples.len() as f64,
            samples.iter().copied().fold(f64::INFINITY, f64::min),
            samples.iter().copied().fold(0.0, f64::max)
        ));
    }
    Ok(lines.join("\n"))
}

/// 連続する書込領域のみを束ねる。穴埋めやコミット境界を跨ぐ結合はしない。
pub(crate) fn compact_writes(packets: impl IntoIterator<Item = Vec<u8>>) -> Vec<Vec<u8>> {
    let valid = |packet: &[u8]| {
        packet.len() >= 8
            && matches!((packet[0], packet.len()), (2, 16) | (3, 64))
            && 8 + u16::from_le_bytes([packet[4], packet[5]]) as usize <= packet.len()
    };
    let mut result: Vec<Vec<u8>> = Vec::new();
    for packet in packets {
        if let Some(previous) = result.last_mut() {
            if !valid(&packet) || !valid(previous) {
                result.push(packet);
                continue;
            }
            let length = u16::from_le_bytes([packet[4], packet[5]]) as usize;
            let previous_length = u16::from_le_bytes([previous[4], previous[5]]) as usize;
            let address = u16::from_le_bytes([packet[2], packet[3]]) as usize;
            let previous_address = u16::from_le_bytes([previous[2], previous[3]]) as usize;
            if packet[1] == 0xf3
                && previous[1] == 0xf3
                && packet[6..8] == previous[6..8]
                && address == previous_address + previous_length
                && length + previous_length <= 48
            {
                previous.resize(
                    if previous_length + length <= 8 {
                        16
                    } else {
                        64
                    },
                    0,
                );
                previous[0] = if previous.len() == 16 { 2 } else { 3 };
                previous[4..6].copy_from_slice(&((previous_length + length) as u16).to_le_bytes());
                previous[8 + previous_length..8 + previous_length + length]
                    .copy_from_slice(&packet[8..8 + length]);
                continue;
            }
        }
        result.push(packet);
    }
    result
}

pub fn benchmark_identification_transport(
    device: &Device,
    directory: &Path,
) -> Result<String, String> {
    let _lock = DeviceLock::acquire()?;
    let mut io = open(device)?;
    let original = read_transport(&mut io)?;
    backup(directory, &original, "before-transport-test")?;
    let borrowed = (original.active() + 4) % 5;
    let plan = crate::identification::prepared(&original, borrowed)?;
    let path = directory.join(crate::identification::JOURNAL);
    let mut lines = Vec::new();
    for millis in [5, 0, 0, 0] {
        let journal = crate::identification::Journal {
            device: device.clone(),
            borrowed,
            original: original.clone(),
        };
        journal.save(&path)?;
        io.1 = Duration::from_millis(millis);
        let started = std::time::Instant::now();
        let outcome: Result<Snapshot, String> = (|| {
            for packet in compact_writes(plan.packets.clone()) {
                io.send(&packet)?;
            }
            let actual = read_transport(&mut io)?;
            if !plan.expected.same_settings(&actual, false) {
                return Err("まとめ書込み後の全体設定が一致しません".into());
            }
            Ok(actual)
        })();
        let elapsed = started.elapsed();
        // 未検証のまとめ書込みは復元に使わない。最悪の途中失敗からも元データで戻す。
        io.1 = Duration::from_millis(5);
        let actual = crate::identification::restore_transport(&mut io, &journal, &plan.expected)?;
        if !original.same_settings(&actual, false) {
            return Err("診断後の全プロファイル・マクロ・使用中番号が一致しません".into());
        }
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        outcome?;
        lines.push(format!(
            "まとめ送信・待機{millis}ms: {:.4}秒 / 全設定・マクロ・使用中番号の復元一致",
            elapsed.as_secs_f64()
        ));
        println!("{}", lines.last().unwrap());
    }
    Ok(format!(
        "設定送信数: {} → {}",
        plan.packets.len(),
        compact_writes(plan.packets).len()
    ))
}

pub fn read_legacy_transport(io: &mut impl Transport) -> Result<Snapshot, String> {
    io.send(&READ_1[0])?;
    let result = (|| {
        let mut parts: [Vec<Vec<u8>>; 3] = std::array::from_fn(|_| Vec::new());
        for part in 0..3 {
            for request in settings::requests(part) {
                io.send(&request)?;
                let reply = io.get(request[0], request.len())?;
                protocol::validate_reply(&request, &reply).map_err(|e| {
                    format!(
                        "part{} / {:02X}{:02X}: {e}",
                        part + 1,
                        request[3],
                        request[2]
                    )
                })?;
                parts[part].push(reply);
            }
        }
        Snapshot::from_parts(parts)
    })();
    let close = io.send(&READ_3[100]);
    match (result, close) {
        (Ok(s), Ok(())) => Ok(s),
        (Err(e), _) => Err(e),
        (_, Err(e)) => Err(e),
    }
}
/// 実機照合済みの最大54バイトへまとめる。戻り値の行順は要求順。
fn read_rows(io: &mut impl Transport, requests: &[Vec<u8>]) -> Result<Vec<Vec<u8>>, String> {
    read_rows_limit(io, requests, 54)
}
fn read_rows_limit(
    io: &mut impl Transport,
    requests: &[Vec<u8>],
    limit: usize,
) -> Result<Vec<Vec<u8>>, String> {
    let address = |r: &Vec<u8>| u16::from_le_bytes([r[2], r[3]]) as usize;
    let length = |r: &Vec<u8>| u16::from_le_bytes([r[4], r[5]]) as usize;
    let mut order: Vec<_> = (0..requests.len()).collect();
    order.sort_by_key(|i| address(&requests[*i]));
    let mut result = vec![Vec::new(); requests.len()];
    let mut i = 0;
    while i < order.len() {
        let start = address(&requests[order[i]]);
        let mut end = start + length(&requests[order[i]]);
        let mut j = i + 1;
        while j < order.len() {
            let next_end = address(&requests[order[j]]) + length(&requests[order[j]]);
            if next_end - start > limit {
                break;
            }
            end = end.max(next_end);
            j += 1;
        }
        let short = end - start <= 8;
        let mut request = vec![0u8; if short { 16 } else { 64 }];
        request[..6].copy_from_slice(&[
            if short { 2 } else { 3 },
            0xf2,
            start as u8,
            (start >> 8) as u8,
            (end - start) as u8,
            0,
        ]);
        io.send(&request)?;
        let reply = io.get(request[0], request.len())?;
        protocol::validate_reply(&request, &reply)?;
        for &index in &order[i..j] {
            let source = &requests[index];
            let offset = address(source) - start;
            let len = length(source);
            let mut row = source.clone();
            row[1] = 8;
            row[2] &= 0xfe;
            row[3] |= 0x60;
            row[6] = 0xfa;
            row[7] = 0xfa;
            row[8..8 + len].copy_from_slice(&reply[8 + offset..8 + offset + len]);
            result[index] = row;
        }
        i = j;
    }
    Ok(result)
}
/// 変更した領域をすべて読み戻す。書込み前の全通常設定バックアップは別途取得する。
fn verify_writes(io: &mut impl Transport, packets: &[Vec<u8>]) -> Result<(), String> {
    let writes: Vec<_> = packets.iter().filter(|p| p[1] == 0xf3).collect();
    let requests: Vec<_> = writes
        .iter()
        .map(|p| {
            let mut r = (*p).clone();
            r[1] = 0xf2;
            r[6..].fill(0);
            r
        })
        .collect();
    io.send(&READ_1[0])?;
    let result = (|| {
        let actual = read_rows(io, &requests)?;
        for (write, read) in writes.iter().zip(actual) {
            let len = u16::from_le_bytes([write[4], write[5]]) as usize;
            if write[8..8 + len] != read[8..8 + len] {
                return Err("変更した設定の読み戻しが一致しません".into());
            }
        }
        Ok(())
    })();
    let close = io.send(&READ_3[100]);
    result.and(close)
}
/// 他プロファイルは種データのまま、復元対象の全項目と使用中番号を再取得する。
/// 復元照合だけに使い、全体の新規バックアップとして保存しない。
pub(crate) fn refresh_profile(
    io: &mut impl Transport,
    seed: &Snapshot,
    profile: usize,
) -> Result<Snapshot, String> {
    if profile >= 5 {
        return Err("復元プロファイルが不正です".into());
    }
    let poll = if profile < 3 { 6 } else { 7 };
    let mut positions = vec![(0, 0), (0, profile + 1), (0, poll), (1, profile)];
    positions.extend((20 * profile..20 * (profile + 1)).map(|r| (2, r)));
    let requests: Vec<_> = positions
        .iter()
        .map(|(part, row)| settings::requests(*part)[*row].clone())
        .collect();
    io.send(&READ_1[0])?;
    let result = (|| {
        let replies = read_rows(io, &requests)?;
        let mut actual = seed.clone();
        actual.has_macros = false;
        for ((part, row), reply) in positions.into_iter().zip(replies) {
            actual.parts[part][row] = reply;
        }
        Ok(actual)
    })();
    let close = io.send(&READ_3[100]);
    match (result, close) {
        (Ok(s), Ok(())) => Ok(s),
        (Err(e), _) => Err(e),
        (_, Err(e)) => Err(e),
    }
}
/// ボタン割当と共通設定をまとめて読み、Snapshotの論理行へ展開する。
fn read_classic_batched_transport(
    io: &mut impl Transport,
    macros: bool,
) -> Result<Snapshot, String> {
    io.send(&READ_1[0])?;
    let result = (|| {
        let mut parts: [Vec<Vec<u8>>; 3] = std::array::from_fn(|_| Vec::new());
        parts[0] = read_rows_limit(io, &settings::requests(0), 50)?;
        for part in 1..2 {
            for request in settings::requests(part)
                .into_iter()
                .take(if part == 1 && !macros { 5 } else { usize::MAX })
            {
                io.send(&request)?;
                let mut reply = io.get(request[0], request.len())?;
                protocol::validate_reply(&request, &reply)?;
                reply.resize(request.len(), 0);
                parts[part].push(reply);
            }
        }
        let requests = settings::requests(2);
        let mut i = 0;
        while i < requests.len() {
            let start = u16::from_le_bytes([requests[i][2], requests[i][3]]);
            let mut end = i + 1;
            while end < requests.len() {
                let addr = u16::from_le_bytes([requests[end][2], requests[end][3]]);
                if addr < start || addr + 4 > start + 48 {
                    break;
                }
                end += 1;
            }
            let last = u16::from_le_bytes([requests[end - 1][2], requests[end - 1][3]]);
            let payload = (last + 4 - start) as usize;
            let mut request = vec![0; 64];
            request[..6].copy_from_slice(&[
                3,
                0xf2,
                start as u8,
                (start >> 8) as u8,
                payload as u8,
                0,
            ]);
            io.send(&request)?;
            let reply = io.get(3, 64)?;
            protocol::validate_reply(&request, &reply)?;
            for source in &requests[i..end] {
                let address = u16::from_le_bytes([source[2], source[3]]);
                let offset = (address - start) as usize;
                let mut row = source.clone();
                row[1] = 8;
                row[2] &= 0xfe;
                row[3] |= 0x60;
                row[6] = 0xfa;
                row[7] = 0xfa;
                row[8..12].copy_from_slice(&reply[8 + offset..12 + offset]);
                parts[2].push(row);
            }
            i = end;
        }
        if macros {
            Snapshot::from_parts(parts)
        } else {
            Snapshot::from_controls(parts)
        }
    })();
    let close = io.send(&READ_3[100]);
    match (result, close) {
        (Ok(s), Ok(())) => Ok(s),
        (Err(e), _) => Err(e),
        (_, Err(e)) => Err(e),
    }
}
fn read_packed_transport(io: &mut impl Transport, macros: bool) -> Result<Snapshot, String> {
    io.send(&READ_1[0])?;
    let result = (|| {
        let mut parts: [Vec<Vec<u8>>; 3] = std::array::from_fn(|_| Vec::new());
        let mut positions = Vec::new();
        let mut requests = Vec::new();
        for part in 0..3 {
            for request in settings::requests(part)
                .into_iter()
                .take(if part == 1 && !macros { 5 } else { usize::MAX })
            {
                positions.push((part, parts[part].len()));
                parts[part].push(Vec::new());
                requests.push(request);
            }
        }
        for ((part, row), reply) in positions
            .into_iter()
            .zip(read_rows_limit(io, &requests, 54)?)
        {
            parts[part][row] = reply;
        }
        if macros {
            Snapshot::from_parts(parts)
        } else {
            Snapshot::from_controls(parts)
        }
    })();
    let close = io.send(&READ_3[100]);
    result.and_then(|snapshot| close.map(|_| snapshot))
}
pub fn read_batched_transport(io: &mut impl Transport, macros: bool) -> Result<Snapshot, String> {
    read_packed_transport(io, macros)
}
pub fn benchmark_read_packing(device: &Device) -> Result<String, String> {
    let _lock = DeviceLock::acquire()?;
    let mut io = open(device)?;
    let original = read_classic_batched_transport(&mut io, true)?;
    for trial in 1..=3 {
        for (name, packed, macros) in [
            ("従来通常", false, false),
            ("54B通常", true, false),
            ("54B全体", true, true),
        ] {
            let start = std::time::Instant::now();
            let snapshot = if packed {
                read_packed_transport(&mut io, macros)?
            } else {
                read_classic_batched_transport(&mut io, macros)?
            };
            if !(if macros {
                original.same_settings(&snapshot, false)
            } else {
                original.same_controls(&snapshot, false)
            }) {
                return Err(format!("{name}の全対象バイトが一致しません"));
            }
            let line = format!(
                "試行{trial} {name}: {:.4}秒 / 全対象バイト一致",
                start.elapsed().as_secs_f64()
            );
            println!("{line}");
        }
    }
    Ok("54B読取りの照合を完了しました".into())
}
pub fn read_transport(io: &mut impl Transport) -> Result<Snapshot, String> {
    read_batched_transport(io, true)
}
pub fn read_controls_transport(io: &mut impl Transport) -> Result<Snapshot, String> {
    read_batched_transport(io, false)
}
pub fn read_controls(device: &Device) -> Result<Snapshot, String> {
    let _lock = DeviceLock::acquire()?;
    read_controls_transport(&mut open(device)?)
}
/// 診断用。従来の193要求とまとめ読取り・通常設定を同じ実機で比較する。
pub fn benchmark_reads(device: &Device) -> Result<String, String> {
    let _lock = DeviceLock::acquire()?;
    let mut io = open(device)?;
    let t = std::time::Instant::now();
    let legacy = read_legacy_transport(&mut io)?;
    let old = t.elapsed();
    let t = std::time::Instant::now();
    let full = read_transport(&mut io)?;
    let full_time = t.elapsed();
    let t = std::time::Instant::now();
    let controls = read_controls_transport(&mut io)?;
    let normal_time = t.elapsed();
    if !legacy.same_settings(&full, false) || !legacy.same_controls(&controls, false) {
        return Err("まとめ読取りと従来の読取りが一致しません".into());
    }
    Ok(format!(
        "読取り一致: 従来={:.3}秒 / 全体まとめ={:.3}秒 / 通常設定={:.3}秒",
        old.as_secs_f64(),
        full_time.as_secs_f64(),
        normal_time.as_secs_f64()
    ))
}
pub fn read(device: &Device) -> Result<Snapshot, String> {
    let _lock = DeviceLock::acquire()?;
    read_transport(&mut open(device)?)
}
pub fn read_raw(device: &Device) -> Result<String, String> {
    read(device).map(|s| s.to_text())
}
pub fn probe(device: &Device) -> Result<String, String> {
    let snapshot = read(device)?;
    summary(&snapshot)
}
pub fn summary(s: &Snapshot) -> Result<String, String> {
    let mut text = format!("使用中: プロファイル{}\n", s.active() + 1);
    for (i, p) in s.profiles()?.iter().enumerate() {
        text.push_str(&format!(
            "P{}: DPI={:?} 有効={:?} / {} Hz / LED={:02X}{:02X}{:02X} {} 明るさ{} 速度{}\n",
            i + 1,
            p.dpi,
            p.enabled,
            p.poll,
            p.color[0],
            p.color[1],
            p.color[2],
            crate::model::EFFECTS[p.effect],
            p.brightness,
            p.speed
        ));
    }
    Ok(text)
}

pub fn save_raw(path: &Path, text: &str) -> Result<(), String> {
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(text.as_bytes())
        .and_then(|_| f.sync_all())
        .map_err(|e| e.to_string())
}
/// 完全な記録を同期してから公開する。既存の復元記録は置換しない。
pub(crate) fn save_raw_atomic_new(path: &Path, text: &str) -> Result<(), String> {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let temp = path.with_extension(format!("journal-new-{}-{stamp}", std::process::id()));
    // 作成に成功したファイルだけを後始末する。
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    let outcome = (|| {
        file.write_all(text.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            let source: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
            let destination: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            // WRITE_THROUGH。REPLACE_EXISTINGは指定しない。
            if unsafe { mutex::MoveFileExW(source.as_ptr(), destination.as_ptr(), 8) } == 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
        }
        #[cfg(not(windows))]
        std::fs::hard_link(&temp, path).map_err(|e| e.to_string())?;
        Ok(())
    })();
    let _ = std::fs::remove_file(temp);
    outcome
}
pub fn backup(directory: &Path, s: &Snapshot, kind: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let path = directory.join(format!("{kind}-{stamp}.m908"));
    save_raw(&path, &s.to_text())?;
    Ok(path)
}
#[derive(Clone, Debug)]
pub struct Applied {
    pub snapshot: Snapshot,
    pub backup: Option<PathBuf>,
    pub changes: Vec<String>,
}

/// 書き込みの直前に読み直し、競合確認→永続バックアップ→送信→読み戻しを必ず行う。
pub fn apply_transport(
    io: &mut impl Transport,
    base: &Snapshot,
    profile: usize,
    target: &Profile,
    directory: &Path,
) -> Result<Applied, String> {
    // 未対応のキーなどは通信の前に検出する。
    settings::plan(base, profile, target)?;
    let current = read_transport(io)?;
    if !base.same_profile(&current, profile) {
        return Err("実機の設定が読込後に変更されています。再読み込みしてから編集してください。書き込みは行っていません。".into());
    }
    let plan = settings::plan(&current, profile, target)?;
    if plan.packets.is_empty() {
        return Ok(Applied {
            snapshot: current,
            backup: None,
            changes: Vec::new(),
        });
    }
    let backup = backup(directory, &current, "auto")?;
    let result = (|| {
        let mut sent_begin = false;
        for packet in compact_writes(plan.packets.clone()) {
            if let Err(e) = io.send(&packet) {
                if sent_begin {
                    let _ = io.send(&READ_3[100]);
                }
                return Err(e);
            }
            sent_begin = true;
        }
        let actual = read_transport(io)?;
        if !plan.expected.same_settings(&actual, false) {
            return Err("読み戻しが予定した設定と一致しません。成功とは扱いません。".into());
        }
        Ok(actual)
    })();
    match result{Ok(snapshot)=>Ok(Applied{snapshot,backup:Some(backup),changes:plan.changes}),Err(e)=>Err(format!("{e}\n書き込み前バックアップ: {}\n実機を再読み込みしてください。自動で再送は行いません。",backup.display()))}
}
pub fn apply(
    device: &Device,
    base: &Snapshot,
    profile: usize,
    target: &Profile,
    directory: &Path,
) -> Result<Applied, String> {
    let _lock = DeviceLock::acquire()?;
    apply_transport(&mut open(device)?, base, profile, target, directory)
}
/// 通常設定の保存。マクロを含めないことをファイル形式でも明示する。
pub fn apply_controls_transport(
    io: &mut impl Transport,
    base: &Snapshot,
    profile: usize,
    target: &Profile,
    directory: &Path,
) -> Result<Applied, String> {
    settings::plan(base, profile, target)?;
    let current = read_controls_transport(io)?;
    if !base.same_profile(&current, profile) {
        return Err("実機の対象プロファイルが変更されています。再読み込みしてから編集してください。書き込みは行っていません。".into());
    }
    let plan = settings::plan(&current, profile, target)?;
    if plan.packets.is_empty() {
        return Ok(Applied {
            snapshot: current,
            backup: None,
            changes: Vec::new(),
        });
    }
    let path = backup(directory, &current, "settings")?;
    let result: Result<Snapshot, String> = (|| {
        for packet in compact_writes(plan.packets.clone()) {
            if let Err(e) = io.send(&packet) {
                let _ = io.send(&READ_3[100]);
                return Err(e);
            }
        }
        verify_writes(io, &plan.packets)?;
        Ok(plan.expected.clone())
    })();
    result
        .map(|snapshot| Applied {
            snapshot,
            backup: Some(path.clone()),
            changes: plan.changes,
        })
        .map_err(|e| {
            format!(
                "{e}\n変更前の通常設定バックアップ: {}\n再読み込みで状態を確認してください。",
                path.display()
            )
        })
}
pub fn apply_controls(
    device: &Device,
    base: &Snapshot,
    profile: usize,
    target: &Profile,
    directory: &Path,
) -> Result<Applied, String> {
    let _lock = DeviceLock::acquire()?;
    apply_controls_transport(&mut open(device)?, base, profile, target, directory)
}
pub fn multi_key_save_self_test(device: &Device, directory: &Path) -> Result<String, String> {
    let _lock = DeviceLock::acquire()?;
    let mut io = open(device)?;
    let original = read_transport(&mut io)?;
    backup(directory, &original, "before-multi-key-save")?;
    let borrowed = (original.active() + 4) % 5;
    let path = directory.join(crate::identification::JOURNAL);
    let journal = crate::identification::Journal {
        device: device.clone(),
        borrowed,
        original: original.clone(),
    };
    let mut target = original.profiles()?[borrowed].clone();
    for slot in 6..18 {
        let action = format!("Ctrl + Shift + F{}", slot + 7);
        target.buttons[slot] =
            if crate::actions::encode(&action)? == crate::actions::encode(&target.buttons[slot])? {
                format!("Alt + F{}", slot + 7)
            } else {
                action
            };
    }
    let plan = settings::plan(&original, borrowed, &target)?;
    journal.save(&path)?;
    let start = std::time::Instant::now();
    let outcome = apply_controls_transport(&mut io, &original, borrowed, &target, directory);
    let elapsed = start.elapsed().as_secs_f64();
    let actual = crate::identification::restore_transport(&mut io, &journal, &plan.expected)?;
    if !original.same_settings(&actual, false) {
        return Err("複数キー保存診断後の全設定・マクロ・使用中番号が一致しません".into());
    }
    std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    outcome?;
    Ok(format!("12ボタンの保存・全変更領域照合: {elapsed:.4}秒 / F3送信12件→1件 / 全プロファイル・マクロ・使用中番号の復元一致"))
}
pub fn fast_save_self_test(device: &Device, directory: &Path) -> Result<String, String> {
    let _lock = DeviceLock::acquire()?;
    let mut io = open(device)?;
    let original = read_transport(&mut io)?;
    let p = (original.active() + 4) % 5;
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let path = directory.join(crate::identification::JOURNAL);
    let journal = crate::identification::Journal {
        device: device.clone(),
        borrowed: p,
        original: original.clone(),
    };
    let mut target = original.profiles()?[p].clone();
    target.color[2] ^= 1;
    journal.save(&path)?;
    let t = std::time::Instant::now();
    let outcome = apply_controls_transport(&mut io, &original, p, &target, directory);
    let elapsed = t.elapsed();
    // 失敗した場合も、診断前の生バイトへ復元する。確認が済むまで記録を残す。
    let expected = settings::plan(&original, p, &target)?.expected;
    let final_state = crate::identification::restore_transport(&mut io, &journal, &expected)?;
    if !original.same_settings(&final_state, false) {
        return Err(format!(
            "高速保存の診断後に全設定が一致しません。復元情報: {}",
            path.display()
        ));
    }
    std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    outcome?;
    Ok(format!("通常設定の保存・競合確認・バックアップ・読戻し: {:.3}秒。診断後に全プロファイル・マクロ・使用中プロファイルの復元一致を確認しました。",elapsed.as_secs_f64()))
}
pub fn switch_controls(
    device: &Device,
    base: &Snapshot,
    profile: usize,
    directory: &Path,
) -> Result<Applied, String> {
    let packets = settings::switch_packets(profile)?;
    let _lock = DeviceLock::acquire()?;
    let mut io = open(device)?;
    let current = read_controls_transport(&mut io)?;
    if !base.same_controls(&current, false) {
        return Err("実機設定が変更されています。再読み込みしてください".into());
    }
    if current.active() == profile {
        return Ok(Applied {
            snapshot: current,
            backup: None,
            changes: Vec::new(),
        });
    }
    let path = backup(directory, &current, "settings-switch")?;
    for packet in &packets {
        io.send(&packet)?;
    }
    verify_writes(&mut io, &packets).map_err(|e| format!("{e}\n切替前: {}", path.display()))?;
    let mut actual = current;
    actual.parts[0][0][8] = profile as u8;
    Ok(Applied {
        snapshot: actual,
        backup: Some(path),
        changes: vec![format!("プロファイル{}を使用", profile + 1)],
    })
}
pub fn switch_transport(
    io: &mut impl Transport,
    base: &Snapshot,
    profile: usize,
    directory: &Path,
) -> Result<Applied, String> {
    let packets = settings::switch_packets(profile)?;
    let current = read_transport(io)?;
    if !base.same_settings(&current, false) {
        return Err("実機設定が変更されています。再読み込みしてから切り替えてください。".into());
    }
    if current.active() == profile {
        return Ok(Applied {
            snapshot: current,
            backup: None,
            changes: Vec::new(),
        });
    }
    let path = backup(directory, &current, "switch")?;
    let result: Result<Snapshot, String> = (|| {
        for packet in packets {
            io.send(&packet)?;
        }
        let actual = read_transport(io)?;
        if actual.active() != profile || !current.same_settings(&actual, true) {
            return Err("プロファイル切替の読み戻しが一致しません".into());
        }
        Ok(actual)
    })();
    result
        .map(|snapshot| Applied {
            snapshot,
            backup: Some(path.clone()),
            changes: vec![format!("プロファイル{}を使用", profile + 1)],
        })
        .map_err(|e| format!("{e}\n切替前バックアップ: {}", path.display()))
}
pub fn switch(
    device: &Device,
    base: &Snapshot,
    profile: usize,
    directory: &Path,
) -> Result<Applied, String> {
    let _lock = DeviceLock::acquire()?;
    switch_transport(&mut open(device)?, base, profile, directory)
}

/// 明示的な診断コマンド専用。一時変更し、終了前に元の設定へ戻す。
pub fn self_test(
    device: &Device,
    directory: &Path,
    mut progress: impl FnMut(&str),
) -> Result<String, String> {
    let _lock = DeviceLock::acquire()?;
    let mut io = open(device)?;
    progress("実機の元の設定を読み取り、診断前バックアップを保存しています…");
    let original = read_transport(&mut io)?;
    let golden = backup(directory, &original, "before-self-test")?;
    let profiles = original.profiles()?;
    let p = (original.active() + 4) % 5;
    let mut target = profiles[p].clone();
    target.dpi[0] = if target.dpi[0] == 1100 { 1000 } else { 1100 };
    target.enabled[0] = true;
    if target.enabled[..4].iter().any(|v| *v) {
        target.enabled[4] = !target.enabled[4];
    }
    target.poll = if target.poll == 1000 { 500 } else { 1000 };
    target.color[2] ^= 1;
    target.effect = (target.effect + 1) % 11;
    target.brightness = if target.brightness == 3 { 2 } else { 3 };
    target.speed = if target.speed == 8 { 7 } else { 8 };
    target.buttons[17] = if target.buttons[17] == "F12" {
        "F11".into()
    } else {
        "F12".into()
    };
    progress(&format!("未使用のプロファイル{}でLED・DPI・ポーリング・キー割当を一時変更し、読み戻しを検証しています…",p+1));
    let outcome = apply_transport(&mut io, &original, p, &target, directory);
    let changed = match &outcome {
        Ok(a) => a.snapshot.clone(),
        Err(_) => read_transport(&mut io).map_err(|e| {
            format!(
                "診断中の失敗後、再読み取りも失敗しました: {e}\n元の設定: {}",
                golden.display()
            )
        })?,
    };
    progress("診断前の設定へ戻しています…");
    let restored = if original.same_profile(&changed, p) {
        changed
    } else {
        apply_transport(&mut io, &changed, p, &profiles[p], directory)
            .map_err(|e| {
                format!(
                    "元に戻せませんでした: {e}\n診断前バックアップ: {}",
                    golden.display()
                )
            })?
            .snapshot
    };
    if !original.same_settings(&restored, false) {
        return Err(format!(
            "復元後の全領域照合に失敗しました。診断前バックアップ: {}",
            golden.display()
        ));
    }
    if let Err(e) = outcome {
        return Err(format!(
            "書き込み診断は失敗しましたが、元の設定への復元は確認できました。\n{e}"
        ));
    }
    progress("設定の書き込みと元の状態への復元を確認しました。プロファイル切替を検証しています…");
    let other = (original.active() + 1) % 5;
    let switched = switch_transport(&mut io, &restored, other, directory);
    let current = match &switched {
        Ok(a) => a.snapshot.clone(),
        Err(_) => read_transport(&mut io).map_err(|e| {
            format!(
                "切替診断後の読み取りに失敗しました: {e}\n元の設定: {}",
                golden.display()
            )
        })?,
    };
    let final_state = if current.active() == original.active() {
        current
    } else {
        switch_transport(&mut io, &current, original.active(), directory)
            .map_err(|e| {
                format!(
                    "使用中プロファイルを元に戻せません: {e}\n元の設定: {}",
                    golden.display()
                )
            })?
            .snapshot
    };
    if !original.same_settings(&final_state, false) {
        return Err(format!(
            "最終照合に失敗しました。元の設定: {}",
            golden.display()
        ));
    }
    switched?;
    progress("全プロファイル・マクロ領域・使用中プロファイルが診断前と一致しました。");
    Ok(format!(
        "実機診断に合格しました。診断前バックアップ: {}\n{}",
        golden.display(),
        summary(&final_state)?
    ))
}

//! 編集時だけ使う識別用プロファイル。復元ジャーナルは最初の書込みより先に永続化する。
use crate::{
    device::{self, Device, DeviceLock, Transport},
    input::Event,
    protocol::{self, READ_1, READ_3, WRITE_CONFIG, WRITE_LED},
    settings::{self, Snapshot},
};
use std::{
    ffi::CString,
    io::{BufRead, Write},
    path::{Path, PathBuf},
    process::{ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

pub const JOURNAL: &str = "m908-identification.pending";
// キー表に存在するF13〜F24とF1〜F3を、修飾キーなしで重複なく使用する。
pub const SLOTS: [usize; 15] = [6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 3, 4, 5];
pub const MARKERS: [&str; 15] = [
    "F13", "F14", "F15", "F16", "F17", "F18", "F19", "F20", "F21", "F22", "F23", "F24", "F1", "F2",
    "F3",
];

pub fn marker_slot(vkey: u16) -> Option<usize> {
    let n = match vkey {
        0x7c..=0x87 => (vkey - 0x7c) as usize,
        0x70..=0x72 => 12 + (vkey - 0x70) as usize,
        _ => return None,
    };
    Some(SLOTS[n])
}

#[derive(Clone, Debug)]
pub struct Journal {
    pub device: Device,
    pub borrowed: usize,
    pub original: Snapshot,
}
fn hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02X}")).collect()
}
fn unhex(text: &str) -> Result<Vec<u8>, String> {
    if !text.is_ascii() || text.len() % 2 != 0 || text.len() > 32768 {
        return Err("復元情報のパスが不正です".into());
    }
    (0..text.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&text[i..i + 2], 16).map_err(|_| "復元情報の16進数が不正です".into())
        })
        .collect()
}
impl Journal {
    fn text(&self) -> String {
        let body = format!(
            "{}\n{}\n{}",
            hex(self.device.path.as_bytes()),
            self.borrowed,
            self.original.to_text()
        );
        format!(
            "{}\n{:08X}\n{body}",
            if self.original.has_macros {
                "M908-IDENTIFICATION-1"
            } else {
                "M908-IDENTIFICATION-2"
            },
            crc32fast::hash(body.as_bytes())
        )
    }
    fn parse(s: &str) -> Result<Self, String> {
        let mut top = s.splitn(3, '\n');
        let controls = match top.next() {
            Some("M908-IDENTIFICATION-1") => false,
            Some("M908-IDENTIFICATION-2") => true,
            _ => return Err("復元情報の形式が違います".into()),
        };
        let crc = top
            .next()
            .and_then(|s| u32::from_str_radix(s, 16).ok())
            .ok_or("復元情報のCRCが不正です")?;
        let body = top.next().ok_or("復元情報がありません")?;
        if crc32fast::hash(body.as_bytes()) != crc {
            return Err(
                "復元情報の整合性チェックに失敗しました。ファイルを保持して確認してください。"
                    .into(),
            );
        }
        Self::parse_body(body, controls)
    }
    fn parse_body(s: &str, controls: bool) -> Result<Self, String> {
        let mut fields = s.splitn(3, '\n');
        let path = CString::new(unhex(fields.next().ok_or("復元対象がありません")?)?)
            .map_err(|e| e.to_string())?;
        let borrowed = fields
            .next()
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|p| *p < 5)
            .ok_or("復元プロファイルが不正です")?;
        let original = Snapshot::from_text(fields.next().ok_or("復元設定がありません")?)?;
        if !original.has_macros && !controls {
            return Err("識別用の復元情報には完全バックアップが必要です".into());
        }
        if borrowed == original.active() {
            return Err("復元対象と元の使用中プロファイルが同じです".into());
        }
        Ok(Self {
            device: Device {
                path,
                name: "Redragon M908".into(),
                interface: 2,
                usage_page: 0xffa0,
                usage: 1,
            },
            borrowed,
            original,
        })
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > 120000 {
            return Err("復元情報が大きすぎます".into());
        }
        Self::parse(&std::fs::read_to_string(path).map_err(|e| e.to_string())?)
    }
    pub(crate) fn save(&self, path: &Path) -> Result<(), String> {
        // create_new により、未完了の復元情報を決して上書きしない。
        if path.exists() {
            return Err("未完了の復元情報があります".into());
        }
        let _gate = target_lock(path)?;
        if path.exists() {
            return Err("未完了の復元情報があります".into());
        }
        remove_if_exists(&target_path(path))?;
        remove_if_exists(&path.with_extension("restoring"))?;
        device::save_raw_atomic_new(path, &self.text())
    }
}

fn target_path(path: &Path) -> PathBuf {
    path.with_extension("target")
}
fn target_lock(path: &Path) -> Result<DeviceLock, String> {
    let hash = crc32fast::hash(path.to_string_lossy().to_lowercase().as_bytes());
    DeviceLock::acquire_named(&format!("Local\\M908Ctl.RestoreTarget{hash:08X}"), 1000)
}
fn remove_if_exists(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
fn load_target(path: &Path, journal: &Journal) -> Result<Snapshot, String> {
    let path = target_path(path);
    if !path.exists() {
        return Ok(journal.original.clone());
    }
    if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 120000 {
        return Err("復元先の設定が大きすぎます".into());
    }
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut fields = text.splitn(3, '\n');
    if fields.next() != Some("M908-RESTORE-TARGET-1")
        || fields.next()
            != Some(format!("{:08X}", crc32fast::hash(journal.text().as_bytes())).as_str())
    {
        return Err("復元先と退避した設定の組み合わせが一致しません".into());
    }
    let target = Snapshot::from_text(fields.next().ok_or("復元先の設定がありません")?)?;
    // 更新を許すのは通常のキー割当だけ。未知の値・マクロ・DPI等は退避値を維持。
    let mut allowed = journal.original.clone();
    for (row, target_row) in allowed.parts[2].iter_mut().zip(&target.parts[2]) {
        row[8..12].copy_from_slice(&target_row[8..12]);
    }
    if allowed != target {
        return Err("復元先にキー割当以外の変更が含まれています".into());
    }
    Ok(target)
}

/// USBへは送らず、通常プロファイルへ戻す最終的な割当だけを永続化する。
pub fn stage_keys(
    path: &Path,
    base: &Snapshot,
    profile: usize,
    buttons: &[String; 20],
) -> Result<(), String> {
    let _gate = target_lock(path)?;
    if path.with_extension("restoring").exists() {
        return Err("復元先は確定済みです。復元の完了を待ってください".into());
    }
    let journal = Journal::load(path)?;
    if profile >= 5 || !journal.original.same_profile(base, profile) {
        return Err("編集前の設定が退避した設定と一致しません".into());
    }
    let mut target = load_target(path, &journal)?;
    for (slot, action) in buttons.iter().enumerate() {
        let code = crate::actions::encode(action)?;
        target.parts[2][20 * profile + slot][8..12].copy_from_slice(&code);
    }
    if !target_path(path).exists() {
        device::backup(
            &path.with_file_name("backups"),
            &journal.original,
            "before-key-edit",
        )?;
    }
    let text = format!(
        "M908-RESTORE-TARGET-1\n{:08X}\n{}",
        crc32fast::hash(journal.text().as_bytes()),
        target.to_text()
    );
    let temp = path.with_extension("target.new");
    let mut file = std::fs::File::create(&temp).map_err(|e| e.to_string())?;
    file.write_all(text.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    drop(file);
    // 同じディレクトリ内で置換するため、監視側には更新前か更新後の完全文書だけが見える。
    std::fs::rename(&temp, target_path(path)).map_err(|e| e.to_string())
}
fn freeze_target(path: &Path, journal: &Journal) -> Result<Snapshot, String> {
    let _gate = target_lock(path)?;
    if !path.with_extension("restoring").exists() {
        device::save_raw(&path.with_extension("restoring"), "M908-RESTORING-1")?;
    }
    load_target(path, journal)
}
fn finish_restore(path: &Path) -> Result<(), String> {
    let _gate = target_lock(path)?;
    // ジャーナル削除前に復元先を消さない。途中終了しても最終的な割当を復旧できる。
    std::fs::remove_file(path).map_err(|e| e.to_string())?;
    remove_if_exists(&target_path(path))?;
    remove_if_exists(&path.with_extension("restoring"))
}

/// 通常側を最終設定へ更新してから切り替え、借用先も最終設定へ直接戻す。
fn restore_target_transport(
    io: &mut impl Transport,
    journal: &Journal,
    current: &Snapshot,
    target: &Snapshot,
) -> Result<Snapshot, String> {
    let mut source = current.clone();
    // 準備中の失敗でも「元の値/準備後の値」のどちらからも最終設定へ到達する。
    for slot in 0..100 {
        if journal.original.parts[2][slot][8..12] != target.parts[2][slot][8..12] {
            source.parts[2][slot][8..12].copy_from_slice(&journal.original.parts[2][slot][8..12]);
        }
    }
    let active = target.active();
    let mut restored = current.clone();
    for p in
        std::iter::once(active).chain((0..5).filter(|p| *p != active && *p != journal.borrowed))
    {
        if !journal.original.same_profile(target, p) {
            // 通常側は識別中に編集したキーだけを重ねる。USB切断中の別の変更は保持する。
            let mut expected = restored.clone();
            for slot in 20 * p..20 * (p + 1) {
                let before = &journal.original.parts[2][slot][8..12];
                let after = &target.parts[2][slot][8..12];
                if before != after {
                    expected.parts[2][slot][8..12].copy_from_slice(after);
                }
            }
            send(io, restore_packets(&restored, &expected, p)?)?;
            restored = device::refresh_profile(io, &expected, p)?;
            if !expected.same_profile(&restored, p) {
                return Err("通常プロファイルの最終設定が読み戻しと一致しません".into());
            }
        }
        if p == active {
            send(io, settings::switch_packets(active)?)?;
        }
    }
    send(io, restore_packets(&source, target, journal.borrowed)?)?;
    let actual = device::refresh_profile(io, &restored, journal.borrowed)?;
    if actual.active() != target.active() || !target.same_profile(&actual, journal.borrowed) {
        return Err("識別用プロファイルの復元後の照合に失敗しました".into());
    }
    Ok(actual)
}

/// 変更する範囲だけを生バイトで戻す。無効DPIの未知コードやraw割当も丸めない。
pub fn restore_packets(
    current: &Snapshot,
    original: &Snapshot,
    p: usize,
) -> Result<Vec<Vec<u8>>, String> {
    if p >= 5 {
        return Err("復元プロファイルが不正です".into());
    }
    let mut packets = Vec::new();
    let mut field = |mut packet: Vec<u8>, before: &[u8], after: &[u8]| {
        if before != after {
            packet[8..8 + after.len()].copy_from_slice(after);
            packets.push(packet);
        }
    };
    field(
        WRITE_LED[3 + 2 * p].to_vec(),
        &current.parts[0][p + 1][8..14],
        &original.parts[0][p + 1][8..14],
    );
    field(
        WRITE_LED[4 + 2 * p].to_vec(),
        &current.parts[0][p + 1][14..15],
        &original.parts[0][p + 1][14..15],
    );
    let (row, off, address) = if p < 3 {
        (6, 8 + 2 * p, 0x32 + 2 * p)
    } else {
        (7, 8 + 2 * (p - 3), 0x38 + 2 * (p - 3))
    };
    let mut poll = vec![0; 16];
    poll[..6].copy_from_slice(&[2, 0xf3, address as u8, 0, 1, 0]);
    field(
        poll,
        &current.parts[0][row][off..off + 1],
        &original.parts[0][row][off..off + 1],
    );
    for j in 0..5 {
        let off = 10 + 6 * j;
        field(
            WRITE_CONFIG[7 + 5 * j + p].to_vec(),
            &current.parts[1][p][off..off + 4],
            &original.parts[1][p][off..off + 4],
        );
    }
    for j in 0..20 {
        field(
            WRITE_CONFIG[35 + 20 * p + j].to_vec(),
            &current.parts[2][20 * p + j][8..12],
            &original.parts[2][20 * p + j][8..12],
        );
    }
    if !packets.is_empty() {
        packets.insert(0, READ_1[0].to_vec());
        packets.extend(WRITE_CONFIG[135..139].iter().map(|r| r.to_vec()));
        packets.push(READ_3[100].to_vec());
    }
    Ok(packets)
}

pub fn prepared(original: &Snapshot, p: usize) -> Result<settings::WritePlan, String> {
    if p >= 5 || p == original.active() {
        return Err("識別用には別プロファイルを使用してください".into());
    }
    let mut target = original.profiles()?[original.active()].clone();
    for (slot, name) in SLOTS.into_iter().zip(MARKERS) {
        target.buttons[slot] = name.into();
    }
    // 編集中は通常クリックとホイールを必ず残す。元の割当は復元ジャーナルに保持。
    for (slot, name) in [
        (0, "左クリック"),
        (1, "右クリック"),
        (2, "ホイール押下"),
        (18, "ホイール上"),
        (19, "ホイール下"),
    ] {
        target.buttons[slot] = name.into();
    }
    settings::plan(original, p, &target)
}

fn best_prepared(original: &Snapshot) -> Result<(usize, settings::WritePlan), String> {
    let mut best = None;
    let mut error = "識別用プロファイルを選べません".to_string();
    for distance in [4, 3, 2, 1] {
        let p = (original.active() + distance) % 5;
        let plan = match prepared(original, p) {
            Ok(plan) => plan,
            Err(e) => {
                error = e;
                continue;
            }
        };
        let cost = device::compact_writes(plan.packets.clone()).len();
        if best
            .as_ref()
            .is_none_or(|(_, _, old_cost)| cost < *old_cost)
        {
            best = Some((p, plan, cost));
        }
    }
    let (p, plan, _) = best.ok_or(error)?;
    Ok((p, plan))
}
struct Checked<'a, T, F> {
    io: &'a mut T,
    keep: &'a F,
}
impl<T: Transport, F: Fn() -> bool> Transport for Checked<'_, T, F> {
    fn send(&mut self, row: &[u8]) -> Result<(), String> {
        if !(self.keep)() {
            return Err("識別モードを終了します".into());
        }
        self.io.send(row)
    }
    fn get(&mut self, id: u8, len: usize) -> Result<Vec<u8>, String> {
        if !(self.keep)() {
            return Err("識別モードを終了します".into());
        }
        self.io.get(id, len)
    }
}
fn send(io: &mut impl Transport, packets: impl IntoIterator<Item = Vec<u8>>) -> Result<(), String> {
    for p in device::compact_writes(packets) {
        io.send(&p)?
    }
    Ok(())
}

pub fn restore_transport(
    io: &mut impl Transport,
    j: &Journal,
    current: &Snapshot,
) -> Result<Snapshot, String> {
    // 長い全体読取りより先に通常割当へ戻す。途中失敗でもジャーナルを残す。
    send(io, settings::switch_packets(j.original.active())?)?;
    for packet in restore_packets(current, &j.original, j.borrowed)? {
        io.send(&packet)?;
    }
    let actual = device::read_transport(io)?;
    if !j.original.same_profile(&actual, j.borrowed) || actual.active() != j.original.active() {
        return Err("識別用プロファイルの復元後の照合に失敗しました".into());
    }
    Ok(actual)
}

/// 起動時・再試行時の復元。対象USBパスが同じ実機だけを開く。
pub fn recover(path: &Path) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }
    let _lock = DeviceLock::acquire()?;
    let j = Journal::load(path)?;
    let mut io=device::open(&j.device).map_err(|e|format!("未完了の識別設定を復元する必要があります。同じUSB端子にM908を接続して再読み込みしてください。\n{e}"))?;
    recover_transport(&mut io, path)?;
    Ok(true)
}
pub(crate) fn recover_transport(io: &mut impl Transport, path: &Path) -> Result<Snapshot, String> {
    let j = Journal::load(path)?;
    let target = freeze_target(path, &j)?;
    let current = device::read_controls_transport(io)?;
    let actual = restore_target_transport(io, &j, &current, &target)?;
    finish_restore(path)?;
    Ok(actual)
}

/// 独立プロセスがロックを保持し、親の終了・応答停止・フォーカス移動でも復元する。
pub fn run_session(
    device: &Device,
    path: &Path,
    keep: impl Fn() -> bool,
    mut notify: impl FnMut(&str),
) -> Result<(), String> {
    let _lock = DeviceLock::acquire()?;
    let mut io = device::open(device)?;
    session_transport(&mut io, device, path, keep, &mut notify)
}
pub(crate) fn session_transport(
    io: &mut impl Transport,
    device: &Device,
    path: &Path,
    keep: impl Fn() -> bool,
    mut notify: impl FnMut(&str),
) -> Result<(), String> {
    if path.exists() {
        return Err("未完了の復元情報があります。先に再読み込みしてください".into());
    }
    let original = match device::read_controls_transport(&mut Checked { io, keep: &keep }) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            // 読取り途中のフォーカス喪失では、本体設定を書き換えていない。
            let _ = io.send(&READ_3[100]);
            if !keep() {
                notify("CANCELLED");
                return Ok(());
            }
            return Err(error);
        }
    };
    if !keep() {
        notify("CANCELLED");
        return Ok(());
    }
    let (borrowed, plan) = best_prepared(&original)?;
    let j = Journal {
        device: device.clone(),
        borrowed,
        original,
    };
    j.save(path)?;
    let result: Result<(), String> = (|| {
        let mut checked = Checked { io, keep: &keep };
        send(&mut checked, plan.packets.clone())?;
        // 一時変更した借用先の全項目を読戻す。完全バックアップは取得済み。
        let verified = device::refresh_profile(&mut checked, &plan.expected, borrowed)?;
        if !plan.expected.same_profile(&verified, borrowed)
            || verified.active() != j.original.active()
        {
            return Err("識別用設定の読み戻しが一致しません".into());
        }
        send(&mut checked, settings::switch_packets(borrowed)?)?;
        // 使用中プロファイルを短い単一要求で検証。
        checked.send(&READ_1[0])?;
        checked.send(&READ_1[1])?;
        let reply = checked.get(2, 16)?;
        protocol::validate_reply(&READ_1[1], &reply)?;
        checked.send(&READ_3[100])?;
        if reply[8] as usize != borrowed {
            return Err("識別用プロファイルへ切り替えられませんでした".into());
        }
        notify("READY");
        while keep() {
            std::thread::park_timeout(Duration::from_millis(20));
        }
        Ok(())
    })();
    notify("RESTORING");
    let restored: Result<Snapshot, String> = (|| {
        let target = freeze_target(path, &j)?;
        restore_target_transport(io, &j, &plan.expected, &target)
    })();
    match restored {
        Ok(snapshot) => {
            finish_restore(path)?;
            if let Err(e) = result {
                if keep() {
                    return Err(format!("{e}（元の設定への復元は確認済み）"));
                }
            }
            notify(&format!("SNAPSHOT {}", hex(snapshot.to_text().as_bytes())));
            Ok(())
        }
        Err(e) => Err(format!(
            "復元が完了していません: {e}\n再接続後に起動して復元してください。復元情報: {}",
            path.display()
        )),
    }
}

#[cfg(windows)]
pub fn foreground_process() -> u32 {
    unsafe {
        use windows_sys::Win32::UI::WindowsAndMessaging::*;
        let mut pid = 0;
        GetWindowThreadProcessId(GetForegroundWindow(), &mut pid);
        pid
    }
}
#[cfg(not(windows))]
pub fn foreground_process() -> u32 {
    0
}

#[derive(Debug)]
pub enum State {
    Ready,
    Restoring,
    Restored,
    Cancelled,
    Button(usize, bool),
    ButtonMask(u32),
    Snapshot(Snapshot),
    Failed(String),
}
pub struct Client {
    pub events: Receiver<State>,
    stdin: Option<ChildStdin>,
    last_ping: Instant,
    pub stopping: bool,
}
impl Client {
    pub fn start(exe: &Path, device: &Device, journal: &Path) -> Result<Self, String> {
        Self::start_with_wake(exe, device, journal, || {})
    }
    pub fn start_with_wake(
        exe: &Path,
        device: &Device,
        journal: &Path,
        wake: impl Fn() + Send + 'static,
    ) -> Result<Self, String> {
        let mut cmd = Command::new(exe);
        cmd.arg(WORKER_FLAG)
            .arg(journal)
            .arg(std::process::id().to_string())
            .arg(hex(device.path.as_bytes()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("復元監視プログラムを起動できません: {e}"))?;
        let stdin = child.stdin.take();
        let stdout = child
            .stdout
            .take()
            .ok_or("監視プログラムの応答を取得できません")?;
        let (tx, events) = mpsc::channel();
        std::thread::spawn(move || {
            let mut terminal = false;
            let mut cancelled = false;
            for line in std::io::BufReader::new(stdout).lines() {
                match line {
                    Ok(s) => {
                        if terminal {
                            continue;
                        }
                        let state = match s.as_str() {
                            "READY" => Some(State::Ready),
                            "RESTORING" => Some(State::Restoring),
                            "DONE" => {
                                terminal = true;
                                Some(if cancelled {
                                    State::Cancelled
                                } else {
                                    State::Restored
                                })
                            }
                            "CANCELLED" => {
                                // 通信ロックの解放と監視の終了を表すDONEまで待つ。
                                // 素早いフォーカス復帰で次の監視を早く作りすぎない。
                                cancelled = true;
                                Some(State::Restoring)
                            }
                            _ => parse_button(&s).or_else(|| parse_button_mask(&s)).or_else(|| parse_snapshot(&s)).or_else(|| {
                                s.strip_prefix("ERROR ").map(|v| {
                                    terminal = true;
                                    State::Failed(
                                        unhex(v)
                                            .map(|v| String::from_utf8_lossy(&v).into_owned())
                                            .unwrap_or_else(|e| e),
                                    )
                                })
                            }),
                        };
                        if matches!(state, Some(State::Failed(_))) {
                            terminal = true;
                        }
                        if let Some(state) = state {
                            let _ = tx.send(state);
                            wake();
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = child.wait();
            if !terminal {
                let _ = tx.send(State::Failed(
                    "復元監視が終了しました。再読み込みで保存済み情報から復元してください。".into(),
                ));
                wake();
            }
        });
        let mut client = Self {
            events,
            stdin,
            last_ping: Instant::now(),
            stopping: false,
        };
        client.ping_now()?;
        Ok(client)
    }
    fn ping_now(&mut self) -> Result<(), String> {
        self.stdin
            .as_mut()
            .ok_or("監視接続が閉じています")?
            .write_all(b"PING\n")
            .map_err(|e| e.to_string())?;
        self.last_ping = Instant::now();
        Ok(())
    }
    pub fn heartbeat(&mut self) {
        if !self.stopping && self.last_ping.elapsed() > Duration::from_millis(200) {
            if self.ping_now().is_err() {
                self.stop();
            }
        }
    }
    pub fn stop(&mut self) {
        self.stopping = true;
        self.stdin.take();
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        self.stop();
    }
}

enum OutputLine {
    Status(String),
    Button(String),
    ButtonMask,
}

#[derive(Default)]
struct ButtonQueue {
    pending: usize,
    resync: bool,
    held: u32,
}

#[derive(Clone)]
struct WorkerOutput {
    tx: mpsc::Sender<OutputLine>,
    buttons: std::sync::Arc<std::sync::Mutex<ButtonQueue>>,
}

impl WorkerOutput {
    fn send(&self, line: &str) {
        let Some(State::Button(slot, down)) = parse_button(line) else {
            let _ = self.tx.send(OutputLine::Status(line.into()));
            return;
        };
        let mut queue = self.buttons.lock().unwrap();
        if down && slot < 18 {
            queue.held |= 1 << slot;
        } else {
            queue.held &= !(1 << slot);
        }
        if queue.resync {
            return;
        }
        if queue.pending >= 64 {
            // 未送信の通知の後で最新状態を1回送る。解除だけが欠けて点灯が残るのを防ぐ。
            queue.resync = self.tx.send(OutputLine::ButtonMask).is_ok();
        } else if self.tx.send(OutputLine::Button(line.into())).is_ok() {
            queue.pending += 1;
        }
    }

    fn take_line(buttons: &std::sync::Mutex<ButtonQueue>, message: OutputLine) -> String {
        match message {
            OutputLine::Status(line) => line,
            OutputLine::Button(line) => {
                buttons.lock().unwrap().pending -= 1;
                line
            }
            OutputLine::ButtonMask => {
                let mut queue = buttons.lock().unwrap();
                queue.resync = false;
                format!("BUTTONS {:05X}", queue.held)
            }
        }
    }
}
pub const WORKER_FLAG: &str = "--m908-restore-worker";

pub fn worker_main_with_args(args: impl Iterator<Item = String>) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    let (completed, completion) = mpsc::channel();
    let output = WorkerOutput {
        tx,
        buttons: Default::default(),
    };
    let buttons = output.buttons.clone();
    std::thread::spawn(move || {
        while let Ok(message) = rx.recv() {
            let line = WorkerOutput::take_line(&buttons, message);
            output_line(&line);
            if line == "DONE" || line.starts_with("ERROR ") {
                let _ = completed.send(());
                break;
            }
        }
    });
    // 親が停止してパイプが詰まっても、復元を行うスレッドは出力を待たない。
    let result = worker_session(&output, args.collect());
    output.send(&match &result {
        Ok(()) => "DONE".into(),
        Err(e) => format!("ERROR {}", hex(e.as_bytes())),
    });
    let _ = completion.recv_timeout(Duration::from_millis(250));
    result
}
fn worker_session(output: &WorkerOutput, args: Vec<String>) -> Result<(), String> {
    if args.len() != 3 {
        return Err("復元監視の引数が不正です".into());
    }
    let path = PathBuf::from(&args[0]);
    let pid = args[1].parse::<u32>().map_err(|e| e.to_string())?;
    let device = Device {
        path: CString::new(unhex(&args[2])?).map_err(|e| e.to_string())?,
        name: "M908".into(),
        interface: 2,
        usage_page: 0xffa0,
        usage: 1,
    };
    let lease = std::sync::Arc::new(std::sync::Mutex::new((true, Instant::now())));
    let session_thread = std::thread::current();
    let reader_wake = session_thread.clone();
    let reader = lease.clone();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            if !matches!(line,Ok(ref s) if s=="PING") {
                break;
            }
            if let Ok(mut l) = reader.lock() {
                l.1 = Instant::now();
            }
        }
        if let Ok(mut l) = reader.lock() {
            l.0 = false;
        }
        reader_wake.unpark();
    });
    let monitor = crate::input::start_with_owner(pid)?;
    let relay_stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stopped = relay_stop.clone();
    let input_lost = relay_stop.clone();
    let disconnect_lease = lease.clone();
    let relay_output = output.clone();
    std::thread::spawn(move || {
        while !stopped.load(std::sync::atomic::Ordering::Relaxed) {
            match monitor.events.recv_timeout(Duration::from_millis(20)) {
                Ok(Event::Reset | Event::Blur) => {
                    if let Ok(mut l) = disconnect_lease.lock() {
                        l.0 = false;
                    }
                    session_thread.unpark();
                    break;
                }
                Ok(event) => {
                    for (slot, down) in transitions(&event) {
                        relay_output.send(&format!("BUTTON {slot} {}", if down { 1 } else { 0 }));
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if let Ok(mut l) = disconnect_lease.lock() {
                        l.0 = false;
                    }
                    session_thread.unpark();
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    });
    let keep = move || {
        lease
            .lock()
            .is_ok_and(|l| l.0 && l.1.elapsed() < Duration::from_secs(2))
            && foreground_process() == pid
    };
    let result = run_session(&device, &path, keep, |line| output.send(line));
    input_lost.store(true, std::sync::atomic::Ordering::Relaxed);
    result
}
fn parse_button(line: &str) -> Option<State> {
    let mut parts = line.strip_prefix("BUTTON ")?.split_whitespace();
    let slot = parts.next()?.parse::<usize>().ok()?;
    let down = match parts.next()? {
        "0" => false,
        "1" => true,
        _ => return None,
    };
    if slot >= 20 || parts.next().is_some() {
        return None;
    }
    Some(State::Button(slot, down))
}
fn parse_button_mask(line: &str) -> Option<State> {
    let mask = u32::from_str_radix(line.strip_prefix("BUTTONS ")?, 16).ok()?;
    (mask < (1 << 18)).then_some(State::ButtonMask(mask))
}

fn parse_snapshot(line: &str) -> Option<State> {
    let value = line.strip_prefix("SNAPSHOT ")?;
    Some(
        match unhex(value)
            .and_then(|bytes| String::from_utf8(bytes).map_err(|e| e.to_string()))
            .and_then(|text| Snapshot::from_text(&text))
        {
            Ok(snapshot) => State::Snapshot(snapshot),
            Err(e) => State::Failed(format!("復元後の設定を受け取れません: {e}")),
        },
    )
}
pub fn report_error(error: &str) {
    output_line(&format!("ERROR {}", hex(error.as_bytes())));
}
/// 親が異常終了して出力先が閉じても、復元処理をpanicで止めない。
pub fn output_line(line: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

/// 物理の通常3クリックとホイール、および識別キーだけを解釈する。
pub fn transitions(event: &Event) -> Vec<(usize, bool)> {
    match *event {
        Event::Key { vkey, flags, .. } => marker_slot(vkey)
            .map(|s| vec![(s, flags & 1 == 0)])
            .unwrap_or_default(),
        Event::Mouse { flags, wheel, .. } => {
            let mut out = Vec::new();
            for (slot, down, up) in [(0, 1, 2), (1, 4, 8), (2, 16, 32)] {
                if flags & down != 0 {
                    out.push((slot, true));
                }
                if flags & up != 0 {
                    out.push((slot, false));
                }
            }
            if flags & 0x400 != 0 && wheel != 0 {
                out.push((if wheel > 0 { 18 } else { 19 }, true));
            }
            out
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions;
    #[test]
    fn borrowed_selection_skips_unusable_candidate_and_keeps_active_untouched() {
        let mut snapshot = settings::fixture();
        snapshot.parts[0][1][11] = 0xee;
        snapshot.parts[0][3] = snapshot.parts[0][1].clone();
        assert!(prepared(&snapshot, 4).is_err());
        let (borrowed, plan) = best_prepared(&snapshot).unwrap();
        assert_eq!(borrowed, 2);
        assert!(snapshot.same_profile(&plan.expected, 0));
    }
    #[test]
    fn blocked_output_never_blocks_restore_notifications_or_grows_button_queue() {
        let (tx, rx) = mpsc::channel();
        let output = WorkerOutput {
            tx,
            buttons: Default::default(),
        };
        // writerが全く進まなくても復元通知を送れること。
        for _ in 0..1000 {
            output.send("BUTTON 6 1");
        }
        output.send("BUTTON 6 0");
        output.send("BUTTON 7 1");
        output.send("RESTORING");
        output.send("DONE");
        let queued: Vec<_> = rx.try_iter()
            .map(|message| WorkerOutput::take_line(&output.buttons, message)).collect();
        assert_eq!(queued.len(), 67);
        assert!(matches!(parse_button_mask(&queued[64]), Some(State::ButtonMask(0x80))));
        assert_eq!(queued[65], "RESTORING");
        assert_eq!(queued[66], "DONE");
    }
    #[test]
    fn restored_snapshot_protocol_roundtrips_and_rejects_damage() {
        let snapshot = settings::fixture();
        let snapshot = Snapshot::from_controls([
            snapshot.parts[0].clone(),
            snapshot.parts[1][..5].to_vec(),
            snapshot.parts[2].clone(),
        ])
        .unwrap();
        let line = format!("SNAPSHOT {}", hex(snapshot.to_text().as_bytes()));
        assert!(matches!(parse_snapshot(&line), Some(State::Snapshot(value)) if value == snapshot));
        assert!(matches!(
            parse_snapshot("SNAPSHOT 00"),
            Some(State::Failed(_))
        ));
    }
    #[test]
    fn controls_journal_is_explicit_and_legacy_full_journal_still_loads() {
        let s = settings::fixture();
        let device = Device {
            path: CString::new("test-device").unwrap(),
            name: "test".into(),
            interface: 2,
            usage_page: 0xffa0,
            usage: 1,
        };
        let mut journal = Journal {
            device,
            borrowed: 4,
            original: s.clone(),
        };
        assert!(journal.text().starts_with("M908-IDENTIFICATION-1\n"));
        assert_eq!(Journal::parse(&journal.text()).unwrap().original, s);
        journal.original = Snapshot::from_controls([
            s.parts[0].clone(),
            s.parts[1][..5].to_vec(),
            s.parts[2].clone(),
        ])
        .unwrap();
        let text = journal.text();
        assert!(text.starts_with("M908-IDENTIFICATION-2\n"));
        assert!(Journal::parse(&text)
            .unwrap()
            .original
            .same_controls(&s, false));
        assert!(
            Journal::parse(&text.replace("M908-IDENTIFICATION-2", "M908-IDENTIFICATION-1"))
                .is_err()
        );
    }
    #[test]
    fn button_protocol_checks_slot_and_press_release() {
        assert!(matches!(
            parse_button("BUTTON 6 1"),
            Some(State::Button(6, true))
        ));
        assert!(matches!(
            parse_button("BUTTON 6 0"),
            Some(State::Button(6, false))
        ));
        for line in [
            "BUTTON 20 1",
            "BUTTON 0 2",
            "BUTTON 6 1 extra",
            "BUTTON x 0",
        ] {
            assert!(parse_button(line).is_none());
        }
    }
    #[test]
    fn markers_unique_and_complete() {
        let mut seen = std::collections::HashSet::new();
        for (slot, name) in SLOTS.into_iter().zip(MARKERS) {
            assert!(seen.insert(actions::encode(name).unwrap()));
            let number = name[1..].parse::<u16>().unwrap();
            assert_eq!(marker_slot(0x6f + number), Some(slot));
        }
        assert_eq!(marker_slot(65), None);
    }
    #[test]
    fn journal_roundtrip_and_validation() {
        let j = Journal {
            device: Device {
                path: CString::new("test-device").unwrap(),
                name: "test".into(),
                interface: 2,
                usage_page: 0xffa0,
                usage: 1,
            },
            borrowed: 4,
            original: settings::fixture(),
        };
        let parsed = Journal::parse(&j.text()).unwrap();
        assert_eq!(parsed.original, j.original);
        assert_eq!(parsed.device.path, j.device.path);
        assert!(Journal::parse(&j.text().replace("CRC32=", "CRC32=00")).is_err());
    }
    #[test]
    fn preparation_leaves_original_and_all_other_profiles() {
        let s = settings::fixture();
        let p = prepared(&s, 4).unwrap();
        for i in 0..4 {
            assert!(s.same_profile(&p.expected, i));
        }
        assert_eq!(s.active(), p.expected.active());
        for (slot, name) in SLOTS.into_iter().zip(MARKERS) {
            assert_eq!(&p.expected.profiles().unwrap()[4].buttons[slot], name);
        }
        assert!(prepared(&s, 0).is_err());
    }
    #[test]
    fn releases_and_wheel() {
        assert_eq!(
            transitions(&Event::Key {
                vkey: 0x7c,
                scan: 0,
                flags: 1
            }),
            vec![(6, false)]
        );
        assert_eq!(
            transitions(&Event::Mouse {
                flags: 0x405,
                wheel: -120,
                raw: 0
            }),
            vec![(0, true), (1, true), (19, true)]
        );
    }
}

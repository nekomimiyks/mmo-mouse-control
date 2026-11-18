use crate::{
    device::*,
    protocol::*,
    settings::{self, Snapshot},
};
use std::path::PathBuf;
struct Mock {
    state: Snapshot,
    pending: Option<Vec<u8>>,
    writes: usize,
    fail_write: bool,
    fail_at: Option<usize>,
    tamper: bool,
    dir: PathBuf,
    ended: bool,
    reads: usize,
    ignore_write: bool,
    tamper_restore: bool,
    history: Vec<Vec<u8>>,
}
impl Mock {
    fn new(dir: PathBuf) -> Self {
        Self {
            state: settings::fixture(),
            pending: None,
            writes: 0,
            fail_write: false,
            fail_at: None,
            tamper: false,
            dir,
            ended: false,
            reads: 0,
            ignore_write: false,
            tamper_restore: false,
            history: Vec::new(),
        }
    }
}
impl Transport for Mock {
    fn send(&mut self, row: &[u8]) -> Result<(), String> {
        if row[1] == 0xf2 {
            self.pending = Some(row.to_vec());
            return Ok(());
        }
        if row[1] == 0xf5 {
            self.ended = row[2] == 1;
            return Ok(());
        }
        if row[1] != 0xf3 {
            return Ok(());
        }
        let journal = self.dir.join(crate::identification::JOURNAL);
        assert!(
            (journal.exists() && crate::identification::Journal::load(&journal).is_ok())
                || std::fs::read_dir(&self.dir).unwrap().any(|v| v
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|s| s == "m908")),
            "backup must be durable before writing"
        );
        self.writes += 1;
        self.history.push(row.to_vec());
        if self.fail_at == Some(self.writes) {
            self.fail_at = None;
            return Err("注入した単発の送信エラー".into());
        }
        if self.fail_write {
            return Err("注入した送信エラー".into());
        }
        if self.ignore_write {
            return Ok(());
        }
        let address = u16::from_le_bytes([row[2], row[3]]);
        if address == 0x2c {
            if self.tamper_restore && row[8] == 0 && self.state.active() == 4 {
                self.state.parts[2][80][8] ^= 1;
            }
            self.state.parts[0][0][8] = row[8];
            return Ok(());
        }
        for p in 0..5 {
            let led = 0x449 + 8 * p as u16;
            if address == led {
                self.state.parts[0][p + 1][8..14].copy_from_slice(&row[8..14]);
                if row[4] == 7 {
                    self.state.parts[0][p + 1][14] = row[14];
                }
                return Ok(());
            }
            if address == led + 6 {
                self.state.parts[0][p + 1][14] = row[8];
                return Ok(());
            }
            let poll = if p < 3 {
                0x32 + 2 * p as u16
            } else {
                0x38 + 2 * (p - 3) as u16
            };
            if address == poll {
                if p < 3 {
                    self.state.parts[0][6][8 + 2 * p] = row[8];
                } else {
                    self.state.parts[0][7][8 + 2 * (p - 3)] = row[8];
                }
                return Ok(());
            }
            for j in 0..5 {
                let packet = WRITE_CONFIG[7 + 5 * j + p];
                if address == u16::from_le_bytes([packet[2], packet[3]]) {
                    self.state.parts[1][p][10 + 6 * j..14 + 6 * j].copy_from_slice(&row[8..12]);
                    if self.tamper {
                        self.state.parts[1][20][8] ^= 1;
                    }
                    return Ok(());
                }
            }
        }
        for j in 0..100 {
            let packet = READ_3[j];
            if address == u16::from_le_bytes([packet[2], packet[3]]) {
                let length = u16::from_le_bytes([row[4], row[5]]) as usize;
                assert_eq!(length % 4, 0);
                for offset in (0..length).step_by(4) {
                    let request = READ_3[j + offset / 4];
                    assert_eq!(
                        u16::from_le_bytes([request[2], request[3]]) as usize,
                        address as usize + offset,
                        "未知の隙間へ書かない"
                    );
                    self.state.parts[2][j + offset / 4][8..12]
                        .copy_from_slice(&row[8 + offset..12 + offset]);
                }
                return Ok(());
            }
        }
        panic!("unexpected write address {address:04X}")
    }
    fn get(&mut self, id: u8, len: usize) -> Result<Vec<u8>, String> {
        self.reads += 1;
        let req = self.pending.take().unwrap();
        assert_eq!(id, req[0]);
        assert_eq!(len, req.len());
        for p in 0..3 {
            if let Some(index) = settings::requests(p).iter().position(|r| r == &req) {
                let mut data = self.state.parts[p][index].clone();
                data.truncate(8 + req[4] as usize);
                return Ok(data);
            }
        }
        if matches!(req[0], 2 | 3) && req[1] == 0xf2 && req[4] <= 54 {
            let start = u16::from_le_bytes([req[2], req[3]]);
            let count = req[4] as usize;
            let mut reply = req.clone();
            reply[1] = 8;
            reply[2] &= 0xfe;
            reply[3] |= 0x60;
            reply[6] = 0xfa;
            reply[7] = 0xfa;
            for part in 0..3 {
                for (i, request) in settings::requests(part).iter().enumerate() {
                    let address = u16::from_le_bytes([request[2], request[3]]);
                    let length = request[4] as usize;
                    for k in 0..length {
                        let a = address + k as u16;
                        if a >= start && a < start + count as u16 {
                            reply[8 + (a - start) as usize] = self.state.parts[part][i][8 + k];
                        }
                    }
                }
            }
            reply.truncate(8 + count);
            return Ok(reply);
        }
        Err("unknown request".into())
    }
}
fn dir() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "m908-transaction-test-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    std::fs::create_dir(&p).unwrap();
    p
}
fn clean(path: &std::path::Path) {
    let resolved = path.canonicalize().unwrap();
    assert_eq!(
        resolved.parent().unwrap(),
        std::env::temp_dir().canonicalize().unwrap()
    );
    assert!(resolved
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("m908-transaction-test-"));
    std::fs::remove_dir_all(resolved).unwrap();
}
#[test]
fn backup_then_write_then_verify() {
    let d = dir();
    let mut io = Mock::new(d.clone());
    let base = io.state.clone();
    let mut target = base.profiles().unwrap()[4].clone();
    target.dpi[1] = 1600;
    target.poll = 1000;
    target.brightness = 3;
    target.buttons[7] = "Ctrl + C".into();
    let result = apply_transport(&mut io, &base, 4, &target, &d).unwrap();
    assert_eq!(result.snapshot.profiles().unwrap()[4], target);
    let file = std::fs::read_to_string(result.backup.unwrap()).unwrap();
    assert_eq!(Snapshot::from_text(&file).unwrap(), base);
    assert!(io.ended);
    clean(&d);
}
#[test]
fn do_not_write_if_backup_fails() {
    let d = dir();
    let mut io = Mock::new(d.clone());
    let base = io.state.clone();
    let mut target = base.profiles().unwrap()[0].clone();
    target.dpi[0] = 1600;
    let blocked = d.join("file");
    std::fs::write(&blocked, "x").unwrap();
    assert!(apply_transport(&mut io, &base, 0, &target, &blocked).is_err());
    assert_eq!(io.writes, 0);
    clean(&d);
}
#[test]
fn conflict_stops_without_writes() {
    let d = dir();
    let mut io = Mock::new(d.clone());
    let base = io.state.clone();
    io.state.parts[0][1][8] ^= 1;
    let mut target = base.profiles().unwrap()[0].clone();
    target.poll = 1000;
    assert!(apply_transport(&mut io, &base, 0, &target, &d).is_err());
    assert_eq!(io.writes, 0);
    clean(&d);
}
#[test]
fn other_profile_change_is_preserved() {
    let d = dir();
    let mut io = Mock::new(d.clone());
    let base = io.state.clone();
    io.state.parts[0][2][8] ^= 1;
    let external = io.state.clone();
    let mut target = base.profiles().unwrap()[0].clone();
    target.poll = 1000;
    let result = apply_transport(&mut io, &base, 0, &target, &d).unwrap();
    assert!(result.snapshot.same_profile(&external, 1));
    clean(&d);
}
#[test]
fn failure_never_retries_and_keeps_backup() {
    let d = dir();
    let mut io = Mock::new(d.clone());
    let base = io.state.clone();
    let mut target = base.profiles().unwrap()[0].clone();
    target.poll = 1000;
    io.fail_write = true;
    let error = apply_transport(&mut io, &base, 0, &target, &d).unwrap_err();
    assert!(error.contains("バックアップ"));
    assert_eq!(io.writes, 1);
    assert!(io.ended);
    clean(&d);
}
#[test]
fn unexpected_macro_change_is_failure() {
    let d = dir();
    let mut io = Mock::new(d.clone());
    let base = io.state.clone();
    let mut target = base.profiles().unwrap()[0].clone();
    target.dpi[0] = 1600;
    io.tamper = true;
    assert!(apply_transport(&mut io, &base, 0, &target, &d)
        .unwrap_err()
        .contains("一致しません"));
    clean(&d);
}
#[test]
fn switch_verified_and_backed_up() {
    let d = dir();
    let mut io = Mock::new(d.clone());
    let base = io.state.clone();
    let result = switch_transport(&mut io, &base, 4, &d).unwrap();
    assert_eq!(result.snapshot.active(), 4);
    assert!(base.same_settings(&result.snapshot, true));
    clean(&d);
}

fn identification_device() -> Device {
    Device {
        path: std::ffi::CString::new("mock-m908").unwrap(),
        name: "M908".into(),
        interface: 2,
        usage_page: 0xffa0,
        usage: 1,
    }
}
#[test]
fn compact_writes_preserve_every_byte_and_do_not_cross_gaps_or_commits() {
    let mut packets = vec![READ_1[0].to_vec()];
    packets.extend(WRITE_CONFIG[38..53].iter().map(|packet| packet.to_vec()));
    packets.extend(WRITE_CONFIG[135..139].iter().map(|packet| packet.to_vec()));
    packets.extend(WRITE_CONFIG[53..55].iter().map(|packet| packet.to_vec()));
    let result = compact_writes(packets.clone());
    assert_eq!(result[1][4], 48);
    assert_eq!(result[2][4], 12);
    let expanded = |packets: &[Vec<u8>]| {
        packets
            .iter()
            .filter(|p| p[1] == 0xf3)
            .flat_map(|p| {
                let start = u16::from_le_bytes([p[2], p[3]]);
                (0..u16::from_le_bytes([p[4], p[5]]))
                    .map(move |offset| (start + offset, p[8 + offset as usize]))
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(expanded(&packets), expanded(&result));
    let commands = |packets: &[Vec<u8>]| {
        packets
            .iter()
            .filter(|p| p[1] != 0xf3)
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(commands(&packets), commands(&result));
    let gaps = vec![
        WRITE_CONFIG[52].to_vec(),
        WRITE_CONFIG[53].to_vec(),
        WRITE_CONFIG[7].to_vec(),
        WRITE_CONFIG[12].to_vec(),
    ];
    assert_eq!(compact_writes(gaps.clone()), gaps);
    assert_eq!(
        compact_writes(vec![vec![2, 0xf3], vec![3]]),
        vec![vec![2, 0xf3], vec![3]]
    );
}
#[test]
fn identification_focus_loss_restores_all_raw_settings() {
    use crate::identification::*;
    let d = dir();
    let mut io = Mock::new(d.clone());
    // 無効段階の未対応DPIコードと、未対応割当を生バイトのまま復元する。
    io.state.parts[1][4][10] = 0;
    io.state.parts[1][4][11..13].copy_from_slice(&[0xde, 0xad]);
    io.state.parts[2][99][8..12].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
    let original = io.state.clone();
    let alive = std::cell::Cell::new(true);
    let ready = std::cell::Cell::new(false);
    session_transport(
        &mut io,
        &identification_device(),
        &d.join(JOURNAL),
        || alive.get(),
        |s| {
            if s == "READY" {
                ready.set(true);
                alive.set(false);
            }
        },
    )
    .unwrap();
    assert!(ready.get());
    assert!(original.same_settings(&io.state, false));
    assert!(!d.join(JOURNAL).exists());
    clean(&d);
}
#[test]
fn identification_restores_final_edits_directly_before_switching_to_normal() {
    use crate::identification::*;
    let d = dir();
    let path = d.join(JOURNAL);
    let mut io = Mock::new(d.clone());
    let original = io.state.clone();
    let mut profiles = original.profiles().unwrap();
    profiles[0].buttons[6] = "Chrome: タブを閉じる".into();
    profiles[1].buttons[7] = "Ctrl + T".into();
    // 識別中と同じF13を借用先の最終値にしても、準備の成否に依存しない。
    profiles[4].buttons[6] = "F13".into();
    let mut target = original.clone();
    for p in [0, 1, 4] {
        target = settings::plan(&target, p, &profiles[p]).unwrap().expected;
    }
    let alive = std::cell::Cell::new(true);
    let mut reported = None;
    session_transport(
        &mut io,
        &identification_device(),
        &path,
        || alive.get(),
        |line| {
            if line == "READY" {
                let mut intermediate = profiles[0].buttons.clone();
                intermediate[6] = "Ctrl + C".into();
                stage_keys(&path, &original, 0, &intermediate).unwrap();
                for p in [0, 1, 4] {
                    stage_keys(&path, &original, p, &profiles[p].buttons).unwrap();
                }
                alive.set(false);
            }
            if let Some(encoded) = line.strip_prefix("SNAPSHOT ") {
                reported = Some(encoded.to_string());
            }
        },
    )
    .unwrap();
    assert!(target.same_settings(&io.state, false));
    assert!(reported.is_some());
    let borrowed_switch = io
        .history
        .iter()
        .position(|packet| packet[2..4] == [0x2c, 0] && packet[8] == 4)
        .unwrap();
    let final_writes = &io.history[borrowed_switch + 1..];
    let normal_switch = final_writes
        .iter()
        .position(|packet| packet[2..4] == [0x2c, 0] && packet[8] == 0)
        .unwrap();
    for (p, slot) in [(0, 6), (1, 7), (4, 6)] {
        let request = WRITE_CONFIG[35 + 20 * p + slot];
        let matching: Vec<_> = final_writes
            .iter()
            .enumerate()
            .filter(|(_, packet)| {
                let start = u16::from_le_bytes([packet[2], packet[3]]);
                let address = u16::from_le_bytes([request[2], request[3]]);
                packet[1] == 0xf3
                    && start <= address
                    && address + 4 <= start + u16::from_le_bytes([packet[4], packet[5]])
            })
            .collect();
        assert_eq!(
            matching.len(),
            1,
            "変更したキーを古い値へ戻す書き込みがないこと"
        );
        let offset = (u16::from_le_bytes([request[2], request[3]])
            - u16::from_le_bytes([matching[0].1[2], matching[0].1[3]]))
            as usize;
        assert_eq!(
            matching[0].1[8 + offset..12 + offset],
            target.parts[2][20 * p + slot][8..12]
        );
        assert_eq!(matching[0].0 < normal_switch, p == 0);
    }
    assert!(!path.exists());
    assert!(!path.with_extension("target").exists());
    clean(&d);
}
#[test]
fn edited_restore_target_survives_crash_and_failed_recovery() {
    use crate::identification::*;
    let d = dir();
    let path = d.join(JOURNAL);
    let mut io = Mock::new(d.clone());
    let original = io.state.clone();
    Journal {
        device: identification_device(),
        borrowed: 4,
        original: original.clone(),
    }
    .save(&path)
    .unwrap();
    let mut profiles = original.profiles().unwrap();
    profiles[0].buttons[6] = "Ctrl + W".into();
    profiles[4].buttons[7] = "Ctrl + T".into();
    let mut target = original.clone();
    for p in [0, 4] {
        stage_keys(&path, &original, p, &profiles[p].buttons).unwrap();
        target = settings::plan(&target, p, &profiles[p]).unwrap().expected;
    }
    io.state = prepared(&original, 4).unwrap().expected;
    io.state.parts[0][0][8] = 4;
    // 切断中に別のPCで、通常プロファイルのLEDと別のキーが変更された場合。
    io.state.parts[0][1][8] ^= 0xFF;
    io.state.parts[2][8][9] ^= 0x01;
    target.parts[0][1][8] = io.state.parts[0][1][8];
    target.parts[2][8][9] = io.state.parts[2][8][9];
    io.fail_at = Some(1);
    assert!(recover_transport(&mut io, &path).is_err());
    assert!(path.exists() && path.with_extension("target").exists());
    assert!(stage_keys(&path, &original, 0, &profiles[0].buttons).is_err());
    let actual = recover_transport(&mut io, &path).unwrap();
    assert!(target.same_controls(&actual, false));
    assert!(target.same_settings(&io.state, false));
    assert!(!path.exists());
    clean(&d);
}
#[test]
fn edited_restore_target_rejects_damage_without_changing_hardware() {
    use crate::identification::*;
    let d = dir();
    let path = d.join(JOURNAL);
    let mut io = Mock::new(d.clone());
    let original = io.state.clone();
    Journal {
        device: identification_device(),
        borrowed: 4,
        original: original.clone(),
    }
    .save(&path)
    .unwrap();
    let mut buttons = original.profiles().unwrap()[0].buttons.clone();
    buttons[6] = "Ctrl + W".into();
    stage_keys(&path, &original, 0, &buttons).unwrap();
    let saved = std::fs::read_to_string(path.with_extension("target")).unwrap();
    std::fs::write(
        path.with_extension("target"),
        saved
            .lines()
            .map(|line| {
                if line.starts_with("CRC32=") {
                    "CRC32=00000000"
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    assert!(recover_transport(&mut io, &path).is_err());
    assert_eq!(io.writes, 0);
    assert!(path.exists());
    clean(&d);
}
#[test]
fn identification_partial_preparation_failure_is_rolled_back() {
    use crate::identification::*;
    let d = dir();
    let mut io = Mock::new(d.clone());
    let original = io.state.clone();
    io.fail_at = Some(2);
    let result = session_transport(
        &mut io,
        &identification_device(),
        &d.join(JOURNAL),
        || true,
        |_| {},
    );
    assert!(result.unwrap_err().contains("復元は確認済み"));
    assert!(original.same_settings(&io.state, false));
    assert!(!d.join(JOURNAL).exists());
    clean(&d);
}
#[test]
fn identification_failed_restore_keeps_durable_journal_for_recovery() {
    use crate::identification::*;
    let d = dir();
    let mut io = Mock::new(d.clone());
    let original = io.state.clone();
    io.fail_write = true;
    assert!(session_transport(
        &mut io,
        &identification_device(),
        &d.join(JOURNAL),
        || true,
        |_| {}
    )
    .is_err());
    let j = Journal::load(&d.join(JOURNAL)).unwrap();
    assert!(j.original.same_controls(&original, false));
    assert!(!j.original.has_macros);
    io.fail_write = false;
    let current = io.state.clone();
    restore_transport(&mut io, &j, &current).unwrap();
    assert!(original.same_settings(&io.state, false));
    clean(&d);
}
#[test]
fn identification_cancellation_before_write_does_not_create_journal() {
    use crate::identification::*;
    let d = dir();
    let mut io = Mock::new(d.clone());
    assert!(session_transport(
        &mut io,
        &identification_device(),
        &d.join(JOURNAL),
        || false,
        |_| {}
    )
    .is_ok());
    assert_eq!(io.writes, 0);
    assert!(!d.join(JOURNAL).exists());
    clean(&d);
}
#[test]
fn identification_will_not_overwrite_pending_recovery() {
    use crate::identification::*;
    let d = dir();
    let mut io = Mock::new(d.clone());
    std::fs::write(d.join(JOURNAL), "pending").unwrap();
    assert!(session_transport(
        &mut io,
        &identification_device(),
        &d.join(JOURNAL),
        || true,
        |_| {}
    )
    .is_err());
    assert_eq!(io.writes, 0);
    assert_eq!(std::fs::read_to_string(d.join(JOURNAL)).unwrap(), "pending");
    clean(&d);
}

#[test]
fn batched_reads_preserve_all_data_and_reduce_requests() {
    let d = dir();
    let mut io = Mock::new(d.clone());
    let original = io.state.clone();
    let full = read_transport(&mut io).unwrap();
    assert!(full.same_settings(&original, false));
    assert_eq!(io.reads, 96);
    io.reads = 0;
    let normal = read_controls_transport(&mut io).unwrap();
    assert!(normal.same_controls(&original, false));
    assert!(!normal.has_macros);
    assert_eq!(io.reads, 16);
    let text = normal.to_text();
    assert!(text.starts_with("M908-SETTINGS-1\n"));
    assert_eq!(Snapshot::from_text(&text).unwrap(), normal);
    assert!(Snapshot::from_text(&text.replace("M908-SETTINGS-1", "M908-BACKUP-1")).is_err());
    clean(&d);
}
#[test]
fn fast_save_checks_conflicts_and_preserves_macros() {
    let d = dir();
    let mut io = Mock::new(d.clone());
    io.state.parts[1][20][8] = 0xab;
    let base = io.state.clone();
    let mut target = base.profiles().unwrap()[4].clone();
    target.color[2] ^= 1;
    target.buttons[7] = "Ctrl + C".into();
    let applied = apply_controls_transport(&mut io, &base, 4, &target, &d).unwrap();
    assert_eq!(io.reads, 18);
    assert_eq!(io.state.parts[1][20][8], 0xab);
    assert_eq!(applied.snapshot.profiles().unwrap()[4], target);
    let text = std::fs::read_to_string(applied.backup.unwrap()).unwrap();
    let backup = Snapshot::from_text(&text).unwrap();
    assert!(!backup.has_macros);
    assert!(backup.same_controls(&base, false));
    let writes = io.writes;
    assert!(apply_controls_transport(&mut io, &base, 4, &target, &d).is_err());
    assert_eq!(io.writes, writes);
    clean(&d);
}
#[test]
fn multiple_key_save_is_one_write_and_all_changed_keys_are_verified() {
    let d = dir();
    for full in [false, true] {
        let mut io = Mock::new(d.clone());
        io.state.parts[1][20][8] = 0xab;
        let base = io.state.clone();
        let mut target = base.profiles().unwrap()[4].clone();
        for slot in 6..18 {
            target.buttons[slot] = "Ctrl + W".into();
        }
        let result = if full {
            apply_transport(&mut io, &base, 4, &target, &d)
        } else {
            apply_controls_transport(&mut io, &base, 4, &target, &d)
        }
        .unwrap();
        assert_eq!(io.writes, 1);
        assert_eq!(io.state.profiles().unwrap()[4], target);
        assert_eq!(result.snapshot.profiles().unwrap()[4], target);
        assert_eq!(io.state.parts[1][20][8], 0xab);
        let mut rejected = Mock::new(d.clone());
        rejected.ignore_write = true;
        assert!(apply_controls_transport(&mut rejected, &base, 4, &target, &d).is_err());
        assert_eq!(rejected.writes, 1);
    }
    clean(&d);
}
#[test]
fn legacy_full_recovery_skips_macro_reads_and_preserves_unknown_bytes() {
    use crate::identification::*;
    let d = dir();
    let path = d.join(JOURNAL);
    let mut io = Mock::new(d.clone());
    io.state.parts[1][20][8..20].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    io.state.parts[0][6][9] = 0xa5;
    io.state.parts[1][4][22] = 0;
    io.state.parts[1][4][23..25].copy_from_slice(&[0xde, 0xad]);
    let original = io.state.clone();
    Journal {
        device: identification_device(),
        borrowed: 4,
        original: original.clone(),
    }
    .save(&path)
    .unwrap();
    io.state = prepared(&original, 4).unwrap().expected;
    io.state.parts[0][0][8] = 4;
    recover_transport(&mut io, &path).unwrap();
    assert_eq!(io.reads, 21, "通常設定16要求と借用先5要求だけを取得する");
    assert!(original.same_settings(&read_transport(&mut io).unwrap(), false));
    clean(&d);
}
#[test]
fn journal_atomic_publish_is_complete_and_never_replaces_existing_file() {
    let d = dir();
    let path = d.join("journal");
    save_raw_atomic_new(&path, "完全な元データ").unwrap();
    assert!(save_raw_atomic_new(&path, "置換してはいけない").is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "完全な元データ");
    assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
    let interrupted = d.join("unfinished.journal-new-interrupted");
    std::fs::write(&interrupted, "途中").unwrap();
    let complete = d.join("unfinished");
    save_raw_atomic_new(&complete, "完全な新規データ").unwrap();
    assert_eq!(
        std::fs::read_to_string(complete).unwrap(),
        "完全な新規データ"
    );
    assert_eq!(std::fs::read_to_string(interrupted).unwrap(), "途中");
    clean(&d);
}

#[test]
fn targeted_readback_rejects_unapplied_write() {
    let d = dir();
    let mut io = Mock::new(d.clone());
    let base = io.state.clone();
    let mut target = base.profiles().unwrap()[4].clone();
    target.color[2] ^= 1;
    io.ignore_write = true;
    let error = apply_controls_transport(&mut io, &base, 4, &target, &d).unwrap_err();
    assert!(error.contains("一致しません"));
    assert_eq!(io.writes, 1);
    assert!(io.ended);
    clean(&d);
}
#[test]
fn fast_restore_checks_unchanged_fields_and_retains_journal_on_conflict() {
    use crate::identification::*;
    let d = dir();
    let mut io = Mock::new(d.clone());
    let original = io.state.clone();
    let plan = prepared(&original, 4).unwrap();
    assert_eq!(
        original.parts[2][80][8..12],
        plan.expected.parts[2][80][8..12]
    );
    io.tamper_restore = true;
    let alive = std::cell::Cell::new(true);
    assert!(session_transport(
        &mut io,
        &identification_device(),
        &d.join(JOURNAL),
        || alive.get(),
        |s| {
            if s == "READY" {
                alive.set(false);
            }
        }
    )
    .is_err());
    assert!(d.join(JOURNAL).exists());
    clean(&d);
}

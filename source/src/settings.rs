use crate::{
    actions,
    model::{self, Profile},
    protocol::*,
};
const EFFECT_CODES: [[u8; 2]; 11] = [
    [1, 2],
    [1, 4],
    [1, 1],
    [1, 8],
    [2, 0],
    [6, 0],
    [7, 0],
    [3, 0],
    [1, 16],
    [0, 0],
    [4, 0],
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub parts: [Vec<Vec<u8>>; 3],
    /// falseならマクロ領域は未取得。通常設定の保存に架空のマクロデータを含めない。
    pub has_macros: bool,
}
pub fn requests(part: usize) -> Vec<Vec<u8>> {
    match part {
        0 => READ_1[1..].iter().map(|r| r.to_vec()).collect(),
        1 => READ_2.iter().map(|r| r.to_vec()).collect(),
        2 => READ_3[..100].iter().map(|r| r.to_vec()).collect(),
        _ => Vec::new(),
    }
}
impl Snapshot {
    pub fn from_parts(mut parts: [Vec<Vec<u8>>; 3]) -> Result<Self, String> {
        for part in 0..3 {
            let req = requests(part);
            if parts[part].len() != req.len() {
                return Err(format!("バックアップのpart{}の行数が違います", part + 1));
            }
            for (row, request) in parts[part].iter_mut().zip(req) {
                validate_reply(&request, row)?;
                row.resize(request.len(), 0);
            }
        }
        let s = Self {
            parts,
            has_macros: true,
        };
        s.profiles()?;
        if s.active() >= 5 {
            return Err("使用中プロファイルの値が不正です".into());
        }
        Ok(s)
    }
    pub fn from_controls(mut parts: [Vec<Vec<u8>>; 3]) -> Result<Self, String> {
        if parts[1].len() != 5 {
            return Err("通常設定のDPI行数が不正です".into());
        }
        for request in requests(1).into_iter().skip(5) {
            let mut row = request;
            row[1] = 8;
            row[2] &= 0xfe;
            row[3] |= 0x60;
            row[6] = 0xfa;
            row[7] = 0xfa;
            row[8..].fill(0);
            parts[1].push(row);
        }
        let mut snapshot = Self::from_parts(parts)?;
        snapshot.has_macros = false;
        Ok(snapshot)
    }
    pub fn active(&self) -> usize {
        self.parts[0][0][8] as usize
    }
    pub fn profiles(&self) -> Result<[Profile; 5], String> {
        let mut result = model::defaults();
        for (i, p) in result.iter_mut().enumerate() {
            let led = &self.parts[0][i + 1];
            p.color.copy_from_slice(&led[8..11]);
            p.effect = EFFECT_CODES
                .iter()
                .position(|v| *v == [led[11], led[13]])
                .unwrap_or(11);
            p.speed = led[12] as u32;
            p.brightness = led[14] as u32;
            let byte = if i < 3 {
                self.parts[0][6][8 + 2 * i]
            } else {
                self.parts[0][7][8 + 2 * (i - 3)]
            };
            p.poll = match byte {
                8 => 125,
                4 => 250,
                2 => 500,
                1 => 1000,
                _ => {
                    return Err(format!(
                        "プロファイル{}のポーリング値が不正: {byte:02X}",
                        i + 1
                    ))
                }
            };
            let dpi = &self.parts[1][i];
            for j in 0..5 {
                let off = 10 + 6 * j;
                p.enabled[j] = match dpi[off] {
                    0 => false,
                    1 => true,
                    _ => return Err("DPIの有効フラグが不正です".into()),
                };
                let pair = [dpi[off + 1], dpi[off + 2]];
                p.dpi[j] = match DPI_CODES.iter().find(|(_, bytes)| *bytes == pair) {
                    Some((value, _)) => *value,
                    None if !p.enabled[j] => 200,
                    None => {
                        return Err(format!("未対応のDPIコード: {:02X}{:02X}", pair[0], pair[1]))
                    }
                };
            }
            for j in 0..20 {
                p.buttons[j] =
                    actions::decode(self.parts[2][20 * i + j][8..12].try_into().unwrap());
            }
        }
        model::validate(&result)?;
        Ok(result)
    }
    pub fn to_text(&self) -> String {
        let mut body = String::new();
        for (part, rows) in self.parts.iter().enumerate() {
            body.push_str(&format!("[part{}]\n", part + 1));
            for row in rows.iter().take(if !self.has_macros && part == 1 {
                5
            } else {
                rows.len()
            }) {
                body.push_str(
                    &row.iter()
                        .map(|b| format!("{b:02X}"))
                        .collect::<Vec<_>>()
                        .join(" "),
                );
                body.push('\n');
            }
        }
        format!(
            "{}\nCRC32={:08X}\n{}",
            if self.has_macros {
                "M908-BACKUP-1"
            } else {
                "M908-SETTINGS-1"
            },
            crc32fast::hash(body.as_bytes()),
            body
        )
    }
    pub fn from_text(text: &str) -> Result<Self, String> {
        if text.len() > 100000 {
            return Err("バックアップファイルが大きすぎます".into());
        }
        let text = text.replace("\r\n", "\n");
        let mut split = text.splitn(3, '\n');
        let has_macros = match split.next() {
            Some("M908-BACKUP-1") => true,
            Some("M908-SETTINGS-1") => false,
            _ => return Err("M908バックアップではありません".into()),
        };
        let expected = split
            .next()
            .and_then(|s| s.strip_prefix("CRC32="))
            .and_then(|s| u32::from_str_radix(s, 16).ok())
            .ok_or("チェックサムが不正です")?;
        let body = split.next().ok_or("データがありません")?;
        if crc32fast::hash(body.as_bytes()) != expected {
            return Err("バックアップの整合性チェックに失敗しました".into());
        }
        let mut parts: [Vec<Vec<u8>>; 3] = std::array::from_fn(|_| Vec::new());
        let mut part = None;
        let mut next = 0;
        for line in body.lines() {
            if line.starts_with('[') {
                if line != format!("[part{}]", next + 1) || next >= 3 {
                    return Err("セクションの順序が不正です".into());
                }
                part = Some(next);
                next += 1;
            } else {
                let n = part.ok_or("セクションがありません")?;
                let row = line
                    .split_whitespace()
                    .map(|v| {
                        u8::from_str_radix(v, 16).map_err(|_| "16進データが不正です".to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                parts[n].push(row);
            }
        }
        if has_macros {
            Self::from_parts(parts)
        } else {
            Self::from_controls(parts)
        }
    }
    /// 全プロファイル、予約領域、マクロを照合。操作中に切り替わり得るDPIスロットだけを除く。
    pub fn same_settings(&self, other: &Self, ignore_active: bool) -> bool {
        if self.has_macros != other.has_macros {
            return false;
        }
        for part in 0..3 {
            for (row, req) in requests(part).iter().enumerate() {
                if !self.has_macros && part == 1 && row >= 5 {
                    continue;
                }
                if ignore_active && part == 0 && row == 0 {
                    continue;
                }
                let start = if part == 1 && row < 5 { 10 } else { 8 };
                let end = 8 + u16::from_le_bytes([req[4], req[5]]) as usize;
                if self.parts[part][row][start..end] != other.parts[part][row][start..end] {
                    return false;
                }
            }
        }
        true
    }
    pub fn same_controls(&self, other: &Self, ignore_active: bool) -> bool {
        let mut a = self.clone();
        let mut b = other.clone();
        a.has_macros = false;
        b.has_macros = false;
        a.same_settings(&b, ignore_active)
    }
    pub fn same_profile(&self, other: &Self, profile: usize) -> bool {
        if profile >= 5 {
            return false;
        }
        let p = profile;
        let poll = |s: &Self| {
            if p < 3 {
                s.parts[0][6][8 + 2 * p]
            } else {
                s.parts[0][7][8 + 2 * (p - 3)]
            }
        };
        self.parts[0][p + 1][8..15] == other.parts[0][p + 1][8..15]
            && poll(self) == poll(other)
            && self.parts[1][p][10..40] == other.parts[1][p][10..40]
            && (0..20)
                .all(|j| self.parts[2][20 * p + j][8..12] == other.parts[2][20 * p + j][8..12])
    }
}
pub struct WritePlan {
    pub packets: Vec<Vec<u8>>,
    pub expected: Snapshot,
    pub changes: Vec<String>,
}
pub fn plan(current: &Snapshot, profile: usize, target: &Profile) -> Result<WritePlan, String> {
    if profile >= 5 {
        return Err("プロファイルは1〜5です".into());
    }
    let originals = current.profiles()?;
    let original = &originals[profile];
    let mut validation = originals.clone();
    validation[profile] = target.clone();
    model::validate(&validation)?;
    let p = profile;
    let mut expected = current.clone();
    let mut packets = Vec::new();
    let mut changes = Vec::new();
    if original.color != target.color
        || original.effect != target.effect
        || original.speed != target.speed
    {
        let mut packet = WRITE_LED[3 + 2 * p];
        packet[8..14].copy_from_slice(&current.parts[0][p + 1][8..14]);
        packet[8..11].copy_from_slice(&target.color);
        packet[12] = target.speed as u8;
        if original.effect != target.effect {
            let code = EFFECT_CODES
                .get(target.effect)
                .ok_or("不明なLED効果は新規に設定できません")?;
            packet[11] = code[0];
            packet[13] = code[1];
        }
        expected.parts[0][p + 1][8..14].copy_from_slice(&packet[8..14]);
        packets.push(packet.to_vec());
        changes.push("LEDの色・効果・速度".into());
    }
    if original.brightness != target.brightness {
        let mut packet = WRITE_LED[4 + 2 * p];
        packet[8] = target.brightness as u8;
        expected.parts[0][p + 1][14] = packet[8];
        packets.push(packet.to_vec());
        changes.push("LEDの明るさ".into());
    }
    if original.poll != target.poll {
        let address = if p < 3 {
            0x32 + 2 * p
        } else {
            0x38 + 2 * (p - 3)
        };
        let mut packet = [0u8; 16];
        packet[..6].copy_from_slice(&[2, 0xf3, address as u8, 0, 1, 0]);
        packet[8] = match target.poll {
            125 => 8,
            250 => 4,
            500 => 2,
            1000 => 1,
            _ => unreachable!(),
        };
        if p < 3 {
            expected.parts[0][6][8 + 2 * p] = packet[8];
        } else {
            expected.parts[0][7][8 + 2 * (p - 3)] = packet[8];
        }
        packets.push(packet.to_vec());
        changes.push("ポーリング".into());
    }
    for j in 0..5 {
        if original.dpi[j] != target.dpi[j] || original.enabled[j] != target.enabled[j] {
            let mut packet = WRITE_CONFIG[7 + 5 * j + p];
            let off = 10 + 6 * j;
            packet[8..12].copy_from_slice(&current.parts[1][p][off..off + 4]);
            packet[8] = u8::from(target.enabled[j]);
            packet[9..11].copy_from_slice(
                &DPI_CODES
                    .iter()
                    .find(|(v, _)| *v == target.dpi[j])
                    .ok_or("未対応のDPI値です")?
                    .1,
            );
            expected.parts[1][p][off..off + 4].copy_from_slice(&packet[8..12]);
            packets.push(packet.to_vec());
            changes.push(format!("DPI {}", j + 1));
        }
    }
    for j in 0..20 {
        if original.buttons[j] != target.buttons[j] {
            let code = actions::encode(&target.buttons[j])
                .map_err(|e| format!("{}: {e}", model::BUTTONS[j]))?;
            let mut packet = WRITE_CONFIG[35 + 20 * p + j];
            packet[8..12].copy_from_slice(&code);
            expected.parts[2][20 * p + j][8..12].copy_from_slice(&code);
            packets.push(packet.to_vec());
            changes.push(format!("ボタン {}", j + 1));
        }
    }
    // 書き込み予定全体を検証してから先頭のコマンドを送る。
    if !packets.is_empty() {
        packets.insert(0, READ_1[0].to_vec());
        packets.extend(WRITE_CONFIG[135..139].iter().map(|r| r.to_vec()));
        packets.push(READ_3[100].to_vec());
    }
    Ok(WritePlan {
        packets,
        expected,
        changes,
    })
}
pub fn switch_packets(profile: usize) -> Result<Vec<Vec<u8>>, String> {
    if profile >= 5 {
        return Err("プロファイルは1〜5です".into());
    }
    let mut packets = SWITCH.map(|r| r.to_vec()).to_vec();
    packets[0][4] = 1;
    packets[0][8] = profile as u8;
    Ok(packets)
}

#[cfg(test)]
pub fn fixture() -> Snapshot {
    // 公開ドライバの初期値パケットから応答を作る。個人の実機データはテストに含めない。
    let mut parts: [Vec<Vec<u8>>; 3] = std::array::from_fn(|p| {
        requests(p)
            .iter()
            .map(|r| {
                let mut v = r.clone();
                v[1] = 8;
                v[2] &= 0xfe;
                v[3] |= 0x60;
                v[6] = 0xfa;
                v[7] = 0xfa;
                v[8..].fill(0);
                v
            })
            .collect()
    });
    parts[0][0][8] = 0;
    for p in 0..5 {
        let row = WRITE_LED[3 + 2 * p];
        parts[0][p + 1][8..14].copy_from_slice(&row[8..14]);
        parts[0][p + 1][14] = WRITE_LED[4 + 2 * p][8];
        if p < 3 {
            parts[0][6][8 + 2 * p] = 2;
        } else {
            parts[0][7][8 + 2 * (p - 3)] = 2;
        }
        for j in 0..5 {
            let r = WRITE_CONFIG[7 + 5 * j + p];
            parts[1][p][10 + 6 * j..14 + 6 * j].copy_from_slice(&r[8..12]);
        }
        for j in 0..20 {
            parts[2][20 * p + j][8..12].copy_from_slice(&WRITE_CONFIG[35 + 20 * p + j][8..12]);
        }
    }
    Snapshot::from_parts(parts).unwrap()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checksum_and_short_data() {
        let s = fixture();
        assert_eq!(Snapshot::from_text(&s.to_text()).unwrap(), s);
        assert!(Snapshot::from_text(&s.to_text().replace("81 00", "82 00")).is_err());
        let mut parts = s.parts.clone();
        parts[1][0].truncate(12);
        assert!(Snapshot::from_parts(parts).is_err());
    }
    #[test]
    fn addresses_are_not_transposed() {
        for p in 0..5 {
            for j in 0..5 {
                let packet = WRITE_CONFIG[7 + 5 * j + p];
                let req = READ_2[p];
                assert_eq!(
                    u16::from_le_bytes([packet[2], packet[3]]),
                    u16::from_le_bytes([req[2], req[3]]) + 2 + 6 * j as u16
                );
            }
        }
    }
    #[test]
    fn selected_profile_only_and_reserved_preserved() {
        let mut s = fixture();
        s.parts[1][4][13] = 0x72;
        s.parts[1][20][8] = 0xab;
        s.parts[2][99][8..12].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        let mut p = s.profiles().unwrap()[4].clone();
        p.dpi[0] = 1600;
        p.color = [1, 2, 3];
        p.effect = 3;
        p.brightness = 3;
        p.poll = 1000;
        p.buttons[6] = "Ctrl + C".into();
        let plan = plan(&s, 4, &p).unwrap();
        assert!(!plan.packets.is_empty());
        assert_eq!(plan.expected.profiles().unwrap()[4], p);
        for i in 0..4 {
            assert!(s.same_profile(&plan.expected, i));
        }
        assert_eq!(plan.expected.parts[1][4][13], 0x72);
        assert_eq!(s.parts[1][20], plan.expected.parts[1][20]);
        assert_eq!(s.parts[2][99], plan.expected.parts[2][99]);
        assert!(!s.same_settings(&plan.expected, false));
    }
    #[test]
    fn no_op_and_validation_before_write() {
        let s = fixture();
        let mut p = s.profiles().unwrap()[0].clone();
        assert!(plan(&s, 0, &p).unwrap().packets.is_empty());
        p.buttons[2] = "Ctrl + unknownKey".into();
        assert!(plan(&s, 0, &p).is_err());
        p.buttons[2] = "無効".into();
        p.enabled = [false; 5];
        assert!(plan(&s, 0, &p).is_err());
    }
}

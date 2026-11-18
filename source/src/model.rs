//! 本体の設定と、PC上の下書きを混同しないための編集モデル。
use std::path::Path;

pub const BUTTONS: [&str; 20] = [
    "左クリック",
    "右クリック",
    "ホイール押下",
    "連射ボタン",
    "DPI ＋",
    "DPI －",
    "サイド 1",
    "サイド 2",
    "サイド 3",
    "サイド 4",
    "サイド 5",
    "サイド 6",
    "サイド 7",
    "サイド 8",
    "サイド 9",
    "サイド 10",
    "サイド 11",
    "サイド 12",
    "ホイール上",
    "ホイール下",
];
pub const ACTIONS: [&str; 15] = [
    "左クリック",
    "右クリック",
    "ホイール押下",
    "戻る",
    "進む",
    "DPI ＋",
    "DPI －",
    "DPI 切替",
    "プロファイル切替",
    "ホイール上",
    "ホイール下",
    "Ctrl + C",
    "Ctrl + V",
    "Ctrl + Z",
    "無効",
];
pub const EFFECTS: [&str; 12] = [
    "常時点灯",
    "呼吸",
    "虹色呼吸",
    "虹色",
    "波",
    "交互",
    "反応",
    "ボタン反応",
    "点滅",
    "消灯",
    "ランダム",
    "不明（実機の値を保持）",
];

#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub name: String,
    pub dpi: [u32; 5],
    pub enabled: [bool; 5],
    pub poll: u32,
    pub color: [u8; 3],
    pub effect: usize,
    pub brightness: u32,
    pub speed: u32,
    pub buttons: [String; 20],
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            name: "標準".into(),
            dpi: [800, 1600, 2400, 3200, 6200],
            enabled: [true; 5],
            poll: 1000,
            color: [0, 120, 212],
            effect: 0,
            brightness: 2,
            speed: 3,
            buttons: std::array::from_fn(|i| match i {
                0..=2 => BUTTONS[i].into(),
                3 => "連射（初期値未確認）".into(),
                4..=5 => BUTTONS[i].into(),
                6..=17 => format!("キー {}", i - 5),
                _ => BUTTONS[i].into(),
            }),
        }
    }
}
pub fn defaults() -> [Profile; 5] {
    std::array::from_fn(|i| Profile {
        name: format!("プロファイル {}", i + 1),
        ..Profile::default()
    })
}

pub fn valid_dpi(v: u32) -> bool {
    (200..=6200).contains(&v) && v % 100 == 0 || (6400..=12400).contains(&v) && v % 200 == 0
}
pub fn normalize_dpi(v: u32) -> u32 {
    let v = v.clamp(200, 12400);
    if v <= 6200 {
        ((v + 50) / 100) * 100
    } else {
        ((v + 100) / 200) * 200
    }
}
pub fn validate(profiles: &[Profile; 5]) -> Result<(), String> {
    for (i, p) in profiles.iter().enumerate() {
        if !p.enabled.iter().any(|x| *x) {
            return Err(format!(
                "プロファイル {}: DPIを最低1段階有効にしてください",
                i + 1
            ));
        }
        if !p.dpi.iter().all(|v| valid_dpi(*v)) {
            return Err(format!("プロファイル {}: 対応していないDPI値です", i + 1));
        }
        if ![125, 250, 500, 1000].contains(&p.poll)
            || p.effect >= EFFECTS.len()
            || !(1..=3).contains(&p.brightness)
            || !(1..=8).contains(&p.speed)
        {
            return Err("設定値が範囲外です".into());
        }
        if std::iter::once(&p.name)
            .chain(p.buttons.iter())
            .any(|v| v.contains(['\r', '\n']) || v.len() > 1024)
        {
            return Err("名前が長すぎるか改行を含んでいます".into());
        }
    }
    Ok(())
}
pub fn encode(profiles: &[Profile; 5]) -> Result<String, String> {
    validate(profiles)?;
    let mut s = "M908-DRAFT-1\n".to_string();
    for (i, p) in profiles.iter().enumerate() {
        s.push_str(&format!("[{}]\nname={}\npoll={}\ncolor={:02X}{:02X}{:02X}\neffect={}\nbrightness={}\nspeed={}\n", i, p.name, p.poll, p.color[0], p.color[1], p.color[2], p.effect, p.brightness, p.speed));
        for j in 0..5 {
            s.push_str(&format!("dpi{j}={},{}\n", p.dpi[j], u8::from(p.enabled[j])));
        }
        for j in 0..20 {
            s.push_str(&format!("button{j}={}\n", p.buttons[j]));
        }
    }
    Ok(s)
}
pub fn decode(s: &str) -> Result<[Profile; 5], String> {
    let err = || "下書きファイルの形式が正しくありません".to_string();
    let mut lines = s.lines();
    if lines.next() != Some("M908-DRAFT-1") {
        return Err(err());
    }
    let mut result = defaults();
    for (i, p) in result.iter_mut().enumerate() {
        if lines.next() != Some(format!("[{i}]").as_str()) {
            return Err(err());
        }
        let mut field = |key: &str| -> Result<&str, String> {
            lines
                .next()
                .and_then(|v| v.strip_prefix(&format!("{key}=")))
                .ok_or_else(err)
        };
        p.name = field("name")?.into();
        p.poll = field("poll")?.parse().map_err(|_| err())?;
        let c = field("color")?;
        if c.len() != 6 || !c.is_ascii() {
            return Err(err());
        }
        for j in 0..3 {
            p.color[j] = u8::from_str_radix(&c[j * 2..j * 2 + 2], 16).map_err(|_| err())?;
        }
        p.effect = field("effect")?.parse().map_err(|_| err())?;
        p.brightness = field("brightness")?.parse().map_err(|_| err())?;
        p.speed = field("speed")?.parse().map_err(|_| err())?;
        for j in 0..5 {
            let (v, on) = field(&format!("dpi{j}"))?.split_once(',').ok_or_else(err)?;
            p.dpi[j] = v.parse().map_err(|_| err())?;
            p.enabled[j] = match on {
                "0" => false,
                "1" => true,
                _ => return Err(err()),
            };
        }
        for j in 0..20 {
            p.buttons[j] = field(&format!("button{j}"))?.into();
        }
    }
    if lines.next().is_some() {
        return Err(err());
    }
    validate(&result)?;
    Ok(result)
}
pub fn save(path: &Path, profiles: &[Profile; 5]) -> Result<(), String> {
    let data = encode(profiles)?;
    // 古い下書きは置き換え前に退避する。
    if path.exists() {
        std::fs::copy(path, path.with_extension("draft.bak")).map_err(|e| e.to_string())?;
    }
    let pending = path.with_extension("pending");
    std::fs::write(&pending, data).map_err(|e| e.to_string())?;
    std::fs::rename(pending, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip_and_reject_damage() {
        let mut p = defaults();
        p[1].name = "仕事=日本語".into();
        p[2].buttons[6] = "Ctrl + Shift + A".into();
        p[2].buttons[7] = "Chrome: シークレットウィンドウ".into();
        let data = encode(&p).unwrap();
        assert_eq!(decode(&data).unwrap(), p);
        assert!(decode(&data.replace("color=0078D4", "color=あいう")).is_err());
        assert!(decode(&data.replace("dpi0=800,1", "dpi0=90000,1")).is_err());
        assert!(decode(&data[..data.len() / 2]).is_err());
    }
    #[test]
    fn dpi_invariants() {
        for v in 0..15000 {
            assert!(valid_dpi(normalize_dpi(v)));
        }
        let mut p = defaults();
        p[0].enabled = [false; 5];
        assert!(validate(&p).is_err());
    }
}

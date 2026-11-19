use hidapi::{HidApi, HidDevice};
use crate::protocol::*;

pub struct ReadSettings {
    pub active: Active,
    pub active_raw: Vec<u8>,
    pub profiles: Vec<Profile>,
}

pub fn read_settings(dev: &HidDevice) -> Result<ReadSettings, String> {
    let active_raw = get_report(dev, REPORT_ACTIVE, ACTIVE_LEN)?;
    let active = Active::parse(&active_raw)?;
    let profiles = REPORT_PROFILES.iter().map(|&id| {
        Profile::parse(&get_report(dev, id, PROFILE_LEN)?)
    }).collect::<Result<Vec<_>, _>>()?;
    Ok(ReadSettings { active, active_raw, profiles })
}

pub fn descriptor(dev: &HidDevice) -> Result<(), String> {
    let mut buf = [0u8; 4096];
    let len = dev.get_report_descriptor(&mut buf)
        .map_err(|e| format!("ディスクリプタの読み取りに失敗: {e}"))?;
    let data = &buf[..len];
    println!("レポートディスクリプタ（{len} バイト）: {}", hex(data));
    let mut pos = 0;
    let (mut id, mut size, mut count) = (0u32, 0u32, 0u32);
    let mut stack = Vec::new();
    let mut features = std::collections::BTreeMap::<u32, u64>::new();
    while pos < data.len() {
        let start = pos;
        let prefix = data[pos];
        pos += 1;
        if prefix == 0xFE {
            if pos + 2 > data.len() { return Err("長形式アイテムのヘッダーが途中で終わっています".into()); }
            let n = data[pos] as usize;
            pos += 2;
            if pos + n > data.len() { return Err("長形式アイテムが途中で終わっています".into()); }
            pos += n;
            println!("  {start}: 長形式 {}", hex(&data[start..pos]));
            continue;
        }
        let n = match prefix & 3 { 3 => 4, x => x as usize };
        if pos + n > data.len() { return Err("短形式アイテムが途中で終わっています".into()); }
        let mut value = 0u32;
        for (i, byte) in data[pos..pos + n].iter().enumerate() { value |= (*byte as u32) << (8 * i); }
        pos += n;
        let kind = prefix & 0xFC;
        let name = match kind {
            0x04 => "Usage Page", 0x08 => "Usage", 0x14 => "Logical Minimum",
            0x24 => "Logical Maximum", 0x74 => { size = value; "Report Size" },
            0x84 => { id = value; "Report ID" }, 0x94 => { count = value; "Report Count" },
            0xA4 => { stack.push((id, size, count)); "Push" },
            0xB4 => { (id, size, count) = stack.pop().ok_or("対応する Push のない Pop")?; "Pop" },
            0xB0 => { *features.entry(id).or_default() += size as u64 * count as u64; "Feature" },
            0x80 => "Input", 0x90 => "Output", 0xA0 => "Collection", 0xC0 => "End Collection",
            _ => "その他",
        };
        println!("  {start}: {}  {name} = 0x{value:X}", hex(&data[start..pos]));
    }
    println!("Feature レポートの長さ（ID を除く）:");
    if features.is_empty() { println!("  なし"); }
    for (id, bits) in features {
        println!("  ID 0x{id:02X}: {} バイト（{bits} ビット）", (bits + 7) / 8);
    }
    Ok(())
}

pub fn list(api: &HidApi) {
    let mut n = 0;
    for d in api.device_list().filter(|d| d.vendor_id() == VENDOR_LOGITECH) {
        n += 1;
        println!(
            "{:04x}:{:04x}  interface={}  usage_page=0x{:04X} usage=0x{:04X}  {}{}",
            d.vendor_id(),
            d.product_id(),
            d.interface_number(),
            d.usage_page(),
            d.usage(),
            d.product_string().unwrap_or("?"),
            if d.product_id() == PRODUCT_G600 { "  <- G600" } else { "" }
        );
    }
    if n == 0 {
        println!("Logicool製のHIDデバイスが見つかりませんでした。");
    }
}

/// The G600 exposes several HID interfaces; only one answers the 0xF0
/// feature report. Try each until one does.
pub fn open_g600(api: &HidApi, pid: u16) -> Result<HidDevice, String> {
    open_g600_with_path(api, pid).map(|(device, _)| device)
}

pub(crate) fn open_g600_with_path(api: &HidApi, pid: u16) -> Result<(HidDevice, std::ffi::CString), String> {
    let mut seen = 0;
    let mut last_err = String::new();
    for info in api
        .device_list()
        .filter(|d| d.vendor_id() == VENDOR_LOGITECH && d.product_id() == pid)
    {
        seen += 1;
        let dev = match info.open_device(api) {
            Ok(d) => d,
            Err(e) => {
                last_err = e.to_string();
                continue;
            }
        };
        if get_report(&dev, REPORT_ACTIVE, ACTIVE_LEN).is_ok() {
            return Ok((dev, info.path().to_owned()));
        }
    }
    if seen == 0 {
        Err(format!(
            "製品ID {pid:04x} のマウスが見つかりません。`g600ctl list` で接続中のデバイスを確認し、\
             違うIDなら `--pid` で指定してください。"
        ))
    } else {
        Err(format!(
            "マウスは見つかりましたが、設定用インターフェースを開けませんでした（{last_err}）。\
             G HUB を終了してから再度お試しください。"
        ))
    }
}

pub fn get_report(dev: &HidDevice, id: u8, len: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; len];
    buf[0] = id;
    let n = dev
        .get_feature_report(&mut buf)
        .map_err(|e| format!("レポート 0x{id:02X} の読み取りに失敗: {e}"))?;
    buf.truncate(n.max(1));
    // Some backends strip the report ID; put it back so offsets stay fixed.
    if buf[0] != id {
        buf.insert(0, id);
    }
    if buf.len() < len {
        return Err(format!("レポート 0x{id:02X} が短すぎます（{} バイト）", buf.len()));
    }
    Ok(buf)
}

pub fn set_report(dev: &HidDevice, buf: &[u8]) -> Result<(), String> {
    let id = buf.first().ok_or("書き込むレポートが空です")?;
    dev.send_feature_report(buf)
        .map_err(|e| format!("レポート 0x{id:02X} の書き込みに失敗: {e}"))
}

pub fn select_profile(dev: &HidDevice, index: u8) -> Result<(), String> {
    if index as usize >= REPORT_PROFILES.len() {
        return Err("プロファイル番号が不正です".into());
    }
    set_report(dev, &active_set_profile(index))?;
    let active = Active::parse(&get_report(dev, REPORT_ACTIVE, ACTIVE_LEN)?)?;
    if active.profile != index {
        return Err(format!("切替後の読み戻しが一致しません（使用中: プロファイル{}）", active.profile + 1));
    }
    Ok(())
}

/// Write one profile, wait for the mouse to store it, then read it back and
/// check it stuck. Reading right after the write can race the mouse's flash
/// update, so we pause first.
pub fn write_profile(dev: &HidDevice, index: u8, buf: &[u8]) -> Result<(), String> {
    let &id = REPORT_PROFILES.get(index as usize).ok_or("プロファイル番号が不正です")?;
    if buf.len() != PROFILE_LEN || buf[0] != id {
        return Err(format!("プロファイル{}の書き込みデータが不正です", index + 1));
    }
    set_report(dev, buf)?;
    std::thread::sleep(std::time::Duration::from_millis(1000));
    let back = get_report(dev, id, PROFILE_LEN)?;
    if back[..PROFILE_LEN] != buf[..PROFILE_LEN] {
        let diff: Vec<String> = (0..PROFILE_LEN)
            .filter(|&i| back[i] != buf[i])
            .map(|i| format!("{i}:{:02X}->{:02X}", buf[i], back[i]))
            .collect();
        return Err(format!(
            "プロファイル{}の読み戻しが一致しません（{}か所）。\n  違い(位置:書込->読戻): {}\n  書込: {}\n  読戻: {}",
            index + 1,
            diff.len(),
            diff.join(" "),
            hex(&buf[..PROFILE_LEN]),
            hex(&back)
        ));
    }
    let active = Active::parse(&get_report(dev, REPORT_ACTIVE, ACTIVE_LEN)?)?;
    if active.profile == index {
        set_report(dev, &active_set_profile(index))?;
    }
    Ok(())
}


pub fn g600_interfaces() -> Result<HidApi, String> {
    let mut api = HidApi::new().map_err(|e| e.to_string())?;
    api.reset_devices().map_err(|e| e.to_string())?;
    api.add_devices(VENDOR_LOGITECH, PRODUCT_G600).map_err(|e| e.to_string())?;
    Ok(api)
}

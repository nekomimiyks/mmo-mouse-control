//! GPL-3.0-or-later; M908 protocol (dokutan/mouse_m908 + Windows実機検証).
mod tables {
    include!("read_tables.rs");
}
pub use tables::{READ_1, READ_2, READ_3};
mod write_tables {
    include!("write_tables.rs");
}
pub use write_tables::{ACTION_CODES, DPI_CODES, KEY_CODES, SWITCH, WRITE_CONFIG, WRITE_LED};

pub fn validate_reply(request: &[u8], response: &[u8]) -> Result<(), String> {
    if ![16, 64].contains(&request.len()) {
        return Err("未対応の読み取り要求長です".into());
    }
    let payload = u16::from_le_bytes([request[4], request[5]]) as usize;
    if payload + 8 > request.len() || response.len() < payload + 8 || response.len() > request.len()
    {
        return Err(format!(
            "HID応答長が異なります: {} / {}",
            response.len(),
            request.len()
        ));
    }
    // 実機: コマンド08、偶数境界へ丸めたアドレス、上位に60、FAFAの応答マーカー。
    // GetFeatureは16/64バイト固定ではなく、ヘッダー8+要求したペイロード長を返す。
    if response[0] != request[0]
        || response[2] != (request[2] & 0xfe)
        || response[3] != (request[3] | 0x60)
        || response[4..6] != request[4..6]
    {
        return Err("異なるレポートIDまたはアドレスの応答を受信しました".into());
    }
    if response[1] != 0x08 || response[6..8] != [0xfa, 0xfa] {
        return Err(format!("未対応の応答opcode: {:02X}", response[1]));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_read_commands_and_session_markers() {
        assert_eq!((READ_1.len(), READ_2.len(), READ_3.len()), (9, 85, 101));
        for row in READ_1.iter().chain(READ_3.iter()) {
            assert!(row[1] == 0xf2 || row[1] == 0xf5);
        }
        for row in READ_2 {
            assert_eq!((row[0], row[1]), (3, 0xf2));
        }
        assert_eq!(READ_1[0][1], 0xf5);
        assert_eq!(READ_3[100][2], 1);
    }
    #[test]
    fn reject_short_or_wrong_address() {
        let req = READ_1[1];
        assert!(validate_reply(&req, &req[..8]).is_err());
        let mut response = req;
        response[1] = 0x08;
        response[3] |= 0x60;
        response[6] = 0xfa;
        response[7] = 0xfa;
        response[8] = 1;
        assert!(validate_reply(&req, &response).is_ok());
        response[2] ^= 1;
        assert!(validate_reply(&req, &response).is_err());
    }
}

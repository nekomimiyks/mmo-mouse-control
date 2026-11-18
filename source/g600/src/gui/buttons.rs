//! G600 の設定用コレクション（UsagePage 0xFF80）が出す入力レポート 0x80 から、
//! いま押されている物理ボタンを読む（割り当てに関係なく、どのボタンかが分かる）。
//!
//! 2026-11-12 の実機ログ（input-log.txt）より:
//!   80 b1 b2 b3 00 st 00 …（32 バイト）
//!   b1〜b3 が押されているボタンのビット（b1 の bit0 = ボタン1 … b3 の bit3 = ボタン20 の 20 ビット）。
//!   st は状態（0x10/0x11/0x16 などで変化。意味は未確定なので使わない）。

use std::time::{Duration, Instant};

const RELEASE_FLASH: Duration = Duration::from_millis(80);

#[derive(Default)]
pub struct PressState {
    held: u32,
    last: u32,
    released: [Option<Instant>; 20],
}

impl PressState {
    /// HIDレポートを受けた時点で記録し、描画の間に終わった短い押下も残す。
    pub fn report(&mut self, mask: u32, now: Instant) -> bool {
        if self.held == mask {
            return false;
        }
        for slot in pressed_slots(self.held & !mask) {
            self.released[slot] = Some(now);
        }
        self.held = mask;
        if mask != 0 {
            self.last = mask;
        }
        true
    }

    pub fn display(&self, now: Instant) -> (u32, u32, bool) {
        let mut visible = self.held;
        for (slot, released) in self.released.iter().enumerate() {
            if released.is_some_and(|at| now.saturating_duration_since(at) < RELEASE_FLASH) {
                visible |= 1 << slot;
            }
        }
        (visible, self.last, visible != self.held)
    }
}

/// 入力レポート → 押されているボタンのビット（bit n = ボタン n+1）。レポート 0x80 以外は None。
pub fn mask_from_report(report: &[u8]) -> Option<u32> {
    if report.len() < 4 || report[0] != 0x80 { return None; }
    Some(report[1] as u32 | (report[2] as u32) << 8 | ((report[3] & 0x0F) as u32) << 16)
}

/// ビット → 押されているボタン番号（0 始まり）の一覧
pub fn pressed_slots(mask: u32) -> Vec<usize> {
    (0..20).filter(|i| mask & (1 << i) != 0).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_between_frames_keeps_a_short_highlight_and_the_last_button() {
        let now = Instant::now();
        let mut state = PressState::default();
        for slot in 0..20 {
            let bit = 1 << slot;
            let time = now + Duration::from_secs(slot);
            assert!(state.report(bit, time));
            assert!(state.report(0, time + Duration::from_millis(5)));
            assert_eq!(state.display(time + Duration::from_millis(16)), (bit, bit, true));
            assert_eq!(state.display(time + Duration::from_millis(85)), (0, bit, false));
        }
        let time = now + Duration::from_secs(21);
        state.report((1 << 8) | (1 << 19), time);
        state.report(1 << 19, time + Duration::from_millis(10));
        assert_eq!(state.display(time + Duration::from_millis(20)), ((1 << 8) | (1 << 19), 1 << 19, true));
        assert_eq!(state.display(time + Duration::from_millis(90)), (1 << 19, 1 << 19, false));
    }

    #[test]
    fn reads_button_bits_from_report() {
        assert_eq!(mask_from_report(&[0x80, 0x01, 0, 0, 0, 0x11]), Some(1)); // 左ボタン
        assert_eq!(pressed_slots(1), vec![0]);
        assert_eq!(pressed_slots(mask_from_report(&[0x80, 0x80, 0, 0, 0, 0x11]).unwrap()), vec![7]); // G8
        assert_eq!(pressed_slots(mask_from_report(&[0x80, 0, 0x01, 0, 0, 0x11]).unwrap()), vec![8]); // G9
        assert_eq!(pressed_slots(mask_from_report(&[0x80, 0, 0, 0x08, 0, 0x11]).unwrap()), vec![19]); // G20
        assert_eq!(pressed_slots(mask_from_report(&[0x80, 0x41, 0x02, 0, 0, 0x11]).unwrap()), vec![0, 6, 9]);
        assert_eq!(mask_from_report(&[0x80, 0, 0, 0, 0, 0x11]), Some(0));
        assert_eq!(mask_from_report(&[0xF6, 1, 0, 0]), None);
        assert_eq!(mask_from_report(&[0x80, 1]), None);
    }
}

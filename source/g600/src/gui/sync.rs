//! マウス本体への即時反映（自動書き込み）の判定。egui に依存しない純粋関数。

use std::time::{Duration, Instant};

/// 最後の変更からこれだけ待ってから書く（連続した変更をまとめる）
pub const DEBOUNCE: Duration = Duration::from_millis(500);
/// 同じプロファイルへの書き込みは、前の書き込みが終わってからこれだけ空ける
pub const SAME_PROFILE_GAP: Duration = Duration::from_millis(1000);

pub struct SyncInput<'a> {
    pub now: Instant,
    /// 自動反映が有効か（実機表示中で、失敗して止まっていない）
    pub enabled: bool,
    /// 書き込み中か（worker に送って結果待ち）
    pub in_flight: bool,
    /// マウスのボタンを押したまま（スライダーや色ピッカーをドラッグ中など）か
    pub pointer_down: bool,
    /// 最後に編集内容が変わった時刻
    pub last_edit: Option<Instant>,
    /// プロファイルごとに、編集内容がマウスの内容と違うか
    pub dirty: &'a [bool],
    /// プロファイルごとに、最後に書き込みが終わった時刻
    pub last_done: &'a [Option<Instant>],
}

/// 今書くべきプロファイル（0 始まり）。書かないなら None。
pub fn next_write(s: &SyncInput) -> Option<usize> {
    if !s.enabled || s.in_flight || s.pointer_down { return None; }
    if s.last_edit.is_some_and(|t| s.now.duration_since(t) < DEBOUNCE) { return None; }
    (0..s.dirty.len()).find(|&i| s.dirty[i] && s.last_done.get(i).copied().flatten().is_none_or(|t| s.now.duration_since(t) >= SAME_PROFILE_GAP))
}

/// 次に判定し直すまでの待ち時間（再描画の予約用）。書くものがなければ None。
pub fn wait_hint(s: &SyncInput) -> Option<Duration> {
    if !s.enabled || !s.dirty.iter().any(|d| *d) { return None; }
    Some(Duration::from_millis(100))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(now: Instant, dirty: &'a [bool], last_done: &'a [Option<Instant>]) -> SyncInput<'a> {
        SyncInput { now, enabled: true, in_flight: false, pointer_down: false, last_edit: None, dirty, last_done }
    }

    #[test]
    fn writes_dirty_profile_after_debounce() {
        let t0 = Instant::now();
        let dirty = [false, true, false]; let done = [None, None, None];
        let mut s = input(t0 + Duration::from_millis(100), &dirty, &done);
        s.last_edit = Some(t0);
        assert_eq!(next_write(&s), None); // まだ 0.5 秒たっていない
        s.now = t0 + DEBOUNCE;
        assert_eq!(next_write(&s), Some(1));
    }
    #[test]
    fn does_not_write_while_dragging_in_flight_or_disabled() {
        let t0 = Instant::now();
        let dirty = [true, false, false]; let done = [None, None, None];
        let mut s = input(t0, &dirty, &done);
        s.pointer_down = true; assert_eq!(next_write(&s), None);
        s.pointer_down = false; s.in_flight = true; assert_eq!(next_write(&s), None);
        s.in_flight = false; s.enabled = false; assert_eq!(next_write(&s), None);
        s.enabled = true; assert_eq!(next_write(&s), Some(0));
    }
    #[test]
    fn keeps_gap_between_writes_to_same_profile() {
        let t0 = Instant::now();
        let dirty = [true, true, false]; let done = [Some(t0), None, None];
        let s = input(t0 + Duration::from_millis(300), &dirty, &done);
        assert_eq!(next_write(&s), Some(1)); // プロファイル1 は直前に書いたので後回し
        let dirty = [true, false, false];
        let mut s = input(t0 + Duration::from_millis(300), &dirty, &done);
        assert_eq!(next_write(&s), None);
        s.now = t0 + SAME_PROFILE_GAP;
        assert_eq!(next_write(&s), Some(0));
    }
    #[test]
    fn nothing_to_write_when_clean() {
        let t0 = Instant::now();
        let dirty = [false, false, false]; let done = [None, None, None];
        let s = input(t0, &dirty, &done);
        assert_eq!(next_write(&s), None);
        assert_eq!(wait_hint(&s), None);
    }
}

//! 画面の並びと、ページ切替・折りたたみなどの純粋な判定（egui に依存しない）。

/// キーコンフィグ: 上面のボタン 1〜8 を 横 2 列 × 縦 4 段（値はボタン番号 0 始まり）。
/// 実物の並びと違っていたら、この表を入れ替えるだけで直せる。
pub const TOP_ROWS: [[usize; 2]; 4] = [[0, 1], [2, 3], [4, 5], [6, 7]];
/// サイドボタン（親指側）9〜20 を 横 3 列 × 縦 4 段。左上から右下へ順番に
/// G9 G10 G11 / G12 G13 G14 / G15 G16 G17 / G18 G19 G20
pub const SIDE_ROWS: [[usize; 3]; 4] = [[8, 9, 10], [11, 12, 13], [14, 15, 16], [17, 18, 19]];

/// 割り当て編集の「ショートカットキー」の中の折りたたみ
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section { Windows, Chrome, Custom }

/// 見出しを押したとき: 開いていれば閉じ、別のものなら開いて他は閉じる（同時に開くのは 1 つだけ）
pub fn toggle_section(open: Option<Section>, clicked: Section) -> Option<Section> {
    if open == Some(clicked) { None } else { Some(clicked) }
}

/// ページ切替: (表示する範囲の開始, 終了, ページ数, 補正したページ番号)
pub fn page_range(len: usize, page: usize, per: usize) -> (usize, usize, usize, usize) {
    let pages = len.div_ceil(per.max(1)).max(1);
    let page = page.min(pages - 1);
    let start = page * per;
    (start, (start + per).min(len), pages, page)
}

/// 長い割り当て名を、表示できる文字数で切って「…」を付ける（全文はツールチップで出す）
pub fn ellipsize(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars { return text.to_string(); }
    let mut s: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    s.push('…');
    s
}

/// スライダーのつまみの位置（0〜1）。egui と同じく、logarithmic のときは対数で並べる（範囲は正の値）
pub fn slider_position(value: f64, min: f64, max: f64, log: bool) -> f64 {
    if max <= min { return 0.5; }
    let v = value.clamp(min, max);
    if log && min > 0.0 { (v.ln() - min.ln()) / (max.ln() - min.ln()) } else { (v - min) / (max - min) }
}

/// 手動の折り返し: 各まとまりの幅 widths を、横幅 avail に、間隔 gap で左から詰めて並べたときの行ごとの番号。
/// まとまりの途中では折り返さない（1 つで avail を超えるまとまりは、その 1 つだけで 1 行にする）
pub fn pack_rows(widths: &[f32], avail: f32, gap: f32) -> Vec<Vec<usize>> {
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut used = 0.0;
    for (i, &w) in widths.iter().enumerate() {
        match rows.last_mut() {
            Some(row) if used + gap + w <= avail => { row.push(i); used += gap + w; }
            _ => { rows.push(vec![i]); used = w; }
        }
    }
    rows
}

/// 組と組の間に入れる幅: 間隔 ＋ 区切り線 1px ＋ 間隔。pack_rows の gap に渡すと、区切りの幅も含めて行が決まる
/// （区切りは組と組の間だけなので、行の先頭と末尾の分は数えない）
pub fn separator_gap(space: f32) -> f32 { space * 2.0 + 1.0 }

#[cfg(test)]
mod tests {
    #[test]
    fn separator_is_counted_between_groups_only() {
        use super::{pack_rows, separator_gap};
        let gap = separator_gap(10.0);
        assert_eq!(gap, 21.0);
        // 100 + 21 + 100 + 21 + 100 = 342。先頭・末尾の区切りは数えないので 342 ちょうどで 1 行に入る
        assert_eq!(pack_rows(&[100.0, 100.0, 100.0], 342.0, gap), vec![vec![0, 1, 2]]);
        assert_eq!(pack_rows(&[100.0, 100.0, 100.0], 341.0, gap), vec![vec![0, 1], vec![2]]);
    }
    #[test]
    fn packs_groups_basic() {
        use super::pack_rows;
        assert_eq!(pack_rows(&[100.0, 100.0, 100.0], 316.0, 8.0), vec![vec![0, 1, 2]]);
        assert_eq!(pack_rows(&[100.0, 100.0, 100.0], 315.0, 8.0), vec![vec![0, 1], vec![2]]);
        assert_eq!(pack_rows(&[400.0, 50.0], 300.0, 8.0), vec![vec![0], vec![1]]); // 1 つで幅を超えても 1 行に
        assert!(pack_rows(&[], 300.0, 8.0).is_empty());
    }
    #[test]
    fn slider_positions() {
        use super::slider_position;
        assert_eq!(slider_position(200.0, 200.0, 8200.0, false), 0.0);
        assert_eq!(slider_position(8200.0, 200.0, 8200.0, false), 1.0);
        assert!((slider_position(10.0, 1.0, 100.0, true) - 0.5).abs() < 1e-9);
        assert_eq!(slider_position(0.0, 1.0, 20.0, false), 0.0); // 範囲外は端
    }
    use super::*;
    #[test]
    fn card_tables_cover_all_20_buttons_once() {
        let mut all: Vec<usize> = TOP_ROWS.iter().flatten().chain(SIDE_ROWS.iter().flatten()).copied().collect();
        all.sort();
        assert_eq!(all, (0..20).collect::<Vec<_>>());
        assert_eq!(TOP_ROWS[0], [0, 1]); // 1 段目は 1 と 2
        assert_eq!(SIDE_ROWS[0], [8, 9, 10]); // 1 段目は G9〜G11
        assert_eq!(SIDE_ROWS[2], [14, 15, 16]); // 3 段目は G15〜G17
        assert_eq!(SIDE_ROWS[3], [17, 18, 19]); // 4 段目は G18〜G20
    }
    #[test]
    fn only_one_section_open() {
        assert_eq!(toggle_section(None, Section::Windows), Some(Section::Windows));
        assert_eq!(toggle_section(Some(Section::Windows), Section::Chrome), Some(Section::Chrome));
        assert_eq!(toggle_section(Some(Section::Chrome), Section::Chrome), None);
    }
    #[test]
    fn pages() {
        assert_eq!(page_range(45, 0, 16), (0, 16, 3, 0));
        assert_eq!(page_range(45, 2, 16), (32, 45, 3, 2));
        assert_eq!(page_range(45, 9, 16), (32, 45, 3, 2)); // 範囲外は最後のページ
        assert_eq!(page_range(0, 0, 16), (0, 0, 1, 0));
    }
    #[test]
    fn ellipsis() {
        assert_eq!(ellipsize("キー LCtrl+Num1", 20), "キー LCtrl+Num1");
        assert_eq!(ellipsize("プロファイル切り替え", 6), "プロファイ…");
    }
}

//! 配色 T2「ダーク＋青」と文字・余白の設定（2026-11-13 にユーザーが選んだ案）。

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle};

const fn rgb(hex: u32) -> Color32 { Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8) }

pub const BG: Color32 = rgb(0x111827);
pub const PANEL: Color32 = rgb(0x1E293B);
pub const TEXT: Color32 = rgb(0xF1F5F9);
pub const MUTED: Color32 = rgb(0xAAB8CC);
pub const LINE: Color32 = rgb(0x40516B);
pub const BUTTON: Color32 = rgb(0x29384F);
pub const HOVER: Color32 = rgb(0x374D6B);
pub const PRESS: Color32 = rgb(0x456389);
pub const ACCENT: Color32 = rgb(0x69B4FF);
pub const SELECTED: Color32 = rgb(0x223F63);
/// いま押している物理ボタンの「番号・名称」
pub const HOT_BG: Color32 = rgb(0xFFD166);
pub const HOT_FG: Color32 = rgb(0x201A08);
pub const WARN: Color32 = rgb(0xF6B85E);
/// 入力欄・数値欄の背景
pub const INPUT: Color32 = rgb(0x0B1220);
/// 主ボタン（青）の上の文字
pub const ON_ACCENT: Color32 = rgb(0x0B1A2E);
pub const ERROR: Color32 = rgb(0xFF8F9C);

/// 外周・パネル内の余白
pub const MARGIN: i8 = 8;

/// 本文の文字の大きさ（論理ピクセル）。起動時に Windows の UI の文字の大きさに合わせる
static BODY_PX: std::sync::OnceLock<f32> = std::sync::OnceLock::new();

/// 本文の大きさ（apply の後で有効。読めなければ 12）
pub fn body_px() -> f32 { *BODY_PX.get().unwrap_or(&12.0) }

/// 幅や高さを文字の大きさに合わせて縮める（値は本文 14 のときの大きさで書く）
pub fn s(x: f32) -> f32 { (x * body_px() / 14.0).round() }

/// LOGFONT の高さ（負なら文字の高さ、正ならセルの高さ）と DPI から、論理ピクセルの文字の大きさを求める
pub fn font_px_from(lf_height: i32, dpi: u32) -> f32 {
    if lf_height == 0 || dpi == 0 { return 12.0; }
    let px = lf_height.unsigned_abs() as f32 * 96.0 / dpi as f32;
    // 正の値（セルの高さ）は内部余白を含むので少し小さくする
    let px = if lf_height > 0 { px * 0.8 } else { px };
    px.round().clamp(10.0, 20.0)
}

/// Windows のメッセージ用フォント（エクスプローラーなどと同じ大きさ）を読む
#[cfg(target_os = "windows")]
fn system_font_px() -> Option<f32> {
    #[link(name = "user32")]
    extern "system" {
        fn SystemParametersInfoW(action: u32, param: u32, pv: *mut std::ffi::c_void, win_ini: u32) -> i32;
        fn GetDpiForSystem() -> u32;
    }
    const SPI_GETNONCLIENTMETRICS: u32 = 0x0029;
    // NONCLIENTMETRICSW（504 バイト）。lfMessageFont は 408 バイト目から（先頭が lfHeight）
    let mut buf = [0u8; 504];
    buf[..4].copy_from_slice(&504u32.to_le_bytes());
    let ok = unsafe { SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, 504, buf.as_mut_ptr() as *mut _, 0) } != 0;
    if !ok { return None; }
    let height = i32::from_le_bytes(buf[408..412].try_into().ok()?);
    let dpi = unsafe { GetDpiForSystem() };
    Some(font_px_from(height, dpi))
}

#[cfg(not(target_os = "windows"))]
fn system_font_px() -> Option<f32> { None }

pub fn apply(ctx: &egui::Context) {
    let body = *BODY_PX.get_or_init(|| system_font_px().unwrap_or(12.0));
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.window_stroke = Stroke::new(1.0_f32, LINE);
    v.window_corner_radius = CornerRadius::same(8);
    v.extreme_bg_color = INPUT;
    v.faint_bg_color = rgb(0x1A2436);
    v.warn_fg_color = WARN;
    v.error_fg_color = ERROR;
    v.hyperlink_color = ACCENT;
    v.selection.bg_fill = SELECTED;
    v.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    v.slider_trailing_fill = true;
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.weak_bg_fill = PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, LINE);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    for (state, fill) in [(&mut w.inactive, BUTTON), (&mut w.hovered, HOVER), (&mut w.active, PRESS), (&mut w.open, HOVER)] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        // ボタンは角丸 6。チェックボックスも同じ値を使うため、丸く見えないよう 4 にとどめる
        state.corner_radius = CornerRadius::same(6);
        state.fg_stroke = Stroke::new(1.0_f32, TEXT);
    }
    w.inactive.bg_stroke = Stroke::new(1.0_f32, LINE);
    w.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    w.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    w.open.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    ctx.set_visuals(v);

    ctx.style_mut(|s| {
        s.text_styles = [
            // Windows の UI と同じ大きさ: 本文・ボタン = システムの文字、注記 = 1 小さく、見出し = 2 大きく
            (TextStyle::Heading, FontId::new(body + 2.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(body, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(body, FontFamily::Proportional)),
            (TextStyle::Small, FontId::new(body - 1.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(body, FontFamily::Monospace)),
        ].into();
        s.spacing.item_spacing = egui::vec2(8.0, 4.0);
        // ラジオボタンなどの丸と文字の間
        s.spacing.icon_spacing = 6.0;
        s.spacing.button_padding = egui::vec2(8.0, 3.0);
        s.spacing.interact_size.y = (body + 10.0).max(22.0);
        s.spacing.slider_width = (body * 9.0).round();
        s.spacing.window_margin = egui::Margin::same(MARGIN);
    });
}

/// セクションを囲むパネル
pub fn panel() -> egui::Frame {
    egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0_f32, LINE)).corner_radius(CornerRadius::same(8)).inner_margin(egui::Margin::same(MARGIN))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_font_size() {
        assert_eq!(font_px_from(-12, 96), 12.0);   // 9pt・100%
        assert_eq!(font_px_from(-18, 144), 12.0);  // 9pt・150%
        assert_eq!(font_px_from(-15, 96), 15.0);
        assert_eq!(font_px_from(0, 96), 12.0);     // 読めないとき
        assert_eq!(font_px_from(-60, 96), 20.0);   // 大きすぎる値は抑える
    }
}

/// パネルの見出し（太字）と、右に小さい薄い注記
pub fn header(ui: &mut egui::Ui, title: &str, note_text: Option<&str>) {
    ui.horizontal_wrapped(|ui| {
        ui.label(egui::RichText::new(super::i18n::tr(title)).strong().family(bold_family()).size(body_px() + 1.0));
        if let Some(n) = note_text { ui.label(note(n)); }
    });
}

/// 普通のボタン（背景 #29384F・枠 #40516B・角丸 6）
pub fn button<'a>(text: impl AsRef<str>) -> egui::Button<'a> {
    egui::Button::new(super::i18n::tr(text)).fill(BUTTON).stroke(Stroke::new(1.0_f32, LINE)).corner_radius(CornerRadius::same(6))
}

/// 主ボタン（青い背景・濃い文字・太字）: キーコンフィグを開く、登録、変更
pub fn primary<'a>(text: &str) -> egui::Button<'a> {
    egui::Button::new(egui::RichText::new(super::i18n::tr(text)).strong().family(bold_family()).color(ON_ACCENT)).fill(ACCENT).stroke(Stroke::new(1.0_f32, ACCENT)).corner_radius(CornerRadius::same(6))
}

/// 選択できるボタン（タブなど）。選択中は 背景 #223F63・枠 #69B4FF・太字
pub fn toggle<'a>(text: impl Into<String>, on: bool) -> egui::Button<'a> {
    let text = super::i18n::tr(text.into());
    toggle_raw(&text, on)
}

/// User-assigned shortcut names are never translated.
pub fn toggle_raw<'a>(text: &str, on: bool) -> egui::Button<'a> {
    if on { egui::Button::new(egui::RichText::new(text).strong().family(bold_family())).fill(SELECTED).stroke(Stroke::new(1.0_f32, ACCENT)).corner_radius(CornerRadius::same(6)) }
    else { egui::Button::new(text.to_string()).fill(BUTTON).stroke(Stroke::new(1.0_f32, LINE)).corner_radius(CornerRadius::same(6)) }
}

/// チェックボックス（□ と文字を 1 つのまとまりにした標準の形。□ と文字の間は 6、オンは □ を青く塗り白いチェック）
pub fn chk(ui: &mut egui::Ui, value: &mut bool, text: impl AsRef<str>) -> egui::Response {
    let on = *value;
    ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        for state in [&mut w.inactive, &mut w.hovered, &mut w.active] {
            state.corner_radius = CornerRadius::same(3);
            state.bg_fill = if on { ACCENT } else { INPUT };
            state.weak_bg_fill = state.bg_fill;
            state.bg_stroke = Stroke::new(1.0_f32, if on { ACCENT } else { MUTED });
            // チェックの印と文字は同じ色（egui の仕様）。白にする
            state.fg_stroke = Stroke::new(1.5_f32, TEXT);
        }
        // egui のチェックボックスは最小幅（interact_size）まで広がるので、□＋文字の幅ちょうどにする
        ui.spacing_mut().interact_size.x = 0.0;
        ui.spacing_mut().icon_spacing = 6.0;
        ui.checkbox(value, super::i18n::tr(text))
    }).inner
}

/// ラジオボタン（○ と文字のまとまり。選択中は青）
pub fn radio(ui: &mut egui::Ui, enabled: bool, selected: bool, text: &str) -> egui::Response {
    ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        for state in [&mut w.inactive, &mut w.hovered, &mut w.active] {
            state.bg_fill = INPUT;
            state.bg_stroke = Stroke::new(1.0_f32, if selected { ACCENT } else { MUTED });
            state.fg_stroke = Stroke::new(1.0_f32, if selected { ACCENT } else { TEXT });
        }
        ui.spacing_mut().interact_size.x = 0.0;
        ui.spacing_mut().icon_spacing = 6.0;
        ui.add_enabled(enabled, egui::RadioButton::new(selected, egui::RichText::new(super::i18n::tr(text)).color(TEXT)))
    }).inner
}

/// 色見本ボタン（幅 36・高さ 22・角丸 6・枠 #40516B、中は色で塗る）。押すとカラーピッカーを開く。色が変わったら true
pub fn color_swatch(ui: &mut egui::Ui, id: &str, rgb: &mut [u8; 3]) -> bool {
    let size = egui::vec2(s(36.0), s(22.0));
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
    let stroke = if response.hovered() { Stroke::new(1.0_f32, ACCENT) } else { Stroke::new(1.0_f32, LINE) };
    ui.painter().rect(rect, CornerRadius::same(6), color, stroke, egui::StrokeKind::Inside);
    let popup = ui.make_persistent_id(id);
    if response.clicked() { ui.memory_mut(|m| m.toggle_popup(popup)); }
    let mut changed = false;
    egui::popup::popup_below_widget(ui, popup, &response, egui::PopupCloseBehavior::CloseOnClickOutside, |ui| {
        let mut c = color;
        if egui::color_picker::color_picker_color32(ui, &mut c, egui::color_picker::Alpha::Opaque) {
            *rgb = [c.r(), c.g(), c.b()];
            changed = true;
        }
    });
    changed
}

/// 選択欄（ドロップダウン）・数値欄を入力欄の見た目にする: 背景 #0b1220・枠 #40516B・角丸 6。
/// ホバー中は枠を青にする（開いたメニューの行のホバー色 #374D6B はそのまま）
pub fn input_scope<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        w.inactive.weak_bg_fill = INPUT;
        w.inactive.bg_fill = INPUT;
        w.inactive.bg_stroke = Stroke::new(1.0_f32, LINE);
        w.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
        w.inactive.corner_radius = CornerRadius::same(6);
        w.hovered.weak_bg_fill = INPUT;
        w.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
        w.open.weak_bg_fill = INPUT;
        add(ui)
    }).inner
}

/// パネルの中の小枠（DPI の各段、折りたたみ）
pub fn sub_box() -> egui::Frame {
    egui::Frame::new().stroke(Stroke::new(1.0_f32, LINE)).corner_radius(CornerRadius::same(6)).inner_margin(egui::Margin::symmetric(8, 6))
}

/// 段階の小見出し（青・太字・小さめ）: 「1. 組み合わせを作る」など
pub fn step(text: &str) -> egui::RichText {
    egui::RichText::new(super::i18n::tr(text)).small().strong().family(bold_family()).color(ACCENT)
}

/// 太字用のフォントファミリー（g600gui.rs で Yu Gothic UI Bold を登録。egui には疑似ボールドが無い）
pub fn bold_family() -> egui::FontFamily { egui::FontFamily::Name("bold".into()) }

/// 太字の文字
pub fn bold(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(super::i18n::tr(text.into())).strong().family(bold_family())
}

/// 注記（薄い文字）
pub fn note(text: impl Into<String>) -> egui::RichText {
    egui::RichText::new(super::i18n::tr(text.into())).small().color(MUTED)
}

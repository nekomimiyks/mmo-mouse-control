use eframe::egui::{self, Color32, Stroke};

pub fn body_px() -> f32 {
    static BODY: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *BODY.get_or_init(|| {
        #[cfg(windows)]
        unsafe {
            #[link(name = "user32")]
            extern "system" {
                fn SystemParametersInfoW(
                    action: u32,
                    param: u32,
                    value: *mut std::ffi::c_void,
                    flags: u32,
                ) -> i32;
                fn GetDpiForSystem() -> u32;
            }
            let mut buffer = [0u8; 504];
            buffer[..4].copy_from_slice(&504u32.to_le_bytes());
            if SystemParametersInfoW(0x29, 504, buffer.as_mut_ptr().cast(), 0) != 0 {
                let height = i32::from_le_bytes(buffer[408..412].try_into().unwrap());
                let dpi = GetDpiForSystem();
                if height != 0 && dpi != 0 {
                    return (height.unsigned_abs() as f32 * 96.0 / dpi as f32
                        * if height > 0 { 0.8 } else { 1.0 })
                    .round()
                    .clamp(10.0, 20.0);
                }
            }
        }
        12.0
    })
}
pub fn s(value: f32) -> f32 {
    (value * body_px() / 14.0).round()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SystemTheme {
    pub dark: bool,
    pub accent: Color32,
}
pub fn system() -> SystemTheme {
    let mut value = SystemTheme {
        dark: false,
        accent: Color32::from_rgb(0, 95, 184),
    };
    #[cfg(windows)]
    unsafe {
        #[link(name = "advapi32")]
        extern "system" {
            fn RegGetValueW(
                key: isize,
                sub: *const u16,
                name: *const u16,
                flags: u32,
                kind: *mut u32,
                data: *mut std::ffi::c_void,
                size: *mut u32,
            ) -> i32;
        }
        #[link(name = "dwmapi")]
        extern "system" {
            fn DwmGetColorizationColor(color: *mut u32, opaque: *mut i32) -> i32;
        }
        let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let key = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
        let name = wide("AppsUseLightTheme");
        let mut light = 1u32;
        let mut size = 4u32;
        if RegGetValueW(
            0x80000001u32 as i32 as isize,
            key.as_ptr(),
            name.as_ptr(),
            0x10,
            std::ptr::null_mut(),
            (&mut light as *mut u32).cast(),
            &mut size,
        ) == 0
        {
            value.dark = light == 0;
        }
        let mut color = 0u32;
        let mut opaque = 0;
        if DwmGetColorizationColor(&mut color, &mut opaque) >= 0 {
            value.accent = Color32::from_rgb((color >> 16) as u8, (color >> 8) as u8, color as u8);
        }
    }
    value
}
pub fn rgb(h: u32) -> Color32 {
    Color32::from_rgb((h >> 16) as u8, (h >> 8) as u8, h as u8)
}
fn luminance(c: Color32) -> f32 {
    let linear = |x: u8| {
        let s = x as f32 / 255.0;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
}
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let a = luminance(a);
    let b = luminance(b);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}
pub fn on_accent(color: Color32) -> Color32 {
    if contrast(color, Color32::BLACK) >= contrast(color, Color32::WHITE) {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}
#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub surface: Color32,
    pub alt: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub border: Color32,
    pub accent: Color32,
    pub selected: Color32,
}
impl Palette {
    pub fn new(s: SystemTheme) -> Self {
        let (bg, surface, alt, text, muted, border) = if s.dark {
            (0x202020, 0x2B2B2B, 0x242424, 0xF5F5F5, 0xBFBFBF, 0x939393)
        } else {
            (0xF3F3F3, 0xFFFFFF, 0xF7F7F7, 0x1A1A1A, 0x5A5A5A, 0x797979)
        };
        let surface = rgb(surface);
        let blend = |a: u8, b: u8| ((a as u16 * 17 + b as u16 * 83) / 100) as u8;
        Self {
            bg: rgb(bg),
            surface,
            alt: rgb(alt),
            text: rgb(text),
            muted: rgb(muted),
            border: rgb(border),
            accent: s.accent,
            selected: Color32::from_rgb(
                blend(s.accent.r(), surface.r()),
                blend(s.accent.g(), surface.g()),
                blend(s.accent.b(), surface.b()),
            ),
        }
    }
    pub fn apply(self, ctx: &egui::Context, dark: bool) {
        let body = body_px();
        let mut v = if dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        v.override_text_color = None;
        v.panel_fill = self.bg;
        v.window_fill = self.surface;
        v.extreme_bg_color = self.surface;
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.fg_stroke.color = self.text;
        }
        v.selection.bg_fill = self.selected;
        v.selection.stroke = Stroke::new(1.0_f32, self.text);
        v.slider_trailing_fill = true;
        v.window_corner_radius = egui::CornerRadius::same(8);
        v.widgets.inactive.bg_fill = self.border;
        v.widgets.inactive.weak_bg_fill = self.surface;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, self.border);
        v.widgets.hovered.bg_fill = self.accent;
        v.widgets.hovered.weak_bg_fill = self.selected;
        v.widgets.hovered.bg_stroke = Stroke::new(1.5_f32, self.accent);
        v.widgets.active.bg_fill = self.accent;
        v.widgets.active.weak_bg_fill = self.selected;
        for state in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            state.corner_radius = egui::CornerRadius::same(6);
        }
        ctx.set_visuals(v);
        ctx.style_mut(|s| {
            s.spacing.item_spacing = egui::vec2(8.0, 4.0);
            s.spacing.button_padding = egui::vec2(8.0, 3.0);
            s.spacing.interact_size.y = (body + 10.0).max(22.0);
            s.spacing.slider_width = 108.0;
            s.spacing.window_margin = egui::Margin::same(8);
            s.text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(body));
            s.text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(body));
            s.text_styles.insert(
                egui::TextStyle::Heading,
                egui::FontId::proportional(body + 3.0),
            );
            s.text_styles.insert(
                egui::TextStyle::Small,
                egui::FontId::proportional(body - 1.0),
            );
        });
    }
    pub fn card(self) -> egui::Frame {
        egui::Frame::new()
            .fill(self.surface)
            .corner_radius(8)
            .inner_margin(8)
            .stroke(Stroke::new(1.0_f32, self.border.gamma_multiply(0.35)))
    }
    pub fn primary(self, label: &str) -> egui::Button<'_> {
        egui::Button::new(
            egui::RichText::new(label)
                .family(egui::FontFamily::Name("bold".into()))
                .strong()
                .color(on_accent(self.accent)),
        )
        .fill(self.accent)
        .stroke(Stroke::new(1.0_f32, self.accent))
        .corner_radius(6)
    }
}
pub fn install_fonts(ctx: &egui::Context) {
    let root = std::path::PathBuf::from(std::env::var_os("WINDIR").unwrap_or("C:\\Windows".into()))
        .join("Fonts");
    for (file, index) in [("meiryo.ttc", 2), ("YuGothM.ttc", 1)] {
        if let Ok(bytes) = std::fs::read(root.join(file)) {
            let mut data = egui::FontData::from_owned(bytes);
            data.index = index;
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert("japanese".into(), data.into());
            fonts
                .families
                .get_mut(&egui::FontFamily::Proportional)
                .unwrap()
                .insert(0, "japanese".into());
            let mut bold_family = fonts.families[&egui::FontFamily::Proportional].clone();
            if let Ok(bytes) = std::fs::read(root.join("meiryob.ttc")) {
                let mut bold = egui::FontData::from_owned(bytes);
                bold.index = 2;
                fonts.font_data.insert("japanese-bold".into(), bold.into());
                bold_family.insert(0, "japanese-bold".into());
            }
            fonts
                .families
                .insert(egui::FontFamily::Name("bold".into()), bold_family);
            ctx.set_fonts(fonts);
            break;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accent_text_readable() {
        for color in [
            rgb(0xFFFF00),
            rgb(0xFF0000),
            rgb(0x0078D4),
            rgb(0xFFFFFF),
            rgb(0x000000),
        ] {
            assert!(contrast(color, on_accent(color)) >= 4.5);
        }
    }
}

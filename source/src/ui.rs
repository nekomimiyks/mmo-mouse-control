//! G600rと共通のメイン／キーコンフィグ／割当編集という画面構成。
#[path = "editor.rs"]
mod editor;
use crate::{
    pointer,
    theme::{self, Palette, SystemTheme},
};
use eframe::egui::{self, RichText, Stroke, Vec2};
use g600ctl::{gui::{file_dialog, i18n::tr}, mouse_backup::Mouse};
use m908ctl::{
    device::{self, Device},
    identification::{self, Client, State},
    model::{self, Profile, BUTTONS, EFFECTS},
    presets::{CHROME, WINDOWS},
    settings::Snapshot,
};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

pub struct Options {
    pub theme: usize,
    pub capture: Option<PathBuf>,
    pub size: [f32; 2],
    pub scale: Option<f32>,
    page: usize,
    editor: bool,
    capture_device: bool,
    fixed_size: bool,
    editor_tab: Option<usize>,
    editor_page: usize,
    preview_press: Option<usize>,
}
impl Options {
    pub fn parse() -> Self {
        let mut o = Self {
            theme: 0,
            capture: None,
            size: [900.0, 560.0],
            scale: None,
            page: 0,
            editor: false,
            capture_device: false,
            fixed_size: false,
            editor_tab: None,
            editor_page: 0,
            preview_press: None,
        };
        for arg in std::env::args().skip(1) {
            if let Some(v) = arg.strip_prefix("--preview-press=") {
                o.preview_press = v
                    .parse::<usize>()
                    .ok()
                    .filter(|v| (1..=20).contains(v))
                    .map(|v| v - 1);
            }
            if arg == "--capture-device" {
                o.capture_device = true;
            }
            if let Some(v) = arg.strip_prefix("--theme=") {
                o.theme = match v {
                    "light" => 1,
                    "dark" => 2,
                    _ => 0,
                };
            }
            if let Some(v) = arg.strip_prefix("--capture=") {
                o.capture = Some(v.into());
            }
            if let Some(v) = arg.strip_prefix("--page=") {
                o.page = match v {
                    "keys" | "buttons" => 1,
                    "device" => 2,
                    "settings" => 3,
                    "editor" => {
                        o.editor = true;
                        1
                    }
                    _ => 0,
                };
            }
            if let Some(v) = arg.strip_prefix("--size=") {
                if let Some((w, h)) = v.split_once('x') {
                    if let (Ok(w), Ok(h)) = (w.parse::<f32>(), h.parse::<f32>()) {
                        if w.is_finite() && h.is_finite() {
                            o.size = [w.max(640.0), h.max(520.0)];
                            o.fixed_size = true;
                        }
                    }
                }
            }
            if let Some(v) = arg.strip_prefix("--editor-tab=") {
                o.editor_tab = match v {
                    "windows" => Some(0),
                    "chrome" => Some(1),
                    "custom" => Some(2),
                    _ => None,
                };
            }
            if let Some(v) = arg.strip_prefix("--editor-page=") {
                o.editor_page = v.parse().unwrap_or(0);
            }
            if let Some(v) = arg.strip_prefix("--scale=") {
                o.scale = v
                    .parse::<f32>()
                    .ok()
                    .filter(|s| s.is_finite() && (1.0..=2.5).contains(s));
            }
        }
        if o.capture.is_none() {
            o.scale = None;
        }
        o
    }
}
enum Reply {
    Devices(Result<Vec<Device>, String>),
    Read(Result<Snapshot, String>),
    Applied(Option<usize>, bool, Result<device::Applied, String>),
    Backup(Result<(Snapshot, PathBuf), String>),
    Restored(Result<device::Applied, String>),
}
struct Editor {
    slot: usize,
    original: String,
    action: String,
    tab: Option<usize>,
    capture: bool,
    name: String,
    notice: String,
    list_page: usize,
    custom_page: usize,
    candidate_page: usize,
    search: String,
    modifiers: u8,
    key: String,
    last_auto_name: String,
    tab_height: f32,
    content_height: f32,
}
#[derive(Clone, Copy)]
enum ReloadTarget {
    Scan,
    SelectedDevice,
}

pub struct App {
    options: Options,
    profiles: [Profile; 5],
    saved: [Profile; 5],
    initial: [Profile; 5],
    profile: usize,
    page: usize,
    devices: Vec<Device>,
    selected_device: usize,
    job: Option<Receiver<Reply>>,
    raw: Option<String>,
    live: Option<Snapshot>,
    baselines: [Option<Snapshot>; 5],
    backup_view: bool,
    reload_confirm: Option<ReloadTarget>,
    initial_loaded: bool,
    message: String,
    draft: PathBuf,
    last_backup_path: Option<PathBuf>,
    pal: Palette,
    last_theme: SystemTheme,
    theme_check: Instant,
    frame: usize,
    capture_sent: bool,
    started: Instant,
    pointer: Option<pointer::PointerSettings>,
    pointer_message: String,
    pointer_pending: bool,
    editor: Option<Editor>,
    custom: Vec<(String, String)>,
    custom_path: PathBuf,
    show_device: bool,
    show_settings: bool,
    confirm_reset: bool,
    close_confirm: bool,
    exit_allowed: bool,
    identification: Option<Client>,
    identifying: bool,
    pressed: [bool; 20],
    flashed: [Option<Instant>; 20],
    physical_slot: Option<usize>,
    identification_retry_blocked: bool,
    recovery_failed: bool,
    close_after_restore: bool,
    marker_frame: bool,
    main_pending: [Option<Instant>; 5],
    key_pending: [bool; 5],
    pending_backup: Option<PathBuf>,
    backup_confirmation: Option<(Option<std::ffi::CString>, bool)>,
    editor_bottom: f32,
}
fn retain_shortcut_names(actual: &mut [Profile; 5], previous: &[Profile; 5]) {
    for (profile, old) in actual.iter_mut().zip(previous) {
        for (action, label) in profile.buttons.iter_mut().zip(&old.buttons) {
            if m908ctl::actions::shortcut(label).is_some()
                && matches!((m908ctl::actions::encode(action), m908ctl::actions::encode(label)),
                    (Ok(a), Ok(b)) if a == b)
            {
                *action = label.clone();
            }
        }
    }
}
fn merge_profiles(
    local: &mut [Profile; 5],
    saved: &mut [Profile; 5],
    actual: &[Profile; 5],
    scope: Option<usize>,
    edited: [bool; 5],
    automatic: bool,
) -> [bool; 5] {
    std::array::from_fn(|p| {
        let refresh = scope == Some(p) || !edited[p];
        if refresh {
            let buttons = local[p].buttons.clone();
            local[p] = actual[p].clone();
            saved[p] = actual[p].clone();
            if automatic && scope == Some(p) {
                local[p].buttons = buttons;
            }
        }
        refresh
    })
}
fn bold(text: impl Into<String>) -> RichText {
    RichText::new(text)
        .family(egui::FontFamily::Name("bold".into()))
        .strong()
}
fn note(text: impl Into<String>, p: Palette) -> RichText {
    RichText::new(text).small().color(p.muted)
}
fn toggle(p: Palette, text: impl Into<String>, on: bool) -> egui::Button<'static> {
    egui::Button::new(if on { bold(text) } else { RichText::new(text) })
        .fill(if on { p.selected } else { p.alt })
        .stroke(Stroke::new(1.0_f32, if on { p.accent } else { p.border }))
        .corner_radius(6)
}
fn heading(ui: &mut egui::Ui, p: Palette, title: &str, description: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.label(bold(title).size(theme::body_px() + 1.0));
        if !description.is_empty() {
            ui.label(note(description, p));
        }
    });
}
fn name_cell(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    name: &str,
    lit: bool,
    pal: Palette,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
    let fg = if lit { theme::rgb(0x201A08) } else { pal.text };
    if lit {
        ui.painter().rect_filled(rect, 6, theme::rgb(0xFFD166));
        ui.painter()
            .rect_stroke(rect, 6, Stroke::new(2.0_f32, fg), egui::StrokeKind::Inside);
    }
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect.shrink2(Vec2::new(6.0, 3.0)))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child.add(egui::Label::new(bold(name).color(fg)).truncate());
    response.on_hover_text(name)
}
#[derive(Debug, PartialEq, Eq)]
enum IdentificationAction {
    Start,
    Stop,
    Wait,
}
/// Some(true)は復元中。戻ってきても、復元完了の通知までは次の監視を起動しない。
fn identification_action(
    wanted: bool,
    session: Option<bool>,
    can_start: bool,
) -> IdentificationAction {
    match session {
        Some(false) if !wanted => IdentificationAction::Stop,
        Some(_) => IdentificationAction::Wait,
        None if wanted && can_start => IdentificationAction::Start,
        None => IdentificationAction::Wait,
    }
}
const TOP: [[usize; 2]; 4] = [[0, 1], [2, 3], [4, 5], [18, 19]];
fn check(ui: &mut egui::Ui, pal: Palette, value: &mut bool, text: &str) -> egui::Response {
    let on = *value;
    ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        for state in [&mut w.inactive, &mut w.hovered, &mut w.active] {
            state.corner_radius = egui::CornerRadius::same(3);
            state.bg_fill = if on { pal.accent } else { pal.surface };
            state.weak_bg_fill = state.bg_fill;
        }
        ui.checkbox(value, text)
    })
    .inner
}
// G600rと同じ細いレール、左側のアクセント色、直径14の丸いつまみ。
fn slider_field(
    ui: &mut egui::Ui,
    pal: Palette,
    value: &mut u32,
    range: std::ops::RangeInclusive<u32>,
    reserve: f32,
) -> bool {
    let field_width = theme::s(if *range.end() <= 100 { 46.0 } else { 60.0 });
    let width = (ui.available_width() - field_width - reserve - 16.0).max(60.0);
    let (min, max) = (*range.start() as f32, *range.end() as f32);
    let response = ui
        .scope(|ui| {
            ui.spacing_mut().slider_width = width;
            let v = ui.visuals_mut();
            v.selection.bg_fill = egui::Color32::TRANSPARENT;
            for w in [
                &mut v.widgets.inactive,
                &mut v.widgets.hovered,
                &mut v.widgets.active,
            ] {
                w.bg_fill = egui::Color32::TRANSPARENT;
                w.bg_stroke = Stroke::NONE;
                w.fg_stroke = Stroke::NONE;
                w.expansion = 0.0;
            }
            ui.add(egui::Slider::new(value, range.clone()).show_value(false))
        })
        .inner;
    let r = response.rect;
    let norm = (*value as f32 - min) / (max - min);
    let radius = r.height() / 2.5;
    let x = r.left() + radius + (r.width() - 2.0 * radius) * norm.clamp(0.0, 1.0);
    let y = r.center().y;
    let alpha = if ui.is_enabled() { 1.0 } else { 0.4 };
    ui.painter().rect_filled(
        egui::Rect::from_min_max(
            egui::pos2(r.left(), y - 2.0),
            egui::pos2(r.right(), y + 2.0),
        ),
        2,
        pal.border.gamma_multiply(alpha),
    );
    ui.painter().rect_filled(
        egui::Rect::from_min_max(egui::pos2(r.left(), y - 2.0), egui::pos2(x, y + 2.0)),
        2,
        pal.accent.gamma_multiply(alpha),
    );
    ui.painter()
        .circle_filled(egui::pos2(x, y), 7.0, pal.text.gamma_multiply(alpha));
    let field = ui.add_sized(
        [field_width, theme::s(24.0)],
        egui::DragValue::new(value).range(range),
    );
    response.changed() || field.changed()
}
const SIDE: [[usize; 3]; 4] = [[6, 7, 8], [9, 10, 11], [12, 13, 14], [15, 16, 17]];

pub(crate) fn settings_directory() -> std::path::PathBuf {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    #[cfg(windows)]
    {
        use windows_sys::Win32::{Foundation::ERROR_INSUFFICIENT_BUFFER, Storage::Packaging::Appx::GetCurrentPackageFullName};
        let mut length = 0;
        // Query the length only. An unpackaged process has no package identity.
        let result = unsafe { GetCurrentPackageFullName(&mut length, std::ptr::null_mut()) };
        if result == ERROR_INSUFFICIENT_BUFFER || exe_dir.join(".mmo-installed").is_file() {
            if let Some(local) = std::env::var_os("LOCALAPPDATA") {
                let path = std::path::PathBuf::from(local).join("MMOMouseControl");
                let _ = std::fs::create_dir_all(&path);
                return path;
            }
        }
    }
    exe_dir
}

impl App {
    pub fn new(ctx: &egui::Context, options: Options) -> Self {
        let sys = theme::system();
        let pal = Palette::new(sys);
        pal.apply(ctx, sys.dark);
        let dir = settings_directory();
        let draft = dir.join("m908.draft");
        let custom_path = dir.join("m908-custom.txt");
        let mut profiles = model::defaults();
        let mut message = "マウスの接続を確認しています…".to_string();
        if options.capture.is_none() {
            match std::fs::read_to_string(&draft) {
                Ok(data) => match model::decode(&data) {
                    Ok(p) => profiles = p,
                    Err(e) => message = format!("下書きを読めません: {e}"),
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => message = e.to_string(),
            }
        }
        if options.capture.is_some() {
            profiles[0].buttons[10] = "Chrome: タブを閉じる".into();
        }
        let custom = if options.capture.is_none() {
            std::fs::read_to_string(&custom_path)
                .ok()
                .map(|s| {
                    s.lines()
                        .filter_map(|l| l.split_once('\t'))
                        .filter(|(n, a)| {
                            !n.is_empty() && !a.is_empty() && n.len() <= 1024 && a.len() <= 1024
                        })
                        .map(|(n, a)| (n.to_string(), a.to_string()))
                        .collect()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let pointer = if options.capture.is_some() {
            Some(pointer::PointerSettings {
                speed: pointer::SPEED_DEFAULT,
                acceleration: false,
                wheel_lines: 3,
                wheel_page: false,
                wheel_chars: pointer::CHARS_DEFAULT,
            })
        } else {
            pointer::read().ok()
        };
        let page = options.page.min(1);
        let show_device = options.page == 2;
        let show_settings = options.page == 3;
        let show_editor = options.editor;
        let mut app = Self {
            saved: profiles.clone(),
            initial: profiles.clone(),
            profiles,
            options,
            profile: 0,
            page,
            devices: Vec::new(),
            selected_device: 0,
            job: None,
            raw: None,
            live: None,
            baselines: std::array::from_fn(|_| None),
            backup_view: false,
            reload_confirm: None,
            initial_loaded: false,
            message,
            last_backup_path: None,
            draft,
            pal,
            last_theme: sys,
            theme_check: Instant::now() - Duration::from_secs(2),
            frame: 0,
            capture_sent: false,
            started: Instant::now(),
            pointer,
            pointer_message: String::new(),
            pointer_pending: false,
            editor: None,
            custom,
            custom_path,
            show_device,
            show_settings,
            confirm_reset: false,
            close_confirm: false,
            exit_allowed: false,
            identification: None,
            identifying: false,
            pressed: [false; 20],
            flashed: [None; 20],
            physical_slot: None,
            identification_retry_blocked: false,
            recovery_failed: false,
            close_after_restore: false,
            marker_frame: false,
            main_pending: [None; 5],
            key_pending: [false; 5],
            pending_backup: None,
            backup_confirmation: None,
            editor_bottom: 0.0,
        };
        if show_editor {
            app.open_editor(10);
            if let Some(editor) = &mut app.editor {
                editor.tab = app.options.editor_tab;
                editor.list_page = app.options.editor_page;
            }
        }
        if app.preview() {
            if let Some(slot) = app.options.preview_press {
                app.identifying = true;
                app.pressed[slot] = true;
                app.physical_slot = Some(slot);
            }
        }
        if app.options.capture_device {
            app.scan();
        }
        app
    }
    fn start_identification(&mut self, ctx: &egui::Context) {
        if self.preview()
            || self.job.is_some()
            || self.identification.is_some()
            || self.recovery_failed
        {
            return;
        }
        self.identification_retry_blocked = true;
        let Some(device) = self
            .devices
            .get(self.selected_device)
            .filter(|d| d.is_config())
            .cloned()
        else {
            self.message = "先にM908を接続して再読み込みしてください。".into();
            return;
        };
        if self.live.is_none() {
            return;
        }
        if self.devices.iter().filter(|d| d.is_config()).count() != 1 {
            self.message = "物理ボタンの識別中はM908を1台だけ接続してください。".into();
            return;
        }
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => {
                self.message = format!("復元監視を起動できません: {e}");
                return;
            }
        };
        let repaint = ctx.clone();
        match Client::start_with_wake(
            &exe,
            &device,
            &self.draft.with_file_name(identification::JOURNAL),
            move || repaint.request_repaint(),
        ) {
            Ok(client) => {
                self.identification_retry_blocked = false;
                self.identification = Some(client);
                self.identifying = false;
                self.physical_slot = None;
                self.message="識別用プロファイルを準備しています…（変更対象の退避と借用先の確認）。画面を離れると取り消して復元します。".into();
            }
            Err(e) => self.message = e,
        }
    }
    fn stop_identification(&mut self) {
        if let Some(client) = &mut self.identification {
            if !client.stopping {
                client.stop();
                self.message = "変更後の通常設定へ直接戻し、借用先も復元・確認しています…".into();
            }
        }
        self.identifying = false;
        self.pressed.fill(false);
        self.flashed.fill(None);
    }
    fn sync_identification(&mut self, ctx: &egui::Context) {
        let focused =
            ctx.input(|i| i.focused) && identification::foreground_process() == std::process::id();
        if self.page != 1 || !focused {
            self.identification_retry_blocked = false;
        }
        let wanted = self.page == 1
            && focused
            && !self.preview()
            && !self.exit_allowed
            && !self.close_after_restore
            && !self.close_confirm
            && self.reload_confirm.is_none()
            && !self.confirm_reset
            && !self.show_device
            && !ctx.input(|i| i.viewport().close_requested());
        let can_start = self.job.is_none()
            && self.live.is_some()
            && !self.recovery_failed
            && !self.identification_retry_blocked
            && !self.key_pending.iter().any(|v| *v)
            && self.main_pending.iter().all(Option::is_none)
            && self.pending_backup.is_none();
        match identification_action(
            wanted,
            self.identification.as_ref().map(|c| c.stopping),
            can_start,
        ) {
            IdentificationAction::Start => self.start_identification(ctx),
            IdentificationAction::Stop => self.stop_identification(),
            IdentificationAction::Wait => {}
        }
    }
    fn update_identified_button(&mut self, slot: usize, down: bool) {
        if !self.identifying {
            return;
        }
        if identification::SLOTS.contains(&slot) {
            self.marker_frame = true;
        }
        self.pressed[slot] = down && slot < 18;
        if down {
            self.flashed[slot] = Some(Instant::now());
            self.physical_slot = Some(slot);
        }
    }

    fn update_identification(&mut self, ctx: &egui::Context) {
        self.marker_frame = false;
        if self.identification.is_some() && (!ctx.input(|i| i.focused) || self.page != 1) {
            self.stop_identification();
        }
        let mut events = Vec::new();
        if let Some(client) = &mut self.identification {
            client.heartbeat();
            while let Ok(s) = client.events.try_recv() {
                events.push(s);
            }
        }
        for state in events {
            match state {
                State::Ready => {
                    if self.identification.as_ref().is_some_and(|c| !c.stopping) {
                        self.identifying = true;
                        self.message="識別中です。押した物理ボタンの箇所が点灯します。割当のセルをクリックすると編集できます。画面を離れると復元します。".into();
                    }
                }
                State::Restoring => {
                    self.stop_identification();
                }
                State::Button(slot, down) => self.update_identified_button(slot, down),
                State::ButtonMask(mask) => {
                    for slot in 0..18 {
                        let down = mask & (1 << slot) != 0;
                        if down != self.pressed[slot] {
                            self.update_identified_button(slot, down);
                        }
                    }
                }
                State::Snapshot(snapshot) => self.accept_identified_snapshot(snapshot),
                State::Restored => {
                    self.identification = None;
                    self.identifying = false;
                    self.pressed.fill(false);
                    self.flashed.fill(None);
                    self.message = "変更後の設定へ直接復元し、読み戻し一致を確認しました。".into();
                }
                State::Cancelled => {
                    self.identification = None;
                    self.identifying = false;
                    self.pressed.fill(false);
                    self.flashed.fill(None);
                    self.message =
                        "識別モードの準備を中止しました。本体の設定は変更していません。".into();
                }
                State::Failed(e) => {
                    self.identification_retry_blocked = true;
                    self.main_pending = [None; 5];
                    self.key_pending = [false; 5];
                    self.pending_backup = None;
                    self.close_after_restore = false;
                    self.identification = None;
                    self.identifying = false;
                    self.pressed.fill(false);
                    self.flashed.fill(None);
                    self.recovery_failed =
                        self.draft.with_file_name(identification::JOURNAL).exists();
                    self.live = None;
                    self.message = e;
                }
            }
        }
    }
    fn preview(&self) -> bool {
        self.options.capture.is_some()
    }
    fn accept_identified_snapshot(&mut self, snapshot: Snapshot) {
        if !self.backup_view {
            if let Ok(mut actual) = snapshot.profiles() {
                retain_shortcut_names(&mut actual, &self.profiles);
                let edited = std::array::from_fn(|p| {
                    let differs = m908ctl::settings::plan(&snapshot, p, &self.profiles[p])
                        .map_or(true, |plan| !plan.packets.is_empty());
                    if !differs {
                        self.key_pending[p] = false;
                    }
                    differs
                });
                let refresh = merge_profiles(
                    &mut self.profiles,
                    &mut self.saved,
                    &actual,
                    None,
                    edited,
                    false,
                );
                for p in 0..5 {
                    if refresh[p] {
                        self.baselines[p] = Some(snapshot.clone());
                    }
                }
            }
        }
        self.raw = Some(snapshot.to_text());
        self.live = Some(snapshot);
    }
    fn dirty(&self) -> bool {
        self.profiles != self.saved
    }
    fn unapplied(&self) -> bool {
        self.live.as_ref().is_some_and(|s| {
            (0..5).any(|p| {
                m908ctl::settings::plan(s, p, &self.profiles[p])
                    .map_or(true, |plan| !plan.packets.is_empty())
            })
        })
    }
    fn scan(&mut self) {
        if self.identification.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.job = Some(rx);
        let journal = self.draft.with_file_name(identification::JOURNAL);
        std::thread::spawn(move || {
            let result = identification::recover(&journal).and_then(|_| device::enumerate());
            let _ = tx.send(Reply::Devices(result));
        });
    }
    fn save(&mut self) {
        match model::save(&self.draft, &self.profiles) {
            Ok(()) => {
                self.saved = self.profiles.clone();
                self.message = "下書きをPCに保存しました。".into();
            }
            Err(e) => self.message = format!("保存できません: {e}"),
        }
    }
    fn reset_device_snapshot(&mut self) {
        self.initial_loaded = false;
        self.live = None;
        self.raw = None;
        self.baselines = std::array::from_fn(|_| None);
        self.main_pending = [None; 5];
        self.key_pending = [false; 5];
        self.pending_backup = None;
        if let Some((path, _)) = self.backup_confirmation.as_mut() { *path = None; }
    }

    fn select_device(&mut self, index: usize) {
        if self.selected_device != index {
            self.selected_device = index;
            self.reset_device_snapshot();
        }
    }

    fn load_hardware(&mut self) {
        if self.identification.is_some() || self.recovery_failed {
            return;
        }
        let Some(d) = self
            .devices
            .get(self.selected_device)
            .filter(|d| d.is_config())
            .cloned()
        else {
            self.message = "M908の設定用HIDを選択してください。".into();
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.job = Some(rx);
        self.message = "マウスの通常設定を読み取り中…".into();
        std::thread::spawn(move || {
            let _ = tx.send(Reply::Read(device::read_controls(&d)));
        });
    }
    fn send_apply(&mut self, switch: bool) {
        self.send_apply_kind(switch, false, self.profile);
    }
    fn queue_key_apply(&mut self) {
        if !self.preview() && !self.backup_view && self.live.is_some() && !self.recovery_failed {
            self.key_pending[self.profile] = true;
            if self
                .identification
                .as_ref()
                .is_some_and(|client| !client.stopping)
                && self.identifying
            {
                let result = self.baselines[self.profile]
                    .as_ref()
                    .ok_or("編集前の設定がありません".to_string())
                    .and_then(|base| {
                        identification::stage_keys(
                            &self.draft.with_file_name(identification::JOURNAL),
                            base,
                            self.profile,
                            &self.profiles[self.profile].buttons,
                        )
                    });
                self.message = match result {
                    Ok(()) => "復元先を変更後の割当に更新しました。識別を続け、画面やウィンドウを離れると直接反映します。".into(),
                    Err(e) => format!("復元先を更新できません: {e}。編集は保持し、復元完了後に反映します。"),
                };
            }
        }
    }
    fn send_apply_kind(&mut self, switch: bool, automatic: bool, profile: usize) {
        if self.identification.is_some() {
            self.stop_identification();
            self.message = "識別モードを終了して元の設定へ戻しています。復元完了後に書き込みを押してください。".into();
            return;
        }
        if self.recovery_failed {
            self.message = "先に再読み込みで識別設定を復元してください。".into();
            return;
        }
        let (Some(d), Some(base)) = (
            self.devices
                .get(self.selected_device)
                .filter(|d| d.is_config())
                .cloned(),
            if switch {
                self.live.clone()
            } else {
                self.baselines[profile].clone()
            },
        ) else {
            self.message = "先に実機を再読み込みしてください。".into();
            return;
        };
        let mut target = self.profiles[profile].clone();
        if automatic {
            let Ok(actual) = base.profiles() else {
                return;
            };
            // メイン画面の自動反映に、未反映のキー割当を巻き込まない。
            target.buttons = actual[profile].buttons.clone();
        }
        if !switch {
            if let Err(e) = m908ctl::settings::plan(&base, profile, &target) {
                self.message = e;
                return;
            }
        }
        let directory = self.draft.with_file_name("backups");
        let (tx, rx) = mpsc::channel();
        self.job = Some(rx);
        self.main_pending[profile] = None;
        if !automatic {
            self.key_pending[profile] = false;
        }
        self.message = if switch {
            "プロファイルを切替中…（バックアップ・読み戻し確認）"
        } else {
            "書き込み中…（通常設定の競合確認・バックアップ・読み戻し確認）"
        }
        .into();
        if !automatic {
            self.editor = None;
        }
        std::thread::spawn(move || {
            let result = if switch {
                device::switch_controls(&d, &base, profile, &directory)
            } else {
                device::apply_controls(&d, &base, profile, &target, &directory)
            };
            let _ = tx.send(Reply::Applied(
                if switch { None } else { Some(profile) },
                automatic,
                result,
            ));
        });
    }
    fn save_backup(&mut self, path: PathBuf) {
        if self.identification.is_some() || self.recovery_failed {
            return;
        }
        if self.key_pending.iter().any(|v| *v) || self.main_pending.iter().any(Option::is_some) {
            self.pending_backup = Some(path);
            self.message = "変更を自動反映してから、最新の実機設定を保存します…".into();
            return;
        }
        let Some(d) = self
            .devices
            .get(self.selected_device)
            .filter(|d| d.is_config())
            .cloned()
        else {
            self.message = "対応するマウスが見つかりません。バックアップは保存されていません。".into();
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.job = Some(rx);
        self.message = "マクロを含む完全バックアップを読み取り中…".into();
        let settings = self.draft.parent().map(std::path::Path::to_path_buf);
        std::thread::spawn(move || {
            let result = device::read(&d).or_else(|_| device::read_controls(&d)).and_then(|snapshot| {
                Mouse::M908.save(&path, &snapshot.to_text(), settings.as_deref())?;
                Ok((snapshot, path))
            });
            let _ = tx.send(Reply::Backup(result));
        });
    }
    fn collect(&mut self) {
        let reply = self.job.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(v) => Some(Ok(v)),
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(())),
            Err(_) => None,
        });
        if let Some(reply) = reply {
            self.job = None;
            match reply {
                Ok(Reply::Devices(Ok(devices))) => {
                    self.identification_retry_blocked = false;
                    self.recovery_failed = false;
                    let count = devices.iter().filter(|d| d.is_config()).count();
                    let previous = self.devices.get(self.selected_device)
                        .and_then(|old| devices.iter().position(|d| d.path == old.path && d.is_config()));
                    if previous.is_none() {
                        self.reset_device_snapshot();
                    }
                    self.selected_device = previous.or_else(|| devices.iter().position(Device::is_config)).unwrap_or(0);
                    self.devices = devices;
                    if count == 1 || previous.is_some() {
                        self.load_hardware();
                    } else {
                        self.live = None;
                        self.message = if count == 0 {
                            "M908未検出。USBアクセス権と接続を確認してください。PC下書きは編集できます。".into()
                        } else {
                            "M908が複数見つかりました。デバイス情報で選択してください。".into()
                        };
                    }
                }
                Ok(Reply::Read(Ok(snapshot))) => match snapshot.profiles() {
                    Ok(mut profiles) => {
                        retain_shortcut_names(&mut profiles, &self.profiles);
                        self.identification_retry_blocked = false;
                        if self.live.is_none() {
                            self.profile = snapshot.active();
                        }
                        if !self.initial_loaded {
                            self.initial = profiles.clone();
                            self.initial_loaded = true;
                        }
                        self.saved = profiles.clone();
                        self.main_pending = [None; 5];
                        self.key_pending = [false; 5];
                        self.pending_backup = None;
                        self.profiles = profiles;
                        self.backup_view = false;
                        self.editor = None;
                        self.message="実機の通常設定を読み込みました。キー割当は識別中の変更をまとめ、画面やウィンドウを離れると反映します。".into();
                        self.raw = Some(snapshot.to_text());
                        self.baselines = std::array::from_fn(|_| Some(snapshot.clone()));
                        if self.options.capture_device {
                            eprintln!(
                                "実機読込完了: プロファイル{} / {} Hz",
                                snapshot.active() + 1,
                                self.profiles[snapshot.active()].poll
                            );
                        }
                        self.live = Some(snapshot);
                    }
                    Err(e) => {
                        self.live = None;
                        self.message = e;
                    }
                },
                Ok(reply @ (Reply::Applied(_, _, Ok(_)) | Reply::Restored(Ok(_)))) => {
                    let (scope, automatic, restored, applied) = match reply {
                        Reply::Applied(scope, automatic, Ok(applied)) => (scope, automatic, false, applied),
                        Reply::Restored(Ok(applied)) => (None, false, true, applied),
                        _ => unreachable!(),
                    };
                    if restored {
                        self.backup_view = false;
                    }
                    if let Ok(mut actual) = applied.snapshot.profiles() {
                        retain_shortcut_names(&mut actual, &self.profiles);
                        let edited = std::array::from_fn(|p| {
                            self.baselines[p].as_ref().is_some_and(|s| {
                                !restored && m908ctl::settings::plan(s, p, &self.profiles[p])
                                    .map_or(true, |plan| !plan.packets.is_empty())
                            })
                        });
                        let refresh = merge_profiles(
                            &mut self.profiles,
                            &mut self.saved,
                            &actual,
                            scope,
                            edited,
                            automatic,
                        );
                        for p in 0..5 {
                            if refresh[p] {
                                self.baselines[p] = Some(applied.snapshot.clone());
                            }
                        }
                    }
                    self.message = if applied.changes.is_empty() {
                        "変更はありません。実機の内容と一致しています。".into()
                    } else {
                        format!("反映しました・読み戻し一致: {}", applied.changes.join("、"))
                    };
                    if let Some(path) = applied.backup {
                        self.message
                            .push_str(&format!(" / バックアップ: {}", path.display()));
                    }
                    self.raw = Some(applied.snapshot.to_text());
                    self.live = Some(applied.snapshot);
                }
                Ok(Reply::Backup(Ok((snapshot, path)))) => {
                    self.last_backup_path = Some(path.clone());
                    self.message = format!(
                        "実機の{}を保存しました: {}",
                        if snapshot.has_macros {
                            "完全バックアップ（マクロを含む）"
                        } else {
                            "通常設定（マクロを含まない）"
                        },
                        path.display()
                    );
                    self.raw = Some(snapshot.to_text());
                }
                Ok(Reply::Read(Err(e))) | Ok(Reply::Applied(_, _, Err(e))) | Ok(Reply::Restored(Err(e))) => {
                    self.main_pending = [None; 5];
                    self.key_pending = [false; 5];
                    self.pending_backup = None;
                    self.live = None;
                    self.message = e;
                }
                Ok(Reply::Devices(Err(e))) => {
                    self.recovery_failed =
                        self.draft.with_file_name(identification::JOURNAL).exists();
                    self.live = None;
                    self.message = e;
                }
                Ok(Reply::Backup(Err(e))) => self.message = e,
                Err(()) => {
                    self.live = None;
                    self.message = "通信処理が終了しました。再読み込みしてください。".into();
                }
            }
        }
    }
    fn refresh_theme(&mut self, ctx: &egui::Context) {
        if self.theme_check.elapsed() >= Duration::from_millis(750) {
            let mut s = if self.preview() {
                SystemTheme {
                    dark: false,
                    accent: theme::rgb(0x0078D4),
                }
            } else {
                theme::system()
            };
            if self.options.theme > 0 {
                s.dark = self.options.theme == 2;
            }
            if s != self.last_theme || self.frame == 1 {
                self.pal = Palette::new(s);
                self.pal.apply(ctx, s.dark);
                self.last_theme = s;
            }
            self.theme_check = Instant::now();
        }
    }
    fn status(&mut self, ui: &mut egui::Ui) {
        let p = self.pal;
        ui.horizontal_wrapped(|ui| {
            ui.label(bold(if self.preview() {
                "● プレビュー"
            } else if self.live.is_some() {
                "● 接続済み"
            } else if self.devices.is_empty() {
                "● 未接続"
            } else {
                "● 機器を検出"
            }));
            if ui
                .add_enabled(
                    self.job.is_none() && self.identification.is_none() && !self.preview(),
                    egui::Button::new("再読み込み").small(),
                )
                .clicked()
            {
                if self.dirty() || self.unapplied() {
                    self.reload_confirm = Some(ReloadTarget::Scan);
                } else {
                    self.scan();
                }
                match pointer::read() {
                    Ok(v) => self.pointer = Some(v),
                    Err(e) => self.pointer_message = e,
                }
            }
            if let Some(d) = self.devices.get(self.selected_device).filter(|d| d.is_config()) {
                ui.label(note(d.name.as_str(), p));
            }
            ui.label(note("VID/PID: 04D9/FC4D", p));
            ui.label(note(
                if self.backup_view {
                    "表示元: ファイル／編集データ"
                } else if self.live.is_some() {
                    "表示元: 実機"
                } else {
                    "表示元: PC上の下書き"
                },
                p,
            ));
            if let Some(s) = &self.live {
                ui.label(format!(
                    "使用中: プロファイル{} / DPIスロット{}",
                    s.active() + 1,
                    s.parts[1][s.active()][8] + 1
                ));
            }
            if ui.small_button("デバイス情報").clicked() {
                self.show_device = true;
            }
            if ui.small_button("表示設定").clicked() {
                self.show_settings = true;
            }
            if self.job.is_some() {
                ui.spinner();
                ui.label(note("通信中…", p)).on_hover_text(&self.message);
            }
        });
    }
    fn profile_label(&self, i: usize) -> String {
        format!(
            "プロファイル{}{}",
            i + 1,
            if self.live.as_ref().is_some_and(|s| s.active() == i) {
                "・使用中"
            } else {
                ""
            }
        )
    }
    fn tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for i in 0..5 {
                if ui
                    .add(toggle(self.pal, self.profile_label(i), self.profile == i))
                    .clicked()
                {
                    self.profile = i;
                    self.editor = None;
                }
            }
        });
    }
    fn pointer_panel(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        pal.card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            heading(
                ui,
                pal,
                "カーソル・ホイール",
                "Windows の設定（このPCのすべてのマウス共通）",
            );
            let Some(mut p) = self.pointer else {
                ui.label("Windowsの設定を読み取れません。");
                return;
            };
            let before = p;
            ui.add_enabled_ui(true, |ui| {
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        Vec2::new(theme::s(120.0), 20.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(theme::s(120.0));
                            ui.label("カーソルの速さ");
                        },
                    );
                    slider_field(ui, pal, &mut p.speed, 1..=20, 80.0);
                    check(ui, pal, &mut p.acceleration, "加速");
                });
                let column_count = if ui.available_width() < 720.0 { 1 } else { 2 };
                ui.columns(column_count, |columns| {
                    columns[0].horizontal(|ui| {
                        ui.label("スクロール行数");
                        ui.add_enabled_ui(!p.wheel_page, |ui| {
                            slider_field(ui, pal, &mut p.wheel_lines, 1..=100, 88.0);
                        });
                        check(ui, pal, &mut p.wheel_page, "1画面ずつ");
                    });
                    let second = columns.len() - 1;
                    columns[second].horizontal(|ui| {
                        ui.label("横スクロール（チルト）")
                            .on_hover_text("Windowsの全マウス共通の設定です。");
                        slider_field(ui, pal, &mut p.wheel_chars, 1..=100, 24.0);
                        ui.label(note("文字", pal));
                    });
                });
            });
            if p != before && !self.preview() {
                let dragging = ui.input(|i| i.pointer.any_down());
                match pointer::write(p, !dragging) {
                    Ok(()) => {
                        self.pointer = Some(p);
                        self.pointer_pending = dragging;
                        self.pointer_message.clear();
                    }
                    Err(e) => self.pointer_message = e,
                }
            }
            // ドラッグ中の変更も、離したフレームで永続化する。
            if !self.preview() && self.pointer_pending && !ui.input(|i| i.pointer.any_down()) {
                match pointer::write(p, true) {
                    Ok(()) => self.pointer_pending = false,
                    Err(e) => self.pointer_message = e,
                }
            }
            if !self.pointer_message.is_empty() {
                ui.label(note(&self.pointer_message, pal));
            }
        });
    }
    fn main_screen(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        self.pointer_panel(ui);
        self.tabs(ui);
        let editing_profile = self.profile;
        let before = self.profiles[editing_profile].clone();
        pal.card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            heading(ui, pal, "LED・ポーリング", "操作後に自動反映（接続中）");
            let p = &mut self.profiles[self.profile];
            ui.horizontal_wrapped(|ui| {
                ui.label("LED");
                ui.color_edit_button_srgb(&mut p.color);
                egui::ComboBox::from_id_salt("effect")
                    .width(95.0)
                    .selected_text(EFFECTS[p.effect])
                    .show_ui(ui, |ui| {
                        for (i, e) in EFFECTS.iter().enumerate() {
                            if i < 11 || p.effect == 11 {
                                ui.selectable_value(&mut p.effect, i, *e);
                            }
                        }
                    });
                ui.label("明るさ");
                ui.add(egui::DragValue::new(&mut p.brightness).range(1..=3));
                ui.label("速度");
                ui.add(egui::DragValue::new(&mut p.speed).range(1..=8));
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("ポーリング");
                egui::ComboBox::from_id_salt("poll")
                    .width(90.0)
                    .selected_text(format!("{} Hz", p.poll))
                    .show_ui(ui, |ui| {
                        for rate in [125, 250, 500, 1000] {
                            ui.selectable_value(&mut p.poll, rate, format!("{rate} Hz"));
                        }
                    });
            });
        });
        pal.card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            heading(ui, pal, "DPI", "5段階 / 200〜12400 DPI");
            let cols = if ui.available_width() >= 500.0 { 2 } else { 1 };
            let cell = (ui.available_width() - 8.0 * (cols - 1) as f32) / cols as f32;
            let grid_slots: usize = 5;
            for row in 0..grid_slots.div_ceil(cols) {
                ui.horizontal_top(|ui| {
                    for col in 0..cols {
                        let i = row * cols + col;
                        if i >= grid_slots {
                            break;
                        }
                        ui.allocate_ui_with_layout(
                            Vec2::new(cell, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(cell);
                                egui::Frame::new()
                                    .fill(pal.alt)
                                    .stroke(Stroke::new(1.0_f32, pal.border.gamma_multiply(0.4)))
                                    .corner_radius(6)
                                    .inner_margin(8)
                                    .show(ui, |ui| {
                                        ui.set_width((cell - 18.0).max(160.0));
                                        let p = &mut self.profiles[self.profile];
                                        ui.horizontal(|ui| {
                                            ui.label(bold(format!("DPI {}", i + 1)));
                                            let last = p.enabled[i]
                                                && p.enabled.iter().filter(|v| **v).count() == 1;
                                            ui.add_enabled_ui(!last, |ui| {
                                                check(ui, pal, &mut p.enabled[i], "有効")
                                            })
                                            .inner
                                            .on_disabled_hover_text(
                                                "最低1段階は有効にしてください",
                                            );
                                        });
                                        ui.add_enabled_ui(p.enabled[i], |ui| {
                                            ui.horizontal(|ui| {
                                                if slider_field(
                                                    ui,
                                                    pal,
                                                    &mut p.dpi[i],
                                                    200..=12400,
                                                    0.0,
                                                ) {
                                                    p.dpi[i] = model::normalize_dpi(p.dpi[i]);
                                                }
                                            });
                                        });
                                    });
                            },
                        );
                    }
                });
            }
            if ui
                .add(
                    pal.primary("キーコンフィグ（ボタンの割り当て）を開く ▶")
                        .min_size(Vec2::new(0.0, 28.0)),
                )
                .clicked()
            {
                self.page = 1;
            }
        });
        if self.profiles[editing_profile] != before
            && !self.preview()
            && self.live.is_some()
            && !self.backup_view
        {
            self.main_pending[editing_profile] = Some(Instant::now());
        }
        self.backup_panel(ui);
    }
    fn backup_picker_directory(&self) -> Option<&std::path::Path> {
        self.last_backup_path.as_deref().and_then(std::path::Path::parent)
            .filter(|path| path.is_absolute() && path.is_dir())
            .or_else(|| self.draft.parent())
    }

    fn choose_backup_destination(&mut self) {
        let filter = format!("{} (*.mmo-backup)", tr("マウスのバックアップ"));
        let path = match file_dialog::choose(
            &[(filter.as_str(), "*.mmo-backup")],
            Some(("MMOMouseControl-M908-Backup.mmo-backup", "mmo-backup")),
            self.backup_picker_directory(),
        ) {
            Ok(Some(path)) => path,
            Ok(None) => return,
            Err(error) => { self.message = tr(format!("保存先を選べません: {error}")); return; }
        };
        if !path.extension().and_then(|value| value.to_str()).is_some_and(|value| value.eq_ignore_ascii_case("mmo-backup")) {
            self.message = tr("拡張子 .mmo-backup のファイルを指定してください。");
            return;
        }
        if self.live.is_some() {
            self.save_backup(path);
        } else {
            let result = model::encode(&self.profiles).and_then(|data| {
                Mouse::M908.save(&path, &data, self.draft.parent())
            });
            match result {
                Ok(()) => {
                    self.last_backup_path = Some(path.clone());
                    self.message = tr(format!("バックアップを保存しました: {}", path.display()));
                }
                Err(error) => self.message = tr(format!("保存できません: {error}")),
            }
        }
    }

    fn choose_backup_to_open(&mut self) {
        let backup = format!("{} (*.mmo-backup)", tr("マウスのバックアップ"));
        let all = tr("すべてのファイル");
        let filters = [
            (backup.as_str(), "*.mmo-backup;*.mmo-draft;*.m908;*.draft;*.mmo-settings"),
            (all.as_str(), "*.*"),
        ];
        let path = match file_dialog::choose(&filters, None, self.backup_picker_directory()) {
            Ok(Some(path)) => path,
            Ok(None) => return,
            Err(error) => { self.message = tr(format!("ファイルを選べません: {error}")); return; }
        };
        let result = Mouse::M908.load(&path).and_then(|archive| {
            let profiles = archive.payload.as_deref().map(|data| {
                if data.starts_with("M908-BACKUP-1") || data.starts_with("M908-SETTINGS-1") {
                    Snapshot::from_text(data).and_then(|snapshot| snapshot.profiles())
                } else {
                    model::decode(data)
                }
            }).transpose()?;
            Ok((profiles, archive.stage_settings()?))
        });
        match result {
            Ok((profiles, staged)) => {
                self.last_backup_path = Some(path.clone());
                if let Some(profiles) = profiles {
                    self.profiles = profiles;
                    self.main_pending = [None; 5];
                    self.key_pending = [false; 5];
                    self.pending_backup = None;
                    self.backup_view = true;
                    self.editor = None;
                    let device_path = self.backup_write_device().map(|device| device.path.clone());
                    self.backup_confirmation = Some((device_path, staged));
                }
                self.message = tr(if staged { "アプリ設定は次回起動時に復元されます。" }
                    else { "バックアップを読み込みました。" });
            }
            Err(error) => self.message = tr(format!("ファイルを開けません: {error}")),
        }
    }

    fn backup_write_device(&self) -> Option<&Device> {
        if self.live.is_none() || self.job.is_some() || self.identification.is_some()
            || self.recovery_failed || self.baselines.iter().any(Option::is_none)
        {
            return None;
        }
        self.devices.get(self.selected_device).filter(|device| device.is_config())
    }

    fn confirm_backup_write(&mut self, ctx: &egui::Context) {
        let Some((path, staged)) = &self.backup_confirmation else { return; };
        let device = self.backup_write_device().filter(|device| path.as_ref() == Some(&device.path));
        let choice = file_dialog::confirm_write(
            ctx, "m908-backup-write", device.is_some(), *staged,
            "5つのプロファイルの通常設定をマウスに書き込みます。",
        );
        let Some(write) = choice else { return; };
        let device = device.cloned();
        self.backup_confirmation = None;
        if let Some(device) = device.filter(|_| write) {
            let baselines = self.baselines.clone();
            let profiles = self.profiles.clone();
            let directory = self.draft.with_file_name("backups");
            let (tx, rx) = mpsc::channel();
            self.job = Some(rx);
            self.message = "書き込み中…（通常設定の競合確認・バックアップ・読み戻し確認）".into();
            let ctx_request_repaint = ctx.clone();
            std::thread::spawn(move || {
                let result = device::restore_controls(&device, &baselines, &profiles, &directory);
                let _ = tx.send(Reply::Restored(result));
                ctx_request_repaint.request_repaint();
            });
        }
    }

    fn backup_panel(&mut self, ui: &mut egui::Ui) {
        let pal = self.pal;
        let ready = self.live.is_some()
            && self.devices.get(self.selected_device).is_some_and(Device::is_config)
            && !self.preview();
        pal.card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            heading(ui, pal, &tr("バックアップ"), "");
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.preview() && !self.recovery_failed, egui::Button::new(tr("保存先を選択"))).clicked() {
                    self.choose_backup_destination();
                }
                if ui.add_enabled(!self.preview(), egui::Button::new(tr("設定を読み込む"))).clicked() {
                    self.choose_backup_to_open();
                }
                file_dialog::show_backup_path(ui, self.last_backup_path.as_deref());
            });
        });
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(ready, egui::Button::new("このプロファイルを使用")).clicked() { self.send_apply(true); }
            if ui.add_enabled(!self.preview(), egui::Button::new("起動時の編集状態に戻す"))
                .on_hover_text("画面を起動後最初に読み取った内容へ戻します。本体には自動では書き込みません。")
                .clicked() { self.confirm_reset = true; }
            if ui.add_enabled(ready, pal.primary("マウスへ書き込む"))
                .on_hover_text("表示中のプロファイルだけをバックアップ後に反映し、読み戻して確認します。")
                .clicked() { self.send_apply(false); }
            if self.unapplied() { ui.label(note("● 未反映の編集あり", pal)); }
        });
    }

    fn open_editor(&mut self, slot: usize) {
        let original = self.profiles[self.profile].buttons[slot].clone();
        let (modifiers, key) = editor::key_parts(&original);
        self.editor = Some(Editor {
            slot,
            action: original.clone(),
            original,
            tab: None,
            capture: false,
            name: String::new(),
            notice: String::new(),
            list_page: 0,
            custom_page: 0,
            candidate_page: 0,
            search: String::new(),
            modifiers,
            key,
            last_auto_name: String::new(),
            tab_height: 0.0,
            content_height: 0.0,
        });
    }
    fn key_group<const C: usize>(
        &mut self,
        ui: &mut egui::Ui,
        title: &str,
        description: &str,
        rows: &[[usize; C]],
    ) {
        let pal = self.pal;
        let mut clicked = None;
        pal.card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            heading(ui, pal, title, description);
            let cols = if ui.available_width() < 560.0 { 1 } else { C };
            let gap = theme::s(12.0);
            let cell =
                ((ui.available_width() - (cols - 1) as f32 * gap) / cols as f32).floor() - 1.0;
            let name_width = theme::s(140.0).min(cell * 0.55);
            let action_width = (cell - name_width - 8.0).max(40.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for _ in 0..cols {
                    ui.allocate_ui_with_layout(
                        Vec2::new(cell, theme::s(18.0)),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(cell);
                            ui.spacing_mut().item_spacing.x = 4.0;
                            for (width, text) in
                                [(name_width, "番号・名称"), (action_width, "通常")]
                            {
                                ui.allocate_ui_with_layout(
                                    Vec2::new(width, 16.0),
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.set_min_width(width);
                                        ui.add_space(6.0);
                                        ui.label(note(text, pal));
                                    },
                                );
                            }
                        },
                    );
                }
            });
            let slots: Vec<usize> = rows.iter().flatten().copied().collect();
            for row in slots.chunks(cols) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for &slot in row {
                        let row = ui.allocate_ui_with_layout(
                            Vec2::new(cell, theme::s(30.0)),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.set_min_width(cell);
                                ui.spacing_mut().item_spacing.x = 4.0;
                                let lit = self.identifying
                                    && (self.pressed[slot]
                                        || self.flashed[slot].is_some_and(|t| {
                                            t.elapsed() < Duration::from_millis(60)
                                        }));
                                let name = if (6..18).contains(&slot) {
                                    format!("{} サイド {}", slot + 1, slot - 5)
                                } else {
                                    format!("{} {}", slot + 1, BUTTONS[slot])
                                };
                                name_cell(ui, name_width, theme::s(30.0), &name, lit, pal);
                                let action = m908ctl::actions::display(
                                    &self.profiles[self.profile].buttons[slot],
                                );
                                if ui
                                    .add_sized(
                                        [action_width, theme::s(26.0)],
                                        egui::Button::new(&action).truncate(),
                                    )
                                    .on_hover_text(&action)
                                    .clicked()
                                {
                                    clicked = Some(slot);
                                }
                            },
                        );
                        let rect = row.response.rect;
                        ui.painter().hline(
                            rect.x_range(),
                            rect.bottom() + 2.0,
                            Stroke::new(1.0_f32, pal.border.gamma_multiply(0.4)),
                        );
                    }
                });
            }
        });
        if let Some(slot) = clicked {
            self.open_editor(slot);
        }
    }
    fn keys_screen(&mut self, ui: &mut egui::Ui) {
        let narrow = ui.available_width() < 760.0;
        ui.horizontal(|ui| {
            if ui.button("◀ 戻る").clicked() {
                self.page = 0;
                self.editor = None;
            }
            ui.label(bold("キーコンフィグ").size(14.0));
            if !narrow {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    for i in (0..5).rev() {
                        if ui
                            .add(toggle(self.pal, self.profile_label(i), self.profile == i))
                            .clicked()
                        {
                            self.profile = i;
                            self.editor = None;
                        }
                    }
                });
            }
        });
        if narrow {
            self.tabs(ui);
        }
        self.pal.card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label("押したボタン:");
                ui.label(self.physical_slot.map(|s| BUTTONS[s]).unwrap_or("—"));
                ui.add_space(theme::s(16.0));
                if let Some(client) = &self.identification {
                    ui.label(note(
                        if client.stopping {
                            "通常設定へ直接復元中…"
                        } else if self.identifying {
                            "押したボタンで選択：自動識別中"
                        } else {
                            "自動識別を準備中…"
                        },
                        self.pal,
                    ));
                    if !self.identifying {
                        ui.spinner();
                    }
                } else if self.preview() {
                    ui.label(note("押したボタンで選択：自動", self.pal));
                } else if self.live.is_none() || self.recovery_failed {
                    ui.label(note("接続・復元待ち", self.pal));
                } else if self.identification_retry_blocked {
                    if ui.button("識別を再試行").clicked() {
                        self.identification_retry_blocked = false;
                    }
                } else {
                    ui.label(note("フォーカスが戻ると自動識別", self.pal));
                }
            });
            ui.label(note(
                "キーコンフィグで自動識別します。別ウィンドウへ移ると変更後の設定へ直接戻り、戻ると識別を再開します。",
                self.pal,
            ));
        });
        self.key_group(
            ui,
            "上面のボタン",
            "上面6ボタン・ホイール上下 / 2列×4段",
            &TOP,
        );
        self.key_group(
            ui,
            "サイドボタン（親指側）",
            "12ボタン / 番号順の一覧・3列×4段",
            &SIDE,
        );
        ui.label(note(
            if self.backup_view {
                "バックアップを表示中です。反映するには「マウスへ書き込む」を押してください。"
            } else {
                "「変更」で編集を保存します。識別中は本体を切り替えず、画面やウィンドウを離れるとまとめて反映します。"
            },
            self.pal,
        ));
    }
    fn device_window(&mut self, ctx: &egui::Context) {
        if !self.show_device || self.identification.is_some() {
            return;
        }
        let mut open = true;
        egui::Window::new("デバイス情報・読み取り").open(&mut open).default_width(580.0).show(ctx,|ui|{
            ui.label(format!("{}  VID 04D9 / PID FC4D", device::DISPLAY_NAME));
            let mut selected = self.selected_device;
            let can_select = !self.communication_pending();
            for (i, d) in self.devices.iter().enumerate() {
                if ui.add_enabled(can_select && d.is_config(), egui::RadioButton::new(selected == i, d.label())).clicked() {
                    selected = i;
                }
            }
            self.select_device(selected);
            if self.devices.is_empty(){ui.label("対象の機器が見つかりません。接続とUSBアクセス権を確認してください。");}
            ui.separator();ui.label("設定用: IF 2 / Usage FFA0:0001。読み取り・設定書き込みはWindows実機で検証済みです。");
            let can=self.job.is_none()&&self.devices.get(self.selected_device).is_some_and(Device::is_config)&&!self.preview();
            if ui.add_enabled(can && can_select, egui::Button::new("選択した機器の設定を読み取る")).clicked() {
                if self.dirty() || self.unapplied() {
                    self.reload_confirm = Some(ReloadTarget::SelectedDevice);
                } else {
                    self.load_hardware();
                }
            }
            if let Some(raw)=&self.raw{ui.label(format!("取得済み: {} bytes（{}）",raw.len(),if raw.starts_with("M908-BACKUP-1"){"マクロを含む"}else{"通常設定のみ"}));if ui.button("読み取り済みデータを保存").clicked(){let stamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();let dir=self.draft.with_file_name("backups");self.message=std::fs::create_dir_all(&dir).map_err(|e|e.to_string()).and_then(|_|{let path=dir.join(format!("read-{stamp}.m908"));device::save_raw(&path,raw).map(|_|format!("保存しました: {}",path.display()))}).unwrap_or_else(|e|e);}}
            ui.label(note(&self.message,self.pal));
        });
        self.show_device = open;
    }
    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let mut open = true;
        egui::Window::new("表示設定")
            .open(&mut open)
            .default_width(420.0)
            .show(ctx, |ui| {
                ui.label("配色");
                ui.horizontal_wrapped(|ui| {
                    for (i, title) in ["Windowsに合わせる", "ライト", "ダーク"].iter().enumerate()
                    {
                        if ui
                            .selectable_value(&mut self.options.theme, i, *title)
                            .changed()
                        {
                            self.theme_check = Instant::now() - Duration::from_secs(2);
                        }
                    }
                });
                ui.label("DPI: Windowsの表示倍率にモニターごとに追従します。");
                ui.label("画面構成・文字・余白はG600r制御ソフトと共通です。");
            });
        self.show_settings = open;
    }
}
impl App {
    pub fn activate(&mut self, ctx: &egui::Context) {
        self.pal.apply(ctx, self.last_theme.dark);
        if !self.preview() {
            self.pointer = pointer::read().ok();
        }
    }

    pub fn suspend(&mut self) {
        if self.identification.is_some() {
            self.stop_identification();
        }
    }

    pub fn close_in_progress(&self) -> bool {
        self.close_confirm || self.close_after_restore || self.exit_allowed
    }

    pub fn needs_close_handling(&self) -> bool {
        self.dirty() || self.communication_pending() || self.close_confirm || self.close_after_restore
    }

    pub fn communication_pending(&self) -> bool {
        self.job.is_some()
            || self.identification.is_some()
            || (self.live.is_some()
                && (self.main_pending.iter().any(Option::is_some)
                    || self.key_pending.iter().any(|v| *v)
                    || self.pending_backup.is_some()))
    }

    pub fn update_connection(
        &mut self,
        paths: &[std::ffi::CString],
        removed: &std::collections::HashSet<std::ffi::CString>,
        same_list: bool,
    ) -> bool {
        let selected_removed = self.devices.get(self.selected_device)
            .filter(|device| device.is_config())
            .is_some_and(|device| removed.contains(&device.path) || !paths.contains(&device.path));
        if same_list && !selected_removed {
            return true;
        }
        if self.identification.is_some() {
            self.suspend();
            return false;
        }
        if self.job.is_some() {
            return false;
        }
        if selected_removed || !self.devices.get(self.selected_device).is_some_and(|d| paths.contains(&d.path)) {
            self.devices.clear();
            self.selected_device = 0;
            self.reset_device_snapshot();
            self.reload_confirm = None;
        }
        if paths.is_empty() {
            self.message = format!("{} が接続されていません。", device::DISPLAY_NAME);
        } else if self.backup_view || self.dirty() {
            self.reload_confirm = Some(ReloadTarget::Scan);
        } else {
            self.scan();
        }
        true
    }

    pub fn tick(&mut self, ctx: &egui::Context) {
        self.collect();
        self.update_identification(ctx);
    }

    pub fn flush_pending(&mut self, ctx: &egui::Context) {
        if self.job.is_none()
            && self.identification.is_none()
            && self.live.is_some()
            && !self.recovery_failed
            && !self.backup_view
            && !self.close_confirm
            && self.reload_confirm.is_none()
            && !self.confirm_reset
            && !ctx.input(|i| i.pointer.any_down())
        {
            if let Some(p) = (0..5).find(|p| self.key_pending[*p]) {
                self.send_apply_kind(false, false, p);
            } else if let Some(p) = (0..5).find(|p| {
                self.main_pending[*p].is_some_and(|t| t.elapsed() >= Duration::from_millis(120))
            }) {
                self.send_apply_kind(false, true, p);
            } else if self.key_pending.iter().all(|v| !*v)
                && self.main_pending.iter().all(Option::is_none)
            {
                if let Some(path) = self.pending_backup.take() {
                    self.save_backup(path);
                }
            }
        }
    }

    pub fn draw(&mut self, ctx: &egui::Context, header_height: f32, handle_close: bool) {
        self.frame += 1;
        self.editor_bottom = 0.0;
        if self.close_after_restore
            && self.identification.is_none()
            && self.job.is_none()
            && (self.live.is_none()
                || (self.pending_backup.is_none()
                    && !self.key_pending.iter().any(|v| *v)
                    && self.main_pending.iter().all(Option::is_none)))
        {
            self.close_after_restore = false;
            if self.dirty() {
                self.close_confirm = true;
            } else {
                self.exit_allowed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        self.refresh_theme(ctx);
        if self.frame == 1 {
            if !self.preview() {
                if let Some(monitor) = ctx.input(|i| i.viewport().monitor_size) {
                    let size = Vec2::new(
                        self.options.size[0].min((monitor.x - 40.0).max(640.0)),
                        self.options.size[1].min((monitor.y - 60.0).max(520.0)),
                    );
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                }
            }
            if let Some(scale) = self.options.scale {
                ctx.set_pixels_per_point(scale);
            }
        }
        if handle_close && ctx.input(|i| i.viewport().close_requested()) && !self.exit_allowed {
            self.reload_confirm = None;
            self.confirm_reset = false;
            self.backup_confirmation = None;
            if self.identification.is_some() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.close_after_restore = true;
                self.stop_identification();
            } else if self.communication_pending() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.close_after_restore = true;
                self.message = "設定の自動反映が終了してから閉じます…".into();
            } else if self.dirty() {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.close_confirm = true;
            }
        }
        let mut content_height = 0.0;
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.pal.bg).inner_margin(8))
            .show(ctx, |ui| {
                let scroll = egui::ScrollArea::vertical()
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = theme::s(10.0);
                        self.status(ui);
                        ui.add_enabled_ui(
                            self.job.is_none()
                                && (self.identification.is_none() || self.identifying),
                            |ui| {
                                if self.page == 0 {
                                    self.main_screen(ui);
                                } else {
                                    self.keys_screen(ui);
                                }
                            },
                        );
                        ui.add(egui::Label::new(note(&self.message, self.pal)).truncate())
                            .on_hover_text(&self.message);
                    });
                content_height = scroll.content_size.y;
            });
        self.device_window(ctx);
        self.settings_window(ctx);
        self.editor_window(ctx);
        if !(self.preview() && self.options.fixed_size) {
            let monitor = ctx
                .input(|i| i.viewport().monitor_size)
                .unwrap_or(Vec2::new(1920.0, 1080.0));
            let width = theme::s(if self.page == 0 { 1010.0 } else { 1060.0 })
                .min((monitor.x - 40.0).max(600.0));
            let height = (content_height + header_height + 20.0)
                .max(self.editor_bottom + 12.0)
                .clamp(300.0, (monitor.y - 60.0).max(300.0));
            let current = ctx.screen_rect().size();
            if (current.x - width).abs() > 2.0 || (current.y - height).abs() > 2.0 {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(width, height)));
            }
        }
        self.flush_pending(ctx);
        if self.identification.is_some() && self.page != 1 {
            self.stop_identification();
        }
        if let Some(target) = self.reload_confirm {
            egui::Window::new("実機を再読み込み")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("画面上の編集を破棄して、マウス本体の設定を読み直します。");
                    ui.horizontal(|ui| {
                        if ui.button("再読み込み").clicked() {
                            self.reload_confirm = None;
                            self.editor = None;
                            match target {
                                ReloadTarget::Scan => self.scan(),
                                ReloadTarget::SelectedDevice => self.load_hardware(),
                            }
                        }
                        if ui.button("キャンセル").clicked() {
                            self.reload_confirm = None;
                        }
                    });
                });
        }
        self.confirm_backup_write(ctx);
        if self.confirm_reset {
            egui::Window::new("起動時の編集状態に戻す")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("画面の5プロファイルを最初に読み取った内容に戻します。本体には自動では書き込みません。反映はプロファイルごとに「マウスへ書き込む」を押してください。");
                    ui.horizontal(|ui| {
                        if ui.button("戻す").clicked() {
                            self.profiles = self.initial.clone();
                            self.main_pending=[None;5];
                            self.key_pending=[false;5];
                            self.pending_backup=None;
                            self.backup_view = true;
                            self.editor = None;
                            self.confirm_reset = false;
                        }
                        if ui.button("キャンセル").clicked() {
                            self.confirm_reset = false;
                        }
                    });
                });
        }
        if self.close_confirm {
            egui::Window::new("未保存の編集があります")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("PCに編集を保存してもマウス本体には反映されません。");
                    if ui.button("PCに保存して終了").clicked() {
                        self.save();
                        if !self.dirty() {
                            self.exit_allowed = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                    if ui.button("保存せず終了").clicked() {
                        self.exit_allowed = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    if ui.button("編集を続ける").clicked() {
                        self.close_confirm = false;
                    }
                });
        }
        self.sync_identification(ctx);
        if self.preview() {
            if self.frame >= 5
                && self.started.elapsed() >= Duration::from_millis(600)
                && !self.capture_sent
                && (!self.options.capture_device || self.job.is_none() && self.live.is_some())
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.capture_sent = true;
            }
            for event in ctx.input(|i| i.events.clone()) {
                if let egui::Event::Screenshot { image, .. } = event {
                    let pixels = image
                        .pixels
                        .iter()
                        .flat_map(|c| c.to_array())
                        .collect::<Vec<_>>();
                    if let Err(e) = image::save_buffer(
                        self.options.capture.as_ref().unwrap(),
                        &pixels,
                        image.size[0] as u32,
                        image.size[1] as u32,
                        image::ColorType::Rgba8,
                    ) {
                        eprintln!("画像保存に失敗: {e}");
                    }
                    self.exit_allowed = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            ctx.request_repaint_after(Duration::from_millis(100));
        } else {
            ctx.request_repaint_after(Duration::from_millis(
                if self.identification.is_some()
                    || self.job.is_some()
                    || self.main_pending.iter().any(Option::is_some)
                    || self.key_pending.iter().any(|v| *v)
                    || self.pending_backup.is_some()
                {
                    30
                } else {
                    250
                },
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auto_identification_waits_for_restore_before_fast_focus_return() {
        use IdentificationAction::*;
        assert_eq!(identification_action(true, None, true), Start);
        assert_eq!(identification_action(true, Some(false), true), Wait);
        assert_eq!(identification_action(false, Some(false), true), Stop);
        // フォーカスが戻っても、旧セッションの復元中は新しい監視を作らない。
        assert_eq!(identification_action(true, Some(true), true), Wait);
        assert_eq!(identification_action(true, None, true), Start);
        assert_eq!(identification_action(false, None, true), Wait);
    }
    #[test]
    fn auto_identification_yields_to_pending_writes_and_errors() {
        use IdentificationAction::*;
        assert_eq!(identification_action(true, None, false), Wait);
        // 編集キューが増えても、起動済みの識別用設定はそのまま維持する。
        assert_eq!(identification_action(true, Some(false), false), Wait);
        assert_eq!(identification_action(false, Some(false), false), Stop);
        assert_eq!(identification_action(false, Some(true), false), Wait);
        assert_eq!(identification_action(true, None, true), Start);
    }
    #[test]
    fn shortcut_names_survive_apply_and_reload_only_when_device_bytes_match() {
        let mut local = model::defaults();
        local[0].buttons[6] = "Chrome: 新しいウィンドウ".into();
        local[0].buttons[7] = "Windows: 新規作成".into();
        local[0].buttons[8] = "Chrome: タブを閉じる".into();
        let mut actual = model::defaults();
        actual[0].buttons[6] = "Ctrl + N".into();
        actual[0].buttons[7] = "Ctrl + N".into();
        actual[0].buttons[8] = "Ctrl + T".into();
        retain_shortcut_names(&mut actual, &local);
        let mut saved = local.clone();
        merge_profiles(&mut local, &mut saved, &actual, Some(0), [false; 5], false);
        assert_eq!(local[0].buttons[6], "Chrome: 新しいウィンドウ");
        assert_eq!(local[0].buttons[7], "Windows: 新規作成");
        assert_eq!(local[0].buttons[8], "Ctrl + T");
        assert_eq!(local, saved);
    }
    #[test]
    fn pressed_name_does_not_move_normal_assignment() {
        let ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        fonts.families.insert(
            egui::FontFamily::Name("bold".into()),
            fonts.families[&egui::FontFamily::Proportional].clone(),
        );
        ctx.set_fonts(fonts);
        let pal = Palette::new(SystemTheme {
            dark: false,
            accent: theme::rgb(0x0078D4),
        });
        let mut rectangles = Vec::new();
        for lit in [false, true, false] {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(800.0, 600.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        let name = name_cell(ui, 120.0, 26.0, "7 サイド 1", lit, pal);
                        let action = ui.add_sized([180.0, 24.0], egui::Button::new("通常"));
                        rectangles.push((name.rect, action.rect));
                    });
                });
            });
        }
        assert_eq!(rectangles[0], rectangles[1]);
        assert_eq!(rectangles[0], rectangles[2]);
    }
    #[test]
    fn invalidates_only_the_selected_m908_when_an_observed_path_disappears() {
        let ctx = egui::Context::default();
        let mut options = Options::parse();
        options.capture = Some(PathBuf::from("test-preview.png"));
        options.capture_device = false;
        let mut app = App::new(&ctx, options);
        let paths = ["A", "B"].map(|id| std::ffi::CString::new(id).unwrap());
        app.devices = paths.iter().map(|path| Device {
            path: path.clone(),
            name: "M908".into(),
            interface: 2,
            usage_page: 0xffa0,
            usage: 1,
        }).collect();
        app.initial_loaded = true;
        app.raw = Some("元の設定".into());
        app.profiles[0].poll = if app.saved[0].poll == 1000 { 250 } else { 1000 };

        assert!(app.update_connection(&paths, &[paths[1].clone()].into_iter().collect(), true));
        assert!(app.initial_loaded);
        assert!(app.raw.is_some());
        assert_eq!(app.devices.len(), 2);
        assert!(app.update_connection(&paths[..1], &[paths[0].clone()].into_iter().collect(), false));
        assert!(!app.initial_loaded);
        assert!(app.raw.is_none());
        assert!(app.devices.is_empty());
        assert!(matches!(app.reload_confirm, Some(ReloadTarget::Scan)));
    }

    #[test]
    fn m908_key_groups_keep_every_row_when_buttons_light_up() {
        let ctx = egui::Context::default();
        let mut fonts = egui::FontDefinitions::default();
        fonts.families.insert(
            egui::FontFamily::Name("bold".into()),
            fonts.families[&egui::FontFamily::Proportional].clone(),
        );
        ctx.set_fonts(fonts);
        theme::install_fonts(&ctx);
        let mut options = Options::parse();
        options.capture = Some(PathBuf::from("test-preview.png"));
        options.capture_device = false;
        let mut app = App::new(&ctx, options);
        app.page = 1;
        app.identifying = true;

        for width in [600.0, 800.0, 1060.0] {
            let mut render = |slot: Option<usize>| {
                app.physical_slot = slot;
                app.pressed.fill(false);
                if let Some(slot) = slot {
                    app.pressed[slot] = true;
                }
                let divider = app.pal.border.gamma_multiply(0.4);
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            Vec2::new(width, 1200.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| app.keys_screen(ui));
                    },
                );
                let lines = output.shapes.iter().filter_map(|shape| match &shape.shape {
                    egui::Shape::LineSegment { points, stroke } if stroke.color == divider => Some(*points),
                    _ => None,
                }).collect::<Vec<_>>();
                let highlighted = output.shapes.iter().filter(|shape| matches!(&shape.shape,
                    egui::Shape::Rect(rect) if rect.fill == theme::rgb(0xFFD166)
                )).count();
                (lines, highlighted)
            };

            let (idle, _) = render(None);
            assert_eq!(idle.len(), 20);
            for slot in 0..20 {
                let (pressed, highlighted) = render(Some(slot));
                assert_eq!(pressed, idle, "{width}, slot {slot}");
                assert_eq!(highlighted, 1, "{width}, slot {slot}");
            }
            assert_eq!(render(None), (idle, 0));
        }
    }

    #[test]
    fn preserve_pending_edits_and_refresh_unedited_external_changes() {
        let mut local = model::defaults();
        let mut saved = local.clone();
        local[0].poll = 250;
        local[1].dpi[0] = 1600;
        let mut actual = model::defaults();
        actual[0].poll = 250;
        actual[2].color = [1, 2, 3];
        let refresh = merge_profiles(
            &mut local,
            &mut saved,
            &actual,
            Some(0),
            [true, true, false, false, false],
            false,
        );
        assert_eq!(local[0], actual[0]);
        assert_eq!(local[1].dpi[0], 1600);
        assert_eq!(saved[1].dpi[0], 800);
        assert_eq!(local[2], actual[2]);
        assert_eq!(refresh, [true, false, true, true, true]);
    }
    #[test]
    fn pc_save_does_not_erase_edit_baseline_on_switch() {
        let mut local = model::defaults();
        local[1].poll = 250;
        let mut saved = local.clone();
        let mut actual = model::defaults();
        actual[1].color = [1, 2, 3];
        let refresh = merge_profiles(
            &mut local,
            &mut saved,
            &actual,
            None,
            [false, true, false, false, false],
            false,
        );
        assert!(!refresh[1]);
        assert_eq!(local[1].poll, 250);
        assert_eq!(local[1].color, model::defaults()[1].color);
    }
    #[test]
    fn automatic_main_save_preserves_pending_key_assignment() {
        let mut local = model::defaults();
        let mut saved = local.clone();
        let mut actual = local.clone();
        local[0].buttons[6] = "Ctrl + C".into();
        local[0].dpi[0] = 1600;
        actual[0].dpi[0] = 1600;
        merge_profiles(
            &mut local,
            &mut saved,
            &actual,
            Some(0),
            [true, false, false, false, false],
            true,
        );
        assert_eq!(local[0].buttons[6], "Ctrl + C");
        assert_eq!(saved[0].buttons[6], actual[0].buttons[6]);
        assert_eq!(local[0].dpi[0], 1600);
    }
    #[test]
    fn committed_key_queues_live_apply_but_not_preview_or_backup() {
        let ctx = egui::Context::default();
        let mut options = Options::parse();
        options.capture = Some(PathBuf::from("unused-preview.png"));
        options.editor = false;
        options.page = 0;
        let mut app = App::new(&ctx, options);
        // 接続有無によるキュー制御だけを確認し、設定解析や実機通信は行わない。
        app.live = Some(Snapshot {
            parts: Default::default(),
            has_macros: false,
        });
        app.queue_key_apply();
        assert!(!app.key_pending.iter().any(|v| *v));
        app.options.capture = None;
        app.backup_view = true;
        app.queue_key_apply();
        assert!(!app.key_pending.iter().any(|v| *v));
        app.backup_view = false;
        app.queue_key_apply();
        assert!(app.key_pending[app.profile]);
        assert!(app.job.is_none());
    }
}

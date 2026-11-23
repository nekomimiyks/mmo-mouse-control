use crate::ui;
use eframe::egui;
use g600ctl::gui;
use m908ctl::device;
use std::{collections::HashSet, ffi::CString, sync::mpsc, time::Duration};

pub struct Preview(pub ui::App);

impl eframe::App for Preview {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.0.tick(ctx);
        self.0.draw(ctx, 0.0, true);
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
enum Mouse {
    #[default]
    M908,
    G600,
}

impl Mouse {
    fn for_connections(self, m908: bool, g600: bool) -> Self {
        match (m908, g600) {
            (true, false) => Self::M908,
            (false, true) => Self::G600,
            _ => self,
        }
    }

    fn on_connection_change(self, previous: &mut (bool, bool), current: (bool, bool)) -> Self {
        if *previous == current { return self; }
        *previous = current;
        self.for_connections(current.0, current.1)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Presence {
    #[default]
    Unseen,
    Connected,
    Disconnected,
}

impl Presence {
    fn observe(&mut self, connected: bool) {
        if connected {
            *self = Self::Connected;
        } else if *self == Self::Connected {
            *self = Self::Disconnected;
        }
    }

    fn button_label(self, name: &str) -> Option<String> {
        match self {
            Self::Unseen => None,
            Self::Connected => Some(name.into()),
            Self::Disconnected => Some(format!("{name}（切断）")),
        }
    }
}

type ScanResult = Result<Vec<CString>, String>;

#[derive(Clone, PartialEq, Eq)]
struct Connections {
    m908: ScanResult,
    g600: ScanResult,
}

#[derive(Default)]
struct Detection {
    paths: Option<Vec<CString>>,
    presence: Presence,
    error: Option<String>,
    refresh: bool,
}

impl Detection {
    fn observe(&mut self, result: ScanResult) {
        match result {
            Ok(paths) => {
                self.presence.observe(!paths.is_empty());
                self.refresh |= self.paths.as_ref() != Some(&paths);
                self.paths = Some(paths);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }
}

fn detect_connections() -> Connections {
    let m908 = device::enumerate().map(|devices| devices.into_iter()
        .filter(device::Device::is_config).map(|d| d.path).collect());
    let g600 = g600ctl::device::g600_interfaces().map(|api| {
        api.device_list().map(|d| d.path().to_owned()).collect()
    });
    Connections { m908, g600 }
}

fn record_removed(removed: &mut HashSet<CString>, before: &[CString], after: &[CString]) {
    removed.extend(before.iter().filter(|path| !after.contains(path)).cloned());
}

fn watch_connections(ctx: egui::Context) -> mpsc::Receiver<Connections> {
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let mut last = None;
        loop {
            let mut current = detect_connections();
            for result in [&mut current.m908, &mut current.g600] {
                if let Ok(paths) = result {
                    paths.sort_unstable();
                }
            }
            if last.as_ref() != Some(&current) {
                if send.send(current.clone()).is_err() {
                    break;
                }
                last = Some(current);
                ctx.request_repaint();
            }
            std::thread::sleep(Duration::from_millis(750));
        }
    });
    receive
}

pub struct App {
    m908: ui::App,
    g600: gui::App,
    active: Mouse,
    last_availability: (bool, bool),
    watcher: mpsc::Receiver<Connections>,
    m908_detection: Detection,
    g600_detection: Detection,
    m908_paths: Option<Vec<CString>>,
    removed_m908: HashSet<CString>,
    close_after_g600: bool,
    confirm_g600_close: bool,
    discard_g600_on_close: bool,
    backup: crate::settings_backup::Panel,
}

impl App {
    pub fn new(ctx: &egui::Context, m908: ui::App, g600: gui::App) -> Self {
        Self {
            m908,
            g600,
            active: Mouse::default(),
            last_availability: (false, false),
            watcher: watch_connections(ctx.clone()),
            m908_detection: Detection::default(),
            g600_detection: Detection::default(),
            m908_paths: None,
            removed_m908: HashSet::new(),
            close_after_g600: false,
            confirm_g600_close: false,
            discard_g600_on_close: false,
            backup: crate::settings_backup::Panel::default(),
        }
    }

    fn switch(&mut self, ctx: &egui::Context, next: Mouse) {
        if self.active == next {
            return;
        }
        self.active = next;
        match next {
            Mouse::M908 => self.m908.activate(ctx),
            Mouse::G600 => {
                self.m908.suspend();
                self.g600.activate(ctx);
            }
        }
        ctx.request_repaint();
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.m908.tick(ctx);
        self.g600.tick();
        let closing = ctx.input(|i| i.viewport().close_requested());
        while let Ok(connections) = self.watcher.try_recv() {
            if let (Some(last), Ok(current)) = (&self.m908_detection.paths, &connections.m908) {
                record_removed(&mut self.removed_m908, last, current);
            }
            self.m908_detection.observe(connections.m908);
            self.g600_detection.observe(connections.g600);
        }

        let mut pending = false;
        if !closing {
            if let Some(paths) = self.m908_detection.paths.as_ref().filter(|_| self.m908_detection.refresh) {
                let same_list = self.m908_paths.as_ref() == Some(paths);
                if self.m908.update_connection(paths, &self.removed_m908, same_list) {
                    self.m908_paths = Some(paths.clone());
                    self.m908_detection.refresh = false;
                    self.removed_m908.clear();
                } else {
                    pending = true;
                }
            }
            if self.g600_detection.refresh {
                if self.g600.update_connection(self.g600_detection.presence == Presence::Connected) {
                    self.g600_detection.refresh = false;
                } else {
                    pending = true;
                }
            }
        }
        if closing {
            self.g600.prepare_close();
        }
        let waiting_for_g600 = self.g600.has_pending_writes();
        if closing && waiting_for_g600 {
            self.close_after_g600 = true;
        }
        if (closing || self.close_after_g600) && !waiting_for_g600 {
            if self.g600.has_unsaved_changes() && !self.discard_g600_on_close {
                self.confirm_g600_close = true;
            } else if self.close_after_g600 && !closing {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            self.close_after_g600 = false;
        }
        let defer_close = self.close_after_g600 || self.confirm_g600_close;
        if closing && defer_close {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        }
        let close_m908 = closing && !defer_close && self.m908.needs_close_handling();
        let m908_connected = self.m908_detection.presence == Presence::Connected;
        let g600_connected = self.g600_detection.presence == Presence::Connected;
        let mut next = if close_m908 {
            Mouse::M908
        } else if !closing && !self.m908.close_in_progress() {
            self.active.on_connection_change(&mut self.last_availability, (m908_connected, g600_connected))
        } else {
            self.active
        };
        let header = egui::TopBottomPanel::top("mouse-device-selector").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("マウス");
                ui.add_enabled_ui(!closing && !self.m908.close_in_progress(), |ui| {
                    self.backup.button(ui);
                    for (mouse, presence, name) in [
                        (Mouse::M908, self.m908_detection.presence, device::DISPLAY_NAME),
                        (Mouse::G600, self.g600_detection.presence, "G600 / G600r"),
                    ] {
                        if let Some(label) = presence.button_label(name) {
                            if ui.add(egui::Button::new(label).selected(mouse == next)).clicked() {
                                next = mouse;
                            }
                        }
                    }
                });
                let mut detection_failed = false;
                for (name, detection) in [
                    (device::DISPLAY_NAME, &self.m908_detection),
                    ("G600 / G600r", &self.g600_detection),
                ] {
                    if let Some(error) = &detection.error {
                        ui.label(format!("{name}のUSB検出に失敗しました")).on_hover_text(error);
                        detection_failed = true;
                    }
                }
                if !detection_failed {
                    if self.m908_detection.paths.is_none() || self.g600_detection.paths.is_none() {
                        ui.label("USB機器を確認中…");
                    } else if self.m908_detection.presence == Presence::Unseen && self.g600_detection.presence == Presence::Unseen {
                        ui.label("対応するマウスを接続してください");
                    }
                }
            });
        }).response.rect.height();

        self.switch(ctx, next);
        match self.active {
            Mouse::M908 => {
                self.g600.flush_pending(ctx);
                self.m908.draw(ctx, header, !defer_close);
            }
            Mouse::G600 => {
                self.m908.flush_pending(ctx);
                self.g600.draw(ctx, header);
            }
        }
        if !closing && !self.m908.close_in_progress() && !self.confirm_g600_close {
            // M908側の終了確認を取り消した後は、G600rの破棄確認を再び有効にする。
            self.discard_g600_on_close = false;
        }
        if self.confirm_g600_close {
            egui::Modal::new(egui::Id::new("g600-close-confirmation"))
                .show(ctx, |ui| {
                    ui.heading("G600rの未反映の編集");
                    ui.label("G600rの未確定または未反映の編集があります。");
                    ui.horizontal(|ui| {
                        if ui.button("編集を続ける").clicked() {
                            self.confirm_g600_close = false;
                        }
                        if ui.button("破棄して終了").clicked() {
                            self.confirm_g600_close = false;
                            self.discard_g600_on_close = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                });
        }
        if !closing && !self.confirm_g600_close {
            self.backup.show(ctx);
        }
        if pending || self.close_after_g600 || self.m908.communication_pending() {
            ctx.request_repaint_after(Duration::from_millis(30));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Mouse::*;

    #[test]
    fn accumulates_removed_devices_across_list_changes() {
        use super::{record_removed, CString, HashSet};
        let a = CString::new("A").unwrap();
        let b = CString::new("B").unwrap();
        let both = [a.clone(), b.clone()];
        let mut removed = HashSet::new();
        record_removed(&mut removed, &both, std::slice::from_ref(&b));
        record_removed(&mut removed, std::slice::from_ref(&b), std::slice::from_ref(&a));
        assert!(removed.contains(&a));

        removed.clear();
        record_removed(&mut removed, &both, std::slice::from_ref(&a));
        record_removed(&mut removed, std::slice::from_ref(&a), &both);
        assert!(!removed.contains(&a));
        assert!(removed.contains(&b));
    }

    #[test]
    fn one_failed_scan_does_not_prevent_the_other_from_updating() {
        use super::{CString, Detection, Presence};
        let mut m908 = Detection::default();
        let mut g600 = Detection::default();
        m908.observe(Err("HID enumeration".into()));
        g600.observe(Ok(vec![CString::new("G600").unwrap()]));
        assert_eq!(m908.presence, Presence::Unseen);
        assert_eq!(g600.presence, Presence::Connected);
        assert!(m908.error.is_some());
        assert!(g600.refresh);

        g600.observe(Err("HID enumeration".into()));
        m908.observe(Ok(vec![CString::new("M908").unwrap()]));
        assert_eq!(m908.presence, Presence::Connected);
        assert_eq!(g600.presence, Presence::Connected);
        assert!(m908.error.is_none());
        g600.observe(Ok(vec![]));
        assert_eq!(g600.presence, Presence::Disconnected);
    }

    #[test]
    fn device_selector_shows_only_mice_seen_since_launch() {
        use super::Presence;

        let mut m908 = Presence::default();
        let mut g600 = Presence::default();
        m908.observe(false);
        g600.observe(true);
        assert_eq!(m908.button_label("M908"), None);
        assert_eq!(g600.button_label("G600").as_deref(), Some("G600"));

        g600.observe(false);
        g600.observe(false);
        assert_eq!(g600.button_label("G600").as_deref(), Some("G600（切断）"));
        g600.observe(true);
        assert_eq!(g600.button_label("G600").as_deref(), Some("G600"));
        assert_eq!(Presence::default().button_label("G600"), None);
    }

    #[test]
    fn switches_only_when_the_current_mouse_is_unavailable() {
        assert_eq!(M908.for_connections(false, true), G600);
        assert_eq!(G600.for_connections(true, false), M908);
        assert_eq!(M908.for_connections(true, true), M908);
        assert_eq!(G600.for_connections(true, true), G600);
        assert_eq!(G600.for_connections(false, false), G600);
    }

    #[test]
    fn manually_selected_disconnected_mouse_stays_visible_until_availability_changes() {
        let mut previous = (true, true);
        assert_eq!(G600.on_connection_change(&mut previous, (true, false)), M908);
        // The user explicitly selects the known but disconnected G600 again.
        assert_eq!(G600.on_connection_change(&mut previous, (true, false)), G600);
        assert_eq!(G600.on_connection_change(&mut previous, (true, true)), G600);
    }
}

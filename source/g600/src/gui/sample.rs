use super::*;
use g600ctl::protocol::{Active, ProfileEdit, PROFILE_LEN, REPORT_PROFILES};

/// Render sample profiles for documentation without opening or writing to USB devices.
pub fn sample(ctx: &egui::Context, language: &str, buttons: bool) -> App {
    fonts::install(ctx);
    theme::apply(ctx);
    i18n::activate(language);
    let (commands, _) = std::sync::mpsc::channel();
    let (_, responses) = std::sync::mpsc::channel();
    let source = match language {
        "ja" => "サンプル",
        "zh-Hans" => "示例",
        "zh-Hant" => "範例",
        "ko" => "예시",
        "es" => "Ejemplo",
        "fr" => "Exemple",
        "de" => "Beispiel",
        "pt" => "Exemplo",
        _ => "Sample",
    };
    let mut app = App {
        language: language.into(), settings_open: false, commands, responses,
        settings: None, edits: vec![], device_identity: None, connected: true, busy: false, tab: 1,
        tab_initialized: true, path: String::new(), source: source.into(), message: String::new(),
        button_editor: None, screen: if buttons { Screen::Keys } else { Screen::Main },
        keys: edit::keys(), custom: Vec::new(), custom_path: None, custom_message: String::new(),
        backup_path: None, confirm: None,
        pointer: Some(pointer::PointerSettings { speed: 9, acceleration: false, wheel_lines: 3, wheel_chars: 3, wheel_page: false }),
        pointer_message: String::new(), button_presses: inputlog::Monitor::default(), held: winkeys::Held::default(),
        last_sent: String::new(), in_flight: None, unverified_write: None, last_edit: None,
        edit_snapshot: vec![], last_done: [None; 3], auto_enabled: true, session_backup: None,
        sync_status: String::new(), led_memory: LedMemory::default(), backup_led_memory: LedMemory::default(),
    };
    let profiles = REPORT_PROFILES.into_iter().map(|id| {
        let mut raw = vec![0; PROFILE_LEN];
        raw[0] = id;
        let mut profile = ProfileEdit(&mut raw);
        profile.set_dpi(&[1200, 2400, 0, 0], Some(0)).unwrap();
        profile.set_poll(1000).unwrap();
        profile.set_led([248, 146, 31], LedEffect::Solid, 3).unwrap();
        let actions = [Action::Mouse(1), Action::Mouse(2), Action::Mouse(3), Action::Mouse(4),
            Action::Mouse(5), Action::GShift, Action::DpiCycle, Action::ProfileCycle];
        for (slot, action) in actions.into_iter().enumerate() {
            profile.set_button(slot, false, action);
            profile.set_button(slot, true, action);
        }
        for (offset, key) in [30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 87, 86].into_iter().enumerate() {
            profile.set_button(offset + 8, false, Action::Key { modifiers: 0, usage: key });
            profile.set_button(offset + 8, true, Action::Key { modifiers: 1, usage: key });
        }
        Profile::parse(&raw).unwrap()
    }).collect();
    app.replace(DisplaySettings { active: Active { profile: 1, dpi_slot: 0 }, profiles });
    app
}

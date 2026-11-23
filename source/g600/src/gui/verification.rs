//! Render the real UI with synthetic settings and disconnected command channels.
//! Never starts HID readers, reads keyboard state, or changes Windows settings.
use super::*;
use std::sync::Mutex;

static LOCALE_LOCK: Mutex<()> = Mutex::new(());

fn identity(name: &str) -> DeviceIdentity {
    DeviceIdentity { path: std::ffi::CString::new(name).unwrap(), serial: Some(name.into()) }
}

fn connected(settings: DisplaySettings) -> Response {
    Response::Connected(Ok(binding(identity("A"), settings)))
}

fn binding(identity: DeviceIdentity, settings: DisplaySettings) -> DeviceSettings {
    DeviceSettings { identity, settings, inputs: Vec::new(), continued_handle: false }
}

fn fixture() -> (App, std::sync::mpsc::Receiver<Command>) {
    let (commands, receiver) = std::sync::mpsc::channel();
    let (_, responses) = std::sync::mpsc::channel();
    let mut app = App {
        language: "ja".into(), settings_open: false, commands, responses,
        settings: None, edits: vec![], device_identity: Some(identity("A")), connected: true, busy: false, tab: 0,
        tab_initialized: true, last_backup_path: None, source: "実機".into(), message: String::new(),
        button_editor: None, screen: Screen::Main, keys: edit::keys(),
        custom: vec![("My shortcut".into(), Action::Key { modifiers: 1, usage: 6 })],
        custom_path: None, custom_message: String::new(), backup_path: None, confirm: None, backup_confirmation: None,
        pointer: Some(pointer::PointerSettings { speed: 10, acceleration: true, wheel_lines: 3, wheel_chars: 3, wheel_page: false }),
        pointer_message: String::new(), button_presses: inputlog::Monitor::default(), held: winkeys::Held::default(),
        last_sent: String::new(), in_flight: None, unverified_write: None,
        last_edit: None, edit_snapshot: vec![], last_done: [None; 3], auto_enabled: true,
        session_backup: None, sync_status: String::new(), led_memory: LedMemory::default(), backup_led_memory: LedMemory::default(),
    };
    let profiles = g600ctl::protocol::REPORT_PROFILES.iter().map(|id| {
        let mut raw = vec![0; g600ctl::protocol::PROFILE_LEN]; raw[0] = *id;
        let mut edit = g600ctl::protocol::ProfileEdit(&mut raw);
        edit.set_dpi(&[400, 800, 1600, 3200], Some(2)).unwrap();
        edit.set_poll(1000).unwrap();
        edit.set_led([0,128,255], LedEffect::Breathe, 4).unwrap();
        for i in 0..20 { edit.set_button(i, false, Action::Mouse(1)); }
        Profile::parse(&raw).unwrap()
    }).collect();
    app.replace(DisplaySettings { active: g600ctl::protocol::Active { profile: 0, dpi_slot: 1 }, profiles });
    (app, receiver)
}

fn collect_text(shape: &egui::Shape, output: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(text) => output.push(text.galley.job.text.clone()),
        egui::Shape::Vec(shapes) => for shape in shapes { collect_text(shape, output); },
        _ => {},
    }
}

#[test]
fn all_languages_render_main_keys_editors_and_settings_without_device_writes() {
    let _locale = LOCALE_LOCK.lock().unwrap();
    let mut frames = 0;
    for (code, _) in i18n::LANGUAGES.iter().skip(1) {
        i18n::activate(code);
        let ctx = egui::Context::default(); fonts::install(&ctx); theme::apply(&ctx);
        let (mut app, commands) = fixture(); app.language = code.to_string();
        for width in [1060.0, 800.0, 640.0] {
            for screen in 0..6 {
                app.screen = if screen == 0 { Screen::Main } else { Screen::Keys };
                app.button_editor = None; app.settings_open = screen == 5;
                if (2..=4).contains(&screen) {
                    app.open_button_editor(8, false, Action::Key { modifiers: 1, usage: 6 });
                    app.button_editor.as_mut().unwrap().open_section = Some(match screen {2 => layout::Section::Windows, 3 => layout::Section::Chrome, _ => layout::Section::Custom});
                }
                let mut visible = vec![];
                for _ in 0..2 {
                    let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 900.0))), ..Default::default() };
                    let output = ctx.run(input, |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            app.body(ui, false);
                        });
                        app.editor_window(ctx); app.settings_window(ctx);
                    });
                    visible.clear();
                    for shape in output.shapes {
                        if let egui::Shape::Text(text) = &shape.shape {
                            let right = text.pos.x + text.galley.size().x;
                            assert!(right <= width + 2.0, "text overflows viewport: {code}/{width}/{screen}: {} (pos={:?}, size={:?}, right={right}, clip={:?})", text.galley.job.text, text.pos, text.galley.size(), shape.clip_rect);
                        }
                        collect_text(&shape.shape, &mut visible);
                    }
                }
                assert!(!visible.is_empty(), "empty frame: {code}/{width}/{screen}");
                if *code != "ja" && screen != 5 {
                    for text in &visible {
                        assert!(!text.chars().any(|c| ('\u{3040}'..='\u{30ff}').contains(&c)), "untranslated UI: {code}/{width}/{screen}: {text}");
                    }
                }
                assert!(commands.try_recv().is_err(), "rendering sent a device command");
                assert!(app.dirty().iter().all(|d| !d), "rendering changed profile bytes");
                frames += 1;
            }
        }
    }
    i18n::activate("ja");
    // Keep an edited dialog open while switching all languages. Unsaved data,
    // user-authored names, and raw profile bytes must remain byte-for-byte intact.
    let ctx = egui::Context::default(); fonts::install(&ctx); theme::apply(&ctx);
    let (mut app, commands) = fixture();
    app.screen = Screen::Keys;
    app.open_button_editor(8, false, Action::Key { modifiers: 1, usage: 6 });
    app.custom.push(("保存 {0} 日本語".into(), Action::Key { modifiers: 1, usage: 22 }));
    let editor = app.button_editor.as_mut().unwrap();
    editor.name = "Unsaved {1} 編集内容".into(); editor.search = "F1".into();
    editor.key_mods = 0x41; editor.key_usage = 0x04; editor.open_section = Some(layout::Section::Custom);
    let bytes = app.edits.clone();
    for (code, _) in i18n::LANGUAGES.iter().skip(1) {
        app.language = code.to_string(); i18n::activate(code);
        let _ = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1060.0, 900.0))), ..Default::default() }, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| { app.body(ui, false); });
            app.editor_window(ctx);
        });
        let editor = app.button_editor.as_ref().unwrap();
        assert_eq!(editor.name, "Unsaved {1} 編集内容"); assert_eq!(editor.search, "F1");
        assert_eq!((editor.key_mods, editor.key_usage), (0x41, 0x04));
        assert_eq!(app.custom[1].0, "保存 {0} 日本語");
        assert_eq!(app.edits, bytes); assert!(commands.try_recv().is_err());
    }
    i18n::activate("ja");
    println!("GUI verification: {frames} language/width/screen combinations passed (2 frames each)");
}

fn render_keys(app: &mut App, ctx: &egui::Context, width: f32, scale: f32, events: Vec<egui::Event>) -> egui::FullOutput {
    let mut input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 1000.0))),
        events,
        ..Default::default()
    };
    input.viewports.get_mut(&egui::ViewportId::ROOT).unwrap().native_pixels_per_point = Some(scale);
    ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| app.keys_screen(ui));
    })
}

fn key_cell_lines(frame: &egui::FullOutput) -> Vec<[egui::Pos2; 2]> {
    frame.shapes.iter().filter_map(|s| match &s.shape {
        egui::Shape::LineSegment { points, stroke } if stroke.color == theme::LINE => Some(*points),
        _ => None,
    }).collect()
}

#[test]
fn button_presses_keep_all_cells_in_place_and_both_assignments_clickable() {
    let _locale = LOCALE_LOCK.lock().unwrap();
    i18n::activate("ja");
    for width in [600.0, 800.0, 1060.0] {
        for scale in [1.0, 1.25] {
            let ctx = egui::Context::default();
            fonts::install(&ctx);
            theme::apply(&ctx);
            let (mut app, commands) = fixture();
            app.screen = Screen::Keys;
            render_keys(&mut app, &ctx, width, scale, vec![]);
            let idle = key_cell_lines(&render_keys(&mut app, &ctx, width, scale, vec![]));
            assert_eq!(idle.len(), 20);
            for row in idle[..8].chunks_exact(2).chain(idle[8..].chunks_exact(3)) {
                assert!(row.windows(2).all(|pair| pair[0][0].y == pair[1][0].y && pair[0][1].x < pair[1][0].x));
            }
            assert!(idle.iter().all(|line| line[1].x <= width));
            let now = Instant::now();
            for mask in (0..20).map(|slot| 1u32 << slot).chain([0xFFFFF]) {
                let report = [0x80, mask as u8, (mask >> 8) as u8, (mask >> 16) as u8];
                app.button_presses.set_test_press(buttons::mask_from_report(&report).unwrap(), now);
                app.last_sent = "LCtrl+LShift+Num9, Num8, Num7, Num6, Num5, Num4, Num3".into();
                let output = render_keys(&mut app, &ctx, width, scale, vec![]);
                assert_eq!(key_cell_lines(&output), idle, "{width}/{scale}, mask {mask:#x}");
                let highlights = output.shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::Rect(rect) if rect.fill == theme::HOT_BG)).count();
                assert_eq!(highlights, mask.count_ones() as usize);
            }
            app.button_presses.set_test_press(0, Instant::now());
            assert_eq!(key_cell_lines(&render_keys(&mut app, &ctx, width, scale, vec![])), idle);

            if width == 1060.0 && scale == 1.0 {
                for (slot, line) in idle.iter().enumerate() {
                    for (shifted, fraction) in [(false, 0.56), (true, 0.86)] {
                        let point = egui::pos2(egui::lerp(line[0].x..=line[1].x, fraction), line[0].y - theme::s(15.0) - 2.0);
                        for pressed in [true, false] {
                            render_keys(&mut app, &ctx, width, scale, vec![
                                egui::Event::PointerMoved(point),
                                egui::Event::PointerButton {
                                    pos: point, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE,
                                },
                            ]);
                        }
                        let editor = app.button_editor.take().expect("割り当てのセルが反応しません");
                        assert_eq!((editor.slot, editor.shifted), (slot, shifted));
                    }
                }
            }
            assert!(commands.try_recv().is_err());
            assert!(app.dirty().iter().all(|d| !d));
        }
    }
}

#[test]
fn disconnected_screen_still_offers_to_open_a_backup() {
    let _locale = LOCALE_LOCK.lock().unwrap();
    i18n::activate("ja");
    let ctx = egui::Context::default();
    fonts::install(&ctx);
    theme::apply(&ctx);
    let (mut app, commands) = fixture();
    assert!(app.update_connection(false));
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| app.body(ui, false));
    });
    let mut visible = Vec::new();
    for shape in output.shapes {
        collect_text(&shape.shape, &mut visible);
    }
    assert!(visible.iter().any(|label| label == "設定を読み込む"));
    assert!(commands.try_recv().is_err());
}

#[test]
fn interrupted_write_leaves_unsaved_edits_available_after_reconnect() {
    let (mut app, commands) = fixture();
    app.edits[0][1] = 12;
    app.in_flight = Some(0);
    assert!(app.has_pending_writes());
    assert!(!app.update_connection(false));

    app.in_flight = None;
    app.auto_enabled = false;
    assert!(app.update_connection(false));
    assert!(!app.has_pending_writes());
    assert!(app.unsaved_device_edits());

    assert!(app.update_connection(true));
    assert!(matches!(commands.try_recv(), Ok(Command::Connect(Some(device))) if device == identity("A")));
    let mut readback = fixture().0.settings.unwrap();
    readback.profiles[1].raw[1] = 99;
    app.handle(connected(readback));

    assert_eq!(app.edits[0][1], 12);
    assert_eq!(app.edits[1][1], 99);
    assert!(app.unsaved_device_edits());
    assert!(!app.auto_enabled);
}

#[test]
fn readback_adopts_external_changes_to_unedited_profiles_only() {
    let (mut app, _) = fixture();
    let sent = app.edits[0].clone();
    app.edits[0][1] = 22;
    app.edits[2][1] = 33;
    let mut readback = fixture().0.settings.unwrap();
    let original = readback.profiles[2].raw.clone();
    readback.profiles[1].raw[1] = 99;
    readback.profiles[2].raw[1] = 88;

    app.accept_auto_apply(Some((0, &sent)), readback);

    assert_eq!(app.edits[0][1], 22);
    assert_eq!(app.edits[1][1], 99);
    assert_eq!(app.edits[2][1], 33);
    assert_eq!(app.settings.as_ref().unwrap().profiles[2].raw, original);
    assert_eq!(app.dirty(), [true, false, true]);
}

#[test]
fn reconnect_recognizes_a_successful_write_after_readback_was_interrupted() {
    let (mut app, _) = fixture();
    app.edits[0][1] = 20;
    app.auto_enabled = false;
    let mut actual = fixture().0.settings.unwrap();
    actual.profiles[0].raw[1] = 20;
    app.handle(connected(actual));

    assert!(!app.unsaved_device_edits());
    assert!(app.auto_enabled);
}

#[test]
fn main_screen_aligns_pointer_controls_and_backup_actions() {
    let _locale = LOCALE_LOCK.lock().unwrap();
    i18n::activate("ja");
    let ctx = egui::Context::default();
    fonts::install(&ctx);
    theme::apply(&ctx);
    let (mut app, commands) = fixture();
    app.pointer.as_mut().unwrap().wheel_lines = 9;
    app.pointer.as_mut().unwrap().wheel_chars = 7;
    app.last_backup_path = Some("C:/Backups/G600.mmo-backup".into());

    for width in [600.0, 870.0, 1010.0] {
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 1000.0))),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| app.body(ui, false));
            },
        );

        let position = |label: &str| {
            output.shapes.iter().find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) if text.galley.job.text == label => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            }).unwrap_or_else(|| panic!("画面上で {label} が見つかりません（幅 {width}）"))
        };
        let same_row = |left: &str, right: &str| {
            let left = position(left);
            let right = position(right);
            assert!((left.y - right.y).abs() < 4.0 && left.x < right.x, "{width}: {left:?} / {right:?}");
        };
        same_row("● 接続済み", "設定");
        same_row("カーソルの速さ", "加速");
        same_row("スクロール行数", "1画面ずつ");
        same_row("横スクロール（チルト）", "文字");
        same_row("保存先を選択", "設定を読み込む");
        same_row("設定を読み込む", "パス");
        same_row("パス", "C:/Backups/G600.mmo-backup");
        let vertical = position("スクロール行数");
        let horizontal = position("横スクロール（チルト）");
        if width > 720.0 {
            assert!((horizontal.y - vertical.y).abs() < 4.0 && horizontal.x > vertical.x);
        } else {
            assert!(horizontal.y > vertical.y + 10.0);
        }
        assert!(commands.try_recv().is_err());
    }
}

#[test]
fn remembers_latest_edit_when_the_previous_write_lost_readback() {
    for desired in [0, 35] {
        let (mut app, commands) = fixture();
        let (send, receive) = std::sync::mpsc::channel();
        app.responses = receive;
        let mut sent = app.edits[0].clone();
        sent[1] = 22;
        app.edits[0][1] = desired;
        app.in_flight = Some(0);
        send.send(Response::AutoApply(0, sent.clone(), Err(("通信切断".into(), None)))).unwrap();
        app.tick();
        assert!(app.unsaved_device_edits());
        assert!(!app.has_pending_writes());
        assert!(app.update_connection(false));
        assert_eq!(app.edits[0][1], desired);
        assert!(app.update_connection(true));
        assert!(matches!(commands.try_recv(), Ok(Command::Connect(Some(device))) if device == identity("A")));
        let mut readback = fixture().0.settings.unwrap();
        readback.profiles[0] = Profile::parse(&sent).unwrap();
        app.handle(connected(readback));

        assert_eq!(app.settings.as_ref().unwrap().profiles[0].raw, sent);
        assert_eq!(app.edits[0][1], desired);
        assert!(app.unsaved_device_edits());
        assert!(app.unverified_write.is_none());
    }
}

#[test]
fn reconnect_does_not_transfer_edits_to_another_g600() {
    let (mut app, commands) = fixture();
    app.edits[0][1] = 35;
    assert!(app.update_connection(false));
    assert!(app.update_connection(true));
    assert!(matches!(commands.try_recv(), Ok(Command::Connect(Some(device))) if device == identity("A")));

    let settings = fixture().0.settings.unwrap();
    app.handle(Response::Connected(Ok(binding(identity("B"), settings))));
    assert!(!app.connected);
    assert!(!app.auto_enabled);
    assert_eq!(app.device_identity, Some(identity("A")));
    assert_eq!(app.edits[0][1], 35);
    assert!(app.unsaved_device_edits());
}

#[test]
fn reconnect_restores_an_open_editor_for_the_original_g600() {
    let (mut app, commands) = fixture();
    app.tab = 1;
    app.open_button_editor(8, true, Action::Mouse(1));
    let editor = app.button_editor.as_mut().unwrap();
    editor.action = Action::Key { modifiers: 1, usage: 9 };
    editor.name = "登録前の名前".into();
    assert!(!app.unsaved_device_edits());

    assert!(app.update_connection(false));
    assert!(app.update_connection(true));
    assert!(matches!(commands.try_recv(), Ok(Command::Connect(Some(device))) if device == identity("A")));
    let mut moved = identity("A");
    moved.path = std::ffi::CString::new("別のUSBポート").unwrap();
    app.handle(Response::Connected(Ok(binding(moved, fixture().0.settings.unwrap()))));

    assert!(app.connected);
    assert_eq!(app.tab, 1);
    let editor = app.button_editor.unwrap();
    assert_eq!((editor.slot, editor.shifted), (8, true));
    assert_eq!(editor.action, Action::Key { modifiers: 1, usage: 9 });
    assert_eq!(editor.name, "登録前の名前");
}

#[test]
fn background_assignment_click_leaves_the_open_draft_intact() {
    let _locale = LOCALE_LOCK.lock().unwrap();
    i18n::activate("ja");
    let ctx = egui::Context::default();
    fonts::install(&ctx);
    theme::apply(&ctx);
    let (mut app, _) = fixture();
    app.screen = Screen::Keys;
    render_keys(&mut app, &ctx, 1060.0, 1.0, vec![]);
    let [from, to] = key_cell_lines(&render_keys(&mut app, &ctx, 1060.0, 1.0, vec![]))[0];
    let point = egui::pos2(egui::lerp(from.x..=to.x, 0.56), from.y - theme::s(15.0) - 2.0);
    app.open_button_editor(8, true, Action::Mouse(1));
    app.button_editor.as_mut().unwrap().name = "下書き".into();
    render_keys(&mut app, &ctx, 1060.0, 1.0, vec![]);
    for pressed in [true, false] {
        render_keys(&mut app, &ctx, 1060.0, 1.0, vec![
            egui::Event::PointerMoved(point),
            egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE },
        ]);
    }
    let editor = app.button_editor.unwrap();
    assert_eq!((editor.slot, editor.shifted), (8, true));
    assert_eq!(editor.name, "下書き");
}

#[test]
fn reconnect_refreshes_untouched_assignment_and_does_not_write_it_back() {
    let (mut app, _) = fixture();
    app.open_button_editor(8, false, Action::Mouse(1));
    let mut readback = fixture().0.settings.unwrap();
    let raw = &mut readback.profiles[0].raw;
    g600ctl::protocol::ProfileEdit(raw).set_button(8, false, Action::Mouse(2));
    readback.profiles[0] = Profile::parse(raw).unwrap();

    app.handle(connected(readback));
    let editor = app.button_editor.as_ref().unwrap();
    assert_eq!(editor.original, Action::Mouse(2));
    assert_eq!(editor.action, Action::Mouse(2));
    assert!(!editor.conflict);
    app.commit_button_editor();
    assert!(!app.unsaved_device_edits());
}

#[test]
fn reconnect_keeps_assignment_draft_and_marks_external_conflict() {
    let (mut app, _) = fixture();
    app.open_button_editor(8, false, Action::Mouse(1));
    let draft = Action::Key { modifiers: 1, usage: 9 };
    app.button_editor.as_mut().unwrap().action = draft;
    let mut readback = fixture().0.settings.unwrap();
    let raw = &mut readback.profiles[0].raw;
    g600ctl::protocol::ProfileEdit(raw).set_button(8, false, Action::Mouse(2));
    readback.profiles[0] = Profile::parse(raw).unwrap();

    app.handle(connected(readback));
    let editor = app.button_editor.as_ref().unwrap();
    assert_eq!(editor.original, Action::Mouse(2));
    assert_eq!(editor.action, draft);
    assert!(editor.conflict);
    assert!(!app.unsaved_device_edits());
    app.commit_button_editor();
    assert!(app.unsaved_device_edits());
}

#[test]
fn closing_detects_unconfirmed_key_and_name_without_scheduling_a_write() {
    let (mut app, _) = fixture();
    app.open_button_editor(8, false, Action::Mouse(1));
    assert!(!app.has_unsaved_changes());
    app.button_editor.as_mut().unwrap().name = "登録前の名前".into();
    assert!(app.has_unsaved_changes());
    assert!(!app.has_pending_writes());
    let editor = app.button_editor.as_mut().unwrap();
    editor.name.clear();
    editor.key_usage = 9;
    assert!(app.has_unsaved_changes());
    assert!(!app.has_pending_writes());
}

#[test]
fn only_verified_same_device_keeps_cached_led_colors() {
    let (mut app, _) = fixture();
    app.led_memory.normal[0] = Some([255, 0, 0]);
    app.led_memory.shifted[0] = Some([0, 0, 255]);
    app.accept_binding(identity("A"), true, Vec::new());
    assert_eq!(app.led_memory.normal[0], Some([255, 0, 0]));
    assert_eq!(app.led_memory.shifted[0], Some([0, 0, 255]));
    app.accept_binding(identity("B"), false, Vec::new());
    assert_eq!(app.led_memory.normal, [None; 3]);
    assert_eq!(app.led_memory.shifted, [None; 3]);
}

#[test]
fn reloading_from_the_same_live_handle_keeps_the_first_backup_without_a_serial() {
    let (mut app, _) = fixture();
    let id = DeviceIdentity { serial: None, ..identity("A") };
    app.device_identity = Some(id.clone());
    app.session_backup = Some("original.txt".into());
    app.led_memory.normal[0] = Some([255, 0, 0]);
    let settings = fixture().0.settings.unwrap();
    app.handle(Response::Reload(Ok(DeviceSettings { continued_handle: true, ..binding(id, settings) })));
    assert_eq!(app.session_backup.as_deref(), Some("original.txt"));
    assert_eq!(app.led_memory.normal[0], Some([255, 0, 0]));
}

#[test]
fn confirm_close_and_keep_other_backup_edits_after_writing_one_profile() {
    let (mut app, _) = fixture();
    app.handle(Response::Open(Ok(("loaded.txt".into(), Some(fixture().0.settings.unwrap()), false))));
    app.edits[0][1] = 22;
    app.edits[1][1] = 35;
    assert!(app.has_unsaved_changes());
    assert!(!app.has_pending_writes());

    let mut settings = fixture().0.settings.unwrap();
    settings.profiles[0] = Profile::parse(&app.edits[0]).unwrap();
    app.handle(Response::Write(WriteKind::BackupProfile(0), Ok(("before-write.txt".into(), settings))));
    assert_eq!(app.backup_path.as_deref(), Some(std::path::Path::new("loaded.txt")));
    assert_eq!(app.edits[1][1], 35);
    assert_eq!(app.dirty(), vec![false, true, false]);
    assert!(app.has_unsaved_changes());

    // 全復元では元ファイルのプロファイル1が実機に戻り、プロファイル2の下書きだけ残る。
    let restored = fixture().0.settings.unwrap();
    let restored_profile = restored.profiles[0].raw.clone();
    app.handle(Response::Write(WriteKind::BackupFile, Ok(("before-restore.txt".into(), restored))));
    assert_eq!(app.edits[0], restored_profile);
    assert_eq!(app.edits[1][1], 35);
    assert_eq!(app.dirty(), vec![false, true, false]);
}

#[test]
fn backup_led_edits_do_not_change_remembered_live_colors() {
    let (mut app, _) = fixture();
    app.current_led_memory().normal[0] = Some([255, 0, 0]);
    app.current_led_memory().shifted[0] = Some([0, 255, 0]);
    app.handle(Response::Open(Ok(("loaded.txt".into(), Some(fixture().0.settings.unwrap()), false))));
    assert_eq!(app.current_led_memory().normal[0], None);
    app.current_led_memory().normal[0] = Some([0, 0, 255]);
    app.current_led_memory().shifted[0] = Some([0, 0, 255]);
    app.handle(Response::Reload(Ok(binding(identity("A"), fixture().0.settings.unwrap()))));
    assert_eq!(app.current_led_memory().normal[0], Some([255, 0, 0]));
    assert_eq!(app.current_led_memory().shifted[0], Some([0, 255, 0]));
}

#[test]
fn first_profile_switch_backup_remains_the_startup_restore_point() {
    let (mut app, _) = fixture();
    let mut switched = fixture().0.settings.unwrap();
    switched.active.profile = 1;
    app.handle(Response::Use(Ok((Some("before-first-switch.txt".into()), switched))));
    assert_eq!(app.tab, 1);
    assert_eq!(app.session_backup.as_deref(), Some("before-first-switch.txt"));

    app.handle(Response::Use(Err(("読み戻し失敗".into(), Some("later.txt".into())))));
    assert_eq!(app.session_backup.as_deref(), Some("before-first-switch.txt"));
}

#[test]
fn loaded_backup_waits_for_explicit_confirmation() {
    let (mut app, commands) = fixture();
    let open = || Response::Open(Ok(("loaded.mmo-backup".into(), fixture().0.settings, false)));
    app.handle(open());
    assert!(app.backup_write_ready());
    assert!(commands.try_recv().is_err());

    app.answer_backup_confirmation(false);
    assert!(commands.try_recv().is_err());
    assert!(app.backup_path.is_some());
    assert_eq!(app.last_backup_path.as_deref(), Some(std::path::Path::new("loaded.mmo-backup")));

    app.handle(open());
    app.device_identity = Some(identity("B"));
    app.answer_backup_confirmation(true);
    assert!(commands.try_recv().is_err());

    app.handle(open());
    app.answer_backup_confirmation(true);
    let Command::RestoreBackup(settings) = commands.try_recv().unwrap() else { panic!("expected restore"); };
    assert_eq!(settings.profiles.len(), 3);
}

#[test]
fn backup_dialog_buttons_choose_write_or_preview_and_disable_unavailable_write() {
    let _locale = LOCALE_LOCK.lock().unwrap();
    i18n::activate("ja");
    for (ready, label, expected) in [(true, "はい", Some(true)), (true, "いいえ", Some(false)), (false, "はい", None)] {
        let ctx = egui::Context::default();
        fonts::install(&ctx);
        let mut choice = None;
        let mut frame = |events| ctx.run(egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(850.0, 700.0))),
            events,
            ..Default::default()
        }, |ctx| {
            choice = choice.or(file_dialog::confirm_write(ctx, "backup-test", ready, false,
                "3つのプロファイルをマウスに書き込みます。"));
        });
        frame(vec![]);
        let layout = frame(vec![]);
        let pos = layout.shapes.iter().find_map(|shape| {
            let egui::Shape::Text(text) = &shape.shape else { return None; };
            (text.galley.job.text == label).then(|| text.pos + text.galley.size() * 0.5)
        }).unwrap();
        frame(vec![egui::Event::PointerMoved(pos)]);
        for pressed in [true, false] {
            frame(vec![egui::Event::PointerButton {
                pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE,
            }]);
        }
        assert_eq!(choice, expected, "{label}/{ready}");
    }
}

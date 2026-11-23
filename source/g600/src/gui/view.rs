//! 画面の描画（サンプル図 M1 / K5 / E1・配色 T2 に合わせる）。
//! 状態の更新や書き込みは mod.rs、色や部品の見た目は theme.rs。

use super::*;
use theme::{button, primary, toggle};

/// 名称列の幅（本文 14 のときの大きさ）
const NAME_W: f32 = 140.0;
/// 登録済みの組と組の間の、区切り線の前後の間隔
const SEP_SPACE: f32 = 10.0;
/// 「登録済みから選ぶ」の 1 ページの件数
const CUSTOM_PER_PAGE: usize = 15;

impl App {
    pub(super) fn body(&mut self, ui: &mut egui::Ui, syncing: bool) -> f32 {
        egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
            self.status_row(ui, syncing);
            match self.screen { Screen::Main => self.main_screen(ui, syncing), Screen::Keys => self.keys_screen(ui) }
        }).content_size.y
    }

    /// 上端の状態行: ● 接続済み [再読み込み] VID/PID 表示元 使用中
    pub(super) fn status_row(&mut self, ui: &mut egui::Ui, syncing: bool) {
        ui.horizontal_wrapped(|ui| {
            if self.connected { ui.label(egui::RichText::new(tr("● 接続済み")).strong().family(theme::bold_family()).color(theme::ACCENT)); }
            else { ui.label(egui::RichText::new(tr("● 未接続")).strong().family(theme::bold_family()).color(theme::ERROR)); }
            if ui.add_enabled(self.in_flight.is_none() && (!syncing || !self.auto_enabled),
                              button("再読み込み").small()).clicked() { self.request_reload(); }
            if !self.connected {
                if let Some(identity) = self.editing_device() {
                    if ui.add(button("再接続").small()).clicked() { self.send(Command::Connect(Some(identity))); }
                }
                if self.device_identity.is_some() && ui.add(button("接続中の実機を読み込む").small()).clicked() {
                    self.request_rebind();
                }
            }
            ui.label(theme::note(format!("VID/PID: {VENDOR_LOGITECH:04X}/{PRODUCT_G600:04X}")));
            if let Some(s) = &self.settings {
                ui.label(theme::note(format!("表示元: {}", self.source)));
                ui.label(tr(format!("使用中: プロファイル{} / DPIスロット{}", s.active.profile + 1, s.active.dpi_slot + 1)));
            }
            if ui.add(button("設定").small()).clicked() {
                self.settings_open = !self.settings_open;
            }
            if self.busy {
                ui.spinner().on_hover_text(tr(&self.message));
            }
        });
    }

    pub(super) fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open { return; }
        let mut open = self.settings_open;
        egui::Window::new(tr("設定")).id(egui::Id::new("app_settings")).open(&mut open).resizable(false).show(ctx, |ui| {
            ui.label(tr("言語"));
            let previous = self.language.clone();
            egui::ComboBox::from_id_salt("language").selected_text(i18n::LANGUAGES.iter().find(|(c, _)| *c == self.language).map(|(_, n)| tr(n)).unwrap_or_default()).show_ui(ui, |ui| {
                for (code, name) in i18n::LANGUAGES {
                    ui.selectable_value(&mut self.language, code.to_string(), tr(name));
                }
            });
            if previous != self.language {
                if let Err(error) = i18n::save(&self.language) { self.language = previous; self.message = error; }
            }
            ui.separator();
            ui.label(tr("保存先"));
            match g600ctl::storage::data_path() {
                Ok(path) => {
                    ui.label(path.to_string_lossy());
                    if ui.button(tr("フォルダを開く")).clicked() {
                        match g600ctl::storage::data_dir() {
                            Ok(path) => if let Err(e) = std::process::Command::new("explorer.exe").arg(path).spawn() { self.message = e.to_string(); },
                            Err(e) => self.message = e,
                        }
                    }
                }
                Err(e) => { ui.label(tr(e)); }
            }
            ui.separator();
            ui.label(tr("非公式のG600/G600r設定ツールです。"));
            ui.collapsing(tr("プライバシー"), |ui| {
                ui.set_max_width(theme::s(440.0));
                ui.label(tr("設定・バックアップ・診断ログはこのPCだけに保存し、外部へ送信しません。キー入力の検出は割り当て表示とキー登録に使用します。"));
            });
        });
        self.settings_open = open;
    }

    /// 状態の短い表示（右端に置く）: ✓ 反映しました（青）・失敗（赤）・反映中
    fn status_text(&self) -> Option<egui::RichText> {
        if self.sync_status.is_empty() { return None; }
        Some(if self.sync_status.starts_with("失敗") { egui::RichText::new(tr(&self.sync_status)).color(theme::ERROR) }
            else if self.sync_status.starts_with("反映しました") { egui::RichText::new(format!("✓ {}", tr(&self.sync_status))).color(theme::ACCENT) }
            else { egui::RichText::new(tr(&self.sync_status)).color(theme::MUTED) })
    }

    /// パネル「カーソル・ホイール」
    fn pointer_panel(&mut self, ui: &mut egui::Ui) {
        theme::panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            theme::header(ui, "カーソル・ホイール", Some("Windows の設定（このPCのすべてのマウス共通）"));
            let Some(mut p) = self.pointer else { if !self.pointer_message.is_empty() { ui.label(tr(&self.pointer_message)); } return; };
            let mut live = false;
            let mut commit = false;

            ui.horizontal(|ui| {
                let speed_label = tr("カーソルの速さ");
                let speed_hint = tr(format!("1〜20（標準 {}）", pointer::SPEED_DEFAULT));
                ui.add_sized(
                    [theme::s(120.0), ui.spacing().interact_size.y],
                    egui::Label::new(speed_label).truncate(),
                ).on_hover_text(speed_hint);

                let (changed, finished) = live_slider(
                    ui, &mut p.speed, pointer::SPEED_MIN..=pointer::SPEED_MAX,
                    false, 1.0, 46.0, checkbox_width(ui, "加速"),
                );
                live |= changed;
                commit |= finished;
                commit |= theme::chk(ui, &mut p.acceleration, "加速")
                    .on_hover_text(tr("速く動かすとカーソルが加速する（ポインターの精度を高める）"))
                    .changed();
            });

            let column_count = if ui.available_width() < 720.0 { 1 } else { 2 };
            ui.columns(column_count, |columns| {
                columns[0].horizontal(|ui| {
                    ui.label(tr("スクロール行数"));
                    let reserve = checkbox_width(ui, "1画面ずつ");
                    ui.add_enabled_ui(!p.wheel_page, |ui| {
                        let (changed, finished) = live_slider(
                            ui, &mut p.wheel_lines, pointer::LINES_MIN..=pointer::LINES_MAX,
                            true, 1.0, 46.0, reserve,
                        );
                        live |= changed;
                        commit |= finished;
                    });
                    commit |= theme::chk(ui, &mut p.wheel_page, "1画面ずつ").changed();
                });

                let second = column_count - 1;
                columns[second].horizontal(|ui| {
                    ui.label(tr("横スクロール（チルト）"))
                        .on_hover_text(tr(format!("1〜100 文字（標準 {}）", pointer::CHARS_DEFAULT)));
                    let (changed, finished) = live_slider(
                        ui, &mut p.wheel_chars, pointer::CHARS_MIN..=pointer::CHARS_MAX,
                        true, 1.0, 46.0, label_width(ui, "文字", egui::TextStyle::Small),
                    );
                    live |= changed;
                    commit |= finished;
                    ui.label(theme::note("文字"));
                });
            });
            if commit { self.set_pointer(p, true); } else if live { self.set_pointer(p, false); }
            if !self.pointer_message.is_empty() { ui.colored_label(theme::ERROR,tr( &self.pointer_message)); }
        });
    }

    /// プロファイルのタブ（右寄せにするかどうかは呼び出し側）
    fn tabs(&mut self, ui: &mut egui::Ui) {
        let Some((active, count)) = self.settings.as_ref().map(|s| (s.active.profile as usize, s.profiles.len())) else { return; };
        for i in 0..count {
            let mark = if active == i { "・使用中" } else { "" };
            if ui.add(toggle(format!("プロファイル{}{mark}", i + 1), self.tab == i)).clicked() { self.tab = i; self.button_editor = None; }
        }
    }

    /// 表示中プロファイルについての注意（必要なときだけ）
    fn profile_notice(&self, ui: &mut egui::Ui) {
        let Some(active) = self.settings.as_ref().map(|s| s.active.profile as usize) else { return; };
        if self.backup_path.is_some() {
            ui.colored_label(theme::WARN,tr( "バックアップファイルを表示中です。変更してもマウスには書き込みません（「マウスへ書き込む」を押したときだけ書き込みます）。"));
        } else if self.tab != active {
            ui.label(theme::note(format!("このプロファイルはマウスで使われていません（使用中はプロファイル{}）。変更はマウスに保存されますが、試すには「このプロファイルを使用」を押してください。", active + 1)));
        }
    }

    /// メイン画面 M1
    pub(super) fn main_screen(&mut self, ui: &mut egui::Ui, syncing: bool) {
        self.pointer_panel(ui);
        if self.settings.is_none() {
            ui.label(tr("表示する設定がありません。"));
            self.backup_panel(ui, syncing);
            self.message_lines(ui);
            return;
        }
        ui.horizontal_wrapped(|ui| self.tabs(ui));
        self.profile_notice(ui);
        let p = Profile::parse(&self.edits[self.tab]).unwrap();
        // パネル「LED・ポーリング」
        theme::panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            theme::header(ui, "LED・ポーリング", Some("マウス本体に保存"));
            ui.horizontal_wrapped(|ui| {
                let mut rgb = p.led_rgb; ui.label(tr("LED"));
                if theme::color_swatch(ui, "led_color", &mut rgb) { self.change(Change::Led(rgb, p.led_effect, p.led_seconds.clamp(1, 15))); }
                // 効果: 点灯 / 呼吸 / 色循環 / 消灯（消灯 = 色 000000 ＋ 点灯。消す前の色を覚えておき、別の効果で戻す）
                let current = edit::led_choice(p.led_rgb, p.led_effect);
                let mut choice = current;
                theme::input_scope(ui, |ui| egui::ComboBox::from_id_salt("effect").width(theme::s(80.0)).selected_text(tr(led_choice_name(current))).show_ui(ui, |ui| {
                    for value in [edit::LedChoice::Effect(LedEffect::Solid), edit::LedChoice::Effect(LedEffect::Breathe), edit::LedChoice::Effect(LedEffect::Cycle), edit::LedChoice::Off] {
                        ui.selectable_value(&mut choice, value,tr( led_choice_name(value)));
                    }
                }));
                if choice != current {
                    let slot = self.tab;
                    let memory = &mut self.current_led_memory().normal[slot];
                    if choice == edit::LedChoice::Off && p.led_rgb != [0, 0, 0] { *memory = Some(p.led_rgb); }
                    let (rgb, effect) = edit::apply_led_choice(choice, p.led_rgb, *memory);
                    self.change(Change::Led(rgb, effect, p.led_seconds.clamp(1, 15)));
                }
                let mut seconds = p.led_seconds.clamp(1, 15);
                if theme::input_scope(ui, |ui| ui.add_enabled(p.led_effect != LedEffect::Solid, egui::DragValue::new(&mut seconds).range(1..=15))).changed() { self.change(Change::Led(p.led_rgb, p.led_effect, seconds)); }
                ui.label(theme::note("秒"));
                ui.add_space(theme::s(16.0));
                let mut rgb = p.gshift_rgb; ui.label(tr("GシフトLED"));
                if theme::color_swatch(ui, "gshift_color", &mut rgb) { self.change(Change::GshiftLed(rgb)); }
                // 点灯 / 消灯（消灯 = 黒。消す前の色を覚えておき、点灯に戻すとその色。覚えていなければ白）
                let off = p.gshift_rgb == [0, 0, 0];
                let mut choose_off = off;
                theme::input_scope(ui, |ui| egui::ComboBox::from_id_salt("gshift_effect").width(theme::s(70.0)).selected_text(tr(if off { "消灯" } else { "点灯" })).show_ui(ui, |ui| {
                    ui.selectable_value(&mut choose_off, false,tr( "点灯"));
                    ui.selectable_value(&mut choose_off, true,tr( "消灯"));
                }));
                if choose_off != off {
                    let slot = self.tab;
                    let memory = &mut self.current_led_memory().shifted[slot];
                    if choose_off { *memory = Some(p.gshift_rgb); }
                    let choice = if choose_off { edit::LedChoice::Off } else { edit::LedChoice::Effect(LedEffect::Solid) };
                    let (rgb, _) = edit::apply_led_choice(choice, p.gshift_rgb, *memory);
                    self.change(Change::GshiftLed(rgb));
                }
            });
            ui.horizontal_wrapped(|ui| {
                let mut poll = p.poll_hz; ui.label(tr("ポーリング"));
                theme::input_scope(ui, |ui| egui::ComboBox::from_id_salt("poll").width(theme::s(90.0)).selected_text(tr(format!("{poll} Hz"))).show_ui(ui, |ui| { for rate in POLL_RATES { ui.selectable_value(&mut poll, rate,tr( format!("{rate} Hz"))); } }));
                if poll != p.poll_hz { self.change(Change::Poll(poll)); }
            });
        });
        // DPIスロットは2列、幅が狭い場合は1列に並べる。
        let mut slots = p.dpi; let mut default = p.dpi_default; let mut updated = false;
        theme::panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            theme::header(ui, "DPI", None);
            let columns = if ui.available_width() >= 500.0 { 2 } else { 1 };
            for row in 0..slots.len().div_ceil(columns) {
                ui.columns(columns, |cells| {
                    for (column, cell) in cells.iter_mut().enumerate() {
                        let i = row * columns + column;
                        theme::sub_box().fill(theme::BG).show(cell, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal_wrapped(|ui| {
                                ui.label(theme::bold(format!("DPI {}", i + 1)));
                                if i > 0 {
                                    let mut enabled = slots[i] != 0;
                                    if theme::chk(ui, &mut enabled, "有効").changed() {
                                        slots[i] = if enabled { 800 } else { 0 };
                                        updated = true;
                                    }
                                } else {
                                    ui.label(theme::note("常に有効"));
                                }
                                if theme::radio(ui, slots[i] != 0, default as usize == i + 1, "既定").clicked() {
                                    default = (i + 1) as u8;
                                    updated = true;
                                }
                            });
                            ui.horizontal(|ui| {
                                if slots[i] != 0 {
                                    let mut value = slots[i].clamp(DPI_MIN, DPI_MAX);
                                    if live_slider(ui, &mut value, DPI_MIN..=DPI_MAX, false, 50.0, 60.0, 0.0).0 {
                                        slots[i] = round_dpi(value);
                                        updated = true;
                                    }
                                } else {
                                    ui.add_sized(
                                        [theme::s(60.0), theme::s(24.0)],
                                        egui::Label::new(theme::note("無効")),
                                    );
                                }
                            });
                        });
                    }
                });
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(theme::bold("DPIシフト"));
                let mut enabled = p.dpi_shift != 0;
                if theme::chk(ui, &mut enabled, "有効").changed() { self.change(Change::DpiShift(if enabled { 800 } else { 0 })); }
                if enabled {
                    let mut value = p.dpi_shift.clamp(DPI_MIN, DPI_MAX);
                    if live_slider(ui, &mut value, DPI_MIN..=DPI_MAX, false, 50.0, 60.0, label_width(ui, "押している間だけ", egui::TextStyle::Small)).0 { self.change(Change::DpiShift(round_dpi(value))); }
                }
                ui.label(theme::note("押している間だけ"));
            });
            if ui.add(primary("キーコンフィグ（ボタンの割り当て）を開く ▶").min_size(egui::vec2(0.0, 28.0))).clicked() { self.screen = Screen::Keys; }
        });
        if updated { self.change(Change::Dpi(slots, default)); }
        self.backup_panel(ui, syncing);
        self.message_lines(ui);
    }

    fn backup_panel(&mut self, ui: &mut egui::Ui, syncing: bool) {
        theme::panel().show(ui, |ui| {
            ui.set_width(ui.available_width());
            theme::header(ui, "バックアップ", None);
            ui.horizontal(|ui| {
                if ui.add_enabled(self.connected && !syncing, button("保存先を選択")).clicked() {
                    self.request_backup_save();
                }
                if ui.add_enabled(self.in_flight.is_none(), button("設定を読み込む")).clicked() {
                    self.request_backup_open();
                }
                file_dialog::show_backup_path(ui, self.last_backup_path.as_deref());
            });
        });
        ui.horizontal_wrapped(|ui| {
            if self.settings.is_some() && self.backup_path.is_none() {
                if ui.add_enabled(self.connected && !syncing, button("このプロファイルを使用")).clicked() {
                    self.confirm = Some((format!("プロファイル{}に切り替えますか？", self.tab + 1), vec![], Command::Use { profile: self.tab as u8, backup: self.session_backup.is_none() }));
                }
            }
            if let (Some(path), Some(settings)) = (self.backup_path.clone(), &self.settings) {
                if ui.add_enabled(self.connected, button("このプロファイルをマウスへ書き込む")).clicked() {
                    let after = Profile::parse(&self.edits[self.tab]).unwrap();
                    let mut summary = vec![format!("バックアップ「{}」のプロファイル{}（画面で変えた内容を含む）をマウスへ書き込みます。", path.display(), self.tab + 1)];
                    summary.extend(diff_summary(&settings.profiles[self.tab], &after));
                    self.confirm = Some((format!("プロファイル{}をマウスへ書き込みますか？", self.tab + 1), summary, Command::Apply { profile: self.tab as u8, data: self.edits[self.tab].clone() }));
                }
                if ui.add_enabled(self.connected, button("3つともマウスへ書き込む")).clicked() {
                    self.backup_confirmation = Some((self.device_identity.clone(), false));
                }
                if ui.add_enabled(self.connected, button("実機の表示に戻る")).clicked() { self.request_reload(); }
            }
            let restore = self.session_backup.clone();
            if ui.add_enabled(self.connected && restore.is_some() && self.in_flight.is_none(), button("起動時の状態に戻す"))
                .on_disabled_hover_text(tr("このソフトでマウスに書き込むと使えるようになります")).clicked() {
                if let Some(path) = restore {
                    let mut summary = vec![format!("このソフトで最初に書き込む前に取ったバックアップ（{path}）から、3つのプロファイルを書き戻します。")];
                    if self.unsaved_device_edits() { summary.push("実機に反映していない編集を破棄します。".into()); }
                    self.confirm = Some(("起動時の状態に戻しますか？".into(), summary, Command::RestoreSession(path.into())));
                }
            }
            if !self.auto_enabled && ui.add_enabled(self.connected, button("自動反映を再開")).clicked() { self.resume_auto_apply(); }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(text) = self.status_text() { ui.add(egui::Label::new(text).truncate()); }
            });
        });
    }

    /// 処理の結果などの説明（薄い文字）
    fn message_lines(&self, ui: &mut egui::Ui) {
        if !self.message.is_empty() {
            let color = if self.message.contains("失敗") { theme::ERROR } else { theme::MUTED };
            ui.label(egui::RichText::new(tr(&self.message)).small().color(color));
        }
        if !self.custom_message.is_empty() { ui.label(theme::note(self.custom_message.clone())); }
    }

    /// キーコンフィグ画面 K5
    pub(super) fn keys_screen(&mut self, ui: &mut egui::Ui) {
        let can_open_editor = self.button_editor.is_none();
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(can_open_editor, button("◀ 戻る")).clicked() { self.screen = Screen::Main; }
            ui.label(egui::RichText::new(tr("キーコンフィグ")).strong().family(theme::bold_family()).size(theme::body_px() + 2.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // 右から並ぶので、逆順に置く
                let Some((active, count)) = self.settings.as_ref().map(|s| (s.active.profile as usize, s.profiles.len())) else { return; };
                for i in (0..count).rev() {
                    let mark = if active == i { "・使用中" } else { "" };
                    if ui.add_enabled(can_open_editor, toggle(format!("プロファイル{}{mark}", i + 1), self.tab == i)).clicked() { self.tab = i; }
                }
            });
        });
        if self.settings.is_none() { ui.label(tr("表示する設定がありません。")); return; }
        self.profile_notice(ui);
        let (lit, last_pressed, flashing) = self.button_presses.display(Instant::now());
        if flashing {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
        }
        let last_button = if last_pressed == 0 {
            tr("（まだありません）")
        } else {
            buttons::pressed_slots(last_pressed).iter()
                .map(|&slot| format!("{} {}", slot + 1, tr(button_label(slot))))
                .collect::<Vec<_>>().join(", ")
        };
        egui::Frame::new().fill(theme::PANEL).stroke(egui::Stroke::new(1.0_f32, theme::LINE))
            .corner_radius(egui::CornerRadius::same(6)).inner_margin(egui::Margin::symmetric(10, 4))
            .show(ui, |ui| {
                let width = ui.available_width();
                let height = theme::s(22.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    let physical = format!("{} {last_button}", tr("押したボタン:"));
                    let input = if self.last_sent.is_empty() {
                        String::new()
                    } else {
                        tr(format!("PC に届いた入力: {}", self.last_sent))
                    };
                    for (fraction, text, label) in [
                        (0.34, physical.clone(), theme::bold(&physical)),
                        (0.25, input.clone(), theme::note(&input)),
                        (0.41, tr("DPI切替・Gシフトなど PC に入力が届かないボタンも光ります"),
                            theme::note("DPI切替・Gシフトなど PC に入力が届かないボタンも光ります")),
                    ] {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(width * fraction, height), egui::Sense::hover());
                        text_cell(ui, rect.shrink2(egui::vec2(4.0, 0.0)), label, text);
                    }
                });
            });
        let p = Profile::parse(&self.edits[self.tab]).unwrap();
        let mut clicked = None;
        // 上面のボタン 1〜8 は 横 2 列 × 縦 4 段、サイドボタン 9〜20 は 横 3 列 × 縦 4 段（左上から右下へ順番）。
        // 並びは layout.rs の表で変えられる
        let top: Vec<Vec<usize>> = layout::TOP_ROWS.iter().map(|r| r.to_vec()).collect();
        let side: Vec<Vec<usize>> = layout::SIDE_ROWS.iter().map(|r| r.to_vec()).collect();
        for (title, note, rows) in [("上面のボタン", "1〜8・2列×4段", top), ("サイドボタン（親指側）", "9〜20・3列×4段", side)] {
            theme::panel().show(ui, |ui| {
                ui.set_width(ui.available_width());
                theme::header(ui, title, Some(note));
                let cols = rows[0].len() as f32;
                let gap = theme::s(12.0);
                let cell_w = ((ui.available_width() - (cols - 1.0) * gap) / cols).floor() - 1.0;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for _ in 0..rows[0].len() { column_header(ui, cell_w); }
                });
                for row in &rows {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for &slot in row {
                            if let Some(c) = button_row(ui, slot, cell_w, lit & (1 << slot) != 0, p.buttons[slot], p.gshift_buttons[slot], can_open_editor) { clicked = Some(c); }
                        }
                    });
                }
            });
        }
        if let Some((slot, shifted)) = clicked {
            let action = if shifted { p.gshift_buttons[slot] } else { p.buttons[slot] };
            self.open_button_editor(slot, shifted, action);
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(theme::note("割り当てのセルを押すと編集できます"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(text) = self.status_text() { ui.add(egui::Label::new(text).truncate()); }
            });
        });
        self.message_lines(ui);
    }

    /// 割り当て編集 E1（キーコンフィグ画面の上に開く）
    pub(super) fn editor_window(&mut self, ctx: &egui::Context) -> Option<egui::Rect> {
        if self.busy || self.confirm.is_some() { return None; }
        let editor = self.button_editor.as_mut()?;
        let mut open = true; let mut cancel = false; let mut commit = false; let mut save_custom = false; let mut remove_custom = None;
        let windows = edit::windows_shortcuts(); let chrome = edit::chrome_shortcuts();
        let custom = &mut self.custom; let keys = &self.keys;
        let title = format!("割り当て編集: {} {} / {}", editor.slot + 1, button_label(editor.slot), if editor.shifted { "Gシフト" } else { "通常" });
        let width = theme::s(720.0).min((ctx.screen_rect().width() - 40.0).max(300.0));
        let response = egui::Window::new(egui::RichText::new(tr(title)).strong().family(theme::bold_family()))
            .id(egui::Id::new("assignment_editor"))
            .vscroll(true).max_height((ctx.screen_rect().height() - 100.0).max(200.0))
            .open(&mut open).collapsible(false).resizable(false).default_width(width)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 50.0]).show(ctx, |ui| {
            ui.set_width(width);
            ui.horizontal_wrapped(|ui| {
                ui.label(theme::note("現在:")); ui.label(tr(combo_text(&editor.original)));
                ui.add_space(theme::s(16.0));
                ui.label(theme::note("選択中:")); ui.label(theme::bold(combo_text(&editor.action)));
            });
            // パネル: マウス / DPI・プロファイル
            theme::panel().show(ui, |ui| {
                ui.set_width(ui.available_width());
                theme::header(ui, "マウス", None);
                ui.horizontal_wrapped(|ui| {
                    for (action, label) in [(Action::Mouse(1), "左"), (Action::Mouse(2), "右"), (Action::Mouse(3), "中"), (Action::Mouse(4), "戻る"), (Action::Mouse(5), "進む"), (Action::None, "なし")] {
                        if ui.add(toggle(label, editor.action == action)).on_hover_text(tr(action.describe())).clicked() { editor.action = action; }
                    }
                });
                theme::header(ui, "DPI・プロファイル", None);
                ui.horizontal_wrapped(|ui| {
                    for (action, label) in [(Action::DpiUp, "DPI+"), (Action::DpiDown, "DPI−"), (Action::DpiCycle, "DPI循環"), (Action::DpiShift, "DPIシフト"), (Action::ProfileCycle, "プロファイル循環"), (Action::GShift, "Gシフト")] {
                        if ui.add(toggle(label, editor.action == action)).on_hover_text(tr(action.describe())).clicked() { editor.action = action; }
                    }
                });
            });
            // パネル: ショートカットキー（v0.9.0 と同じタブ切り替え。3 つのタブを横に並べ、選んだタブの中身だけ下に出す。
            // 選んでいるタブをもう一度押すと閉じる）
            theme::panel().show(ui, |ui| {
                ui.set_width(ui.available_width());
                theme::header(ui, "ショートカットキー", Some("G600 に保存できるのは「キー1つ＋修飾キー」まで"));
                ui.horizontal_wrapped(|ui| {
                    for (section, name) in [(layout::Section::Windows, "Windows ショートカット"), (layout::Section::Chrome, "Chrome ショートカット"), (layout::Section::Custom, "自分で登録")] {
                        let is_open = editor.open_section == Some(section);
                        if ui.add(toggle(format!("{} {name}", if is_open { "▼" } else { "▶" }), is_open)).clicked() {
                            editor.open_section = layout::toggle_section(editor.open_section, section);
                            editor.list_page = 0;
                        }
                    }
                });
                let Some(section) = editor.open_section else { return; };
                // タブを切り替えても高さがガタつかないよう、中身の高さを測り、これまでの最大に足りない分だけ空ける
                let h = ui.vertical(|ui| {
                match section {
                    layout::Section::Windows | layout::Section::Chrome => {
                        let list = if section == layout::Section::Windows { &windows } else { &chrome };
                        let (s, e, pages, page) = layout::page_range(list.len(), editor.list_page, 16);
                        editor.list_page = page;
                        ui.horizontal_wrapped(|ui| for (name, action) in &list[s..e] {
                            if ui.add(toggle(*name, editor.action == *action).small()).on_hover_text(tr(combo_text(action))).clicked() { editor.action = *action; }
                        });
                        pager(ui, &mut editor.list_page, pages);
                    }
                    layout::Section::Custom => custom_section(ui, editor, custom, keys, &mut save_custom, &mut remove_custom),
                }
                }).response.rect.height();
                editor.tab_height = editor.tab_height.max(h);
                if h < editor.tab_height { ui.add_space(editor.tab_height - h); }
            });
            if !editor.notice.is_empty() { ui.label(theme::note(editor.notice.clone())); }
            let overwrite = editor.conflict && editor.action != editor.original;
            if overwrite { ui.label(tr("このボタンの割り当てが変更されました。上書きする前に現在の割り当てを確認してください。")); }
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label(theme::note("選択中:")); ui.label(theme::bold(combo_text(&editor.action)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if overwrite { "上書き" } else { "変更" };
                    if ui.push_id(overwrite, |ui| ui.add(primary(label).min_size(egui::vec2(theme::s(90.0), theme::s(30.0))))).inner.clicked() { commit = true; }
                    if ui.add(button("キャンセル").min_size(egui::vec2(theme::s(90.0), theme::s(30.0)))).clicked() { cancel = true; }
                });
            });
        });
        let rect = response.map(|r| r.response.rect);
        if let Some(i) = remove_custom { self.custom.remove(i); save_custom = true; }
        if save_custom {
            self.custom_message = match &self.custom_path {
                Some(path) => match g600ctl::storage::write_atomic(path, edit::format_custom(&self.custom).as_bytes()) {
                    Ok(()) => format!("自分で登録した組み合わせを保存しました（{}）", path.display()), Err(e) => format!("保存できません: {e}"),
                },
                None => "保存先が見つかりません".into(),
            };
        }
        if commit { self.commit_button_editor(); }
        else if !open || cancel { self.button_editor = None; }
        rect
    }
}

/// 「自分で登録」の中: 1. 組み合わせを作る → 2. 名前を付けて登録 → 3. 登録済みから選ぶ
fn custom_section(ui: &mut egui::Ui, editor: &mut ButtonEditor, custom: &mut Vec<(String, Action)>, keys: &[(u8, String)], save_custom: &mut bool, remove_custom: &mut Option<usize>) {
    ui.label(theme::step("1. 組み合わせを作る"));
    ui.horizontal_wrapped(|ui| {
        if ui.add(toggle(if editor.capture { "キーを押してください（もう一度押すと中止）" } else { "キーを押して登録" }, editor.capture)).clicked() { editor.capture = !editor.capture; }
        ui.label(theme::note("押したキーと修飾キーをそのまま取り込みます"));
    });
    for names in [["左Ctrl", "左Shift", "左Alt", "左Win"], ["右Ctrl", "右Shift", "右Alt", "右Win"]] {
        ui.horizontal_wrapped(|ui| for name in names {
            let bit = ["左Ctrl", "左Shift", "左Alt", "左Win", "右Ctrl", "右Shift", "右Alt", "右Win"].iter().position(|n| *n == name).unwrap();
            let mut on = editor.key_mods & (1 << bit) != 0;
            if theme::chk(ui, &mut on, name).changed() { if on { editor.key_mods |= 1 << bit; } else { editor.key_mods &= !(1 << bit); } }
        });
    }
    ui.horizontal_wrapped(|ui| {
        ui.label(tr("キーを検索"));
        if ui.add(egui::TextEdit::singleline(&mut editor.search).desired_width(theme::s(160.0)).hint_text(tr("例: F1、Enter、テンキー"))).changed() { editor.cand_page = 0; }
        let usage = &mut editor.key_usage;
        theme::input_scope(ui, |ui| egui::ComboBox::from_id_salt("key").width(theme::s(170.0)).selected_text(tr(if *usage == 0 { "キーなし（修飾キーだけ）".into() } else { key_name(*usage) })).show_ui(ui, |ui| {
            ui.selectable_value(usage, 0,tr( "キーなし（修飾キーだけ）"));
            for (value, name) in keys { ui.selectable_value(usage, *value,tr( name)); }
        }));
    });
    if !editor.search.trim().is_empty() {
        let found = edit::search_keys(&editor.search, keys);
        let (s, e, pages, page) = layout::page_range(found.len(), editor.cand_page, 12);
        editor.cand_page = page;
        ui.horizontal_wrapped(|ui| {
            ui.label(theme::note("候補:"));
            if found.is_empty() { ui.label(tr("見つかりません")); }
            for (usage, name) in &found[s..e] {
                let b = if editor.key_usage == *usage { primary(name.as_str()).small() } else { button(name.as_str()).small() };
                if ui.add(b).clicked() { editor.key_usage = *usage; } // 下書きに入れるだけ。保存は「登録」を押したとき
            }
            if pages > 1 { pager(ui, &mut editor.cand_page, pages); }
        });
    }
    let combo = if editor.key_usage != 0 || editor.key_mods != 0 { Some(Action::Key { modifiers: editor.key_mods, usage: editor.key_usage }) } else { None };
    // 名前欄: 空か自動の名前のままなら、毎フレーム登録内容で書き替える
    let auto = combo.map_or(String::new(), |a| combo_text(&a));
    editor.name = edit::auto_name(&editor.name, &editor.last_auto_name, &auto);
    editor.last_auto_name = auto;
    ui.label(theme::step("2. 名前を付けて登録"));
    ui.horizontal_wrapped(|ui| {
        ui.label(tr("名前"));
        ui.add(egui::TextEdit::singleline(&mut editor.name).desired_width(theme::s(220.0)));
        let can = combo.is_some() && !editor.name.trim().is_empty();
        if ui.add_enabled(can, primary("登録")).clicked() {
            if editor.name.contains(['\t', '\r', '\n']) { editor.notice = "名前にタブ・改行は使えません。".into(); }
            else if let Some(action) = combo {
                let (index, added) = edit::register_custom(custom, &editor.name, action);
                *save_custom |= added;
                editor.action = action;
                editor.key_baseline = (editor.key_mods, editor.key_usage);
                editor.registered_name.clone_from(&editor.name);
                editor.custom_page = index / CUSTOM_PER_PAGE;
                editor.notice = if added { "登録して選択しました。下の「変更」でこのボタンに割り当てます。" } else { "同じ組み合わせが登録済みなので、それを選びました。" }.into();
            }
        }
        ui.label(theme::note(format!("登録される内容: {}", combo.map_or("（未選択）".to_string(), |a| combo_text(&a)))));
    });
    ui.label(theme::step("3. 登録済みから選ぶ"));
    if custom.is_empty() { ui.label(theme::note("まだ登録はありません")); }
    // 各項目は「[名前のボタン] 登録内容（薄い文字）[削除]」の 1 組。Chrome の一覧と同じく中身の長さに合わせた幅で
    // 左から詰めて並べ、入らなくなったら次の行へ。組の途中で折り返さないよう、幅を先に測って行を決める（手動の折り返し）
    let (s, e, pages, page) = layout::page_range(custom.len(), editor.custom_page, CUSTOM_PER_PAGE);
    editor.custom_page = page;
    let small = egui::FontId::proportional(theme::body_px() - 1.0);
    let pad = ui.spacing().button_padding.x * 2.0 + 2.0; // ボタンの左右の余白と枠
    let inner = 6.0; // 組の中の間隔
    // 組と組の間: 間隔 SEP_SPACE ＋ 縦の区切り線（1px）＋ 間隔 SEP_SPACE。行の先頭と末尾には入れない
    let gap = layout::separator_gap(SEP_SPACE);
    let measure = |text: &str, font: &egui::FontId| ui.fonts(|f| f.layout_no_wrap(text.to_string(), font.clone(), theme::TEXT).size().x);
    let bold_small = egui::FontId::new(theme::body_px() - 1.0, theme::bold_family());
    let widths: Vec<f32> = (s..e).map(|i| {
        let (name, action) = &custom[i];
        let name_font = if editor.action == *action { &bold_small } else { &small };
        measure(name, name_font) + pad + inner + measure(&combo_text(action), &small) + inner + measure(&tr("削除"), &small) + pad
    }).collect();
    for row in layout::pack_rows(&widths, ui.available_width(), gap) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = inner;
            for (n, k) in row.iter().enumerate() {
                if n > 0 {
                    // 同じ行の組と組の間だけに、ボタンと同じくらいの高さの縦線（#40516B・1px）
                    ui.add_space(SEP_SPACE - inner);
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, theme::s(22.0)), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 0.0, theme::LINE);
                    ui.add_space(SEP_SPACE - inner);
                }
                let i = s + *k;
                let (name, action) = &custom[i];
                if ui.add(theme::toggle_raw(name, editor.action == *action).small()).clicked() { editor.action = *action; }
                ui.label(theme::note(combo_text(action)));
                if ui.add(button("削除").small()).clicked() { *remove_custom = Some(i); }
            }
        });
    }
    if pages > 1 { pager(ui, &mut editor.custom_page, pages); }
}

fn label_width(ui: &egui::Ui, label: &str, style: egui::TextStyle) -> f32 {
    let font = style.resolve(ui.style());
    ui.painter().layout_no_wrap(tr(label), font, egui::Color32::WHITE).size().x
}

fn checkbox_width(ui: &egui::Ui, label: &str) -> f32 {
    (label_width(ui, label, egui::TextStyle::Button) + ui.spacing().icon_width + ui.spacing().icon_spacing)
        .max(ui.spacing().interact_size.y)
}

/// 枠線や文字幅の変化が隣のセルに影響しないよう、配置を先に確保する。
fn text_cell(ui: &mut egui::Ui, rect: egui::Rect, label: egui::RichText, tooltip: String) {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect)
        .layout(egui::Layout::left_to_right(egui::Align::Center)));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    child.add(egui::Label::new(label).truncate()).on_hover_text(tooltip);
}

fn button_columns(ui: &mut egui::Ui, cell_w: f32, height: f32) -> (egui::Rect, [egui::Rect; 3]) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(cell_w, height), egui::Sense::hover());
    let name_w = theme::s(NAME_W).min(cell_w * 0.42);
    let gutter = 4.0;
    let assign_w = (cell_w - name_w - 2.0 * gutter) / 2.0;
    let widths = [name_w, assign_w, assign_w];
    let offsets = [0.0, name_w + gutter, name_w + assign_w + 2.0 * gutter];
    let columns = std::array::from_fn(|i| egui::Rect::from_min_size(
        rect.min + egui::vec2(offsets[i], 0.0),
        egui::vec2(widths[i], height),
    ));
    (rect, columns)
}

/// 各マスの上の小さな見出し「番号・名称 / 通常 / Gシフト」
fn column_header(ui: &mut egui::Ui, cell_w: f32) {
    let (_, columns) = button_columns(ui, cell_w, theme::s(18.0));
    for (rect, text) in columns.into_iter().zip(["番号・名称", "通常", "Gシフト"]) {
        text_cell(ui, rect.shrink2(egui::vec2(6.0, 0.0)), theme::note(text), tr(text));
    }
}

/// 1マスに番号・名称、通常、Gシフトを配置し、押された割り当てを返す。
fn button_row(ui: &mut egui::Ui, slot: usize, cell_w: f32, lit: bool, normal: Action, shifted: Action, editable: bool) -> Option<(usize, bool)> {
    let (row, [name_rect, normal_rect, shifted_rect]) = button_columns(ui, cell_w, theme::s(30.0));
    let fg = if lit { theme::HOT_FG } else { theme::TEXT };
    if lit {
        ui.painter().rect_filled(name_rect, 6, theme::HOT_BG);
        ui.painter().rect_stroke(name_rect, 6, egui::Stroke::new(2.0_f32, fg), egui::StrokeKind::Inside);
    }
    let name = format!("{} {}", slot + 1, tr(button_label(slot)));
    text_cell(ui, name_rect.shrink2(egui::vec2(6.0, 0.0)), theme::bold(&name).color(fg), name.clone());

    let mut clicked = None;
    for (is_shift, action, rect) in [(false, normal, normal_rect), (true, shifted, shifted_rect)] {
        let rect = egui::Rect::from_center_size(rect.center(), egui::vec2(rect.width(), theme::s(26.0)));
        let mut child = ui.new_child(egui::UiBuilder::new().id_salt((slot, is_shift)).max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)));
        if !editable { child.disable(); }
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        let short = layout::ellipsize(&combo_text(&action), ((rect.width() / (theme::body_px() * 0.68)) as usize).max(4));
        if child.add_sized(rect.size(), button(short).truncate()).on_hover_text(tr(action.describe())).clicked() {
            clicked = Some((slot, is_shift));
        }
    }
    ui.painter().hline(row.x_range(), row.bottom() + 2.0, egui::Stroke::new(1.0_f32, theme::LINE));
    clicked
}

/// 「前へ 1/3 次へ」
fn pager(ui: &mut egui::Ui, page: &mut usize, pages: usize) {
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(*page > 0, button("前へ").small()).clicked() { *page -= 1; }
        ui.label(theme::note(format!("{}/{}", *page + 1, pages)));
        if ui.add_enabled(*page + 1 < pages, button("次へ").small()).clicked() { *page += 1; }
    });
}

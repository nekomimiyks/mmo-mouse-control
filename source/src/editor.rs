//! G600rと共通の割当編集。16件のプリセット表示と、検索・修飾キー・登録済み一覧。
use super::*;
use m908ctl::{actions, protocol::KEY_CODES};

fn page_range(len: usize, page: usize, per: usize) -> (usize, usize, usize, usize) {
    let pages = len.div_ceil(per).max(1);
    let page = page.min(pages - 1);
    let start = page * per;
    (start, (start + per).min(len), pages, page)
}
fn same_action(a: &str, b: &str) -> bool {
    matches!((actions::encode(a),actions::encode(b)),(Ok(a),Ok(b)) if a==b)
}
fn display_key(key: &str) -> String {
    match key {
        "Return" => "Enter".into(),
        "Num_Return" => "NumEnter".into(),
        "PrtSc" => "PrintScreen".into(),
        "ScrLk" => "ScrollLock".into(),
        "Caps_Lock" => "CapsLock".into(),
        "PgUp" => "PageUp".into(),
        "PgDown" => "PageDown".into(),
        "Minus" => "-".into(),
        "Equals" => "=".into(),
        "Comma" => ",".into(),
        "Period" => ".".into(),
        "Slash" => "/".into(),
        "Backslash" => "\\".into(),
        "Semicolon" => ";".into(),
        "Apostrophe" => "'".into(),
        "Tilde" => "`".into(),
        "Bracket_l" => "[".into(),
        "Bracket_r" => "]".into(),
        "Hash" => "#".into(),
        _ if key.len() == 1 => key.to_ascii_uppercase(),
        _ => key.replace("Num_", "Num"),
    }
}
pub(super) fn key_parts(action: &str) -> (u8, String) {
    if let Ok(code) = actions::encode(action) {
        if matches!(code[0], 0x90 | 0x8f) {
            if let Some((key, _)) = KEY_CODES.iter().find(|(_, v)| *v == code[2]) {
                return (code[1], display_key(key));
            }
        }
    }
    (0, String::new())
}
fn combination(modifiers: u8, key: &str) -> Option<String> {
    if key.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    for (bit, name) in [
        (1, "Ctrl"),
        (2, "Shift"),
        (4, "Alt"),
        (8, "Win"),
        (16, "Ctrl_R"),
        (32, "Shift_R"),
        (64, "Alt_R"),
        (128, "Win_R"),
    ] {
        if modifiers & bit != 0 {
            parts.push(name.to_string());
        }
    }
    parts.push(key.to_string());
    let action = parts.join(" + ");
    actions::encode(&action).ok().map(|_| action)
}
fn key_matches(query: &str, name: &str) -> bool {
    let q = query.trim().to_lowercase();
    let lower = name.to_lowercase();
    let by_name = if q.starts_with('f')
        && q.len() > 1
        && q[1..].chars().all(|c| c.is_ascii_digit())
        && lower.starts_with('f')
    {
        lower == q
    } else {
        lower.contains(&q)
    };
    by_name
        || match q.as_str() {
            "テンキー" | "てんきー" | "数字パッド" => name.starts_with("Num"),
            "エンター" | "改行" | "えんたー" => name == "Enter" || name == "NumEnter",
            "矢印" | "やじるし" | "カーソルキー" => {
                matches!(name, "Up" | "Down" | "Left" | "Right")
            }
            "上" | "うえ" => name == "Up",
            "下" | "した" => name == "Down",
            "左" | "ひだり" => name == "Left",
            "右" | "みぎ" => name == "Right",
            "スペース" | "空白" => name == "Space",
            "タブ" => name == "Tab",
            "エスケープ" | "エスケ" => name == "Esc",
            "バックスペース" | "後退" => name == "Backspace",
            "削除" | "デリート" => name == "Delete",
            "挿入" => name == "Insert",
            "ホーム" | "エンド" | "先頭" | "末尾" => name == "Home" || name == "End",
            "ページ" => name == "PageUp" || name == "PageDown",
            "ファンクション" | "ふぁんくしょん" => {
                name.starts_with('F') && name[1..].chars().all(|c| c.is_ascii_digit())
            }
            "プリントスクリーン" | "スクショ" | "画面印刷" => {
                name == "PrintScreen"
            }
            "数字" => name.len() == 1 && name.chars().all(|c| c.is_ascii_digit()),
            _ => false,
        }
}
fn pager(ui: &mut egui::Ui, pal: Palette, page: &mut usize, pages: usize) {
    ui.horizontal(|ui| {
        if ui
            .add_enabled(*page > 0, egui::Button::new("前へ").small())
            .clicked()
        {
            *page -= 1;
        }
        ui.label(note(format!("{}/{}", *page + 1, pages), pal));
        if ui
            .add_enabled(*page + 1 < pages, egui::Button::new("次へ").small())
            .clicked()
        {
            *page += 1;
        }
    });
}
fn packed_rows(widths: &[f32], available: f32, gap: f32) -> Vec<Vec<usize>> {
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut used = 0.0;
    for (i, width) in widths.iter().enumerate() {
        if let Some(row) = rows.last_mut() {
            if used + gap + width <= available {
                row.push(i);
                used += gap + width;
                continue;
            }
        }
        rows.push(vec![i]);
        used = *width;
    }
    rows
}
impl App {
    fn persist_custom(&mut self, entries: Vec<(String, String)>) -> Result<(), String> {
        if !self.preview() {
            let text = entries
                .iter()
                .map(|(n, a)| format!("{n}\t{a}\n"))
                .collect::<String>();
            std::fs::write(&self.custom_path, text).map_err(|e| e.to_string())?;
        }
        self.custom = entries;
        Ok(())
    }
    fn custom_editor(&mut self, ui: &mut egui::Ui, editor: &mut Editor) {
        let pal = self.pal;
        heading(ui, pal, "1. 組み合わせを作る", "");
        ui.horizontal_wrapped(|ui| {
            if ui
                .add(toggle(
                    pal,
                    if editor.capture {
                        "キーを押してください（Escで中止）"
                    } else {
                        "キーを押して登録"
                    },
                    editor.capture,
                ))
                .clicked()
            {
                editor.capture = !editor.capture;
            }
            ui.label(note("押したキーと修飾キーをそのまま取り込みます", pal));
        });
        for row in 0..2 {
            ui.horizontal_wrapped(|ui| {
                for i in 0..4 {
                    let bit = row * 4 + i;
                    let name = [
                        "左Ctrl", "左Shift", "左Alt", "左Win", "右Ctrl", "右Shift", "右Alt",
                        "右Win",
                    ][bit];
                    let mut value = editor.modifiers & (1 << bit) != 0;
                    if check(ui, pal, &mut value, name).changed() {
                        if value {
                            editor.modifiers |= 1 << bit;
                        } else {
                            editor.modifiers &= !(1 << bit);
                        }
                    }
                }
            });
        }
        let keys: Vec<String> = KEY_CODES
            .iter()
            .filter(|(_, usage)| *usage < 0xe0)
            .map(|(name, _)| display_key(name))
            .collect();
        ui.horizontal_wrapped(|ui| {
            ui.label("キーを検索");
            if ui
                .add(
                    egui::TextEdit::singleline(&mut editor.search)
                        .desired_width(theme::s(160.0))
                        .hint_text("例: F1、Enter、テンキー"),
                )
                .changed()
            {
                editor.candidate_page = 0;
            }
            egui::ComboBox::from_id_salt("editor_key")
                .width(theme::s(170.0))
                .selected_text(if editor.key.is_empty() {
                    "キーを選択"
                } else {
                    &editor.key
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut editor.key, String::new(), "キーを選択");
                    for name in &keys {
                        ui.selectable_value(&mut editor.key, name.clone(), name);
                    }
                });
        });
        if !editor.search.trim().is_empty() {
            let found: Vec<_> = keys
                .iter()
                .filter(|name| key_matches(&editor.search, name))
                .collect();
            let (s, e, pages, page) = page_range(found.len(), editor.candidate_page, 12);
            editor.candidate_page = page;
            ui.horizontal_wrapped(|ui| {
                ui.label(note("候補:", pal));
                if found.is_empty() {
                    ui.label("見つかりません");
                }
                for name in &found[s..e] {
                    if ui
                        .add(
                            toggle(pal, (**name).clone(), editor.key.as_str() == name.as_str())
                                .small(),
                        )
                        .clicked()
                    {
                        editor.key = (**name).clone();
                    }
                }
            });
            if pages > 1 {
                pager(ui, pal, &mut editor.candidate_page, pages);
            }
        }
        let combo = combination(editor.modifiers, &editor.key);
        let auto = combo.clone().unwrap_or_default();
        if editor.name.trim().is_empty() || editor.name == editor.last_auto_name {
            editor.name = auto.clone();
        }
        editor.last_auto_name = auto;
        heading(ui, pal, "2. 名前を付けて登録", "");
        ui.horizontal_wrapped(|ui|{
            ui.label("名前");ui.add(egui::TextEdit::singleline(&mut editor.name).desired_width(theme::s(220.0)));
            if ui.add_enabled(combo.is_some()&&!editor.name.trim().is_empty(),pal.primary("登録")).clicked(){
                let name=editor.name.trim();
                if name.contains(['\t','\n','\r'])||name.len()>1024{editor.notice="名前にはタブ・改行を使わず、1024バイト以内にしてください。".into();}
                else if let Some(action)=&combo{
                    if let Some(i)=self.custom.iter().position(|(_,a)|same_action(a,action)){editor.action=self.custom[i].1.clone();editor.custom_page=i/15;editor.notice="同じ組み合わせが登録済みなので、それを選びました。".into();}
                    else {let mut updated=self.custom.clone();updated.push((name.into(),action.clone()));let index=updated.len()-1;match self.persist_custom(updated){Ok(())=>{editor.action=action.clone();editor.custom_page=index/15;editor.notice="登録して選択しました。下の「変更」でこのボタンに割り当てます。".into();},Err(e)=>editor.notice=format!("登録を保存できません: {e}")}}
                }
            }
            ui.label(note(format!("登録される内容: {}",combo.as_deref().unwrap_or("（未選択）")),pal));
        });
        heading(ui, pal, "3. 登録済みから選ぶ", "");
        if self.custom.is_empty() {
            ui.label(note("まだ登録はありません", pal));
        }
        let (s, e, pages, page) = page_range(self.custom.len(), editor.custom_page, 15);
        editor.custom_page = page;
        let font = egui::FontId::proportional(theme::body_px() - 1.0);
        let padding = ui.spacing().button_padding.x * 2.0 + 2.0;
        let widths: Vec<f32> = self.custom[s..e]
            .iter()
            .map(|(name, action)| {
                ui.fonts(|f| {
                    f.layout_no_wrap(format!("{name}{action}削除"), font.clone(), pal.text)
                        .size()
                        .x
                }) + padding * 2.0
                    + 12.0
            })
            .collect();
        let mut remove = None;
        for row in packed_rows(&widths, ui.available_width(), 21.0) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (n, index) in row.iter().enumerate() {
                    if n > 0 {
                        ui.add_space(4.0);
                        ui.separator();
                        ui.add_space(4.0);
                    }
                    let i = s + index;
                    let (name, action) = &self.custom[i];
                    if ui
                        .add(toggle(pal, name, same_action(&editor.action, action)).small())
                        .clicked()
                    {
                        editor.action = action.clone();
                    }
                    ui.label(note(action, pal));
                    if ui.small_button("削除").clicked() {
                        remove = Some(i);
                    }
                }
            });
        }
        if let Some(i) = remove {
            let mut updated = self.custom.clone();
            updated.remove(i);
            match self.persist_custom(updated) {
                Ok(()) => editor.notice = "登録を削除しました。".into(),
                Err(e) => editor.notice = format!("削除を保存できません: {e}"),
            }
        }
        if pages > 1 {
            pager(ui, pal, &mut editor.custom_page, pages);
        }
    }
    pub(super) fn editor_window(&mut self, ctx: &egui::Context) {
        if self.job.is_some() || (self.identification.is_some() && !self.identifying) {
            return;
        }
        let Some(mut editor) = self.editor.take() else {
            return;
        };
        let pal = self.pal;
        let mut open = true;
        let (mut commit, mut cancel) = (false, false);
        if editor.capture && !self.marker_frame {
            for event in ctx.input(|i| i.events.clone()) {
                if let egui::Event::Key {
                    key,
                    physical_key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                {
                    if key != egui::Key::Escape {
                        editor.modifiers = (if modifiers.ctrl { 1 } else { 0 })
                            | (if modifiers.shift { 2 } else { 0 })
                            | (if modifiers.alt { 4 } else { 0 })
                            | (if modifiers.command && !modifiers.ctrl {
                                8
                            } else {
                                0
                            });
                        let value = physical_key.unwrap_or(key).name();
                        editor.key = match value {
                            "Comma" => "Comma",
                            "Period" => "Period",
                            "Slash" => "Slash",
                            "Backslash" => "Backslash",
                            "OpenBracket" => "Bracket_l",
                            "CloseBracket" => "Bracket_r",
                            "Backtick" => "Tilde",
                            "Quote" => "Apostrophe",
                            _ => value,
                        }
                        .to_string();
                    }
                    editor.capture = false;
                }
            }
        }
        let width = theme::s(720.0).min((ctx.screen_rect().width() - 48.0).max(300.0));
        let maximum = ctx
            .input(|i| i.viewport().monitor_size)
            .map_or(900.0, |m| m.y - 160.0)
            .max(200.0);
        let response = egui::Window::new(bold(format!(
            "割り当て編集: {} {} / 通常",
            editor.slot + 1,
            BUTTONS[editor.slot]
        )))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .min_height(editor.content_height.min(maximum))
        .constrain(false)
        .default_width(width)
        .anchor(egui::Align2::CENTER_TOP, [0.0, 50.0])
        .show(ctx, |ui| {
            ui.set_width(width);
            let scroll = egui::ScrollArea::vertical()
                .id_salt("assignment_scroll")
                .max_height(maximum)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(note("現在:", pal));
                        ui.label(actions::display(&editor.original));
                        ui.add_space(theme::s(16.0));
                        ui.label(note("選択中:", pal));
                        ui.label(bold(actions::display(&editor.action)));
                    });
                    pal.card().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        heading(ui, pal, "マウス", "");
                        ui.horizontal_wrapped(|ui| {
                            for (name, action) in [
                                ("左", "左クリック"),
                                ("右", "右クリック"),
                                ("中", "ホイール押下"),
                                ("戻る", "戻る"),
                                ("進む", "進む"),
                                ("なし", "無効"),
                            ] {
                                if ui.add(toggle(pal, name, editor.action == action)).clicked() {
                                    editor.action = action.into();
                                }
                            }
                        });
                        heading(ui, pal, "DPI・プロファイル", "");
                        ui.horizontal_wrapped(|ui| {
                            for (name, action) in [
                                ("DPI+", "DPI ＋"),
                                ("DPI−", "DPI －"),
                                ("DPI循環", "DPI 切替"),
                                ("プロファイル循環", "プロファイル切替"),
                            ] {
                                if ui.add(toggle(pal, name, editor.action == action)).clicked() {
                                    editor.action = action.into();
                                }
                            }
                        });
                    });
                    pal.card().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        heading(
                            ui,
                            pal,
                            "ショートカットキー",
                            "M908 に保存できるのは「キー1つ＋修飾キー」まで",
                        );
                        ui.horizontal_wrapped(|ui| {
                            for (i, title) in [
                                "Windows ショートカット",
                                "Chrome ショートカット",
                                "自分で登録",
                            ]
                            .iter()
                            .enumerate()
                            {
                                let on = editor.tab == Some(i);
                                if ui
                                    .add(toggle(
                                        pal,
                                        format!("{} {title}", if on { "▼" } else { "▶" }),
                                        on,
                                    ))
                                    .clicked()
                                {
                                    editor.tab = if on { None } else { Some(i) };
                                    editor.list_page = 0;
                                    editor.capture = false;
                                }
                            }
                        });
                        let height = ui
                            .vertical(|ui| match editor.tab {
                                Some(0) | Some(1) => {
                                    let list = if editor.tab == Some(0) {
                                        WINDOWS
                                    } else {
                                        CHROME
                                    };
                                    let (s, e, pages, page) =
                                        page_range(list.len(), editor.list_page, 16);
                                    editor.list_page = page;
                                    ui.horizontal_wrapped(|ui| {
                                        for (name, action) in &list[s..e] {
                                            if ui
                                                .add(
                                                    toggle(
                                                        pal,
                                                        *name,
                                                        same_action(&editor.action, action),
                                                    )
                                                    .small(),
                                                )
                                                .on_hover_text(*action)
                                                .clicked()
                                            {
                                                let family = if editor.tab == Some(0) {
                                                    "Windows"
                                                } else {
                                                    "Chrome"
                                                };
                                                editor.action = format!("{family}: {name}");
                                            }
                                        }
                                    });
                                    pager(ui, pal, &mut editor.list_page, pages);
                                }
                                Some(2) => self.custom_editor(ui, &mut editor),
                                _ => {}
                            })
                            .response
                            .rect
                            .height();
                        editor.tab_height = editor.tab_height.max(height);
                        if height < editor.tab_height {
                            ui.add_space(editor.tab_height - height);
                        }
                    });
                    if !editor.notice.is_empty() {
                        ui.label(note(&editor.notice, pal));
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(note("選択中:", pal));
                        ui.label(bold(actions::display(&editor.action)));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_enabled(
                                    actions::encode(&editor.action).is_ok(),
                                    pal.primary("変更")
                                        .min_size(Vec2::new(theme::s(90.0), theme::s(30.0))),
                                )
                                .clicked()
                            {
                                commit = true;
                            }
                            if ui
                                .add_sized(
                                    [theme::s(90.0), theme::s(30.0)],
                                    egui::Button::new("キャンセル"),
                                )
                                .clicked()
                            {
                                cancel = true;
                            }
                        });
                    });
                });
            editor.content_height = scroll.content_size.y + 8.0;
        });
        if let Some(response) = response {
            self.editor_bottom = response.response.rect.bottom().max(
                response.response.rect.top() + 45.0 + editor.content_height.min(maximum) + 16.0,
            );
        }
        if commit {
            let changed = !same_action(
                &self.profiles[self.profile].buttons[editor.slot],
                &editor.action,
            );
            self.profiles[self.profile].buttons[editor.slot] = editor.action;
            if changed {
                self.queue_key_apply();
            }
            if !self.preview() {
                let message = self.message.clone();
                self.save();
                if !self.dirty() {
                    self.message = message;
                }
            }
        } else if open && !cancel {
            self.editor = Some(editor);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_match_g600_counts_and_are_encodable() {
        assert_eq!(WINDOWS.len(), 45);
        assert_eq!(CHROME.len(), 40);
        for (name, action) in WINDOWS.iter().chain(CHROME) {
            assert!(actions::encode(action).is_ok(), "{name}: {action}");
        }
    }
    #[test]
    fn all_pages_reachable() {
        assert_eq!(page_range(45, 2, 16), (32, 45, 3, 2));
        assert_eq!(page_range(40, 2, 16), (32, 40, 3, 2));
        assert_eq!(page_range(45, 99, 16), (32, 45, 3, 2));
    }
    #[test]
    fn search_and_combo() {
        assert!(key_matches("テンキー", "Num1"));
        assert!(!key_matches("F1", "F12"));
        assert!(key_matches("エンター", "Enter"));
        assert_eq!(
            actions::encode(&combination(17, "C").unwrap()).unwrap(),
            [0x8f, 17, 6, 0]
        );
    }
}

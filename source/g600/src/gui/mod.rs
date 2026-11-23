mod buttons;
mod edit;
pub mod file_dialog;
pub mod fonts;
#[cfg(test)]
mod verification;
mod inputlog;
pub mod i18n;
mod layout;
mod pointer;
mod press;
mod sample;
pub use sample::sample;
pub mod single;
mod sync;
mod theme;
mod view;
mod winkeys;
mod worker;

use std::sync::mpsc::{Receiver, Sender};
use std::time::Instant;
use eframe::egui;
use i18n::tr;
use g600ctl::protocol::{button_label, key_name, Action, LedEffect, Profile, DPI_MAX, DPI_MIN, POLL_RATES, PRODUCT_G600, VENDOR_LOGITECH};
use edit::{apply, changed, diff_summary, Change};
use worker::{Command, DeviceIdentity, DeviceSettings, DisplaySettings, Response, WriteKind};

/// 画面ごとの横幅（論理ピクセル）。高さは中身に合わせて自動で決める。
const MAIN_WIDTH: f32 = 1010.0;
const KEYS_WIDTH: f32 = 1060.0;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen { Main, Keys }

/// 割り当て編集。key_mods / key_usage は「キー」欄で選んでいる途中の値（「このキーに決定」で action に入る）
struct ButtonEditor {
    slot: usize, shifted: bool, original: Action, action: Action, key_mods: u8, key_usage: u8, search: String, name: String, last_auto_name: String, capture: bool, notice: String,
    /// ショートカットキーの中で開いている見出し（開くたびに全部閉じた状態から）と、各一覧のページ
    open_section: Option<layout::Section>, list_page: usize, cand_page: usize, custom_page: usize,
    /// ショートカットキーのタブの中身の高さ（これまでの最大。タブを切り替えても高さを変えないため）
    tab_height: f32,
    key_baseline: (u8, u8), registered_name: String, conflict: bool,
}

fn action_key(action: Action) -> (u8, u8) {
    match action { Action::Key { modifiers, usage } => (modifiers, usage), _ => (0, 0) }
}

impl ButtonEditor {
    fn has_draft(&self) -> bool {
        self.action != self.original || (self.key_mods, self.key_usage) != self.key_baseline
            || (self.name != self.last_auto_name && self.name != self.registered_name)
    }

    fn refresh_assignment(&mut self, actual: Action) {
        if actual == self.original { return; }
        if self.action == self.original && !self.conflict {
            self.action = actual;
        }
        if (self.key_mods, self.key_usage) == self.key_baseline && self.registered_name.is_empty()
            && (self.name.is_empty() || self.name == self.last_auto_name)
        {
            self.key_baseline = action_key(actual);
            (self.key_mods, self.key_usage) = self.key_baseline;
        }
        self.conflict = self.action != actual;
        self.original = actual;
    }
}

#[derive(Default)]
struct LedMemory {
    normal: [Option<[u8; 3]>; 3],
    shifted: [Option<[u8; 3]>; 3],
}

pub struct App {
    language: String, settings_open: bool,
    commands: Sender<Command>, responses: Receiver<Response>, settings: Option<DisplaySettings>,
    edits: Vec<Vec<u8>>, device_identity: Option<DeviceIdentity>, connected: bool, busy: bool, tab: usize, tab_initialized: bool, last_backup_path: Option<std::path::PathBuf>,
    source: String, message: String, button_editor: Option<ButtonEditor>, screen: Screen,
    keys: Vec<(u8, String)>, custom: Vec<(String, Action)>, custom_path: Option<std::path::PathBuf>, custom_message: String,
    /// 開いて表示中のバックアップファイル（表示中は自動では書き込まない）
    backup_path: Option<std::path::PathBuf>,
    confirm: Option<(String, Vec<String>, Command)>,
    backup_confirmation: Option<(Option<DeviceIdentity>, bool)>,
    /// Windows のポインター設定（画面の値 = Windows に反映済みの値）
    pointer: Option<pointer::PointerSettings>, pointer_message: String,
    /// 押されている物理ボタン（FF80 の入力レポートから。bit n = ボタン n+1）、PC に届いている入力、表示用の最後の入力
    button_presses: inputlog::Monitor, held: winkeys::Held, last_sent: String,
    /// 即時反映: 書き込み中のプロファイル、最後に編集が変わった時刻と内容、プロファイルごとの最後の書き込み完了時刻
    in_flight: Option<usize>,
    unverified_write: Option<(usize, Vec<u8>)>, last_edit: Option<Instant>, edit_snapshot: Vec<Vec<u8>>, last_done: [Option<Instant>; 3],
    /// 自動反映が有効か（失敗したら止める）、起動後最初の書き込み前に取ったバックアップ、状態表示
    auto_enabled: bool, session_backup: Option<String>, sync_status: String,
    /// 消灯する前の色。実機と編集中のバックアップでは別々に記憶する。
    led_memory: LedMemory, backup_led_memory: LedMemory,
}

impl App {
    pub fn new(ctx: &egui::Context) -> Self {
        let migration = g600ctl::storage::initialize();
        let language = i18n::initialize();
        theme::apply(ctx);
        let (commands, responses) = worker::start(ctx.clone());
        let button_presses = inputlog::Monitor::start(ctx.clone(), commands.clone());
        let custom_path = g600ctl::storage::file("g600-custom.txt").ok();
        let (custom, custom_message) = match &custom_path {
            Some(path) => match std::fs::read(path) {
                Ok(bytes) => {
                    let (entries, invalid) = edit::parse_custom_bytes(&bytes);
                    let message = if invalid > 0 { format!("自分で登録した組み合わせ: {} 件読み込み（読めない行 {invalid} 件は無視しました）", entries.len()) } else { String::new() };
                    (entries, message)
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Vec::new(), String::new()),
                Err(e) => (Vec::new(), format!("自分で登録した組み合わせを読み込めません: {e}")),
            },
            None => (Vec::new(), "自分で登録した組み合わせの保存先が見つかりません".into()),
        };
        let custom_message = migration.err().unwrap_or(custom_message);
        let mut app = Self { language, settings_open: false, commands, responses, settings: None, edits: Vec::new(), device_identity: None, connected: false,
            busy: false, tab: 0, tab_initialized: false, last_backup_path: None, source: String::new(), message: String::new(),
            button_editor: None, screen: Screen::Main, keys: edit::keys(), custom, custom_path, custom_message, backup_path: None,
            confirm: None, backup_confirmation: None, pointer: None, pointer_message: String::new(),
            button_presses, held: winkeys::Held::default(), last_sent: String::new(),
            in_flight: None, unverified_write: None, last_edit: None, edit_snapshot: Vec::new(), last_done: [None; 3],
            auto_enabled: true, session_backup: None, sync_status: String::new(), led_memory: LedMemory::default(), backup_led_memory: LedMemory::default() };
        app.reload_pointer();
        app
    }
    pub fn activate(&mut self, ctx: &egui::Context) {
        theme::apply(ctx);
        self.reload_pointer();
    }

    pub fn communication_pending(&self) -> bool {
        self.busy || self.in_flight.is_some()
    }

    pub fn update_connection(&mut self, connected: bool) -> bool {
        if self.communication_pending() {
            return false;
        }
        let preserve_edits = self.preserve_device_work();
        self.connected = false;
        self.button_presses.bind(None);
        self.confirm = None;
        if let Some((identity, _)) = self.backup_confirmation.as_mut() { *identity = None; }
        self.last_done = [None; 3];
        self.sync_status.clear();
        self.auto_enabled = !preserve_edits;
        if connected {
            if !preserve_edits {
                self.tab_initialized = false;
            }
            self.send(Command::Connect(self.editing_device()));
        } else {
            if let Some(editor) = &mut self.button_editor {
                editor.capture = false;
            }
            if self.backup_path.is_none() && !preserve_edits {
                self.settings = None;
                self.edits.clear();
                self.edit_snapshot.clear();
                self.button_editor = None;
                self.source.clear();
            }
            self.message = "G600rが接続されていません。".into();
        }
        true
    }

    fn preserve_device_work(&self) -> bool {
        self.unsaved_device_edits() || (self.backup_path.is_none() && self.button_editor.is_some())
    }

    fn editing_device(&self) -> Option<DeviceIdentity> {
        self.device_identity.clone().filter(|_| self.preserve_device_work())
    }

    fn accept_binding(&mut self, identity: DeviceIdentity, verified_continuation: bool, inputs: Vec<inputlog::InputDevice>) {
        if self.device_identity.as_ref().is_none_or(|old| !old.same_device(&identity))
            || (identity.serial.is_none() && !verified_continuation)
        {
            self.session_backup = None;
            self.led_memory = LedMemory::default();
        }
        self.button_presses.bind(Some(inputs));
        self.device_identity = Some(identity);
        self.connected = true;
    }

    pub fn unsaved_device_edits(&self) -> bool {
        self.backup_path.is_none() && (self.unverified_write.is_some() || self.dirty().iter().any(|dirty| *dirty))
    }

    pub fn has_unsaved_changes(&self) -> bool {
        self.unverified_write.is_some() || self.dirty().iter().any(|dirty| *dirty)
            || self.button_editor.as_ref().is_some_and(ButtonEditor::has_draft)
    }

    pub fn prepare_close(&mut self) {
        self.confirm = None;
        self.backup_confirmation = None;
        if let Some(editor) = &mut self.button_editor { editor.capture = false; }
    }

    pub fn has_pending_writes(&self) -> bool {
        self.communication_pending() || (self.connected && self.auto_enabled
            && self.unverified_write.is_none() && self.unsaved_device_edits())
    }

    fn resume_auto_apply(&mut self) {
        self.auto_enabled = true;
        if self.unverified_write.is_some() {
            // 直前の書き込み結果を確認してから次の書き込みを再開する。
            self.send(Command::Connect(self.editing_device()));
            self.auto_enabled = self.busy;
        } else {
            self.sync_status.clear();
        }
    }

    fn request_reload(&mut self) {
        self.reload_pointer();
        if self.preserve_device_work() || self.has_unsaved_changes() {
            self.confirm = Some(("未反映の編集を破棄して実機設定を再読み込みしますか？".into(), vec![], Command::Reload));
        } else {
            self.send(Command::Reload);
        }
    }

    fn request_rebind(&mut self) {
        let mut summary = vec!["接続中のG600rから実機設定を読み込みます。".into()];
        if self.has_unsaved_changes() { summary.push("確定していない編集と未反映の編集を破棄します。".into()); }
        self.confirm = Some(("接続中の実機を読み込みますか？".into(), summary, Command::Rebind));
    }

    fn choose_backup_file(&self, save: bool) -> Result<Option<std::path::PathBuf>, String> {
        let extension = "mmo-backup";
        let backup = tr("マウスのバックアップ");
        let all = tr("すべてのファイル");
        let save_filters = [(backup.as_str(), "*.mmo-backup")];
        let open_filters = [(backup.as_str(), "*.mmo-backup;*.txt;*.mmo-settings"), (all.as_str(), "*.*")];
        let initial_directory = self.last_backup_path.as_deref().and_then(std::path::Path::parent)
            .filter(|path| path.is_absolute() && path.is_dir());
        let path = file_dialog::choose(
            if save { &save_filters[..] } else { &open_filters[..] },
            save.then_some(("MMOMouseControl-G600-Backup.mmo-backup", extension)),
            initial_directory,
        )?;
        if save && path.as_ref().is_some_and(|path| {
            !path.extension().and_then(|value| value.to_str()).is_some_and(|value| value.eq_ignore_ascii_case(extension))
        }) {
            return Err(tr("拡張子 .mmo-backup のファイルを指定してください。"));
        }
        Ok(path)
    }

    fn request_backup_save(&mut self) {
        match self.choose_backup_file(true) {
            Ok(Some(path)) => self.send(Command::Save(path)),
            Ok(None) => {}
            Err(error) => self.message = tr(format!("保存先を選べません: {error}")),
        }
    }

    fn request_backup_open(&mut self) {
        let command = match self.choose_backup_file(false) {
            Ok(Some(path)) => Command::Open(path),
            Ok(None) => return,
            Err(error) => { self.message = tr(format!("ファイルを選べません: {error}")); return; }
        };
        if self.has_unsaved_changes() {
            self.confirm = Some(("未反映の編集を破棄してバックアップを開きますか？".into(), vec![], command));
        } else {
            self.send(command);
        }
    }

    fn backup_write_ready(&self) -> bool {
        self.backup_confirmation.as_ref().is_some_and(|(identity, _)| {
            identity.is_some() && identity == &self.device_identity
        }) && self.connected && !self.busy && self.in_flight.is_none() && self.backup_path.is_some()
    }

    fn answer_backup_confirmation(&mut self, write: bool) {
        let ready = self.backup_write_ready();
        self.backup_confirmation = None;
        if let Some(settings) = self.settings.as_ref().filter(|_| write && ready) {
            self.send(Command::RestoreBackup(DisplaySettings {
                active: settings.active,
                profiles: settings.profiles.clone(),
            }));
        }
    }

    fn current_led_memory(&mut self) -> &mut LedMemory {
        if self.backup_path.is_some() { &mut self.backup_led_memory } else { &mut self.led_memory }
    }

    fn reload_pointer(&mut self) {
        match pointer::read() {
            Ok(p) => self.pointer = Some(p),
            Err(e) => { self.pointer = None; self.pointer_message = e; }
        }
    }
    /// Windows の設定へ即時反映する。persist が false ならドラッグ中の一時反映（保存・通知なし）
    fn set_pointer(&mut self, settings: pointer::PointerSettings, persist: bool) {
        self.pointer = Some(settings);
        match pointer::write(settings, persist) {
            Ok(()) => if persist { self.pointer_message.clear(); },
            Err(e) => self.pointer_message = e,
        }
    }
    /// 時間のかかる操作（UI を止める）を送る
    fn send(&mut self, command: Command) {
        let writing = matches!(&command, Command::Apply { .. } | Command::RestoreSession(_) | Command::RestoreBackup(_) | Command::Use { .. });
        match self.commands.send(command) {
            Ok(()) => { self.busy = true; self.message = if writing { "書き込み中…（約2秒）" } else { "処理中..." }.into(); }
            Err(e) => self.message = format!("作業スレッドに送信できません: {e}"),
        }
    }
    fn replace(&mut self, settings: DisplaySettings) {
        self.edits = settings.profiles.iter().map(|p| p.raw.clone()).collect();
        self.edit_snapshot = self.edits.clone();
        self.last_edit = None;
        if !self.tab_initialized { self.tab = settings.active.profile as usize; self.tab_initialized = true; }
        self.settings = Some(settings);
        self.unverified_write = None;
        self.button_editor = None;
    }
    fn accept_auto_apply(&mut self, applied: Option<(usize, &[u8])>, mut actual: DisplaySettings) {
        let Some(previous) = &self.settings else {
            self.replace(actual);
            return;
        };
        for (index, ((old, current), edit)) in previous.profiles.iter()
            .zip(&mut actual.profiles).zip(&mut self.edits).enumerate()
        {
            let sent = applied.filter(|(slot, _)| *slot == index).map(|(_, sent)| sent);
            let expected = sent.unwrap_or(&old.raw);
            if edit.as_slice() == expected || edit.as_slice() == current.raw.as_slice() {
                edit.clone_from(&current.raw);
            } else if current.raw != expected {
                // 編集が残っている間は競合を検出できるよう、元の比較基準を維持する。
                *current = sent.and_then(|raw| Profile::parse(raw).ok()).unwrap_or_else(|| old.clone());
            }
        }
        self.settings = Some(actual);
        if let Some(editor) = &mut self.button_editor {
            if let Some(profile) = self.edits.get(self.tab).and_then(|raw| Profile::parse(raw).ok()) {
                let actual = if editor.shifted { profile.gshift_buttons[editor.slot] } else { profile.buttons[editor.slot] };
                editor.refresh_assignment(actual);
            }
        }
    }

    fn reconcile_after_reconnect(&mut self, mut actual: DisplaySettings) {
        let unverified = self.unverified_write.take();
        let mut applied = None;
        if let Some((slot, sent)) = &unverified {
            let old = &self.settings.as_ref().unwrap().profiles[*slot];
            let current = &mut actual.profiles[*slot];
            if current.raw == *sent {
                applied = Some((*slot, sent.as_slice()));
            } else if current.raw != old.raw && current.raw != self.edits[*slot] {
                // 他の値を検出した場合は編集と比較基準を維持し、自動書き込みを止める。
                *current = old.clone();
                self.unverified_write = unverified.clone();
            }
        }
        self.accept_auto_apply(applied, actual);
    }

    fn open_button_editor(&mut self, slot: usize, shifted: bool, action: Action) {
        let (key_mods, key_usage) = action_key(action);
        self.button_editor = Some(ButtonEditor { slot, shifted, original: action, action, key_mods, key_usage, search: String::new(), name: String::new(), last_auto_name: String::new(), capture: false, notice: String::new(),
            open_section: None, list_page: 0, cand_page: 0, custom_page: 0, tab_height: 0.0,
            key_baseline: (key_mods, key_usage), registered_name: String::new(), conflict: false });
    }

    fn commit_button_editor(&mut self) {
        if let Some(editor) = self.button_editor.take() {
            if editor.action != editor.original {
                self.change(Change::Button(editor.slot, editor.shifted, editor.action));
            }
        }
    }
    fn change(&mut self, change: Change) {
        if let Err(e) = apply(&mut self.edits[self.tab], change) { self.message = e; }
    }
    /// プロファイルごとに、編集内容がマウスの内容と違うか
    fn dirty(&self) -> Vec<bool> {
        match &self.settings { Some(s) => s.profiles.iter().zip(&self.edits).map(|(p, raw)| changed(p, raw)).collect(), None => Vec::new() }
    }
    /// 書き込み中か、まだ書いていない変更があるか
    fn syncing(&self) -> bool { self.in_flight.is_some() || self.unsaved_device_edits() }
    /// 即時反映: 変更が落ち着いたらマウスへ書く（ドラッグ中は書かない）
    fn auto_sync(&mut self, ctx: &egui::Context) {
        if self.settings.is_none() || !self.connected || self.unverified_write.is_some() { return; }
        let now = Instant::now();
        if self.edits != self.edit_snapshot { self.edit_snapshot = self.edits.clone(); self.last_edit = Some(now); }
        let dirty = self.dirty();
        let input = sync::SyncInput {
            now, enabled: self.auto_enabled && self.backup_path.is_none() && !self.busy && self.confirm.is_none(),
            in_flight: self.in_flight.is_some(), pointer_down: ctx.input(|i| i.pointer.any_down()),
            last_edit: self.last_edit, dirty: &dirty, last_done: &self.last_done,
        };
        if let Some(p) = sync::next_write(&input) {
            let command = Command::AutoApply {
                profile: p as u8,
                expected: self.settings.as_ref().unwrap().profiles[p].raw.clone(),
                data: self.edits[p].clone(),
                backup: self.session_backup.is_none(),
            };
            match self.commands.send(command) {
                Ok(()) => { self.in_flight = Some(p); self.sync_status = format!("反映中…（プロファイル{}）", p + 1); }
                Err(e) => { self.auto_enabled = false; self.sync_status = format!("失敗: 作業スレッドに送信できません: {e}"); }
            }
        } else if let Some(wait) = sync::wait_hint(&input) {
            ctx.request_repaint_after(wait);
        }
    }
    /// PCに届いたキーとマウスの入力を読み、表示とキー登録に使う。
    fn detect_presses(&mut self, ctx: &egui::Context) {
        let focused = ctx.input(|i| i.focused);
        let held = if focused { winkeys::read_held() } else { winkeys::Held::default() };
        if focused { ctx.request_repaint_after(std::time::Duration::from_millis(30)); }
        // 「キーを押して登録」中: 押したキー（＋修飾キー）をそのまま取り込む。テンキー・左右の修飾キーも区別できる
        if let Some(editor) = self.button_editor.as_mut().filter(|e| e.capture) {
            if let Some(&usage) = held.usages.first() {
                // 取り込むのは「自分で登録」の組み合わせだけ。使うには名前を付けて「登録」する
                editor.key_mods = held.modifiers; editor.key_usage = usage;
                editor.capture = false;
                editor.notice = "押したキーを取り込みました。名前を確認して「登録」を押してください。".into();
            }
        }
        if held != self.held {
            if press::is_pressed(&held) { self.last_sent = press::describe_held(&held); }
            self.held = held;
        }
    }
}

impl App {
    /// 即時反映以外の応答
    fn handle(&mut self, response: Response) {
        match response {
            Response::Connected(Ok(DeviceSettings { identity, inputs, continued_handle, .. })) if self.backup_path.is_some() => {
                self.accept_binding(identity, continued_handle, inputs);
                self.message = "G600rを検出しました。".into();
            }
            Response::Connected(Ok(DeviceSettings { identity, settings, inputs, continued_handle })) if self.preserve_device_work() => {
                if self.device_identity.as_ref().is_none_or(|old| !old.same_device(&identity)) {
                    self.connected = false;
                    self.auto_enabled = false;
                    self.message = "元のG600rを接続するか、編集を破棄して再読み込みしてください。".into();
                    return;
                }
                self.accept_binding(identity, continued_handle, inputs);
                self.reconcile_after_reconnect(settings);
                let pending = self.unsaved_device_edits();
                let conflict = self.unverified_write.is_some();
                self.auto_enabled = !pending || (self.auto_enabled && !conflict);
                if !conflict {
                    self.sync_status.clear();
                }
                self.message = if conflict {
                    "再接続後の実機設定が送信前後の値と違います。編集内容を確認してください。"
                } else if pending {
                    "再接続しました。未反映の編集を保持しています。"
                } else {
                    "再接続しました。実機の設定と編集内容が一致しています。"
                }.into();
            }
            Response::Reload(Ok(DeviceSettings { identity, settings, inputs, continued_handle }))
            | Response::Connected(Ok(DeviceSettings { identity, settings, inputs, continued_handle })) => {
                self.accept_binding(identity, continued_handle, inputs);
                self.auto_enabled = true;
                self.sync_status.clear();
                self.source = "実機".into();
                self.backup_path = None;
                self.replace(settings);
                self.message = "読み込みました。".into();
            }
            Response::Reload(Err(e)) | Response::Connected(Err(e)) => {
                self.connected = false;
                self.button_presses.bind(None);
                self.auto_enabled = false;
                self.message = format!("読み込み失敗: {e}");
            }
            Response::Save(Ok(path)) => {
                self.last_backup_path = Some(path.clone());
                self.message = format!("バックアップを保存しました: {}", path.display());
            }
            Response::Save(Err(e)) => self.message = format!("バックアップの保存に失敗: {e}"),
            Response::Open(Ok((path, settings, staged))) => {
                self.last_backup_path = Some(path.clone());
                if let Some(settings) = settings {
                    self.source = format!("バックアップ: {}", path.display());
                    self.backup_path = Some(path);
                    self.backup_led_memory = LedMemory::default();
                    self.replace(settings);
                    self.backup_confirmation = Some((self.device_identity.clone().filter(|_| self.connected), staged));
                }
                self.message = if staged { "アプリ設定は次回起動時に復元されます。" }
                    else { "バックアップを読み込みました。" }.into();
            }
            Response::Open(Err(e)) => self.message = format!("バックアップの読み込みに失敗: {e}"),
            Response::Write(kind, Ok((path, s))) => {
                if self.session_backup.is_none() { self.session_backup = Some(path.clone()); }
                self.connected = true;
                if let Some(settings) = self.settings.as_mut().filter(|_| self.backup_path.is_some()) {
                    for (slot, (before, after)) in settings.profiles.iter_mut().zip(&s.profiles).enumerate() {
                        let advance = match kind {
                            WriteKind::BackupProfile(profile) => profile == slot,
                            WriteKind::BackupFile => true,
                            WriteKind::SessionRestore => false,
                        };
                        if advance {
                            if matches!(kind, WriteKind::BackupProfile(_)) || !changed(before, &self.edits[slot]) {
                                self.edits[slot].clone_from(&after.raw);
                            }
                            *before = after.clone();
                        }
                    }
                }
                if self.backup_path.is_none() || !self.has_unsaved_changes() {
                    self.source = "実機".into(); self.backup_path = None; self.replace(s);
                }
                self.message = format!("書き込みました（読み戻し一致）。書き込み前のバックアップ: {path}");
            }
            Response::Write(_, Err((e, path))) => {
                if self.session_backup.is_none() { self.session_backup = path.clone(); }
                self.message = format!("書き込み失敗: {e}\n書き込み前のバックアップ: {}", path.as_deref().unwrap_or("作成されていません"));
            }
            Response::Use(Ok((saved, s))) => {
                if self.session_backup.is_none() { self.session_backup = saved; }
                self.connected = true; self.source = "実機".into(); self.backup_path = None; self.replace(s);
                if let Some(s) = &self.settings { self.tab = s.active.profile as usize; }
                self.message = "プロファイルを切り替え、読み戻しを確認しました。".into();
            }
            Response::Use(Err((e, saved))) => {
                if self.session_backup.is_none() { self.session_backup = saved; }
                self.message = format!("プロファイル切替に失敗: {e}");
            }
            Response::AutoApply(..) | Response::Inputs(..) => {}
        }
    }
}

/// 「LCtrl+F1」のような組み合わせの表示（「キー 」を付けない）
fn combo_text(action: &Action) -> String {
    let d = action.describe();
    tr(d.strip_prefix("キー ").unwrap_or(&d))
}

/// スライダー（数値は出さない）＋固定幅の数値欄。戻り値は (値が変わった, 確定した)。
/// 確定 = ドラッグを離した、またはクリック・キー・数値入力で変わった。
fn live_slider(ui: &mut egui::Ui, value: &mut u32, range: std::ops::RangeInclusive<u32>, log: bool, step: f64, width: f32, reserve: f32) -> (bool, bool) {
    let gap = ui.spacing().item_spacing.x;
    let suffix_width = if reserve > 0.0 { reserve + gap } else { 0.0 };
    let available = (ui.available_width() - suffix_width).max(0.0);
    let height = theme::s(24.0).max(ui.spacing().interact_size.y);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(available, height), egui::Sense::hover());
    let width = theme::s(width).min(available * 0.5);
    let slider_w = (available - width - gap).max(0.0);
    let slider_rect = egui::Rect::from_min_size(rect.min, egui::vec2(slider_w, height));
    let field_rect = egui::Rect::from_min_max(egui::pos2(rect.right() - width, rect.top()), rect.max);
    let mut slider_ui = ui.new_child(egui::UiBuilder::new().max_rect(slider_rect)
        .layout(egui::Layout::left_to_right(egui::Align::Center)));
    slider_ui.set_clip_rect(slider_rect.intersect(ui.clip_rect()));
    let mut field_ui = ui.new_child(egui::UiBuilder::new().max_rect(field_rect)
        .layout(egui::Layout::left_to_right(egui::Align::Center)));
    field_ui.set_clip_rect(field_rect.intersect(ui.clip_rect()));
    // スライダー: egui 自身の描画は透明にして、図のとおりに自分で描く
    // （細いレール #40516B、つまみより左は #69B4FF、つまみは直径 14 の塗りつぶしの丸 #F1F5F9・枠なし）
    let (lo, hi) = (*range.start() as f64, *range.end() as f64);
    let slider = slider_ui.scope(|ui| {
        ui.spacing_mut().slider_width = slider_w;
        let v = ui.visuals_mut();
        v.selection.bg_fill = egui::Color32::TRANSPARENT;
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active] {
            w.bg_fill = egui::Color32::TRANSPARENT;
            w.fg_stroke = egui::Stroke::NONE;
            w.bg_stroke = egui::Stroke::NONE;
            w.expansion = 0.0;
        }
        ui.add(egui::Slider::new(value, range.clone()).show_value(false).logarithmic(log).step_by(step).trailing_fill(true))
    }).inner;
    let norm = layout::slider_position(*value as f64, lo, hi, log) as f32;
    paint_slider(&slider_ui, &slider, norm);
    let field = theme::input_scope(&mut field_ui, |ui| {
        ui.add_sized([width, height], egui::DragValue::new(value).range(range).speed(step.max(1.0) * 0.2))
    });
    let changed = slider.changed() || field.changed();
    let commit = slider.drag_stopped() || field.drag_stopped() || field.lost_focus()
        || (slider.changed() && !slider.dragged()) || (field.changed() && !field.dragged() && !field.has_focus());
    (changed, commit)
}

fn led_choice_name(choice: edit::LedChoice) -> String {
    match choice { edit::LedChoice::Off => "消灯".into(), edit::LedChoice::Effect(e) => effect_name(e) }
}

fn effect_name(effect: LedEffect) -> String {
    match effect { LedEffect::Solid => "点灯".into(), LedEffect::Breathe => "呼吸".into(),
        LedEffect::Cycle => "色循環".into(), LedEffect::Unknown(v) => format!("不明 (0x{v:02X})") }
}

impl App {
    pub fn tick(&mut self) {
        while let Ok(response) = self.responses.try_recv() {
            match response {
                Response::Inputs(generation, inputs) => self.button_presses.refresh(generation, inputs),
                Response::AutoApply(profile, data, result) => {
                    let p = profile as usize;
                    self.in_flight = None;
                    if p < 3 { self.last_done[p] = Some(Instant::now()); }
                    match result {
                        Ok((saved, s)) => {
                            if self.session_backup.is_none() { self.session_backup = saved; }
                            self.connected = true;
                            self.accept_auto_apply(Some((p, &data)), s);
                            self.sync_status = format!("反映しました（プロファイル{}・読み戻し一致）", p + 1);
                        }
                        Err((e, saved)) => {
                            if self.session_backup.is_none() { self.session_backup = saved; }
                            self.unverified_write = Some((p, data));
                            self.auto_enabled = false;
                            self.sync_status = format!("失敗: {e}（自動反映を止めました）");
                            self.message = self.sync_status.clone();
                        }
                    }
                }
                other => { self.busy = false; self.handle(other); }
            }
        }
    }

    pub fn flush_pending(&mut self, ctx: &egui::Context) {
        self.auto_sync(ctx);
    }

    pub fn draw(&mut self, ctx: &egui::Context, header_height: f32) {
        self.detect_presses(ctx);
        self.flush_pending(ctx);
        let syncing = self.syncing();
        let locked = self.busy || self.confirm.is_some() || self.backup_confirmation.is_some();
        let content = egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin::same(theme::MARGIN))).show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = theme::s(10.0);
            if locked { ui.disable(); }
            // 中身の高さ = 描画前後のカーソル位置の差（パネルの大きさに左右されない）
            self.body(ui, syncing)
        }).inner;
        let editor = if self.screen == Screen::Keys { self.editor_window(ctx) } else { None };
        self.settings_window(ctx);
        if self.confirm.is_some() {
            let mut accept = false; let mut cancel = false;
            egui::Window::new(tr("確認")).collapsible(false).resizable(false).show(ctx, |ui| {
                let (title, summary, _) = self.confirm.as_ref().unwrap();
                ui.label(tr(title));
                for line in summary.iter().take(20) { ui.label(tr(line)); }
                ui.horizontal(|ui| { if ui.button(tr("OK")).clicked() { accept = true; } if ui.button(tr("キャンセル")).clicked() { cancel = true; } });
            });
            if accept { let (_, _, command) = self.confirm.take().unwrap(); self.send(command); }
            else if cancel { self.confirm = None; }
        }
        if let Some((_, staged)) = &self.backup_confirmation {
            if let Some(write) = file_dialog::confirm_write(
                ctx, "g600-backup-write", self.backup_write_ready(), *staged,
                "3つのプロファイルをマウスに書き込みます。",
            ) {
                self.answer_backup_confirmation(write);
            }
        }
        // ウィンドウの大きさを中身に合わせる（スクロールバーを出さない）
        let desired_width = theme::s(if self.screen == Screen::Main { MAIN_WIDTH } else { KEYS_WIDTH });
        let max_w = ctx.input(|i| i.viewport().monitor_size).map_or(desired_width, |m| (m.x - 32.0).max(600.0));
        let width = desired_width.min(max_w);
        let mut height = header_height + content + 20.0;
        if let Some(r) = editor { height = height.max(r.bottom() + 12.0); }
        // 画面より大きくしない（暴走防止）
        let max_h = ctx.input(|i| i.viewport().monitor_size).map_or(1000.0, |m| m.y - 60.0);
        let height = height.clamp(300.0, max_h.max(300.0));
        let current = ctx.input(|i| i.screen_rect().size());
        if (current.x - width).abs() > 2.0 || (current.y - height).abs() > 2.0 {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(width, height)));
        }
    }
}

/// スライダーを図の見た目で描く（norm はつまみの位置 0〜1）
fn paint_slider(ui: &egui::Ui, response: &egui::Response, norm: f32) {
    let rect = response.rect;
    let r = rect.height() / 2.5; // egui がつまみの可動範囲を決めるのと同じ値
    let x = rect.left() + r + (rect.width() - 2.0 * r) * norm.clamp(0.0, 1.0);
    let cy = rect.center().y;
    let rail = egui::Rect::from_min_max(egui::pos2(rect.left(), cy - 2.0), egui::pos2(rect.right(), cy + 2.0));
    let p = ui.painter();
    let alpha = if ui.is_enabled() { 1.0 } else { 0.4 };
    p.rect_filled(rail, egui::CornerRadius::same(2), theme::LINE.gamma_multiply(alpha));
    p.rect_filled(egui::Rect::from_min_max(rail.min, egui::pos2(x, rail.max.y)), egui::CornerRadius::same(2), theme::ACCENT.gamma_multiply(alpha));
    let knob = if response.hovered() || response.dragged() { egui::Color32::WHITE } else { theme::TEXT };
    p.circle_filled(egui::pos2(x, cy), 7.0, knob.gamma_multiply(alpha));
}

/// 50 刻みに丸めて範囲内に収める
fn round_dpi(value: u32) -> u32 {
    ((value + 25) / 50 * 50).clamp(DPI_MIN, DPI_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn display_labels() {
        assert_eq!(effect_name(LedEffect::Unknown(22)), "不明 (0x16)");
        assert_eq!(effect_name(LedEffect::Breathe), "呼吸");
    }
    #[test]
    fn dpi_rounding() {
        assert_eq!(round_dpi(1224), 1200);
        assert_eq!(round_dpi(1225), 1250);
        assert_eq!(round_dpi(100), DPI_MIN);
        assert_eq!(round_dpi(9000), DPI_MAX);
    }
}

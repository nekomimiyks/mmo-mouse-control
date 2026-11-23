use eframe::egui;
use g600ctl::{gui::i18n::tr, mouse_backup::UPDATE_CHECK_FILE, storage};
use std::sync::mpsc;

const LATEST: &str = "https://github.com/nekomimiyks/mmo-mouse-control/releases/latest";
const TAG: &str = "https://github.com/nekomimiyks/mmo-mouse-control/releases/tag/v";

pub(crate) fn is_packaged() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::{Foundation::ERROR_INSUFFICIENT_BUFFER, Storage::Packaging::Appx::GetCurrentPackageFullName};
        let mut length = 0;
        // Query the buffer size; an unpackaged process has no package identity.
        unsafe { GetCurrentPackageFullName(&mut length, std::ptr::null_mut()) == ERROR_INSUFFICIENT_BUFFER }
    }
    #[cfg(not(windows))]
    false
}

pub struct Updates {
    packaged: bool,
    enabled: bool,
    open: bool,
    checking: Option<mpsc::Receiver<Result<Check, ()>>>,
    result: Option<Result<Check, ()>>,
    save_failed: bool,
    installing: Option<mpsc::Receiver<bool>>,
    launch_status: Option<bool>,
}

enum Check {
    Current,
    Available(String),
}

impl Updates {
    pub fn new(ctx: &egui::Context) -> Self {
        let enabled = storage::file(UPDATE_CHECK_FILE)
            .ok().and_then(|path| std::fs::read_to_string(path).ok())
            .is_some_and(|value| value.trim() == "on");
        let mut updates = Self {
            packaged: is_packaged(), enabled, open: false, checking: None, result: None, save_failed: false,
            installing: None, launch_status: None,
        };
        if enabled {
            updates.start_check(ctx);
        }
        updates
    }

    fn start_check(&mut self, ctx: &egui::Context) {
        if self.packaged || self.checking.is_some() || self.installing.is_some() {
            return;
        }
        self.result = None;
        self.launch_status = None;
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.checking = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(check_latest());
            ctx.request_repaint();
        });
    }

    pub fn header(&mut self, ui: &mut egui::Ui) {
        if let Some(channel) = &self.checking {
            match channel.try_recv() {
                Ok(result) => { self.result = Some(result); self.checking = None; }
                Err(mpsc::TryRecvError::Disconnected) => { self.result = Some(Err(())); self.checking = None; }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(channel) = &self.installing {
            match channel.try_recv() {
                Ok(started) => { self.launch_status = Some(started); self.installing = None; }
                Err(mpsc::TryRecvError::Disconnected) => { self.launch_status = Some(false); self.installing = None; }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if ui.button(tr("更新設定")).clicked() {
            self.open = true;
        }
        if let Some(Ok(Check::Available(tag))) = &self.result {
            if self.installing.is_none() && self.launch_status != Some(true)
                && ui.button(tr("アップデート")).on_hover_text(tr("新しいバージョンがあります")).clicked()
            {
                let tag = tag.clone();
                self.open = true;
                self.start_install(ui.ctx(), tag);
            }
        }
    }

    fn start_install(&mut self, ctx: &egui::Context, tag: String) {
        let ctx = ctx.clone();
        let (tx, rx) = mpsc::channel();
        self.installing = Some(rx);
        self.launch_status = None;
        std::thread::spawn(move || {
            let child = launch_installer(&tag);
            let _ = tx.send(child.is_some());
            ctx.request_repaint();
            if let Some(mut child) = child {
                let _ = child.wait();
            }
        });
    }

    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new(tr("更新設定")).id(egui::Id::new("update-settings"))
            .open(&mut open).resizable(false).show(ctx, |ui| {
                ui.label(format!("MMO Mouse Control {}", env!("CARGO_PKG_VERSION")));
                if self.packaged {
                    ui.label(tr("Microsoft Storeで更新を管理します。"));
                    return;
                }
                let mut enabled = self.enabled;
                if ui.checkbox(&mut enabled, tr("起動時に更新を確認する")).changed() {
                    let saved = storage::file(UPDATE_CHECK_FILE).and_then(|path| {
                        storage::write_atomic(&path, if enabled { b"on" } else { b"off" }).map_err(|e| e.to_string())
                    });
                    self.save_failed = saved.is_err();
                    if saved.is_ok() {
                        self.enabled = enabled;
                        if enabled {
                            self.start_check(ctx);
                        }
                    }
                }
                if self.save_failed {
                    ui.label(tr("更新設定を保存できませんでした。"));
                }
                if ui.add_enabled(self.checking.is_none() && self.installing.is_none(), egui::Button::new(tr("今すぐ更新を確認"))).clicked() {
                    self.start_check(ctx);
                }
                if self.checking.is_some() {
                    ui.label(tr("更新を確認中…"));
                }
                match &self.result {
                    Some(Ok(Check::Available(tag))) => {
                        ui.label(tr("新しいバージョンがあります"));
                        let available = self.installing.is_none() && self.launch_status != Some(true);
                        if ui.add_enabled(available, egui::Button::new(tr("アップデート"))).clicked() {
                            self.start_install(ctx, tag.clone());
                        }
                    }
                    Some(Ok(Check::Current)) => { ui.label(tr("最新版を使用しています。")); }
                    Some(Err(())) => { ui.label(tr("更新を確認できませんでした。")); }
                    None => {}
                }
                if self.installing.is_some() {
                    ui.label(tr("インストーラーを準備中…"));
                }
                if let Some(started) = self.launch_status {
                    ui.label(tr(if started { "更新インストーラーを起動しました。" } else { "更新インストーラーを起動できませんでした。" }));
                }
            });
        self.open = open;
    }
}

fn version(tag: &str) -> Option<[u32; 3]> {
    let digits = tag.split_once('-').map_or(tag, |(value, _)| value);
    let mut parts = digits.split('.');
    let result = [parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.next()?.parse().ok()?];
    parts.next().is_none().then_some(result)
}

fn check_location(location: &str, installed: &str) -> Option<Check> {
    let tag = location.trim().strip_prefix(TAG)?;
    if !tag.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-')) {
        return None;
    }
    Some(if version(tag)? > version(installed)? { Check::Available(tag.to_owned()) } else { Check::Current })
}

fn check_latest() -> Result<Check, ()> {
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;
    #[cfg(windows)]
    let mut command = std::process::Command::new(
        std::path::PathBuf::from(std::env::var_os("SystemRoot").ok_or(())?).join("System32/curl.exe"),
    );
    #[cfg(not(windows))]
    let mut command = std::process::Command::new("curl");
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let output = command.args([
        "-q", "--head", "--proto", "=https", "--max-time", "8",
        "--silent", "--show-error", "--output", if cfg!(windows) { "NUL" } else { "/dev/null" },
        "--write-out", "%{redirect_url}", LATEST,
    ]).output().map_err(|_| ())?;
    if !output.status.success() {
        return Err(());
    }
    let location = std::str::from_utf8(&output.stdout).map_err(|_| ())?;
    check_location(location, env!("CARGO_PKG_VERSION")).ok_or(())
}

#[cfg(windows)]
fn launch_installer(tag: &str) -> Option<std::process::Child> {
    use std::{
        io::{BufRead, BufReader},
        os::windows::process::CommandExt,
        path::PathBuf,
        process::{Command, Stdio},
    };

    let source = std::env::current_exe().ok()?;
    let system = PathBuf::from(std::env::var_os("SystemRoot")?);
    let mut child = Command::new(system.join("System32/WindowsPowerShell/v1.0/powershell.exe"))
        .creation_flags(0x0800_0000)
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command",
            include_str!("install_update.ps1")])
        .env("MMO_UPDATE_TAG", tag)
        .env("MMO_UPDATE_FROM", source.parent()?)
        .stdout(Stdio::piped()).stderr(Stdio::null())
        .spawn().ok()?;
    let mut line = String::new();
    let mut stdout = BufReader::new(child.stdout.take()?);
    while stdout.read_line(&mut line).ok()? > 0 {
        if line.trim() == "started" {
            return Some(child);
        }
        line.clear();
    }
    let _ = child.wait();
    None
}

#[cfg(not(windows))]
fn launch_installer(_tag: &str) -> Option<std::process::Child> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_numeric_versions_and_only_accepts_the_project_release_url() {
        assert!(matches!(check_location(&format!("{TAG}0.12.10"), "0.12.4"), Some(Check::Available(tag)) if tag == "0.12.10"));
        assert!(matches!(check_location(&format!("{TAG}0.12.4"), "0.12.4"), Some(Check::Current)));
        assert!(matches!(check_location(&format!("{TAG}0.12.2-setup.2"), "0.12.4"), Some(Check::Current)));
        assert!(check_location("https://github.com/other/mmo-mouse-control/releases/tag/v9.0.0", "0.12.4").is_none());
        assert!(check_location(&format!("{TAG}0.12.10?bad"), "0.12.4").is_none());
    }
}

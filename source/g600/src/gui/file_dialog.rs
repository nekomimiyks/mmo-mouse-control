use std::path::{Path, PathBuf};

/// Opens the native Windows file picker. The save mode supplies a name and extension.
#[cfg(windows)]
pub fn choose(
    filters: &[(&str, &str)],
    save: Option<(&str, &str)>,
    initial_directory: Option<&Path>,
) -> Result<Option<PathBuf>, String> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use windows::core::{HRESULT, PCWSTR};
    use windows::Win32::{
        Foundation::HWND,
        System::Com::{
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
        },
        UI::Shell::{
            Common::COMDLG_FILTERSPEC, FileOpenDialog, FileSaveDialog, IFileDialog, IShellItem,
            SHCreateItemFromParsingName, FOS_DONTADDTORECENT, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM,
            FOS_NOCHANGEDIR, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, SIGDN_FILESYSPATH,
        },
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetActiveWindow;

    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            unsafe { CoUninitialize() }
        }
    }

    let wide = |text: &str| text.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let selected = (|| -> windows::core::Result<Option<PathBuf>> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE).ok()?;
            let _apartment = Apartment;
            let class = if save.is_some() { FileSaveDialog } else { FileOpenDialog };
            let dialog: IFileDialog = CoCreateInstance(&class, None, CLSCTX_INPROC_SERVER)?;
            let mut options = dialog.GetOptions()?
                | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR | FOS_DONTADDTORECENT;
            if let Some((name, extension)) = save {
                dialog.SetFileName(PCWSTR(wide(name).as_ptr()))?;
                dialog.SetDefaultExtension(PCWSTR(wide(extension).as_ptr()))?;
                options |= FOS_OVERWRITEPROMPT;
            } else {
                options |= FOS_FILEMUSTEXIST;
            }
            dialog.SetOptions(options)?;
            let filter_text: Vec<_> = filters.iter()
                .map(|(label, pattern)| (wide(label), wide(pattern))).collect();
            let filters: Vec<_> = filter_text.iter().map(|(label, pattern)| COMDLG_FILTERSPEC {
                pszName: PCWSTR(label.as_ptr()),
                pszSpec: PCWSTR(pattern.as_ptr()),
            }).collect();
            dialog.SetFileTypes(&filters)?;
            if let Some(path) = initial_directory {
                let name: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
                if let Ok(directory) = SHCreateItemFromParsingName::<_, _, IShellItem>(PCWSTR(name.as_ptr()), None) {
                    dialog.SetFolder(&directory)?;
                }
            }
            if let Err(error) = dialog.Show(HWND(GetActiveWindow())) {
                if error.code() == HRESULT(0x800704C7u32 as i32) {
                    return Ok(None); // HRESULT_FROM_WIN32(ERROR_CANCELLED)
                }
                return Err(error);
            }
            let raw = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
            let path = PathBuf::from(std::ffi::OsString::from_wide(raw.as_wide()));
            CoTaskMemFree(Some(raw.0.cast()));
            Ok(Some(path))
        }
    })();
    selected.map_err(|error| error.to_string())
}

#[cfg(not(windows))]
pub fn choose(
    _: &[(&str, &str)],
    _: Option<(&str, &str)>,
    _: Option<&Path>,
) -> Result<Option<PathBuf>, String> {
    Ok(None)
}

pub fn show_backup_path(ui: &mut eframe::egui::Ui, path: Option<&Path>) {
    use super::i18n::tr;
    use eframe::egui;

    let display = path.map(Path::to_string_lossy).unwrap_or_default();
    let mut value = display.as_ref();
    ui.label(tr("パス"));
    ui.add(
        egui::TextEdit::singleline(&mut value)
            .desired_width(ui.available_width())
            .hint_text(tr("バックアップファイルのパス")),
    ).on_hover_text(value);
}

pub fn confirm_write(
    ctx: &eframe::egui::Context,
    id: &'static str,
    ready: bool,
    staged: bool,
    scope: &str,
) -> Option<bool> {
    use super::i18n::tr;
    use eframe::egui;
    let response = egui::Modal::new(egui::Id::new(id)).show(ctx, |ui| {
        ui.heading(tr("マウスに書き込みますか？"));
        ui.label(tr(scope));
        if staged {
            ui.label(tr("アプリ設定は次回起動時に復元されます。"));
        }
        if !ready {
            ui.label(tr("書き込むには実機を接続し、設定を読み込んでください。"));
        }
        ui.horizontal(|ui| {
            if ui.button(tr("いいえ")).clicked() {
                Some(false)
            } else if ui.add_enabled(ready, egui::Button::new(tr("はい"))).clicked() {
                Some(true)
            } else {
                None
            }
        }).inner
    });
    response.inner.or_else(|| response.should_close().then_some(false))
}

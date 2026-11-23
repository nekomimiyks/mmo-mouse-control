#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod combined;
mod capture;
mod pointer;
mod settings_backup;
mod theme;
mod ui;

use g600ctl::gui;
use m908ctl::{device, identification};

fn main() -> eframe::Result {
    let mut capture_args = std::env::args_os().skip(1);
    if capture_args.next().as_deref() == Some(std::ffi::OsStr::new("--sample-capture")) {
        if let (Some(language), Some(screen), Some(output), None) =
            (capture_args.next(), capture_args.next(), capture_args.next(), capture_args.next())
        {
            return capture::run(language.to_string_lossy().into(), screen.to_string_lossy().into(), output.into());
        }
        std::process::exit(2);
    }
    device::initialize_hid();
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some(identification::WORKER_FLAG) {
        if identification::worker_main_with_args(args).is_err() {
            std::process::exit(1);
        }
        return Ok(());
    }

    let args = ui::Options::parse();
    let preview = args.capture.is_some();
    let _instance = if preview {
        None
    } else {
        match gui::single::acquire() {
            Some(instance) => Some(instance),
            None => return Ok(()),
        }
    };
    if !preview {
        if let Err(error) = settings_backup::apply_pending() {
            settings_backup::show_startup_error(&error);
            return Ok(());
        }
    }
    let icon = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png"))
        .expect("the bundled application icon is a valid PNG");
    eframe::run_native(
        gui::single::WINDOW_TITLE,
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_icon(icon)
                .with_inner_size(args.size)
                .with_min_inner_size([600.0, 300.0])
                .with_visible(!preview),
            ..Default::default()
        },
        Box::new(move |cc| {
            let ctx = &cc.egui_ctx;
            if preview {
                theme::install_fonts(ctx);
            } else {
                gui::fonts::install(ctx);
            }
            let m908 = ui::App::new(ctx, args);
            if preview {
                return Ok(Box::new(combined::Preview(m908)));
            }
            let g600 = gui::App::new(ctx);
            Ok(Box::new(combined::App::new(ctx, m908, g600)))
        }),
    )
}

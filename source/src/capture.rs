use crate::settings_backup;
use eframe::egui::{self, Event, ViewportCommand};
use g600ctl::gui;
use std::{path::PathBuf, time::{Duration, Instant}};

pub fn run(language: String, screen: String, output: PathBuf) -> eframe::Result {
    eframe::run_native(
        gui::single::WINDOW_TITLE,
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1060.0, 780.0]).with_visible(false),
            ..Default::default()
        },
        Box::new(move |cc| Ok(Box::new(Capture {
            app: gui::sample(&cc.egui_ctx, &language, screen == "buttons"),
            panel: settings_backup::Panel::sample(screen == "backup"),
            output,
            start: Instant::now(),
            requested: false,
        }))),
    )
}

struct Capture {
    app: gui::App,
    panel: settings_backup::Panel,
    output: PathBuf,
    start: Instant,
    requested: bool,
}

impl eframe::App for Capture {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let header = egui::TopBottomPanel::top("mouse-device-selector").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                self.panel.button(ui);
                ui.add(egui::Button::new("G600 / G600r").selected(true));
            });
        }).response.rect.height();
        self.app.draw(ctx, header);
        self.panel.show(ctx);
        if !self.requested && self.start.elapsed() >= Duration::from_millis(650) {
            ctx.send_viewport_cmd(ViewportCommand::Screenshot(egui::UserData::default()));
            self.requested = true;
        }
        for event in ctx.input(|input| input.events.clone()) {
            if let Event::Screenshot { image, .. } = event {
                let pixels: Vec<_> = image.pixels.iter().flat_map(|color| color.to_array()).collect();
                if image::save_buffer(&self.output, &pixels, image.size[0] as u32, image.size[1] as u32, image::ColorType::Rgba8).is_err() {
                    std::process::exit(1);
                }
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
        ctx.request_repaint_after(Duration::from_millis(50));
    }
}

use eframe::egui::{self, Event, ViewportCommand};
use g600ctl::gui::{self, i18n::tr};
use std::{path::PathBuf, time::{Duration, Instant}};

pub fn run(language: String, screen: String, output: PathBuf) -> eframe::Result {
    eframe::run_native(
        gui::single::WINDOW_TITLE,
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([1060.0, 780.0]).with_visible(false),
            ..Default::default()
        },
        Box::new(move |cc| Ok(Box::new(Capture {
            app: gui::sample(&cc.egui_ctx, &language, &screen),
            output,
            start: Instant::now(),
            requested: false,
        }))),
    )
}

struct Capture {
    app: gui::App,
    output: PathBuf,
    start: Instant,
    requested: bool,
}

impl eframe::App for Capture {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let header = egui::TopBottomPanel::top("mouse-device-selector").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(tr("マウス"));
                ui.add(egui::Button::new("G600 / G600r").selected(true));
                let _ = ui.button(m908ctl::device::DISPLAY_NAME);
                let _ = ui.button(tr("更新設定"));
            });
        }).response.rect.height();
        self.app.draw(ctx, header);
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

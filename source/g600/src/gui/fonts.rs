use eframe::egui;

pub fn install(ctx: &egui::Context) {
            #[cfg(target_os = "windows")]
            if let Some(windir) = std::env::var_os("WINDIR") {
                let dir = std::path::PathBuf::from(windir).join("Fonts");
                let load = |list: &[(String, u32)]| list.iter().find_map(|(file, index)| std::fs::read(dir.join(file)).ok().map(|d| {
                    let mut font = egui::FontData::from_owned(d);
                    font.index = *index;
                    font
                }));
                // 本文: 環境変数 G600GUI_FONT（例 "meiryo.ttc:2"）で試せる。既定は Meiryo UI（meiryo.ttc の 3 番目）
                let mut body: Vec<(String, u32)> = std::env::var("G600GUI_FONT").ok()
                    .and_then(|v| v.split_once(':').and_then(|(f, i)| i.parse().ok().map(|i| (f.to_string(), i)))).into_iter().collect();
                body.extend([("meiryo.ttc".to_string(), 2), ("YuGothM.ttc".to_string(), 1)]);
                // 太字: 本文とそろえて Meiryo UI Bold（meiryob.ttc の 3 番目）、無ければ Yu Gothic UI Bold
                let bold = [("meiryob.ttc".to_string(), 2u32), ("YuGothB.ttc".to_string(), 1)];
                let mut fonts = egui::FontDefinitions::default();
                if let Some(font) = load(&body) {
                    fonts.font_data.insert("japanese".into(), font.into());
                    fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap().insert(0, "japanese".into());
                }
                // Installed Windows fonts cover Chinese and Korean as well as Latin scripts.
                for (name, file, index) in [("latin", "segoeui.ttf", 0), ("korean", "malgun.ttf", 0), ("simplified", "msyh.ttc", 0), ("traditional", "msjh.ttc", 0)] {
                    if let Some(font) = load(&[(file.to_string(), index)]) {
                        fonts.font_data.insert(name.into(), font.into());
                        fonts.families.get_mut(&egui::FontFamily::Proportional).unwrap().push(name.into());
                    }
                }
                let mut bold_list: Vec<String> = Vec::new();
                if let Some(font) = load(&bold) { fonts.font_data.insert("japanese-bold".into(), font.into()); bold_list.push("japanese-bold".into()); }
                // 太字に無い文字は本文のフォントで出す
                bold_list.extend(fonts.families[&egui::FontFamily::Proportional].clone());
                fonts.families.insert(egui::FontFamily::Name("bold".into()), bold_list);
                ctx.set_fonts(fonts);
            }
}

use std::path::{Path, PathBuf};

use egui::RichText;
use haru_core::{overrides, properties};

use crate::theme;

pub enum Outcome {
    Nothing,
    Changed(String, String),
    Reset,
    Failed(String),
}

#[derive(Default)]
pub struct Panel {
    loaded: Vec<properties::Property>,
    subject: Option<PathBuf>,
}

impl Panel {
    pub fn show(&mut self, ui: &mut egui::Ui, id: &str, dir: &Path, live: bool) -> Outcome {
        self.load(id, dir);

        ui.label(RichText::new("Settings").small().color(theme::MUTED));
        ui.add_space(2.0);

        if self.loaded.is_empty() {
            ui.label(
                RichText::new("This wallpaper has no settings.")
                    .small()
                    .color(theme::MUTED),
            );
            return Outcome::Nothing;
        }

        if !live {
            ui.label(
                RichText::new("Put it up to change these.")
                    .small()
                    .color(theme::MUTED),
            );
            ui.add_space(4.0);
        }

        let mut changed: Option<(String, String)> = None;
        let mut reset = false;

        ui.add_enabled_ui(live, |ui| {
            for property in &mut self.loaded {
                if crate::widgets::property(ui, property) {
                    changed = Some((property.key.clone(), property.wire()));
                }
                ui.add_space(6.0);
            }
            ui.add_space(4.0);
            reset = ui.button("Reset to defaults").clicked();
        });

        if reset {
            return match overrides::clear(id) {
                Ok(()) => {
                    self.loaded = properties::read(dir);
                    Outcome::Reset
                }
                Err(why) => Outcome::Failed(why),
            };
        }

        let Some((key, value)) = changed else {
            return Outcome::Nothing;
        };
        match overrides::set(id, &key, &value) {
            Ok(()) => Outcome::Changed(key, value),
            Err(why) => Outcome::Failed(why),
        }
    }

    fn load(&mut self, id: &str, dir: &Path) {
        if self.subject.as_deref() == Some(dir) {
            return;
        }

        let mut read = properties::read(dir);
        let saved = overrides::read(id);
        for property in &mut read {
            if let Some(value) = saved.get(&property.key) {
                property.set_from_wire(value);
            }
        }

        self.loaded = read;
        self.subject = Some(dir.to_owned());
    }
}

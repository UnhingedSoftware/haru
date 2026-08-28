//! What the app itself is set to.
//!
//! Small on purpose. Every field here exists because it cannot be worked out:
//! a Steam library in an unusual place, a renderer socket that is not the
//! default. Anything that can be detected is detected instead of asked.

use egui::RichText;
use haru_apply::Backend;
use haru_core::Config;

use crate::theme;

/// The settings pane.
#[derive(Default)]
pub struct Settings {
    /// The socket path being typed, before it is a path.
    socket: String,
    /// The install directory being typed.
    install: String,
    /// A library being added.
    library: String,
    /// What the last save did.
    status: String,
}

impl Settings {
    /// Fills the boxes from the config, for when the pane is first opened.
    pub fn sync(&mut self, config: &Config) {
        self.socket = config
            .socket
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.install = config
            .install_dir
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default();
    }

    /// Draws the pane. Returns whether the config changed.
    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        config: &mut Config,
        backend: Option<&dyn Backend>,
        signed_in: bool,
        client: bool,
    ) -> (bool, bool) {
        let mut changed = false;
        let mut sign_in = false;

        egui::CentralPanel::default()
            .frame(theme::panel_frame(theme::Side::Middle))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_space(6.0);
                        ui.heading("Settings");
                        ui.add_space(14.0);

                        ui.label(RichText::new("Steam").strong());
                        ui.add_space(4.0);
                        match (signed_in, client) {
                            (true, _) => {
                                ui.label(RichText::new("Signed in").color(theme::ACCENT));
                            }
                            (false, true) => {
                                ui.label(
                                    RichText::new("Using the running Steam client")
                                        .color(theme::ACCENT),
                                );
                            }
                            (false, false) => {
                                ui.label(
                                    RichText::new("Not signed in — browsing works, downloading does not")
                                        .color(theme::MUTED),
                                );
                            }
                        }
                        ui.add_space(6.0);
                        if ui.button("Sign in…").clicked() {
                            sign_in = true;
                        }

                        ui.add_space(18.0);
                        ui.label(RichText::new("Renderer").strong());
                        ui.add_space(4.0);
                        match backend {
                            Some(backend) if backend.available() => {
                                ui.label(
                                    RichText::new(format!("{} is running", backend.name()))
                                        .color(theme::ACCENT),
                                );
                            }
                            Some(backend) => {
                                ui.label(
                                    RichText::new(format!("{} is not answering", backend.name()))
                                        .color(theme::MUTED),
                                );
                            }
                            None => {
                                ui.label(
                                    RichText::new(
                                        "No renderer found. Wallpapers can still be browsed and installed.",
                                    )
                                    .color(theme::MUTED),
                                );
                            }
                        }

                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("Control socket — blank uses $XDG_RUNTIME_DIR/lwe.sock")
                                .small()
                                .color(theme::MUTED),
                        );
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut self.socket)
                                    .hint_text("/run/user/1000/lwe.sock")
                                    .desired_width(420.0),
                            )
                            .lost_focus()
                        {
                            config.socket = path_or_none(&self.socket);
                            changed = true;
                        }

                        ui.add_space(18.0);
                        ui.label(RichText::new("Installing").strong());
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "Blank installs into the Steam library, where kirie and Wallpaper Engine already look.",
                            )
                            .small()
                            .color(theme::MUTED),
                        );
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut self.install)
                                    .hint_text("~/.local/share/Steam")
                                    .desired_width(420.0),
                            )
                            .lost_focus()
                        {
                            config.install_dir = path_or_none(&self.install);
                            changed = true;
                        }

                        ui.add_space(18.0);
                        ui.label(RichText::new("Steam libraries").strong());
                        ui.add_space(4.0);
                        for root in config.libraries() {
                            let extra = config.extra_libraries.contains(&root);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(root.to_string_lossy().into_owned())
                                        .small()
                                        .color(theme::MUTED),
                                );
                                if extra && ui.small_button("Remove").clicked() {
                                    config.extra_libraries.retain(|other| other != &root);
                                    changed = true;
                                }
                            });
                        }
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.library)
                                    .hint_text("Add another library")
                                    .desired_width(320.0),
                            );
                            if ui.button("Add").clicked()
                                && let Some(path) = path_or_none(&self.library)
                            {
                                config.extra_libraries.push(path);
                                self.library.clear();
                                changed = true;
                            }
                        });

                        ui.add_space(18.0);
                        ui.label(RichText::new("Browsing").strong());
                        ui.add_space(4.0);
                        changed |= ui
                            .checkbox(&mut config.adult, "Show adult content")
                            .changed();
                        ui.add_space(6.0);
                        changed |= ui
                            .checkbox(
                                &mut config.infinite_scroll,
                                "Keep loading as I scroll",
                            )
                            .on_hover_text(
                                "Results continue instead of being paged. Numbered pages go away.",
                            )
                            .changed();
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label(if config.infinite_scroll {
                                "Results per batch"
                            } else {
                                "Results per page"
                            });
                            changed |= ui
                                .add(egui::Slider::new(&mut config.per_page, 12..=100))
                                .drag_stopped();
                        });

                        ui.add_space(18.0);
                        ui.separator();
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Save").clicked() {
                                self.status = match config.save() {
                                    Ok(()) => "saved".to_owned(),
                                    Err(why) => why,
                                };
                            }
                            if !self.status.is_empty() {
                                ui.label(RichText::new(&self.status).color(theme::MUTED));
                            }
                        });
                        if let Some(path) = Config::path() {
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(path.to_string_lossy().into_owned())
                                    .small()
                                    .color(theme::MUTED),
                            );
                        }
                    });
            });

        (changed, sign_in)
    }
}

/// A typed path, or nothing when the box is empty.
fn path_or_none(raw: &str) -> Option<std::path::PathBuf> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| std::path::PathBuf::from(trimmed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_box_means_the_default_rather_than_an_empty_path() {
        // A `Some("")` here would point the renderer at a socket called
        // nothing, which fails in a way that reads as the renderer being down.
        assert_eq!(path_or_none("   "), None);
        assert_eq!(
            path_or_none(" /run/user/1000/lwe.sock "),
            Some(std::path::PathBuf::from("/run/user/1000/lwe.sock"))
        );
    }

    #[test]
    fn the_boxes_start_from_what_is_configured() {
        let mut settings = Settings::default();
        settings.sync(&Config {
            socket: Some(std::path::PathBuf::from("/tmp/s.sock")),
            ..Config::default()
        });
        assert_eq!(settings.socket, "/tmp/s.sock");
        assert!(settings.install.is_empty(), "a default stays blank");
    }
}

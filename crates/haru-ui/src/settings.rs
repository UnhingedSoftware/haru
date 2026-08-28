use egui::RichText;
use haru_apply::Backend;
use haru_core::Config;

use crate::theme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Renderer {
    Start,
    Restart,
    Stop,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Actions {
    pub changed: bool,
    pub sign_in: bool,
    pub sign_out: bool,
    pub install: bool,
    pub renderer: Option<Renderer>,
}

#[derive(Default)]
pub struct Settings {
    socket: String,
    install: String,
    library: String,
    status: String,
}

impl Settings {
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

    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        config: &mut Config,
        backend: Option<&dyn Backend>,
        signed_in: Option<&str>,
        client: bool,
    ) -> Actions {
        let mut actions = Actions::default();

        egui::CentralPanel::default()
            .frame(theme::panel_frame(theme::Side::Middle))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.add_space(6.0);
                        ui.heading("Settings");
                        ui.add_space(14.0);

                        self.steam(ui, signed_in, client, &mut actions);
                        ui.add_space(18.0);
                        self.renderer(ui, config, backend, &mut actions);
                        ui.add_space(18.0);
                        self.installing(ui, config, &mut actions);
                        ui.add_space(18.0);
                        self.libraries(ui, config, &mut actions);
                        ui.add_space(18.0);
                        Self::browsing(ui, config, &mut actions);
                        ui.add_space(18.0);
                        self.saving(ui, config);
                    });
            });

        actions
    }

    fn steam(
        &mut self,
        ui: &mut egui::Ui,
        signed_in: Option<&str>,
        client: bool,
        actions: &mut Actions,
    ) {
        ui.label(RichText::new("Steam").strong());
        ui.add_space(4.0);
        match (signed_in, client) {
            (Some(who), _) => {
                ui.label(RichText::new(format!("Signed in as {who}")).color(theme::ACCENT));
            }
            (None, true) => {
                ui.label(RichText::new("Using the running Steam client").color(theme::ACCENT));
            }
            (None, false) => {
                ui.label(
                    RichText::new("Not signed in — browsing works, downloading does not")
                        .color(theme::MUTED),
                );
            }
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .button(if signed_in.is_some() {
                    "Sign in again…"
                } else {
                    "Sign in…"
                })
                .clicked()
            {
                actions.sign_in = true;
            }
            if signed_in.is_some()
                && ui
                    .button(RichText::new("Disconnect").color(theme::DANGER))
                    .on_hover_text("Forgets the saved login on this machine")
                    .clicked()
            {
                actions.sign_out = true;
            }
        });

        ui.add_space(18.0);
    }

    fn renderer(
        &mut self,
        ui: &mut egui::Ui,
        config: &mut Config,
        backend: Option<&dyn Backend>,
        actions: &mut Actions,
    ) {
        ui.label(RichText::new("Renderer").strong());
        ui.add_space(4.0);

        let engine = haru_apply::launch::pid();
        let answering = backend.is_some_and(Backend::available);

        match (answering, engine) {
            (true, _) => Self::running(ui, backend, engine, actions),
            (false, Some(pid)) => {
                ui.label(
                    RichText::new(format!("kirie is running as pid {pid} but not answering"))
                        .color(theme::MUTED),
                );
                ui.add_space(6.0);
                if ui
                    .button(RichText::new("Stop").color(theme::DANGER))
                    .clicked()
                {
                    actions.renderer = Some(Renderer::Stop);
                }
            }
            (false, None) if haru_apply::install::installed().is_some() => {
                ui.label(
                    RichText::new(
                        "kirie is installed but not running. Wallpapers can be \
                         browsed and installed meanwhile.",
                    )
                    .color(theme::MUTED),
                );
                ui.add_space(6.0);
                if ui
                    .button("Start")
                    .on_hover_text(
                        "One engine owning every screen, showing what was on \
                         them last",
                    )
                    .clicked()
                {
                    actions.renderer = Some(Renderer::Start);
                }
            }
            (false, None) => {
                ui.label(
                    RichText::new(
                        "No renderer found. Wallpapers can still be browsed and installed.",
                    )
                    .color(theme::MUTED),
                );
                if haru_apply::install::supported() {
                    ui.add_space(6.0);
                    if ui
                        .button("Install kirie…")
                        .on_hover_text(
                            "Fetches the latest release from GitHub into \
                             ~/.local/bin",
                        )
                        .clicked()
                    {
                        actions.install = true;
                    }
                }
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
            actions.changed = true;
        }

        ui.add_space(18.0);
    }

    fn running(
        ui: &mut egui::Ui,
        backend: Option<&dyn Backend>,
        engine: Option<u32>,
        actions: &mut Actions,
    ) {
        let name = backend.map_or("The renderer", Backend::name);
        ui.label(
            RichText::new(match engine {
                Some(pid) => format!("{name} is running · pid {pid}"),
                None => format!("{name} is running"),
            })
            .color(theme::ACCENT),
        );
        if let Some(Ok(screens)) = backend.map(Backend::screens) {
            let names: Vec<&str> = screens.iter().map(|screen| screen.name.as_str()).collect();
            if !names.is_empty() {
                ui.add_space(2.0);
                ui.label(
                    RichText::new(format!("owns {}", names.join(", ")))
                        .small()
                        .color(theme::MUTED),
                );
            }
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui
                .button("Restart")
                .on_hover_text(
                    "Replaces it with one owning every screen this \
                     machine has",
                )
                .clicked()
            {
                actions.renderer = Some(Renderer::Restart);
            }
            if engine.is_some() {
                if ui
                    .button(RichText::new("Stop").color(theme::DANGER))
                    .on_hover_text("Leaves every screen without a wallpaper")
                    .clicked()
                {
                    actions.renderer = Some(Renderer::Stop);
                }
            } else {
                ui.label(
                    RichText::new(
                        "its process was not found, so it \
                                   cannot be stopped from here",
                    )
                    .small()
                    .color(theme::MUTED),
                );
            }
        });
    }

    fn installing(&mut self, ui: &mut egui::Ui, config: &mut Config, actions: &mut Actions) {
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
            actions.changed = true;
        }

        ui.add_space(18.0);
    }

    fn libraries(&mut self, ui: &mut egui::Ui, config: &mut Config, actions: &mut Actions) {
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
                    actions.changed = true;
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
                actions.changed = true;
            }
        });

        ui.add_space(18.0);
    }

    fn browsing(ui: &mut egui::Ui, config: &mut Config, actions: &mut Actions) {
        ui.label(RichText::new("Browsing").strong());
        ui.add_space(4.0);
        actions.changed |= ui
            .checkbox(&mut config.adult, "Show adult content")
            .changed();
        ui.add_space(6.0);
        actions.changed |= ui
            .checkbox(&mut config.infinite_scroll, "Keep loading as I scroll")
            .on_hover_text("Results continue instead of being paged. Numbered pages go away.")
            .changed();
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(if config.infinite_scroll {
                "Results per batch"
            } else {
                "Results per page"
            });
            actions.changed |= ui
                .add(egui::Slider::new(&mut config.per_page, 12..=100))
                .drag_stopped();
        });

        ui.add_space(18.0);
    }

    fn saving(&mut self, ui: &mut egui::Ui, config: &Config) {
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
    }
}

fn path_or_none(raw: &str) -> Option<std::path::PathBuf> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| std::path::PathBuf::from(trimmed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_box_means_the_default_rather_than_an_empty_path() {
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

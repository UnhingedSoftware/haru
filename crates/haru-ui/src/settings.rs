use egui::RichText;
use haru_apply::{Engine, Snapshot};
use haru_core::Config;

use crate::theme;

fn side(value: f64, low: &str, high: &str) -> String {
    if value.abs() < 0.001 {
        return "centre".to_owned();
    }
    let towards = if value < 0.0 { low } else { high };
    format!("{:.0}% {towards}", value.abs() * 100.0)
}

fn frames(value: f64) -> String {
    if value <= 0.0 {
        "unlimited".to_owned()
    } else {
        format!("{value:.0} fps")
    }
}

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
    pub fetch_assets: bool,
    pub tune: bool,
    pub relaunch: bool,
    pub startup: Option<bool>,
    pub register: Option<bool>,
    pub renderer: Option<Renderer>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Category {
    #[default]
    Account,
    Renderer,
    Wallpaper,
    Installing,
    Browsing,
    About,
}

impl Category {
    const ALL: [Self; 6] = [
        Self::Account,
        Self::Renderer,
        Self::Wallpaper,
        Self::Installing,
        Self::Browsing,
        Self::About,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::Account => "Account",
            Self::Renderer => "Renderer",
            Self::Wallpaper => "Wallpaper",
            Self::Installing => "Installing",
            Self::Browsing => "Browsing",
            Self::About => "Versions and files",
        }
    }
}

#[derive(Default)]
pub struct Settings {
    chosen: Category,
    update_note: String,
    renderer_seen: Option<(std::path::PathBuf, String)>,
    pending_tune: bool,
    pending_relaunch: bool,
    assets_note: String,
    socket: String,
    install: String,
    library: String,
    status: String,
}

impl Settings {
    pub fn note(&mut self, why: Option<String>) {
        self.status = why.unwrap_or_else(|| "saved".to_owned());
    }

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

    #[allow(clippy::too_many_arguments)]
    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        config: &mut Config,
        engine: &Engine,
        signed_in: Option<&str>,
        client: bool,
        assets_note: &str,
        update_note: &str,
    ) -> Actions {
        self.assets_note = assets_note.to_owned();
        self.update_note = update_note.to_owned();
        let mut actions = Actions::default();

        egui::CentralPanel::default()
            .frame(theme::panel_frame(theme::Side::Middle))
            .show(ctx, |ui| {
                ui.add_space(6.0);
                ui.heading("Settings");
                ui.add_space(12.0);

                ui.horizontal_top(|ui| {
                    self.categories(ui);
                    ui.add_space(14.0);
                    ui.separator();
                    ui.add_space(14.0);

                    ui.vertical(|ui| {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                match self.chosen {
                                    Category::Account => {
                                        self.steam(ui, signed_in, client, &mut actions);
                                    }
                                    Category::Renderer => {
                                        self.renderer(ui, config, &engine.snapshot(), &mut actions);
                                    }
                                    Category::Wallpaper => self.wallpaper(ui, config, &mut actions),
                                    Category::Installing => {
                                        self.installing(ui, config, &mut actions);
                                        ui.add_space(18.0);
                                        self.libraries(ui, config, &mut actions);
                                    }
                                    Category::Browsing => Self::browsing(ui, config, &mut actions),
                                    Category::About => {
                                        self.about(ui, config, engine, &mut actions);
                                    }
                                }
                                ui.add_space(18.0);
                                self.saving(ui, config);
                            });
                    });
                });
            });

        actions
    }

    fn categories(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            ui.set_width(160.0);
            for category in Category::ALL {
                let chosen = self.chosen == category;
                if ui
                    .add_sized(
                        [ui.available_width(), 28.0],
                        egui::SelectableLabel::new(chosen, category.label()),
                    )
                    .clicked()
                {
                    self.chosen = category;
                }
                ui.add_space(2.0);
            }
        });
    }

    fn steam(
        &mut self,
        ui: &mut egui::Ui,
        signed_in: Option<&str>,
        client: bool,
        actions: &mut Actions,
    ) {
        theme::heading(ui, "Steam");
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

    fn engine_assets(ui: &mut egui::Ui, actions: &mut Actions, note: String) {
        if !note.is_empty() {
            ui.label(RichText::new(note).small().color(theme::MUTED));
            ui.add_space(4.0);
        }
        match haru_core::engine::found() {
            Some(dir) => {
                ui.label(
                    RichText::new(format!("engine assets: {}", dir.display()))
                        .small()
                        .color(theme::MUTED),
                );
            }
            None => {
                ui.label(
                    RichText::new(
                        "Wallpaper Engine's shaders and textures are missing — haru fetches them itself.",
                    )
                    .small()
                    .color(theme::MUTED),
                );
                ui.add_space(4.0);
                if ui
                    .button("Fetch engine assets (377 MB)")
                    .on_hover_text("Downloaded with your own account; no Steam client needed")
                    .clicked()
                {
                    actions.fetch_assets = true;
                }
            }
        }
        ui.add_space(8.0);
    }

    fn as_application(ui: &mut egui::Ui, actions: &mut Actions) {
        let there = haru_apply::desktop::installed();
        ui.horizontal(|ui| {
            if there {
                ui.label(RichText::new("haru is in your applications").color(theme::MUTED));
                if ui.button("Remove").clicked() {
                    actions.register = Some(false);
                }
            } else {
                if ui
                    .button(if cfg!(target_os = "macos") {
                        "Add haru to Applications"
                    } else {
                        "Add haru to your menu"
                    })
                    .on_hover_text(if cfg!(target_os = "macos") {
                        "Builds ~/Applications/haru.app around this binary."
                    } else {
                        "Writes a desktop entry and icons for this binary."
                    })
                    .clicked()
                {
                    actions.register = Some(true);
                }
            }
        });
    }

    fn at_login(ui: &mut egui::Ui, actions: &mut Actions) {
        let mut on = haru_apply::startup::enabled();
        if ui
            .checkbox(&mut on, "Put the wallpaper up at login")
            .on_hover_text(if cfg!(target_os = "macos") {
                "Registers a launch agent that starts the renderer when you log in."
            } else {
                "Registers a user service that starts the renderer with your session."
            })
            .changed()
        {
            actions.startup = Some(on);
        }
        if on && let Some(path) = haru_apply::startup::entry() {
            ui.label(
                RichText::new(path.to_string_lossy().into_owned())
                    .small()
                    .color(theme::MUTED),
            );
        }
    }

    fn renderer(
        &mut self,
        ui: &mut egui::Ui,
        config: &mut Config,
        engine: &Snapshot,
        actions: &mut Actions,
    ) {
        theme::heading(ui, "Renderer");
        ui.add_space(4.0);
        Self::engine_assets(ui, actions, self.assets_note.clone());
        ui.add_space(6.0);
        Self::at_login(ui, actions);
        ui.add_space(6.0);
        Self::as_application(ui, actions);
        ui.add_space(6.0);

        match (engine.available, engine.pid) {
            (true, _) => Self::running(ui, engine, actions),
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
            (false, None) if engine.binary.is_some() => {
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
                        "No renderer found. Wallpapers can still be browsed and installed. \
                         If one is running anyway, `pkill -x kirie` stops it.",
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

    fn running(ui: &mut egui::Ui, engine: &Snapshot, actions: &mut Actions) {
        ui.label(
            RichText::new(match engine.pid {
                Some(pid) => format!("kirie is running · pid {pid}"),
                None => "kirie is running".to_owned(),
            })
            .color(theme::ACCENT),
        );
        {
            let names: Vec<&str> = engine
                .screens
                .iter()
                .map(|screen| screen.name.as_str())
                .collect();
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
            if engine.pid.is_some() {
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

    fn wallpaper(&mut self, ui: &mut egui::Ui, config: &mut Config, actions: &mut Actions) {
        use haru_core::renderer::{Clamp, Scaling};

        let before = config.renderer;
        theme::heading(ui, "Wallpaper");
        ui.add_space(2.0);
        ui.label(
            RichText::new("Applies to every wallpaper, whatever it asks for itself.")
                .weak()
                .size(11.0),
        );
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label("Sizing");
            egui::ComboBox::from_id_salt("haru-scaling")
                .selected_text(config.renderer.scaling.label())
                .show_ui(ui, |ui| {
                    for choice in Scaling::ALL {
                        ui.selectable_value(&mut config.renderer.scaling, choice, choice.label());
                    }
                });
        });
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Outside the edges");
            egui::ComboBox::from_id_salt("haru-clamp")
                .selected_text(config.renderer.clamp.label())
                .show_ui(ui, |ui| {
                    for choice in Clamp::ALL {
                        ui.selectable_value(&mut config.renderer.clamp, choice, choice.label());
                    }
                });
        });

        if config.renderer.scaling == Scaling::Fill {
            ui.add_space(6.0);
            ui.label(
                RichText::new("Fill crops the overflow — move the crop to keep what matters.")
                    .weak()
                    .size(11.0),
            );
            ui.horizontal(|ui| {
                ui.label("Focus across");
                ui.add(
                    egui::Slider::new(&mut config.renderer.focus_x, -1.0..=1.0)
                        .step_by(0.05)
                        .custom_formatter(|value, _| side(value, "left", "right")),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Focus down");
                ui.add(
                    egui::Slider::new(&mut config.renderer.focus_y, -1.0..=1.0)
                        .step_by(0.05)
                        .custom_formatter(|value, _| side(value, "top", "bottom")),
                );
            });
        }

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.label("Frame rate");
            ui.add(
                egui::Slider::new(&mut config.renderer.fps, 0..=240)
                    .custom_formatter(|value, _| frames(value)),
            )
            .on_hover_text("0 leaves it to the wallpaper and the screen.");
        });
        ui.horizontal(|ui| {
            ui.label("On battery");
            ui.add(
                egui::Slider::new(&mut config.renderer.battery_fps, 0..=120)
                    .custom_formatter(|value, _| frames(value)),
            )
            .on_hover_text("Used while the machine runs on battery.");
        });
        ui.horizontal(|ui| {
            ui.label("Render scale");
            ui.add(egui::Slider::new(&mut config.renderer.render_scale, 0.25..=2.0).step_by(0.05))
                .on_hover_text("Below 1.0 draws smaller and scales up: cheaper, softer.");
        });
        ui.horizontal(|ui| {
            ui.label("Speed");
            ui.add(egui::Slider::new(&mut config.renderer.playback_speed, 0.1..=4.0).step_by(0.1));
        });

        ui.add_space(10.0);
        ui.checkbox(&mut config.renderer.mute, "Mute");
        ui.add_enabled_ui(!config.renderer.mute, |ui| {
            ui.horizontal(|ui| {
                ui.label("Volume");
                ui.add(egui::Slider::new(&mut config.renderer.volume, 0..=100));
            });
        });

        ui.add_space(10.0);
        ui.checkbox(&mut config.renderer.disable_parallax, "No parallax")
            .on_hover_text("Stops the scene leaning towards the pointer.");
        ui.checkbox(&mut config.renderer.disable_mouse, "Ignore the pointer");
        ui.checkbox(
            &mut config.renderer.interactive,
            "Let wallpapers take clicks",
        )
        .on_hover_text(
            "Interactive wallpapers become clickable. Clicks on the empty desktop go to the \
             wallpaper instead of the desktop itself.",
        );
        ui.checkbox(&mut config.renderer.disable_particles, "No particles");
        ui.checkbox(
            &mut config.renderer.no_automute,
            "Keep sound when something else plays",
        );
        ui.checkbox(
            &mut config.renderer.no_audio_processing,
            "No audio reactivity",
        )
        .on_hover_text("Wallpapers that dance to sound stop listening.");
        ui.checkbox(
            &mut config.renderer.no_fullscreen_pause,
            "Keep drawing behind fullscreen windows",
        );

        if config.renderer != before {
            self.pending_tune = true;
            self.pending_relaunch |= before.needs_relaunch(&config.renderer);
        }

        let settling = ui.ctx().input(|input| input.pointer.any_down());
        if self.pending_tune && !settling {
            actions.tune = true;
            actions.relaunch |= self.pending_relaunch;
            self.pending_tune = false;
            self.pending_relaunch = false;
        }
    }

    fn installing(&mut self, ui: &mut egui::Ui, config: &mut Config, actions: &mut Actions) {
        theme::heading(ui, "Installing");
        ui.add_space(4.0);
        ui.label(
            RichText::new(
                "Blank installs into your Steam library if you have one, otherwise into haru's own folder.",
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
        theme::heading(ui, "Steam libraries");
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
        theme::heading(ui, "Browsing");
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
        actions.changed |= ui
            .checkbox(
                &mut config.fit_per_page,
                "Ask for as many as the grid holds",
            )
            .on_hover_text("A page fills the window: more on a wide one, fewer on a small one.")
            .changed();
        ui.add_space(6.0);
        ui.add_enabled_ui(!config.fit_per_page, |ui| {
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
        });

        ui.add_space(18.0);
    }

    fn about(
        &mut self,
        ui: &mut egui::Ui,
        config: &mut Config,
        engine: &Engine,
        actions: &mut Actions,
    ) {
        theme::heading(ui, "Versions and files");
        ui.add_space(6.0);

        if !self.update_note.is_empty() {
            ui.label(RichText::new(&self.update_note).color(theme::ACCENT));
            ui.add_space(6.0);
        }
        actions.changed |= ui
            .checkbox(
                &mut config.auto_update,
                "Keep haru and the renderer up to date",
            )
            .on_hover_text("Fetches the renderer when it is missing, and newer releases of both.")
            .changed();
        ui.add_space(8.0);

        let mine = std::env::current_exe().ok();
        Self::entry(
            ui,
            &format!("haru {}", env!("CARGO_PKG_VERSION")),
            mine.as_deref(),
        );

        let binary = engine.snapshot().binary;
        let version = binary
            .as_deref()
            .and_then(|path| self.renderer_version(path));
        let named = match version {
            Some(version) => format!("kirie {version}"),
            None if binary.is_some() => "kirie (version unknown)".to_owned(),
            None => "kirie is not installed".to_owned(),
        };
        Self::entry(ui, &named, binary.as_deref());

        ui.add_space(6.0);
        for (what, path) in [
            ("wallpapers", haru_core::Config::load().install_root()),
            ("engine assets", haru_core::engine::assets_home()),
            ("settings", Config::path()),
            (
                "at login",
                haru_apply::startup::enabled()
                    .then(haru_apply::startup::entry)
                    .flatten(),
            ),
            (
                "in applications",
                haru_apply::desktop::installed()
                    .then(haru_apply::desktop::entry)
                    .flatten(),
            ),
        ] {
            if let Some(path) = path {
                Self::entry(ui, what, Some(&path));
            }
        }
    }

    fn entry(ui: &mut egui::Ui, what: &str, path: Option<&std::path::Path>) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(what).color(theme::MUTED));
            if let Some(path) = path {
                let text = path.to_string_lossy().into_owned();
                let shown = RichText::new(&text).small().color(if path.exists() {
                    theme::MUTED
                } else {
                    theme::DANGER
                });
                if ui.selectable_label(false, shown).clicked() {
                    ui.ctx().copy_text(text);
                }
            }
        });
    }

    fn renderer_version(&mut self, binary: &std::path::Path) -> Option<String> {
        if let Some((seen, version)) = &self.renderer_seen
            && seen == binary
        {
            return Some(version.clone());
        }
        let version = haru_apply::install::version_of(binary)?;
        self.renderer_seen = Some((binary.to_path_buf(), version.clone()));
        Some(version)
    }

    fn saving(&mut self, ui: &mut egui::Ui, config: &Config) {
        ui.separator();
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui
                .add(
                    egui::Button::new(RichText::new("Save").size(12.5).color(egui::Color32::WHITE))
                        .fill(theme::ACCENT)
                        .min_size(egui::vec2(110.0, 30.0)),
                )
                .clicked()
            {
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

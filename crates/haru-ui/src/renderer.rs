use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

use egui::{Align, Color32, Layout, RichText};
use haru_apply::install::{self, Web};
use haru_core::human_size;

use crate::theme;

enum Phase {
    Choosing,
    Working(u64, u64),
    /// Installed, and anything that went wrong on the side: the WebView2
    /// Runtime failing to install does not undo a good kirie install.
    Done(PathBuf, Option<String>),
    Failed(String),
}

enum Note {
    Progress(u64, u64),
    Done(PathBuf, Option<String>),
    Failed(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Nothing,
    Dismissed,
    /// Installed, with the flavour and whether it was a beta, so the updater
    /// keeps following the channel the user picked here.
    Installed(Web, bool),
}

pub struct Installer {
    open: bool,
    web: Web,
    webkit: bool,
    betas: bool,
    /// Whether web wallpapers' runtime is missing here, which only Windows
    /// asks: `None` until the prompt is first offered.
    webview_missing: Option<bool>,
    /// Whether to install it along with kirie.
    webview: bool,
    phase: Phase,
    notes: Option<Receiver<Note>>,
}

impl Default for Installer {
    fn default() -> Self {
        Self::new()
    }
}

impl Installer {
    #[must_use]
    pub fn new() -> Self {
        let webkit = install::webkit_present();
        Self {
            open: false,
            web: if webkit { Web::WebKit } else { Web::Cef },
            webkit,
            betas: false,
            webview_missing: None,
            webview: true,
            phase: Phase::Choosing,
            notes: None,
        }
    }

    /// Opens the prompt. `betas` is where the beta choice starts, which is
    /// whatever the updater is already set to follow.
    pub fn offer(&mut self, betas: bool) {
        self.betas = betas;
        if haru_apply::webview2::needed() && self.webview_missing.is_none() {
            self.webview_missing = Some(haru_apply::webview2::installed().is_none());
        }
        self.open = true;
        self.phase = Phase::Choosing;
    }

    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }

    pub fn ui(&mut self, ctx: &egui::Context) -> Outcome {
        self.collect(ctx);
        if !self.open {
            return Outcome::Nothing;
        }

        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("renderer-shade"))
            .order(egui::Order::Background)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.painter()
                    .rect_filled(screen, 0.0, Color32::from_black_alpha(180));
            });

        let mut outcome = Outcome::Nothing;
        let mut close = false;
        let mut start = false;

        egui::Window::new("install a renderer")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(420.0)
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(theme::MODAL)
                    .inner_margin(egui::Margin::same(18.0)),
            )
            .show(ctx, |ui| {
                ui.set_max_width(420.0);
                ui.horizontal(|ui| {
                    ui.heading("haru needs a renderer");
                    ui.with_layout(Layout::right_to_left(Align::TOP), |ui| {
                        if crate::icons::button(ui, crate::icons::Icon::Close, false).clicked() {
                            close = true;
                        }
                    });
                });
                ui.add_space(2.0);
                let into = install::destination()
                    .and_then(|path| path.parent().map(|dir| dir.display().to_string()))
                    .unwrap_or_else(|| "your user folder".to_owned());
                ui.label(
                    RichText::new(format!(
                        "Wallpapers are drawn by kirie, and this machine does not have it. \
                         haru can fetch the latest release into {into}."
                    ))
                    .small()
                    .color(theme::MUTED),
                );
                ui.add_space(14.0);

                start |= self.phase_ui(ui, &mut close);
            });

        if start {
            self.start(ctx);
        }
        if close {
            self.open = false;
            outcome = match self.phase {
                Phase::Done(..) => Outcome::Installed(self.web, self.betas),
                _ => Outcome::Dismissed,
            };
        }
        outcome
    }

    fn phase_ui(&mut self, ui: &mut egui::Ui, close: &mut bool) -> bool {
        let mut start = false;
        match &self.phase {
            Phase::Choosing => {
                start = self.choices(ui);
            }
            Phase::Working(done, total) => {
                ui.label(RichText::new("Downloading…").strong());
                ui.add_space(6.0);
                let fraction = if *total == 0 {
                    0.0
                } else {
                    (*done as f32 / *total as f32).clamp(0.0, 1.0)
                };
                ui.add(egui::ProgressBar::new(fraction).desired_height(8.0));
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!("{} of {}", human_size(*done), human_size(*total)))
                        .small()
                        .color(theme::MUTED),
                );
            }
            Phase::Done(path, aside) => {
                ui.label(
                    RichText::new(format!("Installed to {}", path.display())).color(theme::ACCENT),
                );
                if let Some(aside) = aside {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!(
                            "Web wallpapers will not run yet: {aside}. Settings › Renderer can \
                             try again."
                        ))
                        .small()
                        .color(theme::DANGER),
                    );
                }
                ui.add_space(4.0);
                ui.label(
                    RichText::new(
                        "haru uses it as soon as it is running — start it, or let \
                 whatever puts your wallpaper up at login do it.",
                    )
                    .small()
                    .color(theme::MUTED),
                );
                ui.add_space(12.0);
                if ui
                    .add_sized([ui.available_width(), 30.0], egui::Button::new("Close"))
                    .clicked()
                {
                    *close = true;
                }
            }
            Phase::Failed(why) => {
                ui.label(RichText::new(why).color(theme::DANGER));
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Try again").clicked() {
                        start = true;
                    }
                    if ui.button("Not now").clicked() {
                        *close = true;
                    }
                });
            }
        }
        start
    }

    fn choices(&mut self, ui: &mut egui::Ui) -> bool {
        // Only Linux publishes two builds to choose between; macOS and Windows
        // get the one their release carries whichever card is picked.
        if cfg!(target_os = "linux") {
            self.flavours(ui);
        }

        if self.webview_missing == Some(true) {
            ui.add_space(2.0);
            ui.checkbox(
                &mut self.webview,
                "Also install WebView2, for web wallpapers",
            )
            .on_hover_text(
                "Web wallpapers run in the Microsoft Edge WebView2 Runtime, which this \
                     machine does not have. haru downloads Microsoft's installer, checks it \
                     is signed by Microsoft, and Windows asks for administrator permission \
                     before it runs.",
            );
        }

        ui.add_space(2.0);
        ui.checkbox(&mut self.betas, "Install the beta")
            .on_hover_text(
                "Takes the newest pre-release instead of the newest stable release, \
                 and keeps updating to betas afterwards. Change this later in Settings.",
            );

        ui.add_space(8.0);
        let mut start = false;
        ui.horizontal(|ui| {
            if ui
                .add_sized([200.0, 32.0], egui::Button::new("Install kirie"))
                .clicked()
            {
                start = true;
            }
            if ui.button("Not now").clicked() {
                self.open = false;
            }
        });
        start
    }

    fn flavours(&mut self, ui: &mut egui::Ui) {
        let found = self.webkit;
        for (web, note) in [
            (
                Web::WebKit,
                if found {
                    "Uses the WebKitGTK this machine already has · 32 MB"
                } else {
                    "WebKitGTK was not found here — web wallpapers would not run · 32 MB"
                },
            ),
            (
                Web::Cef,
                "Brings its own Chromium and needs nothing installed · 112 MB",
            ),
        ] {
            let chosen = self.web == web;
            let response = ui.add_sized(
                [ui.available_width(), 46.0],
                egui::SelectableLabel::new(chosen, ""),
            );
            let inner = response.rect.shrink2(egui::vec2(10.0, 6.0));
            ui.painter().text(
                inner.left_top(),
                egui::Align2::LEFT_TOP,
                web.label(),
                egui::FontId::proportional(14.0),
                ui.visuals().text_color(),
            );
            ui.painter().text(
                inner.left_bottom(),
                egui::Align2::LEFT_BOTTOM,
                note,
                egui::FontId::proportional(11.0),
                theme::MUTED,
            );
            if response.clicked() {
                self.web = web;
            }
            ui.add_space(6.0);
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        let (notes, heard) = channel();
        let web = self.web;
        let betas = self.betas;
        let webview = self.webview && self.webview_missing == Some(true);
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("haru-install".to_owned())
            .spawn(move || {
                let newest = if betas {
                    install::latest_including_betas(web)
                } else {
                    install::latest(web)
                };
                let build = match newest {
                    Ok(build) => build,
                    Err(why) => {
                        let _ = notes.send(Note::Failed(why));
                        ctx.request_repaint();
                        return;
                    }
                };
                let mut report = |done, total| {
                    let _ = notes.send(Note::Progress(done, total));
                    ctx.request_repaint();
                };
                let Some(target) = install::destination() else {
                    let _ =
                        notes.send(Note::Failed("no home directory to install into".to_owned()));
                    ctx.request_repaint();
                    return;
                };
                let note = match install::fetch(&build, &target, &mut report) {
                    Ok(path) if webview => {
                        let aside = haru_apply::webview2::install(&mut report).err();
                        Note::Done(path, aside)
                    }
                    Ok(path) => Note::Done(path, None),
                    Err(why) => Note::Failed(why),
                };
                let _ = notes.send(note);
                ctx.request_repaint();
            });

        if spawned.is_ok() {
            self.notes = Some(heard);
            self.phase = Phase::Working(0, 0);
        } else {
            self.phase = Phase::Failed("could not start the download".to_owned());
        }
    }

    fn collect(&mut self, ctx: &egui::Context) {
        let Some(notes) = self.notes.as_ref() else {
            return;
        };
        let mut finished = false;
        while let Ok(note) = notes.try_recv() {
            match note {
                Note::Progress(done, total) => self.phase = Phase::Working(done, total),
                Note::Done(path, aside) => {
                    if aside.is_none() {
                        self.webview_missing = self.webview_missing.map(|_| false);
                    }
                    self.phase = Phase::Done(path, aside);
                    finished = true;
                }
                Note::Failed(why) => {
                    self.phase = Phase::Failed(why);
                    finished = true;
                }
            }
        }
        if finished {
            self.notes = None;
        } else if matches!(self.phase, Phase::Working(_, _)) {
            ctx.request_repaint_after(std::time::Duration::from_millis(120));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_opens_on_the_build_this_machine_suggests() {
        let installer = Installer::new();
        assert!(!installer.is_open(), "nothing is asked until it is offered");
        assert_eq!(
            installer.web,
            if install::webkit_present() {
                Web::WebKit
            } else {
                Web::Cef
            },
            "the suggestion is the machine's own answer"
        );
    }

    #[test]
    fn offering_it_starts_at_the_choice() {
        let mut installer = Installer::new();
        installer.phase = Phase::Failed("earlier".to_owned());
        installer.offer(false);
        assert!(installer.is_open());
        assert!(matches!(installer.phase, Phase::Choosing));
    }

    #[test]
    fn the_beta_choice_starts_where_the_updater_is() {
        let mut installer = Installer::new();
        installer.offer(true);
        assert!(installer.betas);
        installer.offer(false);
        assert!(!installer.betas);
    }
}

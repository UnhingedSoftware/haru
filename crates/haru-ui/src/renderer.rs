//! The first-run offer to install a renderer.
//!
//! haru draws no wallpapers. Without kirie it can browse, download and manage
//! a library and put none of it on a screen, which is a strange thing for a
//! wallpaper picker to be — so on a machine that has never had one, it offers
//! to fetch it.
//!
//! Offers, rather than does. A background download of 32 MB that lands an
//! executable in someone's `~/.local/bin` is not a thing to do quietly, so the
//! machine is asked which build fits it, the answer is preselected, and a
//! person confirms it.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};

use egui::{Align, Color32, Layout, RichText};
use haru_apply::install::{self, Web};
use haru_core::human_size;

use crate::theme;

/// What the overlay is doing.
enum Phase {
    /// Waiting for someone to pick a build.
    Choosing,
    /// Fetching, with bytes so far and expected.
    Working(u64, u64),
    /// Installed, and where.
    Done(PathBuf),
    /// Why it did not happen.
    Failed(String),
}

/// What the worker thread reports back.
enum Note {
    /// Bytes so far, bytes expected.
    Progress(u64, u64),
    /// Installed, and where it went.
    Done(PathBuf),
    /// Why it stopped.
    Failed(String),
}

/// What the overlay decided this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Still open, or not open at all.
    Nothing,
    /// Turned down. Worth remembering, so it is not asked at every start.
    Dismissed,
    /// A renderer was installed, of this flavour.
    Installed(Web),
}

/// The offer.
pub struct Installer {
    /// Whether it is on screen.
    open: bool,
    /// Which build is selected.
    web: Web,
    /// Whether the machine has a webkit to load.
    webkit: bool,
    /// What it is doing.
    phase: Phase,
    /// Where the worker's notes arrive.
    notes: Option<Receiver<Note>>,
}

impl Default for Installer {
    fn default() -> Self {
        Self::new()
    }
}

impl Installer {
    /// A closed offer, with the build this machine suggests already chosen.
    #[must_use]
    pub fn new() -> Self {
        let webkit = install::webkit_present();
        Self {
            open: false,
            web: if webkit { Web::WebKit } else { Web::Cef },
            webkit,
            phase: Phase::Choosing,
            notes: None,
        }
    }

    /// Puts the offer on screen.
    pub fn offer(&mut self) {
        self.open = true;
        self.phase = Phase::Choosing;
    }

    /// Whether it is on screen, so nothing else opens over it.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// Draws the overlay and reports anything it settled.
    pub fn ui(&mut self, ctx: &egui::Context) -> Outcome {
        self.collect(ctx);
        if !self.open {
            return Outcome::Nothing;
        }

        // Everything behind it is dimmed and unclickable: this is in the way
        // on purpose, and a half-interactive modal is worse than either kind.
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
                ui.label(
                    RichText::new(
                        "Wallpapers are drawn by kirie, and this machine does not have it. \
                         haru can fetch the latest release into ~/.local/bin.",
                    )
                    .small()
                    .color(theme::MUTED),
                );
                ui.add_space(14.0);

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
                            RichText::new(format!(
                                "{} of {}",
                                human_size(*done),
                                human_size(*total)
                            ))
                            .small()
                            .color(theme::MUTED),
                        );
                    }
                    Phase::Done(path) => {
                        ui.label(
                            RichText::new(format!("Installed to {}", path.display()))
                                .color(theme::ACCENT),
                        );
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
                            close = true;
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
                                close = true;
                            }
                        });
                    }
                }
            });

        if start {
            self.start(ctx);
        }
        if close {
            self.open = false;
            outcome = match self.phase {
                // Installed and acknowledged: remember the flavour, so an
                // update later fetches the same one.
                Phase::Done(_) => Outcome::Installed(self.web),
                // Turned down: do not ask again at every start.
                _ => Outcome::Dismissed,
            };
        }
        outcome
    }

    /// The two builds, as something to pick between. Returns whether to fetch.
    fn choices(&mut self, ui: &mut egui::Ui) -> bool {
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
            // Painted over the label rather than into it: two lines of
            // different sizes is not something a `SelectableLabel` draws.
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

    /// Fetches the chosen build on a thread of its own.
    fn start(&mut self, ctx: &egui::Context) {
        let (notes, heard) = channel();
        let web = self.web;
        let ctx = ctx.clone();
        let spawned = std::thread::Builder::new()
            .name("haru-install".to_owned())
            .spawn(move || {
                let build = match install::latest(web) {
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
                    Ok(path) => Note::Done(path),
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

    /// Takes whatever the worker has said since the last frame.
    fn collect(&mut self, ctx: &egui::Context) {
        let Some(notes) = self.notes.as_ref() else {
            return;
        };
        let mut finished = false;
        while let Ok(note) = notes.try_recv() {
            match note {
                Note::Progress(done, total) => self.phase = Phase::Working(done, total),
                Note::Done(path) => {
                    self.phase = Phase::Done(path);
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
            // A download reports from another thread, and egui only draws when
            // something happens to it.
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
        installer.offer();
        assert!(installer.is_open());
        assert!(matches!(installer.phase, Phase::Choosing));
    }
}

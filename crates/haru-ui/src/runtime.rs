//! The WebView2 Runtime row in Settings: whether web wallpapers can run on this
//! Windows machine, and a button that installs what they need.
//!
//! Checking means asking the registry through `reg`, which is a process start,
//! so it happens once and again only after an install.

use std::sync::mpsc::{Receiver, channel};

use egui::RichText;
use haru_apply::webview2;

use crate::theme;

#[derive(Default)]
pub struct WebRuntime {
    version: Option<Option<String>>,
    work: Option<Receiver<Result<String, String>>>,
    note: String,
}

impl WebRuntime {
    /// The installed runtime's version, checking the first time it is asked.
    pub fn version(&mut self) -> Option<&str> {
        self.version
            .get_or_insert_with(webview2::installed)
            .as_deref()
    }

    #[must_use]
    pub const fn busy(&self) -> bool {
        self.work.is_some()
    }

    /// Download and run Microsoft's installer in the background. Windows puts
    /// its administrator prompt up on its own.
    pub fn install(&mut self, ctx: &egui::Context) {
        if self.busy() {
            return;
        }
        let (tell, heard) = channel();
        let ctx = ctx.clone();
        let started = std::thread::Builder::new()
            .name("haru-webview2".to_owned())
            .spawn(move || {
                let _ = tell.send(webview2::install(&mut |_, _| {}));
                ctx.request_repaint();
            });
        match started {
            Ok(_) => {
                self.work = Some(heard);
                self.note =
                    "Downloading the installer; Windows will ask for permission…".to_owned();
            }
            Err(_) => self.note = "could not start the install".to_owned(),
        }
    }

    fn collect(&mut self) {
        let Some(work) = self.work.as_ref() else {
            return;
        };
        let Ok(outcome) = work.try_recv() else {
            return;
        };
        self.work = None;
        match outcome {
            Ok(version) => {
                self.note = String::new();
                self.version = Some(Some(version));
            }
            Err(why) => {
                self.note = why;
                self.version = None;
            }
        }
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        if !webview2::needed() {
            return;
        }
        self.collect();
        if self.busy() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(250));
        }

        match self.version().map(str::to_owned) {
            Some(version) => {
                ui.label(
                    RichText::new(format!("Web wallpapers: WebView2 {version} is installed"))
                        .color(theme::MUTED),
                );
            }
            None => {
                ui.label(
                    RichText::new(
                        "Web wallpapers need the Microsoft Edge WebView2 Runtime, \
                         which this machine does not have.",
                    )
                    .color(theme::MUTED),
                );
                ui.add_space(4.0);
                let button = ui
                    .add_enabled(!self.busy(), egui::Button::new("Install WebView2…"))
                    .on_hover_text(
                        "Downloads Microsoft's installer, checks it is signed by \
                         Microsoft, and runs it. Windows asks for administrator \
                         permission first.",
                    );
                if button.clicked() {
                    self.install(ui.ctx());
                }
            }
        }
        if !self.note.is_empty() {
            ui.add_space(2.0);
            ui.label(RichText::new(&self.note).small().color(theme::MUTED));
        }
    }
}

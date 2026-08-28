//! Looking at a wallpaper, and changing it, without putting it up.
//!
//! The seed of the studio. A wallpaper is rendered off-screen, its own
//! properties are on the right, and moving one re-renders — none of which
//! touches whatever is actually on your screens.
//!
//! Rendering happens on a worker thread, one frame at a time, and only the
//! newest request survives: dragging a slider produces a request per frame,
//! and rendering every one of them would fall further behind the longer you
//! drag.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use egui::{Align, Layout, RichText};
use haru_apply::Offscreen;
use haru_core::{Installed, properties};

use crate::theme;

/// What the worker was asked for.
struct Job {
    dir: PathBuf,
    properties: Vec<(String, String)>,
    /// Which request this is, so a stale answer can be dropped.
    seq: u64,
}

/// What came back.
struct Frame {
    seq: u64,
    /// Where the frame landed, or why there is none.
    result: Result<PathBuf, String>,
    /// How long it took, which is worth showing while it is seconds.
    took: std::time::Duration,
}

/// The preview view.
pub struct Preview {
    /// What is being looked at.
    item: Option<Installed>,
    /// Its properties, as edited here rather than on disk.
    settings: Vec<properties::Property>,
    /// The frame on screen, and the request it answered.
    shown: Option<PathBuf>,
    /// A number for the texture, so a re-render is not mistaken for the cache.
    generation: u64,
    texture: Option<egui::TextureHandle>,
    status: String,
    /// The newest request, and whether it is still out.
    seq: u64,
    waiting: bool,
    took: Option<std::time::Duration>,
    /// The worker's inbox: only ever holds the latest request.
    pending: Arc<Mutex<Option<Job>>>,
    frames: Receiver<Frame>,
    wake: Sender<()>,
}

impl Default for Preview {
    fn default() -> Self {
        Self::new()
    }
}

impl Preview {
    /// An empty preview with its renderer waiting.
    #[must_use]
    pub fn new() -> Self {
        let pending: Arc<Mutex<Option<Job>>> = Arc::new(Mutex::new(None));
        let (frames_out, frames) = channel::<Frame>();
        let (wake, woken) = channel::<()>();

        let worker_pending = Arc::clone(&pending);
        std::thread::Builder::new()
            .name("haru-preview-render".to_owned())
            .spawn(move || worker(&worker_pending, &frames_out, &woken))
            .ok();

        Self {
            item: None,
            settings: Vec::new(),
            shown: None,
            generation: 0,
            texture: None,
            status: String::new(),
            seq: 0,
            waiting: false,
            took: None,
            pending,
            frames,
            wake,
        }
    }

    /// What is being previewed, if anything.
    #[must_use]
    pub fn item(&self) -> Option<&Installed> {
        self.item.as_ref()
    }

    /// Opens a wallpaper, reading its properties from disk.
    pub fn open(&mut self, item: Installed) {
        self.settings = properties::read(&item.dir);
        self.item = Some(item);
        self.texture = None;
        self.shown = None;
        self.status = String::new();
        self.request();
    }

    /// Asks for a frame with whatever the properties are now.
    fn request(&mut self) {
        let Some(item) = self.item.clone() else {
            return;
        };
        self.seq = self.seq.saturating_add(1);
        let job = Job {
            dir: item.dir,
            properties: self
                .settings
                .iter()
                .map(|property| (property.key.clone(), property.wire()))
                .collect(),
            seq: self.seq,
        };

        // Replaced rather than queued: a slider drag makes one of these per
        // frame, and the only one worth rendering is the last.
        if let Ok(mut pending) = self.pending.lock() {
            *pending = Some(job);
        }
        self.waiting = true;
        let _ = self.wake.send(());
    }

    /// Draws the view.
    pub fn ui(&mut self, ctx: &egui::Context, sidebar: bool) {
        self.collect(ctx);

        if sidebar {
            egui::SidePanel::right("preview-properties")
                .resizable(false)
                .exact_width(300.0)
                .frame(theme::panel_frame(theme::Side::Right))
                .show(ctx, |ui| self.properties(ui));
        }

        egui::TopBottomPanel::bottom("preview-status")
            .frame(theme::panel_frame(theme::Side::Left))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if self.waiting {
                        ui.spinner();
                        ui.label("rendering…");
                    } else if !self.status.is_empty() {
                        ui.label(RichText::new(&self.status).color(theme::MUTED));
                    }
                    if let Some(took) = self.took {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.weak(format!("{:.1}s per frame", took.as_secs_f32()));
                        });
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(theme::panel_frame(theme::Side::Middle))
            .show(ctx, |ui| self.canvas(ui));
    }

    /// Takes any finished frame.
    fn collect(&mut self, ctx: &egui::Context) {
        while let Ok(frame) = self.frames.try_recv() {
            // An answer to an edit that has already been superseded.
            if frame.seq != self.seq {
                continue;
            }
            self.waiting = false;
            self.took = Some(frame.took);
            match frame.result {
                Ok(path) => {
                    self.status.clear();
                    self.shown = Some(path);
                    self.generation = self.generation.saturating_add(1);
                    self.texture = None;
                }
                Err(why) => self.status = why,
            }
        }

        // Loading here rather than in the paint: a texture belongs to the
        // context, and this is the one place that has both.
        if self.texture.is_none()
            && let Some(path) = self.shown.clone()
            && let Ok(bytes) = std::fs::read(&path)
            && let Ok(decoded) = image::load_from_memory(&bytes)
        {
            let rgba = decoded.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            let image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
            self.texture = Some(ctx.load_texture(
                format!("preview-{}", self.generation),
                image,
                egui::TextureOptions::LINEAR,
            ));
        }
    }

    /// The frame itself.
    fn canvas(&mut self, ui: &mut egui::Ui) {
        let Some(item) = self.item.clone() else {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("Pick a wallpaper in the Library and press Preview.")
                        .color(theme::MUTED),
                );
            });
            return;
        };

        ui.horizontal(|ui| {
            ui.heading(RichText::new(&item.title).size(16.0));
            ui.label(RichText::new(&item.kind).small().color(theme::MUTED));
        });
        ui.add_space(8.0);

        match self.texture.clone() {
            Some(texture) => {
                ui.centered_and_justified(|ui| {
                    ui.add(
                        egui::Image::new(&texture)
                            .maintain_aspect_ratio(true)
                            .fit_to_fraction(egui::vec2(1.0, 1.0))
                            .rounding(8.0),
                    );
                });
            }
            None => {
                ui.centered_and_justified(|ui| {
                    if self.status.is_empty() {
                        ui.spinner();
                    } else {
                        ui.label(RichText::new(&self.status).color(ui.visuals().error_fg_color));
                    }
                });
            }
        }
    }

    /// The wallpaper's own settings, edited here and nowhere else.
    fn properties(&mut self, ui: &mut egui::Ui) {
        ui.heading("Properties");
        ui.add_space(2.0);
        ui.label(
            RichText::new("Changes here are previewed only — nothing on your screens moves.")
                .small()
                .color(theme::MUTED),
        );
        ui.add_space(8.0);

        if self.item.is_none() {
            return;
        }
        if self.settings.is_empty() {
            ui.label(
                RichText::new("This wallpaper has no settings.")
                    .small()
                    .color(theme::MUTED),
            );
            return;
        }

        let mut edited = false;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for property in &mut self.settings {
                    edited |= crate::widgets::property(ui, property);
                    ui.add_space(6.0);
                }

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(6.0);
                if ui.button("Reset to defaults").clicked()
                    && let Some(item) = self.item.as_ref()
                {
                    self.settings = properties::read(&item.dir);
                    edited = true;
                }
            });

        if edited {
            self.request();
        }
    }
}

/// The render loop: newest request only, one at a time.
fn worker(pending: &Arc<Mutex<Option<Job>>>, frames: &Sender<Frame>, woken: &Receiver<()>) {
    let offscreen = Offscreen::new(None);
    let out = std::env::temp_dir().join("haru-preview.png");

    while woken.recv().is_ok() {
        // Whatever is there now, not whatever was there when the wake was
        // sent: several edits may have landed while the last frame rendered.
        let Some(job) = pending.lock().ok().and_then(|mut held| held.take()) else {
            continue;
        };

        let started = std::time::Instant::now();
        let result = offscreen
            .render(&job.dir, &job.properties, &out)
            .map(|()| out.clone());
        let frame = Frame {
            seq: job.seq,
            result,
            took: started.elapsed(),
        };
        if frames.send(frame).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preview_with_nothing_open_asks_for_nothing() {
        let mut preview = Preview::new();
        preview.request();
        assert!(!preview.waiting, "a request went out with no wallpaper");
    }

    #[test]
    fn only_the_newest_edit_is_kept_to_render() {
        // A slider drag makes one request per frame; rendering each in turn
        // would fall further behind the longer the drag lasts.
        let preview = Preview::new();
        if let Ok(mut pending) = preview.pending.lock() {
            *pending = Some(Job {
                dir: PathBuf::from("/tmp/a"),
                properties: Vec::new(),
                seq: 1,
            });
            *pending = Some(Job {
                dir: PathBuf::from("/tmp/b"),
                properties: Vec::new(),
                seq: 2,
            });
            assert_eq!(pending.as_ref().map(|job| job.seq), Some(2));
        }
    }
}

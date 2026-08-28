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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use egui::{Align, Layout, RichText};
use haru_apply::{Offscreen, PreviewStream};
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
    /// The frame, or why there is none.
    result: Result<Rendered, String>,
    /// How long it took, which is worth showing while it is seconds.
    took: std::time::Duration,
}

/// How a frame arrived.
enum Rendered {
    /// Pixels straight from a running renderer.
    Live(haru_apply::Frame),
    /// A PNG on disk, from a renderer with no streaming mode.
    Still(PathBuf),
}

/// The preview view.
pub struct Preview {
    /// What is being looked at.
    item: Option<Installed>,
    /// Its properties, as edited here rather than on disk.
    settings: Vec<properties::Property>,
    /// The frame on screen, waiting to become a texture.
    shown: Option<Rendered>,
    /// A number for the texture, so a re-render is not mistaken for the cache.
    generation: u64,
    texture: Option<egui::TextureHandle>,
    status: String,
    /// The newest request, and whether it is still out.
    seq: u64,
    waiting: bool,
    took: Option<std::time::Duration>,
    /// Whether the preview is on screen.
    ///
    /// A renderer holds a wallpaper's textures — hundreds of megabytes for a
    /// large scene — and holding them while another tab is showing is holding
    /// them for nobody.
    watching: Arc<AtomicBool>,
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

        let watching = Arc::new(AtomicBool::new(false));
        let worker_pending = Arc::clone(&pending);
        let worker_watching = Arc::clone(&watching);
        std::thread::Builder::new()
            .name("haru-preview-render".to_owned())
            .spawn(move || worker(&worker_pending, &worker_watching, &frames_out, &woken))
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
            watching,
            pending,
            frames,
            wake,
        }
    }

    /// Stops the renderer until the preview is looked at again.
    ///
    /// Called when another tab takes over. The wallpaper's textures go with
    /// it; coming back costs the second it takes to build the scene again.
    pub fn suspend(&mut self) {
        self.watching.store(false, Ordering::Relaxed);
        let _ = self.wake.send(());
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
        self.watching.store(true, Ordering::Relaxed);
        self.collect(ctx);

        // egui draws on events, and frames arriving on a channel are not one.
        // Without this a streamed preview shows its first frame and stops.
        if self.item.is_some() {
            ctx.request_repaint();
        }

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
            // An answer to an edit that has already been superseded. Frames
            // that keep arriving carry the seq of the edit they show, so a
            // running stream matches until the next edit replaces it.
            if frame.seq != self.seq {
                continue;
            }
            self.waiting = false;
            self.took = Some(frame.took);
            match frame.result {
                Ok(rendered) => {
                    self.status.clear();
                    self.shown = Some(rendered);
                    self.generation = self.generation.saturating_add(1);
                    self.texture = None;
                }
                Err(why) => self.status = why,
            }
        }

        // Uploaded here rather than in the paint: a texture belongs to the
        // context, and this is the one place that has both.
        if self.texture.is_none()
            && let Some(rendered) = self.shown.take()
        {
            let image = match rendered {
                Rendered::Live(frame) => Some(egui::ColorImage::from_rgba_unmultiplied(
                    [frame.width as usize, frame.height as usize],
                    &frame.pixels,
                )),
                Rendered::Still(path) => std::fs::read(&path)
                    .ok()
                    .and_then(|bytes| image::load_from_memory(&bytes).ok())
                    .map(|decoded| {
                        let rgba = decoded.to_rgba8();
                        let size = [rgba.width() as usize, rgba.height() as usize];
                        egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw())
                    }),
            };
            if let Some(image) = image {
                self.texture = Some(ctx.load_texture(
                    format!("preview-{}", self.generation),
                    image,
                    egui::TextureOptions::LINEAR,
                ));
            }
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

/// The render loop.
///
/// Two ways of getting frames, and the good one is tried first: a renderer
/// with `preview` streams them, so the wallpaper animates and an edit costs a
/// rebuild. An older renderer has no such mode, and one screenshot per edit is
/// the same picture arriving slower.
///
/// While a stream is up this loop never blocks on the UI — it reads frames as
/// fast as the renderer sends them and checks for edits between each. With no
/// stream there is nothing to do until something is asked for.
fn worker(
    pending: &Arc<Mutex<Option<Job>>>,
    watching: &Arc<AtomicBool>,
    frames: &Sender<Frame>,
    woken: &Receiver<()>,
) {
    let offscreen = Offscreen::new(None);
    let still = std::env::temp_dir().join("haru-preview.png");
    let mut live: Option<Live> = None;
    let mut seq: u64 = 0;

    loop {
        // Nobody is looking: let the renderer go, and with it the wallpaper's
        // textures. Dropping the stream stops the process it started.
        if !watching.load(Ordering::Relaxed) {
            live = None;
            if woken.recv().is_err() {
                return;
            }
            continue;
        }

        // Whatever is pending now, not whatever was pending when the wake was
        // sent: several edits may have landed while the last frame rendered.
        let job = pending.lock().ok().and_then(|mut held| held.take());

        if let Some(job) = job {
            seq = job.seq;
            let started = std::time::Instant::now();

            // A stream already showing this wallpaper only needs the edits.
            let updated = match live.as_mut() {
                Some(open) if open.dir == job.dir => open.update(&job.properties).ok(),
                _ => None,
            };

            let result = match updated {
                Some(frame) => Ok(Rendered::Live(frame)),
                None => match Live::start(offscreen.binary(), &job.dir, &job.properties) {
                    Ok((open, frame)) => {
                        live = Some(open);
                        Ok(Rendered::Live(frame))
                    }
                    Err(_) => {
                        // No streaming renderer: one still per edit, which is
                        // the same picture arriving slower.
                        live = None;
                        offscreen
                            .render(&job.dir, &job.properties, &still)
                            .map(|()| Rendered::Still(still.clone()))
                    }
                },
            };

            let failed = result.is_err();
            if frames
                .send(Frame {
                    seq,
                    result,
                    took: started.elapsed(),
                })
                .is_err()
            {
                return;
            }
            if failed {
                // Nothing to pump; wait to be asked again rather than
                // hammering a renderer that just refused.
                if woken.recv().is_err() {
                    return;
                }
            }
            continue;
        }

        let Some(open) = live.as_mut() else {
            // Idle: nothing on screen to animate.
            if woken.recv().is_err() {
                return;
            }
            continue;
        };

        let started = std::time::Instant::now();
        match open.stream.frame() {
            Ok(frame) => {
                if frames
                    .send(Frame {
                        seq,
                        result: Ok(Rendered::Live(frame)),
                        took: started.elapsed(),
                    })
                    .is_err()
                {
                    return;
                }
            }
            // A renderer that went away is not fatal: the next request starts
            // another one.
            Err(_) => live = None,
        }
    }
}

/// A running preview stream, and which wallpaper it is showing.
struct Live {
    dir: std::path::PathBuf,
    stream: PreviewStream,
}

/// How large a preview is rendered.
///
/// Wide enough to look at, small enough that a frame is 2 MB rather than the
/// 3.7 MB of a 1280-wide one — which at thirty frames a second is the
/// difference between 60 and 110 MB/s through a socket.
const EDGE: u32 = 960;

/// How many frames a second to ask for.
const FPS: u32 = 30;

impl Live {
    /// Starts a renderer, applies the edits, and takes the first frame.
    fn start(
        binary: &std::path::Path,
        dir: &std::path::Path,
        properties: &[(String, String)],
    ) -> Result<(Self, haru_apply::Frame), String> {
        let mut stream = PreviewStream::start(binary, dir, EDGE, FPS)?;
        for (key, value) in properties {
            stream.set_property(key, value)?;
        }
        let frame = stream.frame()?;
        Ok((
            Self {
                dir: dir.to_owned(),
                stream,
            },
            frame,
        ))
    }

    /// Applies edits and returns the next frame.
    fn update(&mut self, properties: &[(String, String)]) -> Result<haru_apply::Frame, String> {
        for (key, value) in properties {
            self.stream.set_property(key, value)?;
        }
        self.stream.frame()
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

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use egui::{Align, Layout, RichText};
use haru_apply::{Offscreen, PreviewStream};
use haru_core::{Installed, properties};

use crate::theme;

struct Job {
    dir: PathBuf,
    properties: Vec<(String, String)>,
    seq: u64,
}

struct Frame {
    seq: u64,
    result: Result<Rendered, String>,
    took: std::time::Duration,
}

enum Rendered {
    Live(haru_apply::Frame),
    Still(PathBuf),
}

pub struct Preview {
    item: Option<Installed>,
    settings: Vec<properties::Property>,
    shown: Option<Rendered>,
    generation: u64,
    texture: Option<egui::TextureHandle>,
    status: String,
    seq: u64,
    waiting: bool,
    took: Option<std::time::Duration>,
    watching: Arc<AtomicBool>,
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

    pub fn suspend(&mut self) {
        self.watching.store(false, Ordering::Relaxed);
        let _ = self.wake.send(());
    }

    #[must_use]
    pub fn item(&self) -> Option<&Installed> {
        self.item.as_ref()
    }

    pub fn open(&mut self, item: Installed) {
        self.settings = properties::read(&item.dir);
        self.item = Some(item);
        self.texture = None;
        self.shown = None;
        self.status = String::new();
        self.request();
    }

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

        if let Ok(mut pending) = self.pending.lock() {
            *pending = Some(job);
        }
        self.waiting = true;
        let _ = self.wake.send(());
    }

    pub fn ui(&mut self, ctx: &egui::Context, sidebar: bool) {
        if !self.watching.swap(true, Ordering::Relaxed) {
            let _ = self.wake.send(());
        }
        self.collect(ctx);

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

    fn collect(&mut self, ctx: &egui::Context) {
        while let Ok(frame) = self.frames.try_recv() {
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

fn render(
    offscreen: &Offscreen,
    still: &std::path::Path,
    live: &mut Option<Live>,
    job: &Job,
) -> Result<Rendered, String> {
    let updated = match live.as_mut() {
        Some(open) if open.dir == job.dir => open.update(&job.properties).ok(),
        _ => None,
    };

    match updated {
        Some(frame) => Ok(Rendered::Live(frame)),
        None => match Live::start(offscreen.binary(), &job.dir, &job.properties) {
            Ok((open, frame)) => {
                *live = Some(open);
                Ok(Rendered::Live(frame))
            }
            Err(_) => {
                *live = None;
                offscreen
                    .render(&job.dir, &job.properties, still)
                    .map(|()| Rendered::Still(still.to_path_buf()))
            }
        },
    }
}

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
        if !watching.load(Ordering::Relaxed) {
            live = None;
            if woken.recv().is_err() {
                return;
            }
            continue;
        }

        let job = pending.lock().ok().and_then(|mut held| held.take());

        if let Some(job) = job {
            seq = job.seq;
            let started = std::time::Instant::now();

            let result = render(&offscreen, &still, &mut live, &job);
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
            if failed && woken.recv().is_err() {
                return;
            }
            continue;
        }

        let Some(open) = live.as_mut() else {
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
            Err(_) => live = None,
        }
    }
}

struct Live {
    dir: std::path::PathBuf,
    stream: PreviewStream,
}

const EDGE: u32 = 960;

const FPS: u32 = 30;

impl Live {
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

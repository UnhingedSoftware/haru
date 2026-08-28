use std::collections::HashMap;
use std::io::Read as _;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

const IN_FLIGHT: usize = 6;

const MAX_EDGE: u32 = 320;

const KEEP: usize = 240;

const IDLE_FRAMES: u64 = 120;

const MAX_BYTES: usize = 10 * 1024 * 1024;

enum State {
    Loading,
    Ready(egui::TextureHandle, u64),
    Failed,
}

struct Decoded {
    url: String,
    image: Option<egui::ColorImage>,
}

pub struct Previews {
    entries: HashMap<String, State>,
    order: Vec<String>,
    queue: Arc<Mutex<Vec<String>>>,
    active: Arc<Mutex<usize>>,
    inbound: Receiver<Decoded>,
    outbound: Sender<Decoded>,
    frame: u64,
}

impl Default for Previews {
    fn default() -> Self {
        Self::new()
    }
}

impl Previews {
    #[must_use]
    pub fn new() -> Self {
        let (outbound, inbound) = channel();
        Self {
            entries: HashMap::new(),
            order: Vec::new(),
            queue: Arc::new(Mutex::new(Vec::new())),
            active: Arc::new(Mutex::new(0)),
            inbound,
            outbound,
            frame: 0,
        }
    }

    pub fn texture(&mut self, ctx: &egui::Context, url: &str) -> Option<egui::TextureHandle> {
        self.drain(ctx);

        let now = self.frame;
        match self.entries.get_mut(url) {
            Some(State::Ready(texture, seen)) => {
                *seen = now;
                return Some(texture.clone());
            }
            Some(State::Loading | State::Failed) => return None,
            None => {}
        }

        self.remember(url.to_owned());
        if let Ok(mut queue) = self.queue.lock() {
            queue.push(url.to_owned());
        }
        self.pump(ctx);
        None
    }

    pub fn texture_path(
        &mut self,
        ctx: &egui::Context,
        path: &std::path::Path,
    ) -> Option<egui::TextureHandle> {
        let key = path.to_string_lossy().into_owned();
        self.drain(ctx);

        let now = self.frame;
        match self.entries.get_mut(&key) {
            Some(State::Ready(texture, seen)) => {
                *seen = now;
                return Some(texture.clone());
            }
            Some(State::Loading | State::Failed) => return None,
            None => {}
        }

        self.remember(key.clone());
        if let Ok(mut queue) = self.queue.lock() {
            queue.push(key);
        }
        self.pump(ctx);
        None
    }

    fn remember(&mut self, key: String) {
        self.entries.insert(key.clone(), State::Loading);
        self.order.push(key);

        while self.order.len() > KEEP {
            let Some(oldest) = self.order.first().cloned() else {
                break;
            };
            self.order.remove(0);
            if !matches!(self.entries.get(&oldest), Some(State::Loading)) {
                self.entries.remove(&oldest);
            }
        }
    }

    pub fn sweep(&mut self) {
        self.frame = self.frame.saturating_add(1);
        let now = self.frame;

        self.entries.retain(|_, state| match state {
            State::Loading => true,
            State::Ready(_, seen) => now.saturating_sub(*seen) <= IDLE_FRAMES,
            State::Failed => true,
        });
        self.order.retain(|key| self.entries.contains_key(key));
    }

    #[must_use]
    pub fn held(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn loading(&self) -> usize {
        self.entries
            .values()
            .filter(|state| matches!(state, State::Loading))
            .count()
    }

    fn drain(&mut self, ctx: &egui::Context) {
        let now = self.frame;
        while let Ok(Decoded { url, image }) = self.inbound.try_recv() {
            let state = image.map_or(State::Failed, |image| {
                State::Ready(
                    ctx.load_texture(&url, image, egui::TextureOptions::LINEAR),
                    now,
                )
            });
            self.entries.insert(url, state);
            if let Ok(mut active) = self.active.lock() {
                *active = active.saturating_sub(1);
            }
        }
        self.pump(ctx);
    }

    fn pump(&self, ctx: &egui::Context) {
        loop {
            let Ok(mut active) = self.active.lock() else {
                return;
            };
            if *active >= IN_FLIGHT {
                return;
            }
            let Ok(mut queue) = self.queue.lock() else {
                return;
            };
            let Some(url) = queue.pop() else { return };
            *active = active.saturating_add(1);
            drop(queue);
            drop(active);

            let outbound = self.outbound.clone();
            let ctx = ctx.clone();
            std::thread::Builder::new()
                .name("haru-preview".to_owned())
                .spawn(move || {
                    let image = fetch(&url);
                    let _ = outbound.send(Decoded { url, image });
                    ctx.request_repaint();
                })
                .ok();
        }
    }
}

fn fetch(source: &str) -> Option<egui::ColorImage> {
    if !source.starts_with("https://") {
        if source.contains("://") {
            return None;
        }
        let body = std::fs::read(source).ok()?;
        if body.len() > MAX_BYTES {
            return None;
        }
        return decode(&body);
    }
    let url = source;

    let response = ureq::get(url)
        .timeout(std::time::Duration::from_secs(20))
        .call()
        .ok()?;

    let mut body = Vec::new();
    let mut reader = response.into_reader();
    std::io::copy(&mut (&mut reader).take(MAX_BYTES as u64 + 1), &mut body).ok()?;
    if body.len() > MAX_BYTES {
        return None;
    }

    decode(&body)
}

fn decode(body: &[u8]) -> Option<egui::ColorImage> {
    let decoded = image::load_from_memory(body).ok()?;
    let scaled = decoded.thumbnail(MAX_EDGE, MAX_EDGE).to_rgba8();
    let size = [scaled.width() as usize, scaled.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(
        size,
        scaled.as_raw(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texture() -> egui::TextureHandle {
        egui::Context::default().load_texture(
            "test",
            egui::ColorImage::new([1, 1], egui::Color32::BLACK),
            egui::TextureOptions::LINEAR,
        )
    }

    #[test]
    fn a_plain_http_url_is_refused_without_a_request() {
        assert!(fetch("http://example.invalid/preview.jpg").is_none());
        assert!(fetch("ftp://example.invalid/preview.jpg").is_none());
    }

    #[test]
    fn a_missing_local_preview_is_a_miss_rather_than_a_panic() {
        assert!(fetch("/nonexistent/haru/preview.jpg").is_none());
    }

    #[test]
    fn an_empty_cache_is_loading_nothing() {
        assert_eq!(Previews::new().loading(), 0);
    }

    #[test]
    fn a_picture_nobody_draws_is_dropped() {
        let mut previews = Previews::new();
        previews
            .entries
            .insert("kept".to_owned(), State::Ready(texture(), 0));
        previews
            .entries
            .insert("dropped".to_owned(), State::Ready(texture(), 0));

        for _ in 0..=IDLE_FRAMES {
            if let Some(State::Ready(_, seen)) = previews.entries.get_mut("kept") {
                *seen = previews.frame;
            }
            previews.sweep();
        }

        assert!(previews.entries.contains_key("kept"));
        assert!(!previews.entries.contains_key("dropped"));
    }

    #[test]
    fn a_picture_still_arriving_is_never_swept() {
        let mut previews = Previews::new();
        previews.entries.insert("late".to_owned(), State::Loading);
        for _ in 0..(IDLE_FRAMES * 2) {
            previews.sweep();
        }
        assert!(previews.entries.contains_key("late"));
    }

    #[test]
    fn the_cache_stops_growing() {
        let mut previews = Previews::new();
        for index in 0..(KEEP * 3) {
            previews.remember(format!("https://example.invalid/{index}.jpg"));
            previews.entries.insert(
                format!("https://example.invalid/{index}.jpg"),
                State::Failed,
            );
        }
        assert!(previews.held() <= KEEP + 1, "{}", previews.held());
    }
}

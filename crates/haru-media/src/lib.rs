//! Preview art: fetched once, decoded once, kept.
//!
//! A page of results is twenty images from Steam's CDN, and a grid asks for
//! every one of them on the same frame. Three rules keep that from being a
//! stampede:
//!
//! * **A bounded number in flight.** Twenty parallel TLS handshakes to one host
//!   is slower than six, and the rest of the page is not visible yet anyway.
//! * **Scaled at decode.** A Workshop preview can be 4K; a tile is 150 px. A
//!   full-size texture per tile is tens of megabytes of VRAM for pixels nobody
//!   sees.
//! * **Asked for once.** The grid re-requests every frame by design — it does
//!   not know what it already has — so the cache, not the caller, remembers.

use std::collections::HashMap;
use std::io::Read as _;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

/// How many downloads run at once.
///
/// Six is the browser convention for one host and about where a page of
/// previews stops getting faster.
const IN_FLIGHT: usize = 6;

/// The longest edge a decoded preview keeps.
///
/// Twice the tile so a preview stays sharp on a HiDPI screen, and no more.
const MAX_EDGE: u32 = 320;

/// Refuse anything larger, before decoding it.
///
/// Preview art is a few hundred kilobytes. Ten megabytes is not a preview, and
/// decoding whatever arrives is how a picker gets an image bomb.
const MAX_BYTES: usize = 10 * 1024 * 1024;

/// What a tile knows about its picture.
enum State {
    /// Being fetched.
    Loading,
    /// Ready to draw.
    Ready(egui::TextureHandle),
    /// Not coming: a dead link, a refusal, an undecodable body.
    Failed,
}

/// Decoded pixels on their way back from a worker.
struct Decoded {
    url: String,
    image: Option<egui::ColorImage>,
}

/// Every preview this session has looked at.
pub struct Previews {
    entries: HashMap<String, State>,
    queue: Arc<Mutex<Vec<String>>>,
    active: Arc<Mutex<usize>>,
    inbound: Receiver<Decoded>,
    outbound: Sender<Decoded>,
}

impl Default for Previews {
    fn default() -> Self {
        Self::new()
    }
}

impl Previews {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        let (outbound, inbound) = channel();
        Self {
            entries: HashMap::new(),
            queue: Arc::new(Mutex::new(Vec::new())),
            active: Arc::new(Mutex::new(0)),
            inbound,
            outbound,
        }
    }

    /// The texture for `url`, starting a fetch if this is the first ask.
    ///
    /// Returns `None` while loading and after a failure alike: a tile draws a
    /// placeholder either way, and telling them apart would only invite a
    /// retry loop against a link that is still dead.
    pub fn texture(&mut self, ctx: &egui::Context, url: &str) -> Option<egui::TextureHandle> {
        self.drain(ctx);

        match self.entries.get(url) {
            Some(State::Ready(texture)) => return Some(texture.clone()),
            Some(State::Loading | State::Failed) => return None,
            None => {}
        }

        self.entries.insert(url.to_owned(), State::Loading);
        if let Ok(mut queue) = self.queue.lock() {
            queue.push(url.to_owned());
        }
        self.pump(ctx);
        None
    }

    /// The texture for a file already on this machine.
    ///
    /// Installed wallpapers keep their preview beside `project.json`, so the
    /// library grid has nothing to download — but it still wants the same
    /// decode-once-and-scale that remote art gets, and the same cache, so a
    /// path goes through here rather than being loaded per frame.
    pub fn texture_path(
        &mut self,
        ctx: &egui::Context,
        path: &std::path::Path,
    ) -> Option<egui::TextureHandle> {
        let key = path.to_string_lossy().into_owned();
        self.drain(ctx);

        match self.entries.get(&key) {
            Some(State::Ready(texture)) => return Some(texture.clone()),
            Some(State::Loading | State::Failed) => return None,
            None => {}
        }

        self.entries.insert(key.clone(), State::Loading);
        if let Ok(mut queue) = self.queue.lock() {
            queue.push(key);
        }
        self.pump(ctx);
        None
    }

    /// How many previews are still on their way.
    #[must_use]
    pub fn loading(&self) -> usize {
        self.entries
            .values()
            .filter(|state| matches!(state, State::Loading))
            .count()
    }

    /// Takes whatever workers have finished and turns it into textures.
    ///
    /// Uploading happens here rather than on the worker because a texture
    /// belongs to the render context, which is not this thread's to touch.
    fn drain(&mut self, ctx: &egui::Context) {
        while let Ok(Decoded { url, image }) = self.inbound.try_recv() {
            let state = image.map_or(State::Failed, |image| {
                State::Ready(ctx.load_texture(&url, image, egui::TextureOptions::LINEAR))
            });
            self.entries.insert(url, state);
            if let Ok(mut active) = self.active.lock() {
                *active = active.saturating_sub(1);
            }
        }
        self.pump(ctx);
    }

    /// Starts as many queued fetches as the limit allows.
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
                    // The frame that asked is long gone; without this the
                    // picture only appears when something else causes a
                    // repaint, which on an idle window is never.
                    ctx.request_repaint();
                })
                .ok();
        }
    }
}

/// Reads one preview, from wherever it is, scaled.
///
/// A local path and an `https` URL are the same job past the first few lines,
/// and keeping them in one function is what makes both use the same size cap
/// and the same scaling.
fn fetch(source: &str) -> Option<egui::ColorImage> {
    if !source.starts_with("https://") {
        // Anything that is not an https URL is a path on this machine. A
        // plaintext URL is neither, and is refused rather than fetched: these
        // strings arrive in a network reply.
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

/// Turns image bytes into something egui can upload, scaled down first.
///
/// Steam serves previews as JPEG, PNG and animated GIF alike, with the
/// extension often absent from the URL, so the format comes from the bytes.
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

    #[test]
    fn a_plain_http_url_is_refused_without_a_request() {
        // The URLs arrive in a network reply; one of them pointing somewhere
        // unencrypted should cost nothing at all.
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
}

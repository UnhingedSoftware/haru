//! A live preview: frames from a renderer, edits back to it.
//!
//! The renderer serves `kirie preview` — one process, one socket, frames
//! streaming out and `property` lines going in. That is what makes editing
//! feel live: the wallpaper animates, and changing a slider costs a rebuild
//! rather than an engine start.
//!
//! An older renderer has no `preview` subcommand. Starting one fails cleanly
//! here so the caller can fall back to [`crate::Offscreen`], which does the
//! same job a frame at a time.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

/// The header every frame carries, little-endian.
const HEADER_BYTES: usize = 24;

/// What a frame header must start with.
const MAGIC: [u8; 4] = *b"KPV1";

/// The only pixel format the stream speaks.
const FORMAT_RGBA8: u32 = 0;

/// How long to wait for the renderer to come up and answer.
///
/// A cold scene builds in seconds; past this the renderer is not starting.
const STARTUP: Duration = Duration::from_secs(30);

/// One rendered frame.
pub struct Frame {
    /// Pixels across.
    pub width: u32,
    /// Pixels down.
    pub height: u32,
    /// RGBA8, `width * height * 4` bytes.
    pub pixels: Vec<u8>,
}

/// A renderer rendering one wallpaper, for as long as this is held.
pub struct Preview {
    child: Child,
    socket: PathBuf,
    stream: UnixStream,
}

impl Preview {
    /// Starts a renderer on its own socket and connects to it.
    ///
    /// # Errors
    /// When the renderer is missing, too old to know `preview`, or does not
    /// answer before the deadline.
    pub fn start(binary: &Path, background: &Path, edge: u32, fps: u32) -> Result<Self, String> {
        let socket = socket_path();
        // A path left behind by a previous run would be bound by nothing, and
        // connecting to it fails in a way that reads as the renderer refusing.
        let _ = std::fs::remove_file(&socket);

        let mut child = Command::new(binary)
            .arg("preview")
            .arg("--socket")
            .arg(&socket)
            .arg("--bg")
            .arg(background)
            .arg("--fps")
            .arg(fps.to_string())
            .arg("--size")
            .arg(edge.to_string())
            // The renderer picks a backend from these; a preview must not open
            // anything on the desktop.
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|error| format!("could not start the renderer ({error})"))?;

        let deadline = Instant::now() + STARTUP;
        loop {
            if let Ok(stream) = UnixStream::connect(&socket) {
                return Ok(Self {
                    child,
                    socket,
                    stream,
                });
            }
            // A renderer that exited is one that does not know this
            // subcommand, which is the ordinary case against an older build.
            if matches!(child.try_wait(), Ok(Some(_))) {
                let _ = std::fs::remove_file(&socket);
                return Err("this renderer has no preview mode".to_owned());
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = std::fs::remove_file(&socket);
                return Err("the renderer did not start in time".to_owned());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Sets one property. The next frames show it.
    ///
    /// # Errors
    /// When the renderer has gone away.
    pub fn set_property(&mut self, key: &str, value: &str) -> Result<(), String> {
        // The value is the rest of the line, which is what lets a colour
        // travel as `0.5 0.25 1` with no quoting.
        self.send(&format!("property {key} {value}"))
    }

    /// Switches to another wallpaper, dropping the previous overrides.
    ///
    /// # Errors
    /// When the renderer has gone away.
    pub fn set_background(&mut self, dir: &Path) -> Result<(), String> {
        self.send(&format!("bg {}", dir.display()))
    }

    /// Reads the next frame. Blocks until one arrives.
    ///
    /// # Errors
    /// When the stream ends, or a header does not describe the frame behind
    /// it — which means the stream is out of step, and painting on would show
    /// one frame's pixels at another's size.
    pub fn frame(&mut self) -> Result<Frame, String> {
        let mut header = [0_u8; HEADER_BYTES];
        self.stream
            .read_exact(&mut header)
            .map_err(|error| format!("the preview stream ended ({error})"))?;

        let field = |at: usize| -> u32 {
            let mut four = [0_u8; 4];
            four.copy_from_slice(header.get(at..at + 4).unwrap_or(&[0; 4]));
            u32::from_le_bytes(four)
        };
        if header.get(0..4) != Some(&MAGIC[..]) {
            return Err("not a preview frame".to_owned());
        }
        if field(16) != FORMAT_RGBA8 {
            return Err(format!("unknown pixel format {}", field(16)));
        }

        let (width, height, bytes) = (field(8), field(12), field(20) as usize);
        if u64::from(width) * u64::from(height) * 4 != bytes as u64 {
            return Err("a frame's size and length disagree".to_owned());
        }

        // One allocation per frame, handed to the caller. Keeping a scratch
        // buffer here would not save it: the pixels leave this thread, so the
        // buffer would have to be replaced anyway.
        let mut pixels = vec![0_u8; bytes];
        self.stream
            .read_exact(&mut pixels)
            .map_err(|error| format!("the preview stream ended mid-frame ({error})"))?;

        Ok(Frame {
            width,
            height,
            pixels,
        })
    }

    /// Sends one command line.
    fn send(&mut self, line: &str) -> Result<(), String> {
        writeln!(self.stream, "{line}")
            .map_err(|error| format!("the renderer stopped listening ({error})"))
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        // Asked first, killed if it does not go: a renderer left running would
        // hold a GPU device for a window that has closed.
        let _ = self.send("quit");
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// A socket path unique to this process.
///
/// Two haru windows preview different wallpapers, and one socket between them
/// would mean one of the two watching the other's.
fn socket_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join(format!("haru-preview-{}.sock", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_renderer_that_is_not_there_fails_rather_than_waits() {
        let error = Preview::start(Path::new("/nonexistent/kirie"), Path::new("/tmp"), 480, 30);
        assert!(error.is_err());
    }

    #[test]
    fn the_socket_is_this_process_only() {
        // Two windows previewing different wallpapers must not share one.
        let path = socket_path();
        let name = path.to_string_lossy().into_owned();
        assert!(name.contains(&std::process::id().to_string()), "{name}");
    }
}

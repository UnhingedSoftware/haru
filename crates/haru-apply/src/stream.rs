use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::{Duration, Instant};

use crate::socket::UnixStream;

const HEADER_BYTES: usize = 24;

const MAGIC: [u8; 4] = *b"KPV1";

const FORMAT_RGBA8: u32 = 0;

const STARTUP: Duration = Duration::from_secs(30);

/// The largest frame kirie sends: `--size` is clamped to 3840 on the longest
/// edge, so 3840x3840 RGBA. A header asking for more is not believed.
const MAX_FRAME_BYTES: u64 = 3840 * 3840 * 4;

pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub struct Preview {
    child: Child,
    socket: PathBuf,
    stream: UnixStream,
}

impl Preview {
    pub fn start(binary: &Path, background: &Path, edge: u32, fps: u32) -> Result<Self, String> {
        let socket = socket_path();
        let _ = std::fs::remove_file(&socket);

        let mut child = crate::child::quiet(binary)
            .arg("preview")
            .arg("--socket")
            .arg(&socket)
            .arg("--bg")
            .arg(background)
            .arg("--fps")
            .arg(fps.to_string())
            .arg("--size")
            .arg(edge.to_string())
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .envs(crate::renderer_env())
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

    pub fn set_property(&mut self, key: &str, value: &str) -> Result<(), String> {
        self.send(&format!("property {key} {value}"))
    }

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

        let (width, height, bytes) = (field(8), field(12), field(20));
        let expected = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|pixels| pixels.checked_mul(4));
        if expected != Some(u64::from(bytes)) {
            return Err("a frame's size and length disagree".to_owned());
        }
        if u64::from(bytes) > MAX_FRAME_BYTES {
            return Err(format!(
                "a {width}x{height} frame is larger than any preview"
            ));
        }

        let mut pixels = vec![0_u8; bytes as usize];
        self.stream
            .read_exact(&mut pixels)
            .map_err(|error| format!("the preview stream ended mid-frame ({error})"))?;

        Ok(Frame {
            width,
            height,
            pixels,
        })
    }

    /// One command per line, so a line break inside a property value would
    /// start a second command; it is folded to a space, as `Kirie::ask` does.
    fn send(&mut self, line: &str) -> Result<(), String> {
        let line = line.replace(['\n', '\r'], " ");
        writeln!(self.stream, "{line}")
            .map_err(|error| format!("the renderer stopped listening ({error})"))
    }
}

impl Drop for Preview {
    fn drop(&mut self) {
        let _ = self.send("quit");
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_file(&self.socket);
    }
}

fn socket_path() -> PathBuf {
    haru_core::runtime_dir().join(format!("haru-preview-{}.sock", std::process::id()))
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
        let path = socket_path();
        let name = path.to_string_lossy().into_owned();
        assert!(name.contains(&std::process::id().to_string()), "{name}");
    }
}

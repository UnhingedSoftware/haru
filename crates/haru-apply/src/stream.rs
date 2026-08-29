use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

const HEADER_BYTES: usize = 24;

const MAGIC: [u8; 4] = *b"KPV1";

const FORMAT_RGBA8: u32 = 0;

const STARTUP: Duration = Duration::from_secs(30);

pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

fn kirie_env() -> Vec<(String, std::ffi::OsString)> {
    let mut out = Vec::new();
    if let Some(assets) = haru_core::engine::found() {
        out.push(("KIRIE_WE_ASSETS".to_owned(), assets.into_os_string()));
    }
    let roots = haru_core::Config::load().libraries();
    if let Ok(joined) = std::env::join_paths(roots) {
        out.push(("KIRIE_STEAM_LIBRARY".to_owned(), joined));
    }
    out
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
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("DISPLAY")
            .envs(kirie_env())
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

    pub fn set_background(&mut self, dir: &Path) -> Result<(), String> {
        self.send(&format!("bg {}", dir.display()))
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

        let (width, height, bytes) = (field(8), field(12), field(20) as usize);
        if u64::from(width) * u64::from(height) * 4 != bytes as u64 {
            return Err("a frame's size and length disagree".to_owned());
        }

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

    fn send(&mut self, line: &str) -> Result<(), String> {
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
        let path = socket_path();
        let name = path.to_string_lossy().into_owned();
        assert!(name.contains(&std::process::id().to_string()), "{name}");
    }
}

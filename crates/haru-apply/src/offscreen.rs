//! Rendering a wallpaper without putting it on a screen.
//!
//! What a preview needs, and what the studio will need more of: a real frame of
//! a real wallpaper, with properties set to whatever is being tried, that has
//! no effect on the wallpaper actually up.
//!
//! kirie renders a screenshot **on a throwaway headless device and exits**, so
//! this neither opens a window nor touches the running renderer. Measured on
//! this machine: 0.74 s for a scene with a warm bundle cache.
//!
//! A process per frame is not where this ends — a render loop streaming frames
//! over a socket is the shape that makes editing feel live — but it is honest,
//! it works today, and the difference is invisible to everything above here.

use std::path::{Path, PathBuf};
use std::process::Command;

/// How long a single frame may take before it is given up on.
///
/// A cold scene bundle takes seconds; a wallpaper that has not finished
/// building after a minute is not going to.
const DEADLINE: std::time::Duration = std::time::Duration::from_secs(60);

/// A renderer that can produce a frame off-screen.
pub struct Offscreen {
    binary: PathBuf,
}

impl Offscreen {
    /// Finds the renderer, or is told where it is.
    #[must_use]
    pub fn new(binary: Option<PathBuf>) -> Self {
        Self {
            binary: binary.unwrap_or_else(find_kirie),
        }
    }

    /// Whether there is a renderer to call.
    #[must_use]
    pub fn available(&self) -> bool {
        self.binary.is_file()
    }

    /// Where the renderer is.
    #[must_use]
    pub fn binary(&self) -> &Path {
        &self.binary
    }

    /// Renders one frame of `dir` with `properties` applied, into `out`.
    ///
    /// `properties` are `key=value` pairs; a colour is the space-separated
    /// triple the renderer parses, which is why they arrive already formatted
    /// rather than as typed values.
    ///
    /// # Errors
    /// When the renderer is missing, refuses the wallpaper, or writes nothing.
    /// A web wallpaper is the common refusal: capturing one needs a build with
    /// an off-screen web backend, and saying so is better than a blank frame.
    pub fn render(
        &self,
        dir: &Path,
        properties: &[(String, String)],
        out: &Path,
    ) -> Result<(), String> {
        if !self.available() {
            return Err("no renderer found to preview with".to_owned());
        }
        // A leftover frame from a previous render would otherwise be read as
        // this one's, which is how a failed edit looks like it worked.
        let _ = std::fs::remove_file(out);

        let mut command = Command::new(&self.binary);
        command.arg("--bg").arg(dir);
        for (key, value) in properties {
            command.arg("--set-property").arg(format!("{key}={value}"));
        }
        command.arg("--screenshot").arg(out);

        // The renderer picks a backend from these when they are set, and a
        // preview must not open anything on the desktop.
        command.env_remove("WAYLAND_DISPLAY");
        command.env_remove("DISPLAY");

        let child = command
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| format!("could not start the renderer ({error})"))?;

        let finished = wait_for(child, DEADLINE)?;
        if out.is_file() {
            return Ok(());
        }

        // The renderer's own last line says why, and it is a better message
        // than anything this could invent.
        let reason = String::from_utf8_lossy(&finished.stderr)
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("the renderer wrote no frame")
            .to_owned();
        Err(strip_log_prefix(&reason))
    }
}

/// Waits for a child, killing it if it outstays the deadline.
fn wait_for(
    mut child: std::process::Child,
    deadline: std::time::Duration,
) -> Result<std::process::Output, String> {
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .map_err(|error| format!("the renderer could not be read ({error})"));
            }
            Ok(None) if started.elapsed() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
            Ok(None) => {
                let _ = child.kill();
                return Err("the renderer took too long".to_owned());
            }
            Err(error) => return Err(format!("the renderer could not be waited on ({error})")),
        }
    }
}

/// Drops the timestamp and target a tracing line carries.
///
/// Renderer output is `2026-08-28T00:58:53Z INFO kirie::compat: message`, and
/// only the last part means anything in a panel.
fn strip_log_prefix(line: &str) -> String {
    let cleaned: String = line
        .chars()
        .filter(|character| *character != '\u{1b}')
        .collect();
    cleaned
        .rsplit_once(": ")
        .map_or(cleaned.clone(), |(_, message)| message.trim().to_owned())
}

/// Where a renderer might be, in the order worth trying.
fn find_kirie() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        let local = PathBuf::from(&home).join(".local/bin/kirie");
        if local.is_file() {
            return local;
        }
    }
    for path in ["/usr/local/bin/kirie", "/usr/bin/kirie"] {
        let candidate = PathBuf::from(path);
        if candidate.is_file() {
            return candidate;
        }
    }
    // Named rather than absent, so the error says what it looked for.
    PathBuf::from("kirie")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_renderer_is_reported_rather_than_run() {
        let offscreen = Offscreen::new(Some(PathBuf::from("/nonexistent/kirie")));
        assert!(!offscreen.available());
        let error = offscreen.render(Path::new("/tmp"), &[], Path::new("/tmp/haru-none.png"));
        assert!(
            matches!(error, Err(ref why) if why.contains("no renderer")),
            "{error:?}"
        );
    }

    #[test]
    fn a_log_line_is_reduced_to_its_message() {
        // The renderer's refusals are the useful part; its timestamps are not.
        let line = "2026-08-28T00:58:53.4Z ERROR kirie::compat::run: cannot screenshot a web wallpaper";
        assert_eq!(strip_log_prefix(line), "cannot screenshot a web wallpaper");
        assert_eq!(strip_log_prefix("plain words"), "plain words");
    }
}

use std::path::{Path, PathBuf};

const DEADLINE: std::time::Duration = std::time::Duration::from_secs(60);

pub struct Offscreen {
    binary: PathBuf,
}

impl Offscreen {
    #[must_use]
    pub fn new(binary: Option<PathBuf>) -> Self {
        Self {
            binary: binary.unwrap_or_else(find_kirie),
        }
    }

    #[must_use]
    pub fn available(&self) -> bool {
        self.binary.is_file()
    }

    #[must_use]
    pub fn binary(&self) -> &Path {
        &self.binary
    }

    pub fn render(
        &self,
        dir: &Path,
        properties: &[(String, String)],
        out: &Path,
    ) -> Result<(), String> {
        if !self.available() {
            return Err("no renderer found to preview with".to_owned());
        }
        let _ = std::fs::remove_file(out);

        let mut command = crate::child::quiet(&self.binary);
        command.arg("--bg").arg(dir);
        for (key, value) in properties {
            command.arg("--set-property").arg(format!("{key}={value}"));
        }
        command.arg("--screenshot").arg(out);

        command.env_remove("WAYLAND_DISPLAY");
        command.env_remove("DISPLAY");
        for (key, value) in crate::renderer_env() {
            command.env(key, value);
        }

        let mut child = command
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| format!("could not start the renderer ({error})"))?;

        // Read on its own thread while the renderer runs: a pipe nobody drains
        // fills up, and the renderer then blocks on its next log line until
        // the deadline kills it.
        let said = child.stderr.take().map(|mut stderr| {
            std::thread::spawn(move || {
                let mut text = Vec::new();
                let _ = std::io::Read::read_to_end(&mut stderr, &mut text);
                text
            })
        });
        wait_for(child, DEADLINE)?;
        if out.is_file() {
            return Ok(());
        }
        let said = said
            .and_then(|reading| reading.join().ok())
            .unwrap_or_default();

        let reason = String::from_utf8_lossy(&said)
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("the renderer wrote no frame")
            .to_owned();
        Err(strip_log_prefix(&reason))
    }
}

fn wait_for(mut child: std::process::Child, deadline: std::time::Duration) -> Result<(), String> {
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return Ok(()),
            Ok(None) if started.elapsed() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(30));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("the renderer took too long".to_owned());
            }
            Err(error) => return Err(format!("the renderer could not be waited on ({error})")),
        }
    }
}

fn strip_log_prefix(line: &str) -> String {
    let cleaned: String = line
        .chars()
        .filter(|character| *character != '\u{1b}')
        .collect();
    cleaned
        .rsplit_once(": ")
        .map_or(cleaned.clone(), |(_, message)| message.trim().to_owned())
}

/// The renderer to take a screenshot with, when the caller named none.
///
/// `install::installed` knows every place the renderer can be on this
/// platform, `KIRIE_BINARY` and `PATH` included; the bare name is the last
/// resort, and leaves `available()` false.
fn find_kirie() -> PathBuf {
    crate::install::installed().unwrap_or_else(|| PathBuf::from(crate::install::RENDERER))
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
        let line =
            "2026-08-28T00:58:53.4Z ERROR kirie::compat::run: cannot screenshot a web wallpaper";
        assert_eq!(strip_log_prefix(line), "cannot screenshot a web wallpaper");
        assert_eq!(strip_log_prefix("plain words"), "plain words");
    }
}

use std::path::{Path, PathBuf};
use std::process::Command;

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

        let mut command = Command::new(&self.binary);
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

        let child = command
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| format!("could not start the renderer ({error})"))?;

        let finished = wait_for(child, DEADLINE)?;
        if out.is_file() {
            return Ok(());
        }

        let reason = String::from_utf8_lossy(&finished.stderr)
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("the renderer wrote no frame")
            .to_owned();
        Err(strip_log_prefix(&reason))
    }
}

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

fn strip_log_prefix(line: &str) -> String {
    let cleaned: String = line
        .chars()
        .filter(|character| *character != '\u{1b}')
        .collect();
    cleaned
        .rsplit_once(": ")
        .map_or(cleaned.clone(), |(_, message)| message.trim().to_owned())
}

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
        let line =
            "2026-08-28T00:58:53.4Z ERROR kirie::compat::run: cannot screenshot a web wallpaper";
        assert_eq!(strip_log_prefix(line), "cannot screenshot a web wallpaper");
        assert_eq!(strip_log_prefix("plain words"), "plain words");
    }
}

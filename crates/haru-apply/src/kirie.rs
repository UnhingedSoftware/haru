use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::{Backend, Screen};

const DEADLINE: Duration = Duration::from_secs(20);

pub struct Kirie {
    socket: PathBuf,
}

impl Kirie {
    #[must_use]
    pub fn new(socket: Option<PathBuf>) -> Self {
        Self {
            socket: socket.unwrap_or_else(default_socket),
        }
    }

    #[must_use]
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    fn ask(&self, line: &str) -> Result<Vec<String>, String> {
        let stream = UnixStream::connect(&self.socket)
            .map_err(|error| format!("the renderer is not reachable ({error})"))?;
        stream.set_read_timeout(Some(DEADLINE)).ok();
        stream.set_write_timeout(Some(DEADLINE)).ok();

        let mut writing = &stream;
        writeln!(writing, "{line}").map_err(|error| format!("write failed ({error})"))?;
        writing.flush().ok();

        let mut lines = Vec::new();
        for read in BufReader::new(&stream).lines() {
            lines.push(read.map_err(|error| format!("read failed ({error})"))?);
        }
        Ok(lines)
    }
}

impl Backend for Kirie {
    fn name(&self) -> &'static str {
        "kirie"
    }

    fn available(&self) -> bool {
        self.ask("ping")
            .is_ok_and(|reply| reply.first().is_some_and(|line| line.trim() == "pong"))
    }

    fn screens(&self) -> Result<Vec<Screen>, String> {
        let reply = self.ask("status")?;

        Ok(reply
            .iter()
            .filter_map(|line| {
                let rest = line.strip_prefix("screen=")?;
                let (name, background) = rest.split_once(" bg=")?;
                Some(Screen {
                    name: name.to_owned(),
                    current: (!background.is_empty()).then(|| PathBuf::from(background)),
                })
            })
            .collect())
    }

    fn set_property(&self, screen: &str, key: &str, value: &str) -> Result<(), String> {
        let reply = self.ask(&format!("property {screen} {key} {value}"))?;
        match reply.first().map(|line| line.trim()) {
            Some("ok") => Ok(()),
            Some("error") => Err(format!("the renderer would not take {key}")),
            Some(other) => Err(format!("unexpected answer: {other}")),
            None => Err("the renderer stopped answering".to_owned()),
        }
    }

    fn tune(&self, commands: &[String]) -> Result<(), String> {
        for command in commands {
            let reply = self.ask(command)?;
            match reply.first().map(|line| line.trim()) {
                Some("ok" | "unknown command") | None => {}
                Some("error") => return Err(format!("the renderer would not take `{command}`")),
                Some(other) => return Err(format!("unexpected answer: {other}")),
            }
        }
        Ok(())
    }

    fn stage(&self, key: &str, value: &str) -> Result<(), String> {
        let reply = self.ask(&format!("stage {key} {value}"))?;
        match reply.first().map(|line| line.trim()) {
            Some("ok") => Ok(()),
            Some("error") => Err(format!("the renderer would not take {key}")),
            Some("unknown command") => Err(format!("this renderer cannot pre-set {key}")),
            Some(other) => Err(format!("unexpected answer: {other}")),
            None => Err("the renderer stopped answering".to_owned()),
        }
    }

    fn apply(&self, screen: &str, dir: &Path) -> Result<(), String> {
        let reply = self.ask(&put_up_command(
            screen,
            dir,
            self.screens().map(|s| s.len()).unwrap_or(1),
        ))?;
        match reply.first().map(|line| line.trim()) {
            Some("ok") => Ok(()),
            Some(said) if said.starts_with("error") => Err(why(said)),
            Some(other) => Err(format!("unexpected answer: {other}")),
            None => Err("the renderer stopped answering".to_owned()),
        }
    }
}

#[must_use]
pub fn put_up_command(screen: &str, dir: &Path, screens: usize) -> String {
    if screen.is_empty() || screens <= 1 {
        return format!("bg {}", dir.display());
    }
    format!("bg {screen} {}", dir.display())
}

fn why(said: &str) -> String {
    let reason = said.strip_prefix("error").unwrap_or(said).trim();
    if reason.is_empty() {
        "the renderer would not load it".to_owned()
    } else {
        reason.to_owned()
    }
}

pub(crate) fn default_socket() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("lwe.sock")
}

#[cfg(test)]
mod tests {

    #[test]
    fn one_screen_needs_no_name() {
        let command = put_up_command("Built-in Retina Display", Path::new("/wall/one"), 1);
        assert_eq!(command, "bg /wall/one");
    }

    #[test]
    fn several_screens_still_name_the_one_to_change() {
        let command = put_up_command("DP-1", Path::new("/wall/one"), 2);
        assert_eq!(command, "bg DP-1 /wall/one");
    }

    #[test]
    fn no_screen_named_means_every_screen() {
        assert_eq!(
            put_up_command("", Path::new("/wall/one"), 3),
            "bg /wall/one"
        );
    }
    #[test]
    fn a_reason_is_shown_when_the_renderer_gives_one() {
        assert_eq!(
            super::why("error Wallpaper Engine's base assets are missing"),
            "Wallpaper Engine's base assets are missing"
        );
    }

    #[test]
    fn a_bare_refusal_still_reads_as_one() {
        assert_eq!(super::why("error"), "the renderer would not load it");
    }

    use super::*;

    #[test]
    fn an_absent_socket_is_unavailable_rather_than_a_panic() {
        let backend = Kirie::new(Some(PathBuf::from("/nonexistent/haru-test.sock")));
        assert!(!backend.available());
        assert!(backend.screens().is_err());
        assert!(backend.apply("DP-1", Path::new("/tmp")).is_err());
    }

    #[test]
    fn the_default_socket_follows_the_runtime_directory() {
        let socket = default_socket();
        assert!(socket.ends_with("lwe.sock"), "{}", socket.display());
    }

    #[test]
    fn a_status_line_keeps_a_path_with_spaces() {
        let line = "screen=DP-1 bg=/home/a/My Wallpapers/123";
        let rest = line.strip_prefix("screen=").unwrap_or_default();
        let (name, background) = rest.split_once(" bg=").unwrap_or_default();
        assert_eq!(name, "DP-1");
        assert_eq!(background, "/home/a/My Wallpapers/123");
    }
}

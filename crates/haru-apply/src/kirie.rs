//! The kirie backend: a Unix socket, one line per request.
//!
//! kirie serves a control socket that Wallpaper Engine's own Linux port
//! established: connect, write one line, read until the far end closes. There
//! is no session and no handshake, so every request here is a fresh
//! connection, which is also what makes a dead renderer obvious rather than
//! silent.
//!
//! Two answers matter and both are one word: `ok` and `error`. A refusal is
//! always a no-op — a failed `bg` leaves the previous wallpaper up — so an
//! error here never means "half applied".

use std::io::{BufRead as _, BufReader, Write as _};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::{Backend, Screen};

/// How long to wait on a reply.
///
/// A `bg` is answered once the scene is *built*, not when it is queued, and
/// building a heavy scene takes seconds. Anything under about ten and a slow
/// wallpaper reads as a failure while it is still working.
const DEADLINE: Duration = Duration::from_secs(20);

/// A renderer reachable over its control socket.
pub struct Kirie {
    socket: PathBuf,
}

impl Kirie {
    /// Points at a socket, or at the one kirie uses by default.
    #[must_use]
    pub fn new(socket: Option<PathBuf>) -> Self {
        Self {
            socket: socket.unwrap_or_else(default_socket),
        }
    }

    /// Where this backend is listening.
    #[must_use]
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    /// Sends one line and returns every line of the reply.
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
        // The socket answers `ping` from its own layer, so this proves the
        // socket is alive without proving the renderer behind it is — which is
        // the distinction `screens` then makes.
        self.ask("ping")
            .is_ok_and(|reply| reply.first().is_some_and(|line| line.trim() == "pong"))
    }

    fn screens(&self) -> Result<Vec<Screen>, String> {
        let reply = self.ask("status")?;

        // `screen=<name> bg=<path>` per line. Split on the literal " bg=" so a
        // path containing spaces survives, which several do.
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
        // The value is the rest of the line, which is what lets a colour
        // travel as `0.5 0.25 1` without any quoting.
        let reply = self.ask(&format!("property {screen} {key} {value}"))?;
        match reply.first().map(|line| line.trim()) {
            Some("ok") => Ok(()),
            Some("error") => Err(format!("the renderer would not take {key}")),
            Some(other) => Err(format!("unexpected answer: {other}")),
            None => Err("the renderer stopped answering".to_owned()),
        }
    }

    fn stage(&self, key: &str, value: &str) -> Result<(), String> {
        // Recorded against no screen and folded into the next `bg` build, so
        // the value arrives with the wallpaper rather than after it.
        let reply = self.ask(&format!("stage {key} {value}"))?;
        match reply.first().map(|line| line.trim()) {
            Some("ok") => Ok(()),
            Some("error") => Err(format!("the renderer would not take {key}")),
            // An older renderer has no `stage`; the wallpaper still goes up,
            // without the change, which is better than refusing to apply it.
            Some("unknown command") => Err(format!("this renderer cannot pre-set {key}")),
            Some(other) => Err(format!("unexpected answer: {other}")),
            None => Err("the renderer stopped answering".to_owned()),
        }
    }

    fn apply(&self, screen: &str, dir: &Path) -> Result<(), String> {
        // The path is the rest of the line, so it needs no quoting and must
        // not be given any: kirie takes everything after the screen name
        // verbatim.
        let reply = self.ask(&format!("bg {screen} {}", dir.display()))?;
        match reply.first().map(|line| line.trim()) {
            Some("ok") => Ok(()),
            Some("error") => Err("the renderer would not load it".to_owned()),
            Some(other) => Err(format!("unexpected answer: {other}")),
            // Zero bytes back means the socket is there and the renderer
            // behind it is gone, which is a different problem from a refusal.
            None => Err("the renderer stopped answering".to_owned()),
        }
    }
}

/// Where kirie puts its socket when nobody says otherwise.
fn default_socket() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("lwe.sock")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_absent_socket_is_unavailable_rather_than_a_panic() {
        // The ordinary state on a machine with no renderer running, and the
        // picker still has to open.
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
        // Several wallpapers have spaces in their directory names, and
        // splitting on whitespace loses everything after the first.
        let line = "screen=DP-1 bg=/home/a/My Wallpapers/123";
        let rest = line.strip_prefix("screen=").unwrap_or_default();
        let (name, background) = rest.split_once(" bg=").unwrap_or_default();
        assert_eq!(name, "DP-1");
        assert_eq!(background, "/home/a/My Wallpapers/123");
    }
}

//! Starting a renderer that is not running yet.
//!
//! kirie is not a service: it is a process that owns a screen and draws on it,
//! and it takes the first wallpaper on its command line because there is
//! nothing to show without one. So the moment worth starting it is the moment
//! someone picks a wallpaper and nothing is there to render it.
//!
//! It is started **detached** — its own session, no controlling terminal, no
//! inherited streams — because it outlives the window that asked for it. A
//! wallpaper that vanishes when the picker is closed is not a wallpaper.
//!
//! One engine serves every screen. A second would fight the first for the same
//! control socket, so nothing here starts one while another is alive, which
//! also means an engine somebody else's script started is left alone.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::Backend as _;

/// How long to wait for a new engine to answer.
///
/// It answers once it has built the scene, and a heavy one takes seconds. The
/// script this mirrors waits ten; a slow first run on a cold shader cache can
/// want more.
const READY: Duration = Duration::from_secs(25);

/// How often to ask while waiting.
const POLL: Duration = Duration::from_millis(200);

/// Whether a renderer is already running, whoever started it.
///
/// Read from `/proc` rather than a pid file, because the pid file belongs to
/// whichever script wrote it and there may not be one. Processes that cannot
/// be read are somebody else's and are not kirie's.
#[must_use]
pub fn running() -> bool {
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    entries.flatten().any(|entry| {
        // `/proc/<pid>/exe` is the binary itself, so a process that renamed
        // its command line still answers honestly, and no shell running the
        // word "kirie" is mistaken for the engine.
        std::fs::read_link(entry.path().join("exe"))
            .is_ok_and(|exe| exe.file_name().is_some_and(|name| name == "kirie"))
    })
}

/// The screens this machine has, asked of the kernel.
///
/// What a renderer would report, except there is no renderer yet: the engine
/// has to be told which output to own on its command line, and "the one kirie
/// says it has" is unavailable precisely when it is needed. Connector names
/// are what compositors call outputs, so `card1-DP-1` is the `DP-1` a
/// `--screen-root` wants.
#[must_use]
pub fn connectors() -> Vec<String> {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };

    let mut found: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let status = std::fs::read_to_string(entry.path().join("status")).ok()?;
            if status.trim() != "connected" {
                return None;
            }
            let name = entry.file_name().into_string().ok()?;
            // `card1-DP-1` — the card is this machine's business, not the
            // compositor's.
            let connector = name.split_once('-').map(|(_, rest)| rest)?;
            // A writeback connector is a capture target, not a screen.
            (!connector.starts_with("Writeback")).then(|| connector.to_owned())
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

/// Starts a renderer showing `dir` on `screen`, and waits for it to answer.
///
/// Blocking, for up to [`READY`]: the caller is expected to be off the drawing
/// thread. Returns once the control socket replies, which is also when the
/// wallpaper is actually up.
///
/// # Errors
/// When one is already running, when the process cannot be started, or when it
/// never answers.
pub fn start(binary: &Path, socket: &Path, screen: &str, dir: &Path) -> Result<(), String> {
    if running() {
        return Err("a renderer is already running".to_owned());
    }
    // Checked here rather than left to the spawn: `setsid` is what gets
    // started, and it reports a missing engine by exiting quietly, which would
    // otherwise be indistinguishable from an engine that is slow to answer.
    if !binary.is_file() {
        return Err(format!("no renderer at {}", binary.display()));
    }

    // The `--flag=value` form on purpose: an engine that predates a flag skips
    // the whole token, where `--flag value` leaves the value behind as a
    // positional argument and is read as a wallpaper.
    let arguments = [
        format!("--control-socket={}", socket.display()),
        format!("--screen-root={screen}"),
        format!("--bg={}", dir.display()),
    ];

    spawn_detached(binary, &arguments)?;
    wait_for(socket)
}

/// Where a started engine's output goes.
///
/// A file rather than nothing: when an engine refuses to start, its own
/// complaint is the only thing that says why, and a window cannot show what
/// went to `/dev/null`.
#[must_use]
pub fn log() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("haru-kirie.log")
}

/// Starts the engine in a session of its own, holding none of this process's
/// streams.
///
/// Detached through `setsid(1)` rather than the syscall, because a session is
/// made by `setsid(2)` between fork and exec, and reaching that from Rust
/// means `pre_exec`, which is unsafe — and this workspace does not write
/// unsafe. The binary is part of util-linux and is on every Linux this runs
/// on; without it the engine is still spawned and still outlives the window,
/// it merely shares this process's session and would take a terminal's
/// Ctrl-C with it.
fn spawn_detached(binary: &Path, arguments: &[String]) -> Result<(), String> {
    let mut command = match which("setsid") {
        Some(setsid) => {
            let mut wrapper = Command::new(setsid);
            wrapper.arg(binary);
            wrapper
        }
        None => Command::new(binary),
    };
    command.args(arguments);

    // Nothing of this process's: stdin so it can never read from a terminal,
    // and its own log so a refusal has somewhere to say why.
    command.stdin(Stdio::null());
    match std::fs::File::create(log()).and_then(|file| Ok((file.try_clone()?, file))) {
        Ok((errors, output)) => {
            command
                .stdout(Stdio::from(output))
                .stderr(Stdio::from(errors));
        }
        Err(_) => {
            command.stdout(Stdio::null()).stderr(Stdio::null());
        }
    }

    command
        .spawn()
        .map(drop)
        .map_err(|error| format!("could not start the renderer ({error})"))
}

/// Where a program is on `PATH`, if it is there at all.
fn which(program: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|directory| directory.join(program))
        .find(|candidate| candidate.is_file())
}

/// Waits until the engine answers on its socket.
fn wait_for(socket: &Path) -> Result<(), String> {
    let started = Instant::now();
    let engine = crate::Kirie::new(Some(socket.to_path_buf()));
    while started.elapsed() < READY {
        if engine.available() {
            return Ok(());
        }
        std::thread::sleep(POLL);
    }
    Err(format!(
        "the renderer did not answer within {} seconds — see {}",
        READY.as_secs(),
        log().display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_writeback_connector_is_not_a_screen() {
        // It is a capture target. Offering it as a place to put a wallpaper
        // would launch an engine that owns nothing anybody can see.
        assert!(
            connectors()
                .iter()
                .all(|name| !name.starts_with("Writeback"))
        );
    }

    #[test]
    fn a_connector_is_named_the_way_a_compositor_names_it() {
        // `card1-DP-1` on disk is `DP-1` on the command line.
        for name in connectors() {
            assert!(!name.starts_with("card"), "{name}");
        }
    }

    #[test]
    fn starting_one_beside_another_is_refused() {
        // Only when one is actually up: this machine may have none.
        if running() {
            let refused = start(
                Path::new("/nonexistent/kirie"),
                Path::new("/nonexistent/haru-test.sock"),
                "DP-1",
                Path::new("/tmp"),
            );
            assert_eq!(refused, Err("a renderer is already running".to_owned()));
        }
    }

    #[test]
    fn a_missing_binary_is_said_plainly_rather_than_waited_out() {
        // `setsid` starts fine and the engine never does, so without this the
        // answer is a 25-second timeout blamed on a slow renderer.
        if running() {
            return;
        }
        let refused = start(
            Path::new("/nonexistent/kirie"),
            Path::new("/nonexistent/haru-test.sock"),
            "DP-1",
            Path::new("/tmp"),
        );
        assert_eq!(refused, Err("no renderer at /nonexistent/kirie".to_owned()));
    }
}

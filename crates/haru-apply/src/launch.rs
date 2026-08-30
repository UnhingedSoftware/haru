use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::Backend as _;

const READY: Duration = Duration::from_secs(25);

const APPEARS: Duration = Duration::from_secs(10);

const POLL: Duration = Duration::from_millis(200);

const STOP: Duration = Duration::from_secs(8);

#[must_use]
#[cfg(target_os = "linux")]
pub fn pid() -> Option<u32> {
    std::fs::read_dir("/proc")
        .ok()?
        .flatten()
        .find_map(|entry| {
            let exe = std::fs::read_link(entry.path().join("exe")).ok()?;
            if exe.file_name()? != "kirie" {
                return None;
            }
            entry.file_name().to_str()?.parse().ok()
        })
}

#[cfg(not(target_os = "linux"))]
pub fn pid() -> Option<u32> {
    let listed = Command::new("ps")
        .args(["-Ao", "pid=,comm="])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    first_kirie(&String::from_utf8_lossy(&listed.stdout))
}

#[cfg(not(target_os = "linux"))]
fn first_kirie(listing: &str) -> Option<u32> {
    listing.lines().find_map(|line| {
        let (pid, command) = line.trim_start().split_once(char::is_whitespace)?;
        let name = command.trim().rsplit('/').next()?;
        (name == "kirie").then(|| pid.parse().ok())?
    })
}

#[must_use]
pub fn running() -> bool {
    pid().is_some()
}

pub const DESKTOP: &str = "Desktop";

#[cfg(not(target_os = "linux"))]
#[must_use]
pub fn connectors() -> Vec<String> {
    vec![DESKTOP.to_owned()]
}

#[cfg(target_os = "linux")]
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
            let connector = name.split_once('-').map(|(_, rest)| rest)?;
            (!connector.starts_with("Writeback")).then(|| connector.to_owned())
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub screen: String,
    pub wallpaper: Option<PathBuf>,
}

impl Plan {
    #[must_use]
    pub fn showing(screen: impl Into<String>, wallpaper: impl Into<PathBuf>) -> Self {
        Self {
            screen: screen.into(),
            wallpaper: Some(wallpaper.into()),
        }
    }

    #[must_use]
    pub fn empty(screen: impl Into<String>) -> Self {
        Self {
            screen: screen.into(),
            wallpaper: None,
        }
    }
}

#[must_use]
pub fn arguments_for(socket: &Path, plan: &[Plan]) -> Vec<String> {
    let mut arguments = vec![format!("--control-socket={}", socket.display())];
    arguments.extend(haru_core::Config::load().renderer.arguments());
    for screen in plan {
        if cfg!(target_os = "linux") {
            arguments.push(format!("--screen-root={}", screen.screen));
        }
        if let Some(wallpaper) = screen.wallpaper.as_ref() {
            arguments.push(format!("--bg={}", wallpaper.display()));
        }
    }
    arguments
}

pub fn start(binary: &Path, socket: &Path, plan: &[Plan]) -> Result<(), String> {
    if running() {
        return Err("a renderer is already running".to_owned());
    }
    if plan.is_empty() {
        return Err("no screen to start on".to_owned());
    }
    if !plan.iter().any(|screen| screen.wallpaper.is_some()) {
        return Err("no wallpaper to start with".to_owned());
    }
    if !binary.is_file() {
        return Err(format!("no renderer at {}", binary.display()));
    }

    let arguments = arguments_for(socket, plan);

    spawn_detached(binary, &arguments)?;
    if cfg!(target_os = "linux") {
        return wait_for(socket);
    }
    wait_for_process()
}

fn wait_for_process() -> Result<(), String> {
    let deadline = Instant::now() + APPEARS;
    while Instant::now() < deadline {
        if running() {
            return Ok(());
        }
        std::thread::sleep(POLL);
    }
    Err(why_it_never_started())
}

fn why_it_never_started() -> String {
    let said = std::fs::read_to_string(log())
        .ok()
        .and_then(|text| {
            text.lines()
                .rev()
                .find(|line| !line.trim().is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_default();

    if said.is_empty() {
        return format!(
            "the renderer started and died without saying why. On macOS that is usually a \
             broken code signature — reinstall it with `rm -f {0} && cp <build> {0}`, or run \
             `codesign --force --sign - {0}`",
            crate::install::installed()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "the renderer".to_owned())
        );
    }
    format!("the renderer did not come up: {said}")
}

pub fn stop() -> Result<(), String> {
    let Some(pid) = pid() else {
        return Err("no renderer is running".to_owned());
    };

    let sent = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| format!("could not stop the renderer ({error})"))?;
    if !sent.success() {
        return Err("the renderer refused to stop".to_owned());
    }

    let deadline = Instant::now() + STOP;
    while Instant::now() < deadline {
        if !running() {
            return Ok(());
        }
        std::thread::sleep(POLL);
    }
    Err("the renderer is still running".to_owned())
}

pub fn restart(binary: &Path, socket: &Path, plan: &[Plan]) -> Result<(), String> {
    if running() {
        stop()?;
    }
    start(binary, socket, plan)
}

#[must_use]
pub fn log() -> PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("haru-kirie.log")
}

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
    for (key, value) in crate::renderer_env() {
        command.env(key, value);
    }

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

pub(crate) fn which(program: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    std::env::split_paths(&paths)
        .map(|directory| directory.join(program))
        .find(|candidate| candidate.is_file())
}

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
        assert!(
            connectors()
                .iter()
                .all(|name| !name.starts_with("Writeback"))
        );
    }

    #[test]
    fn a_connector_is_named_the_way_a_compositor_names_it() {
        for name in connectors() {
            assert!(!name.starts_with("card"), "{name}");
        }
    }

    #[test]
    fn a_plan_with_no_wallpaper_anywhere_is_refused() {
        if running() {
            return;
        }
        let refused = start(
            Path::new("/nonexistent/kirie"),
            Path::new("/nonexistent/haru-test.sock"),
            &[Plan::empty("DP-1"), Plan::empty("HDMI-A-1")],
        );
        assert_eq!(refused, Err("no wallpaper to start with".to_owned()));
    }

    #[test]
    fn an_empty_plan_is_refused() {
        if running() {
            return;
        }
        let refused = start(
            Path::new("/nonexistent/kirie"),
            Path::new("/nonexistent/haru-test.sock"),
            &[],
        );
        assert_eq!(refused, Err("no screen to start on".to_owned()));
    }

    #[test]
    fn stopping_nothing_says_so() {
        if !running() {
            assert_eq!(stop(), Err("no renderer is running".to_owned()));
        }
    }

    #[test]
    fn starting_one_beside_another_is_refused() {
        if running() {
            let refused = start(
                Path::new("/nonexistent/kirie"),
                Path::new("/nonexistent/haru-test.sock"),
                &[Plan::showing("DP-1", "/tmp")],
            );
            assert_eq!(refused, Err("a renderer is already running".to_owned()));
        }
    }

    #[test]
    fn a_missing_binary_is_said_plainly_rather_than_waited_out() {
        if running() {
            return;
        }
        let refused = start(
            Path::new("/nonexistent/kirie"),
            Path::new("/nonexistent/haru-test.sock"),
            &[Plan::showing("DP-1", "/tmp")],
        );
        assert_eq!(refused, Err("no renderer at /nonexistent/kirie".to_owned()));
    }
}

#[cfg(all(test, not(target_os = "linux")))]
mod tests {
    use super::first_kirie;

    #[test]
    fn the_renderer_is_found_by_its_own_name() {
        let listing = "  501 /usr/sbin/cfprefsd\n  733 /Users/me/.local/bin/kirie\n";
        assert_eq!(first_kirie(listing), Some(733));
    }

    #[test]
    fn something_merely_mentioning_it_is_not_the_renderer() {
        let listing = "  90 /Applications/kirie-helper\n  91 /usr/bin/haru\n";
        assert_eq!(first_kirie(listing), None);
    }

    #[test]
    fn an_empty_table_finds_nothing() {
        assert_eq!(first_kirie(""), None);
    }
}

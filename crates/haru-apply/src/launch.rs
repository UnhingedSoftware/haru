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

#[cfg(target_os = "macos")]
pub fn pid() -> Option<u32> {
    let listed = crate::child::quiet("ps")
        .args(["-Ao", "pid=,comm="])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    first_kirie(&String::from_utf8_lossy(&listed.stdout))
}

#[cfg(target_os = "macos")]
fn first_kirie(listing: &str) -> Option<u32> {
    listing.lines().find_map(|line| {
        let (pid, command) = line.trim_start().split_once(char::is_whitespace)?;
        let name = command.trim().rsplit('/').next()?;
        (name == "kirie").then(|| pid.parse().ok())?
    })
}

/// Windows has no `/proc` and no `ps`, but `tasklist` answers the same
/// question. Asked for one image name in CSV with no header, it prints one
/// quoted row per match and the pid is its second field.
#[cfg(windows)]
pub fn pid() -> Option<u32> {
    let listed = crate::child::quiet("tasklist")
        .args(["/FI", "IMAGENAME eq kirie.exe", "/NH", "/FO", "CSV"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    first_kirie(&String::from_utf8_lossy(&listed.stdout))
}

#[cfg(windows)]
fn first_kirie(listing: &str) -> Option<u32> {
    listing.lines().find_map(|line| {
        // "kirie.exe","1234","Console","1","52,000 K" -- and, when nothing
        // matched, a sentence saying so, which has no quoted pid to find.
        let mut fields = line
            .split('"')
            .filter(|field| *field != "," && !field.is_empty());
        let name = fields.next()?;
        if !name.eq_ignore_ascii_case("kirie.exe") {
            return None;
        }
        fields.next()?.trim().parse().ok()
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
        let wallpaper = wallpaper.into();
        Self {
            screen: screen.into(),
            wallpaper: (!wallpaper.as_os_str().is_empty()).then_some(wallpaper),
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
        if let Some(wallpaper) = screen
            .wallpaper
            .as_ref()
            .filter(|path| !path.as_os_str().is_empty())
        {
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
        let renderer = crate::install::installed()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "the renderer".to_owned());
        if cfg!(windows) {
            return format!(
                "the renderer started and died without saying why. On Windows that is \
                 usually a graphics driver too old for Vulkan or DirectX 12, or an \
                 anti-virus holding {renderer} — try running it yourself from a terminal \
                 to see what it says"
            );
        }
        return format!(
            "the renderer started and died without saying why. On macOS that is usually a \
             broken code signature — reinstall it with `rm -f {renderer} && cp <build> \
             {renderer}`, or run `codesign --force --sign - {renderer}`"
        );
    }
    format!("the renderer did not come up: {said}")
}

pub fn stop() -> Result<(), String> {
    let Some(pid) = pid() else {
        return Err("no renderer is running".to_owned());
    };

    // Windows has no signals to send, and `taskkill` without /F still asks
    // politely: it posts WM_CLOSE first, which the renderer's own handler takes
    // as its cue to unlink the socket.
    let mut stopper = if cfg!(windows) {
        let mut command = crate::child::quiet("taskkill");
        command.args(["/PID", &pid.to_string()]);
        command
    } else {
        let mut command = crate::child::quiet("kill");
        command.args(["-TERM", &pid.to_string()]);
        command
    };
    let sent = stopper
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
    haru_core::runtime_dir().join("haru-kirie.log")
}

fn spawn_detached(binary: &Path, arguments: &[String]) -> Result<(), String> {
    let mut command = match setsid() {
        Some(setsid) => {
            let mut wrapper = Command::new(setsid);
            wrapper.arg(binary);
            wrapper
        }
        None => Command::new(binary),
    };
    command.args(arguments);
    // What `setsid` does on Linux, this flag does here: the renderer gets no
    // console of its own, so it outlives haru instead of dying with the window
    // that started it.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;

        const DETACHED_PROCESS: u32 = 0x0000_0008;
        command.creation_flags(DETACHED_PROCESS | crate::child::NO_WINDOW);
    }
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

/// `setsid` is a unix program; Windows detaches with a flag instead.
#[cfg(unix)]
fn setsid() -> Option<PathBuf> {
    which("setsid")
}

#[cfg(windows)]
const fn setsid() -> Option<PathBuf> {
    None
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

    #[test]
    fn a_screen_with_no_wallpaper_named_is_not_shown_one() {
        let plan = Plan::showing("DP-1", PathBuf::new());
        assert_eq!(plan.wallpaper, None);
    }

    #[test]
    fn an_empty_wallpaper_never_reaches_the_renderer() {
        let plan = [Plan {
            screen: "DP-1".to_owned(),
            wallpaper: Some(PathBuf::new()),
        }];
        let args = arguments_for(Path::new("/run/lwe.sock"), &plan);
        assert!(
            !args.iter().any(|arg| arg.starts_with("--bg")),
            "{args:?} must not carry an empty background"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_renderer_is_found_by_its_own_name() {
        let listing = "  501 /usr/sbin/cfprefsd\n  733 /Users/me/.local/bin/kirie\n";
        assert_eq!(super::first_kirie(listing), Some(733));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn something_merely_mentioning_it_is_not_the_renderer() {
        let listing = "  90 /Applications/kirie-helper\n  91 /usr/bin/haru\n";
        assert_eq!(super::first_kirie(listing), None);
    }

    #[cfg(windows)]
    #[test]
    fn the_renderer_is_found_in_a_tasklist_row() {
        let listing = "\"kirie.exe\",\"733\",\"Console\",\"1\",\"52,000 K\"\r\n";
        assert_eq!(super::first_kirie(listing), Some(733));
    }

    #[cfg(windows)]
    #[test]
    fn tasklist_saying_it_found_nothing_is_not_a_renderer() {
        let listing = "INFO: No tasks are running which match the specified criteria.\r\n";
        assert_eq!(super::first_kirie(listing), None);
    }

    #[cfg(windows)]
    #[test]
    fn something_merely_mentioning_it_is_not_the_renderer() {
        let listing = "\"kirie-helper.exe\",\"90\",\"Console\",\"1\",\"8,000 K\"\r\n";
        assert_eq!(super::first_kirie(listing), None);
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn an_empty_table_finds_nothing() {
        assert_eq!(super::first_kirie(""), None);
    }
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

use std::path::{Path, PathBuf};

mod child;
pub mod desktop;
pub mod engine;
pub mod install;
mod kirie;
pub mod launch;
mod monitors;
mod offscreen;
mod relaunch;
mod socket;
pub mod startup;
mod stream;
pub mod update;
pub mod webview2;

pub use engine::{Engine, Snapshot};
pub use kirie::Kirie;
pub use offscreen::Offscreen;
pub use relaunch::Relaunch;

#[must_use]
pub fn default_socket() -> PathBuf {
    kirie::default_socket()
}
pub use stream::{Frame, Preview as PreviewStream};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    /// What the renderer calls it, and what commands to it must use.
    pub name: String,
    /// What to show a person: the monitor's own name where the system has
    /// one, since Windows' `DISPLAY6` says nothing about which screen it is.
    pub label: String,
    pub current: Option<PathBuf>,
}

impl Screen {
    #[must_use]
    pub fn new(name: impl Into<String>, current: Option<PathBuf>) -> Self {
        let name = name.into();
        Self {
            label: name.clone(),
            name,
            current,
        }
    }
}

pub trait Backend: Send + Sync {
    fn name(&self) -> &'static str;

    fn available(&self) -> bool;

    fn screens(&self) -> Result<Vec<Screen>, String>;

    fn apply(&self, screen: &str, dir: &Path) -> Result<(), String>;

    fn set_property(&self, screen: &str, key: &str, value: &str) -> Result<(), String>;

    fn stage(&self, key: &str, value: &str) -> Result<(), String>;

    fn tune(&self, commands: &[String]) -> Result<(), String> {
        let _ = commands;
        Ok(())
    }
}

/// Hand a URL or a directory to whatever the desktop opens it with.
///
/// The Windows arm used to be `cmd /C start "" <target>`. Rust only quotes an
/// argument on Windows when it contains a space, so a URL went to `cmd`
/// unquoted and `cmd` read the first `&` in it as a command separator: the
/// Steam browser sign-in link, which ends `...&redir_ssl=1`, opened truncated
/// and the sign-in did not work. `explorer` takes the target as one argument
/// with no shell in between, and opens `https:`, `steam:` and plain
/// directories all the same way.
pub fn open_link(target: &str) {
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = crate::child::quiet(program).arg(target).spawn();
}

#[must_use]
pub fn renderer_env() -> Vec<(&'static str, std::ffi::OsString)> {
    let mut set: Vec<(&'static str, std::ffi::OsString)> = Vec::new();
    if let Some(assets) = haru_core::engine::found() {
        set.push(("KIRIE_WE_ASSETS", assets.into_os_string()));
    }
    let roots = haru_core::Config::load().libraries();
    if let Ok(joined) = std::env::join_paths(roots)
        && !joined.is_empty()
    {
        set.push(("KIRIE_STEAM_LIBRARY", joined));
    }
    set
}

#[must_use]
pub fn for_this_platform(socket: Option<PathBuf>) -> Box<dyn Backend> {
    if cfg!(target_os = "linux") {
        return Box::new(Kirie::new(socket));
    }
    let live = Kirie::new(socket.clone());
    if live.available() {
        return Box::new(live);
    }
    Box::new(Relaunch::new(
        socket.unwrap_or_else(crate::kirie::default_socket),
    ))
}

#[must_use]
pub fn detect(socket: Option<PathBuf>) -> Option<Box<dyn Backend>> {
    let kirie = Kirie::new(socket.clone());
    if kirie.available() {
        return Some(Box::new(kirie));
    }
    if cfg!(target_os = "linux") {
        return None;
    }
    let socket = socket.unwrap_or_else(crate::kirie::default_socket);
    let relaunch = Relaunch::new(socket);
    relaunch
        .available()
        .then(|| Box::new(relaunch) as Box<dyn Backend>)
}

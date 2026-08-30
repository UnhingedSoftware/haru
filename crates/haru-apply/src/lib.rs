use std::path::{Path, PathBuf};

pub mod engine;
pub mod install;
mod kirie;
pub mod launch;
mod offscreen;
mod relaunch;
mod stream;

pub use engine::{Engine, Snapshot};
pub use kirie::Kirie;
pub use offscreen::Offscreen;
pub use relaunch::Relaunch;
pub use stream::{Frame, Preview as PreviewStream};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    pub name: String,
    pub current: Option<PathBuf>,
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

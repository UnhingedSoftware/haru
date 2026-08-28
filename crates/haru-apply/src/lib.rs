use std::path::{Path, PathBuf};

pub mod install;
mod kirie;
pub mod launch;
mod offscreen;
mod stream;

pub use kirie::Kirie;
pub use offscreen::Offscreen;
pub use stream::{Frame, Preview as PreviewStream};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    pub name: String,
    pub current: Option<PathBuf>,
}

pub trait Backend {
    fn name(&self) -> &'static str;

    fn available(&self) -> bool;

    fn screens(&self) -> Result<Vec<Screen>, String>;

    fn apply(&self, screen: &str, dir: &Path) -> Result<(), String>;

    fn set_property(&self, screen: &str, key: &str, value: &str) -> Result<(), String>;

    fn stage(&self, key: &str, value: &str) -> Result<(), String>;
}

#[must_use]
pub fn detect(socket: Option<PathBuf>) -> Option<Box<dyn Backend>> {
    let kirie = Kirie::new(socket);
    kirie
        .available()
        .then(|| Box::new(kirie) as Box<dyn Backend>)
}

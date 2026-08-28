//! Putting a wallpaper on a screen.
//!
//! haru installs wallpapers everywhere and renders none of them. What renders
//! them differs per platform — kirie on Linux, Wallpaper Engine itself on
//! Windows — so applying is a trait with one implementation per backend, and
//! the picker holds a `dyn Backend` without knowing which it got.
//!
//! Nothing here decides *what* to apply. A backend is handed a directory and a
//! screen name and reports whether it worked.

use std::path::{Path, PathBuf};

pub mod install;
mod kirie;
pub mod launch;
mod offscreen;
mod stream;

pub use kirie::Kirie;
pub use offscreen::Offscreen;
pub use stream::{Frame, Preview as PreviewStream};

/// A screen a wallpaper can go on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Screen {
    /// What the backend calls it — a connector name, usually: `DP-1`.
    pub name: String,
    /// What is on it now, when the backend knows.
    pub current: Option<PathBuf>,
}

/// Something that can put a wallpaper on a screen.
pub trait Backend {
    /// What to call this backend in a settings pane.
    fn name(&self) -> &'static str;

    /// Whether it is usable right now.
    ///
    /// Separate from the constructor: a renderer that is installed but not
    /// running is a normal state, and the picker says so rather than hiding
    /// the backend or failing to start.
    fn available(&self) -> bool;

    /// The screens it knows about.
    ///
    /// # Errors
    /// When the backend cannot be reached.
    fn screens(&self) -> Result<Vec<Screen>, String>;

    /// Puts `dir` on `screen`.
    ///
    /// # Errors
    /// When the backend refuses or cannot be reached. A refusal leaves
    /// whatever was on the screen already.
    fn apply(&self, screen: &str, dir: &Path) -> Result<(), String>;

    /// Changes one of the current wallpaper's own settings.
    ///
    /// Applies to whatever is on `screen` right now, which is the only thing
    /// the renderer has loaded: settings belong to a wallpaper *in place*, not
    /// to one sitting on disk.
    ///
    /// # Errors
    /// When the backend refuses the value or cannot be reached.
    fn set_property(&self, screen: &str, key: &str, value: &str) -> Result<(), String>;

    /// Holds a value for the *next* wallpaper the backend loads.
    ///
    /// What editing a wallpaper that is not up yet needs: the value cannot be
    /// applied to something not loaded, so it is staged and folded into the
    /// build when that wallpaper goes on a screen.
    ///
    /// # Errors
    /// When the backend refuses the value or cannot be reached.
    fn stage(&self, key: &str, value: &str) -> Result<(), String>;
}

/// The backend for this machine, if there is one.
///
/// One probe rather than a list, because a machine has one desktop: the answer
/// to "what renders wallpapers here" is not a preference.
#[must_use]
pub fn detect(socket: Option<PathBuf>) -> Option<Box<dyn Backend>> {
    let kirie = Kirie::new(socket);
    kirie
        .available()
        .then(|| Box::new(kirie) as Box<dyn Backend>)
}

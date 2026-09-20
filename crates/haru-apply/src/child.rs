//! Starting another program without a console window flashing up.
//!
//! haru is a window, not a command, so a release build on Windows has no
//! console of its own. Every console program it runs — `tasklist`, `taskkill`,
//! the renderer when asked for a version or a screenshot — would otherwise be
//! handed a brand new one, which appears on screen for as long as the program
//! takes and then vanishes. `CREATE_NO_WINDOW` says not to bother. Everywhere
//! else this is `Command::new` and nothing more.

use std::ffi::OsStr;
use std::process::Command;

/// The flag that keeps a console program's window off the screen.
#[cfg(windows)]
pub(crate) const NO_WINDOW: u32 = 0x0800_0000;

#[must_use]
#[cfg(windows)]
pub(crate) fn quiet(program: impl AsRef<OsStr>) -> Command {
    use std::os::windows::process::CommandExt as _;

    let mut command = Command::new(program);
    command.creation_flags(NO_WINDOW);
    command
}

#[must_use]
#[cfg(not(windows))]
pub(crate) fn quiet(program: impl AsRef<OsStr>) -> Command {
    Command::new(program)
}

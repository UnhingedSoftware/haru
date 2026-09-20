//! One name for the socket haru talks to the renderer over.
//!
//! The control socket is a unix-domain socket at a path on disk. Windows has
//! had those since Windows 10 1803; what it does not have is a way to reach
//! them from the standard library, which files them under `std::os::unix`.
//! `uds_windows` is the same sockets behind the same API, so the two sides keep
//! the same path, the same protocol and the same code above this line.

#[cfg(unix)]
pub use std::os::unix::net::UnixStream;

#[cfg(windows)]
pub use uds_windows::UnixStream;

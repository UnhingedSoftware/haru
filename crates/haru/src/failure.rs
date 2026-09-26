//! Saying why haru stopped, somewhere a person will see it.
//!
//! A release build on Windows has no console, so everything haru printed on
//! the way out went nowhere: a window that could not open, or a panic, looked
//! exactly like double-clicking and nothing happening. Every way `main` gives
//! up now goes through `report`, which still prints, also appends to a log in
//! haru's data folder, and on Windows puts up a message box.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Mutex;

/// The last panic, kept for `main` to report once the event loop has unwound.
static LAST_PANIC: Mutex<Option<String>> = Mutex::new(None);

/// Log every panic, and keep the last one for `panicked`.
///
/// Rust's own hook still runs first, so a console, when there is one, shows
/// what it always did. The message box is left to `main`: putting one up from
/// inside the hook would run a modal loop while winit is still unwinding.
pub fn record_panics() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        default(info);
        let thread = std::thread::current();
        let said = format!(
            "haru crashed in thread {}: {info}",
            thread.name().unwrap_or("unnamed")
        );
        let _ = log(&said);
        if let Ok(mut last) = LAST_PANIC.lock() {
            *last = Some(said);
        }
    }));
}

/// Report the panic that ended the event loop.
pub fn panicked() -> ExitCode {
    let said = LAST_PANIC
        .lock()
        .ok()
        .and_then(|mut last| last.take())
        .unwrap_or_else(|| "haru crashed".to_owned());
    // Already logged by the hook.
    show(&said, log_path().as_deref());
    ExitCode::FAILURE
}

/// Say why haru could not go on, everywhere it might be seen.
pub fn report(message: &str) -> ExitCode {
    eprintln!("haru: {message}");
    let logged = log(message).ok();
    show(message, logged.as_deref());
    ExitCode::FAILURE
}

/// Why eframe could not open the window, in words that point at the fix.
pub fn window_error(error: &eframe::Error) -> String {
    match error {
        eframe::Error::Wgpu(_) => format!(
            "haru could not find a graphics driver it can draw with (Vulkan or OpenGL). \
             Updating the graphics driver usually fixes this.\n\n({error})"
        ),
        error => format!("haru could not open its window.\n\n({error})"),
    }
}

fn log_path() -> Option<PathBuf> {
    Some(haru_core::data_home()?.join("haru").join("haru.log"))
}

/// Append to haru's log, returning where it went.
fn log(message: &str) -> std::io::Result<PathBuf> {
    let path = log_path().ok_or_else(|| std::io::Error::other("no data folder"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs())
        .unwrap_or(0);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "[unix time {seconds}] {message}")?;
    Ok(path)
}

#[cfg(windows)]
fn show(message: &str, logged: Option<&std::path::Path>) {
    let description = match logged {
        Some(path) => format!("{message}\n\nThis was also saved to {}", path.display()),
        None => message.to_owned(),
    };
    let _ = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title("haru")
        .set_description(description)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

// Elsewhere haru is started from a terminal often enough, and a desktop
// launcher keeps what it printed in the journal or Console.
#[cfg(not(windows))]
const fn show(_message: &str, _logged: Option<&std::path::Path>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_graphics_driver_is_named_as_such() {
        let said = window_error(&eframe::Error::Wgpu(
            eframe::egui_wgpu::WgpuError::NoSuitableAdapterFound,
        ));
        assert!(said.contains("graphics driver"), "{said}");
        assert!(said.contains("no suitable adapter"), "{said}");
    }
}

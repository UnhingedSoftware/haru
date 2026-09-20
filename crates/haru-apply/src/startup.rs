use std::path::PathBuf;
use std::process::Stdio;

const LABEL: &str = "dev.unhingedsoftware.kirie";

/// The file whose existence makes the wallpaper come back at login.
///
/// Each desktop has its own idea of what that file is: a launch agent on
/// macOS, a systemd user unit or an autostart entry on Linux, and on Windows a
/// script in the Start menu's Startup folder, which needs no registry write
/// and no shortcut to build.
#[must_use]
#[cfg(windows)]
pub fn entry() -> Option<PathBuf> {
    Some(startup_folder()?.join("kirie.vbs"))
}

#[cfg(windows)]
fn startup_folder() -> Option<PathBuf> {
    let roaming = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())?;
    Some(roaming.join(r"Microsoft\Windows\Start Menu\Programs\Startup"))
}

/// Startup entries earlier versions left behind, to be swept up.
///
/// The wallpaper used to start from a `kirie.cmd` in the same folder. Leaving
/// one there would put a second renderer up at every login, and `enabled()`
/// would read the switch as off while it ran.
#[cfg(windows)]
fn stale_entries() -> Vec<PathBuf> {
    startup_folder().map_or_else(Vec::new, |folder| vec![folder.join("kirie.cmd")])
}

#[cfg(not(windows))]
fn stale_entries() -> Vec<PathBuf> {
    Vec::new()
}

#[must_use]
#[cfg(unix)]
pub fn entry() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    if cfg!(target_os = "macos") {
        return Some(home.join(format!("Library/LaunchAgents/{LABEL}.plist")));
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".config"));
    if systemd_user() {
        return Some(base.join("systemd/user/kirie.service"));
    }
    Some(base.join("autostart/kirie.desktop"))
}

#[must_use]
pub fn enabled() -> bool {
    entry().is_some_and(|path| path.is_file())
}

pub fn enable(command: &[String], environment: &[(String, String)]) -> Result<(), String> {
    let path = entry().ok_or("no home directory to install into")?;
    let parent = path.parent().ok_or("no directory to install into")?;
    std::fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;

    let text = if cfg!(windows) {
        script(command, environment)
    } else if cfg!(target_os = "macos") {
        plist(command, environment)
    } else if systemd_user() {
        unit(command, environment)
    } else {
        desktop_entry(command, environment)
    };
    std::fs::write(&path, text).map_err(|error| format!("{}: {error}", path.display()))?;

    for stale in stale_entries() {
        if stale != path {
            let _ = std::fs::remove_file(&stale);
        }
    }
    register(&path);
    Ok(())
}

pub fn disable() -> Result<(), String> {
    let path = entry().ok_or("no home directory to look in")?;
    for stale in stale_entries() {
        if stale != path {
            let _ = std::fs::remove_file(&stale);
        }
    }
    if !path.is_file() {
        return Ok(());
    }
    unregister(&path);
    std::fs::remove_file(&path).map_err(|error| format!("{}: {error}", path.display()))
}

fn register(path: &std::path::Path) {
    if cfg!(target_os = "macos") {
        run("launchctl", &["unload".into(), display(path)]);
        run("launchctl", &["load".into(), "-w".into(), display(path)]);
        return;
    }
    if systemd_user() {
        run("systemctl", &["--user".into(), "daemon-reload".into()]);
        run(
            "systemctl",
            &["--user".into(), "enable".into(), "kirie.service".into()],
        );
    }
}

fn unregister(path: &std::path::Path) {
    if cfg!(target_os = "macos") {
        run("launchctl", &["unload".into(), "-w".into(), display(path)]);
        return;
    }
    if systemd_user() {
        run(
            "systemctl",
            &["--user".into(), "disable".into(), "kirie.service".into()],
        );
    }
}

fn display(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

fn run(program: &str, arguments: &[String]) {
    let _ = crate::child::quiet(program)
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn systemd_user() -> bool {
    cfg!(target_os = "linux") && crate::launch::which("systemctl").is_some()
}

#[must_use]
pub fn plist(command: &[String], environment: &[(String, String)]) -> String {
    let arguments = command
        .iter()
        .map(|part| format!("\t\t<string>{}</string>\n", escape(part)))
        .collect::<String>();
    let variables = environment
        .iter()
        .map(|(key, value)| {
            format!(
                "\t\t<key>{}</key>\n\t\t<string>{}</string>\n",
                escape(key),
                escape(value)
            )
        })
        .collect::<String>();

    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         \t<key>Label</key>\n\t<string>{LABEL}</string>\n\
         \t<key>ProgramArguments</key>\n\t<array>\n{arguments}\t</array>\n\
         \t<key>EnvironmentVariables</key>\n\t<dict>\n{variables}\t</dict>\n\
         \t<key>RunAtLoad</key>\n\t<true/>\n\
         \t<key>KeepAlive</key>\n\t<false/>\n\
         \t<key>ProcessType</key>\n\t<string>Background</string>\n\
         </dict>\n</plist>\n"
    )
}

#[must_use]
pub fn unit(command: &[String], environment: &[(String, String)]) -> String {
    let line = command
        .iter()
        .map(|part| quote(part))
        .collect::<Vec<_>>()
        .join(" ");
    let variables = environment
        .iter()
        .map(|(key, value)| format!("Environment={key}={}\n", quote(value)))
        .collect::<String>();

    format!(
        "[Unit]\n\
         Description=kirie wallpaper renderer\n\
         PartOf=graphical-session.target\n\
         After=graphical-session.target\n\n\
         [Service]\n\
         Type=simple\n\
         {variables}ExecStart={line}\n\
         Restart=on-failure\n\
         RestartSec=5\n\n\
         [Install]\n\
         WantedBy=graphical-session.target\n"
    )
}

/// The Startup-folder script Windows runs at login.
///
/// This is VBScript rather than a `.cmd` because the renderer is a console
/// program, and there is no way to start one from a batch file without a
/// console window. `start "" /B` ran it inside the Startup script's own
/// console, which then stayed on screen and in the taskbar for the whole
/// session -- and closing it, which is the obvious thing to do with a stray
/// black window, killed the wallpaper. Plain `start ""` only moves the window
/// rather than removing it. `WScript.Shell.Run` takes a window style, and 0
/// means the renderer never gets one; `False` says not to wait for it.
///
/// The cost is a dependency on Windows Script Host. It is present and enabled
/// on every stock Windows install, but an administrator can turn it off by
/// policy, and then nothing comes up at login.
#[must_use]
pub fn script(command: &[String], environment: &[(String, String)]) -> String {
    let variables = environment
        .iter()
        .map(|(key, value)| {
            format!(
                "shell.Environment(\"PROCESS\").Item({}) = {}\r\n",
                literal(key),
                literal(value)
            )
        })
        .collect::<String>();
    // The renderer reads this back with the usual Windows rules, so each
    // argument is quoted. Any quote inside one is dropped: nothing haru passes
    // contains one, and a half-quoted command line is worse than a lost
    // character.
    let line = command
        .iter()
        .map(|part| format!("\"{}\"", part.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" ");

    format!(
        "Set shell = CreateObject(\"WScript.Shell\")\r\n\
         {variables}shell.Run {}, 0, False\r\n",
        literal(&line)
    )
}

/// `text` as a VBScript string literal, where a quote is written twice.
fn literal(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}

#[must_use]
pub fn desktop_entry(command: &[String], environment: &[(String, String)]) -> String {
    let exported = environment
        .iter()
        .map(|(key, value)| format!("{key}={} ", quote(value)))
        .collect::<String>();
    let line = command
        .iter()
        .map(|part| quote(part))
        .collect::<Vec<_>>()
        .join(" ");

    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=kirie\n\
         Comment=Put the wallpaper up at login\n\
         Exec=sh -c \"{exported}exec {line}\"\n\
         Terminal=false\n\
         X-GNOME-Autostart-enabled=true\n"
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn quote(text: &str) -> String {
    if text.contains(' ') || text.contains('"') {
        format!("\"{}\"", text.replace('"', "\\\""))
    } else {
        text.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> (Vec<String>, Vec<(String, String)>) {
        (
            vec![
                "/home/me/.local/bin/kirie".to_owned(),
                "--bg=/tmp/a wallpaper".to_owned(),
            ],
            vec![("KIRIE_WE_ASSETS".to_owned(), "/tmp/assets".to_owned())],
        )
    }

    #[test]
    fn the_agent_runs_at_login_without_a_dock_icon() {
        let (command, environment) = sample();
        let text = plist(&command, &environment);
        assert!(text.contains("<key>RunAtLoad</key>\n\t<true/>"));
        assert!(text.contains("<string>Background</string>"));
        assert!(text.contains("<string>--bg=/tmp/a wallpaper</string>"));
        assert!(text.contains("<key>KIRIE_WE_ASSETS</key>"));
    }

    #[test]
    fn the_unit_waits_for_a_session_and_carries_the_assets() {
        let (command, environment) = sample();
        let text = unit(&command, &environment);
        assert!(text.contains("WantedBy=graphical-session.target"));
        assert!(text.contains("Environment=KIRIE_WE_ASSETS=/tmp/assets"));
        assert!(text.contains("ExecStart=/home/me/.local/bin/kirie \"--bg=/tmp/a wallpaper\""));
    }

    #[test]
    fn the_startup_script_sets_the_assets_and_lets_go() {
        let (command, environment) = sample();
        let text = script(&command, &environment);
        assert!(
            text.contains(
                "shell.Environment(\"PROCESS\").Item(\"KIRIE_WE_ASSETS\") = \"/tmp/assets\""
            ),
            "{text}"
        );
        // Window style 0, and False for "do not wait": the renderer outlives
        // the script and never shows a console.
        assert!(text.contains(", 0, False"), "{text}");
        assert!(
            text.contains(
                "shell.Run \"\"\"/home/me/.local/bin/kirie\"\" \"\"--bg=/tmp/a wallpaper\"\"\", 0, False"
            ),
            "{text}"
        );
        assert!(
            !text.contains("start \"\" /B"),
            "the console window is the bug: {text}"
        );
        assert!(
            text.ends_with("\r\n"),
            "the script host wants CRLF: {text:?}"
        );
    }

    #[test]
    fn a_quote_in_a_path_cannot_end_the_script_line_early() {
        let text = script(&[r#"C:\a"b\kirie.exe"#.to_owned()], &[]);
        // One opening and one closing quote for the VBScript literal, and the
        // doubled pair that stands for the argument's own quotes.
        assert!(
            text.contains("shell.Run \"\"\"C:\\ab\\kirie.exe\"\"\", 0, False"),
            "{text}"
        );
    }

    #[test]
    fn the_desktop_entry_exports_before_it_runs() {
        let (command, environment) = sample();
        let text = desktop_entry(&command, &environment);
        assert!(text.starts_with("[Desktop Entry]"));
        assert!(text.contains("KIRIE_WE_ASSETS=/tmp/assets exec /home/me/.local/bin/kirie"));
    }

    #[test]
    fn markup_in_a_path_cannot_break_the_plist() {
        let text = plist(&["/tmp/a&b<c>".to_owned()], &[]);
        assert!(text.contains("<string>/tmp/a&amp;b&lt;c&gt;</string>"));
    }
}

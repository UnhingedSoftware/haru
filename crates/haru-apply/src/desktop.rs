use std::path::{Path, PathBuf};

const DESKTOP: &str = include_str!("../../../packaging/haru.desktop");
const SVG: &[u8] = include_bytes!("../../../packaging/haru.svg");
const LARGE: &[u8] = include_bytes!("../../../packaging/haru-1024.png");
const PNGS: [(u32, &[u8]); 4] = [
    (48, include_bytes!("../../../packaging/haru-48.png")),
    (64, include_bytes!("../../../packaging/haru-64.png")),
    (128, include_bytes!("../../../packaging/haru-128.png")),
    (256, include_bytes!("../../../packaging/haru-256.png")),
];

#[must_use]
pub fn entry() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    if cfg!(target_os = "macos") {
        return Some(home.join("Applications/haru.app"));
    }
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    Some(base.join("applications/haru.desktop"))
}

#[must_use]
pub fn installed() -> bool {
    entry().is_some_and(|path| path.exists())
}

pub fn install() -> Result<PathBuf, String> {
    let path = entry().ok_or("no home directory to install into")?;
    let binary = std::env::current_exe().map_err(|error| format!("{error}"))?;

    if cfg!(target_os = "macos") {
        bundle(&path, &binary)?;
    } else {
        launcher(&path, &binary)?;
    }
    Ok(path)
}

pub fn uninstall() -> Result<(), String> {
    let Some(path) = entry() else {
        return Ok(());
    };
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        std::fs::remove_dir_all(&path)
    } else {
        std::fs::remove_file(&path)
    }
    .map_err(|error| format!("{}: {error}", path.display()))
}

fn launcher(path: &Path, binary: &Path) -> Result<(), String> {
    let parent = path.parent().ok_or("no applications directory")?;
    std::fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    std::fs::write(path, desktop_text(DESKTOP, binary))
        .map_err(|error| format!("{}: {error}", path.display()))?;

    if let Some(icons) = icon_root() {
        let scalable = icons.join("scalable/apps");
        if std::fs::create_dir_all(&scalable).is_ok() {
            let _ = std::fs::write(scalable.join("haru.svg"), SVG);
        }
        for (size, bytes) in PNGS {
            let dir = icons.join(format!("{size}x{size}/apps"));
            if std::fs::create_dir_all(&dir).is_ok() {
                let _ = std::fs::write(dir.join("haru.png"), bytes);
            }
        }
    }
    refresh(parent);
    Ok(())
}

fn icon_root() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"));
    Some(base.join("icons/hicolor"))
}

fn refresh(applications: &Path) {
    let _ = std::process::Command::new("update-desktop-database")
        .arg(applications)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
}

#[must_use]
pub fn desktop_text(template: &str, binary: &Path) -> String {
    let program = binary.to_string_lossy();
    template
        .lines()
        .map(|line| match line.split_once('=') {
            Some(("Exec", rest)) => {
                let arguments = rest.strip_prefix("haru").unwrap_or("").trim();
                if arguments.is_empty() {
                    format!("Exec={program}")
                } else {
                    format!("Exec={program} {arguments}")
                }
            }
            Some(("TryExec", _)) => format!("TryExec={program}"),
            _ => line.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

fn bundle(path: &Path, binary: &Path) -> Result<(), String> {
    let contents = path.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    for dir in [&macos, &resources] {
        std::fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    }

    std::fs::write(contents.join("Info.plist"), info_plist())
        .map_err(|error| format!("{error}"))?;

    let inside = macos.join("haru");
    let _ = std::fs::remove_file(&inside);
    std::fs::copy(binary, &inside).map_err(|error| format!("{}: {error}", inside.display()))?;

    icon(&resources);
    Ok(())
}

// A single-size icns looks soft in the Dock, so build the whole iconset the
// way the release does. Best effort: an app without an icon still runs.
fn icon(resources: &Path) {
    let png = resources.join("haru.png");
    if std::fs::write(&png, LARGE).is_err() {
        return;
    }
    let iconset = resources.join("haru.iconset");
    let _ = std::fs::remove_dir_all(&iconset);
    if std::fs::create_dir_all(&iconset).is_err() {
        let _ = std::fs::remove_file(&png);
        return;
    }

    for size in [16_u32, 32, 128, 256, 512] {
        for (pixels, name) in [
            (size, format!("icon_{size}x{size}.png")),
            (size * 2, format!("icon_{size}x{size}@2x.png")),
        ] {
            let _ = std::process::Command::new("sips")
                .args(["-z", &pixels.to_string(), &pixels.to_string()])
                .arg(&png)
                .arg("--out")
                .arg(iconset.join(name))
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    }

    let _ = std::process::Command::new("iconutil")
        .args(["-c", "icns"])
        .arg(&iconset)
        .arg("-o")
        .arg(resources.join("haru.icns"))
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    let _ = std::fs::remove_dir_all(&iconset);
    let _ = std::fs::remove_file(&png);
}

#[must_use]
pub fn info_plist() -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         \t<key>CFBundleName</key>\n\t<string>haru</string>\n\
         \t<key>CFBundleDisplayName</key>\n\t<string>haru</string>\n\
         \t<key>CFBundleIdentifier</key>\n\t<string>dev.unhingedsoftware.haru</string>\n\
         \t<key>CFBundleVersion</key>\n\t<string>{version}</string>\n\
         \t<key>CFBundleShortVersionString</key>\n\t<string>{version}</string>\n\
         \t<key>CFBundleExecutable</key>\n\t<string>haru</string>\n\
         \t<key>CFBundleIconFile</key>\n\t<string>haru</string>\n\
         \t<key>CFBundlePackageType</key>\n\t<string>APPL</string>\n\
         \t<key>NSHighResolutionCapable</key>\n\t<true/>\n\
         </dict>\n</plist>\n",
        version = env!("CARGO_PKG_VERSION")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_launcher_points_at_the_binary_that_installed_it() {
        let text = desktop_text(DESKTOP, Path::new("/opt/haru/bin/haru"));
        assert!(text.contains("Exec=/opt/haru/bin/haru\n"));
        assert!(text.contains("TryExec=/opt/haru/bin/haru"));
        assert!(text.contains("Exec=/opt/haru/bin/haru workshop"));
        assert!(text.contains("Icon=haru"));
    }

    #[test]
    fn the_bundle_names_the_executable_it_carries() {
        let text = info_plist();
        assert!(text.contains("<key>CFBundleExecutable</key>\n\t<string>haru</string>"));
        assert!(text.contains("dev.unhingedsoftware.haru"));
        assert!(text.contains("<key>NSHighResolutionCapable</key>"));
    }

    #[test]
    fn the_entry_lands_where_this_os_looks_for_applications() {
        let Some(path) = entry() else { return };
        let text = path.to_string_lossy();
        if cfg!(target_os = "macos") {
            assert!(text.ends_with("Applications/haru.app"), "{text}");
        } else {
            assert!(text.ends_with("applications/haru.desktop"), "{text}");
        }
    }
}

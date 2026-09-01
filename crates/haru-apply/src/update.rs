use std::path::{Path, PathBuf};

use crate::install::{self, Build};

const HARU: &str = "UnhingedSoftware/haru";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Checking,
    Fetching { done: u64, size: u64 },
    Ready(String),
    Nothing,
    Failed(String),
}

#[must_use]
pub fn system() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

#[must_use]
pub fn haru_asset() -> String {
    format!("haru-{}-{}", system(), std::env::consts::ARCH)
}

#[must_use]
pub fn newer(offered: &str, running: &str) -> bool {
    let parts = |text: &str| -> Vec<u64> {
        text.trim()
            .trim_start_matches('v')
            .split(['.', '-', '+'])
            .map(|piece| piece.parse::<u64>().unwrap_or(0))
            .take(3)
            .collect()
    };
    let (offered, running) = (parts(offered), parts(running));
    if offered.iter().all(|piece| *piece == 0) {
        return false;
    }
    offered > running
}

pub fn haru_update(betas: bool) -> Result<Option<Build>, String> {
    let build = install::newest_from(HARU, &haru_asset(), betas)?;
    Ok(newer(&build.tag, env!("CARGO_PKG_VERSION")).then_some(build))
}

pub fn renderer_update(binary: &Path, web: install::Web, betas: bool) -> Result<Option<Build>, String> {
    let build = if betas {
        install::latest_including_betas(web)?
    } else {
        install::latest(web)?
    };
    let running = install::version_of(binary).unwrap_or_default();
    Ok(newer(&build.tag, &running).then_some(build))
}

pub fn take_renderer(build: &Build, progress: &mut dyn FnMut(u64, u64)) -> Result<PathBuf, String> {
    let target = install::installed()
        .or_else(install::destination)
        .ok_or("nowhere to install the renderer")?;
    install::fetch(build, &target, progress)
}

pub fn take_haru(build: &Build, progress: &mut dyn FnMut(u64, u64)) -> Result<PathBuf, String> {
    install::fetch(build, &running_binary()?, progress)
}

pub fn relaunch_self() -> Result<(), String> {
    let running = running_binary()?;
    let arguments: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    std::process::Command::new(running)
        .args(arguments)
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("{error}"))
}

pub fn running_binary() -> Result<PathBuf, String> {
    let running = std::env::current_exe().map_err(|error| format!("{error}"))?;
    Ok(still_there(&running).unwrap_or(running))
}

fn still_there(running: &Path) -> Option<PathBuf> {
    if running.exists() {
        return Some(running.to_path_buf());
    }
    let replaced = replaced_path(running)?;
    replaced.exists().then_some(replaced)
}

fn replaced_path(running: &Path) -> Option<PathBuf> {
    let text = running.to_string_lossy();
    let cleaned = text.strip_suffix(" (deleted)")?;
    (!cleaned.is_empty()).then(|| PathBuf::from(cleaned))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_update_asks_for_the_variant_it_was_given() {
        for web in [install::Web::WebKit, install::Web::Cef] {
            let asset = web.asset();
            if cfg!(target_os = "macos") {
                assert!(asset.starts_with("kirie-macos-"), "{asset}");
            } else {
                assert!(asset.contains(web.key().replace("webkit", "webview").as_str()), "{asset}");
            }
        }
    }

    #[test]
    fn a_replaced_binary_is_found_where_it_was_put_back() {
        let dir = std::env::temp_dir().join("haru-update-replaced");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let real = dir.join("haru");
        let _ = std::fs::write(&real, b"binary");
        let deleted = dir.join("haru (deleted)");

        assert_eq!(still_there(&deleted), Some(real.clone()));
        assert_eq!(still_there(&real), Some(real));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_binary_that_is_really_gone_is_not_invented() {
        let missing = std::env::temp_dir().join("haru-update-missing/haru (deleted)");
        assert_eq!(still_there(&missing), None);
    }

    #[test]
    fn a_path_that_ends_in_deleted_by_name_is_left_alone() {
        let plain = std::path::Path::new("/tmp/haru");
        assert_eq!(replaced_path(plain), None);
    }

    #[test]
    fn a_higher_version_is_offered() {
        assert!(newer("v0.7.0", "0.6.0"));
        assert!(newer("0.6.1", "0.6.0"));
        assert!(newer("v1.0.0", "0.9.9"));
    }

    #[test]
    fn the_same_or_older_is_not() {
        assert!(!newer("v0.6.0", "0.6.0"));
        assert!(!newer("v0.5.9", "0.6.0"));
        assert!(!newer("v0.6.0", "0.6.1"));
    }

    #[test]
    fn a_tag_that_makes_no_sense_is_never_newer() {
        assert!(!newer("nightly", "0.6.0"));
        assert!(!newer("", "0.6.0"));
    }

    #[test]
    fn a_missing_version_still_lets_an_update_through() {
        assert!(newer("v0.6.0", ""));
    }

    #[test]
    fn the_asset_names_this_platform() {
        let asset = haru_asset();
        assert!(asset.starts_with("haru-"), "{asset}");
        assert!(asset.contains(std::env::consts::ARCH), "{asset}");
        if cfg!(target_os = "macos") {
            assert!(asset.contains("macos"), "{asset}");
        }
    }
}

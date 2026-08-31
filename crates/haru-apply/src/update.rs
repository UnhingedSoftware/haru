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

pub fn haru_update() -> Result<Option<Build>, String> {
    let build = install::latest_from(HARU, &haru_asset())?;
    Ok(newer(&build.tag, env!("CARGO_PKG_VERSION")).then_some(build))
}

pub fn renderer_update(binary: &Path) -> Result<Option<Build>, String> {
    let build = install::latest(install::Web::suggested())?;
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
    let running = std::env::current_exe().map_err(|error| format!("{error}"))?;
    install::fetch(build, &running, progress)
}

#[cfg(test)]
mod tests {
    use super::*;

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

use std::path::PathBuf;

pub const WALLPAPER_ENGINE_APP: u32 = 431960;

#[must_use]
pub fn assets_home() -> Option<PathBuf> {
    if let Some(set) = std::env::var_os("KIRIE_WE_ASSETS") {
        let dir = PathBuf::from(set);
        return dir.is_dir().then_some(dir);
    }
    data_home().map(|base| base.join("haru/wallpaper-engine/assets"))
}

#[must_use]
pub fn install_root() -> Option<PathBuf> {
    data_home().map(|base| base.join("haru/wallpaper-engine"))
}

fn data_home() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        return std::env::var_os("HOME")
            .map(|home| PathBuf::from(home).join("Library/Application Support"));
    }
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
}

#[must_use]
pub fn steam_assets() -> Option<PathBuf> {
    crate::library::steam_roots()
        .into_iter()
        .map(|root| root.join("steamapps/common/wallpaper_engine/assets"))
        .find(|dir| looks_complete(dir))
}

#[must_use]
pub fn looks_complete(assets: &std::path::Path) -> bool {
    assets.join("shaders").is_dir() && assets.join("materials").is_dir()
}

#[must_use]
pub fn found() -> Option<PathBuf> {
    steam_assets().or_else(|| assets_home().filter(|dir| looks_complete(dir)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_are_only_complete_with_shaders_and_materials() {
        let dir = std::env::temp_dir().join("haru-engine-assets");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(dir.join("shaders"));
        assert!(!looks_complete(&dir));
        let _ = std::fs::create_dir_all(dir.join("materials"));
        assert!(looks_complete(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_managed_install_lives_under_this_platforms_data_home() {
        let Some(root) = install_root() else { return };
        let text = root.to_string_lossy().into_owned();
        assert!(text.ends_with("haru/wallpaper-engine"));
        if cfg!(target_os = "macos") {
            assert!(text.contains("Library/Application Support"));
        }
    }
}

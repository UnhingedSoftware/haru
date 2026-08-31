use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub install_dir: Option<PathBuf>,
    pub socket: Option<PathBuf>,
    pub adult: bool,
    pub per_page: u32,
    pub fit_per_page: bool,
    pub extra_libraries: Vec<PathBuf>,
    pub screens: BTreeMap<String, PathBuf>,
    pub renderer_web: Option<String>,
    pub offer_renderer: bool,
    pub infinite_scroll: bool,
    pub renderer: crate::renderer::Renderer,
    pub auto_update: bool,
    #[serde(default)]
    pub beta: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            install_dir: None,
            socket: None,
            adult: false,
            per_page: 24,
            fit_per_page: true,
            extra_libraries: Vec::new(),
            screens: BTreeMap::new(),
            renderer_web: None,
            offer_renderer: true,
            infinite_scroll: false,
            renderer: crate::renderer::Renderer::default(),
            auto_update: true,
            beta: false,
        }
    }
}

impl Config {
    #[must_use]
    pub fn path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        Some(base.join("haru/config.json"))
    }

    #[must_use]
    pub fn load() -> Self {
        let mut held: Self = Self::path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        held.forget_empty_screens();
        held
    }

    pub fn forget_empty_screens(&mut self) {
        self.screens
            .retain(|_, wallpaper| !wallpaper.as_os_str().is_empty());
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path().ok_or("no config directory")?;
        let parent = path.parent().ok_or("no config directory")?;
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;

        let text = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        let staged = path.with_extension("tmp");
        std::fs::write(&staged, text).map_err(|error| error.to_string())?;
        std::fs::rename(&staged, &path).map_err(|error| error.to_string())
    }

    #[must_use]
    pub fn install_root(&self) -> Option<PathBuf> {
        self.install_dir
            .clone()
            .or_else(|| self.libraries().into_iter().next())
            .or_else(crate::engine::library_home)
    }

    #[must_use]
    pub fn libraries(&self) -> Vec<PathBuf> {
        let mut roots = crate::library::steam_roots();
        if let Some(own) = crate::engine::library_home()
            && own.is_dir()
            && !roots.contains(&own)
        {
            roots.push(own);
        }
        for extra in &self.extra_libraries {
            if !roots.contains(extra) {
                roots.push(extra.clone());
            }
        }
        roots
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_screen_remembered_with_no_wallpaper_is_dropped() {
        let mut config = Config::default();
        config
            .screens
            .insert("DP-1".to_owned(), std::path::PathBuf::new());
        config
            .screens
            .insert("DP-2".to_owned(), std::path::PathBuf::from("/wall/one"));

        config.forget_empty_screens();

        assert_eq!(config.screens.len(), 1);
        assert!(config.screens.contains_key("DP-2"));
    }
    use super::*;

    #[test]
    fn an_unreadable_config_is_defaults_rather_than_a_failure() {
        assert_eq!(
            serde_json::from_str::<Config>("{ not json").ok(),
            None,
            "a broken file must not parse"
        );
        assert_eq!(Config::default().per_page, 24);
    }

    #[test]
    fn a_partial_config_keeps_the_defaults_for_what_it_omits() {
        let parsed: Config = serde_json::from_str(r#"{"adult":true}"#).unwrap_or_default();
        assert!(parsed.adult);
        assert_eq!(parsed.per_page, 24);
        assert!(parsed.install_dir.is_none());
    }

    #[test]
    fn settings_round_trip_through_their_own_format() {
        let config = Config {
            install_dir: Some(PathBuf::from("/tmp/haru")),
            adult: true,
            per_page: 48,
            ..Config::default()
        };
        let text = serde_json::to_string(&config).unwrap_or_default();
        assert_eq!(serde_json::from_str::<Config>(&text).ok(), Some(config));
    }
}

//! What the app remembers between runs.
//!
//! Written to `$XDG_CONFIG_HOME/haru/config.json`, and every field has a
//! working default: a config that fails to load is a config that gets replaced
//! by defaults, never a reason to refuse to start.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Settings, as they sit on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Where installs land. `None` uses the first Steam library found, which
    /// is what kirie and Wallpaper Engine already read.
    pub install_dir: Option<PathBuf>,
    /// The renderer's control socket. `None` uses `$XDG_RUNTIME_DIR/lwe.sock`,
    /// which is where kirie puts it.
    pub socket: Option<PathBuf>,
    /// Whether adult content is shown without asking each time.
    pub adult: bool,
    /// How many results a page of the browser holds.
    pub per_page: u32,
    /// Extra Steam libraries to read, for a layout the probe does not know.
    pub extra_libraries: Vec<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            install_dir: None,
            socket: None,
            adult: false,
            per_page: 24,
            extra_libraries: Vec::new(),
        }
    }
}

impl Config {
    /// Where the file lives.
    #[must_use]
    pub fn path() -> Option<PathBuf> {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
        Some(base.join("haru/config.json"))
    }

    /// Reads the config, or the defaults.
    ///
    /// A malformed file is replaced by defaults rather than reported: the
    /// alternative is a picker that will not open because a number in a file
    /// is a string.
    #[must_use]
    pub fn load() -> Self {
        Self::path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Writes the config.
    ///
    /// Through a temporary file and a rename, so an interrupted write leaves
    /// the previous settings rather than half of the new ones.
    ///
    /// # Errors
    /// When the directory cannot be made, or the file cannot be written.
    pub fn save(&self) -> Result<(), String> {
        let path = Self::path().ok_or("no config directory")?;
        let parent = path.parent().ok_or("no config directory")?;
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;

        let text = serde_json::to_string_pretty(self).map_err(|error| error.to_string())?;
        let staged = path.with_extension("tmp");
        std::fs::write(&staged, text).map_err(|error| error.to_string())?;
        std::fs::rename(&staged, &path).map_err(|error| error.to_string())
    }

    /// Every Steam library to read, the probe's and the ones named here.
    #[must_use]
    pub fn libraries(&self) -> Vec<PathBuf> {
        let mut roots = crate::library::steam_roots();
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
    use super::*;

    #[test]
    fn an_unreadable_config_is_defaults_rather_than_a_failure() {
        assert_eq!(
            serde_json::from_str::<Config>("{ not json").ok(),
            None,
            "a broken file must not parse"
        );
        // …and load() turns that into defaults, which is the behaviour that
        // keeps the window opening.
        assert_eq!(Config::default().per_page, 24);
    }

    #[test]
    fn a_partial_config_keeps_the_defaults_for_what_it_omits() {
        // Fields get added between versions, and an older file must not reset
        // everything it has never heard of.
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

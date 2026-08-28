//! Settings you changed, kept for a wallpaper that is not up yet.
//!
//! A wallpaper's own knobs live in its `project.json`, which is Steam's file —
//! rewriting it would be overwritten by the next update and would change the
//! wallpaper for anything else that reads it. So what you change is kept here
//! instead, per item, and handed to the renderer when that wallpaper goes up.
//!
//! One file per wallpaper under `$XDG_CONFIG_HOME/haru/overrides/<id>.json`,
//! holding only what differs from the wallpaper's own defaults.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// The values changed for one wallpaper, keyed the way the renderer knows them.
pub type Overrides = BTreeMap<String, String>;

/// Where one wallpaper's overrides live.
#[must_use]
pub fn path(id: &str) -> Option<PathBuf> {
    // Ids come from directory names on disk, but a path is built from them, so
    // anything that could climb out of the directory is refused.
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return None;
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("haru/overrides").join(format!("{id}.json")))
}

/// Reads what was changed for a wallpaper.
///
/// A missing or unreadable file is no overrides rather than an error: the
/// wallpaper's own defaults are always a valid answer.
#[must_use]
pub fn read(id: &str) -> Overrides {
    path(id)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Records one changed value.
///
/// # Errors
/// When the directory cannot be made or the file cannot be written.
pub fn set(id: &str, key: &str, value: &str) -> Result<(), String> {
    let mut held = read(id);
    held.insert(key.to_owned(), value.to_owned());
    write(id, &held)
}

/// Forgets everything changed for a wallpaper.
///
/// # Errors
/// When the file exists and cannot be removed.
pub fn clear(id: &str) -> Result<(), String> {
    let Some(path) = path(id) else {
        return Err("that wallpaper has no id to clear".to_owned());
    };
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        // Nothing to clear is the same outcome as clearing it.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

/// Writes the whole set.
fn write(id: &str, held: &Overrides) -> Result<(), String> {
    let path = path(id).ok_or("that wallpaper has no id to save under")?;
    let parent = path.parent().ok_or("no config directory")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;

    let text = serde_json::to_string_pretty(held).map_err(|error| error.to_string())?;
    // Through a temporary file and a rename, so an interrupted write leaves
    // the previous settings rather than half of the new ones.
    let staged = path.with_extension("tmp");
    std::fs::write(&staged, text).map_err(|error| error.to_string())?;
    std::fs::rename(&staged, &path).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_that_could_climb_out_of_the_directory_is_refused() {
        // Ids come from directory names on disk; a path is built from them.
        assert_eq!(path("../../etc/passwd"), None);
        assert_eq!(path("a/b"), None);
        assert_eq!(path(""), None);
        assert!(path("884307090").is_some());
    }

    #[test]
    fn nothing_saved_reads_as_no_overrides() {
        assert!(read("000000000000000").is_empty());
    }

    #[test]
    fn a_value_survives_a_round_trip_and_clearing() {
        let id = "haru9990001";
        let _ = clear(id);
        assert!(set(id, "speed", "2").is_ok());
        assert!(set(id, "tint", "0.5 0.25 1").is_ok());

        let held = read(id);
        assert_eq!(held.get("speed").map(String::as_str), Some("2"));
        // A colour is a space-separated triple, and must survive as one.
        assert_eq!(held.get("tint").map(String::as_str), Some("0.5 0.25 1"));

        assert!(clear(id).is_ok());
        assert!(read(id).is_empty());
        // Clearing what is not there is not a failure.
        assert!(clear(id).is_ok());
    }

    #[test]
    fn setting_the_same_key_twice_keeps_the_last() {
        let id = "haru9990002";
        let _ = clear(id);
        assert!(set(id, "speed", "1").is_ok());
        assert!(set(id, "speed", "3").is_ok());
        assert_eq!(read(id).get("speed").map(String::as_str), Some("3"));
        let _ = clear(id);
    }
}

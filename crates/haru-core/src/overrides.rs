use std::collections::BTreeMap;
use std::path::PathBuf;

pub type Overrides = BTreeMap<String, String>;

#[must_use]
pub fn path(id: &str) -> Option<PathBuf> {
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("haru/overrides").join(format!("{id}.json")))
}

#[must_use]
pub fn read(id: &str) -> Overrides {
    path(id)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn set(id: &str, key: &str, value: &str) -> Result<(), String> {
    let mut held = read(id);
    held.insert(key.to_owned(), value.to_owned());
    write(id, &held)
}

pub fn clear(id: &str) -> Result<(), String> {
    let Some(path) = path(id) else {
        return Err("that wallpaper has no id to clear".to_owned());
    };
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

fn write(id: &str, held: &Overrides) -> Result<(), String> {
    let path = path(id).ok_or("that wallpaper has no id to save under")?;
    let parent = path.parent().ok_or("no config directory")?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;

    let text = serde_json::to_string_pretty(held).map_err(|error| error.to_string())?;
    let staged = path.with_extension("tmp");
    std::fs::write(&staged, text).map_err(|error| error.to_string())?;
    std::fs::rename(&staged, &path).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_that_could_climb_out_of_the_directory_is_refused() {
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
        assert_eq!(held.get("tint").map(String::as_str), Some("0.5 0.25 1"));

        assert!(clear(id).is_ok());
        assert!(read(id).is_empty());
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

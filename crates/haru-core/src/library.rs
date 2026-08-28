//! What is already installed.
//!
//! An installed wallpaper is a directory with a `project.json` in it, and that
//! file is the whole of what a picker needs: a title, a kind and the name of a
//! preview image beside it. Nothing here decodes a scene — a picker shows
//! artwork and hands the directory to whatever renders it.
//!
//! Steam's own layout is the one to read, because it is where Steam, Wallpaper
//! Engine, kirie and haru all agree an item lives:
//! `<library>/steamapps/workshop/content/431960/<id>`.

use std::path::{Path, PathBuf};

/// Wallpaper Engine's Workshop content directory, under any Steam library.
const CONTENT: &str = "steamapps/workshop/content/431960";

/// One wallpaper on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// The Workshop id, which is the directory's name.
    pub id: String,
    /// Where it lives.
    pub dir: PathBuf,
    /// Its title, or its id when `project.json` gives none.
    pub title: String,
    /// `scene`, `video`, `web`, `application` — whatever the project says.
    pub kind: String,
    /// The preview image beside the project file, when there is one.
    pub preview: Option<PathBuf>,
    /// How much space it takes.
    pub size: u64,
    /// When it arrived, from the directory's own timestamp.
    ///
    /// Steam does not record a subscription date anywhere a reader can get at,
    /// and the directory's mtime is what "newest first" has to mean.
    pub installed: std::time::SystemTime,
}

/// Every Steam library on this machine.
///
/// The four well-known roots, plus whatever `libraryfolders.vdf` in each of
/// them points at — a wallpaper on a second drive is invisible otherwise, and
/// second drives are where large libraries live.
#[must_use]
pub fn steam_roots() -> Vec<PathBuf> {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return Vec::new();
    };

    let mut roots: Vec<PathBuf> = [
        ".local/share/Steam",
        ".steam/steam",
        ".var/app/com.valvesoftware.Steam/.local/share/Steam",
        "snap/steam/common/.local/share/Steam",
    ]
    .iter()
    .map(|relative| home.join(relative))
    .filter(|root| root.is_dir())
    .collect();

    for index in 0..roots.len() {
        let Some(root) = roots.get(index).cloned() else {
            continue;
        };
        for extra in library_folders(&root) {
            if extra.is_dir() && !roots.contains(&extra) {
                roots.push(extra);
            }
        }
    }
    roots
}

/// The libraries `libraryfolders.vdf` names.
///
/// Not a VDF parser: the file is read for the second quoted string on any line
/// whose first is `path` or a bare number, which covers both the modern
/// per-library block and the old `"1" "/mnt/games"` form. A file it cannot
/// make sense of yields nothing rather than an error, because a missing second
/// library is a smaller problem than refusing to start.
fn library_folders(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for name in ["steamapps/libraryfolders.vdf", "config/libraryfolders.vdf"] {
        let Ok(text) = std::fs::read_to_string(root.join(name)) else {
            continue;
        };
        for line in text.lines() {
            let mut quoted = line.split('"').skip(1).step_by(2);
            let (Some(key), Some(value)) = (quoted.next(), quoted.next()) else {
                continue;
            };
            if key == "path" || key.chars().all(|c| c.is_ascii_digit()) {
                found.push(PathBuf::from(value.replace("\\\\", "\\")));
            }
        }
    }
    found
}

/// Everything installed under these libraries, newest first.
#[must_use]
pub fn scan(roots: &[PathBuf]) -> Vec<Installed> {
    let mut items: Vec<Installed> = Vec::new();

    for root in roots {
        let Ok(entries) = std::fs::read_dir(root.join(CONTENT)) else {
            continue;
        };
        for entry in entries.flatten() {
            let dir = entry.path();
            if !dir.is_dir() {
                continue;
            }
            // The first library wins: the same id under two libraries is one
            // wallpaper Steam happened to write twice.
            let id = dir
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            if id.is_empty() || items.iter().any(|item| item.id == id) {
                continue;
            }
            if let Some(item) = read(&dir, id) {
                items.push(item);
            }
        }
    }

    // Newest first, which is what someone looking for what they just
    // subscribed to wants.
    items.sort_by_key(|item| std::cmp::Reverse(item.installed));
    items
}

/// Reads one item directory, if it holds a wallpaper at all.
fn read(dir: &Path, id: String) -> Option<Installed> {
    // Unsubscribing leaves the directory behind when something else has
    // written inside it — a renderer's cache, usually — so a directory with no
    // project file is a ghost rather than a wallpaper.
    let project = std::fs::read_to_string(dir.join("project.json")).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&project).ok()?;

    let title = parsed
        .get("title")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .map_or_else(|| id.clone(), crate::plain_text);

    let kind = parsed
        .get("type")
        .and_then(serde_json::Value::as_str)
        .map(str::to_lowercase)
        .unwrap_or_else(|| "scene".to_owned());

    let preview = parsed
        .get("preview")
        .and_then(serde_json::Value::as_str)
        .map(|name| dir.join(name))
        .filter(|path| path.is_file());

    let installed = std::fs::metadata(dir)
        .and_then(|meta| meta.modified())
        .unwrap_or(std::time::UNIX_EPOCH);

    Some(Installed {
        id,
        title,
        kind,
        preview,
        size: directory_size(dir),
        installed,
        dir: dir.to_owned(),
    })
}

/// How much space a directory takes, one level deep plus its subdirectories.
///
/// Walked rather than trusted to any index: nothing records an item's size on
/// disk, and the number is what a library view exists to show.
fn directory_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.metadata() {
            Ok(meta) if meta.is_dir() => directory_size(&entry.path()),
            Ok(meta) => meta.len(),
            Err(_) => 0,
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway directory that cleans itself up.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("haru-library-{name}"));
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::create_dir_all(&dir);
            Self(dir)
        }

        fn item(&self, id: &str, project: &str) -> PathBuf {
            let dir = self.0.join("steamapps/workshop/content/431960").join(id);
            let _ = std::fs::create_dir_all(&dir);
            let _ = std::fs::write(dir.join("project.json"), project);
            dir
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn an_item_reads_its_title_kind_and_preview() {
        let scratch = Scratch::new("reads");
        let dir = scratch.item(
            "123",
            r#"{"title":"Neon &amp; rain","type":"Video","preview":"preview.gif"}"#,
        );
        let _ = std::fs::write(dir.join("preview.gif"), [0_u8; 8]);

        let found = scan(std::slice::from_ref(&scratch.0));
        assert_eq!(found.len(), 1);
        let Some(item) = found.first() else { return };
        assert_eq!(item.id, "123");
        // Titles carry the markup their authors typed.
        assert_eq!(item.title, "Neon & rain");
        assert_eq!(item.kind, "video");
        assert!(item.preview.is_some());
        assert!(item.size >= 8);
    }

    #[test]
    fn a_directory_with_no_project_is_not_a_wallpaper() {
        // Unsubscribing leaves these behind whenever a renderer has written a
        // cache inside the item, and they would otherwise show as untitled
        // wallpapers that cannot be applied.
        let scratch = Scratch::new("ghost");
        let dir = scratch.0.join("steamapps/workshop/content/431960/999/.cache");
        let _ = std::fs::create_dir_all(&dir);

        assert!(scan(std::slice::from_ref(&scratch.0)).is_empty());
    }

    #[test]
    fn a_missing_preview_file_is_not_reported_as_one() {
        // project.json names a preview that unsubscribing removed; drawing it
        // would be a broken image in the grid.
        let scratch = Scratch::new("preview");
        scratch.item("5", r#"{"title":"Gone","preview":"preview.jpg"}"#);
        let found = scan(std::slice::from_ref(&scratch.0));
        assert_eq!(found.first().and_then(|item| item.preview.clone()), None);
    }

    #[test]
    fn a_project_that_is_not_json_is_skipped_rather_than_fatal() {
        let scratch = Scratch::new("garbage");
        scratch.item("7", "not json at all");
        assert!(scan(std::slice::from_ref(&scratch.0)).is_empty());
    }

    #[test]
    fn library_folders_reads_both_forms() {
        let scratch = Scratch::new("folders");
        let steamapps = scratch.0.join("steamapps");
        let _ = std::fs::create_dir_all(&steamapps);
        let _ = std::fs::write(
            steamapps.join("libraryfolders.vdf"),
            "\"libraryfolders\"\n{\n\t\"0\"\t\"/mnt/games\"\n\t{\n\t\t\"path\"\t\"/mnt/second\"\n\t}\n}",
        );

        let found = library_folders(&scratch.0);
        assert!(found.contains(&PathBuf::from("/mnt/games")), "{found:?}");
        assert!(found.contains(&PathBuf::from("/mnt/second")), "{found:?}");
    }
}

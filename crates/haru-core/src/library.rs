use std::path::{Path, PathBuf};

const CONTENT: &str = "steamapps/workshop/content/431960";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    pub id: String,
    pub dir: PathBuf,
    pub title: String,
    pub kind: String,
    pub preview: Option<PathBuf>,
    pub size: u64,
    pub installed: std::time::SystemTime,
}

#[must_use]
pub fn home_relative_roots() -> &'static [&'static str] {
    #[cfg(target_os = "macos")]
    {
        &["Library/Application Support/Steam"]
    }
    #[cfg(not(target_os = "macos"))]
    {
        &[
            ".local/share/Steam",
            ".steam/steam",
            ".var/app/com.valvesoftware.Steam/.local/share/Steam",
            "snap/steam/common/.local/share/Steam",
        ]
    }
}

fn windows_roots() -> Vec<PathBuf> {
    ["ProgramFiles(x86)", "ProgramFiles", "ProgramW6432"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|base| PathBuf::from(base).join("Steam"))
        .collect()
}

#[must_use]
pub fn steam_roots() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);

    let mut roots: Vec<PathBuf> = home
        .iter()
        .flat_map(|home| {
            home_relative_roots()
                .iter()
                .map(|relative| home.join(relative))
        })
        .chain(windows_roots())
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

#[must_use]
pub fn scan(roots: &[PathBuf]) -> Vec<Installed> {
    scan_dirs(&content_dirs(roots))
}

/// Everything the library shows: the Workshop items under `roots`, and the
/// user's own pictures and videos (see `crate::own`).
#[must_use]
pub fn scan_all(roots: &[PathBuf]) -> Vec<Installed> {
    scan_dirs(&all_dirs(roots))
}

/// The item directories under `roots` that are not among `known`, which is
/// what `scan_all` of the same roots returned.
#[must_use]
pub fn unreadable(roots: &[PathBuf], known: &[Installed]) -> Vec<String> {
    unreadable_dirs(&all_dirs(roots), known)
}

fn content_dirs(roots: &[PathBuf]) -> Vec<PathBuf> {
    roots.iter().map(|root| root.join(CONTENT)).collect()
}

fn all_dirs(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs = content_dirs(roots);
    dirs.extend(crate::own::home());
    dirs
}

/// The items directly inside each of `dirs`, newest first. An id seen in an
/// earlier directory wins over the same id in a later one.
#[must_use]
pub fn scan_dirs(dirs: &[PathBuf]) -> Vec<Installed> {
    let mut items: Vec<Installed> = Vec::new();

    for parent in dirs {
        for (id, dir) in item_dirs(parent) {
            if items.iter().any(|item| item.id == id) {
                continue;
            }
            if let Some(item) = read(&dir, id) {
                items.push(item);
            }
        }
    }

    items.sort_by_key(|item| std::cmp::Reverse(item.installed));
    items
}

fn unreadable_dirs(dirs: &[PathBuf], known: &[Installed]) -> Vec<String> {
    let mut broken: Vec<String> = Vec::new();

    for parent in dirs {
        for (id, _) in item_dirs(parent) {
            if known.iter().any(|item| item.id == id) || broken.contains(&id) {
                continue;
            }
            broken.push(id);
        }
    }

    broken.sort();
    broken
}

/// The folders directly inside `parent`, with their names. Names starting
/// with a dot are skipped: that is an item still being added.
fn item_dirs(parent: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|dir| dir.is_dir())
        .filter_map(|dir| {
            let id = dir.file_name()?.to_string_lossy().into_owned();
            (!id.is_empty() && !id.starts_with('.')).then_some((id, dir))
        })
        .collect()
}

fn read(dir: &Path, id: String) -> Option<Installed> {
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

    // The name is the author's, so it is only followed when it stays inside
    // the item: an absolute path or a `..` would point the picture decoder at
    // any file on the machine.
    let preview = parsed
        .get("preview")
        .and_then(serde_json::Value::as_str)
        .filter(|name| {
            Path::new(name).components().all(|part| {
                matches!(
                    part,
                    std::path::Component::Normal(_) | std::path::Component::CurDir
                )
            })
        })
        .and_then(|name| inside(dir, &dir.join(name)));

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

/// `path` if it is a file that, with symlinks followed, still lies inside
/// `dir`: an item can ship a link that points anywhere.
fn inside(dir: &Path, path: &Path) -> Option<PathBuf> {
    let root = dir.canonicalize().ok()?;
    let target = path.canonicalize().ok()?;
    (target.starts_with(&root) && target.is_file()).then(|| path.to_owned())
}

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

    #[test]
    fn the_steam_root_matches_where_this_os_keeps_it() {
        let roots = home_relative_roots();
        assert!(!roots.is_empty());
        if cfg!(target_os = "macos") {
            assert_eq!(roots, ["Library/Application Support/Steam"]);
        } else {
            assert!(roots.contains(&".local/share/Steam"));
        }
    }

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
        assert_eq!(item.title, "Neon & rain");
        assert_eq!(item.kind, "video");
        assert!(item.preview.is_some());
        assert!(item.size >= 8);
    }

    #[test]
    fn a_directory_with_no_project_is_not_a_wallpaper() {
        let scratch = Scratch::new("ghost");
        let dir = scratch
            .0
            .join("steamapps/workshop/content/431960/999/.cache");
        let _ = std::fs::create_dir_all(&dir);

        assert!(scan(std::slice::from_ref(&scratch.0)).is_empty());
    }

    #[test]
    fn a_missing_preview_file_is_not_reported_as_one() {
        let scratch = Scratch::new("preview");
        scratch.item("5", r#"{"title":"Gone","preview":"preview.jpg"}"#);
        let found = scan(std::slice::from_ref(&scratch.0));
        assert_eq!(found.first().and_then(|item| item.preview.clone()), None);
    }

    #[test]
    fn a_preview_outside_the_item_is_not_followed() {
        let scratch = Scratch::new("escape");
        let outside = scratch.0.join("secret.png");
        let _ = std::fs::write(&outside, [0_u8; 8]);
        let absolute = format!(
            r#"{{"title":"A","preview":{:?}}}"#,
            outside.to_string_lossy()
        );
        scratch.item("1", &absolute);
        scratch.item(
            "2",
            r#"{"title":"B","preview":"../../../../../secret.png"}"#,
        );
        let found = scan(std::slice::from_ref(&scratch.0));
        assert_eq!(found.len(), 2);
        assert!(found.iter().all(|item| item.preview.is_none()), "{found:?}");
    }

    #[test]
    fn an_item_that_cannot_be_read_is_reported_by_id() {
        let scratch = Scratch::new("broken");
        scratch.item("11", r#"{"title":"Fine","type":"scene"}"#);
        scratch.item("22", "not json at all");

        let roots = std::slice::from_ref(&scratch.0);
        let found = scan(roots);
        assert_eq!(found.len(), 1);
        assert_eq!(unreadable(roots, &found), vec!["22".to_owned()]);
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

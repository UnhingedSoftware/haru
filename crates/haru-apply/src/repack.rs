//! Turning downloaded Wallpaper Engine items into kirie packages.
//!
//! Each item is handed to `kirie convert`, which writes a `.kpk` holding the
//! item's files as they are. An item haru fetched itself is repacked and its
//! folder removed, so it lives on disk as one file. An item the Steam client
//! downloaded keeps its folder, since Steam fetches a missing one again, and
//! gets a copy in `haru_core::package::copies` instead (see `mirror`). If
//! kirie is missing or too old to convert, the folders work as before.

use std::path::{Path, PathBuf};

/// Repack the item in `item_dir` into haru's package folder, and remove the
/// folder once the package is there. The package's path, or why it could not
/// be made, in which case the folder is untouched.
pub fn repack(item_dir: &Path) -> Result<PathBuf, String> {
    let kirie = crate::install::installed().ok_or("kirie is not installed")?;
    let into =
        haru_core::package::home().ok_or("there is no data directory to keep packages in")?;
    std::fs::create_dir_all(&into).map_err(|e| format!("cannot create {}: {e}", into.display()))?;
    let name = item_dir
        .file_name()
        .ok_or("the item has no folder name")?
        .to_string_lossy()
        .into_owned();
    let package = into.join(format!("{name}.{}", haru_core::package::EXTENSION));
    convert(&kirie, item_dir, &package)?;
    // The package is complete, and the library prefers it to a folder with
    // the same id, so a folder that cannot be removed costs only space.
    let _ = std::fs::remove_dir_all(item_dir);
    Ok(package)
}

/// Run `kirie convert` on `item_dir`, writing `package`, and check haru can
/// read what it wrote.
fn convert(kirie: &Path, item_dir: &Path, package: &Path) -> Result<(), String> {
    let output = crate::child::quiet(kirie)
        .arg("convert")
        .arg(item_dir)
        .arg("-o")
        .arg(package)
        .output()
        .map_err(|e| format!("cannot run kirie: {e}"))?;
    if !output.status.success() {
        let said = String::from_utf8_lossy(&output.stderr);
        return Err(format!("kirie could not repack it: {}", said.trim()));
    }
    // Only a package haru can read stands in for the folder.
    if haru_core::package::Package::open(package).is_none() {
        let _ = std::fs::remove_file(package);
        return Err("kirie wrote a package haru cannot read".to_owned());
    }
    Ok(())
}

/// Copy every Wallpaper Engine item in the Workshop folders under `roots`
/// into a package, leaving the folders where they are, and remove copies
/// whose folder is gone. A copy is made again when its folder changes.
///
/// An item kirie cannot convert (an application wallpaper, or a kirie too old
/// to have `convert`) is noted beside the copies and not tried again until the
/// item or kirie changes. Returns whether any copy was made or removed.
#[must_use]
pub fn mirror(roots: &[PathBuf]) -> bool {
    let Some(kirie) = crate::install::installed() else {
        return false;
    };
    let Some(into) = haru_core::package::copies() else {
        return false;
    };
    let items = haru_core::library::workshop_items(roots);
    if items.is_empty() && !into.is_dir() {
        return false;
    }
    if std::fs::create_dir_all(&into).is_err() {
        return false;
    }
    let kirie_changed = modified(&kirie);
    let mut changed = false;

    for item in &items {
        let package = into.join(format!("{}.{}", item.id, haru_core::package::EXTENSION));
        if haru_core::library::copy_is_current(&package, &item.dir) {
            continue;
        }
        let refused = refusal(&into, &item.id);
        if haru_core::library::copy_is_current(&refused, &item.dir)
            && modified(&refused) >= kirie_changed
        {
            continue;
        }
        match convert(&kirie, &item.dir, &package) {
            Ok(()) => {
                let _ = std::fs::remove_file(&refused);
                changed = true;
            }
            Err(why) => {
                let _ = std::fs::write(&refused, why);
            }
        }
    }

    changed |= forget(&into, &items);
    changed
}

/// Where `mirror` notes why it could not copy item `id`.
fn refusal(into: &Path, id: &str) -> PathBuf {
    into.join(format!(".{id}.refused"))
}

fn modified(path: &Path) -> Option<std::time::SystemTime> {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
}

/// Remove the copies, and the notes, of items no longer among `items`.
fn forget(into: &Path, items: &[haru_core::Installed]) -> bool {
    let Ok(entries) = std::fs::read_dir(into) else {
        return false;
    };
    let mut removed = false;
    for path in entries.flatten().map(|entry| entry.path()) {
        let Some(name) = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
        else {
            continue;
        };
        let id =
            if let Some(stem) = name.strip_suffix(&format!(".{}", haru_core::package::EXTENSION)) {
                stem
            } else if let Some(id) = name
                .strip_prefix('.')
                .and_then(|rest| rest.strip_suffix(".refused"))
            {
                id
            } else {
                // kirie's half-written packages and the previews folder.
                continue;
            };
        if path.is_file()
            && !items.iter().any(|item| item.id == id)
            && std::fs::remove_file(&path).is_ok()
        {
            removed = true;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("haru-repack-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn item(id: &str, dir: &Path) -> haru_core::Installed {
        haru_core::Installed {
            id: id.to_owned(),
            dir: dir.join(id),
            title: id.to_owned(),
            kind: "scene".to_owned(),
            preview: None,
            size: 0,
            installed: std::time::UNIX_EPOCH,
            source: None,
        }
    }

    #[test]
    fn copies_and_notes_of_items_that_are_gone_are_removed()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = scratch("forget");
        std::fs::create_dir_all(dir.join(".previews"))?;
        for name in [
            "1.kpk",
            "2.kpk",
            ".2.refused",
            ".3.refused",
            ".4.1234.partial",
            "notes.txt",
        ] {
            std::fs::write(dir.join(name), b"")?;
        }

        assert!(forget(&dir, &[item("1", &dir)]));

        let mut left: Vec<String> = std::fs::read_dir(&dir)?
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, [".4.1234.partial", ".previews", "1.kpk", "notes.txt"]);
        assert!(!forget(&dir, &[item("1", &dir)]), "nothing more to remove");
        std::fs::remove_dir_all(&dir)?;
        Ok(())
    }
}

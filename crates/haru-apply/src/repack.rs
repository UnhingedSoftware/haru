//! Turning a downloaded Wallpaper Engine item into one kirie package.
//!
//! An item haru fetched itself (not through the Steam client, which would
//! fetch the folder again) is handed to `kirie convert`, which writes a
//! `.kpk` holding the item's files as they are. Once the package is written
//! the folder is removed, so the item lives on disk as one file. If kirie is
//! missing or too old to convert, the folder stays and works as before.

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

    let output = crate::child::quiet(&kirie)
        .arg("convert")
        .arg(item_dir)
        .arg("-o")
        .arg(&package)
        .output()
        .map_err(|e| format!("cannot run kirie: {e}"))?;
    if !output.status.success() {
        let said = String::from_utf8_lossy(&output.stderr);
        return Err(format!("kirie could not repack it: {}", said.trim()));
    }
    // Only a package haru can read stands in for the folder.
    if haru_core::package::Package::open(&package).is_none() {
        let _ = std::fs::remove_file(&package);
        return Err("kirie wrote a package haru cannot read".to_owned());
    }
    // The package is complete, and the library prefers it to a folder with
    // the same id, so a folder that cannot be removed costs only space.
    let _ = std::fs::remove_dir_all(item_dir);
    Ok(package)
}

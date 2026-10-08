//! Wallpapers made from the user's own pictures and videos.
//!
//! None of this touches Steam or Wallpaper Engine: kirie draws a picture or a
//! video with nothing but the file. Each one is kept as a folder shaped like a
//! Workshop item, a `project.json` beside the file, so the library, the
//! screens and kirie treat it exactly like one, and deleting it from the
//! library deletes haru's copy and never the original.

use std::path::{Path, PathBuf};

/// The pictures kirie can draw. Kept in step with kirie's `IMAGE_EXTS`, less
/// `tex`, which is Wallpaper Engine's own format and never a user's file.
pub const PICTURES: [&str; 6] = ["png", "jpg", "jpeg", "bmp", "gif", "webp"];
/// The videos kirie can play. Kept in step with kirie's `VIDEO_EXTS`.
pub const VIDEOS: [&str; 6] = ["mp4", "webm", "mkv", "avi", "mov", "m4v"];

/// Ids of these items start with this, which no Workshop id (all digits) can.
pub const ID_PREFIX: &str = "own-";

/// The longest side of the thumbnail made for a picture. The library decodes
/// previews whole, and a 6000-pixel photo is 96 MB of texture per tile.
const THUMBNAIL: u32 = 512;

/// Where haru keeps them.
#[must_use]
pub fn home() -> Option<PathBuf> {
    crate::data_home().map(|base| base.join("haru/own"))
}

/// Whether `id` names one of these rather than a Workshop item.
#[must_use]
pub fn is_own(id: &str) -> bool {
    id.starts_with(ID_PREFIX)
}

/// "image" or "video", as Wallpaper Engine's `type` spells it, for a file
/// kirie can draw.
#[must_use]
pub fn kind_of(file: &Path) -> Option<&'static str> {
    let ext = file.extension()?.to_string_lossy().to_ascii_lowercase();
    if PICTURES.contains(&ext.as_str()) {
        Some("image")
    } else if VIDEOS.contains(&ext.as_str()) {
        Some("video")
    } else {
        None
    }
}

/// Adds `file` to the library under `home` and returns the item's folder.
///
/// The file is hard-linked when it sits on the same disk and copied when it
/// does not, so the original can move or go without breaking the wallpaper.
/// Adding the same file again returns the folder it already has.
///
/// # Errors
///
/// When the file is not a picture or video kirie draws, or cannot be read
/// or copied.
pub fn add(file: &Path, home: &Path) -> Result<PathBuf, String> {
    let kind = kind_of(file).ok_or_else(|| {
        format!(
            "{} is not a picture or video haru can put up (pictures: {}; videos: {})",
            display_name(file),
            PICTURES.join(", "),
            VIDEOS.join(", ")
        )
    })?;
    let meta =
        std::fs::metadata(file).map_err(|error| format!("{}: {error}", display_name(file)))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", display_name(file)));
    }

    let source = file.canonicalize().unwrap_or_else(|_| file.to_owned());
    let id = format!("{ID_PREFIX}{:016x}", fingerprint(&source, meta.len()));
    let dir = home.join(&id);
    if dir.join("project.json").is_file() {
        return Ok(dir);
    }

    std::fs::create_dir_all(home).map_err(|error| format!("{}: {error}", home.display()))?;
    // Built beside the library and moved into place whole, so a scan never
    // sees an item with its project.json and no file, or a half-copied video.
    let staging = home.join(format!(".adding-{id}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&staging);
    let built = build(&source, kind, &staging);
    let placed = built.and_then(|()| {
        std::fs::rename(&staging, &dir).map_err(|error| format!("{}: {error}", dir.display()))
    });
    if placed.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    placed.map(|()| dir)
}

fn build(source: &Path, kind: &str, staging: &Path) -> Result<(), String> {
    std::fs::create_dir_all(staging).map_err(|error| format!("{}: {error}", staging.display()))?;

    // A fixed name rather than the user's: theirs can carry characters
    // another platform's file system refuses, and nothing reads it but kirie.
    let ext = source
        .extension()
        .map(|ext| ext.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let name = format!("wallpaper.{ext}");
    let target = staging.join(&name);
    if std::fs::hard_link(source, &target).is_err() {
        std::fs::copy(source, &target)
            .map_err(|error| format!("copying {}: {error}", display_name(source)))?;
    }

    let preview = (kind == "image" && thumbnail(&target, &staging.join("preview.jpg")))
        .then_some("preview.jpg");

    let title = source
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|stem| !stem.trim().is_empty())
        .unwrap_or_else(|| name.clone());
    let mut project = serde_json::json!({
        "title": title,
        "type": kind,
        "file": name,
        "source": source.to_string_lossy(),
    });
    if let Some(preview) = preview
        && let Some(map) = project.as_object_mut()
    {
        map.insert("preview".to_owned(), preview.into());
    }
    let text = serde_json::to_string_pretty(&project).map_err(|error| error.to_string())?;
    std::fs::write(staging.join("project.json"), text)
        .map_err(|error| format!("{}: {error}", staging.display()))
}

/// Writes a small JPEG of the picture at `from`. A picture that will not
/// decode gets no thumbnail; kirie may still draw it, and the tile shows
/// its kind instead.
fn thumbnail(from: &Path, to: &Path) -> bool {
    let Ok(picture) = image::open(from) else {
        return false;
    };
    picture
        .thumbnail(THUMBNAIL, THUMBNAIL)
        .into_rgb8()
        .save_with_format(to, image::ImageFormat::Jpeg)
        .is_ok()
}

/// FNV-1a over the path and size: stable across builds, unlike std's hasher,
/// so the same file keeps the same id after an update.
fn fingerprint(path: &Path, size: u64) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let path = path.to_string_lossy();
    for byte in path.as_bytes().iter().chain(&size.to_le_bytes()) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

fn display_name(file: &Path) -> String {
    file.file_name().map_or_else(
        || file.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("haru-own-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::create_dir_all(&dir);
            Self(dir)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn added(source: &Path, home: &Path) -> PathBuf {
        let result = add(source, home);
        assert!(result.is_ok(), "{result:?}");
        result.unwrap_or_default()
    }

    fn picture(at: &Path) {
        let mut canvas = image::RgbImage::new(1200, 600);
        for (x, _, pixel) in canvas.enumerate_pixels_mut() {
            *pixel = image::Rgb([(x % 256) as u8, 40, 90]);
        }
        let _ = canvas.save(at);
    }

    #[test]
    fn kinds_follow_the_extension_whatever_its_case() {
        assert_eq!(kind_of(Path::new("a/Beach.JPG")), Some("image"));
        assert_eq!(kind_of(Path::new("rain.webm")), Some("video"));
        assert_eq!(kind_of(Path::new("notes.txt")), None);
        assert_eq!(kind_of(Path::new("scene.tex")), None);
        assert_eq!(kind_of(Path::new("no-extension")), None);
    }

    #[test]
    fn a_picture_becomes_an_item_the_library_reads() {
        let scratch = Scratch::new("picture");
        let source = scratch.0.join("Sunset over the bay.png");
        picture(&source);
        let home = scratch.0.join("own");

        let dir = added(&source, &home);
        assert!(dir.join("wallpaper.png").is_file());
        assert!(dir.join("preview.jpg").is_file());

        let found = crate::library::scan_dirs(&[home]);
        assert_eq!(found.len(), 1, "{found:?}");
        let Some(item) = found.first() else { return };
        assert!(is_own(&item.id), "{}", item.id);
        assert_eq!(item.title, "Sunset over the bay");
        assert_eq!(item.kind, "image");
        assert!(item.preview.is_some());

        let thumb = image::image_dimensions(dir.join("preview.jpg")).ok();
        assert_eq!(thumb, Some((512, 256)));
    }

    #[test]
    fn a_video_is_kept_without_a_thumbnail() {
        let scratch = Scratch::new("video");
        let source = scratch.0.join("rain.MP4");
        let _ = std::fs::write(&source, b"not really a video");
        let home = scratch.0.join("own");

        let dir = added(&source, &home);
        assert!(dir.join("wallpaper.mp4").is_file());
        let project: serde_json::Value = std::fs::read_to_string(dir.join("project.json"))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        assert_eq!(project.get("type").and_then(|v| v.as_str()), Some("video"));
        assert_eq!(
            project.get("file").and_then(|v| v.as_str()),
            Some("wallpaper.mp4")
        );
        assert!(project.get("preview").is_none());
    }

    #[test]
    fn adding_a_file_twice_keeps_one_item() {
        let scratch = Scratch::new("twice");
        let source = scratch.0.join("a.webm");
        let _ = std::fs::write(&source, b"x");
        let home = scratch.0.join("own");

        let first = added(&source, &home);
        let second = added(&source, &home);
        assert_eq!(first, second);
        assert_eq!(crate::library::scan_dirs(&[home]).len(), 1);
    }

    #[test]
    fn the_copy_outlives_the_original() {
        let scratch = Scratch::new("outlives");
        let source = scratch.0.join("a.mkv");
        let _ = std::fs::write(&source, b"frames");
        let home = scratch.0.join("own");

        let dir = added(&source, &home);
        let _ = std::fs::remove_file(&source);
        assert_eq!(
            std::fs::read(dir.join("wallpaper.mkv")).ok().as_deref(),
            Some(&b"frames"[..])
        );
    }

    #[test]
    fn what_kirie_cannot_draw_is_refused_and_leaves_nothing() {
        let scratch = Scratch::new("refused");
        let source = scratch.0.join("readme.txt");
        let _ = std::fs::write(&source, b"x");
        let home = scratch.0.join("own");

        assert!(add(&source, &home).is_err());
        assert!(add(&scratch.0.join("missing.png"), &home).is_err());
        assert!(crate::library::scan_dirs(&[home]).is_empty());
    }

    #[test]
    fn own_ids_can_never_be_workshop_ids() {
        assert!(is_own("own-0123456789abcdef"));
        assert!(!is_own("1388331347"));
    }
}

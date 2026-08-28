//! Getting a renderer onto the machine.
//!
//! haru renders nothing itself. On Linux that job is kirie's, and a machine
//! that has never had it installed cannot put a wallpaper up at all — so this
//! fetches it: the latest release from kirie's own repository, checked against
//! the digest GitHub publishes beside it, into `~/.local/bin/kirie`.
//!
//! Two builds exist because web wallpapers need a browser and there are two
//! ways to have one. Which to take is not a preference so much as a fact about
//! the machine, which is why [`Web::suggested`] asks it rather than guessing —
//! but it is still an install, so nothing here starts without being asked.

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

/// Where the releases come from.
const REPOSITORY: &str = "UnhingedSoftware/kirie";

/// Enough of a browser to refuse an unexpected body.
///
/// The CEF build is 112 MB; anything past this is not a kirie release.
const MAX_BYTES: u64 = 256 * 1024 * 1024;

/// How long a request may take before it is a failure rather than a wait.
const DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

/// What to send as a user agent. GitHub's API refuses requests without one.
const AGENT: &str = concat!("haru/", env!("CARGO_PKG_VERSION"));

/// The webkit libraries kirie loads at run time, in the order it tries them.
///
/// Copied from kirie's own list rather than shortened to the current one: the
/// question here is exactly the one kirie will ask later, and answering a
/// different question would recommend a build that cannot show a web
/// wallpaper on this machine.
const WEBKIT: [&str; 3] = [
    // Current distros, libsoup-3.
    "libwebkit2gtk-4.1.so.0",
    // Old LTS, libsoup-2.4.
    "libwebkit2gtk-4.0.so.37",
    "libwebkit2gtk-4.0.so",
];

/// Which browser a kirie build carries for web wallpapers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Web {
    /// The system's WebKitGTK, loaded at run time. 32 MB.
    WebKit,
    /// A bundled Chromium. 112 MB, and needs nothing from the machine.
    Cef,
}

impl Web {
    /// The release asset this build is published as.
    #[must_use]
    pub const fn asset(self) -> &'static str {
        match self {
            Self::WebKit => "kirie-web-webview-linux-x86_64",
            Self::Cef => "kirie-web-cef-linux-x86_64",
        }
    }

    /// What it is called in a window.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::WebKit => "WebKit",
            Self::Cef => "Chromium (CEF)",
        }
    }

    /// How it is written in the config.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::WebKit => "webkit",
            Self::Cef => "cef",
        }
    }

    /// Reads back [`Web::key`].
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "webkit" => Some(Self::WebKit),
            "cef" => Some(Self::Cef),
            _ => None,
        }
    }

    /// The build worth offering on this machine.
    ///
    /// WebKit when the machine already has it, because it is a third of the
    /// size and gets its security updates from the distribution. CEF only when
    /// there is nothing to load — where the smaller build would install
    /// cleanly and then fail to show a web wallpaper, which is worse than a
    /// large download.
    #[must_use]
    pub fn suggested() -> Self {
        if webkit_present() {
            Self::WebKit
        } else {
            Self::Cef
        }
    }
}

/// Whether this machine has a webkit kirie could load.
///
/// The honest version of this question is `dlopen`, which is what kirie itself
/// will do — but that is an unsafe call, and this workspace does not make
/// them. So the loader's own search path is walked instead: the directories in
/// `LD_LIBRARY_PATH`, the ones `/etc/ld.so.conf.d` adds, and the standard
/// ones, looking for the sonames kirie asks for.
///
/// It can be wrong in one direction — a library in a place only the loader's
/// cache knows about reads as absent — which is the direction that matters,
/// because being wrong here suggests the larger download rather than a build
/// that cannot show a web wallpaper. And it is a suggestion: the choice is
/// confirmed before anything is fetched.
#[must_use]
pub fn webkit_present() -> bool {
    library_directories()
        .iter()
        .any(|directory| WEBKIT.iter().any(|soname| directory.join(soname).exists()))
}

/// Where the dynamic loader looks for a library, near enough.
fn library_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();

    if let Some(paths) = std::env::var_os("LD_LIBRARY_PATH") {
        directories.extend(std::env::split_paths(&paths));
    }

    // What a distribution adds: multiarch directories on Debian, /usr/lib32
    // and the like elsewhere. One line per path, `include` lines and comments
    // skipped — an absolute path is the only line this needs.
    if let Ok(entries) = std::fs::read_dir("/etc/ld.so.conf.d") {
        for entry in entries.flatten() {
            let Ok(text) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            for line in text.lines() {
                let line = line.trim();
                if line.starts_with('/') {
                    directories.push(PathBuf::from(line));
                }
            }
        }
    }

    directories.extend(
        [
            "/usr/lib",
            "/usr/lib64",
            "/usr/lib/x86_64-linux-gnu",
            "/lib",
            "/lib64",
            "/lib/x86_64-linux-gnu",
            "/usr/local/lib",
        ]
        .map(PathBuf::from),
    );
    directories
}

/// Where a renderer is on this machine, if it is on it at all.
#[must_use]
pub fn installed() -> Option<PathBuf> {
    let mut places = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        places.push(PathBuf::from(home).join(".local/bin/kirie"));
    }
    places.push(PathBuf::from("/usr/local/bin/kirie"));
    places.push(PathBuf::from("/usr/bin/kirie"));
    if let Some(paths) = std::env::var_os("PATH") {
        places.extend(std::env::split_paths(&paths).map(|dir| dir.join("kirie")));
    }
    places.into_iter().find(|path| path.is_file())
}

/// Where an install this app performs puts the binary.
///
/// Under the home directory, never a system one: haru is not a package manager
/// and must not need to be root to work.
#[must_use]
pub fn destination() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/bin/kirie"))
}

/// Whether a renderer can be installed on this platform at all.
///
/// kirie publishes one target. Saying so plainly beats offering a download
/// that cannot run.
#[must_use]
pub const fn supported() -> bool {
    cfg!(all(target_os = "linux", target_arch = "x86_64"))
}

/// A build, located and vouched for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    /// The release it belongs to: `v0.5.0`.
    pub tag: String,
    /// Where to fetch it.
    pub url: String,
    /// The digest GitHub holds for it, lowercase hex.
    pub sha256: String,
    /// How big it is, for a progress bar that means something.
    pub size: u64,
}

/// Finds the newest published build of one flavour.
///
/// # Errors
/// When the release cannot be read, or carries no asset for this platform.
pub fn latest(web: Web) -> Result<Build, String> {
    let url = format!("https://api.github.com/repos/{REPOSITORY}/releases/latest");
    let body = ureq::get(&url)
        .set("User-Agent", AGENT)
        .set("Accept", "application/vnd.github+json")
        .timeout(DEADLINE)
        .call()
        .map_err(|error| format!("could not reach GitHub ({error})"))?
        .into_string()
        .map_err(|error| format!("could not read the release ({error})"))?;

    let release: serde_json::Value =
        serde_json::from_str(&body).map_err(|error| format!("unreadable release ({error})"))?;

    let tag = release
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .ok_or("the release has no tag")?
        .to_owned();

    let assets = release
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .ok_or("the release lists no files")?;
    let asset = assets
        .iter()
        .find(|asset| asset.get("name").and_then(serde_json::Value::as_str) == Some(web.asset()))
        .ok_or_else(|| format!("{tag} has no {}", web.asset()))?;

    let url = asset
        .get("browser_download_url")
        .and_then(serde_json::Value::as_str)
        .ok_or("that file has no address")?
        .to_owned();
    // Not a formality: everything below trusts the transport to have delivered
    // what GitHub signed for, and plain HTTP does not.
    if !url.starts_with("https://") {
        return Err("the download is not over https".to_owned());
    }

    // `sha256:abc…` — the algorithm is named, so an unknown one is refused
    // rather than compared against a hash of the wrong kind.
    let sha256 = asset
        .get("digest")
        .and_then(serde_json::Value::as_str)
        .and_then(|digest| digest.strip_prefix("sha256:"))
        .ok_or("GitHub published no sha256 for that file")?
        .to_ascii_lowercase();

    let size = asset
        .get("size")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    if size > MAX_BYTES {
        return Err("that file is larger than any kirie release".to_owned());
    }

    Ok(Build {
        tag,
        url,
        sha256,
        size,
    })
}

/// Fetches a build and puts it at `target`.
///
/// The destination is passed in rather than assumed, so the only code that
/// decides to overwrite a renderer is the code that asked a person first.
///
/// `progress` is called with bytes so far and the expected total.
///
/// The download lands beside its destination and is renamed onto it, so an
/// interrupted fetch leaves either the previous renderer or nothing — never
/// half a binary with the execute bit set.
///
/// # Errors
/// When the download fails, the digest does not match, or the file cannot be
/// written.
pub fn fetch(
    build: &Build,
    target: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, String> {
    let parent = target.parent().ok_or("no directory to install into")?;
    std::fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;

    let response = ureq::get(&build.url)
        .set("User-Agent", AGENT)
        .timeout(DEADLINE)
        .call()
        .map_err(|error| format!("the download failed ({error})"))?;

    let staged = parent.join(format!("kirie.{}.part", std::process::id()));
    let outcome = write(response, build, &staged, progress);
    match outcome {
        Ok(()) => {
            std::fs::rename(&staged, target)
                .map_err(|error| format!("could not put it in place ({error})"))?;
            Ok(target.to_path_buf())
        }
        Err(error) => {
            let _ = std::fs::remove_file(&staged);
            Err(error)
        }
    }
}

/// Streams the body to `staged`, hashing it, and marks it executable.
fn write(
    response: ureq::Response,
    build: &Build,
    staged: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), String> {
    /// Big enough that the progress bar is not the bottleneck, small enough
    /// that a cancelled download does not sit in memory.
    const CHUNK: usize = 64 * 1024;

    let mut file =
        std::fs::File::create(staged).map_err(|error| format!("cannot write it ({error})"))?;
    let mut body = response.into_reader().take(MAX_BYTES);
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    let mut buffer = vec![0_u8; CHUNK];
    let mut done: u64 = 0;

    loop {
        let read = body
            .read(&mut buffer)
            .map_err(|error| format!("the download stopped ({error})"))?;
        if read == 0 {
            break;
        }
        let chunk = buffer.get(..read).ok_or("short read")?;
        sha2::Digest::update(&mut hasher, chunk);
        file.write_all(chunk)
            .map_err(|error| format!("cannot write it ({error})"))?;
        done = done.saturating_add(read as u64);
        progress(done, build.size);
    }
    file.flush().map_err(|error| format!("{error}"))?;

    let got = hex(&sha2::Digest::finalize(hasher));
    if got != build.sha256 {
        return Err(format!(
            "the download does not match what GitHub published for {} \
             (expected {}, got {got})",
            build.tag, build.sha256
        ));
    }

    executable(&file)?;
    Ok(())
}

/// Marks a file runnable by its owner.
#[cfg(unix)]
fn executable(file: &std::fs::File) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    file.set_permissions(std::fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("cannot make it runnable ({error})"))
}

/// Nothing to do where permissions are not a mode.
#[cfg(not(unix))]
fn executable(_file: &std::fs::File) -> Result<(), String> {
    Ok(())
}

/// Lowercase hex, to compare against what GitHub publishes.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
        out
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flavour_survives_the_config() {
        for web in [Web::WebKit, Web::Cef] {
            assert_eq!(Web::from_key(web.key()), Some(web));
        }
        assert_eq!(Web::from_key("netscape"), None);
    }

    #[test]
    fn the_two_builds_are_different_files() {
        // One asset name in both arms would install CEF for someone who chose
        // webkit, and the difference is 80 MB and a system dependency.
        assert_ne!(Web::WebKit.asset(), Web::Cef.asset());
    }

    #[test]
    fn hex_is_what_github_publishes() {
        assert_eq!(hex(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
    }

    /// Asks GitHub for a real release. Ignored by default: it needs a network,
    /// and a test that fails when the wifi does is a test nobody trusts.
    ///
    /// Run with `cargo test -p haru-apply -- --ignored`.
    #[test]
    #[ignore = "needs the network"]
    fn the_published_release_carries_both_builds() {
        for web in [Web::WebKit, Web::Cef] {
            let found = latest(web);
            assert!(found.is_ok(), "{}: {found:?}", web.asset());
            if let Ok(build) = found {
                assert!(build.tag.starts_with('v'), "{}", build.tag);
                assert!(build.url.starts_with("https://"), "{}", build.url);
                // A sha256 is 64 hex characters, and everything the fetch does
                // rests on having the right one.
                assert_eq!(build.sha256.len(), 64, "{}", build.sha256);
                assert!(build.sha256.chars().all(|c| c.is_ascii_hexdigit()));
                assert!(build.size > 1_000_000, "{} is too small", build.size);
            }
        }
    }

    /// Fetches a real 32 MB release into a scratch directory and checks it
    /// arrived whole. Ignored by default: it needs a network and moves real
    /// bytes.
    #[test]
    #[ignore = "needs the network"]
    fn a_fetched_build_matches_its_digest_and_is_runnable() {
        let Ok(build) = latest(Web::WebKit) else {
            return;
        };
        let scratch = std::env::temp_dir().join(format!("haru-install-{}", std::process::id()));
        let target = scratch.join("kirie");

        let mut last = 0_u64;
        let installed = fetch(&build, &target, &mut |done, _| last = done);
        assert!(installed.is_ok(), "{installed:?}");

        // The digest is checked inside `fetch`; this is the other half of the
        // claim — that what it checked is what ended up on disk.
        let written = std::fs::metadata(&target);
        assert!(written.is_ok(), "nothing at {}", target.display());
        if let Ok(written) = written {
            assert_eq!(written.len(), build.size);
            assert_eq!(last, build.size, "progress stopped short of the end");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                assert_eq!(written.permissions().mode() & 0o777, 0o755);
            }
        }
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn an_install_goes_under_the_home_directory() {
        // Never a system path: haru must not need to be root to work.
        if let Some(target) = destination() {
            assert!(target.ends_with(".local/bin/kirie"), "{}", target.display());
        }
    }
}

use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

const REPOSITORY: &str = "UnhingedSoftware/kirie";

const MAX_BYTES: u64 = 256 * 1024 * 1024;

const DEADLINE: std::time::Duration = std::time::Duration::from_secs(30);

const AGENT: &str = concat!("haru/", env!("CARGO_PKG_VERSION"));

const WEBKIT: [&str; 3] = [
    "libwebkit2gtk-4.1.so.0",
    "libwebkit2gtk-4.0.so.37",
    "libwebkit2gtk-4.0.so",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Web {
    WebKit,
    Cef,
}

impl Web {
    #[must_use]
    pub fn asset(self) -> String {
        if cfg!(target_os = "macos") {
            return format!("kirie-macos-{}", machine());
        }
        match self {
            Self::WebKit => format!("kirie-web-webview-linux-{}", machine()),
            Self::Cef => format!("kirie-web-cef-linux-{}", machine()),
        }
    }

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::WebKit => "WebKit",
            Self::Cef => "Chromium (CEF)",
        }
    }

    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::WebKit => "webkit",
            Self::Cef => "cef",
        }
    }

    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "webkit" => Some(Self::WebKit),
            "cef" => Some(Self::Cef),
            _ => None,
        }
    }

    #[must_use]
    pub fn suggested() -> Self {
        if cfg!(target_os = "macos") || webkit_present() {
            Self::WebKit
        } else {
            Self::Cef
        }
    }

    #[must_use]
    pub fn choosable() -> bool {
        !cfg!(target_os = "macos")
    }
}

#[must_use]
const fn machine() -> &'static str {
    std::env::consts::ARCH
}

#[must_use]
pub fn webkit_present() -> bool {
    library_directories()
        .iter()
        .any(|directory| WEBKIT.iter().any(|soname| directory.join(soname).exists()))
}

fn library_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();

    if let Some(paths) = std::env::var_os("LD_LIBRARY_PATH") {
        directories.extend(std::env::split_paths(&paths));
    }

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

#[must_use]
pub fn installed() -> Option<PathBuf> {
    if let Some(set) = std::env::var_os("KIRIE_BINARY") {
        let path = PathBuf::from(set);
        return path.is_file().then_some(path);
    }
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

#[must_use]
pub fn version_of(binary: &Path) -> Option<String> {
    let spoke = std::process::Command::new(binary)
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let said = String::from_utf8_lossy(&spoke.stdout);
    let line = said.lines().next()?.trim();
    line.strip_prefix("kirie ")
        .map(str::to_owned)
        .filter(|version| !version.is_empty())
}

#[must_use]
pub fn destination() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/bin/kirie"))
}

#[must_use]
pub const fn supported() -> bool {
    cfg!(any(
        all(target_os = "linux", target_arch = "x86_64"),
        target_os = "macos"
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Build {
    pub tag: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

pub fn latest(web: Web) -> Result<Build, String> {
    latest_from(REPOSITORY, &web.asset())
}

pub fn latest_including_betas(web: Web) -> Result<Build, String> {
    newest_from(REPOSITORY, &web.asset(), true)
}

pub fn latest_from(repository: &str, asset: &str) -> Result<Build, String> {
    newest_from(repository, asset, false)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Card {
    pub value: String,
    pub label: String,
}

#[must_use]
pub fn cards_from(text: &str) -> Vec<Card> {
    let Ok(list) = serde_json::from_str::<Vec<serde_json::Value>>(text) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|entry| {
            let value = entry.get("value").and_then(serde_json::Value::as_str)?;
            let label = entry
                .get("label")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(value);
            Some(Card {
                value: value.to_owned(),
                label: label.to_owned(),
            })
        })
        .collect()
}

#[must_use]
pub fn graphics_cards() -> Vec<Card> {
    let Some(binary) = installed() else {
        return Vec::new();
    };
    let Ok(out) = std::process::Command::new(binary)
        .args(["gpus", "--json"])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    cards_from(&String::from_utf8_lossy(&out.stdout))
}

#[must_use]
pub fn newest_release(releases: &[serde_json::Value], betas: bool) -> Option<&serde_json::Value> {
    releases.iter().find(|release| {
        let draft = release
            .get("draft")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let pre = release
            .get("prerelease")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        !draft && (betas || !pre)
    })
}

pub fn newest_from(repository: &str, asset: &str, betas: bool) -> Result<Build, String> {
    if betas {
        let url = format!("https://api.github.com/repos/{repository}/releases?per_page=20");
        let body = read_json(&url)?;
        let releases: Vec<serde_json::Value> = serde_json::from_str(&body)
            .map_err(|error| format!("unreadable releases ({error})"))?;
        let release = newest_release(&releases, true).ok_or("no release published yet")?;
        return build_from(release, asset);
    }
    let url = format!("https://api.github.com/repos/{repository}/releases/latest");
    let body = read_json(&url)?;

    let release: serde_json::Value =
        serde_json::from_str(&body).map_err(|error| format!("unreadable release ({error})"))?;
    build_from(&release, asset)
}

fn read_json(url: &str) -> Result<String, String> {
    ureq::get(url)
        .set("User-Agent", AGENT)
        .set("Accept", "application/vnd.github+json")
        .timeout(DEADLINE)
        .call()
        .map_err(|error| match error {
            ureq::Error::Status(404, _) => "no release published yet".to_owned(),
            other => format!("could not reach GitHub ({other})"),
        })?
        .into_string()
        .map_err(|error| format!("could not read the release ({error})"))
}

fn build_from(release: &serde_json::Value, asset: &str) -> Result<Build, String> {
    let tag = release
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .ok_or("the release has no tag")?
        .to_owned();

    let (url, sha256, size) = asset_in(release, &tag, asset)?;

    Ok(Build {
        tag,
        url,
        sha256,
        size,
    })
}

fn asset_in(
    release: &serde_json::Value,
    tag: &str,
    wanted: &str,
) -> Result<(String, String, u64), String> {
    let assets = release
        .get("assets")
        .and_then(serde_json::Value::as_array)
        .ok_or("the release lists no files")?;
    let asset = assets
        .iter()
        .find(|asset| asset.get("name").and_then(serde_json::Value::as_str) == Some(wanted))
        .ok_or_else(|| format!("{tag} has no {wanted}"))?;

    let url = asset
        .get("browser_download_url")
        .and_then(serde_json::Value::as_str)
        .ok_or("that file has no address")?
        .to_owned();
    if !url.starts_with("https://") {
        return Err("the download is not over https".to_owned());
    }

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
    Ok((url, sha256, size))
}

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

    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "download".to_owned());
    static ORDINAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let ordinal = ORDINAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let staged = parent.join(format!("{name}.{}.{ordinal}.part", std::process::id()));
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

fn write(
    response: ureq::Response,
    build: &Build,
    staged: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), String> {
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

#[cfg(unix)]
fn executable(file: &std::fs::File) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    file.set_permissions(std::fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("cannot make it runnable ({error})"))
}

#[cfg(not(unix))]
fn executable(_file: &std::fs::File) -> Result<(), String> {
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut out, byte| {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
        out
    })
}

#[cfg(test)]
mod tests {

    #[test]
    fn the_renderers_own_list_names_real_cards() {
        let text = r#"[{"icd":null,"kind":"auto","label":"Automatic (no pinning)","value":"auto"},
            {"icd":"/usr/share/vulkan/icd.d/nvidia_icd.json","kind":"discrete","label":"NVIDIA GeForce RTX 4080 (discrete)","value":"nvidia"}]"#;
        let cards = cards_from(text);
        assert_eq!(cards.len(), 2);
        let named = cards
            .last()
            .map(|card| (card.value.clone(), card.label.clone()));
        assert_eq!(
            named,
            Some((
                "nvidia".to_owned(),
                "NVIDIA GeForce RTX 4080 (discrete)".to_owned()
            ))
        );
    }

    #[test]
    fn a_card_without_a_label_falls_back_to_its_value() {
        let cards = cards_from(r#"[{"value":"radv"}]"#);
        assert_eq!(cards.len(), 1);
        assert_eq!(
            cards.first().map(|card| card.label.clone()),
            Some("radv".to_owned())
        );
    }

    #[test]
    fn nonsense_lists_nothing() {
        assert!(cards_from("not json").is_empty());
        assert!(cards_from("{}").is_empty());
    }

    fn release(tag: &str, draft: bool, prerelease: bool) -> serde_json::Value {
        serde_json::json!({ "tag_name": tag, "draft": draft, "prerelease": prerelease })
    }

    #[test]
    fn the_stable_channel_skips_pre_releases() {
        let list = [
            release("v0.4.0-beta.1", false, true),
            release("v0.3.0", false, false),
        ];
        let tag = newest_release(&list, false).map(|release| release["tag_name"].clone());
        assert_eq!(tag, Some(serde_json::json!("v0.3.0")));
    }

    #[test]
    fn the_beta_channel_takes_the_newest_of_either() {
        let list = [
            release("v0.4.0-beta.1", false, true),
            release("v0.3.0", false, false),
        ];
        let tag = newest_release(&list, true).map(|release| release["tag_name"].clone());
        assert_eq!(tag, Some(serde_json::json!("v0.4.0-beta.1")));
    }

    #[test]
    fn drafts_are_never_offered() {
        let list = [
            release("v0.5.0", true, false),
            release("v0.4.0-beta.1", false, true),
        ];
        let tag = newest_release(&list, true).map(|release| release["tag_name"].clone());
        assert_eq!(tag, Some(serde_json::json!("v0.4.0-beta.1")));
        assert!(newest_release(&list, false).is_none());
    }
    use super::*;

    #[test]
    fn a_flavour_survives_the_config() {
        for web in [Web::WebKit, Web::Cef] {
            assert_eq!(Web::from_key(web.key()), Some(web));
        }
        assert_eq!(Web::from_key("netscape"), None);
    }

    #[test]
    fn the_build_matches_this_platform() {
        let asset = Web::suggested().asset();
        if cfg!(target_os = "macos") {
            assert!(asset.starts_with("kirie-macos-"), "{asset}");
            assert_eq!(Web::WebKit.asset(), Web::Cef.asset());
            assert!(!Web::choosable());
        } else {
            assert!(asset.contains("linux"), "{asset}");
            assert_ne!(Web::WebKit.asset(), Web::Cef.asset());
        }
    }

    #[test]
    fn hex_is_what_github_publishes() {
        assert_eq!(hex(&[0x00, 0x0f, 0xa5, 0xff]), "000fa5ff");
    }

    #[test]
    #[ignore = "needs the network"]
    fn the_published_release_carries_both_builds() {
        for web in [Web::WebKit, Web::Cef] {
            let found = latest(web);
            assert!(found.is_ok(), "{}: {found:?}", web.asset());
            if let Ok(build) = found {
                assert!(build.tag.starts_with('v'), "{}", build.tag);
                assert!(build.url.starts_with("https://"), "{}", build.url);
                assert_eq!(build.sha256.len(), 64, "{}", build.sha256);
                assert!(build.sha256.chars().all(|c| c.is_ascii_hexdigit()));
                assert!(build.size > 1_000_000, "{} is too small", build.size);
            }
        }
    }

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
        if let Some(target) = destination() {
            assert!(target.ends_with(".local/bin/kirie"), "{}", target.display());
        }
    }
}

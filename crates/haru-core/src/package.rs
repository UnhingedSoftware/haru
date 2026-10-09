//! Reading kirie's `.kpk` packages, enough to list them.
//!
//! kirie writes and plays packages; haru only needs a package's manifest, its
//! preview and, for a repacked Wallpaper Engine item, its `project.json`. The
//! format is read here rather than through kirie's code, as haru shares no
//! code with kirie. See kirie's `docs/PACKAGE.md` and `crates/kirie-pack`.
//!
//! Layout: a 64-byte header (magic `KIRIEPKG`, version 1, flags 0, then the
//! manifest's and the index's offset and length as little-endian u64s, then
//! the first 16 bytes of blake3 over both), entries, then the manifest and
//! index as JSON. Every entry carries the blake3 hash of its stored bytes.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The extension a package file carries.
pub const EXTENSION: &str = "kpk";

const MAGIC: &[u8; 8] = b"KIRIEPKG";
const VERSION: u16 = 1;
const HEADER_LEN: usize = 64;
const MAX_MANIFEST_LEN: u64 = 1 << 20;
const MAX_INDEX_LEN: u64 = 64 << 20;
/// The most an entry read here may be: previews and project files are small,
/// and a package claiming more is not decompressed into memory.
const MAX_READ_LEN: u64 = 64 << 20;

/// Where haru keeps the packages it makes from the items it downloads.
#[must_use]
pub fn home() -> Option<PathBuf> {
    crate::data_home().map(|base| base.join("haru/packages"))
}

/// Where haru keeps packages copied from items the Steam client downloaded.
/// Steam fetches a folder again if it goes missing, so those folders stay and
/// the copy sits beside them; see `crate::library::copy_is_current`.
#[must_use]
pub fn copies() -> Option<PathBuf> {
    home().map(|dir| dir.join("steam"))
}

/// Whether `path` names a package rather than an item folder.
#[must_use]
pub fn is_package(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(EXTENSION))
}

#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub entry: String,
    #[serde(default)]
    pub preview: Option<String>,
    #[serde(default)]
    pub provenance: Provenance,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Provenance {
    #[serde(default)]
    pub origin: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub source_id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct Index {
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, Deserialize)]
struct Entry {
    path: String,
    offset: u64,
    stored_len: u64,
    len: u64,
    compression: String,
    #[serde(default = "none")]
    cipher: String,
    blake3: String,
}

fn none() -> String {
    "none".to_owned()
}

/// An opened package: its manifest and where its entries are.
pub struct Package {
    path: PathBuf,
    pub manifest: Manifest,
    entries: Vec<Entry>,
    fingerprint: String,
}

impl Package {
    /// Open `path` and read its manifest and index. Nothing is returned for a
    /// file that is not a version 1 package or whose directory is damaged.
    #[must_use]
    pub fn open(path: &Path) -> Option<Self> {
        let mut file = std::fs::File::open(path).ok()?;
        let file_len = file.metadata().ok()?.len();
        let mut header = [0_u8; HEADER_LEN];
        file.read_exact(&mut header).ok()?;
        let bytes = |at: usize, len: usize| header.get(at..at + len);
        let u16_at = |at: usize| Some(u16::from_le_bytes(bytes(at, 2)?.try_into().ok()?));
        let u64_at = |at: usize| Some(u64::from_le_bytes(bytes(at, 8)?.try_into().ok()?));
        if bytes(0, 8)? != MAGIC || u16_at(8)? != VERSION || u16_at(10)? != 0 {
            return None;
        }
        let (manifest_offset, manifest_len) = (u64_at(16)?, u64_at(24)?);
        let (index_offset, index_len) = (u64_at(32)?, u64_at(40)?);
        let directory_hash = bytes(48, 16)?;
        if manifest_len > MAX_MANIFEST_LEN
            || index_len > MAX_INDEX_LEN
            || index_offset != manifest_offset.checked_add(manifest_len)?
            || index_offset.checked_add(index_len)? != file_len
        {
            return None;
        }
        let manifest_bytes = read_at(&mut file, manifest_offset, manifest_len)?;
        let index_bytes = read_at(&mut file, index_offset, index_len)?;
        let mut hasher = blake3::Hasher::new();
        hasher.update(&manifest_bytes);
        hasher.update(&index_bytes);
        if hasher.finalize().as_bytes().get(..16)? != directory_hash {
            return None;
        }
        let manifest: Manifest = serde_json::from_slice(&manifest_bytes).ok()?;
        let index: Index = serde_json::from_slice(&index_bytes).ok()?;
        Some(Self {
            path: path.to_owned(),
            manifest,
            entries: index.entries,
            fingerprint: directory_hash.iter().map(|b| format!("{b:02x}")).collect(),
        })
    }

    /// 32 hex digits that change whenever the package's contents do.
    #[must_use]
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// The id the library knows this package by: the Workshop id of the item
    /// it was made from, so it lines up with the Workshop, or else its own.
    #[must_use]
    pub fn library_id(&self) -> String {
        let provenance = &self.manifest.provenance;
        if provenance.source == "wallpaper_engine" && !provenance.source_id.is_empty() {
            provenance.source_id.clone()
        } else {
            self.manifest.id.clone()
        }
    }

    /// One entry's bytes, checked against its hash and decompressed.
    #[must_use]
    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        let entry = self.entries.iter().find(|entry| entry.path == path)?;
        if entry.cipher != "none" || entry.len > MAX_READ_LEN || entry.stored_len > MAX_READ_LEN {
            return None;
        }
        let mut file = std::fs::File::open(&self.path).ok()?;
        let stored = read_at(&mut file, entry.offset, entry.stored_len)?;
        if blake3::hash(&stored).to_hex().as_str() != entry.blake3 {
            return None;
        }
        let bytes = match entry.compression.as_str() {
            "none" => stored,
            "lz4" => lz4_flex::decompress(&stored, usize::try_from(entry.len).ok()?).ok()?,
            _ => return None,
        };
        (bytes.len() as u64 == entry.len).then_some(bytes)
    }

    /// The `project.json` of a repacked Wallpaper Engine item.
    #[must_use]
    pub fn project(&self) -> Option<serde_json::Value> {
        if self.manifest.kind != "wallpaper_engine" {
            return None;
        }
        serde_json::from_slice(&self.read(&self.manifest.entry)?).ok()
    }
}

fn read_at(file: &mut std::fs::File, offset: u64, len: u64) -> Option<Vec<u8>> {
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut buf = vec![0_u8; usize::try_from(len).ok()?];
    file.read_exact(&mut buf).ok()?;
    Some(buf)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    type Outcome = Result<(), Box<dyn std::error::Error>>;

    /// A package as kirie writes one, built by hand. Each entry is stored as
    /// is, or compressed with LZ4 when its flag is set.
    pub(crate) fn write_package(
        path: &Path,
        manifest: &str,
        entries: &[(&str, &[u8], bool)],
    ) -> std::io::Result<()> {
        let mut entry_area = Vec::new();
        let mut index = Vec::new();
        for (name, bytes, compress) in entries {
            let stored = if *compress {
                lz4_flex::compress(bytes)
            } else {
                bytes.to_vec()
            };
            let offset = 4096 + entry_area.len() as u64;
            entry_area.extend_from_slice(&stored);
            entry_area.resize(entry_area.len().div_ceil(4096) * 4096, 0);
            index.push(serde_json::json!({
                "path": name,
                "offset": offset,
                "stored_len": stored.len(),
                "len": bytes.len(),
                "compression": if *compress { "lz4" } else { "none" },
                "cipher": "none",
                "blake3": blake3::hash(&stored).to_hex().to_string(),
            }));
        }
        let manifest = manifest.as_bytes();
        let index = serde_json::to_vec(&serde_json::json!({ "entries": index }))?;
        let manifest_offset = 4096 + entry_area.len() as u64;
        let mut hasher = blake3::Hasher::new();
        hasher.update(manifest);
        hasher.update(&index);

        let mut header = Vec::with_capacity(4096);
        header.extend_from_slice(MAGIC);
        header.extend_from_slice(&VERSION.to_le_bytes());
        header.extend_from_slice(&[0; 6]);
        header.extend_from_slice(&manifest_offset.to_le_bytes());
        header.extend_from_slice(&(manifest.len() as u64).to_le_bytes());
        header.extend_from_slice(&(manifest_offset + manifest.len() as u64).to_le_bytes());
        header.extend_from_slice(&(index.len() as u64).to_le_bytes());
        header.extend_from_slice(hasher.finalize().as_bytes().get(..16).unwrap_or_default());
        header.resize(4096, 0);

        let mut out = header;
        out.extend_from_slice(&entry_area);
        out.extend_from_slice(manifest);
        out.extend_from_slice(&index);
        std::fs::write(path, out)
    }

    pub(crate) const CONVERTED: &str = r#"{"id":"we-1388331347","title":"Rainy street","kind":"wallpaper_engine",
        "entry":"project.json","preview":"preview.jpg",
        "provenance":{"origin":"converted","source":"wallpaper_engine","source_id":"1388331347"}}"#;

    pub(crate) const PROJECT: &[u8] =
        br#"{"title":"Rainy street","file":"scene.json","type":"Scene","general":{"properties":{"rain":{"type":"bool","text":"Rain","value":true,"order":1}}}}"#;

    #[test]
    fn a_converted_item_reads_back_with_its_workshop_id() -> Outcome {
        let path = std::env::temp_dir().join(format!("haru-package-{}.kpk", std::process::id()));
        write_package(
            &path,
            CONVERTED,
            &[
                ("preview.jpg", b"jpeg", false),
                ("project.json", PROJECT, true),
            ],
        )?;

        let package = Package::open(&path).ok_or("did not open")?;
        assert_eq!(package.library_id(), "1388331347");
        assert_eq!(package.manifest.title, "Rainy street");
        assert_eq!(package.read("preview.jpg").as_deref(), Some(&b"jpeg"[..]));
        let project = package.project().ok_or("no project.json")?;
        assert_eq!(project.get("type"), Some(&serde_json::json!("Scene")));
        assert_eq!(package.fingerprint().len(), 32);
        std::fs::remove_file(&path)?;
        Ok(())
    }

    #[test]
    fn a_damaged_package_is_refused() -> Outcome {
        let path =
            std::env::temp_dir().join(format!("haru-package-bad-{}.kpk", std::process::id()));
        write_package(&path, CONVERTED, &[("preview.jpg", b"jpeg", false)])?;
        let mut bytes = std::fs::read(&path)?;

        // A changed entry byte: the package opens, the entry does not read.
        if let Some(byte) = bytes.get_mut(4096) {
            *byte ^= 1;
        }
        std::fs::write(&path, &bytes)?;
        assert!(
            Package::open(&path)
                .ok_or("did not open")?
                .read("preview.jpg")
                .is_none()
        );

        // A changed manifest byte: the package does not open.
        let at = bytes
            .windows(5)
            .position(|w| w == b"Rainy")
            .ok_or("no title")?;
        if let Some(byte) = bytes.get_mut(at) {
            *byte = b'X';
        }
        std::fs::write(&path, &bytes)?;
        assert!(Package::open(&path).is_none());
        std::fs::remove_file(&path)?;
        Ok(())
    }
}

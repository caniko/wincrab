use std::borrow::Cow;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use tracing::info;

use crate::error::{Error, ensure_dir, write_file};

/// Build output metadata written alongside the ISO.
///
/// `wincrab_version` uses `Cow<'static, str>` because the version string
/// is always `env!("CARGO_PKG_VERSION")` (a `&'static str`) at build time,
/// avoiding a heap allocation. When deserializing from JSON, serde
/// transparently promotes the value to an owned `String` inside the `Cow`.
#[derive(Debug, Serialize, Deserialize)]
pub struct BuildManifest {
    pub wincrab_version: Cow<'static, str>,
    pub source_iso_sha256: String,
    pub output_iso_sha256: String,
    pub source_iso_size_bytes: u64,
    pub output_iso_size_bytes: u64,
    pub config_snapshot: String,
    pub timestamp: String,
}

pub fn compute_sha256(path: &Path) -> Result<String, Error> {
    let file = std::fs::File::open(path).map_err(|e| Error::Io {
        context: format!("opening {} for SHA-256", path.display()),
        source: e,
    })?;

    let mut hasher = Sha256::new();
    let mut reader = std::io::BufReader::with_capacity(64 * 1024, file);
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).map_err(|e| Error::Io {
            context: format!("reading {} for SHA-256", path.display()),
            source: e,
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let result = hasher.finalize();

    Ok(format!("{result:x}"))
}

pub fn write_manifest(manifest: &BuildManifest, output_path: &Path) -> Result<(), Error> {
    let json = serde_json::to_string_pretty(manifest).map_err(|e| Error::Io {
        context: "serializing build manifest to JSON".into(),
        source: std::io::Error::other(e),
    })?;

    if let Some(parent) = output_path.parent() {
        ensure_dir(parent)?;
    }

    write_file(output_path, &json)?;

    info!(path = %output_path.display(), "wrote build manifest");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_of_known_data() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.bin");
        std::fs::write(&path, b"hello world").unwrap();

        let hash = compute_sha256(&path).unwrap();
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn sha256_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty");
        std::fs::write(&path, b"").unwrap();

        let hash = compute_sha256(&path).unwrap();
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_nonexistent_returns_error() {
        let result = compute_sha256(Path::new("/nonexistent/file"));
        assert!(result.is_err());
    }

    #[test]
    fn write_and_read_manifest() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manifest.json");

        let manifest = BuildManifest {
            wincrab_version: "0.1.0".into(),
            source_iso_sha256: "abc123".into(),
            output_iso_sha256: "def456".into(),
            source_iso_size_bytes: 5_000_000_000,
            output_iso_size_bytes: 3_500_000_000,
            config_snapshot: "wim_index = 6".into(),
            timestamp: "2026-03-15T00:00:00Z".into(),
        };

        write_manifest(&manifest, &path).unwrap();

        let content = std::fs::read_to_string(&path).unwrap();
        let loaded: BuildManifest = serde_json::from_str(&content).unwrap();
        assert_eq!(loaded.wincrab_version, "0.1.0");
        assert_eq!(loaded.source_iso_size_bytes, 5_000_000_000);
        assert_eq!(loaded.output_iso_size_bytes, 3_500_000_000);
    }

    #[test]
    fn manifest_serialization_is_pretty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manifest.json");

        let manifest = BuildManifest {
            wincrab_version: "0.1.0".into(),
            source_iso_sha256: "abc".into(),
            output_iso_sha256: "def".into(),
            source_iso_size_bytes: 100,
            output_iso_size_bytes: 50,
            config_snapshot: "{}".into(),
            timestamp: "now".into(),
        };

        write_manifest(&manifest, &path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains('\n'));
        assert!(content.contains("  "));
    }
}

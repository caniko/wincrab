use std::path::{Path, PathBuf};
use std::process::Command;

use tracing::info;

use crate::error::{ensure_dir, read_file_string, run_cmd, Error};

/// Describes a GitHub release asset to download and cache.
///
/// Implement this trait to specify which asset to fetch from a repository's
/// latest release. [`download_asset`] handles the API query, caching, and
/// the actual download.
pub trait GitHubAsset {
    /// GitHub releases API URL (e.g. `https://api.github.com/repos/o/r/releases/latest`).
    fn api_url(&self) -> &str;

    /// Human-readable label for log messages.
    fn label(&self) -> &str;

    /// If set, the downloaded file is saved (and cache-checked) under this name
    /// instead of the filename derived from the download URL.
    fn output_filename(&self) -> Option<&str> {
        None
    }

    /// Return a match priority for the given asset download URL.
    /// Higher values are preferred; return 0 for no match.
    fn match_priority(&self, url: &str) -> u32;
}

/// A simple [`GitHubAsset`] backed by a function-pointer predicate.
pub struct SimpleAsset {
    pub api_url: &'static str,
    pub label: &'static str,
    pub predicate: fn(&str) -> bool,
}

impl GitHubAsset for SimpleAsset {
    fn api_url(&self) -> &str {
        self.api_url
    }
    fn label(&self) -> &str {
        self.label
    }
    fn match_priority(&self, url: &str) -> u32 {
        if (self.predicate)(url) { 1 } else { 0 }
    }
}

/// Download the asset described by `asset`, caching under `cache_dir`.
///
/// Returns the path to the downloaded (or cached) file.
pub fn download_asset(asset: &impl GitHubAsset, cache_dir: &Path) -> Result<PathBuf, Error> {
    ensure_dir(cache_dir)?;

    // Early cache hit when the output filename is known upfront.
    if let Some(filename) = asset.output_filename() {
        let cached = cache_dir.join(filename);
        if cached.exists() {
            info!(path = %cached.display(), "using cached {}", asset.label());
            return Ok(cached);
        }
    }

    let label = asset.label();
    let json_path = cache_dir.join(format!("{label}-release.json"));

    info!(label, "querying GitHub for latest {label} release");
    run_cmd(
        Command::new("curl")
            .arg("-fsSL")
            .arg("-H")
            .arg("Accept: application/vnd.github+json")
            .arg("-o")
            .arg(&json_path)
            .arg(asset.api_url()),
    )?;

    let json = read_file_string(&json_path)?;

    let download_url = select_asset_url(&json, label, |url| asset.match_priority(url))?;

    let filename = asset.output_filename().unwrap_or_else(|| {
        download_url.rsplit('/').next().unwrap_or("download")
    });

    let cached = cache_dir.join(filename);
    if cached.exists() {
        info!(path = %cached.display(), "using cached {label}");
        return Ok(cached);
    }

    info!(url = %download_url, "downloading {label}");
    run_cmd(
        Command::new("curl")
            .arg("-fsSL")
            .arg("-o")
            .arg(&cached)
            .arg(&download_url),
    )?;

    let size_mb = std::fs::metadata(&cached)
        .map(|m| m.len() / (1024 * 1024))
        .unwrap_or(0);
    info!(size_mb, path = %cached.display(), "{label} download complete");

    Ok(cached)
}

/// Parse GitHub releases JSON and return the best matching asset URL.
///
/// Calls `priority_fn` on each `browser_download_url` found in the JSON and
/// returns the URL with the highest non-zero priority.
pub fn select_asset_url(
    json: &str,
    label: &str,
    priority_fn: impl Fn(&str) -> u32,
) -> Result<String, Error> {
    let mut best: Option<(u32, String)> = None;

    for line in json.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("\"browser_download_url\":") {
            let url = rest
                .trim()
                .trim_matches('"')
                .trim_end_matches(',')
                .trim_matches('"');
            let priority = priority_fn(url);
            if priority > 0 && best.as_ref().is_none_or(|(p, _)| priority > *p) {
                best = Some((priority, url.to_string()));
            }
        }
    }

    best.map(|(_, url)| url).ok_or_else(|| Error::Config {
        message: format!("could not find a matching asset for {label} in GitHub release"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_finds_matching_url() {
        let json = r#"
    "browser_download_url": "https://example.com/foo.zip"
    "browser_download_url": "https://example.com/bar.tar.gz"
"#;
        let url =
            select_asset_url(json, "test", |u| if u.ends_with(".zip") { 1 } else { 0 }).unwrap();
        assert!(url.ends_with(".zip"));
    }

    #[test]
    fn select_prefers_higher_priority() {
        let json = r#"
    "browser_download_url": "https://example.com/fixed.exe"
    "browser_download_url": "https://example.com/normal.exe"
"#;
        let url = select_asset_url(json, "test", |u| {
            if u.contains("normal") {
                2
            } else {
                1
            }
        })
        .unwrap();
        assert!(url.contains("normal"));
    }

    #[test]
    fn select_no_match_returns_error() {
        let result = select_asset_url("", "test", |_| 0);
        assert!(result.is_err());
    }

    #[test]
    fn select_handles_trailing_comma() {
        let json = r#"      "browser_download_url": "https://example.com/file.exe","#;
        let url = select_asset_url(json, "test", |_| 1).unwrap();
        assert_eq!(url, "https://example.com/file.exe");
    }

    #[test]
    fn simple_asset_match_priority() {
        let asset = SimpleAsset {
            api_url: "",
            label: "test",
            predicate: |u| u.ends_with(".zip"),
        };
        assert_eq!(asset.match_priority("foo.zip"), 1);
        assert_eq!(asset.match_priority("foo.tar.gz"), 0);
    }

    #[test]
    fn output_filename_default_is_none() {
        let asset = SimpleAsset {
            api_url: "",
            label: "test",
            predicate: |_| true,
        };
        assert!(asset.output_filename().is_none());
    }
}

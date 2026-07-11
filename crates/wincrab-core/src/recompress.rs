use std::path::Path;
use std::process::Command;

use tracing::info;

use crate::error::{file_size_or_zero, remove_file, run_cmd, Error};

/// Re-export the WIM image with solid LZX compression to reduce ISO size.
///
/// This runs `wimlib-imagex export` on the install.wim to produce a new,
/// more tightly compressed copy. This can save several hundred MB on the
/// final ISO.
pub fn recompress_wim(wim_path: &Path) -> Result<(), Error> {
    let parent = wim_path.parent().ok_or_else(|| Error::Config {
        message: format!("WIM path has no parent directory: {}", wim_path.display()),
    })?;
    let recompressed = parent.join("install_recompressed.wim");

    let original_size = file_size_or_zero(wim_path) / (1024 * 1024);

    info!(
        src = %wim_path.display(),
        dest = %recompressed.display(),
        original_size_mb = original_size,
        "re-exporting WIM with solid LZX compression"
    );

    // Export all images from the original WIM into a new file with --solid compression.
    // --solid uses LZMS solid-mode compression which produces significantly smaller files.
    run_cmd(
        Command::new("wimlib-imagex")
            .arg("export")
            .arg(wim_path)
            .arg("all")
            .arg(&recompressed)
            .arg("--solid"),
    )?;

    // Replace original with recompressed version.
    remove_file(wim_path)?;

    std::fs::rename(&recompressed, wim_path).map_err(|e| Error::Io {
        context: format!(
            "renaming {} -> {}",
            recompressed.display(),
            wim_path.display()
        ),
        source: e,
    })?;

    let new_size = file_size_or_zero(wim_path) / (1024 * 1024);

    info!(
        original_mb = original_size,
        new_mb = new_size,
        saved_mb = original_size.saturating_sub(new_size),
        "WIM recompression complete"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recompress_no_parent_returns_error() {
        // A bare filename with no parent directory component.
        let result = recompress_wim(Path::new("install.wim"));
        // This should at minimum fail — either at parent() check or at wimlib.
        assert!(result.is_err());
    }

    #[test]
    fn recompress_nonexistent_wim_fails() {
        let dir = tempfile::tempdir().unwrap();
        let wim_path = dir.path().join("nonexistent.wim");
        let result = recompress_wim(&wim_path);
        assert!(result.is_err());
    }

    #[test]
    fn recompress_with_valid_parent_but_missing_wim() {
        let dir = tempfile::tempdir().unwrap();
        let wim_path = dir.path().join("sources").join("install.wim");
        std::fs::create_dir_all(wim_path.parent().unwrap()).unwrap();
        // Don't create the file -- wimlib-imagex should fail.
        let result = recompress_wim(&wim_path);
        assert!(result.is_err());
    }

    #[test]
    fn recompress_path_has_parent() {
        // Verify that a path with a parent doesn't trigger the "no parent" error.
        let dir = tempfile::tempdir().unwrap();
        let wim_path = dir.path().join("install.wim");
        std::fs::write(&wim_path, b"fake wim data").unwrap();

        let result = recompress_wim(&wim_path);
        // Will fail because wimlib-imagex isn't available, but NOT due to
        // parent directory check.
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Config { message } if message.contains("parent") => {
                panic!("should not fail on parent check for valid path");
            }
            _ => (), // Expected: ToolNotFound or Command error.
        }
    }
}

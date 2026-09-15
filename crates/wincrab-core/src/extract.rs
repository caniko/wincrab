use std::path::Path;
use std::process::Command;

use tracing::info;

use crate::error::{Error, ensure_dir, run_cmd};

/// Extract a Windows ISO to `staging_dir` using `7z`.
///
/// The staging directory will contain the full ISO tree (boot/, efi/, sources/, etc.).
pub fn extract_iso(iso_path: &Path, staging_dir: &Path) -> Result<(), Error> {
    if !iso_path.exists() {
        return Err(Error::Io {
            context: format!("ISO not found: {}", iso_path.display()),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "source ISO does not exist"),
        });
    }

    ensure_dir(staging_dir)?;

    info!(
        iso = %iso_path.display(),
        dest = %staging_dir.display(),
        "extracting ISO with 7z"
    );

    // 7z x -o<dir> <iso> extracts preserving directory structure.
    // -aoa = overwrite all existing files without prompt.
    run_cmd(
        Command::new("7z")
            .arg("x")
            .arg(format!("-o{}", staging_dir.display()))
            .arg("-aoa")
            .arg(iso_path),
    )?;

    // Verify critical file exists (reuse find_install_image to avoid duplication).
    find_install_image(staging_dir)?;

    info!("ISO extraction complete");
    Ok(())
}

/// Locate the WIM/ESD file inside the staging directory.
/// Prefers `.wim` over `.esd`.
pub fn find_install_image(staging_dir: &Path) -> Result<std::path::PathBuf, Error> {
    let wim = staging_dir.join("sources").join("install.wim");
    if wim.exists() {
        return Ok(wim);
    }

    let esd = staging_dir.join("sources").join("install.esd");
    if esd.exists() {
        return Ok(esd);
    }

    Err(Error::WimNotFound { path: wim })
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // find_install_image
    // -----------------------------------------------------------------------

    #[test]
    fn find_wim_prefers_wim_over_esd() {
        let dir = tempfile::tempdir().unwrap();
        let sources = dir.path().join("sources");
        std::fs::create_dir_all(&sources).unwrap();
        std::fs::write(sources.join("install.wim"), b"wim data").unwrap();
        std::fs::write(sources.join("install.esd"), b"esd data").unwrap();

        let result = find_install_image(dir.path()).unwrap();
        assert!(result.to_string_lossy().ends_with("install.wim"));
    }

    #[test]
    fn find_falls_back_to_esd() {
        let dir = tempfile::tempdir().unwrap();
        let sources = dir.path().join("sources");
        std::fs::create_dir_all(&sources).unwrap();
        std::fs::write(sources.join("install.esd"), b"esd data").unwrap();

        let result = find_install_image(dir.path()).unwrap();
        assert!(result.to_string_lossy().ends_with("install.esd"));
    }

    #[test]
    fn find_returns_error_when_neither_exists() {
        let dir = tempfile::tempdir().unwrap();
        let sources = dir.path().join("sources");
        std::fs::create_dir_all(&sources).unwrap();

        let result = find_install_image(dir.path());
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::WimNotFound { .. }));
    }

    #[test]
    fn find_returns_error_when_sources_missing() {
        let dir = tempfile::tempdir().unwrap();
        let result = find_install_image(dir.path());
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // extract_iso
    // -----------------------------------------------------------------------

    #[test]
    fn extract_iso_nonexistent_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let result = extract_iso(
            Path::new("/nonexistent/win11.iso"),
            &dir.path().join("staging"),
        );
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Io { context, .. } => assert!(context.contains("ISO not found")),
            other => panic!("expected Io error, got: {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // Edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn find_wim_ignores_other_files_in_sources() {
        let dir = tempfile::tempdir().unwrap();
        let sources = dir.path().join("sources");
        std::fs::create_dir_all(&sources).unwrap();
        std::fs::write(sources.join("install.wim"), b"wim").unwrap();
        std::fs::write(sources.join("boot.wim"), b"boot").unwrap();
        std::fs::write(sources.join("setup.dll"), b"dll").unwrap();
        std::fs::write(sources.join("install.swm"), b"split").unwrap();

        let result = find_install_image(dir.path()).unwrap();
        assert!(result.ends_with("install.wim"));
    }

    #[test]
    fn find_install_image_esd_only() {
        let dir = tempfile::tempdir().unwrap();
        let sources = dir.path().join("sources");
        std::fs::create_dir_all(&sources).unwrap();
        std::fs::write(sources.join("install.esd"), b"esd").unwrap();

        let path = find_install_image(dir.path()).unwrap();
        assert!(path.ends_with("install.esd"));
    }

    #[test]
    fn find_returns_absolute_path() {
        let dir = tempfile::tempdir().unwrap();
        let sources = dir.path().join("sources");
        std::fs::create_dir_all(&sources).unwrap();
        std::fs::write(sources.join("install.wim"), b"wim").unwrap();

        let result = find_install_image(dir.path()).unwrap();
        assert!(result.is_absolute());
    }
}

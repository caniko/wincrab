use std::path::Path;
use std::process::Command;

use tracing::info;

use crate::error::{Error, run_cmd};
use crate::extract::{extract_iso, find_install_image};

pub fn inspect_iso(iso_path: &Path, work_dir: &Path) -> Result<String, Error> {
    let staging_dir = work_dir.join("inspect_staging");

    info!(iso = %iso_path.display(), "inspecting ISO");

    extract_iso(iso_path, &staging_dir)?;

    let image_path = find_install_image(&staging_dir)?;

    info!(image = %image_path.display(), "querying image info");

    let output = run_cmd(Command::new("wimlib-imagex").arg("info").arg(&image_path))?;

    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();

    info!("ISO inspection complete");

    Ok(stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspect_nonexistent_iso_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let result = inspect_iso(Path::new("/nonexistent/windows.iso"), dir.path());
        assert!(result.is_err());
    }

    #[test]
    fn staging_dir_path() {
        let work = Path::new("/tmp/work");
        let staging = work.join("inspect_staging");
        assert_eq!(staging, Path::new("/tmp/work/inspect_staging"));
    }
}

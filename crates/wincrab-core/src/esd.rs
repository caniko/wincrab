use std::path::{Path, PathBuf};
use std::process::Command;

use tracing::info;

use crate::config::WimIndex;
use crate::error::{Error, remove_file, run_cmd};

pub fn convert_esd_to_wim(esd_path: &Path, wim_index: WimIndex) -> Result<PathBuf, Error> {
    let output_wim = esd_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("install.wim");

    info!(
        esd = %esd_path.display(),
        wim = %output_wim.display(),
        index = %wim_index,
        "converting ESD to WIM"
    );

    run_cmd(
        Command::new("wimlib-imagex")
            .arg("export")
            .arg(esd_path)
            .arg(wim_index.to_string())
            .arg(&output_wim)
            .arg("--compress=LZX"),
    )?;

    remove_file(esd_path)?;

    info!(wim = %output_wim.display(), "ESD to WIM conversion complete");

    Ok(output_wim)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_path_is_install_wim_in_same_dir() {
        let esd = Path::new("/tmp/sources/install.esd");
        let expected = Path::new("/tmp/sources/install.wim");
        let parent = esd.parent().unwrap_or(Path::new("."));
        let output = parent.join("install.wim");
        assert_eq!(output, expected);
    }

    #[test]
    fn output_path_no_parent() {
        let esd = Path::new("install.esd");
        let parent = esd.parent().unwrap_or(Path::new("."));
        let output = parent.join("install.wim");
        assert_eq!(output, Path::new("install.wim"));
    }
}

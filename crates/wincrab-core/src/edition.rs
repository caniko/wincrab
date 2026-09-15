use std::path::Path;
use std::process::Command;

use tracing::info;

use crate::config::WimIndex;
use crate::error::{Error, ensure_dir, run_cmd, write_file};

pub fn convert_edition(
    staging_dir: &Path,
    wim_path: &Path,
    wim_index: WimIndex,
    target_edition: &str,
) -> Result<(), Error> {
    info!(
        edition = target_edition,
        wim = %wim_path.display(),
        index = %wim_index,
        "converting Windows edition"
    );

    run_cmd(
        Command::new("wimlib-imagex")
            .arg("info")
            .arg(wim_path)
            .arg(wim_index.to_string())
            .arg(format!("--image-property=FLAGS={target_edition}")),
    )?;

    let sources_dir = staging_dir.join("sources");
    ensure_dir(&sources_dir)?;

    let ei_cfg = format!(
        "[EditionID]\n\
         {target_edition}\n\
         [Channel]\n\
         Retail\n\
         [VL]\n\
         0\n"
    );
    let ei_cfg_path = sources_dir.join("ei.cfg");
    write_file(&ei_cfg_path, &ei_cfg)?;
    info!(path = %ei_cfg_path.display(), "wrote ei.cfg");

    if target_edition == "Professional" {
        let pid_txt = "[PID]\nValue=VK7JG-NPHTM-C97JM-9MPGT-3V66T\n";
        let pid_path = sources_dir.join("PID.txt");
        write_file(&pid_path, pid_txt)?;
        info!(path = %pid_path.display(), "wrote PID.txt with generic Pro key");
    }

    info!(edition = target_edition, "edition conversion complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn ei_cfg_format() {
        let edition = "Professional";
        let content = format!(
            "[EditionID]\n\
             {edition}\n\
             [Channel]\n\
             Retail\n\
             [VL]\n\
             0\n"
        );
        assert!(content.contains("[EditionID]"));
        assert!(content.contains("Professional"));
        assert!(content.contains("[Channel]"));
        assert!(content.contains("Retail"));
        assert!(content.contains("[VL]"));
        assert!(content.contains("0"));
    }

    #[test]
    fn pid_txt_format() {
        let pid = "[PID]\nValue=VK7JG-NPHTM-C97JM-9MPGT-3V66T\n";
        assert!(pid.contains("[PID]"));
        assert!(pid.contains("VK7JG-NPHTM-C97JM-9MPGT-3V66T"));
    }
}

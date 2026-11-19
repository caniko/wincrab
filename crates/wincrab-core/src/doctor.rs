use std::process::Command;

use tracing::{info, warn};

use crate::error::Error;

struct ToolCheck {
    name: &'static str,
    version_args: &'static [&'static str],
}

const TOOLS: &[ToolCheck] = &[
    ToolCheck {
        name: "7z",
        version_args: &[],
    },
    ToolCheck {
        name: "wimlib-imagex",
        version_args: &["--version"],
    },
    #[cfg(not(windows))]
    ToolCheck {
        name: "hivexregedit",
        version_args: &["--version"],
    },
    #[cfg(not(windows))]
    ToolCheck {
        name: "genisoimage",
        version_args: &["--version"],
    },
    #[cfg(target_os = "linux")]
    ToolCheck {
        name: "sgdisk",
        version_args: &["--version"],
    },
    #[cfg(target_os = "linux")]
    ToolCheck {
        name: "partprobe",
        version_args: &["--version"],
    },
    #[cfg(target_os = "linux")]
    ToolCheck {
        name: "mkfs.fat",
        version_args: &["--help"],
    },
    #[cfg(target_os = "linux")]
    ToolCheck {
        name: "mkfs.ntfs",
        version_args: &["--help"],
    },
    #[cfg(target_os = "linux")]
    ToolCheck {
        name: "ntfs-3g",
        version_args: &["--version"],
    },
    #[cfg(target_os = "linux")]
    ToolCheck {
        name: "bcd-sys",
        version_args: &[],
    },
    #[cfg(windows)]
    ToolCheck {
        name: "dism",
        version_args: &[],
    },
    #[cfg(windows)]
    ToolCheck {
        name: "reg",
        version_args: &[],
    },
    #[cfg(windows)]
    ToolCheck {
        name: "oscdimg",
        version_args: &[],
    },
    ToolCheck {
        name: "curl",
        version_args: &["--version"],
    },
];

pub fn run_doctor() -> Result<(), Error> {
    info!("running wincrab doctor checks");

    for tool in TOOLS {
        let result = Command::new(tool.name).args(tool.version_args).output();

        match result {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let first_line = stdout.lines().next().unwrap_or("(no version info)");
                info!(tool = tool.name, version = first_line, "found");
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                warn!(tool = tool.name, "not found on PATH");
            }
            Err(e) => {
                warn!(tool = tool.name, error = %e, "failed to check");
            }
        }
    }

    check_disk_space()?;

    info!("doctor checks complete");
    Ok(())
}

fn check_disk_space() -> Result<(), Error> {
    // `df` is not available everywhere (e.g. Windows); doctor only warns.
    let Ok(output) = Command::new("df").arg("-BG").arg(".").output() else {
        warn!("could not determine available disk space (df unavailable)");
        return Ok(());
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let avail = stdout
        .lines()
        .nth(1)
        .and_then(|line| line.split_whitespace().nth(3))
        .and_then(|s| s.trim_end_matches('G').parse::<u64>().ok());

    match avail {
        Some(gb) if gb >= 15 => {
            info!(available_gb = gb, "disk space OK");
        }
        Some(gb) => {
            warn!(
                available_gb = gb,
                recommended_gb = 15,
                "low disk space — at least 15 GB recommended"
            );
        }
        None => {
            warn!("could not determine available disk space");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_list_has_expected_entries() {
        let names: Vec<&str> = TOOLS.iter().map(|t| t.name).collect();
        assert!(names.contains(&"7z"));
        assert!(names.contains(&"wimlib-imagex"));
        #[cfg(not(windows))]
        assert!(names.contains(&"hivexregedit"));
        #[cfg(not(windows))]
        assert!(names.contains(&"genisoimage"));
        #[cfg(target_os = "linux")]
        for name in [
            "sgdisk",
            "partprobe",
            "mkfs.fat",
            "mkfs.ntfs",
            "ntfs-3g",
            "bcd-sys",
        ] {
            assert!(names.contains(&name));
        }
        #[cfg(not(target_os = "linux"))]
        for name in [
            "sgdisk",
            "partprobe",
            "mkfs.fat",
            "mkfs.ntfs",
            "ntfs-3g",
            "bcd-sys",
        ] {
            assert!(!names.contains(&name));
        }
        #[cfg(windows)]
        for name in ["dism", "reg", "oscdimg"] {
            assert!(names.contains(&name));
        }
        assert!(names.contains(&"curl"));
    }

    #[test]
    fn run_doctor_does_not_error() {
        // Doctor should always return Ok even if tools are missing.
        let result = run_doctor();
        assert!(result.is_ok());
    }
}

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
    ToolCheck {
        name: "hivexsh",
        version_args: &["--version"],
    },
    ToolCheck {
        name: "genisoimage",
        version_args: &["--version"],
    },
    ToolCheck {
        name: "curl",
        version_args: &["--version"],
    },
];

pub fn run_doctor() -> Result<(), Error> {
    info!("running wincrab doctor checks");

    for tool in TOOLS {
        let result = Command::new(tool.name)
            .args(tool.version_args)
            .output();

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
    let output = Command::new("df")
        .arg("-BG")
        .arg(".")
        .output()
        .map_err(|e| Error::Io {
            context: "running df to check disk space".into(),
            source: e,
        })?;

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
        assert!(names.contains(&"hivexsh"));
        assert!(names.contains(&"genisoimage"));
        assert!(names.contains(&"curl"));
    }

    #[test]
    fn tools_list_count() {
        assert_eq!(TOOLS.len(), 5);
    }

    #[test]
    fn run_doctor_does_not_error() {
        // Doctor should always return Ok even if tools are missing.
        let result = run_doctor();
        assert!(result.is_ok());
    }
}

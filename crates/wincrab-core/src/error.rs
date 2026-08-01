use std::path::PathBuf;

/// All errors produced by wincrab-core.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error ({context}): {source}")]
    Io {
        context: String,
        source: std::io::Error,
    },

    #[error("configuration error: {message}")]
    Config { message: String },

    #[error("external command `{command}` failed (exit {code}): {stderr}")]
    Command {
        command: String,
        code: i32,
        stderr: String,
    },

    #[error("external command `{command}` was killed by signal")]
    CommandSignaled { command: String },

    #[error("{tool} not found on PATH — please install it")]
    ToolNotFound { tool: String },

    #[error("WIM file not found at {path}")]
    WimNotFound { path: PathBuf },

    #[error("registry hive not found at {path}")]
    HiveNotFound { path: PathBuf },

    #[error("EFI boot image not found at {noprompt_path} or {fallback_path}")]
    EfiBootImageNotFound {
        noprompt_path: PathBuf,
        fallback_path: PathBuf,
    },
}

// ---------------------------------------------------------------------------
// Filesystem helpers — thin wrappers that attach path context to I/O errors
// ---------------------------------------------------------------------------

/// Read a file's size in bytes, returning 0 and logging a warning on failure.
pub(crate) fn file_size_or_zero(path: &std::path::Path) -> u64 {
    match std::fs::metadata(path) {
        Ok(m) => m.len(),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "could not read file metadata");
            0
        }
    }
}

/// Create a directory and all parent directories.
pub(crate) fn ensure_dir(path: &std::path::Path) -> Result<(), Error> {
    std::fs::create_dir_all(path).map_err(|e| Error::Io {
        context: format!("creating {}", path.display()),
        source: e,
    })
}

/// Write `data` to `path`.
pub(crate) fn write_file(path: &std::path::Path, data: impl AsRef<[u8]>) -> Result<(), Error> {
    std::fs::write(path, data).map_err(|e| Error::Io {
        context: format!("writing {}", path.display()),
        source: e,
    })
}

/// Remove a single file.
pub(crate) fn remove_file(path: &std::path::Path) -> Result<(), Error> {
    std::fs::remove_file(path).map_err(|e| Error::Io {
        context: format!("removing {}", path.display()),
        source: e,
    })
}

/// Remove a directory tree.
pub(crate) fn remove_dir_all(path: &std::path::Path) -> Result<(), Error> {
    std::fs::remove_dir_all(path).map_err(|e| Error::Io {
        context: format!("removing {}", path.display()),
        source: e,
    })
}

/// Read a file to `String`.
pub(crate) fn read_file_string(path: &std::path::Path) -> Result<String, Error> {
    std::fs::read_to_string(path).map_err(|e| Error::Io {
        context: format!("reading {}", path.display()),
        source: e,
    })
}

/// Copy a file using buffered read+write instead of `std::fs::copy`.
///
/// `std::fs::copy` on Linux uses `copy_file_range`/`sendfile` which FUSE
/// filesystems like wimlib may not support (ENOTSUP).  This helper streams
/// through a buffer, which always works and avoids loading entire files into
/// memory.
pub(crate) fn copy_file(src: &std::path::Path, dest: &std::path::Path) -> Result<(), Error> {
    use std::io::{BufReader, BufWriter, Read, Write};

    let src_file = std::fs::File::open(src).map_err(|e| Error::Io {
        context: format!("reading {}", src.display()),
        source: e,
    })?;
    let dest_file = std::fs::File::create(dest).map_err(|e| Error::Io {
        context: format!("writing {}", dest.display()),
        source: e,
    })?;

    let mut reader = BufReader::with_capacity(64 * 1024, src_file);
    let mut writer = BufWriter::with_capacity(64 * 1024, dest_file);
    let mut buf = [0u8; 64 * 1024];

    loop {
        let n = reader.read(&mut buf).map_err(|e| Error::Io {
            context: format!("reading {}", src.display()),
            source: e,
        })?;
        if n == 0 {
            break;
        }
        writer.write_all(&buf[..n]).map_err(|e| Error::Io {
            context: format!("writing {}", dest.display()),
            source: e,
        })?;
    }
    writer.flush().map_err(|e| Error::Io {
        context: format!("flushing {}", dest.display()),
        source: e,
    })?;
    Ok(())
}

/// Run a `std::process::Command`, returning a structured error on failure.
pub(crate) fn run_cmd(cmd: &mut std::process::Command) -> Result<std::process::Output, Error> {
    let program = format!("{:?}", cmd.get_program());

    // Match directly instead of `map_err` to move `program` without cloning.
    let output = match cmd.output() {
        Ok(output) => output,
        Err(e) => {
            return Err(if e.kind() == std::io::ErrorKind::NotFound {
                Error::ToolNotFound { tool: program }
            } else {
                Error::Io {
                    context: format!("spawning {program}"),
                    source: e,
                }
            });
        }
    };

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        if code == -1 {
            return Err(Error::CommandSignaled { command: program });
        }
        return Err(Error::Command {
            command: program,
            code,
            // Try zero-copy conversion; fall back to lossy replacement only
            // when stderr contains invalid UTF-8.
            stderr: String::from_utf8(output.stderr)
                .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()),
        });
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // Error Display
    // -----------------------------------------------------------------------

    #[test]
    fn io_error_display() {
        let err = Error::Io {
            context: "reading file".into(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "gone"),
        };
        let msg = format!("{err}");
        assert!(msg.contains("reading file"));
        assert!(msg.contains("gone"));
    }

    #[test]
    fn config_error_display() {
        let err = Error::Config {
            message: "bad value".into(),
        };
        assert!(format!("{err}").contains("bad value"));
    }

    #[test]
    fn command_error_display() {
        let err = Error::Command {
            command: "7z".into(),
            code: 2,
            stderr: "not found".into(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("7z"));
        assert!(msg.contains("2"));
        assert!(msg.contains("not found"));
    }

    #[test]
    fn command_signaled_display() {
        let err = Error::CommandSignaled {
            command: "ffmpeg".into(),
        };
        assert!(format!("{err}").contains("signal"));
    }

    #[test]
    fn tool_not_found_display() {
        let err = Error::ToolNotFound {
            tool: "hivexsh".into(),
        };
        let msg = format!("{err}");
        assert!(msg.contains("hivexsh"));
        assert!(msg.contains("not found"));
    }

    #[test]
    fn wim_not_found_display() {
        let err = Error::WimNotFound {
            path: "/mnt/install.wim".into(),
        };
        assert!(format!("{err}").contains("install.wim"));
    }

    #[test]
    fn hive_not_found_display() {
        let err = Error::HiveNotFound {
            path: "/mnt/SOFTWARE".into(),
        };
        assert!(format!("{err}").contains("SOFTWARE"));
    }

    // -----------------------------------------------------------------------
    // copy_file
    // -----------------------------------------------------------------------

    #[test]
    fn copy_file_success() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.txt");
        let dest = dir.path().join("dest.txt");
        std::fs::write(&src, b"hello world").unwrap();

        copy_file(&src, &dest).unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "hello world");
    }

    #[test]
    fn copy_file_preserves_binary_data() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("bin.dat");
        let dest = dir.path().join("bin_copy.dat");
        let data: Vec<u8> = (0..=255).collect();
        std::fs::write(&src, &data).unwrap();

        copy_file(&src, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
    }

    #[test]
    fn copy_file_empty_file() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("empty");
        let dest = dir.path().join("empty_copy");
        std::fs::write(&src, b"").unwrap();

        copy_file(&src, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap().len(), 0);
    }

    #[test]
    fn copy_file_src_missing_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let result = copy_file(&dir.path().join("nonexistent"), &dir.path().join("dest"));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Io { .. }));
    }

    #[test]
    fn copy_file_dest_dir_missing_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.txt");
        std::fs::write(&src, b"data").unwrap();
        let result = copy_file(&src, &dir.path().join("no_such_dir").join("dest.txt"));
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // run_cmd
    // -----------------------------------------------------------------------

    #[test]
    fn run_cmd_success() {
        let output = run_cmd(&mut std::process::Command::new("true")).unwrap();
        assert!(output.status.success());
    }

    #[test]
    fn run_cmd_not_found_returns_tool_not_found() {
        let result = run_cmd(&mut std::process::Command::new(
            "this_command_does_not_exist_xyz",
        ));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::ToolNotFound { .. }));
    }

    #[test]
    fn run_cmd_failure_returns_command_error() {
        let result = run_cmd(&mut std::process::Command::new("false"));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Command { .. }));
    }

    #[test]
    fn run_cmd_captures_stderr() {
        let result = run_cmd(
            std::process::Command::new("sh")
                .arg("-c")
                .arg("echo 'oops' >&2; exit 1"),
        );
        match result.unwrap_err() {
            Error::Command { stderr, .. } => assert!(stderr.contains("oops")),
            other => panic!("expected Command error, got: {other:?}"),
        }
    }

    #[test]
    fn run_cmd_captures_stdout() {
        let output = run_cmd(std::process::Command::new("echo").arg("hello")).unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("hello"));
    }

    // -----------------------------------------------------------------------
    // Error is Debug
    // -----------------------------------------------------------------------

    #[test]
    fn error_is_debug() {
        let err = Error::Config {
            message: "test".into(),
        };
        let _ = format!("{err:?}");
    }

    // -----------------------------------------------------------------------
    // Error source chaining
    // -----------------------------------------------------------------------

    #[test]
    fn io_error_has_source() {
        let err = Error::Io {
            context: "test".into(),
            source: std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied"),
        };
        let _: &dyn std::error::Error = &err;
        assert!(std::error::Error::source(&err).is_some());
    }

    #[test]
    fn command_error_has_no_source() {
        let err = Error::Command {
            command: "cmd".into(),
            code: 1,
            stderr: "err".into(),
        };
        assert!(std::error::Error::source(&err).is_none());
    }

    // -----------------------------------------------------------------------
    // copy_file edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn copy_file_large_data() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("large.bin");
        let dest = dir.path().join("large_copy.bin");
        // 1 MB of data.
        let data: Vec<u8> = (0..1_000_000).map(|i| (i % 256) as u8).collect();
        std::fs::write(&src, &data).unwrap();

        copy_file(&src, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
    }

    #[test]
    fn copy_file_overwrites_existing() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.txt");
        let dest = dir.path().join("dest.txt");
        std::fs::write(&src, b"new content").unwrap();
        std::fs::write(&dest, b"old content").unwrap();

        copy_file(&src, &dest).unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "new content");
    }

    // -----------------------------------------------------------------------
    // run_cmd edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn run_cmd_exit_code_in_error() {
        let result = run_cmd(std::process::Command::new("sh").arg("-c").arg("exit 42"));
        match result.unwrap_err() {
            Error::Command { code, .. } => assert_eq!(code, 42),
            other => panic!("expected Command, got: {other:?}"),
        }
    }

    #[test]
    fn run_cmd_with_arguments() {
        let output = run_cmd(std::process::Command::new("echo").arg("hello").arg("world")).unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("hello world"));
    }
}

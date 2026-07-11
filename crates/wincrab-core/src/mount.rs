use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};
use std::process::Command;

use tracing::{error, info, warn};

use crate::config::WimIndex;
use crate::error::{ensure_dir, run_cmd, Error};

/// RAII guard for a FUSE-mounted WIM image.
///
/// Uses a compile-time typestate pattern instead of runtime `AtomicBool`:
/// - If a `WimMount` value exists, the image is mounted.
/// - [`unmount_commit`](WimMount::unmount_commit) consumes `self` via
///   `ManuallyDrop`, preventing `Drop` from firing (no double-unmount).
/// - If dropped without an explicit commit (early `?` return or panic
///   unwinding), `Drop` unmounts without `--commit`, discarding changes.
///
/// This eliminates atomic operations and makes the state machine zero-cost.
#[derive(Debug)]
pub struct WimMount {
    mount_dir: PathBuf,
}

impl WimMount {
    /// Mount the WIM image at `wim_path` (index `index`) read-write onto
    /// `mount_dir` using `wimlib-imagex mountrw`.
    pub fn mount(wim_path: &Path, mount_dir: &Path, index: WimIndex) -> Result<Self, Error> {
        if !wim_path.exists() {
            return Err(Error::WimNotFound {
                path: wim_path.to_path_buf(),
            });
        }

        ensure_dir(mount_dir)?;

        info!(
            wim = %wim_path.display(),
            mount = %mount_dir.display(),
            index = %index,
            "mounting WIM read-write via wimlib-imagex"
        );

        run_cmd(
            Command::new("wimlib-imagex")
                .arg("mountrw")
                .arg(wim_path)
                .arg(index.to_string())
                .arg(mount_dir),
        )?;

        info!("WIM mounted successfully");

        Ok(Self {
            mount_dir: mount_dir.to_path_buf(),
        })
    }

    /// The directory where the WIM image is mounted.
    pub fn mount_dir(&self) -> &Path {
        &self.mount_dir
    }

    /// Explicitly unmount **and commit** changes back into the WIM.
    ///
    /// This is the happy-path call. Consumes `self` via `ManuallyDrop` so
    /// that `Drop` does not fire — preventing a double-unmount without any
    /// runtime state flag.
    pub fn unmount_commit(self) -> Result<(), Error> {
        // Wrap in ManuallyDrop to suppress Drop (which would unmount without
        // commit). This is the typestate transition: Mounted -> Committed.
        let me = ManuallyDrop::new(self);

        info!(
            mount = %me.mount_dir.display(),
            "unmounting WIM with --commit"
        );

        run_cmd(
            Command::new("wimlib-imagex")
                .arg("unmount")
                .arg("--commit")
                .arg(&me.mount_dir),
        )?;

        info!("WIM unmounted and committed successfully");
        Ok(())
    }

}

/// Unmount-without-commit logic used by `Drop`.
fn unmount_no_commit(mount_dir: &Path) {
    warn!(
        mount = %mount_dir.display(),
        "unmounting WIM WITHOUT commit (discarding changes)"
    );

    let result = run_cmd(
        Command::new("wimlib-imagex")
            .arg("unmount")
            .arg(mount_dir),
    );

    if let Err(e) = result {
        error!(%e, "failed to unmount WIM during discard — mount may be stuck");
    }
}

impl Drop for WimMount {
    fn drop(&mut self) {
        // If we reach Drop, no explicit unmount was called — this means
        // either an early return via `?` or a panic unwind. Unmount
        // without commit to avoid corrupting the image.
        unmount_no_commit(&self.mount_dir);
    }
}

/// Install a global panic hook that logs a warning about potential stuck mounts.
/// Call this once at program startup.
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        error!("PANIC detected — WimMount Drop will attempt cleanup");
        default_hook(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WimIndex;

    // -----------------------------------------------------------------------
    // WimMount::mount — error paths (no external tool needed)
    // -----------------------------------------------------------------------

    #[test]
    fn mount_nonexistent_wim_returns_wim_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let result = WimMount::mount(
            &dir.path().join("nonexistent.wim"),
            &dir.path().join("mount"),
            WimIndex(6),
        );
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::WimNotFound { .. }));
    }

    #[test]
    fn mount_nonexistent_wim_preserves_path_in_error() {
        let dir = tempfile::tempdir().unwrap();
        let wim_path = dir.path().join("missing.wim");
        let result = WimMount::mount(&wim_path, &dir.path().join("mount"), WimIndex(1));
        match result.unwrap_err() {
            Error::WimNotFound { path } => assert_eq!(path, wim_path),
            other => panic!("expected WimNotFound, got: {other:?}"),
        }
    }

    #[test]
    fn mount_creates_mount_dir_before_failing() {
        // Even though wimlib-imagex isn't available, mount_dir should be created
        // before attempting the external command.
        let dir = tempfile::tempdir().unwrap();
        let wim_path = dir.path().join("test.wim");
        std::fs::write(&wim_path, b"fake wim").unwrap();
        let mount_dir = dir.path().join("deep").join("nested").join("mount");

        let result = WimMount::mount(&wim_path, &mount_dir, WimIndex(1));
        // Will fail because wimlib-imagex isn't available, but mount_dir should exist.
        assert!(result.is_err());
        assert!(mount_dir.is_dir(), "mount dir should have been created");
    }

    #[test]
    fn mount_with_existing_wim_fails_on_missing_tool() {
        let dir = tempfile::tempdir().unwrap();
        let wim_path = dir.path().join("install.wim");
        std::fs::write(&wim_path, b"not a real WIM").unwrap();

        let result = WimMount::mount(&wim_path, &dir.path().join("mount"), WimIndex(6));
        assert!(result.is_err());
        // Should fail with either ToolNotFound or Command error.
        match result.unwrap_err() {
            Error::ToolNotFound { .. } | Error::Command { .. } => (),
            other => panic!("expected ToolNotFound or Command, got: {other:?}"),
        }
    }

    #[test]
    fn mount_with_index_zero() {
        let dir = tempfile::tempdir().unwrap();
        let wim_path = dir.path().join("install.wim");
        std::fs::write(&wim_path, b"fake").unwrap();

        let result = WimMount::mount(&wim_path, &dir.path().join("mount"), WimIndex(0));
        // Should get past the WimNotFound check but fail on wimlib-imagex.
        assert!(result.is_err());
        assert!(!matches!(result.unwrap_err(), Error::WimNotFound { .. }));
    }

    #[test]
    fn mount_with_index_max() {
        let dir = tempfile::tempdir().unwrap();
        let wim_path = dir.path().join("install.wim");
        std::fs::write(&wim_path, b"fake").unwrap();

        let result = WimMount::mount(&wim_path, &dir.path().join("mount"), WimIndex(u32::MAX));
        assert!(result.is_err());
        assert!(!matches!(result.unwrap_err(), Error::WimNotFound { .. }));
    }

    // -----------------------------------------------------------------------
    // Accessor methods
    // -----------------------------------------------------------------------

    // We can't construct a WimMount without a real mount, but we can test
    // the accessor contracts by verifying mount() passes the right paths
    // through to the error.

    #[test]
    fn wim_not_found_error_contains_exact_path() {
        let path = PathBuf::from("/some/unlikely/path/install.wim");
        let result = WimMount::mount(&path, Path::new("/tmp/mount"), WimIndex(6));
        match result.unwrap_err() {
            Error::WimNotFound { path: p } => {
                assert_eq!(p, path);
            }
            other => panic!("expected WimNotFound, got: {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // Drop safety
    // -----------------------------------------------------------------------

    #[test]
    fn drop_on_nonexistent_mount_does_not_panic() {
        // If WimMount were somehow created with mounted=false, drop should be safe.
        // We can't construct WimMount directly (fields are private), but we
        // can verify that failed mount attempts don't leave dangling state.
        let dir = tempfile::tempdir().unwrap();
        let wim_path = dir.path().join("install.wim");
        // Don't create the file -- mount should return Err, nothing to drop.
        let result = WimMount::mount(&wim_path, &dir.path().join("m"), WimIndex(1));
        assert!(result.is_err());
        // No WimMount was created, so no Drop runs. This is a smoke test
        // that the error path doesn't leave a half-constructed guard.
    }

    // -----------------------------------------------------------------------
    // install_panic_hook
    // -----------------------------------------------------------------------

    // NOTE: We don't test install_panic_hook in unit tests because it
    // replaces the global panic hook, which interferes with the test harness.
    // It is implicitly tested by the CLI binary integration.
}

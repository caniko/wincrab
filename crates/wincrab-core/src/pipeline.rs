use std::path::{Path, PathBuf};

use tracing::{info, warn};

use crate::config::Config;
use crate::debloat::{prune_appx_packages, prune_seelen_replacements, remove_scheduled_tasks};
use crate::drivers::{download_drivers, inject_drivers};
use crate::edition::convert_edition;
use crate::error::{ensure_dir, file_size_or_zero, Error};
use crate::esd::convert_esd_to_wim;
use crate::extract::{extract_iso, find_install_image};
use crate::hooks::run_hook;
use crate::hosts::inject_telemetry_hosts;
use crate::manifest::{compute_sha256, write_manifest, BuildManifest};
use crate::mount::WimMount;
use crate::oobe::inject_autounattend;
use crate::performance::inject_performance_script;
use crate::recompress::recompress_wim;
use crate::registry::apply_registry_edits;
use crate::repack::repack_iso;
use crate::seelen::{download_seelen, inject_seelen};

/// Directories used throughout the pipeline. All created under a single
/// work directory so cleanup is a single `rm -rf`.
pub struct WorkDirs {
    /// Top-level temporary work area.
    pub root: PathBuf,
    /// Extracted ISO contents.
    pub staging: PathBuf,
    /// FUSE mount point for the WIM image.
    pub wim_mount: PathBuf,
}

impl WorkDirs {
    pub fn new(work_dir: &Path) -> Result<Self, Error> {
        let root = work_dir.to_path_buf();
        let staging = root.join("staging");
        let wim_mount = root.join("wim_mount");

        for dir in [&root, &staging, &wim_mount] {
            ensure_dir(dir)?;
        }

        Ok(Self {
            root,
            staging,
            wim_mount,
        })
    }
}

/// Count the total number of pipeline phases for progress reporting.
fn count_phases(config: &Config) -> usize {
    let mut n = 9; // extract, mount, prune apps, remove tasks, registry, commit, autounattend, recompress, repack

    if config.seelen.bundle {
        n += 2; // download + inject
    }
    if config.drivers.any_enabled() {
        n += 2; // download + inject
    }
    if config.telemetry.block_telemetry_hosts || !config.telemetry.extra_blocked_hosts.is_empty() {
        n += 1; // hosts file
    }
    if config.performance.high_perf_power_plan {
        n += 1; // performance script
    }
    if config.oobe.convert_edition.is_some() {
        n += 1; // edition conversion
    }
    if !config.inject.files.is_empty() {
        n += 1; // file injection
    }

    n
}

/// Run the full debloat pipeline.
pub fn run(
    config: &Config,
    iso_path: &Path,
    output_iso: &Path,
    work_dir: &Path,
) -> Result<(), Error> {
    let pipeline_start = std::time::Instant::now();
    let dirs = WorkDirs::new(work_dir)?;
    let total_phases = count_phases(config);

    let mut phase = 0;
    let mut next_phase = |label: &str| {
        phase += 1;
        info!("=== Phase {phase}/{total_phases}: {label} ===");
    };

    if config.seelen.bundle {
        warn!("Seelen-UI is enabled -- Edge/WebView2 will NOT be removed (Seelen depends on it)");
    }

    // --- Hook: pre_extract ---
    run_hook(
        "pre_extract",
        &config.hooks.pre_extract,
        &[
            ("WINCRAB_ISO", iso_path.to_string_lossy().as_ref()),
            ("WINCRAB_WORK_DIR", work_dir.to_string_lossy().as_ref()),
        ],
    )?;

    // --- Extract ISO ---
    next_phase("Extracting ISO");
    extract_iso(iso_path, &dirs.staging)?;

    // --- ESD to WIM conversion (if needed) ---
    let mut wim_path = find_install_image(&dirs.staging)?;
    if wim_path.extension().is_some_and(|ext| ext == "esd") {
        info!("source image is ESD format -- converting to WIM");
        wim_path = convert_esd_to_wim(&wim_path, config.wim_index)?;
    }

    // --- Download Seelen-UI (if enabled) ---
    let seelen_setup = if config.seelen.bundle {
        next_phase("Downloading Seelen-UI");
        download_seelen(&dirs.root, &config.seelen)?
    } else {
        None
    };

    // --- Download filesystem drivers (if enabled) ---
    let driver_paths = if config.drivers.any_enabled() {
        next_phase("Downloading filesystem drivers");
        download_drivers(&dirs.root, &config.drivers)?
    } else {
        None
    };

    // --- Mount WIM ---
    next_phase("Mounting WIM");
    let wim = WimMount::mount(&wim_path, &dirs.wim_mount, config.wim_index)?;

    // --- Prune apps ---
    next_phase("Pruning apps");
    let app_stats = prune_appx_packages(wim.mount_dir(), &config.apps)?;
    info!(
        dirs = app_stats.dirs_removed,
        files = app_stats.files_removed,
        "app pruning summary"
    );

    // --- Prune Seelen-replaced components ---
    if config.seelen.bundle {
        next_phase("Removing Seelen-replaced components");
        let seelen_stats = prune_seelen_replacements(wim.mount_dir(), &config.seelen)?;
        info!(
            dirs = seelen_stats.dirs_removed,
            files = seelen_stats.files_removed,
            "Seelen replacement pruning summary"
        );
    }

    // --- Remove scheduled tasks ---
    next_phase("Removing scheduled tasks");
    let task_stats = remove_scheduled_tasks(wim.mount_dir(), &config.scheduled_tasks)?;
    info!(
        dirs = task_stats.dirs_removed,
        files = task_stats.files_removed,
        "scheduled task cleanup summary"
    );

    // --- Hook: post_debloat ---
    run_hook(
        "post_debloat",
        &config.hooks.post_debloat,
        &[("WINCRAB_MOUNT", wim.mount_dir().to_string_lossy().as_ref())],
    )?;

    // --- Inject Seelen-UI into WIM ---
    if let Some(ref setup_path) = seelen_setup {
        next_phase("Injecting Seelen-UI into WIM");
        inject_seelen(wim.mount_dir(), setup_path, &config.seelen)?;
    }

    // --- Inject filesystem drivers into WIM ---
    if let Some(ref dp) = driver_paths {
        next_phase("Injecting filesystem drivers into WIM");
        inject_drivers(wim.mount_dir(), dp, &config.drivers)?;
    }

    // --- Block telemetry hosts ---
    if config.telemetry.block_telemetry_hosts || !config.telemetry.extra_blocked_hosts.is_empty() {
        next_phase("Blocking telemetry hosts");
        inject_telemetry_hosts(wim.mount_dir(), &config.telemetry)?;
    }

    // --- Performance script ---
    if config.performance.high_perf_power_plan {
        next_phase("Injecting performance script");
        inject_performance_script(wim.mount_dir(), &config.performance)?;
    }

    // --- Custom file injection ---
    if !config.inject.files.is_empty() {
        next_phase("Injecting custom files");
        inject_custom_files(wim.mount_dir(), config)?;
    }

    // --- Registry edits ---
    next_phase("Applying registry edits");
    apply_registry_edits(wim.mount_dir(), config)?;

    // --- Hook: pre_repack ---
    run_hook(
        "pre_repack",
        &config.hooks.pre_repack,
        &[("WINCRAB_MOUNT", wim.mount_dir().to_string_lossy().as_ref())],
    )?;

    // --- Commit WIM ---
    next_phase("Committing WIM");
    wim.unmount_commit()?;

    // --- Edition conversion (post-commit, modifies WIM metadata + staging) ---
    if let Some(ref target_edition) = config.oobe.convert_edition {
        next_phase("Converting Windows edition");
        convert_edition(&dirs.staging, &wim_path, config.wim_index, target_edition)?;
    }

    // --- Inject autounattend.xml ---
    next_phase("Injecting autounattend.xml");
    inject_autounattend(&dirs.staging, &config.oobe)?;

    // --- Recompress + Repack ---
    next_phase("Recompressing WIM and repacking ISO");
    recompress_wim(&wim_path)?;
    repack_iso(&dirs.staging, output_iso)?;

    // --- Hook: post_build ---
    run_hook(
        "post_build",
        &config.hooks.post_build,
        &[("WINCRAB_OUTPUT", output_iso.to_string_lossy().as_ref())],
    )?;

    // --- Build manifest ---
    let source_size = file_size_or_zero(iso_path);
    let output_size = file_size_or_zero(output_iso);
    let manifest = BuildManifest {
        wincrab_version: std::borrow::Cow::Borrowed(env!("CARGO_PKG_VERSION")),
        source_iso_sha256: compute_sha256(iso_path)?,
        output_iso_sha256: compute_sha256(output_iso)?,
        source_iso_size_bytes: source_size,
        output_iso_size_bytes: output_size,
        config_snapshot: serde_json::to_string_pretty(config).unwrap_or_else(|e| {
            warn!(error = %e, "failed to serialize config for manifest");
            "(serialization failed)".into()
        }),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_else(|e| {
                warn!(error = %e, "failed to read system time for manifest");
                "0".into()
            }),
    };
    let manifest_path = output_iso.with_extension("manifest.json");
    write_manifest(&manifest, &manifest_path)?;
    info!(path = %manifest_path.display(), "wrote build manifest");

    let elapsed = pipeline_start.elapsed();
    let saved = source_size.saturating_sub(output_size);

    info!(
        output = %output_iso.display(),
        elapsed_secs = elapsed.as_secs(),
        source_mb = source_size / (1024 * 1024),
        output_mb = output_size / (1024 * 1024),
        saved_mb = saved / (1024 * 1024),
        "debloated ISO is ready"
    );
    Ok(())
}

/// Copy custom files from host into the mounted WIM.
fn inject_custom_files(mount_dir: &Path, config: &Config) -> Result<(), Error> {
    for entry in &config.inject.files {
        let dest = mount_dir.join(&entry.dest);

        // Prevent path traversal — reject dest paths containing ".." components
        // which could escape the mount point (e.g. "../../etc/passwd").
        let dest_path = Path::new(&entry.dest);
        if dest_path.is_absolute()
            || dest_path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            return Err(Error::Config {
                message: format!(
                    "inject dest '{}' must be a relative path without '..' components",
                    entry.dest,
                ),
            });
        }

        if let Some(parent) = dest.parent() {
            ensure_dir(parent)?;
        }

        if entry.src.is_dir() {
            copy_dir_recursive(&entry.src, &dest)?;
        } else {
            crate::error::copy_file(&entry.src, &dest)?;
        }
        info!(
            src = %entry.src.display(),
            dest = %entry.dest,
            "injected custom file"
        );
    }
    Ok(())
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<(), Error> {
    ensure_dir(dest)?;

    for entry in walkdir::WalkDir::new(src).min_depth(1) {
        let entry = entry.map_err(|e| Error::Io {
            context: format!("walking {}", src.display()),
            source: e.into_io_error().unwrap_or_else(|| {
                std::io::Error::other("walkdir error")
            }),
        })?;

        let relative = entry.path().strip_prefix(src).map_err(|_| Error::Io {
            context: format!(
                "path {} is not under {}",
                entry.path().display(),
                src.display()
            ),
            source: std::io::Error::other("strip_prefix failed"),
        })?;
        let target = dest.join(relative);

        if entry.file_type().is_dir() {
            ensure_dir(&target)?;
        } else {
            crate::error::copy_file(entry.path(), &target)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // WorkDirs
    // -----------------------------------------------------------------------

    #[test]
    fn work_dirs_creates_all_directories() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("pipeline_work");

        let dirs = WorkDirs::new(&work).unwrap();
        assert!(dirs.root.is_dir());
        assert!(dirs.staging.is_dir());
        assert!(dirs.wim_mount.is_dir());
        assert_eq!(dirs.root, work);
        assert_eq!(dirs.staging, work.join("staging"));
        assert_eq!(dirs.wim_mount, work.join("wim_mount"));
    }

    #[test]
    fn work_dirs_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("pipeline_work");

        let _dirs1 = WorkDirs::new(&work).unwrap();
        let dirs2 = WorkDirs::new(&work).unwrap();
        assert!(dirs2.root.is_dir());
    }

    #[test]
    fn work_dirs_nested_path() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("a").join("b").join("c");

        let dirs = WorkDirs::new(&work).unwrap();
        assert!(dirs.root.is_dir());
        assert!(dirs.staging.is_dir());
    }

    // -----------------------------------------------------------------------
    // Phase counting
    // -----------------------------------------------------------------------

    #[test]
    fn phase_count_no_extras() {
        let config = Config {
            seelen: crate::config::Seelen {
                bundle: false,
                ..Default::default()
            },
            telemetry: crate::config::Telemetry {
                block_telemetry_hosts: false,
                ..Default::default()
            },
            performance: crate::config::Performance {
                high_perf_power_plan: false,
                ..Default::default()
            },
            drivers: crate::config::Drivers::default(),
            ..Config::default()
        };
        assert_eq!(count_phases(&config), 9);
    }

    #[test]
    fn phase_count_with_seelen() {
        let config = Config {
            telemetry: crate::config::Telemetry {
                block_telemetry_hosts: false,
                ..Default::default()
            },
            performance: crate::config::Performance {
                high_perf_power_plan: false,
                ..Default::default()
            },
            ..Config::default()
        }; // seelen.bundle = true by default
        assert_eq!(count_phases(&config), 11);
    }

    #[test]
    fn phase_count_with_drivers() {
        let config = Config {
            seelen: crate::config::Seelen {
                bundle: false,
                ..Default::default()
            },
            telemetry: crate::config::Telemetry {
                block_telemetry_hosts: false,
                ..Default::default()
            },
            performance: crate::config::Performance {
                high_perf_power_plan: false,
                ..Default::default()
            },
            drivers: crate::config::Drivers {
                btrfs: true,
                ..Default::default()
            },
            ..Config::default()
        };
        assert_eq!(count_phases(&config), 11);
    }

    #[test]
    fn phase_count_all_features() {
        let config = Config {
            drivers: crate::config::Drivers {
                ext4: true,
                ..Default::default()
            },
            inject: crate::config::Inject {
                files: vec![crate::config::InjectEntry {
                    src: PathBuf::from("/tmp/test"),
                    dest: "test".into(),
                }],
            },
            ..Config::default()
        };
        // 9 base + 2 seelen + 2 drivers + 1 hosts + 1 perf + 1 inject = 16
        assert_eq!(count_phases(&config), 16);
    }

    // -----------------------------------------------------------------------
    // Custom file injection
    // -----------------------------------------------------------------------

    #[test]
    fn inject_custom_files_copies_file() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path().join("mount");
        std::fs::create_dir_all(&mount).unwrap();

        let src = dir.path().join("test.txt");
        std::fs::write(&src, "hello").unwrap();

        let config = Config {
            inject: crate::config::Inject {
                files: vec![crate::config::InjectEntry {
                    src: src.clone(),
                    dest: "Users/Public/Desktop/test.txt".into(),
                }],
            },
            ..Config::default()
        };

        inject_custom_files(&mount, &config).unwrap();
        let content =
            std::fs::read_to_string(mount.join("Users/Public/Desktop/test.txt")).unwrap();
        assert_eq!(content, "hello");
    }

    #[test]
    fn inject_custom_files_rejects_path_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path().join("mount");
        std::fs::create_dir_all(&mount).unwrap();

        let src = dir.path().join("evil.txt");
        std::fs::write(&src, "pwned").unwrap();

        let config = Config {
            inject: crate::config::Inject {
                files: vec![crate::config::InjectEntry {
                    src,
                    dest: "../../etc/passwd".into(),
                }],
            },
            ..Config::default()
        };

        let result = inject_custom_files(&mount, &config);
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Config { message } => assert!(message.contains("without '..' components")),
            other => panic!("expected Config error, got: {other:?}"),
        }
    }

    #[test]
    fn inject_custom_files_copies_directory() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path().join("mount");
        std::fs::create_dir_all(&mount).unwrap();

        let src_dir = dir.path().join("mydir");
        std::fs::create_dir_all(src_dir.join("sub")).unwrap();
        std::fs::write(src_dir.join("a.txt"), "file a").unwrap();
        std::fs::write(src_dir.join("sub/b.txt"), "file b").unwrap();

        let config = Config {
            inject: crate::config::Inject {
                files: vec![crate::config::InjectEntry {
                    src: src_dir.clone(),
                    dest: "Tools".into(),
                }],
            },
            ..Config::default()
        };

        inject_custom_files(&mount, &config).unwrap();
        assert_eq!(
            std::fs::read_to_string(mount.join("Tools/a.txt")).unwrap(),
            "file a"
        );
        assert_eq!(
            std::fs::read_to_string(mount.join("Tools/sub/b.txt")).unwrap(),
            "file b"
        );
    }
}

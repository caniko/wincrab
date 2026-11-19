//! Direct install of Windows onto a whole disk from a running Linux host.
//!
//! No Windows Setup or WinPE is involved: the disk is repartitioned, the WIM
//! image is applied straight to the NTFS partition with `wimlib-imagex`, and
//! the UEFI boot files plus BCD store are written by the external `bcd-sys`
//! tool.

use std::path::{Path, PathBuf};

use crate::config::WimIndex;
use crate::error::Error;

/// Options for [`run`].
pub struct InstallOptions {
    /// Windows ISO, or an `install.wim` / `install.esd` file.
    pub source: PathBuf,
    /// Whole target disk (e.g. `/dev/sdb`, `/dev/nvme1n1`).
    pub disk: PathBuf,
    /// The same disk, repeated as explicit confirmation of the wipe.
    pub confirm_wipe: PathBuf,
    /// WIM image index to apply.
    pub index: WimIndex,
    /// Optional `unattend.xml`, copied to `Windows/Panther/unattend.xml`.
    pub unattend: Option<PathBuf>,
    /// Work directory for the extracted ISO and mount points.
    pub work_dir: PathBuf,
}

/// Install Windows onto `opts.disk`, destroying everything on it.
pub fn run(opts: &InstallOptions) -> Result<(), Error> {
    #[cfg(target_os = "linux")]
    {
        linux::run(opts)
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = opts;
        Err(Error::Config {
            message: "wincrab install is only supported on Linux hosts".into(),
        })
    }
}

// ---------------------------------------------------------------------------
// Pure helpers (compiled on every host so they can be unit-tested anywhere)
// ---------------------------------------------------------------------------

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn device_numbers(dev: u64) -> (u64, u64) {
    (
        ((dev >> 8) & 0xfff) | ((dev >> 32) & !0xfff),
        (dev & 0xff) | ((dev >> 12) & !0xff),
    )
}

/// Parse the effective UID (second field) from `/proc/self/status` content.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn effective_uid(proc_status: &str) -> Option<u32> {
    let line = proc_status.lines().find(|line| line.starts_with("Uid:"))?;
    line.split_whitespace().nth(2)?.parse().ok()
}

/// Collect the non-blank lines of `lsblk -nrpo MOUNTPOINTS` output.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_lsblk_mountpoints(stdout: &str) -> Vec<String> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Compare two already-canonicalized paths (`--disk` vs `--confirm-wipe`).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn disks_match(disk: &Path, confirm: &Path) -> bool {
    disk == confirm
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
enum SourceKind {
    Iso,
    Wim,
}

/// Classify a source path by its (case-insensitive) extension.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn source_kind(path: &Path) -> Option<SourceKind> {
    match path.extension()?.to_str()?.to_ascii_lowercase().as_str() {
        "iso" => Some(SourceKind::Iso),
        "wim" | "esd" => Some(SourceKind::Wim),
        _ => None,
    }
}

/// The `Architecture:` value from `wimlib-imagex info <image> <index>` output.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn wim_architecture(info: &str) -> Option<&str> {
    info.lines()
        .find_map(|line| line.trim().strip_prefix("Architecture:"))
        .map(str::trim)
}

/// External tools `wincrab install` runs; all are checked before any write.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const REQUIRED_TOOLS: &[&str] = &[
    "lsblk",
    "sgdisk",
    "partprobe",
    "udevadm",
    "mkfs.fat",
    "mkfs.ntfs",
    "wimlib-imagex",
    "ntfs-3g",
    "mount",
    "umount",
    "sync",
    "bcd-sys",
];

// ---------------------------------------------------------------------------
// Linux implementation
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
mod linux {
    use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use tracing::{info, warn};

    use super::{
        InstallOptions, REQUIRED_TOOLS, SourceKind, device_numbers, disks_match, effective_uid,
        parse_lsblk_mountpoints, source_kind, wim_architecture,
    };
    use crate::error::{Error, copy_file, ensure_dir, remove_dir_all, run_cmd, write_file};

    /// How long to wait for the new partition device nodes to appear.
    const PARTITION_TIMEOUT: Duration = Duration::from_secs(15);
    const PARTITION_POLL: Duration = Duration::from_millis(250);

    pub(super) fn run(opts: &InstallOptions) -> Result<(), Error> {
        info!("checking install preconditions");
        let status = std::fs::read_to_string("/proc/self/status").map_err(|e| Error::Io {
            context: "reading /proc/self/status".into(),
            source: e,
        })?;
        let uid = effective_uid(&status).ok_or_else(|| Error::Config {
            message: "could not determine the effective UID from /proc/self/status".into(),
        })?;
        if uid != 0 {
            return Err(Error::Config {
                message: "wincrab install must run as root — re-run it with sudo".into(),
            });
        }
        if !opts.source.is_file() {
            return Err(Error::Config {
                message: format!(
                    "source {} does not exist or is not a file",
                    opts.source.display()
                ),
            });
        }
        if source_kind(&opts.source).is_none() {
            return Err(Error::Config {
                message: format!(
                    "source {} must be an .iso, .wim or .esd file",
                    opts.source.display()
                ),
            });
        }
        let missing: Vec<&str> = REQUIRED_TOOLS
            .iter()
            .copied()
            .chain((source_kind(&opts.source) == Some(SourceKind::Iso)).then_some("7z"))
            .filter(|tool| !on_path(tool))
            .collect();
        if !missing.is_empty() {
            return Err(Error::Config {
                message: format!("missing required tools: {}", missing.join(", ")),
            });
        }
        // Identify the disk now and again right before the first write, so a
        // device node reused during the long extraction is not wiped.
        let identity = preflight(opts)?;
        let unattend = opts
            .unattend
            .as_ref()
            .map(|path| {
                std::fs::read(path).map_err(|e| Error::Io {
                    context: format!("reading {}", path.display()),
                    source: e,
                })
            })
            .transpose()?;
        ensure_dir(&opts.work_dir)?;
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| Error::Config {
                message: format!("system clock before Unix epoch: {e}"),
            })?
            .as_nanos();
        let workspace = opts
            .work_dir
            .join(format!("install-{}-{nanos}", std::process::id()));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&workspace)
            .map_err(|e| Error::Io {
                context: format!("creating {}", workspace.display()),
                source: e,
            })?;
        info!("resolving Windows image");
        let image = resolve_image(opts, &workspace)?;
        let info = run_cmd(
            Command::new("wimlib-imagex")
                .arg("info")
                .arg(&image)
                .arg(opts.index.to_string()),
        )
        .map_err(|_| Error::Config {
            message: format!(
                "image index {} not found in {}",
                opts.index,
                image.display()
            ),
        })?;
        // The boot setup (bcd-sys -f uefi, EFI/Boot/bootx64.efi) is x64-only.
        let info = String::from_utf8_lossy(&info.stdout);
        if wim_architecture(&info) != Some("x86_64") {
            return Err(Error::Config {
                message: format!(
                    "image index {} is {:?}; only x86_64 images can be installed",
                    opts.index,
                    wim_architecture(&info).unwrap_or("of unknown architecture")
                ),
            });
        }

        let (disk, sys_disk, diskseq) = preflight(opts)?;
        if (&disk, &sys_disk, &diskseq) != (&identity.0, &identity.1, &identity.2) {
            return Err(Error::Config {
                message: format!(
                    "{} changed while the image was being prepared — refusing to wipe it",
                    disk.display()
                ),
            });
        }
        info!(disk = %disk.display(), "partitioning disk (this erases the disk)");
        partition(&disk)?;
        let esp = wait_for_partition(&sys_disk, 1)?;
        let windows = wait_for_partition(&sys_disk, 3)?;

        info!(partition = %esp.display(), "formatting EFI system partition");
        run_cmd(
            Command::new("mkfs.fat")
                .args(["-F", "32", "-n", "SYSTEM"])
                .arg(&esp),
        )?;

        info!(partition = %windows.display(), "formatting Windows partition");
        run_cmd(
            Command::new("mkfs.ntfs")
                .args(["--quick", "--label", "Windows"])
                .arg(&windows),
        )?;

        info!(image = %image.display(), index = %opts.index, "applying Windows image");
        run_cmd(
            Command::new("wimlib-imagex")
                .arg("apply")
                .arg(&image)
                .arg(opts.index.to_string())
                .arg(&windows),
        )?;

        let mnt_esp = workspace.join("mnt-esp");
        let mnt_windows = workspace.join("mnt-windows");
        ensure_dir(&mnt_esp)?;
        ensure_dir(&mnt_windows)?;

        info!(partition = %windows.display(), "mounting Windows partition");
        let windows_mount = MountGuard::mount(
            Command::new("ntfs-3g").arg(&windows).arg(&mnt_windows),
            &mnt_windows,
        )?;
        info!(partition = %esp.display(), "mounting EFI system partition");
        let esp_mount = MountGuard::mount(
            Command::new("mount")
                .args(["-t", "vfat"])
                .arg(&esp)
                .arg(&mnt_esp),
            &mnt_esp,
        )?;

        info!("installing UEFI boot files with bcd-sys");
        run_cmd(
            Command::new("bcd-sys")
                // bcd-sys parses English sfdisk/lsblk output.
                .env("LC_ALL", "C")
                .args(["-f", "uefi", "-s"])
                .arg(&mnt_esp)
                .arg(&mnt_windows),
        )?;

        let bcd = mnt_esp.join("EFI/Microsoft/Boot/BCD");
        if !bcd.exists() {
            return Err(Error::Config {
                message: format!("bcd-sys did not create the BCD store at {}", bcd.display()),
            });
        }

        let bootmgfw = mnt_esp.join("EFI/Microsoft/Boot/bootmgfw.efi");
        let fallback = mnt_esp.join("EFI/Boot/bootx64.efi");
        if !fallback.exists() {
            info!("adding removable-media fallback boot loader");
            ensure_dir(&mnt_esp.join("EFI/Boot"))?;
            copy_file(&bootmgfw, &fallback)?;
        }

        if let Some(unattend) = &unattend {
            let panther = mnt_windows.join("Windows/Panther");
            ensure_dir(&panther)?;
            write_file(&panther.join("unattend.xml"), unattend)?;
            info!("copied unattend.xml");
        }

        info!("flushing writes");
        run_cmd(&mut Command::new("sync"))?;

        info!("unmounting EFI system partition");
        esp_mount.unmount()?;
        info!("unmounting Windows partition");
        windows_mount.unmount()?;

        if workspace.join("source").exists() {
            remove_dir_all(&workspace.join("source"))?;
        }
        Ok(())
    }

    /// Destructive-safety gate. Nothing is written until all checks pass.
    /// Returns the canonical disk, its sysfs directory and its kernel
    /// `diskseq` (unique per attached disk; absent before Linux 5.15).
    fn preflight(opts: &InstallOptions) -> Result<(PathBuf, PathBuf, Option<String>), Error> {
        let disk = canonical_device(&opts.disk, "--disk")?;
        let meta = std::fs::metadata(&disk).map_err(|e| Error::Io {
            context: format!("inspecting {}", disk.display()),
            source: e,
        })?;
        if !meta.file_type().is_block_device() {
            return Err(Error::Config {
                message: format!("{} is not a block device", disk.display()),
            });
        }
        let (major, minor) = device_numbers(meta.rdev());
        let sys_disk = PathBuf::from(format!("/sys/dev/block/{major}:{minor}"));
        if !sys_disk.exists() {
            return Err(Error::Config {
                message: format!("no sysfs entry for {}", disk.display()),
            });
        }
        if sys_disk.join("partition").exists() {
            return Err(Error::Config {
                message: format!(
                    "{} is a partition, not a whole disk — pass the whole disk",
                    disk.display()
                ),
            });
        }

        let confirm = canonical_device(&opts.confirm_wipe, "--confirm-wipe")?;
        let confirm_meta = std::fs::metadata(&confirm).map_err(|e| Error::Io {
            context: format!("inspecting {}", confirm.display()),
            source: e,
        })?;
        if !disks_match(&disk, &confirm) || confirm_meta.rdev() != meta.rdev() {
            return Err(Error::Config {
                message: format!(
                    "--confirm-wipe ({}) does not match --disk ({})",
                    confirm.display(),
                    disk.display()
                ),
            });
        }

        let output = run_cmd(
            Command::new("lsblk")
                .arg("-nrpo")
                .arg("MOUNTPOINTS")
                .arg(&disk),
        )?;
        let mountpoints = parse_lsblk_mountpoints(&String::from_utf8_lossy(&output.stdout));
        if !mountpoints.is_empty() {
            return Err(Error::Config {
                message: format!(
                    "refusing to wipe {}: it is still mounted or in use at {}",
                    disk.display(),
                    mountpoints.join(", ")
                ),
            });
        }

        // Linux value on all architectures wincrab builds for.
        const O_EXCL: i32 = 0o200;
        let file = std::fs::OpenOptions::new().read(true).custom_flags(O_EXCL).open(&disk).map_err(|e| {
            if e.raw_os_error() == Some(16) {
                Error::Config { message: format!("refusing to wipe {}: it is in use (mounted, swap, LVM/RAID/btrfs member)", disk.display()) }
            } else {
                Error::Io { context: format!("opening {} exclusively", disk.display()), source: e }
            }
        })?;
        drop(file);
        let diskseq = std::fs::read_to_string(sys_disk.join("diskseq")).ok();
        Ok((disk, sys_disk, diskseq))
    }

    fn on_path(tool: &str) -> bool {
        std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path).any(|dir| {
                std::fs::metadata(dir.join(tool))
                    .is_ok_and(|meta| meta.is_file() && meta.mode() & 0o111 != 0)
            })
        })
    }

    /// Canonicalize a device path, reporting failures as a config error.
    fn canonical_device(path: &Path, flag: &str) -> Result<PathBuf, Error> {
        std::fs::canonicalize(path).map_err(|e| Error::Config {
            message: format!("{flag} {}: {e}", path.display()),
        })
    }

    /// GPT with a 260 MiB EFI system partition, a 16 MiB MSR and Windows on
    /// the rest of the disk.
    fn partition(disk: &Path) -> Result<(), Error> {
        run_cmd(Command::new("sgdisk").arg("--zap-all").arg(disk))?;
        run_cmd(
            Command::new("sgdisk")
                .args([
                    "-n",
                    "1:0:+260M",
                    "-t",
                    "1:EF00",
                    "-c",
                    "1:EFI",
                    "-n",
                    "2:0:+16M",
                    "-t",
                    "2:0C01",
                    "-c",
                    "2:MSR",
                    "-n",
                    "3:0:0",
                    "-t",
                    "3:0700",
                    "-c",
                    "3:Windows",
                ])
                .arg(disk),
        )?;

        run_cmd(Command::new("partprobe").arg(disk))?;
        if let Err(e) = run_cmd(Command::new("udevadm").arg("settle")) {
            warn!(%e, "udevadm settle failed — continuing");
        }

        Ok(())
    }

    fn wait_for_partition(sys_disk: &Path, n: u32) -> Result<PathBuf, Error> {
        let deadline = Instant::now() + PARTITION_TIMEOUT;
        loop {
            if let Ok(entries) = std::fs::read_dir(sys_disk) {
                for entry in entries.flatten() {
                    if std::fs::read_to_string(entry.path().join("partition"))
                        .is_ok_and(|value| value.trim().parse::<u32>() == Ok(n))
                    {
                        let path = Path::new("/dev").join(entry.file_name());
                        let dev = std::fs::read_to_string(entry.path().join("dev"));
                        if let (Ok(meta), Ok(dev)) = (std::fs::metadata(&path), dev) {
                            let (major, minor) = device_numbers(meta.rdev());
                            if !meta.file_type().is_block_device()
                                || dev.trim() != format!("{major}:{minor}")
                            {
                                return Err(Error::Config {
                                    message: format!(
                                        "{} is not the kernel's partition {n} ({})",
                                        path.display(),
                                        dev.trim()
                                    ),
                                });
                            }
                            return Ok(path);
                        }
                    }
                }
            }
            if Instant::now() >= deadline {
                return Err(Error::Config {
                    message: format!(
                        "timed out waiting for partition {n} of {}",
                        sys_disk.display()
                    ),
                });
            }
            std::thread::sleep(PARTITION_POLL);
        }
    }

    /// Extract the install image from an ISO, or use a WIM/ESD directly.
    fn resolve_image(opts: &InstallOptions, workspace: &Path) -> Result<PathBuf, Error> {
        match source_kind(&opts.source) {
            Some(SourceKind::Iso) => {
                let dir = workspace.join("source");
                ensure_dir(&dir)?;

                info!(
                    iso = %opts.source.display(),
                    dest = %dir.display(),
                    "extracting install image from ISO with 7z"
                );
                run_cmd(
                    Command::new("7z")
                        .arg("x")
                        .arg(format!("-o{}", dir.display()))
                        .arg("-aoa")
                        .arg(&opts.source)
                        .arg("sources/install.*"),
                )?;
                crate::extract::find_install_image(&dir)
            }
            Some(SourceKind::Wim) => Ok(opts.source.clone()),
            None => Err(Error::Config {
                message: format!(
                    "source {} must be an .iso, .wim or .esd file",
                    opts.source.display()
                ),
            }),
        }
    }

    /// RAII guard: unmounts on drop, logs failures and never panics.
    struct MountGuard {
        dir: PathBuf,
    }

    impl MountGuard {
        fn mount(cmd: &mut Command, dir: &Path) -> Result<Self, Error> {
            run_cmd(cmd)?;
            Ok(Self {
                dir: dir.to_path_buf(),
            })
        }

        /// Unmount explicitly, surfacing errors. Consumes the guard.
        fn unmount(self) -> Result<(), Error> {
            run_cmd(Command::new("umount").arg(&self.dir))?;
            std::mem::forget(self);
            Ok(())
        }
    }

    // ponytail: no signal handling; Ctrl-C can leave a mount behind.
    impl Drop for MountGuard {
        fn drop(&mut self) {
            if let Err(e) = run_cmd(Command::new("umount").arg(&self.dir)) {
                warn!(%e, dir = %self.dir.display(), "failed to unmount");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_device_numbers() {
        assert_eq!(device_numbers(0x810), (8, 16));
        assert_eq!(device_numbers(0x10301), (259, 1));
        assert_eq!(device_numbers(0x10082c), (8, 300));
    }

    #[test]
    fn parse_lsblk_accepts_empty_output() {
        assert!(parse_lsblk_mountpoints("").is_empty());
        assert!(parse_lsblk_mountpoints("\n\n   \n").is_empty());
    }

    #[test]
    fn parse_lsblk_reports_mounts_and_swap() {
        assert_eq!(parse_lsblk_mountpoints("\n/\n"), vec!["/".to_string()]);
        assert_eq!(
            parse_lsblk_mountpoints("[SWAP]\n"),
            vec!["[SWAP]".to_string()]
        );
        assert_eq!(
            parse_lsblk_mountpoints("  /mnt/data  \n\n[SWAP]\n"),
            vec!["/mnt/data".to_string(), "[SWAP]".to_string()]
        );
    }

    #[test]
    fn effective_uid_reads_second_field() {
        let status = "Name:\twincrab\nUid:\t1000\t1000\t1000\t1000\nGid:\t1000\t1000\t1000\t1000\n";
        assert_eq!(effective_uid(status), Some(1000));
        assert_eq!(effective_uid("Uid:\t0\t0\t0\t0\n"), Some(0));
        assert_eq!(effective_uid("Name:\twincrab\n"), None);
        assert_eq!(effective_uid("Uid:\tnotanumber\n"), None);
    }

    #[test]
    fn confirm_wipe_must_match_disk() {
        let disk = Path::new("/dev/sdb");
        assert!(disks_match(disk, Path::new("/dev/sdb")));
        assert!(!disks_match(disk, Path::new("/dev/sdc")));
    }

    #[test]
    fn wim_architecture_reads_info_output() {
        let info = "Index:                  6\nName:                   Windows 11 Pro\nArchitecture:           x86_64\n";
        assert_eq!(wim_architecture(info), Some("x86_64"));
        assert_eq!(wim_architecture("Architecture:   ARM64\n"), Some("ARM64"));
        assert_eq!(wim_architecture("Name: x\n"), None);
    }

    #[test]
    fn source_kind_accepts_iso_wim_esd() {
        assert_eq!(source_kind(Path::new("Win11.iso")), Some(SourceKind::Iso));
        assert_eq!(source_kind(Path::new("WIN11.ISO")), Some(SourceKind::Iso));
        assert_eq!(source_kind(Path::new("install.wim")), Some(SourceKind::Wim));
        assert_eq!(source_kind(Path::new("install.ESD")), Some(SourceKind::Wim));
        assert_eq!(source_kind(Path::new("install.txt")), None);
        assert_eq!(source_kind(Path::new("install")), None);
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn run_refuses_non_linux_hosts() {
        let opts = InstallOptions {
            source: PathBuf::from("install.wim"),
            disk: PathBuf::from("/dev/sdb"),
            confirm_wipe: PathBuf::from("/dev/sdb"),
            index: WimIndex(6),
            unattend: None,
            work_dir: PathBuf::from("wincrab-work"),
        };
        match run(&opts).unwrap_err() {
            Error::Config { message } => assert!(message.contains("only supported on Linux")),
            other => panic!("expected Config error, got: {other:?}"),
        }
    }
}

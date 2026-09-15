use std::path::{Path, PathBuf};

use smallvec::SmallVec;
use tracing::info;

use crate::config::Drivers;
use crate::error::{Error, copy_file, ensure_dir, write_file};
use crate::github::{self, SimpleAsset};

const BTRFS_API: &str = "https://api.github.com/repos/maharmstone/btrfs/releases/latest";
const EXT2FSD_API: &str = "https://api.github.com/repos/bobranten/Ext2Fsd/releases/latest";
const WINFSP_API: &str = "https://api.github.com/repos/winfsp/winfsp/releases/latest";
const MERGERFS_API: &str = "https://api.github.com/repos/trapexit/mergerfs-windows/releases/latest";

/// Paths to downloaded driver packages, populated by [`download_drivers`].
pub struct DriverPaths {
    pub btrfs: Option<PathBuf>,
    pub ext2fsd: Option<PathBuf>,
    pub winfsp: Option<PathBuf>,
    pub mergerfs: Option<PathBuf>,
}

/// Download enabled driver packages from GitHub releases.
///
/// Caches downloads in `work_dir/driver-cache/` so repeated runs skip the
/// network requests. Returns `None` if no drivers are enabled.
pub fn download_drivers(work_dir: &Path, config: &Drivers) -> Result<Option<DriverPaths>, Error> {
    if !config.any_enabled() {
        return Ok(None);
    }

    // Auto-enable winfsp when mergerfs is requested.
    let winfsp_needed = config.winfsp || config.mergerfs;

    let cache_dir = work_dir.join("driver-cache");

    let download = |asset: SimpleAsset| -> Result<PathBuf, Error> {
        github::download_asset(&asset, &cache_dir)
    };

    let btrfs = if config.btrfs {
        Some(download(SimpleAsset {
            api_url: BTRFS_API,
            label: "WinBtrfs",
            predicate: |url| url.ends_with(".zip") && !url.contains("source"),
        })?)
    } else {
        None
    };

    let ext2fsd = if config.ext4 {
        Some(download(SimpleAsset {
            api_url: EXT2FSD_API,
            label: "Ext2Fsd",
            predicate: |url| url.ends_with(".exe") && url.contains("Ext2Fsd"),
        })?)
    } else {
        None
    };

    let winfsp = if winfsp_needed {
        Some(download(SimpleAsset {
            api_url: WINFSP_API,
            label: "WinFsp",
            predicate: |url| url.ends_with(".msi") && url.contains("x64"),
        })?)
    } else {
        None
    };

    let mergerfs = if config.mergerfs {
        Some(download(SimpleAsset {
            api_url: MERGERFS_API,
            label: "mergerfs-windows",
            predicate: |url| url.ends_with(".zip") || url.ends_with(".exe"),
        })?)
    } else {
        None
    };

    Ok(Some(DriverPaths {
        btrfs,
        ext2fsd,
        winfsp,
        mergerfs,
    }))
}

/// Inject downloaded driver packages into the mounted WIM image and generate
/// a first-boot installation script.
pub fn inject_drivers(
    mount_dir: &Path,
    drivers: &DriverPaths,
    config: &Drivers,
) -> Result<(), Error> {
    let dest_dir = mount_dir.join("Drivers");
    ensure_dir(&dest_dir)?;

    for (src, label) in [
        (&drivers.btrfs, "WinBtrfs"),
        (&drivers.ext2fsd, "Ext2Fsd"),
        (&drivers.winfsp, "WinFsp"),
        (&drivers.mergerfs, "mergerfs"),
    ] {
        if let Some(src) = src {
            let dest = dest_dir.join(file_name(src));
            info!(src = %src.display(), dest = %dest.display(), "copying {label} into WIM");
            copy_file(src, &dest)?;
        }
    }

    // Generate the first-boot installer script.
    let script = generate_install_script(drivers, config);
    let script_path = dest_dir.join("install-drivers.ps1");
    info!(path = %script_path.display(), "writing driver install script");
    write_file(&script_path, &script)?;

    Ok(())
}

/// Generate a PowerShell script that installs the bundled drivers on first boot.
fn generate_install_script(drivers: &DriverPaths, config: &Drivers) -> String {
    let mut sections: SmallVec<[String; 4]> = SmallVec::new();

    if let Some(ref path) = drivers.btrfs {
        let name = file_name(path);
        sections.push(format!(
            r#"
# --- WinBtrfs (btrfs filesystem driver) ---
Write-Host "Installing WinBtrfs..."
$btrfsZip = "C:\Drivers\{name}"
$btrfsDir = "C:\Drivers\btrfs"
Expand-Archive -Path $btrfsZip -DestinationPath $btrfsDir -Force
# Install the driver via pnputil (looks for .inf in extracted dir)
$inf = Get-ChildItem -Path $btrfsDir -Filter "*.inf" -Recurse | Select-Object -First 1
if ($inf) {{
    pnputil /add-driver $inf.FullName /install
    Write-Host "WinBtrfs driver installed via pnputil."
}} else {{
    Write-Host "WARNING: No .inf found in WinBtrfs archive, skipping."
}}
$needsReboot = $true"#
        ));
    }

    if let Some(ref path) = drivers.ext2fsd {
        let name = file_name(path);
        sections.push(format!(
            r#"
# --- Ext2Fsd (ext2/ext3/ext4 filesystem driver) ---
Write-Host "Installing Ext2Fsd..."
Start-Process -FilePath "C:\Drivers\{name}" -ArgumentList '/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART' -Wait -NoNewWindow
Write-Host "Ext2Fsd installation complete."
$needsReboot = $true"#
        ));
    }

    // WinFsp must be installed before mergerfs.
    let winfsp_needed = config.winfsp || config.mergerfs;
    if winfsp_needed && let Some(ref path) = drivers.winfsp {
        let name = file_name(path);
        sections.push(format!(
            r#"
# --- WinFsp (Windows FUSE layer) ---
Write-Host "Installing WinFsp..."
Start-Process -FilePath "msiexec.exe" -ArgumentList '/i',"C:\Drivers\{name}",'/qn','/norestart' -Wait -NoNewWindow
Write-Host "WinFsp installation complete.""#
        ));
    }

    if let Some(ref path) = drivers.mergerfs {
        let name = file_name(path);
        let is_zip = name.ends_with(".zip");
        if is_zip {
            sections.push(format!(
                r#"
# --- mergerfs-windows ---
Write-Host "Installing mergerfs-windows..."
$mfsZip = "C:\Drivers\{name}"
$mfsDir = "$env:ProgramFiles\mergerfs"
New-Item -ItemType Directory -Path $mfsDir -Force | Out-Null
Expand-Archive -Path $mfsZip -DestinationPath $mfsDir -Force
# Add to PATH for convenience
$machinePath = [Environment]::GetEnvironmentVariable('Path', 'Machine')
if ($machinePath -notlike "*mergerfs*") {{
    [Environment]::SetEnvironmentVariable('Path', "$machinePath;$mfsDir", 'Machine')
}}
Write-Host "mergerfs-windows installed to $mfsDir.""#
            ));
        } else {
            sections.push(format!(
                r#"
# --- mergerfs-windows ---
Write-Host "Installing mergerfs-windows..."
$mfsDir = "$env:ProgramFiles\mergerfs"
New-Item -ItemType Directory -Path $mfsDir -Force | Out-Null
Copy-Item -Path "C:\Drivers\{name}" -Destination "$mfsDir\mergerfs.exe" -Force
$machinePath = [Environment]::GetEnvironmentVariable('Path', 'Machine')
if ($machinePath -notlike "*mergerfs*") {{
    [Environment]::SetEnvironmentVariable('Path', "$machinePath;$mfsDir", 'Machine')
}}
Write-Host "mergerfs-windows installed to $mfsDir.""#
            ));
        }
    }

    let driver_sections = sections.join("\n");

    format!(
        r#"# Filesystem Driver First-Boot Installer
# Generated by wincrab -- do not edit.
# This script runs once on first user login via RunOnce registry key.

$ErrorActionPreference = 'Stop'
$needsReboot = $false
{driver_sections}

# Clean up the installer directory.
Remove-Item -Path 'C:\Drivers' -Recurse -Force -ErrorAction SilentlyContinue
Write-Host "Driver installer cleanup done."

if ($needsReboot) {{
    Write-Host "A reboot is recommended to load the new filesystem drivers."
    # Schedule a reboot in 30 seconds so the user sees the message.
    shutdown /r /t 30 /c "Rebooting to complete filesystem driver installation."
}}
"#
    )
}

fn file_name(path: &Path) -> &str {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Drivers as DriversConfig;

    // -----------------------------------------------------------------------
    // select_asset_url integration (via SimpleAsset predicates)
    // -----------------------------------------------------------------------

    const BTRFS_JSON: &str = r#"
{
  "assets": [
    {
      "name": "btrfs-1.9.2.zip",
      "browser_download_url": "https://github.com/maharmstone/btrfs/releases/download/v1.9.2/btrfs-1.9.2.zip"
    },
    {
      "name": "source.tar.gz",
      "browser_download_url": "https://github.com/maharmstone/btrfs/archive/refs/tags/v1.9.2.tar.gz"
    }
  ]
}
"#;

    fn select_with(json: &str, predicate: fn(&str) -> bool) -> Result<String, crate::Error> {
        crate::github::select_asset_url(json, "test", |u| if predicate(u) { 1 } else { 0 })
    }

    #[test]
    fn extract_btrfs_zip() {
        let url =
            select_with(BTRFS_JSON, |u| u.ends_with(".zip") && !u.contains("source")).unwrap();
        assert!(url.ends_with(".zip"));
        assert!(url.contains("btrfs"));
    }

    #[test]
    fn extract_no_match_returns_error() {
        let result = select_with(BTRFS_JSON, |_| false);
        assert!(result.is_err());
    }

    #[test]
    fn extract_empty_json_returns_error() {
        let result = select_with("", |_| true);
        assert!(result.is_err());
    }

    #[test]
    fn extract_winfsp_msi() {
        let json = r#"
    "browser_download_url": "https://example.com/winfsp-2.0.x64.msi"
    "browser_download_url": "https://example.com/winfsp-2.0.arm64.msi"
"#;
        let url = select_with(json, |u| u.ends_with(".msi") && u.contains("x64")).unwrap();
        assert!(url.contains("x64"));
    }

    #[test]
    fn extract_ext2fsd_exe() {
        let json = r#"
    "browser_download_url": "https://example.com/Ext2Fsd-0.71.exe"
    "browser_download_url": "https://example.com/readme.txt"
"#;
        let url = select_with(json, |u| u.ends_with(".exe") && u.contains("Ext2Fsd")).unwrap();
        assert!(url.contains("Ext2Fsd"));
    }

    // -----------------------------------------------------------------------
    // file_name helper
    // -----------------------------------------------------------------------

    #[test]
    fn file_name_normal() {
        assert_eq!(file_name(Path::new("/tmp/foo.zip")), "foo.zip");
    }

    #[test]
    fn file_name_no_extension() {
        assert_eq!(file_name(Path::new("/tmp/myfile")), "myfile");
    }

    #[test]
    fn file_name_root_returns_unknown() {
        assert_eq!(file_name(Path::new("/")), "unknown");
    }

    // -----------------------------------------------------------------------
    // generate_install_script
    // -----------------------------------------------------------------------

    #[test]
    fn script_with_btrfs_only() {
        let paths = DriverPaths {
            btrfs: Some(PathBuf::from("/tmp/btrfs-1.9.zip")),
            ext2fsd: None,
            winfsp: None,
            mergerfs: None,
        };
        let config = DriversConfig {
            btrfs: true,
            ext4: false,
            winfsp: false,
            mergerfs: false,
            ..Default::default()
        };
        let script = generate_install_script(&paths, &config);
        assert!(script.contains("WinBtrfs"));
        assert!(script.contains("pnputil"));
        assert!(script.contains("$needsReboot = $true"));
        assert!(!script.contains("Ext2Fsd"));
        assert!(!script.contains("WinFsp"));
        assert!(!script.contains("mergerfs"));
    }

    #[test]
    fn script_with_all_drivers() {
        let paths = DriverPaths {
            btrfs: Some(PathBuf::from("/tmp/btrfs.zip")),
            ext2fsd: Some(PathBuf::from("/tmp/Ext2Fsd.exe")),
            winfsp: Some(PathBuf::from("/tmp/winfsp-x64.msi")),
            mergerfs: Some(PathBuf::from("/tmp/mergerfs.zip")),
        };
        let config = DriversConfig {
            btrfs: true,
            ext4: true,
            winfsp: true,
            mergerfs: true,
            ..Default::default()
        };
        let script = generate_install_script(&paths, &config);
        assert!(script.contains("WinBtrfs"));
        assert!(script.contains("Ext2Fsd"));
        assert!(script.contains("WinFsp"));
        assert!(script.contains("mergerfs"));
        assert!(script.contains("shutdown /r"));
    }

    #[test]
    fn script_mergerfs_zip_vs_exe() {
        let paths_zip = DriverPaths {
            btrfs: None,
            ext2fsd: None,
            winfsp: Some(PathBuf::from("/tmp/winfsp.msi")),
            mergerfs: Some(PathBuf::from("/tmp/mergerfs.zip")),
        };
        let config = DriversConfig {
            btrfs: false,
            ext4: false,
            winfsp: false,
            mergerfs: true,
            ..Default::default()
        };
        let script_zip = generate_install_script(&paths_zip, &config);
        assert!(script_zip.contains("Expand-Archive"));

        let paths_exe = DriverPaths {
            btrfs: None,
            ext2fsd: None,
            winfsp: Some(PathBuf::from("/tmp/winfsp.msi")),
            mergerfs: Some(PathBuf::from("/tmp/mergerfs.exe")),
        };
        let script_exe = generate_install_script(&paths_exe, &config);
        assert!(script_exe.contains("Copy-Item"));
        assert!(!script_exe.contains("Expand-Archive"));
    }

    #[test]
    fn script_cleanup_section() {
        let paths = DriverPaths {
            btrfs: None,
            ext2fsd: None,
            winfsp: None,
            mergerfs: None,
        };
        let config = DriversConfig::default();
        let script = generate_install_script(&paths, &config);
        assert!(script.contains("Remove-Item"));
        assert!(script.contains("C:\\Drivers"));
    }

    #[test]
    fn script_header() {
        let paths = DriverPaths {
            btrfs: None,
            ext2fsd: None,
            winfsp: None,
            mergerfs: None,
        };
        let config = DriversConfig::default();
        let script = generate_install_script(&paths, &config);
        assert!(script.contains("$ErrorActionPreference = 'Stop'"));
        assert!(script.contains("Generated by wincrab"));
    }

    // -----------------------------------------------------------------------
    // inject_drivers
    // -----------------------------------------------------------------------

    #[test]
    fn inject_drivers_copies_files_and_script() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path().join("mount");
        std::fs::create_dir_all(&mount).unwrap();

        let btrfs_src = dir.path().join("btrfs.zip");
        std::fs::write(&btrfs_src, b"fake zip").unwrap();

        let paths = DriverPaths {
            btrfs: Some(btrfs_src),
            ext2fsd: None,
            winfsp: None,
            mergerfs: None,
        };
        let config = DriversConfig {
            btrfs: true,
            ..Default::default()
        };

        inject_drivers(&mount, &paths, &config).unwrap();

        assert!(mount.join("Drivers").join("btrfs.zip").exists());
        assert!(mount.join("Drivers").join("install-drivers.ps1").exists());
    }

    // -----------------------------------------------------------------------
    // download_drivers
    // -----------------------------------------------------------------------

    #[test]
    fn download_returns_none_when_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let config = DriversConfig::default();
        let result = download_drivers(dir.path(), &config).unwrap();
        assert!(result.is_none());
    }
}

use std::path::Path;
use std::process::Command;

use tracing::info;

use crate::error::{ensure_dir, run_cmd, Error};

/// Repack the modified ISO tree into a new UEFI-bootable ISO.
///
/// Uses `genisoimage` to create a hybrid ISO9660+UDF filesystem with dual
/// El Torito boot entries (BIOS + UEFI). This is the same approach used by
/// the production-proven dockur/windows project.
///
/// The boot chain:
///   1. UEFI firmware reads El Torito → loads `efisys_noprompt.bin` (FAT image)
///   2. Runs `\EFI\BOOT\BOOTX64.EFI` from the FAT image
///   3. Windows Boot Manager uses its built-in UDF driver to read BCD and
///      boot.wim from the disc's UDF filesystem
///
/// Both El Torito (for firmware boot) AND UDF (for the Windows Boot Manager)
/// are required — OVMF has no UDF driver, and the Boot Manager has no
/// ISO9660 driver.
pub fn repack_iso(
    staging_dir: &Path,
    output_iso: &Path,
) -> Result<(), Error> {
    // Determine which EFI boot image to use.
    let efisys_noprompt = staging_dir
        .join("efi")
        .join("microsoft")
        .join("boot")
        .join("efisys_noprompt.bin");
    let efisys = staging_dir
        .join("efi")
        .join("microsoft")
        .join("boot")
        .join("efisys.bin");
    let efi_boot_path = if efisys_noprompt.exists() {
        "efi/microsoft/boot/efisys_noprompt.bin"
    } else if efisys.exists() {
        "efi/microsoft/boot/efisys.bin"
    } else {
        return Err(Error::EfiBootImageNotFound {
            noprompt_path: efisys_noprompt,
            fallback_path: efisys,
        });
    };

    if let Some(parent) = output_iso.parent() {
        ensure_dir(parent)?;
    }

    info!(
        staging = %staging_dir.display(),
        output = %output_iso.display(),
        "repacking ISO with genisoimage (UDF + El Torito dual boot)"
    );

    // genisoimage command matching the production-proven dockur/windows approach.
    //
    // El Torito dual boot:
    //   1. BIOS: boot/etfsboot.com (with boot-info-table patched in)
    //   2. UEFI: efi/microsoft/boot/efisys_noprompt.bin (FAT12 with BOOTX64.EFI)
    //
    // Filesystem: ISO9660 level 4 + Joliet + UDF
    //   - UDF is required because the Windows Boot Manager reads BCD via UDF
    //   - ISO9660 level 4 allows long filenames and large files
    //   - -allow-limited-size handles install.wim > 4GB
    run_cmd(
        Command::new("genisoimage")
            .arg("-o")
            .arg(output_iso)
            .arg("-b")
            .arg("boot/etfsboot.com")
            .arg("-no-emul-boot")
            .arg("-c")
            .arg("boot/boot.cat")
            .arg("-iso-level")
            .arg("4")
            .arg("-J")
            .arg("-l")
            .arg("-D")
            .arg("-N")
            .arg("-joliet-long")
            .arg("-relaxed-filenames")
            .arg("-udf")
            .arg("-boot-info-table")
            .arg("-eltorito-alt-boot")
            .arg("-eltorito-boot")
            .arg(efi_boot_path)
            .arg("-no-emul-boot")
            .arg("-allow-limited-size")
            .arg("-quiet")
            .arg(staging_dir),
    )?;

    let metadata = std::fs::metadata(output_iso).map_err(|e| Error::Io {
        context: "reading output ISO metadata".into(),
        source: e,
    })?;

    let size_mb = metadata.len() / (1024 * 1024);
    info!(size_mb, path = %output_iso.display(), "ISO repacked successfully");

    Ok(())
}

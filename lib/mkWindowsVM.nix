# mkWindowsVM — declarative Windows VM management scripts.
#
# Usage from a consumer flake:
#   let
#     iso = wincrab.lib.x86_64-linux.mkDebloatedISO { sourceIso = ./Win11.iso; };
#     vm = wincrab.lib.x86_64-linux.mkWindowsVM {
#       name = "dev-win11";
#       iso = iso;                  # derivation or path string
#       usbPassthrough = [{ vendorId = "1234"; productId = "5678"; }];
#       pciPassthrough = [{ address = "01:00.0"; }];
#     };
#   in { packages.x86_64-linux = { inherit (vm) create install boot; }; }

{ pkgs }:

{
  name,
  iso ? null,
  ram ? "8G",
  cpus ? 4,
  diskSize ? "64G",
  display ? "gtk",
  stateDir ? null,
  # USB passthrough: list of { vendorId, productId } attrsets.
  # Find IDs with `lsusb`.
  # Requires host udev rules or appropriate permissions.
  usbPassthrough ? [ ],
  # PCI passthrough (VFIO): list of { address } attrsets.
  # address is the BDF notation, e.g. "01:00.0".
  # Requires IOMMU enabled, vfio-pci driver bound to the device.
  pciPassthrough ? [ ],
}:

let
  inherit (pkgs) lib;

  stateDir' =
    if stateDir != null then
      "\"${stateDir}\""
    else
      "\"$HOME/.local/share/wincrab-vms/${name}\"";

  ovmfCode = "${pkgs.OVMFFull.fd}/FV/OVMF_CODE.fd";
  ovmfVarsTemplate = "${pkgs.OVMFFull.fd}/FV/OVMF_VARS.fd";

  commonRuntimeInputs = [
    pkgs.qemu
    pkgs.swtpm
  ];

  setStateDir = ''
    STATE_DIR=${stateDir'}
  '';

  requireDisk = ''
    if [ ! -f "$STATE_DIR/disk.qcow2" ]; then
      echo "ERROR: No VM found at $STATE_DIR"
      echo "Run the 'create' command first."
      exit 1
    fi
  '';

  # Resolve ISO: derivation path, baked-in string, or $1
  resolveIso =
    if iso != null then
      if builtins.isPath iso || lib.isDerivation iso then
        # Derivation or Nix path — resolve to store path at build time
        let
          isoPath =
            if lib.isDerivation iso then "${iso}/windows.iso" else "${iso}";
        in
        ''ISO="${isoPath}"''
      else
        # Plain string path — bake it in
        ''ISO="${iso}"''
    else
      # No iso provided — accept as CLI argument
      ''ISO="''${1:?Usage: ${name}-install <path-to-iso>}"'';

  startTpm = ''
    TPM_DIR="$STATE_DIR/tpm"
    mkdir -p "$TPM_DIR"

    # Kill any leftover swtpm for this VM
    if [ -e "$TPM_DIR/sock" ]; then
      rm -f "$TPM_DIR/sock"
    fi

    swtpm socket \
      --tpmstate dir="$TPM_DIR" \
      --ctrl type=unixio,path="$TPM_DIR/sock" \
      --tpm2 \
      --daemon

    # shellcheck disable=SC2329,SC2317
    _cleanup_tpm() {
      pkill -f "swtpm.*$TPM_DIR" 2>/dev/null || true
    }
    trap _cleanup_tpm EXIT
  '';

  # USB passthrough args: xHCI controller + one usb-host per device
  usbArgs =
    if usbPassthrough != [ ] then
      lib.concatStringsSep " \\\n    " (
        [ "-device qemu-xhci,id=xhci" ]
        ++ lib.imap0 (
          i: dev:
          "-device usb-host,bus=xhci.0,vendorid=0x${dev.vendorId},productid=0x${dev.productId}"
        ) usbPassthrough
      )
    else
      "";

  # PCI/VFIO passthrough args
  pciArgs =
    if pciPassthrough != [ ] then
      lib.concatStringsSep " \\\n    " (
        lib.map (dev: "-device vfio-pci,host=${dev.address}") pciPassthrough
      )
    else
      "";

  # Extra device args (USB + PCI combined)
  extraDeviceArgs = lib.concatStringsSep " \\\n    " (
    lib.filter (s: s != "") [
      usbArgs
      pciArgs
    ]
  );

  qemuCommonArgs = ''
    -machine q35,accel=kvm \
    -cpu host \
    -smp ${toString cpus} \
    -m ${ram} \
    -drive "if=pflash,format=raw,readonly=on,file=${ovmfCode}" \
    -drive "if=pflash,format=raw,file=$STATE_DIR/OVMF_VARS.fd" \
    -chardev "socket,id=chrtpm,path=$STATE_DIR/tpm/sock" \
    -tpmdev emulator,id=tpm0,chardev=chrtpm \
    -device tpm-tis,tpmdev=tpm0 \
    -device virtio-net-pci,netdev=net0 \
    -netdev user,id=net0 \
    -display ${display} \
    -device virtio-vga \
    -usb \
    -device usb-tablet \
    -global isa-debugcon.iobase=0x402 \
    -debugcon "file:$STATE_DIR/ovmf-debug.log"${
      if extraDeviceArgs != "" then
        " \\\n    ${extraDeviceArgs}"
      else
        ""
    }
  '';

in
{
  create = pkgs.writeShellApplication {
    name = "${name}-create";
    runtimeInputs = [ pkgs.qemu ];
    text = ''
      set -euo pipefail
      ${setStateDir}

      if [ -f "$STATE_DIR/disk.qcow2" ]; then
        echo "VM '${name}' already exists at $STATE_DIR"
        echo "Run the 'destroy' command first to recreate."
        exit 1
      fi

      echo "Creating VM '${name}' at $STATE_DIR"
      mkdir -p "$STATE_DIR/tpm"

      echo "  Creating ${diskSize} disk..."
      qemu-img create -f qcow2 "$STATE_DIR/disk.qcow2" "${diskSize}"

      echo "  Copying OVMF firmware vars..."
      cp "${ovmfVarsTemplate}" "$STATE_DIR/OVMF_VARS.fd"
      chmod u+w "$STATE_DIR/OVMF_VARS.fd"

      echo ""
      echo "VM '${name}' created. Run the 'install' command to install Windows."
    '';
  };

  install = pkgs.writeShellApplication {
    name = "${name}-install";
    runtimeInputs = commonRuntimeInputs;
    text = ''
      set -euo pipefail
      ${setStateDir}
      ${requireDisk}
      ${resolveIso}

      if [ ! -f "$ISO" ]; then
        echo "ERROR: ISO not found: $ISO"
        exit 1
      fi

      echo "Installing Windows from ISO into VM '${name}'"
      echo "  ISO:  $ISO"
      echo "  Disk: $STATE_DIR/disk.qcow2"
      echo "  RAM:  ${ram}  CPUs: ${toString cpus}"
      ${lib.optionalString (usbPassthrough != [ ]) ''echo "  USB:  ${toString (builtins.length usbPassthrough)} device(s) passed through"''}
      ${lib.optionalString (pciPassthrough != [ ]) ''echo "  PCI:  ${toString (builtins.length pciPassthrough)} device(s) passed through"''}
      echo ""
      echo "Tip: Press Ctrl+Alt+G to release mouse grab"

      ${startTpm}

      qemu-system-x86_64 \
        ${qemuCommonArgs}
        -device ahci,id=ahci \
        -drive "file=$STATE_DIR/disk.qcow2,format=qcow2,if=none,id=hd0" \
        -device ide-hd,drive=hd0,bus=ahci.1 \
        -drive "file=$ISO,media=cdrom,if=none,id=cd0,readonly=on" \
        -device ide-cd,drive=cd0,bus=ahci.0 \
        -boot d
    '';
  };

  boot = pkgs.writeShellApplication {
    name = "${name}-boot";
    runtimeInputs = commonRuntimeInputs;
    text = ''
      set -euo pipefail
      ${setStateDir}
      ${requireDisk}

      echo "Booting VM '${name}'"
      echo "  Disk: $STATE_DIR/disk.qcow2"
      echo "  RAM:  ${ram}  CPUs: ${toString cpus}"
      ${lib.optionalString (usbPassthrough != [ ]) ''echo "  USB:  ${toString (builtins.length usbPassthrough)} device(s) passed through"''}
      ${lib.optionalString (pciPassthrough != [ ]) ''echo "  PCI:  ${toString (builtins.length pciPassthrough)} device(s) passed through"''}
      echo ""
      echo "Tip: Press Ctrl+Alt+G to release mouse grab"

      ${startTpm}

      qemu-system-x86_64 \
        ${qemuCommonArgs}
        -device ahci,id=ahci \
        -drive "file=$STATE_DIR/disk.qcow2,format=qcow2,if=none,id=hd0" \
        -device ide-hd,drive=hd0,bus=ahci.1
    '';
  };

  destroy = pkgs.writeShellApplication {
    name = "${name}-destroy";
    text = ''
      set -euo pipefail
      ${setStateDir}

      if [ ! -d "$STATE_DIR" ]; then
        echo "VM '${name}' does not exist at $STATE_DIR"
        exit 1
      fi

      echo "This will permanently delete VM '${name}' at:"
      echo "  $STATE_DIR"
      echo ""
      printf "Are you sure? [y/N] "
      read -r confirm
      if [ "$confirm" != "y" ] && [ "$confirm" != "Y" ]; then
        echo "Aborted."
        exit 0
      fi

      rm -rf "$STATE_DIR"
      echo "VM '${name}' destroyed."
    '';
  };

  snapshot-save = pkgs.writeShellApplication {
    name = "${name}-snapshot-save";
    runtimeInputs = [ pkgs.qemu ];
    text = ''
      set -euo pipefail
      ${setStateDir}
      ${requireDisk}

      SNAP="''${1:?Usage: ${name}-snapshot-save <snapshot-name>}"
      echo "Saving snapshot '$SNAP' for VM '${name}'..."
      qemu-img snapshot -c "$SNAP" "$STATE_DIR/disk.qcow2"
      echo "Snapshot '$SNAP' saved."
    '';
  };

  snapshot-restore = pkgs.writeShellApplication {
    name = "${name}-snapshot-restore";
    runtimeInputs = [ pkgs.qemu ];
    text = ''
      set -euo pipefail
      ${setStateDir}
      ${requireDisk}

      SNAP="''${1:?Usage: ${name}-snapshot-restore <snapshot-name>}"
      echo "Restoring snapshot '$SNAP' for VM '${name}'..."
      qemu-img snapshot -a "$SNAP" "$STATE_DIR/disk.qcow2"
      echo "Snapshot '$SNAP' restored."
    '';
  };

  snapshot-list = pkgs.writeShellApplication {
    name = "${name}-snapshot-list";
    runtimeInputs = [ pkgs.qemu ];
    text = ''
      set -euo pipefail
      ${setStateDir}
      ${requireDisk}

      echo "Snapshots for VM '${name}':"
      qemu-img snapshot -l "$STATE_DIR/disk.qcow2"
    '';
  };
}

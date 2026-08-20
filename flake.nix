{
  description = "wincrab — debloated Windows 11 ISO builder for Linux";

  inputs = {
    rs-harbor.url = "git+ssh://git@github.com/caniko/rs-harbor.git?ref=trunk&rev=05cc4f162b55fa904b687db1821e2463fa813e50";
    nixpkgs.follows = "rs-harbor/nixpkgs";
    crane.follows = "rs-harbor/crane";

    flake-utils.url = "github:numtide/flake-utils";

    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
  };

  outputs =
    {
      self,
      rs-harbor,
      nixpkgs,
      crane,
      flake-utils,
      advisory-db,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [(import rs-harbor.inputs.rust-overlay)];
        };

        inherit (pkgs) lib;

        mkWindowsVM = import ./lib/mkWindowsVM.nix { inherit pkgs; };
        mkDebloatedISO = import ./lib/mkDebloatedISO.nix { inherit pkgs wincrab; };

        toolchain = rs-harbor.lib.mkToolchain { inherit pkgs; toolchainProfile = "nightly"; };
        craneLib = toolchain.craneLib;
        src = craneLib.cleanCargoSource ./.;

        commonArgs = {
          inherit src;
          strictDeps = true;

          buildInputs =
            [ ]
            ++ lib.optionals pkgs.stdenv.isDarwin [
              pkgs.libiconv
            ];
        };

        cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        buildCache = rs-harbor.lib.mkBuildCachePolicy {
          inherit pkgs;
          buildPackageSet = pkgs.buildPackages;
          sccachePackage = pkgs.buildPackages.sccache;
          cacheRoot = null;
          namespaceScope = "canix-rust";
          namespaceGeneration = 5;
        };

        individualCrateArgs = commonArgs // {
          inherit cargoArtifacts;
          inherit (craneLib.crateNameFromCargoToml { inherit src; }) version;
          doCheck = false;
        };

        fileSetForCrate =
          crate:
          lib.fileset.toSource {
            root = ./.;
            fileset = lib.fileset.unions [
              ./Cargo.toml
              ./Cargo.lock
              (craneLib.fileset.commonCargoSources ./crates/wincrab-core)
              (craneLib.fileset.commonCargoSources crate)
            ];
          };

        wincrab = buildCache.withRustCache {
          package = craneLib.buildPackage (
            individualCrateArgs
            // {
              pname = "wincrab";
              cargoExtraArgs = "-p wincrab";
              src = fileSetForCrate ./crates/wincrab;
            }
          );
        };
      in
      {
        checks = {
          inherit wincrab;

          wincrab-workspace-clippy = craneLib.cargoClippy (
            commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            }
          );

          wincrab-workspace-doc = craneLib.cargoDoc (
            commonArgs
            // {
              inherit cargoArtifacts;
              env.RUSTDOCFLAGS = "--deny warnings";
            }
          );

          wincrab-workspace-fmt = craneLib.cargoFmt {
            inherit src;
          };

          wincrab-workspace-toml-fmt = craneLib.taploFmt {
            src = pkgs.lib.sources.sourceFilesBySuffices src [ ".toml" ];
          };

          wincrab-workspace-audit = craneLib.cargoAudit {
            inherit src advisory-db;
          };

          wincrab-workspace-deny = craneLib.cargoDeny {
            inherit src;
          };

          wincrab-workspace-nextest = craneLib.cargoNextest (
            commonArgs
            // {
              inherit cargoArtifacts;
              partitions = 1;
              partitionType = "count";
              cargoNextestPartitionsExtraArgs = "--no-tests=pass";
            }
          );
        };

        packages = let
          defaultVM = mkWindowsVM { name = "wincrab-dev"; };
        in {
          inherit wincrab;
          default = wincrab;

          # Convenience wrappers using mkWindowsVM library
          vm-create = defaultVM.create;
          vm-install = defaultVM.install;
          vm-boot = defaultVM.boot;
          vm-destroy = defaultVM.destroy;
          vm-snapshot-save = defaultVM.snapshot-save;
          vm-snapshot-restore = defaultVM.snapshot-restore;
          vm-snapshot-list = defaultVM.snapshot-list;

          # Launch a QEMU VM to test a Windows ISO with UEFI + TPM 2.0.
          # Usage: nix run .#test-vm -- path/to/Win11.iso
          test-vm = pkgs.writeShellApplication {
            name = "test-vm";
            runtimeInputs = [
              pkgs.qemu
              pkgs.swtpm
              pkgs.OVMFFull
            ];
            text = ''
              set -euo pipefail

              ISO="''${1:?Usage: test-vm <path-to-iso>}"
              VM_DIR="''${WINCRAB_VM_DIR:-./wincrab-vm}"
              RAM="''${WINCRAB_VM_RAM:-8G}"
              CPUS="''${WINCRAB_VM_CPUS:-4}"
              DISK_SIZE="''${WINCRAB_VM_DISK:-64G}"

              mkdir -p "$VM_DIR"

              # Create virtual disk if it doesn't exist
              DISK="$VM_DIR/disk.qcow2"
              if [ ! -f "$DISK" ]; then
                echo "Creating $DISK_SIZE virtual disk..."
                qemu-img create -f qcow2 "$DISK" "$DISK_SIZE"
              fi

              # Set up swtpm (TPM 2.0 emulator — required by Windows 11)
              TPM_DIR="$VM_DIR/tpm"
              mkdir -p "$TPM_DIR"
              TPM_SOCK="$TPM_DIR/sock"

              echo "Starting swtpm..."
              swtpm socket \
                --tpmstate dir="$TPM_DIR" \
                --ctrl type=unixio,path="$TPM_SOCK" \
                --tpm2 \
                --daemon

              # Copy OVMF firmware vars (needs to be writable)
              OVMF_CODE="${pkgs.OVMFFull.fd}/FV/OVMF_CODE.fd"
              OVMF_VARS="$VM_DIR/OVMF_VARS.fd"
              if [ ! -f "$OVMF_VARS" ]; then
                cp "${pkgs.OVMFFull.fd}/FV/OVMF_VARS.fd" "$OVMF_VARS"
                chmod u+w "$OVMF_VARS"
              fi

              echo "Launching QEMU..."
              echo "  ISO:  $ISO"
              echo "  Disk: $DISK ($DISK_SIZE)"
              echo "  RAM:  $RAM  CPUs: $CPUS"
              echo ""
              echo "Tip: Press Ctrl+Alt+G to release mouse grab"

              OVMF_LOG="$VM_DIR/ovmf-debug.log"

              qemu-system-x86_64 \
                -machine q35,accel=kvm \
                -cpu host \
                -smp "$CPUS" \
                -m "$RAM" \
                -drive "if=pflash,format=raw,readonly=on,file=$OVMF_CODE" \
                -drive "if=pflash,format=raw,file=$OVMF_VARS" \
                -device ahci,id=ahci \
                -drive "file=$DISK,format=qcow2,if=none,id=hd0" \
                -device ide-hd,drive=hd0,bus=ahci.1 \
                -drive "file=$ISO,media=cdrom,if=none,id=cd0,readonly=on" \
                -device ide-cd,drive=cd0,bus=ahci.0 \
                -boot d \
                -chardev "socket,id=chrtpm,path=$TPM_SOCK" \
                -tpmdev emulator,id=tpm0,chardev=chrtpm \
                -device tpm-tis,tpmdev=tpm0 \
                -device virtio-net-pci,netdev=net0 \
                -netdev user,id=net0 \
                -display gtk \
                -device virtio-vga \
                -usb \
                -device usb-tablet \
                -global isa-debugcon.iobase=0x402 \
                -debugcon "file:$OVMF_LOG"
            '';
          };

          # End-to-end VM test: build debloated ISO + unattended install + verification.
          # Single command that does everything:
          #   nix run .#test-e2e -- path/to/Win11_source.iso [config.toml | --profile vm]
          #
          # Steps:
          #   1. Runs wincrab to build a debloated ISO from the source
          #   2. Extracts the debloated ISO, injects autounattend.xml + verify.ps1
          #   3. Repacks with genisoimage
          #   4. Boots QEMU headless with serial output
          #   5. Monitors serial for WINCRAB_TEST_PASS/FAIL (timeout: 45 min)
          #   6. Exits 0 on pass, 1 on fail, 2 on timeout
          #
          # Environment variables:
          #   WINCRAB_TEST_TIMEOUT  — VM timeout in seconds (default: 2700 = 45 min)
          #   WINCRAB_VM_RAM        — VM RAM (default: 8G)
          #   WINCRAB_VM_CPUS       — VM CPUs (default: 4)
          #   WINCRAB_TEST_DISPLAY  — QEMU display (default: none)
          #   WINCRAB_TEST_KEEP     — set to 1 to keep work dir after test
          #   WINCRAB_TEST_WORKDIR  — override work directory
          test-e2e = pkgs.writeShellApplication {
            name = "test-e2e";
            runtimeInputs = [
              wincrab
              pkgs.qemu
              pkgs.swtpm
              pkgs.OVMFFull
              pkgs.p7zip
              pkgs.wimlib
              pkgs.hivex
              pkgs.xorriso
              pkgs.cdrtools
              pkgs.cdrkit
              pkgs.mtools
              pkgs.dosfstools
              pkgs.socat
            ];
            text = ''
              set -euo pipefail

              usage() {
                echo "Usage: test-e2e <source-windows.iso> [config.toml | --profile <name>]"
                echo ""
                echo "Builds a debloated ISO from source, installs it in a headless VM,"
                echo "and verifies all wincrab modifications automatically."
                echo ""
                echo "Examples:"
                echo "  test-e2e Win11_24H2.iso                     # use default config"
                echo "  test-e2e Win11_24H2.iso my-config.toml      # use custom config"
                echo "  test-e2e Win11_24H2.iso --profile vm         # use built-in profile"
                exit 1
              }

              if [ $# -lt 1 ]; then
                usage
              fi

              SOURCE_ISO="$(realpath "$1")"
              shift

              # Parse optional config / profile argument
              CONFIG_ARGS=""
              CONFIG_DESC="default"
              if [ $# -ge 1 ]; then
                if [ "$1" = "--profile" ]; then
                  if [ $# -lt 2 ]; then
                    echo "ERROR: --profile requires a name (minimal, gaming, privacy, enterprise, vm, vm-seelen)"
                    exit 1
                  fi
                  CONFIG_ARGS="--profile $2"
                  CONFIG_DESC="profile: $2"
                  shift 2
                else
                  CONFIG_FILE="$(realpath "$1")"
                  CONFIG_ARGS="--config $CONFIG_FILE"
                  CONFIG_DESC="config: $1"
                  shift
                fi
              fi

              TIMEOUT="''${WINCRAB_TEST_TIMEOUT:-2700}"
              RAM="''${WINCRAB_VM_RAM:-8G}"
              CPUS="''${WINCRAB_VM_CPUS:-4}"

              WORK="''${WINCRAB_TEST_WORKDIR:-$(mktemp -d)}"
              mkdir -p "$WORK"
              # shellcheck disable=SC2329,SC2317
              _cleanup() {
                if [ "''${WINCRAB_TEST_KEEP:-}" = "1" ]; then
                  echo "Keeping work dir: $WORK"
                else
                  echo "Cleaning up $WORK..."
                  rm -rf "$WORK"
                fi
              }
              trap _cleanup EXIT

              echo "=== wincrab end-to-end VM test ==="
              echo "Source ISO: $SOURCE_ISO"
              echo "Config:    $CONFIG_DESC"
              echo "Timeout:   ''${TIMEOUT}s"
              echo ""

              # --- 1. Build debloated ISO with wincrab ------------------------------
              echo "[1/5] Building debloated ISO with wincrab..."
              DEBLOATED_ISO="$WORK/debloated.iso"

              # shellcheck disable=SC2086
              wincrab -v build \
                --iso "$SOURCE_ISO" \
                --output "$DEBLOATED_ISO" \
                --work-dir "$WORK/wincrab-work" \
                $CONFIG_ARGS

              echo "  Built: $(du -h "$DEBLOATED_ISO" | cut -f1)"
              echo ""

              # --- 2. Extract ISO and inject test files ----------------------------
              echo "[2/5] Extracting debloated ISO..."
              STAGING="$WORK/staging"
              7z x -o"$STAGING" "$DEBLOATED_ISO" -y >/dev/null

              echo "[2/5] Injecting test autounattend.xml and verify.ps1..."
              cp ${./tests/vm/autounattend.xml} "$STAGING/autounattend.xml"
              cp ${./tests/vm/verify.ps1} "$STAGING/wincrab-verify.ps1"

              # --- 3. Repack ISO with test files -----------------------------------
              echo "[3/5] Repacking ISO with test harness..."
              TEST_ISO="$WORK/test.iso"

              EFI_BOOT=""
              if [ -f "$STAGING/efi/microsoft/boot/efisys_noprompt.bin" ]; then
                EFI_BOOT="efi/microsoft/boot/efisys_noprompt.bin"
              elif [ -f "$STAGING/efi/microsoft/boot/efisys.bin" ]; then
                EFI_BOOT="efi/microsoft/boot/efisys.bin"
              else
                echo "ERROR: No EFI boot image found in ISO"
                exit 1
              fi

              genisoimage \
                -o "$TEST_ISO" \
                -b boot/etfsboot.com \
                -no-emul-boot \
                -c boot/boot.cat \
                -iso-level 4 \
                -J -l -D -N -joliet-long \
                -relaxed-filenames \
                -udf \
                -boot-info-table \
                -eltorito-alt-boot \
                -eltorito-boot "$EFI_BOOT" \
                -no-emul-boot \
                -allow-limited-size \
                -quiet \
                "$STAGING"

              echo "  Repacked: $(du -h "$TEST_ISO" | cut -f1)"

              # --- 4. Set up VM and boot headless ----------------------------------
              echo "[4/5] Booting headless VM..."
              VM_DIR="$WORK/vm"
              mkdir -p "$VM_DIR"

              qemu-img create -f qcow2 "$VM_DIR/disk.qcow2" 64G >/dev/null 2>&1

              TPM_DIR="$VM_DIR/tpm"
              mkdir -p "$TPM_DIR"
              swtpm socket \
                --tpmstate dir="$TPM_DIR" \
                --ctrl type=unixio,path="$TPM_DIR/sock" \
                --tpm2 \
                --daemon

              OVMF_CODE="${pkgs.OVMFFull.fd}/FV/OVMF_CODE.fd"
              cp "${pkgs.OVMFFull.fd}/FV/OVMF_VARS.fd" "$VM_DIR/OVMF_VARS.fd"
              chmod u+w "$VM_DIR/OVMF_VARS.fd"

              SERIAL_LOG="$VM_DIR/serial.log"
              touch "$SERIAL_LOG"

              qemu-system-x86_64 \
                -machine q35,accel=kvm \
                -cpu host \
                -smp "$CPUS" \
                -m "$RAM" \
                -drive "if=pflash,format=raw,readonly=on,file=$OVMF_CODE" \
                -drive "if=pflash,format=raw,file=$VM_DIR/OVMF_VARS.fd" \
                -device ahci,id=ahci \
                -drive "file=$VM_DIR/disk.qcow2,format=qcow2,if=none,id=hd0" \
                -device ide-hd,drive=hd0,bus=ahci.1 \
                -drive "file=$TEST_ISO,media=cdrom,if=none,id=cd0,readonly=on" \
                -device ide-cd,drive=cd0,bus=ahci.0 \
                -boot d \
                -chardev "socket,id=chrtpm,path=$TPM_DIR/sock" \
                -tpmdev emulator,id=tpm0,chardev=chrtpm \
                -device tpm-tis,tpmdev=tpm0 \
                -device virtio-net-pci,netdev=net0 \
                -netdev user,id=net0 \
                -display "''${WINCRAB_TEST_DISPLAY:-none}" \
                -vnc :9,to=99 \
                -device virtio-vga \
                -serial "file:$SERIAL_LOG" \
                -monitor "unix:$VM_DIR/monitor.sock,server,nowait" \
                &
              QEMU_PID=$!

              # shellcheck disable=SC2329,SC2317
              screendump() {
                local out="$1"
                echo "screendump $out" | socat - "UNIX-CONNECT:$VM_DIR/monitor.sock" >/dev/null 2>&1 || true
              }

              # --- 5. Monitor serial output for results ----------------------------
              echo "[5/5] Waiting for Windows install + verification (timeout: ''${TIMEOUT}s)..."
              echo "  Serial log: $SERIAL_LOG"
              echo "  QEMU PID:   $QEMU_PID"
              echo ""

              ELAPSED=0
              RESULT=""
              while [ "$ELAPSED" -lt "$TIMEOUT" ]; do
                if ! kill -0 "$QEMU_PID" 2>/dev/null; then
                  echo "  VM shut down after ''${ELAPSED}s"
                  break
                fi

                if grep -q "WINCRAB_TEST_PASS" "$SERIAL_LOG" 2>/dev/null; then
                  RESULT="pass"
                  break
                fi
                if grep -q "WINCRAB_TEST_FAIL" "$SERIAL_LOG" 2>/dev/null; then
                  RESULT="fail"
                  break
                fi

                if [ $((ELAPSED % 30)) -eq 0 ] && [ "$ELAPSED" -gt 0 ]; then
                  echo "  ... ''${ELAPSED}s elapsed"
                fi
                if [ $((ELAPSED % 120)) -eq 0 ] && [ "$ELAPSED" -gt 0 ]; then
                  screendump "$VM_DIR/screen-''${ELAPSED}.ppm"
                fi

                sleep 5
                ELAPSED=$((ELAPSED + 5))
              done

              if kill -0 "$QEMU_PID" 2>/dev/null; then
                kill "$QEMU_PID" 2>/dev/null || true
                wait "$QEMU_PID" 2>/dev/null || true
              fi

              echo ""
              echo "=== Serial output ==="
              cat "$SERIAL_LOG"
              echo ""
              echo "====================="

              if [ "$RESULT" = "pass" ]; then
                echo ""
                echo "PASSED: All wincrab modifications verified."
                exit 0
              elif [ "$RESULT" = "fail" ]; then
                echo ""
                echo "FAILED: Some modifications were not applied correctly."
                exit 1
              else
                echo ""
                echo "TIMEOUT: VM did not complete verification within ''${TIMEOUT}s."
                echo "Check $VM_DIR/ovmf.log for UEFI boot issues."
                exit 2
              fi
            '';
          };

          # E2E test with Seelen-UI bundled.  Same as test-e2e but uses
          # the vm-seelen profile, which enables seelen.bundle = true.
          # Usage: nix run .#test-e2e-seelen -- Win11_25H2.iso
          test-e2e-seelen = let
            e2e = self.packages.${system}.test-e2e;
          in pkgs.writeShellApplication {
            name = "test-e2e-seelen";
            runtimeInputs = [ e2e ];
            text = ''
              set -euo pipefail

              if [ $# -lt 1 ]; then
                echo "Usage: test-e2e-seelen <source-windows.iso>"
                echo ""
                echo "Builds a debloated ISO with Seelen-UI bundled (vm-seelen profile),"
                echo "installs it in a headless VM, and verifies all modifications."
                exit 1
              fi

              exec test-e2e "$1" --profile vm-seelen
            '';
          };

          # Headless VM test for a pre-built debloated ISO (skips the build step).
          # Usage: nix run .#test-vm-headless -- path/to/Win11_debloated.iso
          test-vm-headless = pkgs.writeShellApplication {
            name = "test-vm-headless";
            runtimeInputs = [
              pkgs.qemu
              pkgs.swtpm
              pkgs.OVMFFull
              pkgs.p7zip
              pkgs.cdrkit   # genisoimage
              pkgs.socat    # for QEMU monitor
            ];
            text = ''
              set -euo pipefail

              ISO="$(realpath "''${1:?Usage: test-vm-headless <path-to-debloated-iso>}")"
              TIMEOUT="''${WINCRAB_TEST_TIMEOUT:-2700}"  # 45 minutes default
              RAM="''${WINCRAB_VM_RAM:-8G}"
              CPUS="''${WINCRAB_VM_CPUS:-4}"

              WORK="''${WINCRAB_TEST_WORKDIR:-$(mktemp -d)}"
              mkdir -p "$WORK"
              # shellcheck disable=SC2329,SC2317
              _cleanup() {
                if [ "''${WINCRAB_TEST_KEEP:-}" = "1" ]; then
                  echo "Keeping work dir: $WORK"
                else
                  echo "Cleaning up $WORK..."
                  rm -rf "$WORK"
                fi
              }
              trap _cleanup EXIT

              echo "=== wincrab headless VM test ==="
              echo "ISO:     $ISO"
              echo "Timeout: ''${TIMEOUT}s"
              echo ""

              # --- 1. Extract ISO and inject test files ----------------------------
              echo "[1/4] Extracting ISO..."
              STAGING="$WORK/staging"
              7z x -o"$STAGING" "$ISO" -y >/dev/null

              echo "[1/4] Injecting test autounattend.xml and verify.ps1..."
              cp ${./tests/vm/autounattend.xml} "$STAGING/autounattend.xml"
              cp ${./tests/vm/verify.ps1} "$STAGING/wincrab-verify.ps1"

              # --- 2. Repack ISO with test files -----------------------------------
              echo "[2/4] Repacking ISO..."
              TEST_ISO="$WORK/test.iso"

              # Determine EFI boot image path
              EFI_BOOT=""
              if [ -f "$STAGING/efi/microsoft/boot/efisys_noprompt.bin" ]; then
                EFI_BOOT="efi/microsoft/boot/efisys_noprompt.bin"
              elif [ -f "$STAGING/efi/microsoft/boot/efisys.bin" ]; then
                EFI_BOOT="efi/microsoft/boot/efisys.bin"
              else
                echo "ERROR: No EFI boot image found in ISO"
                exit 1
              fi

              genisoimage \
                -o "$TEST_ISO" \
                -b boot/etfsboot.com \
                -no-emul-boot \
                -c boot/boot.cat \
                -iso-level 4 \
                -J -l -D -N -joliet-long \
                -relaxed-filenames \
                -udf \
                -boot-info-table \
                -eltorito-alt-boot \
                -eltorito-boot "$EFI_BOOT" \
                -no-emul-boot \
                -allow-limited-size \
                -quiet \
                "$STAGING"

              echo "  Repacked: $(du -h "$TEST_ISO" | cut -f1)"

              # --- 3. Set up VM and boot headless ----------------------------------
              echo "[3/4] Booting headless VM..."
              VM_DIR="$WORK/vm"
              mkdir -p "$VM_DIR"

              # Create disk
              qemu-img create -f qcow2 "$VM_DIR/disk.qcow2" 64G >/dev/null 2>&1

              # Set up swtpm
              TPM_DIR="$VM_DIR/tpm"
              mkdir -p "$TPM_DIR"
              swtpm socket \
                --tpmstate dir="$TPM_DIR" \
                --ctrl type=unixio,path="$TPM_DIR/sock" \
                --tpm2 \
                --daemon

              # OVMF firmware
              OVMF_CODE="${pkgs.OVMFFull.fd}/FV/OVMF_CODE.fd"
              cp "${pkgs.OVMFFull.fd}/FV/OVMF_VARS.fd" "$VM_DIR/OVMF_VARS.fd"
              chmod u+w "$VM_DIR/OVMF_VARS.fd"

              SERIAL_LOG="$VM_DIR/serial.log"
              touch "$SERIAL_LOG"

              # Launch QEMU headless with serial output to file
              qemu-system-x86_64 \
                -machine q35,accel=kvm \
                -cpu host \
                -smp "$CPUS" \
                -m "$RAM" \
                -drive "if=pflash,format=raw,readonly=on,file=$OVMF_CODE" \
                -drive "if=pflash,format=raw,file=$VM_DIR/OVMF_VARS.fd" \
                -device ahci,id=ahci \
                -drive "file=$VM_DIR/disk.qcow2,format=qcow2,if=none,id=hd0" \
                -device ide-hd,drive=hd0,bus=ahci.1 \
                -drive "file=$TEST_ISO,media=cdrom,if=none,id=cd0,readonly=on" \
                -device ide-cd,drive=cd0,bus=ahci.0 \
                -boot d \
                -chardev "socket,id=chrtpm,path=$TPM_DIR/sock" \
                -tpmdev emulator,id=tpm0,chardev=chrtpm \
                -device tpm-tis,tpmdev=tpm0 \
                -device virtio-net-pci,netdev=net0 \
                -netdev user,id=net0 \
                -display "''${WINCRAB_TEST_DISPLAY:-none}" \
                -vnc :9,to=99 \
                -device virtio-vga \
                -serial "file:$SERIAL_LOG" \
                -monitor "unix:$VM_DIR/monitor.sock,server,nowait" \
                &
              QEMU_PID=$!

              # Helper: take a screenshot via QEMU monitor
              # shellcheck disable=SC2329,SC2317
              screendump() {
                local out="$1"
                echo "screendump $out" | socat - "UNIX-CONNECT:$VM_DIR/monitor.sock" >/dev/null 2>&1 || true
              }

              # --- 4. Monitor serial output for results ----------------------------
              echo "[4/4] Waiting for Windows install + verification (timeout: ''${TIMEOUT}s)..."
              echo "  Serial log: $SERIAL_LOG"
              echo "  QEMU PID:   $QEMU_PID"
              echo ""

              ELAPSED=0
              RESULT=""
              while [ "$ELAPSED" -lt "$TIMEOUT" ]; do
                if ! kill -0 "$QEMU_PID" 2>/dev/null; then
                  echo "  VM shut down after ''${ELAPSED}s"
                  break
                fi

                if grep -q "WINCRAB_TEST_PASS" "$SERIAL_LOG" 2>/dev/null; then
                  RESULT="pass"
                  break
                fi
                if grep -q "WINCRAB_TEST_FAIL" "$SERIAL_LOG" 2>/dev/null; then
                  RESULT="fail"
                  break
                fi

                # Print progress every 30s, take screenshot every 120s
                if [ $((ELAPSED % 30)) -eq 0 ] && [ "$ELAPSED" -gt 0 ]; then
                  echo "  ... ''${ELAPSED}s elapsed"
                fi
                if [ $((ELAPSED % 120)) -eq 0 ] && [ "$ELAPSED" -gt 0 ]; then
                  screendump "$VM_DIR/screen-''${ELAPSED}.ppm"
                fi

                sleep 5
                ELAPSED=$((ELAPSED + 5))
              done

              # Kill QEMU if still running
              if kill -0 "$QEMU_PID" 2>/dev/null; then
                kill "$QEMU_PID" 2>/dev/null || true
                wait "$QEMU_PID" 2>/dev/null || true
              fi

              echo ""
              echo "=== Serial output ==="
              cat "$SERIAL_LOG"
              echo ""
              echo "====================="

              if [ "$RESULT" = "pass" ]; then
                echo ""
                echo "PASSED: All wincrab modifications verified."
                exit 0
              elif [ "$RESULT" = "fail" ]; then
                echo ""
                echo "FAILED: Some modifications were not applied correctly."
                exit 1
              else
                echo ""
                echo "TIMEOUT: VM did not complete verification within ''${TIMEOUT}s."
                echo "Check $VM_DIR/ovmf.log for UEFI boot issues."
                exit 2
              fi
            '';
          };
        };

        lib = {
          inherit mkWindowsVM mkDebloatedISO;
        };

        apps = {
          wincrab = flake-utils.lib.mkApp {
            drv = wincrab;
          };
          test-vm = flake-utils.lib.mkApp {
            drv = self.packages.${system}.test-vm;
          };
          test-vm-headless = flake-utils.lib.mkApp {
            drv = self.packages.${system}.test-vm-headless;
          };
          test-e2e = flake-utils.lib.mkApp {
            drv = self.packages.${system}.test-e2e;
          };
          default = self.apps.${system}.wincrab;
        };

        devShells.default = craneLib.devShell {
          checks = self.checks.${system};

          packages = [
            # Runtime tools that wincrab orchestrates
            pkgs.p7zip
            pkgs.wimlib
            pkgs.hivex
            pkgs.xorriso
            pkgs.cdrtools
            pkgs.cdrkit
            pkgs.mtools
            pkgs.dosfstools
          ];
        };
      }
    );
}

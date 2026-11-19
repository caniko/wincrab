{ pkgs, installVM }:
pkgs.writeShellApplication {
  name = "test-install-e2e";
  runtimeInputs = [ pkgs.qemu pkgs.swtpm pkgs.coreutils pkgs.gnugrep ];
  text = ''
    set -euo pipefail
    if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
      echo "Usage: test-install-e2e <source.iso> [index]" >&2
      echo "The ISO must be a Pro edition built by 'wincrab build' (default profile):" >&2
      echo "the boot phase runs tests/vm/verify.ps1, which checks wincrab's debloat." >&2
      exit 1
    fi
    source_iso=$(realpath "$1")
    index="''${2:-6}"
    if [[ ! "$index" =~ ^[1-9][0-9]*$ ]] || [ ! -f "$source_iso" ]; then
      echo "Invalid source or image index" >&2
      exit 1
    fi
    work="''${WINCRAB_TEST_WORKDIR:-$(mktemp -d /tmp/wci.XXXX)}"
    mkdir -p "$work"
    work=$(realpath "$work")
    if [ -n "$(ls -A "$work")" ]; then
      echo "Work directory must be empty: $work" >&2
      exit 1
    fi
    qemu_pid=""
    tpm_pid=""
    # shellcheck disable=SC2329
    cleanup() {
      [ -z "$qemu_pid" ] || { kill "$qemu_pid" 2>/dev/null || true; wait "$qemu_pid" 2>/dev/null || true; }
      [ -z "$tpm_pid" ] || { kill "$tpm_pid" 2>/dev/null || true; wait "$tpm_pid" 2>/dev/null || true; }
      if [ "''${WINCRAB_TEST_KEEP:-}" = 1 ]; then
        echo "Keeping work dir: $work"
      else
        rm -rf -- "$work"
      fi
    }
    trap cleanup EXIT
    ln "$source_iso" "$work/source.iso" 2>/dev/null || cp "$source_iso" "$work/source.iso"
    cp ${./install-unattend.xml} "$work/install-unattend.xml"
    cp ${./verify.ps1} "$work/verify.ps1"
    echo "$index" > "$work/index"
    qemu-img create -f qcow2 "$work/target.qcow2" 64G
    echo "[1/2] Running Linux installer VM"
    export WINCRAB_INSTALL_SHARE="$work"
    export NIX_DISK_IMAGE="$work/root.qcow2"
    if ! (cd "$work" && timeout "''${WINCRAB_INSTALL_TIMEOUT:-1800}" ${installVM}/bin/run-wincrab-install-vm) > "$work/install.log" 2>&1; then
      echo "Installer VM failed or timed out" >&2
      tail -n 100 "$work/install.log" >&2
      exit 1
    fi
    if ! grep -qx WINCRAB_INSTALL_OK "$work/status" 2>/dev/null; then
      echo "Installer failed: $(cat "$work/status" 2>/dev/null || echo 'no status')" >&2
      tail -n 100 "$work/install.log" >&2
      exit 1
    fi
    echo "[2/2] Booting Windows target"
    mkdir -p "$work/tpm"
    swtpm socket --tpmstate dir="$work/tpm" --ctrl type=unixio,path="$work/tpm/sock" --tpm2 &
    tpm_pid=$!
    for _ in {1..50}; do
      [ -S "$work/tpm/sock" ] && break
      kill -0 "$tpm_pid" 2>/dev/null || { echo "swtpm failed to start" >&2; exit 1; }
      sleep 0.1
    done
    [ -S "$work/tpm/sock" ] || { echo "swtpm socket not ready" >&2; exit 1; }
    cp ${pkgs.OVMFFull.fd}/FV/OVMF_VARS.fd "$work/OVMF_VARS.fd"
    chmod u+w "$work/OVMF_VARS.fd"
    touch "$work/serial.log"
    qemu-system-x86_64 \
      -machine q35,accel=kvm -cpu host -smp "''${WINCRAB_VM_CPUS:-4}" -m "''${WINCRAB_VM_RAM:-8G}" \
      -drive "if=pflash,format=raw,readonly=on,file=${pkgs.OVMFFull.fd}/FV/OVMF_CODE.fd" \
      -drive "if=pflash,format=raw,file=$work/OVMF_VARS.fd" \
      -device ahci,id=ahci -drive "file=$work/target.qcow2,format=qcow2,if=none,id=hd0" \
      -device ide-hd,drive=hd0,bus=ahci.1 -boot c \
      -chardev "socket,id=chrtpm,path=$work/tpm/sock" -tpmdev emulator,id=tpm0,chardev=chrtpm \
      -device tpm-tis,tpmdev=tpm0 -device virtio-net-pci,netdev=net0 -netdev user,id=net0 \
      -display none -device virtio-vga -serial "file:$work/serial.log" \
      -monitor "unix:$work/monitor.sock,server,nowait" &
    qemu_pid=$!
    timeout_seconds="''${WINCRAB_TEST_TIMEOUT:-2700}"
    elapsed=0
    result=2
    while [ "$elapsed" -lt "$timeout_seconds" ]; do
      if grep -q WINCRAB_TEST_FAIL "$work/serial.log"; then result=1; break; fi
      if grep -q WINCRAB_TEST_PASS "$work/serial.log"; then result=0; break; fi
      if ! kill -0 "$qemu_pid" 2>/dev/null; then result=1; break; fi
      sleep 5
      elapsed=$((elapsed + 5))
    done
    cat "$work/serial.log"
    case "$result" in
      0) echo "PASSED: Windows verification" ;;
      1) echo "FAILED: Windows verification or VM exited" ;;
      2) echo "TIMEOUT: Windows verification" ;;
    esac
    exit "$result"
  '';
}

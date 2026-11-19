# wincrab

<!-- simit:badges:start -->

[![CI](https://img.shields.io/badge/CI-managed-2088ff)](.github/workflows/ci.yaml) ![artifacts](https://img.shields.io/badge/artifacts-configured-2ea44f)

<!-- simit:badges:end -->

A Rust CLI tool that builds debloated Windows 11 ISOs entirely from Linux.

wincrab orchestrates native tools to extract a stock Windows 11 ISO, strip
bloatware and telemetry, apply offline registry hardening, and repack the result
into a bootable UEFI ISO. On Linux it uses `7z`, `wimlib-imagex`, `hivexregedit`
and `genisoimage` (no Windows or DISM required); on Windows it uses `7z`,
`wimlib-imagex`, DISM, `reg` and `oscdimg`.

## Features

- **App pruning** — removes ~40 provisioned Appx packages (TikTok, Candy Crush,
  Clipchamp, Xbox, Teams, OneDrive, Cortana, Widgets, Solitaire, etc.)
- **Telemetry kill** — sets `AllowTelemetry=0`, disables CEIP, DiagTrack, and
  dmwappushservice
- **Copilot removal** — disables Windows Copilot via machine policy and default
  user profile
- **Visual performance** — disables animations, transparency, Aero Peek, lock
  screen spotlight, and suggestion notifications
- **Declarative config** — everything controlled via a single TOML file
- **Safe teardown** — RAII-based WIM mount guard guarantees `wimlib-imagex
  unmount` even on panic or early error return

## Prerequisites

On Linux, the following tools must be on `PATH`:

| Tool | Package | Purpose |
|---|---|---|
| `7z` | p7zip | ISO extraction |
| `wimlib-imagex` | wimlib | WIM mount/modify/commit via FUSE |
| `hivexregedit` | hivex | Offline Windows registry editing |
| `genisoimage` | cdrkit | UDF + El Torito (BIOS/UEFI) ISO repacking |
| `curl` | curl | Optional driver / Seelen-UI downloads |

Run `wincrab doctor` to check them.

### Platform support

| Host | Status |
|---|---|
| Linux | Supported (full pipeline) |
| Windows | Full pipeline; run as Administrator with 7-Zip, wimlib (Windows build), DISM, `reg`, oscdimg (Windows ADK), and curl on PATH |
| macOS | Planned |

If you use the Nix flake, all of these are provided automatically in the dev
shell.

## Build

```sh
# With Nix (recommended)
nix build

# With cargo directly (ensure prerequisites are installed)
cargo build --release
```

The binary is at `target/release/wincrab`.

## Usage

```sh
# Using built-in aggressive defaults
wincrab build --iso Win11_24H2_English_x64.iso --output Win11_debloated.iso

# Using a custom config
wincrab build --iso Win11_24H2_English_x64.iso --output Win11_debloated.iso \
        --config config.toml

# With debug logging
wincrab build --iso Win11_24H2_English_x64.iso --output Win11_debloated.iso -v
```

### Options

```
-i, --iso <ISO>            Path to the source Windows 11 ISO
-o, --output <OUTPUT>      Path to the output (debloated) ISO
-c, --config <CONFIG>      Path to a TOML configuration file (optional)
-w, --work-dir <WORK_DIR>  Working directory for staging files [default: wincrab-work]
-v, --verbose...           Increase log verbosity (-v = debug, -vv = trace)
```

## Direct install from Linux

`wincrab install` writes Windows straight onto a whole disk from a running Linux
system — no Windows Setup and no WinPE. It partitions the disk (260 MiB EFI
system partition, 16 MiB MSR, Windows on the rest), formats, applies the chosen
WIM image with `wimlib-imagex apply`, and installs the UEFI boot files and BCD
store with [BCD-SYS](https://github.com/jpz4085/BCD-SYS). An optional
`unattend.xml` is copied to `Windows/Panther/unattend.xml`.

**This wipes the entire disk** and must run as root. `--confirm-wipe` must name
the same disk as `--disk`.

Extra tools on Linux: `sgdisk` (gdisk), `partprobe` (parted), `mkfs.fat`
(dosfstools), `mkfs.ntfs` and `ntfs-3g` (ntfs-3g), `wimlib-imagex` (wimlib),
`7z` (p7zip) and `bcd-sys` (`nix build .#bcd-sys`); they are checked before the
disk is touched. Only x86_64 UEFI images are supported. Run `wincrab doctor` to
check them.

`nix run .#test-install-e2e -- <iso> [index]` exercises the whole flow in VMs.
It expects a Pro edition ISO built by `wincrab build` with the default profile,
because the boot check runs `tests/vm/verify.ps1`.

```sh
sudo wincrab install --source Win11_debloated.iso --disk /dev/sdX \
        --confirm-wipe /dev/sdX --unattend unattend.xml
```

`--source` may be an ISO or an `install.wim`/`install.esd`; `--index` selects
the image (default 6 = Windows 11 Pro). Drivers for the target hardware are not
staged — install them afterwards.

## Configuration

See `config.toml` for the full reference with comments. The defaults are
opinionated — they remove most bloat and disable telemetry. Notable knobs:

```toml
wim_index = 6  # 6 = Windows 11 Pro

[apps]
remove_bloatware = true
remove_xbox = true
remove_teams = true
remove_onedrive = true
remove_cortana = true
remove_store = false  # keep Store by default
extra_patterns = []   # add your own package name substrings

[telemetry]
disable = true

[copilot]
disable = true

[visuals]
optimize_for_performance = true

[services]
disable_diagtrack = true
disable_dmwappush = true
```

## Pipeline

wincrab runs a 5-phase pipeline:

1. **Extract** — `7z x` unpacks the ISO into a staging directory
2. **Mount** — `wimlib-imagex mountrw` FUSE-mounts the chosen WIM index
3. **Prune** — deletes matching Appx package directories from the mounted image
4. **Registry** — merges `.reg` edits into the offline `SOFTWARE`, `SYSTEM`, and
   `NTUSER.DAT` hives (`hivexregedit` on Linux, `reg load`/`import` on Windows)
5. **Repack** — `genisoimage` rebuilds a UDF + El Torito (BIOS and UEFI) ISO

If any phase fails, the WIM mount is automatically cleaned up without committing
changes.

## Project Structure

```
crates/
  wincrab/         CLI binary (clap + tracing)
  wincrab-core/    Library
    src/
      config.rs    Serde-based TOML configuration
      extract.rs   7z ISO extraction
      mount.rs     WimMount RAII guard with Drop cleanup
      debloat.rs   Appx package pruning
      registry.rs  Offline hivexsh registry edits
      repack.rs    genisoimage ISO rebuild
      pipeline.rs  5-phase orchestrator
      error.rs     Error types
```

## License

EUPL-1.2

# wincrab

<!-- simit:badges:start -->

[![CI](https://img.shields.io/badge/CI-managed-2088ff)](.github/workflows/ci.yaml) ![artifacts](https://img.shields.io/badge/artifacts-configured-2ea44f)

<!-- simit:badges:end -->

A Rust CLI tool that builds debloated Windows 11 ISOs entirely from Linux.

wincrab orchestrates Linux-native tools (`7z`, `wimlib-imagex`, `hivexsh`,
`xorriso`) to extract a stock Windows 11 ISO, strip bloatware and telemetry,
apply offline registry hardening, and repack the result into a bootable UEFI
ISO — no Windows or DISM required.

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

The following tools must be on `PATH`:

| Tool | Package | Purpose |
|---|---|---|
| `7z` | p7zip | ISO extraction |
| `wimlib-imagex` | wimlib | WIM mount/modify/commit via FUSE |
| `hivexsh` | hivex | Offline Windows registry editing |
| `xorriso` | xorriso | UEFI-bootable ISO repacking |

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
wincrab --iso Win11_24H2_English_x64.iso --output Win11_debloated.iso

# Using a custom config
wincrab --iso Win11_24H2_English_x64.iso --output Win11_debloated.iso \
        --config config.toml

# With debug logging
wincrab --iso Win11_24H2_English_x64.iso --output Win11_debloated.iso -v
```

### Options

```
-i, --iso <ISO>            Path to the source Windows 11 ISO
-o, --output <OUTPUT>      Path to the output (debloated) ISO
-c, --config <CONFIG>      Path to a TOML configuration file (optional)
-w, --work-dir <WORK_DIR>  Working directory for staging files [default: wincrab-work]
-v, --verbose...           Increase log verbosity (-v = debug, -vv = trace)
```

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
4. **Registry** — pipes hivexsh commands into offline `SOFTWARE`, `SYSTEM`, and
   `NTUSER.DAT` hives
5. **Repack** — `xorriso -as mkisofs` with UEFI boot flags rebuilds the ISO

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
      repack.rs    xorriso ISO rebuild
      pipeline.rs  5-phase orchestrator
      error.rs     Error types
```

## License

TBD

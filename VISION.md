# wincrab vision

`wincrab` builds debloated Windows 11 ISOs entirely from Linux. It orchestrates
native tools — `7z`, `wimlib-imagex`, `hivexsh`, `xorriso` — to extract a stock
ISO, remove provisioned bloatware, apply offline registry hardening, and repack
a bootable UEFI ISO, with no Windows host and no DISM. It also ships Nix
helpers that wrap the same work: a derivation that produces a debloated ISO and
a declarative QEMU Windows VM with USB/PCI passthrough, so the result can be
consumed without running the CLI by hand.

## Problem

Preparing a Windows install normally means booting Windows, clicking through
DISM and app removal, and trusting that the result is reproducible. On Linux
there is no supported path at all. `wincrab` makes the preparation a file
transformation: one ISO in, one ISO out, driven by a declarative TOML config,
with the same result on every run and no interactive Windows session.

## Principles

- **Linux-native, no Windows step.** The pipeline uses only documented Linux
  tools; the flake provides them, and `wincrab doctor` checks they exist and
  work before a build starts.
- **Configuration is declarative.** One TOML file controls the WIM index, app
  removal, telemetry, Copilot, visuals and services. `wincrab init` writes a
  commented default, `validate` checks a file, and named profiles
  (`wincrab profiles`) provide opinionated bases that a config file deep-merges
  over.
- **Failures must not leave a mounted image.** A `WimMount` RAII guard plus an
  installed panic hook guarantee `wimlib-imagex unmount` on panic or early
  error; a failed phase never commits partial changes.
- **Everything is inspectable before and after.** `inspect` reports ISO/WIM
  contents, `build --dry-run` shows what would change, and a manifest with
  SHA-256 hashes records the output.
- **Offline registry edits only.** Hives are changed through `hivexsh` on the
  mounted image, not by booting the system and running scripts.
- **Defaults are opinionated but reversible.** Bloatware, telemetry, Copilot
  and visual effects are removed by default; keeping the Store is the default
  exception, and every knob can be turned off in config.
- **The build is reproducible enough to verify.** A headless Windows VM test
  installs the result, runs `verify.ps1` against expected registry and app
  state, and reports pass/fail over a serial port.

## Scope

- The `wincrab` CLI: `build` (with `--profile` and `--dry-run`), `doctor`,
  `inspect`, `init`, `validate`, `profiles`, `completions`.
- ISO extraction, WIM mount, Appx pruning, offline registry edits, host
  blocking, OOBE and performance tweaks, driver and companion-app installation
  from GitHub releases, ESD/WIM handling, optional recompression, and UEFI
  repack.
- Profiles as reusable config bases merged with a user config.
- A manifest of the produced image and a panic-safe mount lifecycle.
- `lib/mkDebloatedISO.nix` and `lib/mkWindowsVM.nix` for Nix consumers,
  including an OVMF/QEMU/swtpm VM with configurable disk, RAM, CPUs, display,
  state directory and passthrough devices.
- Tests under `tests/vm`: an unattend file and a PowerShell verification script
  that writes results to COM1.

## Non-goals

- Supporting Windows hosts, or replacing DISM on Windows.
- Building a general Windows image-customization framework; the target is a
  Windows 11 install ISO.
- Shipping Microsoft ISOs or licensing Windows; the user supplies the source
  ISO, and large ISOs and work directories are gitignored external state.
- Interactive installation or post-install configuration of a running system;
  changes are applied offline to the image.

## Direction

The README describes the original five-phase pipeline, while the code has grown
past it: the core crate now also carries ESD/WIM handling, recompression,
driver and companion-app installation from GitHub releases, OOBE and host
blocking, performance tweaks, edition selection, inspection, manifests, hooks
and profiles, and the CLI gained `doctor`, `inspect`, `init`, `validate`,
`profiles`, `completions` and `--dry-run`. Recent commits are platform work:
harbor input migrations, portable sandbox caches for Crane checks, Attic
publication and Simit-generated CI.

## Place in the fleet

Canix lists the repository as a private project migrated to CodeFloe in its CLI
inventory exclusions and tracks it in the Simit updater; no canix module imports
it. The Nix library is the intended consumer surface for other flakes.

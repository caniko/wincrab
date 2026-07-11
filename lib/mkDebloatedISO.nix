# mkDebloatedISO — produce a debloated Windows 11 ISO as a Nix derivation.
#
# Usage from a consumer flake:
#   wincrab.lib.x86_64-linux.mkDebloatedISO {
#     sourceIso = ./Win11_24H2.iso;
#     # config = ./my-debloat.toml;  # optional, uses wincrab defaults
#   }
#
# Returns: a derivation whose output is the debloated ISO at $out/windows.iso

{ pkgs, wincrab }:

{
  sourceIso,
  config ? null,
  # WIM image index (default: 6 = Pro). Override to target a different edition.
  wimIndex ? null,
}:

let
  configArg =
    if config != null then
      "--config ${config}"
    else
      "";
in

pkgs.runCommand "debloated-windows-iso"
  {
    nativeBuildInputs = [
      wincrab
      pkgs.p7zip
      pkgs.wimlib
      pkgs.hivex
      pkgs.xorriso
      pkgs.cdrtools
      pkgs.cdrkit
      pkgs.mtools
      pkgs.dosfstools
    ];

    # Disable the sandbox network restriction — not needed, but this derivation
    # is I/O-heavy and may need significant /tmp space.
    __noChroot = false;
  }
  ''
    mkdir -p $out work

    wincrab -v build \
      --iso ${sourceIso} \
      --output $out/windows.iso \
      --work-dir ./work \
      ${configArg}
  ''

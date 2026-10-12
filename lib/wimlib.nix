# Wincrab uses native wimlib-imagex, never the x86-only mkwinpeimg helper.
{pkgs}: let
  inherit (pkgs) lib;
in
  if pkgs.stdenv.hostPlatform.isLinux && !pkgs.stdenv.hostPlatform.isx86
  then
    (pkgs.wimlib.override {syslinux = null;}).overrideAttrs (_: {
      # mkwinpeimg alone needs syslinux boot sectors. Keep the native WIM
      # library and imagex tools without advertising that unsupported helper.
      preBuild = "";
      postInstall = ''
        rm "$out/bin/mkwinpeimg"
        for prog in "$out"/bin/*; do
          wrapProgram "$prog" --prefix PATH : "$out/bin:${lib.makeBinPath [pkgs.cabextract pkgs.mtools pkgs.ntfs3g pkgs.cdrkit pkgs.fuse3]}"
        done
      '';
    })
  else pkgs.wimlib

{ nixpkgs, pkgs, wincrab, bcd-sys }:
(nixpkgs.lib.nixosSystem {
  system = "x86_64-linux";
  modules = [
    "${nixpkgs}/nixos/modules/virtualisation/qemu-vm.nix"
    ({ ... }: {
      system.stateVersion = "24.11";
      networking.hostName = "wincrab-install";
      virtualisation = {
        graphics = false;
        memorySize = 4096;
        cores = 4;
        diskSize = 4096;
        sharedDirectories.install = {
          source = ''"$WINCRAB_INSTALL_SHARE"'';
          target = "/mnt/share";
          writable = true;
        };
        qemu.options = [ ''-drive "file=$WINCRAB_INSTALL_SHARE/target.qcow2,if=virtio,format=qcow2"'' ];
      };
      environment.systemPackages = with pkgs; [
        wincrab bcd-sys gptfdisk dosfstools ntfs3g wimlib p7zip util-linux parted
      ];
      systemd.services.wincrab-install = {
        wantedBy = [ "multi-user.target" ];
        unitConfig.RequiresMountsFor = [ "/mnt/share" ];
        path = with pkgs; [ wincrab bcd-sys gptfdisk dosfstools ntfs3g wimlib p7zip util-linux parted systemd ];
        serviceConfig = {
          Type = "oneshot";
          StandardOutput = "journal+console";
          StandardError = "journal+console";
        };
        script = ''
          set -eu
          finish() {
            rc=$?
            if [ "$rc" -eq 0 ]; then status=WINCRAB_INSTALL_OK; else status="WINCRAB_INSTALL_FAIL rc=$rc"; fi
            echo "$status" | tee /mnt/share/status /dev/ttyS0 || true
            systemctl --no-block poweroff
          }
          trap finish EXIT
          lsblk
          test -b /dev/vdb
          index=$(cat /mnt/share/index)
          wincrab install --source /mnt/share/source.iso --index "$index" --disk /dev/vdb \
            --confirm-wipe /dev/vdb --unattend /mnt/share/install-unattend.xml --work-dir /mnt/share/work
          mkdir -p /mnt/windows
          ntfs-3g /dev/vdb3 /mnt/windows
          cp /mnt/share/verify.ps1 /mnt/windows/wincrab-verify.ps1
          umount /mnt/windows
        '';
      };
    })
  ];
}).config.system.build.vm

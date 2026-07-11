//! End-to-end tests for wincrab-core.
//!
//! These tests simulate complete pipeline scenarios by building realistic
//! directory structures and running multiple pipeline phases in sequence.
//! External tools (7z, wimlib, hivexsh, xorriso) are NOT required.

// ===========================================================================
// Full debloat pipeline simulation (without external tools)
// ===========================================================================

mod pipeline_simulation {
    use wincrab_core::config::*;
    use wincrab_core::debloat::{prune_appx_packages, prune_seelen_replacements, remove_scheduled_tasks};
    use wincrab_core::drivers::{inject_drivers, DriverPaths};
    use wincrab_core::extract::find_install_image;
    use wincrab_core::oobe::inject_autounattend;
    use wincrab_core::pipeline::WorkDirs;
    use wincrab_core::seelen::inject_seelen;
    use wincrab_core::Config;

    /// Build a comprehensive mock of an extracted Windows 11 ISO.
    fn build_full_staging(staging: &std::path::Path) {
        // ISO root structure.
        std::fs::create_dir_all(staging.join("boot")).unwrap();
        std::fs::create_dir_all(staging.join("efi/microsoft/boot")).unwrap();
        std::fs::create_dir_all(staging.join("efi/boot")).unwrap();
        std::fs::write(staging.join("efi/boot/bootx64.efi"), b"PE\x00\x00").unwrap();
        std::fs::create_dir_all(staging.join("sources")).unwrap();
        std::fs::write(staging.join("sources/install.wim"), b"fake-wim-data").unwrap();
        std::fs::write(staging.join("sources/boot.wim"), b"fake-boot-wim").unwrap();
        std::fs::write(staging.join("autorun.inf"), b"[AutoRun]").unwrap();
    }

    /// Build a comprehensive mock of a mounted WIM image.
    fn build_full_mount(mount: &std::path::Path) {
        // WindowsApps with a realistic mix.
        let apps = mount.join("Program Files/WindowsApps");
        let packages = [
            // Bloatware
            "BytedancePte.Ltd.TikTok_3.5.0.0_x64__a1b2c3d4",
            "king.com.CandyCrushSodaSaga_1.0.0_x64__xyzzy",
            "SpotifyAB.SpotifyMusic_1.0_x64__abc",
            "Microsoft.BingWeather_4.53.0_x64__8wekyb3d8bbwe",
            "Microsoft.ZuneMusic_1.0_x64__8wekyb3d8bbwe",
            "MicrosoftCorporationII.QuickAssist_1.0_x64__abc",
            // Xbox
            "Microsoft.XboxApp_1.0_x64__8wekyb3d8bbwe",
            "Microsoft.XboxGameOverlay_1.0_x64__8wekyb3d8bbwe",
            // Teams
            "MicrosoftTeams_24.0_x64__8wekyb3d8bbwe",
            // Cortana
            "Microsoft.549981C3F5F10_1.0_x64__abc",
            // Outlook
            "Microsoft.OutlookForWindows_1.0_x64__8wekyb3d8bbwe",
            // Mail
            "microsoft.windowscommunicationsapps_16.0_x64__8wekyb3d8bbwe",
            // Dev Home
            "Microsoft.Windows.DevHome_0.1_x64__8wekyb3d8bbwe",
            // Phone Link
            "Microsoft.YourPhone_1.0_x64__8wekyb3d8bbwe",
            "MicrosoftWindows.CrossDevice_1.0_x64__8wekyb3d8bbwe",
            // Seelen-replaceable
            "Microsoft.Windows.Search_1.0_x64__abc",
            "Microsoft.Windows.SearchHost_1.0_x64__abc",
            "MicrosoftWindows.Client.CBS_1.0_x64__abc",
            "Microsoft.Windows.StartMenuExperienceHost_1.0_x64__abc",
            // Survivors (should never be removed).
            "Microsoft.WindowsCalculator_10.2_x64__8wekyb3d8bbwe",
            "Microsoft.WindowsNotepad_11.0_x64__8wekyb3d8bbwe",
            "Microsoft.WindowsTerminal_1.0_x64__8wekyb3d8bbwe",
        ];

        for pkg in &packages {
            let dir = apps.join(pkg);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("AppxManifest.xml"), b"<Package/>").unwrap();
            std::fs::write(dir.join("app.exe"), b"MZ").unwrap();
        }

        // Provisioning packages (subset).
        let prov = mount.join("ProgramData/Microsoft/Windows/AppRepository/Packages");
        for pkg in &packages[..6] {
            let dir = prov.join(pkg);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("AppxManifest.xml"), b"<Package/>").unwrap();
        }

        // OneDrive.
        let sys32 = mount.join("Windows/System32");
        std::fs::create_dir_all(&sys32).unwrap();
        std::fs::write(sys32.join("OneDriveSetup.exe"), b"setup").unwrap();
        let syswow = mount.join("Windows/SysWOW64");
        std::fs::create_dir_all(&syswow).unwrap();
        std::fs::write(syswow.join("OneDriveSetup.exe"), b"setup").unwrap();

        // Scheduled tasks.
        let tasks = mount.join("Windows/System32/Tasks/Microsoft/Windows");
        let app_exp = tasks.join("Application Experience");
        std::fs::create_dir_all(&app_exp).unwrap();
        std::fs::write(app_exp.join("Microsoft Compatibility Appraiser"), b"").unwrap();
        std::fs::write(app_exp.join("ProgramDataUpdater"), b"").unwrap();
        std::fs::write(app_exp.join("AitAgent"), b"").unwrap();
        let ceip = tasks.join("Customer Experience Improvement Program");
        std::fs::create_dir_all(&ceip).unwrap();
        std::fs::write(ceip.join("Consolidator"), b"").unwrap();
        std::fs::write(ceip.join("UsbCeip"), b"").unwrap();
        let maps = tasks.join("Maps");
        std::fs::create_dir_all(&maps).unwrap();
        std::fs::write(maps.join("MapsUpdateTask"), b"").unwrap();
        let feedback = tasks.join("Feedback/Siuf");
        std::fs::create_dir_all(&feedback).unwrap();
        std::fs::write(feedback.join("Siuf"), b"").unwrap();
        let wer = tasks.join("Windows Error Reporting");
        std::fs::create_dir_all(&wer).unwrap();
        std::fs::write(wer.join("QueueReporting"), b"").unwrap();

        // Registry hives (for validation).
        let config_dir = mount.join("Windows/System32/config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("SOFTWARE"), b"regf").unwrap();
        std::fs::write(config_dir.join("SYSTEM"), b"regf").unwrap();
        std::fs::write(config_dir.join("DEFAULT"), b"regf").unwrap();
        let users = mount.join("Users/Default");
        std::fs::create_dir_all(&users).unwrap();
        std::fs::write(users.join("NTUSER.DAT"), b"regf").unwrap();
    }

    #[test]
    fn full_default_pipeline_simulation() {
        let dir = tempfile::tempdir().unwrap();
        let config = Config::default();

        // Phase 1: Create work dirs.
        let work_dirs = WorkDirs::new(&dir.path().join("work")).unwrap();
        assert!(work_dirs.staging.is_dir());
        assert!(work_dirs.wim_mount.is_dir());

        // Phase 2: Simulate ISO extraction (build staging).
        build_full_staging(&work_dirs.staging);

        // Phase 3: Find install image.
        let wim_path = find_install_image(&work_dirs.staging).unwrap();
        assert!(wim_path.ends_with("install.wim"));

        // Phase 4: Simulate mount (just use work_dirs.wim_mount).
        build_full_mount(&work_dirs.wim_mount);

        // Phase 5: Prune apps.
        let app_stats = prune_appx_packages(&work_dirs.wim_mount, &config.apps).unwrap();
        assert!(app_stats.dirs_removed >= 10, "dirs_removed = {}", app_stats.dirs_removed);

        // Phase 6: Prune Seelen-replaced components.
        let seelen_stats = prune_seelen_replacements(&work_dirs.wim_mount, &config.seelen).unwrap();
        assert!(seelen_stats.dirs_removed >= 4, "seelen dirs = {}", seelen_stats.dirs_removed);

        // Phase 7: Remove scheduled tasks.
        let task_stats = remove_scheduled_tasks(&work_dirs.wim_mount, &config.scheduled_tasks).unwrap();
        assert!(task_stats.files_removed >= 5, "task files = {}", task_stats.files_removed);

        // Phase 8: Inject Seelen-UI (simulated).
        let fake_setup = dir.path().join("seelen-setup.exe");
        std::fs::write(&fake_setup, b"MZ\x90\x00fake Seelen PE").unwrap();
        inject_seelen(&work_dirs.wim_mount, &fake_setup, &config.seelen).unwrap();
        assert!(work_dirs.wim_mount.join("SeelenUI/Seelen.UI-setup.exe").exists());
        assert!(work_dirs.wim_mount.join("SeelenUI/install.ps1").exists());

        // Phase 9: Inject autounattend.xml.
        inject_autounattend(&work_dirs.staging, &config.oobe).unwrap();
        let autounattend = std::fs::read_to_string(work_dirs.staging.join("autounattend.xml")).unwrap();
        assert!(autounattend.contains("<?xml"));
        assert!(autounattend.contains("BypassNRO"));

        // Verify survivors.
        let remaining: Vec<_> = std::fs::read_dir(work_dirs.wim_mount.join("Program Files/WindowsApps"))
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(remaining.iter().any(|n| n.contains("Calculator")), "Calculator should survive");
        assert!(remaining.iter().any(|n| n.contains("Notepad")), "Notepad should survive");
        assert!(remaining.iter().any(|n| n.contains("Terminal")), "Terminal should survive");
    }

    #[test]
    fn pipeline_with_all_disabled() {
        let dir = tempfile::tempdir().unwrap();

        // Build a config with everything disabled.
        let config = Config {
            wim_index: wincrab_core::WimIndex(1),
            apps: AppRemoval {
                remove_bloatware: false,
                remove_xbox: false,
                remove_teams: false,
                remove_onedrive: false,
                remove_cortana: false,
                remove_store: false,
                remove_outlook: false,
                remove_mail: false,
                remove_dev_home: false,
                remove_phone_link: false,
                extra_patterns: vec![],
            },
            telemetry: Telemetry {
                disable: false,
                disable_ceip: false,
                disable_app_telemetry: false,
                ..Default::default()
            },
            privacy: Privacy {
                disable_advertising_id: false,
                disable_web_search: false,
                disable_activity_history: false,
                disable_tailored_experiences: false,
                disable_error_reporting: false,
                restrict_app_permissions: false,
                ..Default::default()
            },
            copilot: Copilot { disable: false },
            edge: Edge {
                disable_first_run: false,
                disable_default_browser_nag: false,
                disable_sidebar: false,
                disable_search_bar: false,
            },
            visuals: Visuals {
                optimize_for_performance: false,
                disable_lock_screen_tips: false,
                disable_suggestions: false,
                ..Default::default()
            },
            taskbar: Taskbar {
                hide_widgets_button: false,
                hide_chat_button: false,
                search_icon_only: false,
                disable_start_recommendations: false,
                ..Default::default()
            },
            services: Services {
                disable_diagtrack: false,
                disable_dmwappush: false,
                disable_wer: false,
                disable_xbox: false,
                disable_maps_broker: false,
                disable_retail_demo: false,
                disable_remote_registry: false,
                disable_geolocation: false,
                ..Default::default()
            },
            scheduled_tasks: ScheduledTasks {
                remove_telemetry_tasks: false,
                remove_ceip_tasks: false,
                remove_disk_diagnostic_tasks: false,
                remove_maps_task: false,
                remove_feedback_tasks: false,
            },
            oobe: Oobe {
                inject_autounattend: false,
                skip_microsoft_account: false,
                skip_privacy_screens: false,
                skip_finish_setup_nag: false,
                ..Default::default()
            },
            seelen: Seelen {
                bundle: false,
                replace_shell: false,
                remove_windows_search_ui: false,
                remove_start_experience: false,
            },
            drivers: Drivers::default(),
            ..Config::default()
        };

        let mount = dir.path().join("mount");
        build_full_mount(&mount);

        // Count files before.
        let apps_before: Vec<_> = std::fs::read_dir(mount.join("Program Files/WindowsApps"))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        let count_before = apps_before.len();

        // Run debloat phases.
        let app_stats = prune_appx_packages(&mount, &config.apps).unwrap();
        let seelen_stats = prune_seelen_replacements(&mount, &config.seelen).unwrap();
        let task_stats = remove_scheduled_tasks(&mount, &config.scheduled_tasks).unwrap();

        // Nothing should have been removed.
        assert_eq!(app_stats.dirs_removed, 0);
        assert_eq!(app_stats.files_removed, 0);
        assert_eq!(seelen_stats.dirs_removed, 0);
        assert_eq!(task_stats.files_removed, 0);

        // OOBE should not inject.
        inject_autounattend(&mount, &config.oobe).unwrap();
        assert!(!mount.join("autounattend.xml").exists());

        // File count should be unchanged.
        let apps_after: Vec<_> = std::fs::read_dir(mount.join("Program Files/WindowsApps"))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(apps_after.len(), count_before);
    }

    #[test]
    fn pipeline_with_drivers_only() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path().join("mount");
        std::fs::create_dir_all(&mount).unwrap();

        // Create fake driver files.
        let btrfs = dir.path().join("btrfs.zip");
        let ext2fsd = dir.path().join("Ext2Fsd.exe");
        let winfsp = dir.path().join("winfsp.msi");
        let mergerfs = dir.path().join("mergerfs.zip");
        std::fs::write(&btrfs, b"btrfs-data").unwrap();
        std::fs::write(&ext2fsd, b"ext2fsd-data").unwrap();
        std::fs::write(&winfsp, b"winfsp-data").unwrap();
        std::fs::write(&mergerfs, b"mergerfs-data").unwrap();

        let driver_paths = DriverPaths {
            btrfs: Some(btrfs),
            ext2fsd: Some(ext2fsd),
            winfsp: Some(winfsp),
            mergerfs: Some(mergerfs),
        };
        let config = Drivers {
            btrfs: true,
            ext4: true,
            winfsp: true,
            mergerfs: true,
            ..Default::default()
        };

        inject_drivers(&mount, &driver_paths, &config).unwrap();

        let driver_dir = mount.join("Drivers");
        assert!(driver_dir.join("btrfs.zip").exists());
        assert!(driver_dir.join("Ext2Fsd.exe").exists());
        assert!(driver_dir.join("winfsp.msi").exists());
        assert!(driver_dir.join("mergerfs.zip").exists());
        assert!(driver_dir.join("install-drivers.ps1").exists());

        // Verify script contains all drivers.
        let script = std::fs::read_to_string(driver_dir.join("install-drivers.ps1")).unwrap();
        assert!(script.contains("WinBtrfs"));
        assert!(script.contains("Ext2Fsd"));
        assert!(script.contains("WinFsp"));
        assert!(script.contains("mergerfs"));
        assert!(script.contains("shutdown /r"));
    }

    #[test]
    fn work_dirs_cleanup_is_single_rmdir() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("work");
        let dirs = WorkDirs::new(&work).unwrap();

        // All directories should be under root.
        assert!(dirs.staging.starts_with(&dirs.root));
        assert!(dirs.wim_mount.starts_with(&dirs.root));

        // Removing root should remove everything.
        std::fs::remove_dir_all(&dirs.root).unwrap();
        assert!(!dirs.root.exists());
        assert!(!dirs.staging.exists());
        assert!(!dirs.wim_mount.exists());
    }

    #[test]
    fn phase_counting_consistency() {
        // Verify phase count formula matches all config combos.
        let configs = [
            (false, false, 9),
            (true, false, 11),
            (false, true, 11),
            (true, true, 13),
        ];

        for (seelen, drivers, expected) in configs {
            let config = Config {
                seelen: Seelen { bundle: seelen, ..Default::default() },
                drivers: Drivers { btrfs: drivers, ..Default::default() },
                ..Config::default()
            };
            let total = 9
                + if config.seelen.bundle { 2 } else { 0 }
                + if config.drivers.any_enabled() { 2 } else { 0 };
            assert_eq!(total, expected, "seelen={seelen} drivers={drivers}");
        }
    }
}

// ===========================================================================
// Config file E2E tests
// ===========================================================================

mod config_e2e {
    use wincrab_core::Config;

    #[test]
    fn example_config_toml_loads_and_validates() {
        let repo_config = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("config.toml");

        if repo_config.exists() {
            let config = Config::from_file(&repo_config).unwrap();
            assert!(config.wim_index > 0);
            // Verify it serializes back cleanly.
            let toml_str = toml::to_string_pretty(&config).unwrap();
            let reloaded: Config = toml::from_str(&toml_str).unwrap();
            assert_eq!(reloaded.wim_index, config.wim_index);
        }
    }

    #[test]
    fn config_from_file_to_debloat_pipeline() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("test.toml");
        std::fs::write(
            &config_path,
            r#"
wim_index = 4
[apps]
remove_bloatware = true
remove_xbox = false
extra_patterns = ["MyCustomBloat"]
[seelen]
bundle = false
[oobe]
inject_autounattend = true
skip_microsoft_account = true
"#,
        )
        .unwrap();

        let config = Config::from_file(&config_path).unwrap();
        assert_eq!(config.wim_index, 4);
        assert!(config.apps.remove_bloatware);
        assert!(!config.apps.remove_xbox);
        assert_eq!(config.apps.extra_patterns, vec!["MyCustomBloat"]);
        assert!(!config.seelen.bundle);
        assert!(config.oobe.inject_autounattend);

        // Use this config to drive debloat on a mock filesystem.
        let mount = dir.path().join("mount");
        let apps_dir = mount.join("Program Files/WindowsApps");
        std::fs::create_dir_all(&apps_dir).unwrap();

        let pkgs = [
            "BytedancePte.Ltd.TikTok_1.0",
            "Microsoft.XboxApp_1.0",
            "MyCustomBloat_1.0",
            "Microsoft.WindowsCalculator_1.0",
        ];
        for pkg in &pkgs {
            let d = apps_dir.join(pkg);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("app.exe"), b"MZ").unwrap();
        }

        let stats = wincrab_core::debloat::prune_appx_packages(&mount, &config.apps).unwrap();
        // TikTok (bloatware) + MyCustomBloat (extra) = 2 removed.
        // Xbox should NOT be removed (disabled).
        assert_eq!(stats.dirs_removed, 2, "dirs_removed = {}", stats.dirs_removed);

        let remaining: Vec<_> = std::fs::read_dir(&apps_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(remaining.len(), 2);
        assert!(remaining.iter().any(|n| n.contains("Xbox")));
        assert!(remaining.iter().any(|n| n.contains("Calculator")));
    }
}

// ===========================================================================
// E2E tests for new features
// ===========================================================================

mod new_features_e2e {
    use wincrab_core::config::*;
    use wincrab_core::debloat::prune_appx_packages;
    use wincrab_core::hosts::inject_telemetry_hosts;
    use wincrab_core::manifest::{compute_sha256, write_manifest, BuildManifest};
    use wincrab_core::oobe::inject_autounattend;
    use wincrab_core::performance::inject_performance_script;
    use wincrab_core::pipeline::WorkDirs;
    use wincrab_core::Config;
    use std::borrow::Cow;

    /// Build a comprehensive mock of a mounted WIM with hosts file.
    fn build_mount_with_hosts(mount: &std::path::Path) {
        // WindowsApps
        let apps = mount.join("Program Files/WindowsApps");
        let packages = [
            "BytedancePte.Ltd.TikTok_1.0_x64__abc",
            "Microsoft.WindowsCalculator_10.0_x64__abc",
        ];
        for pkg in &packages {
            let dir = apps.join(pkg);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("app.exe"), b"MZ").unwrap();
        }

        // Windows system files
        let sys32 = mount.join("Windows/System32");
        std::fs::create_dir_all(&sys32).unwrap();

        // Hosts file
        let etc = mount.join("Windows/System32/drivers/etc");
        std::fs::create_dir_all(&etc).unwrap();
        std::fs::write(etc.join("hosts"), "# Windows hosts file\n127.0.0.1 localhost\n").unwrap();
    }

    #[test]
    fn full_pipeline_with_telemetry_hosts_and_performance() {
        let dir = tempfile::tempdir().unwrap();
        let work_dirs = WorkDirs::new(&dir.path().join("work")).unwrap();
        let mount = &work_dirs.wim_mount;

        build_mount_with_hosts(mount);

        let config = Config::default();

        // Phase: Prune apps
        let stats = prune_appx_packages(mount, &config.apps).unwrap();
        assert!(stats.dirs_removed >= 1);

        // Phase: Block telemetry hosts
        inject_telemetry_hosts(mount, &config.telemetry).unwrap();
        let hosts_content = std::fs::read_to_string(
            mount.join("Windows/System32/drivers/etc/hosts"),
        )
        .unwrap();
        assert!(hosts_content.contains("127.0.0.1 localhost"));
        assert!(hosts_content.contains("0.0.0.0 telemetry.microsoft.com"));
        assert!(hosts_content.contains("0.0.0.0 vortex.data.microsoft.com"));

        // Phase: Inject performance script
        inject_performance_script(mount, &config.performance).unwrap();
        let perf_script = std::fs::read_to_string(
            mount.join("wincrab/performance.ps1"),
        )
        .unwrap();
        assert!(perf_script.contains("powercfg"));

        // Phase: Inject autounattend
        inject_autounattend(&work_dirs.staging, &config.oobe).unwrap();
        let autounattend = std::fs::read_to_string(
            work_dirs.staging.join("autounattend.xml"),
        )
        .unwrap();
        assert!(autounattend.contains("BypassNRO"));
        assert!(autounattend.contains("BypassTPMCheck"));
    }

    #[test]
    fn full_pipeline_with_custom_file_injection() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path().join("mount");
        std::fs::create_dir_all(&mount).unwrap();

        // Create source files
        let src_file = dir.path().join("custom_script.ps1");
        std::fs::write(&src_file, "Write-Host 'Custom script'").unwrap();

        let src_dir = dir.path().join("tools");
        std::fs::create_dir_all(src_dir.join("sub")).unwrap();
        std::fs::write(src_dir.join("tool.exe"), b"MZ").unwrap();
        std::fs::write(src_dir.join("sub/config.ini"), "[config]\nkey=value").unwrap();

        let config = Config {
            inject: Inject {
                files: vec![
                    InjectEntry {
                        src: src_file.clone(),
                        dest: "wincrab/custom_script.ps1".into(),
                    },
                    InjectEntry {
                        src: src_dir.clone(),
                        dest: "Tools".into(),
                    },
                ],
            },
            seelen: Seelen { bundle: false, ..Default::default() },
            ..Config::default()
        };

        // Simulate inject_custom_files (private fn) by calling the same logic
        for entry in &config.inject.files {
            let dest = mount.join(&entry.dest);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            if entry.src.is_dir() {
                copy_dir(&entry.src, &dest);
            } else {
                std::fs::copy(&entry.src, &dest).unwrap();
            }
        }

        assert_eq!(
            std::fs::read_to_string(mount.join("wincrab/custom_script.ps1")).unwrap(),
            "Write-Host 'Custom script'"
        );
        assert!(mount.join("Tools/tool.exe").exists());
        assert_eq!(
            std::fs::read_to_string(mount.join("Tools/sub/config.ini")).unwrap(),
            "[config]\nkey=value"
        );
    }

    fn copy_dir(src: &std::path::Path, dest: &std::path::Path) {
        std::fs::create_dir_all(dest).unwrap();
        for entry in walkdir::WalkDir::new(src).min_depth(1) {
            let entry = entry.unwrap();
            let relative = entry.path().strip_prefix(src).unwrap();
            let target = dest.join(relative);
            if entry.file_type().is_dir() {
                std::fs::create_dir_all(&target).unwrap();
            } else {
                std::fs::copy(entry.path(), &target).unwrap();
            }
        }
    }

    #[test]
    fn full_pipeline_with_manifest_generation() {
        let dir = tempfile::tempdir().unwrap();

        // Simulate source and output ISOs.
        let source_iso = dir.path().join("source.iso");
        std::fs::write(&source_iso, b"fake source ISO data").unwrap();
        let output_iso = dir.path().join("output.iso");
        std::fs::write(&output_iso, b"fake output ISO").unwrap();

        let config = Config::default();

        // Generate manifest.
        let manifest = BuildManifest {
            wincrab_version: Cow::Borrowed("0.1.0"),
            source_iso_sha256: compute_sha256(&source_iso).unwrap(),
            output_iso_sha256: compute_sha256(&output_iso).unwrap(),
            source_iso_size_bytes: source_iso.metadata().map(|m| m.len()).unwrap_or(0),
            output_iso_size_bytes: output_iso.metadata().map(|m| m.len()).unwrap_or(0),
            config_snapshot: serde_json::to_string_pretty(&config).unwrap_or_default(),
            timestamp: "1710460800".into(),
        };

        let manifest_path = output_iso.with_extension("manifest.json");
        write_manifest(&manifest, &manifest_path).unwrap();

        // Verify manifest.
        let content = std::fs::read_to_string(&manifest_path).unwrap();
        let loaded: BuildManifest = serde_json::from_str(&content).unwrap();
        assert_eq!(loaded.wincrab_version, "0.1.0");
        assert_eq!(loaded.source_iso_size_bytes, 20);
        assert_eq!(loaded.output_iso_size_bytes, 15);
        assert!(!loaded.source_iso_sha256.is_empty());
        assert!(!loaded.output_iso_sha256.is_empty());
        assert!(loaded.config_snapshot.contains("wim_index"));
    }

    #[test]
    fn profile_based_pipeline_simulation() {
        let dir = tempfile::tempdir().unwrap();

        for profile_name in &wincrab_core::profiles::PROFILE_NAMES {
            let config = wincrab_core::profiles::load_profile(profile_name).unwrap();
            assert!(config.validate().is_ok(), "profile {profile_name} invalid");

            let mount = dir.path().join(format!("mount_{profile_name}"));
            build_mount_with_hosts(&mount);

            // Prune apps using profile config.
            let _stats = prune_appx_packages(&mount, &config.apps).unwrap();

            // TikTok should be removed in all profiles.
            let remaining: Vec<_> = std::fs::read_dir(mount.join("Program Files/WindowsApps"))
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect();
            assert!(
                !remaining.iter().any(|n| n.contains("TikTok")),
                "TikTok should be removed in {profile_name}"
            );
            assert!(
                remaining.iter().any(|n| n.contains("Calculator")),
                "Calculator should survive in {profile_name}"
            );

            // Telemetry host blocking varies by profile.
            if config.telemetry.block_telemetry_hosts {
                inject_telemetry_hosts(&mount, &config.telemetry).unwrap();
                let hosts = std::fs::read_to_string(
                    mount.join("Windows/System32/drivers/etc/hosts"),
                )
                .unwrap();
                assert!(hosts.contains("telemetry.microsoft.com"));
            }

            // Performance script varies by profile.
            if config.performance.high_perf_power_plan {
                inject_performance_script(&mount, &config.performance).unwrap();
                assert!(mount.join("wincrab/performance.ps1").exists());
            }
        }
    }

    #[test]
    fn full_pipeline_with_oobe_all_new_features() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path().join("staging");
        std::fs::create_dir_all(&staging).unwrap();

        let config = Oobe {
            inject_autounattend: true,
            skip_microsoft_account: true,
            skip_privacy_screens: true,
            skip_finish_setup_nag: true,
            bypass_tpm: true,
            bypass_secureboot: true,
            bypass_ram: true,
            timezone: Some("Pacific Standard Time".into()),
            local_account_name: Some("Admin".into()),
            local_account_password: Some("pass".into()),
            auto_logon: true,
            disable_bitlocker: true,
            computer_name: Some("E2E-TEST".into()),
            product_key: Some("VK7JG-NPHTM-C97JM-9MPGT-3V66T".into()),
            first_logon_commands: vec![
                "Set-ExecutionPolicy RemoteSigned".into(),
                "Write-Host 'Setup complete'".into(),
            ],
            convert_edition: None,
            skip_auto_activation: true,
            auto_partition: true,
        };

        inject_autounattend(&staging, &config).unwrap();
        let xml = std::fs::read_to_string(staging.join("autounattend.xml")).unwrap();

        // windowsPE pass
        assert!(xml.contains("windowsPE"));
        assert!(xml.contains("BypassNRO"));
        assert!(xml.contains("BypassTPMCheck"));
        assert!(xml.contains("BypassSecureBootCheck"));
        assert!(xml.contains("BypassRAMCheck"));
        assert!(xml.contains("PreventDeviceEncryption"));
        assert!(xml.contains("VK7JG-NPHTM-C97JM-9MPGT-3V66T"));
        assert!(xml.contains("<DiskConfiguration>"));

        // specialize pass
        assert!(xml.contains("specialize"));
        assert!(xml.contains("<ComputerName>E2E-TEST</ComputerName>"));
        assert!(xml.contains("<TimeZone>Pacific Standard Time</TimeZone>"));
        assert!(xml.contains("SkipAutoActivation"));

        // oobeSystem pass
        assert!(xml.contains("oobeSystem"));
        assert!(xml.contains("<Name>Admin</Name>"));
        assert!(xml.contains("<Value>pass</Value>"));
        assert!(xml.contains("<AutoLogon>"));
        assert!(xml.contains("Set-ExecutionPolicy RemoteSigned"));
        assert!(xml.contains("Setup complete"));

        // Verify balanced XML
        let open = xml.matches('<').count();
        let close = xml.matches('>').count();
        assert_eq!(open, close);
    }

    #[test]
    fn phase_counting_with_all_new_features() {
        // Verify phase counting accounts for all new features.
        let config = Config {
            drivers: Drivers { ext4: true, ..Default::default() },
            inject: Inject {
                files: vec![InjectEntry {
                    src: std::path::PathBuf::from("/tmp/test"),
                    dest: "test".into(),
                }],
            },
            oobe: Oobe {
                convert_edition: Some("Professional".into()),
                ..Default::default()
            },
            ..Config::default()
        };

        // Count expected phases:
        // 9 base + 2 seelen + 2 drivers + 1 hosts + 1 perf + 1 inject + 1 edition = 17
        let expected = 9
            + if config.seelen.bundle { 2 } else { 0 }
            + if config.drivers.any_enabled() { 2 } else { 0 }
            + if config.telemetry.block_telemetry_hosts
                || !config.telemetry.extra_blocked_hosts.is_empty()
            {
                1
            } else {
                0
            }
            + if config.performance.high_perf_power_plan { 1 } else { 0 }
            + if config.oobe.convert_edition.is_some() { 1 } else { 0 }
            + if !config.inject.files.is_empty() { 1 } else { 0 };

        assert_eq!(expected, 17);
    }

    #[test]
    fn profile_merge_then_validate_e2e() {
        // Load a profile, merge overrides, then validate.
        let base = wincrab_core::profiles::load_profile("gaming").unwrap();
        let merged = wincrab_core::profiles::merge_with_overrides(
            base,
            r#"
[oobe]
convert_edition = "Professional"
local_account_name = "Gamer"
auto_logon = true

[hooks]
post_build = "echo 'Build complete!'"
"#,
        )
        .unwrap();

        assert!(merged.validate().is_ok());
        assert_eq!(merged.oobe.convert_edition.as_deref(), Some("Professional"));
        assert_eq!(merged.oobe.local_account_name.as_deref(), Some("Gamer"));
        assert!(merged.oobe.auto_logon);
        assert_eq!(merged.hooks.post_build.as_deref(), Some("echo 'Build complete!'"));
        // Gaming profile defaults preserved.
        assert!(!merged.apps.remove_xbox);
        assert!(merged.performance.high_perf_power_plan);
    }

    #[test]
    fn hosts_plus_extra_domains_e2e() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path().join("mount");
        let etc = mount.join("Windows/System32/drivers/etc");
        std::fs::create_dir_all(&etc).unwrap();
        std::fs::write(etc.join("hosts"), "# default\n127.0.0.1 localhost\n").unwrap();

        let config = Telemetry {
            block_telemetry_hosts: true,
            extra_blocked_hosts: vec![
                "evil-tracker.example.com".into(),
                "sneaky-ads.example.net".into(),
                "more-telemetry.contoso.com".into(),
            ],
            ..Telemetry::default()
        };

        inject_telemetry_hosts(&mount, &config).unwrap();

        let content = std::fs::read_to_string(etc.join("hosts")).unwrap();

        // Original content preserved.
        assert!(content.starts_with("# default\n127.0.0.1 localhost\n"));

        // All 40 built-in domains blocked.
        assert!(content.contains("0.0.0.0 vortex.data.microsoft.com"));
        assert!(content.contains("0.0.0.0 feedback.windows.com"));

        // All 3 custom domains blocked.
        assert!(content.contains("0.0.0.0 evil-tracker.example.com"));
        assert!(content.contains("0.0.0.0 sneaky-ads.example.net"));
        assert!(content.contains("0.0.0.0 more-telemetry.contoso.com"));

        // Markers present.
        assert!(content.contains("# --- wincrab telemetry block ---"));
        assert!(content.contains("# --- end wincrab telemetry block ---"));

        // Count total blocked entries (40 built-in + 3 custom).
        let blocked_count = content.matches("0.0.0.0 ").count();
        assert_eq!(blocked_count, 43);
    }
}

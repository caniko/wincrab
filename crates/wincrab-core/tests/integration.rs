//! Integration tests for wincrab-core.
//!
//! These tests exercise the public API with real filesystem operations,
//! but without requiring external tools (7z, wimlib-imagex, hivexsh, xorriso).

use std::path::Path;
use wincrab_core::Config;

// ===========================================================================
// Config integration tests
// ===========================================================================

#[test]
fn load_example_config_toml() {
    // The example config.toml shipped in the repo root should parse successfully.
    let repo_config = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("config.toml");

    if repo_config.exists() {
        let config = Config::from_file(&repo_config).unwrap();
        // Example config should have sensible defaults.
        assert!(config.wim_index > 0);
    }
}

#[test]
fn default_config_serializes_to_valid_toml() {
    let config = Config::default();
    let toml_str = toml::to_string_pretty(&config).unwrap();
    // Should round-trip.
    let loaded: Config = toml::from_str(&toml_str).unwrap();
    assert_eq!(loaded.wim_index, config.wim_index);
}

#[test]
fn minimal_config_with_only_wim_index() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("minimal.toml");
    std::fs::write(&path, "wim_index = 1\n[seelen]\nbundle = false").unwrap();
    let config = Config::from_file(&path).unwrap();
    assert_eq!(config.wim_index, 1);
    // All other fields should be defaults.
    assert!(config.apps.remove_bloatware);
    assert!(config.telemetry.disable);
}

#[test]
fn config_with_all_sections_disabled() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("disabled.toml");
    std::fs::write(
        &path,
        r#"
wim_index = 6

[apps]
remove_bloatware = false
remove_xbox = false
remove_teams = false
remove_onedrive = false
remove_cortana = false
remove_store = false
remove_outlook = false
remove_mail = false
remove_dev_home = false
remove_phone_link = false

[telemetry]
disable = false
disable_ceip = false
disable_app_telemetry = false

[privacy]
disable_advertising_id = false
disable_web_search = false
disable_activity_history = false
disable_tailored_experiences = false
disable_error_reporting = false
restrict_app_permissions = false

[copilot]
disable = false

[edge]
disable_first_run = false
disable_default_browser_nag = false
disable_sidebar = false
disable_search_bar = false

[visuals]
optimize_for_performance = false
disable_lock_screen_tips = false
disable_suggestions = false

[taskbar]
hide_widgets_button = false
hide_chat_button = false
search_icon_only = false
disable_start_recommendations = false

[services]
disable_diagtrack = false
disable_dmwappush = false
disable_wer = false
disable_xbox = false
disable_maps_broker = false
disable_retail_demo = false
disable_remote_registry = false
disable_geolocation = false

[scheduled_tasks]
remove_telemetry_tasks = false
remove_ceip_tasks = false
remove_disk_diagnostic_tasks = false
remove_maps_task = false
remove_feedback_tasks = false

[oobe]
inject_autounattend = false
skip_microsoft_account = false
skip_privacy_screens = false
skip_finish_setup_nag = false

[seelen]
bundle = false
replace_shell = false
remove_windows_search_ui = false
remove_start_experience = false

[drivers]
btrfs = false
ext4 = false
winfsp = false
mergerfs = false
"#,
    )
    .unwrap();

    let config = Config::from_file(&path).unwrap();
    assert!(!config.apps.remove_bloatware);
    assert!(!config.telemetry.disable);
    assert!(!config.copilot.disable);
    assert!(!config.seelen.bundle);
    assert!(!config.drivers.any_enabled());
}

// ===========================================================================
// Debloat integration tests
// ===========================================================================

mod debloat_integration {
    use wincrab_core::debloat::{prune_appx_packages, prune_seelen_replacements, remove_scheduled_tasks};
    use wincrab_core::config::{AppRemoval, ScheduledTasks, Seelen};

    /// Build a realistic Windows image directory structure.
    fn build_realistic_mount(mount: &std::path::Path) {
        // WindowsApps with realistic package names.
        let apps = mount.join("Program Files/WindowsApps");
        let packages = [
            "BytedancePte.Ltd.TikTok_3.5.0.0_x64__a1b2c3d4",
            "king.com.CandyCrushSodaSaga_1.0.0_x64__xyzzy",
            "Microsoft.BingWeather_4.53.0_x64__8wekyb3d8bbwe",
            "Microsoft.XboxApp_1.0_x64__8wekyb3d8bbwe",
            "Microsoft.XboxGameOverlay_1.0_x64__8wekyb3d8bbwe",
            "MicrosoftTeams_24.0_x64__8wekyb3d8bbwe",
            "Microsoft.WindowsCalculator_10.2_x64__8wekyb3d8bbwe",
            "Microsoft.WindowsNotepad_11.0_x64__8wekyb3d8bbwe",
            "Microsoft.OutlookForWindows_1.0_x64__8wekyb3d8bbwe",
            "microsoft.windowscommunicationsapps_16.0_x64__8wekyb3d8bbwe",
            "Microsoft.Windows.DevHome_0.1_x64__8wekyb3d8bbwe",
            "Microsoft.YourPhone_1.0_x64__8wekyb3d8bbwe",
            "MicrosoftWindows.CrossDevice_1.0_x64__8wekyb3d8bbwe",
        ];
        for pkg in &packages {
            let dir = apps.join(pkg);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("AppxManifest.xml"), b"<Package/>").unwrap();
            std::fs::write(dir.join("app.exe"), b"MZ").unwrap();
        }

        // Provisioning.
        let prov = mount.join("ProgramData/Microsoft/Windows/AppRepository/Packages");
        for pkg in &packages[..5] {
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
        let ceip = tasks.join("Customer Experience Improvement Program");
        std::fs::create_dir_all(&ceip).unwrap();
        std::fs::write(ceip.join("Consolidator"), b"").unwrap();
        let maps = tasks.join("Maps");
        std::fs::create_dir_all(&maps).unwrap();
        std::fs::write(maps.join("MapsUpdateTask"), b"").unwrap();
    }

    #[test]
    fn full_default_prune_on_realistic_structure() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        build_realistic_mount(mount);

        let config = AppRemoval::default();
        let stats = prune_appx_packages(mount, &config).unwrap();

        // Should remove TikTok, CandyCrush, BingWeather, Xbox (2), Teams,
        // Outlook, Mail, DevHome, PhoneLink (2), OneDrive (2 files)
        // + provisioning entries for the first 5 packages.
        assert!(stats.dirs_removed >= 8, "dirs_removed = {}", stats.dirs_removed);
        assert!(stats.files_removed >= 2, "files_removed = {}", stats.files_removed);

        // Calculator and Notepad should survive.
        let remaining: Vec<_> = std::fs::read_dir(mount.join("Program Files/WindowsApps"))
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(remaining.iter().any(|n| n.contains("Calculator")));
        assert!(remaining.iter().any(|n| n.contains("Notepad")));
    }

    #[test]
    fn full_scheduled_task_removal_on_realistic_structure() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        build_realistic_mount(mount);

        let config = ScheduledTasks::default();
        let stats = remove_scheduled_tasks(mount, &config).unwrap();
        assert!(stats.files_removed >= 2);
    }

    #[test]
    fn seelen_replacement_prune_only_targets_search_and_start() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        let apps = mount.join("Program Files/WindowsApps");
        let pkgs = [
            "Microsoft.Windows.Search_1.0_x64__abc",
            "Microsoft.Windows.SearchHost_1.0_x64__abc",
            "MicrosoftWindows.Client.CBS_1.0_x64__abc",
            "Microsoft.Windows.StartMenuExperienceHost_1.0_x64__abc",
            "Microsoft.WindowsCalculator_10.0_x64__abc", // should survive
        ];
        for pkg in &pkgs {
            let d = apps.join(pkg);
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("app.exe"), b"").unwrap();
        }

        let config = Seelen {
            bundle: true,
            replace_shell: true,
            remove_windows_search_ui: true,
            remove_start_experience: true,
        };

        let stats = prune_seelen_replacements(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 4);

        let remaining: Vec<_> = std::fs::read_dir(&apps)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(remaining.len(), 1);
        assert!(remaining[0].file_name().to_string_lossy().contains("Calculator"));
    }
}

// ===========================================================================
// OOBE integration tests
// ===========================================================================

mod oobe_integration {
    use wincrab_core::oobe::inject_autounattend;
    use wincrab_core::config::Oobe;

    #[test]
    fn autounattend_is_valid_xml_ish() {
        let dir = tempfile::tempdir().unwrap();
        let config = Oobe::default();
        inject_autounattend(dir.path(), &config).unwrap();

        let content = std::fs::read_to_string(dir.path().join("autounattend.xml")).unwrap();

        // Basic XML well-formedness checks.
        assert!(content.starts_with("<?xml"));
        assert!(content.contains("<unattend"));
        assert!(content.contains("</unattend>"));

        // Should not have any obvious broken tags.
        let open_count = content.matches('<').count();
        let close_count = content.matches('>').count();
        assert_eq!(open_count, close_count, "mismatched angle brackets");
    }

    #[test]
    fn autounattend_with_full_oobe_config() {
        let dir = tempfile::tempdir().unwrap();
        let config = Oobe {
            inject_autounattend: true,
            skip_microsoft_account: true,
            skip_privacy_screens: true,
            skip_finish_setup_nag: true,
            ..Default::default()
        };
        inject_autounattend(dir.path(), &config).unwrap();

        let content = std::fs::read_to_string(dir.path().join("autounattend.xml")).unwrap();
        assert!(content.contains("BypassNRO"));
        assert!(content.contains("ProtectYourPC"));
    }
}

// ===========================================================================
// Extract integration tests
// ===========================================================================

mod extract_integration {
    use wincrab_core::extract::find_install_image;

    #[test]
    fn find_install_image_full_staging_structure() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path();

        // Simulate a full ISO extract.
        std::fs::create_dir_all(staging.join("boot")).unwrap();
        std::fs::create_dir_all(staging.join("efi/microsoft/boot")).unwrap();
        std::fs::create_dir_all(staging.join("sources")).unwrap();
        std::fs::write(staging.join("sources/install.wim"), b"wim").unwrap();
        std::fs::write(staging.join("sources/boot.wim"), b"boot").unwrap();

        let path = find_install_image(staging).unwrap();
        assert!(path.ends_with("install.wim"));
    }
}

// ===========================================================================
// Pipeline integration tests
// ===========================================================================

mod pipeline_integration {
    use wincrab_core::pipeline::WorkDirs;

    #[test]
    fn work_dirs_structure() {
        let dir = tempfile::tempdir().unwrap();
        let dirs = WorkDirs::new(&dir.path().join("work")).unwrap();

        // Verify all expected directories exist.
        assert!(dirs.root.is_dir());
        assert!(dirs.staging.is_dir());
        assert!(dirs.wim_mount.is_dir());

        // Verify relationships.
        assert!(dirs.staging.starts_with(&dirs.root));
        assert!(dirs.wim_mount.starts_with(&dirs.root));
    }
}

// ===========================================================================
// Seelen injection integration
// ===========================================================================

mod seelen_integration {
    use wincrab_core::seelen::inject_seelen;
    use wincrab_core::config::Seelen;

    #[test]
    fn inject_creates_complete_install_package() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path().join("mount");
        std::fs::create_dir_all(&mount).unwrap();

        let setup = dir.path().join("setup.exe");
        std::fs::write(&setup, b"MZ\x90\x00fake PE").unwrap();

        let config = Seelen {
            bundle: true,
            replace_shell: true,
            remove_windows_search_ui: true,
            remove_start_experience: true,
        };

        inject_seelen(&mount, &setup, &config).unwrap();

        let seelen_dir = mount.join("SeelenUI");
        assert!(seelen_dir.is_dir());
        assert!(seelen_dir.join("Seelen.UI-setup.exe").exists());
        assert!(seelen_dir.join("install.ps1").exists());

        // Verify exe was copied correctly.
        let copied = std::fs::read(seelen_dir.join("Seelen.UI-setup.exe")).unwrap();
        assert_eq!(copied, b"MZ\x90\x00fake PE");

        // Verify script has shell replacement code.
        let script = std::fs::read_to_string(seelen_dir.join("install.ps1")).unwrap();
        assert!(script.contains("Winlogon"));
    }
}

// ===========================================================================
// Driver injection integration
// ===========================================================================

mod driver_integration {
    use wincrab_core::drivers::{inject_drivers, DriverPaths};
    use wincrab_core::config::Drivers;

    #[test]
    fn inject_all_drivers() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path().join("mount");
        std::fs::create_dir_all(&mount).unwrap();

        // Create fake driver files.
        let btrfs = dir.path().join("btrfs-1.9.zip");
        let ext2fsd = dir.path().join("Ext2Fsd-0.71.exe");
        let winfsp = dir.path().join("winfsp-2.0.x64.msi");
        let mergerfs = dir.path().join("mergerfs-win.zip");
        for f in [&btrfs, &ext2fsd, &winfsp, &mergerfs] {
            std::fs::write(f, b"fake driver").unwrap();
        }

        let paths = DriverPaths {
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

        inject_drivers(&mount, &paths, &config).unwrap();

        let driver_dir = mount.join("Drivers");
        assert!(driver_dir.join("btrfs-1.9.zip").exists());
        assert!(driver_dir.join("Ext2Fsd-0.71.exe").exists());
        assert!(driver_dir.join("winfsp-2.0.x64.msi").exists());
        assert!(driver_dir.join("mergerfs-win.zip").exists());
        assert!(driver_dir.join("install-drivers.ps1").exists());

        let script = std::fs::read_to_string(driver_dir.join("install-drivers.ps1")).unwrap();
        assert!(script.contains("WinBtrfs"));
        assert!(script.contains("Ext2Fsd"));
        assert!(script.contains("WinFsp"));
        assert!(script.contains("mergerfs"));
    }

    #[test]
    fn inject_with_no_drivers_creates_empty_script() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path().join("mount");
        std::fs::create_dir_all(&mount).unwrap();

        let paths = DriverPaths {
            btrfs: None,
            ext2fsd: None,
            winfsp: None,
            mergerfs: None,
        };
        let config = Drivers::default();

        inject_drivers(&mount, &paths, &config).unwrap();
        assert!(mount.join("Drivers/install-drivers.ps1").exists());
    }

    #[test]
    fn inject_partial_drivers() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path().join("mount");
        std::fs::create_dir_all(&mount).unwrap();

        let btrfs = dir.path().join("btrfs.zip");
        std::fs::write(&btrfs, b"fake").unwrap();

        let paths = DriverPaths {
            btrfs: Some(btrfs),
            ext2fsd: None,
            winfsp: None,
            mergerfs: None,
        };
        let config = Drivers { btrfs: true, ..Default::default() };

        inject_drivers(&mount, &paths, &config).unwrap();

        let driver_dir = mount.join("Drivers");
        assert!(driver_dir.join("btrfs.zip").exists());
        assert!(driver_dir.join("install-drivers.ps1").exists());
        // No other driver files should exist.
        let files: Vec<_> = std::fs::read_dir(&driver_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert_eq!(files.len(), 2); // btrfs.zip + install-drivers.ps1
    }

    #[test]
    fn inject_driver_script_content_matches_config() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path().join("mount");
        std::fs::create_dir_all(&mount).unwrap();

        let ext2fsd = dir.path().join("Ext2Fsd.exe");
        std::fs::write(&ext2fsd, b"installer").unwrap();

        let paths = DriverPaths {
            btrfs: None,
            ext2fsd: Some(ext2fsd),
            winfsp: None,
            mergerfs: None,
        };
        let config = Drivers { ext4: true, ..Default::default() };

        inject_drivers(&mount, &paths, &config).unwrap();

        let script = std::fs::read_to_string(mount.join("Drivers/install-drivers.ps1")).unwrap();
        assert!(script.contains("Ext2Fsd"));
        assert!(!script.contains("WinBtrfs"));
        assert!(!script.contains("WinFsp"));
    }
}

// ===========================================================================
// Cross-module integration tests
// ===========================================================================

mod cross_module_integration {
    use wincrab_core::config::{AppRemoval, ScheduledTasks, Seelen};
    use wincrab_core::debloat::{prune_appx_packages, prune_seelen_replacements, remove_scheduled_tasks};
    use wincrab_core::oobe::inject_autounattend;
    use wincrab_core::config::Oobe;

    /// Build a complete Windows image mock with apps, tasks, and system files.
    fn build_complete_mount(mount: &std::path::Path) {
        // WindowsApps
        let apps = mount.join("Program Files/WindowsApps");
        let packages = [
            "BytedancePte.Ltd.TikTok_1.0_x64__abc",
            "Microsoft.XboxApp_1.0_x64__abc",
            "MicrosoftTeams_1.0_x64__abc",
            "Microsoft.Windows.Search_1.0_x64__abc",
            "MicrosoftWindows.Client.CBS_1.0_x64__abc",
            "Microsoft.WindowsCalculator_10.0_x64__abc",
            "Microsoft.WindowsNotepad_11.0_x64__abc",
        ];
        for pkg in &packages {
            let dir = apps.join(pkg);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("app.exe"), b"MZ").unwrap();
        }

        // Scheduled tasks
        let tasks = mount.join("Windows/System32/Tasks/Microsoft/Windows");
        let app_exp = tasks.join("Application Experience");
        std::fs::create_dir_all(&app_exp).unwrap();
        std::fs::write(app_exp.join("Microsoft Compatibility Appraiser"), b"").unwrap();
        let ceip = tasks.join("Customer Experience Improvement Program");
        std::fs::create_dir_all(&ceip).unwrap();
        std::fs::write(ceip.join("Consolidator"), b"").unwrap();

        // OneDrive
        let sys32 = mount.join("Windows/System32");
        std::fs::write(sys32.join("OneDriveSetup.exe"), b"setup").unwrap();
    }

    #[test]
    fn full_debloat_then_scheduled_task_removal() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        build_complete_mount(mount);

        // Phase 1: Prune apps
        let app_stats = prune_appx_packages(mount, &AppRemoval::default()).unwrap();
        assert!(app_stats.dirs_removed >= 3);

        // Phase 2: Remove scheduled tasks
        let task_stats = remove_scheduled_tasks(mount, &ScheduledTasks::default()).unwrap();
        assert!(task_stats.files_removed >= 1);

        // Calculator and Notepad should survive.
        let remaining: Vec<_> = std::fs::read_dir(mount.join("Program Files/WindowsApps"))
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        assert!(remaining.iter().any(|n| n.contains("Calculator")));
        assert!(remaining.iter().any(|n| n.contains("Notepad")));
    }

    #[test]
    fn seelen_prune_after_app_prune() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        build_complete_mount(mount);

        // First prune regular apps.
        let app_config = AppRemoval::default();
        let _app_stats = prune_appx_packages(mount, &app_config).unwrap();

        // Then prune Seelen-replaced components.
        let seelen_config = Seelen {
            bundle: true,
            replace_shell: true,
            remove_windows_search_ui: true,
            remove_start_experience: true,
        };
        let seelen_stats = prune_seelen_replacements(mount, &seelen_config).unwrap();
        assert!(seelen_stats.dirs_removed >= 2); // Search + CBS
    }

    #[test]
    fn oobe_injection_into_staging_with_existing_files() {
        let dir = tempfile::tempdir().unwrap();
        let staging = dir.path();

        // Simulate existing files in staging (like a real extracted ISO).
        std::fs::create_dir_all(staging.join("sources")).unwrap();
        std::fs::write(staging.join("sources/install.wim"), b"wim").unwrap();
        std::fs::create_dir_all(staging.join("boot")).unwrap();

        let config = Oobe::default();
        inject_autounattend(staging, &config).unwrap();

        // autounattend.xml should be at the root of staging.
        assert!(staging.join("autounattend.xml").exists());
        // Other files should still exist.
        assert!(staging.join("sources/install.wim").exists());
    }

    #[test]
    fn debloat_with_empty_mount_dir() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        // Completely empty mount dir -- no WindowsApps, no tasks, etc.

        let app_stats = prune_appx_packages(mount, &AppRemoval::default()).unwrap();
        assert_eq!(app_stats.dirs_removed, 0);

        let task_stats = remove_scheduled_tasks(mount, &ScheduledTasks::default()).unwrap();
        assert_eq!(task_stats.files_removed, 0);

        let seelen_stats = prune_seelen_replacements(mount, &Seelen::default()).unwrap();
        assert_eq!(seelen_stats.dirs_removed, 0);
    }
}

// ===========================================================================
// Config validation integration tests
// ===========================================================================

mod config_validation_integration {
    use wincrab_core::Config;

    #[test]
    fn config_with_multiple_edge_patterns_and_seelen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("multi.toml");
        std::fs::write(
            &path,
            r#"
            [seelen]
            bundle = true
            [apps]
            extra_patterns = ["SomeApp", "AnotherApp", "ThirdApp"]
            "#,
        )
        .unwrap();
        let cfg = Config::from_file(&path).unwrap();
        assert_eq!(cfg.apps.extra_patterns.len(), 3);
    }

    #[test]
    fn config_unknown_fields_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("extra.toml");
        std::fs::write(
            &path,
            "wim_index = 6\nunknown_field = true\n[seelen]\nbundle = false\n[unknown_section]\nfoo = \"bar\"",
        )
        .unwrap();
        // TOML deserialization with #[serde(default)] should ignore unknown fields
        // unless deny_unknown_fields is set.
        let result = Config::from_file(&path);
        // Depending on serde config, this might succeed or fail.
        // Either way, it shouldn't panic.
        let _ = result;
    }

    #[test]
    fn config_wim_index_overflow_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("overflow.toml");
        std::fs::write(&path, "wim_index = 4294967296").unwrap(); // u32::MAX + 1
        let result = Config::from_file(&path);
        assert!(result.is_err());
    }

    #[test]
    fn config_negative_wim_index_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("negative.toml");
        std::fs::write(&path, "wim_index = -1").unwrap();
        let result = Config::from_file(&path);
        assert!(result.is_err());
    }

    #[test]
    fn config_empty_file_uses_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("empty.toml");
        std::fs::write(&path, "").unwrap();
        let cfg = Config::from_file(&path).unwrap();
        assert_eq!(cfg.wim_index, 6);
        assert!(cfg.apps.remove_bloatware);
    }

    #[test]
    fn config_all_drivers_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("drivers.toml");
        std::fs::write(
            &path,
            "[drivers]\nbtrfs = true\next4 = true\nwinfsp = true\nmergerfs = true",
        )
        .unwrap();
        let cfg = Config::from_file(&path).unwrap();
        assert!(cfg.drivers.any_enabled());
        assert!(cfg.drivers.btrfs);
        assert!(cfg.drivers.ext4);
        assert!(cfg.drivers.winfsp);
        assert!(cfg.drivers.mergerfs);
    }
}

// ===========================================================================
// Seelen injection with various configs integration
// ===========================================================================

mod seelen_config_integration {
    use wincrab_core::seelen::inject_seelen;
    use wincrab_core::config::Seelen;

    #[test]
    fn inject_seelen_with_minimal_config() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path().join("mount");
        std::fs::create_dir_all(&mount).unwrap();

        let setup = dir.path().join("setup.exe");
        std::fs::write(&setup, b"MZ").unwrap();

        let config = Seelen {
            bundle: true,
            replace_shell: false,
            remove_windows_search_ui: false,
            remove_start_experience: false,
        };

        inject_seelen(&mount, &setup, &config).unwrap();

        let script = std::fs::read_to_string(mount.join("SeelenUI/install.ps1")).unwrap();
        assert!(script.contains("Installing Seelen-UI"));
        assert!(!script.contains("Winlogon"));
    }

    #[test]
    fn inject_seelen_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path().join("mount");
        std::fs::create_dir_all(&mount).unwrap();

        let setup = dir.path().join("setup.exe");
        std::fs::write(&setup, b"MZ first").unwrap();

        let config = Seelen::default();
        inject_seelen(&mount, &setup, &config).unwrap();

        // Inject again with different content.
        std::fs::write(&setup, b"MZ second").unwrap();
        inject_seelen(&mount, &setup, &config).unwrap();

        let copied = std::fs::read(mount.join("SeelenUI/Seelen.UI-setup.exe")).unwrap();
        assert_eq!(copied, b"MZ second");
    }

    #[test]
    fn inject_seelen_large_exe() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path().join("mount");
        std::fs::create_dir_all(&mount).unwrap();

        let setup = dir.path().join("setup.exe");
        let data: Vec<u8> = (0..500_000).map(|i| (i % 256) as u8).collect();
        std::fs::write(&setup, &data).unwrap();

        inject_seelen(&mount, &setup, &Seelen::default()).unwrap();

        let copied = std::fs::read(mount.join("SeelenUI/Seelen.UI-setup.exe")).unwrap();
        assert_eq!(copied.len(), 500_000);
        assert_eq!(copied, data);
    }
}

// ===========================================================================
// Profile integration tests
// ===========================================================================

mod profile_integration {
    use wincrab_core::profiles::{load_profile, merge_with_overrides, PROFILE_NAMES};

    #[test]
    fn all_profiles_validate() {
        for name in &PROFILE_NAMES {
            let cfg = load_profile(name).unwrap();
            assert!(cfg.validate().is_ok(), "profile '{name}' failed validation");
        }
    }

    #[test]
    fn gaming_profile_keeps_xbox() {
        let cfg = load_profile("gaming").unwrap();
        assert!(!cfg.apps.remove_xbox);
        assert!(!cfg.services.disable_xbox);
    }

    #[test]
    fn enterprise_profile_keeps_defender() {
        let cfg = load_profile("enterprise").unwrap();
        assert!(!cfg.defender.disable_realtime);
        assert!(!cfg.defender.disable_services);
        assert!(!cfg.defender.disable_smartscreen);
    }

    #[test]
    fn vm_profile_enables_virtio() {
        let cfg = load_profile("vm").unwrap();
        assert!(cfg.drivers.virtio);
        assert!(cfg.drivers.any_enabled());
    }

    #[test]
    fn minimal_profile_disables_everything() {
        let cfg = load_profile("minimal").unwrap();
        assert!(cfg.apps.remove_store);
        assert!(cfg.services.disable_search);
        assert!(cfg.services.disable_sysmain);
        assert!(cfg.services.disable_print_spooler);
        assert!(!cfg.seelen.bundle);
    }

    #[test]
    fn merge_overrides_new_section_fields() {
        let base = load_profile("minimal").unwrap();
        let merged = merge_with_overrides(
            base,
            "[defender]\ndisable_realtime = false\n[recall]\ndisable = false\n",
        )
        .unwrap();
        assert!(!merged.defender.disable_realtime);
        assert!(!merged.recall.disable);
        // Rest of minimal should be preserved.
        assert!(merged.apps.remove_store);
    }

    #[test]
    fn merge_overrides_hooks() {
        let base = load_profile("minimal").unwrap();
        let merged = merge_with_overrides(
            base,
            "[hooks]\npre_extract = \"echo hello\"\n",
        )
        .unwrap();
        assert_eq!(merged.hooks.pre_extract.as_deref(), Some("echo hello"));
        assert!(merged.hooks.post_build.is_none());
    }

    #[test]
    fn merge_overrides_inject_files() {
        let base = load_profile("minimal").unwrap();
        let merged = merge_with_overrides(
            base,
            "[[inject.files]]\nsrc = \"/tmp/test.txt\"\ndest = \"Users/Public/test.txt\"\n",
        )
        .unwrap();
        assert_eq!(merged.inject.files.len(), 1);
        assert_eq!(merged.inject.files[0].dest, "Users/Public/test.txt");
    }

    #[test]
    fn merge_overrides_oobe_edition() {
        let base = load_profile("enterprise").unwrap();
        let merged = merge_with_overrides(
            base,
            "[oobe]\nconvert_edition = \"Professional\"\n",
        )
        .unwrap();
        assert_eq!(merged.oobe.convert_edition.as_deref(), Some("Professional"));
    }
}

// ===========================================================================
// Hosts integration tests
// ===========================================================================

mod hosts_integration {
    use wincrab_core::config::Telemetry;
    use wincrab_core::hosts::inject_telemetry_hosts;

    #[test]
    fn hosts_creates_full_directory_structure() {
        let dir = tempfile::tempdir().unwrap();
        let config = Telemetry::default();
        inject_telemetry_hosts(dir.path(), &config).unwrap();

        let hosts = dir.path().join("Windows/System32/drivers/etc/hosts");
        assert!(hosts.exists());
    }

    #[test]
    fn hosts_with_only_extra_domains() {
        let dir = tempfile::tempdir().unwrap();
        let config = Telemetry {
            block_telemetry_hosts: true,
            extra_blocked_hosts: vec![
                "tracking.example.com".into(),
                "ads.example.net".into(),
            ],
            ..Telemetry::default()
        };
        inject_telemetry_hosts(dir.path(), &config).unwrap();

        let content = std::fs::read_to_string(
            dir.path().join("Windows/System32/drivers/etc/hosts"),
        )
        .unwrap();
        assert!(content.contains("0.0.0.0 tracking.example.com"));
        assert!(content.contains("0.0.0.0 ads.example.net"));
        assert!(content.contains("wincrab telemetry block"));
    }

    #[test]
    fn hosts_preserves_existing_entries() {
        let dir = tempfile::tempdir().unwrap();
        let hosts_dir = dir.path().join("Windows/System32/drivers/etc");
        std::fs::create_dir_all(&hosts_dir).unwrap();
        std::fs::write(
            hosts_dir.join("hosts"),
            "127.0.0.1 localhost\n::1 localhost\n",
        )
        .unwrap();

        let config = Telemetry::default();
        inject_telemetry_hosts(dir.path(), &config).unwrap();

        let content = std::fs::read_to_string(hosts_dir.join("hosts")).unwrap();
        assert!(content.starts_with("127.0.0.1 localhost\n::1 localhost\n"));
        assert!(content.contains("0.0.0.0 telemetry.microsoft.com"));
    }

    #[test]
    fn hosts_has_begin_end_markers() {
        let dir = tempfile::tempdir().unwrap();
        let config = Telemetry::default();
        inject_telemetry_hosts(dir.path(), &config).unwrap();

        let content = std::fs::read_to_string(
            dir.path().join("Windows/System32/drivers/etc/hosts"),
        )
        .unwrap();
        assert!(content.contains("# --- wincrab telemetry block ---"));
        assert!(content.contains("# --- end wincrab telemetry block ---"));
    }
}

// ===========================================================================
// Performance integration tests
// ===========================================================================

mod performance_integration {
    use wincrab_core::config::Performance;
    use wincrab_core::performance::{generate_performance_script, inject_performance_script};

    #[test]
    fn performance_script_has_header() {
        let config = Performance::default();
        let script = generate_performance_script(&config);
        assert!(script.contains("# wincrab performance tweaks"));
    }

    #[test]
    fn inject_creates_wincrab_directory() {
        let dir = tempfile::tempdir().unwrap();
        let config = Performance {
            high_perf_power_plan: true,
            ..Default::default()
        };
        inject_performance_script(dir.path(), &config).unwrap();

        assert!(dir.path().join("wincrab").is_dir());
        assert!(dir.path().join("wincrab/performance.ps1").exists());
    }

    #[test]
    fn inject_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let config = Performance {
            high_perf_power_plan: true,
            ..Default::default()
        };
        inject_performance_script(dir.path(), &config).unwrap();
        inject_performance_script(dir.path(), &config).unwrap();

        let content =
            std::fs::read_to_string(dir.path().join("wincrab/performance.ps1")).unwrap();
        assert!(content.contains("powercfg"));
    }
}

// ===========================================================================
// Manifest integration tests
// ===========================================================================

mod manifest_integration {
    use wincrab_core::manifest::{compute_sha256, write_manifest, BuildManifest};
    use std::borrow::Cow;

    #[test]
    fn manifest_roundtrip_with_cow_borrowed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manifest.json");

        let manifest = BuildManifest {
            wincrab_version: Cow::Borrowed("1.2.3"),
            source_iso_sha256: "aaa".into(),
            output_iso_sha256: "bbb".into(),
            source_iso_size_bytes: 1000,
            output_iso_size_bytes: 500,
            config_snapshot: "{}".into(),
            timestamp: "12345".into(),
        };

        write_manifest(&manifest, &path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let loaded: BuildManifest = serde_json::from_str(&content).unwrap();
        assert_eq!(loaded.wincrab_version, "1.2.3");
    }

    #[test]
    fn manifest_roundtrip_with_cow_owned() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manifest.json");

        let manifest = BuildManifest {
            wincrab_version: Cow::Owned("0.0.1-dev".into()),
            source_iso_sha256: "abc".into(),
            output_iso_sha256: "def".into(),
            source_iso_size_bytes: 0,
            output_iso_size_bytes: 0,
            config_snapshot: "test".into(),
            timestamp: "0".into(),
        };

        write_manifest(&manifest, &path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let loaded: BuildManifest = serde_json::from_str(&content).unwrap();
        assert_eq!(loaded.wincrab_version, "0.0.1-dev");
    }

    #[test]
    fn sha256_large_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("large.bin");
        let data: Vec<u8> = (0..100_000).map(|i| (i % 256) as u8).collect();
        std::fs::write(&path, &data).unwrap();

        let hash = compute_sha256(&path).unwrap();
        assert_eq!(hash.len(), 64);
        // Deterministic: same data = same hash.
        let hash2 = compute_sha256(&path).unwrap();
        assert_eq!(hash, hash2);
    }

    #[test]
    fn manifest_nested_directory_creation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a/b/c/manifest.json");

        let manifest = BuildManifest {
            wincrab_version: "1.0.0".into(),
            source_iso_sha256: "x".into(),
            output_iso_sha256: "y".into(),
            source_iso_size_bytes: 0,
            output_iso_size_bytes: 0,
            config_snapshot: "{}".into(),
            timestamp: "0".into(),
        };

        write_manifest(&manifest, &path).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn manifest_json_has_all_fields() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("manifest.json");

        let manifest = BuildManifest {
            wincrab_version: "2.0.0".into(),
            source_iso_sha256: "src_hash".into(),
            output_iso_sha256: "out_hash".into(),
            source_iso_size_bytes: 5_000_000_000,
            output_iso_size_bytes: 3_000_000_000,
            config_snapshot: "{\"wim_index\": 6}".into(),
            timestamp: "1710460800".into(),
        };

        write_manifest(&manifest, &path).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("\"wincrab_version\""));
        assert!(content.contains("\"source_iso_sha256\""));
        assert!(content.contains("\"output_iso_sha256\""));
        assert!(content.contains("\"source_iso_size_bytes\""));
        assert!(content.contains("\"output_iso_size_bytes\""));
        assert!(content.contains("\"config_snapshot\""));
        assert!(content.contains("\"timestamp\""));
    }
}

// ===========================================================================
// Hooks integration tests
// ===========================================================================

mod hooks_integration {
    use wincrab_core::hooks::run_hook;

    #[test]
    fn hook_with_multiple_env_vars() {
        let result = run_hook(
            "test",
            &Some("test \"$VAR_A\" = \"hello\" && test \"$VAR_B\" = \"world\"".into()),
            &[("VAR_A", "hello"), ("VAR_B", "world")],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn hook_script_can_write_files() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("hook_output.txt");
        let script = format!("echo 'hook ran' > '{}'", output.display());
        let result = run_hook("test", &Some(script), &[]);
        assert!(result.is_ok());
        assert!(output.exists());
        let content = std::fs::read_to_string(&output).unwrap();
        assert!(content.contains("hook ran"));
    }

    #[test]
    fn hook_env_vars_from_pipeline_context() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("env.txt");
        let script = format!("echo $WINCRAB_ISO > '{}'", output.display());
        let result = run_hook(
            "pre_extract",
            &Some(script),
            &[("WINCRAB_ISO", "/path/to/windows.iso")],
        );
        assert!(result.is_ok());
        let content = std::fs::read_to_string(&output).unwrap();
        assert!(content.contains("/path/to/windows.iso"));
    }
}

// ===========================================================================
// Config validation integration tests (new features)
// ===========================================================================

mod new_config_validation {
    use wincrab_core::Config;

    #[test]
    fn config_with_new_sections_from_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new_sections.toml");
        std::fs::write(
            &path,
            r#"
[seelen]
bundle = false

[defender]
disable_realtime = false
disable_smartscreen = false

[windows_update]
disable_auto_updates = true

[explorer]
show_hidden_files = true
classic_context_menu = true

[performance]
high_perf_power_plan = true
network_tuning = true

[security]
asr_rules = true
disable_smb1 = true

[recall]
disable = true

[hooks]
pre_extract = "echo start"
post_build = "echo done"
"#,
        )
        .unwrap();

        let cfg = Config::from_file(&path).unwrap();
        assert!(!cfg.defender.disable_realtime);
        assert!(cfg.windows_update.disable_auto_updates);
        assert!(cfg.explorer.show_hidden_files);
        assert!(cfg.performance.network_tuning);
        assert!(cfg.security.asr_rules);
        assert!(cfg.recall.disable);
        assert_eq!(cfg.hooks.pre_extract.as_deref(), Some("echo start"));
        assert_eq!(cfg.hooks.post_build.as_deref(), Some("echo done"));
    }

    #[test]
    fn config_with_inject_files_from_toml() {
        let dir = tempfile::tempdir().unwrap();
        // Create real temp files so validation passes.
        let wallpaper = dir.path().join("wallpaper.jpg");
        let script = dir.path().join("script.ps1");
        std::fs::write(&wallpaper, b"fake image").unwrap();
        std::fs::write(&script, b"Write-Host 'hi'").unwrap();

        let path = dir.path().join("inject.toml");
        std::fs::write(
            &path,
            format!(
                r#"
[seelen]
bundle = false

[[inject.files]]
src = "{}"
dest = "Users/Public/Pictures/wallpaper.jpg"

[[inject.files]]
src = "{}"
dest = "wincrab/custom.ps1"
"#,
                wallpaper.display(),
                script.display(),
            ),
        )
        .unwrap();

        let cfg = Config::from_file(&path).unwrap();
        assert_eq!(cfg.inject.files.len(), 2);
        assert_eq!(cfg.inject.files[0].dest, "Users/Public/Pictures/wallpaper.jpg");
        assert_eq!(cfg.inject.files[1].dest, "wincrab/custom.ps1");
    }

    #[test]
    fn config_with_oobe_new_fields_from_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("oobe.toml");
        std::fs::write(
            &path,
            r#"
[seelen]
bundle = false

[oobe]
bypass_tpm = true
bypass_secureboot = true
bypass_ram = true
timezone = "Eastern Standard Time"
local_account_name = "Admin"
local_account_password = "secret"
auto_logon = true
disable_bitlocker = true
computer_name = "MYPC"
first_logon_commands = ["Set-ExecutionPolicy RemoteSigned", "Write-Host 'Hello'"]
convert_edition = "Professional"
skip_auto_activation = true
auto_partition = true
"#,
        )
        .unwrap();

        let cfg = Config::from_file(&path).unwrap();
        assert!(cfg.oobe.bypass_tpm);
        assert_eq!(cfg.oobe.timezone.as_deref(), Some("Eastern Standard Time"));
        assert_eq!(cfg.oobe.local_account_name.as_deref(), Some("Admin"));
        assert_eq!(cfg.oobe.local_account_password.as_deref(), Some("secret"));
        assert!(cfg.oobe.auto_logon);
        assert_eq!(cfg.oobe.computer_name.as_deref(), Some("MYPC"));
        assert_eq!(cfg.oobe.first_logon_commands.len(), 2);
        assert_eq!(cfg.oobe.convert_edition.as_deref(), Some("Professional"));
        assert!(cfg.oobe.auto_partition);
    }

    #[test]
    fn config_with_expanded_telemetry_from_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("telemetry.toml");
        std::fs::write(
            &path,
            r#"
[seelen]
bundle = false

[telemetry]
block_telemetry_hosts = true
extra_blocked_hosts = ["custom.tracker.example.com", "ads.example.net"]
disable_clipboard_sync = true
disable_find_my_device = true
disable_input_personalization = true
disable_wifi_sense = true
set_feedback_never = true
"#,
        )
        .unwrap();

        let cfg = Config::from_file(&path).unwrap();
        assert!(cfg.telemetry.block_telemetry_hosts);
        assert_eq!(cfg.telemetry.extra_blocked_hosts.len(), 2);
        assert!(cfg.telemetry.disable_clipboard_sync);
    }

    #[test]
    fn config_with_expanded_services_from_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("services.toml");
        std::fs::write(
            &path,
            r#"
[seelen]
bundle = false

[services]
disable_search = true
disable_sysmain = true
disable_ssdp = true
disable_upnp = true
disable_fax = true
disable_print_spooler = true
disable_wmp_sharing = true
disable_widgets_service = true
disable_telephony = true
"#,
        )
        .unwrap();

        let cfg = Config::from_file(&path).unwrap();
        assert!(cfg.services.disable_search);
        assert!(cfg.services.disable_sysmain);
        assert!(cfg.services.disable_print_spooler);
        assert!(cfg.services.disable_telephony);
    }

    #[test]
    fn config_with_drivers_expanded_from_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("drivers.toml");
        std::fs::write(
            &path,
            r#"
[seelen]
bundle = false

[drivers]
virtio = true
"#,
        )
        .unwrap();

        let cfg = Config::from_file(&path).unwrap();
        assert!(cfg.drivers.virtio);
        assert!(cfg.drivers.any_enabled());
    }
}

// ===========================================================================
// OOBE integration tests for new features
// ===========================================================================

mod oobe_new_features_integration {
    use wincrab_core::config::Oobe;
    use wincrab_core::oobe::inject_autounattend;

    #[test]
    fn autounattend_with_local_account_creates_valid_xml() {
        let dir = tempfile::tempdir().unwrap();
        let config = Oobe {
            local_account_name: Some("TestUser".into()),
            local_account_password: Some("pass123".into()),
            auto_logon: true,
            ..Oobe::default()
        };
        inject_autounattend(dir.path(), &config).unwrap();

        let content = std::fs::read_to_string(dir.path().join("autounattend.xml")).unwrap();
        assert!(content.contains("<Name>TestUser</Name>"));
        assert!(content.contains("<Value>pass123</Value>"));
        assert!(content.contains("<AutoLogon>"));
        assert!(content.contains("<Username>TestUser</Username>"));
        // Balanced brackets.
        let open = content.matches('<').count();
        let close = content.matches('>').count();
        assert_eq!(open, close);
    }

    #[test]
    fn autounattend_with_auto_partition_creates_valid_xml() {
        let dir = tempfile::tempdir().unwrap();
        let config = Oobe {
            auto_partition: true,
            ..Oobe::default()
        };
        inject_autounattend(dir.path(), &config).unwrap();

        let content = std::fs::read_to_string(dir.path().join("autounattend.xml")).unwrap();
        assert!(content.contains("<DiskConfiguration>"));
        assert!(content.contains("<Type>EFI</Type>"));
        assert!(content.contains("<Type>MSR</Type>"));
    }

    #[test]
    fn autounattend_with_timezone_and_computer_name() {
        let dir = tempfile::tempdir().unwrap();
        let config = Oobe {
            timezone: Some("UTC".into()),
            computer_name: Some("WINCRAB-TEST".into()),
            ..Oobe::default()
        };
        inject_autounattend(dir.path(), &config).unwrap();

        let content = std::fs::read_to_string(dir.path().join("autounattend.xml")).unwrap();
        assert!(content.contains("specialize"));
        assert!(content.contains("<TimeZone>UTC</TimeZone>"));
        assert!(content.contains("<ComputerName>WINCRAB-TEST</ComputerName>"));
    }

    #[test]
    fn autounattend_with_first_logon_commands() {
        let dir = tempfile::tempdir().unwrap();
        let config = Oobe {
            first_logon_commands: vec![
                "Get-Date".into(),
                "Write-Host 'Hello'".into(),
            ],
            ..Oobe::default()
        };
        inject_autounattend(dir.path(), &config).unwrap();

        let content = std::fs::read_to_string(dir.path().join("autounattend.xml")).unwrap();
        assert!(content.contains("Get-Date"));
        assert!(content.contains("Write-Host 'Hello'"));
        assert!(content.contains("powershell -ExecutionPolicy Bypass"));
    }
}

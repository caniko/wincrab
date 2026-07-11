//! Smoke tests for wincrab-core.
//!
//! These tests verify the basic public API surface compiles and works
//! at a high level. They catch regressions in the module structure,
//! re-exports, and fundamental type contracts.

use wincrab_core::{Config, Error};

// ===========================================================================
// Public API surface
// ===========================================================================

#[test]
fn config_is_default() {
    let _config = Config::default();
}

#[test]
fn config_is_debug() {
    let config = Config::default();
    let debug = format!("{config:?}");
    assert!(debug.contains("Config"));
}

#[test]
fn config_is_clone() {
    let config = Config::default();
    let cloned = config.clone();
    assert_eq!(cloned.wim_index, config.wim_index);
}

#[test]
fn error_is_debug() {
    let err = Error::Config { message: "test".into() };
    let _ = format!("{err:?}");
}

#[test]
fn error_implements_display() {
    let err = Error::Config { message: "smoke test".into() };
    let msg = format!("{err}");
    assert!(msg.contains("smoke test"));
}

#[test]
fn error_implements_std_error() {
    let err = Error::Config { message: "test".into() };
    let _: &dyn std::error::Error = &err;
}

// ===========================================================================
// Config serialization smoke
// ===========================================================================

#[test]
fn default_config_serializes() {
    let config = Config::default();
    let toml_str = toml::to_string(&config).unwrap();
    assert!(!toml_str.is_empty());
}

#[test]
fn default_config_roundtrips() {
    let original = Config::default();
    let serialized = toml::to_string_pretty(&original).unwrap();
    let deserialized: Config = toml::from_str(&serialized).unwrap();
    assert_eq!(original.wim_index, deserialized.wim_index);
    assert_eq!(original.apps.remove_bloatware, deserialized.apps.remove_bloatware);
    assert_eq!(original.apps.remove_store, deserialized.apps.remove_store);
    assert_eq!(original.telemetry.disable, deserialized.telemetry.disable);
    assert_eq!(original.copilot.disable, deserialized.copilot.disable);
    assert_eq!(original.seelen.bundle, deserialized.seelen.bundle);
    assert_eq!(original.drivers.btrfs, deserialized.drivers.btrfs);
}

// ===========================================================================
// Module accessibility smoke
// ===========================================================================

#[test]
fn debloat_module_accessible() {
    let _stats = wincrab_core::debloat::PruneStats::default();
    assert_eq!(_stats.dirs_removed, 0);
    assert_eq!(_stats.files_removed, 0);
}

#[test]
fn pipeline_work_dirs_accessible() {
    let dir = tempfile::tempdir().unwrap();
    let dirs = wincrab_core::pipeline::WorkDirs::new(&dir.path().join("smoke")).unwrap();
    assert!(dirs.root.exists());
}

// ===========================================================================
// Error variant construction smoke
// ===========================================================================

#[test]
fn all_error_variants_constructible() {
    let errors: Vec<Error> = vec![
        Error::Io {
            context: "test".into(),
            source: std::io::Error::other("e"),
        },
        Error::Config { message: "bad".into() },
        Error::Command {
            command: "cmd".into(),
            code: 1,
            stderr: "err".into(),
        },
        Error::CommandSignaled { command: "cmd".into() },
        Error::ToolNotFound { tool: "tool".into() },
        Error::WimNotFound { path: "/wim".into() },
        Error::HiveNotFound { path: "/hive".into() },
    ];

    for err in &errors {
        let msg = format!("{err}");
        assert!(!msg.is_empty());
    }
}

// ===========================================================================
// Config edge cases
// ===========================================================================

#[test]
fn config_with_empty_extra_patterns_roundtrips() {
    let mut config = Config::default();
    config.apps.extra_patterns = vec![];
    let serialized = toml::to_string(&config).unwrap();
    let deserialized: Config = toml::from_str(&serialized).unwrap();
    assert!(deserialized.apps.extra_patterns.is_empty());
}

#[test]
fn config_with_unicode_extra_pattern() {
    let mut config = Config::default();
    config.seelen.bundle = false;
    config.apps.extra_patterns = vec!["日本語アプリ".into(), "Ёмкость".into()];
    let serialized = toml::to_string_pretty(&config).unwrap();
    let deserialized: Config = toml::from_str(&serialized).unwrap();
    assert_eq!(deserialized.apps.extra_patterns.len(), 2);
    assert_eq!(deserialized.apps.extra_patterns[0], "日本語アプリ");
}

#[test]
fn config_with_special_chars_in_pattern() {
    let mut config = Config::default();
    config.seelen.bundle = false;
    config.apps.extra_patterns = vec![
        "App.With.Dots".into(),
        "App_With_Underscores".into(),
        "App-With-Dashes".into(),
        "App With Spaces".into(),
    ];
    let serialized = toml::to_string_pretty(&config).unwrap();
    let deserialized: Config = toml::from_str(&serialized).unwrap();
    assert_eq!(deserialized.apps.extra_patterns.len(), 4);
}

// ===========================================================================
// Error trait contracts
// ===========================================================================

#[test]
fn io_error_has_source() {
    let err = Error::Io {
        context: "test".into(),
        source: std::io::Error::new(std::io::ErrorKind::NotFound, "gone"),
    };
    assert!(std::error::Error::source(&err).is_some());
}

#[test]
fn command_error_has_no_source() {
    let err = Error::Command {
        command: "cmd".into(),
        code: 1,
        stderr: "err".into(),
    };
    assert!(std::error::Error::source(&err).is_none());
}

#[test]
fn tool_not_found_error_has_no_source() {
    let err = Error::ToolNotFound { tool: "x".into() };
    assert!(std::error::Error::source(&err).is_none());
}

#[test]
fn wim_not_found_error_display() {
    let err = Error::WimNotFound {
        path: std::path::PathBuf::from("/path/to/install.wim"),
    };
    let msg = format!("{err}");
    assert!(msg.contains("install.wim"));
}

#[test]
fn hive_not_found_error_display() {
    let err = Error::HiveNotFound {
        path: std::path::PathBuf::from("/path/to/SOFTWARE"),
    };
    let msg = format!("{err}");
    assert!(msg.contains("SOFTWARE"));
}

#[test]
fn command_signaled_display() {
    let err = Error::CommandSignaled {
        command: "test-cmd".into(),
    };
    let msg = format!("{err}");
    assert!(msg.contains("signal"));
    assert!(msg.contains("test-cmd"));
}

// ===========================================================================
// Config section defaults (comprehensive)
// ===========================================================================

#[test]
fn app_removal_defaults() {
    let cfg = wincrab_core::config::AppRemoval::default();
    assert!(cfg.remove_bloatware);
    assert!(cfg.remove_xbox);
    assert!(cfg.remove_teams);
    assert!(cfg.remove_onedrive);
    assert!(cfg.remove_cortana);
    assert!(!cfg.remove_store);
    assert!(cfg.remove_outlook);
    assert!(cfg.remove_mail);
    assert!(cfg.remove_dev_home);
    assert!(cfg.remove_phone_link);
    assert!(cfg.extra_patterns.is_empty());
}

#[test]
fn telemetry_defaults() {
    let cfg = wincrab_core::config::Telemetry::default();
    assert!(cfg.disable);
    assert!(cfg.disable_ceip);
    assert!(cfg.disable_app_telemetry);
}

#[test]
fn privacy_defaults() {
    let cfg = wincrab_core::config::Privacy::default();
    assert!(cfg.disable_advertising_id);
    assert!(cfg.disable_web_search);
    assert!(cfg.disable_activity_history);
    assert!(cfg.disable_tailored_experiences);
    assert!(cfg.disable_error_reporting);
    assert!(cfg.restrict_app_permissions);
}

#[test]
fn copilot_defaults() {
    let cfg = wincrab_core::config::Copilot::default();
    assert!(cfg.disable);
}

#[test]
fn edge_defaults() {
    let cfg = wincrab_core::config::Edge::default();
    assert!(cfg.disable_first_run);
    assert!(cfg.disable_default_browser_nag);
    assert!(cfg.disable_sidebar);
    assert!(cfg.disable_search_bar);
}

#[test]
fn visuals_defaults() {
    let cfg = wincrab_core::config::Visuals::default();
    assert!(cfg.optimize_for_performance);
    assert!(cfg.disable_lock_screen_tips);
    assert!(cfg.disable_suggestions);
}

#[test]
fn taskbar_defaults() {
    let cfg = wincrab_core::config::Taskbar::default();
    assert!(cfg.hide_widgets_button);
    assert!(cfg.hide_chat_button);
    assert!(cfg.search_icon_only);
    assert!(cfg.disable_start_recommendations);
}

#[test]
fn services_defaults() {
    let cfg = wincrab_core::config::Services::default();
    assert!(cfg.disable_diagtrack);
    assert!(cfg.disable_dmwappush);
    assert!(cfg.disable_wer);
    assert!(cfg.disable_xbox);
    assert!(cfg.disable_maps_broker);
    assert!(cfg.disable_retail_demo);
    assert!(cfg.disable_remote_registry);
    assert!(cfg.disable_geolocation);
}

#[test]
fn scheduled_tasks_defaults() {
    let cfg = wincrab_core::config::ScheduledTasks::default();
    assert!(cfg.remove_telemetry_tasks);
    assert!(cfg.remove_ceip_tasks);
    assert!(cfg.remove_disk_diagnostic_tasks);
    assert!(cfg.remove_maps_task);
    assert!(cfg.remove_feedback_tasks);
}

#[test]
fn oobe_defaults() {
    let cfg = wincrab_core::config::Oobe::default();
    assert!(cfg.inject_autounattend);
    assert!(cfg.skip_microsoft_account);
    assert!(cfg.skip_privacy_screens);
    assert!(cfg.skip_finish_setup_nag);
}

#[test]
fn seelen_defaults() {
    let cfg = wincrab_core::config::Seelen::default();
    assert!(cfg.bundle);
    assert!(cfg.replace_shell);
    assert!(cfg.remove_windows_search_ui);
    assert!(cfg.remove_start_experience);
}

#[test]
fn drivers_defaults() {
    let cfg = wincrab_core::config::Drivers::default();
    assert!(!cfg.btrfs);
    assert!(!cfg.ext4);
    assert!(!cfg.winfsp);
    assert!(!cfg.mergerfs);
    assert!(!cfg.any_enabled());
}

// ===========================================================================
// Config serde edge cases
// ===========================================================================

#[test]
fn config_preserves_all_fields_through_roundtrip() {
    let mut config = Config { wim_index: wincrab_core::WimIndex(3), ..Config::default() };
    config.apps.remove_store = true;
    config.apps.remove_bloatware = false;
    config.telemetry.disable = false;
    config.privacy.disable_advertising_id = false;
    config.copilot.disable = false;
    config.edge.disable_first_run = false;
    config.visuals.optimize_for_performance = false;
    config.taskbar.hide_widgets_button = false;
    config.services.disable_diagtrack = false;
    config.scheduled_tasks.remove_telemetry_tasks = false;
    config.oobe.inject_autounattend = false;
    config.seelen.bundle = false;
    config.drivers.btrfs = true;

    let toml_str = toml::to_string_pretty(&config).unwrap();
    let loaded: Config = toml::from_str(&toml_str).unwrap();

    assert_eq!(loaded.wim_index, 3);
    assert!(loaded.apps.remove_store);
    assert!(!loaded.apps.remove_bloatware);
    assert!(!loaded.telemetry.disable);
    assert!(!loaded.privacy.disable_advertising_id);
    assert!(!loaded.copilot.disable);
    assert!(!loaded.edge.disable_first_run);
    assert!(!loaded.visuals.optimize_for_performance);
    assert!(!loaded.taskbar.hide_widgets_button);
    assert!(!loaded.services.disable_diagtrack);
    assert!(!loaded.scheduled_tasks.remove_telemetry_tasks);
    assert!(!loaded.oobe.inject_autounattend);
    assert!(!loaded.seelen.bundle);
    assert!(loaded.drivers.btrfs);
}

#[test]
fn config_from_toml_only_one_section() {
    let cfg: Config = toml::from_str("[copilot]\ndisable = false").unwrap();
    assert!(!cfg.copilot.disable);
    // Everything else should be default.
    assert_eq!(cfg.wim_index, 6);
    assert!(cfg.apps.remove_bloatware);
    assert!(cfg.telemetry.disable);
}

#[test]
fn prune_stats_is_debug() {
    let stats = wincrab_core::debloat::PruneStats::default();
    let debug = format!("{stats:?}");
    assert!(debug.contains("PruneStats"));
}

#[test]
fn work_dirs_fields_are_public() {
    let dir = tempfile::tempdir().unwrap();
    let dirs = wincrab_core::pipeline::WorkDirs::new(&dir.path().join("t")).unwrap();
    // Verify fields are accessible.
    let _root = &dirs.root;
    let _staging = &dirs.staging;
    let _wim_mount = &dirs.wim_mount;
}

// ===========================================================================
// GitHub module smoke tests
// ===========================================================================

#[test]
fn github_simple_asset_accessible() {
    let asset = wincrab_core::github::SimpleAsset {
        api_url: "https://example.com",
        label: "test",
        predicate: |_| true,
    };
    assert_eq!(
        wincrab_core::github::GitHubAsset::label(&asset),
        "test"
    );
}

#[test]
fn github_select_asset_url_accessible() {
    let json = r#"    "browser_download_url": "https://example.com/file.zip""#;
    let url = wincrab_core::github::select_asset_url(json, "test", |_| 1).unwrap();
    assert_eq!(url, "https://example.com/file.zip");
}

// ===========================================================================
// WimIndex newtype smoke tests
// ===========================================================================

#[test]
fn wim_index_display() {
    let idx = wincrab_core::WimIndex(6);
    assert_eq!(format!("{idx}"), "6");
}

#[test]
fn wim_index_partial_eq_u32() {
    let idx = wincrab_core::WimIndex(6);
    assert_eq!(idx, 6u32);
    assert_ne!(idx, 7u32);
}

#[test]
fn wim_index_partial_ord_u32() {
    let idx = wincrab_core::WimIndex(6);
    assert!(idx > 5u32);
    assert!(idx < 7u32);
}

#[test]
fn wim_index_new_and_get() {
    let idx = wincrab_core::WimIndex::new(42);
    assert_eq!(idx.get(), 42);
}

#[test]
fn wim_index_serde_transparent() {
    let idx = wincrab_core::WimIndex(6);
    let json = serde_json::to_string(&idx).unwrap();
    assert_eq!(json, "6");
    let back: wincrab_core::WimIndex = serde_json::from_str("3").unwrap();
    assert_eq!(back, 3u32);
}

#[test]
fn wim_index_copy() {
    let idx = wincrab_core::WimIndex(6);
    let copy = idx;
    assert_eq!(idx, copy);
}

#[test]
fn wim_index_hash() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(wincrab_core::WimIndex(1));
    set.insert(wincrab_core::WimIndex(2));
    set.insert(wincrab_core::WimIndex(1));
    assert_eq!(set.len(), 2);
}

// ===========================================================================
// New config section defaults smoke tests
// ===========================================================================

#[test]
fn defender_defaults() {
    let cfg = wincrab_core::config::Defender::default();
    assert!(cfg.disable_realtime);
    assert!(cfg.disable_smartscreen);
    assert!(cfg.disable_cloud_protection);
    assert!(cfg.disable_sample_submission);
    assert!(cfg.disable_services);
}

#[test]
fn windows_update_defaults() {
    let cfg = wincrab_core::config::WindowsUpdate::default();
    assert!(!cfg.disable_auto_updates);
    assert!(cfg.disable_delivery_optimization);
    assert!(!cfg.disable_update_tasks);
    assert!(!cfg.exclude_driver_updates);
    assert!(cfg.no_auto_reboot);
}

#[test]
fn explorer_defaults() {
    let cfg = wincrab_core::config::Explorer::default();
    assert!(cfg.classic_context_menu);
    assert!(cfg.show_file_extensions);
    assert!(!cfg.show_hidden_files);
    assert!(cfg.launch_to_this_pc);
    assert!(cfg.disable_recent_files);
    assert!(!cfg.disable_snap_layouts);
}

#[test]
fn performance_defaults() {
    let cfg = wincrab_core::config::Performance::default();
    assert!(cfg.high_perf_power_plan);
    assert!(cfg.faster_shutdown);
    assert!(cfg.disable_game_dvr);
    assert!(cfg.disable_background_apps);
    assert!(cfg.ntfs_disable_last_access);
    assert!(cfg.ntfs_disable_8dot3);
    assert!(!cfg.network_tuning);
}

#[test]
fn security_defaults() {
    let cfg = wincrab_core::config::Security::default();
    assert!(!cfg.asr_rules);
    assert!(cfg.disable_remote_desktop);
    assert!(cfg.disable_smb1);
}

#[test]
fn recall_defaults() {
    let cfg = wincrab_core::config::Recall::default();
    assert!(cfg.disable);
}

#[test]
fn inject_defaults() {
    let cfg = wincrab_core::config::Inject::default();
    assert!(cfg.files.is_empty());
}

#[test]
fn hooks_defaults() {
    let cfg = wincrab_core::config::Hooks::default();
    assert!(cfg.pre_extract.is_none());
    assert!(cfg.post_debloat.is_none());
    assert!(cfg.pre_repack.is_none());
    assert!(cfg.post_build.is_none());
}

// ===========================================================================
// Expanded config field defaults smoke tests
// ===========================================================================

#[test]
fn telemetry_expanded_defaults() {
    let cfg = wincrab_core::config::Telemetry::default();
    assert!(cfg.block_telemetry_hosts);
    assert!(cfg.extra_blocked_hosts.is_empty());
    assert!(cfg.disable_clipboard_sync);
    assert!(cfg.disable_find_my_device);
    assert!(cfg.disable_input_personalization);
    assert!(cfg.disable_wifi_sense);
    assert!(cfg.set_feedback_never);
}

#[test]
fn privacy_expanded_defaults() {
    let cfg = wincrab_core::config::Privacy::default();
    assert!(cfg.disable_suggested_actions);
    assert!(cfg.disable_spotlight_ads);
}

#[test]
fn visuals_expanded_defaults() {
    let cfg = wincrab_core::config::Visuals::default();
    assert!(cfg.dark_mode);
}

#[test]
fn taskbar_expanded_defaults() {
    let cfg = wincrab_core::config::Taskbar::default();
    assert!(cfg.left_align);
    assert!(!cfg.disable_notification_center);
}

#[test]
fn services_expanded_defaults() {
    let cfg = wincrab_core::config::Services::default();
    assert!(!cfg.disable_search);
    assert!(!cfg.disable_sysmain);
    assert!(cfg.disable_ssdp);
    assert!(cfg.disable_upnp);
    assert!(cfg.disable_fax);
    assert!(!cfg.disable_print_spooler);
    assert!(cfg.disable_wmp_sharing);
    assert!(cfg.disable_widgets_service);
    assert!(cfg.disable_telephony);
}

#[test]
fn oobe_expanded_defaults() {
    let cfg = wincrab_core::config::Oobe::default();
    assert!(cfg.bypass_tpm);
    assert!(cfg.bypass_secureboot);
    assert!(cfg.bypass_ram);
    assert!(cfg.timezone.is_none());
    assert!(cfg.local_account_name.is_none());
    assert!(cfg.local_account_password.is_none());
    assert!(!cfg.auto_logon);
    assert!(cfg.disable_bitlocker);
    assert!(cfg.computer_name.is_none());
    assert!(cfg.product_key.is_none());
    assert!(cfg.first_logon_commands.is_empty());
    assert!(cfg.convert_edition.is_none());
    assert!(cfg.skip_auto_activation);
    assert!(!cfg.auto_partition);
}

#[test]
fn drivers_expanded_defaults() {
    let cfg = wincrab_core::config::Drivers::default();
    assert!(!cfg.virtio);
}

// ===========================================================================
// Module accessibility smoke tests (new modules)
// ===========================================================================

#[test]
fn profiles_module_accessible() {
    assert_eq!(wincrab_core::profiles::PROFILE_NAMES.len(), 6);
}

#[test]
fn profiles_load_returns_config() {
    let cfg = wincrab_core::profiles::load_profile("minimal").unwrap();
    assert!(cfg.apps.remove_store);
}

#[test]
fn hosts_module_accessible() {
    let dir = tempfile::tempdir().unwrap();
    let config = wincrab_core::config::Telemetry {
        block_telemetry_hosts: false,
        ..Default::default()
    };
    wincrab_core::hosts::inject_telemetry_hosts(dir.path(), &config).unwrap();
}

#[test]
fn performance_module_accessible() {
    let config = wincrab_core::config::Performance::default();
    let script = wincrab_core::performance::generate_performance_script(&config);
    assert!(!script.is_empty());
}

#[test]
fn hooks_module_accessible() {
    let result = wincrab_core::hooks::run_hook("test", &None, &[]);
    assert!(result.is_ok());
}

#[test]
fn manifest_module_accessible() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.bin");
    std::fs::write(&path, b"data").unwrap();
    let hash = wincrab_core::manifest::compute_sha256(&path).unwrap();
    assert_eq!(hash.len(), 64);
}

#[test]
fn doctor_module_accessible() {
    let result = wincrab_core::doctor::run_doctor();
    assert!(result.is_ok());
}

// ===========================================================================
// New config section roundtrip smoke tests
// ===========================================================================

#[test]
fn defender_roundtrip() {
    let mut cfg = Config::default();
    cfg.seelen.bundle = false;
    cfg.defender.disable_realtime = false;
    cfg.defender.disable_smartscreen = false;
    let s = toml::to_string(&cfg).unwrap();
    let loaded: Config = toml::from_str(&s).unwrap();
    assert!(!loaded.defender.disable_realtime);
    assert!(!loaded.defender.disable_smartscreen);
}

#[test]
fn windows_update_roundtrip() {
    let mut cfg = Config::default();
    cfg.seelen.bundle = false;
    cfg.windows_update.disable_auto_updates = true;
    cfg.windows_update.exclude_driver_updates = true;
    let s = toml::to_string(&cfg).unwrap();
    let loaded: Config = toml::from_str(&s).unwrap();
    assert!(loaded.windows_update.disable_auto_updates);
    assert!(loaded.windows_update.exclude_driver_updates);
}

#[test]
fn explorer_roundtrip() {
    let mut cfg = Config::default();
    cfg.seelen.bundle = false;
    cfg.explorer.show_hidden_files = true;
    cfg.explorer.disable_snap_layouts = true;
    let s = toml::to_string(&cfg).unwrap();
    let loaded: Config = toml::from_str(&s).unwrap();
    assert!(loaded.explorer.show_hidden_files);
    assert!(loaded.explorer.disable_snap_layouts);
}

#[test]
fn security_roundtrip() {
    let mut cfg = Config::default();
    cfg.seelen.bundle = false;
    cfg.security.asr_rules = true;
    cfg.security.disable_remote_desktop = false;
    let s = toml::to_string(&cfg).unwrap();
    let loaded: Config = toml::from_str(&s).unwrap();
    assert!(loaded.security.asr_rules);
    assert!(!loaded.security.disable_remote_desktop);
}

#[test]
fn recall_roundtrip() {
    let mut cfg = Config::default();
    cfg.seelen.bundle = false;
    cfg.recall.disable = false;
    let s = toml::to_string(&cfg).unwrap();
    let loaded: Config = toml::from_str(&s).unwrap();
    assert!(!loaded.recall.disable);
}

#[test]
fn inject_roundtrip() {
    let mut cfg = Config::default();
    cfg.seelen.bundle = false;
    cfg.inject.files = vec![wincrab_core::config::InjectEntry {
        src: std::path::PathBuf::from("/tmp/test.txt"),
        dest: "Users/Public/test.txt".into(),
    }];
    let s = toml::to_string(&cfg).unwrap();
    let loaded: Config = toml::from_str(&s).unwrap();
    assert_eq!(loaded.inject.files.len(), 1);
    assert_eq!(loaded.inject.files[0].dest, "Users/Public/test.txt");
}

#[test]
fn hooks_roundtrip() {
    let mut cfg = Config::default();
    cfg.seelen.bundle = false;
    cfg.hooks.pre_extract = Some("echo pre".into());
    cfg.hooks.post_build = Some("echo post".into());
    let s = toml::to_string(&cfg).unwrap();
    let loaded: Config = toml::from_str(&s).unwrap();
    assert_eq!(loaded.hooks.pre_extract.as_deref(), Some("echo pre"));
    assert_eq!(loaded.hooks.post_build.as_deref(), Some("echo post"));
    assert!(loaded.hooks.post_debloat.is_none());
    assert!(loaded.hooks.pre_repack.is_none());
}

// ===========================================================================
// Config validation smoke tests (new features)
// ===========================================================================

#[test]
fn validate_valid_editions() {
    for edition in &["Professional", "Enterprise", "Education", "Core"] {
        let mut cfg = Config::default();
        cfg.oobe.convert_edition = Some((*edition).into());
        assert!(cfg.validate().is_ok(), "edition {edition} should be valid");
    }
}

#[test]
fn validate_invalid_edition() {
    let mut cfg = Config::default();
    cfg.oobe.convert_edition = Some("HomeBasic".into());
    assert!(cfg.validate().is_err());
}

#[test]
fn validate_no_edition_is_ok() {
    let cfg = Config::default();
    assert!(cfg.validate().is_ok());
}

#[test]
fn config_profile_field_default_is_none() {
    let cfg = Config::default();
    assert!(cfg.profile.is_none());
}

#[test]
fn config_profile_field_roundtrips() {
    let mut cfg = Config::default();
    cfg.seelen.bundle = false;
    cfg.profile = Some("gaming".into());
    let s = toml::to_string(&cfg).unwrap();
    let loaded: Config = toml::from_str(&s).unwrap();
    assert_eq!(loaded.profile.as_deref(), Some("gaming"));
}

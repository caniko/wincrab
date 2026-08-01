use crate::config::Config;
use crate::error::Error;

const PROFILE_MINIMAL: &str = r#"
wim_index = 6

[apps]
remove_bloatware = true
remove_xbox = true
remove_teams = true
remove_onedrive = true
remove_cortana = true
remove_store = true
remove_outlook = true
remove_mail = true
remove_dev_home = true
remove_phone_link = true

[telemetry]
disable = true
disable_ceip = true
disable_app_telemetry = true
block_telemetry_hosts = true
disable_clipboard_sync = true
disable_find_my_device = true
disable_input_personalization = true
disable_wifi_sense = true
set_feedback_never = true

[privacy]
disable_advertising_id = true
disable_web_search = true
disable_activity_history = true
disable_tailored_experiences = true
disable_error_reporting = true
restrict_app_permissions = true
disable_suggested_actions = true
disable_spotlight_ads = true

[copilot]
disable = true

[visuals]
optimize_for_performance = true
disable_lock_screen_tips = true
disable_suggestions = true
dark_mode = true

[services]
disable_diagtrack = true
disable_dmwappush = true
disable_wer = true
disable_xbox = true
disable_maps_broker = true
disable_retail_demo = true
disable_remote_registry = true
disable_geolocation = true
disable_search = true
disable_sysmain = true
disable_ssdp = true
disable_upnp = true
disable_fax = true
disable_print_spooler = true
disable_wmp_sharing = true
disable_widgets_service = true
disable_telephony = true

[seelen]
bundle = false
replace_shell = false
remove_windows_search_ui = false
remove_start_experience = false

[defender]
disable_realtime = true
disable_smartscreen = true
disable_cloud_protection = true
disable_sample_submission = true
disable_services = true

[recall]
disable = true
"#;

const PROFILE_GAMING: &str = r#"
wim_index = 6

[apps]
remove_bloatware = true
remove_xbox = false
remove_teams = true
remove_onedrive = true
remove_cortana = true
remove_store = false
remove_outlook = true
remove_mail = true
remove_dev_home = true
remove_phone_link = true

[telemetry]
disable = true
disable_ceip = true
disable_app_telemetry = true
block_telemetry_hosts = true
disable_clipboard_sync = true
disable_find_my_device = true
disable_input_personalization = true
disable_wifi_sense = true
set_feedback_never = true

[privacy]
disable_advertising_id = true
disable_web_search = true
disable_activity_history = true
disable_tailored_experiences = true
disable_error_reporting = true
restrict_app_permissions = false
disable_suggested_actions = true
disable_spotlight_ads = true

[copilot]
disable = true

[visuals]
optimize_for_performance = true
disable_lock_screen_tips = true
disable_suggestions = true
dark_mode = true

[services]
disable_diagtrack = true
disable_dmwappush = true
disable_wer = true
disable_xbox = false
disable_maps_broker = true
disable_retail_demo = true
disable_remote_registry = true
disable_geolocation = true
disable_search = false
disable_sysmain = false
disable_ssdp = true
disable_upnp = true
disable_fax = true
disable_print_spooler = false
disable_wmp_sharing = true
disable_widgets_service = true
disable_telephony = true

[seelen]
bundle = true
replace_shell = true
remove_windows_search_ui = true
remove_start_experience = true

[performance]
high_perf_power_plan = true
faster_shutdown = true
disable_game_dvr = true
disable_background_apps = true
ntfs_disable_last_access = true
ntfs_disable_8dot3 = true
network_tuning = true

[defender]
disable_realtime = false
disable_smartscreen = false
disable_cloud_protection = false
disable_sample_submission = true
disable_services = false

[recall]
disable = true
"#;

const PROFILE_PRIVACY: &str = r#"
wim_index = 6

[apps]
remove_bloatware = true
remove_xbox = true
remove_teams = true
remove_onedrive = true
remove_cortana = true
remove_store = false
remove_outlook = true
remove_mail = true
remove_dev_home = true
remove_phone_link = true

[telemetry]
disable = true
disable_ceip = true
disable_app_telemetry = true
block_telemetry_hosts = true
disable_clipboard_sync = true
disable_find_my_device = true
disable_input_personalization = true
disable_wifi_sense = true
set_feedback_never = true

[privacy]
disable_advertising_id = true
disable_web_search = true
disable_activity_history = true
disable_tailored_experiences = true
disable_error_reporting = true
restrict_app_permissions = true
disable_suggested_actions = true
disable_spotlight_ads = true

[copilot]
disable = true

[visuals]
optimize_for_performance = false
disable_lock_screen_tips = true
disable_suggestions = true
dark_mode = true

[services]
disable_diagtrack = true
disable_dmwappush = true
disable_wer = true
disable_xbox = true
disable_maps_broker = true
disable_retail_demo = true
disable_remote_registry = true
disable_geolocation = true
disable_search = false
disable_sysmain = false
disable_ssdp = true
disable_upnp = true
disable_fax = true
disable_print_spooler = false
disable_wmp_sharing = true
disable_widgets_service = true
disable_telephony = true

[defender]
disable_realtime = true
disable_smartscreen = true
disable_cloud_protection = true
disable_sample_submission = true
disable_services = true

[recall]
disable = true
"#;

const PROFILE_ENTERPRISE: &str = r#"
wim_index = 6

[apps]
remove_bloatware = true
remove_xbox = true
remove_teams = true
remove_onedrive = false
remove_cortana = true
remove_store = false
remove_outlook = false
remove_mail = false
remove_dev_home = true
remove_phone_link = true

[telemetry]
disable = true
disable_ceip = true
disable_app_telemetry = true
block_telemetry_hosts = false
disable_clipboard_sync = true
disable_find_my_device = false
disable_input_personalization = true
disable_wifi_sense = true
set_feedback_never = true

[privacy]
disable_advertising_id = true
disable_web_search = true
disable_activity_history = true
disable_tailored_experiences = true
disable_error_reporting = false
restrict_app_permissions = false
disable_suggested_actions = true
disable_spotlight_ads = true

[copilot]
disable = true

[visuals]
optimize_for_performance = false
disable_lock_screen_tips = true
disable_suggestions = true
dark_mode = false

[services]
disable_diagtrack = true
disable_dmwappush = true
disable_wer = false
disable_xbox = true
disable_maps_broker = true
disable_retail_demo = true
disable_remote_registry = false
disable_geolocation = false
disable_search = false
disable_sysmain = false
disable_ssdp = true
disable_upnp = true
disable_fax = true
disable_print_spooler = false
disable_wmp_sharing = true
disable_widgets_service = true
disable_telephony = true

[seelen]
bundle = false
replace_shell = false
remove_windows_search_ui = false
remove_start_experience = false

[defender]
disable_realtime = false
disable_smartscreen = false
disable_cloud_protection = false
disable_sample_submission = false
disable_services = false

[security]
asr_rules = true
disable_remote_desktop = false
disable_smb1 = true

[windows_update]
disable_auto_updates = false
disable_delivery_optimization = false
disable_update_tasks = false
exclude_driver_updates = false
no_auto_reboot = true

[recall]
disable = true
"#;

const PROFILE_VM: &str = r#"
wim_index = 6

[apps]
remove_bloatware = true
remove_xbox = true
remove_teams = true
remove_onedrive = true
remove_cortana = true
remove_store = false
remove_outlook = true
remove_mail = true
remove_dev_home = true
remove_phone_link = true

[telemetry]
disable = true
disable_ceip = true
disable_app_telemetry = true
block_telemetry_hosts = true
disable_clipboard_sync = true
disable_find_my_device = true
disable_input_personalization = true
disable_wifi_sense = true
set_feedback_never = true

[privacy]
disable_advertising_id = true
disable_web_search = true
disable_activity_history = true
disable_tailored_experiences = true
disable_error_reporting = true
restrict_app_permissions = true
disable_suggested_actions = true
disable_spotlight_ads = true

[copilot]
disable = true

[visuals]
optimize_for_performance = true
disable_lock_screen_tips = true
disable_suggestions = true
dark_mode = true

[services]
disable_diagtrack = true
disable_dmwappush = true
disable_wer = true
disable_xbox = true
disable_maps_broker = true
disable_retail_demo = true
disable_remote_registry = true
disable_geolocation = true
disable_search = false
disable_sysmain = true
disable_ssdp = true
disable_upnp = true
disable_fax = true
disable_print_spooler = true
disable_wmp_sharing = true
disable_widgets_service = true
disable_telephony = true

[seelen]
bundle = false
replace_shell = false
remove_windows_search_ui = false
remove_start_experience = false

[drivers]
virtio = true

[performance]
high_perf_power_plan = true
faster_shutdown = true
disable_game_dvr = true
disable_background_apps = true
ntfs_disable_last_access = true
ntfs_disable_8dot3 = true
network_tuning = false

[defender]
disable_realtime = true
disable_smartscreen = true
disable_cloud_protection = true
disable_sample_submission = true
disable_services = true

[recall]
disable = true
"#;

const PROFILE_VM_SEELEN: &str = r#"
wim_index = 6

[apps]
remove_bloatware = true
remove_xbox = true
remove_teams = true
remove_onedrive = true
remove_cortana = true
remove_store = false
remove_outlook = true
remove_mail = true
remove_dev_home = true
remove_phone_link = true

[telemetry]
disable = true
disable_ceip = true
disable_app_telemetry = true
block_telemetry_hosts = true
disable_clipboard_sync = true
disable_find_my_device = true
disable_input_personalization = true
disable_wifi_sense = true
set_feedback_never = true

[privacy]
disable_advertising_id = true
disable_web_search = true
disable_activity_history = true
disable_tailored_experiences = true
disable_error_reporting = true
restrict_app_permissions = true
disable_suggested_actions = true
disable_spotlight_ads = true

[copilot]
disable = true

[edge]
disable_first_run = true
disable_default_browser_nag = true
disable_sidebar = true
disable_search_bar = true

[visuals]
optimize_for_performance = true
disable_lock_screen_tips = true
disable_suggestions = true
dark_mode = true

[taskbar]
hide_widgets_button = true
hide_chat_button = true
search_icon_only = true
disable_start_recommendations = true
left_align = true

[services]
disable_diagtrack = true
disable_dmwappush = true
disable_wer = true
disable_xbox = true
disable_maps_broker = true
disable_retail_demo = true
disable_remote_registry = true
disable_geolocation = true
disable_search = false
disable_sysmain = true
disable_ssdp = true
disable_upnp = true
disable_fax = true
disable_print_spooler = true
disable_wmp_sharing = true
disable_widgets_service = true
disable_telephony = true

[seelen]
bundle = true
replace_shell = false
remove_windows_search_ui = false
remove_start_experience = false

[drivers]
virtio = true

[performance]
high_perf_power_plan = true
faster_shutdown = true
disable_game_dvr = true
disable_background_apps = true
ntfs_disable_last_access = true
ntfs_disable_8dot3 = true
network_tuning = false

[defender]
disable_realtime = true
disable_smartscreen = true
disable_cloud_protection = true
disable_sample_submission = true
disable_services = true

[recall]
disable = true

[oobe]
inject_autounattend = true
skip_microsoft_account = true
skip_privacy_screens = true
skip_finish_setup_nag = true
bypass_tpm = true
bypass_secureboot = true
bypass_ram = true
disable_bitlocker = true
skip_auto_activation = true

[scheduled_tasks]
remove_telemetry_tasks = true
remove_ceip_tasks = true
remove_disk_diagnostic_tasks = true
remove_maps_task = true
remove_feedback_tasks = true

[explorer]
classic_context_menu = true
show_file_extensions = true
launch_to_this_pc = true
disable_recent_files = true

[security]
disable_remote_desktop = true
disable_smb1 = true

[windows_update]
disable_delivery_optimization = true
no_auto_reboot = true
"#;

pub fn load_profile(name: &str) -> Result<Config, Error> {
    let toml_str = match name {
        "minimal" => PROFILE_MINIMAL,
        "gaming" => PROFILE_GAMING,
        "privacy" => PROFILE_PRIVACY,
        "enterprise" => PROFILE_ENTERPRISE,
        "vm" => PROFILE_VM,
        "vm-seelen" => PROFILE_VM_SEELEN,
        _ => {
            return Err(Error::Config {
                message: format!(
                    "unknown profile '{name}'. Available: {}",
                    PROFILE_NAMES.join(", ")
                ),
            });
        }
    };

    toml::from_str(toml_str).map_err(|e| Error::Config {
        message: format!("parsing profile '{name}': {e}"),
    })
}

/// Fixed-size array of all known profile names — avoids a heap allocation
/// compared to returning `Vec`.
pub const PROFILE_NAMES: [&str; 6] = [
    "minimal",
    "gaming",
    "privacy",
    "enterprise",
    "vm",
    "vm-seelen",
];

pub fn merge_with_overrides(base: Config, overrides_toml: &str) -> Result<Config, Error> {
    let base_value: toml::Value = toml::Value::try_from(&base).map_err(|e| Error::Config {
        message: format!("serializing base config: {e}"),
    })?;

    let override_value: toml::Value =
        toml::from_str(overrides_toml).map_err(|e| Error::Config {
            message: format!("parsing overrides: {e}"),
        })?;

    let merged = deep_merge(base_value, override_value);

    merged.try_into().map_err(|e| Error::Config {
        message: format!("deserializing merged config: {e}"),
    })
}

fn deep_merge(base: toml::Value, overlay: toml::Value) -> toml::Value {
    match (base, overlay) {
        (toml::Value::Table(mut base_table), toml::Value::Table(overlay_table)) => {
            for (key, overlay_val) in overlay_table {
                let merged_val = match base_table.remove(&key) {
                    Some(base_val) => deep_merge(base_val, overlay_val),
                    None => overlay_val,
                };
                base_table.insert(key, merged_val);
            }
            toml::Value::Table(base_table)
        }
        (_, overlay) => overlay,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_names_contains_all() {
        assert_eq!(PROFILE_NAMES.len(), 6);
        assert!(PROFILE_NAMES.contains(&"minimal"));
        assert!(PROFILE_NAMES.contains(&"gaming"));
        assert!(PROFILE_NAMES.contains(&"privacy"));
        assert!(PROFILE_NAMES.contains(&"enterprise"));
        assert!(PROFILE_NAMES.contains(&"vm"));
        assert!(PROFILE_NAMES.contains(&"vm-seelen"));
    }

    #[test]
    fn load_minimal_profile() {
        let config = load_profile("minimal").unwrap();
        assert!(config.apps.remove_store);
        assert!(!config.seelen.bundle);
        assert!(config.defender.disable_realtime);
    }

    #[test]
    fn load_gaming_profile() {
        let config = load_profile("gaming").unwrap();
        assert!(!config.apps.remove_xbox);
        assert!(!config.apps.remove_store);
        assert!(config.performance.high_perf_power_plan);
        assert!(config.performance.network_tuning);
        assert!(config.performance.disable_game_dvr);
        assert!(config.seelen.bundle);
    }

    #[test]
    fn load_privacy_profile() {
        let config = load_profile("privacy").unwrap();
        assert!(config.telemetry.block_telemetry_hosts);
        assert!(config.privacy.disable_advertising_id);
        assert!(config.defender.disable_realtime);
        assert!(config.copilot.disable);
        assert!(config.recall.disable);
    }

    #[test]
    fn load_enterprise_profile() {
        let config = load_profile("enterprise").unwrap();
        assert!(!config.services.disable_print_spooler);
        assert!(!config.defender.disable_services);
        assert!(config.security.asr_rules);
        assert!(!config.seelen.bundle);
        assert!(!config.security.disable_remote_desktop);
    }

    #[test]
    fn load_vm_profile() {
        let config = load_profile("vm").unwrap();
        assert!(config.drivers.virtio);
        assert!(config.defender.disable_services);
        assert!(!config.seelen.bundle);
    }

    #[test]
    fn load_vm_seelen_profile() {
        let config = load_profile("vm-seelen").unwrap();
        assert!(config.drivers.virtio);
        assert!(config.defender.disable_services);
        assert!(config.seelen.bundle);
        assert!(!config.seelen.replace_shell);
    }

    #[test]
    fn load_unknown_profile_returns_error() {
        let result = load_profile("nonexistent");
        assert!(result.is_err());
        match result.unwrap_err() {
            Error::Config { message } => {
                assert!(message.contains("nonexistent"));
                assert!(message.contains("Available"));
            }
            other => panic!("expected Config error, got: {other:?}"),
        }
    }

    #[test]
    fn merge_overrides_simple() {
        let base = load_profile("minimal").unwrap();
        let merged = merge_with_overrides(base, "wim_index = 3").unwrap();
        assert_eq!(merged.wim_index, 3);
        assert!(merged.apps.remove_store);
    }

    #[test]
    fn merge_overrides_nested() {
        let base = load_profile("minimal").unwrap();
        let merged = merge_with_overrides(base, "[apps]\nremove_store = false\n").unwrap();
        assert!(!merged.apps.remove_store);
        assert!(merged.apps.remove_bloatware);
    }

    #[test]
    fn merge_empty_overrides_preserves_base() {
        let base = load_profile("gaming").unwrap();
        let merged = merge_with_overrides(base, "").unwrap();
        assert_eq!(merged.wim_index, 6);
        assert!(merged.performance.high_perf_power_plan);
    }

    #[test]
    fn deep_merge_replaces_scalars() {
        let base: toml::Value = toml::from_str("a = 1\nb = 2").unwrap();
        let overlay: toml::Value = toml::from_str("a = 99").unwrap();
        let merged = deep_merge(base, overlay);
        assert_eq!(merged["a"].as_integer(), Some(99));
        assert_eq!(merged["b"].as_integer(), Some(2));
    }

    #[test]
    fn deep_merge_nested_tables() {
        let base: toml::Value = toml::from_str("[t]\na = 1\nb = 2").unwrap();
        let overlay: toml::Value = toml::from_str("[t]\nb = 99").unwrap();
        let merged = deep_merge(base, overlay);
        assert_eq!(merged["t"]["a"].as_integer(), Some(1));
        assert_eq!(merged["t"]["b"].as_integer(), Some(99));
    }

    #[test]
    fn all_profiles_parse_successfully() {
        for name in &PROFILE_NAMES {
            let result = load_profile(name);
            assert!(
                result.is_ok(),
                "profile '{name}' failed to parse: {:?}",
                result.err()
            );
        }
    }
}

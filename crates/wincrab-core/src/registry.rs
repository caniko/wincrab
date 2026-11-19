#[cfg(not(windows))]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(not(windows))]
use std::process::Stdio;

use tracing::{info, warn};

use crate::config::Config;
use crate::error::Error;

/// Hive files inside a mounted Windows image under `Windows/System32/config/`.
struct HivePaths {
    software: PathBuf,
    system: PathBuf,
    default: PathBuf,
    ntuser: PathBuf,
}

impl HivePaths {
    fn from_mount(mount_dir: &Path) -> Self {
        let config_dir = mount_dir.join("Windows").join("System32").join("config");
        Self {
            software: config_dir.join("SOFTWARE"),
            system: config_dir.join("SYSTEM"),
            default: config_dir.join("DEFAULT"),
            ntuser: mount_dir.join("Users").join("Default").join("NTUSER.DAT"),
        }
    }

    fn validate(&self) -> Result<(), Error> {
        for (_name, path) in [
            ("SOFTWARE", &self.software),
            ("SYSTEM", &self.system),
            ("DEFAULT", &self.default),
        ] {
            if !path.exists() {
                return Err(Error::HiveNotFound { path: path.clone() });
            }
        }
        if !self.ntuser.exists() {
            warn!(
                path = %self.ntuser.display(),
                "default NTUSER.DAT not found -- user-level registry edits will be skipped"
            );
        }
        Ok(())
    }
}

/// Apply all configured registry modifications to the offline hives.
pub fn apply_registry_edits(mount_dir: &Path, config: &Config) -> Result<(), Error> {
    let hives = HivePaths::from_mount(mount_dir);
    hives.validate()?;

    // Telemetry
    if config.telemetry.disable {
        disable_telemetry(&hives, config)?;
    }

    // Privacy (HKLM parts only; HKCU AdvertisingInfo is a unique key — safe)
    apply_privacy_settings(&hives, config)?;

    // Copilot (HKLM policy + NTUSER policy path — both unique keys, safe)
    if config.copilot.disable {
        disable_copilot(&hives)?;
    }

    // Edge
    apply_edge_policies(&hives, config)?;

    // Visuals — unique NTUSER keys only (VisualEffects, DWM, Desktop, Themes)
    if config.visuals.optimize_for_performance {
        apply_visual_unique_keys(&hives)?;
    }

    // Dark mode (NTUSER — unique key path, safe)
    if config.visuals.dark_mode && hives.ntuser.exists() {
        apply_dark_mode(&hives)?;
    }

    // --- Consolidated NTUSER writes for keys shared across config sections ---
    if hives.ntuser.exists() {
        apply_explorer_advanced_values(&hives, config)?;
        apply_content_delivery_manager_values(&hives, config)?;
    }

    // Defender policies (SOFTWARE + SYSTEM hives)
    apply_defender_policies(&hives, config)?;

    // Windows Update policies (SOFTWARE + SYSTEM hives)
    apply_windows_update_policies(&hives, config)?;

    // Windows Recall / AI (SOFTWARE hive)
    if config.recall.disable {
        apply_recall_policies(&hives)?;
    }

    // Performance tuning (SYSTEM + NTUSER + SOFTWARE hives)
    apply_performance_registry(&hives, config)?;

    // Security hardening (SYSTEM + SOFTWARE hives)
    apply_security_policies(&hives, config)?;

    // Telemetry extensions (NTUSER)
    if hives.ntuser.exists() {
        apply_telemetry_extensions(&hives, config)?;
    }

    // Privacy extensions (NTUSER + SOFTWARE)
    apply_privacy_extensions(&hives, config)?;

    // Notification center (SOFTWARE)
    if config.taskbar.disable_notification_center {
        apply_notification_center_disable(&hives)?;
    }

    // Recent files (NTUSER)
    if config.explorer.disable_recent_files && hives.ntuser.exists() {
        apply_recent_files_disable(&hives)?;
    }

    // Classic context menu (NTUSER)
    if config.explorer.classic_context_menu && hives.ntuser.exists() {
        apply_classic_context_menu(&hives)?;
    }

    // Services
    disable_services(&hives, config)?;

    // Scheduled tasks — disable via SYSTEM registry (survives install)
    disable_scheduled_tasks_via_registry(&hives, config)?;

    // OOBE nag suppression — UserProfileEngagement is a unique key, safe
    if config.oobe.skip_finish_setup_nag {
        suppress_finish_setup_nag(&hives)?;
    }

    // User-fixup script — some HKCU values (visual effects, dark mode,
    // background apps) get overridden during user profile creation even
    // when set in Default NTUSER.DAT.  A batch script registered via
    // Active Setup re-applies them at first logon for every new user.
    inject_user_fixup(mount_dir, &hives, config)?;

    // First-boot install triggers via Active Setup (HKLM).
    if config.seelen.bundle {
        register_active_setup(
            &hives,
            "WinCrab.SeelenUI",
            "Seelen-UI Installer",
            r"powershell.exe -ExecutionPolicy Bypass -WindowStyle Hidden -File C:\SeelenUI\install.ps1",
        )?;
    }
    if config.drivers.any_enabled() {
        register_active_setup(
            &hives,
            "WinCrab.Drivers",
            "Filesystem Driver Installer",
            r"powershell.exe -ExecutionPolicy Bypass -WindowStyle Hidden -File C:\Drivers\install-drivers.ps1",
        )?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Telemetry
// ---------------------------------------------------------------------------

fn disable_telemetry(hives: &HivePaths, config: &Config) -> Result<(), Error> {
    info!("disabling Windows telemetry via registry");

    // Standard data collection path
    merge_reg_values(
        &hives.software,
        "\\Microsoft\\Windows\\CurrentVersion\\Policies\\DataCollection",
        &[
            ("AllowTelemetry", "dword:00000000"),
            ("MaxTelemetryAllowed", "dword:00000000"),
            ("AllowDeviceNameInTelemetry", "dword:00000000"),
        ],
    )?;

    // Group Policy path — takes precedence and survives Windows Setup overrides
    merge_reg_values(
        &hives.software,
        "\\Policies\\Microsoft\\Windows\\DataCollection",
        &[
            ("AllowTelemetry", "dword:00000000"),
            ("MaxTelemetryAllowed", "dword:00000000"),
        ],
    )?;

    if config.telemetry.disable_ceip {
        merge_reg_values(
            &hives.software,
            "\\Microsoft\\SQMClient\\Windows",
            &[("CEIPEnable", "dword:00000000")],
        )?;
    }

    if config.telemetry.disable_app_telemetry {
        merge_reg_values(
            &hives.software,
            "\\Microsoft\\Windows\\CurrentVersion\\Policies\\AppCompat",
            &[("AITEnable", "dword:00000000")],
        )?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Privacy
// ---------------------------------------------------------------------------

fn apply_privacy_settings(hives: &HivePaths, config: &Config) -> Result<(), Error> {
    let privacy = &config.privacy;

    if privacy.disable_advertising_id && hives.ntuser.exists() {
        info!("disabling advertising ID");
        merge_reg_values(
            &hives.ntuser,
            "\\Software\\Microsoft\\Windows\\CurrentVersion\\AdvertisingInfo",
            &[("Enabled", "dword:00000000")],
        )?;
    }

    if privacy.disable_web_search {
        info!("disabling web search in Start menu");
        // Machine-level policy
        merge_reg_values(
            &hives.software,
            "\\Policies\\Microsoft\\Windows\\Explorer",
            &[("DisableSearchBoxSuggestions", "dword:00000001")],
        )?;

        if hives.ntuser.exists() {
            merge_reg_values(
                &hives.ntuser,
                "\\Software\\Microsoft\\Windows\\CurrentVersion\\Search",
                &[
                    ("BingSearchEnabled", "dword:00000000"),
                    ("CortanaConsent", "dword:00000000"),
                ],
            )?;
        }
    }

    if privacy.disable_activity_history && hives.ntuser.exists() {
        info!("disabling activity history");
        merge_reg_values(
            &hives.software,
            "\\Policies\\Microsoft\\Windows\\System",
            &[
                ("EnableActivityFeed", "dword:00000000"),
                ("PublishUserActivities", "dword:00000000"),
            ],
        )?;
    }

    if privacy.disable_tailored_experiences && hives.ntuser.exists() {
        info!("disabling tailored experiences");
        merge_reg_values(
            &hives.ntuser,
            "\\Software\\Microsoft\\Windows\\CurrentVersion\\Privacy",
            &[(
                "TailoredExperiencesWithDiagnosticDataEnabled",
                "dword:00000000",
            )],
        )?;
    }

    if privacy.disable_error_reporting {
        info!("disabling Windows Error Reporting via registry");
        merge_reg_values(
            &hives.software,
            "\\Microsoft\\Windows\\Windows Error Reporting",
            &[("Disabled", "dword:00000001")],
        )?;
    }

    if privacy.restrict_app_permissions && hives.ntuser.exists() {
        info!("restricting default app permissions");
        let capabilities = [
            "location",
            "webcam",
            "microphone",
            "userNotificationListener",
            "activity",
            "appDiagnostics",
        ];
        for cap in &capabilities {
            merge_reg_values(
                &hives.ntuser,
                &format!(
                    "\\Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\{cap}"
                ),
                &[("Value", "string:Deny")],
            )?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Copilot
// ---------------------------------------------------------------------------

fn disable_copilot(hives: &HivePaths) -> Result<(), Error> {
    info!("disabling Windows Copilot via registry");

    // HKLM policy
    merge_reg_values(
        &hives.software,
        "\\Policies\\Microsoft\\Windows\\WindowsCopilot",
        &[("TurnOffWindowsCopilot", "dword:00000001")],
    )?;

    // HKCU policy
    if hives.ntuser.exists() {
        merge_reg_values(
            &hives.ntuser,
            "\\Software\\Policies\\Microsoft\\Windows\\WindowsCopilot",
            &[("TurnOffWindowsCopilot", "dword:00000001")],
        )?;

        // NOTE: ShowCopilotButton in Explorer\Advanced is written by the
        // consolidated apply_explorer_advanced_values() function.
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Edge policies
// ---------------------------------------------------------------------------

fn apply_edge_policies(hives: &HivePaths, config: &Config) -> Result<(), Error> {
    let edge = &config.edge;
    if !edge.disable_first_run
        && !edge.disable_default_browser_nag
        && !edge.disable_sidebar
        && !edge.disable_search_bar
    {
        return Ok(());
    }

    info!("applying Microsoft Edge policies");

    let mut vals: Vec<(&str, &str)> = Vec::new();

    if edge.disable_first_run {
        vals.push(("HideFirstRunExperience", "dword:00000001"));
    }
    if edge.disable_default_browser_nag {
        vals.push(("DefaultBrowserSettingEnabled", "dword:00000000"));
        vals.push(("DefaultBrowserSettingsCampaignEnabled", "dword:00000000"));
    }
    if edge.disable_sidebar {
        vals.push(("HubsSidebarEnabled", "dword:00000000"));
    }
    if edge.disable_search_bar {
        vals.push(("SearchbarAllowed", "dword:00000000"));
    }

    merge_reg_values(&hives.software, "\\Policies\\Microsoft\\Edge", &vals)?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Visual performance
// ---------------------------------------------------------------------------

/// Apply visual settings to NTUSER keys that are NOT shared with other
/// config sections (i.e., not `Explorer\Advanced` or `ContentDeliveryManager`).
/// Those shared keys are handled by their own consolidated functions.
fn apply_visual_unique_keys(hives: &HivePaths) -> Result<(), Error> {
    info!("optimizing visual settings for performance");

    if hives.ntuser.exists() {
        merge_reg_values(
            &hives.ntuser,
            "\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\VisualEffects",
            &[("VisualFXSetting", "dword:00000002")],
        )?;

        merge_reg_values(
            &hives.ntuser,
            "\\Software\\Microsoft\\Windows\\DWM",
            &[("EnableAeroPeek", "dword:00000000")],
        )?;

        // UserPreferencesMask controls which visual effects are active.
        // 90,12,01,80,10,00,00,00 = Windows "Adjust for best performance"
        // with only "Smooth edges of screen fonts" kept on.
        merge_reg_values(
            &hives.ntuser,
            "\\Control Panel\\Desktop",
            &[("UserPreferencesMask", "hex:90,12,01,80,10,00,00,00")],
        )?;

        merge_reg_values(
            &hives.ntuser,
            "\\Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
            &[("EnableTransparency", "dword:00000000")],
        )?;
    }

    Ok(())
}

/// Inject a `user-fixup.cmd` batch script and register it via Active Setup.
///
/// Windows profile creation overrides some Default NTUSER.DAT values (DWM
/// toggles, Explorer\Advanced animations, dark-mode, background apps).
/// Active Setup is HKLM-based and runs once per new user at first logon,
/// making it more reliable than RunOnce (which lives in NTUSER.DAT and
/// can itself be overridden during profile creation).
fn inject_user_fixup(mount_dir: &Path, hives: &HivePaths, config: &Config) -> Result<(), Error> {
    let mut lines: Vec<&str> = vec!["@echo off"];

    // Visual performance fixups
    if config.visuals.optimize_for_performance {
        lines.extend_from_slice(&[
            r"reg add HKCU\Software\Microsoft\Windows\DWM /v EnableAeroPeek /t REG_DWORD /d 0 /f >nul 2>&1",
            r"reg add HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced /v TaskbarAnimations /t REG_DWORD /d 0 /f >nul 2>&1",
            r"reg add HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\Advanced /v ListviewAlphaSelect /t REG_DWORD /d 0 /f >nul 2>&1",
        ]);
    }

    // Dark mode
    if config.visuals.dark_mode {
        lines.extend_from_slice(&[
            r"reg add HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize /v AppsUseLightTheme /t REG_DWORD /d 0 /f >nul 2>&1",
            r"reg add HKCU\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize /v SystemUsesLightTheme /t REG_DWORD /d 0 /f >nul 2>&1",
        ]);
    }

    // Background apps
    if config.performance.disable_background_apps {
        lines.push(
            r"reg add HKCU\Software\Microsoft\Windows\CurrentVersion\BackgroundAccessApplications /v GlobalUserDisabled /t REG_DWORD /d 1 /f >nul 2>&1",
        );
    }

    // Only write the script if there are actual fixups to apply.
    if lines.len() <= 1 {
        return Ok(());
    }

    let script_dir = mount_dir.join("wincrab");
    crate::error::ensure_dir(&script_dir)?;

    let script_path = script_dir.join("user-fixup.cmd");
    let content = lines.join("\r\n") + "\r\n";
    crate::error::write_file(&script_path, &content)?;

    info!(path = %script_path.display(), "injected user-fixup script");

    register_active_setup(
        hives,
        "WinCrab.UserFixup",
        "WinCrab User Fixup",
        r"C:\wincrab\user-fixup.cmd",
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Consolidated NTUSER writes — Explorer\Advanced
// ---------------------------------------------------------------------------

/// Merge values into `HKCU\...\Explorer\Advanced` without destroying
/// Windows' default values.
fn apply_explorer_advanced_values(hives: &HivePaths, config: &Config) -> Result<(), Error> {
    let mut vals: Vec<(&str, &str)> = Vec::new();

    // From copilot
    if config.copilot.disable {
        vals.push(("ShowCopilotButton", "dword:00000000"));
    }

    // From visuals
    if config.visuals.optimize_for_performance {
        vals.push(("TaskbarAnimations", "dword:00000000"));
        vals.push(("ListviewAlphaSelect", "dword:00000000"));
    }

    // From taskbar
    if config.taskbar.hide_widgets_button {
        vals.push(("TaskbarDa", "dword:00000000"));
    }
    if config.taskbar.hide_chat_button {
        vals.push(("TaskbarMn", "dword:00000000"));
    }
    if config.taskbar.search_icon_only {
        vals.push(("SearchboxTaskbarMode", "dword:00000001"));
    }
    if config.taskbar.disable_start_recommendations {
        vals.push(("Start_IrisRecommendations", "dword:00000000"));
    }
    if config.taskbar.left_align {
        vals.push(("TaskbarAl", "dword:00000000"));
    }

    // From explorer
    if config.explorer.show_file_extensions {
        vals.push(("HideFileExt", "dword:00000000"));
    }
    if config.explorer.show_hidden_files {
        vals.push(("Hidden", "dword:00000001"));
    }
    if config.explorer.launch_to_this_pc {
        vals.push(("LaunchTo", "dword:00000001"));
    }
    if config.explorer.disable_snap_layouts {
        vals.push(("EnableSnapAssistFlyout", "dword:00000000"));
    }

    if vals.is_empty() {
        return Ok(());
    }

    merge_reg_values(
        &hives.ntuser,
        "\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\Advanced",
        &vals,
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Consolidated NTUSER writes — ContentDeliveryManager
// ---------------------------------------------------------------------------

/// Merge values into `HKCU\...\ContentDeliveryManager` without destroying
/// Windows' default values.
fn apply_content_delivery_manager_values(hives: &HivePaths, config: &Config) -> Result<(), Error> {
    let mut vals: Vec<(&str, &str)> = Vec::new();

    // From visuals — lock screen tips
    if config.visuals.disable_lock_screen_tips {
        vals.push(("RotatingLockScreenOverlayEnabled", "dword:00000000"));
        vals.push(("SubscribedContent-338387Enabled", "dword:00000000"));
    }

    // From visuals — suggestions
    if config.visuals.disable_suggestions {
        vals.push(("SystemPaneSuggestionsEnabled", "dword:00000000"));
        vals.push(("SubscribedContent-338388Enabled", "dword:00000000"));
        vals.push(("SubscribedContent-310093Enabled", "dword:00000000"));
    }

    // From taskbar — start recommendations
    if config.taskbar.disable_start_recommendations {
        vals.push(("SubscribedContent-338389Enabled", "dword:00000000"));
        vals.push(("SubscribedContent-353698Enabled", "dword:00000000"));
    }

    // From OOBE — finish setup nag
    if config.oobe.skip_finish_setup_nag
        && !vals
            .iter()
            .any(|(n, _)| *n == "SubscribedContent-310093Enabled")
    {
        vals.push(("SubscribedContent-310093Enabled", "dword:00000000"));
    }

    if vals.is_empty() {
        return Ok(());
    }

    merge_reg_values(
        &hives.ntuser,
        "\\Software\\Microsoft\\Windows\\CurrentVersion\\ContentDeliveryManager",
        &vals,
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Scheduled task disabling via registry
// ---------------------------------------------------------------------------

/// Remove scheduled task Tree entries from the SOFTWARE hive TaskCache.
fn disable_scheduled_tasks_via_registry(hives: &HivePaths, config: &Config) -> Result<(), Error> {
    let tasks = &config.scheduled_tasks;

    let mut task_paths: Vec<&str> = Vec::new();

    if tasks.remove_telemetry_tasks {
        task_paths.extend_from_slice(&[
            "Application Experience\\Microsoft Compatibility Appraiser",
            "Application Experience\\ProgramDataUpdater",
            "Application Experience\\AitAgent",
            "Application Experience\\StartupAppTask",
            "Application Experience\\PcaPatchDbTask",
            "Application Experience\\SdbinstMergeDbTask",
        ]);
    }

    if tasks.remove_ceip_tasks {
        task_paths.extend_from_slice(&[
            "Customer Experience Improvement Program\\Consolidator",
            "Customer Experience Improvement Program\\UsbCeip",
            "Customer Experience Improvement Program\\KernelCeipTask",
        ]);
    }

    if tasks.remove_disk_diagnostic_tasks {
        task_paths.extend_from_slice(&[
            "DiskDiagnostic\\Microsoft-Windows-DiskDiagnosticDataCollector",
            "DiskDiagnostic\\Microsoft-Windows-DiskDiagnosticResolver",
        ]);
    }

    if tasks.remove_maps_task {
        task_paths.extend_from_slice(&["Maps\\MapsUpdateTask", "Maps\\MapsToastTask"]);
    }

    if tasks.remove_feedback_tasks {
        task_paths.extend_from_slice(&[
            "Feedback\\Siuf\\DmClient",
            "Feedback\\Siuf\\DmClientOnScenarioDownload",
            "Windows Error Reporting\\QueueReporting",
        ]);
    }

    if task_paths.is_empty() {
        return Ok(());
    }

    info!(
        count = task_paths.len(),
        "disabling scheduled tasks via TaskCache registry"
    );

    let base =
        "\\Microsoft\\Windows NT\\CurrentVersion\\Schedule\\TaskCache\\Tree\\Microsoft\\Windows";

    // ponytail: TaskCache\Tasks\{GUID} entries remain; removing them requires reading each Tree key's Id.
    let sections: Vec<_> = task_paths
        .iter()
        .map(|path| RegSection::Delete {
            key: format!("{base}\\{path}"),
        })
        .collect();
    apply_reg(&hives.software, &sections)
}

// ---------------------------------------------------------------------------
// Services
// ---------------------------------------------------------------------------

fn disable_services(hives: &HivePaths, config: &Config) -> Result<(), Error> {
    let svc = &config.services;

    // Collect all services to disable. Start = 4 means "Disabled".
    let mut services_to_disable: Vec<&str> = Vec::new();

    if svc.disable_diagtrack {
        services_to_disable.push("DiagTrack");
    }
    if svc.disable_dmwappush {
        services_to_disable.push("dmwappushservice");
    }
    if svc.disable_wer {
        services_to_disable.push("WerSvc");
    }
    if svc.disable_xbox {
        services_to_disable.extend_from_slice(&[
            "XblAuthManager",
            "XblGameSave",
            "XboxNetApiSvc",
            "XboxGipSvc",
        ]);
    }
    if svc.disable_maps_broker {
        services_to_disable.push("MapsBroker");
    }
    if svc.disable_retail_demo {
        services_to_disable.push("RetailDemo");
    }
    if svc.disable_remote_registry {
        services_to_disable.push("RemoteRegistry");
    }
    if svc.disable_geolocation {
        services_to_disable.push("lfsvc");
    }
    if svc.disable_search {
        services_to_disable.push("WSearch");
    }
    if svc.disable_sysmain {
        services_to_disable.push("SysMain");
    }
    if svc.disable_ssdp {
        services_to_disable.push("SSDPSRV");
    }
    if svc.disable_upnp {
        services_to_disable.push("upnphost");
    }
    if svc.disable_fax {
        services_to_disable.push("Fax");
    }
    if svc.disable_print_spooler {
        services_to_disable.push("Spooler");
    }
    if svc.disable_wmp_sharing {
        services_to_disable.push("WMPNetworkSvc");
    }
    if svc.disable_widgets_service {
        services_to_disable.push("WpnService");
    }
    if svc.disable_telephony {
        services_to_disable.push("TapiSrv");
    }

    if services_to_disable.is_empty() {
        return Ok(());
    }

    info!(
        count = services_to_disable.len(),
        "disabling services via registry"
    );

    let sections: Vec<_> = services_to_disable
        .iter()
        .map(|name| RegSection::Set {
            key: format!("\\ControlSet001\\Services\\{name}"),
            values: &[("Start", "dword:00000004")],
        })
        .collect();
    apply_reg(&hives.system, &sections)
}

// ---------------------------------------------------------------------------
// Windows Defender
// ---------------------------------------------------------------------------

fn apply_defender_policies(hives: &HivePaths, config: &Config) -> Result<(), Error> {
    let defender = &config.defender;

    if !defender.disable_realtime
        && !defender.disable_smartscreen
        && !defender.disable_cloud_protection
        && !defender.disable_sample_submission
        && !defender.disable_services
    {
        return Ok(());
    }

    info!("applying Windows Defender registry policies");

    // DisableAntiSpyware — top-level Defender policy
    if defender.disable_realtime {
        merge_reg_values(
            &hives.software,
            "\\Policies\\Microsoft\\Windows Defender",
            &[("DisableAntiSpyware", "dword:00000001")],
        )?;

        // Real-Time Protection sub-policies
        merge_reg_values(
            &hives.software,
            "\\Policies\\Microsoft\\Windows Defender\\Real-Time Protection",
            &[
                ("DisableRealtimeMonitoring", "dword:00000001"),
                ("DisableBehaviorMonitoring", "dword:00000001"),
                ("DisableIOAVProtection", "dword:00000001"),
                ("DisableScriptScanning", "dword:00000001"),
            ],
        )?;
    }

    if defender.disable_cloud_protection || defender.disable_sample_submission {
        let mut vals: Vec<(&str, &str)> = Vec::new();
        if defender.disable_cloud_protection {
            vals.push(("SpynetReporting", "dword:00000000"));
        }
        if defender.disable_sample_submission {
            vals.push(("SubmitSamplesConsent", "dword:00000002"));
        }
        merge_reg_values(
            &hives.software,
            "\\Policies\\Microsoft\\Windows Defender\\Spynet",
            &vals,
        )?;
    }

    if defender.disable_smartscreen {
        // System-level SmartScreen policy
        // NOTE: \Policies\Microsoft\Windows\System may already have values
        // from privacy (activity history). Use merge to avoid clobbering.
        merge_reg_values(
            &hives.software,
            "\\Policies\\Microsoft\\Windows\\System",
            &[("EnableSmartScreen", "dword:00000000")],
        )?;

        // SmartScreen for Explorer — uses merge since Explorer key may have
        // other values from different config sections.
        merge_reg_values(
            &hives.software,
            "\\Microsoft\\Windows\\CurrentVersion\\Explorer",
            &[("SmartScreenEnabled", "string:Off")],
        )?;
    }

    // Disable Defender services in SYSTEM hive
    if defender.disable_services {
        // WdFilter (minifilter) and WdBoot (ELAM) enforce Defender's tamper
        // protection; left enabled, Defender restores its services and
        // policies during setup (seen in a real 25H2 VM install).
        for svc_name in &["WinDefend", "WdNisSvc", "WdNisDrv", "WdFilter", "WdBoot"] {
            merge_reg_values(
                &hives.system,
                &format!("\\ControlSet001\\Services\\{svc_name}"),
                &[("Start", "dword:00000004")],
            )?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Windows Update
// ---------------------------------------------------------------------------

fn apply_windows_update_policies(hives: &HivePaths, config: &Config) -> Result<(), Error> {
    let wu = &config.windows_update;

    if !wu.disable_auto_updates
        && !wu.exclude_driver_updates
        && !wu.disable_delivery_optimization
        && !wu.no_auto_reboot
    {
        return Ok(());
    }

    info!("applying Windows Update registry policies");

    // AU (Automatic Updates) sub-key
    if wu.disable_auto_updates || wu.no_auto_reboot {
        let mut vals: Vec<(&str, &str)> = Vec::new();
        if wu.disable_auto_updates {
            vals.push(("NoAutoUpdate", "dword:00000001"));
            vals.push(("AUOptions", "dword:00000002"));
        }
        if wu.no_auto_reboot {
            vals.push(("NoAutoRebootWithLoggedOnUsers", "dword:00000001"));
        }
        merge_reg_values(
            &hives.software,
            "\\Policies\\Microsoft\\Windows\\WindowsUpdate\\AU",
            &vals,
        )?;
    }

    // Exclude driver updates
    if wu.exclude_driver_updates {
        merge_reg_values(
            &hives.software,
            "\\Policies\\Microsoft\\Windows\\WindowsUpdate",
            &[("ExcludeWUDriversInQualityUpdate", "dword:00000001")],
        )?;
    }

    // Delivery Optimization
    if wu.disable_delivery_optimization {
        merge_reg_values(
            &hives.software,
            "\\Policies\\Microsoft\\Windows\\DeliveryOptimization",
            &[("DODownloadMode", "dword:00000000")],
        )?;
    }

    // Disable update services if auto-updates are fully disabled
    if wu.disable_auto_updates {
        for svc_name in &["wuauserv", "UsoSvc"] {
            merge_reg_values(
                &hives.system,
                &format!("\\ControlSet001\\Services\\{svc_name}"),
                &[("Start", "dword:00000004")],
            )?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Windows Recall / AI
// ---------------------------------------------------------------------------

fn apply_recall_policies(hives: &HivePaths) -> Result<(), Error> {
    info!("disabling Windows Recall / AI data analysis");

    merge_reg_values(
        &hives.software,
        "\\Policies\\Microsoft\\Windows\\WindowsAI",
        &[
            ("DisableAIDataAnalysis", "dword:00000001"),
            ("TurnOffSavingSnapshots", "dword:00000001"),
        ],
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Performance tuning
// ---------------------------------------------------------------------------

fn apply_performance_registry(hives: &HivePaths, config: &Config) -> Result<(), Error> {
    let perf = &config.performance;

    if !perf.ntfs_disable_last_access
        && !perf.ntfs_disable_8dot3
        && !perf.faster_shutdown
        && !perf.disable_game_dvr
        && !perf.disable_background_apps
        && !perf.network_tuning
    {
        return Ok(());
    }

    info!("applying performance registry tweaks");

    // NTFS tuning (SYSTEM hive)
    if perf.ntfs_disable_last_access || perf.ntfs_disable_8dot3 {
        let mut vals: Vec<(&str, &str)> = Vec::new();
        if perf.ntfs_disable_last_access {
            vals.push(("NtfsDisableLastAccessUpdate", "dword:00000001"));
        }
        if perf.ntfs_disable_8dot3 {
            vals.push(("NtfsDisable8dot3NameCreation", "dword:00000001"));
        }
        merge_reg_values(&hives.system, "\\ControlSet001\\Control\\FileSystem", &vals)?;
    }

    // Faster shutdown — NTUSER values
    if perf.faster_shutdown && hives.ntuser.exists() {
        merge_reg_values(
            &hives.ntuser,
            "\\Control Panel\\Desktop",
            &[
                ("WaitToKillAppTimeout", "string:2000"),
                ("HungAppTimeout", "string:1000"),
            ],
        )?;
    }

    // Faster shutdown — SYSTEM service timeout
    if perf.faster_shutdown {
        merge_reg_values(
            &hives.system,
            "\\ControlSet001\\Control",
            &[("WaitToKillServiceTimeout", "string:2000")],
        )?;
    }

    // Game DVR (SOFTWARE policy)
    if perf.disable_game_dvr {
        merge_reg_values(
            &hives.software,
            "\\Policies\\Microsoft\\Windows\\GameDVR",
            &[("AllowGameDVR", "dword:00000000")],
        )?;
    }

    // Background apps (NTUSER)
    if perf.disable_background_apps && hives.ntuser.exists() {
        merge_reg_values(
            &hives.ntuser,
            "\\Software\\Microsoft\\Windows\\CurrentVersion\\BackgroundAccessApplications",
            &[("GlobalUserDisabled", "dword:00000001")],
        )?;
    }

    // Network tuning (SYSTEM)
    if perf.network_tuning {
        merge_reg_values(
            &hives.system,
            "\\ControlSet001\\Services\\Tcpip\\Parameters",
            &[("TcpNoDelay", "dword:00000001")],
        )?;
        merge_reg_values(
            &hives.system,
            "\\ControlSet001\\Services\\LanmanWorkstation\\Parameters",
            &[("DisableBandwidthThrottling", "dword:00000001")],
        )?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Security hardening
// ---------------------------------------------------------------------------

fn apply_security_policies(hives: &HivePaths, config: &Config) -> Result<(), Error> {
    let sec = &config.security;

    if !sec.disable_remote_desktop && !sec.disable_smb1 && !sec.asr_rules {
        return Ok(());
    }

    info!("applying security hardening registry policies");

    // Disable Remote Desktop (SYSTEM)
    if sec.disable_remote_desktop {
        merge_reg_values(
            &hives.system,
            "\\ControlSet001\\Control\\Terminal Server",
            &[("fDenyTSConnections", "dword:00000001")],
        )?;
    }

    // Disable SMBv1 (SYSTEM)
    if sec.disable_smb1 {
        merge_reg_values(
            &hives.system,
            "\\ControlSet001\\Services\\mrxsmb10",
            &[("Start", "dword:00000004")],
        )?;
    }

    // ASR rules (SOFTWARE)
    if sec.asr_rules {
        merge_reg_values(
            &hives.software,
            "\\Policies\\Microsoft\\Windows Defender\\Windows Defender Exploit Guard\\ASR",
            &[("ExploitGuard_ASR_Rules", "dword:00000001")],
        )?;

        // Individual ASR rule GUIDs — each set to "1" (Block)
        merge_reg_values(
            &hives.software,
            "\\Policies\\Microsoft\\Windows Defender\\Windows Defender Exploit Guard\\ASR\\Rules",
            &[
                ("be9ba2d9-53ea-4cdc-84e5-9b1eeee46550", "string:1"), // block exe from email
                ("d4f940ab-401b-4efc-aadc-ad5f3c50688a", "string:1"), // block Office child processes
                ("3b576869-a4ec-4529-8536-b80a7769e899", "string:1"), // block Office creating executables
                ("75668c1f-73b5-4cf0-bb93-3ecf5cb7cc84", "string:1"), // block Office injecting into processes
                ("d3e037e1-3eb8-44c8-a917-57927947596d", "string:1"), // block JS/VBS downloads
                ("5beb7efe-fd9a-4556-801d-275e5ffc04cc", "string:1"), // block obfuscated scripts
            ],
        )?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Classic context menu
// ---------------------------------------------------------------------------

/// Restore the classic (full) right-click context menu by registering an
/// empty InprocServer32 for the Windows 11 truncated menu CLSID.
fn apply_classic_context_menu(hives: &HivePaths) -> Result<(), Error> {
    info!("restoring classic context menu");

    // Write the (Default) value as an empty string — name="" for the default value.
    merge_reg_values(
        &hives.ntuser,
        "\\Software\\Classes\\CLSID\\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\\InprocServer32",
        &[("", "string:")],
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Dark mode
// ---------------------------------------------------------------------------

fn apply_dark_mode(hives: &HivePaths) -> Result<(), Error> {
    info!("enabling system-wide dark mode");

    merge_reg_values(
        &hives.ntuser,
        "\\Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
        &[
            ("AppsUseLightTheme", "dword:00000000"),
            ("SystemUsesLightTheme", "dword:00000000"),
        ],
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Telemetry extensions (NTUSER)
// ---------------------------------------------------------------------------

fn apply_telemetry_extensions(hives: &HivePaths, config: &Config) -> Result<(), Error> {
    let tel = &config.telemetry;

    // Clipboard sync
    if tel.disable_clipboard_sync {
        merge_reg_values(
            &hives.ntuser,
            "\\Software\\Microsoft\\Clipboard",
            &[("EnableClipboardHistory", "dword:00000000")],
        )?;
    }

    // Input personalization
    if tel.disable_input_personalization {
        merge_reg_values(
            &hives.ntuser,
            "\\Software\\Microsoft\\InputPersonalization",
            &[
                ("RestrictImplicitInkCollection", "dword:00000001"),
                ("RestrictImplicitTextCollection", "dword:00000001"),
            ],
        )?;
    }

    // Feedback frequency
    if tel.set_feedback_never {
        merge_reg_values(
            &hives.ntuser,
            "\\Software\\Microsoft\\Siuf\\Rules",
            &[("NumberOfSIUFInPeriod", "dword:00000000")],
        )?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Privacy extensions (NTUSER + SOFTWARE)
// ---------------------------------------------------------------------------

fn apply_privacy_extensions(hives: &HivePaths, config: &Config) -> Result<(), Error> {
    let privacy = &config.privacy;

    // Suggested actions (NTUSER)
    if privacy.disable_suggested_actions && hives.ntuser.exists() {
        merge_reg_values(
            &hives.ntuser,
            "\\Software\\Microsoft\\Windows\\CurrentVersion\\SmartActionPlatform\\SmartClipboard",
            &[("Disabled", "dword:00000001")],
        )?;
    }

    // Spotlight / cloud content (SOFTWARE)
    if privacy.disable_spotlight_ads {
        merge_reg_values(
            &hives.software,
            "\\Policies\\Microsoft\\Windows\\CloudContent",
            &[
                ("DisableWindowsConsumerFeatures", "dword:00000001"),
                ("DisableCloudOptimizedContent", "dword:00000001"),
                ("DisableSoftLanding", "dword:00000001"),
            ],
        )?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Notification center disable
// ---------------------------------------------------------------------------

fn apply_notification_center_disable(hives: &HivePaths) -> Result<(), Error> {
    info!("disabling notification center");

    // NOTE: \Policies\Microsoft\Windows\Explorer may already have
    // DisableSearchBoxSuggestions from privacy. Use merge to avoid clobbering.
    merge_reg_values(
        &hives.software,
        "\\Policies\\Microsoft\\Windows\\Explorer",
        &[("DisableNotificationCenter", "dword:00000001")],
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Recent files disable
// ---------------------------------------------------------------------------

fn apply_recent_files_disable(hives: &HivePaths) -> Result<(), Error> {
    info!("disabling recent files in Quick Access");

    // Uses merge since Explorer key has many other default values.
    merge_reg_values(
        &hives.ntuser,
        "\\Software\\Microsoft\\Windows\\CurrentVersion\\Explorer",
        &[
            ("ShowRecent", "dword:00000000"),
            ("ShowFrequent", "dword:00000000"),
        ],
    )?;

    Ok(())
}

// ---------------------------------------------------------------------------
// OOBE nag suppression
// ---------------------------------------------------------------------------

fn suppress_finish_setup_nag(hives: &HivePaths) -> Result<(), Error> {
    info!("suppressing 'Let's finish setting up' nag");

    if hives.ntuser.exists() {
        merge_reg_values(
            &hives.ntuser,
            "\\Software\\Microsoft\\Windows\\CurrentVersion\\UserProfileEngagement",
            &[("ScoobeSystemSettingEnabled", "dword:00000000")],
        )?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Active Setup first-boot registration
// ---------------------------------------------------------------------------

/// Register an Active Setup entry in the SOFTWARE hive (HKLM) so that
/// `command` runs once per user on first login.
///
/// Active Setup is more reliable than RunOnce in Default\NTUSER.DAT because
/// it lives in the machine hive and Windows checks it on every user logon,
/// running any entries the user hasn't seen yet.
fn register_active_setup(
    hives: &HivePaths,
    component_id: &str,
    display_name: &str,
    command: &str,
) -> Result<(), Error> {
    info!("registering {display_name} first-boot script via Active Setup");

    let display_val = format!("string:{display_name}");
    let cmd_val = format!("string:{command}");
    merge_reg_values(
        &hives.software,
        &format!("\\Microsoft\\Active Setup\\Installed Components\\{component_id}"),
        &[
            ("", &display_val),            // (Default) = display name
            ("StubPath", &cmd_val),        // command to run
            ("Version", "string:1,0,0,0"), // version stamp
        ],
    )?;

    Ok(())
}

// All offline registry writes use merge semantics.
enum RegSection<'a> {
    Set {
        key: String,
        values: &'a [(&'a str, &'a str)],
    },
    Delete {
        key: String,
    },
}

fn render_reg(sections: &[RegSection<'_>], root_prefix: &str) -> Result<String, Error> {
    use std::collections::HashSet;
    use std::fmt::Write as _;
    let mut rendered = String::new();
    let mut seen = HashSet::new();
    for section in sections {
        let key = match section {
            RegSection::Set { key, .. } | RegSection::Delete { key } => key,
        };
        validate_registry_component(key)?;
        if !key.starts_with('\\')
            || key.contains(']')
            || key.len() == 1
            || key.contains("\\\\")
            || key.ends_with('\\')
        {
            return Err(Error::Config {
                message: format!("invalid registry key: {key:?}"),
            });
        }
        match section {
            RegSection::Delete { .. } => {
                let _ = writeln!(rendered, "[-{root_prefix}{key}]\n");
                seen.retain(|path: &String| {
                    path != key
                        && !path
                            .strip_prefix(key)
                            .is_some_and(|rest| rest.starts_with('\\'))
                });
            }
            RegSection::Set { values, .. } => {
                let mut path = String::new();
                for part in key.trim_start_matches('\\').split('\\') {
                    path.push('\\');
                    path.push_str(part);
                    if path != *key && seen.insert(path.clone()) {
                        let _ = writeln!(rendered, "[{root_prefix}{path}]\n");
                    }
                }
                seen.insert(key.clone());
                let _ = writeln!(rendered, "[{root_prefix}{key}]");
                for (name, value) in *values {
                    validate_registry_component(name)?;
                    validate_registry_component(value)?;
                    let name = if name.is_empty() {
                        "@".to_string()
                    } else {
                        format!("\"{}\"", name.replace('\\', "\\\\").replace('"', "\\\""))
                    };
                    let value =
                        if let Some(text) = value.strip_prefix("string:") {
                            format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
                        } else if value.strip_prefix("dword:").is_some_and(|v| {
                            v.len() == 8 && v.chars().all(|c| c.is_ascii_hexdigit())
                        }) || value.strip_prefix("hex:").is_some_and(|v| {
                            v.split(',').all(|byte| {
                                byte.len() == 2 && byte.chars().all(|c| c.is_ascii_hexdigit())
                            })
                        }) {
                            value.to_string()
                        } else {
                            return Err(Error::Config {
                                message: format!("unsupported registry value: {value:?}"),
                            });
                        };
                    let _ = writeln!(rendered, "{name}={value}");
                }
                rendered.push('\n');
            }
        }
    }
    Ok(rendered)
}

fn validate_registry_component(component: &str) -> Result<(), Error> {
    if component.chars().any(|c| c.is_control()) {
        return Err(Error::Config {
            message: format!("registry component contains control characters: {component:?}"),
        });
    }
    Ok(())
}

fn merge_reg_values(hive: &Path, key: &str, values: &[(&str, &str)]) -> Result<(), Error> {
    apply_reg(
        hive,
        &[RegSection::Set {
            key: key.to_owned(),
            values,
        }],
    )
}

#[cfg(not(windows))]
fn apply_reg(hive: &Path, sections: &[RegSection<'_>]) -> Result<(), Error> {
    let text = render_reg(sections, "")?;
    let output = spawn_with_stdin(
        Command::new("hivexregedit").arg("--merge").arg(hive),
        text.as_bytes(),
    )?;
    if !output.status.success() {
        return Err(Error::Command {
            command: "hivexregedit".into(),
            code: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(())
}

#[cfg(windows)]
fn apply_reg(hive: &Path, sections: &[RegSection<'_>]) -> Result<(), Error> {
    use std::io::Write as _;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let unique = format!(
        "{}_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let mount = format!("HKLM\\WINCRAB_{unique}");
    let root = format!("HKEY_LOCAL_MACHINE\\WINCRAB_{unique}");
    let text = format!(
        "Windows Registry Editor Version 5.00\n\n{}",
        render_reg(sections, &root)?
    )
    .replace('\n', "\r\n");
    let mut bytes = vec![0xff, 0xfe];
    bytes.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
    let file = std::env::temp_dir().join(format!("wincrab-{unique}.reg"));
    let mut temp = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&file)
        .map_err(|source| Error::Io {
            context: "writing registry import".into(),
            source,
        })?;
    let write = temp.write_all(&bytes).map_err(|source| Error::Io {
        context: "writing registry import".into(),
        source,
    });
    drop(temp);
    let run = |command: &mut Command| -> Result<(), Error> {
        let output = command.output().map_err(|source| Error::Io {
            context: "running reg.exe".into(),
            source,
        })?;
        if !output.status.success() {
            return Err(Error::Command {
                command: format!("{command:?}"),
                code: output.status.code().unwrap_or(-1),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }
        Ok(())
    };
    let result = write.and_then(|()| {
        run(Command::new("reg").arg("load").arg(&mount).arg(hive))?;
        let result = run(Command::new("reg").arg("import").arg(&file));
        let unload = run(Command::new("reg").arg("unload").arg(&mount));
        result.and(unload)
    });
    let cleanup = std::fs::remove_file(&file).map_err(|source| Error::Io {
        context: "removing registry import".into(),
        source,
    });
    result.and(cleanup)
}

#[cfg(not(windows))]
/// Spawn a command with piped stdin, write `input` to it, and return the output.
///
/// Handles `NotFound` → [`Error::ToolNotFound`] conversion automatically.
fn spawn_with_stdin(cmd: &mut Command, input: &[u8]) -> Result<std::process::Output, Error> {
    let program = format!("{:?}", cmd.get_program());
    let mut child = match cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            return Err(if e.kind() == std::io::ErrorKind::NotFound {
                Error::ToolNotFound { tool: program }
            } else {
                Error::Io {
                    context: format!("spawning {program}"),
                    source: e,
                }
            });
        }
    };

    child
        .stdin
        .as_mut()
        .ok_or_else(|| Error::Io {
            context: format!("stdin not available for {program}"),
            source: std::io::Error::other("stdin was piped but is None"),
        })?
        .write_all(input)
        .map_err(|e| Error::Io {
            context: format!("writing to {program} stdin"),
            source: e,
        })?;

    child.wait_with_output().map_err(|e| Error::Io {
        context: format!("waiting for {program}"),
        source: e,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_registry_sections() {
        let sections = [
            RegSection::Set {
                key: "\\A\\B".into(),
                values: &[("", "string:C:\\x \"y\"")],
            },
            RegSection::Set {
                key: "\\A\\C".into(),
                values: &[("n", "dword:00000001")],
            },
            RegSection::Delete {
                key: "\\A\\D".into(),
            },
        ];
        let text = render_reg(&sections, "").unwrap();
        assert_eq!(
            text,
            "[\\A]\n\n[\\A\\B]\n@=\"C:\\\\x \\\"y\\\"\"\n\n[\\A\\C]\n\"n\"=dword:00000001\n\n[-\\A\\D]\n\n"
        );
        let windows = render_reg(&sections, "HKEY_LOCAL_MACHINE\\WINCRAB_test").unwrap();
        assert!(windows.contains("[HKEY_LOCAL_MACHINE\\WINCRAB_test\\A\\B]"));
        assert!(windows.contains("[-HKEY_LOCAL_MACHINE\\WINCRAB_test\\A\\D]"));
    }

    #[test]
    fn render_rejects_invalid_components() {
        for (key, name, value) in [
            ("\\A\nB", "n", "string:x"),
            ("\\A", "n\t", "string:x"),
            ("\\A", "n", "string:\0"),
            ("\\A", "n", "unknown:x"),
        ] {
            assert!(matches!(
                render_reg(
                    &[RegSection::Set {
                        key: key.into(),
                        values: &[(name, value)]
                    }],
                    ""
                ),
                Err(Error::Config { .. })
            ));
        }
    }

    // -----------------------------------------------------------------------
    // HivePaths
    // -----------------------------------------------------------------------

    #[test]
    fn hive_paths_from_mount() {
        let mount = std::path::Path::new("/mnt/wim");
        let hives = HivePaths::from_mount(mount);
        assert_eq!(
            hives.software,
            std::path::PathBuf::from("/mnt/wim/Windows/System32/config/SOFTWARE")
        );
        assert_eq!(
            hives.system,
            std::path::PathBuf::from("/mnt/wim/Windows/System32/config/SYSTEM")
        );
        assert_eq!(
            hives.default,
            std::path::PathBuf::from("/mnt/wim/Windows/System32/config/DEFAULT")
        );
        assert_eq!(
            hives.ntuser,
            std::path::PathBuf::from("/mnt/wim/Users/Default/NTUSER.DAT")
        );
    }

    #[test]
    fn hive_validate_missing_software_errors() {
        let dir = tempfile::tempdir().unwrap();
        let hives = HivePaths::from_mount(dir.path());
        let result = hives.validate();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::HiveNotFound { .. }));
    }

    #[test]
    fn hive_validate_all_present_ok() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("Windows").join("System32").join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("SOFTWARE"), b"hive").unwrap();
        std::fs::write(config_dir.join("SYSTEM"), b"hive").unwrap();
        std::fs::write(config_dir.join("DEFAULT"), b"hive").unwrap();
        // NTUSER.DAT is optional — just a warning if missing.

        let hives = HivePaths::from_mount(dir.path());
        hives.validate().unwrap();
    }

    #[test]
    fn hive_validate_ntuser_missing_is_just_warning() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("Windows").join("System32").join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("SOFTWARE"), b"hive").unwrap();
        std::fs::write(config_dir.join("SYSTEM"), b"hive").unwrap();
        std::fs::write(config_dir.join("DEFAULT"), b"hive").unwrap();
        // NTUSER.DAT intentionally missing.

        let hives = HivePaths::from_mount(dir.path());
        // Should succeed — NTUSER is optional.
        hives.validate().unwrap();
    }

    #[test]
    fn hive_validate_missing_system_errors() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("Windows").join("System32").join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("SOFTWARE"), b"hive").unwrap();
        // SYSTEM missing, DEFAULT missing.

        let hives = HivePaths::from_mount(dir.path());
        let result = hives.validate();
        assert!(result.is_err());
    }

    #[test]
    fn hive_validate_missing_default_errors() {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("Windows").join("System32").join("config");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::write(config_dir.join("SOFTWARE"), b"hive").unwrap();
        std::fs::write(config_dir.join("SYSTEM"), b"hive").unwrap();
        // DEFAULT missing.

        let hives = HivePaths::from_mount(dir.path());
        let result = hives.validate();
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // validate_registry_component
    // -----------------------------------------------------------------------

    #[test]
    fn validate_registry_component_normal_path_ok() {
        assert!(validate_registry_component("\\ControlSet001\\Services\\DiagTrack").is_ok());
    }

    #[test]
    fn validate_registry_component_rejects_newline() {
        let result = validate_registry_component("key\nmalicious command");
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), Error::Config { .. }));
    }

    #[test]
    fn validate_registry_component_rejects_tab() {
        assert!(validate_registry_component("key\tname").is_err());
    }

    #[test]
    fn validate_registry_component_rejects_null() {
        assert!(validate_registry_component("key\0name").is_err());
    }

    #[test]
    fn validate_registry_component_allows_spaces_and_special() {
        assert!(
            validate_registry_component(
                "Application Experience\\Microsoft Compatibility Appraiser"
            )
            .is_ok()
        );
        assert!(validate_registry_component("dword:00000001").is_ok());
    }
}

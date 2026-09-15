use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::Error;

/// Type-safe wrapper for WIM image indices, preventing accidental misuse of
/// arbitrary `u32` values. Zero-cost: identical to `u32` at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WimIndex(pub u32);

impl WimIndex {
    /// Create a new WIM index.
    pub const fn new(index: u32) -> Self {
        Self(index)
    }

    /// Return the raw `u32` value.
    pub const fn get(self) -> u32 {
        self.0
    }
}

impl std::fmt::Display for WimIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl PartialEq<u32> for WimIndex {
    fn eq(&self, other: &u32) -> bool {
        self.0 == *other
    }
}

impl PartialOrd<u32> for WimIndex {
    fn partial_cmp(&self, other: &u32) -> Option<std::cmp::Ordering> {
        self.0.partial_cmp(other)
    }
}

/// Top-level configuration for a wincrab debloat run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Which WIM image index to modify (e.g. 6 = Windows 11 Pro).
    pub wim_index: WimIndex,

    /// Named profile to use as base config (minimal, gaming, privacy, enterprise, vm, vm-seelen).
    /// Config file values override the profile defaults.
    pub profile: Option<String>,

    pub apps: AppRemoval,
    pub telemetry: Telemetry,
    pub privacy: Privacy,
    pub copilot: Copilot,
    pub edge: Edge,
    pub visuals: Visuals,
    pub taskbar: Taskbar,
    pub services: Services,
    pub scheduled_tasks: ScheduledTasks,
    pub oobe: Oobe,
    pub seelen: Seelen,
    pub drivers: Drivers,
    pub defender: Defender,
    pub windows_update: WindowsUpdate,
    pub explorer: Explorer,
    pub performance: Performance,
    pub security: Security,
    pub recall: Recall,
    pub inject: Inject,
    pub hooks: Hooks,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            wim_index: WimIndex(6),
            profile: None,
            apps: AppRemoval::default(),
            telemetry: Telemetry::default(),
            privacy: Privacy::default(),
            copilot: Copilot::default(),
            edge: Edge::default(),
            visuals: Visuals::default(),
            taskbar: Taskbar::default(),
            services: Services::default(),
            scheduled_tasks: ScheduledTasks::default(),
            oobe: Oobe::default(),
            seelen: Seelen::default(),
            drivers: Drivers::default(),
            defender: Defender::default(),
            windows_update: WindowsUpdate::default(),
            explorer: Explorer::default(),
            performance: Performance::default(),
            security: Security::default(),
            recall: Recall::default(),
            inject: Inject::default(),
            hooks: Hooks::default(),
        }
    }
}

impl Config {
    pub fn from_file(path: &Path) -> Result<Self, Error> {
        let content = std::fs::read_to_string(path).map_err(|e| Error::Io {
            context: format!("reading config file {}", path.display()),
            source: e,
        })?;
        let config: Self = toml::from_str(&content).map_err(|e| Error::Config {
            message: format!("parsing {}: {e}", path.display()),
        })?;
        config.validate()?;
        Ok(config)
    }

    /// Validate cross-cutting config constraints.
    pub fn validate(&self) -> Result<(), Error> {
        // Seelen-UI depends on Edge/WebView2.
        if self.seelen.bundle {
            let edge_patterns = ["Microsoft.MicrosoftEdge", "MicrosoftEdge", "WebView2"];
            for pat in &self.apps.extra_patterns {
                if edge_patterns.iter().any(|ep| pat.contains(ep)) {
                    return Err(Error::Config {
                        message: format!(
                            "extra_patterns contains '{pat}' which would remove Edge, \
                             but Seelen-UI requires Edge/WebView2. Either disable \
                             [seelen].bundle or remove this pattern."
                        ),
                    });
                }
            }

            // Defender SmartScreen is separate from Edge functionality -- allowed.
            // But warn if trying to disable Defender while Seelen expects WebView2.
        }

        // OOBE consistency: auto_logon requires a local account name.
        if self.oobe.auto_logon && self.oobe.local_account_name.is_none() {
            return Err(Error::Config {
                message: "auto_logon is enabled but local_account_name is not set — \
                          Windows cannot auto-logon without a known account"
                    .into(),
            });
        }

        // Validate inject file sources exist.
        for entry in &self.inject.files {
            if !entry.src.exists() {
                return Err(Error::Config {
                    message: format!("inject source path does not exist: {}", entry.src.display(),),
                });
            }
        }

        // Edition conversion validation.
        if let Some(ref edition) = self.oobe.convert_edition {
            let valid = ["Professional", "Enterprise", "Education", "Core"];
            if !valid.contains(&edition.as_str()) {
                return Err(Error::Config {
                    message: format!(
                        "convert_edition '{edition}' is not valid. Use one of: {valid:?}"
                    ),
                });
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// App removal
// ---------------------------------------------------------------------------

/// Controls which provisioned Appx packages to strip from the image.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppRemoval {
    /// Remove all known bloatware (TikTok, Candy Crush, Clipchamp, etc.).
    pub remove_bloatware: bool,
    /// Remove Xbox-related apps and services.
    pub remove_xbox: bool,
    /// Remove Microsoft Teams consumer chat.
    pub remove_teams: bool,
    /// Remove OneDrive pre-install.
    pub remove_onedrive: bool,
    /// Remove Cortana.
    pub remove_cortana: bool,
    /// Remove the Microsoft Store.
    pub remove_store: bool,
    /// Remove Outlook (new) for Windows.
    pub remove_outlook: bool,
    /// Remove Mail & Calendar.
    pub remove_mail: bool,
    /// Remove Dev Home.
    pub remove_dev_home: bool,
    /// Remove Phone Link / Cross Device backend.
    pub remove_phone_link: bool,
    /// Additional package name patterns to remove (substring match against
    /// directory names under `Program Files/WindowsApps`).
    pub extra_patterns: Vec<String>,
}

impl Default for AppRemoval {
    fn default() -> Self {
        Self {
            remove_bloatware: true,
            remove_xbox: true,
            remove_teams: true,
            remove_onedrive: true,
            remove_cortana: true,
            remove_store: false,
            remove_outlook: true,
            remove_mail: true,
            remove_dev_home: true,
            remove_phone_link: true,
            extra_patterns: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Telemetry
// ---------------------------------------------------------------------------

/// Controls Windows telemetry and data-collection services.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Telemetry {
    /// Disable telemetry via registry (AllowTelemetry = 0).
    pub disable: bool,
    /// Disable Customer Experience Improvement Program.
    pub disable_ceip: bool,
    /// Disable application telemetry.
    pub disable_app_telemetry: bool,
    /// Block known Microsoft telemetry domains via the hosts file.
    pub block_telemetry_hosts: bool,
    /// Additional domains to block in the hosts file.
    pub extra_blocked_hosts: Vec<String>,
    /// Disable clipboard cloud sync.
    pub disable_clipboard_sync: bool,
    /// Disable Find My Device.
    pub disable_find_my_device: bool,
    /// Disable input personalization / inking telemetry.
    pub disable_input_personalization: bool,
    /// Disable Wi-Fi Sense / hotspot sharing.
    pub disable_wifi_sense: bool,
    /// Set feedback frequency to "Never".
    pub set_feedback_never: bool,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            disable: true,
            disable_ceip: true,
            disable_app_telemetry: true,
            block_telemetry_hosts: true,
            extra_blocked_hosts: Vec::new(),
            disable_clipboard_sync: true,
            disable_find_my_device: true,
            disable_input_personalization: true,
            disable_wifi_sense: true,
            set_feedback_never: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Privacy
// ---------------------------------------------------------------------------

/// Controls privacy-related registry settings beyond telemetry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Privacy {
    /// Disable the advertising ID.
    pub disable_advertising_id: bool,
    /// Disable web search / Bing suggestions in the Start menu.
    pub disable_web_search: bool,
    /// Disable activity history and timeline.
    pub disable_activity_history: bool,
    /// Disable tailored experiences based on diagnostic data.
    pub disable_tailored_experiences: bool,
    /// Disable Windows Error Reporting.
    pub disable_error_reporting: bool,
    /// Set default app permission access to deny (camera, mic, location, etc.).
    pub restrict_app_permissions: bool,
    /// Disable suggested actions on copy.
    pub disable_suggested_actions: bool,
    /// Disable Windows Spotlight ads and cloud-delivered content.
    pub disable_spotlight_ads: bool,
}

impl Default for Privacy {
    fn default() -> Self {
        Self {
            disable_advertising_id: true,
            disable_web_search: true,
            disable_activity_history: true,
            disable_tailored_experiences: true,
            disable_error_reporting: true,
            restrict_app_permissions: true,
            disable_suggested_actions: true,
            disable_spotlight_ads: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Copilot
// ---------------------------------------------------------------------------

/// Controls Windows Copilot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Copilot {
    /// Disable Copilot entirely.
    pub disable: bool,
}

impl Default for Copilot {
    fn default() -> Self {
        Self { disable: true }
    }
}

// ---------------------------------------------------------------------------
// Edge
// ---------------------------------------------------------------------------

/// Controls Microsoft Edge first-run behavior and nagging.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Edge {
    /// Disable Edge first-run experience / welcome page.
    pub disable_first_run: bool,
    /// Disable default browser nagging prompts.
    pub disable_default_browser_nag: bool,
    /// Disable Edge sidebar (Copilot, Discover, etc.).
    pub disable_sidebar: bool,
    /// Disable Edge desktop search bar widget.
    pub disable_search_bar: bool,
}

impl Default for Edge {
    fn default() -> Self {
        Self {
            disable_first_run: true,
            disable_default_browser_nag: true,
            disable_sidebar: true,
            disable_search_bar: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Visuals
// ---------------------------------------------------------------------------

/// Controls desktop visual effects and animations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Visuals {
    /// Optimize visuals for performance (disable animations, transparency, etc.).
    pub optimize_for_performance: bool,
    /// Disable lock screen spotlight / tips.
    pub disable_lock_screen_tips: bool,
    /// Disable "Get tips and suggestions" notifications.
    pub disable_suggestions: bool,
    /// Enable system-wide dark mode.
    pub dark_mode: bool,
}

impl Default for Visuals {
    fn default() -> Self {
        Self {
            optimize_for_performance: true,
            disable_lock_screen_tips: true,
            disable_suggestions: true,
            dark_mode: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Taskbar
// ---------------------------------------------------------------------------

/// Controls taskbar and Start menu layout.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Taskbar {
    /// Hide the Widgets button on the taskbar.
    pub hide_widgets_button: bool,
    /// Hide the Chat (Teams) button on the taskbar.
    pub hide_chat_button: bool,
    /// Use search icon instead of full search bar.
    pub search_icon_only: bool,
    /// Disable Start menu recommendations section.
    pub disable_start_recommendations: bool,
    /// Left-align the taskbar (instead of center).
    pub left_align: bool,
    /// Disable the notification center / action center.
    pub disable_notification_center: bool,
}

impl Default for Taskbar {
    fn default() -> Self {
        Self {
            hide_widgets_button: true,
            hide_chat_button: true,
            search_icon_only: true,
            disable_start_recommendations: true,
            left_align: true,
            disable_notification_center: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Services
// ---------------------------------------------------------------------------

/// Controls unnecessary Windows services.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Services {
    /// Disable DiagTrack (Connected User Experiences and Telemetry).
    pub disable_diagtrack: bool,
    /// Disable dmwappushservice (WAP Push Message Routing).
    pub disable_dmwappush: bool,
    /// Disable Windows Error Reporting service.
    pub disable_wer: bool,
    /// Disable Xbox services (XblAuthManager, XblGameSave, XboxNetApiSvc, XboxGipSvc).
    pub disable_xbox: bool,
    /// Disable Downloaded Maps Manager.
    pub disable_maps_broker: bool,
    /// Disable Retail Demo service.
    pub disable_retail_demo: bool,
    /// Disable Remote Registry.
    pub disable_remote_registry: bool,
    /// Disable Geolocation service.
    pub disable_geolocation: bool,
    /// Disable Windows Search indexer (WSearch).
    pub disable_search: bool,
    /// Disable SysMain / Superfetch (recommended for SSD-only systems).
    pub disable_sysmain: bool,
    /// Disable SSDP Discovery (UPnP).
    pub disable_ssdp: bool,
    /// Disable UPnP Device Host.
    pub disable_upnp: bool,
    /// Disable Fax service.
    pub disable_fax: bool,
    /// Disable Print Spooler.
    pub disable_print_spooler: bool,
    /// Disable Windows Media Player network sharing.
    pub disable_wmp_sharing: bool,
    /// Disable Widgets service entirely (not just hide the button).
    pub disable_widgets_service: bool,
    /// Disable Telephony API (TapiSrv).
    pub disable_telephony: bool,
}

impl Default for Services {
    fn default() -> Self {
        Self {
            disable_diagtrack: true,
            disable_dmwappush: true,
            disable_wer: true,
            disable_xbox: true,
            disable_maps_broker: true,
            disable_retail_demo: true,
            disable_remote_registry: true,
            disable_geolocation: true,
            disable_search: false,
            disable_sysmain: false,
            disable_ssdp: true,
            disable_upnp: true,
            disable_fax: true,
            disable_print_spooler: false,
            disable_wmp_sharing: true,
            disable_widgets_service: true,
            disable_telephony: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Scheduled tasks
// ---------------------------------------------------------------------------

/// Controls removal of scheduled tasks (telemetry, maintenance).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScheduledTasks {
    /// Remove telemetry-related scheduled tasks.
    pub remove_telemetry_tasks: bool,
    /// Remove CEIP scheduled tasks.
    pub remove_ceip_tasks: bool,
    /// Remove disk diagnostic tasks.
    pub remove_disk_diagnostic_tasks: bool,
    /// Remove maps update task.
    pub remove_maps_task: bool,
    /// Remove feedback scheduled tasks.
    pub remove_feedback_tasks: bool,
}

impl Default for ScheduledTasks {
    fn default() -> Self {
        Self {
            remove_telemetry_tasks: true,
            remove_ceip_tasks: true,
            remove_disk_diagnostic_tasks: true,
            remove_maps_task: true,
            remove_feedback_tasks: true,
        }
    }
}

// ---------------------------------------------------------------------------
// OOBE
// ---------------------------------------------------------------------------

/// Controls OOBE (Out-of-Box Experience) automation via autounattend.xml.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Oobe {
    /// Inject autounattend.xml into the ISO root.
    pub inject_autounattend: bool,
    /// Skip the Microsoft account requirement (allow local account).
    pub skip_microsoft_account: bool,
    /// Skip OOBE privacy consent screens.
    pub skip_privacy_screens: bool,
    /// Skip "Let's finish setting up" nag after first login.
    pub skip_finish_setup_nag: bool,
    /// Bypass TPM 2.0 requirement check.
    pub bypass_tpm: bool,
    /// Bypass Secure Boot requirement check.
    pub bypass_secureboot: bool,
    /// Bypass RAM requirement check.
    pub bypass_ram: bool,
    /// Set timezone (e.g. "Pacific Standard Time", "UTC").
    pub timezone: Option<String>,
    /// Create a local account with this username.
    pub local_account_name: Option<String>,
    /// Password for the local account (empty = no password).
    pub local_account_password: Option<String>,
    /// Auto-logon on first boot.
    pub auto_logon: bool,
    /// Disable BitLocker device encryption during OOBE.
    pub disable_bitlocker: bool,
    /// Set computer name ("*" = random).
    pub computer_name: Option<String>,
    /// Embed a product key for edition auto-selection.
    pub product_key: Option<String>,
    /// Additional PowerShell commands to run on first logon.
    pub first_logon_commands: Vec<String>,
    /// Convert Windows edition (e.g. "Professional", "Enterprise").
    pub convert_edition: Option<String>,
    /// Skip automatic Windows activation.
    pub skip_auto_activation: bool,
    /// Automatically partition disk 0 (EFI + MSR + Windows).
    pub auto_partition: bool,
}

impl Default for Oobe {
    fn default() -> Self {
        Self {
            inject_autounattend: true,
            skip_microsoft_account: true,
            skip_privacy_screens: true,
            skip_finish_setup_nag: true,
            bypass_tpm: true,
            bypass_secureboot: true,
            bypass_ram: true,
            timezone: None,
            local_account_name: None,
            local_account_password: None,
            auto_logon: false,
            disable_bitlocker: true,
            computer_name: None,
            product_key: None,
            first_logon_commands: Vec::new(),
            convert_edition: None,
            skip_auto_activation: true,
            auto_partition: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Seelen-UI
// ---------------------------------------------------------------------------

/// Controls Seelen-UI desktop environment bundling and shell replacement.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Seelen {
    /// Download and embed Seelen-UI setup into the ISO.
    pub bundle: bool,
    /// Set Seelen-UI as the Windows shell (replaces explorer.exe taskbar/start).
    pub replace_shell: bool,
    /// Remove Windows Search UI packages (Seelen has its own app launcher).
    pub remove_windows_search_ui: bool,
    /// Remove Start menu experience packages (Seelen replaces Start).
    pub remove_start_experience: bool,
}

impl Default for Seelen {
    fn default() -> Self {
        Self {
            bundle: true,
            replace_shell: true,
            remove_windows_search_ui: true,
            remove_start_experience: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Drivers
// ---------------------------------------------------------------------------

/// Controls injection of Linux filesystem drivers into the Windows image.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Drivers {
    /// Install WinBtrfs -- open-source btrfs filesystem driver.
    pub btrfs: bool,
    /// Install Ext2Fsd -- ext2/ext3/ext4 filesystem driver.
    pub ext4: bool,
    /// Install WinFsp -- Windows FUSE layer (required for mergerfs).
    pub winfsp: bool,
    /// Install mergerfs-windows -- union/pool filesystem (requires winfsp).
    pub mergerfs: bool,
    /// Install VirtIO drivers for QEMU/KVM virtual machines.
    pub virtio: bool,
}

impl Drivers {
    /// Returns `true` if any driver is enabled.
    pub fn any_enabled(&self) -> bool {
        self.btrfs || self.ext4 || self.winfsp || self.mergerfs || self.virtio
    }
}

// ---------------------------------------------------------------------------
// Windows Defender
// ---------------------------------------------------------------------------

/// Controls Windows Defender / Microsoft Defender.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Defender {
    /// Disable Windows Defender real-time protection via policy.
    pub disable_realtime: bool,
    /// Disable SmartScreen reputation checking.
    pub disable_smartscreen: bool,
    /// Disable cloud-delivered protection.
    pub disable_cloud_protection: bool,
    /// Disable automatic sample submission.
    pub disable_sample_submission: bool,
    /// Disable Windows Defender services (WinDefend, WdNisSvc).
    pub disable_services: bool,
}

impl Default for Defender {
    fn default() -> Self {
        Self {
            disable_realtime: true,
            disable_smartscreen: true,
            disable_cloud_protection: true,
            disable_sample_submission: true,
            disable_services: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Windows Update
// ---------------------------------------------------------------------------

/// Controls Windows Update behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowsUpdate {
    /// Disable automatic Windows updates.
    pub disable_auto_updates: bool,
    /// Disable Delivery Optimization (P2P update sharing).
    pub disable_delivery_optimization: bool,
    /// Disable update-related scheduled tasks.
    pub disable_update_tasks: bool,
    /// Exclude driver updates from Windows Update.
    pub exclude_driver_updates: bool,
    /// Disable automatic restart for updates when users are logged in.
    pub no_auto_reboot: bool,
}

impl Default for WindowsUpdate {
    fn default() -> Self {
        Self {
            disable_auto_updates: false,
            disable_delivery_optimization: true,
            disable_update_tasks: false,
            exclude_driver_updates: false,
            no_auto_reboot: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Explorer
// ---------------------------------------------------------------------------

/// Controls File Explorer defaults and shell behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Explorer {
    /// Restore classic right-click context menu (disable Windows 11 truncated menu).
    pub classic_context_menu: bool,
    /// Show file extensions in Explorer.
    pub show_file_extensions: bool,
    /// Show hidden files and folders.
    pub show_hidden_files: bool,
    /// Launch Explorer to "This PC" instead of Quick Access/Home.
    pub launch_to_this_pc: bool,
    /// Disable recent files in Quick Access.
    pub disable_recent_files: bool,
    /// Disable snap layouts popup on maximize hover.
    pub disable_snap_layouts: bool,
}

impl Default for Explorer {
    fn default() -> Self {
        Self {
            classic_context_menu: true,
            show_file_extensions: true,
            show_hidden_files: false,
            launch_to_this_pc: true,
            disable_recent_files: true,
            disable_snap_layouts: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Performance
// ---------------------------------------------------------------------------

/// Controls system performance optimizations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Performance {
    /// Inject a high-performance power plan on first boot.
    pub high_perf_power_plan: bool,
    /// Lower shutdown timeouts for faster shutdown.
    pub faster_shutdown: bool,
    /// Disable Game DVR / Game Bar recording.
    pub disable_game_dvr: bool,
    /// Globally disable background app execution.
    pub disable_background_apps: bool,
    /// Disable NTFS last access timestamps (NtfsDisableLastAccessUpdate).
    pub ntfs_disable_last_access: bool,
    /// Disable NTFS 8.3 short filename creation.
    pub ntfs_disable_8dot3: bool,
    /// Apply network tuning (disable Nagle, NetworkThrottlingIndex).
    pub network_tuning: bool,
}

impl Default for Performance {
    fn default() -> Self {
        Self {
            high_perf_power_plan: true,
            faster_shutdown: true,
            disable_game_dvr: true,
            disable_background_apps: true,
            ntfs_disable_last_access: true,
            ntfs_disable_8dot3: true,
            network_tuning: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Security
// ---------------------------------------------------------------------------

/// Controls security hardening.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Security {
    /// Enable Attack Surface Reduction (ASR) rules via Exploit Guard policy.
    pub asr_rules: bool,
    /// Disable Remote Desktop.
    pub disable_remote_desktop: bool,
    /// Disable SMBv1 (legacy file sharing protocol).
    pub disable_smb1: bool,
}

impl Default for Security {
    fn default() -> Self {
        Self {
            asr_rules: false,
            disable_remote_desktop: true,
            disable_smb1: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Windows Recall / AI
// ---------------------------------------------------------------------------

/// Controls Windows Recall and AI features (24H2+).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Recall {
    /// Disable Windows Recall / AI data analysis.
    pub disable: bool,
}

impl Default for Recall {
    fn default() -> Self {
        Self { disable: true }
    }
}

// ---------------------------------------------------------------------------
// File injection
// ---------------------------------------------------------------------------

/// Controls custom file/folder injection into the ISO.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Inject {
    /// Files or directories to inject into the WIM image.
    pub files: Vec<InjectEntry>,
}

/// A single file or directory to inject into the WIM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectEntry {
    /// Source path on the host filesystem.
    pub src: PathBuf,
    /// Destination path inside the WIM (relative to mount root, e.g. "Users/Public/Desktop").
    pub dest: String,
}

// ---------------------------------------------------------------------------
// Hooks
// ---------------------------------------------------------------------------

/// External scripts to run at pipeline stages.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Hooks {
    /// Script to run before ISO extraction.
    pub pre_extract: Option<String>,
    /// Script to run after debloat (apps pruned, before registry).
    pub post_debloat: Option<String>,
    /// Script to run after registry edits, before WIM commit.
    pub pre_repack: Option<String>,
    /// Script to run after ISO is repacked.
    pub post_build: Option<String>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_expected_defaults() {
        let cfg = Config::default();
        assert_eq!(cfg.wim_index, 6);
        assert!(cfg.apps.remove_bloatware);
        assert!(cfg.apps.remove_xbox);
        assert!(!cfg.apps.remove_store);
        assert!(cfg.apps.extra_patterns.is_empty());
        assert!(cfg.telemetry.disable);
        assert!(cfg.privacy.disable_advertising_id);
        assert!(cfg.copilot.disable);
        assert!(cfg.edge.disable_first_run);
        assert!(cfg.visuals.optimize_for_performance);
        assert!(cfg.taskbar.hide_widgets_button);
        assert!(cfg.services.disable_diagtrack);
        assert!(cfg.scheduled_tasks.remove_telemetry_tasks);
        assert!(cfg.oobe.inject_autounattend);
        assert!(cfg.seelen.bundle);
        assert!(!cfg.drivers.btrfs);
        assert!(!cfg.drivers.ext4);
        // New defaults
        assert!(cfg.defender.disable_realtime);
        assert!(cfg.recall.disable);
        assert!(cfg.explorer.classic_context_menu);
        assert!(cfg.performance.high_perf_power_plan);
        assert!(cfg.oobe.bypass_tpm);
        assert!(cfg.telemetry.block_telemetry_hosts);
        assert!(cfg.visuals.dark_mode);
        assert!(cfg.taskbar.left_align);
    }

    #[test]
    fn serde_roundtrip_preserves_values() {
        let mut cfg = Config {
            wim_index: WimIndex(3),
            ..Config::default()
        };
        cfg.apps.remove_store = true;
        cfg.apps.extra_patterns = vec!["CustomApp".into()];
        cfg.drivers.btrfs = true;
        cfg.seelen.bundle = false;

        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let loaded: Config = toml::from_str(&toml_str).unwrap();

        assert_eq!(loaded.wim_index, 3);
        assert!(loaded.apps.remove_store);
        assert_eq!(loaded.apps.extra_patterns, vec!["CustomApp".to_string()]);
        assert!(loaded.drivers.btrfs);
        assert!(!loaded.seelen.bundle);
    }

    #[test]
    fn empty_toml_yields_defaults() {
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg.wim_index, 6);
        assert!(cfg.apps.remove_bloatware);
        assert!(cfg.seelen.bundle);
    }

    #[test]
    fn partial_toml_fills_defaults() {
        let cfg: Config = toml::from_str("wim_index = 1\n[apps]\nremove_store = true").unwrap();
        assert_eq!(cfg.wim_index, 1);
        assert!(cfg.apps.remove_store);
        assert!(cfg.apps.remove_bloatware);
        assert!(cfg.telemetry.disable);
    }

    #[test]
    fn from_file_nonexistent_returns_io_error() {
        let result = Config::from_file(std::path::Path::new("/nonexistent/config.toml"));
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), crate::Error::Io { .. }));
    }

    #[test]
    fn from_file_invalid_toml_returns_config_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        std::fs::write(&path, "this is not valid toml {{{{").unwrap();
        let result = Config::from_file(&path);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), crate::Error::Config { .. }));
    }

    #[test]
    fn from_file_valid_toml_works() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ok.toml");
        std::fs::write(&path, "wim_index = 4\n[seelen]\nbundle = false").unwrap();
        let cfg = Config::from_file(&path).unwrap();
        assert_eq!(cfg.wim_index, 4);
        assert!(!cfg.seelen.bundle);
    }

    #[test]
    fn validate_rejects_seelen_with_edge_extra_pattern() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("conflict.toml");
        std::fs::write(
            &path,
            r#"
            [seelen]
            bundle = true
            [apps]
            extra_patterns = ["Microsoft.MicrosoftEdge.Stable"]
            "#,
        )
        .unwrap();
        let result = Config::from_file(&path);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("Edge"));
        assert!(msg.contains("Seelen"));
    }

    #[test]
    fn validate_rejects_seelen_with_webview2_pattern() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("wv2.toml");
        std::fs::write(
            &path,
            r#"
            [seelen]
            bundle = true
            [apps]
            extra_patterns = ["WebView2"]
            "#,
        )
        .unwrap();
        let result = Config::from_file(&path);
        assert!(result.is_err());
    }

    #[test]
    fn validate_allows_seelen_without_edge_pattern() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ok.toml");
        std::fs::write(
            &path,
            r#"
            [seelen]
            bundle = true
            [apps]
            extra_patterns = ["SomeOtherApp"]
            "#,
        )
        .unwrap();
        let cfg = Config::from_file(&path).unwrap();
        assert!(cfg.seelen.bundle);
    }

    #[test]
    fn validate_allows_edge_pattern_when_seelen_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ok2.toml");
        std::fs::write(
            &path,
            r#"
            [seelen]
            bundle = false
            [apps]
            extra_patterns = ["Microsoft.MicrosoftEdge"]
            "#,
        )
        .unwrap();
        let cfg = Config::from_file(&path).unwrap();
        assert!(!cfg.seelen.bundle);
    }

    #[test]
    fn drivers_any_enabled_none() {
        let d = Drivers::default();
        assert!(!d.any_enabled());
    }

    #[test]
    fn drivers_any_enabled_btrfs() {
        let d = Drivers {
            btrfs: true,
            ..Default::default()
        };
        assert!(d.any_enabled());
    }

    #[test]
    fn drivers_any_enabled_virtio() {
        let d = Drivers {
            virtio: true,
            ..Default::default()
        };
        assert!(d.any_enabled());
    }

    #[test]
    fn new_config_sections_roundtrip() {
        let mut cfg = Config::default();
        cfg.seelen.bundle = false;
        cfg.defender.disable_realtime = false;
        cfg.windows_update.disable_auto_updates = true;
        cfg.explorer.show_hidden_files = true;
        cfg.performance.network_tuning = true;
        cfg.security.asr_rules = true;
        cfg.recall.disable = false;
        cfg.oobe.bypass_tpm = false;
        cfg.oobe.timezone = Some("UTC".into());
        cfg.oobe.local_account_name = Some("User".into());

        let toml_str = toml::to_string_pretty(&cfg).unwrap();
        let loaded: Config = toml::from_str(&toml_str).unwrap();

        assert!(!loaded.defender.disable_realtime);
        assert!(loaded.windows_update.disable_auto_updates);
        assert!(loaded.explorer.show_hidden_files);
        assert!(loaded.performance.network_tuning);
        assert!(loaded.security.asr_rules);
        assert!(!loaded.recall.disable);
        assert!(!loaded.oobe.bypass_tpm);
        assert_eq!(loaded.oobe.timezone.as_deref(), Some("UTC"));
        assert_eq!(loaded.oobe.local_account_name.as_deref(), Some("User"));
    }

    #[test]
    fn wim_index_string_errors() {
        let result: Result<Config, _> = toml::from_str("wim_index = \"not a number\"");
        assert!(result.is_err());
    }

    #[test]
    fn bool_field_with_int_errors() {
        let result: Result<Config, _> = toml::from_str("[telemetry]\ndisable = 42");
        assert!(result.is_err());
    }

    #[test]
    fn wim_index_zero() {
        let cfg: Config = toml::from_str("wim_index = 0\n[seelen]\nbundle = false").unwrap();
        assert_eq!(cfg.wim_index, 0);
    }

    #[test]
    fn wim_index_max() {
        let cfg: Config =
            toml::from_str("wim_index = 4294967295\n[seelen]\nbundle = false").unwrap();
        assert_eq!(cfg.wim_index, u32::MAX);
    }

    #[test]
    fn validate_rejects_invalid_edition() {
        let mut cfg = Config::default();
        cfg.seelen.bundle = false;
        cfg.oobe.convert_edition = Some("InvalidEdition".into());
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn validate_allows_valid_edition() {
        let mut cfg = Config::default();
        cfg.oobe.convert_edition = Some("Professional".into());
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn validate_rejects_auto_logon_without_account_name() {
        let mut cfg = Config::default();
        cfg.oobe.auto_logon = true;
        cfg.oobe.local_account_name = None;
        let err = cfg.validate().unwrap_err();
        assert!(format!("{err}").contains("auto_logon"));
    }

    #[test]
    fn validate_allows_auto_logon_with_account_name() {
        let mut cfg = Config::default();
        cfg.oobe.auto_logon = true;
        cfg.oobe.local_account_name = Some("User".into());
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn validate_rejects_missing_inject_source() {
        let mut cfg = Config::default();
        cfg.inject.files.push(crate::config::InjectEntry {
            src: PathBuf::from("/nonexistent/path/to/file.txt"),
            dest: "test.txt".into(),
        });
        let err = cfg.validate().unwrap_err();
        assert!(format!("{err}").contains("inject source path"));
    }

    #[test]
    fn validate_allows_existing_inject_source() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("real_file.txt");
        std::fs::write(&src, "data").unwrap();

        let mut cfg = Config::default();
        cfg.inject.files.push(crate::config::InjectEntry {
            src,
            dest: "test.txt".into(),
        });
        assert!(cfg.validate().is_ok());
    }
}

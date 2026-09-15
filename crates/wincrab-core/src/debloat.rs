use std::path::{Path, PathBuf};

use smallvec::SmallVec;
use tracing::{info, warn};
use walkdir::WalkDir;

use crate::config::{AppRemoval, ScheduledTasks, Seelen};
use crate::error::{Error, remove_dir_all, remove_file};

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn windows_apps_dir(mount_dir: &Path) -> PathBuf {
    mount_dir.join("Program Files").join("WindowsApps")
}

fn provision_packages_dir(mount_dir: &Path) -> PathBuf {
    mount_dir
        .join("ProgramData")
        .join("Microsoft")
        .join("Windows")
        .join("AppRepository")
        .join("Packages")
}

/// Collect pattern slices from a table of `(enabled, patterns)` pairs.
///
/// Returns a `SmallVec` that fits up to 32 patterns inline (on the stack),
/// avoiding a heap allocation for the typical case where all built-in
/// pattern groups total well under 32 entries.
fn collect_patterns<'a>(groups: &[(bool, &[&'a str])]) -> SmallVec<[&'a str; 32]> {
    groups
        .iter()
        .filter(|(enabled, _)| *enabled)
        .flat_map(|(_, patterns)| patterns.iter().copied())
        .collect()
}

/// Well-known provisioned Appx package name fragments for bloatware.
const BLOATWARE_PATTERNS: &[&str] = &[
    // Third-party junk
    "BytedancePte.Ltd.TikTok",
    "king.com.CandyCrushSodaSaga",
    "king.com.CandyCrushSaga",
    "king.com.CandyCrushFriends",
    "SpotifyAB.SpotifyMusic",
    "Disney.37853FC22B2CE",
    "Clipchamp.Clipchamp",
    "Facebook.Facebook",
    "Instagram",
    // Microsoft bloat
    "Microsoft.BingNews",
    "Microsoft.BingWeather",
    "Microsoft.BingFinance",
    "Microsoft.BingSports",
    "Microsoft.GamingApp",
    "Microsoft.GetHelp",
    "Microsoft.Getstarted",
    "Microsoft.MicrosoftOfficeHub",
    "Microsoft.MicrosoftSolitaireCollection",
    "Microsoft.MicrosoftStickyNotes",
    "Microsoft.People",
    "Microsoft.PowerAutomateDesktop",
    "Microsoft.Todos",
    "Microsoft.WindowsAlarms",
    "Microsoft.WindowsFeedbackHub",
    "Microsoft.WindowsMaps",
    "Microsoft.WindowsSoundRecorder",
    "Microsoft.YourPhone",
    "Microsoft.ZuneMusic",
    "Microsoft.ZuneVideo",
    "MicrosoftCorporationII.QuickAssist",
    "MicrosoftWindows.Client.WebExperience", // Widgets
];

const XBOX_PATTERNS: &[&str] = &[
    "Microsoft.Xbox",
    "Microsoft.XboxApp",
    "Microsoft.XboxGameOverlay",
    "Microsoft.XboxGamingOverlay",
    "Microsoft.XboxIdentityProvider",
    "Microsoft.XboxSpeechToTextOverlay",
];

const TEAMS_PATTERNS: &[&str] = &["MicrosoftTeams", "MSTeams"];

const CORTANA_PATTERNS: &[&str] = &["Microsoft.549981C3F5F10"];

const OUTLOOK_PATTERNS: &[&str] = &["Microsoft.OutlookForWindows"];

const MAIL_PATTERNS: &[&str] = &["microsoft.windowscommunicationsapps"];

const DEV_HOME_PATTERNS: &[&str] = &["Microsoft.Windows.DevHome"];

const PHONE_LINK_PATTERNS: &[&str] = &["Microsoft.YourPhone", "MicrosoftWindows.CrossDevice"];

const ONEDRIVE_SETUP: &str = "OneDriveSetup.exe";

// Seelen-UI replacement patterns -- removable when Seelen provides alternatives.
const SEARCH_UI_PATTERNS: &[&str] = &["Microsoft.Windows.Search", "Microsoft.Windows.SearchHost"];

const START_EXPERIENCE_PATTERNS: &[&str] = &[
    "MicrosoftWindows.Client.CBS",
    "Microsoft.Windows.StartMenuExperienceHost",
];

/// Remove matching package directories from both `WindowsApps` and the
/// provisioning `Packages` directory. Shared by [`prune_appx_packages`]
/// and [`prune_seelen_replacements`].
fn prune_from_package_dirs(
    mount_dir: &Path,
    patterns: &[&str],
    stats: &mut PruneStats,
) -> Result<(), Error> {
    let apps_dir = windows_apps_dir(mount_dir);
    if apps_dir.is_dir() {
        remove_matching_dirs(&apps_dir, patterns, stats)?;
    }

    let provision_dir = provision_packages_dir(mount_dir);
    if provision_dir.is_dir() {
        remove_matching_dirs(&provision_dir, patterns, stats)?;
    }

    Ok(())
}

/// Remove provisioned Appx packages from the mounted WIM image.
///
/// This operates on two locations inside the mounted image:
/// 1. `Program Files/WindowsApps/` -- the package installation directory
/// 2. `ProgramData/Microsoft/Windows/AppRepository/` -- the provisioning manifests
pub fn prune_appx_packages(mount_dir: &Path, config: &AppRemoval) -> Result<PruneStats, Error> {
    let mut stats = PruneStats::default();

    // Table-driven pattern collection.
    let mut patterns = collect_patterns(&[
        (config.remove_bloatware, BLOATWARE_PATTERNS),
        (config.remove_xbox, XBOX_PATTERNS),
        (config.remove_teams, TEAMS_PATTERNS),
        (config.remove_cortana, CORTANA_PATTERNS),
        (config.remove_outlook, OUTLOOK_PATTERNS),
        (config.remove_mail, MAIL_PATTERNS),
        (config.remove_dev_home, DEV_HOME_PATTERNS),
        (config.remove_phone_link, PHONE_LINK_PATTERNS),
    ]);
    patterns.extend(config.extra_patterns.iter().map(String::as_str));

    if patterns.is_empty() {
        info!("no app removal patterns configured -- skipping");
        return Ok(stats);
    }

    prune_from_package_dirs(mount_dir, &patterns, &mut stats)?;

    if !windows_apps_dir(mount_dir).is_dir() {
        warn!(
            path = %windows_apps_dir(mount_dir).display(),
            "WindowsApps directory not found -- skipping"
        );
    }

    // Remove OneDrive installer if requested.
    if config.remove_onedrive {
        for subdir in ["System32", "SysWOW64"] {
            let p = mount_dir.join("Windows").join(subdir).join(ONEDRIVE_SETUP);
            if p.exists() {
                info!(path = %p.display(), "removing OneDrive installer");
                remove_file(&p)?;
                stats.files_removed += 1;
            }
        }
    }

    // Remove Microsoft Store if requested.
    if config.remove_store {
        let store_patterns = ["Microsoft.WindowsStore", "Microsoft.StorePurchaseApp"];
        let apps_dir = windows_apps_dir(mount_dir);
        if apps_dir.is_dir() {
            remove_matching_dirs(&apps_dir, &store_patterns, &mut stats)?;
        }
    }

    info!(
        dirs_removed = stats.dirs_removed,
        files_removed = stats.files_removed,
        "app pruning complete"
    );

    Ok(stats)
}

/// Remove Windows components that Seelen-UI replaces (search UI, Start menu
/// experience). Only called when Seelen-UI bundling is enabled.
pub fn prune_seelen_replacements(mount_dir: &Path, config: &Seelen) -> Result<PruneStats, Error> {
    let patterns = collect_patterns(&[
        (config.remove_windows_search_ui, SEARCH_UI_PATTERNS),
        (config.remove_start_experience, START_EXPERIENCE_PATTERNS),
    ]);

    if patterns.is_empty() {
        return Ok(PruneStats::default());
    }

    info!("removing Windows components replaced by Seelen-UI");

    let mut stats = PruneStats::default();
    prune_from_package_dirs(mount_dir, &patterns, &mut stats)?;

    info!(
        dirs_removed = stats.dirs_removed,
        files_removed = stats.files_removed,
        "Seelen-UI replacement pruning complete"
    );

    Ok(stats)
}

/// Remove scheduled task XML files from the mounted WIM image.
///
/// Windows stores scheduled tasks as XML files under
/// `Windows/System32/Tasks/Microsoft/Windows/<Category>/`.
pub fn remove_scheduled_tasks(
    mount_dir: &Path,
    config: &ScheduledTasks,
) -> Result<PruneStats, Error> {
    let mut stats = PruneStats::default();
    let tasks_base = mount_dir
        .join("Windows")
        .join("System32")
        .join("Tasks")
        .join("Microsoft")
        .join("Windows");

    if !tasks_base.is_dir() {
        warn!(
            path = %tasks_base.display(),
            "scheduled tasks directory not found -- skipping"
        );
        return Ok(stats);
    }

    if config.remove_telemetry_tasks {
        // Application Experience\Microsoft Compatibility Appraiser
        // Application Experience\ProgramDataUpdater
        // Application Experience\AitAgent
        remove_task_files(
            &tasks_base.join("Application Experience"),
            &[
                "Microsoft Compatibility Appraiser",
                "ProgramDataUpdater",
                "AitAgent",
            ],
            &mut stats,
        )?;
    }

    if config.remove_ceip_tasks {
        // Customer Experience Improvement Program\Consolidator
        // Customer Experience Improvement Program\UsbCeip
        // Customer Experience Improvement Program\KernelCeipTask
        remove_task_dir(
            &tasks_base.join("Customer Experience Improvement Program"),
            &mut stats,
        )?;
    }

    if config.remove_disk_diagnostic_tasks {
        remove_task_dir(&tasks_base.join("DiskDiagnostic"), &mut stats)?;
    }

    if config.remove_maps_task {
        remove_task_files(
            &tasks_base.join("Maps"),
            &["MapsUpdateTask", "MapsToastTask"],
            &mut stats,
        )?;
    }

    if config.remove_feedback_tasks {
        remove_task_dir(&tasks_base.join("Feedback").join("Siuf"), &mut stats)?;
        // Also remove Windows Error Reporting task
        remove_task_files(
            &tasks_base.join("Windows Error Reporting"),
            &["QueueReporting"],
            &mut stats,
        )?;
    }

    info!(
        dirs_removed = stats.dirs_removed,
        files_removed = stats.files_removed,
        "scheduled task cleanup complete"
    );

    Ok(stats)
}

/// Remove specific task files by name from a task category directory.
fn remove_task_files(
    category_dir: &Path,
    task_names: &[&str],
    stats: &mut PruneStats,
) -> Result<(), Error> {
    if !category_dir.is_dir() {
        return Ok(());
    }

    for name in task_names {
        let task_path = category_dir.join(name);
        if task_path.exists() {
            info!(task = %task_path.display(), "removing scheduled task");
            remove_file(&task_path)?;
            stats.files_removed += 1;
        }
    }

    Ok(())
}

/// Remove an entire task category directory.
fn remove_task_dir(dir: &Path, stats: &mut PruneStats) -> Result<(), Error> {
    if !dir.is_dir() {
        return Ok(());
    }

    let file_count = WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .count();

    info!(
        dir = %dir.display(),
        files = file_count,
        "removing scheduled task directory"
    );

    remove_dir_all(dir)?;

    stats.dirs_removed += 1;
    stats.files_removed += file_count;

    Ok(())
}

/// Remove top-level subdirectories whose name contains any of the given patterns.
fn remove_matching_dirs(
    parent: &Path,
    patterns: &[&str],
    stats: &mut PruneStats,
) -> Result<(), Error> {
    let entries = std::fs::read_dir(parent).map_err(|e| Error::Io {
        context: format!("reading {}", parent.display()),
        source: e,
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| Error::Io {
            context: format!("iterating {}", parent.display()),
            source: e,
        })?;

        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        let dominated = patterns.iter().any(|p| name_str.contains(p));
        if !dominated {
            continue;
        }

        let path = entry.path();
        if path.is_dir() {
            // Count files inside before removing.
            let file_count = WalkDir::new(&path)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_file())
                .count();

            info!(
                package = %name_str,
                files = file_count,
                "removing provisioned package"
            );

            remove_dir_all(&path)?;

            stats.dirs_removed += 1;
            stats.files_removed += file_count;
        }
    }

    Ok(())
}

#[derive(Debug, Default)]
pub struct PruneStats {
    pub dirs_removed: usize,
    pub files_removed: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppRemoval, ScheduledTasks, Seelen};

    /// Create a fake WindowsApps directory with given package names.
    fn create_fake_apps(mount_dir: &std::path::Path, package_names: &[&str]) {
        let apps_dir = mount_dir.join("Program Files").join("WindowsApps");
        std::fs::create_dir_all(&apps_dir).unwrap();
        for name in package_names {
            let pkg_dir = apps_dir.join(name);
            std::fs::create_dir_all(&pkg_dir).unwrap();
            std::fs::write(pkg_dir.join("app.exe"), b"fake").unwrap();
        }
    }

    /// Create a fake provisioning dir with given package names.
    fn create_fake_provision(mount_dir: &std::path::Path, package_names: &[&str]) {
        let prov_dir = mount_dir
            .join("ProgramData")
            .join("Microsoft")
            .join("Windows")
            .join("AppRepository")
            .join("Packages");
        std::fs::create_dir_all(&prov_dir).unwrap();
        for name in package_names {
            let pkg_dir = prov_dir.join(name);
            std::fs::create_dir_all(&pkg_dir).unwrap();
            std::fs::write(pkg_dir.join("manifest.xml"), b"<xml/>").unwrap();
        }
    }

    fn all_disabled_config() -> AppRemoval {
        AppRemoval {
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
        }
    }

    // -----------------------------------------------------------------------
    // prune_appx_packages
    // -----------------------------------------------------------------------

    #[test]
    fn prune_removes_bloatware_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        create_fake_apps(
            mount,
            &[
                "BytedancePte.Ltd.TikTok_1.0",
                "king.com.CandyCrushSodaSaga_2.0",
                "Microsoft.VisualStudio_17.0",
            ],
        );

        let config = AppRemoval {
            remove_bloatware: true,
            ..all_disabled_config()
        };

        let stats = prune_appx_packages(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 2);

        let remaining: Vec<_> = std::fs::read_dir(mount.join("Program Files/WindowsApps"))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(remaining.len(), 1);
        assert!(
            remaining[0]
                .file_name()
                .to_string_lossy()
                .contains("VisualStudio")
        );
    }

    #[test]
    fn prune_removes_xbox_packages() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        create_fake_apps(
            mount,
            &[
                "Microsoft.XboxApp_1.0",
                "Microsoft.XboxGameOverlay_1.0",
                "Microsoft.VisualStudio_17.0",
            ],
        );

        let config = AppRemoval {
            remove_xbox: true,
            ..all_disabled_config()
        };

        let stats = prune_appx_packages(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 2);
    }

    #[test]
    fn prune_with_extra_patterns() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        create_fake_apps(mount, &["SomeCustomApp_1.0", "AnotherApp_2.0"]);

        let config = AppRemoval {
            extra_patterns: vec!["SomeCustomApp".into()],
            ..all_disabled_config()
        };

        let stats = prune_appx_packages(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 1);
        assert_eq!(stats.files_removed, 1);
    }

    #[test]
    fn prune_no_patterns_returns_empty_stats() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        create_fake_apps(mount, &["SomeApp_1.0"]);

        let stats = prune_appx_packages(mount, &all_disabled_config()).unwrap();
        assert_eq!(stats.dirs_removed, 0);
        assert_eq!(stats.files_removed, 0);
    }

    #[test]
    fn prune_missing_apps_dir_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let stats = prune_appx_packages(dir.path(), &AppRemoval::default()).unwrap();
        assert_eq!(stats.dirs_removed, 0);
    }

    #[test]
    fn prune_removes_onedrive_installers() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();

        let sys32 = mount.join("Windows").join("System32");
        let syswow = mount.join("Windows").join("SysWOW64");
        std::fs::create_dir_all(&sys32).unwrap();
        std::fs::create_dir_all(&syswow).unwrap();
        std::fs::write(sys32.join("OneDriveSetup.exe"), b"fake").unwrap();
        std::fs::write(syswow.join("OneDriveSetup.exe"), b"fake").unwrap();

        // Need at least one pattern to avoid the early-return in prune_appx_packages.
        let config = AppRemoval {
            remove_onedrive: true,
            extra_patterns: vec!["__nonexistent_placeholder__".into()],
            ..all_disabled_config()
        };

        let stats = prune_appx_packages(mount, &config).unwrap();
        assert_eq!(stats.files_removed, 2);
        assert!(!sys32.join("OneDriveSetup.exe").exists());
        assert!(!syswow.join("OneDriveSetup.exe").exists());
    }

    #[test]
    fn prune_removes_store_when_requested() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        create_fake_apps(
            mount,
            &[
                "Microsoft.WindowsStore_1.0",
                "Microsoft.StorePurchaseApp_1.0",
                "Microsoft.Calculator_1.0",
            ],
        );

        // Need at least one pattern to avoid the early-return in prune_appx_packages.
        let config = AppRemoval {
            remove_store: true,
            extra_patterns: vec!["__nonexistent_placeholder__".into()],
            ..all_disabled_config()
        };

        let stats = prune_appx_packages(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 2);
    }

    #[test]
    fn prune_also_removes_from_provisioning_dir() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        create_fake_apps(mount, &["BytedancePte.Ltd.TikTok_1.0"]);
        create_fake_provision(mount, &["BytedancePte.Ltd.TikTok_1.0"]);

        let config = AppRemoval {
            remove_bloatware: true,
            ..all_disabled_config()
        };

        let stats = prune_appx_packages(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 2);
    }

    #[test]
    fn prune_pattern_is_substring_match() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        create_fake_apps(
            mount,
            &[
                "BytedancePte.Ltd.TikTok_1.2.3_x64__abcdef",
                "SomethingElse_1.0",
            ],
        );

        let config = AppRemoval {
            remove_bloatware: true,
            ..all_disabled_config()
        };

        let stats = prune_appx_packages(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 1);
    }

    #[test]
    fn prune_pattern_case_sensitive() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        create_fake_apps(mount, &["bytedancepte.ltd.tiktok_1.0"]);

        let config = AppRemoval {
            remove_bloatware: true,
            ..all_disabled_config()
        };

        let stats = prune_appx_packages(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 0);
    }

    #[test]
    fn prune_counts_nested_files() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        let apps_dir = mount.join("Program Files").join("WindowsApps");
        let pkg = apps_dir.join("BytedancePte.Ltd.TikTok_1.0");
        std::fs::create_dir_all(pkg.join("subdir")).unwrap();
        std::fs::write(pkg.join("app.exe"), b"fake").unwrap();
        std::fs::write(pkg.join("subdir").join("data.bin"), b"fake").unwrap();
        std::fs::write(pkg.join("subdir").join("more.bin"), b"fake").unwrap();

        let config = AppRemoval {
            remove_bloatware: true,
            ..all_disabled_config()
        };

        let stats = prune_appx_packages(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 1);
        assert_eq!(stats.files_removed, 3);
    }

    #[test]
    fn prune_removes_teams() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        create_fake_apps(mount, &["MicrosoftTeams_1.0", "MSTeams_2.0", "Notepad_1.0"]);

        let config = AppRemoval {
            remove_teams: true,
            ..all_disabled_config()
        };

        let stats = prune_appx_packages(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 2);
    }

    #[test]
    fn prune_removes_cortana() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        create_fake_apps(mount, &["Microsoft.549981C3F5F10_1.0"]);

        let config = AppRemoval {
            remove_cortana: true,
            ..all_disabled_config()
        };

        let stats = prune_appx_packages(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 1);
    }

    #[test]
    fn prune_removes_phone_link() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        create_fake_apps(
            mount,
            &[
                "Microsoft.YourPhone_1.0",
                "MicrosoftWindows.CrossDevice_1.0",
            ],
        );

        let config = AppRemoval {
            remove_phone_link: true,
            ..all_disabled_config()
        };

        let stats = prune_appx_packages(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 2);
    }

    // -----------------------------------------------------------------------
    // prune_seelen_replacements
    // -----------------------------------------------------------------------

    #[test]
    fn prune_seelen_removes_search_and_start() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        create_fake_apps(
            mount,
            &[
                "Microsoft.Windows.Search_1.0",
                "MicrosoftWindows.Client.CBS_1.0",
                "Microsoft.Calculator_1.0",
            ],
        );

        let config = Seelen {
            bundle: true,
            replace_shell: true,
            remove_windows_search_ui: true,
            remove_start_experience: true,
        };

        let stats = prune_seelen_replacements(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 2);
    }

    #[test]
    fn prune_seelen_nothing_when_both_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        create_fake_apps(mount, &["Microsoft.Windows.Search_1.0"]);

        let config = Seelen {
            bundle: true,
            replace_shell: false,
            remove_windows_search_ui: false,
            remove_start_experience: false,
        };

        let stats = prune_seelen_replacements(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 0);
    }

    // -----------------------------------------------------------------------
    // remove_scheduled_tasks
    // -----------------------------------------------------------------------

    fn create_tasks(mount_dir: &std::path::Path) {
        let base = mount_dir
            .join("Windows")
            .join("System32")
            .join("Tasks")
            .join("Microsoft")
            .join("Windows");

        let app_exp = base.join("Application Experience");
        std::fs::create_dir_all(&app_exp).unwrap();
        std::fs::write(app_exp.join("Microsoft Compatibility Appraiser"), b"task").unwrap();
        std::fs::write(app_exp.join("ProgramDataUpdater"), b"task").unwrap();
        std::fs::write(app_exp.join("AitAgent"), b"task").unwrap();

        let ceip = base.join("Customer Experience Improvement Program");
        std::fs::create_dir_all(&ceip).unwrap();
        std::fs::write(ceip.join("Consolidator"), b"task").unwrap();
        std::fs::write(ceip.join("UsbCeip"), b"task").unwrap();

        let diag = base.join("DiskDiagnostic");
        std::fs::create_dir_all(&diag).unwrap();
        std::fs::write(diag.join("ScheduledDiag"), b"task").unwrap();

        let maps = base.join("Maps");
        std::fs::create_dir_all(&maps).unwrap();
        std::fs::write(maps.join("MapsUpdateTask"), b"task").unwrap();
        std::fs::write(maps.join("MapsToastTask"), b"task").unwrap();

        let feedback = base.join("Feedback").join("Siuf");
        std::fs::create_dir_all(&feedback).unwrap();
        std::fs::write(feedback.join("Siuf"), b"task").unwrap();

        let wer = base.join("Windows Error Reporting");
        std::fs::create_dir_all(&wer).unwrap();
        std::fs::write(wer.join("QueueReporting"), b"task").unwrap();
    }

    #[test]
    fn remove_all_scheduled_tasks() {
        let dir = tempfile::tempdir().unwrap();
        create_tasks(dir.path());

        let stats = remove_scheduled_tasks(dir.path(), &ScheduledTasks::default()).unwrap();
        assert!(stats.files_removed > 0);
        assert!(stats.dirs_removed >= 3);
    }

    #[test]
    fn remove_only_telemetry_tasks() {
        let dir = tempfile::tempdir().unwrap();
        create_tasks(dir.path());

        let config = ScheduledTasks {
            remove_telemetry_tasks: true,
            remove_ceip_tasks: false,
            remove_disk_diagnostic_tasks: false,
            remove_maps_task: false,
            remove_feedback_tasks: false,
        };

        let stats = remove_scheduled_tasks(dir.path(), &config).unwrap();
        assert_eq!(stats.files_removed, 3);
        assert_eq!(stats.dirs_removed, 0);
    }

    #[test]
    fn remove_tasks_missing_base_dir_ok() {
        let dir = tempfile::tempdir().unwrap();
        let stats = remove_scheduled_tasks(dir.path(), &ScheduledTasks::default()).unwrap();
        assert_eq!(stats.files_removed, 0);
        assert_eq!(stats.dirs_removed, 0);
    }

    #[test]
    fn remove_tasks_none_configured() {
        let dir = tempfile::tempdir().unwrap();
        create_tasks(dir.path());

        let config = ScheduledTasks {
            remove_telemetry_tasks: false,
            remove_ceip_tasks: false,
            remove_disk_diagnostic_tasks: false,
            remove_maps_task: false,
            remove_feedback_tasks: false,
        };

        let stats = remove_scheduled_tasks(dir.path(), &config).unwrap();
        assert_eq!(stats.files_removed, 0);
        assert_eq!(stats.dirs_removed, 0);
    }

    #[test]
    fn remove_only_maps_tasks() {
        let dir = tempfile::tempdir().unwrap();
        create_tasks(dir.path());

        let config = ScheduledTasks {
            remove_telemetry_tasks: false,
            remove_ceip_tasks: false,
            remove_disk_diagnostic_tasks: false,
            remove_maps_task: true,
            remove_feedback_tasks: false,
        };

        let stats = remove_scheduled_tasks(dir.path(), &config).unwrap();
        assert_eq!(stats.files_removed, 2);
    }

    // -----------------------------------------------------------------------
    // Edge cases
    // -----------------------------------------------------------------------

    #[test]
    fn prune_empty_apps_dir_returns_zero_stats() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        let apps_dir = mount.join("Program Files").join("WindowsApps");
        std::fs::create_dir_all(&apps_dir).unwrap();

        let config = AppRemoval::default();
        let stats = prune_appx_packages(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 0);
        assert_eq!(stats.files_removed, 0);
    }

    #[test]
    fn prune_with_all_categories_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        create_fake_apps(
            mount,
            &[
                "BytedancePte.Ltd.TikTok_1.0",
                "Microsoft.XboxApp_1.0",
                "MicrosoftTeams_1.0",
                "Microsoft.549981C3F5F10_1.0",
                "Microsoft.OutlookForWindows_1.0",
                "microsoft.windowscommunicationsapps_1.0",
                "Microsoft.Windows.DevHome_1.0",
                "Microsoft.YourPhone_1.0",
                "Microsoft.WindowsCalculator_1.0", // survivor
            ],
        );

        let config = AppRemoval {
            remove_bloatware: true,
            remove_xbox: true,
            remove_teams: true,
            remove_onedrive: false,
            remove_cortana: true,
            remove_store: false,
            remove_outlook: true,
            remove_mail: true,
            remove_dev_home: true,
            remove_phone_link: true,
            extra_patterns: vec![],
        };

        let stats = prune_appx_packages(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 8);

        let remaining: Vec<_> = std::fs::read_dir(mount.join("Program Files/WindowsApps"))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(remaining.len(), 1);
        assert!(
            remaining[0]
                .file_name()
                .to_string_lossy()
                .contains("Calculator")
        );
    }

    #[test]
    fn prune_with_multiple_extra_patterns() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        create_fake_apps(
            mount,
            &["MyCustomApp_1.0", "AnotherCustom_2.0", "SafeApp_1.0"],
        );

        let config = AppRemoval {
            extra_patterns: vec!["MyCustomApp".into(), "AnotherCustom".into()],
            ..all_disabled_config()
        };

        let stats = prune_appx_packages(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 2);
    }

    #[test]
    fn prune_extra_pattern_overlapping_with_builtin() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        create_fake_apps(mount, &["BytedancePte.Ltd.TikTok_1.0"]);

        // Both builtin bloatware and extra pattern match the same package.
        let config = AppRemoval {
            remove_bloatware: true,
            extra_patterns: vec!["TikTok".into()],
            ..all_disabled_config()
        };

        let stats = prune_appx_packages(mount, &config).unwrap();
        // Should only be removed once.
        assert_eq!(stats.dirs_removed, 1);
    }

    #[test]
    fn prune_deeply_nested_package_structure() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        let apps_dir = mount.join("Program Files").join("WindowsApps");
        let pkg = apps_dir.join("BytedancePte.Ltd.TikTok_1.0");
        let deep = pkg.join("a").join("b").join("c").join("d");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("deep.bin"), b"data").unwrap();
        std::fs::write(pkg.join("root.exe"), b"exe").unwrap();

        let config = AppRemoval {
            remove_bloatware: true,
            ..all_disabled_config()
        };

        let stats = prune_appx_packages(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 1);
        assert_eq!(stats.files_removed, 2);
    }

    #[test]
    fn prune_stats_default_is_zero() {
        let stats = PruneStats::default();
        assert_eq!(stats.dirs_removed, 0);
        assert_eq!(stats.files_removed, 0);
    }

    #[test]
    fn prune_stats_debug_format() {
        let stats = PruneStats {
            dirs_removed: 5,
            files_removed: 10,
        };
        let debug = format!("{stats:?}");
        assert!(debug.contains("5"));
        assert!(debug.contains("10"));
    }

    #[test]
    fn prune_onedrive_only_sys32_present() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();

        let sys32 = mount.join("Windows").join("System32");
        std::fs::create_dir_all(&sys32).unwrap();
        std::fs::write(sys32.join("OneDriveSetup.exe"), b"fake").unwrap();
        // SysWOW64 does NOT exist.

        let config = AppRemoval {
            remove_onedrive: true,
            extra_patterns: vec!["__placeholder__".into()],
            ..all_disabled_config()
        };

        let stats = prune_appx_packages(mount, &config).unwrap();
        assert_eq!(stats.files_removed, 1);
    }

    #[test]
    fn prune_seelen_only_search_ui() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        create_fake_apps(
            mount,
            &[
                "Microsoft.Windows.Search_1.0",
                "Microsoft.Windows.SearchHost_1.0",
                "MicrosoftWindows.Client.CBS_1.0",
                "Microsoft.Windows.StartMenuExperienceHost_1.0",
            ],
        );

        let config = Seelen {
            bundle: true,
            replace_shell: false,
            remove_windows_search_ui: true,
            remove_start_experience: false,
        };

        let stats = prune_seelen_replacements(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 2); // Search + SearchHost
    }

    #[test]
    fn prune_seelen_only_start_experience() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        create_fake_apps(
            mount,
            &[
                "Microsoft.Windows.Search_1.0",
                "MicrosoftWindows.Client.CBS_1.0",
                "Microsoft.Windows.StartMenuExperienceHost_1.0",
            ],
        );

        let config = Seelen {
            bundle: true,
            replace_shell: false,
            remove_windows_search_ui: false,
            remove_start_experience: true,
        };

        let stats = prune_seelen_replacements(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 2); // CBS + StartMenuExperienceHost
    }

    #[test]
    fn remove_only_feedback_tasks() {
        let dir = tempfile::tempdir().unwrap();
        create_tasks(dir.path());

        let config = ScheduledTasks {
            remove_telemetry_tasks: false,
            remove_ceip_tasks: false,
            remove_disk_diagnostic_tasks: false,
            remove_maps_task: false,
            remove_feedback_tasks: true,
        };

        let stats = remove_scheduled_tasks(dir.path(), &config).unwrap();
        // Feedback/Siuf directory (1 file) + Windows Error Reporting/QueueReporting (1 file)
        assert!(stats.files_removed >= 2);
    }

    #[test]
    fn remove_only_ceip_tasks() {
        let dir = tempfile::tempdir().unwrap();
        create_tasks(dir.path());

        let config = ScheduledTasks {
            remove_telemetry_tasks: false,
            remove_ceip_tasks: true,
            remove_disk_diagnostic_tasks: false,
            remove_maps_task: false,
            remove_feedback_tasks: false,
        };

        let stats = remove_scheduled_tasks(dir.path(), &config).unwrap();
        assert_eq!(stats.dirs_removed, 1);
        assert_eq!(stats.files_removed, 2); // Consolidator + UsbCeip
    }

    #[test]
    fn remove_only_disk_diagnostic_tasks() {
        let dir = tempfile::tempdir().unwrap();
        create_tasks(dir.path());

        let config = ScheduledTasks {
            remove_telemetry_tasks: false,
            remove_ceip_tasks: false,
            remove_disk_diagnostic_tasks: true,
            remove_maps_task: false,
            remove_feedback_tasks: false,
        };

        let stats = remove_scheduled_tasks(dir.path(), &config).unwrap();
        assert_eq!(stats.dirs_removed, 1);
        assert_eq!(stats.files_removed, 1); // ScheduledDiag
    }

    #[test]
    fn prune_removes_outlook() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        create_fake_apps(mount, &["Microsoft.OutlookForWindows_1.0", "Notepad_1.0"]);

        let config = AppRemoval {
            remove_outlook: true,
            ..all_disabled_config()
        };

        let stats = prune_appx_packages(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 1);
    }

    #[test]
    fn prune_removes_mail() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        create_fake_apps(
            mount,
            &["microsoft.windowscommunicationsapps_1.0", "Notepad_1.0"],
        );

        let config = AppRemoval {
            remove_mail: true,
            ..all_disabled_config()
        };

        let stats = prune_appx_packages(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 1);
    }

    #[test]
    fn prune_removes_dev_home() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        create_fake_apps(mount, &["Microsoft.Windows.DevHome_0.1_x64", "Notepad_1.0"]);

        let config = AppRemoval {
            remove_dev_home: true,
            ..all_disabled_config()
        };

        let stats = prune_appx_packages(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 1);
    }

    #[test]
    fn prune_seelen_also_removes_from_provisioning() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path();
        create_fake_apps(mount, &["Microsoft.Windows.Search_1.0"]);
        create_fake_provision(mount, &["Microsoft.Windows.Search_1.0"]);

        let config = Seelen {
            bundle: true,
            replace_shell: false,
            remove_windows_search_ui: true,
            remove_start_experience: false,
        };

        let stats = prune_seelen_replacements(mount, &config).unwrap();
        assert_eq!(stats.dirs_removed, 2); // One from apps, one from provisioning
    }
}

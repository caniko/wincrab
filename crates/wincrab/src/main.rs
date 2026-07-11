use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};
use tracing::{error, info};

/// wincrab -- build debloated Windows 11 ISOs from Linux.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Increase log verbosity (-v = debug, -vv = trace).
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,
}

#[derive(Subcommand)]
enum Commands {
    /// Build a debloated Windows 11 ISO.
    Build {
        /// Path to the source Windows 11 ISO.
        #[arg(short, long)]
        iso: PathBuf,
        /// Path to the output (debloated) ISO.
        #[arg(short, long)]
        output: PathBuf,
        /// Path to a TOML configuration file.
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Working directory for intermediate files.
        #[arg(short, long, default_value = "wincrab-work")]
        work_dir: PathBuf,
        /// Use a named profile as base config.
        #[arg(short, long)]
        profile: Option<String>,
        /// Dry-run mode: show what would be changed without modifying anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// Check that all external tools are installed and working.
    Doctor,
    /// Inspect an ISO or WIM file.
    Inspect {
        /// Path to the source ISO.
        #[arg(short, long)]
        iso: PathBuf,
        /// Working directory for intermediate files.
        #[arg(short, long, default_value = "wincrab-work")]
        work_dir: PathBuf,
    },
    /// Generate a default config.toml.
    Init {
        /// Output path (default: config.toml).
        #[arg(short, long, default_value = "config.toml")]
        output: PathBuf,
    },
    /// Validate a config file.
    Validate {
        /// Path to the config file to validate.
        #[arg(short, long)]
        config: PathBuf,
    },
    /// List available profiles.
    Profiles,
    /// Generate shell completions.
    Completions {
        /// Shell to generate completions for.
        shell: clap_complete::Shell,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Logging.
    let filter = match cli.verbose {
        0 => "wincrab=info,wincrab_core=info",
        1 => "wincrab=debug,wincrab_core=debug",
        _ => "wincrab=trace,wincrab_core=trace",
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| filter.into()),
        )
        .with_target(false)
        .init();

    // Install panic hook so WIM mounts are cleaned up on panic.
    wincrab_core::mount::install_panic_hook();

    match cli.command {
        Commands::Build {
            iso,
            output,
            config,
            work_dir,
            profile,
            dry_run,
        } => cmd_build(iso, output, config, work_dir, profile, dry_run),
        Commands::Doctor => cmd_doctor(),
        Commands::Inspect { iso, work_dir } => cmd_inspect(iso, work_dir),
        Commands::Init { output } => cmd_init(output),
        Commands::Validate { config } => cmd_validate(config),
        Commands::Profiles => cmd_profiles(),
        Commands::Completions { shell } => cmd_completions(shell),
    }
}

fn cmd_build(
    iso: PathBuf,
    output: PathBuf,
    config_path: Option<PathBuf>,
    work_dir: PathBuf,
    profile: Option<String>,
    dry_run: bool,
) -> ExitCode {
    // Load base config from profile if specified.
    let base_config = if let Some(ref profile_name) = profile {
        match wincrab_core::profiles::load_profile(profile_name) {
            Ok(cfg) => {
                info!(profile = %profile_name, "loaded profile as base config");
                Some(cfg)
            }
            Err(e) => {
                error!(%e, "failed to load profile");
                eprintln!(
                    "Unknown profile: {profile_name}. Run `wincrab profiles` to see available profiles."
                );
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };

    // Merge with config file overrides using deep merge if we have a profile base.
    let config = match (base_config, &config_path) {
        (Some(base), Some(path)) => {
            // Read the config file as raw TOML string for deep merge.
            let toml_str = match std::fs::read_to_string(path) {
                Ok(s) => s,
                Err(e) => {
                    error!(%e, path = %path.display(), "failed to read config file");
                    return ExitCode::FAILURE;
                }
            };
            match wincrab_core::profiles::merge_with_overrides(base, &toml_str) {
                Ok(cfg) => {
                    info!(path = %path.display(), "merged config file over profile");
                    cfg
                }
                Err(e) => {
                    error!(%e, "failed to merge config");
                    return ExitCode::FAILURE;
                }
            }
        }
        (Some(base), None) => base,
        (None, Some(path)) => match wincrab_core::Config::from_file(path) {
            Ok(c) => {
                info!(path = %path.display(), "loaded config file");
                c
            }
            Err(e) => {
                error!(%e, "failed to load config");
                return ExitCode::FAILURE;
            }
        },
        (None, None) => {
            info!("no config file or profile specified -- using built-in defaults");
            wincrab_core::Config::default()
        }
    };

    info!(?config, "loaded configuration");

    // Validate inputs early — fail fast instead of 30 minutes into the build.
    if !iso.exists() {
        error!(path = %iso.display(), "source ISO not found");
        return ExitCode::FAILURE;
    }
    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        error!(path = %parent.display(), "output directory does not exist");
        return ExitCode::FAILURE;
    }
    if let Some(parent) = work_dir.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        error!(path = %parent.display(), "work directory parent does not exist");
        return ExitCode::FAILURE;
    }

    if dry_run {
        info!("dry-run mode: printing config and exiting");
        let toml_str =
            toml::to_string_pretty(&config).unwrap_or_else(|e| format!("(serialize error: {e})"));
        println!("{toml_str}");
        return ExitCode::SUCCESS;
    }

    // Run the pipeline.
    if let Err(e) = wincrab_core::pipeline::run(&config, &iso, &output, &work_dir) {
        error!(%e, "pipeline failed");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}

fn cmd_doctor() -> ExitCode {
    match wincrab_core::doctor::run_doctor() {
        Ok(()) => {
            println!("All checks passed.");
            ExitCode::SUCCESS
        }
        Err(e) => {
            error!(%e, "doctor check failed");
            ExitCode::FAILURE
        }
    }
}

fn cmd_inspect(iso: PathBuf, work_dir: PathBuf) -> ExitCode {
    match wincrab_core::inspect::inspect_iso(&iso, &work_dir) {
        Ok(output) => {
            println!("{output}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            error!(%e, "inspect failed");
            ExitCode::FAILURE
        }
    }
}

fn cmd_init(output: PathBuf) -> ExitCode {
    let config = wincrab_core::Config::default();
    match toml::to_string_pretty(&config) {
        Ok(toml_str) => {
            if let Err(e) = std::fs::write(&output, &toml_str) {
                error!(%e, path = %output.display(), "failed to write config");
                return ExitCode::FAILURE;
            }
            info!(path = %output.display(), "wrote default config");
            println!("Wrote default config to {}", output.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            error!(%e, "failed to serialize default config");
            ExitCode::FAILURE
        }
    }
}

fn cmd_validate(config_path: PathBuf) -> ExitCode {
    match wincrab_core::Config::from_file(&config_path) {
        Ok(_) => {
            println!("Config {} is valid.", config_path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Config validation failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_profiles() -> ExitCode {
    let profiles = &wincrab_core::profiles::PROFILE_NAMES;
    println!("Available profiles:\n");
    for name in profiles {
        println!("  {name}");
    }
    ExitCode::SUCCESS
}

fn cmd_completions(shell: clap_complete::Shell) -> ExitCode {
    let mut cmd = Cli::command();
    clap_complete::generate(shell, &mut cmd, "wincrab", &mut std::io::stdout());
    ExitCode::SUCCESS
}

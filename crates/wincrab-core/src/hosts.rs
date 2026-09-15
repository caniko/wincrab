use std::fmt::Write as _;
use std::path::Path;

use tracing::info;

use crate::config::Telemetry;
use crate::error::{Error, ensure_dir, write_file};

const TELEMETRY_DOMAINS: &[&str] = &[
    "vortex.data.microsoft.com",
    "vortex-win.data.microsoft.com",
    "telecommand.telemetry.microsoft.com",
    "telecommand.telemetry.microsoft.com.nsatc.net",
    "oca.telemetry.microsoft.com",
    "oca.telemetry.microsoft.com.nsatc.net",
    "sqm.telemetry.microsoft.com",
    "sqm.telemetry.microsoft.com.nsatc.net",
    "watson.telemetry.microsoft.com",
    "watson.telemetry.microsoft.com.nsatc.net",
    "redir.metaservices.microsoft.com",
    "choice.microsoft.com",
    "choice.microsoft.com.nsatc.net",
    "df.telemetry.microsoft.com",
    "reports.wes.df.telemetry.microsoft.com",
    "wes.df.telemetry.microsoft.com",
    "services.wes.df.telemetry.microsoft.com",
    "sqm.df.telemetry.microsoft.com",
    "telemetry.microsoft.com",
    "watson.ppe.telemetry.microsoft.com",
    "telemetry.appex.bing.net",
    "telemetry.urs.microsoft.com",
    "settings-sandbox.data.microsoft.com",
    "vsgallery.com",
    "watson.live.com",
    "watson.microsoft.com",
    "statsfe2.ws.microsoft.com",
    "corpext.msitadfs.glbdns2.microsoft.com",
    "compatexchange.cloudapp.net",
    "cs1.wpc.v0cdn.net",
    "a-0001.a-msedge.net",
    "statsfe2.update.microsoft.com.akadns.net",
    "diagnostics.support.microsoft.com",
    "corp.sts.microsoft.com",
    "statsfe1.ws.microsoft.com",
    "pre.footprintpredict.com",
    "i1.services.social.microsoft.com",
    "feedback.windows.com",
    "feedback.microsoft-hohm.com",
    "feedback.search.microsoft.com",
];

pub fn inject_telemetry_hosts(mount_dir: &Path, config: &Telemetry) -> Result<(), Error> {
    let builtin_count = if config.block_telemetry_hosts {
        TELEMETRY_DOMAINS.len()
    } else {
        0
    };
    let extra_count = config.extra_blocked_hosts.len();

    if builtin_count + extra_count == 0 {
        return Ok(());
    }

    let hosts_path = mount_dir
        .join("Windows")
        .join("System32")
        .join("drivers")
        .join("etc")
        .join("hosts");

    if let Some(parent) = hosts_path.parent() {
        ensure_dir(parent)?;
    }

    let total = builtin_count + extra_count;
    let mut entries = String::with_capacity(total * 30 + 80);
    entries.push_str("\n# --- wincrab telemetry block ---\n");

    if config.block_telemetry_hosts {
        for domain in TELEMETRY_DOMAINS {
            let _ = writeln!(entries, "0.0.0.0 {domain}");
        }
    }

    for domain in &config.extra_blocked_hosts {
        let _ = writeln!(entries, "0.0.0.0 {domain}");
    }

    entries.push_str("# --- end wincrab telemetry block ---\n");

    let mut existing = match std::fs::read_to_string(&hosts_path) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err(Error::Io {
                context: format!("reading {}", hosts_path.display()),
                source: e,
            });
        }
    };
    existing.push_str(&entries);

    write_file(&hosts_path, existing)?;

    info!(domains = total, "injected telemetry host blocks");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_when_disabled() {
        let dir = tempfile::tempdir().unwrap();
        let config = Telemetry {
            block_telemetry_hosts: false,
            ..Telemetry::default()
        };
        let result = inject_telemetry_hosts(dir.path(), &config);
        assert!(result.is_ok());
        let hosts = dir.path().join("Windows/System32/drivers/etc/hosts");
        assert!(!hosts.exists());
    }

    #[test]
    fn creates_hosts_with_known_domains() {
        let dir = tempfile::tempdir().unwrap();
        let config = Telemetry::default();
        inject_telemetry_hosts(dir.path(), &config).unwrap();

        let hosts = dir.path().join("Windows/System32/drivers/etc/hosts");
        let content = std::fs::read_to_string(&hosts).unwrap();
        assert!(content.contains("0.0.0.0 vortex.data.microsoft.com"));
        assert!(content.contains("0.0.0.0 telemetry.microsoft.com"));
        assert!(content.contains("0.0.0.0 feedback.windows.com"));
    }

    #[test]
    fn appends_extra_blocked_hosts() {
        let dir = tempfile::tempdir().unwrap();
        let config = Telemetry {
            extra_blocked_hosts: vec!["custom.tracker.example.com".into()],
            ..Telemetry::default()
        };
        inject_telemetry_hosts(dir.path(), &config).unwrap();

        let hosts = dir.path().join("Windows/System32/drivers/etc/hosts");
        let content = std::fs::read_to_string(&hosts).unwrap();
        assert!(content.contains("0.0.0.0 custom.tracker.example.com"));
    }

    #[test]
    fn appends_to_existing_hosts() {
        let dir = tempfile::tempdir().unwrap();
        let hosts_dir = dir.path().join("Windows/System32/drivers/etc");
        std::fs::create_dir_all(&hosts_dir).unwrap();
        std::fs::write(hosts_dir.join("hosts"), "127.0.0.1 localhost\n").unwrap();

        let config = Telemetry::default();
        inject_telemetry_hosts(dir.path(), &config).unwrap();

        let content = std::fs::read_to_string(hosts_dir.join("hosts")).unwrap();
        assert!(content.starts_with("127.0.0.1 localhost\n"));
        assert!(content.contains("0.0.0.0 telemetry.microsoft.com"));
    }

    #[test]
    fn extra_hosts_only_without_builtins() {
        let dir = tempfile::tempdir().unwrap();
        let config = Telemetry {
            block_telemetry_hosts: false,
            extra_blocked_hosts: vec!["custom.example.com".into()],
            ..Telemetry::default()
        };
        inject_telemetry_hosts(dir.path(), &config).unwrap();

        let hosts = dir.path().join("Windows/System32/drivers/etc/hosts");
        let content = std::fs::read_to_string(&hosts).unwrap();
        assert!(content.contains("0.0.0.0 custom.example.com"));
        assert!(!content.contains("vortex.data.microsoft.com"));
    }

    #[test]
    fn known_domains_count() {
        assert_eq!(TELEMETRY_DOMAINS.len(), 40);
    }
}

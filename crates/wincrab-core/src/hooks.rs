use std::process::Command;

use tracing::info;

use crate::error::{Error, run_cmd};

pub fn run_hook(
    hook_name: &str,
    script: &Option<String>,
    env_vars: &[(&str, &str)],
) -> Result<(), Error> {
    let script = match script {
        Some(s) => s,
        None => return Ok(()),
    };

    info!(hook = hook_name, script = script, "running hook");

    let mut cmd = Command::new("sh");
    cmd.arg("-c").arg(script);

    for (key, value) in env_vars {
        cmd.env(key, value);
    }

    run_cmd(&mut cmd)?;

    info!(hook = hook_name, "hook completed successfully");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_script_returns_ok() {
        let result = run_hook("test", &None, &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn successful_script_returns_ok() {
        let result = run_hook("test", &Some("true".into()), &[]);
        assert!(result.is_ok());
    }

    #[test]
    fn failing_script_returns_error() {
        let result = run_hook("test", &Some("false".into()), &[]);
        assert!(result.is_err());
    }

    #[test]
    fn env_vars_are_passed() {
        let result = run_hook(
            "test",
            &Some("test \"$WINCRAB_TEST_VAR\" = \"hello\"".into()),
            &[("WINCRAB_TEST_VAR", "hello")],
        );
        assert!(result.is_ok());
    }

    #[test]
    fn env_var_missing_causes_failure() {
        let result = run_hook(
            "test",
            &Some("test \"$WINCRAB_MISSING\" = \"expected\"".into()),
            &[],
        );
        assert!(result.is_err());
    }

    #[test]
    fn script_with_output() {
        let result = run_hook("test", &Some("echo hello >/dev/null".into()), &[]);
        assert!(result.is_ok());
    }
}

use serde_json::Value;
use std::env;
use std::path::Path;
use std::process::Command;

pub trait HostRunner {
    fn command_exists(&self, bin: &str) -> bool;
    fn run_json(&self, bin: &str, args: &[&str]) -> Result<Value, String>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemHostRunner;

impl HostRunner for SystemHostRunner {
    fn command_exists(&self, bin: &str) -> bool {
        if bin.contains('/') {
            return Path::new(bin).is_file();
        }
        env::var_os("PATH")
            .and_then(|paths| {
                env::split_paths(&paths)
                    .map(|path| path.join(bin))
                    .find(|candidate| candidate.is_file())
            })
            .is_some()
    }

    fn run_json(&self, bin: &str, args: &[&str]) -> Result<Value, String> {
        let output = Command::new(bin)
            .args(args)
            .output()
            .map_err(|error| format!("failed to execute {bin}: {error}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let detail = if stderr.is_empty() { stdout } else { stderr };
            return Err(format!(
                "{bin} exited with status {}: {detail}",
                output.status
            ));
        }
        serde_json::from_slice(&output.stdout)
            .map_err(|error| format!("invalid json from {bin}: {error}"))
    }
}

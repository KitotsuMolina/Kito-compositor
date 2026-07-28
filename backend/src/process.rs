use std::env;
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessOutput {
    pub stdout: String,
    pub stderr: String,
}

pub trait ProcessExecutor {
    fn command_exists(&self, binary: &str) -> bool;
    fn run(&self, binary: &str, args: &[String]) -> Result<ProcessOutput, String>;
    fn spawn(&self, binary: &str, args: &[String]) -> Result<u32, String>;
    fn run_foreground(&self, binary: &str, args: &[String]) -> Result<(), String> {
        self.run(binary, args).map(|_| ())
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemProcessExecutor;

impl ProcessExecutor for SystemProcessExecutor {
    fn command_exists(&self, binary: &str) -> bool {
        if binary.contains('/') {
            return Path::new(binary).is_file();
        }
        env::var_os("PATH")
            .and_then(|paths| {
                env::split_paths(&paths)
                    .map(|path| path.join(binary))
                    .find(|candidate| candidate.is_file())
            })
            .is_some()
    }

    fn run(&self, binary: &str, args: &[String]) -> Result<ProcessOutput, String> {
        let output = Command::new(binary)
            .args(args)
            .output()
            .map_err(|error| format!("failed to execute {binary}: {error}"))?;
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if !output.status.success() {
            let detail = if stderr.is_empty() { &stdout } else { &stderr };
            return Err(format!(
                "{binary} exited with status {}: {detail}",
                output.status
            ));
        }
        Ok(ProcessOutput { stdout, stderr })
    }

    fn spawn(&self, binary: &str, args: &[String]) -> Result<u32, String> {
        Command::new(binary)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map(|child| child.id())
            .map_err(|error| format!("failed to start {binary}: {error}"))
    }

    fn run_foreground(&self, binary: &str, args: &[String]) -> Result<(), String> {
        let status = Command::new(binary)
            .args(args)
            .status()
            .map_err(|error| format!("failed to execute {binary}: {error}"))?;
        if !status.success() {
            return Err(format!("{binary} exited with status {status}"));
        }
        Ok(())
    }
}

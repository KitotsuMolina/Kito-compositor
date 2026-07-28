use crate::ProcessExecutor;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WallpaperBackend {
    Awww,
    Swww,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WallpaperTransition {
    pub kind: String,
    pub fps: u32,
    pub duration: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub angle: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<String>,
}

impl Default for WallpaperTransition {
    fn default() -> Self {
        Self {
            kind: "simple".into(),
            fps: 60,
            duration: 0.7,
            angle: None,
            position: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WallpaperApplyRequest {
    pub namespace: String,
    pub output: String,
    pub image: PathBuf,
    pub transition: WallpaperTransition,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WallpaperRuntimeStatus {
    pub backend: WallpaperBackend,
    pub available: bool,
    pub running: bool,
    pub namespace: String,
}

pub struct WallpaperRuntime<E> {
    executor: E,
}

impl<E: ProcessExecutor> WallpaperRuntime<E> {
    pub fn new(executor: E) -> Self {
        Self { executor }
    }

    pub fn status(&self, namespace: &str) -> Result<WallpaperRuntimeStatus, String> {
        validate_identifier("namespace", namespace)?;
        let backend = self.resolve_backend()?;
        let running = self
            .executor
            .run(binary(backend), &query_args(backend, namespace))
            .is_ok();
        Ok(WallpaperRuntimeStatus {
            backend,
            available: true,
            running,
            namespace: namespace.into(),
        })
    }

    pub fn available_backend(&self) -> Option<WallpaperBackend> {
        self.resolve_backend().ok()
    }

    pub fn start(&self, namespace: &str) -> Result<WallpaperRuntimeStatus, String> {
        let status = self.status(namespace)?;
        if status.running {
            return Ok(status);
        }
        self.executor.spawn(
            daemon_binary(status.backend),
            &start_args(status.backend, namespace),
        )?;
        for _ in 0..10 {
            if self
                .executor
                .run(
                    binary(status.backend),
                    &query_args(status.backend, namespace),
                )
                .is_ok()
            {
                return Ok(WallpaperRuntimeStatus {
                    running: true,
                    ..status
                });
            }
            thread::sleep(Duration::from_millis(100));
        }
        Err(format!(
            "wallpaper runtime did not become ready for namespace {namespace}"
        ))
    }

    pub fn serve(&self, namespace: &str) -> Result<(), String> {
        validate_identifier("namespace", namespace)?;
        let backend = self.resolve_backend()?;
        self.executor
            .run_foreground(daemon_binary(backend), &start_args(backend, namespace))
    }

    pub fn stop(&self, namespace: &str) -> Result<WallpaperRuntimeStatus, String> {
        let current = self.status(namespace)?;
        if !current.running {
            return Ok(current);
        }
        self.executor.run(
            binary(current.backend),
            &kill_args(current.backend, namespace),
        )?;
        Ok(WallpaperRuntimeStatus {
            running: false,
            ..current
        })
    }

    pub fn apply(&self, request: &WallpaperApplyRequest) -> Result<WallpaperRuntimeStatus, String> {
        let request = validate_request(request)?;
        let status = self.start(&request.namespace)?;
        self.executor.run(
            binary(status.backend),
            &apply_args(status.backend, &request),
        )?;
        Ok(status)
    }

    fn resolve_backend(&self) -> Result<WallpaperBackend, String> {
        if self.executor.command_exists("awww") && self.executor.command_exists("awww-daemon") {
            return Ok(WallpaperBackend::Awww);
        }
        if self.executor.command_exists("swww") && self.executor.command_exists("swww-daemon") {
            return Ok(WallpaperBackend::Swww);
        }
        Err("no supported wallpaper runtime is installed (expected awww or swww)".into())
    }
}

fn validate_request(request: &WallpaperApplyRequest) -> Result<WallpaperApplyRequest, String> {
    validate_identifier("namespace", &request.namespace)?;
    validate_identifier("output", &request.output)?;
    validate_identifier("transition type", &request.transition.kind)?;
    if request.transition.fps == 0 || request.transition.fps > 240 {
        return Err("transition fps must be between 1 and 240".into());
    }
    if !request.transition.duration.is_finite()
        || request.transition.duration < 0.0
        || request.transition.duration > 60.0
    {
        return Err("transition duration must be between 0 and 60".into());
    }
    let image = canonical_image(&request.image)?;
    Ok(WallpaperApplyRequest {
        image,
        ..request.clone()
    })
}

fn canonical_image(path: &Path) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err("wallpaper image path must be absolute".into());
    }
    let canonical = std::fs::canonicalize(path)
        .map_err(|error| format!("failed to resolve wallpaper image: {error}"))?;
    if !canonical.is_file() {
        return Err("wallpaper image must be a regular file".into());
    }
    Ok(canonical)
}

fn validate_identifier(label: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
    {
        return Err(format!("invalid {label}: {value}"));
    }
    Ok(())
}

fn binary(backend: WallpaperBackend) -> &'static str {
    match backend {
        WallpaperBackend::Awww => "awww",
        WallpaperBackend::Swww => "swww",
    }
}

fn daemon_binary(backend: WallpaperBackend) -> &'static str {
    match backend {
        WallpaperBackend::Awww => "awww-daemon",
        WallpaperBackend::Swww => "swww-daemon",
    }
}

fn query_args(backend: WallpaperBackend, namespace: &str) -> Vec<String> {
    let mut args = vec!["query".into()];
    if backend == WallpaperBackend::Awww {
        args.push("--json".into());
    }
    args.extend(["--namespace".into(), namespace.into()]);
    args
}

fn start_args(backend: WallpaperBackend, namespace: &str) -> Vec<String> {
    let mut args = Vec::new();
    if backend == WallpaperBackend::Awww {
        args.extend(["--layer".into(), "background".into()]);
    }
    args.extend(["--namespace".into(), namespace.into()]);
    args
}

fn kill_args(_backend: WallpaperBackend, namespace: &str) -> Vec<String> {
    vec!["kill".into(), "--namespace".into(), namespace.into()]
}

fn apply_args(backend: WallpaperBackend, request: &WallpaperApplyRequest) -> Vec<String> {
    let mut args = vec![
        "img".into(),
        "--namespace".into(),
        request.namespace.clone(),
    ];
    args.push(if backend == WallpaperBackend::Awww {
        "--outputs".into()
    } else {
        "-o".into()
    });
    args.extend([
        request.output.clone(),
        request.image.to_string_lossy().into_owned(),
        "--transition-type".into(),
        request.transition.kind.clone(),
        "--transition-fps".into(),
        request.transition.fps.to_string(),
        "--transition-duration".into(),
        request.transition.duration.to_string(),
    ]);
    if let Some(angle) = request.transition.angle {
        args.extend(["--transition-angle".into(), angle.to_string()]);
    }
    if let Some(position) = &request.transition.position {
        args.extend(["--transition-pos".into(), position.clone()]);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProcessOutput;
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct FakeExecutor {
        commands: BTreeSet<String>,
        calls: RefCell<Vec<(String, Vec<String>)>>,
    }

    impl ProcessExecutor for FakeExecutor {
        fn command_exists(&self, binary: &str) -> bool {
            self.commands.contains(binary)
        }

        fn run(&self, binary: &str, args: &[String]) -> Result<ProcessOutput, String> {
            self.calls.borrow_mut().push((binary.into(), args.to_vec()));
            Ok(ProcessOutput {
                stdout: String::new(),
                stderr: String::new(),
            })
        }

        fn spawn(&self, binary: &str, args: &[String]) -> Result<u32, String> {
            self.calls.borrow_mut().push((binary.into(), args.to_vec()));
            Ok(42)
        }
    }

    #[test]
    fn applies_with_exact_awww_arguments_and_no_shell() {
        let root = std::env::temp_dir().join(format!(
            "compositor-wallpaper-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let image = root.join("wallpaper.png");
        std::fs::write(&image, b"image").unwrap();
        let executor = FakeExecutor {
            commands: ["awww".into(), "awww-daemon".into()].into_iter().collect(),
            ..Default::default()
        };
        let runtime = WallpaperRuntime::new(executor);
        runtime
            .apply(&WallpaperApplyRequest {
                namespace: "kitowall".into(),
                output: "DP-1".into(),
                image: image.clone(),
                transition: WallpaperTransition {
                    duration: 0.0,
                    angle: Some(45.5),
                    ..WallpaperTransition::default()
                },
            })
            .unwrap();
        let calls = runtime.executor.calls.borrow();
        let apply = calls.last().unwrap();
        assert_eq!(apply.0, "awww");
        assert_eq!(apply.1[0], "img");
        assert!(apply.1.contains(&"--outputs".into()));
        assert!(apply.1.contains(&"DP-1".into()));
        assert!(apply.1.contains(&image.to_string_lossy().into_owned()));
        assert!(apply.1.contains(&"45.5".into()));
        assert!(
            apply
                .1
                .windows(2)
                .any(|pair| { pair == ["--transition-duration".to_owned(), "0".to_owned()] })
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_relative_paths_and_unsafe_namespaces() {
        let request = WallpaperApplyRequest {
            namespace: "bad namespace".into(),
            output: "DP-1".into(),
            image: PathBuf::from("wallpaper.png"),
            transition: WallpaperTransition::default(),
        };
        assert!(validate_request(&request).is_err());
    }

    #[test]
    fn serves_the_runtime_in_the_foreground() {
        let runtime = WallpaperRuntime::new(FakeExecutor {
            commands: ["awww".into(), "awww-daemon".into()].into_iter().collect(),
            ..Default::default()
        });
        runtime.serve("kitowall").unwrap();
        let calls = runtime.executor.calls.borrow();
        assert_eq!(calls[0].0, "awww-daemon");
        assert_eq!(
            calls[0].1,
            ["--layer", "background", "--namespace", "kitowall"]
        );
    }
}

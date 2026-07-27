use crate::provider;
use crate::{
    Capabilities, CompositorKind, Detection, DoctorCheck, DoctorReport, HostRunner, Output, Status,
};

pub struct CompositorBackend<R> {
    runner: R,
}

impl<R: HostRunner> CompositorBackend<R> {
    pub fn new(runner: R) -> Self {
        Self { runner }
    }

    pub fn detect(&self) -> Detection {
        self.detect_with_hints(
            env_present("HYPRLAND_INSTANCE_SIGNATURE"),
            env_present("NIRI_SOCKET"),
        )
    }

    pub fn outputs(&self) -> Result<(CompositorKind, Vec<Output>), String> {
        let detection = self.detect();
        if !detection.ok {
            return Err(detection.reason);
        }
        provider::outputs(detection.compositor, &self.runner)
            .map(|outputs| (detection.compositor, outputs))
    }

    pub fn focused_output(&self) -> Result<(CompositorKind, Option<Output>), String> {
        let (kind, outputs) = self.outputs()?;
        Ok((kind, outputs.into_iter().find(|output| output.focused)))
    }

    pub fn validate_output(&self, name: &str) -> Result<(CompositorKind, bool), String> {
        if name.trim().is_empty() {
            return Err("output name cannot be empty".into());
        }
        let (kind, outputs) = self.outputs()?;
        Ok((kind, outputs.iter().any(|output| output.name == name)))
    }

    pub fn status(&self) -> Result<Status, String> {
        let detection = self.detect();
        if !detection.ok {
            return Err(detection.reason);
        }
        let outputs = provider::outputs(detection.compositor, &self.runner)?;
        let focused_output = outputs.iter().find(|output| output.focused).cloned();
        Ok(Status {
            ok: true,
            detection,
            outputs,
            focused_output,
        })
    }

    pub fn capabilities(&self) -> Capabilities {
        let detection = self.detect();
        Capabilities {
            compositor: detection.compositor,
            multi_output: detection.ok,
            output_focus: detection.ok,
            output_events: detection.ok,
            focus_events: detection.ok,
            wallpaper_runtime: false,
            service_runtime: false,
        }
    }

    pub fn doctor(&self) -> DoctorReport {
        let detection = self.detect();
        let mut checks = vec![DoctorCheck {
            id: "provider".into(),
            ok: detection.ok,
            severity: if detection.ok { "info" } else { "error" }.into(),
            message: detection.reason.clone(),
            remediation: (!detection.ok).then(|| {
                "Start a supported compositor session and ensure its IPC command is available"
                    .into()
            }),
        }];
        if detection.ok {
            let result = provider::outputs(detection.compositor, &self.runner);
            checks.push(DoctorCheck {
                id: "outputs".into(),
                ok: result.is_ok(),
                severity: if result.is_ok() { "info" } else { "error" }.into(),
                message: match result {
                    Ok(outputs) => format!("{} output(s) detected", outputs.len()),
                    Err(error) => error,
                },
                remediation: None,
            });
        }
        DoctorReport {
            healthy: checks.iter().all(|check| check.ok),
            checks,
        }
    }

    fn detect_with_hints(&self, hyprland_session: bool, niri_session: bool) -> Detection {
        if hyprland_session && provider::available(CompositorKind::Hyprland, &self.runner) {
            return detected(
                CompositorKind::Hyprland,
                "hyprctl",
                "Hyprland session is active",
            );
        }
        if niri_session && provider::available(CompositorKind::Niri, &self.runner) {
            return detected(CompositorKind::Niri, "niri msg", "Niri session is active");
        }
        if provider::available(CompositorKind::Hyprland, &self.runner) {
            return detected(
                CompositorKind::Hyprland,
                "hyprctl",
                "hyprctl probe succeeded",
            );
        }
        if provider::available(CompositorKind::Niri, &self.runner) {
            return detected(CompositorKind::Niri, "niri msg", "niri probe succeeded");
        }
        Detection {
            ok: false,
            compositor: CompositorKind::Unknown,
            backend: None,
            reason: "no supported compositor provider responded".into(),
        }
    }
}

fn detected(kind: CompositorKind, backend: &str, reason: &str) -> Detection {
    Detection {
        ok: true,
        compositor: kind,
        backend: Some(backend.into()),
        reason: reason.into(),
    }
}

fn env_present(key: &str) -> bool {
    std::env::var(key).is_ok_and(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::collections::{BTreeMap, BTreeSet};

    #[derive(Default)]
    struct FakeRunner {
        commands: BTreeSet<String>,
        responses: BTreeMap<String, Value>,
    }

    impl FakeRunner {
        fn with(mut self, bin: &str, response: Value) -> Self {
            self.commands.insert(bin.into());
            self.responses.insert(bin.into(), response);
            self
        }
    }

    impl HostRunner for FakeRunner {
        fn command_exists(&self, bin: &str) -> bool {
            self.commands.contains(bin)
        }

        fn run_json(&self, bin: &str, _args: &[&str]) -> Result<Value, String> {
            self.responses
                .get(bin)
                .cloned()
                .ok_or_else(|| format!("no fake response for {bin}"))
        }
    }

    #[test]
    fn session_hint_selects_niri_when_both_tools_exist() {
        let runner = FakeRunner::default()
            .with("hyprctl", serde_json::json!([]))
            .with("niri", serde_json::json!({"outputs": [{"name": "eDP-1"}]}));
        let backend = CompositorBackend::new(runner);
        let detection = backend.detect_with_hints(false, true);
        assert_eq!(detection.compositor, CompositorKind::Niri);
    }

    #[test]
    fn exposes_the_same_output_model_for_consumers() {
        let runner = FakeRunner::default().with(
            "hyprctl",
            serde_json::json!([{"name": "DP-1", "width": 1920, "height": 1080}]),
        );
        let backend = CompositorBackend::new(runner);
        let (_, outputs) = backend.outputs().unwrap();
        assert_eq!(outputs[0].name, "DP-1");
        assert_eq!(outputs[0].backend, "hyprctl");
    }
}

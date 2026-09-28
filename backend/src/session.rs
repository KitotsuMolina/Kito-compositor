//! Only the allowlisted graphical environment is exposed, never the manager's full environment.
use crate::ProcessExecutor;
use serde_json::{Value, json};
pub const GRAPHICAL_KEYS: &[&str] = &[
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "XDG_RUNTIME_DIR",
    "XDG_SESSION_TYPE",
    "XDG_CURRENT_DESKTOP",
    "NIRI_SOCKET",
    "HYPRLAND_INSTANCE_SIGNATURE",
    "DBUS_SESSION_BUS_ADDRESS",
];
pub fn graphical_session<E: ProcessExecutor>(executor: &E) -> Result<Value, String> {
    if !executor.command_exists("systemctl") || !executor.command_exists("busctl") {
        return Err(
            "session context requires the systemd-user adapter (systemctl and busctl)".into(),
        );
    }
    if executor
        .run(
            "systemctl",
            &[
                "--user".into(),
                "is-active".into(),
                "graphical-session.target".into(),
            ],
        )
        .is_err()
    {
        return Ok(
            json!({"ready":false,"reason":"graphical_session_inactive_or_unavailable","environment":{}}),
        );
    }
    let response = executor.run(
        "busctl",
        &[
            "--user",
            "--timeout=2",
            "--json=short",
            "get-property",
            "org.freedesktop.systemd1",
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
            "Environment",
        ]
        .map(str::to_owned),
    )?;
    let value: Value = serde_json::from_str(&response.stdout)
        .map_err(|_| "invalid session environment response")?;
    let values = value["data"]
        .as_array()
        .filter(|_| value["type"] == "as")
        .ok_or("invalid session environment type")?;
    let mut environment = serde_json::Map::new();
    for entry in values {
        let Some((key, value)) = entry.as_str().and_then(|s| s.split_once('=')) else {
            continue;
        };
        if GRAPHICAL_KEYS.contains(&key) && !value.is_empty() && !value.contains(['\0', '\n', '\r'])
        {
            environment.insert(key.into(), json!(value));
        }
    }
    let ready = environment.contains_key("WAYLAND_DISPLAY")
        && environment
            .get("XDG_RUNTIME_DIR")
            .and_then(Value::as_str)
            .is_some_and(|s| s.starts_with('/'));
    Ok(
        json!({"ready":ready,"reason":if ready {"ready"}else{"graphical_environment_missing"},"environment":environment,"manager":"systemd-user"}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProcessOutput;
    struct Executor(bool);
    impl ProcessExecutor for Executor {
        fn command_exists(&self, _: &str) -> bool {
            true
        }
        fn run(&self, b: &str, _: &[String]) -> Result<ProcessOutput, String> {
            if b == "systemctl" && !self.0 {
                return Err("inactive".into());
            }
            Ok(ProcessOutput{stdout:json!({"type":"as","data":["WAYLAND_DISPLAY=wayland-1","XDG_RUNTIME_DIR=/run/user/1000","NIRI_SOCKET=/path with spaces","API_TOKEN=secret","PATH=/untrusted"]}).to_string(),stderr:String::new()})
        }
        fn spawn(&self, _: &str, _: &[String]) -> Result<u32, String> {
            unreachable!()
        }
    }
    #[test]
    fn inactive_session_is_waitable_and_only_graphical_keys_are_exposed() {
        assert_eq!(graphical_session(&Executor(false)).unwrap()["ready"], false);
        let data = graphical_session(&Executor(true)).unwrap();
        assert_eq!(data["ready"], true);
        assert_eq!(data["environment"].as_object().unwrap().len(), 3);
        assert_eq!(data["environment"]["NIRI_SOCKET"], "/path with spaces");
    }
}

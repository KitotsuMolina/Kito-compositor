use super::{bool_value, f64_value, first, nested, string, u32_value};
use crate::{ApplicationWindow, HostRunner, Output};
use serde_json::{Map, Value};

pub(super) fn available<R: HostRunner>(runner: &R) -> bool {
    runner.command_exists("niri")
        && runner
            .run_json("niri", &["msg", "--json", "outputs"])
            .is_ok()
}

pub(super) fn outputs<R: HostRunner>(runner: &R) -> Result<Vec<Output>, String> {
    let mut outputs = parse(runner.run_json("niri", &["msg", "--json", "outputs"])?)?;
    let focused = runner.run_json("niri", &["msg", "--json", "focused-output"])?;
    let name = if focused.is_null() {
        None
    } else {
        Some(
            focused
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| "invalid niri focused-output response".to_string())?,
        )
    };
    for output in &mut outputs {
        output.focused = output.active && name == Some(output.name.as_str());
    }
    Ok(outputs)
}

pub(super) fn windows<R: HostRunner>(runner: &R) -> Result<Vec<ApplicationWindow>, String> {
    let mut windows = parse_windows(runner.run_json("niri", &["msg", "--json", "windows"])?)?;
    // Workspace metadata is optional during compositor shutdown/hotplug.
    if let Ok(Value::Array(workspaces)) = runner.run_json("niri", &["msg", "--json", "workspaces"])
    {
        for window in &mut windows {
            if window.output.is_none() {
                window.output = workspaces
                    .iter()
                    .find(|workspace| {
                        workspace
                            .get("id")
                            .and_then(Value::as_u64)
                            .map(|id| id.to_string())
                            == window.workspace
                            && window.workspace.is_some()
                    })
                    .and_then(|workspace| string(workspace.get("output")));
            }
        }
    }
    Ok(windows)
}

fn parse(value: Value) -> Result<Vec<Output>, String> {
    let objects: Vec<&Map<String, Value>> = match &value {
        Value::Object(map) if map.contains_key("outputs") => {
            return parse(map["outputs"].clone());
        }
        Value::Object(map) => map
            .values()
            .map(|entry| {
                entry
                    .as_object()
                    .ok_or_else(|| "niri output is not an object".to_string())
            })
            .collect::<Result<_, _>>()?,
        Value::Array(items) => items
            .iter()
            .map(|entry| {
                entry
                    .as_object()
                    .ok_or_else(|| "niri output is not an object".to_string())
            })
            .collect::<Result<_, _>>()?,
        _ => return Err("niri outputs must be an object or array".into()),
    };
    objects.into_iter().map(parse_object).collect()
}

fn parse_windows(value: Value) -> Result<Vec<ApplicationWindow>, String> {
    let items = value
        .as_array()
        .or_else(|| value.get("windows").and_then(Value::as_array))
        .ok_or_else(|| "niri windows did not return a json array".to_string())?;
    items
        .iter()
        .map(|item| {
            let object = item
                .as_object()
                .ok_or_else(|| "niri window entry is not an object".to_string())?;
            Ok(ApplicationWindow {
                // Some Wayland clients have no app_id. Keep their PID/title and
                // other windows instead of failing the entire catalog.
                app_id: string(first(object, &["app_id"])).unwrap_or_default(),
                title: string(first(object, &["title"])),
                pid: u32_value(first(object, &["pid"])),
                output: string(first(object, &["output"])),
                workspace: first(object, &["workspace_id"])
                    .and_then(Value::as_u64)
                    .map(|value| value.to_string()),
                focused: bool_value(first(object, &["is_focused", "focused"])).unwrap_or(false),
                fullscreen: bool_value(first(object, &["is_fullscreen", "fullscreen"]))
                    .unwrap_or(false),
                backend: "niri msg".into(),
            })
        })
        .collect()
}

fn parse_object(object: &Map<String, Value>) -> Result<Output, String> {
    let wrapped = Value::Object(object.clone());
    let name = string(first(
        object,
        &["name", "output", "connector", "connector_name"],
    ))
    .or_else(|| string(nested(&wrapped, &["output", "name"])))
    .ok_or_else(|| "niri output is missing its name".to_string())?;
    let indexed_mode = object.contains_key("modes");
    let mode = if indexed_mode {
        match object.get("current_mode") {
            None | Some(Value::Null) => None,
            Some(index) => {
                let index = index
                    .as_u64()
                    .and_then(|index| usize::try_from(index).ok())
                    .ok_or_else(|| format!("invalid current_mode for {name}"))?;
                Some(
                    object
                        .get("modes")
                        .and_then(Value::as_array)
                        .and_then(|modes| modes.get(index))
                        .filter(|mode| mode.is_object())
                        .ok_or_else(|| format!("current_mode out of range for {name}"))?,
                )
            }
        }
    } else {
        object.get("current_mode").filter(|mode| mode.is_object())
    };
    let refresh_hz = if indexed_mode {
        f64_value(mode.and_then(|mode| mode.get("refresh_rate"))).map(|value| value / 1000.0)
    } else {
        f64_value(first(object, &["refresh_hz", "refresh_rate"]))
            .or_else(|| f64_value(nested(&wrapped, &["current_mode", "refresh_rate"])))
            .or_else(|| {
                f64_value(nested(&wrapped, &["current_mode", "refresh_rate_millihz"]))
                    .map(|value| value / 1000.0)
            })
    };
    Ok(Output {
        name,
        make: string(first(object, &["make", "manufacturer"])),
        model: string(first(object, &["model"])),
        serial: string(first(object, &["serial"])),
        width: u32_value(mode.and_then(|mode| mode.get("width")))
            .or_else(|| u32_value(first(object, &["width"])))
            .or_else(|| u32_value(nested(&wrapped, &["current_mode", "width"])))
            .or_else(|| u32_value(nested(&wrapped, &["mode", "width"]))),
        height: u32_value(mode.and_then(|mode| mode.get("height")))
            .or_else(|| u32_value(first(object, &["height"])))
            .or_else(|| u32_value(nested(&wrapped, &["current_mode", "height"])))
            .or_else(|| u32_value(nested(&wrapped, &["mode", "height"]))),
        scale: f64_value(first(object, &["scale"]))
            .or_else(|| f64_value(nested(&wrapped, &["logical", "scale"]))),
        refresh_hz,
        focused: bool_value(first(object, &["focused", "is_focused"]))
            .or_else(|| bool_value(nested(&wrapped, &["focus", "is_focused"])))
            .unwrap_or(false),
        active: if indexed_mode {
            mode.is_some()
        } else {
            bool_value(first(object, &["active", "is_active", "enabled"])).unwrap_or(true)
        },
        backend: "niri msg".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn real_output() -> Value {
        serde_json::json!({"eDP-1": {
            "name": "eDP-1", "make": "Test", "model": "Panel",
            "modes": [
                {"width": 1920, "height": 1080, "refresh_rate": 60002},
                {"width": 2560, "height": 1440, "refresh_rate": 144000}
            ],
            "current_mode": 1,
            "logical": {"width": 1280, "height": 720, "scale": 2.0}
        }})
    }

    #[test]
    fn parses_indexed_modes_without_confusing_logical_dimensions() {
        let result = parse(real_output()).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].width, Some(2560));
        assert_eq!(result[0].height, Some(1440));
        assert_eq!(result[0].refresh_hz, Some(144.0));
        assert_eq!(result[0].scale, Some(2.0));
        assert!(result[0].active);
    }

    #[test]
    fn disabled_and_empty_outputs_are_valid() {
        let mut value = real_output();
        value["eDP-1"]["current_mode"] = Value::Null;
        value["eDP-1"]["logical"] = Value::Null;
        let result = parse(value).unwrap();
        assert!(!result[0].active);
        assert_eq!(result[0].width, None);
        assert_eq!(result[0].refresh_hz, None);
        assert!(parse(serde_json::json!({})).unwrap().is_empty());
    }

    #[test]
    fn malformed_modes_and_responses_are_rejected() {
        for invalid in [
            serde_json::json!(99),
            serde_json::json!(-1),
            serde_json::json!("0"),
        ] {
            let mut value = real_output();
            value["eDP-1"]["current_mode"] = invalid;
            assert!(parse(value).is_err());
        }
        assert!(parse(Value::Null).is_err());
        assert!(parse(serde_json::json!({"unexpected": 2})).is_err());
    }

    struct IpcRunner {
        focus: Value,
    }

    impl HostRunner for IpcRunner {
        fn command_exists(&self, bin: &str) -> bool {
            bin == "niri"
        }
        fn run_json(&self, bin: &str, args: &[&str]) -> Result<Value, String> {
            assert_eq!(bin, "niri");
            assert_eq!(&args[..2], &["msg", "--json"]);
            match args[2] {
                "outputs" => Ok(real_output()),
                "focused-output" => Ok(self.focus.clone()),
                "windows" => Ok(serde_json::json!([
                    {"app_id": null, "pid": 10, "workspace_id": 2},
                    {"app_id": "example", "workspace_id": 2, "is_focused": true}
                ])),
                "workspaces" => Ok(serde_json::json!([{"id": 2, "output": "eDP-1"}])),
                command => Err(format!("unexpected command {command}")),
            }
        }
    }

    #[test]
    fn queries_focus_separately_and_accepts_no_focused_output() {
        let runner = IpcRunner {
            focus: serde_json::json!({"name": "eDP-1"}),
        };
        assert!(outputs(&runner).unwrap()[0].focused);
        let runner = IpcRunner { focus: Value::Null };
        assert!(!outputs(&runner).unwrap()[0].focused);
        let runner = IpcRunner {
            focus: serde_json::json!({"name": "removed"}),
        };
        assert!(!outputs(&runner).unwrap()[0].focused);
    }

    #[test]
    fn windows_without_app_ids_do_not_hide_other_applications() {
        let result = windows(&IpcRunner { focus: Value::Null }).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result[0].app_id.is_empty());
        assert_eq!(result[0].pid, Some(10));
        assert_eq!(result[1].output.as_deref(), Some("eDP-1"));
        assert!(result[1].focused);
    }

    #[test]
    fn normalizes_nested_niri_outputs() {
        let outputs = parse(serde_json::json!({"outputs": [{
            "name": "eDP-1", "current_mode": {
                "width": 2880, "height": 1800, "refresh_rate_millihz": 120000
            }, "scale": 2.0, "is_focused": true
        }]}))
        .unwrap();
        assert_eq!(outputs[0].name, "eDP-1");
        assert_eq!(outputs[0].refresh_hz, Some(120.0));
        assert!(outputs[0].focused);
    }

    #[test]
    fn normalizes_niri_windows() {
        let windows = parse_windows(serde_json::json!([{
            "app_id": "org.example.Game",
            "title": "Game",
            "pid": 84,
            "workspace_id": 3,
            "is_focused": true,
            "is_fullscreen": true
        }]))
        .unwrap();
        assert_eq!(windows[0].workspace.as_deref(), Some("3"));
        assert!(windows[0].fullscreen);
    }
}

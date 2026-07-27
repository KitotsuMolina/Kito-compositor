use super::{bool_value, f64_value, first, string, u32_value};
use crate::{HostRunner, Output};
use serde_json::Value;

pub(super) fn available<R: HostRunner>(runner: &R) -> bool {
    runner.command_exists("hyprctl") && runner.run_json("hyprctl", &["-j", "monitors"]).is_ok()
}

pub(super) fn outputs<R: HostRunner>(runner: &R) -> Result<Vec<Output>, String> {
    parse(runner.run_json("hyprctl", &["-j", "monitors"])?)
}

fn parse(value: Value) -> Result<Vec<Output>, String> {
    let items = value
        .as_array()
        .ok_or_else(|| "hyprctl monitors did not return a json array".to_string())?;
    items
        .iter()
        .map(|item| {
            let object = item
                .as_object()
                .ok_or_else(|| "hyprctl monitor entry is not an object".to_string())?;
            Ok(Output {
                name: string(first(object, &["name"]))
                    .ok_or_else(|| "hyprctl monitor entry missing name".to_string())?,
                make: string(first(object, &["make"])),
                model: string(first(object, &["model"])),
                serial: string(first(object, &["serial"])),
                width: u32_value(first(object, &["width"])),
                height: u32_value(first(object, &["height"])),
                scale: f64_value(first(object, &["scale"])),
                refresh_hz: f64_value(first(object, &["refreshRate", "refresh_rate"])),
                focused: bool_value(first(object, &["focused"])).unwrap_or(false),
                active: bool_value(first(object, &["active", "enabled"])).unwrap_or(true),
                backend: "hyprctl".into(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_hyprland_outputs() {
        let outputs = parse(serde_json::json!([{
            "name": "DP-1", "make": "LG", "width": 2560, "height": 1440,
            "scale": 1.0, "refreshRate": 144.0, "focused": true
        }]))
        .unwrap();
        assert_eq!(outputs[0].name, "DP-1");
        assert_eq!(outputs[0].width, Some(2560));
        assert!(outputs[0].focused);
    }
}

use super::{bool_value, f64_value, first, nested, string, u32_value};
use crate::{HostRunner, Output};
use serde_json::{Map, Value};

pub(super) fn available<R: HostRunner>(runner: &R) -> bool {
    runner.command_exists("niri")
        && runner
            .run_json("niri", &["msg", "--json", "outputs"])
            .is_ok()
}

pub(super) fn outputs<R: HostRunner>(runner: &R) -> Result<Vec<Output>, String> {
    parse(runner.run_json("niri", &["msg", "--json", "outputs"])?)
}

fn parse(value: Value) -> Result<Vec<Output>, String> {
    let mut objects = Vec::new();
    collect(&value, &mut objects);
    let mut outputs = Vec::new();
    for object in objects {
        if let Some(output) = parse_object(object)
            && !outputs.iter().any(|item: &Output| item.name == output.name)
        {
            outputs.push(output);
        }
    }
    if outputs.is_empty() {
        return Err("niri outputs json did not expose any recognizable outputs".into());
    }
    Ok(outputs)
}

fn collect<'a>(value: &'a Value, outputs: &mut Vec<&'a Map<String, Value>>) {
    match value {
        Value::Array(items) => {
            for item in items {
                if let Some(object) = item.as_object()
                    && looks_like_output(object)
                {
                    outputs.push(object);
                }
                collect(item, outputs);
            }
        }
        Value::Object(object) => {
            if looks_like_output(object) {
                outputs.push(object);
            }
            for item in object.values() {
                collect(item, outputs);
            }
        }
        _ => {}
    }
}

fn looks_like_output(object: &Map<String, Value>) -> bool {
    first(object, &["name", "output", "connector", "connector_name"]).is_some()
        || nested(&Value::Object(object.clone()), &["current_mode", "width"]).is_some()
}

fn parse_object(object: &Map<String, Value>) -> Option<Output> {
    let wrapped = Value::Object(object.clone());
    let name = string(first(
        object,
        &["name", "output", "connector", "connector_name"],
    ))
    .or_else(|| string(nested(&wrapped, &["output", "name"])))?;
    let refresh_hz = f64_value(first(object, &["refresh_hz", "refresh_rate"]))
        .or_else(|| f64_value(nested(&wrapped, &["current_mode", "refresh_rate"])))
        .or_else(|| {
            f64_value(nested(&wrapped, &["current_mode", "refresh_rate_millihz"]))
                .map(|value| value / 1000.0)
        });
    Some(Output {
        name,
        make: string(first(object, &["make", "manufacturer"])),
        model: string(first(object, &["model"])),
        serial: string(first(object, &["serial"])),
        width: u32_value(first(object, &["width"]))
            .or_else(|| u32_value(nested(&wrapped, &["current_mode", "width"])))
            .or_else(|| u32_value(nested(&wrapped, &["mode", "width"]))),
        height: u32_value(first(object, &["height"]))
            .or_else(|| u32_value(nested(&wrapped, &["current_mode", "height"])))
            .or_else(|| u32_value(nested(&wrapped, &["mode", "height"]))),
        scale: f64_value(first(object, &["scale"]))
            .or_else(|| f64_value(nested(&wrapped, &["logical", "scale"]))),
        refresh_hz,
        focused: bool_value(first(object, &["focused", "is_focused"]))
            .or_else(|| bool_value(nested(&wrapped, &["focus", "is_focused"])))
            .unwrap_or(false),
        active: bool_value(first(object, &["active", "is_active", "enabled"])).unwrap_or(true),
        backend: "niri msg".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

mod hyprland;
mod niri;

use crate::{CompositorKind, HostRunner, Output};
use serde_json::{Map, Value};

pub(crate) fn available<R: HostRunner>(kind: CompositorKind, runner: &R) -> bool {
    match kind {
        CompositorKind::Hyprland => hyprland::available(runner),
        CompositorKind::Niri => niri::available(runner),
        CompositorKind::Unknown => false,
    }
}

pub(crate) fn outputs<R: HostRunner>(
    kind: CompositorKind,
    runner: &R,
) -> Result<Vec<Output>, String> {
    match kind {
        CompositorKind::Hyprland => hyprland::outputs(runner),
        CompositorKind::Niri => niri::outputs(runner),
        CompositorKind::Unknown => Err("no supported compositor provider responded".into()),
    }
}

fn first<'a>(object: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| object.get(*key))
}

fn nested<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut current = value;
    for segment in path {
        current = current.get(*segment)?;
    }
    Some(current)
}

fn string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn u32_value(value: Option<&Value>) -> Option<u32> {
    value.and_then(|entry| {
        entry
            .as_u64()
            .and_then(|raw| u32::try_from(raw).ok())
            .or_else(|| {
                entry
                    .as_i64()
                    .filter(|raw| *raw >= 0)
                    .and_then(|raw| u32::try_from(raw).ok())
            })
    })
}

fn f64_value(value: Option<&Value>) -> Option<f64> {
    value.and_then(|entry| {
        entry
            .as_f64()
            .or_else(|| entry.as_i64().map(|raw| raw as f64))
            .or_else(|| entry.as_u64().map(|raw| raw as f64))
    })
}

fn bool_value(value: Option<&Value>) -> Option<bool> {
    value.and_then(Value::as_bool)
}

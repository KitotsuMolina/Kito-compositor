use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositorKind {
    Hyprland,
    Niri,
    Unknown,
}

impl CompositorKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hyprland => "hyprland",
            Self::Niri => "niri",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Detection {
    pub ok: bool,
    pub compositor: CompositorKind,
    pub backend: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Output {
    pub name: String,
    pub make: Option<String>,
    pub model: Option<String>,
    pub serial: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub scale: Option<f64>,
    pub refresh_hz: Option<f64>,
    pub focused: bool,
    pub active: bool,
    pub backend: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationWindow {
    pub app_id: String,
    pub title: Option<String>,
    pub pid: Option<u32>,
    pub output: Option<String>,
    pub workspace: Option<String>,
    pub focused: bool,
    pub fullscreen: bool,
    pub backend: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledApplication {
    pub id: String,
    pub name: String,
    pub executable: Option<String>,
    pub icon: Option<String>,
    pub desktop_file: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunningApplication {
    pub id: String,
    pub name: String,
    pub app_id: String,
    pub pid: Option<u32>,
    pub title: Option<String>,
    pub focused: bool,
    pub fullscreen: bool,
    pub backend: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub ok: bool,
    pub detection: Detection,
    pub outputs: Vec<Output>,
    pub focused_output: Option<Output>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Capabilities {
    pub compositor: CompositorKind,
    pub multi_output: bool,
    pub output_focus: bool,
    pub output_events: bool,
    pub focus_events: bool,
    pub application_catalog: bool,
    pub application_runtime: bool,
    pub wallpaper_runtime: bool,
    pub service_runtime: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DoctorCheck {
    pub id: String,
    pub ok: bool,
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remediation: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DoctorReport {
    pub healthy: bool,
    pub checks: Vec<DoctorCheck>,
}

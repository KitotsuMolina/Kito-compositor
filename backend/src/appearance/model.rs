use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceMode {
    NativePalette,
    Exact,
    Nearest,
    GeneratedConfig,
    ReadOnly,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppearanceCapabilities {
    pub supported: bool,
    pub backend: String,
    pub mode: AppearanceMode,
    pub palette_supported: bool,
    pub preview_supported: bool,
    pub apply_supported: bool,
    pub restore_supported: bool,
    pub scopes: Vec<String>,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub advisories: Vec<AppearanceAdvisory>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppearanceAdvisory {
    pub code: String,
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaletteCandidate {
    pub color: String,
    pub weight: f64,
    pub luminance: f64,
    pub saturation: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WallpaperPalette {
    pub dominant: String,
    pub vibrant: String,
    pub muted: String,
    pub accent_light: String,
    pub accent_mid: String,
    pub accent_dark: String,
    pub foreground: String,
    pub background_luminance: f64,
    pub candidates: Vec<PaletteCandidate>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppearancePreview {
    pub image: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_output: Option<String>,
    pub cache_hit: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_warning: Option<String>,
    pub palette: WallpaperPalette,
    pub provider: AppearanceCapabilities,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppearanceCurrent {
    pub active: bool,
    pub provider: AppearanceCapabilities,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub palette: Option<WallpaperPalette>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppearanceOperation {
    pub binary: String,
    pub args: Vec<String>,
    pub mutates: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaelestiaSnapshot {
    pub name: String,
    pub flavour: String,
    pub mode: String,
    pub variant: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wallpaper: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppearanceState {
    pub schema_version: u8,
    pub backend: String,
    pub image: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_output: Option<String>,
    pub palette: WallpaperPalette,
    pub previous_caelestia: CaelestiaSnapshot,
    pub applied_at_unix_ms: u128,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppearancePlan {
    pub provider: AppearanceCapabilities,
    pub image: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_output: Option<String>,
    pub palette: WallpaperPalette,
    pub operations: Vec<AppearanceOperation>,
    pub requires_confirmation: bool,
    pub may_run_caelestia_post_hook: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppearanceApplyResult {
    pub applied: bool,
    pub dry_run: bool,
    pub plan: AppearancePlan,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<AppearanceState>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppearanceRestoreResult {
    pub restored: bool,
    pub dry_run: bool,
    pub provider: AppearanceCapabilities,
    pub operations: Vec<AppearanceOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppearancePolicy {
    pub schema_version: u8,
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_output: Option<String>,
    pub automatic_apply: bool,
    pub post_hook_consent: bool,
    pub updated_at_unix_ms: u128,
}

impl Default for AppearancePolicy {
    fn default() -> Self {
        Self {
            schema_version: 1,
            enabled: false,
            source_output: None,
            automatic_apply: false,
            post_hook_consent: false,
            updated_at_unix_ms: 0,
        }
    }
}

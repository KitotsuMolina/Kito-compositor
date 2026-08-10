use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const MANAGED_CONFIG: &[u8] =
    b"# Managed by Kitsune Compositor for KiSDDM\n[Theme]\nCurrent=kisddm\n";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SddmCapabilities {
    pub sddm_installed: bool,
    pub greeter_qt6: bool,
    pub qt_multimedia: bool,
    pub image_background: bool,
    pub video_background: bool,
    pub greeter_started_selection: bool,
    pub system_apply_requires_privilege: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SddmStatus {
    pub configured: bool,
    pub theme_installed: bool,
    pub sddm_activated: bool,
    pub descriptor_path: String,
    pub installed_preset: Option<String>,
    pub mode: Option<String>,
    pub staged_media: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SddmPlan {
    pub valid: bool,
    pub preset: String,
    pub mode: String,
    pub media_items: usize,
    pub destination: String,
    pub operations: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedMedia {
    pub source: String,
    pub destination: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SddmApplyResult {
    pub applied: bool,
    pub manifest_staged: bool,
    pub sddm_activated: bool,
    pub descriptor_path: String,
    pub staged_media: Vec<StagedMedia>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SddmRestoreResult {
    pub restored: bool,
    pub removed_managed_theme: bool,
    pub restored_previous_config: bool,
}

#[derive(Debug, Clone)]
pub struct SddmManager {
    root: PathBuf,
    theme_root: PathBuf,
    config_path: PathBuf,
}

impl SddmManager {
    pub fn from_environment() -> Self {
        let root = std::env::var_os("KITSUNE_SDDM_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/var/lib/kisddm"));
        let theme_root = std::env::var_os("KITSUNE_SDDM_THEME_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/usr/share/sddm/themes/kisddm"));
        let config_path = std::env::var_os("KITSUNE_SDDM_CONFIG_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/etc/sddm.conf.d/90-kisddm.conf"));
        Self {
            root,
            theme_root,
            config_path,
        }
    }

    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            theme_root: PathBuf::from("/usr/share/sddm/themes/kisddm"),
            config_path: PathBuf::from("/etc/sddm.conf.d/90-kisddm.conf"),
        }
    }

    pub fn with_system_paths(root: PathBuf, theme_root: PathBuf, config_path: PathBuf) -> Self {
        Self {
            root,
            theme_root,
            config_path,
        }
    }

    pub fn capabilities(&self) -> SddmCapabilities {
        let sddm_installed = command_exists("sddm") || Path::new("/usr/bin/sddm").is_file();
        let greeter_qt6 =
            command_exists("sddm-greeter-qt6") || Path::new("/usr/bin/sddm-greeter-qt6").is_file();
        let qt_multimedia = [
            "/usr/lib/qt6/qml/QtMultimedia",
            "/usr/lib64/qt6/qml/QtMultimedia",
        ]
        .iter()
        .any(|path| Path::new(path).is_dir());
        SddmCapabilities {
            sddm_installed,
            greeter_qt6,
            qt_multimedia,
            image_background: greeter_qt6,
            video_background: greeter_qt6 && qt_multimedia,
            greeter_started_selection: true,
            system_apply_requires_privilege: self.root.starts_with("/var"),
        }
    }

    pub fn plan(&self, descriptor_path: &Path) -> Result<SddmPlan, String> {
        let descriptor = read_descriptor(descriptor_path)?;
        let preset = required_string(&descriptor, "theme_preset")?;
        let background = descriptor
            .get("background")
            .and_then(Value::as_object)
            .ok_or_else(|| "descriptor background must be an object".to_string())?;
        let mode = background
            .get("mode")
            .and_then(Value::as_str)
            .ok_or_else(|| "descriptor background.mode is required".to_string())?;
        if !matches!(mode, "static" | "random" | "video") {
            return Err("descriptor background.mode is not supported".into());
        }
        let mut media_items = background
            .get("rotation_pool")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        if background
            .get("selected")
            .is_some_and(|value| !value.is_null())
        {
            media_items += 1;
        }
        if background
            .get("fallback")
            .is_some_and(|value| !value.is_null())
        {
            media_items += 1;
        }
        let mut warnings = Vec::new();
        let dynamic_source = background
            .get("dynamic_source")
            .is_some_and(|value| !value.is_null());
        if media_items == 0 && !dynamic_source {
            warnings
                .push("no resolved media was supplied; the preset fallback will be used".into());
        }
        if mode == "video" && !self.capabilities().video_background {
            warnings.push("QtMultimedia support was not detected".into());
        }
        Ok(SddmPlan {
            valid: true,
            preset: preset.into(),
            mode: mode.into(),
            media_items,
            destination: self.descriptor_path().to_string_lossy().into_owned(),
            operations: vec![
                "validate KiSDDM descriptor".into(),
                "stage resolved media for the sddm user".into(),
                "atomically activate the greeter manifest".into(),
            ],
            warnings,
        })
    }

    pub fn apply(
        &self,
        descriptor_path: &Path,
        media_paths: &[PathBuf],
        theme_source: Option<&Path>,
    ) -> Result<SddmApplyResult, String> {
        let plan = self.plan(descriptor_path)?;
        let descriptor = read_descriptor(descriptor_path)?;
        let dynamic_source = descriptor
            .pointer("/background/dynamic_source")
            .is_some_and(|value| !value.is_null());
        if (!dynamic_source && media_paths.len() != plan.media_items)
            || (dynamic_source && media_paths.is_empty())
        {
            return Err(format!(
                "descriptor requires {} resolved media file(s){}; received {}",
                plan.media_items,
                if dynamic_source {
                    " or a non-empty dynamic pool"
                } else {
                    ""
                },
                media_paths.len()
            ));
        }
        fs::create_dir_all(self.root.join("media"))
            .map_err(|error| format!("create KiSDDM system state: {error}"))?;
        set_directory_readable(&self.root)?;
        set_directory_readable(&self.root.join("media"))?;
        clear_staged_media(&self.root.join("media"))?;
        let mut staged = Vec::new();
        for (index, source) in media_paths.iter().enumerate() {
            if !source.is_absolute() || !source.is_file() {
                return Err(format!(
                    "resolved media must be an existing absolute file: {}",
                    source.display()
                ));
            }
            let extension = source
                .extension()
                .and_then(|value| value.to_str())
                .map(str::to_ascii_lowercase)
                .filter(|value| {
                    matches!(
                        value.as_str(),
                        "jpg" | "jpeg" | "png" | "webp" | "mp4" | "webm" | "mkv"
                    )
                })
                .unwrap_or_else(|| "bin".into());
            let destination = self
                .root
                .join("media")
                .join(format!("media-{index}.{extension}"));
            fs::copy(source, &destination)
                .map_err(|error| format!("stage media {}: {error}", source.display()))?;
            set_public_file(&destination)?;
            staged.push(StagedMedia {
                source: source.to_string_lossy().into_owned(),
                destination: destination.to_string_lossy().into_owned(),
            });
        }
        let mut descriptor = descriptor;
        descriptor["staged_media"] = Value::Array(
            staged
                .iter()
                .map(|item| serde_json::json!({"destination": item.destination}))
                .collect(),
        );
        atomic_json(&self.descriptor_path(), &descriptor)?;
        set_public_file(&self.descriptor_path())?;
        let activated = if let Some(theme_source) = theme_source {
            self.install_theme(theme_source, &descriptor, &staged)?;
            true
        } else {
            false
        };
        Ok(SddmApplyResult {
            applied: true,
            manifest_staged: true,
            sddm_activated: activated,
            descriptor_path: self.descriptor_path().to_string_lossy().into_owned(),
            staged_media: staged,
        })
    }

    pub fn status(&self) -> Result<SddmStatus, String> {
        let path = self.descriptor_path();
        if !path.is_file() {
            return Ok(SddmStatus {
                configured: false,
                theme_installed: false,
                sddm_activated: false,
                descriptor_path: path.to_string_lossy().into_owned(),
                installed_preset: None,
                mode: None,
                staged_media: 0,
            });
        }
        let descriptor = read_descriptor(&path)?;
        let theme_installed = self.theme_root().join(".managed-by-kisddm").is_file();
        let sddm_activated =
            fs::read(self.config_path()).is_ok_and(|value| value.as_slice() == MANAGED_CONFIG);
        Ok(SddmStatus {
            configured: true,
            theme_installed,
            sddm_activated,
            descriptor_path: path.to_string_lossy().into_owned(),
            installed_preset: descriptor
                .get("theme_preset")
                .and_then(Value::as_str)
                .map(str::to_owned),
            mode: descriptor
                .pointer("/background/mode")
                .and_then(Value::as_str)
                .map(str::to_owned),
            staged_media: descriptor
                .get("staged_media")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
        })
    }

    fn descriptor_path(&self) -> PathBuf {
        self.root.join("active.json")
    }

    pub fn restore(&self) -> Result<SddmRestoreResult, String> {
        self.ensure_config_can_be_restored()?;
        let theme_root = self.theme_root();
        let marker = theme_root.join(".managed-by-kisddm");
        let removed_managed_theme = if marker.is_file() {
            fs::remove_dir_all(&theme_root)
                .map_err(|error| format!("remove managed SDDM theme: {error}"))?;
            true
        } else {
            false
        };
        let backup = self.root.join("backup/sddm-config");
        let absent = self.root.join("backup/sddm-config.absent");
        let config = self.config_path();
        if backup.is_file() {
            let original =
                fs::read(&backup).map_err(|error| format!("read SDDM backup: {error}"))?;
            if config.is_file()
                && fs::read(&config).is_ok_and(|current| {
                    current.as_slice() != MANAGED_CONFIG && current != original
                })
            {
                return Err(
                    "SDDM configuration changed after KiSDDM activation; refusing rollback".into(),
                );
            }
        } else if absent.is_file()
            && config.is_file()
            && fs::read(&config).is_ok_and(|current| current.as_slice() != MANAGED_CONFIG)
        {
            return Err(
                "SDDM configuration changed after KiSDDM activation; refusing rollback".into(),
            );
        }
        let restored_previous_config = if backup.is_file() {
            let bytes = fs::read(&backup).map_err(|error| format!("read SDDM backup: {error}"))?;
            atomic_bytes(&config, &bytes)?;
            true
        } else if absent.is_file() {
            if config.is_file() {
                fs::remove_file(&config)
                    .map_err(|error| format!("remove KiSDDM config: {error}"))?;
            }
            true
        } else {
            false
        };
        Ok(SddmRestoreResult {
            restored: removed_managed_theme || restored_previous_config,
            removed_managed_theme,
            restored_previous_config,
        })
    }

    fn install_theme(
        &self,
        source: &Path,
        descriptor: &Value,
        staged: &[StagedMedia],
    ) -> Result<(), String> {
        if !source.is_absolute() || !source.is_dir() {
            return Err("theme source must be an existing absolute directory".into());
        }
        for name in ["Main.qml", "metadata.desktop"] {
            if !source.join(name).is_file() {
                return Err(format!("theme source is missing {name}"));
            }
        }
        let metadata = fs::read_to_string(source.join("metadata.desktop"))
            .map_err(|error| format!("read theme metadata: {error}"))?;
        if !metadata.lines().any(|line| line.trim() == "QtVersion=6") {
            return Err("KiSDDM runtime theme must declare QtVersion=6".into());
        }
        let destination = self.theme_root();
        if fs::symlink_metadata(&destination)
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
        {
            return Err("refusing a symlinked SDDM theme destination".into());
        }
        let marker = destination.join(".managed-by-kisddm");
        if destination.exists() && !marker.is_file() {
            return Err(format!(
                "refusing to replace unmanaged SDDM theme: {}",
                destination.display()
            ));
        }
        fs::create_dir_all(&destination)
            .map_err(|error| format!("create SDDM theme directory: {error}"))?;
        set_directory_readable(&destination)?;
        for name in ["Main.qml", "metadata.desktop"] {
            let bytes = fs::read(source.join(name))
                .map_err(|error| format!("read theme {name}: {error}"))?;
            atomic_bytes(&destination.join(name), &bytes)?;
            set_public_file(&destination.join(name))?;
        }
        let source_assets = source.join("assets");
        if source_assets.is_dir() {
            let destination_assets = destination.join("assets");
            fs::create_dir_all(&destination_assets)
                .map_err(|error| format!("create SDDM theme assets: {error}"))?;
            set_directory_readable(&destination_assets)?;
            for name in [
                "system-suspend.svg",
                "system-reboot.svg",
                "system-shutdown.svg",
                "change-user.svg",
                "NOTICE.md",
            ] {
                let source_asset = source_assets.join(name);
                if source_asset.is_file() {
                    let bytes = fs::read(&source_asset)
                        .map_err(|error| format!("read theme asset {name}: {error}"))?;
                    atomic_bytes(&destination_assets.join(name), &bytes)?;
                    set_public_file(&destination_assets.join(name))?;
                }
            }
        }
        let source_font = source.join("font/PixelifySans-Bold.ttf");
        if source_font.is_file() {
            let destination_font = destination.join("font");
            fs::create_dir_all(&destination_font)
                .map_err(|error| format!("create SDDM theme font directory: {error}"))?;
            set_directory_readable(&destination_font)?;
            let bytes = fs::read(&source_font)
                .map_err(|error| format!("read Pixelify Sans font: {error}"))?;
            atomic_bytes(&destination_font.join("PixelifySans-Bold.ttf"), &bytes)?;
            set_public_file(&destination_font.join("PixelifySans-Bold.ttf"))?;
        }
        atomic_bytes(
            &destination.join("theme.conf"),
            theme_config(descriptor, staged)?.as_bytes(),
        )?;
        set_public_file(&destination.join("theme.conf"))?;
        atomic_bytes(&marker, b"managed by kitsune-compositor\n")?;
        self.backup_config_once()?;
        atomic_bytes(&self.config_path(), MANAGED_CONFIG)?;
        set_public_file(&self.config_path())
    }

    fn backup_config_once(&self) -> Result<(), String> {
        let backup_root = self.root.join("backup");
        fs::create_dir_all(&backup_root)
            .map_err(|error| format!("create SDDM backup directory: {error}"))?;
        let backup = backup_root.join("sddm-config");
        let absent = backup_root.join("sddm-config.absent");
        if backup.exists() || absent.exists() {
            let config = self.config_path();
            if backup.is_file() {
                let original =
                    fs::read(&backup).map_err(|error| format!("read SDDM backup: {error}"))?;
                if config.is_file()
                    && fs::read(&config).is_ok_and(|current| {
                        current.as_slice() != MANAGED_CONFIG && current != original
                    })
                {
                    return Err(
                        "SDDM configuration changed after KiSDDM activation; refusing overwrite"
                            .into(),
                    );
                }
            } else if config.is_file()
                && fs::read(&config).is_ok_and(|current| current.as_slice() != MANAGED_CONFIG)
            {
                return Err(
                    "SDDM configuration appeared after KiSDDM activation; refusing overwrite"
                        .into(),
                );
            }
            return Ok(());
        }
        let config = self.config_path();
        if config.is_file() {
            fs::copy(&config, &backup).map_err(|error| format!("backup SDDM config: {error}"))?;
        } else {
            atomic_bytes(&absent, b"config did not exist before KiSDDM\n")?;
        }
        Ok(())
    }

    fn ensure_config_can_be_restored(&self) -> Result<(), String> {
        let backup = self.root.join("backup/sddm-config");
        let absent = self.root.join("backup/sddm-config.absent");
        let config = self.config_path();
        if backup.is_file() {
            let original =
                fs::read(&backup).map_err(|error| format!("read SDDM backup: {error}"))?;
            if config.is_file()
                && fs::read(&config).is_ok_and(|current| {
                    current.as_slice() != MANAGED_CONFIG && current != original
                })
            {
                return Err(
                    "SDDM configuration changed after KiSDDM activation; refusing rollback".into(),
                );
            }
        } else if absent.is_file()
            && config.is_file()
            && fs::read(&config).is_ok_and(|current| current.as_slice() != MANAGED_CONFIG)
        {
            return Err(
                "SDDM configuration changed after KiSDDM activation; refusing rollback".into(),
            );
        }
        Ok(())
    }

    fn theme_root(&self) -> PathBuf {
        self.theme_root.clone()
    }

    fn config_path(&self) -> PathBuf {
        self.config_path.clone()
    }
}

fn theme_config(descriptor: &Value, staged: &[StagedMedia]) -> Result<String, String> {
    let background = descriptor
        .pointer("/background")
        .ok_or_else(|| "missing background".to_string())?;
    let appearance = descriptor
        .pointer("/appearance")
        .ok_or_else(|| "missing appearance".to_string())?;
    let fixed_main_count = usize::from(
        background
            .get("selected")
            .is_some_and(|value| !value.is_null()),
    ) + background
        .get("rotation_pool")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let fallback_count = usize::from(
        background
            .get("fallback")
            .is_some_and(|value| !value.is_null()),
    );
    let main_count = if background
        .get("dynamic_source")
        .is_some_and(|value| !value.is_null())
    {
        staged.len().saturating_sub(fallback_count)
    } else {
        fixed_main_count
    };
    let media = staged
        .iter()
        .take(main_count)
        .map(|item| format!("file://{}", item.destination))
        .collect::<Vec<_>>()
        .join("|");
    let fallback = background
        .get("fallback")
        .is_some_and(|value| !value.is_null())
        .then(|| staged.get(main_count))
        .flatten()
        .map(|item| format!("file://{}", item.destination))
        .unwrap_or_default();
    let (battery_available, battery_percent) = battery_status();
    Ok(format!(
        "[General]\nPreset={}\nMode={}\nSelectionStrategy={}\nAvoidLast={}\nMediaList={}\nFallback={}\nAccent={}\nTextColour={}\nPanelOpacity={}\nSessionSelectorVisible={}\nBatteryAvailable={}\nBatteryPercent={}\n",
        required_string(descriptor, "theme_preset")?,
        required_string(background, "mode")?,
        required_string(background, "selection_strategy")?,
        background
            .get("avoid_last")
            .and_then(Value::as_bool)
            .ok_or_else(|| "missing avoid_last".to_string())?,
        media,
        fallback,
        required_string(appearance, "accent")?,
        required_string(appearance, "text_colour")?,
        appearance
            .get("panel_opacity")
            .and_then(Value::as_f64)
            .ok_or_else(|| "missing panel_opacity".to_string())?,
        appearance
            .get("session_selector_visible")
            .and_then(Value::as_bool)
            .ok_or_else(|| "missing session_selector_visible".to_string())?,
        battery_available,
        battery_percent,
    ))
}

fn battery_status() -> (bool, u8) {
    let root = std::env::var_os("KITSUNE_SDDM_BATTERY_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/sys/class/power_supply"));
    let Ok(entries) = fs::read_dir(root) else {
        return (false, 0);
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let kind = fs::read_to_string(path.join("type")).unwrap_or_default();
        if kind.trim() != "Battery" {
            continue;
        }
        if let Ok(percent) = fs::read_to_string(path.join("capacity"))
            && let Ok(percent) = percent.trim().parse::<u8>()
        {
            return (true, percent.min(100));
        }
    }
    (false, 0)
}

fn read_descriptor(path: &Path) -> Result<Value, String> {
    if !path.is_absolute() || !path.is_file() {
        return Err("descriptor must be an existing absolute JSON file".into());
    }
    let bytes = fs::read(path).map_err(|error| format!("read descriptor: {error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("parse descriptor: {error}"))
}

fn required_string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("descriptor {key} is required"))
}

fn atomic_json(path: &Path, value: &Value) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "state path has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("create state directory: {error}"))?;
    let temporary = parent.join(format!(".active.json.{}", std::process::id()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| format!("create temporary state: {error}"))?;
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.write_all(b"\n"))
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("write temporary state: {error}"))?;
    fs::rename(&temporary, path).map_err(|error| format!("activate state: {error}"))
}

fn atomic_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| "destination has no parent".to_string())?;
    fs::create_dir_all(parent).map_err(|error| format!("create destination directory: {error}"))?;
    let temporary = parent.join(format!(".kisddm.{}", std::process::id()));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| format!("create temporary file: {error}"))?;
    let result = file
        .write_all(bytes)
        .and_then(|_| file.sync_all())
        .and_then(|_| fs::rename(&temporary, path));
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|error| format!("activate file: {error}"))
}

fn command_exists(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|path| path.join(name).is_file()))
}

fn clear_staged_media(directory: &Path) -> Result<(), String> {
    for entry in fs::read_dir(directory).map_err(|error| format!("read staged media: {error}"))? {
        let entry = entry.map_err(|error| format!("read staged media entry: {error}"))?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("media-")
            && entry.file_type().is_ok_and(|kind| kind.is_file())
        {
            fs::remove_file(entry.path())
                .map_err(|error| format!("remove old staged media: {error}"))?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn set_public_file(path: &Path) -> Result<(), String> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o644))
        .map_err(|error| format!("set readable file permissions: {error}"))
}

#[cfg(not(unix))]
fn set_public_file(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn set_directory_readable(path: &Path) -> Result<(), String> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .map_err(|error| format!("set readable directory permissions: {error}"))
}

#[cfg(not(unix))]
fn set_directory_readable(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_and_applies_into_an_isolated_root() {
        let root = std::env::temp_dir().join(format!("kitsune-sddm-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let descriptor = root.join("desired.json");
        fs::write(&descriptor, r#"{"theme_preset":"aurora-glass","background":{"mode":"video","selected":null,"rotation_pool":[]}}"#).unwrap();
        let manager = SddmManager::new(root.join("system"));
        assert_eq!(manager.plan(&descriptor).unwrap().mode, "video");
        assert!(manager.apply(&descriptor, &[], None).unwrap().applied);
        assert!(manager.status().unwrap().configured);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installs_only_a_managed_theme_and_restores_the_previous_config() {
        let root =
            std::env::temp_dir().join(format!("kitsune-sddm-install-{}", std::process::id()));
        let source = root.join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("Main.qml"), "import QtQuick 2.15\nItem {}\n").unwrap();
        fs::write(
            source.join("metadata.desktop"),
            "[SddmGreeterTheme]\nName=KiSDDM\nQtVersion=6\n",
        )
        .unwrap();
        let descriptor = root.join("desired.json");
        fs::write(
            &descriptor,
            r##"{"theme_preset":"aurora-glass","background":{"mode":"static","selection_strategy":"fixed","avoid_last":true,"selected":null,"rotation_pool":[]},"appearance":{"accent":"#ad3cf3","text_colour":"#ffffff","panel_opacity":0.82,"session_selector_visible":true}}"##,
        )
        .unwrap();
        let config = root.join("etc/90-kisddm.conf");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        fs::write(&config, "[Theme]\nCurrent=previous\n").unwrap();
        let manager = SddmManager::with_system_paths(
            root.join("state"),
            root.join("themes/kisddm"),
            config.clone(),
        );
        let result = manager.apply(&descriptor, &[], Some(&source)).unwrap();
        assert!(result.sddm_activated);
        assert!(manager.status().unwrap().theme_installed);
        fs::write(&config, "[Theme]\nCurrent=user-change\n").unwrap();
        assert!(manager.restore().unwrap_err().contains("changed"));
        assert!(manager.status().unwrap().theme_installed);
        fs::write(&config, MANAGED_CONFIG).unwrap();
        let restored = manager.restore().unwrap();
        assert!(restored.restored_previous_config);
        assert!(
            fs::read_to_string(config)
                .unwrap()
                .contains("Current=previous")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dynamic_pack_uses_every_resolved_image_as_main_media() {
        let root =
            std::env::temp_dir().join(format!("kitsune-sddm-dynamic-{}", std::process::id()));
        let source = root.join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("Main.qml"), "import QtQuick 2.15\nItem {}\n").unwrap();
        fs::write(
            source.join("metadata.desktop"),
            "[SddmGreeterTheme]\nName=KiSDDM\nQtVersion=6\n",
        )
        .unwrap();
        let first = root.join("first.jpg");
        let second = root.join("second.png");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();
        let descriptor = root.join("desired.json");
        fs::write(
            &descriptor,
            r##"{"theme_preset":"aurora-glass","background":{"mode":"random","selection_strategy":"random","avoid_last":false,"selected":null,"rotation_pool":[],"dynamic_source":{"owner":"kitowall","collection":"current_pack","kind":"image"}},"appearance":{"accent":"#ad3cf3","text_colour":"#ffffff","panel_opacity":0.82,"session_selector_visible":true}}"##,
        )
        .unwrap();
        let manager = SddmManager::with_system_paths(
            root.join("state"),
            root.join("themes/kisddm"),
            root.join("etc/90-kisddm.conf"),
        );
        manager
            .apply(&descriptor, &[first, second], Some(&source))
            .unwrap();
        let theme = fs::read_to_string(root.join("themes/kisddm/theme.conf")).unwrap();
        assert!(theme.contains("media-0.jpg|file://"));
        assert!(theme.contains("media-1.png"));
        fs::remove_dir_all(root).unwrap();
    }
}

use super::{AppearanceAdvisory, AppearanceCapabilities, AppearanceMode};
use crate::HostRunner;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub fn detect<R: HostRunner>(runner: &R) -> AppearanceCapabilities {
    let desktop = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let session = std::env::var("XDG_SESSION_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let desktop = format!("{desktop}:{session}");
    detect_for_desktop(runner, &desktop)
}

fn detect_for_desktop<R: HostRunner>(runner: &R, desktop: &str) -> AppearanceCapabilities {
    if desktop.contains("hyprland") && runner.command_exists("caelestia") {
        let mut result = capabilities(
            "caelestia",
            AppearanceMode::NativePalette,
            true,
            true,
            true,
            &["hyprland", "shell", "gtk", "qt", "terminal", "integrations"],
            "Caelestia is available for opt-in full Material palette propagation",
        );
        result.advisories = caelestia_advisories(runner, &caelestia_cli_config_path());
        return result;
    }
    if desktop.contains("gnome") && runner.command_exists("gsettings") {
        return capabilities(
            "gnome",
            AppearanceMode::Nearest,
            true,
            false,
            false,
            &["system_ui", "compatible_apps"],
            "GNOME supports a finite accent palette; colors must be mapped to the nearest value",
        );
    }
    if (desktop.contains("kde") || desktop.contains("plasma"))
        && (runner.command_exists("plasma-apply-colorscheme")
            || runner.command_exists("kwriteconfig6"))
    {
        return capabilities(
            "kde",
            AppearanceMode::Exact,
            true,
            false,
            false,
            &["plasma", "kde_apps", "compatible_apps"],
            "KDE Plasma appearance tools were detected; the exact write adapter is not enabled yet",
        );
    }
    if desktop.contains("hyprland") && runner.command_exists("hyprctl") {
        return capabilities(
            "hyprland",
            AppearanceMode::GeneratedConfig,
            true,
            false,
            false,
            &["hyprland"],
            "Hyprland supports an opt-in generated color fragment; system-wide theming is unavailable",
        );
    }
    if runner.command_exists("gdbus") {
        return capabilities(
            "xdg-portal",
            AppearanceMode::ReadOnly,
            true,
            false,
            false,
            &["published_settings"],
            "XDG Settings Portal can report appearance values but cannot change them",
        );
    }
    capabilities(
        "none",
        AppearanceMode::Unsupported,
        true,
        false,
        false,
        &[],
        "No supported appearance provider was detected",
    )
}

fn capabilities(
    backend: &str,
    mode: AppearanceMode,
    preview_supported: bool,
    apply_supported: bool,
    restore_supported: bool,
    scopes: &[&str],
    reason: &str,
) -> AppearanceCapabilities {
    AppearanceCapabilities {
        supported: mode != AppearanceMode::Unsupported,
        backend: backend.into(),
        mode,
        palette_supported: true,
        preview_supported,
        apply_supported,
        restore_supported,
        scopes: scopes.iter().map(|scope| (*scope).into()).collect(),
        reason: reason.into(),
        advisories: Vec::new(),
    }
}

fn caelestia_cli_config_path() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"))
        .join("caelestia/cli.json")
}

fn caelestia_advisories<R: HostRunner>(runner: &R, config_path: &Path) -> Vec<AppearanceAdvisory> {
    let chromium_detected = [
        "brave",
        "chromium",
        "chromium-browser",
        "google-chrome-stable",
    ]
    .iter()
    .any(|command| runner.command_exists(command));
    if !chromium_detected {
        return Vec::new();
    }
    let chromium_enabled = std::fs::read(config_path)
        .ok()
        .and_then(|content| serde_json::from_slice::<Value>(&content).ok())
        .and_then(|config| {
            config
                .pointer("/theme/enableChromium")
                .and_then(Value::as_bool)
        })
        .unwrap_or(true);
    if !chromium_enabled {
        return Vec::new();
    }
    vec![AppearanceAdvisory {
        code: "caelestia_chromium_refresh_may_hang".into(),
        severity: "blocking".into(),
        message: format!(
            "Caelestia puede quedar esperando al actualizar Brave/Chromium. Edita {} y configura theme.enableChromium en false antes de activar los colores dinamicos.",
            config_path.display()
        ),
        config_path: Some(config_path.to_path_buf()),
        suggested_json: Some("{\n  \"theme\": {\n    \"enableChromium\": false\n  }\n}".into()),
    }]
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::collections::BTreeSet;

    #[derive(Default)]
    struct FakeRunner {
        commands: BTreeSet<String>,
    }

    impl HostRunner for FakeRunner {
        fn command_exists(&self, bin: &str) -> bool {
            self.commands.contains(bin)
        }

        fn run_json(&self, _bin: &str, _args: &[&str]) -> Result<Value, String> {
            unreachable!()
        }
    }

    #[test]
    fn caelestia_has_priority_inside_hyprland() {
        let runner = FakeRunner {
            commands: ["caelestia", "hyprctl"]
                .into_iter()
                .map(str::to_string)
                .collect(),
        };
        let result = detect_for_desktop(&runner, "hyprland");
        assert_eq!(result.backend, "caelestia");
        assert_eq!(result.mode, AppearanceMode::NativePalette);
        assert!(result.apply_supported);
        assert!(result.restore_supported);
    }

    #[test]
    fn warns_when_chromium_theming_is_implicitly_enabled() {
        let runner = FakeRunner {
            commands: ["caelestia", "brave"]
                .into_iter()
                .map(str::to_string)
                .collect(),
        };
        let path = std::env::temp_dir().join("missing-caelestia-cli.json");
        let warnings = caelestia_advisories(&runner, &path);
        assert_eq!(warnings[0].code, "caelestia_chromium_refresh_may_hang");
        assert_eq!(warnings[0].severity, "blocking");
    }

    #[test]
    fn accepts_an_explicitly_disabled_chromium_integration() {
        let runner = FakeRunner {
            commands: ["caelestia", "brave"]
                .into_iter()
                .map(str::to_string)
                .collect(),
        };
        let path = std::env::temp_dir().join(format!(
            "caelestia-cli-{}-{}.json",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        std::fs::write(&path, r#"{"theme":{"enableChromium":false}}"#).unwrap();
        assert!(caelestia_advisories(&runner, &path).is_empty());
        std::fs::remove_file(path).unwrap();
    }
}

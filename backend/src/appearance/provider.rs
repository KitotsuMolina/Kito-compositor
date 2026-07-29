use super::{AppearanceCapabilities, AppearanceMode};
use crate::HostRunner;

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
        return capabilities(
            "caelestia",
            AppearanceMode::NativePalette,
            true,
            true,
            true,
            &["hyprland", "shell", "gtk", "qt", "terminal", "integrations"],
            "Caelestia is available for opt-in full Material palette propagation",
        );
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
    }
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
}

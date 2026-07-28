use crate::{ApplicationWindow, InstalledApplication, RunningApplication};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

pub struct ApplicationCatalog;

impl ApplicationCatalog {
    pub fn installed() -> Vec<InstalledApplication> {
        installed_from_dirs(&xdg_application_dirs())
    }

    pub fn running(windows: Vec<ApplicationWindow>) -> Vec<RunningApplication> {
        let installed = Self::installed();
        running_from(&installed, windows, &processes(Path::new("/proc")))
    }
}

pub fn application_matches(
    running: &[RunningApplication],
    ids: &[String],
) -> Vec<RunningApplication> {
    let wanted = ids
        .iter()
        .map(|value| normalized(value))
        .filter(|value| !value.is_empty())
        .collect::<BTreeSet<_>>();
    running
        .iter()
        .filter(|application| {
            wanted.contains(&normalized(&application.id))
                || wanted.contains(&normalized(&application.app_id))
        })
        .cloned()
        .collect()
}

fn xdg_application_dirs() -> Vec<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let data_home = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new(&home).join(".local/share"));
    let mut dirs = vec![data_home.join("applications")];
    let system =
        std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(
        system
            .split(':')
            .filter(|value| !value.trim().is_empty())
            .map(|value| Path::new(value).join("applications")),
    );
    dirs
}

fn installed_from_dirs(dirs: &[PathBuf]) -> Vec<InstalledApplication> {
    let mut applications = BTreeMap::new();
    for dir in dirs {
        collect_desktop_files(dir, dir, &mut applications);
    }
    applications.into_values().collect()
}

fn collect_desktop_files(
    root: &Path,
    current: &Path,
    applications: &mut BTreeMap<String, InstalledApplication>,
) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_desktop_files(root, &path, applications);
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("desktop") {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let Some(application) = parse_desktop(root, &path, &raw) else {
            continue;
        };
        applications
            .entry(application.id.clone())
            .or_insert(application);
    }
}

fn parse_desktop(root: &Path, path: &Path, raw: &str) -> Option<InstalledApplication> {
    let mut in_entry = false;
    let mut values = BTreeMap::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            values.entry(key.trim()).or_insert(value.trim());
        }
    }
    if values
        .get("Type")
        .is_some_and(|value| *value != "Application")
        || bool_value(values.get("Hidden"))
        || bool_value(values.get("NoDisplay"))
    {
        return None;
    }
    let relative = path.strip_prefix(root).ok().unwrap_or(path);
    let id = relative
        .to_string_lossy()
        .replace('/', "-")
        .trim_end_matches(".desktop")
        .to_string();
    let name = values.get("Name")?.trim();
    if id.is_empty() || name.is_empty() {
        return None;
    }
    Some(InstalledApplication {
        id,
        name: name.into(),
        executable: values.get("Exec").and_then(|value| executable(value)),
        icon: values
            .get("Icon")
            .map(|value| value.to_string())
            .filter(|value| !value.is_empty()),
        desktop_file: path.to_string_lossy().into_owned(),
    })
}

fn executable(exec: &str) -> Option<String> {
    exec.split_whitespace()
        .map(|value| value.trim_matches(['"', '\'']))
        .find(|value| {
            !value.is_empty() && !value.contains('=') && !value.starts_with('%') && *value != "env"
        })
        .and_then(|value| {
            Path::new(value)
                .file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        })
}

fn bool_value(value: Option<&&str>) -> bool {
    value.is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "1" | "true" | "yes"))
}

#[derive(Debug, Clone)]
struct ProcessIdentity {
    pid: u32,
    command: String,
}

fn processes(root: &Path) -> Vec<ProcessIdentity> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_string_lossy().parse::<u32>().ok()?;
            let command = fs::read_link(entry.path().join("exe"))
                .ok()
                .and_then(|path| {
                    path.file_name()
                        .map(|value| value.to_string_lossy().into_owned())
                })
                .or_else(|| {
                    fs::read(entry.path().join("comm"))
                        .ok()
                        .and_then(|bytes| String::from_utf8(bytes).ok())
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty())
                })?;
            Some(ProcessIdentity { pid, command })
        })
        .collect()
}

fn running_from(
    installed: &[InstalledApplication],
    windows: Vec<ApplicationWindow>,
    processes: &[ProcessIdentity],
) -> Vec<RunningApplication> {
    let mut result = Vec::new();
    for window in windows {
        let matched = installed
            .iter()
            .find(|application| application_identity_matches(application, &window.app_id));
        result.push(RunningApplication {
            id: matched
                .map(|application| application.id.clone())
                .unwrap_or_else(|| window.app_id.clone()),
            name: matched
                .map(|application| application.name.clone())
                .unwrap_or_else(|| window.app_id.clone()),
            app_id: window.app_id,
            pid: window.pid,
            title: window.title,
            focused: window.focused,
            fullscreen: window.fullscreen,
            backend: window.backend,
        });
    }
    for process in processes {
        let Some(application) = installed.iter().find(|application| {
            application
                .executable
                .as_deref()
                .filter(|value| !generic_launcher(value))
                .is_some_and(|value| normalized(value) == normalized(&process.command))
        }) else {
            continue;
        };
        if result.iter().any(|item| {
            item.pid == Some(process.pid) || normalized(&item.id) == normalized(&application.id)
        }) {
            continue;
        }
        result.push(RunningApplication {
            id: application.id.clone(),
            name: application.name.clone(),
            app_id: application.id.clone(),
            pid: Some(process.pid),
            title: None,
            focused: false,
            fullscreen: false,
            backend: "procfs".into(),
        });
    }
    result.sort_by(|left, right| left.name.cmp(&right.name).then(left.pid.cmp(&right.pid)));
    result
}

fn application_identity_matches(application: &InstalledApplication, candidate: &str) -> bool {
    let candidate = normalized(candidate);
    candidate == normalized(&application.id)
        || application
            .executable
            .as_deref()
            .is_some_and(|value| candidate == normalized(value))
}

fn normalized(value: &str) -> String {
    value
        .trim()
        .trim_end_matches(".desktop")
        .to_ascii_lowercase()
}

fn generic_launcher(value: &str) -> bool {
    matches!(
        normalized(value).as_str(),
        "env"
            | "flatpak"
            | "steam"
            | "sh"
            | "bash"
            | "python"
            | "python3"
            | "java"
            | "xdg-open"
            | "gio"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_visible_xdg_application() {
        let root = Path::new("/tmp/apps");
        let app = parse_desktop(
            root,
            &root.join("org.example.Game.desktop"),
            "[Desktop Entry]\nType=Application\nName=Example Game\nExec=/opt/game/bin/game %U\nIcon=game\n",
        )
        .unwrap();
        assert_eq!(app.id, "org.example.Game");
        assert_eq!(app.executable.as_deref(), Some("game"));
    }

    #[test]
    fn matches_windows_and_process_fallbacks() {
        let installed = vec![InstalledApplication {
            id: "org.example.Game".into(),
            name: "Example Game".into(),
            executable: Some("game".into()),
            icon: None,
            desktop_file: "/tmp/game.desktop".into(),
        }];
        let running = running_from(
            &installed,
            vec![ApplicationWindow {
                app_id: "org.example.Game".into(),
                title: Some("Playing".into()),
                pid: Some(10),
                output: None,
                workspace: None,
                focused: true,
                fullscreen: true,
                backend: "test".into(),
            }],
            &[ProcessIdentity {
                pid: 10,
                command: "game".into(),
            }],
        );
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].id, "org.example.Game");
    }

    #[test]
    fn process_fallback_ignores_shared_launchers() {
        let installed = vec![InstalledApplication {
            id: "org.example.Flatpak".into(),
            name: "Example Flatpak".into(),
            executable: Some("flatpak".into()),
            icon: None,
            desktop_file: "/tmp/flatpak.desktop".into(),
        }];
        let running = running_from(
            &installed,
            Vec::new(),
            &[ProcessIdentity {
                pid: 20,
                command: "flatpak".into(),
            }],
        );
        assert!(running.is_empty());
    }
}

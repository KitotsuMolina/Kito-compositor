use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn temp_root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "kitsune-compositor-cli-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn fake_hyprctl(root: &Path) {
    let path = root.join("hyprctl");
    fs::write(
        &path,
        "#!/bin/sh\nprintf '%s\\n' '[{\"name\":\"DP-1\",\"width\":2560,\"height\":1440,\"focused\":true}]'\n",
    )
    .unwrap();
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn fake_awww(root: &Path, log: &Path) {
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
        log.display()
    );
    for name in ["awww", "awww-daemon"] {
        let path = root.join(name);
        fs::write(&path, &script).unwrap();
        #[cfg(unix)]
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
}

fn fake_systemctl(root: &Path, log: &Path) {
    let path = root.join("systemctl");
    fs::write(
        &path,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{}'\nexit 0\n",
            log.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

#[test]
fn outputs_use_the_v1_envelope_and_normalized_model() {
    let root = temp_root();
    fs::create_dir_all(&root).unwrap();
    fake_hyprctl(&root);
    let output = Command::new(env!("CARGO_BIN_EXE_kitsune-compositor"))
        .args(["outputs", "--contract-v1"])
        .env("PATH", &root)
        .env("HYPRLAND_INSTANCE_SIGNATURE", "test")
        .env_remove("NIRI_SOCKET")
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["command"], "outputs");
    assert_eq!(value["data"]["compositor"], "hyprland");
    assert_eq!(value["data"]["outputs"][0]["name"], "DP-1");
    assert_eq!(value["data"]["outputs"][0]["width"], 2560);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn invalid_commands_return_the_contract_error_code() {
    let output = Command::new(env!("CARGO_BIN_EXE_kitsune-compositor"))
        .args(["unknown", "--contract-v1"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], "INVALID_ARGUMENT");
}

#[test]
fn watch_outputs_emits_one_compact_v1_event_in_once_mode() {
    let root = temp_root();
    fs::create_dir_all(&root).unwrap();
    fake_hyprctl(&root);
    let output = Command::new(env!("CARGO_BIN_EXE_kitsune-compositor"))
        .args(["watch", "outputs", "--contract-v1", "--once"])
        .env("PATH", &root)
        .env("HYPRLAND_INSTANCE_SIGNATURE", "test")
        .env_remove("NIRI_SOCKET")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.lines().count(), 1);
    let value: Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(value["event"], "outputs_changed");
    assert_eq!(value["sequence"], 1);
    assert_eq!(value["data"]["outputs"][0]["name"], "DP-1");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn watch_focus_uses_the_same_normalized_output_contract() {
    let root = temp_root();
    fs::create_dir_all(&root).unwrap();
    fake_hyprctl(&root);
    let output = Command::new(env!("CARGO_BIN_EXE_kitsune-compositor"))
        .args(["watch", "focus", "--json-lines", "--once"])
        .env("PATH", &root)
        .env("HYPRLAND_INSTANCE_SIGNATURE", "test")
        .env_remove("NIRI_SOCKET")
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["event"], "focus_changed");
    assert_eq!(value["sequence"], 1);
    assert_eq!(value["data"]["output"]["name"], "DP-1");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn wallpaper_apply_uses_validated_output_and_exact_awww_command() {
    let root = temp_root();
    fs::create_dir_all(&root).unwrap();
    fake_hyprctl(&root);
    let log = root.join("awww.log");
    fake_awww(&root, &log);
    let image = root.join("wallpaper.png");
    fs::write(&image, b"image").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_kitsune-compositor"))
        .args([
            "wallpaper",
            "apply",
            "--namespace",
            "kitowall",
            "--output",
            "DP-1",
            "--image",
            image.to_str().unwrap(),
            "--contract-v1",
        ])
        .env("PATH", &root)
        .env("HYPRLAND_INSTANCE_SIGNATURE", "test")
        .env_remove("NIRI_SOCKET")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["command"], "wallpaper apply");
    assert_eq!(value["data"]["runtime"]["backend"], "awww");
    assert_eq!(value["data"]["output"], "DP-1");
    let calls = fs::read_to_string(&log).unwrap();
    assert!(calls.contains("query --json --namespace kitowall"));
    assert!(calls.contains("img --namespace kitowall --outputs DP-1"));
    assert!(calls.contains(image.to_str().unwrap()));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn service_apply_and_remove_use_only_the_private_registry() {
    let root = temp_root();
    let bin = root.join("bin");
    let units = root.join("units");
    let registry = root.join("state/services.json");
    fs::create_dir_all(&bin).unwrap();
    let systemctl_log = root.join("systemctl.log");
    fake_systemctl(&bin, &systemctl_log);
    let executable = bin.join("kitowall");
    fs::write(&executable, b"binary").unwrap();
    let descriptor = root.join("kitowall-next.json");
    fs::write(
        &descriptor,
        serde_json::to_vec_pretty(&serde_json::json!({
            "unit_type": "service",
            "id": "kitowall-next",
            "unit_name": "kitowall-next.service",
            "description": "Kitowall next wallpaper",
            "exec_start": [executable, "rotate-now"],
            "restart": "no",
            "wanted_by": "default.target"
        }))
        .unwrap(),
    )
    .unwrap();

    let run = |arguments: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_kitsune-compositor"))
            .args(arguments)
            .env("PATH", &bin)
            .env("KITSUNE_COMPOSITOR_SYSTEMD_USER_DIR", &units)
            .env("KITSUNE_COMPOSITOR_SERVICE_REGISTRY", &registry)
            .output()
            .unwrap()
    };
    let apply = run(&[
        "service",
        "apply",
        "--descriptor",
        descriptor.to_str().unwrap(),
        "--contract-v1",
    ]);
    assert!(
        apply.status.success(),
        "{}",
        String::from_utf8_lossy(&apply.stderr)
    );
    assert!(units.join("kitowall-next.service").is_file());
    assert!(registry.is_file());
    let remove = run(&[
        "service",
        "remove",
        "--id",
        "kitowall-next",
        "--contract-v1",
    ]);
    assert!(remove.status.success());
    assert!(!units.join("kitowall-next.service").exists());
    let calls = fs::read_to_string(systemctl_log).unwrap();
    assert!(calls.contains("--user daemon-reload"));
    assert!(calls.contains("--user disable --now kitowall-next.service"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn portable_automation_is_translated_and_applied_by_the_systemd_adapter() {
    let root = temp_root();
    let bin = root.join("bin");
    let units = root.join("units");
    let registry = root.join("state/services.json");
    fs::create_dir_all(&bin).unwrap();
    let systemctl_log = root.join("systemctl.log");
    fake_systemctl(&bin, &systemctl_log);
    let executable = bin.join("kitowall");
    fs::write(&executable, b"binary").unwrap();
    let descriptor = root.join("rotation.json");
    fs::write(
        &descriptor,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "id": "kitowall-next",
            "description": "Rotate Kitowall wallpapers",
            "command": [executable, "rotate-now"],
            "kind": "one_shot",
            "restart": "no",
            "autostart": false,
            "schedule": {
                "startup_delay_seconds": 2,
                "every_seconds": 600
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_kitsune-compositor"))
        .args([
            "automation",
            "apply",
            "--descriptor",
            descriptor.to_str().unwrap(),
            "--contract-v1",
        ])
        .env("HOME", &root)
        .env("PATH", &bin)
        .env("KITSUNE_COMPOSITOR_SYSTEMD_USER_DIR", &units)
        .env("KITSUNE_COMPOSITOR_SERVICE_REGISTRY", &registry)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["command"], "automation apply");
    assert_eq!(value["data"]["manager"], "systemd-user");
    assert!(units.join("kitowall-next.service").is_file());
    assert!(units.join("kitowall-next.timer").is_file());
    let control = |action: &str| {
        Command::new(env!("CARGO_BIN_EXE_kitsune-compositor"))
            .args([
                "automation",
                action,
                "--id",
                "kitowall-next",
                "--contract-v1",
            ])
            .env("HOME", &root)
            .env("PATH", &bin)
            .env("KITSUNE_COMPOSITOR_SYSTEMD_USER_DIR", &units)
            .env("KITSUNE_COMPOSITOR_SERVICE_REGISTRY", &registry)
            .output()
            .unwrap()
    };
    assert!(control("status").status.success());
    assert!(control("stop").status.success());
    assert!(control("remove").status.success());
    assert!(!units.join("kitowall-next.service").exists());
    assert!(!units.join("kitowall-next.timer").exists());
    let calls = fs::read_to_string(systemctl_log).unwrap();
    assert!(calls.contains("--user enable --now kitowall-next.timer"));
    let stop_timer = calls.find("--user stop kitowall-next.timer").unwrap();
    let stop_service = calls.find("--user stop kitowall-next.service").unwrap();
    assert!(stop_timer < stop_service);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn automation_batch_materializes_all_kitowall_artifacts_without_activation() {
    let root = temp_root();
    let bin = root.join("bin");
    let units = root.join("units");
    let registry = root.join("state/services.json");
    fs::create_dir_all(&bin).unwrap();
    let systemctl_log = root.join("systemctl.log");
    fake_systemctl(&bin, &systemctl_log);
    let executable = bin.join("kitowall");
    fs::write(&executable, b"binary").unwrap();
    let descriptor = root.join("kitowall-batch.json");
    let automation = |id: &str, kind: &str, autostart: bool, schedule: Value| {
        serde_json::json!({
            "schema_version": 1,
            "id": id,
            "description": format!("Automation {id}"),
            "command": [executable, id],
            "kind": kind,
            "restart": if kind == "daemon" { "on-failure" } else { "no" },
            "autostart": autostart,
            "schedule": schedule
        })
    };
    fs::write(
        &descriptor,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema_version": 1,
            "automations": [
                automation("kitowall-runtime", "one_shot", true, Value::Null),
                automation("kitowall-next", "one_shot", false, serde_json::json!({
                    "startup_delay_seconds": 2,
                    "every_seconds": 600
                })),
                automation("kitowall-watch", "daemon", true, Value::Null),
                automation("kitowall-login-apply", "one_shot", true, Value::Null)
            ]
        }))
        .unwrap(),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_kitsune-compositor"))
        .args([
            "automation",
            "apply-batch",
            "--descriptor",
            descriptor.to_str().unwrap(),
            "--contract-v1",
        ])
        .env("HOME", &root)
        .env("PATH", &bin)
        .env("KITSUNE_COMPOSITOR_SYSTEMD_USER_DIR", &units)
        .env("KITSUNE_COMPOSITOR_SERVICE_REGISTRY", &registry)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["command"], "automation apply-batch");
    assert_eq!(value["data"]["automation_ids"].as_array().unwrap().len(), 4);
    assert_eq!(value["data"]["artifacts"].as_array().unwrap().len(), 5);
    assert_eq!(value["data"]["activation_required"], true);
    for name in [
        "kitowall-runtime.service",
        "kitowall-next.service",
        "kitowall-next.timer",
        "kitowall-watch.service",
        "kitowall-login-apply.service",
    ] {
        assert!(units.join(name).is_file(), "missing {name}");
    }
    let registry: Value = serde_json::from_slice(&fs::read(&registry).unwrap()).unwrap();
    assert_eq!(registry["units"].as_object().unwrap().len(), 5);
    let calls = fs::read_to_string(systemctl_log).unwrap();
    assert_eq!(calls.matches("--user daemon-reload").count(), 1);
    assert!(!calls.contains("enable --now"));
    let _ = fs::remove_dir_all(root);
}
#[test]
fn local_flag_is_global_and_preserves_the_contract() {
    let output = Command::new(env!("CARGO_BIN_EXE_kitsune-compositor"))
        .args(["--lc", "version", "--contract-v1"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["meta"]["cli"], "kitsune-compositor");
}

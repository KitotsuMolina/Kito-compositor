use image::{ImageBuffer, Rgb};
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

fn fake_command(root: &Path, name: &str) {
    let path = root.join(name);
    fs::write(&path, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn fake_caelestia(root: &Path, log: &Path, original_wallpaper: &Path) {
    let name = root.join("caelestia-name");
    let flavour = root.join("caelestia-flavour");
    let mode = root.join("caelestia-mode");
    let variant = root.join("caelestia-variant");
    let wallpaper = root.join("caelestia-wallpaper");
    fs::write(&name, "shadotheme").unwrap();
    fs::write(&flavour, "default").unwrap();
    fs::write(&mode, "dark").unwrap();
    fs::write(&variant, "tonalspot").unwrap();
    fs::write(&wallpaper, original_wallpaper.to_string_lossy().as_bytes()).unwrap();

    let script = format!(
        r#"#!/bin/sh
name='{name}'
flavour='{flavour}'
mode='{mode}'
variant='{variant}'
wallpaper='{wallpaper}'
log='{log}'

if [ "$1" = "wallpaper" ] && [ "$#" -eq 1 ]; then
    if [ -s "$wallpaper" ]; then /bin/cat "$wallpaper"; else echo "No wallpaper set"; fi
    exit 0
fi
if [ "$1" = "wallpaper" ] && [ "$2" = "--file" ]; then
    printf '%s\n' "$*" >> "$log"
    printf '%s' "$3" > "$wallpaper"
    exit 0
fi
if [ "$1" = "scheme" ] && [ "$2" = "get" ]; then
    case "$3" in
        --name) /bin/cat "$name" ;;
        --flavour) /bin/cat "$flavour" ;;
        --mode) /bin/cat "$mode" ;;
        --variant) /bin/cat "$variant" ;;
        *) exit 2 ;;
    esac
    exit 0
fi
if [ "$1" = "scheme" ] && [ "$2" = "set" ]; then
    printf '%s\n' "$*" >> "$log"
    shift 2
    while [ "$#" -gt 1 ]; do
        case "$1" in
            --name) printf '%s' "$2" > "$name" ;;
            --flavour) printf '%s' "$2" > "$flavour" ;;
            --mode) printf '%s' "$2" > "$mode" ;;
            --variant) printf '%s' "$2" > "$variant" ;;
        esac
        shift 2
    done
    exit 0
fi
exit 2
"#,
        name = name.display(),
        flavour = flavour.display(),
        mode = mode.display(),
        variant = variant.display(),
        wallpaper = wallpaper.display(),
        log = log.display(),
    );
    let path = root.join("caelestia");
    fs::write(&path, script).unwrap();
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}

fn test_image(path: &Path) {
    ImageBuffer::from_fn(48, 32, |x, _| {
        if x < 32 {
            Rgb([32_u8, 140, 230])
        } else {
            Rgb([225_u8, 60, 145])
        }
    })
    .save(path)
    .unwrap();
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
fn appearance_capabilities_detect_caelestia_without_applying_changes() {
    let root = temp_root();
    fs::create_dir_all(&root).unwrap();
    fake_command(&root, "caelestia");
    fake_command(&root, "hyprctl");
    let output = Command::new(env!("CARGO_BIN_EXE_kitsune-compositor"))
        .args(["appearance", "capabilities", "--contract-v1"])
        .env("PATH", &root)
        .env("XDG_CURRENT_DESKTOP", "Hyprland")
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["command"], "appearance capabilities");
    assert_eq!(value["data"]["backend"], "caelestia");
    assert_eq!(value["data"]["mode"], "native_palette");
    assert_eq!(value["data"]["palette_supported"], true);
    assert_eq!(value["data"]["preview_supported"], true);
    assert_eq!(value["data"]["apply_supported"], true);
    assert_eq!(value["data"]["restore_supported"], true);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn appearance_apply_dry_run_never_invokes_caelestia() {
    let root = temp_root();
    fs::create_dir_all(&root).unwrap();
    let image = root.join("wallpaper.png");
    let original = root.join("original.png");
    let log = root.join("caelestia.log");
    let state = root.join("appearance.json");
    test_image(&image);
    test_image(&original);
    fake_caelestia(&root, &log, &original);
    fake_command(&root, "hyprctl");

    let output = Command::new(env!("CARGO_BIN_EXE_kitsune-compositor"))
        .args([
            "appearance",
            "apply",
            "--image",
            image.to_str().unwrap(),
            "--dry-run",
            "--contract-v1",
        ])
        .env("HOME", &root)
        .env("PATH", &root)
        .env("XDG_CURRENT_DESKTOP", "Hyprland")
        .env("KITSUNE_COMPOSITOR_PALETTE_CACHE", root.join("cache"))
        .env("KITSUNE_COMPOSITOR_APPEARANCE_STATE", &state)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["command"], "appearance apply");
    assert_eq!(value["data"]["dry_run"], true);
    assert_eq!(value["data"]["applied"], false);
    assert_eq!(
        value["data"]["plan"]["operations"][0]["args"][0],
        "wallpaper"
    );
    assert!(!log.exists());
    assert!(!state.exists());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn active_media_output_resolves_the_registered_representative_image() {
    let root = temp_root();
    fs::create_dir_all(&root).unwrap();
    let image = root.join("wallpaper.png");
    let second_image = root.join("wallpaper-second.png");
    let original = root.join("original.png");
    let log = root.join("caelestia.log");
    let media_state = root.join("active-media.json");
    let appearance_state = root.join("appearance.json");
    let policy_state = root.join("appearance-policy.json");
    test_image(&image);
    test_image(&second_image);
    test_image(&original);
    fake_caelestia(&root, &log, &original);
    fake_hyprctl(&root);

    let publish = Command::new(env!("CARGO_BIN_EXE_kitsune-compositor"))
        .args([
            "active-media",
            "publish",
            "--output",
            "DP-1",
            "--owner",
            "kitowall",
            "--kind",
            "static",
            "--source",
            image.to_str().unwrap(),
            "--contract-v1",
        ])
        .env("PATH", &root)
        .env("HYPRLAND_INSTANCE_SIGNATURE", "test")
        .env("KITSUNE_COMPOSITOR_ACTIVE_MEDIA_STATE", &media_state)
        .env("KITSUNE_COMPOSITOR_APPEARANCE_POLICY", &policy_state)
        .output()
        .unwrap();
    assert!(publish.status.success());

    let preview = Command::new(env!("CARGO_BIN_EXE_kitsune-compositor"))
        .args(["appearance", "preview", "--output", "DP-1", "--contract-v1"])
        .env("HOME", &root)
        .env("PATH", &root)
        .env("XDG_CURRENT_DESKTOP", "Hyprland")
        .env("KITSUNE_COMPOSITOR_PALETTE_CACHE", root.join("cache"))
        .env("KITSUNE_COMPOSITOR_ACTIVE_MEDIA_STATE", &media_state)
        .env("KITSUNE_COMPOSITOR_APPEARANCE_POLICY", &policy_state)
        .output()
        .unwrap();
    assert!(preview.status.success());
    let preview: Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(preview["data"]["source_output"], "DP-1");
    assert_eq!(
        preview["data"]["image"],
        image.canonicalize().unwrap().to_string_lossy().as_ref()
    );

    let apply = Command::new(env!("CARGO_BIN_EXE_kitsune-compositor"))
        .args([
            "appearance",
            "apply",
            "--output",
            "DP-1",
            "--dry-run",
            "--contract-v1",
        ])
        .env("HOME", &root)
        .env("PATH", &root)
        .env("XDG_CURRENT_DESKTOP", "Hyprland")
        .env("KITSUNE_COMPOSITOR_PALETTE_CACHE", root.join("cache"))
        .env("KITSUNE_COMPOSITOR_ACTIVE_MEDIA_STATE", &media_state)
        .env("KITSUNE_COMPOSITOR_APPEARANCE_POLICY", &policy_state)
        .output()
        .unwrap();
    assert!(
        apply.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&apply.stdout),
        String::from_utf8_lossy(&apply.stderr)
    );
    let value: Value = serde_json::from_slice(&apply.stdout).unwrap();
    assert_eq!(value["data"]["plan"]["source_output"], "DP-1");
    assert_eq!(
        value["data"]["plan"]["image"],
        image.canonicalize().unwrap().to_string_lossy().as_ref()
    );
    assert!(!log.exists());

    let enable = Command::new(env!("CARGO_BIN_EXE_kitsune-compositor"))
        .args([
            "appearance",
            "policy",
            "enable",
            "--output",
            "DP-1",
            "--confirm",
            "--contract-v1",
        ])
        .env("HOME", &root)
        .env("PATH", &root)
        .env("XDG_CURRENT_DESKTOP", "Hyprland")
        .env("KITSUNE_COMPOSITOR_PALETTE_CACHE", root.join("cache"))
        .env("KITSUNE_COMPOSITOR_ACTIVE_MEDIA_STATE", &media_state)
        .env("KITSUNE_COMPOSITOR_APPEARANCE_STATE", &appearance_state)
        .env("KITSUNE_COMPOSITOR_APPEARANCE_POLICY", &policy_state)
        .output()
        .unwrap();
    assert!(
        enable.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&enable.stdout),
        String::from_utf8_lossy(&enable.stderr)
    );
    let enabled: Value = serde_json::from_slice(&enable.stdout).unwrap();
    assert_eq!(enabled["data"]["policy"]["source_output"], "DP-1");
    assert_eq!(enabled["data"]["initial_apply"]["applied"], true);
    fs::write(&log, "").unwrap();

    let rotate = Command::new(env!("CARGO_BIN_EXE_kitsune-compositor"))
        .args([
            "active-media",
            "publish",
            "--output",
            "DP-1",
            "--owner",
            "kitowall",
            "--kind",
            "static",
            "--source",
            second_image.to_str().unwrap(),
            "--contract-v1",
        ])
        .env("HOME", &root)
        .env("PATH", &root)
        .env("XDG_CURRENT_DESKTOP", "Hyprland")
        .env("HYPRLAND_INSTANCE_SIGNATURE", "test")
        .env("KITSUNE_COMPOSITOR_PALETTE_CACHE", root.join("cache"))
        .env("KITSUNE_COMPOSITOR_ACTIVE_MEDIA_STATE", &media_state)
        .env("KITSUNE_COMPOSITOR_APPEARANCE_STATE", &appearance_state)
        .env("KITSUNE_COMPOSITOR_APPEARANCE_POLICY", &policy_state)
        .output()
        .unwrap();
    assert!(
        rotate.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&rotate.stdout),
        String::from_utf8_lossy(&rotate.stderr)
    );
    let rotated: Value = serde_json::from_slice(&rotate.stdout).unwrap();
    assert_eq!(rotated["data"]["appearance_sync"]["applied"], true);
    assert_eq!(
        fs::read_to_string(root.join("caelestia-wallpaper")).unwrap(),
        second_image.to_string_lossy()
    );

    fs::write(&log, "").unwrap();
    let repeated = Command::new(env!("CARGO_BIN_EXE_kitsune-compositor"))
        .args([
            "active-media",
            "publish",
            "--output",
            "DP-1",
            "--owner",
            "kitowall",
            "--kind",
            "static",
            "--source",
            second_image.to_str().unwrap(),
            "--contract-v1",
        ])
        .env("HOME", &root)
        .env("PATH", &root)
        .env("XDG_CURRENT_DESKTOP", "Hyprland")
        .env("HYPRLAND_INSTANCE_SIGNATURE", "test")
        .env("KITSUNE_COMPOSITOR_PALETTE_CACHE", root.join("cache"))
        .env("KITSUNE_COMPOSITOR_ACTIVE_MEDIA_STATE", &media_state)
        .env("KITSUNE_COMPOSITOR_APPEARANCE_STATE", &appearance_state)
        .env("KITSUNE_COMPOSITOR_APPEARANCE_POLICY", &policy_state)
        .output()
        .unwrap();
    assert!(repeated.status.success());
    let repeated: Value = serde_json::from_slice(&repeated.stdout).unwrap();
    assert!(repeated["data"]["appearance_sync"].is_null());
    assert_eq!(fs::read_to_string(&log).unwrap(), "");
    let _ = fs::remove_dir_all(root);
}

#[test]
fn appearance_apply_and_restore_preserve_the_original_caelestia_state() {
    let root = temp_root();
    fs::create_dir_all(&root).unwrap();
    let image = root.join("wallpaper.png");
    let second_image = root.join("wallpaper-second.png");
    let original = root.join("original.png");
    let log = root.join("caelestia.log");
    let state = root.join("appearance.json");
    test_image(&image);
    test_image(&second_image);
    test_image(&original);
    fake_caelestia(&root, &log, &original);
    fake_command(&root, "hyprctl");

    let run = |arguments: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_kitsune-compositor"))
            .args(arguments)
            .env("HOME", &root)
            .env("PATH", &root)
            .env("XDG_CURRENT_DESKTOP", "Hyprland")
            .env("KITSUNE_COMPOSITOR_PALETTE_CACHE", root.join("cache"))
            .env("KITSUNE_COMPOSITOR_APPEARANCE_STATE", &state)
            .output()
            .unwrap()
    };

    let apply = run(&[
        "appearance",
        "apply",
        "--image",
        image.to_str().unwrap(),
        "--confirm",
        "--contract-v1",
    ]);
    assert!(
        apply.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&apply.stdout),
        String::from_utf8_lossy(&apply.stderr),
    );
    let applied: Value = serde_json::from_slice(&apply.stdout).unwrap();
    assert_eq!(applied["data"]["applied"], true);
    assert_eq!(
        applied["data"]["state"]["previous_caelestia"]["name"],
        "shadotheme"
    );
    assert_eq!(
        fs::read_to_string(root.join("caelestia-name")).unwrap(),
        "dynamic"
    );
    assert_eq!(
        fs::read_to_string(root.join("caelestia-wallpaper")).unwrap(),
        image.to_string_lossy()
    );
    assert!(state.exists());

    let second_apply = run(&[
        "appearance",
        "apply",
        "--image",
        second_image.to_str().unwrap(),
        "--confirm",
        "--contract-v1",
    ]);
    assert!(
        second_apply.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&second_apply.stdout),
        String::from_utf8_lossy(&second_apply.stderr),
    );
    let second_applied: Value = serde_json::from_slice(&second_apply.stdout).unwrap();
    assert_eq!(
        second_applied["data"]["state"]["previous_caelestia"]["name"],
        "shadotheme"
    );
    assert_eq!(
        second_applied["data"]["state"]["previous_caelestia"]["wallpaper"],
        original.to_string_lossy().as_ref()
    );
    assert_eq!(
        fs::read_to_string(root.join("caelestia-wallpaper")).unwrap(),
        second_image.to_string_lossy()
    );

    fs::write(root.join("caelestia-name"), "manual-theme").unwrap();
    let conflict = run(&["appearance", "restore", "--confirm", "--contract-v1"]);
    assert_eq!(conflict.status.code(), Some(6));
    let conflict: Value = serde_json::from_slice(&conflict.stdout).unwrap();
    assert_eq!(conflict["error"]["code"], "STATE_CONFLICT");
    assert!(state.exists());
    fs::write(root.join("caelestia-name"), "dynamic").unwrap();

    let restore = run(&["appearance", "restore", "--confirm", "--contract-v1"]);
    assert!(
        restore.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&restore.stdout),
        String::from_utf8_lossy(&restore.stderr),
    );
    let restored: Value = serde_json::from_slice(&restore.stdout).unwrap();
    assert_eq!(restored["data"]["restored"], true);
    assert_eq!(
        fs::read_to_string(root.join("caelestia-name")).unwrap(),
        "shadotheme"
    );
    assert_eq!(
        fs::read_to_string(root.join("caelestia-wallpaper")).unwrap(),
        original.to_string_lossy()
    );
    assert!(!state.exists());

    let commands = fs::read_to_string(log).unwrap();
    assert!(commands.contains(&format!("wallpaper --file {}", image.display())));
    assert!(commands.contains(&format!("wallpaper --file {}", second_image.display())));
    assert!(commands.contains("scheme set --name dynamic"));
    assert!(commands.contains("scheme set --name shadotheme"));
    assert!(commands.contains(&format!("wallpaper --file {}", original.display())));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn appearance_preview_returns_a_cached_normalized_palette() {
    let root = temp_root();
    let bin = root.join("bin");
    let cache = root.join("cache");
    fs::create_dir_all(&bin).unwrap();
    fake_command(&bin, "hyprctl");
    let image_path = root.join("wallpaper.png");
    let image = ImageBuffer::from_fn(80, 40, |x, _| {
        if x < 60 {
            Rgb([20_u8, 95, 220])
        } else {
            Rgb([240_u8, 65, 155])
        }
    });
    image.save(&image_path).unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_kitsune-compositor"))
            .args([
                "appearance",
                "preview",
                "--image",
                image_path.to_str().unwrap(),
                "--contract-v1",
            ])
            .env("HOME", &root)
            .env("PATH", &bin)
            .env("XDG_CURRENT_DESKTOP", "Hyprland")
            .env("KITSUNE_COMPOSITOR_PALETTE_CACHE", &cache)
            .output()
            .unwrap()
    };
    let first = run();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(first["command"], "appearance preview");
    assert_eq!(first["data"]["cache_hit"], false);
    assert!(
        first["data"]["palette"]["candidates"]
            .as_array()
            .unwrap()
            .len()
            >= 2
    );
    assert_eq!(first["data"]["provider"]["backend"], "hyprland");
    let second = run();
    assert!(second.status.success());
    let second: Value = serde_json::from_slice(&second.stdout).unwrap();
    assert_eq!(second["data"]["cache_hit"], true);
    assert_eq!(first["data"]["palette"], second["data"]["palette"]);
    let _ = fs::remove_dir_all(root);
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
        .env(
            "KITSUNE_COMPOSITOR_ACTIVE_MEDIA_STATE",
            root.join("active-media.json"),
        )
        .env(
            "KITSUNE_COMPOSITOR_APPEARANCE_POLICY",
            root.join("appearance-policy.json"),
        )
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
    assert_eq!(value["data"]["active_media"]["owner"], "kitowall");
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

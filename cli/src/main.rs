use kitsune_compositor_backend::{
    AutomationBatchDescriptor, AutomationDescriptor, SystemProcessExecutor, WallpaperApplyRequest,
    WallpaperRuntime, WallpaperTransition, application_matches, control_automation,
    detect_service_manager, plan_automation, plan_automation_batch, remove_automation,
};
use kitsune_compositor_backend::{CompositorBackend, EventTracker, HostRunner, SystemHostRunner};
use kitsune_compositor_backend::{UnitDescriptor, UnitManager};
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::process;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const CONTRACT_VERSION: &str = "1.0";

#[derive(Serialize)]
struct Meta {
    cli: &'static str,
    cli_version: &'static str,
    contract_version: &'static str,
}

#[derive(Serialize)]
struct Success<T: Serialize> {
    schema_version: u8,
    ok: bool,
    command: String,
    data: T,
    warnings: Vec<String>,
    meta: Meta,
}

#[derive(Serialize)]
struct Failure {
    schema_version: u8,
    ok: bool,
    command: String,
    error: ErrorBody,
    meta: Meta,
}

#[derive(Serialize)]
struct ErrorBody {
    code: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<String>,
}

#[derive(Serialize)]
struct EventEnvelope<T: Serialize> {
    schema_version: u8,
    event: &'static str,
    sequence: u64,
    timestamp_unix_ms: u128,
    data: T,
    meta: Meta,
}

fn meta() -> Meta {
    Meta {
        cli: "kitsune-compositor",
        cli_version: env!("CARGO_PKG_VERSION"),
        contract_version: CONTRACT_VERSION,
    }
}

fn usage() -> &'static str {
    "kitsune-compositor <command> [options] [--lc]\n\n\
Global options:\n\
  --lc  Resolve integrations from the local refactor workspace\n\n\
Commands:\n\
  detect [--json] [--contract-v1]\n\
  outputs [--json] [--contract-v1]\n\
  focused-output [--json] [--contract-v1]\n\
  validate-output <name> [--json] [--contract-v1]\n\
  applications list|running [--json] [--contract-v1]\n\
  applications match --ids <desktop-id,...> [--json] [--contract-v1]\n\
  watch outputs|focus [--json-lines] [--contract-v1] [--poll-ms <n>] [--once]\n\
  wallpaper status --namespace <name> [--json] [--contract-v1]\n\
  wallpaper start|stop|serve --namespace <name> [--json] [--contract-v1]\n\
  wallpaper apply --namespace <name> --output <name> --image <absolute-path> [transition options]\n\
  automation plan|apply --descriptor <absolute-json-path> [--json] [--contract-v1]\n\
  automation plan-batch|apply-batch --descriptor <absolute-json-path> [--json] [--contract-v1]\n\
  automation status|start|stop|restart|enable|disable|remove --id <id> [--contract-v1]\n\
  service plan|apply --descriptor <absolute-json-path> [--json] [--contract-v1]\n\
  service remove --id <id> [--json] [--contract-v1]\n\
  service list [--json] [--contract-v1]\n\
  service start|stop|restart|enable|disable|status --id <id> [--json] [--contract-v1]\n\
  status [--json] [--contract-v1]\n\
  doctor [--json] [--contract-v1]\n\
  capabilities [--json] [--contract-v1]\n\
  config show [--json] [--contract-v1]\n\
  version | --version | -v [--json] [--contract-v1]\n"
}

fn main() {
    let mut args = std::env::args().collect::<Vec<_>>();
    args.retain(|arg| arg != "--lc");
    let command = args.get(1).map(String::as_str).unwrap_or("help");
    let contract = args.iter().any(|arg| arg == "--contract-v1");
    let json = contract || args.iter().any(|arg| arg == "--json");
    if let Err(error) = dispatch(&args, json, contract) {
        let (code, exit, hint) = classify(&error);
        if contract {
            print_json(&Failure {
                schema_version: 1,
                ok: false,
                command: command.into(),
                error: ErrorBody {
                    code: code.into(),
                    message: error,
                    hint,
                },
                meta: meta(),
            });
        } else {
            eprintln!("kitsune-compositor: {error}");
        }
        process::exit(exit);
    }
}

fn dispatch(args: &[String], json: bool, contract: bool) -> Result<(), String> {
    let backend = CompositorBackend::new(SystemHostRunner);
    let command = args.get(1).map(String::as_str);
    match command {
        None | Some("help" | "--help" | "-h") => print!("{}", usage()),
        Some("version" | "--version" | "-v") => {
            if json {
                emit_json(
                    "version",
                    serde_json::json!({"version": env!("CARGO_PKG_VERSION")}),
                    contract,
                );
            } else {
                println!("{}", env!("CARGO_PKG_VERSION"));
            }
        }
        Some("detect") => {
            let detection = backend.detect();
            if json {
                emit_json("detect", &detection, contract);
            } else {
                println!("ok: {}", detection.ok);
                println!("compositor: {}", detection.compositor.as_str());
                println!("backend: {}", detection.backend.as_deref().unwrap_or("-"));
                println!("reason: {}", detection.reason);
            }
        }
        Some("outputs") => {
            let (compositor, outputs) = backend.outputs()?;
            if json {
                emit_json(
                    "outputs",
                    serde_json::json!({"ok": true, "compositor": compositor, "outputs": outputs}),
                    contract,
                );
            } else {
                for output in outputs {
                    println!(
                        "{}  active={} focused={} {}x{} backend={}",
                        output.name,
                        output.active,
                        output.focused,
                        output.width.unwrap_or(0),
                        output.height.unwrap_or(0),
                        output.backend
                    );
                }
            }
        }
        Some("focused-output") => {
            let (compositor, output) = backend.focused_output()?;
            if json {
                emit_json(
                    "focused-output",
                    serde_json::json!({"ok": output.is_some(), "compositor": compositor, "output": output}),
                    contract,
                );
            } else if let Some(output) = output {
                println!("{}", output.name);
            } else {
                return Err("no focused output available".into());
            }
        }
        Some("validate-output") => {
            let name = args
                .get(2)
                .ok_or_else(|| "missing output name".to_string())?;
            let (compositor, exists) = backend.validate_output(name)?;
            if json {
                emit_json(
                    "validate-output",
                    serde_json::json!({"ok": exists, "compositor": compositor, "name": name, "exists": exists}),
                    contract,
                );
            } else {
                println!("{exists}");
            }
        }
        Some("watch") => run_watch(args, &backend)?,
        Some("applications") => run_applications(args, &backend, json, contract)?,
        Some("wallpaper") => run_wallpaper(args, &backend, json, contract)?,
        Some("automation") => run_automation(args, json, contract)?,
        Some("service") => run_service(args, json, contract)?,
        Some("status") => {
            let status = backend.status()?;
            if json {
                emit_json("status", &status, contract);
            } else {
                println!("compositor: {}", status.detection.compositor.as_str());
                println!("reason: {}", status.detection.reason);
                println!("outputs: {}", status.outputs.len());
                println!(
                    "focused: {}",
                    status
                        .focused_output
                        .as_ref()
                        .map(|output| output.name.as_str())
                        .unwrap_or("-")
                );
            }
        }
        Some("doctor") => {
            let report = backend.doctor();
            if json {
                emit_json("doctor", &report, contract);
            } else {
                for check in report.checks {
                    println!(
                        "{} {}: {}",
                        if check.ok { "[ok]" } else { "[x]" },
                        check.id,
                        check.message
                    );
                }
            }
        }
        Some("capabilities") => {
            let mut capabilities = backend.capabilities();
            capabilities.wallpaper_runtime = WallpaperRuntime::new(SystemProcessExecutor)
                .available_backend()
                .is_some();
            capabilities.service_runtime = unit_manager().available();
            if json {
                emit_json("capabilities", &capabilities, contract);
            } else {
                println!("compositor: {}", capabilities.compositor.as_str());
                println!("multi_output: {}", capabilities.multi_output);
                println!("output_focus: {}", capabilities.output_focus);
                println!("output_events: {}", capabilities.output_events);
                println!("focus_events: {}", capabilities.focus_events);
                println!("application_catalog: {}", capabilities.application_catalog);
                println!("application_runtime: {}", capabilities.application_runtime);
                println!("wallpaper_runtime: {}", capabilities.wallpaper_runtime);
                println!("service_runtime: {}", capabilities.service_runtime);
            }
        }
        Some("config") if args.get(2).map(String::as_str) == Some("show") => {
            let data = serde_json::json!({
                "supported": false,
                "persistent": false,
                "reason": "persistent configuration is not implemented"
            });
            if json {
                emit_json("config show", data, contract);
            } else {
                println!("persistent config: unsupported");
            }
        }
        Some("config") => return Err("invalid config command (use: config show)".into()),
        Some(other) => return Err(format!("unknown command: {other}")),
    }
    Ok(())
}

fn run_applications(
    args: &[String],
    backend: &CompositorBackend<SystemHostRunner>,
    json: bool,
    contract: bool,
) -> Result<(), String> {
    let action = args
        .get(2)
        .map(String::as_str)
        .ok_or_else(|| "missing applications action".to_string())?;
    match action {
        "list" => {
            let applications = backend.installed_applications();
            if json {
                emit_json(
                    "applications list",
                    serde_json::json!({"applications": applications, "count": applications.len()}),
                    contract,
                );
            } else {
                for application in applications {
                    println!("{}\t{}", application.id, application.name);
                }
            }
        }
        "running" => {
            let applications = backend.running_applications();
            if json {
                emit_json(
                    "applications running",
                    serde_json::json!({"applications": applications, "count": applications.len()}),
                    contract,
                );
            } else {
                for application in applications {
                    println!(
                        "{}\t{}\t{}",
                        application.id, application.name, application.backend
                    );
                }
            }
        }
        "match" => {
            let ids = required_option(args, "--ids")?
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>();
            if ids.is_empty() {
                return Err("--ids requires at least one application id".into());
            }
            let applications = application_matches(&backend.running_applications(), &ids);
            if json {
                emit_json(
                    "applications match",
                    serde_json::json!({
                        "matched": !applications.is_empty(),
                        "applications": applications,
                        "count": applications.len()
                    }),
                    contract,
                );
            } else {
                println!("{}", !applications.is_empty());
            }
        }
        _ => {
            return Err(
                "invalid applications action (use: list, running or match --ids <ids>)".into(),
            );
        }
    }
    Ok(())
}

fn run_automation(args: &[String], json: bool, contract: bool) -> Result<(), String> {
    let action = args
        .get(2)
        .map(String::as_str)
        .ok_or_else(|| "missing automation action".to_string())?;
    let manager = unit_manager();
    if matches!(
        action,
        "status" | "start" | "stop" | "restart" | "enable" | "disable"
    ) {
        let id = required_option(args, "--id")?;
        emit_service_result(
            &format!("automation {action}"),
            control_automation(&manager, id, action)?,
            json,
            contract,
        );
        return Ok(());
    }
    if action == "remove" {
        let id = required_option(args, "--id")?;
        emit_service_result(
            "automation remove",
            serde_json::json!({"automation_id": id, "removed": remove_automation(&manager, id)?}),
            json,
            contract,
        );
        return Ok(());
    }
    if matches!(action, "plan-batch" | "apply-batch") {
        let descriptor = read_automation_batch_descriptor(required_option(args, "--descriptor")?)?;
        let manager_kind = detect_service_manager(&SystemProcessExecutor)?;
        let plans = plan_automation_batch(&descriptor, manager_kind)?;
        let automation_ids = plans
            .iter()
            .map(|plan| plan.automation_id.clone())
            .collect::<Vec<_>>();
        let artifacts = plans
            .iter()
            .flat_map(|plan| plan.artifacts.iter().cloned())
            .collect::<Vec<_>>();
        if action == "plan-batch" {
            let rendered = artifacts
                .iter()
                .map(|artifact| manager.plan(artifact))
                .collect::<Result<Vec<_>, _>>()?;
            emit_service_result(
                "automation plan-batch",
                serde_json::json!({
                    "manager": manager_kind,
                    "automation_ids": automation_ids,
                    "artifacts": rendered,
                    "activation_required": false
                }),
                json,
                contract,
            );
        } else {
            let records = manager.apply_batch(&artifacts)?;
            emit_service_result(
                "automation apply-batch",
                serde_json::json!({
                    "manager": manager_kind,
                    "automation_ids": automation_ids,
                    "artifacts": records,
                    "activation_required": true
                }),
                json,
                contract,
            );
        }
        return Ok(());
    }
    if !matches!(action, "plan" | "apply") {
        return Err("unsupported automation action".into());
    }
    let descriptor = read_automation_descriptor(required_option(args, "--descriptor")?)?;
    let manager_kind = detect_service_manager(&SystemProcessExecutor)?;
    let plan = plan_automation(&descriptor, manager_kind)?;
    if action == "plan" {
        let rendered = plan
            .artifacts
            .iter()
            .map(|artifact| manager.plan(artifact))
            .collect::<Result<Vec<_>, _>>()?;
        emit_service_result(
            "automation plan",
            serde_json::json!({
                "manager": plan.manager,
                "automation_id": plan.automation_id,
                "artifacts": rendered
            }),
            json,
            contract,
        );
        return Ok(());
    }

    let records = manager.apply_batch(&plan.artifacts)?;
    if descriptor.schedule.is_some() {
        manager.control(&format!("{}-timer", descriptor.id), "enable")?;
    } else if descriptor.autostart {
        manager.control(&descriptor.id, "enable")?;
    }
    emit_service_result(
        "automation apply",
        serde_json::json!({
            "manager": plan.manager,
            "automation_id": plan.automation_id,
            "artifacts": records
        }),
        json,
        contract,
    );
    Ok(())
}

fn run_service(args: &[String], json: bool, contract: bool) -> Result<(), String> {
    let action = args
        .get(2)
        .map(String::as_str)
        .ok_or_else(|| "missing service action".to_string())?;
    let manager = unit_manager();
    match action {
        "plan" | "apply" => {
            let descriptor = read_descriptor(required_option(args, "--descriptor")?)?;
            if action == "plan" {
                emit_service_result("service plan", manager.plan(&descriptor)?, json, contract);
            } else {
                emit_service_result("service apply", manager.apply(&descriptor)?, json, contract);
            }
        }
        "remove" => {
            let id = required_option(args, "--id")?;
            emit_service_result(
                "service remove",
                serde_json::json!({"id": id, "removed": manager.remove(id)?}),
                json,
                contract,
            );
        }
        "list" => emit_service_result("service list", manager.list()?, json, contract),
        "start" | "stop" | "restart" | "enable" | "disable" | "status" => {
            let id = required_option(args, "--id")?;
            emit_service_result(
                &format!("service {action}"),
                manager.control(id, action)?,
                json,
                contract,
            );
        }
        _ => return Err(format!("unsupported service action: {action}")),
    }
    Ok(())
}

fn unit_manager() -> UnitManager<SystemProcessExecutor> {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let unit_root = std::env::var("KITSUNE_COMPOSITOR_SYSTEMD_USER_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(&home).join(".config/systemd/user"));
    let registry = std::env::var("KITSUNE_COMPOSITOR_SERVICE_REGISTRY")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from(&home).join(".local/state/kitsune-compositor/services.json")
        });
    UnitManager::new(SystemProcessExecutor, unit_root, registry)
}

fn read_descriptor(path: &str) -> Result<UnitDescriptor, String> {
    let path = PathBuf::from(path);
    if !path.is_absolute() || !path.is_file() {
        return Err("service descriptor must be an existing absolute file".into());
    }
    let bytes =
        fs::read(&path).map_err(|error| format!("failed to read service descriptor: {error}"))?;
    serde_json::from_slice(&bytes).map_err(|error| format!("invalid service descriptor: {error}"))
}

fn read_automation_descriptor(path: &str) -> Result<AutomationDescriptor, String> {
    let path = PathBuf::from(path);
    if !path.is_absolute() || !path.is_file() {
        return Err("automation descriptor must be an existing absolute file".into());
    }
    let bytes = fs::read(&path)
        .map_err(|error| format!("failed to read automation descriptor: {error}"))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid automation descriptor: {error}"))
}

fn read_automation_batch_descriptor(path: &str) -> Result<AutomationBatchDescriptor, String> {
    let path = PathBuf::from(path);
    if !path.is_absolute() || !path.is_file() {
        return Err("automation batch descriptor must be an existing absolute file".into());
    }
    let bytes = fs::read(&path)
        .map_err(|error| format!("failed to read automation batch descriptor: {error}"))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("invalid automation batch descriptor: {error}"))
}

fn emit_service_result<T: Serialize>(command: &str, data: T, json: bool, contract: bool) {
    if json {
        emit_json(command, data, contract);
    } else {
        print_json(&data);
    }
}

fn run_wallpaper<R: HostRunner>(
    args: &[String],
    backend: &CompositorBackend<R>,
    json: bool,
    contract: bool,
) -> Result<(), String> {
    let action = args
        .get(2)
        .map(String::as_str)
        .ok_or_else(|| "missing wallpaper action (status|start|stop|apply)".to_string())?;
    let namespace = required_option(args, "--namespace")?;
    let runtime = WallpaperRuntime::new(SystemProcessExecutor);
    match action {
        "status" => emit_wallpaper_result(
            "wallpaper status",
            runtime.status(namespace)?,
            json,
            contract,
        ),
        "start" => {
            emit_wallpaper_result("wallpaper start", runtime.start(namespace)?, json, contract)
        }
        "serve" => {
            runtime.serve(namespace)?;
            emit_wallpaper_result(
                "wallpaper serve",
                serde_json::json!({ "namespace": namespace, "stopped": true }),
                json,
                contract,
            )
        }
        "stop" => emit_wallpaper_result("wallpaper stop", runtime.stop(namespace)?, json, contract),
        "apply" => {
            let output = required_option(args, "--output")?;
            let image = required_option(args, "--image")?;
            let (_, exists) = backend.validate_output(output)?;
            if !exists {
                return Err(format!("output not found: {output}"));
            }
            let transition = WallpaperTransition {
                kind: option_value(args, "--transition-type")
                    .unwrap_or("simple")
                    .into(),
                fps: parse_option(args, "--transition-fps")?.unwrap_or(60),
                duration: parse_option(args, "--transition-duration")?.unwrap_or(0.7),
                angle: parse_option(args, "--transition-angle")?,
                position: option_value(args, "--transition-pos").map(str::to_string),
            };
            let status = runtime.apply(&WallpaperApplyRequest {
                namespace: namespace.into(),
                output: output.into(),
                image: PathBuf::from(image),
                transition,
            })?;
            if json {
                emit_json(
                    "wallpaper apply",
                    serde_json::json!({
                        "runtime": status,
                        "output": output,
                        "image": image
                    }),
                    contract,
                );
            } else {
                println!("applied output={output} image={image}");
            }
        }
        _ => return Err("wallpaper action must be status, start, serve, stop or apply".into()),
    }
    Ok(())
}

fn emit_wallpaper_result<T: Serialize>(command: &str, data: T, json: bool, contract: bool) {
    if json {
        emit_json(command, data, contract);
    } else {
        print_json(&data);
    }
}

fn run_watch<R: HostRunner>(args: &[String], backend: &CompositorBackend<R>) -> Result<(), String> {
    let target = args
        .get(2)
        .map(String::as_str)
        .ok_or_else(|| "missing watch target (outputs|focus)".to_string())?;
    if !matches!(target, "outputs" | "focus") {
        return Err("watch target must be outputs or focus".into());
    }
    let poll_ms = option_value(args, "--poll-ms")
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| "--poll-ms must be an integer".to_string())
        })
        .transpose()?
        .unwrap_or(1000);
    if !(100..=60_000).contains(&poll_ms) {
        return Err("--poll-ms must be between 100 and 60000".into());
    }
    let once = args.iter().any(|arg| arg == "--once");
    let json_lines = args
        .iter()
        .any(|arg| matches!(arg.as_str(), "--json-lines" | "--contract-v1"));
    let mut tracker = EventTracker::default();
    let mut sequence = 0_u64;

    loop {
        let status = backend.status()?;
        let changed = if target == "outputs" {
            tracker.observe_outputs(&status.outputs)
        } else {
            tracker.observe_focus(status.focused_output.as_ref())
        };
        if changed {
            sequence += 1;
            let event = if target == "outputs" {
                EventEnvelope {
                    schema_version: 1,
                    event: "outputs_changed",
                    sequence,
                    timestamp_unix_ms: now_unix_ms(),
                    data: serde_json::json!({
                        "compositor": status.detection.compositor,
                        "outputs": status.outputs
                    }),
                    meta: meta(),
                }
            } else {
                EventEnvelope {
                    schema_version: 1,
                    event: "focus_changed",
                    sequence,
                    timestamp_unix_ms: now_unix_ms(),
                    data: serde_json::json!({
                        "compositor": status.detection.compositor,
                        "output": status.focused_output
                    }),
                    meta: meta(),
                }
            };
            if json_lines {
                print_json_line(&event)?;
            } else if target == "outputs" {
                println!("outputs_changed sequence={sequence}");
            } else {
                println!("focus_changed sequence={sequence}");
            }
        }
        if once {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(poll_ms));
    }
}

fn option_value<'a>(args: &'a [String], key: &str) -> Option<&'a str> {
    args.iter()
        .position(|argument| argument == key)
        .and_then(|index| args.get(index + 1))
        .map(String::as_str)
}

fn required_option<'a>(args: &'a [String], key: &str) -> Result<&'a str, String> {
    option_value(args, key).ok_or_else(|| format!("missing required option: {key}"))
}

fn parse_option<T: std::str::FromStr>(args: &[String], key: &str) -> Result<Option<T>, String> {
    option_value(args, key)
        .map(|value| {
            value
                .parse::<T>()
                .map_err(|_| format!("invalid value for {key}: {value}"))
        })
        .transpose()
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn print_json_line<T: Serialize>(value: &T) -> Result<(), String> {
    let line = serde_json::to_string(value)
        .map_err(|error| format!("failed to serialize event: {error}"))?;
    println!("{line}");
    Ok(())
}

fn emit_json<T: Serialize>(command: &str, data: T, contract: bool) {
    if contract {
        print_json(&Success {
            schema_version: 1,
            ok: true,
            command: command.into(),
            data,
            warnings: Vec::new(),
            meta: meta(),
        });
    } else {
        print_json(&data);
    }
}

fn print_json<T: Serialize>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(value) => println!("{value}"),
        Err(error) => eprintln!("kitsune-compositor: failed to serialize response: {error}"),
    }
}

fn classify(error: &str) -> (&'static str, i32, Option<String>) {
    if error.starts_with("unknown command")
        || error.starts_with("missing output name")
        || error.starts_with("invalid config")
        || error.starts_with("output name cannot")
        || error.starts_with("missing watch target")
        || error.starts_with("missing applications action")
        || error.starts_with("invalid applications action")
        || error.starts_with("--ids requires")
        || error.starts_with("watch target")
        || error.starts_with("--poll-ms")
        || error.starts_with("missing wallpaper action")
        || error.starts_with("wallpaper action")
        || error.starts_with("missing required option")
        || error.starts_with("invalid value")
        || error.starts_with("invalid namespace")
        || error.starts_with("invalid output")
        || error.starts_with("invalid transition")
        || error.starts_with("transition ")
        || error.starts_with("wallpaper image path")
        || error.starts_with("missing service action")
        || error.starts_with("unsupported service action")
        || error.starts_with("service descriptor")
        || error.starts_with("invalid service descriptor")
        || error.starts_with("invalid service id")
        || error.starts_with("invalid unit name")
        || error.starts_with("unit name")
        || error.starts_with("service exec_start")
        || error.starts_with("service executable")
        || error.starts_with("timer requires")
        || error.starts_with("invalid systemd duration")
        || error.starts_with("service unit and registry paths")
    {
        return (
            "INVALID_ARGUMENT",
            2,
            Some("Run kitsune-compositor help".into()),
        );
    }
    if error.contains("no supported compositor") || error.contains("no focused output") {
        return ("COMPOSITOR_UNAVAILABLE", 5, None);
    }
    if error.starts_with("output not found") {
        return ("OUTPUT_NOT_FOUND", 3, None);
    }
    if error.contains("no supported wallpaper runtime") {
        return ("DEPENDENCY_MISSING", 4, None);
    }
    if error.contains("systemctl is not installed") {
        return ("DEPENDENCY_MISSING", 4, None);
    }
    if error.starts_with("service id not found") {
        return ("RESOURCE_NOT_FOUND", 3, None);
    }
    if error.starts_with("service id already owns")
        || error.starts_with("unit is already registered")
        || error.starts_with("timer target is not registered")
        || error.starts_with("unit is required by registered timer")
    {
        return ("STATE_CONFLICT", 6, None);
    }
    if error.starts_with("failed to read service registry")
        || error.starts_with("failed to parse service registry")
        || error.starts_with("failed to create unit directory")
        || error.starts_with("failed to write")
        || error.starts_with("failed to replace")
        || error.starts_with("failed to remove unit file")
        || error.starts_with("unsupported service registry schema")
        || error.starts_with("registered unit path")
    {
        return ("IO_ERROR", 7, None);
    }
    if error.contains("failed to execute") {
        return ("DEPENDENCY_MISSING", 4, None);
    }
    if error.contains("exited with status") || error.contains("invalid json") {
        return ("EXTERNAL_COMMAND_FAILED", 8, None);
    }
    ("INTERNAL_ERROR", 10, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_keep_contract_exit_codes() {
        assert_eq!(classify("unknown command: bad").1, 2);
        assert_eq!(classify("no supported compositor provider responded").1, 5);
        assert_eq!(classify("invalid json from hyprctl").1, 8);
    }

    #[test]
    fn success_envelope_has_v1_metadata() {
        let value = serde_json::to_value(Success {
            schema_version: 1,
            ok: true,
            command: "version".into(),
            data: serde_json::json!({"version": "0.1.0"}),
            warnings: Vec::new(),
            meta: meta(),
        })
        .unwrap();
        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["meta"]["contract_version"], CONTRACT_VERSION);
    }
}

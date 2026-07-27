use crate::ProcessExecutor;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RestartPolicy {
    #[default]
    No,
    OnFailure,
    Always,
}

impl RestartPolicy {
    fn as_systemd(self) -> &'static str {
        match self {
            Self::No => "no",
            Self::OnFailure => "on-failure",
            Self::Always => "always",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "unit_type", rename_all = "snake_case")]
pub enum UnitDescriptor {
    Service {
        id: String,
        unit_name: String,
        description: String,
        exec_start: Vec<String>,
        #[serde(default)]
        restart: RestartPolicy,
        #[serde(default = "default_target")]
        wanted_by: String,
    },
    Timer {
        id: String,
        unit_name: String,
        description: String,
        target_unit: String,
        #[serde(default)]
        on_boot_sec: Option<String>,
        #[serde(default)]
        on_unit_active_sec: Option<String>,
        #[serde(default = "default_timer_target")]
        wanted_by: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitPlan {
    pub id: String,
    pub unit_name: String,
    pub path: PathBuf,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitRecord {
    pub id: String,
    pub unit_name: String,
    pub path: PathBuf,
    pub unit_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_unit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitStatus {
    pub id: String,
    pub unit_name: String,
    pub installed: bool,
    pub enabled: bool,
    pub active: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Registry {
    schema_version: u8,
    units: BTreeMap<String, UnitRecord>,
}

pub struct UnitManager<E> {
    executor: E,
    unit_root: PathBuf,
    registry_path: PathBuf,
}

impl<E: ProcessExecutor> UnitManager<E> {
    pub fn new(executor: E, unit_root: PathBuf, registry_path: PathBuf) -> Self {
        Self {
            executor,
            unit_root,
            registry_path,
        }
    }

    pub fn available(&self) -> bool {
        self.executor.command_exists("systemctl")
    }

    pub fn plan(&self, descriptor: &UnitDescriptor) -> Result<UnitPlan, String> {
        self.validate_roots()?;
        let (id, unit_name, content) = render(descriptor)?;
        Ok(UnitPlan {
            id,
            path: self.unit_root.join(&unit_name),
            unit_name,
            content,
        })
    }

    pub fn apply(&self, descriptor: &UnitDescriptor) -> Result<UnitRecord, String> {
        self.apply_batch(std::slice::from_ref(descriptor))?
            .into_iter()
            .next()
            .ok_or_else(|| "unit batch produced no records".into())
    }

    pub fn apply_batch(&self, descriptors: &[UnitDescriptor]) -> Result<Vec<UnitRecord>, String> {
        if descriptors.is_empty() {
            return Err("unit batch cannot be empty".into());
        }
        if !self.available() {
            return Err("systemctl is not installed".into());
        }
        let previous_registry_file = fs::read(&self.registry_path).ok();
        let previous_registry = self.read_registry()?;
        let mut registry = previous_registry.clone();
        registry.schema_version = 1;
        let mut plans = Vec::new();
        let mut records = Vec::new();
        for descriptor in descriptors {
            validate_runtime_files(descriptor)?;
            let plan = self.plan(descriptor)?;
            if plans.iter().any(|existing: &UnitPlan| {
                existing.id == plan.id || existing.unit_name == plan.unit_name
            }) {
                return Err(format!("duplicate unit in batch: {}", plan.unit_name));
            }
            if let Some(existing) = previous_registry.units.get(&plan.id)
                && existing.unit_name != plan.unit_name
            {
                return Err(format!(
                    "service id already owns another unit: {}",
                    existing.unit_name
                ));
            }
            if previous_registry
                .units
                .values()
                .any(|record| record.id != plan.id && record.unit_name == plan.unit_name)
            {
                return Err(format!(
                    "unit is already registered under another id: {}",
                    plan.unit_name
                ));
            }
            let record = record_for(descriptor, &plan);
            registry.units.insert(record.id.clone(), record.clone());
            plans.push(plan);
            records.push(record);
        }
        for descriptor in descriptors {
            if let UnitDescriptor::Timer { target_unit, .. } = descriptor
                && !registry
                    .units
                    .values()
                    .any(|record| record.unit_name == *target_unit && record.unit_type == "service")
            {
                return Err(format!(
                    "timer target is not registered by the compositor: {target_unit}"
                ));
            }
        }
        fs::create_dir_all(&self.unit_root).map_err(|error| {
            format!(
                "failed to create unit directory {}: {error}",
                self.unit_root.display()
            )
        })?;
        let backups = plans
            .iter()
            .map(|plan| (plan.path.clone(), fs::read(&plan.path).ok()))
            .collect::<Vec<_>>();
        for plan in &plans {
            if let Err(error) = atomic_write(&plan.path, plan.content.as_bytes()) {
                restore_files(&backups);
                return Err(error);
            }
        }
        if let Err(error) = self.write_registry(&registry) {
            restore_files(&backups);
            restore_file(&self.registry_path, previous_registry_file.as_deref());
            return Err(error);
        }
        if let Err(error) = self.systemctl(&["daemon-reload"]) {
            restore_files(&backups);
            restore_file(&self.registry_path, previous_registry_file.as_deref());
            let _ = self.systemctl(&["daemon-reload"]);
            return Err(error);
        }
        Ok(records)
    }

    pub fn remove(&self, id: &str) -> Result<Option<UnitRecord>, String> {
        validate_id(id)?;
        let previous_registry = self.read_registry()?;
        let Some(record) = previous_registry.units.get(id).cloned() else {
            return Ok(None);
        };
        self.validate_record(&record)?;
        if let Some(dependent) = previous_registry
            .units
            .values()
            .find(|candidate| candidate.target_unit.as_deref() == Some(&record.unit_name))
        {
            return Err(format!(
                "unit is required by registered timer: {}",
                dependent.unit_name
            ));
        }
        self.systemctl(&["disable", "--now", &record.unit_name])?;
        let previous_file = fs::read(&record.path).ok();
        if record.path.exists() {
            fs::remove_file(&record.path)
                .map_err(|error| format!("failed to remove unit file: {error}"))?;
        }
        let mut registry = previous_registry.clone();
        registry.units.remove(id);
        if let Err(error) = self.write_registry(&registry) {
            restore_file(&record.path, previous_file.as_deref());
            return Err(error);
        }
        if let Err(error) = self.systemctl(&["daemon-reload"]) {
            restore_file(&record.path, previous_file.as_deref());
            let _ = self.write_registry(&previous_registry);
            return Err(error);
        }
        Ok(Some(record))
    }

    pub fn control(&self, id: &str, action: &str) -> Result<UnitStatus, String> {
        validate_id(id)?;
        let record = self
            .read_registry()?
            .units
            .get(id)
            .cloned()
            .ok_or_else(|| format!("service id not found: {id}"))?;
        self.validate_record(&record)?;
        match action {
            "status" => {}
            "start" | "stop" | "restart" => {
                self.systemctl(&[action, &record.unit_name])?;
            }
            "enable" => {
                self.systemctl(&["enable", "--now", &record.unit_name])?;
            }
            "disable" => {
                self.systemctl(&["disable", "--now", &record.unit_name])?;
            }
            _ => return Err(format!("unsupported service action: {action}")),
        }
        Ok(self.status_for(&record))
    }

    pub fn list(&self) -> Result<Vec<UnitRecord>, String> {
        Ok(self.read_registry()?.units.into_values().collect())
    }

    fn status_for(&self, record: &UnitRecord) -> UnitStatus {
        UnitStatus {
            id: record.id.clone(),
            unit_name: record.unit_name.clone(),
            installed: record.path.is_file(),
            enabled: self.systemctl(&["is-enabled", &record.unit_name]).is_ok(),
            active: self.systemctl(&["is-active", &record.unit_name]).is_ok(),
        }
    }

    fn validate_record(&self, record: &UnitRecord) -> Result<(), String> {
        validate_id(&record.id)?;
        let suffix = if record.unit_name.ends_with(".service") {
            ".service"
        } else {
            ".timer"
        };
        validate_unit_name(&record.unit_name, suffix)?;
        let expected = self.unit_root.join(&record.unit_name);
        if record.path != expected {
            return Err(format!(
                "registered unit path does not match managed root: {}",
                record.path.display()
            ));
        }
        Ok(())
    }

    fn systemctl(&self, args: &[&str]) -> Result<(), String> {
        let args = std::iter::once("--user".to_string())
            .chain(args.iter().map(|value| (*value).to_string()))
            .collect::<Vec<_>>();
        self.executor.run("systemctl", &args).map(|_| ())
    }

    fn read_registry(&self) -> Result<Registry, String> {
        self.validate_roots()?;
        if !self.registry_path.exists() {
            return Ok(Registry {
                schema_version: 1,
                ..Default::default()
            });
        }
        let bytes = fs::read(&self.registry_path)
            .map_err(|error| format!("failed to read service registry: {error}"))?;
        let registry: Registry = serde_json::from_slice(&bytes)
            .map_err(|error| format!("failed to parse service registry: {error}"))?;
        if registry.schema_version != 1 {
            return Err(format!(
                "unsupported service registry schema: {}",
                registry.schema_version
            ));
        }
        Ok(registry)
    }

    fn write_registry(&self, registry: &Registry) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(registry)
            .map_err(|error| format!("failed to serialize service registry: {error}"))?;
        atomic_write(&self.registry_path, &bytes)
    }

    fn validate_roots(&self) -> Result<(), String> {
        if !self.unit_root.is_absolute() || !self.registry_path.is_absolute() {
            return Err("service unit and registry paths must be absolute".into());
        }
        Ok(())
    }
}

fn record_for(descriptor: &UnitDescriptor, plan: &UnitPlan) -> UnitRecord {
    UnitRecord {
        id: plan.id.clone(),
        unit_name: plan.unit_name.clone(),
        path: plan.path.clone(),
        unit_type: match descriptor {
            UnitDescriptor::Service { .. } => "service".into(),
            UnitDescriptor::Timer { .. } => "timer".into(),
        },
        target_unit: match descriptor {
            UnitDescriptor::Timer { target_unit, .. } => Some(target_unit.clone()),
            UnitDescriptor::Service { .. } => None,
        },
    }
}

fn restore_files(backups: &[(PathBuf, Option<Vec<u8>>)]) {
    for (path, content) in backups {
        restore_file(path, content.as_deref());
    }
}

fn render(descriptor: &UnitDescriptor) -> Result<(String, String, String), String> {
    match descriptor {
        UnitDescriptor::Service {
            id,
            unit_name,
            description,
            exec_start,
            restart,
            wanted_by,
        } => {
            validate_id(id)?;
            validate_unit_name(unit_name, ".service")?;
            validate_text("description", description)?;
            validate_target(wanted_by)?;
            if exec_start.is_empty() {
                return Err("service exec_start cannot be empty".into());
            }
            for argument in exec_start {
                validate_argument(argument)?;
            }
            let command = exec_start
                .iter()
                .map(|argument| quote_systemd(argument))
                .collect::<Vec<_>>()
                .join(" ");
            let content = format!(
                "[Unit]\nDescription={}\n\n[Service]\nType=simple\nExecStart={}\nRestart={}\n\n[Install]\nWantedBy={}\n",
                escape_percent(description),
                command,
                restart.as_systemd(),
                wanted_by
            );
            Ok((id.clone(), unit_name.clone(), content))
        }
        UnitDescriptor::Timer {
            id,
            unit_name,
            description,
            target_unit,
            on_boot_sec,
            on_unit_active_sec,
            wanted_by,
        } => {
            validate_id(id)?;
            validate_unit_name(unit_name, ".timer")?;
            validate_unit_name(target_unit, ".service")?;
            validate_text("description", description)?;
            validate_target(wanted_by)?;
            if on_boot_sec.is_none() && on_unit_active_sec.is_none() {
                return Err("timer requires on_boot_sec or on_unit_active_sec".into());
            }
            let mut timer = String::new();
            if let Some(value) = on_boot_sec {
                validate_duration(value)?;
                timer.push_str(&format!("OnBootSec={value}\n"));
            }
            if let Some(value) = on_unit_active_sec {
                validate_duration(value)?;
                timer.push_str(&format!("OnUnitActiveSec={value}\n"));
            }
            let content = format!(
                "[Unit]\nDescription={}\n\n[Timer]\n{}Unit={}\nPersistent=true\n\n[Install]\nWantedBy={}\n",
                escape_percent(description),
                timer,
                target_unit,
                wanted_by
            );
            Ok((id.clone(), unit_name.clone(), content))
        }
    }
}

fn validate_runtime_files(descriptor: &UnitDescriptor) -> Result<(), String> {
    if let UnitDescriptor::Service { exec_start, .. } = descriptor {
        let executable = Path::new(&exec_start[0]);
        if !executable.is_absolute() || !executable.is_file() {
            return Err("service executable must be an existing absolute file".into());
        }
    }
    Ok(())
}

fn validate_id(value: &str) -> Result<(), String> {
    validate_safe_name("service id", value)
}

fn validate_unit_name(value: &str, suffix: &str) -> Result<(), String> {
    validate_safe_name("unit name", value)?;
    if !value.ends_with(suffix) {
        return Err(format!("unit name must end with {suffix}"));
    }
    Ok(())
}

fn validate_target(value: &str) -> Result<(), String> {
    validate_unit_name(value, ".target")
}

fn validate_safe_name(label: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.' | '@')
        })
    {
        return Err(format!("invalid {label}: {value}"));
    }
    Ok(())
}

fn validate_text(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err(format!("invalid {label}"));
    }
    Ok(())
}

fn validate_argument(value: &str) -> Result<(), String> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err("service arguments cannot be empty or contain control characters".into());
    }
    Ok(())
}

fn validate_duration(value: &str) -> Result<(), String> {
    if value.is_empty()
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-'))
    {
        return Err(format!("invalid systemd duration: {value}"));
    }
    Ok(())
}

fn quote_systemd(value: &str) -> String {
    format!(
        "\"{}\"",
        escape_percent(value)
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    )
}

fn escape_percent(value: &str) -> String {
    value.replace('%', "%%")
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&temporary, bytes)
        .map_err(|error| format!("failed to write {}: {error}", temporary.display()))?;
    fs::rename(&temporary, path)
        .map_err(|error| format!("failed to replace {}: {error}", path.display()))
}

fn restore_file(path: &Path, previous: Option<&[u8]>) {
    if let Some(bytes) = previous {
        let _ = atomic_write(path, bytes);
    } else {
        let _ = fs::remove_file(path);
    }
}

fn default_target() -> String {
    "default.target".into()
}

fn default_timer_target() -> String {
    "timers.target".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProcessOutput;
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct FakeExecutor {
        commands: BTreeSet<String>,
        calls: RefCell<Vec<Vec<String>>>,
        fail_reload_once: RefCell<bool>,
    }

    impl ProcessExecutor for FakeExecutor {
        fn command_exists(&self, binary: &str) -> bool {
            self.commands.contains(binary)
        }

        fn run(&self, binary: &str, args: &[String]) -> Result<ProcessOutput, String> {
            assert_eq!(binary, "systemctl");
            self.calls.borrow_mut().push(args.to_vec());
            if args == ["--user", "daemon-reload"] && self.fail_reload_once.replace(false) {
                return Err("forced daemon-reload failure".into());
            }
            Ok(ProcessOutput {
                stdout: String::new(),
                stderr: String::new(),
            })
        }

        fn spawn(&self, _binary: &str, _args: &[String]) -> Result<u32, String> {
            Err("spawn is not used for services".into())
        }
    }

    fn roots(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "compositor-units-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        (root.join("units"), root.join("state/registry.json"), root)
    }

    #[test]
    fn renders_typed_service_and_escapes_systemd_specifiers() {
        let (units, registry, root) = roots("plan");
        let manager = UnitManager::new(FakeExecutor::default(), units, registry);
        let plan = manager
            .plan(&UnitDescriptor::Service {
                id: "kitowall-watch".into(),
                unit_name: "kitowall-watch.service".into(),
                description: "Kitowall 100% watcher".into(),
                exec_start: vec!["/opt/Kitowall/bin/kitowall".into(), "watch outputs".into()],
                restart: RestartPolicy::OnFailure,
                wanted_by: "default.target".into(),
            })
            .unwrap();
        assert!(plan.content.contains("Kitowall 100%% watcher"));
        assert!(plan.content.contains("\"watch outputs\""));
        assert!(plan.content.contains("Restart=on-failure"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn applies_and_removes_only_the_registered_unit() {
        let (units, registry, root) = roots("apply");
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("kitowall");
        fs::write(&executable, b"binary").unwrap();
        let executor = FakeExecutor {
            commands: ["systemctl".into()].into_iter().collect(),
            ..Default::default()
        };
        let manager = UnitManager::new(executor, units, registry);
        let descriptor = UnitDescriptor::Service {
            id: "kitowall-next".into(),
            unit_name: "kitowall-next.service".into(),
            description: "Kitowall next".into(),
            exec_start: vec![
                executable.to_string_lossy().into_owned(),
                "rotate-now".into(),
            ],
            restart: RestartPolicy::No,
            wanted_by: "default.target".into(),
        };
        let record = manager.apply(&descriptor).unwrap();
        assert!(record.path.is_file());
        assert_eq!(manager.list().unwrap().len(), 1);
        assert!(manager.remove("kitowall-next").unwrap().is_some());
        assert!(!record.path.exists());
        assert!(manager.list().unwrap().is_empty());
        let calls = manager.executor.calls.borrow();
        assert!(
            calls
                .iter()
                .any(|args| args == &["--user", "daemon-reload"])
        );
        assert!(
            calls
                .iter()
                .any(|args| { args == &["--user", "disable", "--now", "kitowall-next.service"] })
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_unit_names_that_escape_the_unit_root() {
        let (units, registry, root) = roots("escape");
        let manager = UnitManager::new(FakeExecutor::default(), units, registry);
        let result = manager.plan(&UnitDescriptor::Timer {
            id: "bad".into(),
            unit_name: "../bad.timer".into(),
            description: "Bad timer".into(),
            target_unit: "kitowall-next.service".into(),
            on_boot_sec: Some("1m".into()),
            on_unit_active_sec: None,
            wanted_by: "timers.target".into(),
        });
        assert!(result.is_err());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn refuses_tampered_registry_paths_outside_the_managed_root() {
        let (units, registry, root) = roots("tampered");
        fs::create_dir_all(registry.parent().unwrap()).unwrap();
        let outside = root.join("outside.service");
        fs::write(&outside, b"do not remove").unwrap();
        fs::write(
            &registry,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "units": {
                    "kitowall": {
                        "id": "kitowall",
                        "unit_name": "kitowall.service",
                        "path": outside
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let manager = UnitManager::new(FakeExecutor::default(), units, registry);
        assert!(manager.remove("kitowall").is_err());
        assert!(outside.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn timer_requires_registered_service_and_blocks_early_service_removal() {
        let (units, registry, root) = roots("timer-dependency");
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("kitowall");
        fs::write(&executable, b"binary").unwrap();
        let executor = FakeExecutor {
            commands: ["systemctl".into()].into_iter().collect(),
            ..Default::default()
        };
        let manager = UnitManager::new(executor, units, registry);
        let timer = UnitDescriptor::Timer {
            id: "kitowall-next-timer".into(),
            unit_name: "kitowall-next.timer".into(),
            description: "Kitowall timer".into(),
            target_unit: "kitowall-next.service".into(),
            on_boot_sec: Some("1m".into()),
            on_unit_active_sec: Some("5m".into()),
            wanted_by: "timers.target".into(),
        };
        assert!(manager.apply(&timer).is_err());
        manager
            .apply(&UnitDescriptor::Service {
                id: "kitowall-next".into(),
                unit_name: "kitowall-next.service".into(),
                description: "Kitowall next".into(),
                exec_start: vec![executable.to_string_lossy().into_owned()],
                restart: RestartPolicy::No,
                wanted_by: "default.target".into(),
            })
            .unwrap();
        manager.apply(&timer).unwrap();
        assert!(manager.remove("kitowall-next").is_err());
        manager.remove("kitowall-next-timer").unwrap();
        manager.remove("kitowall-next").unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn batch_applies_timer_with_its_service_and_rolls_back_every_artifact() {
        let (units, registry, root) = roots("batch-rollback");
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("kitowall");
        fs::write(&executable, b"binary").unwrap();
        let executor = FakeExecutor {
            commands: ["systemctl".into()].into_iter().collect(),
            ..Default::default()
        };
        let manager = UnitManager::new(executor, units.clone(), registry.clone());
        let descriptors = |description: &str| {
            vec![
                UnitDescriptor::Service {
                    id: "kitowall-next".into(),
                    unit_name: "kitowall-next.service".into(),
                    description: description.into(),
                    exec_start: vec![
                        executable.to_string_lossy().into_owned(),
                        "rotate-now".into(),
                    ],
                    restart: RestartPolicy::No,
                    wanted_by: "default.target".into(),
                },
                UnitDescriptor::Timer {
                    id: "kitowall-next-timer".into(),
                    unit_name: "kitowall-next.timer".into(),
                    description: format!("Schedule: {description}"),
                    target_unit: "kitowall-next.service".into(),
                    on_boot_sec: Some("2s".into()),
                    on_unit_active_sec: Some("600s".into()),
                    wanted_by: "timers.target".into(),
                },
            ]
        };

        let records = manager.apply_batch(&descriptors("original")).unwrap();
        assert_eq!(records.len(), 2);
        let service = units.join("kitowall-next.service");
        let timer = units.join("kitowall-next.timer");
        let service_before = fs::read(&service).unwrap();
        let timer_before = fs::read(&timer).unwrap();
        let registry_before = fs::read(&registry).unwrap();

        manager.executor.fail_reload_once.replace(true);
        let error = manager.apply_batch(&descriptors("updated")).unwrap_err();
        assert!(error.contains("forced daemon-reload failure"));
        assert_eq!(fs::read(service).unwrap(), service_before);
        assert_eq!(fs::read(timer).unwrap(), timer_before);
        assert_eq!(fs::read(registry).unwrap(), registry_before);
        let reloads = manager
            .executor
            .calls
            .borrow()
            .iter()
            .filter(|args| args.as_slice() == ["--user", "daemon-reload"])
            .count();
        assert_eq!(reloads, 3);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn failed_first_batch_restores_absent_files_and_registry() {
        let (units, registry, root) = roots("batch-first-failure");
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("kitowall");
        fs::write(&executable, b"binary").unwrap();
        let executor = FakeExecutor {
            commands: ["systemctl".into()].into_iter().collect(),
            fail_reload_once: RefCell::new(true),
            ..Default::default()
        };
        let manager = UnitManager::new(executor, units.clone(), registry.clone());
        let result = manager.apply_batch(&[UnitDescriptor::Service {
            id: "kitowall-watch".into(),
            unit_name: "kitowall-watch.service".into(),
            description: "Watch outputs".into(),
            exec_start: vec![executable.to_string_lossy().into_owned(), "watch".into()],
            restart: RestartPolicy::OnFailure,
            wanted_by: "default.target".into(),
        }]);

        assert!(result.is_err());
        assert!(!units.join("kitowall-watch.service").exists());
        assert!(!registry.exists());
        let _ = fs::remove_dir_all(root);
    }
}

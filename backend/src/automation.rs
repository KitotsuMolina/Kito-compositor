use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::{ProcessExecutor, RestartPolicy, UnitDescriptor, UnitManager, UnitRecord, UnitStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutomationKind {
    OneShot,
    Daemon,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schedule {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup_delay_seconds: Option<u64>,
    pub every_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationDescriptor {
    pub schema_version: u8,
    pub id: String,
    pub description: String,
    pub command: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub environment: BTreeMap<String, String>,
    pub kind: AutomationKind,
    #[serde(default)]
    pub restart: RestartPolicy,
    #[serde(default)]
    pub autostart: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<Schedule>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationBatchDescriptor {
    pub schema_version: u8,
    pub automations: Vec<AutomationDescriptor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceManagerKind {
    SystemdUser,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationPlan {
    pub manager: ServiceManagerKind,
    pub automation_id: String,
    pub artifacts: Vec<UnitDescriptor>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationStatus {
    pub manager: ServiceManagerKind,
    pub automation_id: String,
    pub artifacts: Vec<UnitStatus>,
}

pub fn detect_service_manager<E: ProcessExecutor>(
    executor: &E,
) -> Result<ServiceManagerKind, String> {
    if executor.command_exists("systemctl") {
        Ok(ServiceManagerKind::SystemdUser)
    } else {
        Err("no supported user service manager detected".into())
    }
}

pub fn plan_automation(
    descriptor: &AutomationDescriptor,
    manager: ServiceManagerKind,
) -> Result<AutomationPlan, String> {
    validate(descriptor)?;
    match manager {
        ServiceManagerKind::SystemdUser => plan_systemd(descriptor),
    }
}

pub fn plan_automation_batch(
    batch: &AutomationBatchDescriptor,
    manager: ServiceManagerKind,
) -> Result<Vec<AutomationPlan>, String> {
    if batch.schema_version != 1 {
        return Err("automation batch schema_version must be 1".into());
    }
    if batch.automations.is_empty() {
        return Err("automation batch cannot be empty".into());
    }
    let mut ids = std::collections::BTreeSet::new();
    batch
        .automations
        .iter()
        .map(|descriptor| {
            if !ids.insert(descriptor.id.clone()) {
                return Err(format!(
                    "duplicate automation id in batch: {}",
                    descriptor.id
                ));
            }
            plan_automation(descriptor, manager)
        })
        .collect()
}

pub fn control_automation<E: ProcessExecutor>(
    manager: &UnitManager<E>,
    id: &str,
    action: &str,
) -> Result<AutomationStatus, String> {
    let records = automation_records(manager, id)?;
    if records.is_empty() {
        return Err(format!("automation id not found: {id}"));
    }
    let timer = records.iter().find(|record| record.unit_type == "timer");
    let service = records.iter().find(|record| record.unit_type == "service");
    let mut statuses = Vec::new();
    match action {
        "status" => {
            for record in &records {
                statuses.push(manager.control(&record.id, "status")?);
            }
        }
        "start" | "restart" | "enable" => {
            let target = timer
                .or(service)
                .ok_or_else(|| format!("automation has no controllable artifacts: {id}"))?;
            statuses.push(manager.control(&target.id, action)?);
        }
        "stop" | "disable" => {
            if let Some(timer) = timer {
                statuses.push(manager.control(&timer.id, action)?);
            }
            if let Some(service) = service {
                statuses.push(manager.control(&service.id, action)?);
            }
        }
        _ => return Err(format!("unsupported automation action: {action}")),
    }
    Ok(AutomationStatus {
        manager: ServiceManagerKind::SystemdUser,
        automation_id: id.into(),
        artifacts: statuses,
    })
}

pub fn remove_automation<E: ProcessExecutor>(
    manager: &UnitManager<E>,
    id: &str,
) -> Result<Vec<UnitRecord>, String> {
    let records = automation_records(manager, id)?;
    let mut removed = Vec::new();
    for record in records.iter().filter(|record| record.unit_type == "timer") {
        if let Some(record) = manager.remove(&record.id)? {
            removed.push(record);
        }
    }
    for record in records
        .iter()
        .filter(|record| record.unit_type == "service")
    {
        if let Some(record) = manager.remove(&record.id)? {
            removed.push(record);
        }
    }
    Ok(removed)
}

fn automation_records<E: ProcessExecutor>(
    manager: &UnitManager<E>,
    id: &str,
) -> Result<Vec<UnitRecord>, String> {
    let timer_id = format!("{id}-timer");
    Ok(manager
        .list()?
        .into_iter()
        .filter(|record| record.id == id || record.id == timer_id)
        .collect())
}

fn plan_systemd(descriptor: &AutomationDescriptor) -> Result<AutomationPlan, String> {
    let service_name = format!("{}.service", descriptor.id);
    let mut artifacts = vec![UnitDescriptor::Service {
        id: descriptor.id.clone(),
        unit_name: service_name.clone(),
        description: descriptor.description.clone(),
        exec_start: descriptor.command.clone(),
        environment: descriptor.environment.clone(),
        restart: descriptor.restart,
        wanted_by: "default.target".into(),
    }];
    if let Some(schedule) = &descriptor.schedule {
        artifacts.push(UnitDescriptor::Timer {
            id: format!("{}-timer", descriptor.id),
            unit_name: format!("{}.timer", descriptor.id),
            description: format!("Schedule: {}", descriptor.description),
            target_unit: service_name,
            on_boot_sec: None,
            on_active_sec: schedule
                .startup_delay_seconds
                .map(|seconds| format!("{seconds}s")),
            on_unit_active_sec: Some(format!("{}s", schedule.every_seconds)),
            wanted_by: "timers.target".into(),
        });
    }
    Ok(AutomationPlan {
        manager: ServiceManagerKind::SystemdUser,
        automation_id: descriptor.id.clone(),
        artifacts,
    })
}

fn validate(descriptor: &AutomationDescriptor) -> Result<(), String> {
    if descriptor.schema_version != 1 {
        return Err("automation schema_version must be 1".into());
    }
    if descriptor.id.is_empty()
        || !descriptor
            .id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
    {
        return Err(format!("invalid automation id: {}", descriptor.id));
    }
    if descriptor.description.trim().is_empty() {
        return Err("automation description cannot be empty".into());
    }
    if descriptor.command.is_empty() || !descriptor.command[0].starts_with('/') {
        return Err("automation command must start with an absolute executable path".into());
    }
    if descriptor
        .command
        .iter()
        .any(|argument| argument.contains('\0'))
    {
        return Err("automation command contains a null byte".into());
    }
    for (key, value) in &descriptor.environment {
        if key.is_empty()
            || !key.chars().enumerate().all(|(index, character)| {
                character == '_'
                    || character.is_ascii_alphabetic()
                    || (index > 0 && character.is_ascii_digit())
            })
        {
            return Err(format!("invalid automation environment key: {key}"));
        }
        if value.contains('\0') || value.contains(['\n', '\r']) {
            return Err(format!("invalid automation environment value for {key}"));
        }
    }
    if descriptor.kind == AutomationKind::OneShot && descriptor.restart == RestartPolicy::Always {
        return Err("one-shot automation cannot always restart".into());
    }
    if let Some(schedule) = &descriptor.schedule {
        if descriptor.kind != AutomationKind::OneShot {
            return Err("only one-shot automation can use a schedule".into());
        }
        if schedule.every_seconds == 0 {
            return Err("schedule every_seconds must be greater than zero".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portable_schedule_becomes_service_and_timer() {
        let plan = plan_automation(
            &AutomationDescriptor {
                schema_version: 1,
                id: "kitowall-next".into(),
                description: "Rotate static wallpaper".into(),
                command: vec!["/opt/kitowall/bin/kitowall".into(), "rotate-now".into()],
                environment: BTreeMap::new(),
                kind: AutomationKind::OneShot,
                restart: RestartPolicy::No,
                autostart: false,
                schedule: Some(Schedule {
                    startup_delay_seconds: Some(2),
                    every_seconds: 600,
                }),
            },
            ServiceManagerKind::SystemdUser,
        )
        .unwrap();
        assert_eq!(plan.artifacts.len(), 2);
        assert!(matches!(plan.artifacts[0], UnitDescriptor::Service { .. }));
        assert!(matches!(plan.artifacts[1], UnitDescriptor::Timer { .. }));
        let json = serde_json::to_value(plan).unwrap();
        assert_eq!(json["manager"], "systemd-user");
        assert_eq!(json["artifacts"][1]["on_active_sec"], "2s");
        assert!(json["artifacts"][1]["on_boot_sec"].is_null());
        assert_eq!(json["artifacts"][1]["on_unit_active_sec"], "600s");
    }

    #[test]
    fn rejects_system_specific_and_invalid_portable_values() {
        let descriptor = AutomationDescriptor {
            schema_version: 1,
            id: "bad.service".into(),
            description: "Bad".into(),
            command: vec!["kitowall".into()],
            environment: BTreeMap::new(),
            kind: AutomationKind::Daemon,
            restart: RestartPolicy::OnFailure,
            autostart: true,
            schedule: None,
        };
        assert!(plan_automation(&descriptor, ServiceManagerKind::SystemdUser).is_err());
    }

    #[test]
    fn portable_one_shot_preserves_retry_policy() {
        let plan = plan_automation(
            &AutomationDescriptor {
                schema_version: 1,
                id: "kitowall-login-apply".into(),
                description: "Restore wallpaper after login".into(),
                command: vec!["/opt/kitowall/bin/kitowall".into(), "rotate-now".into()],
                environment: BTreeMap::new(),
                kind: AutomationKind::OneShot,
                restart: RestartPolicy::OnFailure,
                autostart: true,
                schedule: None,
            },
            ServiceManagerKind::SystemdUser,
        )
        .unwrap();
        assert!(matches!(
            plan.artifacts[0],
            UnitDescriptor::Service {
                restart: RestartPolicy::OnFailure,
                ..
            }
        ));
    }

    #[test]
    fn portable_one_shot_rejects_unconditional_restart() {
        let descriptor = AutomationDescriptor {
            schema_version: 1,
            id: "bad-loop".into(),
            description: "Never settle".into(),
            command: vec!["/opt/tool".into()],
            environment: BTreeMap::new(),
            kind: AutomationKind::OneShot,
            restart: RestartPolicy::Always,
            autostart: true,
            schedule: None,
        };
        assert!(plan_automation(&descriptor, ServiceManagerKind::SystemdUser).is_err());
    }
}

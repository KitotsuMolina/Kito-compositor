mod automation;
mod events;
mod model;
mod process;
mod provider;
mod runner;
mod service;
mod unit_manager;
mod wallpaper;

pub use automation::{
    AutomationBatchDescriptor, AutomationDescriptor, AutomationKind, AutomationPlan,
    AutomationStatus, Schedule, ServiceManagerKind, control_automation, detect_service_manager,
    plan_automation, plan_automation_batch, remove_automation,
};
pub use events::EventTracker;
pub use model::{
    Capabilities, CompositorKind, Detection, DoctorCheck, DoctorReport, Output, Status,
};
pub use process::{ProcessExecutor, ProcessOutput, SystemProcessExecutor};
pub use runner::{HostRunner, SystemHostRunner};
pub use service::CompositorBackend;
pub use unit_manager::{
    RestartPolicy, UnitDescriptor, UnitManager, UnitPlan, UnitRecord, UnitStatus,
};
pub use wallpaper::{
    WallpaperApplyRequest, WallpaperBackend, WallpaperRuntime, WallpaperRuntimeStatus,
    WallpaperTransition,
};

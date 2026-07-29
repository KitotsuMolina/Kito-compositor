use super::AppearancePolicy;
use std::fs;
use std::path::PathBuf;

pub struct AppearancePolicyStore {
    path: PathBuf,
}

impl AppearancePolicyStore {
    pub fn from_environment() -> Self {
        let path = std::env::var("KITSUNE_COMPOSITOR_APPEARANCE_POLICY")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                std::env::var("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| PathBuf::from(home).join(".config"))
                    .join("kitsune-compositor/appearance.json")
            });
        Self { path }
    }

    pub fn load(&self) -> Result<AppearancePolicy, String> {
        if !self.path.exists() {
            return Ok(AppearancePolicy::default());
        }
        let bytes = fs::read(&self.path)
            .map_err(|error| format!("failed to read appearance policy: {error}"))?;
        let policy = serde_json::from_slice::<AppearancePolicy>(&bytes)
            .map_err(|error| format!("failed to parse appearance policy: {error}"))?;
        if policy.schema_version != 1 {
            return Err(format!(
                "unsupported appearance policy schema: {}",
                policy.schema_version
            ));
        }
        Ok(policy)
    }

    pub fn store(&self, policy: &AppearancePolicy) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "appearance policy path has no parent".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create appearance policy directory: {error}"))?;
        let temporary = self
            .path
            .with_extension(format!("json.tmp-{}", std::process::id()));
        let bytes = serde_json::to_vec_pretty(policy)
            .map_err(|error| format!("failed to serialize appearance policy: {error}"))?;
        fs::write(&temporary, bytes)
            .map_err(|error| format!("failed to write appearance policy: {error}"))?;
        fs::rename(&temporary, &self.path)
            .map_err(|error| format!("failed to replace appearance policy: {error}"))
    }
}

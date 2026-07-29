use super::AppearanceState;
use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;

pub struct AppearanceStateStore {
    path: PathBuf,
}

pub struct AppearanceApplyGuard {
    _file: File,
}

impl AppearanceStateStore {
    pub fn from_environment() -> Self {
        let path = std::env::var("KITSUNE_COMPOSITOR_APPEARANCE_STATE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                std::env::var("XDG_STATE_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| PathBuf::from(home).join(".local/state"))
                    .join("kitsune-compositor/appearance.json")
            });
        Self { path }
    }

    pub fn load(&self) -> Result<Option<AppearanceState>, String> {
        if !self.path.exists() {
            return Ok(None);
        }
        let bytes = fs::read(&self.path)
            .map_err(|error| format!("failed to read appearance state: {error}"))?;
        let state = serde_json::from_slice::<AppearanceState>(&bytes)
            .map_err(|error| format!("failed to parse appearance state: {error}"))?;
        if state.schema_version != 1 {
            return Err(format!(
                "unsupported appearance state schema: {}",
                state.schema_version
            ));
        }
        Ok(Some(state))
    }

    pub fn lock_apply(&self) -> Result<AppearanceApplyGuard, String> {
        let path = std::env::var("KITSUNE_COMPOSITOR_APPEARANCE_LOCK")
            .map(PathBuf::from)
            .unwrap_or_else(|_| self.path.with_extension("lock"));
        let parent = path
            .parent()
            .ok_or_else(|| "appearance lock path has no parent".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create appearance lock directory: {error}"))?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|error| format!("failed to open appearance lock: {error}"))?;
        file.lock()
            .map_err(|error| format!("failed to lock appearance synchronization: {error}"))?;
        Ok(AppearanceApplyGuard { _file: file })
    }

    pub fn store(&self, state: &AppearanceState) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "appearance state path has no parent".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create appearance state directory: {error}"))?;
        let temporary = self
            .path
            .with_extension(format!("json.tmp-{}", std::process::id()));
        let bytes = serde_json::to_vec_pretty(state)
            .map_err(|error| format!("failed to serialize appearance state: {error}"))?;
        fs::write(&temporary, bytes)
            .map_err(|error| format!("failed to write appearance state: {error}"))?;
        fs::rename(&temporary, &self.path)
            .map_err(|error| format!("failed to replace appearance state: {error}"))
    }

    pub fn remove(&self) -> Result<(), String> {
        if self.path.exists() {
            fs::remove_file(&self.path)
                .map_err(|error| format!("failed to remove appearance state: {error}"))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn appearance_apply_lock_serializes_callers() {
        let root = std::env::temp_dir().join(format!(
            "appearance-lock-{}-{}",
            std::process::id(),
            super::super::now_unix_ms()
        ));
        let state_path = root.join("appearance.json");
        let first = AppearanceStateStore {
            path: state_path.clone(),
        };
        let second = AppearanceStateStore { path: state_path };
        let first_guard = first.lock_apply().unwrap();
        let (sender, receiver) = mpsc::channel();

        let waiting = std::thread::spawn(move || {
            let guard = second.lock_apply().unwrap();
            sender.send(()).unwrap();
            drop(guard);
        });

        assert!(receiver.recv_timeout(Duration::from_millis(50)).is_err());
        drop(first_guard);
        receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        waiting.join().unwrap();
        let _ = fs::remove_dir_all(root);
    }
}

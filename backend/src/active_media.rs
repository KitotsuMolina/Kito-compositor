use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: u8 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActiveMediaKind {
    Static,
    Live,
}

impl ActiveMediaKind {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "static" => Ok(Self::Static),
            "live" => Ok(Self::Live),
            _ => Err("active media kind must be static or live".into()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveMediaRecord {
    pub schema_version: u8,
    pub output: String,
    pub owner: String,
    pub media_type: ActiveMediaKind,
    pub source: PathBuf,
    pub representative_image: PathBuf,
    pub updated_at_unix_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActiveMediaIndex {
    schema_version: u8,
    records: BTreeMap<String, ActiveMediaRecord>,
    #[serde(default)]
    suspended: BTreeMap<String, Vec<ActiveMediaRecord>>,
}

impl Default for ActiveMediaIndex {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            records: BTreeMap::new(),
            suspended: BTreeMap::new(),
        }
    }
}

pub struct ActiveMediaStore {
    path: PathBuf,
}

impl ActiveMediaStore {
    pub fn from_environment() -> Self {
        let path = std::env::var("KITSUNE_COMPOSITOR_ACTIVE_MEDIA_STATE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                std::env::var("XDG_STATE_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| PathBuf::from(home).join(".local/state"))
                    .join("kitsune-compositor/active-media.json")
            });
        Self { path }
    }

    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn publish(
        &self,
        output: &str,
        owner: &str,
        media_type: ActiveMediaKind,
        source: &Path,
        representative_image: &Path,
    ) -> Result<ActiveMediaRecord, String> {
        validate_identifier(output, "output")?;
        validate_identifier(owner, "owner")?;
        let source = canonical_file(source, "active media source")?;
        let representative_image = canonical_file(representative_image, "representative image")?;
        let record = ActiveMediaRecord {
            schema_version: SCHEMA_VERSION,
            output: output.into(),
            owner: owner.into(),
            media_type,
            source,
            representative_image,
            updated_at_unix_ms: now_unix_ms(),
        };
        self.update(|index| {
            if let Some(previous) = index.records.get(output)
                && previous.owner != owner
            {
                let suspended = index.suspended.entry(output.into()).or_default();
                suspended.retain(|entry| entry.owner != previous.owner);
                suspended.push(previous.clone());
            }
            index.records.insert(output.into(), record.clone());
            Ok(record)
        })
    }

    pub fn get(&self, output: &str) -> Result<Option<ActiveMediaRecord>, String> {
        validate_identifier(output, "output")?;
        Ok(self.load()?.records.get(output).cloned())
    }

    pub fn list(&self) -> Result<Vec<ActiveMediaRecord>, String> {
        Ok(self.load()?.records.into_values().collect())
    }

    pub fn remove(&self, output: &str, owner: &str) -> Result<bool, String> {
        validate_identifier(output, "output")?;
        validate_identifier(owner, "owner")?;
        self.update(|index| {
            let Some(record) = index.records.get(output) else {
                return Ok(false);
            };
            if record.owner != owner {
                return Err(format!(
                    "active media ownership conflict: {output} belongs to {}",
                    record.owner
                ));
            }
            index.records.remove(output);
            if let Some(previous) = index
                .suspended
                .get_mut(output)
                .and_then(|records| records.pop())
            {
                index.records.insert(output.into(), previous);
            }
            if index
                .suspended
                .get(output)
                .is_some_and(|records| records.is_empty())
            {
                index.suspended.remove(output);
            }
            Ok(true)
        })
    }

    fn update<T>(
        &self,
        mutate: impl FnOnce(&mut ActiveMediaIndex) -> Result<T, String>,
    ) -> Result<T, String> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "active media state path has no parent".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create active media state directory: {error}"))?;
        let _lock = FileLock::acquire(self.path.with_extension("lock"))?;
        let mut index = self.load()?;
        let result = mutate(&mut index)?;
        self.store(&index)?;
        Ok(result)
    }

    fn load(&self) -> Result<ActiveMediaIndex, String> {
        if !self.path.exists() {
            return Ok(ActiveMediaIndex::default());
        }
        let bytes = fs::read(&self.path)
            .map_err(|error| format!("failed to read active media state: {error}"))?;
        let index = serde_json::from_slice::<ActiveMediaIndex>(&bytes)
            .map_err(|error| format!("failed to parse active media state: {error}"))?;
        if index.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "unsupported active media state schema: {}",
                index.schema_version
            ));
        }
        Ok(index)
    }

    fn store(&self, index: &ActiveMediaIndex) -> Result<(), String> {
        let temporary = self
            .path
            .with_extension(format!("json.tmp-{}", std::process::id()));
        let bytes = serde_json::to_vec_pretty(index)
            .map_err(|error| format!("failed to serialize active media state: {error}"))?;
        fs::write(&temporary, bytes)
            .map_err(|error| format!("failed to write active media state: {error}"))?;
        fs::rename(&temporary, &self.path)
            .map_err(|error| format!("failed to replace active media state: {error}"))
    }
}

struct FileLock {
    path: PathBuf,
}

impl FileLock {
    fn acquire(path: PathBuf) -> Result<Self, String> {
        for _ in 0..100 {
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(_) => return Ok(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(error) => {
                    return Err(format!("failed to lock active media state: {error}"));
                }
            }
        }
        Err("timed out waiting for active media state lock".into())
    }
}

impl Drop for FileLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn canonical_file(path: &Path, label: &str) -> Result<PathBuf, String> {
    if !path.is_absolute() || !path.is_file() {
        return Err(format!("{label} must be an existing absolute file"));
    }
    path.canonicalize()
        .map_err(|error| format!("failed to resolve {label}: {error}"))
}

fn validate_identifier(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._:-".contains(character))
    {
        return Err(format!("invalid active media {label}: {value}"));
    }
    Ok(())
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "active-media-{name}-{}-{}",
            std::process::id(),
            now_unix_ms()
        ))
    }

    #[test]
    fn publishes_replaces_and_protects_ownership() {
        let root = root("publish");
        fs::create_dir_all(&root).unwrap();
        let first = root.join("first.png");
        let second = root.join("second.mp4");
        let preview = root.join("preview.jpg");
        fs::write(&first, b"first").unwrap();
        fs::write(&second, b"second").unwrap();
        fs::write(&preview, b"preview").unwrap();
        let store = ActiveMediaStore::new(root.join("state.json"));

        store
            .publish("DP-1", "kitowall", ActiveMediaKind::Static, &first, &first)
            .unwrap();
        let live = store
            .publish(
                "DP-1",
                "kilivepaper",
                ActiveMediaKind::Live,
                &second,
                &preview,
            )
            .unwrap();
        assert_eq!(store.list().unwrap(), vec![live]);
        assert!(store.remove("DP-1", "kitowall").is_err());
        assert!(store.remove("DP-1", "kilivepaper").unwrap());
        assert_eq!(store.get("DP-1").unwrap().unwrap().owner, "kitowall");
        let _ = fs::remove_dir_all(root);
    }
}

use super::WallpaperPalette;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub struct PaletteCache {
    root: PathBuf,
}

impl PaletteCache {
    pub fn from_environment() -> Self {
        let root = std::env::var("KITSUNE_COMPOSITOR_PALETTE_CACHE")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                std::env::var("XDG_CACHE_HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| PathBuf::from(home).join(".cache"))
                    .join("kitsune-compositor/palettes")
            });
        Self { root }
    }

    pub fn load(&self, image: &Path) -> Option<WallpaperPalette> {
        let path = self.path(image).ok()?;
        let bytes = fs::read(path).ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    pub fn store(&self, image: &Path, palette: &WallpaperPalette) -> Result<(), String> {
        fs::create_dir_all(&self.root)
            .map_err(|error| format!("failed to create palette cache: {error}"))?;
        let path = self.path(image)?;
        let temporary = path.with_extension(format!("json.tmp-{}", std::process::id()));
        let bytes = serde_json::to_vec_pretty(palette)
            .map_err(|error| format!("failed to serialize palette cache: {error}"))?;
        fs::write(&temporary, bytes)
            .map_err(|error| format!("failed to write palette cache: {error}"))?;
        fs::rename(&temporary, &path)
            .map_err(|error| format!("failed to commit palette cache: {error}"))
    }

    fn path(&self, image: &Path) -> Result<PathBuf, String> {
        let metadata = fs::metadata(image)
            .map_err(|error| format!("failed to inspect appearance image: {error}"))?;
        let modified = metadata
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|value| value.as_nanos())
            .unwrap_or_default();
        let mut hasher = DefaultHasher::new();
        image.hash(&mut hasher);
        metadata.len().hash(&mut hasher);
        modified.hash(&mut hasher);
        Ok(self.root.join(format!("{:016x}.json", hasher.finish())))
    }
}

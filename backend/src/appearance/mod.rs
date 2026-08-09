mod cache;
mod caelestia;
mod model;
mod palette;
mod policy;
mod provider;
mod state;

pub use model::{
    AppearanceAdvisory, AppearanceApplyResult, AppearanceCapabilities, AppearanceCurrent,
    AppearanceMode, AppearanceOperation, AppearancePlan, AppearancePolicy, AppearancePreview,
    AppearanceRestoreResult, AppearanceState, CaelestiaSnapshot, PaletteCandidate,
    WallpaperPalette,
};

use crate::{ActiveMediaStore, HostRunner, ProcessExecutor};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct AppearanceEngine<R> {
    runner: R,
    cache: cache::PaletteCache,
    state: state::AppearanceStateStore,
    active_media: ActiveMediaStore,
    policy: policy::AppearancePolicyStore,
}

impl<R: HostRunner> AppearanceEngine<R> {
    pub fn new(runner: R) -> Self {
        Self {
            runner,
            cache: cache::PaletteCache::from_environment(),
            state: state::AppearanceStateStore::from_environment(),
            active_media: ActiveMediaStore::from_environment(),
            policy: policy::AppearancePolicyStore::from_environment(),
        }
    }

    pub fn capabilities(&self) -> AppearanceCapabilities {
        provider::detect(&self.runner)
    }

    pub fn policy(&self) -> Result<AppearancePolicy, String> {
        self.policy.load()
    }

    pub fn enable_automatic(
        &self,
        output: &str,
        confirmed: bool,
    ) -> Result<AppearancePolicy, String> {
        if !confirmed {
            return Err(
                "appearance automatic policy requires --confirm because Caelestia may execute postHook"
                    .into(),
            );
        }
        if output.is_empty() {
            return Err("appearance policy output cannot be empty".into());
        }
        let policy = AppearancePolicy {
            schema_version: 1,
            enabled: true,
            source_output: Some(output.into()),
            automatic_apply: true,
            post_hook_consent: true,
            updated_at_unix_ms: now_unix_ms(),
        };
        self.policy.store(&policy)?;
        Ok(policy)
    }

    pub fn disable_automatic(&self) -> Result<AppearancePolicy, String> {
        let policy = AppearancePolicy {
            updated_at_unix_ms: now_unix_ms(),
            ..AppearancePolicy::default()
        };
        self.policy.store(&policy)?;
        Ok(policy)
    }

    pub fn sync_automatic_for_output<E: ProcessExecutor>(
        &self,
        executor: &E,
        output: &str,
    ) -> Result<Option<AppearanceApplyResult>, String> {
        let policy = self.policy.load()?;
        if !policy.enabled
            || !policy.automatic_apply
            || policy.source_output.as_deref() != Some(output)
        {
            return Ok(None);
        }
        if !policy.post_hook_consent {
            return Err("appearance automatic policy lacks postHook consent".into());
        }
        self.ensure_compatible()?;
        let _guard = self.state.lock_apply()?;
        let record = self
            .active_media
            .get(output)?
            .ok_or_else(|| format!("no active media registered for output: {output}"))?;
        if self.state.load()?.is_some_and(|state| {
            state.source_output.as_deref() == Some(output)
                && state.image == record.representative_image
        }) {
            return Ok(None);
        }
        self.apply_for_output(executor, output, false, true)
            .map(Some)
    }

    pub fn preview(&self, image: &Path, use_cache: bool) -> Result<AppearancePreview, String> {
        let image = image
            .canonicalize()
            .map_err(|error| format!("failed to resolve appearance image: {error}"))?;
        let cached = use_cache.then(|| self.cache.load(&image)).flatten();
        let cache_hit = cached.is_some();
        let mut cache_warning = None;
        let palette = match cached {
            Some(palette) => palette,
            None => {
                let palette = palette::extract(&image)?;
                if use_cache {
                    cache_warning = self.cache.store(&image, &palette).err();
                }
                palette
            }
        };
        Ok(AppearancePreview {
            image,
            source_output: None,
            cache_hit,
            cache_warning,
            palette,
            provider: self.capabilities(),
        })
    }

    pub fn preview_for_output(
        &self,
        output: &str,
        use_cache: bool,
    ) -> Result<AppearancePreview, String> {
        let record = self
            .active_media
            .get(output)?
            .ok_or_else(|| format!("no active media registered for output: {output}"))?;
        let mut preview = self.preview(&record.representative_image, use_cache)?;
        preview.source_output = Some(output.into());
        Ok(preview)
    }

    pub fn current(&self) -> AppearanceCurrent {
        match self.state.load() {
            Ok(Some(state)) => AppearanceCurrent {
                active: true,
                provider: self.capabilities(),
                image: Some(state.image),
                source_output: state.source_output,
                palette: Some(state.palette),
                reason: "compositor-owned appearance state is active".into(),
            },
            Ok(None) => AppearanceCurrent {
                active: false,
                provider: self.capabilities(),
                image: None,
                source_output: None,
                palette: None,
                reason: "no compositor-owned appearance state exists".into(),
            },
            Err(error) => AppearanceCurrent {
                active: false,
                provider: self.capabilities(),
                image: None,
                source_output: None,
                palette: None,
                reason: error,
            },
        }
    }

    pub fn plan_apply(&self, image: &Path) -> Result<AppearancePlan, String> {
        self.plan_apply_source(image, None)
    }

    pub fn plan_apply_for_output(&self, output: &str) -> Result<AppearancePlan, String> {
        let record = self
            .active_media
            .get(output)?
            .ok_or_else(|| format!("no active media registered for output: {output}"))?;
        self.plan_apply_source(&record.representative_image, Some(output.into()))
    }

    fn plan_apply_source(
        &self,
        image: &Path,
        source_output: Option<String>,
    ) -> Result<AppearancePlan, String> {
        let preview = self.preview(image, true)?;
        if preview.provider.backend != "caelestia" {
            return Err(format!(
                "appearance apply is not implemented for backend: {}",
                preview.provider.backend
            ));
        }
        Ok(AppearancePlan {
            provider: preview.provider,
            image: preview.image.clone(),
            source_output,
            palette: preview.palette,
            operations: caelestia::apply_operations(&preview.image, true),
            requires_confirmation: true,
            may_run_caelestia_post_hook: true,
        })
    }

    pub fn apply<E: ProcessExecutor>(
        &self,
        executor: &E,
        image: &Path,
        dry_run: bool,
        confirmed: bool,
    ) -> Result<AppearanceApplyResult, String> {
        let plan = self.plan_apply(image)?;
        self.apply_plan(executor, plan, dry_run, confirmed)
    }

    pub fn apply_for_output<E: ProcessExecutor>(
        &self,
        executor: &E,
        output: &str,
        dry_run: bool,
        confirmed: bool,
    ) -> Result<AppearanceApplyResult, String> {
        let plan = self.plan_apply_for_output(output)?;
        self.apply_plan(executor, plan, dry_run, confirmed)
    }

    fn apply_plan<E: ProcessExecutor>(
        &self,
        executor: &E,
        mut plan: AppearancePlan,
        dry_run: bool,
        confirmed: bool,
    ) -> Result<AppearanceApplyResult, String> {
        if dry_run {
            return Ok(AppearanceApplyResult {
                applied: false,
                dry_run: true,
                plan,
                state: None,
            });
        }
        if !confirmed {
            return Err(
                "appearance apply requires --confirm because Caelestia may execute postHook".into(),
            );
        }
        self.ensure_compatible()?;
        if !executor.command_exists("caelestia") {
            return Err("caelestia is not installed".into());
        }
        let existing = self.state.load()?;
        let (previous, current_name) = match existing {
            Some(state) if state.backend == "caelestia" => {
                (state.previous_caelestia, caelestia::current_name(executor)?)
            }
            Some(state) => {
                return Err(format!(
                    "appearance state belongs to a different backend: {}",
                    state.backend
                ));
            }
            None => {
                let snapshot = caelestia::snapshot(executor)?;
                let current_name = snapshot.name.clone();
                (snapshot, current_name)
            }
        };
        if current_name == "dynamic" {
            plan.operations = caelestia::apply_operations(&plan.image, false);
        }
        if let Err(error) = caelestia::execute(executor, &plan.operations) {
            let rollback = caelestia::execute(executor, &caelestia::restore_operations(&previous));
            return Err(match rollback {
                Ok(()) => format!("appearance apply failed and was rolled back: {error}"),
                Err(rollback) => {
                    format!("appearance apply failed: {error}; rollback also failed: {rollback}")
                }
            });
        }
        let state = AppearanceState {
            schema_version: 1,
            backend: "caelestia".into(),
            image: plan.image.clone(),
            source_output: plan.source_output.clone(),
            palette: plan.palette.clone(),
            previous_caelestia: previous,
            applied_at_unix_ms: now_unix_ms(),
        };
        if let Err(error) = self.state.store(&state) {
            let rollback = caelestia::execute(
                executor,
                &caelestia::restore_operations(&state.previous_caelestia),
            );
            return Err(match rollback {
                Ok(()) => {
                    format!("appearance state could not be saved; changes rolled back: {error}")
                }
                Err(rollback) => format!(
                    "appearance state could not be saved: {error}; rollback also failed: {rollback}"
                ),
            });
        }
        Ok(AppearanceApplyResult {
            applied: true,
            dry_run: false,
            plan,
            state: Some(state),
        })
    }

    fn ensure_compatible(&self) -> Result<(), String> {
        let capabilities = self.capabilities();
        let Some(advisory) = capabilities
            .advisories
            .iter()
            .find(|advisory| advisory.severity == "blocking")
        else {
            return Ok(());
        };
        let suggested = advisory
            .suggested_json
            .as_deref()
            .map(|json| format!(" Suggested configuration: {json}"))
            .unwrap_or_default();
        Err(format!("{}{suggested}", advisory.message))
    }

    pub fn restore<E: ProcessExecutor>(
        &self,
        executor: &E,
        dry_run: bool,
        confirmed: bool,
    ) -> Result<AppearanceRestoreResult, String> {
        let provider = self.capabilities();
        let state = self
            .state
            .load()?
            .ok_or_else(|| "no appearance state to restore".to_string())?;
        if state.backend != "caelestia" {
            return Err(format!(
                "appearance restore is not implemented for backend: {}",
                state.backend
            ));
        }
        let operations = caelestia::restore_operations(&state.previous_caelestia);
        if dry_run {
            return Ok(AppearanceRestoreResult {
                restored: false,
                dry_run: true,
                provider,
                operations,
            });
        }
        if !confirmed {
            return Err("appearance restore requires --confirm".into());
        }
        let current_name = caelestia::current_name(executor)?;
        let current_wallpaper = caelestia::current_wallpaper(executor)?;
        if current_name != "dynamic" || current_wallpaper.as_ref() != Some(&state.image) {
            return Err(
                "appearance state conflict: Caelestia was changed after compositor apply".into(),
            );
        }
        caelestia::execute(executor, &operations)?;
        self.state.remove()?;
        Ok(AppearanceRestoreResult {
            restored: true,
            dry_run: false,
            provider,
            operations,
        })
    }
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

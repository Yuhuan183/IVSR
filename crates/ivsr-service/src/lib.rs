//! The ivsr core service: configuration, the registry of implementations,
//! job planning and execution, engine installation and self-update. Both the
//! CLI and the desktop app are thin adapters over this crate.

pub mod advice;
pub mod bench;
pub mod config;
pub mod engines;
mod error;
pub mod filtering;
pub mod history;
pub mod i18n;
pub mod models;
pub mod paths;
pub mod planner;
pub mod preview;
pub mod queue;
pub mod registry;
pub mod request;
pub mod system;
pub mod throttle;
pub mod updates;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use ivsr_core::pipeline::{self, Toolkit};
use ivsr_core::{
    CancelToken, EngineCaps, EngineInfo, FilterStage, FilterStep, JobOutcome, JobSpec, MediaKind, ModelInfo, ParamSpec,
    ParamValues, Reporter, ToolStatus, UpscaleSettings,
};
use ivsr_update::{HttpClient, UreqClient};
use serde::Serialize;

pub use config::{Config, ConflictPolicy, FiltersConfig};
pub use advice::{Advice, Fit};
pub use bench::BenchmarkRecord;
pub use engines::{InstallManifest, InstallProgress, InstallReport};
pub use error::{Error, Result};
pub use filtering::{FilterJob, FilterJobRequest, FilterOutcome, FilterView, PreparedFilters};
pub use history::HistoryEntry;
pub use i18n::Lang;
pub use models::{ImportRequest, ModelEntry, ModelOverview, ModelStatus};
pub use system::{GpuInfo, SystemInfo};
pub use paths::AppPaths;
pub use planner::{PlannedJob, SkipReason};
pub use queue::{EventSink, JobEvent, JobId, JobQueue};
pub use registry::Registry;
pub use request::JobRequest;
pub use updates::{Flavor, Updates};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct Service {
    paths: AppPaths,
    config_file: PathBuf,
    config: Config,
    registry: Registry,
    http: Arc<dyn HttpClient>,
}

/// Everything a frontend needs to present one engine.
#[derive(Debug, Clone, Serialize)]
pub struct EngineView {
    pub info: EngineInfo,
    pub status: ToolStatus,
    pub models: Vec<ModelInfo>,
    pub default_model: Option<String>,
    pub params: Vec<ParamSpec>,
    pub caps: EngineCaps,
    pub installable: bool,
    pub installed: Option<InstallManifest>,
}

/// A validated batch: shared settings plus one planned job per input.
#[derive(Debug, Clone, Serialize)]
pub struct Prepared {
    pub engine: String,
    pub settings: UpscaleSettings,
    pub jobs: Vec<PlannedJob>,
}

impl Prepared {
    /// The executable job for `planned`, or `None` when it is skipped.
    pub fn spec(&self, planned: &PlannedJob) -> Option<JobSpec> {
        if planned.skip.is_some() {
            return None;
        }
        let mut settings = self.settings.clone();
        if planned.kind == MediaKind::Image {
            settings.image_format = planned.format.clone();
        }
        Some(JobSpec { input: planned.input.clone(), output: planned.output.clone(), kind: planned.kind, settings })
    }

    pub fn runnable(&self) -> impl Iterator<Item = (&PlannedJob, JobSpec)> {
        self.jobs.iter().filter_map(|j| self.spec(j).map(|s| (j, s)))
    }
}

impl Service {
    /// Loads configuration from `config_file`, or the platform default location.
    pub fn load(config_file: Option<&Path>) -> Result<Self> {
        let paths = AppPaths::discover();
        let file = config_file.map(Path::to_path_buf).unwrap_or_else(|| paths.config_file.clone());
        let config = Config::load(&file)?;
        Ok(Self::with_config(paths, file, config))
    }

    pub fn with_config(paths: AppPaths, config_file: PathBuf, config: Config) -> Self {
        let registry = Registry::from_config(&config, &paths);
        let http: Arc<dyn HttpClient> = Arc::new(UreqClient::new(&format!("ivsr/{VERSION}")));
        Self { paths, config_file, config, registry, http }
    }

    /// Replaces the implementations (tests, embedding).
    pub fn with_registry(mut self, registry: Registry) -> Self {
        self.registry = registry;
        self
    }

    pub fn with_http(mut self, http: Arc<dyn HttpClient>) -> Self {
        self.http = http;
        self
    }

    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }

    pub fn config_file(&self) -> &Path {
        &self.config_file
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Persists `config` and returns a service rebuilt around it.
    pub fn reconfigure(&self, config: Config) -> Result<Self> {
        config.save(&self.config_file)?;
        Ok(Self::with_config(self.paths.clone(), self.config_file.clone(), config).with_http(self.http.clone()))
    }

    pub fn engine_views(&self) -> Vec<EngineView> {
        self.registry
            .engines()
            .iter()
            .map(|e| EngineView {
                info: e.info(),
                status: e.status(),
                models: e.models(),
                default_model: e.default_model(),
                params: e.params(),
                caps: e.caps(),
                installable: e.distribution().is_some() && e.install_dir().is_some(),
                installed: e.install_dir().and_then(|d| engines::installed(&d)),
            })
            .collect()
    }

    /// Validates `req`, applies defaults and plans one job per input file.
    pub fn prepare(&self, inputs: &[PathBuf], req: &JobRequest) -> Result<Prepared> {
        let resolved = request::resolve(req, &self.config, &self.registry)?;
        let formats = self.registry.formats();
        let opts = planner::PlanOptions {
            output: resolved.output.as_deref(),
            suffix: &resolved.suffix,
            image_format: &resolved.image_format,
            container: &resolved.container,
            video_codec: resolved.codec.as_ref(),
            conflict: resolved.conflict,
            recursive: resolved.recursive,
        };
        let mut jobs = planner::plan(inputs, &opts, &formats)?;
        if jobs.iter().any(|j| j.kind == MediaKind::Video && j.skip.is_none()) {
            match self.registry.video().status().problem() {
                Some(problem) => {
                    for job in jobs.iter_mut().filter(|j| j.kind == MediaKind::Video) {
                        job.skip = Some(SkipReason::VideoUnavailable { detail: problem.to_string() });
                    }
                }
                None => request::validate_video(&resolved.settings.video, resolved.codec.as_ref())?,
            }
        }
        Ok(Prepared { engine: resolved.engine.info().id, settings: resolved.settings, jobs })
    }

    /// Runs one job to completion on the calling thread.
    pub fn run(&self, spec: &JobSpec, engine: &str, reporter: &dyn Reporter, cancel: &CancelToken) -> Result<JobOutcome> {
        let engine = self.registry.engine(engine)?;
        let toolkit = Toolkit {
            engine: engine.as_ref(),
            images: self.registry.images().as_ref(),
            video: Some(self.registry.video().as_ref()),
            filters: self.registry.filters(),
        };
        let started = Instant::now();
        let outcome = pipeline::run(&toolkit, spec, &self.work_dir(), reporter, cancel)?;
        if self.config.history.enabled {
            let entry = HistoryEntry::new(spec, &engine.info().id, &outcome, started.elapsed().as_millis() as u64);
            // History is a convenience; failing to write it must not fail the job.
            let _ = self.history_store().record(entry);
        }
        Ok(outcome)
    }

    fn work_dir(&self) -> PathBuf {
        self.config.work_dir.clone().unwrap_or_else(|| self.paths.default_work_dir())
    }

    /// Interface language for this configuration and environment.
    pub fn lang(&self) -> Lang {
        i18n::resolve(&self.config)
    }

    fn history_store(&self) -> history::History {
        history::History::new(self.paths.history_file(), self.config.history.limit)
    }

    pub fn history(&self) -> Vec<HistoryEntry> {
        self.history_store().load()
    }

    pub fn remove_history(&self, ids: &[u64]) -> Result<()> {
        self.history_store().remove(ids)
    }

    pub fn clear_history(&self) -> Result<()> {
        self.history_store().clear()
    }

    fn catalogs(&self, engine: &dyn ivsr_core::Engine, refresh: bool) -> models::Catalogs {
        models::load_catalogs(engine, &self.config.models.catalogs, &self.paths.catalog_cache(), self.http.as_ref(), refresh)
    }

    /// Installed and available models for `engine`. Remote catalogues come from
    /// cache unless `refresh` (or the cache is older than a day).
    pub fn model_overview(&self, engine: &str, refresh: bool) -> Result<ModelOverview> {
        let engine = self.registry.engine(engine)?;
        Ok(models::overview(engine.as_ref(), self.catalogs(engine.as_ref(), refresh)))
    }

    /// Downloads (or updates) a catalogued model.
    pub fn install_model(
        &self,
        engine: &str,
        id: &str,
        progress: &mut dyn FnMut(InstallProgress),
        cancel: &CancelToken,
    ) -> Result<ModelInfo> {
        let engine = self.registry.engine(engine)?;
        let catalogs = self.catalogs(engine.as_ref(), false);
        models::install(engine.as_ref(), &catalogs, id, self.http.clone(), progress, cancel)
    }

    /// Removes a managed model. Returns the saved configuration when the
    /// removed model was the configured default (which is then cleared).
    pub fn remove_model(&self, engine_id: &str, id: &str) -> Result<Option<Config>> {
        let engine = self.registry.engine(engine_id)?;
        models::remove(engine.as_ref(), id)?;
        let mut config = self.config.clone();
        match config.engines.get_mut(engine_id) {
            Some(cfg) if cfg.model.as_deref() == Some(id) => {
                cfg.model = None;
                config.save(&self.config_file)?;
                Ok(Some(config))
            }
            _ => Ok(None),
        }
    }

    /// Adds a model from local files after proving it runs at the declared scale.
    pub fn import_model(&self, engine: &str, req: &ImportRequest) -> Result<ModelInfo> {
        let engine_ref = self.registry.engine(engine)?;
        let params = self.default_params(engine_ref.as_ref())?;
        models::import(engine_ref.as_ref(), self.registry.images().as_ref(), req, &params, &self.work_dir())
    }

    fn default_params(&self, engine: &dyn ivsr_core::Engine) -> Result<ParamValues> {
        let configured = self.config.engine_config(&engine.info().id).params;
        Ok(ParamValues::resolve(&engine.params(), &configured)?)
    }

    /// Times `model` on synthetic images and stores the result.
    pub fn benchmark(
        &self,
        engine: &str,
        model: &str,
        scale: Option<u32>,
        progress: &mut dyn FnMut(f64),
        cancel: &CancelToken,
    ) -> Result<BenchmarkRecord> {
        let engine = self.registry.engine(engine)?;
        if let Some(problem) = engine.status().problem() {
            return Err(ivsr_core::Error::EngineUnavailable { engine: engine.info().id, reason: problem.into() }.into());
        }
        let info = engine
            .models()
            .into_iter()
            .find(|m| m.id == model)
            .ok_or_else(|| Error::Input(format!("model `{model}` is not installed")))?;
        let scale = scale.unwrap_or(info.scales[0]);
        let params = self.default_params(engine.as_ref())?;
        let record = bench::run(engine.as_ref(), model, scale, &params, &self.work_dir(), progress, cancel)?;
        bench::BenchStore::new(self.paths.benchmarks_file()).save(&record)?;
        Ok(record)
    }

    pub fn benchmarks(&self) -> Vec<BenchmarkRecord> {
        bench::BenchStore::new(self.paths.benchmarks_file()).load()
    }

    /// OS, CPU, memory and the GPUs `engine` can use (may launch the engine once).
    pub fn system_info(&self, engine: &str) -> Result<SystemInfo> {
        let engine = self.registry.engine(engine)?;
        Ok(system::collect(&engine.devices()))
    }

    /// The GPU a run with the configured parameters would use.
    pub fn active_gpu(&self, engine: &str, system: &SystemInfo) -> Option<GpuInfo> {
        let engine = self.registry.engine(engine).ok()?;
        let params = self.default_params(engine.as_ref()).ok()?;
        let wanted = params.get("gpu").and_then(|v| v.as_str()).and_then(|g| g.split(',').next()?.trim().parse::<u32>().ok());
        system.gpus.iter().find(|g| wanted.is_none() || g.index == wanted).cloned()
    }

    /// Supported files named by `inputs`, expanding directories.
    pub fn expand_inputs(&self, inputs: &[PathBuf], recursive: bool) -> Result<Vec<PathBuf>> {
        planner::expand(inputs, recursive, &self.registry.formats())
    }

    /// A small cached JPEG preview of an image, for frontends.
    pub fn thumbnail(&self, image: &Path, max_side: u32) -> Result<PathBuf> {
        preview::thumbnail(self.registry.images().as_ref(), image, &self.thumbnail_dir(), max_side)
    }

    pub fn thumbnail_dir(&self) -> PathBuf {
        self.paths.cache_dir.join("thumbs")
    }

    pub fn filter_views(&self) -> Vec<FilterView> {
        self.registry.filters().iter().map(|f| FilterView { info: f.info(), params: f.params() }).collect()
    }

    /// The steps `stage` runs when switched on: configured, else the built-in order.
    pub fn filter_steps(&self, stage: FilterStage) -> Vec<FilterStep> {
        self.config.filters.chain(stage).steps.clone().unwrap_or_else(|| self.registry.default_filter_steps(stage))
    }

    /// Plans stand-alone filtering of the images named by `inputs`. Without an
    /// explicit reference, each result's original is looked up in the history.
    pub fn prepare_filters(&self, inputs: &[PathBuf], req: &FilterJobRequest) -> Result<PreparedFilters> {
        let steps = req.steps.clone().unwrap_or_else(|| self.filter_steps(req.stage));
        let defaults = (self.config.output.conflict, self.config.output.image_quality);
        let originals: std::collections::HashMap<PathBuf, PathBuf> = match req.reference {
            Some(_) => Default::default(),
            None => self.history().into_iter().map(|e| (e.output, e.input)).collect(),
        };
        let reference_for = |result: &Path| originals.get(result).filter(|p| p.is_file()).cloned();
        filtering::prepare(&self.registry, inputs, req, steps, defaults, reference_for)
    }

    pub fn run_filters(&self, prepared: &PreparedFilters, job: &FilterJob) -> Result<FilterOutcome> {
        let planned = &job.planned;
        self.filter_image(&planned.input, &planned.output, prepared.stage, &prepared.steps, job.reference.as_deref(), Some(prepared.quality))
    }

    /// Runs `steps` on one image and writes `output` in the format its extension names.
    pub fn filter_image(
        &self,
        input: &Path,
        output: &Path,
        stage: FilterStage,
        steps: &[FilterStep],
        reference: Option<&Path>,
        quality: Option<u8>,
    ) -> Result<FilterOutcome> {
        filtering::filter_image(&self.registry, input, output, stage, steps, reference, quality, &self.work_dir())
    }

    /// A cached PNG of `input` filtered by `steps`, for frontends to show.
    pub fn filter_preview(&self, input: &Path, stage: FilterStage, steps: &[FilterStep], reference: Option<&Path>) -> Result<PathBuf> {
        filtering::preview(&self.registry, input, stage, steps, reference, &self.preview_dir(), &self.work_dir())
    }

    pub fn preview_dir(&self) -> PathBuf {
        self.paths.cache_dir.join("previews")
    }

    pub fn install_engine(
        &self,
        id: &str,
        progress: &mut dyn FnMut(InstallProgress),
        cancel: &CancelToken,
    ) -> Result<InstallReport> {
        let engine = self.registry.engine(id)?;
        engines::install(engine.as_ref(), &self.paths.downloads_dir(), self.http.clone(), progress, cancel)
    }

    pub fn updates(&self, flavor: Flavor, current_version: &str) -> Updates<'_> {
        Updates::new(&self.config.update, &self.paths, self.http.clone(), flavor, current_version)
    }
}

#[cfg(test)]
mod tests;

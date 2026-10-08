//! Job orchestration: composes an `Engine` with media I/O to turn one input
//! file into one upscaled output file.

mod image;
mod video;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::filter::Runner;
use crate::scale::{ScalePlan, plan_scale};
use crate::{
    CancelToken, Engine, Error, Filter, FilterSetup, FilterSpec, FilterStage, ImageIo, LogLevel, MediaKind, ModelInfo,
    ParamValues, Progress, Reporter, Result, Stage, VideoEncodeOptions, VideoIo,
};

/// Everything a job needs to know about how to upscale.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UpscaleSettings {
    pub model: String,
    pub scale: f64,
    /// Engine parameters, already validated against the engine schema.
    pub params: ParamValues,
    /// Output image format id; ignored for video.
    pub image_format: String,
    pub image_quality: Option<u8>,
    pub video: VideoEncodeOptions,
    /// Frames upscaled per engine invocation; bounds temporary disk usage.
    pub batch_frames: u32,
    /// Filters run on the source before the engine, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre: Vec<FilterSpec>,
    /// Filters run on the engine output at its final size, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub post: Vec<FilterSpec>,
}

impl UpscaleSettings {
    pub fn filters(&self, stage: FilterStage) -> &[FilterSpec] {
        match stage {
            FilterStage::Pre => &self.pre,
            FilterStage::Post => &self.post,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobSpec {
    pub input: PathBuf,
    pub output: PathBuf,
    pub kind: MediaKind,
    pub settings: UpscaleSettings,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobOutcome {
    pub output: PathBuf,
    pub width: u32,
    pub height: u32,
    pub source_width: u32,
    pub source_height: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frames: Option<u64>,
}

/// The concrete implementations a job runs against.
#[derive(Clone, Copy)]
pub struct Toolkit<'a> {
    pub engine: &'a dyn Engine,
    pub images: &'a dyn ImageIo,
    pub video: Option<&'a dyn VideoIo>,
    /// Filters `UpscaleSettings::pre` and `post` refer to.
    pub filters: &'a [Arc<dyn Filter>],
}

/// Runs `job` to completion inside a private directory under `work_root`.
/// The output appears atomically at `job.output` only on success; the work
/// directory is removed on every exit path.
pub fn run(
    toolkit: &Toolkit<'_>,
    job: &JobSpec,
    work_root: &Path,
    reporter: &dyn Reporter,
    cancel: &CancelToken,
) -> Result<JobOutcome> {
    let tracker = Tracker { reporter };
    tracker.at(Stage::Preparing, 0.0, None);

    let engine = toolkit.engine;
    if let Some(problem) = engine.status().problem() {
        return Err(Error::EngineUnavailable { engine: engine.info().id, reason: problem.to_string() });
    }
    let model = find_model(engine, &job.settings.model)?;
    let plan = plan_scale(&model.scales, job.settings.scale)?;
    if plan.requested > *model.scales.iter().max().unwrap_or(&0) as f64 {
        tracker.log(
            LogLevel::Warn,
            &format!("model `{}` tops out at x{}; the rest is interpolated", model.id, plan.native),
        );
    }

    std::fs::create_dir_all(work_root).map_err(|e| Error::io_at("create work directory", work_root, e))?;
    let work = tempfile::Builder::new()
        .prefix("ivsr-")
        .tempdir_in(work_root)
        .map_err(|e| Error::io_at("create work directory in", work_root, e))?;

    let ctx = JobRun { toolkit, job, plan, work: work.path(), tracker, cancel };
    let outcome = match job.kind {
        MediaKind::Image => image::run(&ctx),
        MediaKind::Video => video::run(&ctx),
    }?;
    tracker.at(Stage::Finalizing, 1.0, None);
    Ok(outcome)
}

fn find_model(engine: &dyn Engine, id: &str) -> Result<ModelInfo> {
    engine
        .models()
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| Error::ModelNotFound { engine: engine.info().id, model: id.to_string() })
}

impl JobRun<'_> {
    /// Starts the job's filters for `stage`; empty when none are configured.
    fn filters(&self, stage: FilterStage) -> Result<Runner> {
        let specs = self.job.settings.filters(stage);
        Runner::start(specs, self.toolkit.filters, FilterSetup { stage, kind: self.job.kind })
    }
}

struct JobRun<'a> {
    toolkit: &'a Toolkit<'a>,
    job: &'a JobSpec,
    plan: ScalePlan,
    work: &'a Path,
    tracker: Tracker<'a>,
    cancel: &'a CancelToken,
}

/// Maps stage-local progress onto the job-wide 0..1 range.
#[derive(Clone, Copy)]
struct Tracker<'a> {
    reporter: &'a dyn Reporter,
}

impl Tracker<'_> {
    fn at(&self, stage: Stage, overall: f64, units: Option<(u64, u64)>) {
        self.reporter.progress(Progress { stage, overall: overall.clamp(0.0, 1.0), units });
    }

    /// Progress within a stage spanning `range` of the whole job.
    fn span(&self, stage: Stage, range: (f64, f64), fraction: f64, units: Option<(u64, u64)>) {
        let fraction = fraction.clamp(0.0, 1.0);
        self.at(stage, range.0 + (range.1 - range.0) * fraction, units);
    }

    fn log(&self, level: LogLevel, message: &str) {
        self.reporter.log(level, message);
    }
}

#[cfg(test)]
mod tests;

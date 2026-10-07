use crate::fsutil;
use crate::{ImageEncodeOptions, JobOutcome, LogLevel, Result, Stage, TaskContext, TaskMode, UpscaleTask};

use super::JobRun;

const UPSCALE: (f64, f64) = (0.02, 0.90);
const ENCODE: (f64, f64) = (0.90, 0.98);

pub(super) fn run(run: &JobRun<'_>) -> Result<JobOutcome> {
    let JobRun { toolkit, job, plan, work, tracker, cancel } = run;
    let images = toolkit.images;
    let engine = toolkit.engine;
    let caps = engine.caps();
    let settings = &job.settings;

    let info = images.probe(&job.input)?;
    let (width, height) = plan.output_size(info.width, info.height);

    // Hand the engine a format it reads natively, converting only when needed.
    let engine_input = if caps.input_formats.iter().any(|f| f == &info.format) {
        job.input.clone()
    } else {
        tracker.log(LogLevel::Debug, &format!("converting {} input to {}", info.format, caps.output_format));
        let converted = work.join(format!("input.{}", caps.output_format));
        let opts = ImageEncodeOptions { format: caps.output_format.clone(), quality: None, resize: None };
        images.convert(&job.input, &converted, &opts)?;
        converted
    };
    cancel.check()?;

    tracker.span(Stage::Upscaling, UPSCALE, 0.0, None);
    let raw = work.join(format!("upscaled.{}", caps.output_format));
    let progress = |f: f64| tracker.span(Stage::Upscaling, UPSCALE, f, None);
    let log = |level: LogLevel, msg: &str| tracker.log(level, msg);
    let task = UpscaleTask {
        input: &engine_input,
        output: &raw,
        mode: TaskMode::File,
        model: &settings.model,
        scale: plan.native,
        params: &settings.params,
    };
    engine.upscale(&task, &TaskContext { cancel, progress: &progress, log: &log })?;
    cancel.check()?;

    tracker.span(Stage::Encoding, ENCODE, 0.0, None);
    let final_file = if plan.needs_resize() || settings.image_format != caps.output_format {
        let encoded = work.join(format!("output.{}", settings.image_format));
        let opts = ImageEncodeOptions {
            format: settings.image_format.clone(),
            quality: settings.image_quality,
            resize: plan.needs_resize().then_some((width, height)),
        };
        images.convert(&raw, &encoded, &opts)?;
        encoded
    } else {
        raw
    };
    cancel.check()?;

    tracker.span(Stage::Finalizing, (ENCODE.1, 1.0), 0.0, None);
    fsutil::persist(&final_file, &job.output)?;
    Ok(JobOutcome {
        output: job.output.clone(),
        width,
        height,
        source_width: info.width,
        source_height: info.height,
        frames: None,
    })
}

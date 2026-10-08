use crate::fsutil;
use crate::{FilterStage, ImageEncodeOptions, JobOutcome, LogLevel, Result, Stage, TaskContext, TaskMode, UpscaleTask};

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
    // Pre-processing always re-encodes, and keeps its result as the reference
    // for post-processing.
    let mut pre = run.filters(FilterStage::Pre)?;
    let mut post = run.filters(FilterStage::Post)?;
    let mut reference = None;
    let engine_input = if !pre.is_empty() {
        tracker.at(Stage::Filtering, 0.0, None);
        let mut frame = images.decode(&job.input, None)?;
        pre.apply(&mut frame, None)?;
        let filtered = work.join(format!("input.{}", caps.output_format));
        images.encode(&frame, &filtered, &ImageEncodeOptions::lossless(&caps.output_format))?;
        reference = Some(frame);
        filtered
    } else if caps.input_formats.iter().any(|f| f == &info.format) {
        job.input.clone()
    } else {
        tracker.log(LogLevel::Debug, &format!("converting {} input to {}", info.format, caps.output_format));
        let converted = work.join(format!("input.{}", caps.output_format));
        images.convert(&job.input, &converted, &ImageEncodeOptions::lossless(&caps.output_format))?;
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

    let resize = plan.needs_resize().then_some((width, height));
    let final_file = if !post.is_empty() {
        // Post-processing sees the picture at its final size.
        tracker.span(Stage::Filtering, ENCODE, 0.0, None);
        let reference = match reference {
            Some(frame) => frame,
            None => images.decode(&engine_input, None)?,
        };
        let mut frame = images.decode(&raw, resize)?;
        post.apply(&mut frame, Some(&reference))?;
        cancel.check()?;
        tracker.span(Stage::Encoding, ENCODE, 0.5, None);
        let encoded = work.join(format!("output.{}", settings.image_format));
        let opts = ImageEncodeOptions { quality: settings.image_quality, ..ImageEncodeOptions::lossless(&settings.image_format) };
        images.encode(&frame, &encoded, &opts)?;
        encoded
    } else {
        tracker.span(Stage::Encoding, ENCODE, 0.0, None);
        if resize.is_some() || settings.image_format != caps.output_format {
            let encoded = work.join(format!("output.{}", settings.image_format));
            let opts = ImageEncodeOptions { format: settings.image_format.clone(), quality: settings.image_quality, resize };
            images.convert(&raw, &encoded, &opts)?;
            encoded
        } else {
            raw
        }
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

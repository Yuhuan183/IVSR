use std::fs;
use std::path::{Path, PathBuf};

use crate::error::IoContext;
use crate::filter::Runner;
use crate::media::EncoderSetup;
use crate::{
    Error, FilterStage, Frame, ImageEncodeOptions, ImageIo, JobOutcome, LogLevel, Result, Stage, TaskContext, TaskMode,
    UpscaleTask, fsutil,
};

use super::JobRun;

const DECODE: (f64, f64) = (0.0, 0.08);
const UPSCALE: (f64, f64) = (0.08, 0.97);

/// Frames are extracted once at source resolution, then processed in batches:
/// pre-processing rewrites the source frames, the engine upscales them, and
/// post-processing (with each source frame as its reference) rewrites the
/// results, which stream straight into a running encoder and are deleted
/// together with their sources. Only one batch of frames is ever on disk.
pub(super) fn run(run: &JobRun<'_>) -> Result<JobOutcome> {
    let JobRun { toolkit, job, plan, work, tracker, cancel } = run;
    let video = toolkit
        .video
        .ok_or_else(|| Error::UnsupportedFormat("video support is not configured".into()))?;
    if let Some(problem) = video.status().problem() {
        return Err(Error::tool("video backend", problem.to_string()));
    }
    let engine = toolkit.engine;
    let caps = engine.caps();
    let settings = &job.settings;
    let mut pre = run.filters(FilterStage::Pre)?;
    let mut post = run.filters(FilterStage::Post)?;

    let info = video.probe(&job.input)?;
    let output_size = even(plan.output_size(info.width, info.height));
    // Post-processing resamples to the final size itself, so the encoder must not.
    let frame_size = if post.is_empty() { (info.width * plan.native, info.height * plan.native) } else { output_size };

    // Decode every frame at source resolution.
    let frames_dir = work.join("frames");
    fs::create_dir_all(&frames_dir).at("create directory", &frames_dir)?;
    tracker.span(Stage::Decoding, DECODE, 0.0, None);
    let decode_progress = |f: f64| tracker.span(Stage::Decoding, DECODE, f, None);
    let log = |level: LogLevel, msg: &str| tracker.log(level, msg);
    let ctx = TaskContext { cancel, progress: &decode_progress, log: &log };
    video.extract_frames(&job.input, &info, &frames_dir, &ctx)?;
    let frames = sorted_files(&frames_dir)?;
    let total = frames.len() as u64;
    if total == 0 {
        return Err(Error::tool("video backend", "no frames were decoded"));
    }
    cancel.check()?;

    let encoded = work.join(format!("output.{}", fsutil::extension(&job.output).unwrap_or_else(|| "mp4".into())));
    let mut encoder = video.open_encoder(&EncoderSetup {
        source: &job.input,
        source_info: &info,
        output: &encoded,
        frame_size,
        output_size,
        options: &settings.video,
    })?;

    // Share of a batch's progress before the engine starts and after it ends.
    let pre_share = if pre.is_empty() { 0.0 } else { 0.05 };
    let post_share = if post.is_empty() { 0.0 } else { 0.15 };
    let batch_in = work.join("batch-in");
    let batch_out = work.join("batch-out");
    let batch = if caps.batch { settings.batch_frames.max(1) as usize } else { 1 };
    let images = toolkit.images;
    let mut done = 0u64;
    // Each phase of a batch counts its frames from the batch start; the
    // reported count only ever rises, so it reads as frames through the chain.
    let shown = std::cell::Cell::new(0u64);
    tracker.span(Stage::Upscaling, UPSCALE, 0.0, Some((0, total)));
    for chunk in frames.chunks(batch) {
        cancel.check()?;
        let base = done;
        let n = chunk.len() as u64;
        // `within` is the fraction of this batch completed; `units` frames done in `stage`.
        let report = |stage: Stage, within: f64, units: u64| {
            let overall = (base as f64 + within.clamp(0.0, 1.0) * n as f64) / total as f64;
            shown.set(shown.get().max(units));
            tracker.span(stage, UPSCALE, overall, Some((shown.get(), total)));
        };

        if !pre.is_empty() {
            for (i, frame) in chunk.iter().enumerate() {
                cancel.check()?;
                rewrite(images, frame, None, &mut pre, None)?;
                report(Stage::Filtering, pre_share * (i + 1) as f64 / n as f64, base + i as u64 + 1);
            }
        }

        let progress = |f: f64| {
            let f = f.clamp(0.0, 1.0);
            report(Stage::Upscaling, pre_share + (1.0 - pre_share - post_share) * f, base + (f * n as f64) as u64);
        };
        let ctx = TaskContext { cancel, progress: &progress, log: &log };
        let upscaled = upscale_chunk(run, chunk, &batch_in, &batch_out, &caps.output_format, &ctx)?;

        for (i, (source, frame)) in upscaled.iter().enumerate() {
            if !post.is_empty() {
                cancel.check()?;
                let reference = post.uses_reference().then(|| images.decode(source, None)).transpose()?;
                rewrite(images, frame, Some(output_size), &mut post, reference.as_ref())?;
                report(Stage::Filtering, 1.0 - post_share + post_share * (i + 1) as f64 / n as f64, base + i as u64 + 1);
            }
            encoder.push_frame(frame)?;
            fs::remove_file(frame).at("remove", frame)?;
            fs::remove_file(source).at("remove", source)?;
        }
        done += n;
        tracker.span(Stage::Upscaling, UPSCALE, done as f64 / total as f64, Some((done, total)));
    }

    tracker.span(Stage::Finalizing, (UPSCALE.1, 1.0), 0.0, Some((total, total)));
    encoder.finish()?;
    fsutil::persist(&encoded, &job.output)?;
    let (width, height) = output_size;
    Ok(JobOutcome {
        output: job.output.clone(),
        width,
        height,
        source_width: info.width,
        source_height: info.height,
        frames: Some(total),
    })
}

/// Runs `filters` on the frame at `path` (resampled to `size`) and rewrites
/// it in place, in the format its extension names.
fn rewrite(
    images: &dyn ImageIo,
    path: &Path,
    size: Option<(u32, u32)>,
    filters: &mut Runner,
    reference: Option<&Frame>,
) -> Result<()> {
    let mut frame = images.decode(path, size)?;
    filters.apply(&mut frame, reference)?;
    let format = fsutil::extension(path).unwrap_or_else(|| "png".into());
    images.encode(&frame, path, &ImageEncodeOptions::lossless(&format))
}

/// Upscales `chunk` and returns `(source, upscaled)` frame pairs in order.
/// Sources are kept so post-processing can use them; the caller deletes both.
fn upscale_chunk(
    run: &JobRun<'_>,
    chunk: &[PathBuf],
    batch_in: &Path,
    batch_out: &Path,
    out_ext: &str,
    ctx: &TaskContext<'_>,
) -> Result<Vec<(PathBuf, PathBuf)>> {
    let settings = &run.job.settings;
    let engine = run.toolkit.engine;
    let out_name = |p: &Path| {
        let stem = p.file_stem().unwrap_or_default().to_string_lossy();
        batch_out.join(format!("{stem}.{out_ext}"))
    };

    if let [frame] = chunk {
        fs::create_dir_all(batch_out).at("create directory", batch_out)?;
        let output = out_name(frame);
        let task = UpscaleTask {
            input: frame,
            output: &output,
            mode: TaskMode::File,
            model: &settings.model,
            scale: run.plan.native,
            params: &settings.params,
        };
        engine.upscale(&task, ctx)?;
        return Ok(vec![(frame.clone(), output)]);
    }

    for dir in [batch_in, batch_out] {
        if dir.exists() {
            fs::remove_dir_all(dir).at("clear", dir)?;
        }
        fs::create_dir_all(dir).at("create directory", dir)?;
    }
    let mut sources = Vec::with_capacity(chunk.len());
    for frame in chunk {
        let target = batch_in.join(frame.file_name().unwrap_or_default());
        fs::rename(frame, &target).at("move", frame)?;
        sources.push(target);
    }
    let task = UpscaleTask {
        input: batch_in,
        output: batch_out,
        mode: TaskMode::Directory { count: chunk.len() as u64 },
        model: &settings.model,
        scale: run.plan.native,
        params: &settings.params,
    };
    engine.upscale(&task, ctx)?;
    let outputs: Vec<PathBuf> = sources.iter().map(|f| out_name(f)).collect();
    if let Some(missing) = outputs.iter().find(|p| !p.exists()) {
        return Err(Error::tool(engine.info().id, format!("engine produced no output for {}", missing.display())));
    }
    Ok(sources.into_iter().zip(outputs).collect())
}

fn sorted_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files: Vec<PathBuf> = fs::read_dir(dir)
        .at("read directory", dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    Ok(files)
}

/// Most codecs with chroma subsampling require even dimensions.
fn even((w, h): (u32, u32)) -> (u32, u32) {
    ((w & !1).max(2), (h & !1).max(2))
}

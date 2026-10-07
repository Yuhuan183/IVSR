use std::fs;
use std::path::{Path, PathBuf};

use crate::error::IoContext;
use crate::media::EncoderSetup;
use crate::{Error, JobOutcome, LogLevel, Result, Stage, TaskContext, TaskMode, UpscaleTask, fsutil};

use super::JobRun;

const DECODE: (f64, f64) = (0.0, 0.08);
const UPSCALE: (f64, f64) = (0.08, 0.97);

/// Frames are extracted once at source resolution, then upscaled in batches
/// whose results stream straight into a running encoder and are deleted, so
/// only one batch of large frames ever sits on disk.
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

    let info = video.probe(&job.input)?;
    let frame_size = (info.width * plan.native, info.height * plan.native);
    let output_size = even(plan.output_size(info.width, info.height));

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

    let batch_in = work.join("batch-in");
    let batch_out = work.join("batch-out");
    let batch = if caps.batch { settings.batch_frames.max(1) as usize } else { 1 };
    let mut done = 0u64;
    tracker.span(Stage::Upscaling, UPSCALE, 0.0, Some((0, total)));
    for chunk in frames.chunks(batch) {
        cancel.check()?;
        let base = done;
        let n = chunk.len() as u64;
        let progress = |f: f64| {
            let units = base + (f.clamp(0.0, 1.0) * n as f64) as u64;
            tracker.span(Stage::Upscaling, UPSCALE, units as f64 / total as f64, Some((units, total)));
        };
        let ctx = TaskContext { cancel, progress: &progress, log: &log };
        let upscaled = upscale_chunk(run, chunk, &batch_in, &batch_out, &caps.output_format, &ctx)?;
        for frame in &upscaled {
            encoder.push_frame(frame)?;
            fs::remove_file(frame).at("remove", frame)?;
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

/// Upscales `chunk` and returns the produced frames in order.
fn upscale_chunk(
    run: &JobRun<'_>,
    chunk: &[PathBuf],
    batch_in: &Path,
    batch_out: &Path,
    out_ext: &str,
    ctx: &TaskContext<'_>,
) -> Result<Vec<PathBuf>> {
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
        fs::remove_file(frame).at("remove", frame)?;
        return Ok(vec![output]);
    }

    for dir in [batch_in, batch_out] {
        if dir.exists() {
            fs::remove_dir_all(dir).at("clear", dir)?;
        }
        fs::create_dir_all(dir).at("create directory", dir)?;
    }
    for frame in chunk {
        let target = batch_in.join(frame.file_name().unwrap_or_default());
        fs::rename(frame, &target).at("move", frame)?;
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
    let outputs: Vec<PathBuf> = chunk.iter().map(|f| out_name(f)).collect();
    if let Some(missing) = outputs.iter().find(|p| !p.exists()) {
        return Err(Error::tool(engine.info().id, format!("engine produced no output for {}", missing.display())));
    }
    fs::remove_dir_all(batch_in).at("clear", batch_in)?;
    Ok(outputs)
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

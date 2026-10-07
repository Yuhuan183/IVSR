//! Orchestration tests against in-memory fakes of the engine and media I/O.
//! Fake files carry their "pixels" as text so each stage's effect is visible.

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

use super::*;
use crate::media::EncoderSetup;
use crate::progress::NullReporter;
use crate::{
    AudioMode, CodecInfo, EngineCaps, EngineInfo, FormatInfo, FrameEncoder, ImageEncodeOptions, ImageInfo, ModelInfo,
    ParamSpec, TaskContext, TaskMode, ToolStatus, UpscaleTask, VideoInfo,
};

#[derive(Default)]
struct FakeEngine {
    calls: Mutex<Vec<TaskMode>>,
    cancel_on_call: Option<CancelToken>,
}

impl Engine for FakeEngine {
    fn info(&self) -> EngineInfo {
        EngineInfo { id: "fake".into(), name: "Fake".into(), description: "".into(), homepage: None }
    }
    fn status(&self) -> ToolStatus {
        ToolStatus::Ready { location: "/fake".into(), version: None }
    }
    fn models(&self) -> Vec<ModelInfo> {
        vec![ModelInfo { id: "m4".into(), name: "M4".into(), scales: vec![4], ..Default::default() }]
    }
    fn params(&self) -> Vec<ParamSpec> {
        vec![]
    }
    fn caps(&self) -> EngineCaps {
        EngineCaps { input_formats: vec!["png".into()], output_format: "png".into(), batch: true }
    }
    fn upscale(&self, task: &UpscaleTask<'_>, ctx: &TaskContext<'_>) -> Result<()> {
        self.calls.lock().unwrap().push(task.mode);
        if let Some(token) = &self.cancel_on_call {
            token.cancel();
        }
        ctx.cancel.check()?;
        let up = |src: &Path, dst: &Path| {
            let body = fs::read_to_string(src).unwrap();
            fs::write(dst, format!("up{}({body})", task.scale)).unwrap();
        };
        match task.mode {
            TaskMode::File => up(task.input, task.output),
            TaskMode::Directory { .. } => {
                for entry in fs::read_dir(task.input).unwrap() {
                    let src = entry.unwrap().path();
                    up(&src, &task.output.join(src.file_name().unwrap()));
                }
            }
        }
        (ctx.progress)(1.0);
        Ok(())
    }
}

/// Image files contain `WxH|format|payload`.
struct FakeImages;

impl ImageIo for FakeImages {
    fn formats(&self) -> Vec<FormatInfo> {
        vec![]
    }
    fn probe(&self, path: &Path) -> Result<ImageInfo> {
        let text = fs::read_to_string(path).unwrap();
        let mut parts = text.split('|');
        let (w, h) = parts.next().unwrap().split_once('x').unwrap();
        Ok(ImageInfo {
            width: w.parse().unwrap(),
            height: h.parse().unwrap(),
            format: parts.next().unwrap().into(),
            has_alpha: false,
        })
    }
    fn convert(&self, src: &Path, dst: &Path, opts: &ImageEncodeOptions) -> Result<()> {
        let body = fs::read_to_string(src).unwrap();
        fs::write(dst, format!("{}{:?}<{body}>", opts.format, opts.resize)).unwrap();
        Ok(())
    }
}

struct FakeVideo {
    frames: u32,
    pushed: Arc<Mutex<Vec<String>>>,
}

struct FakeEncoder {
    output: std::path::PathBuf,
    pushed: Arc<Mutex<Vec<String>>>,
}

impl FrameEncoder for FakeEncoder {
    fn push_frame(&mut self, png: &Path) -> Result<()> {
        self.pushed.lock().unwrap().push(fs::read_to_string(png).unwrap());
        Ok(())
    }
    fn finish(self: Box<Self>) -> Result<()> {
        fs::write(&self.output, self.pushed.lock().unwrap().join(",")).unwrap();
        Ok(())
    }
}

impl VideoIo for FakeVideo {
    fn status(&self) -> ToolStatus {
        ToolStatus::Ready { location: "/ffmpeg".into(), version: None }
    }
    fn formats(&self) -> Vec<FormatInfo> {
        vec![]
    }
    fn codecs(&self) -> Vec<CodecInfo> {
        vec![]
    }
    fn probe(&self, _: &Path) -> Result<VideoInfo> {
        Ok(VideoInfo {
            width: 33,
            height: 17,
            fps_num: 30,
            fps_den: 1,
            frame_count: Some(self.frames as u64),
            duration: None,
            codec: "h264".into(),
            container: "mp4".into(),
            audio: None,
        })
    }
    fn extract_frames(&self, _: &Path, _: &VideoInfo, dir: &Path, _: &TaskContext<'_>) -> Result<u64> {
        for i in 1..=self.frames {
            fs::write(dir.join(format!("{i:08}.png")), format!("f{i}")).unwrap();
        }
        Ok(self.frames as u64)
    }
    fn open_encoder(&self, setup: &EncoderSetup<'_>) -> Result<Box<dyn FrameEncoder>> {
        assert_eq!(setup.frame_size, (132, 68));
        Ok(Box::new(FakeEncoder { output: setup.output.to_path_buf(), pushed: self.pushed.clone() }))
    }
}

fn settings(scale: f64, format: &str) -> UpscaleSettings {
    UpscaleSettings {
        model: "m4".into(),
        scale,
        params: ParamValues::default(),
        image_format: format.into(),
        image_quality: Some(90),
        video: VideoEncodeOptions { codec: "h264".into(), quality: None, preset: None, audio: AudioMode::Auto },
        batch_frames: 2,
    }
}

fn is_empty(dir: &Path) -> bool {
    fs::read_dir(dir).map(|mut d| d.next().is_none()).unwrap_or(true)
}

#[test]
fn image_at_non_native_scale_is_resampled_to_requested_size() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("in.png");
    fs::write(&input, "10x5|png|px").unwrap();
    let job = JobSpec {
        input: input.clone(),
        output: tmp.path().join("out/in_x2.jpg"),
        kind: MediaKind::Image,
        settings: settings(2.0, "jpg"),
    };
    let work = tmp.path().join("work");
    let engine = FakeEngine::default();
    let tk = Toolkit { engine: &engine, images: &FakeImages, video: None };

    let outcome = run(&tk, &job, &work, &NullReporter, &CancelToken::new()).unwrap();

    assert_eq!((outcome.width, outcome.height, outcome.source_width, outcome.source_height), (20, 10, 10, 5));
    assert_eq!(fs::read_to_string(&job.output).unwrap(), "jpgSome((20, 10))<up4(10x5|png|px)>");
    assert!(is_empty(&work), "work directory must be cleaned up");
}

#[test]
fn image_at_native_scale_in_engine_format_skips_reencoding() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("in.webp");
    fs::write(&input, "8x8|webp|px").unwrap();
    let job = JobSpec {
        input,
        output: tmp.path().join("in_x4.png"),
        kind: MediaKind::Image,
        settings: settings(4.0, "png"),
    };
    let engine = FakeEngine::default();
    let tk = Toolkit { engine: &engine, images: &FakeImages, video: None };

    run(&tk, &job, &tmp.path().join("work"), &NullReporter, &CancelToken::new()).unwrap();

    // webp is not an engine input format, so it is converted first; png output is kept as-is.
    assert_eq!(fs::read_to_string(&job.output).unwrap(), "up4(pngNone<8x8|webp|px>)");
}

#[test]
fn video_frames_stream_to_encoder_in_order_across_batches() {
    let tmp = tempfile::tempdir().unwrap();
    let job = JobSpec {
        input: tmp.path().join("clip.mp4"),
        output: tmp.path().join("clip_x3.mp4"),
        kind: MediaKind::Video,
        settings: settings(3.0, "png"),
    };
    let work = tmp.path().join("work");
    let pushed = Arc::new(Mutex::new(Vec::new()));
    let video = FakeVideo { frames: 5, pushed: pushed.clone() };
    let engine = FakeEngine::default();
    let tk = Toolkit { engine: &engine, images: &FakeImages, video: Some(&video) };

    let outcome = run(&tk, &job, &work, &NullReporter, &CancelToken::new()).unwrap();

    assert_eq!(fs::read_to_string(&job.output).unwrap(), "up4(f1),up4(f2),up4(f3),up4(f4),up4(f5)");
    assert_eq!(
        *engine.calls.lock().unwrap(),
        vec![TaskMode::Directory { count: 2 }, TaskMode::Directory { count: 2 }, TaskMode::File]
    );
    // 33x17 at x3 = 99x51, rounded down to even for the encoder.
    assert_eq!((outcome.width, outcome.height, outcome.frames), (98, 50, Some(5)));
    assert!(is_empty(&work));
}

#[test]
fn cancellation_mid_job_leaves_no_output_and_no_work_files() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("in.png");
    fs::write(&input, "4x4|png|px").unwrap();
    let job = JobSpec {
        input,
        output: tmp.path().join("in_x4.png"),
        kind: MediaKind::Image,
        settings: settings(4.0, "png"),
    };
    let work = tmp.path().join("work");
    let cancel = CancelToken::new();
    let engine = FakeEngine { cancel_on_call: Some(cancel.clone()), ..Default::default() };
    let tk = Toolkit { engine: &engine, images: &FakeImages, video: None };

    let err = run(&tk, &job, &work, &NullReporter, &cancel).unwrap_err();

    assert!(err.is_cancelled(), "{err}");
    assert!(!job.output.exists());
    assert!(is_empty(&work));
}

#[test]
fn unknown_model_is_reported_before_any_work() {
    let tmp = tempfile::tempdir().unwrap();
    let mut job = JobSpec {
        input: tmp.path().join("missing.png"),
        output: tmp.path().join("out.png"),
        kind: MediaKind::Image,
        settings: settings(4.0, "png"),
    };
    job.settings.model = "nope".into();
    let engine = FakeEngine::default();
    let tk = Toolkit { engine: &engine, images: &FakeImages, video: None };

    let err = run(&tk, &job, &tmp.path().join("work"), &NullReporter, &CancelToken::new()).unwrap_err();

    assert!(matches!(err, Error::ModelNotFound { .. }), "{err}");
    assert!(engine.calls.lock().unwrap().is_empty());
}

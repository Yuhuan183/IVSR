//! Round-trip tests against the real ffmpeg toolchain; skipped when absent.

use std::fs;
use std::path::Path;
use std::process::Command;

use ivsr_core::{AudioMode, CancelToken, VideoEncodeOptions};

use super::*;

fn toolchain() -> Option<Ffmpeg> {
    let ffmpeg = Ffmpeg::new(FfmpegConfig::default());
    if ffmpeg.status().is_ready() {
        Some(ffmpeg)
    } else {
        eprintln!("ffmpeg not available, skipping");
        None
    }
}

/// 1 s, 10 fps, 64x48 test pattern with a sine audio track.
fn sample_video(path: &Path) {
    let status = Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=64x48:rate=10", "-f", "lavfi", "-i"])
        .args(["sine=frequency=440:sample_rate=44100", "-t", "1", "-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .args(["-c:a", "aac", "-shortest"])
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn probe_extract_and_reencode_round_trip() {
    let Some(ffmpeg) = toolchain() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("in.mp4");
    sample_video(&input);

    let info = ffmpeg.probe(&input).unwrap();
    assert_eq!((info.width, info.height, info.fps_num, info.fps_den), (64, 48, 10, 1));
    assert_eq!(info.audio.as_ref().map(|a| a.codec.as_str()), Some("aac"));

    let frames_dir = tmp.path().join("frames");
    fs::create_dir_all(&frames_dir).unwrap();
    let progress = |_: f64| {};
    let log = |_: LogLevel, _: &str| {};
    let ctx = TaskContext { cancel: &CancelToken::new(), progress: &progress, log: &log };
    let count = ffmpeg.extract_frames(&input, &info, &frames_dir, &ctx).unwrap();
    let mut frames: Vec<_> = fs::read_dir(&frames_dir).unwrap().map(|e| e.unwrap().path()).collect();
    frames.sort();
    assert_eq!(count, 10);
    assert_eq!(frames.len(), 10);
    assert!(frames[0].ends_with("00000001.png"));

    let output = tmp.path().join("out.mkv");
    let options = VideoEncodeOptions { codec: "h264".into(), quality: Some(23), preset: Some("ultrafast".into()), audio: AudioMode::Auto };
    let mut encoder = ffmpeg
        .open_encoder(&EncoderSetup {
            source: &input,
            source_info: &info,
            output: &output,
            frame_size: (64, 48),
            output_size: (32, 24),
            options: &options,
        })
        .unwrap();
    for frame in &frames {
        encoder.push_frame(frame).unwrap();
    }
    encoder.finish().unwrap();

    let out = ffmpeg.probe(&output).unwrap();
    assert_eq!((out.width, out.height), (32, 24));
    assert_eq!(out.audio.map(|a| a.codec), Some("aac".into()), "aac is copied into mkv");
    let duration = out.duration.unwrap();
    assert!((duration - 1.0).abs() < 0.15, "duration {duration}");
}

#[test]
fn encoder_failure_surfaces_ffmpeg_message() {
    let Some(ffmpeg) = toolchain() else { return };
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("in.mp4");
    sample_video(&input);
    let info = ffmpeg.probe(&input).unwrap();
    let garbage = tmp.path().join("garbage.png");
    fs::write(&garbage, b"not a png").unwrap();
    let options = VideoEncodeOptions { codec: "h264".into(), quality: None, preset: None, audio: AudioMode::Drop };
    let mut encoder = ffmpeg
        .open_encoder(&EncoderSetup {
            source: &input,
            source_info: &info,
            output: &tmp.path().join("out.mp4"),
            frame_size: (64, 48),
            output_size: (64, 48),
            options: &options,
        })
        .unwrap();
    let _ = encoder.push_frame(&garbage);
    let err = encoder.finish().unwrap_err();
    assert!(matches!(err, Error::Tool { .. }), "{err}");
}

#[test]
fn h264_into_webm_is_rejected_before_spawning() {
    let Some(ffmpeg) = toolchain() else { return };
    let info = VideoInfo {
        width: 2,
        height: 2,
        fps_num: 1,
        fps_den: 1,
        frame_count: None,
        duration: None,
        codec: "h264".into(),
        container: "mp4".into(),
        audio: None,
    };
    let options = VideoEncodeOptions { codec: "h264".into(), quality: None, preset: None, audio: AudioMode::Auto };
    let result = ffmpeg.open_encoder(&EncoderSetup {
        source: Path::new("in.mp4"),
        source_info: &info,
        output: Path::new("out.webm"),
        frame_size: (2, 2),
        output_size: (2, 2),
        options: &options,
    });
    assert!(matches!(result, Err(Error::UnsupportedFormat(_))));
}

/// ffmpeg before 5.1 (Ubuntu 22.04 ships 4.4) has no `-fps_mode`; frame
/// extraction must fall back to `-vsync`. Simulated by a wrapper that
/// rejects `-fps_mode` the way those versions do, and hands `-vsync` to the
/// real ffmpeg as `-fps_mode` (ffmpeg 8+ removed `-vsync`).
#[cfg(unix)]
#[test]
fn extraction_works_with_ffmpeg_older_than_5_1() {
    use std::os::unix::fs::PermissionsExt;
    let Some(real) = toolchain() else { return };
    let real_ffmpeg = real.ffmpeg().unwrap();
    let tmp = tempfile::tempdir().unwrap();
    let wrapper = tmp.path().join("ffmpeg");
    let script = format!(
        r#"#!/bin/sh
for a in "$@"; do
  [ "$a" = -fps_mode ] && {{ echo "Unrecognized option 'fps_mode'." >&2; echo 'Error splitting the argument list: Option not found' >&2; exit 1; }}
done
n=$#
for a in "$@"; do
  if [ "$a" = -vsync ]; then set -- "$@" -fps_mode; else set -- "$@" "$a"; fi
done
shift $n
exec '{}' "$@"
"#,
        real_ffmpeg.display()
    );
    fs::write(&wrapper, script).unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let old = Ffmpeg::new(FfmpegConfig { ffmpeg: Some(wrapper), ffprobe: None });

    let input = tmp.path().join("in.mp4");
    sample_video(&input);
    let info = old.probe(&input).unwrap();
    let frames_dir = tmp.path().join("frames");
    fs::create_dir_all(&frames_dir).unwrap();
    let ctx = TaskContext { cancel: &CancelToken::new(), progress: &|_| {}, log: &|_, _| {} };

    let count = old.extract_frames(&input, &info, &frames_dir, &ctx).unwrap();

    assert_eq!(count, 10);
    assert_eq!(fs::read_dir(&frames_dir).unwrap().count(), 10);
}

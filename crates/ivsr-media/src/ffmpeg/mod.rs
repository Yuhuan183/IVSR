//! `VideoIo` implementation driving the `ffmpeg` and `ffprobe` executables.

mod codecs;
mod encoder;
mod probe;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ivsr_core::media::EncoderSetup;
use ivsr_core::process;
use ivsr_core::{
    CodecInfo, Error, FormatInfo, FrameEncoder, LogLevel, MediaKind, Result, TaskContext, ToolStatus, VideoInfo,
    VideoIo, fsutil,
};

use encoder::PipeEncoder;

const FFMPEG: &str = if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" };
const FFPROBE: &str = if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" };

#[derive(Debug, Clone, Default)]
pub struct FfmpegConfig {
    pub ffmpeg: Option<PathBuf>,
    pub ffprobe: Option<PathBuf>,
}

pub struct Ffmpeg {
    config: FfmpegConfig,
    /// Probed once: (version line, encoder names).
    toolchain: OnceLock<std::result::Result<(String, HashSet<String>), String>>,
}

impl Ffmpeg {
    pub fn new(config: FfmpegConfig) -> Self {
        Self { config, toolchain: OnceLock::new() }
    }

    fn binary(configured: &Option<PathBuf>, name: &str) -> std::result::Result<PathBuf, String> {
        match configured {
            Some(p) if p.is_file() => Ok(p.clone()),
            Some(p) => Err(format!("configured {name} {} does not exist", p.display())),
            None => which::which(name).map_err(|_| format!("{name} not found on PATH; install ffmpeg or set tools.{}", name.trim_end_matches(".exe"))),
        }
    }

    fn ffmpeg(&self) -> Result<PathBuf> {
        Self::binary(&self.config.ffmpeg, FFMPEG).map_err(|e| Error::tool("ffmpeg", e))
    }

    fn ffprobe(&self) -> Result<PathBuf> {
        Self::binary(&self.config.ffprobe, FFPROBE).map_err(|e| Error::tool("ffprobe", e))
    }

    fn toolchain(&self) -> &std::result::Result<(String, HashSet<String>), String> {
        self.toolchain.get_or_init(|| {
            let ffmpeg = self.ffmpeg().map_err(|e| e.to_string())?;
            self.ffprobe().map_err(|e| e.to_string())?;
            let version = process::output(process::command(&ffmpeg).arg("-version"), "ffmpeg")
                .map_err(|e| e.to_string())?
                .lines()
                .next()
                .unwrap_or_default()
                .to_string();
            let listing = process::output(process::command(&ffmpeg).args(["-hide_banner", "-encoders"]), "ffmpeg")
                .map_err(|e| e.to_string())?;
            // Lines look like ` V....D libx264   libx264 H.264 ...`.
            let encoders = listing
                .lines()
                .filter_map(|l| {
                    let mut cols = l.split_whitespace();
                    let flags = cols.next()?;
                    (flags.len() == 6 && flags.starts_with('V')).then(|| cols.next()).flatten()
                })
                .map(str::to_string)
                .collect();
            Ok((version, encoders))
        })
    }
}

impl Ffmpeg {
    /// Writes every frame of `input` as `dir/%08d.png`, at the source's
    /// constant frame rate via `sync_option` (`-fps_mode` or `-vsync`).
    fn extract_with(&self, input: &Path, info: &VideoInfo, dir: &Path, ctx: &TaskContext<'_>, sync_option: &str) -> Result<u64> {
        let ffmpeg = self.ffmpeg()?;
        let total = info.estimated_frames().max(1) as f64;
        let mut cmd = process::command(&ffmpeg);
        cmd.args(["-hide_banner", "-nostdin", "-v", "error", "-progress", "pipe:1", "-i"]).arg(input);
        // Constant frame rate keeps the frame count consistent with the audio track.
        cmd.args(["-map", "0:v:0", sync_option, "cfr", "-r"]).arg(format!("{}/{}", info.fps_num, info.fps_den));
        cmd.args(["-pix_fmt", "rgb24", "-compression_level", "1", "-start_number", "1", "-f", "image2"]);
        cmd.arg(dir.join("%08d.png"));
        let mut frames = 0u64;
        process::run(
            &mut cmd,
            "ffmpeg",
            ctx.cancel,
            |line| match line.strip_prefix("frame=") {
                Some(n) => {
                    if let Ok(n) = n.trim().parse::<u64>() {
                        frames = n;
                        (ctx.progress)(n as f64 / total);
                    }
                }
                None if !line.contains('=') => (ctx.log)(LogLevel::Debug, line),
                None => {}
            },
            || {},
        )?
        .into_result("ffmpeg")?;
        (ctx.progress)(1.0);
        Ok(frames)
    }
}

impl VideoIo for Ffmpeg {
    fn status(&self) -> ToolStatus {
        match self.toolchain() {
            Ok((version, _)) => ToolStatus::Ready {
                location: self.ffmpeg().unwrap_or_default(),
                version: version.split_whitespace().nth(2).map(str::to_string),
            },
            Err(hint) => ToolStatus::Missing { hint: hint.clone() },
        }
    }

    fn formats(&self) -> Vec<FormatInfo> {
        codecs::CONTAINERS
            .iter()
            .map(|(id, label, exts, encode)| FormatInfo {
                id: id.to_string(),
                label: label.to_string(),
                kind: MediaKind::Video,
                extensions: exts.iter().map(|e| e.to_string()).collect(),
                decode: true,
                encode: *encode,
                lossy: *encode,
                note: None,
            })
            .collect()
    }

    fn codecs(&self) -> Vec<CodecInfo> {
        let empty = HashSet::new();
        let encoders = self.toolchain().as_ref().map(|(_, e)| e).unwrap_or(&empty);
        codecs::catalogue(encoders)
    }

    fn probe(&self, path: &Path) -> Result<VideoInfo> {
        let ffprobe = self.ffprobe()?;
        let mut cmd = process::command(&ffprobe);
        cmd.args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"]).arg(path);
        let json = process::output(&mut cmd, "ffprobe")?;
        probe::parse(&json)
    }

    fn extract_frames(&self, input: &Path, info: &VideoInfo, dir: &Path, ctx: &TaskContext<'_>) -> Result<u64> {
        // `-fps_mode` arrived in ffmpeg 5.1; older builds (Ubuntu 22.04 ships
        // 4.4) only know `-vsync`, which newer ones deprecate. They reject the
        // option before reading any input, so retrying writes nothing twice.
        match self.extract_with(input, info, dir, ctx, "-fps_mode") {
            Err(Error::Tool { message, .. }) if message.contains("Unrecognized option 'fps_mode'") => {
                (ctx.log)(LogLevel::Debug, "ffmpeg predates -fps_mode; using -vsync");
                self.extract_with(input, info, dir, ctx, "-vsync")
            }
            other => other,
        }
    }

    fn open_encoder(&self, setup: &EncoderSetup<'_>) -> Result<Box<dyn FrameEncoder>> {
        let ffmpeg = self.ffmpeg()?;
        let ext = fsutil::extension(setup.output).unwrap_or_default();
        let container = codecs::container_of(&ext)
            .filter(|c| codecs::CONTAINERS.iter().any(|(id, .., enc)| id == c && *enc))
            .ok_or_else(|| Error::UnsupportedFormat(format!("cannot write .{ext} video")))?;
        let video_args = codecs::video_args(setup.options, container)?;
        let audio_args = codecs::audio_args(setup.options.audio, setup.source_info.audio.as_ref(), container);
        let info = setup.source_info;

        let mut args: Vec<String> = ["-hide_banner", "-v", "error", "-y", "-f", "image2pipe", "-c:v", "png"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        args.extend(["-framerate".into(), format!("{}/{}", info.fps_num, info.fps_den), "-i".into(), "-".into()]);
        if audio_args.is_some() {
            args.extend(["-i".into(), setup.source.to_string_lossy().into_owned()]);
        }
        args.extend(["-map".into(), "0:v:0".into()]);
        if audio_args.is_some() {
            args.extend(["-map".into(), "1:a:0?".into()]);
        }
        if setup.output_size != setup.frame_size {
            let (w, h) = setup.output_size;
            args.extend(["-vf".into(), format!("scale={w}:{h}:flags=lanczos")]);
        }
        args.extend(video_args);
        match audio_args {
            Some(a) => args.extend(a),
            None => args.push("-an".into()),
        }
        args.push(setup.output.to_string_lossy().into_owned());
        Ok(Box::new(PipeEncoder::spawn(&ffmpeg, &args)?))
    }
}

#[cfg(test)]
mod tests;

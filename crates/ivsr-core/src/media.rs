//! Media input/output contracts.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Result, TaskContext, Text, ToolStatus};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Image,
    Video,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FormatInfo {
    /// Canonical id, also the default output extension (`png`, `mp4`).
    pub id: String,
    pub label: String,
    pub kind: MediaKind,
    /// Lower-case extensions without the dot.
    pub extensions: Vec<String>,
    pub decode: bool,
    pub encode: bool,
    /// Whether `ImageEncodeOptions::quality` has an effect on encoding.
    #[serde(default)]
    pub lossy: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<Text>,
}

impl FormatInfo {
    pub fn matches_extension(&self, ext: &str) -> bool {
        self.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    /// Format id as detected from content.
    pub format: String,
    pub has_alpha: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageEncodeOptions {
    /// Output format id.
    pub format: String,
    /// 1..=100, used by lossy formats.
    pub quality: Option<u8>,
    /// Resample to exactly these dimensions before encoding.
    pub resize: Option<(u32, u32)>,
}

/// Still-image decoding, resampling and encoding.
pub trait ImageIo: Send + Sync {
    fn formats(&self) -> Vec<FormatInfo>;
    fn probe(&self, path: &Path) -> Result<ImageInfo>;
    /// Decodes `src`, applies `opts.resize`, and encodes to `dst` as `opts.format`.
    fn convert(&self, src: &Path, dst: &Path, opts: &ImageEncodeOptions) -> Result<()>;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioInfo {
    pub codec: String,
    pub channels: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoInfo {
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    pub frame_count: Option<u64>,
    pub duration: Option<f64>,
    pub codec: String,
    pub container: String,
    pub audio: Option<AudioInfo>,
}

impl VideoInfo {
    pub fn fps(&self) -> f64 {
        if self.fps_den == 0 { 0.0 } else { self.fps_num as f64 / self.fps_den as f64 }
    }

    /// Frame count, estimated from duration when the container does not say.
    pub fn estimated_frames(&self) -> u64 {
        self.frame_count
            .or_else(|| self.duration.map(|d| (d * self.fps()).round() as u64))
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioMode {
    /// Copy when the output container accepts the source codec, else re-encode.
    #[default]
    Auto,
    Copy,
    Reencode,
    Drop,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoEncodeOptions {
    /// Codec id from `VideoIo::codecs`.
    pub codec: String,
    /// Codec quality value (CRF/CQ); lower is better.
    pub quality: Option<u32>,
    pub preset: Option<String>,
    pub audio: AudioMode,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QualityRange {
    pub label: String,
    pub min: u32,
    pub max: u32,
    pub default: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodecInfo {
    pub id: String,
    pub label: String,
    /// Container format ids this codec can be muxed into.
    pub containers: Vec<String>,
    pub quality: Option<QualityRange>,
    pub presets: Vec<String>,
    pub default_preset: Option<String>,
    pub hardware: bool,
    /// Whether the encoder exists in the installed toolchain.
    pub available: bool,
}

/// Receives upscaled frames in display order and produces the output video.
pub trait FrameEncoder: Send {
    fn push_frame(&mut self, png: &Path) -> Result<()>;
    /// Flushes and waits for the encoder. Dropping without `finish` aborts.
    fn finish(self: Box<Self>) -> Result<()>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct EncoderSetup<'a> {
    /// Original file, source of the audio track.
    pub source: &'a Path,
    pub source_info: &'a VideoInfo,
    pub output: &'a Path,
    /// Size of frames passed to `push_frame`.
    pub frame_size: (u32, u32),
    /// Size of the encoded video; the encoder resamples when it differs.
    pub output_size: (u32, u32),
    pub options: &'a VideoEncodeOptions,
}

/// Video demuxing, frame extraction and encoding.
pub trait VideoIo: Send + Sync {
    fn status(&self) -> ToolStatus;
    fn formats(&self) -> Vec<FormatInfo>;
    fn codecs(&self) -> Vec<CodecInfo>;
    fn probe(&self, path: &Path) -> Result<VideoInfo>;
    /// Writes every frame of `input` as `dir/%08d.png` at constant frame rate and
    /// returns the number of frames written.
    fn extract_frames(&self, input: &Path, info: &VideoInfo, dir: &Path, ctx: &TaskContext<'_>) -> Result<u64>;
    fn open_encoder(&self, setup: &EncoderSetup<'_>) -> Result<Box<dyn FrameEncoder>>;
}

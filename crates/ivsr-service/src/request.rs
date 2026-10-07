//! A frontend's description of what to upscale, before defaults are applied.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use ivsr_core::scale::{format_scale, plan_scale};
use ivsr_core::{AudioMode, CodecInfo, Engine, ParamValue, ParamValues, UpscaleSettings, VideoEncodeOptions};
use serde::{Deserialize, Serialize};

use crate::config::{Config, ConflictPolicy};
use crate::registry::Registry;
use crate::{Error, Result};

/// Every field is optional; unset fields fall back to the configuration.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct JobRequest {
    pub engine: Option<String>,
    pub model: Option<String>,
    pub scale: Option<f64>,
    /// Engine parameter overrides, merged over configured defaults.
    pub params: BTreeMap<String, ParamValue>,
    pub image_format: Option<String>,
    pub image_quality: Option<u8>,
    pub video_codec: Option<String>,
    pub video_quality: Option<u32>,
    pub video_preset: Option<String>,
    pub audio: Option<AudioMode>,
    pub container: Option<String>,
    pub batch_frames: Option<u32>,
    pub output: Option<PathBuf>,
    pub suffix: Option<String>,
    pub conflict: Option<ConflictPolicy>,
    pub recursive: bool,
}

/// A request with defaults applied and every value validated.
pub struct Resolved {
    pub engine: Arc<dyn Engine>,
    /// Settings shared by all jobs; `image_format` is filled in per job.
    pub settings: UpscaleSettings,
    pub image_format: String,
    pub container: String,
    pub codec: Option<CodecInfo>,
    pub output: Option<PathBuf>,
    pub suffix: String,
    pub conflict: ConflictPolicy,
    pub recursive: bool,
}

pub(crate) fn resolve(req: &JobRequest, config: &Config, registry: &Registry) -> Result<Resolved> {
    let engine_id = req.engine.clone().unwrap_or_else(|| config.engine.clone());
    let engine = registry.engine(&engine_id)?;
    if let Some(problem) = engine.status().problem() {
        return Err(ivsr_core::Error::EngineUnavailable { engine: engine_id, reason: problem.into() }.into());
    }
    let engine_cfg = config.engine_config(&engine_id);
    let models = engine.models();
    let model_id = req.model.clone().or(engine_cfg.model.clone()).or_else(|| engine.default_model()).unwrap_or_default();
    let model = models.iter().find(|m| m.id == model_id).ok_or_else(|| {
        let known: Vec<_> = models.iter().map(|m| m.id.as_str()).collect();
        Error::Input(format!("unknown model `{model_id}` for {engine_id} (available: {})", known.join(", ")))
    })?;

    let mut params = engine_cfg.params.clone();
    params.extend(req.params.clone());
    let params = ParamValues::resolve(&engine.params(), &params)?;

    let scale = req.scale.unwrap_or(config.output.scale);
    plan_scale(&model.scales, scale)?;

    let image_format = req.image_format.clone().unwrap_or_else(|| config.output.image_format.clone()).to_ascii_lowercase();
    if image_format != "same" {
        registry
            .images()
            .formats()
            .iter()
            .find(|f| f.encode && (f.id == image_format || f.matches_extension(&image_format)))
            .ok_or_else(|| Error::Input(format!("cannot write images as `{image_format}`")))?;
    }
    let image_quality = req.image_quality.unwrap_or(config.output.image_quality);
    if !(1..=100).contains(&image_quality) {
        return Err(Error::Input(format!("image quality must be 1-100, got {image_quality}")));
    }

    let video = VideoEncodeOptions {
        codec: req.video_codec.clone().unwrap_or_else(|| config.video.codec.clone()),
        quality: req.video_quality.or(config.video.quality),
        preset: req.video_preset.clone().or_else(|| config.video.preset.clone()),
        audio: req.audio.unwrap_or(config.video.audio),
    };
    let codec = registry.codecs().into_iter().find(|c| c.id == video.codec);

    let suffix_template = req.suffix.clone().unwrap_or_else(|| config.output.suffix.clone());
    let suffix = suffix_template
        .replace("{scale}", &format_scale(scale))
        .replace("{model}", &model.id)
        .replace("{engine}", &engine_id);
    if suffix.contains(['/', '\\']) {
        return Err(Error::Input(format!("suffix must not contain path separators: `{suffix}`")));
    }

    Ok(Resolved {
        settings: UpscaleSettings {
            model: model.id.clone(),
            scale,
            params,
            image_format: String::new(),
            image_quality: Some(image_quality),
            video,
            batch_frames: req.batch_frames.unwrap_or(config.video.batch_frames).max(1),
        },
        engine,
        image_format,
        container: req.container.clone().unwrap_or_else(|| config.video.container.clone()).to_ascii_lowercase(),
        codec,
        output: req.output.clone().or_else(|| config.output.directory.clone()),
        suffix,
        conflict: req.conflict.unwrap_or(config.output.conflict),
        recursive: req.recursive,
    })
}

/// Checks video options against the codec catalogue before any work starts.
pub(crate) fn validate_video(video: &VideoEncodeOptions, codec: Option<&CodecInfo>) -> Result<()> {
    let codec = codec.ok_or_else(|| Error::Input(format!("unknown video codec `{}`", video.codec)))?;
    if !codec.available {
        return Err(Error::Input(format!("{} encoder is not available in the installed ffmpeg", codec.label)));
    }
    if let (Some(q), Some(range)) = (video.quality, &codec.quality) {
        if !(range.min..=range.max).contains(&q) {
            return Err(Error::Input(format!("{} {} must be {}-{}, got {q}", codec.label, range.label, range.min, range.max)));
        }
    }
    if let Some(preset) = &video.preset {
        if !codec.presets.contains(preset) {
            return Err(Error::Input(format!(
                "{} has no preset `{preset}`{}",
                codec.label,
                if codec.presets.is_empty() { String::new() } else { format!(" (use one of: {})", codec.presets.join(", ")) }
            )));
        }
    }
    Ok(())
}

//! The set of concrete implementations available to this build.

use std::path::Path;
use std::sync::Arc;

use ivsr_core::{CodecInfo, Engine, FormatInfo, ImageIo, MediaKind, VideoIo};
use ivsr_engine_realesrgan::{RealEsrgan, RealEsrganConfig};
use ivsr_media::{Ffmpeg, FfmpegConfig, RasterIo};

use crate::config::Config;
use crate::paths::AppPaths;
use crate::{Error, Result};

pub struct Registry {
    engines: Vec<Arc<dyn Engine>>,
    images: Arc<dyn ImageIo>,
    video: Arc<dyn VideoIo>,
}

/// Every engine compiled into this build. Adding an engine means adding a line here.
fn builtin_engines(config: &Config, paths: &AppPaths) -> Vec<Arc<dyn Engine>> {
    let id = ivsr_engine_realesrgan::ENGINE_ID;
    let cfg = config.engine_config(id);
    vec![Arc::new(RealEsrgan::new(RealEsrganConfig {
        binary: cfg.path,
        models_dir: cfg.models_dir,
        install_dir: Some(paths.engine_dir(id)),
        store_dir: Some(paths.model_store(id)),
    }))]
}

impl Registry {
    pub fn from_config(config: &Config, paths: &AppPaths) -> Self {
        let ffmpeg = FfmpegConfig { ffmpeg: config.tools.ffmpeg.clone(), ffprobe: config.tools.ffprobe.clone() };
        Self::new(builtin_engines(config, paths), Arc::new(RasterIo::new()), Arc::new(Ffmpeg::new(ffmpeg)))
    }

    pub fn new(engines: Vec<Arc<dyn Engine>>, images: Arc<dyn ImageIo>, video: Arc<dyn VideoIo>) -> Self {
        Self { engines, images, video }
    }

    pub fn engines(&self) -> &[Arc<dyn Engine>] {
        &self.engines
    }

    pub fn engine(&self, id: &str) -> Result<Arc<dyn Engine>> {
        self.engines.iter().find(|e| e.info().id == id).cloned().ok_or_else(|| Error::UnknownEngine(id.into()))
    }

    pub fn images(&self) -> &Arc<dyn ImageIo> {
        &self.images
    }

    pub fn video(&self) -> &Arc<dyn VideoIo> {
        &self.video
    }

    pub fn formats(&self) -> Vec<FormatInfo> {
        let mut formats = self.images.formats();
        formats.extend(self.video.formats());
        formats
    }

    pub fn codecs(&self) -> Vec<CodecInfo> {
        self.video.codecs()
    }

    /// Classifies a path by extension; image formats win ties (e.g. `.gif`).
    pub fn classify(&self, path: &Path) -> Option<(MediaKind, FormatInfo)> {
        let ext = ivsr_core::fsutil::extension(path)?;
        self.formats().into_iter().find(|f| f.decode && f.matches_extension(&ext)).map(|f| (f.kind, f))
    }
}

//! Cached thumbnails so frontends never decode full-size images.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use ivsr_core::{ImageEncodeOptions, ImageIo};

use crate::Result;

pub(crate) fn thumbnail(images: &dyn ImageIo, src: &Path, dir: &Path, max_side: u32) -> Result<PathBuf> {
    let meta = std::fs::metadata(src).map_err(|e| ivsr_core::Error::io_at("read", src, e))?;
    let mut hasher = DefaultHasher::new();
    src.hash(&mut hasher);
    meta.len().hash(&mut hasher);
    meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).hash(&mut hasher);
    max_side.hash(&mut hasher);
    let target = dir.join(format!("{:016x}.jpg", hasher.finish()));
    if target.is_file() {
        return Ok(target);
    }
    std::fs::create_dir_all(dir).map_err(|e| ivsr_core::Error::io_at("create", dir, e))?;
    let info = images.probe(src)?;
    let fit = (max_side as f64 / info.width.max(info.height) as f64).min(1.0);
    let size = (((info.width as f64 * fit).round() as u32).max(1), ((info.height as f64 * fit).round() as u32).max(1));
    // Write beside the target and rename, so concurrent requests never see a partial file.
    let staging = target.with_extension(format!("{}.part.jpg", std::process::id()));
    images.convert(src, &staging, &ImageEncodeOptions { format: "jpg".into(), quality: Some(80), resize: Some(size) })?;
    std::fs::rename(&staging, &target).map_err(|e| ivsr_core::Error::io_at("rename", &target, e))?;
    Ok(target)
}

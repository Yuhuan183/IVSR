//! Deterministic synthetic images for benchmarks and model verification.

use std::path::Path;

use image::{Rgb, RgbImage};
use ivsr_core::{Error, Result};

/// Writes a `width`×`height` PNG with gradients, edges and noise. Network cost
/// does not depend on content, but realistic structure keeps codecs honest.
pub fn write_png(path: &Path, width: u32, height: u32) -> Result<()> {
    let mut seed: u32 = 0x9e37_79b9;
    let mut noise = move || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        (seed % 48) as u8
    };
    let img = RgbImage::from_fn(width, height, |x, y| {
        let gx = (x * 255 / width.max(1)) as u8;
        let gy = (y * 255 / height.max(1)) as u8;
        let edge = if ((x / 16) + (y / 16)) % 2 == 0 { 40 } else { 0 };
        Rgb([gx ^ noise(), gy.saturating_add(edge), (gx / 2 + gy / 2) ^ noise()])
    });
    img.save(path).map_err(|e| Error::tool("image encoder", format!("{}: {e}", path.display())))
}

//! `ImageIo` implementation on top of the `image` crate, with SIMD resampling
//! from `fast_image_resize`.

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, PngEncoder};
use image::codecs::webp::WebPEncoder;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use ivsr_core::{Error, FormatInfo, Frame, ImageEncodeOptions, ImageInfo, ImageIo, MediaKind, Result};

/// One supported raster format.
struct Format {
    id: &'static str,
    label: &'static str,
    format: ImageFormat,
    extensions: &'static [&'static str],
    encode: bool,
    lossy: bool,
    /// (English, Traditional Chinese)
    note: Option<(&'static str, &'static str)>,
}

const FORMATS: &[Format] = &[
    Format { id: "png", label: "PNG", format: ImageFormat::Png, extensions: &["png"], encode: true, lossy: false, note: None },
    Format { id: "jpg", label: "JPEG", format: ImageFormat::Jpeg, extensions: &["jpg", "jpeg", "jpe", "jfif"], encode: true, lossy: true, note: Some(("alpha is discarded", "會捨棄透明度")) },
    Format { id: "webp", label: "WebP", format: ImageFormat::WebP, extensions: &["webp"], encode: true, lossy: false, note: Some(("encoding is lossless only", "僅支援無損編碼")) },
    Format { id: "bmp", label: "BMP", format: ImageFormat::Bmp, extensions: &["bmp"], encode: true, lossy: false, note: None },
    Format { id: "tiff", label: "TIFF", format: ImageFormat::Tiff, extensions: &["tif", "tiff"], encode: true, lossy: false, note: None },
    Format { id: "tga", label: "TGA", format: ImageFormat::Tga, extensions: &["tga"], encode: true, lossy: false, note: None },
    Format { id: "qoi", label: "QOI", format: ImageFormat::Qoi, extensions: &["qoi"], encode: true, lossy: false, note: None },
    Format { id: "pnm", label: "PNM", format: ImageFormat::Pnm, extensions: &["ppm", "pgm", "pbm", "pam", "pnm"], encode: true, lossy: false, note: None },
    Format { id: "gif", label: "GIF", format: ImageFormat::Gif, extensions: &["gif"], encode: false, lossy: false, note: Some(("first frame only", "僅讀取第一幀")) },
    Format { id: "ico", label: "ICO", format: ImageFormat::Ico, extensions: &["ico"], encode: false, lossy: false, note: None },
];

const DEFAULT_JPEG_QUALITY: u8 = 92;

fn by_id(id: &str) -> Option<&'static Format> {
    FORMATS.iter().find(|f| f.id == id || f.extensions.contains(&id))
}

fn by_image_format(format: ImageFormat) -> Option<&'static Format> {
    FORMATS.iter().find(|f| f.format == format)
}

fn decode_error(path: &Path, e: impl std::fmt::Display) -> Error {
    Error::UnsupportedFormat(format!("{}: {e}", path.display()))
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RasterIo;

impl RasterIo {
    pub fn new() -> Self {
        Self
    }

    fn open(path: &Path) -> Result<ImageReader<BufReader<File>>> {
        ImageReader::open(path)
            .map_err(|e| Error::io_at("open", path, e))?
            .with_guessed_format()
            .map_err(|e| Error::io_at("read", path, e))
    }

    fn read(path: &Path) -> Result<DynamicImage> {
        let mut reader = Self::open(path)?;
        reader.no_limits();
        reader.decode().map_err(|e| decode_error(path, e))
    }
}

impl ImageIo for RasterIo {
    fn formats(&self) -> Vec<FormatInfo> {
        FORMATS
            .iter()
            .map(|f| FormatInfo {
                id: f.id.into(),
                label: f.label.into(),
                kind: MediaKind::Image,
                extensions: f.extensions.iter().map(|e| e.to_string()).collect(),
                decode: true,
                encode: f.encode,
                lossy: f.lossy,
                note: f.note.map(|(en, zh)| ivsr_core::Text::en(en).with("zh-TW", zh)),
            })
            .collect()
    }

    fn probe(&self, path: &Path) -> Result<ImageInfo> {
        let reader = Self::open(path)?;
        let format = reader
            .format()
            .and_then(by_image_format)
            .ok_or_else(|| decode_error(path, "unrecognised image format"))?;
        let decoder = reader.into_decoder().map_err(|e| decode_error(path, e))?;
        let (width, height) = decoder.dimensions();
        Ok(ImageInfo { width, height, format: format.id.into(), has_alpha: decoder.color_type().has_alpha() })
    }

    fn convert(&self, src: &Path, dst: &Path, opts: &ImageEncodeOptions) -> Result<()> {
        let format = by_id(&opts.format)
            .filter(|f| f.encode)
            .ok_or_else(|| Error::UnsupportedFormat(format!("cannot encode `{}`", opts.format)))?;
        let mut image = Self::read(src)?;
        if let Some((w, h)) = opts.resize.filter(|&(w, h)| (w, h) != (image.width(), image.height())) {
            image = resize(&image, w, h)?;
        }
        encode(&image, dst, format, opts.quality)
    }

    fn decode(&self, path: &Path, size: Option<(u32, u32)>) -> Result<Frame> {
        let mut image = Self::read(path)?;
        let has_alpha = image.color().has_alpha();
        // Resample before narrowing to 8 bits, so 16-bit sources keep their precision.
        if let Some((w, h)) = size.filter(|&(w, h)| (w, h) != (image.width(), image.height())) {
            image = resize(&image, w, h)?;
        }
        let rgba = image.into_rgba8();
        Ok(Frame { width: rgba.width(), height: rgba.height(), pixels: rgba.into_raw(), has_alpha })
    }

    fn encode(&self, frame: &Frame, dst: &Path, opts: &ImageEncodeOptions) -> Result<()> {
        let format = by_id(&opts.format)
            .filter(|f| f.encode)
            .ok_or_else(|| Error::UnsupportedFormat(format!("cannot encode `{}`", opts.format)))?;
        let rgba = image::RgbaImage::from_raw(frame.width, frame.height, frame.pixels.clone())
            .ok_or_else(|| Error::Invalid(format!("frame of {}x{} has {} bytes", frame.width, frame.height, frame.pixels.len())))?;
        let mut image = match frame.has_alpha {
            true => DynamicImage::ImageRgba8(rgba),
            false => DynamicImage::ImageRgb8(DynamicImage::ImageRgba8(rgba).to_rgb8()),
        };
        if let Some((w, h)) = opts.resize.filter(|&(w, h)| (w, h) != (image.width(), image.height())) {
            image = resize(&image, w, h)?;
        }
        encode(&image, dst, format, opts.quality)
    }
}

/// Lanczos3 resampling with alpha premultiplication.
fn resize(src: &DynamicImage, width: u32, height: u32) -> Result<DynamicImage> {
    // fast_image_resize handles 8/16-bit and f32 layouts; normalise anything else.
    let src = match src {
        DynamicImage::ImageLuma8(_)
        | DynamicImage::ImageLumaA8(_)
        | DynamicImage::ImageRgb8(_)
        | DynamicImage::ImageRgba8(_)
        | DynamicImage::ImageLuma16(_)
        | DynamicImage::ImageLumaA16(_)
        | DynamicImage::ImageRgb16(_)
        | DynamicImage::ImageRgba16(_) => std::borrow::Cow::Borrowed(src),
        other => std::borrow::Cow::Owned(DynamicImage::ImageRgba8(other.to_rgba8())),
    };
    let mut dst = DynamicImage::new(width, height, src.color());
    let options = ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3));
    Resizer::new()
        .resize(src.as_ref(), &mut dst, &options)
        .map_err(|e| Error::Invalid(format!("resize failed: {e}")))?;
    Ok(dst)
}

fn encode(image: &DynamicImage, dst: &Path, format: &Format, quality: Option<u8>) -> Result<()> {
    let file = File::create(dst).map_err(|e| Error::io_at("create", dst, e))?;
    let mut out = BufWriter::new(file);
    let failed = |e: image::ImageError| Error::tool("image encoder", format!("{}: {e}", dst.display()));
    match format.format {
        ImageFormat::Png => {
            let encoder = PngEncoder::new_with_quality(&mut out, CompressionType::Fast, Default::default());
            image.write_with_encoder(encoder).map_err(failed)?;
        }
        ImageFormat::Jpeg => {
            let q = quality.unwrap_or(DEFAULT_JPEG_QUALITY).clamp(1, 100);
            let rgb = DynamicImage::ImageRgb8(image.to_rgb8());
            rgb.write_with_encoder(JpegEncoder::new_with_quality(&mut out, q)).map_err(failed)?;
        }
        ImageFormat::WebP => {
            let rgba = match image.color().has_alpha() {
                true => DynamicImage::ImageRgba8(image.to_rgba8()),
                false => DynamicImage::ImageRgb8(image.to_rgb8()),
            };
            rgba.write_with_encoder(WebPEncoder::new_lossless(&mut out)).map_err(failed)?;
        }
        other => image.write_to(&mut out, other).map_err(failed)?,
    }
    std::io::Write::flush(&mut out).map_err(|e| Error::io_at("write", dst, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn sample(path: &Path, w: u32, h: u32) {
        let img = RgbaImage::from_fn(w, h, |x, y| Rgba([(x * 40) as u8, (y * 40) as u8, 128, 200]));
        img.save(path).unwrap();
    }

    #[test]
    fn probe_detects_format_from_content_not_extension() {
        let dir = tempfile::tempdir().unwrap();
        let png = dir.path().join("a.png");
        sample(&png, 6, 4);
        let disguised = dir.path().join("a.jpg");
        std::fs::copy(&png, &disguised).unwrap();
        let info = RasterIo.probe(&disguised).unwrap();
        assert_eq!((info.width, info.height, info.format.as_str(), info.has_alpha), (6, 4, "png", true));
    }

    #[test]
    fn convert_resizes_and_encodes_requested_format() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.png");
        sample(&src, 6, 4);
        let dst = dir.path().join("dst.jpg");
        let opts = ImageEncodeOptions { format: "jpg".into(), quality: Some(80), resize: Some((3, 2)) };
        RasterIo.convert(&src, &dst, &opts).unwrap();
        let info = RasterIo.probe(&dst).unwrap();
        assert_eq!((info.width, info.height, info.format.as_str(), info.has_alpha), (3, 2, "jpg", false));
    }

    #[test]
    fn frames_decode_resampled_and_encode_without_alpha_when_the_source_had_none() {
        let dir = tempfile::tempdir().unwrap();
        let rgba = dir.path().join("rgba.png");
        sample(&rgba, 6, 4);
        let frame = RasterIo.decode(&rgba, Some((12, 8))).unwrap();
        assert_eq!((frame.width, frame.height, frame.pixels.len(), frame.has_alpha), (12, 8, 12 * 8 * 4, true));

        let rgb = dir.path().join("rgb.png");
        image::RgbImage::from_pixel(3, 2, image::Rgb([10, 20, 30])).save(&rgb).unwrap();
        let frame = RasterIo.decode(&rgb, None).unwrap();
        assert!(!frame.has_alpha);
        assert_eq!(&frame.pixels[..4], &[10, 20, 30, 255]);
        let out = dir.path().join("out.png");
        RasterIo.encode(&frame, &out, &ImageEncodeOptions { format: "png".into(), quality: None, resize: None }).unwrap();
        let info = RasterIo.probe(&out).unwrap();
        assert_eq!((info.width, info.height, info.has_alpha), (3, 2, false));
        assert_eq!(RasterIo.decode(&out, None).unwrap(), frame);
    }

    #[test]
    fn decode_only_format_is_rejected_as_output() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.png");
        sample(&src, 2, 2);
        let opts = ImageEncodeOptions { format: "gif".into(), quality: None, resize: None };
        let err = RasterIo.convert(&src, &dir.path().join("x.gif"), &opts).unwrap_err();
        assert!(matches!(err, Error::UnsupportedFormat(_)), "{err}");
    }
}

//! Concrete media I/O backends: still images through `image`, video through
//! the ffmpeg command-line tools.

pub mod ffmpeg;
pub mod image_io;
pub mod testpattern;

pub use ffmpeg::{Ffmpeg, FfmpegConfig};
pub use image_io::RasterIo;

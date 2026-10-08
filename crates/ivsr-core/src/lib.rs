//! Domain contracts shared by every ivsr component.
//!
//! This crate defines the authoritative interfaces (`Engine`, `Filter`,
//! `ImageIo`, `VideoIo`, `Reporter`) and the processing pipeline that composes them.
//! It has no knowledge of concrete tools, configuration files, or frontends.

pub mod cancel;
pub mod engine;
pub mod error;
pub mod filter;
pub mod fsutil;
pub mod i18n;
pub mod media;
pub mod model;
pub mod params;
pub mod pipeline;
pub mod process;
pub mod progress;
pub mod scale;
pub mod status;

pub use cancel::CancelToken;
pub use engine::{ComputeDevice, Distribution, Engine, EngineCaps, EngineInfo, ModelInfo, TaskMode, UpscaleTask};
pub use error::{Error, Result};
pub use filter::{Filter, FilterChain, FilterInfo, FilterRun, FilterSetup, FilterSpec, FilterStage, FilterStep};
pub use i18n::Text;
pub use media::{
    AudioInfo, AudioMode, CodecInfo, EncoderSetup, FormatInfo, Frame, FrameEncoder, ImageEncodeOptions, ImageInfo,
    ImageIo, MediaKind, QualityRange, VideoEncodeOptions, VideoInfo, VideoIo,
};
pub use model::{
    BaselinePoint, Catalog, CostClass, HardwareProfile, ModelFile, ModelManifest, ModelOrigin, Reference, Throughput, TileMemory,
};
pub use params::{ParamKind, ParamSpec, ParamValue, ParamValues};
pub use pipeline::{JobOutcome, JobSpec, Toolkit, UpscaleSettings};
pub use progress::{LogLevel, Progress, Reporter, Stage, TaskContext};
pub use status::ToolStatus;

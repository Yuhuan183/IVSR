//! The super-resolution engine contract.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::model::{Catalog, CostClass, HardwareProfile, ModelManifest, ModelOrigin};
use crate::{BaselinePoint, Error, ParamSpec, ParamValues, Result, TaskContext, Text, ToolStatus};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineInfo {
    /// Stable identifier used in config files and on the command line.
    pub id: String,
    pub name: String,
    pub description: Text,
    pub homepage: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub description: Text,
    /// Scales the model produces natively, ascending.
    pub scales: Vec<u32>,
    /// Free-form hints such as `photo`, `anime`, `video`.
    pub tags: Vec<String>,
    pub origin: ModelOrigin,
    /// Bundled models are part of the engine install and cannot be removed alone.
    pub removable: bool,
    pub version: Option<String>,
    pub license: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    /// Bytes on disk.
    pub size: u64,
    pub architecture: Option<String>,
    /// Weight count, when derivable from the files.
    pub parameters: Option<u64>,
    pub class: Option<CostClass>,
    pub hardware: Option<HardwareProfile>,
    /// Reference measurements at `scales[0]`.
    pub baseline: Vec<BaselinePoint>,
    /// `role -> sha256` of installed files, for update detection.
    pub file_hashes: Vec<(String, String)>,
}

/// A device the engine can run on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComputeDevice {
    pub index: u32,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineCaps {
    /// Format ids the engine reads directly; anything else is converted first.
    pub input_formats: Vec<String>,
    /// Format id the engine writes. The pipeline re-encodes to the user's choice.
    pub output_format: String,
    /// Whether a directory of frames can be processed in one invocation.
    pub batch: bool,
}

/// Where an engine's runtime can be downloaded from. Interpreted by the
/// service layer, which maps `provider` onto a concrete release source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Distribution {
    /// Release source kind, e.g. `github`.
    pub provider: String,
    /// Provider-specific locator, e.g. `owner/repo`.
    pub locator: String,
    /// Pinned release tag; `None` means latest.
    pub tag: Option<String>,
    /// Substrings identifying the asset for each OS (`macos`, `linux`, `windows`).
    pub assets: Vec<(String, String)>,
    /// File name of the executable inside the archive, made executable on install.
    pub executable: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskMode {
    /// `input` and `output` are single files.
    File,
    /// `input` is a directory of frames; results go to the existing `output`
    /// directory under the same file stems.
    Directory { count: u64 },
}

#[derive(Debug, Clone)]
pub struct UpscaleTask<'a> {
    pub input: &'a Path,
    pub output: &'a Path,
    pub mode: TaskMode,
    pub model: &'a str,
    /// One of the model's native scales.
    pub scale: u32,
    pub params: &'a ParamValues,
}

/// A super-resolution backend. Implementations wrap one tool or runtime and
/// describe themselves well enough for frontends to stay engine-agnostic.
pub trait Engine: Send + Sync {
    fn info(&self) -> EngineInfo;

    /// Probes the runtime. Called on demand, so it must stay cheap.
    fn status(&self) -> ToolStatus;

    fn models(&self) -> Vec<ModelInfo>;

    fn default_model(&self) -> Option<String> {
        self.models().first().map(|m| m.id.clone())
    }

    fn params(&self) -> Vec<ParamSpec>;

    fn caps(&self) -> EngineCaps;

    fn distribution(&self) -> Option<Distribution> {
        None
    }

    /// Directory into which `distribution` should be installed.
    fn install_dir(&self) -> Option<PathBuf> {
        None
    }

    /// Built-in catalogue: metadata for bundled models plus downloadable ones.
    fn catalog(&self) -> Option<Catalog> {
        None
    }

    /// Directory holding managed (downloaded or imported) models, one
    /// subdirectory per model id. `None` when the engine cannot manage models.
    fn model_store(&self) -> Option<PathBuf> {
        None
    }

    /// Validates `manifest` and maps each file role to its path inside the
    /// model's directory.
    fn model_layout(&self, manifest: &ModelManifest) -> Result<Vec<(String, PathBuf)>> {
        Err(Error::Invalid(format!("engine `{}` does not support managed models (`{}`)", self.info().id, manifest.id)))
    }

    /// Compute devices visible to the engine. May launch the runtime once.
    fn devices(&self) -> Vec<ComputeDevice> {
        Vec::new()
    }

    fn upscale(&self, task: &UpscaleTask<'_>, ctx: &TaskContext<'_>) -> Result<()>;
}

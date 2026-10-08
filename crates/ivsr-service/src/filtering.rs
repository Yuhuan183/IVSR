//! Stand-alone filtering of images: the same filters as the upscale
//! workflow, applied to existing files (CLI `ivsr filters apply`, the
//! desktop viewer's filter panel).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use ivsr_core::filter::{Runner, resolve};
use ivsr_core::{FilterInfo, FilterSetup, FilterStage, FilterStep, ImageEncodeOptions, MediaKind, ParamSpec, fsutil};
use serde::{Deserialize, Serialize};

use crate::config::ConflictPolicy;
use crate::planner::{self, PlanOptions, PlannedJob};
use crate::registry::Registry;
use crate::{Error, Result};

/// A filter as frontends present it.
#[derive(Debug, Clone, Serialize)]
pub struct FilterView {
    pub info: FilterInfo,
    pub params: Vec<ParamSpec>,
}

/// What to filter and how; unset fields fall back to the configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilterJobRequest {
    pub stage: FilterStage,
    /// `None` runs the configured steps for `stage` (its on/off switch is ignored).
    pub steps: Option<Vec<FilterStep>>,
    /// A reference image, or a directory searched for one with the input's
    /// file name. Without it, the original of a result recorded in the
    /// history is used.
    pub reference: Option<PathBuf>,
    pub output: Option<PathBuf>,
    /// Appended to the file stem. Default: `_pre` or `_post`.
    pub suffix: Option<String>,
    pub image_format: Option<String>,
    pub image_quality: Option<u8>,
    pub conflict: Option<ConflictPolicy>,
    pub recursive: bool,
}

impl FilterJobRequest {
    pub fn new(stage: FilterStage) -> Self {
        Self {
            stage,
            steps: None,
            reference: None,
            output: None,
            suffix: None,
            image_format: None,
            image_quality: None,
            conflict: None,
            recursive: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FilterJob {
    #[serde(flatten)]
    pub planned: PlannedJob,
    pub reference: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PreparedFilters {
    pub stage: FilterStage,
    pub steps: Vec<FilterStep>,
    pub quality: u8,
    pub jobs: Vec<FilterJob>,
}

impl PreparedFilters {
    /// Jobs that will run: images that are not skipped.
    pub fn runnable(&self) -> impl Iterator<Item = &FilterJob> {
        self.jobs.iter().filter(|j| j.planned.skip.is_none() && j.planned.kind == MediaKind::Image)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FilterOutcome {
    pub output: PathBuf,
    pub width: u32,
    pub height: u32,
}

pub(crate) fn prepare(
    registry: &Registry,
    inputs: &[PathBuf],
    req: &FilterJobRequest,
    steps: Vec<FilterStep>,
    defaults: (ConflictPolicy, u8),
    reference_for: impl Fn(&Path) -> Option<PathBuf>,
) -> Result<PreparedFilters> {
    resolve(req.stage, &steps, registry.filters())?;
    let suffix = req.suffix.clone().unwrap_or_else(|| format!("_{}", req.stage.label()));
    if suffix.contains(['/', '\\']) {
        return Err(Error::Input(format!("suffix must not contain path separators: `{suffix}`")));
    }
    let quality = req.image_quality.unwrap_or(defaults.1);
    if !(1..=100).contains(&quality) {
        return Err(Error::Input(format!("image quality must be 1-100, got {quality}")));
    }
    let image_format = req.image_format.clone().unwrap_or_else(|| "same".into()).to_ascii_lowercase();
    let opts = PlanOptions {
        output: req.output.as_deref(),
        suffix: &suffix,
        image_format: &image_format,
        container: "same",
        video_codec: None,
        conflict: req.conflict.unwrap_or(defaults.0),
        recursive: req.recursive,
    };
    let planned = planner::plan(inputs, &opts, &registry.formats())?;
    let reference = req.reference.as_deref().map(std::path::absolute).transpose().map_err(|e| Error::Input(e.to_string()))?;
    match &reference {
        Some(path) if !path.exists() => {
            return Err(Error::Input(format!("reference {} does not exist", path.display())));
        }
        Some(file) if file.is_file() && planned.iter().filter(|j| j.kind == MediaKind::Image).count() > 1 => {
            return Err(Error::Input(format!(
                "reference {} is a single image but there are several inputs; pass a directory of originals \
                 matched by file name",
                file.display()
            )));
        }
        _ => {}
    }
    let roots: Vec<PathBuf> = inputs.iter().filter_map(|p| std::path::absolute(p).ok()).filter(|p| p.is_dir()).collect();
    let jobs = planned
        .into_iter()
        .map(|planned| {
            let reference = match &reference {
                Some(dir) if dir.is_dir() => matching_original(dir, &planned.input, &roots),
                Some(file) => Some(file.clone()),
                None => reference_for(&planned.input),
            };
            FilterJob { planned, reference }
        })
        .collect();
    Ok(PreparedFilters { stage: req.stage, steps: enabled(steps), quality, jobs })
}

/// The original for `input` in `dir`: at the same path relative to the input
/// directory it was found under, else by file name alone.
fn matching_original(dir: &Path, input: &Path, roots: &[PathBuf]) -> Option<PathBuf> {
    let relative = roots.iter().find_map(|root| input.strip_prefix(root).ok()).map(|rel| dir.join(rel));
    let by_name = input.file_name().map(|n| dir.join(n));
    relative.into_iter().chain(by_name).find(|p| p.is_file())
}

fn enabled(steps: Vec<FilterStep>) -> Vec<FilterStep> {
    steps.into_iter().filter(|s| s.enabled).collect()
}

/// Runs `steps` on the image `input` and writes `output` atomically, in the
/// format its extension names.
#[allow(clippy::too_many_arguments)]
pub(crate) fn filter_image(
    registry: &Registry,
    input: &Path,
    output: &Path,
    stage: FilterStage,
    steps: &[FilterStep],
    reference: Option<&Path>,
    quality: Option<u8>,
    work_root: &Path,
) -> Result<FilterOutcome> {
    if registry.classify(input).map(|(kind, _)| kind) == Some(MediaKind::Video) {
        return Err(Error::Input(
            "stand-alone filtering handles images; for videos, turn on pre- or post-processing when upscaling".into(),
        ));
    }
    let ext = fsutil::extension(output).unwrap_or_default();
    let format = registry
        .images()
        .formats()
        .into_iter()
        .find(|f| f.encode && f.matches_extension(&ext))
        .ok_or_else(|| Error::Input(format!("cannot write images as `.{ext}`")))?;
    let specs = resolve(stage, steps, registry.filters())?;
    let mut runner = Runner::start(&specs, registry.filters(), FilterSetup { stage, kind: MediaKind::Image })?;

    let images = registry.images();
    let mut frame = images.decode(input, None)?;
    let reference = reference.filter(|_| runner.uses_reference()).map(|p| images.decode(p, None)).transpose()?;
    runner.apply(&mut frame, reference.as_ref())?;

    std::fs::create_dir_all(work_root).map_err(|e| ivsr_core::Error::io_at("create work directory", work_root, e))?;
    let work = tempfile::Builder::new()
        .prefix("ivsr-filter-")
        .tempdir_in(work_root)
        .map_err(|e| ivsr_core::Error::io_at("create work directory in", work_root, e))?;
    let staged = work.path().join(format!("output.{ext}"));
    images.encode(&frame, &staged, &ImageEncodeOptions { format: format.id, quality, resize: None })?;
    fsutil::persist(&staged, output)?;
    Ok(FilterOutcome { output: output.to_path_buf(), width: frame.width, height: frame.height })
}

/// Previews kept on disk; parameter tweaks would otherwise add one each.
const PREVIEWS_KEPT: usize = 24;

/// Removes all but the `keep` most recently written files in `dir`.
pub(crate) fn prune(dir: &Path, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = entries
        .flatten()
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    for (_, path) in files.into_iter().skip(keep) {
        let _ = std::fs::remove_file(path);
    }
}

/// A cached PNG of `input` filtered by `steps`, for previews. The cache key
/// covers the input's identity, the reference and the steps.
pub(crate) fn preview(
    registry: &Registry,
    input: &Path,
    stage: FilterStage,
    steps: &[FilterStep],
    reference: Option<&Path>,
    dir: &Path,
    work_root: &Path,
) -> Result<PathBuf> {
    let mut hasher = DefaultHasher::new();
    for path in std::iter::once(input).chain(reference) {
        let meta = std::fs::metadata(path).map_err(|e| ivsr_core::Error::io_at("read", path, e))?;
        path.hash(&mut hasher);
        meta.len().hash(&mut hasher);
        meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).hash(&mut hasher);
    }
    stage.hash(&mut hasher);
    serde_json::to_string(steps).expect("serializable").hash(&mut hasher);
    let target = dir.join(format!("{:016x}.png", hasher.finish()));
    if !target.is_file() {
        filter_image(registry, input, &target, stage, steps, reference, None, work_root)?;
        prune(dir, PREVIEWS_KEPT);
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pruning_keeps_the_newest_previews() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..5 {
            let path = dir.path().join(format!("{i}.png"));
            std::fs::write(&path, b"x").unwrap();
            let when = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000 + i);
            std::fs::File::options().write(true).open(&path).unwrap().set_modified(when).unwrap();
        }
        prune(dir.path(), 2);
        let mut left: Vec<String> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        left.sort();
        assert_eq!(left, ["3.png", "4.png"]);
    }
}

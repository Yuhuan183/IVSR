//! Expanding user inputs into concrete (input, output) job pairs.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use ivsr_core::{CodecInfo, FormatInfo, MediaKind, fsutil};
use serde::Serialize;

use crate::config::ConflictPolicy;
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct PlanOptions<'a> {
    /// Output file (single input) or directory.
    pub output: Option<&'a Path>,
    /// Already-expanded stem suffix, e.g. `_x4`.
    pub suffix: &'a str,
    /// Image format id or `same`.
    pub image_format: &'a str,
    /// Video container id or `same`.
    pub container: &'a str,
    pub video_codec: Option<&'a CodecInfo>,
    pub conflict: ConflictPolicy,
    pub recursive: bool,
}

/// Why an input will not be processed. Frontends translate by `code`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum SkipReason {
    Unsupported,
    OutputExists,
    VideoUnavailable { detail: String },
}

impl std::fmt::Display for SkipReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SkipReason::Unsupported => f.write_str("unsupported format"),
            SkipReason::OutputExists => f.write_str("output already exists"),
            SkipReason::VideoUnavailable { detail } => write!(f, "video support unavailable: {detail}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlannedJob {
    pub input: PathBuf,
    pub output: PathBuf,
    pub kind: MediaKind,
    /// Output image format id or container id.
    pub format: String,
    /// Why this input will not be processed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip: Option<SkipReason>,
}

struct Source {
    path: PathBuf,
    /// Directory the file was found under, to mirror structure into an output dir.
    root: Option<PathBuf>,
}

/// Supported files named by `inputs`, expanding directories.
pub fn expand(inputs: &[PathBuf], recursive: bool, formats: &[FormatInfo]) -> Result<Vec<PathBuf>> {
    Ok(sources(inputs, recursive, formats)?
        .into_iter()
        .map(|s| s.path)
        .filter(|p| classify(p, formats).is_some())
        .collect())
}

fn sources(inputs: &[PathBuf], recursive: bool, formats: &[FormatInfo]) -> Result<Vec<Source>> {
    let mut sources = Vec::new();
    for input in inputs {
        if input.is_dir() {
            collect(input, input, recursive, formats, &mut sources)?;
        } else if input.is_file() {
            sources.push(Source { path: input.clone(), root: None });
        } else {
            return Err(Error::Input(format!("{} does not exist", input.display())));
        }
    }
    Ok(sources)
}

pub fn plan(inputs: &[PathBuf], opts: &PlanOptions<'_>, formats: &[FormatInfo]) -> Result<Vec<PlannedJob>> {
    let sources = sources(inputs, opts.recursive, formats)?;
    if sources.is_empty() {
        return Err(Error::Input("no supported image or video files found".into()));
    }

    let single_file_output = match opts.output {
        Some(out) => inputs.len() == 1 && sources.len() == 1 && sources[0].root.is_none() && !out.is_dir() && out.extension().is_some(),
        None => false,
    };

    let mut claimed: HashSet<PathBuf> = HashSet::new();
    let mut jobs = Vec::with_capacity(sources.len());
    for source in sources {
        let input = source.path;
        let Some(source_format) = classify(&input, formats) else {
            jobs.push(PlannedJob {
                output: input.clone(),
                input,
                kind: MediaKind::Image,
                format: String::new(),
                skip: Some(SkipReason::Unsupported),
            });
            continue;
        };
        let kind = source_format.kind;

        let (format, output) = if single_file_output {
            let out = opts.output.expect("checked above").to_path_buf();
            let ext = fsutil::extension(&out).unwrap_or_default();
            let target = encodable(formats, kind, &ext)
                .ok_or_else(|| Error::Input(format!("cannot write .{ext} as {}", kind_name(kind))))?;
            (target.id.clone(), out)
        } else {
            let target = output_format(&source_format, opts, formats)?;
            let input_ext = fsutil::extension(&input).unwrap_or_default();
            let ext = if target.id == source_format.id { input_ext } else { target.extensions[0].clone() };
            let dir = match (opts.output, &source.root) {
                (Some(out), Some(root)) => {
                    let rel = input.parent().and_then(|p| p.strip_prefix(root).ok()).unwrap_or(Path::new(""));
                    out.join(rel)
                }
                (Some(out), None) => out.to_path_buf(),
                (None, _) => input.parent().map(Path::to_path_buf).unwrap_or_default(),
            };
            let stem = input.file_stem().unwrap_or_default().to_string_lossy();
            (target.id.clone(), dir.join(format!("{stem}{}.{ext}", opts.suffix)))
        };
        if let Some(codec) = opts.video_codec.filter(|_| kind == MediaKind::Video) {
            if !codec.containers.contains(&format) {
                return Err(Error::Input(format!(
                    "{} cannot be stored in .{format}; choose one of: {}",
                    codec.label,
                    codec.containers.join(", ")
                )));
            }
        }

        let resolved = resolve_conflict(&input, output, opts.conflict, &claimed);
        let job = match resolved {
            Ok(output) => {
                claimed.insert(output.clone());
                PlannedJob { input, output, kind, format, skip: None }
            }
            Err(existing) => {
                PlannedJob { input, output: existing, kind, format, skip: Some(SkipReason::OutputExists) }
            }
        };
        jobs.push(job);
    }
    Ok(jobs)
}

fn kind_name(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Image => "an image",
        MediaKind::Video => "a video",
    }
}

fn classify(path: &Path, formats: &[FormatInfo]) -> Option<FormatInfo> {
    let ext = fsutil::extension(path)?;
    formats.iter().find(|f| f.decode && f.matches_extension(&ext)).cloned()
}

fn encodable<'a>(formats: &'a [FormatInfo], kind: MediaKind, id_or_ext: &str) -> Option<&'a FormatInfo> {
    formats
        .iter()
        .find(|f| f.kind == kind && f.encode && (f.id == id_or_ext || f.matches_extension(id_or_ext)))
}

fn output_format(source: &FormatInfo, opts: &PlanOptions<'_>, formats: &[FormatInfo]) -> Result<FormatInfo> {
    let (requested, fallback) = match source.kind {
        MediaKind::Image => (opts.image_format, "png"),
        MediaKind::Video => (opts.container, "mp4"),
    };
    if requested != "same" {
        return encodable(formats, source.kind, requested)
            .cloned()
            .ok_or_else(|| Error::Input(format!("cannot write `{requested}` as {}", kind_name(source.kind))));
    }
    let keep = source.encode.then(|| source.clone());
    // Keep the source container only when the chosen codec fits in it.
    let keep = match (source.kind, keep, opts.video_codec) {
        (MediaKind::Video, Some(f), Some(codec)) if !codec.containers.contains(&f.id) => {
            encodable(formats, MediaKind::Video, &codec.containers[0]).cloned()
        }
        (_, keep, _) => keep,
    };
    keep.or_else(|| encodable(formats, source.kind, fallback).cloned())
        .ok_or_else(|| Error::Input(format!("no writable format for {}", source.label)))
}

/// `Ok(path)` to write, or `Err(path)` when the job should be skipped.
fn resolve_conflict(
    input: &Path,
    wanted: PathBuf,
    policy: ConflictPolicy,
    claimed: &HashSet<PathBuf>,
) -> std::result::Result<PathBuf, PathBuf> {
    let same_as_input = |p: &Path| p == input || fs::canonicalize(p).ok() == fs::canonicalize(input).ok();
    let taken = |p: &Path| p.exists() || claimed.contains(p);
    if !taken(&wanted) {
        return Ok(wanted);
    }
    // Never overwrite the input itself, and never let two jobs share an output.
    let forced_rename = same_as_input(&wanted) || claimed.contains(&wanted);
    match policy {
        ConflictPolicy::Overwrite if !forced_rename => return Ok(wanted),
        ConflictPolicy::Skip if !forced_rename => return Err(wanted),
        _ => {}
    }
    let stem = wanted.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let ext = wanted.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    (1..)
        .map(|n| wanted.with_file_name(format!("{stem}_{n}{ext}")))
        .find(|p| !taken(p))
        .ok_or(wanted)
}

fn collect(dir: &Path, root: &Path, recursive: bool, formats: &[FormatInfo], out: &mut Vec<Source>) -> Result<()> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| Error::Input(format!("cannot read {}: {e}", dir.display())))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            if recursive {
                collect(&path, root, recursive, formats, out)?;
            }
        } else if classify(&path, formats).is_some() {
            out.push(Source { path, root: Some(root.to_path_buf()) });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fmt(id: &str, kind: MediaKind, exts: &[&str], encode: bool) -> FormatInfo {
        FormatInfo {
            id: id.into(),
            label: id.to_uppercase(),
            kind,
            extensions: exts.iter().map(|e| e.to_string()).collect(),
            decode: true,
            encode,
            lossy: false,
            note: None,
        }
    }

    fn formats() -> Vec<FormatInfo> {
        vec![
            fmt("png", MediaKind::Image, &["png"], true),
            fmt("jpg", MediaKind::Image, &["jpg", "jpeg"], true),
            fmt("gif", MediaKind::Image, &["gif"], false),
            fmt("mp4", MediaKind::Video, &["mp4"], true),
            fmt("webm", MediaKind::Video, &["webm"], true),
            fmt("avi", MediaKind::Video, &["avi"], false),
        ]
    }

    fn h264() -> CodecInfo {
        CodecInfo {
            id: "h264".into(),
            label: "H.264".into(),
            containers: vec!["mp4".into(), "mkv".into()],
            quality: None,
            presets: vec![],
            default_preset: None,
            hardware: false,
            available: true,
        }
    }

    fn opts<'a>(output: Option<&'a Path>, codec: Option<&'a CodecInfo>) -> PlanOptions<'a> {
        PlanOptions {
            output,
            suffix: "_x4",
            image_format: "same",
            container: "same",
            video_codec: codec,
            conflict: ConflictPolicy::Rename,
            recursive: false,
        }
    }

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"").unwrap();
    }

    /// Outputs relative to `root`, joined with `/` so expectations are separator-independent.
    fn names(jobs: &[PlannedJob], root: &Path) -> Vec<String> {
        jobs.iter()
            .map(|j| {
                let rel = j.output.strip_prefix(root).unwrap();
                rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/")
            })
            .collect()
    }

    #[test]
    fn outputs_sit_beside_inputs_keeping_format_and_extension() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.jpeg");
        let g = tmp.path().join("g.gif");
        touch(&a);
        touch(&g);
        let jobs = plan(&[a, g], &opts(None, None), &formats()).unwrap();
        // gif cannot be written, so it falls back to png.
        assert_eq!(names(&jobs, tmp.path()), vec!["a_x4.jpeg", "g_x4.png"]);
        assert_eq!(jobs[1].format, "png");
    }

    #[test]
    fn directory_input_mirrors_structure_only_when_recursive() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        touch(&src.join("one.png"));
        touch(&src.join("nested/two.png"));
        touch(&src.join("notes.txt"));
        touch(&src.join(".hidden.png"));
        let out = tmp.path().join("out");

        let flat = plan(std::slice::from_ref(&src), &opts(Some(&out), None), &formats()).unwrap();
        assert_eq!(names(&flat, tmp.path()), vec!["out/one_x4.png"]);

        let mut o = opts(Some(&out), None);
        o.recursive = true;
        let deep = plan(&[src], &o, &formats()).unwrap();
        assert_eq!(names(&deep, tmp.path()), vec!["out/nested/two_x4.png", "out/one_x4.png"]);
    }

    #[test]
    fn existing_outputs_follow_the_conflict_policy() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.png");
        touch(&a);
        touch(&tmp.path().join("a_x4.png"));

        let renamed = plan(std::slice::from_ref(&a), &opts(None, None), &formats()).unwrap();
        assert_eq!(names(&renamed, tmp.path()), vec!["a_x4_1.png"]);

        let mut o = opts(None, None);
        o.conflict = ConflictPolicy::Skip;
        let skipped = plan(std::slice::from_ref(&a), &o, &formats()).unwrap();
        assert_eq!(skipped[0].skip, Some(SkipReason::OutputExists));

        o.conflict = ConflictPolicy::Overwrite;
        o.suffix = "";
        let not_input = plan(&[a], &o, &formats()).unwrap();
        assert_eq!(names(&not_input, tmp.path()), vec!["a_1.png"], "the input itself is never overwritten");
    }

    #[test]
    fn two_inputs_never_share_an_output() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.png");
        let b = tmp.path().join("a.jpg");
        touch(&a);
        touch(&b);
        let mut o = opts(None, None);
        o.image_format = "png";
        let jobs = plan(&[a, b], &o, &formats()).unwrap();
        assert_eq!(names(&jobs, tmp.path()), vec!["a_x4.png", "a_x4_1.png"]);
    }

    #[test]
    fn explicit_output_file_sets_the_format() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("a.png");
        touch(&a);
        let out = tmp.path().join("final.jpg");
        let jobs = plan(std::slice::from_ref(&a), &opts(Some(&out), None), &formats()).unwrap();
        assert_eq!((jobs[0].output.clone(), jobs[0].format.as_str()), (out, "jpg"));
        assert!(plan(&[a], &opts(Some(&tmp.path().join("x.gif")), None), &formats()).is_err());
    }

    #[test]
    fn video_container_follows_codec_compatibility() {
        let tmp = tempfile::tempdir().unwrap();
        let clip = tmp.path().join("clip.webm");
        let old = tmp.path().join("old.avi");
        touch(&clip);
        touch(&old);
        let codec = h264();
        let jobs = plan(&[clip.clone(), old], &opts(None, Some(&codec)), &formats()).unwrap();
        assert_eq!(names(&jobs, tmp.path()), vec!["clip_x4.mp4", "old_x4.mp4"]);

        let mut o = opts(None, Some(&codec));
        o.container = "webm";
        assert!(plan(&[clip], &o, &formats()).is_err(), "explicit incompatible container is an error");
    }

    #[test]
    fn missing_input_and_empty_selection_are_errors() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(plan(&[tmp.path().join("nope.png")], &opts(None, None), &formats()).is_err());
        assert!(plan(&[tmp.path().to_path_buf()], &opts(None, None), &formats()).is_err());
    }
}

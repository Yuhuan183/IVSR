//! Where `realesrgan-ncnn-vulkan` models live and how they are named.
//!
//! The binary loads `<dir>/<name>.param|.bin` for any `-n <name>`, except the
//! exact name `realesr-animevideov3`, which loads `<dir>/<name>-x<scale>.*`.
//! A missing file crashes it (exit 139) without a message, so every lookup
//! here checks the files before a run.
//!
//! Two sources of models:
//! - bundled: the `models/` directory shipped with the binary;
//! - managed: `<store>/<id>/models/<id>.{param,bin}` with `<store>/<id>/model.json`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ivsr_core::{Catalog, ModelInfo, ModelManifest, ModelOrigin};

pub(crate) const MANIFEST: &str = "model.json";
/// The only name the binary maps to per-scale files.
const PER_SCALE_NAME: &str = "realesr-animevideov3";

pub(crate) fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| serde_json::from_str(include_str!("catalog.json")).expect("built-in catalog is valid"))
}

/// How to invoke one model.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Located {
    pub info: ModelInfo,
    pub dir: PathBuf,
    /// Value for `-n`.
    pub name: String,
}

impl Located {
    pub fn param_file(&self, scale: u32) -> PathBuf {
        param_path(&self.dir, &self.name, scale)
    }

    pub fn has_weights(&self, scale: u32) -> bool {
        let param = self.param_file(scale);
        param.is_file() && param.with_extension("bin").is_file()
    }
}

fn param_path(dir: &Path, name: &str, scale: u32) -> PathBuf {
    if name == PER_SCALE_NAME {
        dir.join(format!("{name}-x{scale}.param"))
    } else {
        dir.join(format!("{name}.param"))
    }
}

/// `x4`, `4x`, `-x2` style scale hints in a file name.
fn scale_hint(name: &str) -> Option<u32> {
    let lower = name.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    for (i, w) in bytes.windows(2).enumerate() {
        let before = i.checked_sub(1).map(|j| bytes[j]);
        let after = bytes.get(i + 2).copied();
        let boundary = |c: Option<u8>| c.is_none_or(|c| !c.is_ascii_digit());
        match w {
            [b'x', d] if d.is_ascii_digit() && boundary(after) && before.is_none_or(|c| !c.is_ascii_alphabetic()) => {
                return Some((d - b'0') as u32);
            }
            [d, b'x'] if d.is_ascii_digit() && boundary(before) => return Some((d - b'0') as u32),
            _ => {}
        }
    }
    None
}

fn file_size(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Applies catalogue metadata (description, licence, hardware, baseline).
fn decorate(info: &mut ModelInfo, manifest: &ModelManifest, catalog: &Catalog) {
    info.name = manifest.name.clone();
    info.description = manifest.description.clone();
    info.tags = manifest.tags.clone();
    info.license = manifest.license.clone();
    info.author = manifest.author.clone();
    info.homepage = manifest.homepage.clone();
    info.architecture = manifest.architecture.clone();
    info.baseline = manifest.baseline.clone();
    if !manifest.version.is_empty() {
        info.version = Some(manifest.version.clone());
    }
    if let Some(profile) = manifest.architecture.as_ref().and_then(|a| catalog.architectures.get(a)) {
        info.class = Some(profile.class);
        info.hardware = Some(profile.clone());
    }
}

/// Models in the directory shipped with the binary.
pub(crate) fn scan_bundled(dir: &Path) -> Vec<Located> {
    let mut stems: Vec<String> = fs::read_dir(dir)
        .map(|d| {
            d.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "param") && p.with_extension("bin").is_file())
                .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                .collect()
        })
        .unwrap_or_default();
    stems.sort();

    let catalog = catalog();
    let mut found: Vec<Located> = Vec::new();
    for stem in stems {
        let (name, scale) = match stem.strip_prefix(PER_SCALE_NAME).and_then(|s| s.strip_prefix("-x")) {
            Some(s) => match s.parse::<u32>() {
                Ok(scale) => (PER_SCALE_NAME.to_string(), Some(scale)),
                Err(_) => continue,
            },
            None => (stem.clone(), None),
        };
        if let Some(existing) = found.iter_mut().find(|l| l.name == name) {
            if let Some(s) = scale {
                existing.info.scales.push(s);
                existing.info.scales.sort_unstable();
            }
            existing.info.size += file_size(&dir.join(format!("{stem}.param"))) + file_size(&dir.join(format!("{stem}.bin")));
            continue;
        }
        let manifest = catalog.models.iter().find(|m| m.id == name);
        let scales = match (scale, manifest) {
            (Some(s), _) => vec![s],
            (None, Some(m)) => m.scales.clone(),
            (None, None) => vec![scale_hint(&name).unwrap_or(4)],
        };
        let bin = dir.join(format!("{stem}.bin"));
        let mut info = ModelInfo {
            id: name.clone(),
            name: name.clone(),
            scales,
            origin: ModelOrigin::Bundled,
            removable: false,
            size: file_size(&dir.join(format!("{stem}.param"))) + file_size(&bin),
            ..Default::default()
        };
        if let Some(m) = manifest {
            decorate(&mut info, m, catalog);
        }
        found.push(Located { info, dir: dir.to_path_buf(), name });
    }
    for located in &mut found {
        // ncnn stores weights as fp16 for these models: two bytes each.
        let bin = located.param_file(located.info.scales[0]).with_extension("bin");
        located.info.parameters = Some(file_size(&bin) / 2);
    }
    found
}

/// Managed models under `store`, skipping anything incomplete.
pub(crate) fn scan_store(store: &Path) -> Vec<Located> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(store)
        .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    dirs.sort();
    let catalog = catalog();
    dirs.into_iter()
        .filter(|d| !d.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')))
        .filter_map(|model_dir| {
            let manifest: ModelManifest = serde_json::from_slice(&fs::read(model_dir.join(MANIFEST)).ok()?).ok()?;
            let dir = model_dir.join("models");
            let located = Located {
                dir: dir.clone(),
                name: manifest.id.clone(),
                info: ModelInfo {
                    id: manifest.id.clone(),
                    scales: manifest.scales.clone(),
                    origin: manifest.origin,
                    removable: true,
                    file_hashes: manifest.files.iter().map(|f| (f.role.clone(), f.sha256.clone())).collect(),
                    ..Default::default()
                },
            };
            let scale = *manifest.scales.first()?;
            if !located.has_weights(scale) {
                return None;
            }
            let mut located = located;
            decorate(&mut located.info, &manifest, catalog);
            let bin = located.param_file(scale).with_extension("bin");
            located.info.size = file_size(&located.param_file(scale)) + file_size(&bin);
            located.info.parameters = Some(file_size(&bin) / 2);
            Some(located)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_hints_follow_common_naming() {
        assert_eq!(scale_hint("realesrgan-x4plus"), Some(4));
        assert_eq!(scale_hint("4xLSDIRCompactC3"), Some(4));
        assert_eq!(scale_hint("2x_model"), Some(2));
        assert_eq!(scale_hint("model-x3"), Some(3));
        assert_eq!(scale_hint("rgx4"), None);
        assert_eq!(scale_hint("photo"), None);
    }

    #[test]
    fn bundled_scan_groups_per_scale_files_and_applies_catalog_metadata() {
        let dir = tempfile::tempdir().unwrap();
        for f in [
            "realesr-animevideov3-x2",
            "realesr-animevideov3-x4",
            "realesrgan-x4plus",
            "my-custom-x2",
        ] {
            fs::write(dir.path().join(format!("{f}.param")), b"7767517").unwrap();
            fs::write(dir.path().join(format!("{f}.bin")), vec![0u8; 10]).unwrap();
        }
        fs::write(dir.path().join("realesr-animevideov3-x3.param"), b"").unwrap(); // no .bin

        let models = scan_bundled(dir.path());
        let summary: Vec<_> = models.iter().map(|l| (l.name.as_str(), l.info.scales.clone())).collect();
        assert_eq!(
            summary,
            vec![("my-custom-x2", vec![2]), ("realesr-animevideov3", vec![2, 4]), ("realesrgan-x4plus", vec![4])]
        );
        let x4plus = &models[2].info;
        assert_eq!(x4plus.license.as_deref(), Some("BSD-3-Clause"));
        assert_eq!(x4plus.description.get("zh-TW"), "通用模型, 適合照片與真實場景影像.");
        assert!(x4plus.hardware.is_some() && !x4plus.removable);
        assert_eq!(models[1].param_file(4), dir.path().join("realesr-animevideov3-x4.param"));
        assert_eq!(models[0].param_file(2), dir.path().join("my-custom-x2.param"));
    }

    #[test]
    fn built_in_catalog_is_consistent() {
        let catalog = catalog();
        for m in &catalog.models {
            assert!(ivsr_core::model::valid_model_id(&m.id), "{}", m.id);
            assert!(m.architecture.as_ref().is_some_and(|a| catalog.architectures.contains_key(a)), "{}", m.id);
            assert!(m.description.get("zh-TW") != m.description.get("en"), "{} lacks zh-TW", m.id);
            assert!(m.license.is_some() && m.author.is_some(), "{} lacks attribution", m.id);
            assert_eq!(m.baseline.len(), 2, "{}", m.id);
            if !m.bundled {
                let roles: Vec<_> = m.files.iter().map(|f| f.role.as_str()).collect();
                assert_eq!(roles, vec!["param", "bin"], "{}", m.id);
                assert!(m.files.iter().all(|f| f.url.starts_with("https://") && f.sha256.len() == 64), "{}", m.id);
                assert_eq!(m.scales.len(), 1, "{}", m.id);
            }
        }
    }
}

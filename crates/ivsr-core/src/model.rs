//! Model catalogues, manifests, hardware profiles and performance baselines.
//!
//! A `Catalog` is what an engine ships built in or a remote URL serves. A
//! `ModelManifest` describes one model; installed models keep a copy of
//! theirs next to the weights.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::i18n::Text;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelOrigin {
    /// Shipped inside the engine distribution.
    #[default]
    Bundled,
    /// Downloaded from a catalogue.
    Catalog,
    /// Added by the user from local files.
    Imported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelFile {
    /// Engine-defined role, e.g. `param` / `bin` for ncnn.
    pub role: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

/// One timed run on a reference or local device.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BaselinePoint {
    pub width: u32,
    pub height: u32,
    /// Wall time of one engine invocation, including model load.
    pub seconds: f64,
}

/// `seconds ≈ startup + per_megapixel × input megapixels`, fitted from points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Throughput {
    pub startup: f64,
    pub per_megapixel: f64,
}

impl Throughput {
    /// Least-squares line through `points` (needs two distinct sizes).
    pub fn fit(points: &[BaselinePoint]) -> Option<Self> {
        let xs: Vec<(f64, f64)> =
            points.iter().map(|p| (p.width as f64 * p.height as f64 / 1e6, p.seconds)).collect();
        let n = xs.len() as f64;
        if xs.len() < 2 {
            return None;
        }
        let (sx, sy) = xs.iter().fold((0.0, 0.0), |(a, b), (x, y)| (a + x, b + y));
        let (mx, my) = (sx / n, sy / n);
        let var: f64 = xs.iter().map(|(x, _)| (x - mx).powi(2)).sum();
        if var <= f64::EPSILON {
            return None;
        }
        let cov: f64 = xs.iter().map(|(x, y)| (x - mx) * (y - my)).sum();
        let per_megapixel = (cov / var).max(0.0);
        let startup = (my - per_megapixel * mx).max(0.0);
        Some(Self { startup, per_megapixel })
    }

    /// Estimated seconds to process `megapixels` of input in `invocations` engine runs.
    pub fn estimate(&self, megapixels: f64, invocations: u64) -> f64 {
        self.startup * invocations.max(1) as f64 + self.per_megapixel * megapixels
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostClass {
    Light,
    Medium,
    Heavy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TileMemory {
    /// Tile size; 0 means the engine's automatic choice.
    pub tile: u32,
    /// Peak memory in MiB on the reference device.
    pub mb: u32,
}

/// Resource needs of a network architecture on a given runtime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HardwareProfile {
    pub class: CostClass,
    pub summary: Text,
    /// Ascending by tile size, `tile = 0` (automatic) first when present.
    pub memory_by_tile: Vec<TileMemory>,
}

impl HardwareProfile {
    pub fn automatic_mb(&self) -> Option<u32> {
        self.memory_by_tile.iter().find(|t| t.tile == 0).map(|t| t.mb)
    }
}

/// Where and how reference baselines were measured.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Reference {
    pub device: String,
    pub runtime: String,
    /// ISO date.
    pub measured_on: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelManifest {
    pub id: String,
    /// Engine id this model runs on.
    pub engine: String,
    pub name: String,
    pub description: Text,
    /// Changes whenever the files change; compared via file hashes anyway.
    #[serde(default)]
    pub version: String,
    pub scales: Vec<u32>,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Key into `Catalog::architectures`.
    #[serde(default)]
    pub architecture: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    /// Downloadable files; empty for bundled models.
    #[serde(default)]
    pub files: Vec<ModelFile>,
    #[serde(default)]
    pub bundled: bool,
    /// Reference measurements at the model's first scale.
    #[serde(default)]
    pub baseline: Vec<BaselinePoint>,
    #[serde(default)]
    pub origin: ModelOrigin,
    /// Unix seconds; set on installed manifests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_at: Option<u64>,
}

impl ModelManifest {
    pub fn download_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    #[serde(default)]
    pub reference: Reference,
    #[serde(default)]
    pub architectures: BTreeMap<String, HardwareProfile>,
    #[serde(default)]
    pub models: Vec<ModelManifest>,
}

/// Model ids become directory names: keep them to a portable alphabet.
pub fn valid_model_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && !id.starts_with('.')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(side: u32, seconds: f64) -> BaselinePoint {
        BaselinePoint { width: side, height: side, seconds }
    }

    #[test]
    fn throughput_fit_recovers_startup_and_rate() {
        // Measured on the reference device for realesrgan-x4plus.
        let t = Throughput::fit(&[p(256, 1.22), p(512, 3.35)]).unwrap();
        assert!((t.per_megapixel - 10.83).abs() < 0.05, "{t:?}");
        assert!((t.startup - 0.51).abs() < 0.02, "{t:?}");
        // A 1920x1080 frame in one invocation.
        assert!((t.estimate(2.0736, 1) - 22.97).abs() < 0.2);
        assert!(Throughput::fit(&[p(256, 1.0)]).is_none());
        assert!(Throughput::fit(&[p(256, 1.0), p(256, 1.2)]).is_none());
    }

    #[test]
    fn model_ids_are_restricted_to_safe_names() {
        for ok in ["realesr-general-x4v3", "4xLSDIR_CompactC3", "my.model-2"] {
            assert!(valid_model_id(ok), "{ok}");
        }
        for bad in ["", "../x", "a/b", ".hidden", "a b", "模型"] {
            assert!(!valid_model_id(bad), "{bad}");
        }
    }

    #[test]
    fn catalog_parses_with_plain_string_descriptions_and_defaults() {
        let json = r#"{"models": [{"id": "m", "engine": "realesrgan", "name": "M",
            "description": "plain", "scales": [4],
            "files": [{"role": "param", "url": "https://x/m.param", "size": 1, "sha256": "ab"}]}]}"#;
        let catalog: Catalog = serde_json::from_str(json).unwrap();
        let m = &catalog.models[0];
        assert_eq!(m.description.get("zh-TW"), "plain");
        assert_eq!((m.origin, m.bundled, m.download_size()), (ModelOrigin::Bundled, false, 1));
    }
}

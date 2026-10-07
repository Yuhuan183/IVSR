//! Matching a model's memory profile against the user's GPU.

use ivsr_core::HardwareProfile;
use serde::Serialize;

use crate::system::GpuInfo;

/// Share of unified memory a GPU workload can reasonably claim on macOS.
const UNIFIED_SHARE: f64 = 0.6;
/// Headroom left on a discrete GPU for the display and other apps.
const DISCRETE_SHARE: f64 = 0.85;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Fit {
    /// Automatic tiling fits with room to spare.
    Comfortable,
    /// Works with the suggested smaller tile.
    Constrained,
    /// Even the smallest measured tile exceeds the available memory.
    Insufficient,
    /// GPU memory is not known; only requirements can be shown.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Advice {
    pub fit: Fit,
    /// Tile size to set when not comfortable.
    pub suggested_tile: Option<u32>,
    /// Peak MiB with automatic tiling on the reference device.
    pub needed_mb: Option<u32>,
    /// MiB this GPU can reasonably give the model.
    pub available_mb: Option<u64>,
}

pub fn advise(profile: Option<&HardwareProfile>, gpu: Option<&GpuInfo>) -> Advice {
    let needed_mb = profile.and_then(HardwareProfile::automatic_mb);
    let available_mb = gpu.and_then(|g| {
        let share = if g.unified { UNIFIED_SHARE } else { DISCRETE_SHARE };
        g.memory_mb.map(|mb| (mb as f64 * share) as u64)
    });
    let (Some(profile), Some(needed), Some(available)) = (profile, needed_mb, available_mb) else {
        return Advice { fit: Fit::Unknown, suggested_tile: None, needed_mb, available_mb };
    };
    if needed as u64 <= available {
        return Advice { fit: Fit::Comfortable, suggested_tile: None, needed_mb, available_mb };
    }
    // Largest explicit tile that fits; larger tiles are faster.
    let fitting = profile.memory_by_tile.iter().filter(|t| t.tile > 0 && t.mb as u64 <= available).max_by_key(|t| t.tile);
    match fitting {
        Some(t) => Advice { fit: Fit::Constrained, suggested_tile: Some(t.tile), needed_mb, available_mb },
        None => {
            let smallest = profile.memory_by_tile.iter().filter(|t| t.tile > 0).min_by_key(|t| t.tile).map(|t| t.tile);
            Advice { fit: Fit::Insufficient, suggested_tile: smallest, needed_mb, available_mb }
        }
    }
}

#[cfg(test)]
mod tests {
    use ivsr_core::{CostClass, TileMemory};

    use super::*;

    fn rrdb() -> HardwareProfile {
        // Measured for the 23-block RRDB on the reference device.
        let t = |tile, mb| TileMemory { tile, mb };
        HardwareProfile {
            class: CostClass::Heavy,
            summary: "".into(),
            memory_by_tile: vec![t(0, 1480), t(32, 656), t(64, 660), t(128, 940), t(256, 2060)],
        }
    }

    fn gpu(mb: Option<u64>, unified: bool) -> GpuInfo {
        GpuInfo { index: Some(0), name: "GPU".into(), memory_mb: mb, unified }
    }

    #[test]
    fn enough_memory_keeps_automatic_tiling() {
        let a = advise(Some(&rrdb()), Some(&gpu(Some(24 * 1024), true)));
        assert_eq!((a.fit, a.suggested_tile, a.available_mb), (Fit::Comfortable, None, Some(14745)));
    }

    #[test]
    fn small_discrete_gpu_gets_the_largest_tile_that_fits() {
        // 1.5 GB card: 85% = 1305 MiB, so tile 128 (940) fits, 256 (2060) does not.
        let a = advise(Some(&rrdb()), Some(&gpu(Some(1536), false)));
        assert_eq!((a.fit, a.suggested_tile), (Fit::Constrained, Some(128)));
        let tiny = advise(Some(&rrdb()), Some(&gpu(Some(512), false)));
        assert_eq!((tiny.fit, tiny.suggested_tile), (Fit::Insufficient, Some(32)));
    }

    #[test]
    fn unknown_memory_reports_requirements_only() {
        let a = advise(Some(&rrdb()), Some(&gpu(None, false)));
        assert_eq!((a.fit, a.needed_mb), (Fit::Unknown, Some(1480)));
        assert_eq!(advise(None, None).fit, Fit::Unknown);
    }
}

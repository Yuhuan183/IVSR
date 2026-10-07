//! Mapping a requested output scale onto the scales a model natively supports.

use serde::Serialize;

use crate::{Error, Result};

pub const MIN_SCALE: f64 = 1.0;
pub const MAX_SCALE: f64 = 16.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct ScalePlan {
    /// Scale passed to the engine.
    pub native: u32,
    /// Final scale requested by the user.
    pub requested: f64,
}

impl ScalePlan {
    /// Whether the engine output must be resampled to reach the requested scale.
    pub fn needs_resize(&self) -> bool {
        (self.native as f64 - self.requested).abs() > 1e-6
    }

    /// Final output dimensions for an input of `width`×`height`.
    pub fn output_size(&self, width: u32, height: u32) -> (u32, u32) {
        let scale = |v: u32| ((v as f64 * self.requested).round() as u32).max(1);
        (scale(width), scale(height))
    }
}

/// Picks the engine scale for `requested`: an exact native match, else the
/// smallest native scale above it (downsampled afterwards), else the largest
/// native scale (upsampled afterwards).
pub fn plan_scale(native_scales: &[u32], requested: f64) -> Result<ScalePlan> {
    if !requested.is_finite() || !(MIN_SCALE..=MAX_SCALE).contains(&requested) {
        return Err(Error::Invalid(format!("scale must be between {MIN_SCALE} and {MAX_SCALE}, got {requested}")));
    }
    let mut scales: Vec<u32> = native_scales.iter().copied().filter(|s| *s > 0).collect();
    scales.sort_unstable();
    let native = scales
        .iter()
        .copied()
        .find(|s| *s as f64 >= requested - 1e-6)
        .or_else(|| scales.last().copied())
        .ok_or_else(|| Error::Invalid("model declares no supported scale".into()))?;
    Ok(ScalePlan { native, requested })
}

/// Formats a scale for file names and messages: `4`, `2.5`.
pub fn format_scale(scale: f64) -> String {
    let text = format!("{scale:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_native_scale_needs_no_resize() {
        let plan = plan_scale(&[2, 3, 4], 3.0).unwrap();
        assert_eq!(plan.native, 3);
        assert!(!plan.needs_resize());
    }

    #[test]
    fn fractional_scale_uses_next_native_and_downsamples() {
        let plan = plan_scale(&[4], 2.0).unwrap();
        assert_eq!(plan.native, 4);
        assert!(plan.needs_resize());
        assert_eq!(plan.output_size(101, 50), (202, 100));

        let plan = plan_scale(&[2, 3, 4], 2.5).unwrap();
        assert_eq!(plan.native, 3);
    }

    #[test]
    fn scale_beyond_model_uses_largest_native() {
        let plan = plan_scale(&[2, 4], 8.0).unwrap();
        assert_eq!(plan.native, 4);
        assert!(plan.needs_resize());
    }

    #[test]
    fn out_of_bounds_scale_is_rejected() {
        assert!(plan_scale(&[4], 0.5).is_err());
        assert!(plan_scale(&[4], f64::NAN).is_err());
        assert!(plan_scale(&[], 2.0).is_err());
    }

    #[test]
    fn scale_formatting_drops_trailing_zeros() {
        assert_eq!(format_scale(4.0), "4");
        assert_eq!(format_scale(2.5), "2.5");
        assert_eq!(format_scale(1.25), "1.25");
    }
}

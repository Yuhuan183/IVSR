//! Saturation: scales a* and b* around neutral, leaving lightness alone.

use ivsr_core::{Filter, FilterInfo, FilterRun, FilterSetup, FilterStage, Frame, ParamSpec, ParamValues, Result, Text};
use rayon::prelude::*;

use crate::lab::{to_lab, to_rgb};
use crate::param::{float, float_param};

pub const ID: &str = "saturation";

pub struct Saturation;

impl Filter for Saturation {
    fn info(&self) -> FilterInfo {
        FilterInfo {
            id: ID.into(),
            name: Text::en("Saturation").with("zh-TW", "飽和度"),
            description: Text::en("Scales colourfulness without changing brightness; upscaled colours often come out slightly washed out.")
                .with("zh-TW", "調整色彩鮮豔度, 不改變亮度; 高畫質化後的顏色常會略為泛白."),
            stages: vec![FilterStage::Pre, FilterStage::Post],
            uses_reference: false,
        }
    }

    fn params(&self) -> Vec<ParamSpec> {
        vec![float("amount", ("Amount", "倍率"), ("1 keeps colours unchanged.", "1 = 不改變."), (0.0, 2.0), 1.05)]
    }

    fn start(&self, params: &ParamValues, _: FilterSetup) -> Result<Box<dyn FilterRun>> {
        Ok(Box::new(SaturationRun { amount: float_param(params, "amount") }))
    }
}

struct SaturationRun {
    amount: f32,
}

impl FilterRun for SaturationRun {
    fn apply(&mut self, frame: &mut Frame, _: Option<&Frame>) -> Result<()> {
        if (self.amount - 1.0).abs() < 1e-6 {
            return Ok(());
        }
        let s = self.amount;
        let scale = |v: f32| ((v - 128.0) * s + 128.0).clamp(0.0, 255.0);
        frame.pixels.par_chunks_exact_mut(4).filter(|p| p[3] > 0).for_each(|px| {
            let [l, a, b] = to_lab([px[0], px[1], px[2]]);
            px[..3].copy_from_slice(&to_rgb([l, scale(a), scale(b)]));
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ivsr_core::{MediaKind, ParamValue};

    fn apply(amount: f64, frame: &mut Frame) {
        let input = [("amount".to_string(), ParamValue::Float(amount))].into_iter().collect();
        let params = ParamValues::resolve(&Saturation.params(), &input).unwrap();
        let setup = FilterSetup { stage: FilterStage::Pre, kind: MediaKind::Image };
        Saturation.start(&params, setup).unwrap().apply(frame, None).unwrap();
    }

    #[test]
    fn zero_greys_out_colour_and_greys_are_unchanged_by_boosts() {
        let mut colour = Frame::filled(2, 2, [200, 60, 40, 255], false);
        apply(0.0, &mut colour);
        let p = &colour.pixels[..3];
        assert!(p.iter().all(|v| (*v as i32 - p[0] as i32).abs() <= 1), "{p:?}");

        let mut grey = Frame::filled(2, 2, [128, 128, 128, 255], false);
        apply(1.8, &mut grey);
        assert_eq!(&grey.pixels[..4], &[128, 128, 128, 255]);
    }

    #[test]
    fn boost_spreads_channels_and_skips_transparent_pixels() {
        let mut f = Frame::filled(1, 2, [180, 120, 100, 255], true);
        f.pixels[4..8].copy_from_slice(&[180, 120, 100, 0]);
        apply(1.3, &mut f);
        assert!(f.pixels[0] as i32 - f.pixels[2] as i32 > 80);
        assert_eq!(&f.pixels[4..8], &[180, 120, 100, 0]);
    }
}

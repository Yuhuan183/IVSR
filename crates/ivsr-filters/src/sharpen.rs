//! Detail sharpen: two-band unsharp masking of lightness with ISP-style
//! coring, so fine texture and mid-frequency shape come back without
//! amplifying noise in smooth gradients or haloing transparent edges.

use std::borrow::Cow;

use ivsr_core::{Filter, FilterInfo, FilterRun, FilterSetup, FilterStage, Frame, ParamSpec, ParamValues, Result, Text};
use rayon::prelude::*;

use crate::bleed::{DEFAULT_DISTANCE, bleed};
use crate::lab::{to_lab, to_rgb};
use crate::param::{advanced, float, float_param};
use crate::plane::{Plane, erode_ellipse5};

pub const ID: &str = "detail-sharpen";

const FINE_SIGMA: f32 = 0.85;
const MID_SIGMA: f32 = 2.5;
/// Mid-band detail is clipped to this many lightness levels.
const MID_LIMIT: f32 = 20.0;
/// Only pixels more opaque than this (after erosion) are sharpened fully.
const INTERIOR_ALPHA: u8 = 150;
const EDGE_SIGMA: f32 = 1.5;

pub struct DetailSharpen;

impl Filter for DetailSharpen {
    fn info(&self) -> FilterInfo {
        FilterInfo {
            id: ID.into(),
            name: Text::en("Detail sharpen").with("zh-TW", "細節銳化"),
            description: Text::en(
                "Two-band sharpening of brightness: fine detail plus mid-frequency clarity. Changes below the \
                 noise threshold are left alone so smooth gradients stay clean, and transparent edges are protected.",
            )
            .with("zh-TW", "針對亮度的雙頻銳化: 高頻細節加中頻清晰度. 低於防噪門檻的細微變化不處理, 平滑漸層保持乾淨, 去背邊緣也受保護."),
            stages: vec![FilterStage::Pre, FilterStage::Post],
            uses_reference: false,
        }
    }

    fn params(&self) -> Vec<ParamSpec> {
        vec![
            float(
                "amount",
                ("Detail", "細節強度"),
                (
                    "Fine detail strength; 1.3–1.5 suits metal and hard surfaces, about 0.9 suits skin.",
                    "高頻細節強度; 金屬、硬表面可用 1.3–1.5, 皮膚約 0.9.",
                ),
                (0.0, 3.0),
                1.1,
            ),
            float(
                "clarity",
                ("Clarity", "清晰度"),
                ("Mid-frequency contrast that adds depth to shapes.", "中頻對比, 增加形體的立體感."),
                (0.0, 1.5),
                0.3,
            ),
            advanced(float(
                "coring",
                ("Noise threshold", "防噪門檻"),
                (
                    "Changes smaller than this are not sharpened; raise it if flat areas look grainy.",
                    "小於此振幅的變化不銳化; 平坦區域出現雜訊感時調高.",
                ),
                (0.0, 10.0),
                2.2,
            )),
        ]
    }

    fn start(&self, params: &ParamValues, _: FilterSetup) -> Result<Box<dyn FilterRun>> {
        Ok(Box::new(SharpenRun {
            amount: float_param(params, "amount"),
            clarity: float_param(params, "clarity"),
            coring: float_param(params, "coring"),
        }))
    }
}

struct SharpenRun {
    amount: f32,
    clarity: f32,
    coring: f32,
}

impl FilterRun for SharpenRun {
    fn apply(&mut self, frame: &mut Frame, _: Option<&Frame>) -> Result<()> {
        if self.amount <= 0.0 && self.clarity <= 0.0 {
            return Ok(());
        }
        let (w, h) = (frame.width as usize, frame.height as usize);
        // Blur a copy whose fully transparent pixels carry the edge colours, so
        // the kernels never pull the black stored under them inwards. Visible
        // pixels keep their own colours, so every sharpened pixel is measured
        // against what it shows.
        let mut source = Cow::Borrowed(&*frame);
        if frame.has_alpha && frame.pixels.chunks_exact(4).any(|p| p[3] == 0) {
            bleed(source.to_mut(), 0, DEFAULT_DISTANCE);
        }
        let lightness = Plane::new(w, h, source.pixels.par_chunks_exact(4).map(|p| to_lab([p[0], p[1], p[2]])[0]).collect());
        let fine = lightness.gaussian(FINE_SIGMA);
        let mid = lightness.gaussian(MID_SIGMA);
        let weight = frame.has_alpha.then(|| {
            let solid: Vec<bool> = frame.pixels.chunks_exact(4).map(|p| p[3] > INTERIOR_ALPHA).collect();
            let interior = erode_ellipse5(&solid, w, h).into_iter().map(|v| if v { 1.0 } else { 0.0 }).collect();
            Plane::new(w, h, interior).gaussian(EDGE_SIGMA)
        });

        let (amount, clarity, coring) = (self.amount, self.clarity, self.coring);
        frame.pixels.par_chunks_exact_mut(4).enumerate().filter(|(_, p)| p[3] > 0).for_each(|(i, px)| {
            let l = lightness.data[i];
            let high = soft_coring(l - fine.data[i], coring);
            let mid = soft_coring((l - mid.data[i]).clamp(-MID_LIMIT, MID_LIMIT), coring * 0.7);
            let delta = (amount * high + clarity * mid) * weight.as_ref().map_or(1.0, |w| w.data[i]);
            if delta.abs() > 1e-3 {
                let [l, a, b] = to_lab([px[0], px[1], px[2]]);
                px[..3].copy_from_slice(&to_rgb([(l + delta).clamp(0.0, 255.0), a, b]));
            }
        });
        Ok(())
    }
}

/// Fades detail below `threshold` to zero along a smoothstep, so small
/// fluctuations (noise, gentle gradients) are not amplified.
fn soft_coring(detail: f32, threshold: f32) -> f32 {
    let t = ((detail.abs() - threshold) / threshold.max(1e-4)).clamp(0.0, 1.0);
    detail * t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ivsr_core::{MediaKind, ParamValue};

    fn run(amount: f64, clarity: f64) -> Box<dyn FilterRun> {
        let input = [("amount", amount), ("clarity", clarity)].map(|(k, v)| (k.to_string(), ParamValue::Float(v)));
        let params = ParamValues::resolve(&DetailSharpen.params(), &input.into_iter().collect()).unwrap();
        DetailSharpen.start(&params, FilterSetup { stage: FilterStage::Post, kind: MediaKind::Image }).unwrap()
    }

    /// A soft vertical edge from dark to light grey.
    fn soft_edge(has_alpha: bool) -> Frame {
        let mut f = Frame::filled(32, 8, [0, 0, 0, 255], has_alpha);
        for (i, px) in f.pixels.chunks_exact_mut(4).enumerate() {
            let x = (i % 32) as f32;
            let v = (60.0 + 120.0 / (1.0 + (-(x - 16.0) / 1.5).exp())).round() as u8;
            px[..3].copy_from_slice(&[v, v, v]);
        }
        f
    }

    fn row(f: &Frame) -> Vec<i32> {
        f.pixels.chunks(4).take(32).map(|p| p[0] as i32).collect()
    }

    #[test]
    fn edges_get_steeper_and_flat_areas_stay_put() {
        let mut f = soft_edge(false);
        run(1.1, 0.3).apply(&mut f, None).unwrap();
        let (before, after) = (row(&soft_edge(false)), row(&f));
        assert!(after[14] < before[14] && after[18] > before[18], "{before:?}\n{after:?}");
        assert_eq!((after[0], after[31]), (before[0], before[31]), "flat ends untouched by coring");
    }

    #[test]
    fn translucent_interiors_are_sharpened_exactly_like_opaque_ones() {
        // Glass-like alpha 170 inside a flat grey solid frame is still "interior"
        // (alpha > 150), so its colours must be sharpened exactly as if opaque.
        let frame = |interior_alpha: u8| {
            let mut f = soft_edge(true);
            for (i, px) in f.pixels.chunks_exact_mut(4).enumerate() {
                if (4..28).contains(&(i % 32)) {
                    px[3] = interior_alpha;
                } else {
                    px[..3].copy_from_slice(&[100, 100, 100]);
                }
            }
            f
        };
        let (mut glass, mut opaque) = (frame(170), frame(255));
        run(1.1, 0.3).apply(&mut glass, None).unwrap();
        run(1.1, 0.3).apply(&mut opaque, None).unwrap();
        let rgb = |f: &Frame| f.pixels.chunks(4).map(|p| [p[0], p[1], p[2]]).collect::<Vec<_>>();
        assert_eq!(rgb(&glass), rgb(&opaque));
    }

    #[test]
    fn zero_amounts_are_an_exact_identity() {
        let mut f = soft_edge(true);
        run(0.0, 0.0).apply(&mut f, None).unwrap();
        assert_eq!(f, soft_edge(true));
    }

    #[test]
    fn transparent_pixels_are_untouched_and_cut_out_edges_are_protected() {
        let mut f = soft_edge(true);
        // Left third fully transparent (black underneath), the rest solid.
        for (i, px) in f.pixels.chunks_exact_mut(4).enumerate() {
            if i % 32 < 10 {
                px.copy_from_slice(&[0, 0, 0, 0]);
            }
        }
        let before = f.clone();
        run(1.5, 0.5).apply(&mut f, None).unwrap();
        for (i, (a, b)) in f.pixels.chunks(4).zip(before.pixels.chunks(4)).enumerate() {
            if b[3] == 0 {
                assert_eq!(a, b, "pixel {i}");
            }
        }
        // The first solid column sits on the cut-out edge, so it is barely changed.
        assert!((f.pixels[10 * 4] as i32 - before.pixels[10 * 4] as i32).abs() <= 2);
    }
}

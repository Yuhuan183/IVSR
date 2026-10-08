//! Tone restore: brings back the contrast that super-resolution models tend
//! to flatten (lifted blacks, dimmed highlights) by matching the result's
//! lightness distribution to the picture the engine received.

use ivsr_core::{
    Filter, FilterInfo, FilterRun, FilterSetup, FilterStage, Frame, MediaKind, ParamSpec, ParamValues, Result, Text,
};
use rayon::prelude::*;

use crate::lab::{to_lab, to_rgb};
use crate::param::{advanced, float, float_param};

pub const ID: &str = "tone-restore";

/// Pixels more opaque than this take part in the histograms and are remapped,
/// with or without a reference; fainter ones (glows, soft shadows) are kept.
const MASK_ALPHA: u8 = 120;
/// Smoothing of the matching curve, in histogram bins.
const LUT_SIGMA: f64 = 4.0;
/// Reference histograms further apart than this (L1, 0..=2) mark a scene
/// cut, where temporal smoothing restarts instead of dragging the old curve.
const SCENE_CUT: f64 = 0.5;

type Histogram = [f64; 256];
type Lut = [f64; 256];

pub struct ToneRestore;

impl Filter for ToneRestore {
    fn info(&self) -> FilterInfo {
        FilterInfo {
            id: ID.into(),
            name: Text::en("Tone restore").with("zh-TW", "階調還原"),
            description: Text::en(
                "Matches the result's brightness distribution to the picture the engine received, restoring \
                 contrast that upscaling flattened. Without a reference it applies a gentle S-curve. In videos \
                 the curve is blended across frames to avoid flicker.",
            )
            .with("zh-TW", "讓成果的亮度分布貼近送進引擎的原圖, 拉回放大時被壓平的對比與暗部. 沒有參考圖時改用和緩的 S 曲線. 影片會跨幀平滑曲線, 避免閃爍."),
            stages: vec![FilterStage::Post],
            uses_reference: true,
        }
    }

    fn params(&self) -> Vec<ParamSpec> {
        vec![
            float(
                "strength",
                ("Strength", "強度"),
                ("0 keeps the result as is, 1 matches the reference fully.", "0 = 保留成果, 1 = 完全貼齊參考圖的階調."),
                (0.0, 1.0),
                0.65,
            ),
            advanced(float(
                "temporal",
                ("Temporal smoothing", "時間平滑"),
                (
                    "Video only: weight of the previous frame's curve. Restarts at scene cuts.",
                    "僅影片: 沿用前一幀曲線的比重, 遇到場景切換會重新開始.",
                ),
                (0.0, 0.95),
                0.5,
            )),
        ]
    }

    fn start(&self, params: &ParamValues, setup: FilterSetup) -> Result<Box<dyn FilterRun>> {
        let temporal = match setup.kind {
            MediaKind::Video => float_param(params, "temporal") as f64,
            MediaKind::Image => 0.0,
        };
        Ok(Box::new(ToneRun { strength: float_param(params, "strength"), temporal, previous: None }))
    }
}

struct ToneRun {
    strength: f32,
    temporal: f64,
    /// Curve and normalised reference histogram of the previous frame.
    previous: Option<(Lut, Histogram)>,
}

impl FilterRun for ToneRun {
    fn apply(&mut self, frame: &mut Frame, reference: Option<&Frame>) -> Result<()> {
        if self.strength <= 0.0 {
            return Ok(());
        }
        let w = self.strength;
        match reference {
            Some(reference) => {
                let (source, target) = (lightness_histogram(frame), lightness_histogram(reference));
                let (Some(source), Some(target)) = (source, target) else { return Ok(()) };
                let mut lut = smooth(&match_cdf(&source, &target));
                let target = normalised(&target);
                if let Some((prev_lut, prev_target)) = &self.previous {
                    let distance: f64 = prev_target.iter().zip(&target).map(|(a, b)| (a - b).abs()).sum();
                    if self.temporal > 0.0 && distance < SCENE_CUT {
                        for (v, p) in lut.iter_mut().zip(prev_lut) {
                            *v = self.temporal * p + (1.0 - self.temporal) * *v;
                        }
                    }
                }
                self.previous = Some((lut, target));
                remap(frame, |l, alpha| {
                    if alpha > MASK_ALPHA { (1.0 - w) * l + w * interpolate(&lut, l) } else { l }
                });
            }
            None => remap(frame, |l, alpha| {
                if alpha > MASK_ALPHA { (1.0 - w * 0.5) * l + w * 0.5 * s_curve(l) } else { l }
            }),
        }
        Ok(())
    }
}

/// A tanh S-curve over 0..=255 that keeps black and white in place. (The
/// prototype's curve was not normalised, so it lifted blacks and dimmed
/// whites by about 25 levels, the opposite of restoring contrast.)
fn s_curve(l: f32) -> f32 {
    const K: f32 = 2.2;
    let t = (K * (l / 255.0 - 0.5)).tanh() / (K * 0.5).tanh();
    (t + 1.0) * 0.5 * 255.0
}

/// Rewrites the lightness of every visible pixel through `map(L, alpha)`.
fn remap(frame: &mut Frame, map: impl Fn(f32, u8) -> f32 + Sync) {
    frame.pixels.par_chunks_exact_mut(4).filter(|p| p[3] > 0).for_each(|px| {
        let [l, a, b] = to_lab([px[0], px[1], px[2]]);
        let mapped = map(l, px[3]).clamp(0.0, 255.0);
        if (mapped - l).abs() > 1e-3 {
            px[..3].copy_from_slice(&to_rgb([mapped, a, b]));
        }
    });
}

/// Lightness histogram (256 bins) of pixels more opaque than `MASK_ALPHA`;
/// `None` when no pixel qualifies. Bins are computed in parallel and counted
/// serially: a parallel fold would carry a 2 KB array through every level of
/// rayon's recursion, which overflows worker stacks in debug builds.
fn lightness_histogram(frame: &Frame) -> Option<Histogram> {
    let bins: Vec<Option<u8>> = frame
        .pixels
        .par_chunks_exact(4)
        .map(|p| (p[3] > MASK_ALPHA).then(|| to_lab([p[0], p[1], p[2]])[0].round().clamp(0.0, 255.0) as u8))
        .collect();
    let mut hist = [0.0f64; 256];
    for bin in bins.into_iter().flatten() {
        hist[bin as usize] += 1.0;
    }
    (hist.iter().sum::<f64>() > 0.0).then_some(hist)
}

fn normalised(hist: &Histogram) -> Histogram {
    let total: f64 = hist.iter().sum::<f64>().max(1.0);
    hist.map(|v| v / total)
}

fn cdf(hist: &Histogram) -> Histogram {
    let total: f64 = hist.iter().sum::<f64>().max(1.0);
    let mut acc = 0.0;
    hist.map(|v| {
        acc += v;
        acc / total
    })
}

/// For each source level, the target level at the same cumulative share
/// (`np.interp(s_cdf[i], t_cdf, arange(256))`).
fn match_cdf(source: &Histogram, target: &Histogram) -> Lut {
    let (s, t) = (cdf(source), cdf(target));
    s.map(|x| np_interp(x, &t))
}

/// `np.interp(x, xp, arange(len))` for non-decreasing `xp`.
fn np_interp(x: f64, xp: &[f64; 256]) -> f64 {
    let last = xp.len() - 1;
    if x < xp[0] {
        return 0.0;
    }
    if x > xp[last] {
        return last as f64;
    }
    let j = xp.partition_point(|v| *v <= x) - 1;
    if j == last || xp[j] == x {
        return j as f64;
    }
    j as f64 + (x - xp[j]) / (xp[j + 1] - xp[j])
}

/// `scipy.ndimage.gaussian_filter1d(lut, LUT_SIGMA)`: mode `reflect`, truncate 4.
fn smooth(lut: &Lut) -> Lut {
    let radius = (4.0 * LUT_SIGMA + 0.5) as isize;
    let weights: Vec<f64> = (-radius..=radius).map(|k| (-0.5 * (k * k) as f64 / (LUT_SIGMA * LUT_SIGMA)).exp()).collect();
    let total: f64 = weights.iter().sum();
    let n = lut.len() as isize;
    let reflect = |mut i: isize| loop {
        if i < 0 {
            i = -i - 1;
        } else if i >= n {
            i = 2 * n - 1 - i;
        } else {
            return i as usize;
        }
    };
    std::array::from_fn(|i| {
        weights.iter().enumerate().map(|(k, w)| w * lut[reflect(i as isize + k as isize - radius)]).sum::<f64>() / total
    })
}

/// `lut` evaluated at fractional level `l`.
fn interpolate(lut: &Lut, l: f32) -> f32 {
    let l = (l as f64).clamp(0.0, 255.0);
    let i = (l.floor() as usize).min(254);
    let t = l - i as f64;
    (lut[i] * (1.0 - t) + lut[i + 1] * t) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use ivsr_core::ParamValue;

    fn run(kind: MediaKind, strength: f64) -> Box<dyn FilterRun> {
        let params = ParamValues::resolve(
            &ToneRestore.params(),
            &[("strength".to_string(), ParamValue::Float(strength))].into_iter().collect(),
        )
        .unwrap();
        ToneRestore.start(&params, FilterSetup { stage: FilterStage::Post, kind }).unwrap()
    }

    /// Greys from `lo` to `hi` across the width.
    fn ramp(lo: f32, hi: f32, has_alpha: bool) -> Frame {
        let mut f = Frame::filled(64, 4, [0, 0, 0, 255], has_alpha);
        for (i, px) in f.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let v = (lo + (hi - lo) * (i % 64) as f32 / 63.0).round() as u8;
            px[..3].copy_from_slice(&[v, v, v]);
        }
        f
    }

    fn spread(f: &Frame) -> i32 {
        let greys: Vec<i32> = f.pixels.chunks(4).map(|p| p[0] as i32).collect();
        greys.iter().max().unwrap() - greys.iter().min().unwrap()
    }

    #[test]
    fn flattened_result_regains_the_reference_contrast() {
        let reference = ramp(10.0, 245.0, false);
        let mut flat = ramp(50.0, 200.0, false);
        run(MediaKind::Image, 1.0).apply(&mut flat, Some(&reference)).unwrap();
        assert!(spread(&flat) > 200, "spread {}", spread(&flat));
    }

    #[test]
    fn matching_a_picture_to_itself_changes_little() {
        let reference = ramp(10.0, 245.0, false);
        let mut same = reference.clone();
        run(MediaKind::Image, 0.65).apply(&mut same, Some(&reference)).unwrap();
        let worst = same.pixels.iter().zip(&reference.pixels).map(|(a, b)| (*a as i32 - *b as i32).abs()).max();
        assert!(worst.unwrap() <= 6, "{worst:?}");
    }

    #[test]
    fn without_reference_an_s_curve_adds_contrast_and_transparent_pixels_stay() {
        let mut f = ramp(60.0, 190.0, true);
        f.pixels[3] = 0;
        // A faint glow pixel: excluded with or without a reference.
        f.pixels[63 * 4 + 3] = 100;
        let before = f.clone();
        run(MediaKind::Image, 0.65).apply(&mut f, None).unwrap();
        assert!(spread(&f) > spread(&before));
        assert_eq!(&f.pixels[..4], &before.pixels[..4]);
        assert_eq!(&f.pixels[63 * 4..64 * 4], &before.pixels[63 * 4..64 * 4]);
    }

    #[test]
    fn video_curve_follows_previous_frame_until_a_scene_cut() {
        let reference = ramp(10.0, 245.0, false);
        let mut video = run(MediaKind::Video, 1.0);
        let mut first = ramp(50.0, 200.0, false);
        video.apply(&mut first, Some(&reference)).unwrap();
        // Same reference, but this frame was flattened differently: smoothing pulls it toward frame one.
        let mut second = ramp(80.0, 170.0, false);
        let mut alone = second.clone();
        video.apply(&mut second, Some(&reference)).unwrap();
        run(MediaKind::Video, 1.0).apply(&mut alone, Some(&reference)).unwrap();
        assert_ne!(second.pixels, alone.pixels);

        // A completely different reference is a cut: no blending with the old curve.
        let dark = ramp(0.0, 60.0, false);
        let mut after_cut = ramp(10.0, 50.0, false);
        let mut fresh = after_cut.clone();
        video.apply(&mut after_cut, Some(&dark)).unwrap();
        run(MediaKind::Video, 1.0).apply(&mut fresh, Some(&dark)).unwrap();
        assert_eq!(after_cut.pixels, fresh.pixels);
    }

    #[test]
    fn s_curve_keeps_end_points_and_steepens_mid_tones() {
        assert!(s_curve(0.0).abs() < 1e-3 && (s_curve(255.0) - 255.0).abs() < 1e-3);
        assert!(s_curve(64.0) < 64.0 && s_curve(191.0) > 191.0);
        assert!((s_curve(127.5) - 127.5).abs() < 1e-3);
    }

    #[test]
    fn interp_matches_numpy_on_flat_runs_and_bounds() {
        let mut xp = [1.0; 256];
        xp[..3].copy_from_slice(&[0.0, 0.0, 0.5]);
        assert_eq!(np_interp(0.0, &xp), 1.0, "last of the equal run");
        assert_eq!(np_interp(0.25, &xp), 1.5);
        assert_eq!(np_interp(1.0, &xp), 255.0);
        assert_eq!(np_interp(-0.1, &xp), 0.0);
    }
}

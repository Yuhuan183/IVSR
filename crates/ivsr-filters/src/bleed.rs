//! Edge colour bleed: recolours (semi-)transparent pixels from nearby solid
//! ones, so neither the viewer nor later spatial filters see the black that
//! cut-out images usually store under transparent pixels.

use ivsr_core::{Filter, FilterInfo, FilterRun, FilterSetup, FilterStage, Frame, ParamSpec, ParamValues, Result, Text};
use rayon::prelude::*;

use crate::param::{int, int_param};
use crate::plane::dilate_square5;

pub const ID: &str = "alpha-bleed";

/// Pixels whose alpha is above this count as solid by default.
pub const DEFAULT_THRESHOLD: u8 = 180;
pub const DEFAULT_DISTANCE: u32 = 24;

pub struct AlphaBleed;

impl Filter for AlphaBleed {
    fn info(&self) -> FilterInfo {
        FilterInfo {
            id: ID.into(),
            name: Text::en("Edge colour bleed").with("zh-TW", "透明邊緣色彩外推"),
            description: Text::en(
                "Recolours transparent and semi-transparent edge pixels from nearby solid colours, removing dark \
                 fringes around cut-out images. Soft glows and shadows are recoloured too; lower the threshold for those.",
            )
            .with("zh-TW", "以鄰近實體像素的顏色重新填入透明與半透明邊緣, 消除去背圖輪廓的深色髒邊. 柔光、陰影等半透明效果也會被改色, 這類圖請調低門檻."),
            stages: vec![FilterStage::Pre, FilterStage::Post],
            uses_reference: false,
        }
    }

    fn params(&self) -> Vec<ParamSpec> {
        vec![
            int(
                "threshold",
                ("Solid above", "實體門檻"),
                (
                    "Pixels with alpha at or below this are recoloured (0 = only fully transparent ones).",
                    "透明度小於等於此值的像素會重新填色 (0 = 只處理完全透明的像素).",
                ),
                (0, 254),
                DEFAULT_THRESHOLD as i64,
            ),
            int(
                "distance",
                ("Distance (px)", "外推距離 (px)"),
                ("How far colours spread from solid pixels.", "顏色從實體像素向外延伸的距離."),
                (1, 256),
                DEFAULT_DISTANCE as i64,
            ),
        ]
    }

    fn start(&self, params: &ParamValues, _: FilterSetup) -> Result<Box<dyn FilterRun>> {
        Ok(Box::new(BleedRun {
            threshold: int_param(params, "threshold") as u8,
            distance: int_param(params, "distance") as u32,
        }))
    }
}

struct BleedRun {
    threshold: u8,
    distance: u32,
}

impl FilterRun for BleedRun {
    fn apply(&mut self, frame: &mut Frame, _: Option<&Frame>) -> Result<()> {
        bleed(frame, self.threshold, self.distance);
        Ok(())
    }
}

/// Recolours pixels with alpha ≤ `threshold` within `distance` pixels of a
/// solid one. Each pass grows the filled area by the 5×5 neighbourhood
/// (2 px) and gives every new pixel the mean colour of the pixels around it
/// that are filled or at least partly visible, so semi-transparent colours
/// (glows) blend in. Fully transparent pixels not yet filled are left out:
/// the OpenCV prototype averaged them too and pulled their black inwards.
pub fn bleed(frame: &mut Frame, threshold: u8, distance: u32) {
    if !frame.has_alpha {
        return;
    }
    let (w, h) = (frame.width as usize, frame.height as usize);
    let mut known: Vec<bool> = frame.pixels.chunks_exact(4).map(|p| p[3] > threshold).collect();
    if known.iter().all(|k| *k) || !known.iter().any(|k| *k) {
        return;
    }
    let mut colour: Vec<[f32; 3]> = frame.pixels.chunks_exact(4).map(|p| [p[0], p[1], p[2]].map(f32::from)).collect();
    let visible: Vec<bool> = frame.pixels.chunks_exact(4).map(|p| p[3] > 0).collect();
    for _ in 0..distance.div_ceil(2) {
        let near = dilate_square5(&known, w, h);
        let frontier: Vec<usize> = (0..w * h).into_par_iter().filter(|&i| near[i] && !known[i]).collect();
        if frontier.is_empty() {
            break;
        }
        let fills: Vec<[f32; 3]> =
            frontier.par_iter().map(|&i| neighbourhood_mean(&colour, |j| known[j] || visible[j], w, h, i)).collect();
        for (&i, fill) in frontier.iter().zip(fills) {
            colour[i] = fill;
            known[i] = true;
        }
    }
    frame.pixels.par_chunks_exact_mut(4).zip(colour.par_iter()).for_each(|(px, c)| {
        if px[3] <= threshold {
            for (v, c) in px.iter_mut().zip(c) {
                *v = c.round().clamp(0.0, 255.0) as u8;
            }
        }
    });
}

fn neighbourhood_mean(colour: &[[f32; 3]], counts: impl Fn(usize) -> bool, w: usize, h: usize, i: usize) -> [f32; 3] {
    let (x, y) = (i % w, i / w);
    let mut sum = [0.0f32; 3];
    let mut count = 0.0f32;
    for ny in y.saturating_sub(2)..(y + 3).min(h) {
        for nx in x.saturating_sub(2)..(x + 3).min(w) {
            let j = ny * w + nx;
            if counts(j) {
                for (s, c) in sum.iter_mut().zip(colour[j]) {
                    *s += c;
                }
                count += 1.0;
            }
        }
    }
    sum.map(|s| s / count)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 9×1 strip: solid red on the left, then a 120-alpha edge, then black transparency.
    fn strip() -> Frame {
        let mut f = Frame::filled(9, 1, [0, 0, 0, 0], true);
        for x in 0..3 {
            f.pixels[x * 4..x * 4 + 4].copy_from_slice(&[200, 10, 10, 255]);
        }
        f.pixels[12..16].copy_from_slice(&[90, 90, 90, 120]);
        f
    }

    #[test]
    fn edge_pixels_blend_with_solid_colour_and_transparent_black_never_leaks_in() {
        let mut f = strip();
        bleed(&mut f, 180, 24);
        // x = 3 (alpha 120) averages two solid reds with its own grey.
        assert_eq!(&f.pixels[12..15], &[163, 37, 37]);
        // Transparent pixels mix red and the edge grey; their own black never enters
        // (the prototype's plain box blur would drag red below the grey's 90).
        for x in 4..9 {
            let px = &f.pixels[x * 4..x * 4 + 3];
            assert!(px[0] >= 140 && px[1] <= 55, "pixel {x}: {px:?}");
        }
        let alpha: Vec<u8> = f.pixels.chunks(4).map(|p| p[3]).collect();
        assert_eq!(alpha, strip().pixels.chunks(4).map(|p| p[3]).collect::<Vec<_>>(), "alpha untouched");
    }

    #[test]
    fn threshold_zero_keeps_visible_pixels_and_distance_limits_reach() {
        let mut f = strip();
        bleed(&mut f, 0, 2);
        assert_eq!(&f.pixels[12..15], &[90, 90, 90], "semi-transparent edge kept");
        // One pass reaches two pixels past the last visible one (x = 3).
        assert_ne!(&f.pixels[4 * 4..4 * 4 + 3], &[0, 0, 0]);
        assert_eq!(&f.pixels[6 * 4..6 * 4 + 3], &[0, 0, 0]);
    }

    #[test]
    fn opaque_frames_are_untouched() {
        let mut f = Frame::filled(4, 4, [1, 2, 3, 255], false);
        let before = f.clone();
        bleed(&mut f, 180, 24);
        assert_eq!(f, before);
    }
}

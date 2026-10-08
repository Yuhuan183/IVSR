//! sRGB <-> CIE L*a*b* (D65) in OpenCV's 8-bit scaling: L in 0..=255
//! (L* × 255/100), a* and b* offset by 128. The filters were prototyped with
//! OpenCV, and their thresholds keep their meaning in these units.

use std::sync::OnceLock;

const XN: f32 = 0.950456;
const ZN: f32 = 1.088754;
/// (6/29)^3 and its cube root: where the CIE curve switches to linear.
const EPSILON: f32 = 0.008856;
const EPSILON_CBRT: f32 = 0.206893;

fn linear_table() -> &'static [f32; 256] {
    static TABLE: OnceLock<[f32; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        std::array::from_fn(|i| {
            let v = i as f32 / 255.0;
            if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        })
    })
}

fn f(t: f32) -> f32 {
    if t > EPSILON { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 }
}

pub fn to_lab(rgb: [u8; 3]) -> [f32; 3] {
    let table = linear_table();
    let [r, g, b] = rgb.map(|c| table[c as usize]);
    let x = (0.412453 * r + 0.357580 * g + 0.180423 * b) / XN;
    let y = 0.212671 * r + 0.715160 * g + 0.072169 * b;
    let z = (0.019334 * r + 0.119193 * g + 0.950227 * b) / ZN;
    let (fx, fy, fz) = (f(x), f(y), f(z));
    let l = if y > EPSILON { 116.0 * fy - 16.0 } else { 903.3 * y };
    [l * 255.0 / 100.0, 500.0 * (fx - fy) + 128.0, 200.0 * (fy - fz) + 128.0]
}

pub fn to_rgb(lab: [f32; 3]) -> [u8; 3] {
    let l = lab[0] * 100.0 / 255.0;
    let (a, b) = (lab[1] - 128.0, lab[2] - 128.0);
    let (y, fy) = if l <= 8.0 {
        let y = l / 903.3;
        (y, 7.787 * y + 16.0 / 116.0)
    } else {
        let fy = (l + 16.0) / 116.0;
        (fy * fy * fy, fy)
    };
    let inv = |f: f32| if f > EPSILON_CBRT { f * f * f } else { (f - 16.0 / 116.0) / 7.787 };
    let x = inv(fy + a / 500.0) * XN;
    let z = inv(fy - b / 200.0) * ZN;
    let r = 3.240479 * x - 1.53715 * y - 0.498535 * z;
    let g = -0.969256 * x + 1.875991 * y + 0.041556 * z;
    let b = 0.055648 * x - 0.204043 * y + 1.057311 * z;
    [r, g, b].map(encode)
}

fn encode(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round().clamp(0.0, 255.0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_colours_match_opencv_8bit_lab() {
        // cv2.cvtColor(np.uint8([[[r, g, b]]]), cv2.COLOR_RGB2LAB)
        let cases: [([u8; 3], [u8; 3]); 6] = [
            ([255, 255, 255], [255, 128, 128]),
            ([0, 0, 0], [0, 128, 128]),
            ([255, 0, 0], [136, 208, 195]),
            ([40, 120, 200], [126, 133, 80]),
            ([1, 2, 3], [1, 128, 127]),
            ([250, 240, 10], [237, 112, 219]),
        ];
        for (rgb, expected) in cases {
            let lab = to_lab(rgb).map(|v| v.round() as i32);
            for (got, want) in lab.iter().zip(expected) {
                assert!((got - want as i32).abs() <= 1, "{rgb:?}: {lab:?} vs {expected:?}");
            }
        }
    }

    #[test]
    fn round_trip_is_lossless_for_every_grey_and_sampled_colours() {
        for v in 0..=255u8 {
            assert_eq!(to_rgb(to_lab([v, v, v])), [v, v, v]);
        }
        for r in (0..=255u8).step_by(17) {
            for g in (0..=255u8).step_by(17) {
                for b in (0..=255u8).step_by(17) {
                    assert_eq!(to_rgb(to_lab([r, g, b])), [r, g, b]);
                }
            }
        }
    }
}

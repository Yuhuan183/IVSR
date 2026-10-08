//! Single-channel float images and the neighbourhood operations the filters
//! need, with OpenCV's border conventions so results match the prototype.

use rayon::prelude::*;

pub struct Plane {
    pub width: usize,
    pub height: usize,
    pub data: Vec<f32>,
}

impl Plane {
    pub fn new(width: usize, height: usize, data: Vec<f32>) -> Self {
        debug_assert_eq!(data.len(), width * height);
        Self { width, height, data }
    }

    /// Gaussian blur as OpenCV's `GaussianBlur(src, (0, 0), sigma)` on float
    /// input: kernel size `round(8σ + 1) | 1`, `BORDER_REFLECT_101`.
    pub fn gaussian(&self, sigma: f32) -> Plane {
        let kernel = gaussian_kernel(sigma);
        let r = kernel.len() / 2;
        let (w, h) = (self.width, self.height);
        let mut tmp = vec![0.0f32; w * h];
        tmp.par_chunks_mut(w).enumerate().for_each(|(y, out)| {
            let row = &self.data[y * w..(y + 1) * w];
            for (x, o) in out.iter_mut().enumerate() {
                *o = kernel
                    .iter()
                    .enumerate()
                    .map(|(k, wt)| wt * row[reflect101(x as isize + k as isize - r as isize, w)])
                    .sum();
            }
        });
        let mut data = vec![0.0f32; w * h];
        data.par_chunks_mut(w).enumerate().for_each(|(y, out)| {
            for (k, wt) in kernel.iter().enumerate() {
                let src = reflect101(y as isize + k as isize - r as isize, h);
                for (o, v) in out.iter_mut().zip(&tmp[src * w..(src + 1) * w]) {
                    *o += wt * v;
                }
            }
        });
        Plane::new(w, h, data)
    }
}

fn gaussian_kernel(sigma: f32) -> Vec<f32> {
    let size = ((sigma * 8.0 + 1.0).round() as usize) | 1;
    let half = (size / 2) as f32;
    let raw: Vec<f32> = (0..size).map(|i| (-((i as f32 - half).powi(2)) / (2.0 * sigma * sigma)).exp()).collect();
    let sum: f32 = raw.iter().sum();
    raw.into_iter().map(|v| v / sum).collect()
}

/// `gfedcb|abcdefgh|gfedcba`
fn reflect101(i: isize, n: usize) -> usize {
    if n == 1 {
        return 0;
    }
    let n = n as isize;
    let mut i = i;
    loop {
        if i < 0 {
            i = -i;
        } else if i >= n {
            i = 2 * n - 2 - i;
        } else {
            return i as usize;
        }
    }
}

/// Erosion with OpenCV's 5×5 elliptical element (a plus-capped 5×3 block);
/// pixels outside the image count as set, so borders do not erode.
pub fn erode_ellipse5(mask: &[bool], w: usize, h: usize) -> Vec<bool> {
    let at = |x: isize, y: isize| x < 0 || y < 0 || x >= w as isize || y >= h as isize || mask[y as usize * w + x as usize];
    let mut out = vec![false; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let y = y as isize;
        for (x, o) in row.iter_mut().enumerate() {
            let x = x as isize;
            *o = at(x, y - 2) && at(x, y + 2) && (-1..=1).all(|dy| (-2..=2).all(|dx| at(x + dx, y + dy)));
        }
    });
    out
}

/// Dilation with a 5×5 square; pixels outside the image count as unset.
pub fn dilate_square5(mask: &[bool], w: usize, h: usize) -> Vec<bool> {
    let mut rows = vec![false; w * h];
    rows.par_chunks_mut(w).enumerate().for_each(|(y, out)| {
        let row = &mask[y * w..(y + 1) * w];
        for (x, o) in out.iter_mut().enumerate() {
            *o = row[x.saturating_sub(2)..(x + 3).min(w)].iter().any(|v| *v);
        }
    });
    let mut out = vec![false; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, out)| {
        for src in y.saturating_sub(2)..(y + 3).min(h) {
            for (o, v) in out.iter_mut().zip(&rows[src * w..(src + 1) * w]) {
                *o |= *v;
            }
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_matches_opencv() {
        // cv2.getGaussianKernel(9, 0.85)
        let expected = [1e-05, 0.00093, 0.02946, 0.23493, 0.46934, 0.23493, 0.02946, 0.00093, 1e-05];
        let kernel = gaussian_kernel(0.85);
        assert_eq!(kernel.len(), 9);
        for (k, e) in kernel.iter().zip(expected) {
            assert!((k - e).abs() < 1e-5, "{kernel:?}");
        }
        assert_eq!(gaussian_kernel(2.5).len(), 21);
        assert_eq!(gaussian_kernel(1.5).len(), 13);
    }

    #[test]
    fn blur_keeps_constants_and_mean_with_reflected_borders() {
        let flat = Plane::new(5, 3, vec![7.0; 15]);
        assert!(flat.gaussian(2.5).data.iter().all(|v| (v - 7.0).abs() < 1e-4));
        let tiny = Plane::new(1, 1, vec![3.0]);
        assert!((tiny.gaussian(1.5).data[0] - 3.0).abs() < 1e-5);
        assert_eq!(reflect101(-1, 4), 1);
        assert_eq!(reflect101(4, 4), 2);
        assert_eq!(reflect101(-9, 3), 1);
    }

    #[test]
    fn morphology_uses_the_elliptical_element_and_ignores_outside() {
        let (w, h) = (7, 7);
        let mut mask = vec![true; w * h];
        mask[3 * w + 3] = false;
        let eroded = erode_ellipse5(&mask, w, h);
        let off: Vec<(usize, usize)> = (0..w * h).filter(|i| !eroded[*i]).map(|i| (i % w, i / w)).collect();
        // The hole spreads to the element's shape; image borders stay set.
        assert_eq!(off.len(), 17);
        assert!(off.contains(&(3, 1)) && !off.contains(&(1, 1)) && off.contains(&(1, 2)));
        assert!(erode_ellipse5(&[true; 4], 2, 2).iter().all(|v| *v));

        let mut dot = vec![false; w * h];
        dot[0] = true;
        let grown = dilate_square5(&dot, w, h);
        assert_eq!(grown.iter().filter(|v| **v).count(), 9);
    }
}

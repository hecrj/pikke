/// A 0-1 RGBA color (straight alpha): linear light or sRGB, by context.
#[derive(Clone, Copy, Debug)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    /// A stored sRGB pixel (0-255) as a 0-1 sRGB color.
    pub fn from_rgba_u8(pixel: [u8; 4]) -> Self {
        Self {
            r: pixel[0] as f32 / 255.0,
            g: pixel[1] as f32 / 255.0,
            b: pixel[2] as f32 / 255.0,
            a: pixel[3] as f32 / 255.0,
        }
    }

    /// This color in linear light (the IEC 61966-2-1 decode of the
    /// channels); the alpha is untouched.
    pub fn srgb_to_linear(self) -> Self {
        Self {
            r: srgb_to_linear(self.r),
            g: srgb_to_linear(self.g),
            b: srgb_to_linear(self.b),
            a: self.a,
        }
    }

    /// This color in sRGB (the IEC 61966-2-1 encode of the channels);
    /// the alpha is untouched.
    pub fn linear_to_srgb(self) -> Self {
        Self {
            r: linear_to_srgb(self.r),
            g: linear_to_srgb(self.g),
            b: linear_to_srgb(self.b),
            a: self.a,
        }
    }

    /// The 8-bit stored pixel: each channel through the single encode
    /// rounding.
    pub fn to_rgba_u8(self) -> [u8; 4] {
        [
            encode_u8(self.r),
            encode_u8(self.g),
            encode_u8(self.b),
            encode_u8(self.a),
        ]
    }

    /// The buffer's premultiplied form.
    pub fn premultiply(self) -> Self {
        Self {
            r: self.r * self.a,
            g: self.g * self.a,
            b: self.b * self.a,
            a: self.a,
        }
    }

    /// The straight form from the buffer's premultiplied one. A fully
    /// transparent pixel's color is unrecoverable and reads as 0.
    pub fn demultiply(self) -> Self {
        if self.a == 0.0 {
            return Self {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            };
        }

        Self {
            r: self.r / self.a,
            g: self.g / self.a,
            b: self.b / self.a,
            a: self.a,
        }
    }

    /// Porter-Duff 'over' in the buffer's premultiplied form (the
    /// caller's domain: linear light or sRGB): no division, and a 0
    /// or 1 alpha on either side falls out of the formula exactly.
    pub fn over(self, layer: Color) -> Color {
        let retain = 1.0 - layer.a;

        Color {
            r: layer.r + self.r * retain,
            g: layer.g + self.g * retain,
            b: layer.b + self.b * retain,
            a: layer.a + self.a * retain,
        }
    }
}

/// 0-1 sRGB (stored) -> 0-1 linear light, the IEC 61966-2-1 curve.
pub fn srgb_to_linear(stored: f32) -> f32 {
    if stored <= 0.04045 {
        stored / 12.92
    } else {
        ((stored + 0.055) / 1.055).powf(2.4)
    }
}

/// 0-1 linear light -> 0-1 sRGB (stored), the IEC 61966-2-1 curve;
/// out-of-range linear clamps to 0-1 here, the encode path's only
/// clamp.
pub fn linear_to_srgb(linear: f32) -> f32 {
    let linear = linear.clamp(0.0, 1.0);
    if linear <= 0.0031308 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

/// The single encode rounding: a 0-1 stored value to 8 bits (nearest
/// 1/255, ties away from zero).
pub fn encode_u8(stored: f32) -> u8 {
    (stored * 255.0).round() as u8
}

/// The Rec. 709 luma of a stored sRGB pixel (0-255 channels), 0-255.
pub fn luma(pixel: [u8; 3]) -> f64 {
    0.2126 * pixel[0] as f64 + 0.7152 * pixel[1] as f64 + 0.0722 * pixel[2] as f64
}

/// The ITU-R 601 luma of a stored sRGB pixel — the stretch's histogram
/// channel.
pub fn luma601(pixel: [u8; 3]) -> u8 {
    (((pixel[0] as u32) * 19595 + (pixel[1] as u32) * 38470 + (pixel[2] as u32) * 7471 + 0x8000)
        >> 16) as u8
}

/// The tone-preserving stretch's point map: the stored 8-bit values
/// over [lo, hi] (the histogram's extent on the 601 luma channel)
/// mapped to [0, 255]; degenerate (hi <= lo) is the identity.
pub fn stretch_lut(lo: u32, hi: u32) -> [u8; 256] {
    let map: Vec<u8> = if hi <= lo {
        (0u8..=255).collect()
    } else {
        let scale = 255.0 / (hi - lo) as f64;
        let offset = -(lo as f64) * scale;
        (0u8..=255)
            .map(|value| ((value as f64) * scale + offset).trunc().clamp(0.0, 255.0) as u8)
            .collect()
    };
    map.try_into().unwrap()
}

/// The resize's resampling kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    Cubic,
    Lanczos,
}

/// The buffer's form (the filter over premultiplied light is
/// alpha-blind).
pub fn resize(
    filter: Filter,
    src: &[f32],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
) -> Vec<f32> {
    match filter {
        Filter::Lanczos => resample(lanczos3, 3.0, src, src_w, src_h, dst_w, dst_h),
        Filter::Cubic => resample_cubic(src, src_w, src_h, dst_w, dst_h),
    }
}

/// The window arithmetic (the support-scaled kernel
/// window, the weights renormalized per output sample); an unchanged
/// axis copies (the unit-scale kernel would blur it).
fn resample(
    kernel: impl Fn(f64) -> f64,
    support: f64,
    src: &[f32],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
) -> Vec<f32> {
    /// One separable pass: `src_len` samples, `src_stride` apart, in
    /// `src`, scaled to `dst_len` (`dst_stride` apart in `out`), the
    /// weights normalized per output sample; out-of-range taps are
    /// absent (the weights renormalize over the window that remains).
    fn pass(
        src: &[f32],
        src_stride: usize,
        out: &mut [f32],
        dst_stride: usize,
        src_len: usize,
        dst_len: usize,
        kernel: impl Fn(f64) -> f64,
        support: f64,
    ) {
        // Source samples per target sample; the filter scale
        // is the ratio on a downscale and 1 on an upscale (the window
        // stays at the kernel's width, the kernel argument unscaled).
        let scale = src_len as f64 / dst_len as f64;
        let filterscale = scale.max(1.0);
        let support = support * filterscale;

        for i in 0..dst_len {
            // The kernel's zero: the target sample's center in source
            // samples (the pixel-center convention, tap j at j + 0.5).
            let center = (i as f64 + 0.5) * scale;
            // The tap bounds: half-up, clamped to the source.
            let start = (center - support + 0.5).trunc().clamp(0.0, src_len as f64) as usize;
            let end = (center + support + 0.5).trunc().clamp(0.0, src_len as f64) as usize;

            let mut acc = 0.0f32;
            let mut weights = 0.0f32;
            for j in start..end {
                let w = kernel((j as f64 + 0.5 - center) / filterscale) as f32;
                acc += w * src[j * src_stride];
                weights += w;
            }
            out[i * dst_stride] = if weights == 0.0 { 0.0 } else { acc / weights };
        }
    }

    let (sw, sh, dw, dh) = (
        src_w as usize,
        src_h as usize,
        dst_w as usize,
        dst_h as usize,
    );

    let mut across = vec![0.0f32; dw * sh];
    if dw == sw {
        across.copy_from_slice(src);
    } else {
        for y in 0..sh {
            pass(
                &src[y * sw..(y + 1) * sw],
                1,
                &mut across[y * dw..(y + 1) * dw],
                1,
                sw,
                dw,
                &kernel,
                support,
            );
        }
    }

    let mut out = vec![0.0f32; dw * dh];
    if dh == sh {
        out.copy_from_slice(&across);
    } else {
        for x in 0..dw {
            pass(
                &across[x..],
                dw,
                &mut out[x..],
                dw,
                sh,
                dh,
                &kernel,
                support,
            );
        }
    }

    out
}

/// GIMP's resize as-is (GEGL's cubic sampler): the tap's fixed
/// 4-window, a downsample of `n >= 2` averaged at the `n` sub-block
/// centers (capped at 5), the out-of-range taps reading the abyss
/// (zero; the 4-window's weights sum to one — no renormalization).
/// An unchanged axis runs the tap (GIMP's; not a copy).
fn resample_cubic(src: &[f32], src_w: u32, src_h: u32, dst_w: u32, dst_h: u32) -> Vec<f32> {
    let (sw, sh, dw, dh) = (
        src_w as usize,
        src_h as usize,
        dst_w as usize,
        dst_h as usize,
    );

    let mut across = vec![0.0f32; dw * sh];
    for y in 0..sh {
        pass_cubic(
            &src[y * sw..(y + 1) * sw],
            1,
            &mut across[y * dw..(y + 1) * dw],
            1,
            sw,
            dw,
        );
    }

    let mut out = vec![0.0f32; dw * dh];
    for x in 0..dw {
        pass_cubic(&across[x..], dw, &mut out[x..], dw, sh, dh);
    }

    out
}

/// One separable pass in GIMP's window arithmetic: `src_len` samples
/// (`src_stride` apart) scaled to `dst_len` (`dst_stride` apart in
/// `out`): the downsample averages the tap at the sub-block centers,
/// the out-of-range taps read the abyss.
fn pass_cubic(
    src: &[f32],
    src_stride: usize,
    out: &mut [f32],
    dst_stride: usize,
    src_len: usize,
    dst_len: usize,
) {
    // The sub-blocks: the downscale factor, capped at 5 (GIMP's).
    let scale = src_len as f64 / dst_len as f64;
    let n = (scale as usize).clamp(1, 5);
    let u_dx = scale / n as f64;

    for i in 0..dst_len {
        // The sub-block centers: the target sample's source block,
        // the pixel-center convention (tap j at j + 0.5).
        let x0 = (i as f64 + 0.5) * scale - (scale - u_dx) / 2.0;
        let mut acc = 0.0f32;
        for j in 0..n {
            // The tap's zero: the sub-block center on the source
            // pixel grid, `ix` the top-left tap's pixel.
            let shifted = x0 + j as f64 * u_dx - 0.5;
            let ix = shifted.floor() as i64;
            let frac = shifted - ix as f64;
            for d in -1..=2i64 {
                let t = ix + d;
                if (0..src_len as i64).contains(&t) {
                    acc += cubic(frac - d as f64) as f32 * src[t as usize * src_stride];
                }
            }
        }
        out[i * dst_stride] = acc / n as f32;
    }
}

/// The Lanczos-3 kernel: the 3-lobed sinc, zero at the window's edge.
fn lanczos3(x: f64) -> f64 {
    if x.abs() >= 3.0 {
        return 0.0;
    }
    if x == 0.0 {
        return 1.0;
    }
    let pi_x = std::f64::consts::PI * x;
    let pi_x3 = pi_x / 3.0;
    (pi_x.sin() / pi_x) * (pi_x3.sin() / pi_x3)
}

/// GIMP's resize kernel (GEGL's cubic sampler: the BC-spline
/// b = 1/2, c = 1/4), zero outside ±2.
fn cubic(x: f64) -> f64 {
    let x = if x < 0.0 { -x } else { x };
    if x < 1.0 {
        ((12.0 * x - 21.0) * x * x + 10.0) / 12.0
    } else if x < 2.0 {
        (((-4.0 * x + 21.0) * x - 36.0) * x + 20.0) / 12.0
    } else {
        0.0
    }
}

/// The `out_w`×`out_h` region `left`/`top` into a `src_w`-wide
/// row-major plane.
pub fn crop(src: &[f32], src_w: u32, left: u32, top: u32, out_w: u32, out_h: u32) -> Vec<f32> {
    let (sw, ow, oh) = (src_w as usize, out_w as usize, out_h as usize);
    let (left, top) = (left as usize, top as usize);
    let mut out = vec![0.0f32; ow * oh];
    for y in 0..oh {
        let row = (top + y) * sw + left;
        out[y * ow..(y + 1) * ow].copy_from_slice(&src[row..row + ow]);
    }
    out
}

#[cfg(test)]
mod kernels {
    use super::{cubic, lanczos3};

    #[test]
    fn the_kernels_match_their_defining_values() {
        assert_eq!(lanczos3(0.0), 1.0);
        // GIMP's cubic's exact grid values (a polynomial, the sinc is
        // not) and its lobe (the fringe at GIMP's level).
        assert_eq!(cubic(0.0), 5.0 / 6.0);
        assert_eq!(cubic(1.0), 1.0 / 12.0);
        assert_eq!(cubic(1.5), -1.0 / 48.0);
        for x in [2.0, 3.0] {
            assert_eq!(cubic(x), 0.0);
            assert_eq!(cubic(-x), 0.0);
        }
        // The partition of unity over the integer shifts: the 4-tap's
        // weights sum to one (why GIMP's arithmetic renormalizes
        // nothing).
        for i in 0..20 {
            let x = i as f64 / 20.0;
            let sum: f64 = (-3..=3).map(|d| cubic(x - d as f64)).sum();
            assert!((sum - 1.0).abs() < 1e-12);
        }
    }
}

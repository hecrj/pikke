#[derive(Clone, Copy, Debug)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub fn from_rgba_u8(pixel: [u8; 4]) -> Self {
        Self {
            r: pixel[0] as f32 / 255.0,
            g: pixel[1] as f32 / 255.0,
            b: pixel[2] as f32 / 255.0,
            a: pixel[3] as f32 / 255.0,
        }
    }

    pub fn srgb_to_linear(self) -> Self {
        Self {
            r: srgb_to_linear(self.r),
            g: srgb_to_linear(self.g),
            b: srgb_to_linear(self.b),
            a: self.a,
        }
    }

    pub fn linear_to_srgb(self) -> Self {
        Self {
            r: linear_to_srgb(self.r),
            g: linear_to_srgb(self.g),
            b: linear_to_srgb(self.b),
            a: self.a,
        }
    }

    pub fn to_rgba_u8(self) -> [u8; 4] {
        [
            encode_u8(self.r),
            encode_u8(self.g),
            encode_u8(self.b),
            encode_u8(self.a),
        ]
    }

    pub fn premultiply(self) -> Self {
        Self {
            r: self.r * self.a,
            g: self.g * self.a,
            b: self.b * self.a,
            a: self.a,
        }
    }

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

pub fn srgb_to_linear(stored: f32) -> f32 {
    if stored <= 0.04045 {
        stored / 12.92
    } else {
        ((stored + 0.055) / 1.055).powf(2.4)
    }
}

pub fn linear_to_srgb(linear: f32) -> f32 {
    let linear = linear.clamp(0.0, 1.0);
    if linear <= 0.0031308 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

pub fn encode_u8(stored: f32) -> u8 {
    (stored * 255.0).round() as u8
}

const LUMA_R: f64 = 0.2126;
const LUMA_G: f64 = 0.7152;
const LUMA_B: f64 = 0.0722;

pub fn luma(pixel: [u8; 3]) -> f64 {
    LUMA_R * pixel[0] as f64 + LUMA_G * pixel[1] as f64 + LUMA_B * pixel[2] as f64
}

pub fn luma_linear(r: f32, g: f32, b: f32) -> f32 {
    LUMA_R as f32 * r + LUMA_G as f32 * g + LUMA_B as f32 * b
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    Cubic,
    Lanczos,
}

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

fn resample(
    kernel: impl Fn(f64) -> f64,
    support: f64,
    src: &[f32],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
) -> Vec<f32> {
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
        let scale = src_len as f64 / dst_len as f64;
        let filterscale = scale.max(1.0);
        let support = support * filterscale;

        for i in 0..dst_len {
            let center = (i as f64 + 0.5) * scale;
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

fn pass_cubic(
    src: &[f32],
    src_stride: usize,
    out: &mut [f32],
    dst_stride: usize,
    src_len: usize,
    dst_len: usize,
) {
    let scale = src_len as f64 / dst_len as f64;
    let n = (scale as usize).clamp(1, 5);
    let u_dx = scale / n as f64;

    for i in 0..dst_len {
        let x0 = (i as f64 + 0.5) * scale - (scale - u_dx) / 2.0;
        let mut acc = 0.0f32;
        for j in 0..n {
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
        assert_eq!(cubic(0.0), 5.0 / 6.0);
        assert_eq!(cubic(1.0), 1.0 / 12.0);
        assert_eq!(cubic(1.5), -1.0 / 48.0);
        for x in [2.0, 3.0] {
            assert_eq!(cubic(x), 0.0);
            assert_eq!(cubic(-x), 0.0);
        }
        for i in 0..20 {
            let x = i as f64 / 20.0;
            let sum: f64 = (-3..=3).map(|d| cubic(x - d as f64)).sum();
            assert!((sum - 1.0).abs() < 1e-12);
        }
    }
}

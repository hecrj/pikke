use std::marker::PhantomData;

use mlua::{UserData, UserDataMethods, Value};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color<Space = Linear, Alpha = Normal> {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
    _space: PhantomData<Space>,
    _alpha: PhantomData<Alpha>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Linear;
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Srgb;
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Normal;
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Premultiplied;

impl<S, N> Color<S, N> {
    pub const TRANSPARENT: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 0.0,
        _space: PhantomData,
        _alpha: PhantomData,
    };
}

impl Color<Srgb> {
    pub const fn from_rgba8(pixel: [u8; 4]) -> Self {
        Self {
            r: pixel[0] as f32 / 255.0,
            g: pixel[1] as f32 / 255.0,
            b: pixel[2] as f32 / 255.0,
            a: pixel[3] as f32 / 255.0,
            _space: PhantomData,
            _alpha: PhantomData,
        }
    }

    pub fn to_linear(self) -> Color<Linear> {
        Color {
            r: srgb_to_linear(self.r),
            g: srgb_to_linear(self.g),
            b: srgb_to_linear(self.b),
            a: self.a,
            _space: PhantomData,
            _alpha: PhantomData,
        }
    }

    pub const fn to_rgba8(self) -> [u8; 4] {
        [
            encode_u8(self.r),
            encode_u8(self.g),
            encode_u8(self.b),
            encode_u8(self.a),
        ]
    }
}

impl Color {
    pub fn with_luma(self, luma: f32) -> Self {
        let luma = luma.clamp(0.0, 1.0);
        let current = self.premultiply().luma();

        if current == 0.0 {
            return Self {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                ..self
            };
        }

        let scale = luma / current;

        Self {
            r: self.r * scale,
            g: self.g * scale,
            b: self.b * scale,
            ..self
        }
    }

    pub fn to_srgb(self) -> Color<Srgb> {
        Color {
            r: linear_to_srgb(self.r),
            g: linear_to_srgb(self.g),
            b: linear_to_srgb(self.b),
            a: self.a,
            _space: PhantomData,
            _alpha: PhantomData,
        }
    }
}

impl<S> Color<S> {
    pub const fn premultiply(self) -> Color<S, Premultiplied> {
        Color {
            r: self.r * self.a,
            g: self.g * self.a,
            b: self.b * self.a,
            a: self.a,
            _space: PhantomData,
            _alpha: PhantomData,
        }
    }

    pub const fn invert(self) -> Self {
        Self {
            r: invert(self.r),
            g: invert(self.g),
            b: invert(self.b),
            a: self.a,
            _space: PhantomData,
            _alpha: PhantomData,
        }
    }
}

impl<S> Color<S, Premultiplied> {
    pub const fn demultiply(self) -> Color<S> {
        if self.a == 0.0 {
            return Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
                _space: PhantomData,
                _alpha: PhantomData,
            };
        }

        Color {
            r: self.r / self.a,
            g: self.g / self.a,
            b: self.b / self.a,
            a: self.a,
            _space: PhantomData,
            _alpha: PhantomData,
        }
    }

    pub const fn over(self, layer: Self) -> Self {
        let retain = 1.0 - layer.a;

        Self {
            r: layer.r + self.r * retain,
            g: layer.g + self.g * retain,
            b: layer.b + self.b * retain,
            a: layer.a + self.a * retain,
            _space: PhantomData,
            _alpha: PhantomData,
        }
    }
}

impl Color<Linear, Premultiplied> {
    pub fn from_rgba([r, g, b, a]: [f32; 4]) -> Self {
        Self {
            r,
            g,
            b,
            a,
            _space: PhantomData,
            _alpha: PhantomData,
        }
    }

    pub fn luma(&self) -> f32 {
        const LUMA_R: f64 = 0.2126;
        const LUMA_G: f64 = 0.7152;
        const LUMA_B: f64 = 0.0722;

        LUMA_R as f32 * self.r + LUMA_G as f32 * self.g + LUMA_B as f32 * self.b
    }

    pub fn scale(self, alpha: f32) -> Self {
        Self {
            r: self.r * alpha,
            g: self.g * alpha,
            b: self.b * alpha,
            a: self.a * alpha,
            _space: PhantomData,
            _alpha: PhantomData,
        }
    }
}

impl UserData for Color {
    fn add_methods<M: UserDataMethods<Color>>(methods: &mut M) {
        methods.add_method(
            "with_luma",
            |_lua, this, luma: Value| -> mlua::Result<Color> {
                let luma = match luma {
                    Value::Number(luma) => luma as f32,
                    Value::Integer(luma) => luma as f32,
                    _ => {
                        return Err(mlua::Error::RuntimeError(
                            "with_luma: 'luma' is not a number".to_owned(),
                        ));
                    }
                };

                if !luma.is_finite() {
                    return Err(mlua::Error::RuntimeError(
                        "with_luma: 'luma' must be finite".to_owned(),
                    ));
                }

                Ok(this.with_luma(luma))
            },
        );
    }
}

#[derive(Debug, Clone)]
pub struct Buffer {
    r: Vec<f32>,
    g: Vec<f32>,
    b: Vec<f32>,
    a: Vec<f32>,
}

impl Buffer {
    pub fn new(color: Color<Linear, Premultiplied>, count: usize) -> Self {
        Self {
            r: vec![color.r; count],
            g: vec![color.g; count],
            b: vec![color.b; count],
            a: vec![color.a; count],
        }
    }

    pub fn from_rgba8(rgba: &[u8]) -> Self {
        let count = rgba.len() / 4;

        let (mut r, mut g, mut b, mut a) = (
            Vec::with_capacity(count),
            Vec::with_capacity(count),
            Vec::with_capacity(count),
            Vec::with_capacity(count),
        );

        for pixel in rgba.as_chunks::<4>().0 {
            let color = Color::from_rgba8(*pixel).to_linear().premultiply();

            r.push(color.r);
            g.push(color.g);
            b.push(color.b);
            a.push(color.a);
        }

        Self { r, g, b, a }
    }

    pub fn at(&self, i: usize) -> Color<Linear, Premultiplied> {
        Color::from_rgba([self.r[i], self.g[i], self.b[i], self.a[i]])
    }

    pub fn luma(&self) -> (f32, f32) {
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        let count = self.r.len();

        for i in 0..count {
            let luma = self.at(i).luma();
            lo = lo.min(luma);
            hi = hi.max(luma);
        }

        (lo, hi)
    }

    pub fn resize(&mut self, filter: Filter, width: u16, height: u16, box_w: u16, box_h: u16) {
        self.r = resize(filter, &self.r, width, height, box_w, box_h);
        self.g = resize(filter, &self.g, width, height, box_w, box_h);
        self.b = resize(filter, &self.b, width, height, box_w, box_h);
        self.a = resize(filter, &self.a, width, height, box_w, box_h);
    }

    pub fn crop(&mut self, box_w: u16, left: u16, top: u16, target_w: u16, target_h: u16) {
        self.r = crop(&self.r, box_w, left, top, target_w, target_h);
        self.g = crop(&self.g, box_w, left, top, target_w, target_h);
        self.b = crop(&self.b, box_w, left, top, target_w, target_h);
        self.a = crop(&self.a, box_w, left, top, target_w, target_h);
    }

    pub fn clamp_alpha(&mut self) {
        for value in self.a.iter_mut() {
            *value = value.clamp(0.0, 1.0);
        }
    }

    pub fn invert(&mut self) {
        let count = self.r.len();

        for i in 0..count {
            let Color { r, g, b, .. } = self.at(i).demultiply().invert().premultiply();

            self.r[i] = r;
            self.g[i] = g;
            self.b[i] = b;
        }
    }

    pub fn put(&mut self, i: usize, color: Color<Linear, Premultiplied>) {
        self.r[i] = color.r;
        self.g[i] = color.g;
        self.b[i] = color.b;
        self.a[i] = color.a;
    }

    pub fn apply(
        &mut self,
        f: impl Fn(Color<Linear, Premultiplied>) -> Color<Linear, Premultiplied>,
    ) {
        let count = self.r.len();

        for i in 0..count {
            let Color { r, g, b, a, .. } = f(self.at(i));

            self.r[i] = r;
            self.g[i] = g;
            self.b[i] = b;
            self.a[i] = a;
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

pub const fn invert(color: f32) -> f32 {
    1.0 - color
}

pub const fn encode_u8(stored: f32) -> u8 {
    (stored * 255.0).round() as u8
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    Cubic,
    Lanczos,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Space {
    #[default]
    Linear,
    Srgb,
}

pub fn resize(
    filter: Filter,
    src: &[f32],
    src_w: u16,
    src_h: u16,
    dst_w: u16,
    dst_h: u16,
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
    src_w: u16,
    src_h: u16,
    dst_w: u16,
    dst_h: u16,
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

fn resample_cubic(src: &[f32], src_w: u16, src_h: u16, dst_w: u16, dst_h: u16) -> Vec<f32> {
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
                let t = (ix + d).clamp(0, (src_len - 1) as i64) as usize;
                acc += cubic(frac - d as f64) as f32 * src[t * src_stride];
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

pub fn crop(src: &[f32], src_w: u16, left: u16, top: u16, out_w: u16, out_h: u16) -> Vec<f32> {
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

#[cfg(test)]
mod with_luma {
    use super::Color;

    #[test]
    fn the_with_luma_hits_the_target_and_keeps_the_ratios() {
        let color = Color::from_rgba([0.5, 0.25, 0.125, 1.0]).demultiply();
        let result = color.with_luma(0.1);

        assert!((result.premultiply().luma() - 0.1).abs() < 1e-6);
        assert_eq!(result.g, result.r / 2.0);
        assert_eq!(result.b, result.r / 4.0);
        assert_eq!(result.a, 1.0);
    }

    #[test]
    fn the_with_luma_keeps_black_black() {
        let black = Color::from_rgba([0.0, 0.0, 0.0, 1.0]).demultiply();

        assert_eq!(black.with_luma(0.5), black);
    }

    #[test]
    fn the_with_luma_keeps_the_channels_unclamped_until_the_srgb_encode() {
        let result = Color::from_rgba([0.0, 0.0, 1.0, 1.0])
            .demultiply()
            .with_luma(0.3);

        assert!(result.b > 1.0);
        assert_eq!(result.to_srgb().to_rgba8(), [0, 0, 255, 255]);
    }

    #[test]
    fn the_with_luma_clamps_the_target_to_zero_and_one() {
        let white = Color::from_rgba([1.0, 1.0, 1.0, 1.0]).demultiply();

        assert_eq!(white.with_luma(-1.0).r, 0.0);
        assert_eq!(white.with_luma(2.0), white);
    }

    #[test]
    fn the_with_luma_binding_scales_a_color() {
        let lua = mlua::Lua::new();
        let color = lua
            .create_userdata(Color::from_rgba8([128, 128, 128, 255]).to_linear())
            .unwrap();

        let env = lua.create_table().unwrap();
        env.set("color", color).unwrap();

        let result: mlua::AnyUserData = lua
            .load("color:with_luma(0.0)")
            .set_environment(env.clone())
            .eval()
            .unwrap();
        let result = *result.borrow::<Color>().unwrap();
        assert_eq!(result.to_srgb().to_rgba8(), [0, 0, 0, 255]);

        let result: mlua::AnyUserData = lua
            .load("color:with_luma(1.0)")
            .set_environment(env.clone())
            .eval()
            .unwrap();
        let result = *result.borrow::<Color>().unwrap();
        assert_eq!(result.to_srgb().to_rgba8(), [255, 255, 255, 255]);

        let result: mlua::AnyUserData = lua
            .load("color:with_luma(0.215744)")
            .set_environment(env.clone())
            .eval()
            .unwrap();
        let result = *result.borrow::<Color>().unwrap();
        assert_eq!(result.to_srgb().to_rgba8(), [128, 128, 128, 255]);

        let error = lua
            .load("color:with_luma('x')")
            .set_environment(env.clone())
            .eval::<()>()
            .unwrap_err()
            .to_string();
        assert!(error.contains("with_luma: 'luma' is not a number"));

        env.set("huge", f64::INFINITY).unwrap();
        let error = lua
            .load("color:with_luma(huge)")
            .set_environment(env)
            .eval::<()>()
            .unwrap_err()
            .to_string();
        assert!(error.contains("with_luma: 'luma' must be finite"));
    }
}

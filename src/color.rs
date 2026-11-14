use std::marker::PhantomData;

use mlua::{Lua, Table, UserData, UserDataFields, UserDataMethods, Value};

use crate::lua;

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
    pub const fn from_pixel(pixel: Pixel) -> Self {
        Self {
            r: pixel.r as f32 / 255.0,
            g: pixel.g as f32 / 255.0,
            b: pixel.b as f32 / 255.0,
            a: pixel.a as f32 / 255.0,
            _space: PhantomData,
            _alpha: PhantomData,
        }
    }

    pub const fn from_rgba8(rgba: [u8; 4]) -> Self {
        Self::from_pixel(Pixel::from_rgba8(rgba))
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

    pub const fn to_pixel(self) -> Pixel {
        Pixel {
            r: encode_u8(self.r),
            g: encode_u8(self.g),
            b: encode_u8(self.b),
            a: encode_u8(self.a),
        }
    }

    pub const fn to_rgba8(self) -> [u8; 4] {
        self.to_pixel().to_rgba8()
    }
}

impl Color {
    pub fn with_alpha(self, alpha: f32) -> Self {
        Self { a: alpha, ..self }
    }

    pub(crate) fn mix(lower: Color, upper: Color, weight: f32) -> Color {
        Color {
            r: lower.r + (upper.r - lower.r) * weight,
            g: lower.g + (upper.g - lower.g) * weight,
            b: lower.b + (upper.b - lower.b) * weight,
            a: lower.a + (upper.a - lower.a) * weight,
            _space: PhantomData,
            _alpha: PhantomData,
        }
    }

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

    pub fn to_oklch(self) -> Oklch {
        const LMS_L_R: f64 = 0.4122214708;
        const LMS_L_G: f64 = 0.5363325363;
        const LMS_L_B: f64 = 0.0514459929;
        const LMS_M_R: f64 = 0.2119034982;
        const LMS_M_G: f64 = 0.6806995451;
        const LMS_M_B: f64 = 0.1073969566;
        const LMS_S_R: f64 = 0.0883024619;
        const LMS_S_G: f64 = 0.2817188376;
        const LMS_S_B: f64 = 0.6299787005;
        const OKLAB_L_L: f64 = 0.2104542553;
        const OKLAB_L_M: f64 = 0.7936177850;
        const OKLAB_L_S: f64 = -0.0040720468;
        const OKLAB_A_L: f64 = 1.9779984951;
        const OKLAB_A_M: f64 = -2.4285922050;
        const OKLAB_A_S: f64 = 0.4505937099;
        const OKLAB_B_L: f64 = 0.0259040371;
        const OKLAB_B_M: f64 = 0.7827717662;
        const OKLAB_B_S: f64 = -0.8086757660;

        let l = LMS_L_R as f32 * self.r + LMS_L_G as f32 * self.g + LMS_L_B as f32 * self.b;
        let m = LMS_M_R as f32 * self.r + LMS_M_G as f32 * self.g + LMS_M_B as f32 * self.b;
        let s = LMS_S_R as f32 * self.r + LMS_S_G as f32 * self.g + LMS_S_B as f32 * self.b;

        let l = l.cbrt();
        let m = m.cbrt();
        let s = s.cbrt();

        let lightness = OKLAB_L_L as f32 * l + OKLAB_L_M as f32 * m + OKLAB_L_S as f32 * s;
        let a = OKLAB_A_L as f32 * l + OKLAB_A_M as f32 * m + OKLAB_A_S as f32 * s;
        let b = OKLAB_B_L as f32 * l + OKLAB_B_M as f32 * m + OKLAB_B_S as f32 * s;

        let chroma = (a * a + b * b).sqrt();
        let hue = b.atan2(a) * 180.0 / std::f32::consts::PI;

        Oklch {
            l: lightness,
            c: chroma,
            h: if hue < 0.0 { hue + 360.0 } else { hue },
            a: self.a,
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

pub fn install(lua: &Lua, env: &Table) -> crate::Result<()> {
    env.set(
        "srgb",
        lua.create_function(
            |_lua, (r, g, b, a): (f32, f32, f32, Option<f32>)| -> mlua::Result<Color> {
                let color = Color {
                    r,
                    g,
                    b,
                    a: a.unwrap_or(1.0),
                    _space: PhantomData,
                    _alpha: PhantomData,
                };

                Ok(color.to_linear())
            },
        )?,
    )?;

    Ok(())
}

impl UserData for Color {
    fn add_methods<M: UserDataMethods<Color>>(methods: &mut M) {
        methods.add_method(
            "with_luma",
            |_lua, this, luma: Value| -> mlua::Result<Color> {
                let luma = lua::number("with_luma", "luma", Some(luma))?;

                if !luma.is_finite() {
                    return Err(mlua::Error::RuntimeError(
                        "with_luma: 'luma' must be finite".to_owned(),
                    ));
                }

                Ok(this.with_luma(luma))
            },
        );

        methods.add_method(
            "with_alpha",
            |_lua, this, alpha: Value| -> mlua::Result<Color> {
                let alpha = lua::number("with_alpha", "alpha", Some(alpha))?;

                if !alpha.is_finite() {
                    return Err(mlua::Error::RuntimeError(
                        "with_luma: 'alpha' must be finite".to_owned(),
                    ));
                }

                Ok(this.with_alpha(alpha))
            },
        );

        methods.add_method("to_oklch", |_lua, this, _args: ()| -> mlua::Result<Oklch> {
            Ok(this.to_oklch())
        });
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pixel {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Pixel {
    pub const fn from_rgba8([r, g, b, a]: [u8; 4]) -> Self {
        Self { r, g, b, a }
    }

    pub const fn to_rgba8(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }
}

impl UserData for Pixel {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field("__name", "Pixel");

        fields.add_field_method_get("r", |_lua, this| -> mlua::Result<u8> { Ok(this.r) });
        fields.add_field_method_get("g", |_lua, this| -> mlua::Result<u8> { Ok(this.g) });
        fields.add_field_method_get("b", |_lua, this| -> mlua::Result<u8> { Ok(this.b) });
        fields.add_field_method_get("a", |_lua, this| -> mlua::Result<u8> { Ok(this.a) });
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Oklch {
    pub l: f32,
    pub c: f32,
    pub h: f32,
    pub a: f32,
}

impl Oklch {
    pub fn to_linear_rgb(self) -> Color {
        const LMS_L_A: f64 = 0.3963377774;
        const LMS_L_B: f64 = 0.2158037573;
        const LMS_M_A: f64 = -0.1055613458;
        const LMS_M_B: f64 = -0.0638541728;
        const LMS_S_A: f64 = -0.0894841775;
        const LMS_S_B: f64 = -1.2914855480;
        const RGB_R_L: f64 = 4.0767416621;
        const RGB_R_M: f64 = -3.3077115913;
        const RGB_R_S: f64 = 0.2309699292;
        const RGB_G_L: f64 = -1.2684380046;
        const RGB_G_M: f64 = 2.6097574011;
        const RGB_G_S: f64 = -0.3413193965;
        const RGB_B_L: f64 = -0.0041960863;
        const RGB_B_M: f64 = -0.7034186147;
        const RGB_B_S: f64 = 1.7076147010;

        let hue = self.h * std::f32::consts::PI / 180.0;
        let a = self.c * hue.cos();
        let b = self.c * hue.sin();

        let l = self.l + LMS_L_A as f32 * a + LMS_L_B as f32 * b;
        let m = self.l + LMS_M_A as f32 * a + LMS_M_B as f32 * b;
        let s = self.l + LMS_S_A as f32 * a + LMS_S_B as f32 * b;

        let l = l * l * l;
        let m = m * m * m;
        let s = s * s * s;

        Color {
            r: RGB_R_L as f32 * l + RGB_R_M as f32 * m + RGB_R_S as f32 * s,
            g: RGB_G_L as f32 * l + RGB_G_M as f32 * m + RGB_G_S as f32 * s,
            b: RGB_B_L as f32 * l + RGB_B_M as f32 * m + RGB_B_S as f32 * s,
            a: self.a,
            _space: PhantomData,
            _alpha: PhantomData,
        }
    }
}

impl UserData for Oklch {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field("__name", "Oklch");

        fields.add_field_method_get("l", |_lua, this| -> mlua::Result<f32> { Ok(this.l) });
        fields.add_field_method_get("c", |_lua, this| -> mlua::Result<f32> { Ok(this.c) });
        fields.add_field_method_get("h", |_lua, this| -> mlua::Result<f32> { Ok(this.h) });
        fields.add_field_method_get("a", |_lua, this| -> mlua::Result<f32> { Ok(this.a) });

        fields.add_field_method_set("l", |_lua, this, value: Value| -> mlua::Result<()> {
            this.l = lua::number("set", "l", Some(value))?;
            Ok(())
        });
        fields.add_field_method_set("c", |_lua, this, value: Value| -> mlua::Result<()> {
            this.c = lua::number("set", "c", Some(value))?;
            Ok(())
        });
        fields.add_field_method_set("h", |_lua, this, value: Value| -> mlua::Result<()> {
            this.h = lua::number("set", "h", Some(value))?;
            Ok(())
        });
        fields.add_field_method_set("a", |_lua, this, value: Value| -> mlua::Result<()> {
            this.a = lua::number("set", "a", Some(value))?;
            Ok(())
        });
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
}

#[cfg(test)]
mod oklch {
    use super::{Color, Oklch};

    #[test]
    fn the_oklch_to_linear_rgb_matches_the_oklab_reference() {
        let color = Oklch {
            l: 0.7,
            c: 0.15,
            h: 260.0,
            a: 1.0,
        }
        .to_linear_rgb();

        assert!((color.r - 0.1300774).abs() < 1e-5);
        assert!((color.g - 0.3385161).abs() < 1e-5);
        assert!((color.b - 0.9611975).abs() < 1e-5);
        assert_eq!(color.a, 1.0);
        assert_eq!(color.to_srgb().to_rgba8(), [101, 157, 251, 255]);
    }

    #[test]
    fn the_oklch_neutral_gives_the_gray_of_its_lightness() {
        let color = Oklch {
            l: 0.5,
            c: 0.0,
            h: 0.0,
            a: 1.0,
        }
        .to_linear_rgb();

        assert!((color.r - 0.125).abs() < 1e-5);
        assert_eq!(color.to_srgb().to_rgba8(), [99, 99, 99, 255]);
    }

    #[test]
    fn the_oklch_allows_out_of_gamut_channels() {
        let color = Oklch {
            l: 0.5,
            c: 0.5,
            h: 260.0,
            a: 1.0,
        }
        .to_linear_rgb();

        assert!((color.r - 0.0120694).abs() < 1e-5);
        assert!((color.g + 0.1571394).abs() < 1e-5);
        assert!((color.b - 2.4433003).abs() < 1e-5);
        assert_eq!(color.a, 1.0);
    }

    #[test]
    fn the_oklch_keeps_the_alpha() {
        let color = Oklch {
            l: 0.9,
            c: 0.05,
            h: 100.0,
            a: 0.5,
        }
        .to_linear_rgb();

        assert!((color.r - 0.7857574).abs() < 1e-5);
        assert_eq!(color.a, 0.5);
    }

    #[test]
    fn the_color_to_oklch_matches_the_oklab_reference() {
        let oklch = Color::from_rgba([0.3, 0.15, 0.45, 0.75])
            .demultiply()
            .to_oklch();

        assert!((oklch.l - 0.6606662).abs() < 1e-5);
        assert!((oklch.c - 0.1248309).abs() < 1e-5);
        assert!((oklch.h - 309.8361).abs() < 1e-3);
        assert_eq!(oklch.a, 0.75);
    }

    #[test]
    fn the_oklch_and_color_conversions_are_inverses() {
        let oklch = Oklch {
            l: 0.7,
            c: 0.15,
            h: 260.0,
            a: 0.9,
        };
        let round = oklch.to_linear_rgb().to_oklch();

        assert!((round.l - oklch.l).abs() < 1e-5);
        assert!((round.c - oklch.c).abs() < 1e-5);
        assert!((round.h - oklch.h).abs() < 1e-3);
        assert_eq!(round.a, oklch.a);

        let color = Color::from_rgba([0.4, 0.2, 0.6, 1.0]).demultiply();
        let round = color.to_oklch().to_linear_rgb();

        assert!((round.r - color.r).abs() < 1e-5);
        assert!((round.g - color.g).abs() < 1e-5);
        assert!((round.b - color.b).abs() < 1e-5);
        assert_eq!(round.a, color.a);
    }
}

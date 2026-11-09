use std::path::Path;
use std::sync::Arc;

use anywho::anywho;
use image::{ColorType, ImageFormat};
use mlua::{AnyUserData, Lua, Result, Table, UserData, UserDataFields, UserDataMethods};

use crate::color::{Color, Filter, Space, crop, luma_linear, resize};
use crate::{Rectangle, Viewport};

#[derive(Clone, Debug)]
pub struct Image {
    buffer: Arc<Buffer>,
    width: u32,
    height: u32,
    scale: f32,
    viewport: Viewport,
}

#[derive(Clone, Debug)]
struct Buffer {
    r: Vec<f32>,
    g: Vec<f32>,
    b: Vec<f32>,
    a: Vec<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Stretch,
    Cover,
}

impl Image {
    pub fn transparent(width: u32, height: u32, viewport: Viewport) -> Image {
        let count = (width * height) as usize;
        Image {
            buffer: Arc::new(Buffer {
                r: vec![0.0; count],
                g: vec![0.0; count],
                b: vec![0.0; count],
                a: vec![0.0; count],
            }),
            width,
            height,
            scale: viewport.scale,
            viewport,
        }
    }

    pub fn physical_width(&self) -> u32 {
        self.width
    }

    pub fn physical_height(&self) -> u32 {
        self.height
    }

    pub fn logical_width(&self) -> u32 {
        (self.width as f32 / self.scale).round() as u32
    }

    pub fn logical_height(&self) -> u32 {
        (self.height as f32 / self.scale).round() as u32
    }

    pub fn to_rgba8(&self) -> Vec<u8> {
        let count = (self.width * self.height) as usize;
        let mut out = Vec::with_capacity(count * 4);
        for i in 0..count {
            let color = Color {
                r: self.buffer.r[i],
                g: self.buffer.g[i],
                b: self.buffer.b[i],
                a: self.buffer.a[i],
            }
            .demultiply()
            .linear_to_srgb()
            .to_rgba_u8();
            out.extend_from_slice(&color);
        }
        out
    }

    pub fn resize(&mut self, new_width: f32, new_height: f32, filter: Filter, mode: Mode) {
        let scale = self.viewport.scale;
        let (target_w, target_h) = (
            self.viewport.to_physical(new_width),
            self.viewport.to_physical(new_height),
        );
        assert!(
            target_w != 0 && target_h != 0,
            "resize: the physical target is {target_w}×{target_h}; it must be at least 1×1"
        );

        let (box_w, box_h) = match mode {
            Mode::Cover => {
                let factor = f32::max(
                    target_w as f32 / self.width as f32,
                    target_h as f32 / self.height as f32,
                );
                (
                    (self.width as f32 * factor).round() as u32,
                    (self.height as f32 * factor).round() as u32,
                )
            }
            Mode::Stretch => (target_w, target_h),
        };

        let left = (box_w - target_w) / 2;
        let top = (box_h - target_h) / 2;

        if (box_w, box_h) == (self.width, self.height) && left == 0 && top == 0 {
            self.scale = scale;
            return;
        }

        let buffer = Arc::make_mut(&mut self.buffer);
        let resized = (box_w, box_h) != (self.width, self.height);
        if resized {
            let (w, h) = (self.width, self.height);
            buffer.r = resize(filter, &buffer.r, w, h, box_w, box_h);
            buffer.g = resize(filter, &buffer.g, w, h, box_w, box_h);
            buffer.b = resize(filter, &buffer.b, w, h, box_w, box_h);
            buffer.a = resize(filter, &buffer.a, w, h, box_w, box_h);
        }
        if left != 0 || top != 0 {
            buffer.r = crop(&buffer.r, box_w, left, top, target_w, target_h);
            buffer.g = crop(&buffer.g, box_w, left, top, target_w, target_h);
            buffer.b = crop(&buffer.b, box_w, left, top, target_w, target_h);
            buffer.a = crop(&buffer.a, box_w, left, top, target_w, target_h);
        }

        if resized {
            for value in buffer.a.iter_mut() {
                *value = value.clamp(0.0, 1.0);
            }
        }

        self.width = target_w;
        self.height = target_h;
        self.scale = scale;
    }

    pub fn autocontrast(&mut self) {
        let count = (self.width * self.height) as usize;

        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for i in 0..count {
            let luma = luma_linear(self.buffer.r[i], self.buffer.g[i], self.buffer.b[i]);
            lo = lo.min(luma);
            hi = hi.max(luma);
        }

        if hi <= lo {
            return;
        }

        let buffer = Arc::make_mut(&mut self.buffer);

        let scale = 1.0 / (hi - lo);
        let stretch = |value: f32| {
            if value < lo || value > hi {
                value
            } else {
                (value - lo) * scale
            }
        };
        for i in 0..count {
            buffer.r[i] = stretch(buffer.r[i]);
            buffer.g[i] = stretch(buffer.g[i]);
            buffer.b[i] = stretch(buffer.b[i]);
        }
    }

    pub fn blend(&mut self, other: &Image, at: Rectangle, space: Space) {
        let other = if other.scale != self.scale {
            let mut other = other.clone();
            other.resize(
                other.logical_width() as f32,
                other.logical_height() as f32,
                Filter::default(),
                Mode::Stretch,
            );
            other
        } else {
            other.clone()
        };

        let at_x = self.viewport.to_physical_signed(at.x);
        let at_y = self.viewport.to_physical_signed(at.y);
        let left = at_x.max(0);
        let top = at_y.max(0);
        let right = (at_x + other.width as i32).min(self.width as i32);
        let bottom = (at_y + other.height as i32).min(self.height as i32);
        if left >= right || top >= bottom {
            return;
        }

        let buffer = Arc::make_mut(&mut self.buffer);
        let (base_r, base_g, base_b, base_a) =
            (&mut buffer.r, &mut buffer.g, &mut buffer.b, &mut buffer.a);
        let (layer_r, layer_g, layer_b, layer_a) = (
            &other.buffer.r,
            &other.buffer.g,
            &other.buffer.b,
            &other.buffer.a,
        );

        for y in top..bottom {
            for x in left..right {
                let i = (y * self.width as i32 + x) as usize;
                let j = ((y - at_y) * other.width as i32 + (x - at_x)) as usize;
                let base = Color {
                    r: base_r[i],
                    g: base_g[i],
                    b: base_b[i],
                    a: base_a[i],
                };
                let layer = Color {
                    r: layer_r[j],
                    g: layer_g[j],
                    b: layer_b[j],
                    a: layer_a[j],
                };
                let blended = match space {
                    Space::Linear => base.over(layer),
                    Space::Srgb => base
                        .linear_to_srgb()
                        .over(layer.linear_to_srgb())
                        .srgb_to_linear(),
                };
                base_r[i] = blended.r;
                base_g[i] = blended.g;
                base_b[i] = blended.b;
                base_a[i] = blended.a;
            }
        }
    }

    pub fn write(&self) -> anywho::Result<()> {
        let rgba8 = self.to_rgba8();
        let target = Path::new("output.png");
        let temp = Path::new("output.png.tmp");
        image::save_buffer_with_format(
            temp,
            &rgba8,
            self.width,
            self.height,
            ColorType::Rgba8,
            ImageFormat::Png,
        )
        .map_err(|error| {
            let _ = std::fs::remove_file(temp);
            anywho!("failed to write output.png: {error}")
        })?;
        std::fs::rename(temp, target)
            .map_err(|error| anywho!("failed to finalize output.png: {error}"))?;
        Ok(())
    }
}

fn load(path: &Path, viewport: Viewport) -> anywho::Result<Image> {
    let rgba = image::open(path)
        .map_err(|error| anywho!("failed to load {}: {error}", path.display()))?
        .to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    let count = (width * height) as usize;
    let (mut r, mut g, mut b, mut a) = (
        Vec::with_capacity(count),
        Vec::with_capacity(count),
        Vec::with_capacity(count),
        Vec::with_capacity(count),
    );
    for pixel in rgba.as_chunks::<4>().0 {
        let color = Color::from_rgba_u8([pixel[0], pixel[1], pixel[2], pixel[3]])
            .srgb_to_linear()
            .premultiply();
        r.push(color.r);
        g.push(color.g);
        b.push(color.b);
        a.push(color.a);
    }
    Ok(Image {
        buffer: Arc::new(Buffer { r, g, b, a }),
        width,
        height,
        scale: 1.0,
        viewport,
    })
}

impl UserData for Image {
    fn add_fields<F: UserDataFields<Image>>(fields: &mut F) {
        fields.add_meta_field("__name", "Image");
    }

    fn add_methods<M: UserDataMethods<Image>>(methods: &mut M) {
        methods.add_method("width", |_lua, this, _args: ()| -> Result<u32> {
            Ok(this.logical_width())
        });
        methods.add_method("height", |_lua, this, _args: ()| -> Result<u32> {
            Ok(this.logical_height())
        });
        methods.add_method("size", |lua, this, _args: ()| -> Result<Table> {
            let size = lua.create_table()?;
            size.set(1, this.logical_width())?;
            size.set(2, this.logical_height())?;
            Ok(size)
        });
        methods.add_method("clone", |_lua, this, _args: ()| -> Result<Image> {
            Ok(this.clone())
        });
        methods.add_method_mut("resize", |_lua, this, opts: Table| -> Result<()> {
            let width: Option<f32> = opts.get("width")?;
            let height: Option<f32> = opts.get("height")?;
            let filter: Option<String> = opts.get("filter")?;
            let mode: Option<String> = opts.get("mode")?;

            let (width, height) = match (width, height) {
                (Some(width), Some(height)) => (width, height),
                (Some(width), None) => (
                    width,
                    (width * this.height as f32 / this.width as f32).round(),
                ),
                (None, Some(height)) => (
                    (height * this.width as f32 / this.height as f32).round(),
                    height,
                ),
                (None, None) => {
                    return Err(mlua::Error::RuntimeError(
                        "resize: needs width or height".to_owned(),
                    ));
                }
            };

            let filter = match filter.as_deref() {
                None => Filter::default(),
                Some("cubic") => Filter::Cubic,
                Some("lanczos") => Filter::Lanczos,
                Some(other) => {
                    return Err(mlua::Error::RuntimeError(format!(
                        "resize: unknown filter: {other}"
                    )));
                }
            };

            let mode = match mode.as_deref() {
                None => Mode::default(),
                Some("cover") => Mode::Cover,
                Some(other) => {
                    return Err(mlua::Error::RuntimeError(format!(
                        "resize: unknown mode: {other}"
                    )));
                }
            };

            this.resize(width, height, filter, mode);
            Ok(())
        });
        methods.add_method_mut("autocontrast", |_lua, this, _args: ()| -> Result<()> {
            this.autocontrast();
            Ok(())
        });
        methods.add_method_mut(
            "blend",
            |lua, this, (other, opts): (AnyUserData, Option<Table>)| -> Result<()> {
                let opts = match opts {
                    Some(opts) => opts,
                    None => lua.create_table()?,
                };

                let (width, height) = (this.physical_width(), this.physical_height());
                let (physical_width, physical_height) =
                    (this.viewport.physical_width(), this.viewport.physical_height());
                if (width, height) != (physical_width, physical_height) {
                    return Err(mlua::Error::RuntimeError(format!(
                        "blend: the receiver is {width}×{height}, the viewport is {physical_width}×{physical_height}"
                    )));
                }

                let other = other.borrow::<Image>()?;
                let at: Option<AnyUserData> = opts.get("at")?;
                let at = match at {
                    Some(at) => *at.borrow::<Rectangle>()?,
                    None => Rectangle::default(),
                };
                let color_space: Option<String> = opts.get("color_space")?;
                let space = match color_space.as_deref() {
                    None | Some("linear") => Space::Linear,
                    Some("srgb") => Space::Srgb,
                    Some(other) => {
                        return Err(mlua::Error::RuntimeError(format!(
                            "blend: unknown color_space: {other}"
                        )))
                    }
                };

                this.blend(&other, at, space);
                Ok(())
            },
        );
        methods.add_method_mut(
            "fill",
            |_lua, _this, (_region, _color): (mlua::Value, String)| -> Result<()> { Ok(()) },
        );
        methods.add_method(
            "luma_at",
            |_lua, _this, _region: mlua::Value| -> Result<f64> { Ok(0.0) },
        );
    }
}

pub fn install(
    lua: &Lua,
    env: &Table,
    script_dir: &Path,
    viewport: Viewport,
) -> anywho::Result<()> {
    let script_dir = script_dir.to_path_buf();
    env.set(
        "image",
        lua.create_function(move |_lua, path: String| -> Result<Image> {
            load(&script_dir.join(&path), viewport)
                .map_err(|error| mlua::Error::RuntimeError(error.to_string()))
        })?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use super::{Buffer, Filter, Image, Mode, load};
    use crate::color::{Color, Space, luma_linear};
    use crate::{Rectangle, Viewport};

    #[test]
    fn the_base_background_loads_at_its_file_dims() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("images/Lenna.png");
        let file = std::fs::read(&path).unwrap();

        let file_width = u32::from_be_bytes([file[16], file[17], file[18], file[19]]);
        let file_height = u32::from_be_bytes([file[20], file[21], file[22], file[23]]);

        let viewport = Viewport {
            width: 512,
            height: 512,
            scale: 1.0,
        };

        let image = load(&path, viewport).unwrap();
        assert_eq!(
            (image.logical_width(), image.logical_height()),
            (file_width, file_height)
        );

        let count = (file_width * file_height) as usize;

        for plane in [
            &image.buffer.r,
            &image.buffer.g,
            &image.buffer.b,
            &image.buffer.a,
        ] {
            assert_eq!(plane.len(), count);
        }
    }

    #[test]
    fn resize_lands_on_its_targets_and_forks() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("images/Lenna.png");
        let mut image = load(
            &path,
            Viewport {
                width: 512,
                height: 512,
                scale: 0.5,
            },
        )
        .unwrap();
        let first_pixel = image.buffer.r[0];

        let mut fork = image.clone();

        image.resize(200.0, 250.0, Filter::Lanczos, Mode::Stretch);
        assert_eq!(
            (image.physical_width(), image.physical_height()),
            (100, 125)
        );

        assert_eq!((fork.physical_width(), fork.physical_height()), (512, 512));
        assert_eq!(fork.buffer.r[0], first_pixel);

        fork.resize(400.0, 100.0, Filter::Cubic, Mode::Cover);
        assert_eq!((fork.physical_width(), fork.physical_height()), (200, 50));

        let mut image = load(
            &path,
            Viewport {
                width: 512,
                height: 512,
                scale: 2.0,
            },
        )
        .unwrap();
        image.resize(256.0, 256.0, Filter::Cubic, Mode::Stretch);
        assert_eq!(image.scale, 2.0);
        assert_eq!((image.logical_width(), image.logical_height()), (256, 256));
        assert_eq!(
            (image.physical_width(), image.physical_height()),
            (512, 512)
        );
    }

    #[test]
    fn the_resize_keeps_a_constant_plane_constant() {
        let viewport = Viewport {
            width: 8,
            height: 8,
            scale: 1.0,
        };
        let expected = Color {
            r: 0.5,
            g: 0.5,
            b: 0.5,
            a: 1.0,
        }
        .demultiply()
        .linear_to_srgb()
        .to_rgba_u8();

        for filter in [Filter::Cubic, Filter::Lanczos] {
            for target in [4.0, 16.0] {
                let mut image = square([0.5, 0.5, 0.5, 1.0], 8, viewport);
                image.resize(target, target, filter, Mode::Stretch);
                for pixel in image.to_rgba8().chunks(4) {
                    assert_eq!(pixel, &expected);
                }
            }
        }
    }

    fn synthetic_linear(pixels: &[[f32; 4]], viewport: Viewport) -> Image {
        let plane = |channel: usize| pixels.iter().map(|pixel| pixel[channel]).collect();

        Image {
            buffer: Arc::new(Buffer {
                r: plane(0),
                g: plane(1),
                b: plane(2),
                a: plane(3),
            }),
            width: pixels.len() as u32,
            height: 1,
            scale: 1.0,
            viewport,
        }
    }

    fn square(color: [f32; 4], size: u32, viewport: Viewport) -> Image {
        let count = (size * size) as usize;
        Image {
            buffer: Arc::new(Buffer {
                r: vec![color[0]; count],
                g: vec![color[1]; count],
                b: vec![color[2]; count],
                a: vec![color[3]; count],
            }),
            width: size,
            height: size,
            scale: 1.0,
            viewport,
        }
    }

    #[test]
    fn the_autocontrast_is_full_range_and_tone_preserving() {
        let pixels = [
            [0.3, 0.3, 0.3, 1.0],
            [0.5, 0.5, 0.5, 1.0],
            [0.7, 0.7, 0.7, 1.0],
            [1.0, 0.12, 0.12, 1.0],
        ];

        let mut image = synthetic_linear(
            &pixels,
            Viewport {
                width: 4,
                height: 1,
                scale: 1.0,
            },
        );
        image.autocontrast();

        let lo = luma_linear(0.3, 0.3, 0.3);
        let hi = luma_linear(0.7, 0.7, 0.7);
        let scale = 1.0 / (hi - lo);

        for (i, pixel) in pixels.iter().enumerate() {
            for (value, stretched) in [
                (pixel[0], image.buffer.r[i]),
                (pixel[1], image.buffer.g[i]),
                (pixel[2], image.buffer.b[i]),
            ] {
                let expected = if value < lo || value > hi {
                    value
                } else {
                    (value - lo) * scale
                };
                assert!((expected - stretched).abs() <= 1e-6);
            }
        }

        assert!((image.buffer.r[1] - 0.5).abs() <= 1e-5);

        assert_eq!(image.buffer.r[3], 1.0);
        assert_eq!(image.buffer.g[3], 0.12);
        assert_eq!(image.buffer.b[3], 0.12);
        assert_eq!(image.buffer.a[3], 1.0);
    }

    #[test]
    fn the_uniform_autocontrast_is_the_identity() {
        let pixels = [[0.4, 0.3, 0.5, 1.0]; 3];

        let mut image = synthetic_linear(
            &pixels,
            Viewport {
                width: 3,
                height: 1,
                scale: 1.0,
            },
        );
        image.autocontrast();

        for i in 0..3 {
            assert_eq!(image.buffer.r[i], 0.4);
            assert_eq!(image.buffer.g[i], 0.3);
            assert_eq!(image.buffer.b[i], 0.5);
        }
    }

    #[test]
    fn the_sky_background_autocontrast_applies_the_ramp() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("personal/images/sky-night.png");
        let mut image = load(
            &path,
            Viewport {
                width: 1640,
                height: 2048,
                scale: 1.0,
            },
        )
        .unwrap();

        let count = (image.width * image.height) as usize;
        let (r, g, b) = (
            image.buffer.r.clone(),
            image.buffer.g.clone(),
            image.buffer.b.clone(),
        );

        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for i in 0..count {
            let luma = luma_linear(r[i], g[i], b[i]);
            lo = lo.min(luma);
            hi = hi.max(luma);
        }

        assert!(hi > lo);

        image.autocontrast();
        let scale = 1.0 / (hi - lo);

        for i in 0..count {
            for (value, stretched) in [
                (r[i], image.buffer.r[i]),
                (g[i], image.buffer.g[i]),
                (b[i], image.buffer.b[i]),
            ] {
                let expected = if value < lo || value > hi {
                    value
                } else {
                    (value - lo) * scale
                };
                assert!((expected - stretched).abs() <= 1e-6, "pixel {i}");
            }
        }
    }

    #[test]
    fn the_blend_is_byte_identical_where_the_layer_is_opaque() {
        let lenna = Path::new(env!("CARGO_MANIFEST_DIR")).join("images/Lenna.png");
        let sky = Path::new(env!("CARGO_MANIFEST_DIR")).join("personal/images/sky-night.png");
        let viewport = Viewport {
            width: 512,
            height: 512,
            scale: 1.0,
        };
        let mut base = load(&sky, viewport).unwrap();
        let before = base.to_rgba8();
        let layer = load(&lenna, viewport).unwrap();
        let (base_width, base_height) = (base.physical_width(), base.physical_height());
        let (layer_width, layer_height) = (layer.physical_width(), layer.physical_height());

        base.blend(&layer, Rectangle::default(), Space::Linear);

        let after = base.to_rgba8();
        let layer = layer.to_rgba8();
        for y in 0..base_height {
            for x in 0..base_width {
                let pixel = (y * base_width + x) as usize * 4;
                if x < layer_width && y < layer_height {
                    let overlapped = (y * layer_width + x) as usize * 4;
                    assert_eq!(&after[pixel..pixel + 4], &layer[overlapped..overlapped + 4]);
                } else {
                    assert_eq!(&after[pixel..pixel + 4], &before[pixel..pixel + 4]);
                }
            }
        }
    }

    #[test]
    fn the_blend_matches_the_pinned_over_on_real_data() {
        let lenna = Path::new(env!("CARGO_MANIFEST_DIR")).join("images/Lenna.png");
        let mask =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("personal/images/Card_Mask_Standard.png");
        let viewport = Viewport {
            width: 512,
            height: 512,
            scale: 1.0,
        };
        let mut base = load(&lenna, viewport).unwrap();
        let layer = load(&mask, viewport).unwrap();
        let base_width = base.physical_width();
        let layer_width = layer.physical_width();

        let (base_r, base_g, base_b, base_a) = (
            base.buffer.r.clone(),
            base.buffer.g.clone(),
            base.buffer.b.clone(),
            base.buffer.a.clone(),
        );

        base.blend(&layer, Rectangle::default(), Space::Linear);

        let actual = base.to_rgba8();
        for y in 0..base.height {
            for x in 0..base.width {
                let i = (y * base_width + x) as usize;
                let j = (y * layer_width + x) as usize;
                let expected = Color {
                    r: base_r[i],
                    g: base_g[i],
                    b: base_b[i],
                    a: base_a[i],
                }
                .over(Color {
                    r: layer.buffer.r[j],
                    g: layer.buffer.g[j],
                    b: layer.buffer.b[j],
                    a: layer.buffer.a[j],
                })
                .demultiply()
                .linear_to_srgb()
                .to_rgba_u8();
                assert_eq!(&actual[i * 4..i * 4 + 4], &expected[..]);
            }
        }
    }

    #[test]
    fn the_blend_offset_and_clip_are_exact() {
        let viewport = Viewport {
            width: 4,
            height: 4,
            scale: 1.0,
        };
        let black = square([0.0, 0.0, 0.0, 1.0], 4, viewport);
        let white = square([1.0, 1.0, 1.0, 1.0], 2, viewport);

        let mut base = black.clone();
        base.blend(&white, Rectangle::new(-1.0, -1.0, 0.0, 0.0), Space::Linear);
        let rgba = base.to_rgba8();
        assert_eq!(&rgba[0..4], &[255, 255, 255, 255]);
        for i in 1..16 {
            assert_eq!(&rgba[i * 4..i * 4 + 4], &[0, 0, 0, 255]);
        }

        let mut base = black.clone();
        base.blend(&white, Rectangle::new(3.0, 3.0, 0.0, 0.0), Space::Linear);
        let rgba = base.to_rgba8();
        for i in 0..15 {
            assert_eq!(&rgba[i * 4..i * 4 + 4], &[0, 0, 0, 255]);
        }
        assert_eq!(&rgba[60..64], &[255, 255, 255, 255]);
    }

    #[test]
    fn the_blend_color_spaces_diverge() {
        let viewport = Viewport {
            width: 2,
            height: 1,
            scale: 1.0,
        };
        let base = synthetic_linear(&[[0.5, 0.5, 0.5, 1.0]; 2], viewport);
        let layer = synthetic_linear(&[[0.5, 0.5, 0.5, 0.5]; 2], viewport);

        let mut linear = base.clone();
        linear.blend(&layer, Rectangle::default(), Space::Linear);
        let mut srgb = base.clone();
        srgb.blend(&layer, Rectangle::default(), Space::Srgb);

        for i in 0..2 {
            let linear_expected = Color {
                r: 0.5,
                g: 0.5,
                b: 0.5,
                a: 1.0,
            }
            .over(Color {
                r: 0.5,
                g: 0.5,
                b: 0.5,
                a: 0.5,
            });
            let srgb_expected = Color {
                r: 0.5,
                g: 0.5,
                b: 0.5,
                a: 1.0,
            }
            .linear_to_srgb()
            .over(
                Color {
                    r: 0.5,
                    g: 0.5,
                    b: 0.5,
                    a: 0.5,
                }
                .linear_to_srgb(),
            )
            .srgb_to_linear();
            assert!((linear.buffer.r[i] - linear_expected.r).abs() <= 1e-6);
            assert!((srgb.buffer.r[i] - srgb_expected.r).abs() <= 1e-6);
            assert!((linear_expected.r - srgb_expected.r).abs() > 0.01);
        }
    }

    #[test]
    fn the_blend_resamples_the_file_scale_layer() {
        let lenna = Path::new(env!("CARGO_MANIFEST_DIR")).join("images/Lenna.png");
        let viewport = Viewport {
            width: 256,
            height: 256,
            scale: 0.5,
        };
        let mut base = Image::transparent(256, 256, viewport);
        let layer = load(&lenna, viewport).unwrap();

        base.blend(&layer, Rectangle::default(), Space::Linear);

        let mut expected = layer.clone();
        expected.resize(512.0, 512.0, Filter::default(), Mode::Stretch);
        assert_eq!(base.to_rgba8(), expected.to_rgba8());
    }
}

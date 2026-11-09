use std::path::Path;
use std::sync::Arc;

use anywho::anywho;
use image::{ColorType, ImageFormat};
use mlua::{AnyUserData, Lua, Result, Table, UserData, UserDataFields, UserDataMethods};
use skrifa::outline::{OutlineGlyphCollection, OutlinePen};
use skrifa::prelude::{FontRef, GlyphId, LocationRef, NormalizedCoord, Size};
use vello_cpu::kurbo::{Affine, BezPath, Rect};
use vello_cpu::{
    Pixmap, RenderContext, Resources,
    color::{AlphaColor, Srgb},
};

use crate::color::{Color, Filter, Space, crop, luma_linear, resize};
use crate::text::Text;
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

    pub fn fill(&mut self, region: Rectangle, paint: Color) {
        let left = region.x;
        let top = region.y;
        let right = region.x + region.width;
        let bottom = region.y + region.height;
        if region.width <= 0.0 || region.height <= 0.0 {
            return;
        }
        if !left.is_finite() || !top.is_finite() || !right.is_finite() || !bottom.is_finite() {
            return;
        }

        let width = self.width as u16;
        let height = self.height as u16;
        let mut resources = Resources::new();
        let mut context = RenderContext::new(width, height);
        context.set_transform(Affine::scale(self.scale.into()));
        context.set_paint(AlphaColor::<Srgb>::BLACK);
        context.fill_rect(&Rect::new(
            left.into(),
            top.into(),
            right.into(),
            bottom.into(),
        ));
        let mut pixmap = Pixmap::new(width, height);
        context.render(&mut pixmap, &mut resources);

        let buffer = Arc::make_mut(&mut self.buffer);
        let (base_r, base_g, base_b, base_a) =
            (&mut buffer.r, &mut buffer.g, &mut buffer.b, &mut buffer.a);
        for (i, pixel) in pixmap.data().iter().enumerate() {
            let cover = pixel.a as f32 / 255.0;
            if cover > 0.0 {
                let base = Color {
                    r: base_r[i],
                    g: base_g[i],
                    b: base_b[i],
                    a: base_a[i],
                };
                let layer = Color {
                    r: paint.r * cover,
                    g: paint.g * cover,
                    b: paint.b * cover,
                    a: cover,
                };
                let blended = base.over(layer);
                base_r[i] = blended.r;
                base_g[i] = blended.g;
                base_b[i] = blended.b;
                base_a[i] = blended.a;
            }
        }
    }

    pub fn fill_text(&mut self, text: &Text, paint: Color) {
        let (box_x, box_y) = (text.x, text.y);
        let (box_width, box_height) = (text.width, text.height);
        if box_width == 0 || box_height == 0 {
            return;
        }

        let (scratch_width, scratch_height) = (box_width + 2, box_height + 2);
        let (origin_x, origin_y) = (box_x - 1, box_y - 1);
        let (layout_x, layout_y) = (box_x as f32 - text.ink_min.0, box_y as f32 - text.ink_min.1);

        let mut resources = Resources::new();
        let mut context = RenderContext::new(scratch_width as u16, scratch_height as u16);
        context.set_transform(Affine::translate((
            (layout_x - origin_x as f32) as f64,
            (layout_y - origin_y as f32) as f64,
        )));
        context.set_paint(AlphaColor::<Srgb>::BLACK);

        for run in text.runs() {
            for glyph in run.glyphs() {
                let Some(font) =
                    FontRef::from_index(glyph.font.data.as_ref(), glyph.font.index).ok()
                else {
                    continue;
                };
                let collection = OutlineGlyphCollection::new(&font);
                let Some(glyph_outline) = collection.get(GlyphId::new(glyph.id)) else {
                    continue;
                };
                let coords: Vec<NormalizedCoord> = glyph
                    .location
                    .iter()
                    .map(|coord| NormalizedCoord::from_bits(coord.to_bits()))
                    .collect();
                let location = LocationRef::new(&coords);
                let mut pen = PathPen::new();
                let Ok(_) = glyph_outline.draw((Size::new(glyph.size), location), &mut pen) else {
                    continue;
                };
                let mut path = pen.path;
                path.apply_affine(Affine::new([
                    1.0,
                    0.0,
                    0.0,
                    -1.0,
                    glyph.x as f64,
                    glyph.y as f64,
                ]));
                context.fill_path(&path);
            }
        }

        let mut pixmap = Pixmap::new(scratch_width as u16, scratch_height as u16);
        context.render(&mut pixmap, &mut resources);

        let buffer = Arc::make_mut(&mut self.buffer);
        let (base_r, base_g, base_b, base_a) =
            (&mut buffer.r, &mut buffer.g, &mut buffer.b, &mut buffer.a);
        for (y, row) in pixmap
            .data()
            .chunks_exact(scratch_width as usize)
            .enumerate()
        {
            for (x, pixel) in row.iter().enumerate() {
                let cover = pixel.a as f32 / 255.0;
                if cover <= 0.0 {
                    continue;
                }
                let canvas_x = origin_x + x as i32;
                let canvas_y = origin_y + y as i32;
                let inside_box = (box_x..box_x + box_width as i32).contains(&canvas_x)
                    && (box_y..box_y + box_height as i32).contains(&canvas_y);
                let inside_canvas = canvas_x >= 0
                    && canvas_x < self.width as i32
                    && canvas_y >= 0
                    && canvas_y < self.height as i32;
                if !inside_box || !inside_canvas {
                    continue;
                }
                let i = (canvas_y * self.width as i32 + canvas_x) as usize;
                let base = Color {
                    r: base_r[i],
                    g: base_g[i],
                    b: base_b[i],
                    a: base_a[i],
                };
                let layer = Color {
                    r: paint.r * cover,
                    g: paint.g * cover,
                    b: paint.b * cover,
                    a: cover,
                };
                let blended = base
                    .linear_to_srgb()
                    .over(layer.linear_to_srgb())
                    .srgb_to_linear();
                base_r[i] = blended.r;
                base_g[i] = blended.g;
                base_b[i] = blended.b;
                base_a[i] = blended.a;
            }
        }
    }

    pub fn luma_at(&self, region: Rectangle) -> f32 {
        let left = self.viewport.to_physical_signed(region.x);
        let top = self.viewport.to_physical_signed(region.y);
        let right = self.viewport.to_physical_signed(region.x + region.width);
        let bottom = self.viewport.to_physical_signed(region.y + region.height);
        let left = left.max(0).min(self.width as i32);
        let top = top.max(0).min(self.height as i32);
        let right = right.max(0).min(self.width as i32);
        let bottom = bottom.max(0).min(self.height as i32);
        if left >= right || top >= bottom {
            return 0.0;
        }

        let (r, g, b) = (&self.buffer.r, &self.buffer.g, &self.buffer.b);
        let mut luma = 0.0f32;
        for y in top..bottom {
            for x in left..right {
                let i = (y * self.width as i32 + x) as usize;
                luma += luma_linear(r[i], g[i], b[i]);
            }
        }
        luma / ((right - left) * (bottom - top)) as f32 * 255.0
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

struct PathPen {
    path: BezPath,
}

impl PathPen {
    fn new() -> PathPen {
        PathPen {
            path: BezPath::new(),
        }
    }
}

impl OutlinePen for PathPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to((x as f64, y as f64));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to((x as f64, y as f64));
    }

    fn quad_to(&mut self, control_x: f32, control_y: f32, x: f32, y: f32) {
        self.path
            .quad_to((control_x as f64, control_y as f64), (x as f64, y as f64));
    }

    fn curve_to(
        &mut self,
        control_x: f32,
        control_y: f32,
        control1_x: f32,
        control1_y: f32,
        x: f32,
        y: f32,
    ) {
        self.path.curve_to(
            (control_x as f64, control_y as f64),
            (control1_x as f64, control1_y as f64),
            (x as f64, y as f64),
        );
    }

    fn close(&mut self) {
        self.path.close_path();
    }
}

fn paint_from_hex(color: &str) -> Result<Color> {
    let message = || format!("fill: {color} is not a #RRGGBB hex color");
    let digits = color
        .strip_prefix('#')
        .filter(|digits| digits.len() == 6)
        .ok_or_else(|| mlua::Error::RuntimeError(message()))?;
    let channel = |start: usize| -> Result<u8> {
        u8::from_str_radix(&digits[start..start + 2], 16)
            .map_err(|_| mlua::Error::RuntimeError(message()))
    };
    Ok(Color::from_rgba_u8([channel(0)?, channel(2)?, channel(4)?, 255]).srgb_to_linear())
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
            |_lua, this, (region, color): (AnyUserData, String)| -> Result<()> {
                let paint = paint_from_hex(&color)?;
                match region.borrow::<Rectangle>() {
                    Ok(region) => {
                        this.fill(*region, paint);
                        Ok(())
                    }
                    Err(_) => {
                        let text = region.borrow::<Text>().map_err(|_| {
                            mlua::Error::RuntimeError(
                                "fill: the region must be a `Rectangle` or a `Text`".to_owned(),
                            )
                        })?;
                        this.fill_text(&text, paint);
                        Ok(())
                    }
                }
            },
        );
        methods.add_method(
            "luma_at",
            |_lua, this, region: AnyUserData| -> Result<f64> {
                let region: Rectangle = *region.borrow::<Rectangle>()?;
                Ok(this.luma_at(region) as f64)
            },
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

    use mlua::Lua;

    use super::{Buffer, Filter, Image, Mode, load};
    use crate::color::{Color, Space, luma_linear};
    use crate::text::Text;
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

    #[test]
    fn the_luma_at_is_the_709_mean_of_the_stored_channels() {
        let viewport = Viewport {
            width: 4,
            height: 4,
            scale: 1.0,
        };
        let r: [f32; 16] = [
            0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 0.0, 0.1, 0.2, 0.3, 0.4, 0.5,
        ];
        let image = Image {
            buffer: Arc::new(Buffer {
                r: r.to_vec(),
                g: vec![0.5; 16],
                b: vec![0.6; 16],
                a: vec![1.0; 16],
            }),
            width: 4,
            height: 4,
            scale: 1.0,
            viewport,
        };

        let luma = |r: f32| 0.2126 * r as f64 + 0.7152 * 0.5 + 0.0722 * 0.6;
        let mean = |indices: &[usize]| {
            indices.iter().map(|i| luma(r[*i])).sum::<f64>() / indices.len() as f64 * 255.0
        };

        let full: Vec<usize> = (0..16).collect();
        assert!(
            (image.luma_at(Rectangle::new(0.0, 0.0, 4.0, 4.0)) as f64 - mean(&full)).abs() < 1e-2
        );

        let top_left = vec![0, 1, 4, 5];
        assert!(
            (image.luma_at(Rectangle::new(0.0, 0.0, 2.0, 2.0)) as f64 - mean(&top_left)).abs()
                < 1e-2
        );

        let right = vec![3, 7, 11, 15];
        assert!(
            (image.luma_at(Rectangle::new(3.0, 0.0, 1.0, 4.0)) as f64 - mean(&right)).abs() < 1e-2
        );

        let clipped = vec![0];
        assert!(
            (image.luma_at(Rectangle::new(-1.0, -1.0, 2.0, 2.0)) as f64 - mean(&clipped)).abs()
                < 1e-2
        );

        assert_eq!(image.luma_at(Rectangle::new(10.0, 10.0, 2.0, 2.0)), 0.0);
        assert_eq!(image.luma_at(Rectangle::new(0.0, 0.0, -2.0, 2.0)), 0.0);
    }

    #[test]
    fn the_luma_at_reads_regions_in_logical_px() {
        let viewport = Viewport {
            width: 8,
            height: 8,
            scale: 0.5,
        };
        let image = square([0.5, 0.5, 0.5, 1.0], 4, viewport);

        assert!((image.luma_at(Rectangle::new(0.0, 0.0, 8.0, 8.0)) - 127.5).abs() < 1e-2);
        assert!((image.luma_at(Rectangle::new(0.0, 0.0, 4.0, 4.0)) - 127.5).abs() < 1e-2);
        assert_eq!(image.luma_at(Rectangle::new(8.0, 8.0, 4.0, 4.0)), 0.0);
    }

    #[test]
    fn the_luma_at_matches_the_709_mean_on_a_real_image() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("images/Lenna.png");
        let image = load(
            &path,
            Viewport {
                width: 512,
                height: 512,
                scale: 1.0,
            },
        )
        .unwrap();

        let luma = image.luma_at(Rectangle::new(10.0, 20.0, 100.0, 50.0));

        let (r, g, b) = (&image.buffer.r, &image.buffer.g, &image.buffer.b);
        let sum: f32 = (20..70)
            .flat_map(|y| (10..110).map(move |x| y * 512 + x))
            .map(|i| luma_linear(r[i], g[i], b[i]))
            .sum();
        let expected = sum / 5000.0 * 255.0;
        assert!((luma - expected).abs() < 1e-2);
    }

    #[test]
    fn the_luma_at_matches_the_independent_709_mean() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("personal/images/sky-night.png");
        let image = load(
            &path,
            Viewport {
                width: 1640,
                height: 2048,
                scale: 1.0,
            },
        )
        .unwrap();

        // Frozen oracles: an independent Python pass (PIL, the IEC 61966-2-1
        // decode, the Rec. 709 mean) over personal/images/sky-night.png.
        let corner = image.luma_at(Rectangle::new(10.0, 20.0, 300.0, 200.0));
        assert!((corner - 2.7941046).abs() < 1e-2);

        let full = image.luma_at(Rectangle::new(0.0, 0.0, 1640.0, 2048.0));
        assert!((full - 11.452538).abs() < 1e-2);
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

    #[test]
    fn the_fill_paints_an_aligned_region_exactly() {
        let viewport = Viewport {
            width: 8,
            height: 8,
            scale: 1.0,
        };
        let mut image = Image::transparent(8, 8, viewport);
        let paint = Color::from_rgba_u8([128, 160, 192, 255]).srgb_to_linear();

        image.fill(Rectangle::new(1.0, 2.0, 4.0, 3.0), paint);

        let painted = image.to_rgba8();
        for y in 0..8u32 {
            for x in 0..8u32 {
                let pixel = (y * 8 + x) as usize * 4;
                let inside = (1..5).contains(&x) && (2..5).contains(&y);
                let expected = if inside {
                    [128, 160, 192, 255]
                } else {
                    [0, 0, 0, 0]
                };
                assert_eq!(&painted[pixel..pixel + 4], &expected);
            }
        }
    }

    #[test]
    fn the_fill_antialiases_fractional_edges() {
        let viewport = Viewport {
            width: 4,
            height: 4,
            scale: 1.0,
        };
        let mut image = Image::transparent(4, 4, viewport);
        let paint = Color::from_rgba_u8([255, 0, 0, 255]).srgb_to_linear();

        image.fill(Rectangle::new(0.5, 0.5, 3.0, 3.0), paint);

        let coverages = [
            64, 128, 128, 64, 128, 255, 255, 128, 128, 255, 255, 128, 64, 128, 128, 64,
        ];
        let painted = image.to_rgba8();
        for (i, coverage) in coverages.iter().enumerate() {
            let cover = *coverage as f32 / 255.0;
            let expected = Color {
                r: paint.r * cover,
                g: 0.0,
                b: 0.0,
                a: cover,
            }
            .demultiply()
            .linear_to_srgb()
            .to_rgba_u8();
            assert_eq!(&painted[i * 4..i * 4 + 4], &expected);
        }
    }

    #[test]
    fn the_fill_antialiases_at_the_run_scale() {
        let viewport = Viewport {
            width: 8,
            height: 8,
            scale: 0.5,
        };
        let mut image = Image::transparent(8, 8, viewport);
        let paint = Color::from_rgba_u8([0, 255, 255, 255]).srgb_to_linear();

        image.fill(Rectangle::new(0.5, 0.5, 3.0, 3.0), paint);

        let coverages = [
            143, 143, 0, 0, 0, 0, 0, 0, 143, 143, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let painted = image.to_rgba8();
        for (i, coverage) in coverages.iter().enumerate() {
            let cover = *coverage as f32 / 255.0;
            let expected = Color {
                r: 0.0,
                g: paint.g * cover,
                b: paint.b * cover,
                a: cover,
            }
            .demultiply()
            .linear_to_srgb()
            .to_rgba_u8();
            assert_eq!(&painted[i * 4..i * 4 + 4], &expected);
        }
    }

    #[test]
    fn the_fill_is_a_no_op_for_degenerate_regions() {
        let viewport = Viewport {
            width: 4,
            height: 4,
            scale: 1.0,
        };
        let mut image = Image::transparent(4, 4, viewport);
        let paint = Color::from_rgba_u8([255, 255, 255, 255]).srgb_to_linear();

        image.fill(Rectangle::new(0.0, 0.0, 0.0, 0.0), paint);
        image.fill(Rectangle::new(0.0, 0.0, -2.0, 2.0), paint);
        image.fill(Rectangle::new(f32::NAN, 0.0, 2.0, 2.0), paint);

        let painted = image.to_rgba8();
        assert!(painted.iter().all(|&byte| byte == 0));
    }

    #[test]
    fn the_fill_binding_validates_its_arguments() {
        let lua = Lua::new();
        let viewport = Viewport {
            width: 4,
            height: 4,
            scale: 1.0,
        };
        let image = Image::transparent(4, 4, viewport);
        let output = lua.create_userdata(image).unwrap();
        let region = lua
            .create_userdata(Rectangle::new(0.0, 0.0, 2.0, 2.0))
            .unwrap();
        let text = lua
            .create_userdata(Text::new("Any Family", 100.0, 400.0, 0.0, 0.0, 1.0, ""))
            .unwrap();

        let env = lua.create_table().unwrap();
        env.set("output", output.clone()).unwrap();
        env.set("region", region.clone()).unwrap();
        env.set("text", text.clone()).unwrap();

        for color in ["808080", "#12345", "#GGGGGG", "#1234567"] {
            let error = lua
                .load(format!("output:fill(region, \"{color}\")"))
                .set_environment(env.clone())
                .eval::<()>()
                .unwrap_err();
            assert!(error.to_string().contains("not a #RRGGBB hex color"));
        }

        let error = lua
            .load("output:fill(output, \"#000000\")")
            .set_environment(env.clone())
            .eval::<()>()
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("the region must be a `Rectangle` or a `Text`")
        );

        lua.load("output:fill(text, \"#000000\")")
            .set_environment(env.clone())
            .eval::<()>()
            .unwrap();

        lua.load("output:fill(region, \"#FF0000\")")
            .set_environment(env)
            .eval::<()>()
            .unwrap();
        let painted = output.borrow::<Image>().unwrap().clone();
        assert!(painted.buffer.r[5] > 0.0);
        assert_eq!(painted.buffer.a[5], 1.0);
    }

    fn register_vendored_font() {
        let lua = Lua::new();
        let env = lua.create_table().unwrap();
        let script_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts");
        crate::font::install(&lua, &env, &script_dir).unwrap();
        let font: mlua::Function = env.get("font").unwrap();
        font.call::<mlua::AnyUserData>("NotoSans-Regular.ttf")
            .unwrap();
    }

    #[test]
    fn the_fill_text_paints_the_ink_in_the_pinned_srgb_space() {
        register_vendored_font();
        let text = Text::new("Noto Sans", 100.0, 400.0, 10.0, 10.0, 1.0, "N");
        assert!(text.width > 0 && text.height > 0);

        let viewport = Viewport {
            width: 200,
            height: 200,
            scale: 1.0,
        };
        let mut image = Image::transparent(200, 200, viewport);
        let paint = Color::from_rgba_u8([200, 30, 60, 255]).srgb_to_linear();
        image.fill_text(&text, paint);

        let painted = image.to_rgba8();
        let (box_x, box_y) = (text.x as u32, text.y as u32);
        let (box_width, box_height) = (text.width, text.height);
        let mut solid = 0;
        let mut partial = 0;
        for y in 0..200u32 {
            for x in 0..200u32 {
                let pixel = (y * 200 + x) as usize * 4;
                let inside =
                    (x >= box_x && x < box_x + box_width) && (y >= box_y && y < box_y + box_height);
                if inside {
                    let alpha = painted[pixel + 3];
                    if alpha == 255 {
                        assert_eq!(
                            &painted[pixel..pixel + 4],
                            &[200, 30, 60, 255],
                            "pixel ({x}, {y})"
                        );
                        solid += 1;
                    } else if alpha >= 128 {
                        assert!(
                            painted[pixel].abs_diff(200) <= 2
                                && painted[pixel + 1].abs_diff(30) <= 2
                                && painted[pixel + 2].abs_diff(60) <= 2,
                            "pixel ({x}, {y})"
                        );
                        partial += 1;
                    } else if alpha > 0 {
                        partial += 1;
                    } else {
                        assert_eq!(&painted[pixel..pixel + 4], &[0, 0, 0, 0]);
                    }
                } else {
                    assert_eq!(
                        &painted[pixel..pixel + 4],
                        &[0, 0, 0, 0],
                        "pixel ({x}, {y})"
                    );
                }
            }
        }
        assert!(solid > 20);
        assert!(partial > 0);
    }

    #[test]
    fn the_fill_text_is_a_no_op_for_degenerate_text() {
        let viewport = Viewport {
            width: 4,
            height: 4,
            scale: 1.0,
        };
        let mut image = Image::transparent(4, 4, viewport);
        let paint = Color::from_rgba_u8([255, 255, 255, 255]).srgb_to_linear();

        let empty = Text::new("Noto Sans", 100.0, 400.0, 0.0, 0.0, 1.0, "");
        image.fill_text(&empty, paint);

        register_vendored_font();
        let unregistered = Text::new(
            "An Unregistered Family",
            100.0,
            400.0,
            0.0,
            0.0,
            1.0,
            "Noto",
        );
        image.fill_text(&unregistered, paint);

        assert!(image.to_rgba8().iter().all(|&byte| byte == 0));
    }

    #[test]
    fn the_fill_text_clips_at_the_canvas_corner() {
        register_vendored_font();
        let text = Text::new("Noto Sans", 100.0, 400.0, -5.0, -5.0, 1.0, "N");
        assert!(text.width > 0 && text.height > 0);

        let viewport = Viewport {
            width: 200,
            height: 200,
            scale: 1.0,
        };
        let mut image = Image::transparent(200, 200, viewport);
        let paint = Color::from_rgba_u8([200, 30, 60, 255]).srgb_to_linear();
        image.fill_text(&text, paint);

        let painted = image.to_rgba8();
        let mut solid = 0;
        for y in 0..200u32 {
            for x in 0..200u32 {
                let pixel = (y * 200 + x) as usize * 4;
                let alpha = painted[pixel + 3];
                if alpha == 255 {
                    assert_eq!(
                        &painted[pixel..pixel + 4],
                        &[200, 30, 60, 255],
                        "pixel ({x}, {y})"
                    );
                    solid += 1;
                } else if alpha >= 128 {
                    assert!(
                        painted[pixel].abs_diff(200) <= 2
                            && painted[pixel + 1].abs_diff(30) <= 2
                            && painted[pixel + 2].abs_diff(60) <= 2,
                        "pixel ({x}, {y})"
                    );
                }
            }
        }
        let pixel = (2 * 200 + 2) as usize * 4;
        assert_eq!(&painted[pixel..pixel + 4], &[200, 30, 60, 255]);
        assert!(solid > 20);
    }
}

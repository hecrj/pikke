use std::io;
use std::path::{self, PathBuf};
use std::sync::{Arc, Mutex};

use image::{ColorType, ImageFormat};
use mlua::{AnyUserData, Lua, Table, UserData, UserDataFields, UserDataMethods, Value};
use skrifa::outline::{OutlineGlyphCollection, OutlinePen};
use skrifa::prelude::{FontRef, GlyphId, LocationRef, NormalizedCoord, Size};
use vello_cpu::kurbo::{Affine, BezPath, Rect};
use vello_cpu::{
    Pixmap, RenderContext, Resources,
    color::{AlphaColor, Srgb},
};

use crate::color;
use crate::lua;
use crate::{Color, Error, Paint, Path, Rectangle, Result, Scale, Text, Viewport};

#[derive(Debug, Clone)]
pub struct Image {
    buffer: Arc<color::Buffer>,
    viewport: Viewport,
    target_scale: Scale,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dimension {
    Width(f32),
    Height(f32),
    Both { width: f32, height: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Stretch,
    Cover,
}

impl Image {
    pub fn new(scale: Scale, width: f32, height: f32, color: Color) -> Result<Self> {
        let viewport = Viewport::from_logical(width, height, scale)?;
        let count = viewport.total_pixels();
        let color = color.premultiply();

        Ok(Self {
            buffer: Arc::new(color::Buffer::new(color, count)),
            viewport,
            target_scale: scale,
        })
    }

    pub fn from_buffer(viewport: Viewport, buffer: color::Buffer) -> Result<Self> {
        let count = viewport.total_pixels();

        if buffer.len() != count {
            return Err(Error::InvalidPixelData {
                width: viewport.width(),
                height: viewport.height(),
                expected: count,
                got: buffer.len(),
            });
        }

        Ok(Self {
            buffer: Arc::new(buffer),
            viewport,
            target_scale: viewport.scale(),
        })
    }

    fn load(path: impl AsRef<path::Path>, target_scale: Scale) -> Result<Self> {
        let rgba = image::open(path)?.to_rgba8();
        let (width, height) = (rgba.width(), rgba.height());

        Ok(Self {
            buffer: Arc::new(color::Buffer::from_rgba::<color::Straight>(rgba.into_raw())),
            viewport: Viewport::from_physical_u32(width, height, Scale::default())?,
            target_scale,
        })
    }

    pub fn width(&self) -> u16 {
        self.viewport.width()
    }

    pub fn height(&self) -> u16 {
        self.viewport.height()
    }

    pub fn logical_width(&self) -> f32 {
        self.viewport.logical_width()
    }

    pub fn logical_height(&self) -> f32 {
        self.viewport.logical_height()
    }

    pub fn to_rgba8(&self) -> Vec<u8> {
        let count = self.viewport.total_pixels();
        let mut out = Vec::with_capacity(count * 4);

        for i in 0..count {
            let color = self.buffer.at(i).demultiply().to_srgb().to_rgba8();
            out.extend_from_slice(&color);
        }

        out
    }

    pub fn autocontrast(&mut self) {
        let (lo, hi) = self.buffer.luma();

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

        buffer.apply(|mut color| {
            color.r = stretch(color.r);
            color.g = stretch(color.g);
            color.b = stretch(color.b);
            color
        });
    }

    pub fn invert(&mut self) {
        let buffer = Arc::make_mut(&mut self.buffer);
        buffer.invert();
    }

    pub fn monochrome(&mut self, color: Color) {
        let buffer = Arc::make_mut(&mut self.buffer);

        buffer.apply(|pixel| {
            let shade = color.with_alpha(pixel.a);

            shade.with_luma(pixel.luma()).premultiply()
        });
    }

    pub fn resize(
        &mut self,
        dimension: Dimension,
        filter: color::Filter,
        mode: Mode,
        scale: Option<Scale>,
    ) -> Result<()> {
        let (new_width, new_height) = match dimension {
            Dimension::Width(width) => {
                (width, width * self.logical_height() / self.logical_width())
            }
            Dimension::Height(height) => (
                height * self.logical_width() / self.logical_height(),
                height,
            ),
            Dimension::Both { width, height } => (width, height),
        };

        let new_viewport =
            Viewport::from_logical(new_width, new_height, scale.unwrap_or(self.target_scale))?;

        if self.width() == new_viewport.width() && self.height() == new_viewport.height() {
            self.viewport = new_viewport;
            return Ok(());
        }

        let box_viewport = match mode {
            Mode::Cover => {
                let factor = f32::max(
                    new_viewport.width() as f32 / self.width() as f32,
                    new_viewport.height() as f32 / self.height() as f32,
                );

                Viewport::from_logical(self.logical_width(), self.logical_height(), factor)?
            }
            Mode::Stretch => new_viewport,
        };

        let left = (box_viewport.width() - new_viewport.width()) / 2;
        let top = (box_viewport.height() - new_viewport.height()) / 2;
        let (box_w, box_h) = (box_viewport.width(), box_viewport.height());
        let resized = box_w != self.width() || box_h != self.height();
        let buffer = Arc::make_mut(&mut self.buffer);

        if resized {
            let (width, height) = (self.viewport.width(), self.viewport.height());

            buffer.resize(filter, width, height, box_w, box_h);
        }

        if left != 0 || top != 0 {
            let (target_w, target_h) = (new_viewport.width(), new_viewport.height());

            buffer.crop(box_w, left, top, target_w, target_h);
        }

        if resized {
            buffer.clamp_alpha();
        }

        self.viewport = new_viewport;

        Ok(())
    }

    pub fn blend(&mut self, other: &Image, at: Rectangle, space: color::Space) -> Result<()> {
        let scale = self.viewport.scale();

        let other = if other.viewport.scale() != scale {
            let mut other = other.clone();

            other.resize(
                Dimension::Both {
                    width: other.logical_width(),
                    height: other.logical_height(),
                },
                color::Filter::default(),
                Mode::Stretch,
                Some(scale),
            )?;

            other
        } else {
            other.clone()
        };

        let at_x = scale.to_physical_signed(at.x)?;
        let at_y = scale.to_physical_signed(at.y)?;
        let left = at_x.max(0);
        let top = at_y.max(0);
        let right = (at_x + other.width() as i32).min(self.width() as i32);
        let bottom = (at_y + other.height() as i32).min(self.height() as i32);

        if left >= right || top >= bottom {
            return Ok(());
        }

        let buffer = Arc::make_mut(&mut self.buffer);

        let width = (right - left) as usize;
        let layer_left = (left - at_x) as usize;
        let self_width = self.viewport.width() as usize;
        let other_width = other.width() as usize;

        match space {
            color::Space::Linear => {
                for y in top..bottom {
                    let layer = other.buffer.row((y - at_y) as usize, other_width);
                    let layer = layer.crop(layer_left, layer_left + width);

                    let mut base = buffer
                        .row_mut(y as usize, self_width)
                        .crop(left as usize, right as usize);

                    for (mut base, layer) in base.iter_mut().zip(layer.iter()) {
                        base.put(base.color().over(layer));
                    }
                }
            }

            color::Space::Srgb => {
                for y in top..bottom {
                    let layer = other.buffer.row((y - at_y) as usize, other_width);
                    let layer = layer.crop(layer_left, layer_left + width);

                    let mut base = buffer
                        .row_mut(y as usize, self_width)
                        .crop(left as usize, right as usize);

                    for (mut base, layer) in base.iter_mut().zip(layer.iter()) {
                        let blended = base
                            .color()
                            .demultiply()
                            .to_srgb_fast()
                            .premultiply()
                            .over(layer.demultiply().to_srgb_fast().premultiply())
                            .demultiply()
                            .to_linear_fast()
                            .premultiply();

                        base.put(blended);
                    }
                }
            }
        }

        Ok(())
    }

    pub fn fill(&mut self, fill: impl Into<Geometry>, paint: impl Into<Paint>) {
        let mut resources = Resources::new();
        let mut context = RenderContext::new(self.width(), self.height());
        context.set_paint(AlphaColor::<Srgb>::BLACK);

        let geometry = fill.into();

        let bounds = match &geometry {
            Geometry::Rectangle(region) => {
                let scale = self.viewport.scale();
                let left = region.x * scale;
                let top = region.y * scale;
                let right = (region.x + region.width) * scale;
                let bottom = (region.y + region.height) * scale;

                if region.width <= 0.0 || region.height <= 0.0 {
                    return;
                }

                if !left.is_finite()
                    || !top.is_finite()
                    || !right.is_finite()
                    || !bottom.is_finite()
                {
                    return;
                }

                let rect = Rect::new(left.into(), top.into(), right.into(), bottom.into()).round();
                context.fill_rect(&rect);

                (
                    rect.x0 as f32,
                    rect.y0 as f32,
                    rect.x1 as f32,
                    rect.y1 as f32,
                )
            }
            Geometry::Path(path) => {
                let path = path.snap(self.viewport.scale());
                let bounds = path.bounds();

                context.fill_path(path.as_ref());

                (
                    bounds.x0 as f32,
                    bounds.y0 as f32,
                    bounds.x1 as f32,
                    bounds.y1 as f32,
                )
            }
        };

        let mut pixmap = Pixmap::new(self.width(), self.height());
        context.render(&mut pixmap, &mut resources);

        let buffer = Arc::make_mut(&mut self.buffer);

        match paint.into() {
            Paint::Color(color) => {
                let color = color.premultiply();

                for (i, pixel) in pixmap.data().iter().enumerate() {
                    let cover = pixel.a as f32 / 255.0;

                    if cover > 0.0 {
                        let base = buffer.at(i);
                        let layer = color.scale(cover);

                        buffer.put(i, base.over(layer));
                    }
                }
            }
            Paint::Gradient(gradient) => {
                let width = pixmap.width() as usize;
                let (dx, dy) = gradient.axis();
                let (min_x, min_y, max_x, max_y) = bounds;

                let origin_x = if dx >= 0.0 { min_x } else { max_x };
                let origin_y = if dy >= 0.0 { min_y } else { max_y };
                let min_projection = origin_x * dx + origin_y * dy;
                let span = dx.abs() * (max_x - min_x) + dy.abs() * (max_y - min_y);

                for (y, row) in pixmap.data().chunks_exact(width).enumerate() {
                    for (x, pixel) in row.iter().enumerate() {
                        let cover = pixel.a as f32 / 255.0;

                        if cover <= 0.0 {
                            continue;
                        }

                        let t = if span > 0.0 {
                            ((x as f32 + 0.5) * dx + (y as f32 + 0.5) * dy - min_projection) / span
                        } else {
                            0.0
                        };
                        let color = gradient.color_at(t);
                        let i = y * width + x;
                        let base = buffer.at(i);
                        let layer = color.premultiply().scale(cover);

                        buffer.put(i, base.over(layer));
                    }
                }
            }
        }
    }

    pub fn fill_text(&mut self, text: &Text, paint: impl Into<Paint>) -> Result<()> {
        let (box_width, box_height) = (text.viewport.width(), text.viewport.height());

        if box_width == 0 || box_height == 0 {
            return Ok(());
        }

        let scale = text.viewport.scale();
        let (box_x, box_y) = (
            scale.to_physical_signed(text.x)?,
            scale.to_physical_signed(text.y)?,
        );
        let (ink_x, ink_y) = (
            scale.to_physical_signed(text.ink_min.0)?,
            scale.to_physical_signed(text.ink_min.1)?,
        );

        let (ink_width, ink_height) = (self.viewport.width() as i32, self.viewport.height() as i32);
        let raster_x0 = (box_x - 1).max(0);
        let raster_y0 = (box_y - 1).max(0);
        let raster_x1 = (box_x + box_width as i32 + 1).min(ink_width);
        let raster_y1 = (box_y + box_height as i32 + 1).min(ink_height);

        if raster_x1 <= raster_x0 || raster_y1 <= raster_y0 {
            return Ok(());
        }

        let (raster_width, raster_height) = (
            (raster_x1 - raster_x0) as u16,
            (raster_y1 - raster_y0) as u16,
        );
        let (layout_x, layout_y) = (box_x - ink_x, box_y - ink_y);

        let mut resources = Resources::new();
        let mut context = RenderContext::new(raster_width, raster_height);
        context.set_transform(Affine::translate((
            (layout_x - raster_x0) as f64,
            (layout_y - raster_y0) as f64,
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

        let mut pixmap = Pixmap::new(raster_width, raster_height);
        context.render(&mut pixmap, &mut resources);

        let buffer = Arc::make_mut(&mut self.buffer);
        let paint = paint.into();

        let (dx, dy, min_projection, span) = match &paint {
            Paint::Color(_) => (0.0, 0.0, 0.0, 1.0),
            Paint::Gradient(gradient) => {
                let (dx, dy) = gradient.axis();
                let origin_x = if dx >= 0.0 {
                    box_x as f32
                } else {
                    (box_x + box_width as i32) as f32
                };
                let origin_y = if dy >= 0.0 {
                    box_y as f32
                } else {
                    (box_y + box_height as i32) as f32
                };
                let min_projection = origin_x * dx + origin_y * dy;
                let span = dx.abs() * box_width as f32 + dy.abs() * box_height as f32;

                (dx, dy, min_projection, span)
            }
        };

        for (y, row) in pixmap
            .data()
            .chunks_exact(raster_width as usize)
            .enumerate()
        {
            for (x, pixel) in row.iter().enumerate() {
                let cover = pixel.a as f32 / 255.0;

                if cover <= 0.0 {
                    continue;
                }

                let canvas_x = raster_x0 + x as i32;
                let canvas_y = raster_y0 + y as i32;
                let i = (canvas_y * ink_width + canvas_x) as usize;
                let base = buffer.at(i);

                let layer = match &paint {
                    Paint::Color(color) => color.premultiply().scale(cover),
                    Paint::Gradient(gradient) => {
                        let projection =
                            (canvas_x as f32 + 0.5) * dx + (canvas_y as f32 + 0.5) * dy;
                        let t = if span > 0.0 {
                            (projection - min_projection) / span
                        } else {
                            0.0
                        };

                        gradient.color_at(t).premultiply().scale(cover)
                    }
                };

                let blended = base
                    .demultiply()
                    .to_srgb()
                    .premultiply()
                    .over(layer.demultiply().to_srgb().premultiply())
                    .demultiply()
                    .to_linear()
                    .premultiply();

                buffer.put(i, blended);
            }
        }

        Ok(())
    }

    pub fn pixel(&self, x: f32, y: f32) -> Result<Option<color::Pixel>> {
        let scale = self.viewport.scale();
        let left = scale.to_physical_signed(x)?;
        let top = scale.to_physical_signed(y)?;

        if left < 0 || top < 0 || left >= self.width() as i32 || top >= self.height() as i32 {
            return Ok(None);
        }

        Ok(Some(
            self.buffer
                .at((top * self.width() as i32 + left) as usize)
                .demultiply()
                .to_srgb()
                .to_pixel(),
        ))
    }

    pub fn luma_in(&self, region: Rectangle) -> Result<f32> {
        let scale = self.viewport.scale();
        let left = scale.to_physical_signed(region.x)?;
        let top = scale.to_physical_signed(region.y)?;
        let right = scale.to_physical_signed(region.x + region.width)?;
        let bottom = scale.to_physical_signed(region.y + region.height)?;

        let left = left.max(0).min(self.width() as i32);
        let top = top.max(0).min(self.height() as i32);
        let right = right.max(0).min(self.width() as i32);
        let bottom = bottom.max(0).min(self.height() as i32);

        if left >= right || top >= bottom {
            return Ok(0.0);
        }

        let width = self.width() as usize;
        let (left, right) = (left as usize, right as usize);
        let mut luma = 0.0f32;

        for row in top as usize..bottom as usize {
            let row = self.buffer.row(row, width).crop(left, right);

            for color in row.iter() {
                luma += color.luma();
            }
        }

        let count = (right - left) * (bottom - top) as usize;

        Ok(luma / count as f32 * 255.0)
    }

    pub fn dominant_in(&self, region: Rectangle) -> Result<Color> {
        #[derive(Clone, Default)]
        struct Bucket {
            weight: f32,
            r: f32,
            g: f32,
            b: f32,
            a: f32,
        }

        let scale = self.viewport.scale();
        let left = scale.to_physical_signed(region.x)?;
        let top = scale.to_physical_signed(region.y)?;
        let right = scale.to_physical_signed(region.x + region.width)?;
        let bottom = scale.to_physical_signed(region.y + region.height)?;

        let left = left.max(0).min(self.width() as i32);
        let top = top.max(0).min(self.height() as i32);
        let right = right.max(0).min(self.width() as i32);
        let bottom = bottom.max(0).min(self.height() as i32);

        if left >= right || top >= bottom {
            return Ok(Color::TRANSPARENT);
        }

        let width = self.width() as usize;
        let (left, right) = (left as usize, right as usize);

        let mut buckets = vec![Bucket::default(); 16 * 16 * 16];

        for row in top as usize..bottom as usize {
            let row = self.buffer.row(row, width).crop(left, right);

            for color in row.iter() {
                let a = color.a;

                if a > 0.0 {
                    let r = (color.r / a).clamp(0.0, 1.0);
                    let g = (color.g / a).clamp(0.0, 1.0);
                    let b = (color.b / a).clamp(0.0, 1.0);

                    let bucket_index = (r * 15.0).round() as usize
                        + (g * 15.0).round() as usize * 16
                        + (b * 15.0).round() as usize;

                    let bucket = &mut buckets[bucket_index];
                    bucket.weight += a;
                    bucket.r += r * a;
                    bucket.g += g * a;
                    bucket.b += b * a;
                    bucket.a += a * a;
                }
            }
        }

        let best = buckets
            .iter()
            .enumerate()
            .max_by(|(_, first), (_, second)| {
                first
                    .weight
                    .partial_cmp(&second.weight)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(index, _)| index)
            .unwrap_or(0);
        let bucket = &buckets[best];

        if bucket.weight == 0.0 {
            return Ok(Color::TRANSPARENT);
        }

        let alpha = bucket.a / bucket.weight;

        Ok(Color::from_rgba([
            bucket.r / bucket.weight * alpha,
            bucket.g / bucket.weight * alpha,
            bucket.b / bucket.weight * alpha,
            alpha,
        ])
        .demultiply())
    }

    pub fn coverage_in(&self, region: Rectangle) -> Result<usize> {
        let scale = self.viewport.scale();
        let left = scale.to_physical_signed(region.x)?;
        let top = scale.to_physical_signed(region.y)?;
        let right = scale.to_physical_signed(region.x + region.width)?;
        let bottom = scale.to_physical_signed(region.y + region.height)?;

        let left = left.max(0).min(self.width() as i32);
        let top = top.max(0).min(self.height() as i32);
        let right = right.max(0).min(self.width() as i32);
        let bottom = bottom.max(0).min(self.height() as i32);

        if left >= right || top >= bottom {
            return Ok(0);
        }

        let width = self.width() as usize;
        let (left, right) = (left as usize, right as usize);
        let mut covered = 0usize;

        for row in top as usize..bottom as usize {
            let row = self.buffer.row(row, width).crop(left, right);

            for color in row.iter() {
                covered += (color.a > 0.0) as usize;
            }
        }

        Ok(covered)
    }

    pub fn write(&self, output: &path::Path) -> Result<()> {
        let file_name = output.file_name().ok_or_else(|| {
            io::Error::other(format!("invalid output path: {}", output.display()))
        })?;

        let mut temp_name = file_name.to_os_string();
        temp_name.push(".tmp");
        let temp = output.with_file_name(temp_name);

        let rgba8 = self.to_rgba8();
        image::save_buffer_with_format(
            &temp,
            &rgba8,
            u32::from(self.width()),
            u32::from(self.height()),
            ColorType::Rgba8,
            ImageFormat::Png,
        )
        .inspect_err(|_| {
            let _ = std::fs::remove_file(&temp);
        })?;

        std::fs::rename(&temp, output)?;

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct Reference(Arc<Mutex<Thunk>>);

impl Reference {
    pub fn new(image: Image) -> Self {
        Self(Arc::new(Mutex::new(Thunk::Image(image))))
    }

    pub fn image(&self) -> Result<Image> {
        self.evaluate(|image| Ok(image.clone()))
    }

    fn blank(scale: Scale, width: f32, height: f32, color: Color) -> Self {
        Self(Arc::new(Mutex::new(Thunk::Blank {
            scale,
            width,
            height,
            color,
        })))
    }

    fn load(path: impl AsRef<path::Path>, scale: Scale) -> Self {
        Self(Arc::new(Mutex::new(Thunk::File {
            path: path.as_ref().to_path_buf(),
            scale,
        })))
    }

    fn resize(
        &mut self,
        dimension: Dimension,
        filter: color::Filter,
        mode: Mode,
        scale: Option<Scale>,
    ) {
        self.replace(|source| Thunk::Resize {
            source,
            dimension,
            filter,
            mode,
            scale,
        });
    }

    fn autocontrast(&mut self) {
        self.replace(|source| Thunk::Autocontrast { source });
    }

    fn invert(&mut self) {
        self.replace(|source| Thunk::Invert { source });
    }

    fn monochrome(&mut self, color: Color) {
        self.replace(|source| Thunk::Monochrome { source, color });
    }

    fn blend(&mut self, other: &Reference, at: Rectangle, space: color::Space) {
        self.replace(|source| Thunk::Blend {
            source,
            other: other.clone(),
            at,
            space,
        });
    }

    fn fill(&mut self, geometry: impl Into<Geometry>, paint: impl Into<Paint>) {
        self.replace(|source| Thunk::Fill {
            source,
            geometry: geometry.into(),
            paint: paint.into(),
        });
    }

    fn fill_text(&mut self, text: &Text, paint: impl Into<Paint>) {
        self.replace(|source| Thunk::FillText {
            source,
            text: text.clone(),
            paint: paint.into(),
        });
    }

    fn logical_width(&self) -> Result<f32> {
        self.evaluate(|image| Ok(image.logical_width()))
    }

    fn logical_height(&self) -> Result<f32> {
        self.evaluate(|image| Ok(image.logical_height()))
    }

    fn luma_in(&self, region: Rectangle) -> Result<f32> {
        self.evaluate(|image| image.luma_in(region))
    }

    fn pixel(&self, x: f32, y: f32) -> Result<Option<color::Pixel>> {
        self.evaluate(|image| image.pixel(x, y))
    }

    fn dominant_in(&self, region: Rectangle) -> Result<Color> {
        self.evaluate(|image| image.dominant_in(region))
    }

    fn coverage_in(&self, region: Rectangle) -> Result<usize> {
        self.evaluate(|image| image.coverage_in(region))
    }

    fn evaluate<T>(&self, f: impl FnOnce(&mut Image) -> Result<T>) -> Result<T> {
        let mut thunk = self.0.lock().expect("lock image reference");

        if !matches!(*thunk, Thunk::Image(_)) {
            *thunk = Thunk::Image(thunk.evaluate()?);
        };

        let Thunk::Image(image) = &mut *thunk else {
            unreachable!();
        };

        f(image)
    }

    fn replace(&mut self, thunk: impl FnOnce(Self) -> Thunk) {
        self.0 = Arc::new(Mutex::new(if Arc::strong_count(&self.0) == 1 {
            thunk(self.clone())
        } else {
            thunk(Self(Arc::new(Mutex::new(Thunk::Snapshot(self.clone())))))
        }));
    }
}

#[derive(Debug, Clone)]
pub enum Thunk {
    Blank {
        scale: Scale,
        width: f32,
        height: f32,
        color: Color,
    },
    File {
        path: PathBuf,
        scale: Scale,
    },
    Image(Image),
    Snapshot(Reference),
    Resize {
        source: Reference,
        dimension: Dimension,
        filter: color::Filter,
        mode: Mode,
        scale: Option<Scale>,
    },
    Autocontrast {
        source: Reference,
    },
    Invert {
        source: Reference,
    },
    Monochrome {
        source: Reference,
        color: Color,
    },
    Blend {
        source: Reference,
        other: Reference,
        at: Rectangle,
        space: color::Space,
    },
    Fill {
        source: Reference,
        geometry: Geometry,
        paint: Paint,
    },
    FillText {
        source: Reference,
        text: Text,
        paint: Paint,
    },
}

impl Thunk {
    fn evaluate(&self) -> Result<Image> {
        match self {
            Thunk::Blank {
                scale,
                width,
                height,
                color,
            } => Image::new(*scale, *width, *height, *color),
            Thunk::File { path, scale } => Image::load(path, *scale),
            Thunk::Image(image) => Ok(image.clone()),
            Thunk::Snapshot(reference) => reference.evaluate(|image| Ok(image.clone())),
            Thunk::Resize {
                source,
                dimension,
                filter,
                mode,
                scale,
            } => source.evaluate(|source| {
                source.resize(*dimension, *filter, *mode, *scale)?;
                Ok(source.clone())
            }),
            Thunk::Autocontrast { source } => source.evaluate(|source| {
                source.autocontrast();
                Ok(source.clone())
            }),
            Thunk::Invert { source } => source.evaluate(|source| {
                source.invert();
                Ok(source.clone())
            }),
            Thunk::Monochrome { source, color } => source.evaluate(|source| {
                source.monochrome(*color);
                Ok(source.clone())
            }),
            Thunk::Blend {
                source,
                other,
                at,
                space,
            } => source.evaluate(|source| {
                other.evaluate(|other| {
                    source.blend(other, *at, *space)?;
                    Ok(source.clone())
                })
            }),
            Thunk::Fill {
                source,
                geometry,
                paint,
            } => source.evaluate(|source| {
                source.fill(geometry.clone(), paint.clone());
                Ok(source.clone())
            }),
            Thunk::FillText {
                source,
                text,
                paint,
            } => source.evaluate(|source| {
                source.fill_text(text, paint.clone())?;
                Ok(source.clone())
            }),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Geometry {
    Rectangle(Rectangle),
    Path(Path),
}

impl From<Rectangle> for Geometry {
    fn from(rectangle: Rectangle) -> Self {
        Self::Rectangle(rectangle)
    }
}

impl From<Path> for Geometry {
    fn from(path: Path) -> Self {
        Self::Path(path)
    }
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

impl UserData for Reference {
    fn add_fields<F: UserDataFields<Reference>>(fields: &mut F) {
        fields.add_meta_field("__name", "Image");
        fields.add_field_method_get("width", |_lua, this| -> mlua::Result<f32> {
            Ok(this.logical_width()?)
        });
        fields.add_field_method_get("height", |_lua, this| -> mlua::Result<f32> {
            Ok(this.logical_height()?)
        });
        fields.add_field_method_get("size", |lua, this| -> mlua::Result<Table> {
            let size = lua.create_table()?;
            size.set(1, this.logical_width()?)?;
            size.set(2, this.logical_height()?)?;
            Ok(size)
        });
    }

    fn add_methods<M: UserDataMethods<Reference>>(methods: &mut M) {
        methods.add_method(
            "clone",
            |_lua, this, _args: ()| -> mlua::Result<Reference> { Ok(this.clone()) },
        );
        methods.add_method_mut("resize", |_lua, this, opts: Table| -> mlua::Result<()> {
            let width: Option<f32> = opts.get("width")?;
            let height: Option<f32> = opts.get("height")?;
            let filter: Option<String> = opts.get("filter")?;
            let mode: Option<String> = opts.get("mode")?;

            let dimension = match (width, height) {
                (Some(width), Some(height)) => Dimension::Both { width, height },
                (Some(width), None) => Dimension::Width(width),
                (None, Some(height)) => Dimension::Height(height),
                (None, None) => {
                    return Err(mlua::Error::RuntimeError(
                        "resize: needs width or height".to_owned(),
                    ));
                }
            };

            let filter = match filter.as_deref() {
                None => color::Filter::default(),
                Some("cubic") => color::Filter::Cubic,
                Some("lanczos") => color::Filter::Lanczos,
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

            this.resize(dimension, filter, mode, None);
            Ok(())
        });
        methods.add_method_mut(
            "autocontrast",
            |_lua, this, _args: ()| -> mlua::Result<()> {
                this.autocontrast();
                Ok(())
            },
        );
        methods.add_method_mut("invert", |_lua, this, _args: ()| -> mlua::Result<()> {
            this.invert();
            Ok(())
        });
        methods.add_method_mut(
            "monochrome",
            |_lua, this, color: Value| -> mlua::Result<()> {
                let color = lua::color("monochrome", &color)?;
                this.monochrome(color);
                Ok(())
            },
        );
        methods.add_method_mut(
            "blend",
            |lua, this, (other, opts): (AnyUserData, Option<Table>)| -> mlua::Result<()> {
                let opts = match opts {
                    Some(opts) => opts,
                    None => lua.create_table()?,
                };

                let other = other.borrow::<Reference>()?;
                let at: Option<AnyUserData> = opts.get("at")?;
                let at = match at {
                    Some(at) => *at.borrow::<Rectangle>()?,
                    None => Rectangle::default(),
                };
                let color_space: Option<String> = opts.get("color_space")?;
                let space = match color_space.as_deref() {
                    None | Some("linear") => color::Space::Linear,
                    Some("srgb") => color::Space::Srgb,
                    Some(other) => {
                        return Err(mlua::Error::RuntimeError(format!(
                            "blend: unknown color_space: {other}"
                        )));
                    }
                };

                this.blend(&other, at, space);
                Ok(())
            },
        );
        methods.add_method_mut(
            "fill",
            |_lua, this, (region, paint): (AnyUserData, Value)| -> mlua::Result<()> {
                let paint = lua::paint("fill", &paint)?;

                if let Ok(rectangle) = region.borrow::<Rectangle>() {
                    this.fill(*rectangle, paint);
                    return Ok(());
                }

                if let Ok(text) = region.borrow::<Text>() {
                    this.fill_text(&text, paint);
                    return Ok(());
                }

                let path = region.borrow::<crate::Path>().map_err(|_| {
                    mlua::Error::RuntimeError(
                        "fill: the region must be a `Rectangle`, a `Text`, or a `Path`".to_owned(),
                    )
                })?;
                this.fill(path.clone(), paint);
                Ok(())
            },
        );
        methods.add_method(
            "luma_in",
            |_lua, this, region: AnyUserData| -> mlua::Result<f32> {
                let region: Rectangle = *region.borrow::<Rectangle>()?;
                Ok(this.luma_in(region)?)
            },
        );

        methods.add_method(
            "dominant_in",
            |_lua, this, region: AnyUserData| -> mlua::Result<Color> {
                let region: Rectangle = *region.borrow::<Rectangle>()?;
                Ok(this.dominant_in(region)?)
            },
        );

        methods.add_method(
            "coverage_in",
            |_lua, this, region: AnyUserData| -> mlua::Result<usize> {
                let region: Rectangle = *region.borrow::<Rectangle>()?;
                Ok(this.coverage_in(region)?)
            },
        );

        methods.add_method(
            "pixel",
            |_lua, this, (x, y): (f32, f32)| -> mlua::Result<Option<color::Pixel>> {
                Ok(this.pixel(x, y)?)
            },
        );
    }
}

pub fn install(lua: &Lua, env: &Table, scale: Scale) -> Result<()> {
    env.set(
        "image",
        lua.create_function(move |lua, path: String| -> mlua::Result<Reference> {
            let module_dir = lua::module_dir(lua);
            Ok(Reference::load(module_dir.join(&path), scale))
        })?,
    )?;
    env.set(
        "blank",
        lua.create_function(move |_lua, opts: Table| -> mlua::Result<Reference> {
            let width: Option<f32> = opts.get("width")?;
            let height: Option<f32> = opts.get("height")?;
            let color: Option<String> = opts.get("color")?;

            let width = width
                .ok_or_else(|| mlua::Error::RuntimeError("blank: needs 'width'".to_owned()))?;
            let height = height
                .ok_or_else(|| mlua::Error::RuntimeError("blank: needs 'height'".to_owned()))?;

            if width <= 0.0 || !width.is_finite() {
                return Err(mlua::Error::RuntimeError(
                    "blank: 'width' must be a positive number".to_owned(),
                ));
            }

            if height <= 0.0 || !height.is_finite() {
                return Err(mlua::Error::RuntimeError(
                    "blank: 'height' must be a positive number".to_owned(),
                ));
            }

            let paint = if let Some(color) = color {
                lua::hex("blank", &color)?
            } else {
                Color::TRANSPARENT
            };

            Ok(Reference::blank(scale, width, height, paint))
        })?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use super::{Dimension, Image, Mode, Reference};
    use crate::color;
    use crate::text::Text;
    use crate::{Color, Rectangle, Result, Scale, Viewport};

    #[test]
    fn the_base_background_loads_at_its_file_dims() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("images/Lenna.png");
        let file = std::fs::read(&path).unwrap();

        let file_width = u32::from_be_bytes([file[16], file[17], file[18], file[19]]);
        let file_height = u32::from_be_bytes([file[20], file[21], file[22], file[23]]);

        let image = Image::load(&path, Scale::default()).unwrap();
        assert_eq!(
            (image.logical_width(), image.logical_height()),
            (file_width as f32, file_height as f32)
        );

        let count = (file_width * file_height) as usize;

        assert_eq!(image.to_rgba8().len(), count * 4);
    }

    #[test]
    fn the_blank_paints_the_whole_canvas() -> Result<()> {
        let paint = Color::from_pixel(color::Pixel::from_rgba8([200, 30, 60, 255])).to_linear();
        let image = Image::new(Scale::default(), 4.0, 2.0, paint)?;

        assert_eq!((image.width(), image.height()), (4, 2));
        for pixel in image.to_rgba8().chunks(4) {
            assert_eq!(pixel, &[200, 30, 60, 255]);
        }

        Ok(())
    }

    #[test]
    fn the_blank_sizes_its_buffer_at_the_run_scale() -> Result<()> {
        let paint = Color::from_pixel(color::Pixel::from_rgba8([255, 255, 255, 255])).to_linear();
        let image = Image::new(Scale::new(0.5), 300.0, 100.0, paint)?;

        assert_eq!((image.width(), image.height()), (150, 50));
        assert_eq!(image.to_rgba8().len(), 150 * 50 * 4);
        assert_eq!(
            (image.logical_width(), image.logical_height()),
            (300.0, 100.0)
        );

        Ok(())
    }

    #[test]
    fn resize_lands_on_its_targets_and_forks() -> Result<()> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("images/Lenna.png");
        let mut image = Image::load(&path, Scale::new(0.5)).unwrap();
        let first_pixel = image.buffer.at(0).r;

        let mut fork = image.clone();

        image.resize(
            Dimension::Both {
                width: 200.0,
                height: 250.0,
            },
            color::Filter::Lanczos,
            Mode::Stretch,
            Some(Scale::new(0.5)),
        )?;
        assert_eq!((image.width(), image.height()), (100, 125));

        assert_eq!((fork.width(), fork.height()), (512, 512));
        assert_eq!(fork.buffer.at(0).r, first_pixel);

        fork.resize(
            Dimension::Both {
                width: 400.0,
                height: 100.0,
            },
            color::Filter::Cubic,
            Mode::Cover,
            Some(Scale::new(0.5)),
        )?;
        assert_eq!((fork.width(), fork.height()), (200, 50));

        let mut image = Image::load(&path, Scale::new(2.0)).unwrap();
        image.resize(
            Dimension::Both {
                width: 256.0,
                height: 256.0,
            },
            color::Filter::Cubic,
            Mode::Stretch,
            Some(Scale::new(2.0)),
        )?;
        assert_eq!(image.viewport.scale(), Scale::new(2.0));
        assert_eq!(
            (image.logical_width(), image.logical_height()),
            (256.0, 256.0)
        );
        assert_eq!((image.width(), image.height()), (512, 512));

        Ok(())
    }

    #[test]
    fn the_resize_keeps_a_constant_plane_constant() -> Result<()> {
        let expected = Color::from_rgba([0.5, 0.5, 0.5, 1.0])
            .demultiply()
            .to_srgb()
            .to_rgba8();

        for filter in [color::Filter::Cubic, color::Filter::Lanczos] {
            for target in [4.0, 16.0] {
                let mut image = square([0.5, 0.5, 0.5, 1.0], 8, Scale::default())?;
                image.resize(
                    Dimension::Both {
                        width: target,
                        height: target,
                    },
                    filter,
                    Mode::Stretch,
                    None,
                )?;
                for pixel in image.to_rgba8().chunks(4) {
                    assert_eq!(pixel, &expected);
                }
            }
        }

        Ok(())
    }

    #[test]
    fn the_resize_keeps_the_aspect_for_a_single_dimension() -> Result<()> {
        let color = Color::from_pixel(color::Pixel::from_rgba8([200, 30, 60, 255])).to_linear();

        let mut image = Image::new(Scale::default(), 64.0, 32.0, color)?;
        image.resize(
            Dimension::Width(128.0),
            color::Filter::default(),
            Mode::default(),
            None,
        )?;
        assert_eq!(
            (image.logical_width(), image.logical_height()),
            (128.0, 64.0)
        );

        let mut image = Image::new(Scale::default(), 64.0, 32.0, color)?;
        image.resize(
            Dimension::Height(16.0),
            color::Filter::default(),
            Mode::default(),
            None,
        )?;
        assert_eq!(
            (image.logical_width(), image.logical_height()),
            (32.0, 16.0)
        );

        Ok(())
    }

    #[test]
    fn the_luma_in_is_the_709_mean_of_the_stored_channels() -> Result<()> {
        let r: [f32; 16] = [
            0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1.0, 0.0, 0.1, 0.2, 0.3, 0.4, 0.5,
        ];
        let mut image = Image {
            buffer: Arc::new(color::Buffer::new(Color::TRANSPARENT, 16)),
            viewport: Viewport::from_physical_u32(4, 4, Scale::default())?,
            target_scale: Scale::default(),
        };
        {
            let buffer = Arc::make_mut(&mut image.buffer);
            for (i, &channel) in r.iter().enumerate() {
                buffer.put(i, Color::from_rgba([channel, 0.5, 0.6, 1.0]));
            }
        }

        let luma = |r: f32| 0.2126 * r as f64 + 0.7152 * 0.5 + 0.0722 * 0.6;
        let mean = |indices: &[usize]| {
            indices.iter().map(|i| luma(r[*i])).sum::<f64>() / indices.len() as f64 * 255.0
        };

        let full: Vec<usize> = (0..16).collect();
        assert!(
            (image.luma_in(Rectangle::new(0.0, 0.0, 4.0, 4.0))? as f64 - mean(&full)).abs() < 1e-2
        );

        let top_left = vec![0, 1, 4, 5];
        assert!(
            (image.luma_in(Rectangle::new(0.0, 0.0, 2.0, 2.0))? as f64 - mean(&top_left)).abs()
                < 1e-2
        );

        let right = vec![3, 7, 11, 15];
        assert!(
            (image.luma_in(Rectangle::new(3.0, 0.0, 1.0, 4.0))? as f64 - mean(&right)).abs() < 1e-2
        );

        let clipped = vec![0];
        assert!(
            (image.luma_in(Rectangle::new(-1.0, -1.0, 2.0, 2.0))? as f64 - mean(&clipped)).abs()
                < 1e-2
        );

        assert_eq!(image.luma_in(Rectangle::new(10.0, 10.0, 2.0, 2.0))?, 0.0);
        assert_eq!(image.luma_in(Rectangle::new(0.0, 0.0, -2.0, 2.0))?, 0.0);

        Ok(())
    }

    #[test]
    fn the_luma_in_reads_regions_in_logical_px() -> Result<()> {
        let image = square([0.5, 0.5, 0.5, 1.0], 4, Scale::new(0.5))?;

        assert!((image.luma_in(Rectangle::new(0.0, 0.0, 8.0, 8.0))? - 127.5).abs() < 1e-2);
        assert!((image.luma_in(Rectangle::new(0.0, 0.0, 4.0, 4.0))? - 127.5).abs() < 1e-2);
        assert_eq!(image.luma_in(Rectangle::new(8.0, 8.0, 4.0, 4.0))?, 0.0);

        Ok(())
    }

    #[test]
    fn the_luma_in_matches_the_709_mean_on_a_real_image() -> Result<()> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("images/Lenna.png");
        let image = Image::load(&path, Scale::default()).unwrap();

        let luma = image.luma_in(Rectangle::new(10.0, 20.0, 100.0, 50.0))?;

        let sum: f32 = (20..70)
            .flat_map(|y| (10..110).map(move |x| y * 512 + x))
            .map(|i| image.buffer.at(i).luma())
            .sum();
        let expected = sum / 5000.0 * 255.0;
        assert!((luma - expected).abs() < 1e-2);

        Ok(())
    }

    #[test]
    fn the_luma_in_matches_the_independent_709_mean() -> Result<()> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("personal/images/sky-night.png");
        let image = Image::load(&path, Scale::default()).unwrap();

        // Frozen oracles: an independent Python pass (PIL, the IEC 61966-2-1
        // decode, the Rec. 709 mean) over personal/images/sky-night.png.
        let corner = image.luma_in(Rectangle::new(10.0, 20.0, 300.0, 200.0))?;
        assert!((corner - 2.7941046).abs() < 1e-2);

        let full = image.luma_in(Rectangle::new(0.0, 0.0, 1640.0, 2048.0))?;
        assert!((full - 11.452538).abs() < 1e-2);

        Ok(())
    }

    #[test]
    fn the_dominant_in_is_the_mean_of_the_dominant_bucket() -> Result<()> {
        let viewport = Viewport::from_logical(4.0, 4.0, Scale::default())?;
        let mut rgba = vec![0u8; 16 * 4];
        rgba[..12 * 4].copy_from_slice(&[200, 30, 60, 255].repeat(12));
        rgba[12 * 4..].copy_from_slice(&[0, 0, 255, 255].repeat(4));
        let image = Image::from_buffer(
            viewport,
            color::Buffer::from_rgba::<color::Straight>(rgba.as_slice()),
        )?;

        let dominant = image.dominant_in(Rectangle::new(0.0, 0.0, 4.0, 4.0))?;

        assert_eq!(dominant.to_srgb().to_rgba8(), [200, 30, 60, 255]);

        Ok(())
    }

    #[test]
    fn the_dominant_in_blends_the_members_of_its_bucket() -> Result<()> {
        let viewport = Viewport::from_logical(2.0, 1.0, Scale::default())?;
        let rgba = [200, 30, 60, 255, 202, 32, 62, 255];
        let image = Image::from_buffer(
            viewport,
            color::Buffer::from_rgba::<color::Straight>(rgba.as_slice()),
        )?;

        let dominant = image.dominant_in(Rectangle::new(0.0, 0.0, 2.0, 1.0))?;

        assert_eq!(dominant.to_srgb().to_rgba8(), [201, 31, 61, 255]);

        Ok(())
    }

    #[test]
    fn the_dominant_in_ignores_the_transparent_pixels() -> Result<()> {
        let viewport = Viewport::from_logical(2.0, 2.0, Scale::default())?;
        let rgba = [0, 255, 0, 255, 255, 0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0];
        let image = Image::from_buffer(
            viewport,
            color::Buffer::from_rgba::<color::Straight>(rgba.as_slice()),
        )?;

        let dominant = image.dominant_in(Rectangle::new(0.0, 0.0, 2.0, 2.0))?;

        assert_eq!(dominant.to_srgb().to_rgba8(), [0, 255, 0, 255]);

        Ok(())
    }

    #[test]
    fn the_dominant_in_is_transparent_outside_the_image_or_on_a_transparent_region() -> Result<()> {
        let image = Image::new(Scale::default(), 4.0, 4.0, Color::TRANSPARENT)?;

        assert_eq!(
            image.dominant_in(Rectangle::new(0.0, 0.0, 4.0, 4.0))?,
            Color::TRANSPARENT
        );
        assert_eq!(
            image.dominant_in(Rectangle::new(10.0, 10.0, 2.0, 2.0))?,
            Color::TRANSPARENT
        );

        Ok(())
    }

    fn synthetic_linear(pixels: &[[f32; 4]], scale: Scale) -> Result<Image> {
        let count = pixels.len();
        let mut image = Image {
            buffer: Arc::new(color::Buffer::new(Color::TRANSPARENT, count)),
            viewport: Viewport::from_physical_u32(count as u32, 1, scale)?,
            target_scale: scale,
        };
        let buffer = Arc::make_mut(&mut image.buffer);
        for (i, pixel) in pixels.iter().enumerate() {
            buffer.put(i, Color::from_rgba(*pixel));
        }

        Ok(image)
    }

    fn square(color: [f32; 4], size: u32, scale: Scale) -> Result<Image> {
        let count = (size * size) as usize;
        Ok(Image {
            buffer: Arc::new(color::Buffer::new(Color::from_rgba(color), count)),
            viewport: Viewport::from_physical_u32(size, size, scale)?,
            target_scale: scale,
        })
    }

    #[test]
    fn the_autocontrast_is_full_range_and_tone_preserving() -> Result<()> {
        let pixels = [
            [0.3, 0.3, 0.3, 1.0],
            [0.5, 0.5, 0.5, 1.0],
            [0.7, 0.7, 0.7, 1.0],
            [1.0, 0.12, 0.12, 1.0],
        ];

        let mut image = synthetic_linear(&pixels, Scale::default())?;
        image.autocontrast();

        let lo = Color::from_rgba([0.3, 0.3, 0.3, 1.0]).luma();
        let hi = Color::from_rgba([0.7, 0.7, 0.7, 1.0]).luma();
        let scale = 1.0 / (hi - lo);

        for (i, pixel) in pixels.iter().enumerate() {
            for (value, stretched) in [
                (pixel[0], image.buffer.at(i).r),
                (pixel[1], image.buffer.at(i).g),
                (pixel[2], image.buffer.at(i).b),
            ] {
                let expected = if value < lo || value > hi {
                    value
                } else {
                    (value - lo) * scale
                };
                assert!((expected - stretched).abs() <= 1e-6);
            }
        }

        assert!((image.buffer.at(1).r - 0.5).abs() <= 1e-5);

        assert_eq!(image.buffer.at(3).r, 1.0);
        assert_eq!(image.buffer.at(3).g, 0.12);
        assert_eq!(image.buffer.at(3).b, 0.12);
        assert_eq!(image.buffer.at(3).a, 1.0);

        Ok(())
    }

    #[test]
    fn the_uniform_autocontrast_is_the_identity() -> Result<()> {
        let pixels = [[0.4, 0.3, 0.5, 1.0]; 3];

        let mut image = synthetic_linear(&pixels, Scale::default())?;
        image.autocontrast();

        for i in 0..3 {
            assert_eq!(image.buffer.at(i).r, 0.4);
            assert_eq!(image.buffer.at(i).g, 0.3);
            assert_eq!(image.buffer.at(i).b, 0.5);
        }

        Ok(())
    }

    #[test]
    fn the_sky_background_autocontrast_applies_the_ramp() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("personal/images/sky-night.png");
        let mut image = Image::load(&path, Scale::default()).unwrap();

        let count = image.viewport.total_pixels();
        let before: Vec<_> = (0..count).map(|i| image.buffer.at(i)).collect();

        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for value in &before {
            let luma = value.luma();
            lo = lo.min(luma);
            hi = hi.max(luma);
        }

        assert!(hi > lo);

        image.autocontrast();
        let scale = 1.0 / (hi - lo);

        for i in 0..count {
            let (before, after) = (before[i], image.buffer.at(i));

            for (value, stretched) in [
                (before.r, after.r),
                (before.g, after.g),
                (before.b, after.b),
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
    fn the_invert_inverts_the_straight_color() -> Result<()> {
        let pixels = [
            [1.0, 1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0, 1.0],
            [0.1, 0.2, 0.3, 0.5],
            [0.0, 0.0, 0.0, 0.0],
        ];
        let mut image = synthetic_linear(&pixels, Scale::default())?;
        image.invert();

        let expected = [
            [0.0, 0.0, 0.0, 1.0],
            [1.0, 1.0, 1.0, 1.0],
            [0.4, 0.3, 0.2, 0.5],
            [0.0, 0.0, 0.0, 0.0],
        ];

        for (i, expected) in expected.iter().enumerate() {
            assert!(
                (image.buffer.at(i).r - expected[0]).abs() < 1e-6,
                "pixel {i}"
            );
            assert!(
                (image.buffer.at(i).g - expected[1]).abs() < 1e-6,
                "pixel {i}"
            );
            assert!(
                (image.buffer.at(i).b - expected[2]).abs() < 1e-6,
                "pixel {i}"
            );
            assert!(
                (image.buffer.at(i).a - expected[3]).abs() < 1e-6,
                "pixel {i}"
            );
        }

        Ok(())
    }

    #[test]
    fn the_blend_is_byte_identical_where_the_layer_is_opaque() -> Result<()> {
        let lenna = Path::new(env!("CARGO_MANIFEST_DIR")).join("images/Lenna.png");
        let sky = Path::new(env!("CARGO_MANIFEST_DIR")).join("personal/images/sky-night.png");
        let mut base = Image::load(&sky, Scale::default()).unwrap();
        let before = base.to_rgba8();
        let layer = Image::load(&lenna, Scale::default()).unwrap();
        let (base_width, base_height) = (base.width() as usize, base.height() as usize);
        let (layer_width, layer_height) = (layer.width() as usize, layer.height() as usize);

        base.blend(&layer, Rectangle::default(), color::Space::Linear)?;

        let after = base.to_rgba8();
        let layer = layer.to_rgba8();
        for y in 0..base_height {
            for x in 0..base_width {
                let pixel = (y * base_width + x) * 4;
                if x < layer_width && y < layer_height {
                    let overlapped = (y * layer_width + x) * 4;
                    assert_eq!(&after[pixel..pixel + 4], &layer[overlapped..overlapped + 4]);
                } else {
                    assert_eq!(&after[pixel..pixel + 4], &before[pixel..pixel + 4]);
                }
            }
        }

        Ok(())
    }

    #[test]
    fn the_blend_matches_the_pinned_over_on_real_data() -> Result<()> {
        let lenna = Path::new(env!("CARGO_MANIFEST_DIR")).join("images/Lenna.png");
        let mask =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("personal/images/Card_Mask_Standard.png");
        let mut base = Image::load(&lenna, Scale::default()).unwrap();
        let layer = Image::load(&mask, Scale::default()).unwrap();
        let (base_width, base_height) = (base.width() as usize, base.height() as usize);
        let layer_width = layer.width() as usize;

        let base_pixels: Vec<_> = (0..base_width * base_height)
            .map(|i| base.buffer.at(i))
            .collect();
        let layer_pixels: Vec<_> = (0..layer.width() as usize * layer.height() as usize)
            .map(|i| layer.buffer.at(i))
            .collect();

        base.blend(&layer, Rectangle::default(), color::Space::Linear)?;

        let actual = base.to_rgba8();
        for y in 0..base_height {
            for x in 0..base_width {
                let i = y * base_width + x;
                let j = y * layer_width + x;
                let expected = base_pixels[i]
                    .over(layer_pixels[j])
                    .demultiply()
                    .to_srgb()
                    .to_rgba8();
                assert_eq!(&actual[i * 4..i * 4 + 4], &expected[..]);
            }
        }

        Ok(())
    }

    #[test]
    fn the_blend_offset_and_clip_are_exact() -> Result<()> {
        let black = square([0.0, 0.0, 0.0, 1.0], 4, Scale::default())?;
        let white = square([1.0, 1.0, 1.0, 1.0], 2, Scale::default())?;

        let mut base = black.clone();
        base.blend(
            &white,
            Rectangle::new(-1.0, -1.0, 0.0, 0.0),
            color::Space::Linear,
        )?;
        let rgba = base.to_rgba8();
        assert_eq!(&rgba[0..4], &[255, 255, 255, 255]);
        for i in 1..16 {
            assert_eq!(&rgba[i * 4..i * 4 + 4], &[0, 0, 0, 255]);
        }

        let mut base = black.clone();
        base.blend(
            &white,
            Rectangle::new(3.0, 3.0, 0.0, 0.0),
            color::Space::Linear,
        )?;
        let rgba = base.to_rgba8();
        for i in 0..15 {
            assert_eq!(&rgba[i * 4..i * 4 + 4], &[0, 0, 0, 255]);
        }
        assert_eq!(&rgba[60..64], &[255, 255, 255, 255]);

        Ok(())
    }

    #[test]
    fn the_blend_color_spaces_diverge() -> Result<()> {
        let base = synthetic_linear(&[[0.5, 0.5, 0.5, 1.0]; 2], Scale::default())?;
        let layer = synthetic_linear(&[[0.5, 0.5, 0.5, 0.5]; 2], Scale::default())?;

        let mut linear = base.clone();
        linear.blend(&layer, Rectangle::default(), color::Space::Linear)?;
        let mut srgb = base.clone();
        srgb.blend(&layer, Rectangle::default(), color::Space::Srgb)?;

        for i in 0..2 {
            let opaque = Color::from_rgba([0.5, 0.5, 0.5, 1.0]);
            let half = Color::from_rgba([0.5, 0.5, 0.5, 0.5]);

            let linear_expected = opaque.over(half);
            let srgb_expected = opaque
                .demultiply()
                .to_srgb_fast()
                .premultiply()
                .over(half.demultiply().to_srgb_fast().premultiply())
                .demultiply()
                .to_linear_fast()
                .premultiply();
            assert!((linear.buffer.at(i).r - linear_expected.r).abs() <= 1e-6);
            assert!((srgb.buffer.at(i).r - srgb_expected.r).abs() <= 1e-6);
            assert!((linear_expected.r - srgb_expected.r).abs() > 0.01);
        }

        Ok(())
    }

    #[test]
    fn the_blend_resamples_the_file_scale_layer() -> Result<()> {
        let lenna = Path::new(env!("CARGO_MANIFEST_DIR")).join("images/Lenna.png");
        let mut base = Image::new(Scale::new(0.5), 512.0, 512.0, Color::TRANSPARENT)?;
        let layer = Image::load(&lenna, Scale::new(0.5)).unwrap();

        base.blend(&layer, Rectangle::default(), color::Space::Linear)?;

        let mut expected = layer.clone();
        expected.resize(
            Dimension::Both {
                width: 512.0,
                height: 512.0,
            },
            color::Filter::default(),
            Mode::Stretch,
            Some(Scale::new(0.5)),
        )?;
        assert_eq!(base.to_rgba8(), expected.to_rgba8());

        Ok(())
    }

    #[test]
    fn the_fill_paints_an_aligned_region_exactly() -> Result<()> {
        let mut image = Image::new(Scale::default(), 8.0, 8.0, Color::TRANSPARENT)?;
        let paint = Color::from_pixel(color::Pixel::from_rgba8([128, 160, 192, 255])).to_linear();

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

        Ok(())
    }

    #[test]
    fn the_fill_snaps_fractional_edges_to_the_pixel_grid() -> Result<()> {
        let mut image = Image::new(Scale::default(), 4.0, 4.0, Color::TRANSPARENT)?;
        let paint = Color::from_pixel(color::Pixel::from_rgba8([255, 0, 0, 255])).to_linear();

        image.fill(Rectangle::new(0.5, 0.5, 3.0, 3.0), paint);

        let coverages = [
            0, 0, 0, 0, 0, 255, 255, 255, 0, 255, 255, 255, 0, 255, 255, 255,
        ];
        let painted = image.to_rgba8();
        for (i, coverage) in coverages.iter().enumerate() {
            let cover = *coverage as f32 / 255.0;
            let expected = Color::from_rgba([paint.r * cover, 0.0, 0.0, cover])
                .demultiply()
                .to_srgb()
                .to_rgba8();
            assert_eq!(&painted[i * 4..i * 4 + 4], &expected);
        }

        Ok(())
    }

    #[test]
    fn the_fill_snaps_at_the_run_scale() -> Result<()> {
        let mut image = Image::new(Scale::new(0.5), 16.0, 16.0, Color::TRANSPARENT)?;
        let paint = Color::from_pixel(color::Pixel::from_rgba8([0, 255, 255, 255])).to_linear();

        image.fill(Rectangle::new(0.5, 0.5, 3.0, 3.0), paint);

        let coverages = [
            255, 255, 0, 0, 0, 0, 0, 0, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0,
        ];
        let painted = image.to_rgba8();
        for (i, coverage) in coverages.iter().enumerate() {
            let cover = *coverage as f32 / 255.0;
            let expected = Color::from_rgba([0.0, paint.g * cover, paint.b * cover, cover])
                .demultiply()
                .to_srgb()
                .to_rgba8();
            assert_eq!(&painted[i * 4..i * 4 + 4], &expected);
        }

        Ok(())
    }

    #[test]
    fn the_fill_is_a_no_op_for_degenerate_regions() -> Result<()> {
        let mut image = Image::new(Scale::default(), 4.0, 4.0, Color::TRANSPARENT)?;
        let paint = Color::from_pixel(color::Pixel::from_rgba8([255, 255, 255, 255])).to_linear();

        image.fill(Rectangle::new(0.0, 0.0, 0.0, 0.0), paint);
        image.fill(Rectangle::new(0.0, 0.0, -2.0, 2.0), paint);
        image.fill(Rectangle::new(f32::NAN, 0.0, 2.0, 2.0), paint);

        let painted = image.to_rgba8();
        assert!(painted.iter().all(|&byte| byte == 0));

        Ok(())
    }

    #[test]
    fn the_fill_path_paints_the_aligned_path_exactly() -> Result<()> {
        let mut image = Image::new(Scale::default(), 10.0, 10.0, Color::TRANSPARENT)?;
        let paint = Color::from_pixel(color::Pixel::from_rgba8([255, 255, 255, 255])).to_linear();

        let mut path = crate::path::Path::new();
        path.move_to(2.0, 2.0);
        path.line_to(6.0, 2.0);
        path.line_to(6.0, 6.0);
        path.line_to(2.0, 6.0);
        path.close();
        image.fill(path, paint);

        let painted = image.to_rgba8();
        let pixel = |x: usize, y: usize| {
            let i = (y * 10 + x) * 4;
            painted[i..i + 4].to_vec()
        };

        assert_eq!(pixel(2, 2), vec![255, 255, 255, 255]);
        assert_eq!(pixel(5, 5), vec![255, 255, 255, 255]);
        assert_eq!(pixel(6, 4), vec![0, 0, 0, 0]);
        assert_eq!(pixel(0, 0), vec![0, 0, 0, 0]);

        Ok(())
    }

    #[test]
    fn the_fill_path_is_a_no_op_for_degenerate_paths() -> Result<()> {
        let mut image = Image::new(Scale::default(), 4.0, 4.0, Color::TRANSPARENT)?;
        let paint = Color::from_pixel(color::Pixel::from_rgba8([255, 255, 255, 255])).to_linear();

        image.fill(crate::path::Path::new(), paint);

        let mut non_finite = crate::path::Path::new();
        non_finite.move_to(f32::INFINITY, 0.0);
        non_finite.line_to(0.0, f32::NAN);
        non_finite.close();
        image.fill(non_finite, paint);

        let painted = image.to_rgba8();
        assert!(painted.iter().all(|&byte| byte == 0));

        Ok(())
    }

    fn register_vendored_font() {
        crate::font::Font::load(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("fonts/NotoSans-Regular.ttf")
                .as_path(),
        )
        .unwrap();
    }

    #[test]
    fn the_fill_text_paints_the_ink_in_the_pinned_srgb_space() -> Result<()> {
        register_vendored_font();
        let text = Text::new(Scale::default(), "Noto Sans", 100.0, 400.0, 10.0, 10.0, "N")?;
        assert!(text.viewport.logical_width() > 0.0 && text.viewport.logical_height() > 0.0);

        let mut image = Image::new(Scale::default(), 200.0, 200.0, Color::TRANSPARENT)?;
        let paint = Color::from_pixel(color::Pixel::from_rgba8([200, 30, 60, 255])).to_linear();
        image.fill_text(&text, paint)?;

        let painted = image.to_rgba8();
        let (box_x, box_y) = (text.x as u32, text.y as u32);
        let (box_width, box_height) = (text.viewport.width() as u32, text.viewport.height() as u32);
        let mut solid = 0;
        let mut partial = 0;
        for y in 0..200u32 {
            for x in 0..200u32 {
                let pixel = (y * 200 + x) as usize * 4;
                let inside =
                    (x >= box_x && x < box_x + box_width) && (y >= box_y && y < box_y + box_height);
                let inside_scratch = (x >= box_x - 1 && x < box_x + box_width + 1)
                    && (y >= box_y - 1 && y < box_y + box_height + 1);
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
                } else if inside_scratch {
                    let alpha = painted[pixel + 3];
                    if alpha > 0 {
                        assert!(
                            painted[pixel].abs_diff(200) <= 2
                                && painted[pixel + 1].abs_diff(30) <= 2
                                && painted[pixel + 2].abs_diff(60) <= 2,
                            "pixel ({x}, {y})"
                        );
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

        Ok(())
    }

    #[test]
    fn the_fill_text_is_a_no_op_for_degenerate_text() -> Result<()> {
        let mut image = Image::new(Scale::default(), 4.0, 4.0, Color::TRANSPARENT)?;
        let paint = Color::from_pixel(color::Pixel::from_rgba8([255, 255, 255, 255])).to_linear();

        let empty = Text::new(Scale::default(), "Noto Sans", 100.0, 400.0, 0.0, 0.0, "")?;
        image.fill_text(&empty, paint)?;

        register_vendored_font();
        let unregistered = Text::new(
            Scale::default(),
            "An Unregistered Family",
            100.0,
            400.0,
            0.0,
            0.0,
            "Noto",
        )?;
        image.fill_text(&unregistered, paint)?;

        assert!(image.to_rgba8().iter().all(|&byte| byte == 0));

        Ok(())
    }

    #[test]
    fn the_fill_text_clips_at_the_canvas_corner() -> Result<()> {
        register_vendored_font();
        let text = Text::new(Scale::default(), "Noto Sans", 100.0, 400.0, -5.0, -5.0, "N")?;
        assert!(text.viewport.logical_width() > 0.0 && text.viewport.logical_height() > 0.0);

        let mut image = Image::new(Scale::default(), 200.0, 200.0, Color::TRANSPARENT)?;
        let paint = Color::from_pixel(color::Pixel::from_rgba8([200, 30, 60, 255])).to_linear();
        image.fill_text(&text, paint)?;

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

        Ok(())
    }

    #[test]
    fn the_clone_diverges_on_the_first_in_place_write() -> Result<()> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("images/Lenna.png");
        let file = Image::load(&path, Scale::default())?;
        let file_pixels = file.to_rgba8();

        let mut reference = Reference::load(&path, Scale::default());
        let branch = reference.clone();

        assert_eq!(branch.logical_width()?, 512.0);

        reference.resize(
            Dimension::Both {
                width: 128.0,
                height: 128.0,
            },
            color::Filter::Cubic,
            Mode::Stretch,
            None,
        );

        assert_eq!(reference.logical_width()?, 128.0);
        assert_eq!(branch.logical_width()?, 512.0);
        assert_eq!(branch.image()?.to_rgba8(), file_pixels);

        Ok(())
    }

    #[test]
    fn the_divergent_branches_share_one_load() -> Result<()> {
        let base_color =
            Color::from_pixel(color::Pixel::from_rgba8([200, 30, 60, 255])).to_linear();
        let source = Image::new(Scale::default(), 32.0, 32.0, base_color)?;
        let file = std::env::temp_dir().join(format!(
            "pikke-image-test-{}-shared-load.png",
            std::process::id()
        ));

        let reference = Reference::load(&file, Scale::default());
        let mut left = reference.clone();
        let mut right = reference.clone();
        left.fill(
            Rectangle::new(0.0, 0.0, 16.0, 32.0),
            Color::from_pixel(color::Pixel::from_rgba8([255, 255, 255, 255])).to_linear(),
        );
        right.fill(
            Rectangle::new(16.0, 0.0, 16.0, 32.0),
            Color::from_pixel(color::Pixel::from_rgba8([0, 0, 0, 255])).to_linear(),
        );

        source.write(&file)?;
        let left = left.image()?;
        std::fs::remove_file(&file)?;
        let right = right.image()?;

        let mut left_row = [255u8, 255, 255, 255].repeat(16);
        left_row.extend([200, 30, 60, 255].repeat(16));
        let mut right_row = [200u8, 30, 60, 255].repeat(16);
        right_row.extend([0, 0, 0, 255].repeat(16));

        assert_eq!(left.to_rgba8(), left_row.repeat(32));
        assert_eq!(right.to_rgba8(), right_row.repeat(32));

        Ok(())
    }

    #[test]
    fn the_blend_uses_the_other_as_it_was_at_the_call() -> Result<()> {
        let black = Color::from_pixel(color::Pixel::from_rgba8([0, 0, 0, 255])).to_linear();
        let white = Color::from_pixel(color::Pixel::from_rgba8([255, 255, 255, 255])).to_linear();

        let mut base = Reference::blank(Scale::default(), 100.0, 100.0, black);
        let mut layer = Reference::blank(Scale::default(), 50.0, 50.0, white);

        base.blend(
            &layer,
            Rectangle::new(0.0, 0.0, 50.0, 50.0),
            color::Space::Linear,
        );
        layer.resize(
            Dimension::Both {
                width: 25.0,
                height: 25.0,
            },
            color::Filter::Cubic,
            Mode::Stretch,
            None,
        );

        assert_eq!(layer.logical_width()?, 25.0);

        let image = base.image()?;
        let luma = image.luma_in(Rectangle::new(0.0, 0.0, 50.0, 50.0))?;

        assert!(luma > 200.0);
        Ok(())
    }
}

use std::sync::{Arc, LazyLock, Mutex};

use mlua::{Lua, Table, UserData, UserDataFields, UserDataMethods, Value};
use parlance::NormalizedCoord as ParlayCoord;
use parley::{
    Alignment, AlignmentOptions, FontData, FontFamily, FontWeight, Layout, LayoutContext,
    PositionedLayoutItem, StyleProperty,
};
use skrifa::outline::{OutlineGlyphCollection, OutlinePen};
use skrifa::prelude::{FontRef, GlyphId, LocationRef, NormalizedCoord, Size};

use crate::font::Font;
use crate::lua;
use crate::{Rectangle, Result, Scale, Viewport};

static LAYOUTS: LazyLock<Mutex<LayoutContext<()>>> =
    LazyLock::new(|| Mutex::new(LayoutContext::new()));

#[derive(Clone, Debug)]
pub struct Text {
    pub x: f32,
    pub y: f32,
    pub viewport: Viewport,
    pub ink_min: (f32, f32),
    layout: Arc<Layout<()>>,
}

pub struct Glyph<'a> {
    pub font: &'a FontData,
    pub size: f32,
    pub location: &'a [ParlayCoord],
    pub id: u32,
    pub x: f32,
    pub y: f32,
}

impl Text {
    pub fn new(
        scale: Scale,
        family: &str,
        size: f32,
        weight: f32,
        x: f32,
        y: f32,
        string: &str,
    ) -> Result<Self> {
        let degenerate = |layout: Layout<()>| Text {
            x,
            y,
            viewport: Viewport::zero(scale),
            ink_min: (0.0, 0.0),
            layout: Arc::new(layout),
        };

        if string.is_empty() {
            return Ok(degenerate(Layout::new()));
        }

        let layout = {
            let mut fonts = crate::font::FONTS.lock().unwrap();
            let mut layouts = LAYOUTS.lock().unwrap();
            let mut builder = layouts.ranged_builder(&mut fonts, string, scale.into(), true);
            builder.push_default(StyleProperty::FontFamily(FontFamily::named(family)));
            builder.push_default(StyleProperty::FontSize(size));
            builder.push_default(StyleProperty::FontWeight(FontWeight::new(weight)));
            let mut layout: Layout<()> = builder.build(string);
            layout.break_all_lines(None);
            layout.align(Alignment::Start, AlignmentOptions::default());
            layout
        };

        let Some((min_x, min_y, max_x, max_y)) = ink(&layout) else {
            return Ok(degenerate(layout));
        };

        let width = (max_x - min_x) / scale;
        let height = (max_y - min_y) / scale;

        Ok(Self {
            x,
            y,
            viewport: Viewport::from_logical(width, height, scale)?,
            ink_min: (min_x / scale, min_y / scale),
            layout: Arc::new(layout),
        })
    }

    pub fn runs(&self) -> impl Iterator<Item = Run<'_>> {
        runs_of(&self.layout)
    }

    pub fn move_to(&mut self, x: f32, y: f32) {
        self.x = x;
        self.y = y;
    }

    pub fn translate(&mut self, dx: f32, dy: f32) {
        self.x += dx;
        self.y += dy;
    }
}

pub struct Run<'a>(parley::GlyphRun<'a, ()>);

impl Run<'_> {
    pub fn glyphs(&self) -> impl Iterator<Item = Glyph<'_>> {
        let run = self.0.run();
        let font = run.font();
        let size = run.font_size();
        let location = run.normalized_coords();

        self.0.positioned_glyphs().map(move |glyph| Glyph {
            font,
            size,
            location,
            id: glyph.id,
            x: glyph.x,
            y: glyph.y,
        })
    }
}

fn runs_of(layout: &Layout<()>) -> impl Iterator<Item = Run<'_>> {
    layout
        .lines()
        .flat_map(|line| line.items())
        .filter_map(|item| match item {
            PositionedLayoutItem::GlyphRun(glyph_run) => Some(glyph_run),
            _ => None,
        })
        .map(Run)
}

fn ink(layout: &Layout<()>) -> Option<(f32, f32, f32, f32)> {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for run in runs_of(layout) {
        for glyph in run.glyphs() {
            let Some(font) = FontRef::from_index(glyph.font.data.as_ref(), glyph.font.index).ok()
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
            let mut pen = BboxPen::new();
            let Ok(_) = glyph_outline.draw((Size::new(glyph.size), location), &mut pen) else {
                continue;
            };
            min_x = min_x.min(glyph.x + pen.min_x);
            min_y = min_y.min(glyph.y - pen.max_y);
            max_x = max_x.max(glyph.x + pen.max_x);
            max_y = max_y.max(glyph.y - pen.min_y);
        }
    }

    if min_x.is_infinite() {
        None
    } else {
        Some((min_x, min_y, max_x, max_y))
    }
}

struct BboxPen {
    min_x: f32,
    min_y: f32,
    max_x: f32,
    max_y: f32,
}

impl BboxPen {
    fn new() -> BboxPen {
        BboxPen {
            min_x: f32::INFINITY,
            min_y: f32::INFINITY,
            max_x: f32::NEG_INFINITY,
            max_y: f32::NEG_INFINITY,
        }
    }

    fn point(&mut self, x: f32, y: f32) {
        self.min_x = self.min_x.min(x);
        self.min_y = self.min_y.min(y);
        self.max_x = self.max_x.max(x);
        self.max_y = self.max_y.max(y);
    }
}

impl OutlinePen for BboxPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.point(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.point(x, y);
    }

    fn quad_to(&mut self, control_x: f32, control_y: f32, x: f32, y: f32) {
        self.point(control_x, control_y);
        self.point(x, y);
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
        self.point(control_x, control_y);
        self.point(control1_x, control1_y);
        self.point(x, y);
    }

    fn close(&mut self) {}
}

impl UserData for Text {
    fn add_fields<F: UserDataFields<Text>>(fields: &mut F) {
        fields.add_meta_field("__name", "Text");
        fields.add_field_method_get("x", |_lua, this| -> mlua::Result<f32> { Ok(this.x) });
        fields.add_field_method_get("y", |_lua, this| -> mlua::Result<f32> { Ok(this.y) });
        fields.add_field_method_get("width", |_lua, this| -> mlua::Result<f32> {
            Ok(this.viewport.logical_width())
        });
        fields.add_field_method_get("height", |_lua, this| -> mlua::Result<f32> {
            Ok(this.viewport.logical_height())
        });
        fields.add_field_method_get("bounds", |_lua, this| -> mlua::Result<Rectangle> {
            Ok(Rectangle::new(
                this.x,
                this.y,
                this.viewport.logical_width(),
                this.viewport.logical_height(),
            ))
        });
    }

    fn add_methods<M: UserDataMethods<Text>>(methods: &mut M) {
        methods.add_method_mut("move_to", |_lua, this, offset: Table| -> mlua::Result<()> {
            let x = lua::number("move_to", "x", offset.get("x")?)?;
            let y = lua::number("move_to", "y", offset.get("y")?)?;

            this.move_to(x, y);

            Ok(())
        });

        methods.add_method_mut(
            "translate",
            |_lua, this, offset: Table| -> mlua::Result<()> {
                let dx = lua::number("translate", "x", offset.get("x")?)?;
                let dy = lua::number("translate", "y", offset.get("y")?)?;

                this.translate(dx, dy);

                Ok(())
            },
        );
    }
}

pub fn install(lua: &Lua, env: &Table, scale: Scale) -> Result<()> {
    env.set(
        "text",
        lua.create_function(
            move |_lua, (string, opts): (String, Table)| -> mlua::Result<Text> {
                let number = |name: &str, value: Option<Value>| -> mlua::Result<Option<f32>> {
                    match value {
                        None => Ok(None),
                        Some(Value::Number(value)) => Ok(Some(value as f32)),
                        Some(Value::Integer(value)) => Ok(Some(value as f32)),
                        Some(_) => Err(mlua::Error::RuntimeError(format!(
                            "text: '{name}' is missing or not a number"
                        ))),
                    }
                };

                let font: Option<Value> = opts.get("font")?;
                let Some(Value::UserData(font)) = font else {
                    return Err(mlua::Error::RuntimeError(
                        "text: 'font' is missing or not a `Font`".to_owned(),
                    ));
                };
                let font = font.borrow::<Font>().map_err(|_| {
                    mlua::Error::RuntimeError("text: 'font' is missing or not a `Font`".to_owned())
                })?;

                let Some(size) = number("size", opts.get("size")?)? else {
                    return Err(mlua::Error::RuntimeError(
                        "text: 'size' is missing or not a number".to_owned(),
                    ));
                };
                if size <= 0.0 || !size.is_finite() {
                    return Err(mlua::Error::RuntimeError(
                        "text: 'size' must be positive".to_owned(),
                    ));
                }

                let weight = number("weight", opts.get("weight")?)?.unwrap_or(400.0);
                let x = number("x", opts.get("x")?)?.unwrap_or(0.0);
                let y = number("y", opts.get("y")?)?.unwrap_or(0.0);

                if !x.is_finite() {
                    return Err(mlua::Error::RuntimeError(
                        "text: 'x' must be finite".to_owned(),
                    ));
                }

                if !y.is_finite() {
                    return Err(mlua::Error::RuntimeError(
                        "text: 'y' must be finite".to_owned(),
                    ));
                }

                Ok(Text::new(scale, &font.family, size, weight, x, y, &string)?)
            },
        )?,
    )?;
    Ok(())
}

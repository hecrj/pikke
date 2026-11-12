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

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use mlua::{AnyUserData, Function, Lua, Table};

    use super::{Text, install};
    use crate::font::{Font, install as font_install};
    use crate::{Rectangle, Scale, Viewport};

    fn installed() -> (Lua, Table, Function, Function) {
        let lua = Lua::new();
        let env = lua.create_table().unwrap();
        let script_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("fonts");
        font_install(&lua, &env, &script_dir).unwrap();
        install(&lua, &env, Scale::default()).unwrap();
        let font: Function = env.get("font").unwrap();
        let text: Function = env.get("text").unwrap();
        (lua, env, font, text)
    }

    fn text_of(text: &Function, string: &str, opts: Table) -> Text {
        let userdata: AnyUserData = text.call((string, opts)).unwrap();
        userdata.borrow::<Text>().unwrap().clone()
    }

    #[test]
    fn the_op_lays_out_the_vendored_face() {
        let (lua, _env, font, text) = installed();
        let font: AnyUserData = font.call("NotoSans-Regular.ttf").unwrap();

        let opts = lua.create_table().unwrap();
        opts.set("font", font).unwrap();
        opts.set("size", 100.0).unwrap();
        opts.set("x", 7.0).unwrap();
        opts.set("y", 3.0).unwrap();
        let text = text_of(&text, "Noto", opts);
        assert_eq!((text.x, text.y), (7.0, 3.0));
        assert!(text.viewport.logical_width() > 0.0);
        assert!(text.viewport.logical_height() > 0.0);

        let env = lua.create_table().unwrap();
        env.set("text", lua.create_userdata(text).unwrap()).unwrap();
        let bounds: AnyUserData = lua.load("text.bounds").set_environment(env).eval().unwrap();
        let bounds = *bounds.borrow::<Rectangle>().unwrap();
        assert_eq!((bounds.x, bounds.y), (7.0, 3.0));
        assert!(bounds.width > 0.0);
        assert!(bounds.height > 0.0);
    }

    #[test]
    fn the_op_is_degenerate_for_an_unregistered_family() {
        let (lua, _env, _font, text) = installed();

        let family = "An Unregistered Family".to_string();
        let font = lua.create_userdata(Font { family }).unwrap();

        let opts = lua.create_table().unwrap();
        opts.set("font", font).unwrap();
        opts.set("size", 100.0).unwrap();
        let text = text_of(&text, "Noto", opts);
        assert_eq!(
            (
                text.x,
                text.y,
                text.viewport.logical_width(),
                text.viewport.logical_height()
            ),
            (0.0, 0.0, 0.0, 0.0)
        );
    }

    #[test]
    fn the_op_is_degenerate_for_an_empty_string() {
        let (lua, _env, font, text) = installed();
        let font: AnyUserData = font.call("NotoSans-Regular.ttf").unwrap();

        let opts = lua.create_table().unwrap();
        opts.set("font", font).unwrap();
        opts.set("size", 100.0).unwrap();
        let text = text_of(&text, "", opts);
        assert_eq!(
            (
                text.x,
                text.y,
                text.viewport.logical_width(),
                text.viewport.logical_height()
            ),
            (0.0, 0.0, 0.0, 0.0)
        );
    }

    #[test]
    fn the_defaults_flow_through() {
        let (lua, _env, font, text) = installed();
        let font: AnyUserData = font.call("NotoSans-Regular.ttf").unwrap();

        let opts = lua.create_table().unwrap();
        opts.set("font", font).unwrap();
        opts.set("size", 100.0).unwrap();
        let text = text_of(&text, "Noto", opts);
        assert_eq!((text.x, text.y), (0.0, 0.0));
        assert!(text.viewport.logical_width() > 0.0);
    }

    #[test]
    fn the_bounds_reports_logical_px() {
        let lua = Lua::new();
        let text = Text {
            x: 20.0,
            y: 40.0,
            viewport: Viewport::from_logical(200.0, 100.0, Scale::new(0.5)).unwrap(),
            ink_min: (0.0, 0.0),
            layout: Arc::default(),
        };
        let env = lua.create_table().unwrap();
        env.set("text", lua.create_userdata(text).unwrap()).unwrap();
        let bounds: AnyUserData = lua.load("text.bounds").set_environment(env).eval().unwrap();
        let bounds = *bounds.borrow::<Rectangle>().unwrap();
        assert_eq!(
            (bounds.x, bounds.y, bounds.width, bounds.height),
            (20.0, 40.0, 200.0, 100.0)
        );
    }

    #[test]
    fn the_binding_validates_its_arguments() {
        let (lua, _env, font, text) = installed();
        let font: AnyUserData = font.call("NotoSans-Regular.ttf").unwrap();

        let eval = |opts: Table| -> String {
            text.call::<AnyUserData>(("Noto", opts))
                .unwrap_err()
                .to_string()
        };

        let opts = lua.create_table().unwrap();
        opts.set("size", 100.0).unwrap();
        assert!(eval(opts).contains("text: 'font' is missing or not a `Font`"));

        let opts = lua.create_table().unwrap();
        opts.set("font", font.clone()).unwrap();
        assert!(eval(opts).contains("text: 'size' is missing or not a number"));

        let opts = lua.create_table().unwrap();
        opts.set("font", font.clone()).unwrap();
        opts.set("size", "big").unwrap();
        assert!(eval(opts).contains("text: 'size' is missing or not a number"));

        let opts = lua.create_table().unwrap();
        opts.set("font", font.clone()).unwrap();
        opts.set("size", -1.0).unwrap();
        assert!(eval(opts).contains("text: 'size' must be positive"));

        let opts = lua.create_table().unwrap();
        opts.set("font", font.clone()).unwrap();
        opts.set("size", 100.0).unwrap();
        opts.set("weight", "bold").unwrap();
        assert!(eval(opts).contains("text: 'weight' is missing or not a number"));

        let opts = lua.create_table().unwrap();
        opts.set("font", font.clone()).unwrap();
        opts.set("size", 100.0).unwrap();
        opts.set("x", "left").unwrap();
        assert!(eval(opts).contains("text: 'x' is missing or not a number"));

        let opts = lua.create_table().unwrap();
        opts.set("font", font.clone()).unwrap();
        opts.set("size", 100.0).unwrap();
        opts.set("y", f64::INFINITY).unwrap();
        assert!(eval(opts).contains("text: 'y' must be finite"));
    }

    fn text_value(x: f32, y: f32, scale: f32) -> (Lua, Table, AnyUserData) {
        let lua = Lua::new();
        let text = Text {
            x,
            y,
            viewport: Viewport::from_physical_u32(100, 50, Scale::new(scale)).unwrap(),
            ink_min: (0.0, 0.0),
            layout: Arc::default(),
        };
        let value = lua.create_userdata(text).unwrap();
        let env = lua.create_table().unwrap();
        env.set("text", value.clone()).unwrap();
        (lua, env, value)
    }

    #[test]
    fn the_width_and_height_report_logical_px() {
        let (lua, env, _text) = text_value(10.0, 20.0, 0.5);
        let width: f64 = lua
            .load("text.width")
            .set_environment(env.clone())
            .eval()
            .unwrap();
        let height: f64 = lua.load("text.height").set_environment(env).eval().unwrap();
        assert_eq!((width, height), (200.0, 100.0));
    }

    #[test]
    fn the_translate_moves_the_anchor() {
        let (lua, env, text) = text_value(10.0, 20.0, 1.0);

        let offset = lua.create_table().unwrap();
        offset.set("x", 5.0).unwrap();
        offset.set("y", -30.0).unwrap();
        env.set("offset", offset).unwrap();
        lua.load("text:translate(offset)")
            .set_environment(env.clone())
            .eval::<()>()
            .unwrap();
        let moved = text.borrow::<Text>().unwrap().clone();
        assert_eq!((moved.x, moved.y), (15.0, -10.0));

        let empty = lua.create_table().unwrap();
        env.set("offset", empty).unwrap();
        lua.load("text:translate(offset)")
            .set_environment(env.clone())
            .eval::<()>()
            .unwrap();
        let moved = text.borrow::<Text>().unwrap().clone();
        assert_eq!((moved.x, moved.y), (15.0, -10.0));

        let offset = lua.create_table().unwrap();
        offset.set("x", 7.0).unwrap();
        env.set("offset", offset).unwrap();
        lua.load("text:translate(offset)")
            .set_environment(env)
            .eval::<()>()
            .unwrap();
        let moved = text.borrow::<Text>().unwrap().clone();
        assert_eq!((moved.x, moved.y), (22.0, -10.0));
    }

    #[test]
    fn the_translate_adds_logical_offsets() {
        let (lua, env, text) = text_value(20.0, 40.0, 2.0);
        let offset = lua.create_table().unwrap();
        offset.set("x", 1.5).unwrap();
        offset.set("y", -2.5).unwrap();
        env.set("offset", offset).unwrap();
        lua.load("text:translate(offset)")
            .set_environment(env)
            .eval::<()>()
            .unwrap();
        let moved = text.borrow::<Text>().unwrap().clone();
        assert_eq!((moved.x, moved.y), (21.5, 37.5));
    }

    #[test]
    fn the_translate_is_reflected_in_bounds() {
        let (lua, env, text) = text_value(10.0, 20.0, 1.0);
        let offset = lua.create_table().unwrap();
        offset.set("y", -5.0).unwrap();
        env.set("offset", offset).unwrap();
        lua.load("text:translate(offset)")
            .set_environment(env.clone())
            .eval::<()>()
            .unwrap();
        let bounds: AnyUserData = lua.load("text.bounds").set_environment(env).eval().unwrap();
        let bounds = *bounds.borrow::<Rectangle>().unwrap();
        assert_eq!((bounds.x, bounds.y), (10.0, 15.0));
        let moved = text.borrow::<Text>().unwrap().clone();
        assert_eq!(moved.y, 15.0);
    }

    #[test]
    fn the_translate_validates_its_arguments() {
        let (lua, env, _text) = text_value(10.0, 20.0, 1.0);

        let offset = lua.create_table().unwrap();
        offset.set("x", "left").unwrap();
        env.set("offset", offset).unwrap();
        let error = lua
            .load("text:translate(offset)")
            .set_environment(env.clone())
            .eval::<()>()
            .unwrap_err()
            .to_string();
        assert!(error.contains("translate: 'x' is not a number"));

        let offset = lua.create_table().unwrap();
        offset.set("y", f64::INFINITY).unwrap();
        env.set("offset", offset).unwrap();
        let error = lua
            .load("text:translate(offset)")
            .set_environment(env)
            .eval::<()>()
            .unwrap_err()
            .to_string();
        assert!(error.contains("translate: 'y' must be finite"));
    }

    #[test]
    fn the_move_to_sets_logical_positions() {
        let (lua, env, text) = text_value(20.0, 40.0, 2.0);
        let offset = lua.create_table().unwrap();
        offset.set("x", 1.5).unwrap();
        offset.set("y", -2.5).unwrap();
        env.set("offset", offset).unwrap();
        lua.load("text:move_to(offset)")
            .set_environment(env)
            .eval::<()>()
            .unwrap();
        let moved = text.borrow::<Text>().unwrap().clone();
        assert_eq!((moved.x, moved.y), (1.5, -2.5));
    }

    #[test]
    fn the_move_to_defaults_omitted_coordinates_to_zero() {
        let (lua, env, text) = text_value(10.0, 20.0, 1.0);
        lua.load("text:move_to({})")
            .set_environment(env)
            .eval::<()>()
            .unwrap();
        let moved = text.borrow::<Text>().unwrap().clone();
        assert_eq!((moved.x, moved.y), (0.0, 0.0));
    }

    #[test]
    fn the_move_to_validates_its_arguments() {
        let (lua, env, _text) = text_value(10.0, 20.0, 1.0);

        let offset = lua.create_table().unwrap();
        offset.set("x", "left").unwrap();
        env.set("offset", offset).unwrap();
        let error = lua
            .load("text:move_to(offset)")
            .set_environment(env.clone())
            .eval::<()>()
            .unwrap_err()
            .to_string();
        assert!(error.contains("move_to: 'x' is not a number"));

        let offset = lua.create_table().unwrap();
        offset.set("y", f64::INFINITY).unwrap();
        env.set("offset", offset).unwrap();
        let error = lua
            .load("text:move_to(offset)")
            .set_environment(env)
            .eval::<()>()
            .unwrap_err()
            .to_string();
        assert!(error.contains("move_to: 'y' must be finite"));
    }
}

use std::sync::{LazyLock, Mutex};

use mlua::{Lua, Result, Table, UserData, UserDataFields, UserDataMethods, Value};
use parlance::NormalizedCoord as ParlayCoord;
use parley::{
    Alignment, AlignmentOptions, FontData, FontFamily, FontWeight, Layout, LayoutContext,
    PositionedLayoutItem, StyleProperty,
};
use skrifa::outline::{OutlineGlyphCollection, OutlinePen};
use skrifa::prelude::{FontRef, GlyphId, LocationRef, NormalizedCoord, Size};

use crate::font::Font;
use crate::{Rectangle, Viewport};

static LAYOUTS: LazyLock<Mutex<LayoutContext<()>>> =
    LazyLock::new(|| Mutex::new(LayoutContext::new()));

#[derive(Clone, Debug)]
pub struct Text {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    pub ink_min: (f32, f32),
    layout: Layout<()>,
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
        family: &str,
        size: f32,
        weight: f32,
        x: f32,
        y: f32,
        scale: f32,
        string: &str,
    ) -> Self {
        let anchor = ((x * scale).round() as i32, (y * scale).round() as i32);
        let degenerate = |layout: Layout<()>| Text {
            x: anchor.0,
            y: anchor.1,
            width: 0,
            height: 0,
            scale,
            ink_min: (0.0, 0.0),
            layout,
        };
        if string.is_empty() {
            return degenerate(Layout::new());
        }

        let layout = {
            let mut fonts = crate::font::FONTS.lock().unwrap();
            let mut layouts = LAYOUTS.lock().unwrap();
            let mut builder = layouts.ranged_builder(&mut fonts, string, scale, true);
            builder.push_default(StyleProperty::FontFamily(FontFamily::named(family)));
            builder.push_default(StyleProperty::FontSize(size));
            builder.push_default(StyleProperty::FontWeight(FontWeight::new(weight)));
            let mut layout: Layout<()> = builder.build(string);
            layout.break_all_lines(None);
            layout.align(Alignment::Start, AlignmentOptions::default());
            layout
        };

        let Some((min_x, min_y, max_x, max_y)) = ink(&layout) else {
            return degenerate(layout);
        };

        Self {
            x: anchor.0,
            y: anchor.1,
            width: (max_x - min_x).round().max(0.0) as u32,
            height: (max_y - min_y).round().max(0.0) as u32,
            scale,
            ink_min: (min_x, min_y),
            layout,
        }
    }

    pub fn runs(&self) -> impl Iterator<Item = Run<'_>> {
        runs_of(&self.layout)
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
    }

    fn add_methods<M: UserDataMethods<Text>>(methods: &mut M) {
        methods.add_method("bounds", |_lua, this, _args: ()| -> Result<Rectangle> {
            Ok(Rectangle::new(
                this.x as f32 / this.scale,
                this.y as f32 / this.scale,
                this.width as f32 / this.scale,
                this.height as f32 / this.scale,
            ))
        });
    }
}

pub fn install(lua: &Lua, env: &Table, viewport: Viewport) -> anywho::Result<()> {
    env.set(
        "text",
        lua.create_function(
            move |_lua, (string, opts): (String, Table)| -> Result<Text> {
                let number = |name: &str, value: Option<Value>| -> Result<Option<f32>> {
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

                Ok(Text::new(
                    &font.family,
                    size,
                    weight,
                    x,
                    y,
                    viewport.scale,
                    &string,
                ))
            },
        )?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use mlua::{AnyUserData, Function, Lua, Table};
    use parley::Layout;

    use super::{Text, install};
    use crate::font::{Font, install as font_install};
    use crate::{Rectangle, Viewport};

    fn installed() -> (Lua, Table, Function, Function) {
        let lua = Lua::new();
        let env = lua.create_table().unwrap();
        let script_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fonts");
        font_install(&lua, &env, &script_dir).unwrap();
        install(
            &lua,
            &env,
            Viewport {
                width: 100,
                height: 100,
                scale: 1.0,
            },
        )
        .unwrap();
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
        assert_eq!((text.x, text.y), (7, 3));
        assert!(text.width > 0);
        assert!(text.height > 0);

        let env = lua.create_table().unwrap();
        env.set("text", lua.create_userdata(text).unwrap()).unwrap();
        let bounds: AnyUserData = lua
            .load("text:bounds()")
            .set_environment(env)
            .eval()
            .unwrap();
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
        assert_eq!((text.x, text.y, text.width, text.height), (0, 0, 0, 0));
    }

    #[test]
    fn the_op_is_degenerate_for_an_empty_string() {
        let (lua, _env, font, text) = installed();
        let font: AnyUserData = font.call("NotoSans-Regular.ttf").unwrap();

        let opts = lua.create_table().unwrap();
        opts.set("font", font).unwrap();
        opts.set("size", 100.0).unwrap();
        let text = text_of(&text, "", opts);
        assert_eq!((text.x, text.y, text.width, text.height), (0, 0, 0, 0));
    }

    #[test]
    fn the_defaults_flow_through() {
        let (lua, _env, font, text) = installed();
        let font: AnyUserData = font.call("NotoSans-Regular.ttf").unwrap();

        let opts = lua.create_table().unwrap();
        opts.set("font", font).unwrap();
        opts.set("size", 100.0).unwrap();
        let text = text_of(&text, "Noto", opts);
        assert_eq!((text.x, text.y), (0, 0));
        assert!(text.width > 0);
    }

    #[test]
    fn the_bounds_reports_logical_px() {
        let lua = Lua::new();
        let text = Text {
            x: 10,
            y: 20,
            width: 100,
            height: 50,
            scale: 0.5,
            ink_min: (0.0, 0.0),
            layout: Layout::new(),
        };
        let env = lua.create_table().unwrap();
        env.set("text", lua.create_userdata(text).unwrap()).unwrap();
        let bounds: AnyUserData = lua
            .load("text:bounds()")
            .set_environment(env)
            .eval()
            .unwrap();
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
}

use mlua::{Lua, Result, Table, UserData, UserDataFields, UserDataMethods, Value};

#[derive(Clone, Copy, Debug, Default)]
pub struct Rectangle {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

struct Padding {
    top: f32,
    right: f32,
    bottom: f32,
    left: f32,
}

impl Padding {
    fn uniform(n: f32) -> Padding {
        Padding {
            top: n,
            right: n,
            bottom: n,
            left: n,
        }
    }
}

impl Rectangle {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Rectangle {
        Rectangle {
            x,
            y,
            width,
            height,
        }
    }

    fn padded(self, padding: Padding) -> Rectangle {
        Rectangle::new(
            self.x - padding.left,
            self.y - padding.top,
            self.width + padding.left + padding.right,
            self.height + padding.top + padding.bottom,
        )
    }
}

impl UserData for Rectangle {
    fn add_fields<F: UserDataFields<Rectangle>>(fields: &mut F) {
        fields.add_meta_field("__name", "Rectangle");
        fields.add_field_method_get("x", |_lua, rect| Ok(rect.x));
        fields.add_field_method_get("y", |_lua, rect| Ok(rect.y));
        fields.add_field_method_get("width", |_lua, rect| Ok(rect.width));
        fields.add_field_method_get("height", |_lua, rect| Ok(rect.height));
    }

    fn add_methods<M: UserDataMethods<Rectangle>>(methods: &mut M) {
        methods.add_method("padded", |_lua, this, value: Value| -> Result<Rectangle> {
            let padding = match value {
                Value::Number(n) => Padding::uniform(n as f32),
                Value::Integer(n) => Padding::uniform(n as f32),
                Value::Table(table) => {
                    let left: Option<f32> = table.get("left")?;
                    let top: Option<f32> = table.get("top")?;
                    let right: Option<f32> = table.get("right")?;
                    let bottom: Option<f32> = table.get("bottom")?;

                    if left.is_some() || top.is_some() || right.is_some() || bottom.is_some() {
                        Padding {
                            top: top.unwrap_or(0.0),
                            right: right.unwrap_or(0.0),
                            bottom: bottom.unwrap_or(0.0),
                            left: left.unwrap_or(0.0),
                        }
                    } else {
                        let vertical: Option<f32> = table.get(1)?;
                        let horizontal: Option<f32> = table.get(2)?;

                        match (vertical, horizontal) {
                            (Some(vertical), Some(horizontal)) => Padding {
                                top: vertical,
                                right: horizontal,
                                bottom: vertical,
                                left: horizontal,
                            },
                            _ => {
                                return Err(mlua::Error::RuntimeError(
                                    "padded: the {vertical, horizontal} pair needs two numbers"
                                        .to_owned(),
                                ));
                            }
                        }
                    }
                }
                _ => {
                    return Err(mlua::Error::RuntimeError(
                        "padded: needs a number or a table".to_owned(),
                    ));
                }
            };

            Ok(this.padded(padding))
        });
    }
}

pub fn install(lua: &Lua, env: &Table) -> crate::Result<()> {
    env.set(
        "rectangle",
        lua.create_function(|_lua, opts: Table| -> Result<Rectangle> {
            let x: Option<f32> = opts.get("x")?;
            let y: Option<f32> = opts.get("y")?;
            let size: Option<Value> = opts.get("size")?;

            let Some(size) = size else {
                return Err(mlua::Error::RuntimeError(
                    "rectangle: needs 'size'".to_owned(),
                ));
            };

            let (width, height) = match size {
                Value::Number(size) => {
                    let size = size as f32;
                    (size, size)
                }
                Value::Integer(size) => {
                    let size = size as f32;
                    (size, size)
                }
                Value::Table(size) => {
                    let width: Option<f32> = size.get(1)?;
                    let height: Option<f32> = size.get(2)?;
                    match (width, height) {
                        (Some(width), Some(height)) => (width, height),
                        _ => {
                            return Err(mlua::Error::RuntimeError(
                                "rectangle: 'size' needs width and height".to_owned(),
                            ));
                        }
                    }
                }
                _ => {
                    return Err(mlua::Error::RuntimeError(
                        "rectangle: 'size' must be a number or a {w, h} pair".to_owned(),
                    ));
                }
            };

            Ok(Rectangle::new(
                x.unwrap_or(0.0),
                y.unwrap_or(0.0),
                width,
                height,
            ))
        })?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Padding, Rectangle, install};
    use mlua::{AnyUserData, Function, Lua, ObjectLike, Table, Value};

    #[test]
    fn the_op_reads_its_real_px_geometry() {
        let lua = Lua::new();
        let env = lua.create_table().unwrap();
        install(&lua, &env).unwrap();
        let rectangle: Function = env.get("rectangle").unwrap();

        let call_rect = |opts: Table| -> Rectangle {
            let userdata: AnyUserData = rectangle.call(opts).unwrap();
            *userdata.borrow::<Rectangle>().unwrap()
        };

        let opts = lua.create_table().unwrap();
        opts.set("size", 100.5).unwrap();
        let rect = call_rect(opts);
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (0.0, 0.0, 100.5, 100.5)
        );

        let opts = lua.create_table().unwrap();
        opts.set("size", 100).unwrap();
        let rect = call_rect(opts);
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (0.0, 0.0, 100.0, 100.0)
        );

        let opts = lua.create_table().unwrap();
        opts.set("x", 10.5).unwrap();
        opts.set("y", -10.5).unwrap();
        let size = lua.create_table().unwrap();
        size.set(1, 100.5).unwrap();
        size.set(2, 50.5).unwrap();
        opts.set("size", size).unwrap();
        let rect = call_rect(opts);
        assert_eq!(
            (rect.x, rect.y, rect.width, rect.height),
            (10.5, -10.5, 100.5, 50.5)
        );

        let opts = lua.create_table().unwrap();
        assert!(rectangle.call::<AnyUserData>(opts).is_err());
        let size = lua.create_table().unwrap();
        size.set(1, 100.0).unwrap();
        let opts = lua.create_table().unwrap();
        opts.set("size", size).unwrap();
        assert!(rectangle.call::<AnyUserData>(opts).is_err());
    }

    #[test]
    fn padded_grows_all_sides() {
        let rect = Rectangle::new(10.0, 20.0, 100.0, 50.0);

        let padded = rect.padded(Padding::uniform(10.0));
        assert_eq!(
            (padded.x, padded.y, padded.width, padded.height),
            (0.0, 10.0, 120.0, 70.0)
        );

        let padded = rect.padded(Padding::uniform(2.5));
        assert_eq!(
            (padded.x, padded.y, padded.width, padded.height),
            (7.5, 17.5, 105.0, 55.0)
        );
        let padded = rect.padded(Padding::uniform(-10.0));
        assert_eq!(
            (padded.x, padded.y, padded.width, padded.height),
            (20.0, 30.0, 80.0, 30.0)
        );
    }

    #[test]
    fn padded_supports_its_variants() {
        let lua = Lua::new();
        let env = lua.create_table().unwrap();
        install(&lua, &env).unwrap();
        let rectangle: Function = env.get("rectangle").unwrap();

        let opts = lua.create_table().unwrap();
        opts.set("x", 10.0).unwrap();
        opts.set("y", 20.0).unwrap();
        let size = lua.create_table().unwrap();
        size.set(1, 100.0).unwrap();
        size.set(2, 50.0).unwrap();
        opts.set("size", size).unwrap();
        let rect: AnyUserData = rectangle.call(opts).unwrap();

        let call_padded = |value: Value| -> (f32, f32, f32, f32) {
            let padded: AnyUserData = rect.call_method("padded", (value,)).unwrap();
            let padded = *padded.borrow::<Rectangle>().unwrap();
            (padded.x, padded.y, padded.width, padded.height)
        };

        assert_eq!(call_padded(Value::Integer(10)), (0.0, 10.0, 120.0, 70.0));

        let pair = lua.create_table().unwrap();
        pair.set(1, 10.0).unwrap();
        pair.set(2, 5.0).unwrap();
        assert_eq!(call_padded(Value::Table(pair)), (5.0, 10.0, 110.0, 70.0));

        let sides = lua.create_table().unwrap();
        sides.set("left", 1.0).unwrap();
        sides.set("top", 2.0).unwrap();
        sides.set("right", 3.0).unwrap();
        sides.set("bottom", 4.0).unwrap();
        assert_eq!(call_padded(Value::Table(sides)), (9.0, 18.0, 104.0, 56.0));

        let one = lua.create_table().unwrap();
        one.set(1, 10.0).unwrap();
        assert!(rect.call_method::<AnyUserData>("padded", (one,)).is_err());

        let empty = lua.create_table().unwrap();
        assert!(rect.call_method::<AnyUserData>("padded", (empty,)).is_err());
    }
}

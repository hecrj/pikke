use mlua::{Lua, Result, Table, UserData, UserDataFields, UserDataMethods, Value};

#[derive(Clone, Copy, Debug, Default)]
pub struct Rectangle {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
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

    pub fn padded(self, n: f32) -> Rectangle {
        Rectangle::new(
            self.x - n,
            self.y - n,
            self.width + 2.0 * n,
            self.height + 2.0 * n,
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
        methods.add_method("padded", |_lua, this, n: f32| -> Result<Rectangle> {
            Ok(this.padded(n))
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
    use super::{Rectangle, install};
    use mlua::{AnyUserData, Function, Lua, Table};

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

        let padded = rect.padded(10.0);
        assert_eq!(
            (padded.x, padded.y, padded.width, padded.height),
            (0.0, 10.0, 120.0, 70.0)
        );

        let padded = rect.padded(2.5);
        assert_eq!(
            (padded.x, padded.y, padded.width, padded.height),
            (7.5, 17.5, 105.0, 55.0)
        );
        let padded = rect.padded(-10.0);
        assert_eq!(
            (padded.x, padded.y, padded.width, padded.height),
            (20.0, 30.0, 80.0, 30.0)
        );
    }
}

use mlua::{Lua, Result, Table, UserData, UserDataFields, UserDataMethods};
use vello_cpu::kurbo::BezPath;

#[derive(Clone, Debug, Default)]
pub struct Path {
    path: BezPath,
}

impl Path {
    pub fn new() -> Path {
        Path {
            path: BezPath::new(),
        }
    }

    pub fn move_to(&mut self, x: f32, y: f32) {
        self.path.move_to((x as f64, y as f64));
    }

    pub fn line_to(&mut self, x: f32, y: f32) {
        self.path.line_to((x as f64, y as f64));
    }

    pub fn close(&mut self) {
        self.path.close_path();
    }
}

impl AsRef<BezPath> for Path {
    fn as_ref(&self) -> &BezPath {
        &self.path
    }
}

impl UserData for Path {
    fn add_fields<F: UserDataFields<Path>>(fields: &mut F) {
        fields.add_meta_field("__name", "Path");
    }

    fn add_methods<M: UserDataMethods<Path>>(methods: &mut M) {
        methods.add_method_mut("move_to", |_lua, this, point: Table| -> mlua::Result<()> {
            let x: Option<f32> = point.get("x")?;
            let y: Option<f32> = point.get("y")?;

            this.move_to(x.unwrap_or(0.0), y.unwrap_or(0.0));

            Ok(())
        });

        methods.add_method_mut("line_to", |_lua, this, point: Table| -> mlua::Result<()> {
            let x: Option<f32> = point.get("x")?;
            let y: Option<f32> = point.get("y")?;

            this.line_to(x.unwrap_or(0.0), y.unwrap_or(0.0));

            Ok(())
        });

        methods.add_method_mut("close", |_lua, this, (): ()| -> mlua::Result<()> {
            this.close();

            Ok(())
        });
    }
}

pub fn install(lua: &Lua, env: &Table) -> crate::Result<()> {
    env.set(
        "path",
        lua.create_function(|_lua, (): ()| -> Result<Path> { Ok(Path::new()) })?,
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use mlua::{AnyUserData, Function, Lua, ObjectLike, Table};
    use vello_cpu::kurbo::{PathEl, Point};

    use super::{Path, install};

    fn installed() -> (Lua, Table, Function) {
        let lua = Lua::new();
        let env = lua.create_table().unwrap();
        install(&lua, &env).unwrap();
        let path: Function = env.get("path").unwrap();
        (lua, env, path)
    }

    #[test]
    fn the_path_records_its_segments() {
        let mut path = Path::new();
        path.move_to(1.0, 2.0);
        path.line_to(3.0, 4.0);
        path.close();

        assert_eq!(
            path.path.elements(),
            &[
                PathEl::MoveTo(Point::new(1.0, 2.0)),
                PathEl::LineTo(Point::new(3.0, 4.0)),
                PathEl::ClosePath,
            ]
        );
    }

    #[test]
    fn the_op_records_its_segments() {
        let (lua, _env, path) = installed();

        let path: AnyUserData = path.call(()).unwrap();
        let point = lua.create_table().unwrap();
        point.set("x", 1.0).unwrap();
        point.set("y", 2.0).unwrap();
        path.call_method::<()>("move_to", (point,)).unwrap();

        let point = lua.create_table().unwrap();
        point.set("x", 3.0).unwrap();
        path.call_method::<()>("line_to", (point,)).unwrap();

        path.call_method::<()>("close", ()).unwrap();

        let path = path.borrow::<Path>().unwrap();
        assert_eq!(
            path.path.elements(),
            &[
                PathEl::MoveTo(Point::new(1.0, 2.0)),
                PathEl::LineTo(Point::new(3.0, 0.0)),
                PathEl::ClosePath,
            ]
        );
    }
}

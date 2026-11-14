use mlua::{Lua, Table, UserData, UserDataFields, Value};

use crate::lua;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl UserData for Point {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field("__name", "Point");
        fields.add_field_method_get("x", |_lua, this| -> mlua::Result<f32> { Ok(this.x) });
        fields.add_field_method_get("y", |_lua, this| -> mlua::Result<f32> { Ok(this.y) });
    }
}

pub fn install(lua: &Lua, env: &Table) -> crate::Result<()> {
    env.set(
        "point",
        lua.create_function(|_lua, value: Value| -> mlua::Result<Point> {
            lua::point("point", &value)
        })?,
    )?;

    Ok(())
}

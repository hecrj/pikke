use std::cell::RefCell;

use mlua::{
    Lua, MetaMethod, ObjectLike, Result, Table, UserData, UserDataFields, UserDataMethods, Value,
};

use crate::{Color, Image, Scale};

pub struct Stream {
    scale: Scale,
    done: RefCell<bool>,
}

impl UserData for Stream {
    fn add_fields<F: UserDataFields<Stream>>(fields: &mut F) {
        fields.add_meta_field("__name", "Stream");
    }

    fn add_methods<M: UserDataMethods<Stream>>(methods: &mut M) {
        methods.add_meta_method(
            MetaMethod::Call,
            |lua, this: &Stream, _args: ()| -> Result<Value> {
                if *this.done.borrow() {
                    return Ok(Value::Nil);
                }
                *this.done.borrow_mut() = true;
                let frame =
                    lua.create_userdata(Image::new(this.scale, 1.0, 1.0, Color::TRANSPARENT)?)?;
                Ok(frame.to_value())
            },
        );
    }
}

pub fn install(lua: &Lua, env: &Table, scale: Scale) -> anywho::Result<()> {
    env.set(
        "spawn",
        lua.create_function(
            move |_lua, (_program, _args): (String, Table)| -> Result<Stream> {
                Ok(Stream {
                    scale,
                    done: RefCell::new(false),
                })
            },
        )?,
    )?;
    Ok(())
}

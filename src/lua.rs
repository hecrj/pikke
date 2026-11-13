use std::borrow::Cow;
use std::path::{Path, PathBuf};

use mlua::{Lua, Result, Value};

pub fn number(op: &str, name: &str, value: Option<Value>) -> Result<f32> {
    let value = match value {
        None => 0.0,
        Some(Value::Number(value)) => value as f32,
        Some(Value::Integer(value)) => value as f32,
        Some(_) => {
            return Err(mlua::Error::RuntimeError(format!(
                "{op}: '{name}' is not a number"
            )));
        }
    };

    if !value.is_finite() {
        return Err(mlua::Error::RuntimeError(format!(
            "{op}: '{name}' must be finite"
        )));
    }

    Ok(value)
}

pub fn module_dir(lua: &Lua) -> PathBuf {
    let mut i = 0;

    loop {
        let Some(source) = lua.inspect_stack(i, |debug| debug.source().source.map(Cow::into_owned))
        else {
            return PathBuf::new();
        };

        i += 1;

        let Some(source) = source.and_then(|source| {
            if source.starts_with('=') {
                return None;
            }

            Path::new(source.trim_prefix('@'))
                .parent()
                .map(Path::to_path_buf)
        }) else {
            continue;
        };

        return source;
    }
}

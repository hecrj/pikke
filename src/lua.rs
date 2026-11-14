use std::borrow::Cow;
use std::path::{Path, PathBuf};

use mlua::{Lua, Result, Value};

use crate::Color;
use crate::color::Oklch;

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

pub fn hex(operation: &str, color: &str) -> Result<Color> {
    let message = || format!("{operation}: {color} is not a #RRGGBB hex color");
    let digits = color
        .strip_prefix('#')
        .filter(|digits| digits.len() == 6)
        .ok_or_else(|| mlua::Error::RuntimeError(message()))?;

    let channel = |start: usize| -> Result<u8> {
        u8::from_str_radix(&digits[start..start + 2], 16)
            .map_err(|_| mlua::Error::RuntimeError(message()))
    };

    Ok(Color::from_rgba8([channel(0)?, channel(2)?, channel(4)?, 255]).to_linear())
}

pub fn color(operation: &str, value: &Value) -> Result<Color> {
    if let Some(color) = value.as_string() {
        return hex(operation, &color.to_str()?);
    }

    if let Some(color) = value
        .as_userdata()
        .and_then(|color| color.borrow::<Color>().ok())
    {
        return Ok(*color);
    }

    if let Some(color) = value
        .as_userdata()
        .and_then(|color| color.borrow::<Oklch>().ok())
    {
        return Ok(color.to_linear_rgb());
    }

    Err(mlua::Error::RuntimeError(format!(
        "{operation}: the color must be a `Color`, an `Oklch`, or a hex string (was {})",
        value.type_name()
    )))
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

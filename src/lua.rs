use mlua::{Result, Value};

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

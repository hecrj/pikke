use std::sync::Arc;

use mlua::{Lua, Table, UserData, UserDataFields, UserDataMethods, Value};

use crate::Color;
use crate::lua;

#[derive(Debug, Clone, PartialEq)]
pub struct Gradient {
    angle: f32,
    stops: Arc<[Stop]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Stop {
    offset: f32,
    color: Color,
}

impl Gradient {
    pub(crate) fn axis(&self) -> (f32, f32) {
        let radians = f64::from(self.angle) * std::f64::consts::PI / 180.0;
        let (sin, cos) = radians.sin_cos();

        (sin as f32, -cos as f32)
    }

    pub(crate) fn color_at(&self, t: f32) -> Color {
        let stops = self.stops.as_ref();
        let Some(first) = stops.first() else {
            return Color::from_rgba([0.0; 4]).demultiply();
        };

        if t <= first.offset {
            return first.color;
        }

        let last = stops.last().unwrap();

        if t >= last.offset {
            return last.color;
        }

        for pair in stops.windows(2) {
            let (lower, upper) = (&pair[0], &pair[1]);

            if lower.offset <= t && t <= upper.offset {
                if lower.offset == upper.offset {
                    return lower.color;
                }

                let weight = (t - lower.offset) / (upper.offset - lower.offset);

                return Color::mix(lower.color, upper.color, weight);
            }
        }

        last.color
    }
}

impl UserData for Gradient {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_meta_field("__name", "Gradient");
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut(
            "stop",
            |_lua, this, (offset, color): (Value, Value)| -> mlua::Result<Gradient> {
                let offset = lua::number("stop", "offset", Some(offset))?;

                if !(0.0..=1.0).contains(&offset) {
                    return Err(mlua::Error::RuntimeError(
                        "stop: 'offset' must be between 0 and 1".to_owned(),
                    ));
                }

                let color = lua::color("stop", &color)?;

                let mut stops = (*this.stops).to_vec();
                stops.push(Stop { offset, color });
                this.stops = stops.into();

                Ok(this.clone())
            },
        );
    }
}

pub fn install(lua: &Lua, env: &Table) -> crate::Result<()> {
    env.set(
        "gradient",
        lua.create_function(|_lua, angle: Value| -> mlua::Result<Gradient> {
            let angle = lua::number("gradient", "angle", Some(angle))?;

            Ok(Gradient {
                angle,
                stops: Arc::from([]),
            })
        })?,
    )?;

    Ok(())
}

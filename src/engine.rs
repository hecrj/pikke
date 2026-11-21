use std::path::{Path, PathBuf};
use std::sync::Arc;

use mlua::{AnyUserData, Function, IntoLua, Lua, Table, Value};

use crate::{
    Compression, Error, Result, Scale, Video, color, font, gradient, image, path, point, rectangle,
    stream, text,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub compression: Compression,
    pub scale_video: f32,
    pub framerate: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            compression: Compression::default(),
            scale_video: 1.0,
            framerate: 60.0,
        }
    }
}

pub struct Engine {
    script: PathBuf,
    lua: Lua,
    render: Function,
    arguments: Vec<String>,
    env: Table,
}

impl Engine {
    pub fn load(script: impl AsRef<Path>, scale: impl Into<Scale>) -> Result<Engine> {
        let script = script.as_ref().to_path_buf();
        let scale = scale.into();

        let source = std::fs::read_to_string(&script).map_err(|error| Error::ReadFailed {
            script: script.clone(),
            error: Arc::new(error),
        })?;

        let lua = Lua::new();
        let env = lua.globals();
        env.set("scale", f32::from(scale))?;

        let script_dir = script
            .parent()
            .map(|dir| dir.to_path_buf())
            .unwrap_or_default();

        point::install(&lua, &env)?;
        color::install(&lua, &env)?;
        gradient::install(&lua, &env)?;
        image::install(&lua, &env, scale)?;
        font::install(&lua, &env)?;
        rectangle::install(&lua, &env)?;
        path::install(&lua, &env)?;
        text::install(&lua, &env, scale)?;
        stream::install(&lua, &env, scale)?;

        let package: Table = env.get("package")?;
        let existing: String = package.get("path")?;
        let path = format!(
            "{};{};{}",
            script_dir.join("?.lua").display(),
            script_dir.join("?/init.lua").display(),
            existing
        );
        package.set("path", path)?;

        let chunk = lua.load(source).set_name(format!("@{}", script.display()));
        let module = chunk.call::<Table>(()).map_err(|error| match error {
            mlua::Error::FromLuaConversionError { .. } => Error::ModuleNotTable {
                script: script.clone(),
                error: Arc::new(error),
            },
            error => error.into(),
        })?;

        let render: Function = module.get("render").map_err(|_| Error::RenderMissing {
            script: script.clone(),
        })?;

        let arguments = arguments(&script, &module)?;

        Ok(Engine {
            script,
            lua,
            render,
            arguments,
            env,
        })
    }

    pub fn script(&self) -> &Path {
        &self.script
    }

    pub fn arguments(&self) -> &[String] {
        &self.arguments
    }

    pub fn run(
        &self,
        params: &[(String, serde_json::Value)],
        output: Option<&Path>,
        settings: Settings,
    ) -> Result<()> {
        let script = self.script.clone();

        let output = output
            .map(Path::to_path_buf)
            .unwrap_or_else(|| default_output(&script));
        let video = output.with_extension("mp4");

        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut number = 0;
        let mut run = None;

        self.lua
            .set_app_data(Video::new(&video, settings.scale_video, settings.framerate));

        self.env.set(
            "yield",
            self.lua
                .create_function_mut(move |lua, frame: AnyUserData| -> mlua::Result<()> {
                    number += 1;

                    let frame = frame.borrow::<image::Reference>()?;
                    let image = frame.image()?;

                    let width = image.width();
                    let height = image.height();

                    if let Some((run_width, run_height)) = run
                        && (width != run_width || height != run_height)
                    {
                        return Err(mlua::Error::RuntimeError(format!(
                            "yield: frame {number} is {}×{}, the run is {}×{}",
                            width, height, run_width, run_height
                        )));
                    }

                    eprintln!("yield: frame {number} ({width}×{height})",);

                    if number == 1 {
                        run = Some((width, height));
                        image.write(&output, settings.compression)?;

                        eprintln!("output: {} ({width}×{height})", output.display(),);
                    }

                    lua.app_data_mut::<Video>()
                        .expect("video type must be set")
                        .push(width, height, move || image.to_rgba8())?;

                    lua.gc_collect()?;
                    lua.gc_collect()?;

                    Ok(())
                })?,
        )?;

        let args = self.lua.create_table()?;

        for (name, value) in params {
            let value = lua_value(&self.lua, name.as_str(), value)?;

            args.set(name.as_str(), value)?;
        }

        self.render.call::<()>(args)?;

        let frames = self
            .lua
            .remove_app_data::<Video>()
            .expect("video type must be set")
            .finish()?;

        if frames > 1 {
            eprintln!("output: {} ({frames} frames)", video.display());
        }

        self.lua.gc_collect()?;
        self.lua.gc_collect()?;

        Ok(())
    }
}

fn default_output(script: &Path) -> PathBuf {
    script
        .file_name()
        .map(|name| Path::new(name).with_extension("png"))
        .unwrap_or(PathBuf::from("output.png"))
}

fn lua_value(lua: &Lua, option: &str, value: &serde_json::Value) -> Result<Value> {
    Ok(match value {
        serde_json::Value::Null => Value::Nil,
        serde_json::Value::String(value) => value.as_str().into_lua(lua)?,
        serde_json::Value::Number(number) => {
            if let Some(integer) = number.as_i64() {
                integer.into_lua(lua)?
            } else {
                number
                    .as_f64()
                    .expect("a JSON number is always a float")
                    .into_lua(lua)?
            }
        }
        serde_json::Value::Object(object) => {
            let table = lua.create_table()?;

            for (key, value) in object {
                table.set(key.as_str(), lua_value(lua, option, value)?)?;
            }

            Value::Table(table)
        }
        serde_json::Value::Array(array) => {
            let table = lua.create_table()?;

            for value in array {
                table.push(lua_value(lua, option, value)?)?;
            }

            Value::Table(table)
        }
        value => {
            return Err(Error::OptionInvalidValue {
                option: option.to_owned(),
                value: value.to_string(),
                note: "must be a string, number, null, or object".to_owned(),
            });
        }
    })
}

fn arguments(script: &Path, module: &Table) -> Result<Vec<String>> {
    let arguments: Option<Table> =
        module
            .get("arguments")
            .map_err(|error| Error::ArgumentsNotTable {
                script: script.to_path_buf(),
                error: Arc::new(error),
            })?;

    let Some(arguments) = arguments else {
        return Ok(Vec::new());
    };

    let mut names = Vec::new();

    for (i, argument) in arguments.sequence_values().enumerate() {
        match argument {
            Ok(name) if name == "scale" || name == "output" => {
                return Err(Error::ArgumentReserved {
                    script: script.to_path_buf(),
                    name,
                });
            }
            Ok(name) => names.push(name),
            Err(error) => {
                return Err(Error::ArgumentNotString {
                    script: script.to_path_buf(),
                    index: i + 1,
                    error: Arc::new(error),
                });
            }
        }
    }

    Ok(names)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::default_output;

    #[test]
    fn the_default_output_is_the_script_name_with_a_png_extension() {
        assert_eq!(
            default_output(Path::new("hello.lua")),
            std::path::PathBuf::from("hello.png")
        );
        assert_eq!(
            default_output(Path::new("personal/standard.lua")),
            std::path::PathBuf::from("standard.png")
        );
        assert_eq!(
            default_output(Path::new("noext")),
            std::path::PathBuf::from("noext.png")
        );
    }
}

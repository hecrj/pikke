use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use anywho::anywho;
use mlua::{AnyUserData, Function, Lua, Table};

use crate::{Image, Scale, font, image, rectangle, stream, text};

pub struct Engine {
    script: PathBuf,
    lua: Lua,
    render: Function,
    arguments: Vec<String>,
    env: Table,
}

impl Engine {
    pub fn load(script: impl AsRef<Path>, scale: impl Into<Scale>) -> anywho::Result<Engine> {
        let script = script.as_ref().to_path_buf();
        let scale = scale.into();

        let source = std::fs::read_to_string(&script)
            .map_err(|error| anywho!("failed to read {}: {error}", script.display()))?;

        let lua = Lua::new();

        let env = lua.create_table()?;
        env.set("_G", env.clone())?;

        let script_dir = script
            .parent()
            .map(|dir| dir.to_path_buf())
            .unwrap_or_default();

        image::install(&lua, &env, &script_dir, scale)?;
        font::install(&lua, &env, &script_dir)?;
        rectangle::install(&lua, &env)?;
        text::install(&lua, &env, scale)?;
        stream::install(&lua, &env, scale)?;

        let chunk = lua
            .load(source)
            .set_name(script.display().to_string())
            .set_environment(env.clone());

        chunk.call::<()>(())?;

        let render: Function = env.get("render").map_err(|_| {
            anywho!(
                "script {}: 'render' is missing or not a function",
                script.display()
            )
        })?;

        let arguments = arguments(&script, &env)?;

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
        params: &[(String, Option<String>)],
        output: Option<&Path>,
    ) -> anywho::Result<()> {
        let frames = Rc::new(RefCell::new(Vec::new()));
        let yielded = frames.clone();

        self.env.set(
            "yield",
            self.lua
                .create_function(move |_lua, frame: AnyUserData| -> mlua::Result<()> {
                    let frame = frame.borrow::<Image>()?;
                    let frame_number = yielded.borrow().len() + 1;

                    eprintln!(
                        "yield: frame {frame_number} ({}×{})",
                        frame.width(),
                        frame.height()
                    );

                    yielded.borrow_mut().push((*frame).clone());

                    Ok(())
                })?,
        )?;

        let args = self.lua.create_table()?;

        for (name, value) in params {
            args.set(name.as_str(), value.clone())?;
        }

        self.render.call::<()>(args)?;

        let frames = frames.borrow();

        match frames.len() {
            1 => {
                let output = output
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| default_output(self.script()));

                frames[0].write(&output)?;

                eprintln!(
                    "output: {} ({}×{})",
                    output.display(),
                    frames[0].width(),
                    frames[0].height()
                );
            }
            0 => {}
            _ => return Err(anywho!("the MP4 path lands in Phase 9")),
        }

        Ok(())
    }
}

fn default_output(script: &Path) -> PathBuf {
    script
        .file_name()
        .map(|name| Path::new(name).with_extension("png"))
        .unwrap_or(PathBuf::from("output.png"))
}

fn arguments(script: &Path, env: &Table) -> anywho::Result<Vec<String>> {
    let arguments: Option<Table> = env.get("arguments").map_err(|error| {
        anywho!(
            "script {}: 'arguments' must be a table ({error})",
            script.display()
        )
    })?;

    let Some(arguments) = arguments else {
        return Ok(Vec::new());
    };

    let mut names = Vec::new();

    for (i, argument) in arguments.sequence_values().enumerate() {
        match argument {
            Ok(name) if name == "scale" || name == "output" => {
                return Err(anywho!(
                    "script {}: '{name}' is a reserved argument name",
                    script.display()
                ));
            }
            Ok(name) => names.push(name),
            Err(error) => {
                return Err(anywho!(
                    "script {}: 'arguments' entry {} must be a string ({error})",
                    i + 1,
                    script.display()
                ));
            }
        }
    }

    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::{Engine, Scale, default_output};

    use std::io::Write;
    use std::path::Path;

    fn load_script(name: &str, source: &str) -> (Engine, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("pikke-engine-test-{}-{}", std::process::id(), name));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("script.lua");
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(source.as_bytes()).unwrap();
        drop(file);
        (Engine::load(&path, Scale::default()).unwrap(), path)
    }

    #[test]
    fn the_load_defaults_a_missing_arguments_to_empty() {
        let (engine, _path) = load_script(
            "no-args",
            r#"
function render(args)
end
"#,
        );
        assert!(engine.arguments().is_empty());
    }

    #[test]
    fn the_load_reads_the_declared_arguments() {
        let (engine, _path) = load_script(
            "with-args",
            r#"
arguments = {"id", "title"}
function render(args)
end
"#,
        );
        assert_eq!(engine.arguments(), &["id".to_string(), "title".to_string()]);
    }

    #[test]
    fn the_load_rejects_a_non_table_arguments() {
        let dir =
            std::env::temp_dir().join(format!("pikke-engine-test-{}-bad", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("script.lua");
        std::fs::write(
            &path,
            r#"
arguments = "nope"
function render(args)
end
"#,
        )
        .unwrap();
        let error = match Engine::load(&path, Scale::default()) {
            Ok(_) => panic!("expected a load error"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("must be a table"));
    }

    #[test]
    fn the_load_rejects_a_reserved_argument_name() {
        let dir =
            std::env::temp_dir().join(format!("pikke-engine-test-{}-reserved", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("script.lua");
        std::fs::write(
            &path,
            r#"
arguments = {"output"}
function render(args)
end
"#,
        )
        .unwrap();
        let error = match Engine::load(&path, Scale::default()) {
            Ok(_) => panic!("expected a load error"),
            Err(error) => error.to_string(),
        };
        assert!(error.contains("reserved argument name"));
    }

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

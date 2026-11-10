use std::path::{Path, PathBuf};

use anywho::anywho;
use mlua::{AnyUserData, Function, Lua, Table};

use crate::{Image, Scale, Video, font, image, rectangle, stream, text};

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
        let env = lua.globals();

        let script_dir = script
            .parent()
            .map(|dir| dir.to_path_buf())
            .unwrap_or_default();

        image::install(&lua, &env, &script_dir, scale)?;
        font::install(&lua, &env, &script_dir)?;
        rectangle::install(&lua, &env)?;
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

        let chunk = lua.load(source).set_name(script.display().to_string());
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
        let script = self.script.clone();

        let output = output
            .map(Path::to_path_buf)
            .unwrap_or_else(|| default_output(&script));
        let video = output.with_extension("mp4");

        let mut frame_number = 0;
        let mut dimensions = None;

        self.lua.set_app_data(Video::new(&video));

        self.env.set(
            "yield",
            self.lua
                .create_function_mut(move |lua, frame: AnyUserData| -> mlua::Result<()> {
                    frame_number += 1;

                    let frame = frame.borrow::<Image>()?;
                    let width = frame.width();
                    let height = frame.height();

                    if let Some((run_width, run_height)) = dimensions
                        && (width != run_width || height != run_height)
                    {
                        return Err(mlua::Error::RuntimeError(format!(
                            "yield: frame {frame_number} is {}×{}, the run is {}×{}",
                            width, height, run_width, run_height
                        )));
                    }

                    eprintln!(
                        "yield: frame {frame_number} ({}×{})",
                        frame.width(),
                        frame.height()
                    );

                    if frame_number == 1 {
                        dimensions = Some((frame.width(), frame.height()));

                        frame.write(&output)?;

                        eprintln!(
                            "output: {} ({}×{})",
                            output.display(),
                            frame.width(),
                            frame.height()
                        );
                    }

                    lua.app_data_mut::<Video>()
                        .expect("video type must be set")
                        .push(&frame)?;

                    Ok(())
                })?,
        )?;

        let args = self.lua.create_table()?;

        for (name, value) in params {
            args.set(name.as_str(), value.clone())?;
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
    let arguments: Option<Table> = env.get("ARGUMENTS").map_err(|error| {
        anywho!(
            "script {}: 'ARGUMENTS' must be a table ({error})",
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
                    "script {}: 'ARGUMENTS' entry {} must be a string ({error})",
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
ARGUMENTS = { "id", "title" }
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
ARGUMENTS = "nope"
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
ARGUMENTS = { "output" }
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
    fn the_require_resolves_against_the_script_directory() {
        let dir =
            std::env::temp_dir().join(format!("pikke-engine-test-{}-require", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.lua"), "return { id = \"config\" }\n").unwrap();
        let path = dir.join("script.lua");
        std::fs::write(
            &path,
            r#"
required = (require "config").id
function render(args)
end
"#,
        )
        .unwrap();
        let engine = Engine::load(&path, Scale::default()).unwrap();
        let required: String = engine.env.get("required").unwrap();
        assert_eq!(required, "config");
    }

    #[test]
    fn the_yield_rejects_a_frame_at_other_dims() {
        let (engine, _path) = load_script(
            "yield-dims",
            r#"
function render(args)
  yield(blank({ width = 10, height = 10 }))
  yield(blank({ width = 20, height = 10 }))
end
"#,
        );

        let params: &[(String, Option<String>)] = &[];

        let error = match engine.run(params, None) {
            Ok(()) => panic!("expected a run error"),
            Err(error) => error.to_string(),
        };

        assert!(error.contains("yield"), "{error}");
        assert!(error.contains("20×10"), "{error}");
        assert!(error.contains("10×10"), "{error}");
    }

    fn packets(path: &std::path::Path) -> u32 {
        let out = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-count_packets",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=nb_read_packets",
                "-of",
                "default=nw=1:nk=1",
            ])
            .arg(path)
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&out.stdout);

        out.trim().parse().unwrap()
    }

    #[test]
    fn the_run_yields_a_still() {
        let (engine, _path) = load_script(
            "still",
            r##"
function render(args)
  yield(blank({ width = 10, height = 10, color = "#FF0000" }))
end
"##,
        );

        let dir =
            std::env::temp_dir().join(format!("pikke-engine-test-{}-still", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let output = dir.join("still.png");

        let params: &[(String, Option<String>)] = &[];

        engine.run(params, Some(&output)).unwrap();

        let rgba: Vec<u8> = image::open(&output).unwrap().to_rgba8().into_raw();
        assert_eq!(rgba, [255, 0, 0, 255].repeat(100));
        assert!(!dir.join("still.mp4").exists());
    }

    #[test]
    fn the_run_yields_a_video() {
        let (engine, _path) = load_script(
            "video",
            r##"
function render(args)
  for i = 1, 3 do
    yield(blank({ width = 16, height = 16, color = "#00FF00" }))
  end
end
"##,
        );

        let dir =
            std::env::temp_dir().join(format!("pikke-engine-test-{}-video", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let params: &[(String, Option<String>)] = &[];

        engine.run(params, Some(&dir.join("video.png"))).unwrap();

        let rgba: Vec<u8> = image::open(dir.join("video.png"))
            .unwrap()
            .to_rgba8()
            .into_raw();
        assert_eq!(rgba, [0, 255, 0, 255].repeat(256));
        assert_eq!(packets(&dir.join("video.mp4")), 3);
        assert!(!dir.join("video.mp4.tmp").exists());
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

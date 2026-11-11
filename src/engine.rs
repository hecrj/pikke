use std::path::{Path, PathBuf};
use std::sync::Arc;

use mlua::{AnyUserData, Function, Lua, Table};

use crate::{Error, Result, Scale, Video, font, image, path, rectangle, stream, text};

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

        let script_dir = script
            .parent()
            .map(|dir| dir.to_path_buf())
            .unwrap_or_default();

        image::install(&lua, &env, &script_dir, scale)?;
        font::install(&lua, &env, &script_dir)?;
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

        let chunk = lua.load(source).set_name(script.display().to_string());
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

    pub fn run(&self, params: &[(String, Option<String>)], output: Option<&Path>) -> Result<()> {
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

                    let frame = frame.borrow::<image::Reference>()?.image()?;
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

                    eprintln!("yield: frame {frame_number} ({width}×{height})",);

                    if frame_number == 1 {
                        dimensions = Some((width, height));

                        frame.write(&output)?;

                        eprintln!("output: {} ({width}×{height})", output.display(),);
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
return {
  render = function(args)
  end,
}
"#,
        );
        assert!(engine.arguments().is_empty());
    }

    #[test]
    fn the_load_reads_the_declared_arguments() {
        let (engine, _path) = load_script(
            "with-args",
            r#"
return {
  arguments = { "id", "title" },
  render = function(args)
  end,
}
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
return {
  arguments = "nope",
  render = function(args)
  end,
}
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
return {
  arguments = { "output" },
  render = function(args)
  end,
}
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
return {
  render = function(args)
  end,
}
"#,
        )
        .unwrap();
        let engine = Engine::load(&path, Scale::default()).unwrap();
        let required: String = engine.env.get("required").unwrap();
        assert_eq!(required, "config");
    }

    #[test]
    fn the_yield_rejects_a_frame_at_other_dims() {
        let (engine, path) = load_script(
            "yield-dims",
            r#"
return {
  render = function(args)
    yield(blank({ width = 10, height = 10 }))
    yield(blank({ width = 20, height = 10 }))
  end,
}
"#,
        );

        let params: &[(String, Option<String>)] = &[];

        let error = match engine.run(params, Some(&path.with_extension("png"))) {
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
return {
  render = function(args)
    yield(blank({ width = 10, height = 10, color = "#FF0000" }))
  end,
}
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
    fn the_run_respects_the_clone_branch_point() {
        let (engine, _path) = load_script(
            "clone-branch",
            r##"
return {
  render = function(args)
    local base = blank({ width = 256, height = 256, color = "#808080" })
    local branch = base:clone()

    base:resize({ width = 64 })

    assert(base.width == 64, "base: " .. base.width)
    assert(branch.width == 256, "branch: " .. branch.width)

    yield(branch)
  end,
}
"##,
        );

        let dir = std::env::temp_dir().join(format!(
            "pikke-engine-test-{}-clone-branch",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let output = dir.join("branch.png");

        let params: &[(String, Option<String>)] = &[];

        engine.run(params, Some(&output)).unwrap();

        let rgba: Vec<u8> = image::open(&output).unwrap().to_rgba8().into_raw();
        assert_eq!(rgba, [128, 128, 128, 255].repeat(256 * 256));
        assert!(!dir.join("branch.mp4").exists());
    }

    #[test]
    fn the_run_loads_the_shared_reference_once() {
        let (engine, script_path) = load_script(
            "shared-load",
            r##"
return {
  arguments = { "data" },
  render = function(args)
    local base = image(args.data)
    local left = base:clone()
    local right = base:clone()

    left:fill(rectangle({ x = 0, y = 0, size = { 16, 32 } }), "#FFFFFF")
    right:fill(rectangle({ x = 16, y = 0, size = { 16, 32 } }), "#000000")

    yield(left)
    os.remove(args.data)
    yield(right)
  end,
}
"##,
        );

        let data = script_path.parent().unwrap().join("data.png");
        let source = crate::image::Image::new(
            Scale::default(),
            32.0,
            32.0,
            crate::Color::from_rgba8([200, 30, 60, 255]).to_linear(),
        )
        .unwrap();
        source.write(&data).unwrap();

        let dir = std::env::temp_dir().join(format!(
            "pikke-engine-test-{}-shared-load",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let params: &[(String, Option<String>)] =
            &[("data".to_string(), Some(data.display().to_string()))];

        engine.run(params, Some(&dir.join("shared.png"))).unwrap();

        let rgba: Vec<u8> = image::open(dir.join("shared.png"))
            .unwrap()
            .to_rgba8()
            .into_raw();
        let mut row = [255u8, 255, 255, 255].repeat(16);
        row.extend([200, 30, 60, 255].repeat(16));
        assert_eq!(rgba, row.repeat(32));
        assert_eq!(packets(&dir.join("shared.mp4")), 2);
        assert!(!data.exists());
    }

    #[test]
    fn the_run_yields_a_video() {
        let (engine, _path) = load_script(
            "video",
            r##"
return {
  render = function(args)
    for i = 1, 3 do
      yield(blank({ width = 16, height = 16, color = "#00FF00" }))
    end
  end,
}
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

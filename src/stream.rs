use std::io::{self, Read};
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use mlua::{Lua, MetaMethod, ObjectLike, Table, UserData, UserDataFields, UserDataMethods, Value};

use crate::image;
use crate::{Image, Result, Scale, Viewport};

const STDERR_TAIL: usize = 1 << 20;

pub struct Stream {
    program: String,
    child: Child,
    viewport: Viewport,
    tail: Option<mpsc::Receiver<Vec<u8>>>,
    drain: Option<JoinHandle<()>>,
    buffer: Vec<u8>,
}

impl Stream {
    pub fn spawn(
        program: String,
        args: Vec<String>,
        width: f32,
        height: f32,
        scale: Scale,
    ) -> Result<Stream> {
        let viewport = Viewport::from_logical(width, height, scale)?;

        let mut command = Command::new(&program);
        command.args(args);
        command
            .arg("--width")
            .arg(viewport.width().to_string())
            .arg("--height")
            .arg(viewport.height().to_string());
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = command.spawn().map_err(|error| {
            io::Error::other(format!("stream: failed to spawn {program}: {error}"))
        })?;

        let stderr = child.stderr.take().unwrap();
        let (tail, drain) = drain_stderr(stderr);

        Ok(Stream {
            program,
            child,
            viewport,
            tail: Some(tail),
            drain: Some(drain),
            buffer: vec![0; viewport.total_pixels() * 4],
        })
    }

    pub fn frame(&mut self) -> Result<Option<Image>> {
        if self.drain.is_none() {
            return Err(io::Error::other(format!("stream: {} has ended", self.program)).into());
        }

        let stdout = self.child.stdout.as_mut().expect("stdout must be piped");
        let mut read = 0;

        loop {
            match stdout.read(&mut self.buffer[read..]) {
                Ok(0) => {
                    let _ = self.child.kill();
                    let status = self.child.wait().map_err(|error| {
                        io::Error::other(format!("stream: {}: {error}", self.program))
                    })?;

                    if read == 0 && status.success() {
                        self.drain.take();
                        self.tail.take();

                        return Ok(None);
                    }

                    let message = if read > 0 {
                        format!("stream: {} ended the stream mid-frame", self.program)
                    } else {
                        format!("stream: {} exited with {status}", self.program)
                    };

                    return Err(self.report(message).into());
                }
                Ok(count) if read + count == self.buffer.len() => {
                    return Ok(Some(Image::from_rgba8(self.viewport, &self.buffer)?));
                }
                Ok(count) => read += count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();

                    let message = format!("stream: {}: {error}", self.program);
                    return Err(self.report(message).into());
                }
            }
        }
    }

    fn report(&mut self, message: String) -> io::Error {
        let _ = self.drain.take().unwrap().join();
        let tail = self.tail.take().unwrap().recv().unwrap_or_default();
        let tail = String::from_utf8_lossy(&tail);

        if tail.is_empty() {
            io::Error::other(message)
        } else {
            io::Error::other(format!("{message}: {tail}"))
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl UserData for Stream {
    fn add_fields<F: UserDataFields<Stream>>(fields: &mut F) {
        fields.add_meta_field("__name", "Stream");
    }

    fn add_methods<M: UserDataMethods<Stream>>(methods: &mut M) {
        methods.add_meta_method_mut(
            MetaMethod::Call,
            |lua, this, (): ()| -> mlua::Result<Value> {
                match this.frame()? {
                    Some(frame) => {
                        let frame = lua.create_userdata(image::Reference::new(frame))?;
                        Ok(frame.to_value())
                    }
                    None => Ok(Value::Nil),
                }
            },
        );
    }
}

fn drain_stderr(mut stderr: ChildStderr) -> (mpsc::Receiver<Vec<u8>>, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel();

    let drain = thread::spawn(move || {
        let mut tail = Vec::new();
        let mut chunk = [0u8; 8192];

        loop {
            match stderr.read(&mut chunk) {
                Ok(0) => break,
                Ok(count) => {
                    tail.extend_from_slice(&chunk[..count]);

                    if tail.len() > STDERR_TAIL {
                        tail.drain(..tail.len() - STDERR_TAIL);
                    }
                }
                Err(_) => break,
            }
        }

        let _ = tx.send(tail);
    });

    (rx, drain)
}

pub fn install(lua: &Lua, env: &Table, scale: Scale) -> Result<()> {
    env.set(
        "stream",
        lua.create_function(
            move |_lua,
                  (program, width, height, args): (String, f32, f32, Table)|
                  -> mlua::Result<Stream> {
                if !width.is_finite() || width <= 0.0 {
                    return Err(mlua::Error::RuntimeError(
                        "stream: width must be a finite positive number".to_owned(),
                    ));
                }

                if !height.is_finite() || height <= 0.0 {
                    return Err(mlua::Error::RuntimeError(
                        "stream: height must be a finite positive number".to_owned(),
                    ));
                }

                let mut argv = Vec::new();

                for (index, argument) in args.sequence_values().enumerate() {
                    match argument {
                        Ok(Value::Nil) | Err(_) => {
                            return Err(mlua::Error::RuntimeError(format!(
                                "stream: argument {} is nil",
                                index + 1
                            )));
                        }
                        Ok(Value::String(string)) => {
                            let string = string.to_string_lossy();

                            if string == "--width" || string == "--height" {
                                return Err(mlua::Error::RuntimeError(format!(
                                    "stream: '{string}' is a reserved argument"
                                )));
                            }

                            argv.push(string);
                        }
                        Ok(Value::Number(number)) => {
                            argv.push(number.to_string());
                        }
                        Ok(_) => {
                            return Err(mlua::Error::RuntimeError(format!(
                                "stream: argument {} must be a string or a number",
                                index + 1
                            )));
                        }
                    }
                }

                Stream::spawn(program, argv, width, height, scale)
                    .map_err(|error| mlua::Error::RuntimeError(error.to_string()))
            },
        )?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use mlua::{AnyUserData, Function, Lua, ObjectLike, Table};

    use super::install;
    use crate::{Image, Scale, image};

    const PRODUCER: &str = r#"
import sys

args = sys.argv[1:]
width = 0
height = 0
frames = 0
truncate = False
fail = False

i = 0
while i < len(args):
    if args[i] == "--width":
        width = int(args[i + 1])
        i += 2
    elif args[i] == "--height":
        height = int(args[i + 1])
        i += 2
    elif args[i] == "--frames":
        frames = int(args[i + 1])
        i += 2
    elif args[i] == "--truncate":
        truncate = True
        i += 1
    elif args[i] == "--fail":
        fail = True
        i += 1
    else:
        i += 1

out = sys.stdout.buffer
reps = (width * 4 + 1023) // 1024

for frame in range(frames):
    x = bytes((i + frame) % 256 for i in range(256))
    y = bytes((i + frame) % 256 for i in range(256))
    tile = b"".join(
        bytes(v for i in range(256) for v in (x[i], y[j], frame % 256, 255))
        for j in range(256)
    )
    image = b"".join(
        (tile[(j % 256) * 1024:(j % 256 + 1) * 1024] * reps)[: width * 4]
        for j in range(height)
    )

    if truncate and frame == frames - 1:
        sys.stderr.write("truncating the stream\n")
        out.write(image[: len(image) // 2])
    else:
        out.write(image)

out.flush()

if fail:
    sys.stderr.write("the producer failed\n")
    sys.exit(3)
"#;

    fn engine(scale: f32) -> (Lua, Table) {
        let lua = Lua::new();
        let env = lua.globals();
        install(&lua, &env, Scale::new(scale)).unwrap();
        (lua, env)
    }

    fn stream(
        lua: &Lua,
        env: &Table,
        width: f32,
        height: f32,
        args: &[&str],
    ) -> mlua::Result<AnyUserData> {
        let stream: Function = env.get("stream")?;
        let args_table = lua.create_table()?;

        for (index, argument) in args.iter().enumerate() {
            args_table.set(index + 1, *argument)?;
        }

        stream.call(("python3", width, height, args_table))
    }

    fn pull(stream: &AnyUserData) -> mlua::Result<Vec<Image>> {
        let mut frames = Vec::new();

        loop {
            let frame: mlua::Value = stream.call(())?;

            if frame.is_nil() {
                return Ok(frames);
            }

            let frame = frame.as_userdata().unwrap();
            frames.push(frame.borrow::<image::Reference>()?.image()?);
        }
    }

    fn frame_bytes(width: u16, height: u16, frame: u32) -> Vec<u8> {
        let mut bytes = Vec::new();

        for y in 0..u32::from(height) {
            for x in 0..u32::from(width) {
                bytes.extend_from_slice(&[
                    ((x + frame) % 256) as u8,
                    ((y + frame) % 256) as u8,
                    (frame % 256) as u8,
                    255,
                ]);
            }
        }

        bytes
    }

    #[test]
    fn the_stream_yields_frames_at_the_run_scale() {
        for (scale, physical_width, physical_height) in [(1.0, 100u16, 100u16), (0.5, 50u16, 50u16)]
        {
            let (lua, env) = engine(scale);
            let stream =
                stream(&lua, &env, 100.0, 100.0, &["-c", PRODUCER, "--frames", "3"]).unwrap();
            let frames = pull(&stream).unwrap();

            assert_eq!(frames.len(), 3);

            for (index, frame) in frames.iter().enumerate() {
                assert_eq!(
                    (frame.width(), frame.height()),
                    (physical_width, physical_height)
                );
                assert_eq!(
                    (frame.logical_width(), frame.logical_height()),
                    (100.0, 100.0)
                );
                assert_eq!(
                    frame.to_rgba8(),
                    frame_bytes(physical_width, physical_height, index as u32)
                );
            }
        }
    }

    #[test]
    fn the_stream_surfaces_a_truncated_stream() {
        let (lua, env) = engine(1.0);
        let stream = stream(
            &lua,
            &env,
            100.0,
            100.0,
            &["-c", PRODUCER, "--frames", "3", "--truncate"],
        )
        .unwrap();

        let error = match pull(&stream) {
            Ok(_) => panic!("expected a stream error"),
            Err(error) => error.to_string(),
        };

        assert!(error.contains("mid-frame"), "{error}");
        assert!(error.contains("truncating the stream"), "{error}");
    }

    #[test]
    fn the_stream_surfaces_a_non_zero_exit() {
        let (lua, env) = engine(1.0);
        let stream = stream(
            &lua,
            &env,
            100.0,
            100.0,
            &["-c", PRODUCER, "--frames", "2", "--fail"],
        )
        .unwrap();

        let error = match pull(&stream) {
            Ok(_) => panic!("expected a stream error"),
            Err(error) => error.to_string(),
        };

        assert!(error.contains("exit status: 3"), "{error}");
        assert!(error.contains("the producer failed"), "{error}");
    }

    #[test]
    fn the_stream_fails_to_spawn_a_missing_producer() {
        let (lua, env) = engine(1.0);
        let stream: Function = env.get("stream").unwrap();
        let args = lua.create_table().unwrap();

        let error = stream
            .call::<AnyUserData>(("pikke-no-such-producer", 100.0, 100.0, args))
            .unwrap_err()
            .to_string();

        assert!(error.contains("failed to spawn"), "{error}");
    }

    #[test]
    fn the_stream_rejects_bad_viewports() {
        for width in [0.0f32, -1.0, f32::INFINITY] {
            let (lua, env) = engine(1.0);

            let error = stream(&lua, &env, width, 100.0, &["-c", PRODUCER])
                .unwrap_err()
                .to_string();

            assert!(error.contains("width"), "{error}");
        }

        let (lua, env) = engine(1.0);

        let error = stream(&lua, &env, 100.0, 0.0, &["-c", PRODUCER])
            .unwrap_err()
            .to_string();

        assert!(error.contains("height"), "{error}");
    }

    #[test]
    fn the_stream_rejects_reserved_arguments() {
        for arg in ["--width", "--height"] {
            let (lua, env) = engine(1.0);

            let error = stream(&lua, &env, 100.0, 100.0, &["-c", PRODUCER, arg])
                .unwrap_err()
                .to_string();

            assert!(error.contains("reserved"), "{error}");
        }
    }

    #[test]
    fn the_stream_rejects_a_boolean_argument() {
        let (lua, env) = engine(1.0);
        let stream: Function = env.get("stream").unwrap();
        let args = lua.create_table().unwrap();
        args.set(1, true).unwrap();

        let error = stream
            .call::<AnyUserData>(("python3", 100.0, 100.0, args))
            .unwrap_err()
            .to_string();

        assert!(error.contains("string or a number"), "{error}");
    }
}

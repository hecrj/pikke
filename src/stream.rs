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

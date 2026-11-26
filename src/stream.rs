use std::io::{self, Read};
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use mlua::{Lua, MetaMethod, ObjectLike, Table, UserData, UserDataFields, UserDataMethods, Value};

use crate::color;
use crate::image;
use crate::lua;
use crate::{Image, Result, Scale, Viewport};

const STDERR_TAIL: usize = 1 << 20;

pub struct Stream {
    program: String,
    child: Child,
    viewport: Viewport,
    space: color::Space,
    alpha: color::Alpha,
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
        framerate: u32,
        scale: Scale,
        space: color::Space,
        alpha: color::Alpha,
    ) -> Result<Stream> {
        let viewport = Viewport::from_logical(width, height, scale)?;

        let mut command = Command::new(&program);
        command.args(args);
        command
            .arg("--width")
            .arg(viewport.width().to_string())
            .arg("--height")
            .arg(viewport.height().to_string())
            .arg("--framerate")
            .arg(framerate.to_string());

        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        log::debug!("stream: {command:?}");

        let mut child = command.spawn().map_err(|error| {
            io::Error::other(format!("stream: failed to spawn {program}: {error}"))
        })?;

        let stderr = child.stderr.take().unwrap();
        let (tail, drain) = drain_stderr(stderr);

        Ok(Stream {
            program,
            child,
            viewport,
            space,
            alpha,
            tail: Some(tail),
            drain: Some(drain),
            buffer: vec![0; viewport.total_pixels() * 4],
        })
    }

    pub fn frame(&mut self) -> Result<Option<image::Value>> {
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
                    fn from_rgba<Space: 'static>(
                        alpha: color::Alpha,
                        buffer: &[u8],
                    ) -> color::Buffer<Space>
                    where
                        color::Srgb: color::ToSpace<Space>,
                    {
                        match alpha {
                            color::Alpha::Straight => {
                                color::Buffer::from_rgba::<color::Straight>(buffer)
                            }
                            color::Alpha::Premultiplied => {
                                color::Buffer::from_rgba::<color::Premultiplied>(buffer)
                            }
                        }
                    }

                    return Ok(Some(match self.space {
                        color::Space::Linear => Image::from_buffer(
                            self.viewport,
                            from_rgba(self.alpha, self.buffer.as_slice()),
                        )
                        .map(image::Value::Linear),
                        color::Space::Srgb => Image::from_buffer(
                            self.viewport,
                            from_rgba(self.alpha, self.buffer.as_slice()),
                        )
                        .map(image::Value::Srgb),
                    }?));
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

pub fn install(lua: &Lua, env: &Table, scale: Scale, framerate: u32) -> Result<()> {
    env.set(
        "stream",
        lua.create_function(
            move |_lua, (program, opts): (String, Table)| -> mlua::Result<Stream> {
                let width = lua::number("stream", "width", opts.get("width")?)?;
                let height = lua::number("stream", "height", opts.get("height")?)?;

                if width <= 0.0 {
                    return Err(mlua::Error::RuntimeError(
                        "stream: width must be a positive number".to_owned(),
                    ));
                }

                if height <= 0.0 {
                    return Err(mlua::Error::RuntimeError(
                        "stream: height must be a positive number".to_owned(),
                    ));
                }

                let color_space = opts
                    .get::<Option<String>>("color_space")?
                    .as_deref()
                    .map_or(Ok(color::Space::Linear), |value| {
                        lua::color_space("stream", value)
                    })?;

                let alpha: Option<String> = opts.get("alpha")?;

                let alpha = match alpha.as_deref() {
                    None => color::Alpha::default(),
                    Some("straight") => color::Alpha::Straight,
                    Some("premultiplied") => color::Alpha::Premultiplied,
                    _ => {
                        return Err(mlua::Error::RuntimeError(
                            "stream: alpha must be either straight or premultiplied".to_owned(),
                        ));
                    }
                };

                let argv = argv(opts)?;

                Stream::spawn(
                    program,
                    argv,
                    width,
                    height,
                    framerate,
                    scale,
                    color_space,
                    alpha,
                )
                .map_err(|error| mlua::Error::RuntimeError(error.to_string()))
            },
        )?,
    )?;
    Ok(())
}

fn argv(opts: Table) -> mlua::Result<Vec<String>> {
    let args: Option<Table> = opts.get("args")?;

    let mut argv = Vec::new();

    if let Some(args) = args {
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
                Ok(Value::Integer(integer)) => {
                    argv.push(integer.to_string());
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
    }

    Ok(argv)
}

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStderr, Command, Stdio};
use std::sync::mpsc;
use std::thread;

use crate::color;

use crate::{Error, Result};

const STDERR_TAIL: usize = 1 << 20;

pub struct Video {
    output: PathBuf,
    scale: f32,
    frames: usize,
    state: State,
}

type ToRgba = Box<dyn FnOnce() -> color::Rgba<'static> + Send + Sync + 'static>;

#[derive(Default)]
enum State {
    #[default]
    Empty,
    Still {
        frame: ToRgba,
    },
    Encoding {
        sender: mpsc::SyncSender<Option<ToRgba>>,
        handle: thread::JoinHandle<Result<()>>,
    },
}

impl Video {
    pub fn new(path: impl AsRef<Path>, scale: f32) -> Self {
        Self {
            output: path.as_ref().to_path_buf(),
            scale,
            frames: 0,
            state: State::Empty,
        }
    }

    pub fn push(
        &mut self,
        width: u16,
        height: u16,
        frame: impl FnOnce() -> color::Rgba<'static> + Send + Sync + 'static,
    ) -> Result<()> {
        self.frames += 1;

        match std::mem::take(&mut self.state) {
            State::Empty => {
                self.state = State::Still {
                    frame: Box::new(frame),
                };
            }
            State::Still { frame: first } => {
                if width % 2 == 1 || height % 2 == 1 {
                    return Err(Error::OddDimensions { width, height });
                }

                let (sender, receiver) = mpsc::sync_channel(1);

                let output = self.output.clone();
                let scale = self.scale;
                let handle = thread::spawn(move || worker(output, width, height, scale, receiver));

                sender.send(Some(first)).map_err(io::Error::other)?;
                sender
                    .send(Some(Box::new(frame)))
                    .map_err(io::Error::other)?;

                self.state = State::Encoding { sender, handle }
            }
            State::Encoding { sender, handle } => {
                sender
                    .send(Some(Box::new(frame)))
                    .map_err(io::Error::other)?;
                self.state = State::Encoding { sender, handle };
            }
        }

        Ok(())
    }

    pub fn finish(self) -> Result<usize> {
        let State::Encoding { sender, handle } = self.state else {
            return Ok(self.frames);
        };

        let _ = sender.send(None);

        handle
            .join()
            .map_err(|_| io::Error::other("video encoder failed to join"))??;

        Ok(self.frames)
    }
}

fn worker(
    output: PathBuf,
    width: u16,
    height: u16,
    scale: f32,
    receiver: mpsc::Receiver<Option<ToRgba>>,
) -> Result<()> {
    let file_name = output
        .file_name()
        .ok_or_else(|| io::Error::other(format!("invalid output path: {}", output.display())))?;

    let temp = {
        let mut temp_name = file_name.to_os_string();
        temp_name.push(".tmp");
        output.with_file_name(temp_name)
    };

    let _ = std::fs::remove_file(&temp);

    let mut command = Command::new("ffmpeg");
    command.args(["-f", "rawvideo", "-pix_fmt", "rgba", "-s"]);
    command.arg(format!("{width}x{height}"));
    command.args(["-framerate", "60", "-i", "-"]);

    if scale != 1.0 {
        command.args(["-vf", &format!("scale=iw*{scale}:-1:flags=lanczos")]);
    }

    command.args([
        "-c:v",
        "libx264",
        "-preset",
        "slow",
        "-crf",
        "16",
        "-pix_fmt",
        "yuv420p",
        "-movflags",
        "+faststart",
        "-f",
        "mp4",
    ]);
    command
        .arg("-x264-params")
        .arg(format!(
            "threads={}",
            (rayon::current_num_threads() / 4).clamp(2, 8)
        ))
        .arg(&temp)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .map_err(|error| io::Error::other(format!("failed to spawn ffmpeg: {error}")))?;

    let stderr = child.stderr.take().unwrap();
    let (tail, drain) = drain_stderr(stderr);
    let mut stdin = child.stdin.take().unwrap();
    let mut frames = receiver.iter();

    loop {
        match frames.next() {
            Some(frame) => {
                if let Some(frame) = frame
                    && stdin.write_all(frame().as_slice()).is_ok()
                {
                    continue;
                }

                drop(stdin);

                let status = child.wait().map_err(|error| {
                    io::Error::other(format!("failed to wait for ffmpeg: {error}"))
                })?;

                if status.success() {
                    std::fs::rename(&temp, &output).map_err(|error| {
                        io::Error::other(format!("failed to write {}: {error}", output.display()))
                    })?;

                    return Ok(());
                }

                let _ = std::fs::remove_file(&temp);
                let _ = drain.join();
                let tail = tail.recv().unwrap_or_default();
                let tail = String::from_utf8_lossy(&tail);

                return if tail.is_empty() {
                    Err(io::Error::other(format!("ffmpeg exited with {status}")).into())
                } else {
                    Err(io::Error::other(format!("ffmpeg exited with {status}: {tail}")).into())
                };
            }
            None => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = std::fs::remove_file(&temp);

                return Ok(());
            }
        }
    }
}

fn drain_stderr(mut stderr: ChildStderr) -> (mpsc::Receiver<Vec<u8>>, thread::JoinHandle<()>) {
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

#[cfg(test)]
mod tests {
    use super::Video;
    use crate::color;
    use crate::{Image, Scale, Viewport};

    fn frame(width: u16, height: u16, value: u8) -> Image<color::Linear> {
        let viewport =
            Viewport::from_logical(f32::from(width), f32::from(height), Scale::default()).unwrap();
        let rgba = [value, value, value, 255].repeat((width as usize) * (height as usize));

        Image::<color::Linear>::from_buffer(
            viewport,
            color::Buffer::from_rgba::<color::Straight>(rgba.as_slice()),
        )
        .unwrap()
    }

    fn dir(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("pikke-video-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
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

    fn dimensions(path: &std::path::Path) -> (u32, u32) {
        let out = std::process::Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-show_entries",
                "stream=width,height",
                "-of",
                "default=nw=1:nk=1",
            ])
            .arg(path)
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&out.stdout);
        let values: Vec<u32> = out
            .trim()
            .lines()
            .map(|line| line.parse().unwrap())
            .collect();

        (values[0], values[1])
    }

    #[test]
    fn the_video_encodes_n_frames() {
        let dir = dir("frames");
        let output = dir.join("frames.mp4");

        let mut video = Video::new(&output, 1.0);

        for value in 0..3u8 {
            video
                .push(32, 24, move || frame(32, 24, value * 30).to_rgba8())
                .unwrap();
        }

        assert_eq!(video.finish().unwrap(), 3);

        assert!(output.exists());
        assert_eq!(packets(&output), 3);
        assert!(!dir.join("frames.mp4.tmp").exists());
    }

    #[test]
    fn the_video_applies_the_scale_video_filter() {
        let dir = dir("scale_video");
        let output = dir.join("scaled.mp4");

        let mut video = Video::new(&output, 0.5);

        for value in 0..3u8 {
            video
                .push(32, 24, move || frame(32, 24, value * 30).to_rgba8())
                .unwrap();
        }

        assert_eq!(video.finish().unwrap(), 3);

        assert!(output.exists());
        assert_eq!(dimensions(&output), (16, 12));
    }

    #[test]
    fn the_video_rejects_odd_dimensions() {
        let mut video = Video::new(dir("odd").join("odd.mp4"), 1.0);

        video.push(32, 24, || frame(32, 24, 0).to_rgba8()).unwrap();

        let error = video
            .push(31, 24, || frame(31, 24, 0).to_rgba8())
            .unwrap_err()
            .to_string();

        assert!(error.contains("even dimensions"), "{error}");
    }

    #[test]
    fn the_video_surfaces_a_ffmpeg_failure() {
        let output = dir("missing").join("missing-dir").join("missing.mp4");

        let mut video = Video::new(&output, 1.0);

        for value in 0..3u8 {
            video
                .push(32, 24, move || frame(32, 24, value * 30).to_rgba8())
                .unwrap();
        }

        let error = video.finish().unwrap_err().to_string();

        assert!(error.contains("ffmpeg exited"), "{error}");
        assert!(!output.exists());
    }

    #[test]
    fn a_still_finishes_with_its_frame_count() {
        let dir = dir("still");

        let mut video = Video::new(dir.join("still.mp4"), 1.0);

        video.push(32, 24, || frame(32, 24, 0).to_rgba8()).unwrap();

        assert_eq!(video.finish().unwrap(), 1);
        assert!(!dir.join("still.mp4").exists());
    }
}

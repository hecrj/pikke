#[allow(dead_code)]
mod color;
mod engine;
mod error;
mod font;
mod image;
mod lua;
mod path;
mod rectangle;
mod scale;
mod stream;
mod text;
mod video;
mod viewport;

use std::collections::BTreeMap;
use std::io;
use std::process::ExitCode;
use std::sync::mpsc;

use notify::Watcher;

pub use color::Color;
pub use engine::Engine;
pub use error::Error;
pub use image::Image;
pub use path::Path;
pub use rectangle::Rectangle;
pub use scale::Scale;
pub use text::Text;
pub use video::Video;
pub use viewport::Viewport;

pub type Result<T> = std::result::Result<T, Error>;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let mut args = std::env::args();
    let _process = args.next();

    let Some(script) = args.next() else {
        println!("{}", help(None));
        std::process::exit(1);
    };

    if script == "--help" {
        println!("{}", help(None));
        std::process::exit(1);
    }

    let script = std::path::Path::new(&script);

    let scale = {
        let mut args = std::env::args();

        loop {
            let Some(arg) = args.next() else {
                break 1.0;
            };

            if arg == "--scale" {
                let value = args.next().ok_or_else(|| Error::OptionMissingValue {
                    option: "--scale".to_owned(),
                })?;

                let value: f32 = value.parse().map_err(|_| Error::OptionInvalidValue {
                    option: "--scale".to_owned(),
                    value: value.clone(),
                    note: "not a number".to_owned(),
                })?;

                if value <= 0.0 || !value.is_finite() {
                    return Err(Error::OptionInvalidValue {
                        option: "--scale".to_owned(),
                        value: value.to_string(),
                        note: "must be a positive number".to_owned(),
                    });
                }

                break value;
            }
        }
    };

    let engine = Engine::load(script, scale)?;

    let mut show_help = false;
    let mut must_watch = false;
    let mut output = None;
    let mut values = BTreeMap::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" => {
                show_help = true;
            }
            "--watch" => {
                must_watch = true;
            }
            "--scale" => {
                // Already parsed
                let _ = args.next();
            }
            "--output" => {
                output = Some(args.next().ok_or_else(|| Error::OptionMissingValue {
                    option: "--output".to_owned(),
                })?);
            }
            argument if argument.starts_with('-') => {
                let option = argument.trim_start_matches('-').to_owned();

                if !argument.starts_with("--") || !engine.arguments().contains(&option) {
                    return Err(Error::OptionUnknown {
                        option: argument.to_owned(),
                    });
                }

                let value = args.next().ok_or_else(|| Error::OptionMissingValue {
                    option: argument.to_owned(),
                })?;

                values.insert(option, value);
            }
            _ => {
                return Err(Error::ArgumentUnknown {
                    argument: arg.clone(),
                });
            }
        }
    }

    if show_help {
        println!("{}", help(Some(&engine)));
        return Ok(());
    }

    let arguments: Vec<_> = engine
        .arguments()
        .iter()
        .map(|argument| {
            (
                argument.clone(),
                values
                    .get(argument)
                    .cloned()
                    .filter(|value| !value.is_empty()),
            )
        })
        .collect();

    eprintln!(
        "run: {} ×{scale} {}",
        engine.script().display(),
        arguments
            .iter()
            .map(|(name, value)| match value {
                Some(value) => format!("{name}={value}"),
                None => format!("{name}=nil"),
            })
            .collect::<Vec<_>>()
            .join(" ")
    );

    let output = output.as_deref().map(std::path::Path::new);
    engine.run(&arguments, output)?;

    if must_watch {
        watch(script, scale, &arguments, output)?;
    }

    Ok(())
}

fn help(engine: Option<&Engine>) -> String {
    let mut help = String::from(
        "pikke — a compositing engine for Lua design scripts

Usage:
  pikke <script> [--scale <s>] [--output <path>] [options]

Arguments:
  <script>          Path to a design script (.lua)

Options:
  --scale <s>       The run's scale (default 1)
  --output <path>   The still's path; the video is the same path as .mp4 (default <script>.png)
  --help            Print this help",
    );

    if let Some(engine) = engine {
        help.push_str(&format!(
            "\n\nOptions from the script's entry function ({}):\n",
            engine.script().display(),
        ));

        if engine.arguments().is_empty() {
            help.push_str("  (none)\n");
        } else {
            for argument in engine.arguments() {
                help.push_str(&format!("  --{argument}\n"));
            }
        }
    }

    help.push_str(
        "\nAn omitted or empty option binds nil; values are strings (the\nscript converts where it needs a number).",
    );

    help
}

fn watch(
    script: &std::path::Path,
    scale: impl Into<Scale>,
    arguments: &[(String, Option<String>)],
    output: Option<&std::path::Path>,
) -> Result<()> {
    let Some(parent) = script.parent() else {
        return Err(io::Error::other("script has no parent directory").into());
    };

    let (sender, receiver) = mpsc::channel();
    let mut watcher = notify::recommended_watcher(sender)?;
    watcher.watch(parent, notify::RecursiveMode::Recursive)?;

    let scale = scale.into();
    let mut i = 0;

    loop {
        let mut rerun = false;

        for event in receiver.iter().take(1).chain(receiver.try_recv()) {
            let Ok(event) = event else {
                return Err(io::Error::other("file watcher stopped unexpectedly").into());
            };

            if !matches!(
                event.kind,
                notify::EventKind::Modify(notify::event::ModifyKind::Data(_))
            ) {
                continue;
            }

            rerun = rerun
                || event
                    .paths
                    .iter()
                    .any(|path| path.extension().is_some_and(|extension| extension == "lua"));
        }

        if !rerun {
            continue;
        }

        i += 1;

        println!("[#{i:<04}] -------------------");

        let _ = Engine::load(script, scale)
            .and_then(|engine| engine.run(arguments, output))
            .inspect_err(|error| eprintln!("error running {}: {error}", script.display()));
    }
}

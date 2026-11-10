#[allow(dead_code)]
mod color;
mod engine;
mod error;
mod font;
mod image;
mod rectangle;
mod scale;
mod stream;
mod text;
mod viewport;

use std::collections::BTreeMap;

use anywho::anywho;

pub use color::Color;
pub use engine::Engine;
pub use error::Error;
pub use image::Image;
pub use rectangle::Rectangle;
pub use scale::Scale;
pub use viewport::Viewport;

pub type Result<T> = std::result::Result<T, Error>;

fn main() -> anywho::Result<()> {
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

    let scale = {
        let mut args = std::env::args();

        loop {
            let Some(arg) = args.next() else {
                break 1.0;
            };

            if arg == "--scale" {
                let value = args
                    .next()
                    .ok_or_else(|| anywho!("--scale requires a value"))?;

                let value: f32 = value
                    .parse()
                    .map_err(|_| anywho!("invalid --scale value: {value}"))?;

                if value <= 0.0 || !value.is_finite() {
                    return Err(anywho!(
                        "invalid --scale value: {value} (must be a positive number)"
                    ));
                }

                break value;
            }
        }
    };

    let engine = Engine::load(script, scale)?;

    let mut show_help = false;
    let mut values = BTreeMap::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" => {
                show_help = true;
            }
            "--scale" => {
                // Already parsed
                let _ = args.next();
            }
            argument if argument.starts_with('-') => {
                let option = argument.trim_start_matches('-').to_owned();

                if !argument.starts_with("--") || !engine.arguments().contains(&option) {
                    return Err(anywho!("invalid option: {argument}"));
                }

                let value = args
                    .next()
                    .ok_or_else(|| anywho!("{argument} requires a value"))?;

                values.insert(option, value);
            }
            _ => {
                return Err(anywho!("invalid argument: {arg}"));
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

    engine.run(&arguments)
}

fn help(engine: Option<&Engine>) -> String {
    let mut help = String::from(
        "pikke — a compositing engine for Lua design scripts

Usage:
  pikke <script> [--scale <s>] [options]

Arguments:
  <script>          Path to a design script (.lua)

Options:
  --scale <s>       The run's scale (default 1)
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

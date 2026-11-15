use std::collections::BTreeMap;
use std::io;
use std::process::ExitCode;
use std::sync::mpsc;

use notify::Watcher;

use pikke::{Engine, Error, Result, Scale};

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
    let mut stdin = false;
    let mut must_watch = false;
    let mut output = None;
    let mut values = BTreeMap::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" => {
                show_help = true;
            }
            "--stdin" => {
                stdin = true;
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

    if stdin {
        let arguments = engine.arguments();
        return batch(script, scale, arguments, io::stdin());
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
        return watch(script, scale, &arguments, output);
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
  --stdin           Read jobs from stdin, one JSON object per line
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
        "\nAn omitted or empty option binds nil.\n\n\
        A --stdin job is a JSON object per line: its keys bind the\n\
        options as strings, numbers, or null.",
    );

    help
}

fn batch(
    script: &std::path::Path,
    scale: f32,
    arguments: &[String],
    input: impl io::Read + Send,
) -> Result<()> {
    use std::cell::OnceCell;
    use std::io::BufRead;
    use std::path::PathBuf;
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex};
    use std::thread_local;

    thread_local! {
        static ENGINE: OnceCell<Result<Engine>> = const { OnceCell::new() };
    }

    let reader = io::BufReader::new(input);
    let mut lines = reader.lines();
    let arguments: Arc<[String]> = Arc::from(arguments);
    let errors = Arc::new(Mutex::new(Vec::new()));

    rayon::scope(|scope| {
        let max_in_flight = 4 * rayon::current_num_threads();
        let (sender, permits) = mpsc::sync_channel(max_in_flight);

        for _ in 0..max_in_flight {
            let _ = sender.send(());
        }

        while let Some(job) = lines.next().transpose()? {
            let job = job.trim();

            if job.is_empty() {
                continue;
            }

            let _ = permits.recv();
            let script = script.to_owned();
            let job = job.to_owned();
            let arguments = arguments.clone();
            let errors = errors.clone();
            let sender = sender.clone();

            fn work(script: PathBuf, arguments: &[String], scale: f32, job: &str) -> Result<()> {
                let mut values: serde_json::Map<String, serde_json::Value> =
                    serde_json::from_str(job)?;

                for key in values.keys() {
                    if key != "output" && !arguments.contains(key) {
                        return Err(Error::OptionUnknown {
                            option: key.clone(),
                        });
                    }
                }

                let output = values
                    .remove("output")
                    .as_ref()
                    .and_then(serde_json::Value::as_str)
                    .map(std::path::PathBuf::from)
                    .ok_or_else(|| Error::NoBatchOutput(job.to_owned()))?;

                let arguments: Vec<_> = values
                    .into_iter()
                    .map(|(key, value)| argument(&key, value))
                    .collect::<Result<_>>()?;

                ENGINE.with(|cell| {
                    let engine = cell
                        .get_or_init(|| Engine::load(script, scale))
                        .as_ref()
                        .map_err(Error::clone)?;

                    engine.run(&arguments, Some(&output))
                })
            }

            scope.spawn(move |_scope| {
                if let Err(error) = work(script, &arguments, scale, &job) {
                    errors
                        .lock()
                        .expect("lock errors")
                        .push((job.clone(), error));
                }

                let _ = sender.send(());
            });
        }

        Ok::<_, Error>(())
    })?;

    let mut errors = errors.lock().expect("lock errors");

    if !errors.is_empty() {
        let errors = std::mem::take(&mut *errors);

        return Err(Error::BatchFailed(
            errors
                .into_iter()
                .map(|(job, error)| format!("Job failed:\n{job}\nError: {error}"))
                .collect::<Vec<_>>()
                .join("\n\n"),
        ));
    }

    Ok(())
}

fn argument(key: &str, value: serde_json::Value) -> Result<(String, Option<String>)> {
    let value = match value {
        serde_json::Value::String(value) => Some(value),
        serde_json::Value::Number(value) => Some(value.to_string()),
        serde_json::Value::Null => None,
        value => {
            return Err(Error::OptionInvalidValue {
                option: key.to_owned(),
                value: value.to_string(),
                note: "must be a string, number, or null".to_owned(),
            });
        }
    };

    Ok((key.to_owned(), value))
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

#[cfg(test)]
mod tests {
    use super::argument;
    use pikke::Error;

    #[test]
    fn the_argument_binds_strings_numbers_and_null() {
        assert_eq!(
            argument("tint", serde_json::Value::String("1".to_owned())).unwrap(),
            ("tint".to_owned(), Some("1".to_owned()))
        );
        assert_eq!(
            argument("size", serde_json::Value::Number(2.into())).unwrap(),
            ("size".to_owned(), Some("2".to_owned()))
        );
        assert_eq!(
            argument("tint", serde_json::Value::Null).unwrap(),
            ("tint".to_owned(), None)
        );
    }

    #[test]
    fn the_argument_rejects_other_value_types() {
        let error = argument("tint", serde_json::Value::Bool(true)).unwrap_err();

        assert!(matches!(error, Error::OptionInvalidValue { .. }), "{error}");
    }
}

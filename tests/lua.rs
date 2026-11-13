use std::fs;
use std::path::Path;

use pikke::{Engine, Result};

#[test]
fn run() -> Result<()> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let scripts_dir = manifest_dir.join("tests/lua");
    let output_dir = manifest_dir.join("target/test-lua");
    fs::create_dir_all(&output_dir)?;

    let mut scripts: Vec<_> = fs::read_dir(&scripts_dir)?
        .filter_map(|entry| {
            let path = entry.ok()?.path();

            if path.is_file() && path.extension().is_some_and(|extension| extension == "lua") {
                Some(path)
            } else {
                None
            }
        })
        .collect();

    scripts.sort();

    if scripts.is_empty() {
        return Err(std::io::Error::other(format!("no test scripts in {scripts_dir:?}")).into());
    }

    let mut failures = Vec::new();

    for script in scripts {
        let Some((name, stem)) = script.file_name().zip(script.file_stem()) else {
            continue;
        };

        let output = output_dir.join(format!("{}.png", stem.display()));
        let result = Engine::load(&script, 1.0).and_then(|engine| engine.run(&[], Some(&output)));

        match result {
            Ok(()) => println!("{}: ok", name.display()),
            Err(error) => failures.push((name.to_owned(), error.to_string())),
        }
    }

    if !failures.is_empty() {
        let report = failures
            .iter()
            .map(|(name, error)| format!("{}: {error}", name.display()))
            .collect::<Vec<_>>()
            .join("\n");

        panic!("{report}")
    }

    Ok(())
}

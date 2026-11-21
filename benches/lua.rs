use std::fs;
use std::path::Path;

use criterion::{Criterion, criterion_group, criterion_main};

use pikke::{Compression, Engine};

fn lua_scripts(criterion: &mut Criterion) {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let scripts_dir = manifest_dir.join("benches/lua");
    let output_dir = manifest_dir.join("target/bench-lua");

    fs::create_dir_all(&output_dir).expect("create the output directory");

    let mut scripts: Vec<_> = fs::read_dir(&scripts_dir)
        .expect("read the scripts directory")
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
        panic!("no benchmark scripts in {scripts_dir:?}");
    }

    let mut group = criterion.benchmark_group("lua");

    for script in scripts {
        let Some(stem) = script.file_stem() else {
            continue;
        };

        let output = output_dir.join(format!("{}.png", stem.display()));

        let engine = Engine::load(&script, 1.0).expect("the benchmark script failed to load");

        group.bench_function(stem.to_string_lossy().into_owned(), move |benchmark| {
            benchmark.iter(|| {
                engine
                    .run(&[], Some(&output), Compression::default())
                    .expect("the benchmark script failed")
            });
        });
    }

    group.finish();
}

criterion_group!(benches, lua_scripts);
criterion_main!(benches);

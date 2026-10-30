//! Randomized robustness test of the `.rzxl` reader and the document load
//! that follows it (a small mutation fuzzer; `cargo fuzz` needs a nightly
//! toolchain and the project builds on stable).
//!
//! Seeds are the format candidate fixtures in
//! `rizxel-io/tests/fixtures/rzxl_0_10/`. Each case mutates a seed (the JSON
//! body as a tree, the JSON text, or the raw file), loads it, and when the
//! load succeeds, uses the document the way the editor does after opening a
//! file: meshes, palette, layer tree, export scene, and saving it again. Any
//! panic fails the test; a clean error is fine.
//!
//! CI runs a few hundred cases. For a longer run:
//!
//! ```text
//! RIZXEL_FUZZ_SECS=600 cargo test --release -p rizxel-app --test rzxl_fuzz -- --nocapture
//! ```
//!
//! `RIZXEL_FUZZ_SEED` picks the random seed (printed on failure).

use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use rizxel_app::Document;
use serde_json::{Number, Value};

/// xorshift64*: small, deterministic, good enough for picking mutations.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
}

fn seeds() -> Vec<(String, Vec<u8>)> {
    let dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../rizxel-io/tests/fixtures/rzxl_0_10");
    let mut seeds: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "rzxl"))
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            (name, std::fs::read(&p).unwrap())
        })
        .collect();
    seeds.sort();
    assert!(seeds.len() >= 6, "fixtures missing in {}", dir.display());
    seeds
}

/// JSON body of a `.rzxl` file.
fn body(file: &[u8]) -> Vec<u8> {
    let mut json = Vec::new();
    zstd::Decoder::new(&file[12..])
        .unwrap()
        .read_to_end(&mut json)
        .unwrap();
    json
}

fn wrap(version: u32, json: &[u8]) -> Vec<u8> {
    let mut out = b"RZXL".to_vec();
    out.extend(version.to_le_bytes());
    out.extend((json.len() as u32).to_le_bytes());
    out.extend(zstd::encode_all(json, 1).unwrap());
    out
}

/// Values that tend to break range checks and arithmetic.
fn hostile_value(rng: &mut Rng, like: &Value) -> Value {
    let ints: [i64; 12] = [
        0,
        1,
        -1,
        2,
        255,
        256,
        1024,
        65_535,
        16_777_216,
        i32::MAX as i64,
        u32::MAX as i64,
        i64::MIN,
    ];
    let floats = [0.5, -0.5, 1e-30, 1e30, f64::MAX, -1e9, 3.0, 100.0];
    match rng.below(8) {
        0 | 1 => Value::from(ints[rng.below(ints.len())]),
        2 => Value::from(u64::MAX),
        3 => Value::Number(Number::from_f64(floats[rng.below(floats.len())]).unwrap()),
        4 => match like {
            // Nudge a number instead of replacing it.
            Value::Number(n) => {
                let v = n.as_f64().unwrap_or(0.0);
                let delta = [1.0, -1.0, 0.5, 10.0, -100.0][rng.below(5)];
                Number::from_f64(v + delta).map_or(Value::Null, Value::Number)
            }
            _ => Value::Bool(rng.chance(50)),
        },
        5 => Value::String(
            [
                "",
                "x",
                "voxels",
                "group",
                "clone",
                "shape",
                "sphere",
                "xy",
                "\u{1F600}",
            ][rng.below(9)]
            .into(),
        ),
        6 => Value::String("a".repeat(rng.below(600))),
        _ => [
            Value::Null,
            Value::Array(Vec::new()),
            Value::Object(Default::default()),
        ][rng.below(3)]
        .clone(),
    }
}

/// Number of nodes in the tree (for picking one uniformly).
fn count(v: &Value) -> usize {
    1 + match v {
        Value::Array(a) => a.iter().map(count).sum(),
        Value::Object(o) => o.values().map(count).sum(),
        _ => 0,
    }
}

/// The `n`-th node in pre-order (`n < count(v)`).
fn nth(v: &mut Value, mut n: usize) -> &mut Value {
    if n == 0 {
        return v;
    }
    n -= 1;
    let children: Vec<&mut Value> = match v {
        Value::Array(a) => a.iter_mut().collect(),
        Value::Object(o) => o.values_mut().collect(),
        _ => unreachable!("leaf has no node {n}"),
    };
    for child in children {
        let c = count(child);
        if n < c {
            return nth(child, n);
        }
        n -= c;
    }
    unreachable!("node index past the end")
}

fn pick<'a>(v: &'a mut Value, rng: &mut Rng) -> &'a mut Value {
    let n = rng.below(count(v));
    nth(v, n)
}

fn mutate_tree(root: &mut Value, rng: &mut Rng) {
    let donor = pick(root, rng).clone();
    let node = pick(root, rng);
    match node {
        Value::Object(o) if !o.is_empty() && rng.chance(40) => {
            let key = o.keys().nth(rng.below(o.len())).unwrap().clone();
            o.remove(&key);
        }
        Value::Array(a) if !a.is_empty() && rng.chance(50) => {
            let i = rng.below(a.len());
            match rng.below(4) {
                0 => {
                    a.remove(i);
                }
                1 => {
                    let copy = a[i].clone();
                    a.insert(rng.below(a.len() + 1), copy);
                }
                2 => {
                    let j = rng.below(a.len());
                    a.swap(i, j);
                }
                _ => a.truncate(i),
            }
        }
        _ if rng.chance(15) => *node = donor,
        other => *other = hostile_value(rng, other),
    }
}

fn mutate(seed: &[u8], rng: &mut Rng) -> Vec<u8> {
    let json = body(seed);
    match rng.below(10) {
        // Raw file: flips, truncation.
        0 => {
            let mut file = seed.to_vec();
            if rng.chance(50) {
                file.truncate(rng.below(file.len()));
            } else {
                for _ in 0..1 + rng.below(8) {
                    let i = rng.below(file.len());
                    file[i] ^= 1 << rng.below(8);
                }
            }
            file
        }
        // JSON text: byte changes, still wrapped in a valid container.
        1 => {
            let mut text = json;
            for _ in 0..1 + rng.below(4) {
                let i = rng.below(text.len());
                text[i] = b"0123456789-.,:[]{}\"e "[rng.below(21)];
            }
            wrap(5, &text)
        }
        // JSON tree, any version the reader accepts.
        _ => {
            let mut tree: Value = serde_json::from_slice(&json).unwrap();
            for _ in 0..1 + rng.below(4) {
                mutate_tree(&mut tree, rng);
            }
            let version = if rng.chance(80) {
                5
            } else {
                1 + rng.below(5) as u32
            };
            wrap(version, &serde_json::to_vec(&tree).unwrap())
        }
    }
}

/// Loads `file` and uses the document like the editor does after opening.
fn exercise(file: &[u8]) {
    let Ok(project) = rizxel_io::read_rzxl(file) else {
        return;
    };
    let Ok(mut doc) = Document::from_project(project, None) else {
        return;
    };
    let _ = doc.view_meshes(false);
    let _ = doc.view_meshes(true);
    let _ = doc.visible_voxel_count();
    let _ = doc.occupied_bounds();
    let _ = doc.selected_rgba();
    let _ = doc.active_layer();
    let _ = doc.used_colors();
    let _ = doc.data.tree();
    let _ = doc.title();
    let _ = doc.export_scene();
    for i in 0..doc.data.layers.len() {
        let _ = doc.layer_geometry(i);
        let _ = doc.layer_material(i);
        let _ = doc.view_alpha(i, true);
    }
    // A loaded document must save again, and load back the same.
    let project = doc.to_project();
    let mut out = Vec::new();
    rizxel_io::write_rzxl(&mut out, &project)
        .unwrap_or_else(|e| panic!("a loaded project failed to save: {e}"));
    let back = rizxel_io::read_rzxl(out.as_slice()).unwrap();
    assert_eq!(back, project);
    doc.commit_edit();
}

#[test]
fn fixtures_open_as_documents_and_save_unchanged() {
    for (name, bytes) in seeds() {
        let project = rizxel_io::read_rzxl(bytes.as_slice()).unwrap();
        let doc =
            Document::from_project(project.clone(), None).unwrap_or_else(|e| panic!("{name}: {e}"));
        let mut saved = doc.to_project();
        saved.generator = project.generator.clone();
        assert_eq!(saved, project, "{name}");
        exercise(&bytes);
    }
}

#[test]
fn out_of_range_active_layer_and_color_are_clamped() {
    let (_, bytes) = &seeds()[0];
    let mut project = rizxel_io::read_rzxl(bytes.as_slice()).unwrap();
    project.active_layer = 99;
    project.selected_color = 0;
    let doc = Document::from_project(project.clone(), None).unwrap();
    assert_eq!(doc.active_layer, doc.data.layers.len() - 1);
    assert_eq!(doc.selected_color, 1, "slot 0 is never selected");
    project.selected_color = 10_000;
    let doc = Document::from_project(project, None).unwrap();
    assert_eq!(doc.selected_color, doc.data.palette.len() - 1);
}

#[test]
fn mutated_project_files_never_panic() {
    let seeds = seeds();
    let secs: u64 = std::env::var("RIZXEL_FUZZ_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let start_seed: u64 = std::env::var("RIZXEL_FUZZ_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x5EED_0010);
    let deadline = Instant::now() + Duration::from_secs(secs);
    let min_cases = 400;
    let mut case = 0u64;
    while case < min_cases || Instant::now() < deadline {
        let seed = start_seed.wrapping_add(case);
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let (name, bytes) = &seeds[rng.below(seeds.len())];
        let input = mutate(bytes, &mut rng);
        let result = std::panic::catch_unwind(|| exercise(&input));
        if result.is_err() {
            let path = std::env::temp_dir().join(format!("rizxel_fuzz_{seed}.rzxl"));
            std::fs::write(&path, &input).unwrap();
            panic!(
                "case {case} (RIZXEL_FUZZ_SEED={seed}, seed file {name}) panicked; input saved to {}",
                path.display()
            );
        }
        case += 1;
    }
    eprintln!("{case} cases");
}

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

use ivsr_core::{CancelToken, ParamValues};

use super::*;

/// Lays out `<root>/bin/realesrgan-ncnn-vulkan` (a shell stand-in that
/// records its arguments, prints progress and copies input to output) plus
/// a `models/` directory with weights for `realesrgan-x4plus`.
#[cfg(unix)]
fn fake_install(root: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let bin_dir = root.join("bin");
    fs::create_dir_all(bin_dir.join("models")).unwrap();
    for f in ["realesrgan-x4plus.param", "realesrgan-x4plus.bin"] {
        fs::write(bin_dir.join("models").join(f), b"").unwrap();
    }
    let script = bin_dir.join(BINARY);
    fs::write(
        &script,
        r#"#!/bin/sh
echo "$@" > "$(dirname "$0")/args.txt"
while [ $# -gt 0 ]; do case "$1" in -i) in="$2"; shift;; -o) out="$2"; shift;; esac; shift; done
echo "[0 Fake GPU]  queueC=0[1]" >&2
echo "0.00%" >&2
echo "50.00%" >&2
if [ -d "$in" ]; then for f in "$in"/*; do cp "$f" "$out/$(basename "$f")"; done; else cp "$in" "$out"; fi
"#,
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    bin_dir
}

fn engine_at(install: &Path) -> RealEsrgan {
    RealEsrgan::new(RealEsrganConfig { install_dir: Some(install.to_path_buf()), ..Default::default() })
}

fn params(engine: &RealEsrgan, pairs: &[&str]) -> ParamValues {
    let parsed = ParamValues::parse_pairs(&engine.params(), pairs).unwrap();
    ParamValues::resolve(&engine.params(), &parsed).unwrap()
}

#[test]
fn percent_lines_parse_and_noise_does_not() {
    assert_eq!(parse_percent("43.75%"), Some(43.75));
    assert_eq!(parse_percent("[0 Apple M4 Pro]  queueC=0[1]"), None);
}

#[test]
fn missing_binary_reports_install_hint() {
    let tmp = tempfile::tempdir().unwrap();
    let engine = RealEsrgan::new(RealEsrganConfig {
        binary: Some(tmp.path().join("nope")),
        ..Default::default()
    });
    assert!(matches!(engine.status(), ToolStatus::Missing { .. }));
    assert!(engine.models().is_empty());
}

#[cfg(unix)]
#[test]
fn file_mode_passes_model_scale_and_params_and_reports_progress() {
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = fake_install(tmp.path());
    let engine = engine_at(tmp.path());
    assert!(engine.status().is_ready(), "{:?}", engine.status());

    let input = tmp.path().join("in.png");
    let output = tmp.path().join("out.png");
    fs::write(&input, b"pixels").unwrap();
    let values = params(&engine, &["tile=64", "tta=true"]);
    let seen = RefCell::new(Vec::new());
    let progress = |f: f64| seen.borrow_mut().push(f);
    let log = |_: LogLevel, _: &str| {};
    let task = UpscaleTask {
        input: &input,
        output: &output,
        mode: TaskMode::File,
        model: "realesrgan-x4plus",
        scale: 4,
        params: &values,
    };
    engine.upscale(&task, &TaskContext { cancel: &CancelToken::new(), progress: &progress, log: &log }).unwrap();

    assert_eq!(fs::read(&output).unwrap(), b"pixels");
    let args = fs::read_to_string(bin_dir.join("args.txt")).unwrap();
    for expected in ["-n realesrgan-x4plus", "-s 4", "-t 64", "-x", "-f png"] {
        assert!(args.contains(expected), "missing `{expected}` in `{args}`");
    }
    assert!(!args.contains("-g"), "auto GPU must not pass -g: {args}");
    assert_eq!(*seen.borrow(), vec![0.0, 0.5, 1.0]);
}

#[cfg(unix)]
#[test]
fn directory_mode_creates_output_dir_and_upscales_every_frame() {
    let tmp = tempfile::tempdir().unwrap();
    fake_install(tmp.path());
    let engine = engine_at(tmp.path());
    let input = tmp.path().join("frames");
    let output = tmp.path().join("upscaled");
    fs::create_dir_all(&input).unwrap();
    for i in 1..=3 {
        fs::write(input.join(format!("{i:08}.png")), format!("f{i}")).unwrap();
    }
    let values = params(&engine, &[]);
    let progress = |_: f64| {};
    let log = |_: LogLevel, _: &str| {};
    let task = UpscaleTask {
        input: &input,
        output: &output,
        mode: TaskMode::Directory { count: 3 },
        model: "realesrgan-x4plus",
        scale: 4,
        params: &values,
    };
    engine.upscale(&task, &TaskContext { cancel: &CancelToken::new(), progress: &progress, log: &log }).unwrap();
    assert_eq!(fs::read_to_string(output.join("00000003.png")).unwrap(), "f3");
}

#[cfg(unix)]
#[test]
fn unknown_model_or_scale_is_rejected_without_running_the_binary() {
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = fake_install(tmp.path());
    let engine = engine_at(tmp.path());
    let input = tmp.path().join("in.png");
    fs::write(&input, b"x").unwrap();
    let output = tmp.path().join("out.png");
    let values = params(&engine, &[]);
    let progress = |_: f64| {};
    let log = |_: LogLevel, _: &str| {};
    let ctx = TaskContext { cancel: &CancelToken::new(), progress: &progress, log: &log };

    for (model, scale) in [("4x-UltraSharp", 4), ("realesrgan-x4plus", 2)] {
        let task = UpscaleTask { input: &input, output: &output, mode: TaskMode::File, model, scale, params: &values };
        assert!(engine.upscale(&task, &ctx).is_err(), "{model} x{scale} should be rejected");
    }
    assert!(!bin_dir.join("args.txt").exists(), "binary must not be spawned");
}

#[cfg(unix)]
#[test]
fn tile_below_minimum_is_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    fake_install(tmp.path());
    let engine = engine_at(tmp.path());
    let input = tmp.path().join("in.png");
    fs::write(&input, b"x").unwrap();
    let output = tmp.path().join("out.png");
    let values = params(&engine, &["tile=16"]);
    let progress = |_: f64| {};
    let log = |_: LogLevel, _: &str| {};
    let task = UpscaleTask {
        input: &input,
        output: &output,
        mode: TaskMode::File,
        model: "realesrgan-x4plus",
        scale: 4,
        params: &values,
    };
    let err = engine
        .upscale(&task, &TaskContext { cancel: &CancelToken::new(), progress: &progress, log: &log })
        .unwrap_err();
    assert!(matches!(err, Error::InvalidParam { ref key, .. } if key == "tile"), "{err}");
}

/// Runs the real binary. `IVSR_REALESRGAN_BIN=/path/to/realesrgan-ncnn-vulkan
/// IVSR_TEST_IMAGE=/path/to/small.png cargo test -- --ignored real_binary`
#[test]
#[ignore = "needs a real realesrgan-ncnn-vulkan and a GPU"]
fn real_binary_upscales_an_image() {
    let binary = PathBuf::from(std::env::var("IVSR_REALESRGAN_BIN").expect("IVSR_REALESRGAN_BIN"));
    let image = PathBuf::from(std::env::var("IVSR_TEST_IMAGE").expect("IVSR_TEST_IMAGE"));
    let engine = RealEsrgan::new(RealEsrganConfig { binary: Some(binary), ..Default::default() });
    assert!(engine.status().is_ready(), "{:?}", engine.status());
    let tmp = tempfile::tempdir().unwrap();
    let output = tmp.path().join("out.png");
    let values = params(&engine, &[]);
    let progress = |_: f64| {};
    let log = |_: LogLevel, _: &str| {};
    let task = UpscaleTask {
        input: &image,
        output: &output,
        mode: TaskMode::File,
        model: "realesrgan-x4plus",
        scale: 4,
        params: &values,
    };
    engine.upscale(&task, &TaskContext { cancel: &CancelToken::new(), progress: &progress, log: &log }).unwrap();
    assert!(fs::metadata(&output).unwrap().len() > 0);
}

fn manifest(id: &str, scales: Vec<u32>) -> ModelManifest {
    let file = |role: &str| ivsr_core::ModelFile { role: role.into(), url: String::new(), size: 0, sha256: "ab".into() };
    ModelManifest {
        id: id.into(),
        engine: ENGINE_ID.into(),
        name: id.into(),
        description: "custom".into(),
        version: String::new(),
        scales,
        tags: vec![],
        architecture: Some("compact".into()),
        license: None,
        author: None,
        homepage: None,
        files: vec![file("param"), file("bin")],
        bundled: false,
        baseline: vec![],
        origin: ivsr_core::ModelOrigin::Imported,
        installed_at: None,
    }
}

#[test]
fn layout_places_files_under_a_models_leaf_and_rejects_bad_manifests() {
    let engine = RealEsrgan::new(RealEsrganConfig::default());
    let layout = engine.model_layout(&manifest("my-net", vec![2])).unwrap();
    assert_eq!(
        layout,
        vec![("param".into(), PathBuf::from("models/my-net.param")), ("bin".into(), PathBuf::from("models/my-net.bin"))]
    );
    assert!(engine.model_layout(&manifest("my-net", vec![2, 4])).is_err(), "one scale per managed model");
    assert!(engine.model_layout(&manifest("../escape", vec![4])).is_err());
    assert!(engine.model_layout(&manifest("realesr-animevideov3", vec![4])).is_err(), "reserved per-scale name");
}

#[cfg(unix)]
#[test]
fn managed_model_in_store_is_listed_and_invoked_from_its_own_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let bin_dir = fake_install(tmp.path());
    let store = tmp.path().join("store");
    let model_dir = store.join("my-net");
    fs::create_dir_all(model_dir.join("models")).unwrap();
    fs::write(model_dir.join("models/my-net.param"), b"7767517").unwrap();
    fs::write(model_dir.join("models/my-net.bin"), vec![0u8; 8]).unwrap();
    fs::write(model_dir.join("model.json"), serde_json::to_vec(&manifest("my-net", vec![2])).unwrap()).unwrap();
    // An incomplete model directory must be ignored, not offered.
    fs::create_dir_all(store.join("broken/models")).unwrap();
    fs::write(store.join("broken/model.json"), serde_json::to_vec(&manifest("broken", vec![4])).unwrap()).unwrap();

    let engine = RealEsrgan::new(RealEsrganConfig {
        install_dir: Some(tmp.path().to_path_buf()),
        store_dir: Some(store.clone()),
        ..Default::default()
    });
    let models = engine.models();
    let ids: Vec<_> = models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["realesrgan-x4plus", "my-net"]);
    let mine = &models[1];
    assert_eq!((mine.scales.clone(), mine.removable, mine.parameters), (vec![2], true, Some(4)));
    assert_eq!(mine.class, Some(ivsr_core::CostClass::Light));

    let input = tmp.path().join("in.png");
    fs::write(&input, b"px").unwrap();
    let output = tmp.path().join("out.png");
    let values = params(&engine, &[]);
    let progress = |_: f64| {};
    let log = |_: LogLevel, _: &str| {};
    let task = UpscaleTask { input: &input, output: &output, mode: TaskMode::File, model: "my-net", scale: 2, params: &values };
    engine.upscale(&task, &TaskContext { cancel: &CancelToken::new(), progress: &progress, log: &log }).unwrap();
    let args = fs::read_to_string(bin_dir.join("args.txt")).unwrap();
    assert!(args.contains(&format!("-m {}", model_dir.join("models").display())), "{args}");
    assert!(args.contains("-n my-net -s 2"), "{args}");
}

#[cfg(unix)]
#[test]
fn devices_are_parsed_from_the_binary_banner() {
    let tmp = tempfile::tempdir().unwrap();
    fake_install(tmp.path());
    let engine = engine_at(tmp.path());
    assert_eq!(engine.devices(), vec![ivsr_core::ComputeDevice { index: 0, name: "Fake GPU".into() }]);
}

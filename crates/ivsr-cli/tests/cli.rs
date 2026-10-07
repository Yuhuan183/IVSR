//! Runs the real `ivsr` binary against an isolated `IVSR_HOME` with a shell
//! stand-in for `realesrgan-ncnn-vulkan` that copies input to output.

#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const FAKE_ENGINE: &str = r#"#!/bin/sh
while [ $# -gt 0 ]; do case "$1" in -i) in="$2"; shift;; -o) out="$2"; shift;; esac; shift; done
echo "50.00%" >&2
cp "$in" "$out"
"#;

fn home_with_engine(root: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let home = root.join("home");
    let engine = home.join("data/engines/realesrgan");
    fs::create_dir_all(engine.join("models")).unwrap();
    for f in ["realesrgan-x4plus.param", "realesrgan-x4plus.bin"] {
        fs::write(engine.join("models").join(f), b"").unwrap();
    }
    let bin = engine.join("realesrgan-ncnn-vulkan");
    fs::write(&bin, FAKE_ENGINE).unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    home
}

fn ivsr(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ivsr"))
        .args(args)
        .env("IVSR_HOME", home)
        .env("IVSR_LANG", "en")
        .env_remove("IVSR_CONFIG")
        .env("TMPDIR", home.join("tmp"))
        .output()
        .unwrap()
}

fn events(out: &Output) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(&out.stdout).lines().map(|l| serde_json::from_str(l).unwrap()).collect()
}

#[test]
fn default_command_upscales_with_json_events() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home_with_engine(tmp.path());
    let input = tmp.path().join("pic.png");
    image::RgbImage::new(8, 6).save(&input).unwrap();

    let out = ivsr(&home, &["--json", input.to_str().unwrap(), "-s", "2", "-f", "jpg", "-p", "tile=64"]);

    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let events = events(&out);
    let kinds: Vec<&str> = events.iter().map(|e| e["event"].as_str().unwrap()).collect();
    assert_eq!(kinds.first(), Some(&"plan"));
    assert!(kinds.contains(&"completed"), "{kinds:?}");
    assert_eq!(kinds.last(), Some(&"summary"));
    let done = events.iter().find(|e| e["event"] == "completed").unwrap();
    assert_eq!((done["outcome"]["width"].as_u64(), done["outcome"]["height"].as_u64()), (Some(16), Some(12)));
    let written = image::open(tmp.path().join("pic_x2.jpg")).unwrap();
    assert_eq!((written.width(), written.height()), (16, 12));
}

#[test]
fn bad_engine_param_fails_before_processing() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home_with_engine(tmp.path());
    let input = tmp.path().join("pic.png");
    image::RgbImage::new(4, 4).save(&input).unwrap();

    let out = ivsr(&home, &[input.to_str().unwrap(), "-p", "tile=9999"]);

    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("tile"), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(!tmp.path().join("pic_x4.png").exists());
}

#[test]
fn config_set_is_used_by_the_next_run() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home_with_engine(tmp.path());
    assert!(ivsr(&home, &["config", "set", "output.suffix", "_hd"]).status.success());
    assert_eq!(String::from_utf8_lossy(&ivsr(&home, &["config", "get", "output.suffix"]).stdout).trim(), "_hd");
    assert!(!ivsr(&home, &["config", "set", "output.nope", "1"]).status.success());

    let input = tmp.path().join("pic.png");
    image::RgbImage::new(4, 4).save(&input).unwrap();
    let out = ivsr(&home, &["--json", "--dry-run", input.to_str().unwrap()]);
    let plan = &events(&out)[0];
    assert_eq!(plan["jobs"][0]["output"].as_str().unwrap(), tmp.path().join("pic_hd.png").to_str().unwrap());
    assert!(!tmp.path().join("pic_hd.png").exists(), "dry run writes nothing");
}

#[test]
fn update_without_source_explains_how_to_configure() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    assert!(ivsr(&home, &["config", "set", "update.repository", ""]).status.success());
    let out = ivsr(&home, &["update", "check"]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("update.repository"));
}

#[test]
fn global_flags_before_a_subcommand_still_select_it() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home_with_engine(tmp.path());
    let out = ivsr(&home, &["--json", "engines"]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let listing: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(listing[0]["info"]["id"], "realesrgan");

    let input = tmp.path().join("pic.png");
    image::RgbImage::new(4, 4).save(&input).unwrap();
    // Upscale flags before the input still mean the default command.
    let out = ivsr(&home, &["--quiet", "-s", "2", input.to_str().unwrap()]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert!(tmp.path().join("pic_x2.png").exists());
}

#[test]
fn models_import_use_and_remove_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home_with_engine(tmp.path());
    // The fake engine copies its input, so a x1 model passes the import probe.
    let param = tmp.path().join("net.param");
    let bin = tmp.path().join("net.bin");
    fs::write(&param, "7767517\n").unwrap();
    fs::write(&bin, vec![0u8; 16]).unwrap();
    let (p, b) = (param.to_str().unwrap(), bin.to_str().unwrap());

    let out = ivsr(&home, &["models", "import", "my-net", "--param", p, "--bin", b, "--scale", "1"]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let out = ivsr(&home, &["models", "import", "x4-net", "--param", p, "--bin", b, "--scale", "4"]);
    assert!(!out.status.success(), "a x4 claim must fail verification against a copying engine");

    assert!(ivsr(&home, &["models", "use", "my-net"]).status.success());
    let listing: serde_json::Value = serde_json::from_slice(&ivsr(&home, &["--json", "models"]).stdout).unwrap();
    let ids: Vec<&str> = listing["entries"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"my-net") && !ids.contains(&"x4-net"), "{ids:?}");

    let out = ivsr(&home, &["models", "remove", "my-net", "--yes"]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let model = ivsr(&home, &["config", "get", "engines.realesrgan.model"]);
    assert!(!model.status.success(), "default cleared after removal");
}

#[test]
fn traditional_chinese_output_and_help() {
    let tmp = tempfile::tempdir().unwrap();
    let home = home_with_engine(tmp.path());
    let help = ivsr(&home, &["--lang", "zh-TW", "models", "--help"]);
    assert!(String::from_utf8_lossy(&help.stdout).contains("安裝、更新、移除、匯入與測速模型"));
    let out = ivsr(&home, &["--lang", "zh-TW", "engines"]);
    assert!(String::from_utf8_lossy(&out.stdout).contains("可用"), "{}", String::from_utf8_lossy(&out.stdout));
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_ivsr"));
    cmd.args(["engines"]).env("IVSR_HOME", &home).env("IVSR_LANG", "zh-TW");
    assert!(String::from_utf8_lossy(&cmd.output().unwrap().stdout).contains("名稱"));
}

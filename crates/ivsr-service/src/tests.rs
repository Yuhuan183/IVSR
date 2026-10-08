//! End-to-end service tests: real image I/O, real planner and queue, with a
//! shell stand-in for `realesrgan-ncnn-vulkan` (it copies input to output, so
//! any size change observed comes from ivsr's own resampling).

#![cfg(unix)]

use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};

use crate::models::ImportRequest;
use std::time::Duration;

use ivsr_core::{CancelToken, ToolStatus};
use ivsr_update::http::HttpResponse;

use super::*;

const FAKE_ENGINE: &str = r#"#!/bin/sh
while [ $# -gt 0 ]; do case "$1" in -i) in="$2"; shift;; -o) out="$2"; shift;; esac; shift; done
echo "50.00%" >&2
if [ -d "$in" ]; then for f in "$in"/*; do cp "$f" "$out/$(basename "$f")"; done; else cp "$in" "$out"; fi
"#;

fn write_fake_engine(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(dir.join("models")).unwrap();
    for f in ["realesrgan-x4plus.param", "realesrgan-x4plus.bin"] {
        fs::write(dir.join("models").join(f), b"").unwrap();
    }
    let bin = dir.join("realesrgan-ncnn-vulkan");
    fs::write(&bin, FAKE_ENGINE).unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
}

fn service(root: &Path) -> Service {
    let paths = AppPaths::rooted(root);
    let config = Config { work_dir: Some(root.join("work")), ..Config::default() };
    Service::with_config(paths.clone(), paths.config_file.clone(), config)
}

fn sample_png(path: &Path, w: u32, h: u32) {
    image::RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 9) as u8, (y * 9) as u8, 77])).save(path).unwrap();
}

#[test]
fn missing_engine_fails_preparation_with_install_hint() {
    let tmp = tempfile::tempdir().unwrap();
    let svc = service(tmp.path());
    let input = tmp.path().join("a.png");
    sample_png(&input, 4, 4);
    let err = match svc.prepare(&[input], &JobRequest::default()) {
        Err(e) => e.to_string(),
        Ok(_) => panic!("expected failure"),
    };
    assert!(err.contains("ivsr engines install realesrgan"), "{err}");
}

#[test]
fn image_job_runs_end_to_end_with_resampling_and_format_change() {
    let tmp = tempfile::tempdir().unwrap();
    let svc = service(tmp.path());
    write_fake_engine(&svc.paths().engine_dir("realesrgan"));
    let input = tmp.path().join("photo.png");
    sample_png(&input, 10, 6);

    let req = JobRequest { scale: Some(2.0), image_format: Some("jpg".into()), ..Default::default() };
    let prepared = svc.prepare(&[input], &req).unwrap();
    let (planned, spec) = prepared.runnable().next().unwrap();
    assert_eq!(planned.output, tmp.path().join("photo_x2.jpg"));

    let outcome = svc.run(&spec, &prepared.engine, &ivsr_core::progress::NullReporter, &CancelToken::new()).unwrap();

    // The stand-in engine returns the input unchanged, so 10x6 -> 20x12 is ivsr's resize.
    let info = svc.registry().images().probe(&outcome.output).unwrap();
    assert_eq!((info.width, info.height, info.format.as_str()), (20, 12, "jpg"));
    assert!(fs::read_dir(tmp.path().join("work")).unwrap().next().is_none(), "work dir cleaned");
}

#[test]
fn queue_streams_lifecycle_events_and_cancels_queued_jobs() {
    let tmp = tempfile::tempdir().unwrap();
    let svc = service(tmp.path());
    write_fake_engine(&svc.paths().engine_dir("realesrgan"));
    let a = tmp.path().join("a.png");
    let b = tmp.path().join("b.png");
    sample_png(&a, 4, 4);
    sample_png(&b, 4, 4);
    let prepared = svc.prepare(&[a, b], &JobRequest::default()).unwrap();
    let specs: Vec<_> = prepared.runnable().map(|(_, s)| s).collect();
    let svc = Arc::new(svc);

    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let queue = JobQueue::new(Arc::new(move |e| tx.lock().unwrap().send(e).unwrap()));
    // The worker is busy with the first job while the second is cancelled in the queue.
    let first = queue.submit(svc.clone(), prepared.engine.clone(), specs[0].clone());
    let second = queue.submit(svc.clone(), prepared.engine.clone(), specs[1].clone());
    assert!(queue.cancel(second));

    let mut finished = Vec::new();
    while finished.len() < 2 {
        match rx.recv_timeout(Duration::from_secs(20)).expect("event") {
            JobEvent::Completed { id, outcome, .. } => {
                assert!(outcome.output.exists());
                finished.push((id, "completed"));
            }
            JobEvent::Cancelled { id } => finished.push((id, "cancelled")),
            JobEvent::Failed { id, error } => panic!("job {id} failed: {error}"),
            _ => {}
        }
    }
    finished.sort();
    assert_eq!(finished, vec![(first, "completed"), (second, "cancelled")]);
    assert_eq!(queue.pending(), 0);
    assert!(!queue.cancel(first), "finished jobs cannot be cancelled");
}

/// Serves a GitHub release listing plus a zip of the stand-in engine.
struct FakeGitHub {
    zip: Vec<u8>,
}

impl HttpClient for FakeGitHub {
    fn get(&self, url: &str, _: &[(&str, &str)]) -> ivsr_update::Result<HttpResponse> {
        let os = ivsr_update::Platform::current().os_key();
        let suffix = match os {
            "macos" => "macos",
            "linux" => "ubuntu",
            _ => "windows",
        };
        let body = if url.ends_with("/releases/tags/v0.2.5.0") {
            format!(
                r#"{{"tag_name": "v0.2.5.0", "assets": [
                    {{"name": "realesr-animevideov3.pth", "size": 1, "url": "u", "browser_download_url": "https://dl/pth"}},
                    {{"name": "realesrgan-ncnn-vulkan-20220424-{suffix}.zip", "size": {}, "url": "u",
                      "browser_download_url": "https://dl/engine.zip"}}]}}"#,
                self.zip.len()
            )
            .into_bytes()
        } else if url == "https://dl/engine.zip" {
            self.zip.clone()
        } else {
            return Ok(HttpResponse { status: 404, content_length: None, body: Box::new(Cursor::new(Vec::new())) });
        };
        Ok(HttpResponse { status: 200, content_length: Some(body.len() as u64), body: Box::new(Cursor::new(body)) })
    }
}

fn engine_zip() -> Vec<u8> {
    let mut buf = Cursor::new(Vec::new());
    let mut zip = zip_writer(&mut buf);
    // No unix permissions on purpose: the installer must mark the binary executable.
    for (name, data) in [
        ("realesrgan-ncnn-vulkan", FAKE_ENGINE.as_bytes()),
        ("models/realesrgan-x4plus.param", b"".as_slice()),
        ("models/realesrgan-x4plus.bin", b"".as_slice()),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(data).unwrap();
    }
    zip.finish().unwrap();
    buf.into_inner()
}

fn zip_writer(buf: &mut Cursor<Vec<u8>>) -> zip::ZipWriter<&mut Cursor<Vec<u8>>> {
    zip::ZipWriter::new(buf)
}

#[test]
fn engine_install_downloads_extracts_and_becomes_ready() {
    let tmp = tempfile::tempdir().unwrap();
    let svc = service(tmp.path()).with_http(Arc::new(FakeGitHub { zip: engine_zip() }));
    assert!(!svc.registry().engine("realesrgan").unwrap().status().is_ready());

    let mut phases = Vec::new();
    let report = svc
        .install_engine("realesrgan", &mut |p| phases.push(format!("{p:?}")), &CancelToken::new())
        .unwrap();

    assert!(matches!(svc.registry().engine("realesrgan").unwrap().status(), ToolStatus::Ready { .. }));
    assert_eq!(report.executable, svc.paths().engine_dir("realesrgan").join("realesrgan-ncnn-vulkan"));
    assert_eq!(report.manifest.verification, ivsr_update::Verification::SizeOnly);
    let view = svc.engine_views().into_iter().find(|v| v.info.id == "realesrgan").unwrap();
    assert_eq!(view.installed.map(|m| m.release), Some("v0.2.5.0".into()));
    assert!(phases.first().unwrap().contains("Resolving") && phases.last().unwrap().contains("Done"));
    let leftovers: Vec<PathBuf> =
        fs::read_dir(svc.paths().downloads_dir()).unwrap().map(|e| e.unwrap().path()).collect();
    assert!(leftovers.is_empty(), "downloaded archive removed: {leftovers:?}");
}

// ---- model management -------------------------------------------------------

/// Like `FAKE_ENGINE`, but when a `scaled-x<s>.png` fixture sits next to the
/// script it is written as the output, standing in for a real `s`x result.
const SCALING_ENGINE: &str = r#"#!/bin/sh
while [ $# -gt 0 ]; do case "$1" in -i) in="$2"; shift;; -o) out="$2"; shift;; -s) s="$2"; shift;; esac; shift; done
fixture="$(dirname "$0")/scaled-x$s.png"
if [ -f "$fixture" ]; then cp "$fixture" "$out"; else cp "$in" "$out"; fi
"#;

fn write_scaling_engine(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    write_fake_engine(dir);
    let bin = dir.join("realesrgan-ncnn-vulkan");
    fs::write(&bin, SCALING_ENGINE).unwrap();
    fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
    // The import probe is 16x16; a correct x4 model yields 64x64.
    sample_png(&dir.join("scaled-x4.png"), 64, 64);
}

fn sha(bytes: &[u8]) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(bytes))
}

/// Serves one remote catalogue and its files.
struct CatalogHost {
    catalog: Mutex<String>,
    files: Vec<(String, Vec<u8>)>,
}

impl HttpClient for CatalogHost {
    fn get(&self, url: &str, _: &[(&str, &str)]) -> ivsr_update::Result<HttpResponse> {
        let body = if url == "https://catalog.test/models.json" {
            self.catalog.lock().unwrap().clone().into_bytes()
        } else if let Some((_, data)) = self.files.iter().find(|(u, _)| u == url) {
            data.clone()
        } else {
            return Ok(HttpResponse { status: 404, content_length: None, body: Box::new(Cursor::new(Vec::new())) });
        };
        Ok(HttpResponse { status: 200, content_length: Some(body.len() as u64), body: Box::new(Cursor::new(body)) })
    }
}

fn catalog_json(bin_sha: &str, bin_len: usize) -> String {
    format!(
        r#"{{"models": [{{"id": "tiny-net", "engine": "realesrgan", "name": "Tiny", "description": {{"en": "Tiny", "zh-TW": "小"}},
            "scales": [4], "architecture": "compact", "license": "CC0-1.0",
            "files": [
              {{"role": "param", "url": "https://files.test/tiny.param", "size": 7, "sha256": "{}"}},
              {{"role": "bin", "url": "https://files.test/tiny.bin", "size": {bin_len}, "sha256": "{bin_sha}"}}]}},
            {{"id": "../evil", "engine": "realesrgan", "name": "x", "description": "x", "scales": [4], "files": []}}]}}"#,
        sha(b"7767517")
    )
}

fn catalog_service(root: &Path) -> (Service, Arc<CatalogHost>) {
    let bin = vec![1u8; 32];
    let host = Arc::new(CatalogHost {
        catalog: Mutex::new(catalog_json(&sha(&bin), bin.len())),
        files: vec![("https://files.test/tiny.param".into(), b"7767517".to_vec()), ("https://files.test/tiny.bin".into(), bin)],
    });
    let paths = AppPaths::rooted(root);
    let mut config = Config { work_dir: Some(root.join("work")), ..Config::default() };
    config.models.catalogs = vec!["https://catalog.test/models.json".into()];
    let svc = Service::with_config(paths.clone(), paths.config_file.clone(), config).with_http(host.clone());
    write_fake_engine(&svc.paths().engine_dir("realesrgan"));
    (svc, host)
}

fn status_of(svc: &Service, id: &str, refresh: bool) -> Option<ModelStatus> {
    let overview = svc.model_overview("realesrgan", refresh).unwrap();
    overview.entries.iter().find(|e| e.id == id).map(|e| e.status)
}

#[test]
fn remote_catalogue_model_installs_lists_and_detects_updates_by_hash() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, host) = catalog_service(tmp.path());

    let overview = svc.model_overview("realesrgan", true).unwrap();
    assert!(overview.entries.iter().all(|e| e.id != "../evil"), "unsafe ids are dropped");
    assert_eq!(status_of(&svc, "tiny-net", false), Some(ModelStatus::Available));
    assert_eq!(status_of(&svc, "realesrgan-x4plus", false), Some(ModelStatus::Bundled));

    let info = svc.install_model("realesrgan", "tiny-net", &mut |_| {}, &CancelToken::new()).unwrap();
    assert_eq!((info.scales.clone(), info.removable, info.class), (vec![4], true, Some(ivsr_core::CostClass::Light)));
    let store = svc.paths().model_store("realesrgan").join("tiny-net");
    assert_eq!(fs::read(store.join("models/tiny-net.param")).unwrap(), b"7767517");
    assert_eq!(status_of(&svc, "tiny-net", false), Some(ModelStatus::Installed));

    // The catalogue now publishes different weights for the same id.
    *host.catalog.lock().unwrap() = catalog_json(&sha(b"newer weights"), 13);
    assert_eq!(status_of(&svc, "tiny-net", true), Some(ModelStatus::UpdateAvailable));
}

#[test]
fn corrupt_download_leaves_no_model_behind() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, host) = catalog_service(tmp.path());
    *host.catalog.lock().unwrap() = catalog_json(&sha(b"something else"), 32);
    svc.model_overview("realesrgan", true).unwrap();
    let err = svc.install_model("realesrgan", "tiny-net", &mut |_| {}, &CancelToken::new()).unwrap_err();
    assert!(err.to_string().contains("checksum"), "{err}");
    let store = svc.paths().model_store("realesrgan");
    let leftovers: Vec<_> = fs::read_dir(&store).map(|d| d.flatten().map(|e| e.file_name()).collect()).unwrap_or_default();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn removing_the_default_model_clears_it_and_bundled_models_stay() {
    let tmp = tempfile::tempdir().unwrap();
    let (svc, _) = catalog_service(tmp.path());
    svc.model_overview("realesrgan", true).unwrap();
    svc.install_model("realesrgan", "tiny-net", &mut |_| {}, &CancelToken::new()).unwrap();
    let mut config = svc.config().clone();
    config.set("engines.realesrgan.model", "tiny-net").unwrap();
    let svc = svc.reconfigure(config).unwrap();

    let saved = svc.remove_model("realesrgan", "tiny-net").unwrap().expect("default was cleared");
    assert_eq!(saved.engine_config("realesrgan").model, None);
    assert_eq!(Config::load(svc.config_file()).unwrap().engine_config("realesrgan").model, None);
    assert!(svc.remove_model("realesrgan", "realesrgan-x4plus").is_err(), "bundled models are not removable");
}

#[test]
fn import_accepts_a_model_that_produces_its_declared_scale_and_rejects_others() {
    let tmp = tempfile::tempdir().unwrap();
    let svc = service(tmp.path());
    write_scaling_engine(&svc.paths().engine_dir("realesrgan"));
    let param = tmp.path().join("net.param");
    let bin = tmp.path().join("net.bin");
    fs::write(&param, "7767517\n1 1\n").unwrap();
    fs::write(&bin, vec![0u8; 64]).unwrap();
    let req = |id: &str, scale: u32| ImportRequest {
        id: id.into(),
        name: Some("My net".into()),
        description: None,
        scale,
        param: param.clone(),
        bin: bin.clone(),
        license: None,
    };

    let info = svc.import_model("realesrgan", &req("my-net", 4)).unwrap();
    assert_eq!((info.origin, info.scales.clone()), (ivsr_core::ModelOrigin::Imported, vec![4]));
    assert_eq!(status_of(&svc, "my-net", false), Some(ModelStatus::Imported));

    // The probe comes back 64x64; claiming x2 must be refused and rolled back.
    let err = svc.import_model("realesrgan", &req("wrong-scale", 2)).unwrap_err();
    assert!(err.to_string().contains("verification"), "{err}");
    assert!(!svc.paths().model_store("realesrgan").join("wrong-scale").exists());

    let not_ncnn = tmp.path().join("bad.param");
    fs::write(&not_ncnn, "hello").unwrap();
    let mut bad = req("bad", 4);
    bad.param = not_ncnn;
    assert!(svc.import_model("realesrgan", &bad).unwrap_err().to_string().contains("ncnn"));
    assert!(svc.import_model("realesrgan", &req("my-net", 4)).is_err(), "duplicate id");
}

#[test]
fn finished_jobs_are_recorded_in_history() {
    let tmp = tempfile::tempdir().unwrap();
    let svc = service(tmp.path());
    write_fake_engine(&svc.paths().engine_dir("realesrgan"));
    let input = tmp.path().join("h.png");
    sample_png(&input, 4, 4);
    let prepared = svc.prepare(std::slice::from_ref(&input), &JobRequest::default()).unwrap();
    let (_, spec) = prepared.runnable().next().unwrap();
    svc.run(&spec, &prepared.engine, &ivsr_core::progress::NullReporter, &CancelToken::new()).unwrap();
    let history = svc.history();
    assert_eq!(history.len(), 1);
    assert_eq!((history[0].input.clone(), history[0].width, history[0].source_width), (input, 16, 4));
}

fn post_on(steps: Option<Vec<ivsr_core::FilterStep>>) -> Option<ivsr_core::FilterChain> {
    Some(ivsr_core::FilterChain { enabled: true, steps })
}

fn upscale(svc: &Service, input: &Path, req: &JobRequest) -> JobOutcome {
    let prepared = svc.prepare(&[input.to_path_buf()], req).unwrap();
    let (_, spec) = prepared.runnable().next().unwrap();
    svc.run(&spec, &prepared.engine, &ivsr_core::progress::NullReporter, &CancelToken::new()).unwrap()
}

#[test]
fn post_processing_changes_output_only_when_its_switch_is_on() {
    let tmp = tempfile::tempdir().unwrap();
    let svc = service(tmp.path());
    write_fake_engine(&svc.paths().engine_dir("realesrgan"));
    let input = tmp.path().join("p.png");
    sample_png(&input, 12, 8);
    // x2 makes ivsr resample the stand-in engine's copy, so filters run at the final size.
    let req = |suffix: &str, post| JobRequest { scale: Some(2.0), suffix: Some(suffix.into()), post, ..Default::default() };

    let plain = upscale(&svc, &input, &req("_plain", None));
    let filtered = upscale(&svc, &input, &req("_post", post_on(None)));
    let switched_off = ivsr_core::FilterChain { enabled: false, steps: Some(vec![ivsr_core::FilterStep::new("saturation")]) };
    let off = upscale(&svc, &input, &req("_off", Some(switched_off)));

    assert_eq!(fs::read(&plain.output).unwrap(), fs::read(&off.output).unwrap(), "switch off = no filtering");
    let images = svc.registry().images();
    let (a, b) = (images.decode(&plain.output, None).unwrap(), images.decode(&filtered.output, None).unwrap());
    assert_eq!((b.width, b.height), (24, 16));
    assert_ne!(a.pixels, b.pixels);
}

#[test]
fn filter_chains_are_validated_when_preparing() {
    let tmp = tempfile::tempdir().unwrap();
    let svc = service(tmp.path());
    write_fake_engine(&svc.paths().engine_dir("realesrgan"));
    let input = tmp.path().join("v.png");
    sample_png(&input, 4, 4);
    let unknown = JobRequest { post: post_on(Some(vec![ivsr_core::FilterStep::new("nope")])), ..Default::default() };
    let err = svc.prepare(std::slice::from_ref(&input), &unknown).err().unwrap().to_string();
    assert!(err.contains("unknown filter `nope`"), "{err}");
    let wrong_stage = JobRequest { pre: post_on(Some(vec![ivsr_core::FilterStep::new("tone-restore")])), ..Default::default() };
    let err = svc.prepare(&[input], &wrong_stage).err().unwrap().to_string();
    assert!(err.contains("cannot run as pre-processing"), "{err}");
}

#[test]
fn video_frames_are_pre_and_post_processed_and_encoded_at_the_final_size() {
    let tmp = tempfile::tempdir().unwrap();
    let svc = service(tmp.path());
    let video = svc.registry().video().clone();
    if video.status().problem().is_some() {
        eprintln!("ffmpeg not available, skipping");
        return;
    }
    write_fake_engine(&svc.paths().engine_dir("realesrgan"));
    let input = tmp.path().join("clip.mp4");
    let status = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi", "-i", "testsrc=size=64x48:rate=10", "-t", "1"])
        .args(["-c:v", "libx264", "-pix_fmt", "yuv420p"])
        .arg(&input)
        .status()
        .unwrap();
    assert!(status.success());
    let req = |suffix: &str, on: bool| JobRequest {
        scale: Some(2.0),
        suffix: Some(suffix.into()),
        pre: on.then_some(ivsr_core::FilterChain { enabled: true, steps: None }),
        post: if on { post_on(None) } else { None },
        ..Default::default()
    };

    let filtered = upscale(&svc, &input, &req("_post", true));
    let plain = upscale(&svc, &input, &req("_plain", false));

    let info = video.probe(&filtered.output).unwrap();
    assert_eq!((info.width, info.height, info.estimated_frames()), (128, 96, 10));
    let first_frame = |path: &Path, dir: &str| {
        let dir = tmp.path().join(dir);
        fs::create_dir_all(&dir).unwrap();
        let ctx = ivsr_core::TaskContext { cancel: &CancelToken::new(), progress: &|_| {}, log: &|_, _| {} };
        video.extract_frames(path, &video.probe(path).unwrap(), &dir, &ctx).unwrap();
        svc.registry().images().decode(&dir.join("00000001.png"), None).unwrap()
    };
    assert_ne!(first_frame(&filtered.output, "a").pixels, first_frame(&plain.output, "b").pixels);
    assert!(fs::read_dir(tmp.path().join("work")).unwrap().next().is_none(), "work dir cleaned");
}

#[test]
fn standalone_filtering_finds_the_original_of_a_result_in_history() {
    let tmp = tempfile::tempdir().unwrap();
    let svc = service(tmp.path());
    write_fake_engine(&svc.paths().engine_dir("realesrgan"));
    let input = tmp.path().join("s.png");
    sample_png(&input, 6, 4);
    let result = upscale(&svc, &input, &JobRequest::default()).output;
    let clip = tmp.path().join("clip.mp4");
    fs::write(&clip, b"not decoded while planning").unwrap();

    let prepared = svc.prepare_filters(&[result.clone(), clip], &FilterJobRequest::new(ivsr_core::FilterStage::Post)).unwrap();
    let runnable: Vec<_> = prepared.runnable().collect();
    assert_eq!(runnable.len(), 1, "videos are not filtered stand-alone");
    let job = runnable[0];
    assert_eq!(job.reference.as_deref(), Some(input.as_path()));
    assert_eq!(job.planned.output, tmp.path().join("s_x4_post.png"));
    let outcome = svc.run_filters(&prepared, job).unwrap();
    // The stand-in engine copies, so the "result" is 6x4.
    assert_eq!((outcome.width, outcome.height), (6, 4));
    assert_ne!(fs::read(&outcome.output).unwrap(), fs::read(&result).unwrap());

    let preview = svc.filter_preview(&result, ivsr_core::FilterStage::Post, &prepared.steps, Some(&input)).unwrap();
    assert!(preview.starts_with(svc.preview_dir()));
    assert_eq!(svc.filter_preview(&result, ivsr_core::FilterStage::Post, &prepared.steps, Some(&input)).unwrap(), preview);
}

#[test]
fn standalone_reference_must_exist_and_a_single_file_fits_a_single_input() {
    let tmp = tempfile::tempdir().unwrap();
    let svc = service(tmp.path());
    let (a, b, original) = (tmp.path().join("a.png"), tmp.path().join("b.png"), tmp.path().join("orig.png"));
    for p in [&a, &b, &original] {
        sample_png(p, 4, 4);
    }
    let request = |reference: &Path| FilterJobRequest {
        reference: Some(reference.to_path_buf()),
        ..FilterJobRequest::new(ivsr_core::FilterStage::Post)
    };

    let err = svc.prepare_filters(std::slice::from_ref(&a), &request(&tmp.path().join("orignals"))).err().unwrap();
    assert!(err.to_string().contains("orignals"), "{err}");
    let err = svc.prepare_filters(&[a.clone(), b], &request(&original)).err().unwrap();
    assert!(err.to_string().contains("directory"), "{err}");
    let one = svc.prepare_filters(&[a], &request(&original)).unwrap();
    assert_eq!(one.jobs[0].reference.as_deref(), Some(original.as_path()));
}

/// Serves a release listing with v99.0.0 for every request.
struct ReleaseHost;

impl HttpClient for ReleaseHost {
    fn get(&self, _: &str, _: &[(&str, &str)]) -> ivsr_update::Result<HttpResponse> {
        let body = br#"[{"tag_name": "v99.0.0", "draft": false, "prerelease": false, "assets": []}]"#.to_vec();
        Ok(HttpResponse { status: 200, content_length: Some(body.len() as u64), body: Box::new(Cursor::new(body)) })
    }
}

#[test]
fn a_skipped_version_is_not_announced_by_automatic_checks_but_explicit_checks_still_see_it() {
    let tmp = tempfile::tempdir().unwrap();
    let svc = service(tmp.path()).with_http(Arc::new(ReleaseHost));
    let updates = svc.updates(Flavor::Desktop, "0.1.0");
    let latest = match updates.check().unwrap() {
        ivsr_update::UpdateCheck::Available { latest, .. } => latest,
        other => panic!("expected an update, got {other:?}"),
    };
    updates.skip(&latest).unwrap();

    let automatic = updates.reminder(updates.check().unwrap());
    assert!(matches!(automatic, ivsr_update::UpdateCheck::UpToDate { .. }), "{automatic:?}");
    assert!(matches!(updates.check().unwrap(), ivsr_update::UpdateCheck::Available { .. }));
}

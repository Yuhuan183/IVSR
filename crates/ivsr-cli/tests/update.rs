//! Self-update end to end: a copy of the real `ivsr` binary checks a local
//! stand-in for the GitHub releases API, picks its platform's archive,
//! downloads it, verifies the SHA-256 digest, unpacks it and replaces itself.
//!
//! By default the served archive holds a synthetic executable. With
//! `IVSR_UPDATE_E2E_ARCHIVE=<path>` it serves that archive instead (the release
//! workflow passes the freshly packaged one) and the replaced binary must run
//! and report `IVSR_UPDATE_E2E_VERSION`.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use sha2::{Digest, Sha256};

const EXE: &str = if cfg!(windows) { "ivsr.exe" } else { "ivsr" };

/// Serves `routes` (request path without the query -> body) on a local port
/// until the test process exits; returns the base URL.
fn serve(routes: impl Fn(&str) -> Option<Vec<u8>> + Send + 'static) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut request = String::new();
            if reader.read_line(&mut request).is_err() {
                continue;
            }
            // Drain the headers; requests carry no body.
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let target = request.split_whitespace().nth(1).unwrap_or_default();
            let path = target.split('?').next().unwrap_or_default().to_string();
            let response = match routes(&path) {
                Some(body) => [format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).into_bytes(), body].concat(),
                None => b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec(),
            };
            let _ = stream.write_all(&response);
        }
    });
    base
}

/// The served archive: its file name, bytes and the executable inside it.
struct Archive {
    name: String,
    bytes: Vec<u8>,
    executable: Vec<u8>,
}

fn synthetic_archive() -> Archive {
    // Runnable on Unix; on Windows only its bytes are compared.
    let executable = b"#!/bin/sh\necho 'ivsr 99.0.0'\n".to_vec();
    let platform = format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS);
    if cfg!(windows) {
        let mut cursor = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut cursor);
        zip.start_file(EXE, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(&executable).unwrap();
        zip.finish().unwrap();
        Archive { name: format!("ivsr-cli-99.0.0-{platform}.zip"), bytes: cursor.into_inner(), executable }
    } else {
        let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        let mut header = tar::Header::new_gnu();
        header.set_size(executable.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, EXE, executable.as_slice()).unwrap();
        let bytes = tar.into_inner().unwrap().finish().unwrap();
        Archive { name: format!("ivsr-cli-99.0.0-{platform}.tar.gz"), bytes, executable }
    }
}

/// A real release archive, unpacked and searched exactly as the updater does.
fn real_archive(path: &Path) -> Archive {
    let bytes = std::fs::read(path).unwrap();
    let name = path.file_name().unwrap().to_string_lossy().into_owned();
    let unpacked = tempfile::tempdir().unwrap();
    ivsr_update::archive::extract(path, unpacked.path()).unwrap();
    let exe = ivsr_update::archive::find_file(unpacked.path(), EXE, 2)
        .unwrap_or_else(|| panic!("{EXE} not found where the updater looks in {}", path.display()));
    Archive { name, bytes, executable: std::fs::read(exe).unwrap() }
}

fn archive() -> Archive {
    match std::env::var_os("IVSR_UPDATE_E2E_ARCHIVE") {
        Some(path) => real_archive(Path::new(&path)),
        None => synthetic_archive(),
    }
}

/// A release listing that offers `archive` as v99.0.0 with `digest`.
fn listing(base: &str, archive: &Archive, digest: &str) -> Vec<u8> {
    let release = serde_json::json!([{
        "tag_name": "v99.0.0",
        "name": "IVSR 99.0.0",
        "body": "Test release",
        "draft": false,
        "prerelease": false,
        "published_at": "2026-10-01T00:00:00Z",
        "html_url": format!("{base}/release"),
        "assets": [{
            "name": archive.name,
            "size": archive.bytes.len(),
            "url": format!("{base}/api/assets/1"),
            "browser_download_url": format!("{base}/download/{}", archive.name),
            "digest": digest,
        }],
    }]);
    serde_json::to_vec(&release).unwrap()
}

/// A copy of the built `ivsr` (never the build output itself) and a home
/// configured to update from `base`.
fn client(root: &Path, base: &str) -> (PathBuf, PathBuf) {
    let dir = root.join("client");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join(EXE);
    std::fs::copy(env!("CARGO_BIN_EXE_ivsr"), &exe).unwrap();
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let config = format!("[update]\nrepository = \"acme/ivsr\"\napi_base = \"{base}\"\nauto_check = false\n");
    std::fs::write(home.join("config.toml"), config).unwrap();
    (exe, home)
}

fn run(exe: &Path, home: &Path, args: &[&str]) -> Output {
    Command::new(exe)
        .args(args)
        .env("IVSR_HOME", home)
        .env("IVSR_LANG", "en")
        .env_remove("IVSR_CONFIG")
        .output()
        .unwrap()
}

fn last_event(out: &Output) -> serde_json::Value {
    let stdout = String::from_utf8_lossy(&out.stdout);
    let line = stdout.lines().last().unwrap_or_else(|| panic!("no output; stderr: {}", String::from_utf8_lossy(&out.stderr)));
    serde_json::from_str(line).unwrap()
}

fn digest_of(bytes: &[u8]) -> String {
    let hex: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

fn server_for(archive: &Archive, digest: String) -> String {
    let (name, bytes) = (archive.name.clone(), archive.bytes.clone());
    let listing_for = |base: &str| listing(base, archive, &digest);
    // The listing embeds the base URL, which is only known once bound.
    let slot = std::sync::Arc::new(std::sync::OnceLock::<Vec<u8>>::new());
    let served = slot.clone();
    let base = serve(move |path| match path {
        "/repos/acme/ivsr/releases" => served.get().cloned(),
        p if p == format!("/download/{name}") => Some(bytes.clone()),
        _ => None,
    });
    slot.set(listing_for(&base)).unwrap();
    base
}

#[test]
fn update_install_replaces_the_running_binary_with_the_verified_release() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = archive();
    let base = server_for(&archive, digest_of(&archive.bytes));
    let (exe, home) = client(tmp.path(), &base);

    let check = last_event(&run(&exe, &home, &["--json", "update", "check"]));
    assert_eq!(check["status"], "available", "{check}");
    assert_eq!(check["asset"]["name"], archive.name.as_str(), "picked another asset: {check}");

    let out = run(&exe, &home, &["--json", "update", "install", "--yes"]);
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let installed = last_event(&out);
    assert_eq!((installed["status"].as_str(), installed["verification"].as_str()), (Some("installed"), Some("sha256")));
    assert!(std::fs::read(&exe).unwrap() == archive.executable, "client binary was not replaced");

    // The replaced binary must run where it can: always for a real release
    // archive, and on Unix for the synthetic shell script.
    if std::env::var_os("IVSR_UPDATE_E2E_ARCHIVE").is_some() || cfg!(unix) {
        let version = run(&exe, &home, &["--version"]);
        let expected = std::env::var("IVSR_UPDATE_E2E_VERSION").unwrap_or_else(|_| "99.0.0".into());
        assert!(String::from_utf8_lossy(&version.stdout).contains(&expected), "{version:?}");
    }
}

#[test]
fn a_digest_mismatch_aborts_and_leaves_the_binary_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let archive = archive();
    let base = server_for(&archive, digest_of(b"something else"));
    let (exe, home) = client(tmp.path(), &base);
    let before = std::fs::read(&exe).unwrap();

    let out = run(&exe, &home, &["--json", "update", "install", "--yes"]);

    assert!(!out.status.success(), "a tampered download must fail");
    assert!(std::fs::read(&exe).unwrap() == before, "client binary changed despite the bad digest");
}

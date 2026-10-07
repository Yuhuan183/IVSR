//! Streaming download with size and checksum verification.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::http::HttpClient;
use crate::{Asset, Error, ReleaseSource, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verification {
    /// SHA-256 matched the digest published by the source.
    Sha256,
    /// The source published no digest; only the byte count was checked.
    SizeOnly,
}

#[derive(Debug, Clone)]
pub struct Downloaded {
    pub path: PathBuf,
    pub verification: Verification,
}

/// What a download must match.
#[derive(Debug, Clone, Default)]
pub struct Expected<'a> {
    /// Bytes; 0 means unknown.
    pub size: u64,
    /// `sha256:<hex>` or bare hex.
    pub digest: Option<&'a str>,
}

/// Downloads `asset` into `dir`, reporting `(received, total)` bytes. The file
/// only appears under its final name once size and digest have been checked.
pub fn download(
    http: &dyn HttpClient,
    source: &dyn ReleaseSource,
    asset: &Asset,
    dir: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancelled: &dyn Fn() -> bool,
) -> Result<Downloaded> {
    let (url, headers) = source.download_request(asset);
    let header_refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    let expected = Expected { size: asset.size, digest: asset.digest.as_deref() };
    download_url(http, &url, &header_refs, &asset.name, &expected, dir, progress, cancelled)
}

/// Downloads `url` into `dir/name` with the same guarantees as [`download`].
#[allow(clippy::too_many_arguments)]
pub fn download_url(
    http: &dyn HttpClient,
    url: &str,
    headers: &[(&str, &str)],
    name: &str,
    expected: &Expected<'_>,
    dir: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancelled: &dyn Fn() -> bool,
) -> Result<Downloaded> {
    fs::create_dir_all(dir).map_err(|e| Error::io(format!("create {}", dir.display()), e))?;
    // Names may come from the network: keep only the final component.
    let file_name = Path::new(name)
        .file_name()
        .filter(|n| !n.is_empty())
        .ok_or_else(|| Error::Parse(format!("invalid file name `{name}`")))?;
    let target = dir.join(file_name);
    let partial = dir.join(format!("{}.part", file_name.to_string_lossy()));

    let resp = http.get(url, headers)?;
    if !(200..300).contains(&resp.status) {
        return Err(Error::Http { status: resp.status, url: url.to_string() });
    }
    let total = resp.content_length.or((expected.size > 0).then_some(expected.size));

    let result = (|| {
        let mut file = File::create(&partial).map_err(|e| Error::io(format!("create {}", partial.display()), e))?;
        let mut body = resp.body;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut received = 0u64;
        loop {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            let n = body.read(&mut buf).map_err(|e| Error::Network(e.to_string()))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n]).map_err(|e| Error::io(format!("write {}", partial.display()), e))?;
            received += n as u64;
            progress(received, total);
        }
        file.sync_all().map_err(|e| Error::io(format!("flush {}", partial.display()), e))?;
        let file_name = name.to_string();
        if expected.size > 0 && received != expected.size {
            return Err(Error::Size { file: file_name, expected: expected.size, actual: received });
        }
        let actual = format!("{:x}", hasher.finalize());
        let wanted = expected.digest.map(|d| d.strip_prefix("sha256:").unwrap_or(d));
        match wanted {
            Some(expected) if !expected.eq_ignore_ascii_case(&actual) => {
                Err(Error::Checksum { file: file_name, expected: expected.to_string(), actual })
            }
            Some(_) => Ok(Verification::Sha256),
            None => Ok(Verification::SizeOnly),
        }
    })();

    match result {
        Ok(verification) => {
            fs::rename(&partial, &target).map_err(|e| Error::io(format!("rename to {}", target.display()), e))?;
            Ok(Downloaded { path: target, verification })
        }
        Err(e) => {
            let _ = fs::remove_file(&partial);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use crate::http::HttpResponse;
    use crate::Release;

    struct Bytes(&'static [u8]);
    impl HttpClient for Bytes {
        fn get(&self, _: &str, _: &[(&str, &str)]) -> Result<HttpResponse> {
            Ok(HttpResponse { status: 200, content_length: Some(self.0.len() as u64), body: Box::new(Cursor::new(self.0)) })
        }
    }
    struct NoSource;
    impl ReleaseSource for NoSource {
        fn describe(&self) -> String {
            "test".into()
        }
        fn releases(&self) -> Result<Vec<Release>> {
            Ok(vec![])
        }
        fn release(&self, _: &str) -> Result<Release> {
            Err(Error::NoRelease(String::new()))
        }
    }

    // sha256("hello")
    const HELLO: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

    fn asset(name: &str, size: u64, digest: Option<&str>) -> Asset {
        Asset { name: name.into(), size, download_url: "https://x/y".into(), api_url: None, digest: digest.map(String::from) }
    }

    fn fetch(a: &Asset, dir: &Path) -> Result<Downloaded> {
        download(&Bytes(b"hello"), &NoSource, a, dir, &mut |_, _| {}, &|| false)
    }

    #[test]
    fn matching_digest_is_reported_as_verified() {
        let dir = tempfile::tempdir().unwrap();
        let got = fetch(&asset("a.zip", 5, Some(&format!("sha256:{HELLO}"))), dir.path()).unwrap();
        assert_eq!(got.verification, Verification::Sha256);
        assert_eq!(fs::read(&got.path).unwrap(), b"hello");
    }

    #[test]
    fn digest_mismatch_fails_and_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let err = fetch(&asset("a.zip", 5, Some("sha256:00ff")), dir.path()).unwrap_err();
        assert!(matches!(err, Error::Checksum { .. }), "{err}");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn missing_digest_falls_back_to_size_check() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(fetch(&asset("a.zip", 5, None), dir.path()).unwrap().verification, Verification::SizeOnly);
        assert!(matches!(fetch(&asset("b.zip", 6, None), dir.path()), Err(Error::Size { .. })));
    }

    #[test]
    fn direct_url_download_accepts_bare_hex_digest() {
        let dir = tempfile::tempdir().unwrap();
        let expected = Expected { size: 5, digest: Some(HELLO) };
        let got = download_url(&Bytes(b"hello"), "https://x/f", &[], "f.bin", &expected, dir.path(), &mut |_, _| {}, &|| false)
            .unwrap();
        assert_eq!(got.verification, Verification::Sha256);
        let bad = Expected { size: 5, digest: Some("00") };
        assert!(download_url(&Bytes(b"hello"), "https://x/f", &[], "g.bin", &bad, dir.path(), &mut |_, _| {}, &|| false).is_err());
    }

    #[test]
    fn asset_name_cannot_escape_the_target_directory() {
        let dir = tempfile::tempdir().unwrap();
        let got = fetch(&asset("../../evil.sh", 5, None), dir.path()).unwrap();
        assert_eq!(got.path, dir.path().join("evil.sh"));
    }
}

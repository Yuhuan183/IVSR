//! Unpacking downloaded archives without letting entries escape the target.

use std::fs::{self, File};
use std::io;
use std::path::{Component, Path};

use crate::{Error, Result};

/// Extracts a `.zip`, `.tar.gz`/`.tgz` or `.tar` archive into `dest`. Entries
/// with absolute paths or `..` components are rejected.
pub fn extract(archive: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest).map_err(|e| Error::io(format!("create {}", dest.display()), e))?;
    let name = archive.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let open = || File::open(archive).map_err(|e| Error::io(format!("open {}", archive.display()), e));
    if name.ends_with(".zip") {
        extract_zip(open()?, dest)
    } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
        extract_tar(flate2::read::GzDecoder::new(open()?), dest)
    } else if name.ends_with(".tar") {
        extract_tar(open()?, dest)
    } else {
        Err(Error::Archive(format!("unsupported archive type: {name}")))
    }
}

fn extract_zip(file: File, dest: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(file).map_err(|e| Error::Archive(e.to_string()))?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| Error::Archive(e.to_string()))?;
        let rel = entry
            .enclosed_name()
            .ok_or_else(|| Error::Archive(format!("unsafe path in archive: {}", entry.name())))?;
        let out = dest.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| Error::io(format!("create {}", out.display()), e))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| Error::io(format!("create {}", parent.display()), e))?;
        }
        let mut file = File::create(&out).map_err(|e| Error::io(format!("create {}", out.display()), e))?;
        io::copy(&mut entry, &mut file).map_err(|e| Error::io(format!("write {}", out.display()), e))?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&out, fs::Permissions::from_mode(mode & 0o777));
        }
    }
    Ok(())
}

fn extract_tar(reader: impl io::Read, dest: &Path) -> Result<()> {
    let mut archive = tar::Archive::new(reader);
    for entry in archive.entries().map_err(|e| Error::Archive(e.to_string()))? {
        let mut entry = entry.map_err(|e| Error::Archive(e.to_string()))?;
        let path = entry.path().map_err(|e| Error::Archive(e.to_string()))?.into_owned();
        if path.components().any(|c| !matches!(c, Component::Normal(_) | Component::CurDir)) {
            return Err(Error::Archive(format!("unsafe path in archive: {}", path.display())));
        }
        entry.unpack_in(dest).map_err(|e| Error::Archive(e.to_string()))?;
    }
    Ok(())
}

/// Finds a file named `name` at most `depth` directories below `root`.
pub fn find_file(root: &Path, name: &str, depth: usize) -> Option<std::path::PathBuf> {
    let direct = root.join(name);
    if direct.is_file() {
        return Some(direct);
    }
    if depth == 0 {
        return None;
    }
    fs::read_dir(root).ok()?.flatten().filter(|e| e.path().is_dir()).find_map(|e| find_file(&e.path(), name, depth - 1))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn zip_with(path: &Path, entries: &[(&str, &[u8])]) {
        let mut w = zip::ZipWriter::new(File::create(path).unwrap());
        for (name, data) in entries {
            w.start_file(*name, zip::write::SimpleFileOptions::default().unix_permissions(0o755)).unwrap();
            w.write_all(data).unwrap();
        }
        w.finish().unwrap();
    }

    #[test]
    fn zip_extracts_nested_files_with_permissions() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("pkg.zip");
        zip_with(&archive, &[("bin/tool", b"#!/bin/sh"), ("models/a.param", b"p")]);
        let dest = tmp.path().join("out");
        extract(&archive, &dest).unwrap();
        assert_eq!(fs::read(dest.join("models/a.param")).unwrap(), b"p");
        assert_eq!(find_file(&dest, "tool", 2), Some(dest.join("bin/tool")));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(dest.join("bin/tool")).unwrap().permissions().mode() & 0o111, 0o111);
        }
    }

    #[test]
    fn zip_slip_entry_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("evil.zip");
        zip_with(&archive, &[("../escaped.txt", b"x")]);
        let dest = tmp.path().join("out");
        assert!(matches!(extract(&archive, &dest), Err(Error::Archive(_))));
        assert!(!tmp.path().join("escaped.txt").exists());
    }
}

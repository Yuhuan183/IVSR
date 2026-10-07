//! Filesystem helpers that keep partially written outputs invisible.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::IoContext;
use crate::Result;

/// Moves `src` to `dst`, replacing it. Falls back to copy-then-rename through a
/// hidden sibling file when `src` is on another filesystem, so `dst` never
/// appears half-written.
pub fn persist(src: &Path, dst: &Path) -> Result<()> {
    if let Some(parent) = dst.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).at("create directory", parent)?;
    }
    if fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    let staging = sibling_part(dst);
    let copied = fs::copy(src, &staging).at("copy to", &staging);
    if let Err(e) = copied {
        let _ = fs::remove_file(&staging);
        return Err(e);
    }
    fs::rename(&staging, dst).at("rename to", dst)?;
    let _ = fs::remove_file(src);
    Ok(())
}

fn sibling_part(dst: &Path) -> PathBuf {
    let name = dst.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    dst.with_file_name(format!(".{name}.part"))
}

/// Lower-cased extension of `path`, if any.
pub fn extension(path: &Path) -> Option<String> {
    path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persist_replaces_destination_and_removes_source() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("a.bin");
        let dst = dir.path().join("nested/b.bin");
        fs::write(&src, b"new").unwrap();
        fs::create_dir_all(dst.parent().unwrap()).unwrap();
        fs::write(&dst, b"old").unwrap();
        persist(&src, &dst).unwrap();
        assert_eq!(fs::read(&dst).unwrap(), b"new");
        assert!(!src.exists());
    }
}

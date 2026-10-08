//! Applying a downloaded update.

use std::path::Path;

use crate::{Error, Result};

/// Replaces the running executable with `new_binary` (handles the Windows
/// "file in use" case by renaming the old image aside).
pub fn replace_current_exe(new_binary: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(new_binary, std::fs::Permissions::from_mode(0o755));
    }
    self_replace::self_replace(new_binary).map_err(|e| Error::io("replace current executable", e))
}

/// Replaces the file at `target` with a copy of `new_file`, marked
/// executable. The copy is written beside `target` and renamed over it, so
/// `target` is never half-written; a running program keeps its old image.
/// A symlinked `target` stays a link: the file it points at is replaced.
/// Used for AppImages, which are single executable files.
pub fn replace_file(new_file: &Path, target: &Path) -> Result<()> {
    let target = &std::fs::canonicalize(target).map_err(|e| Error::io(format!("resolve {}", target.display()), e))?;
    let name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let staging = target.with_file_name(format!(".{name}.update"));
    let result = std::fs::copy(new_file, &staging)
        .map_err(|e| Error::io(format!("copy {} to {}", new_file.display(), staging.display()), e))
        .and_then(|_| make_executable(&staging))
        .and_then(|_| std::fs::rename(&staging, target).map_err(|e| Error::io(format!("replace {}", target.display()), e)));
    if result.is_err() {
        let _ = std::fs::remove_file(&staging);
    }
    result
}

/// Marks `path` executable on Unix; a no-op elsewhere.
pub fn make_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path).map_err(|e| Error::io(format!("stat {}", path.display()), e))?.permissions();
        perms.set_mode(perms.mode() | 0o755);
        std::fs::set_permissions(path, perms).map_err(|e| Error::io(format!("chmod {}", path.display()), e))?;
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn replace_file_swaps_contents_in_place_and_makes_them_executable() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("IVSR.AppImage");
        std::fs::write(&target, b"old").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
        let download = dir.path().join("download.AppImage");
        std::fs::write(&download, b"new").unwrap();

        replace_file(&download, &target).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        assert_eq!(std::fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o755);
        let names: Vec<_> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names.len(), 2, "no staging file left behind: {names:?}");
    }

    #[test]
    fn replace_file_updates_the_file_a_symlink_points_at_and_keeps_the_link() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("IVSR-0.1.AppImage");
        std::fs::write(&real, b"old").unwrap();
        let link = dir.path().join("ivsr");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let download = dir.path().join("download.AppImage");
        std::fs::write(&download, b"new").unwrap();

        replace_file(&download, &link).unwrap();

        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(std::fs::read(&real).unwrap(), b"new");
    }

    #[test]
    fn replace_file_leaves_the_target_alone_when_the_source_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("IVSR.AppImage");
        std::fs::write(&target, b"old").unwrap();
        assert!(replace_file(&dir.path().join("missing"), &target).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"old");
    }
}

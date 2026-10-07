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

//! Installing engine runtimes from their published distributions.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use ivsr_core::{CancelToken, Engine};
use ivsr_update::{GitHubSource, HttpClient, Platform, ReleaseSource, Verification, archive, install};
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const MANIFEST: &str = "ivsr-install.json";

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum InstallProgress {
    Resolving,
    Downloading { received: u64, total: Option<u64> },
    Extracting,
    Done,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstallManifest {
    pub release: String,
    pub asset: String,
    pub verification: Verification,
    /// Unix seconds.
    pub installed_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstallReport {
    pub engine: String,
    pub executable: PathBuf,
    pub manifest: InstallManifest,
}

/// Reads the manifest written by `install`, if the engine was installed by ivsr.
pub fn installed(dir: &Path) -> Option<InstallManifest> {
    fs::read(dir.join(MANIFEST)).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

pub(crate) fn source_for(provider: &str, locator: &str, http: Arc<dyn HttpClient>) -> Result<Arc<dyn ReleaseSource>> {
    match provider {
        "github" => Ok(Arc::new(GitHubSource::new(locator, http)?)),
        other => Err(Error::Input(format!("unsupported release provider `{other}`"))),
    }
}

pub(crate) fn install(
    engine: &dyn Engine,
    downloads: &Path,
    http: Arc<dyn HttpClient>,
    progress: &mut dyn FnMut(InstallProgress),
    cancel: &CancelToken,
) -> Result<InstallReport> {
    let id = engine.info().id;
    let dist = engine
        .distribution()
        .ok_or_else(|| Error::Input(format!("engine `{id}` has no downloadable distribution")))?;
    let dir = engine.install_dir().ok_or_else(|| Error::Input(format!("engine `{id}` has no install location")))?;

    progress(InstallProgress::Resolving);
    let source = source_for(&dist.provider, &dist.locator, http.clone())?;
    let release = match &dist.tag {
        Some(tag) => source.release(tag)?,
        None => source
            .releases()?
            .into_iter()
            .filter(|r| !r.prerelease)
            .max_by(|a, b| a.version.cmp(&b.version))
            .ok_or_else(|| ivsr_update::Error::NoRelease(format!(" in {}", source.describe())))?,
    };
    let platform = Platform::current();
    let no_asset = || ivsr_update::Error::NoAsset { release: release.tag.clone(), platform: platform.to_string() };
    let needle = dist.assets.iter().find(|(os, _)| os == platform.os_key()).map(|(_, n)| n).ok_or_else(no_asset)?;
    let asset = release.assets.iter().find(|a| a.name.contains(needle.as_str())).ok_or_else(no_asset)?;

    let downloaded = ivsr_update::download(
        http.as_ref(),
        source.as_ref(),
        asset,
        downloads,
        &mut |received, total| progress(InstallProgress::Downloading { received, total }),
        &|| cancel.is_cancelled(),
    )?;
    cancel.check()?;

    progress(InstallProgress::Extracting);
    let parent = dir.parent().ok_or_else(|| Error::Input("invalid install directory".into()))?;
    fs::create_dir_all(parent).map_err(|e| ivsr_core::Error::io_at("create", parent, e))?;
    let staging = parent.join(format!(".{id}.staging"));
    let io = |what: &str, p: &Path, e| Error::from(ivsr_core::Error::io_at(what, p, e));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| io("clear", &staging, e))?;
    }
    let staged = (|| -> Result<PathBuf> {
        archive::extract(&downloaded.path, &staging)?;
        let exe = archive::find_file(&staging, &dist.executable, 2)
            .ok_or_else(|| ivsr_update::Error::Archive(format!("{} not found in {}", dist.executable, asset.name)))?;
        install::make_executable(&exe)?;
        Ok(exe)
    })();
    let exe = match staged {
        Ok(exe) => exe,
        Err(e) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    let manifest = InstallManifest {
        release: release.tag.clone(),
        asset: asset.name.clone(),
        verification: downloaded.verification,
        installed_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs(),
    };
    let manifest_json = serde_json::to_vec_pretty(&manifest).expect("manifest serializes");
    fs::write(staging.join(MANIFEST), manifest_json).map_err(|e| io("write", &staging, e))?;

    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| io("replace", &dir, e))?;
    }
    fs::rename(&staging, &dir).map_err(|e| io("install into", &dir, e))?;
    let _ = fs::remove_file(&downloaded.path);
    progress(InstallProgress::Done);

    let executable = dir.join(exe.strip_prefix(&staging).unwrap_or(&exe));
    Ok(InstallReport { engine: id, executable, manifest })
}

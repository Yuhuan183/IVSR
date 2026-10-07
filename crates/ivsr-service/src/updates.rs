//! Self-update for ivsr itself, driven by `[update]` in the configuration.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use ivsr_core::CancelToken;
use ivsr_update::{
    Asset, Downloaded, HttpClient, Platform, PlatformSelector, StateStore, UpdateCheck, Updater, archive, install,
};
use semver::Version;
use serde::Serialize;

use crate::config::UpdateConfig;
use crate::engines::source_for;
use crate::paths::AppPaths;
use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Flavor {
    /// Archives named `ivsr-cli-<version>-<target>.tar.gz|.zip`, applied in place.
    Cli,
    /// Platform installers (`.dmg`, `-setup.exe`, `.msi`, `.AppImage`), opened for the user.
    Desktop,
}

pub struct Updates<'a> {
    config: &'a UpdateConfig,
    paths: &'a AppPaths,
    http: Arc<dyn HttpClient>,
    flavor: Flavor,
    current: Version,
}

impl<'a> Updates<'a> {
    pub(crate) fn new(
        config: &'a UpdateConfig,
        paths: &'a AppPaths,
        http: Arc<dyn HttpClient>,
        flavor: Flavor,
        current: &str,
    ) -> Self {
        let current = Version::parse(current).unwrap_or_else(|_| Version::new(0, 0, 0));
        Self { config, paths, http, flavor, current }
    }

    pub fn is_configured(&self) -> bool {
        !self.config.repository.trim().is_empty()
    }

    pub fn current(&self) -> &Version {
        &self.current
    }

    fn source(&self) -> Result<Arc<dyn ivsr_update::ReleaseSource>> {
        if !self.is_configured() {
            return Err(ivsr_update::Error::NotConfigured.into());
        }
        if self.config.provider != "github" {
            return source_for(&self.config.provider, &self.config.repository, self.http.clone());
        }
        let token = Some(self.config.token_env.trim())
            .filter(|name| !name.is_empty())
            .and_then(|name| std::env::var(name).ok());
        let mut source = ivsr_update::GitHubSource::new(&self.config.repository, self.http.clone())?.with_token(token);
        if let Some(api) = &self.config.api_base {
            source = source.with_api_base(api.clone());
        }
        Ok(Arc::new(source))
    }

    fn selector(&self) -> PlatformSelector {
        let selector = PlatformSelector::new(Platform::current());
        match self.flavor {
            Flavor::Cli => selector.require("cli").suffixes([".tar.gz", ".zip"]),
            Flavor::Desktop => selector.suffixes([".dmg", "-setup.exe", ".msi", ".appimage", ".deb"]),
        }
    }

    fn store(&self) -> StateStore {
        StateStore::new(self.paths.update_state())
    }

    /// Queries the source now and records the result for automatic checks.
    pub fn check(&self) -> Result<UpdateCheck> {
        let updater = Updater::new(self.source()?, Arc::new(self.selector()), self.current.clone(), self.config.channel);
        let result = updater.check()?;
        let store = self.store();
        let mut state = store.load();
        let available = match &result {
            UpdateCheck::Available { latest, .. } => Some(latest.to_string()),
            UpdateCheck::UpToDate { .. } => None,
        };
        state.record(available, SystemTime::now());
        let _ = store.save(&state);
        Ok(result)
    }

    /// Checks only when automatic checks are enabled and the interval elapsed.
    /// Failures are swallowed: automatic checks must never disturb real work.
    pub fn check_if_due(&self) -> Option<UpdateCheck> {
        if !self.config.auto_check || !self.is_configured() {
            return None;
        }
        let interval = Duration::from_secs(u64::from(self.config.interval_hours.max(1)) * 3600);
        if !self.store().load().is_due(interval, SystemTime::now()) {
            return None;
        }
        self.check().ok()
    }

    /// Newer version recorded by an earlier check, without network access.
    pub fn known_update(&self) -> Option<Version> {
        let state = self.store().load();
        let available = Version::parse(state.available.as_deref()?).ok()?;
        let skipped = state.skipped.as_deref().and_then(|s| Version::parse(s).ok());
        (available > self.current && skipped.as_ref() != Some(&available)).then_some(available)
    }

    pub fn skip(&self, version: &Version) -> Result<()> {
        let store = self.store();
        let mut state = store.load();
        state.skipped = Some(version.to_string());
        store.save(&state).map_err(|e| ivsr_core::Error::io("save update state", e).into())
    }

    pub fn download(
        &self,
        asset: &Asset,
        progress: &mut dyn FnMut(u64, Option<u64>),
        cancel: &CancelToken,
    ) -> Result<Downloaded> {
        let source = self.source()?;
        let dir = self.paths.downloads_dir();
        Ok(ivsr_update::download(self.http.as_ref(), source.as_ref(), asset, &dir, progress, &|| cancel.is_cancelled())?)
    }

    /// Unpacks a CLI archive and swaps the running executable for the one inside.
    pub fn apply_cli(&self, archive_path: &Path, executable_name: &str) -> Result<PathBuf> {
        let staging = self.paths.downloads_dir().join("cli-update");
        if staging.exists() {
            std::fs::remove_dir_all(&staging).map_err(|e| ivsr_core::Error::io_at("clear", &staging, e))?;
        }
        archive::extract(archive_path, &staging)?;
        let exe = archive::find_file(&staging, executable_name, 2)
            .ok_or_else(|| Error::Input(format!("{executable_name} not found in the downloaded archive")))?;
        install::replace_current_exe(&exe)?;
        let _ = std::fs::remove_dir_all(&staging);
        let _ = std::fs::remove_file(archive_path);
        Ok(std::env::current_exe().unwrap_or(exe))
    }
}

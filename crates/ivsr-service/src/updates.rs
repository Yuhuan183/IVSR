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
    /// Platform installers (`.dmg`, `-setup.exe`, `.msi`, `.deb`), opened for the user.
    Desktop,
    /// The desktop app running as an AppImage: the new AppImage replaces it in place.
    AppImage,
}

/// How `flavor` recognises its asset for `platform` among a release's files.
fn selector_for(flavor: Flavor, platform: Platform) -> PlatformSelector {
    let selector = PlatformSelector::new(platform);
    match flavor {
        Flavor::Cli => selector.require("cli").suffixes([".tar.gz", ".zip"]),
        Flavor::Desktop => selector.suffixes([".dmg", "-setup.exe", ".msi", ".deb"]),
        Flavor::AppImage => selector.suffixes([".appimage"]),
    }
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
        selector_for(self.flavor, Platform::current())
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

    /// `check` as an automatic reminder: a version the user chose to skip is
    /// reported as up to date. Explicit checks show it regardless.
    pub fn reminder(&self, check: UpdateCheck) -> UpdateCheck {
        let skipped = self.store().load().skipped.and_then(|s| Version::parse(&s).ok());
        match check {
            UpdateCheck::Available { current, latest, .. } if skipped.as_ref() == Some(&latest) => {
                UpdateCheck::UpToDate { current, latest: Some(latest) }
            }
            other => other,
        }
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

    /// Replaces the AppImage at `target` (the `APPIMAGE` the app runs from)
    /// with the downloaded one; the next launch runs the new version.
    pub fn apply_appimage(&self, downloaded: &Path, target: &Path) -> Result<()> {
        install::replace_file(downloaded, target)?;
        let _ = std::fs::remove_file(downloaded);
        Ok(())
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

#[cfg(test)]
mod tests {
    use ivsr_update::{Arch, AssetSelector, Os, Release};

    use super::*;

    /// What the release workflow uploads for version 1.2.3: CLI archives named
    /// by target triple and Tauri's default installer names.
    const CANONICAL_ASSETS: &[&str] = &[
        "ivsr-cli-1.2.3-aarch64-apple-darwin.tar.gz",
        "ivsr-cli-1.2.3-x86_64-unknown-linux-gnu.tar.gz",
        "ivsr-cli-1.2.3-x86_64-pc-windows-msvc.zip",
        "IVSR_1.2.3_aarch64.dmg",
        "IVSR_1.2.3_x64-setup.exe",
        "IVSR_1.2.3_x64_en-US.msi",
        "IVSR_1.2.3_amd64.AppImage",
        "IVSR_1.2.3_amd64.deb",
    ];

    /// Per shipped platform: CLI archive suffix and installer suffix. Linux
    /// desktop installs from the .deb update through a .deb; AppImages are
    /// checked separately.
    const SHIPPED: [(Os, Arch, &str, &str); 3] = [
        (Os::Macos, Arch::Arm64, "-aarch64-apple-darwin.tar.gz", ".dmg"),
        (Os::Linux, Arch::X64, "-x86_64-unknown-linux-gnu.tar.gz", ".deb"),
        (Os::Windows, Arch::X64, "-x86_64-pc-windows-msvc.zip", "-setup.exe"),
    ];

    fn release(names: &[String]) -> Release {
        Release {
            tag: "v1.2.3".into(),
            version: None,
            name: String::new(),
            notes: String::new(),
            prerelease: false,
            published_at: None,
            page_url: None,
            assets: names
                .iter()
                .map(|n| Asset { name: n.clone(), size: 1, download_url: String::new(), api_url: None, digest: None })
                .collect(),
        }
    }

    /// Asset names from `IVSR_RELEASE_ASSETS` (one per line; the release
    /// workflow passes the real upload list), else the canonical set.
    fn assets() -> Vec<String> {
        match std::env::var_os("IVSR_RELEASE_ASSETS") {
            Some(file) => std::fs::read_to_string(&file)
                .unwrap_or_else(|e| panic!("{}: {e}", std::path::Path::new(&file).display()))
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect(),
            None => CANONICAL_ASSETS.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn release_assets_give_each_shipped_platform_one_cli_and_one_installer() {
        let release = release(&assets());
        for (os, arch, cli_suffix, installer_suffix) in SHIPPED {
            let platform = Platform { os, arch };
            let pick = |flavor| selector_for(flavor, platform).select(&release).map(|a| a.name.to_ascii_lowercase());
            let cli = pick(Flavor::Cli).unwrap_or_else(|| panic!("{platform}: no CLI archive in {:?}", release.assets));
            assert!(cli.starts_with("ivsr-cli-") && cli.ends_with(cli_suffix), "{platform}: CLI picked {cli}");
            let app = pick(Flavor::Desktop).unwrap_or_else(|| panic!("{platform}: no installer in {:?}", release.assets));
            assert!(app.ends_with(installer_suffix) && !app.contains("cli"), "{platform}: desktop picked {app}");
        }
    }

    #[test]
    fn release_assets_update_a_running_appimage_with_an_appimage() {
        let release = release(&assets());
        let platform = Platform { os: Os::Linux, arch: Arch::X64 };
        let picked = selector_for(Flavor::AppImage, platform).select(&release).map(|a| a.name.to_ascii_lowercase());
        assert!(picked.as_deref().is_some_and(|n| n.ends_with(".appimage")), "{picked:?}");
    }

    #[test]
    fn release_assets_never_match_unshipped_platforms() {
        let release = release(&assets());
        for platform in [Platform { os: Os::Macos, arch: Arch::X64 }, Platform { os: Os::Linux, arch: Arch::Arm64 }] {
            for flavor in [Flavor::Cli, Flavor::Desktop, Flavor::AppImage] {
                let picked = selector_for(flavor, platform).select(&release).map(|a| a.name.clone());
                assert_eq!(picked, None, "{platform} {flavor:?}");
            }
        }
    }
}

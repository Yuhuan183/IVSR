//! Deciding whether a newer release exists for this build.

use std::sync::Arc;

use semver::Version;
use serde::Serialize;

use crate::{Asset, AssetSelector, Channel, Release, ReleaseSource, Result};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum UpdateCheck {
    UpToDate { current: Version, latest: Option<Version> },
    Available { current: Version, latest: Version, release: Box<Release>, asset: Option<Asset> },
}

pub struct Updater {
    source: Arc<dyn ReleaseSource>,
    selector: Arc<dyn AssetSelector>,
    current: Version,
    channel: Channel,
}

impl Updater {
    pub fn new(source: Arc<dyn ReleaseSource>, selector: Arc<dyn AssetSelector>, current: Version, channel: Channel) -> Self {
        Self { source, selector, current, channel }
    }

    pub fn source(&self) -> &Arc<dyn ReleaseSource> {
        &self.source
    }

    /// Highest semver release on the channel, compared with the running version.
    pub fn check(&self) -> Result<UpdateCheck> {
        let newest = self
            .source
            .releases()?
            .into_iter()
            .filter(|r| self.channel.accepts(r))
            .filter_map(|r| r.version.clone().map(|v| (v, r)))
            .max_by(|(a, _), (b, _)| a.cmp(b));
        Ok(match newest {
            Some((latest, release)) if latest > self.current => {
                let asset = self.selector.select(&release).cloned();
                UpdateCheck::Available { current: self.current.clone(), latest, release: Box::new(release), asset }
            }
            other => UpdateCheck::UpToDate { current: self.current.clone(), latest: other.map(|(v, _)| v) },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::select::{Arch, Os, Platform, PlatformSelector};
    use crate::{Error, release::parse_tag};

    struct Fixed(Vec<Release>);
    impl ReleaseSource for Fixed {
        fn describe(&self) -> String {
            "fixed".into()
        }
        fn releases(&self) -> Result<Vec<Release>> {
            Ok(self.0.clone())
        }
        fn release(&self, _: &str) -> Result<Release> {
            Err(Error::NoRelease(String::new()))
        }
    }

    fn rel(tag: &str, prerelease: bool, assets: &[&str]) -> Release {
        Release {
            tag: tag.into(),
            version: parse_tag(tag),
            name: tag.into(),
            notes: String::new(),
            prerelease,
            published_at: None,
            page_url: None,
            assets: assets
                .iter()
                .map(|n| Asset { name: n.to_string(), size: 1, download_url: String::new(), api_url: None, digest: None })
                .collect(),
        }
    }

    fn updater(releases: Vec<Release>, current: &str, channel: Channel) -> Updater {
        let selector = PlatformSelector::new(Platform { os: Os::Macos, arch: Arch::Arm64 }).suffixes([".tar.gz"]);
        Updater::new(Arc::new(Fixed(releases)), Arc::new(selector), Version::parse(current).unwrap(), channel)
    }

    fn listing() -> Vec<Release> {
        vec![
            // Listed out of order on purpose: ordering must come from semver.
            rel("v0.10.0", false, &["ivsr-0.10.0-aarch64-apple-darwin.tar.gz"]),
            rel("v0.11.0-rc.1", true, &[]),
            rel("v0.9.0", false, &[]),
            rel("nightly", false, &[]),
        ]
    }

    #[test]
    fn stable_channel_picks_highest_semver_and_platform_asset() {
        match updater(listing(), "0.9.0", Channel::Stable).check().unwrap() {
            UpdateCheck::Available { latest, asset, .. } => {
                assert_eq!(latest, Version::new(0, 10, 0));
                assert_eq!(asset.unwrap().name, "ivsr-0.10.0-aarch64-apple-darwin.tar.gz");
            }
            other => panic!("expected update, got {other:?}"),
        }
    }

    #[test]
    fn beta_channel_sees_prereleases() {
        match updater(listing(), "0.10.0", Channel::Beta).check().unwrap() {
            UpdateCheck::Available { latest, asset, .. } => {
                assert_eq!(latest.to_string(), "0.11.0-rc.1");
                assert!(asset.is_none());
            }
            other => panic!("expected update, got {other:?}"),
        }
    }

    #[test]
    fn current_or_newer_build_is_up_to_date() {
        assert!(matches!(
            updater(listing(), "0.10.0", Channel::Stable).check().unwrap(),
            UpdateCheck::UpToDate { latest: Some(v), .. } if v == Version::new(0, 10, 0)
        ));
        assert!(matches!(updater(listing(), "1.0.0", Channel::Stable).check().unwrap(), UpdateCheck::UpToDate { .. }));
    }
}

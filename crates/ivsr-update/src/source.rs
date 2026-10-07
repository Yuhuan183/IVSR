use serde::{Deserialize, Serialize};

use crate::{Release, Result};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// Published, non-prerelease versions only.
    #[default]
    Stable,
    /// Includes prereleases.
    Beta,
}

impl Channel {
    pub fn accepts(self, release: &Release) -> bool {
        match self {
            Channel::Beta => true,
            Channel::Stable => !release.prerelease && release.version.as_ref().is_none_or(|v| v.pre.is_empty()),
        }
    }
}

/// Somewhere releases are published. Implementations return releases newest
/// first where possible; the updater does its own version ordering anyway.
pub trait ReleaseSource: Send + Sync {
    /// Short description for messages, e.g. `github:owner/repo`.
    fn describe(&self) -> String;

    /// Recent published releases (drafts excluded).
    fn releases(&self) -> Result<Vec<Release>>;

    /// One release by tag.
    fn release(&self, tag: &str) -> Result<Release>;

    /// Extra headers needed to download `asset` (e.g. authentication), and the URL to use.
    fn download_request(&self, asset: &crate::Asset) -> (String, Vec<(String, String)>) {
        (asset.download_url.clone(), Vec::new())
    }
}

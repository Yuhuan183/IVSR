//! A small, self-contained update framework.
//!
//! ```text
//! ReleaseSource ──releases()──▶ Updater::check ──▶ UpdateCheck
//!   (GitHub, …)                    │  Channel filter + semver compare
//!                                  └─ AssetSelector picks the platform asset
//! download() ──▶ verified file ──▶ archive::extract / install::replace_executable
//! ```
//!
//! Nothing here knows about ivsr: sources, selectors and HTTP are traits, so
//! other release hosts or products can plug in without changes.

pub mod archive;
pub mod download;
pub mod error;
pub mod github;
pub mod http;
pub mod install;
pub mod release;
pub mod schedule;
pub mod select;
pub mod source;
pub mod updater;

pub use download::{Downloaded, Expected, Verification, download, download_url};
pub use error::{Error, Result};
pub use github::GitHubSource;
pub use http::{HttpClient, UreqClient};
pub use release::{Asset, Release};
pub use schedule::{CheckState, StateStore};
pub use select::{AssetSelector, Platform, PlatformSelector};
pub use source::{Channel, ReleaseSource};
pub use updater::{UpdateCheck, Updater};

use semver::Version;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    pub size: u64,
    /// Public download URL.
    pub download_url: String,
    /// Authenticated API URL (private repositories), if the source has one.
    pub api_url: Option<String>,
    /// `algorithm:hex`, e.g. `sha256:ab12…`, when the source publishes one.
    pub digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Release {
    pub tag: String,
    /// Parsed from the tag (`v1.2.3` → `1.2.3`); `None` when not semver.
    pub version: Option<Version>,
    pub name: String,
    pub notes: String,
    pub prerelease: bool,
    pub published_at: Option<String>,
    pub page_url: Option<String>,
    pub assets: Vec<Asset>,
}

/// Parses a release tag as semver, tolerating a leading `v` and a product prefix
/// (`v1.2.3`, `1.2`, `ivsr-v1.2.3`).
pub fn parse_tag(tag: &str) -> Option<Version> {
    let start = tag.find(|c: char| c.is_ascii_digit())?;
    let raw = &tag[start..];
    if let Ok(v) = Version::parse(raw) {
        return Some(v);
    }
    // Pad `1.2` / `1` to a full triple.
    let (core, rest) = raw.find(['-', '+']).map_or((raw, ""), |i| raw.split_at(i));
    let parts: Vec<&str> = core.split('.').collect();
    if parts.is_empty() || parts.len() > 3 || parts.iter().any(|p| p.parse::<u64>().is_err()) {
        return None;
    }
    let mut padded = parts.join(".");
    for _ in parts.len()..3 {
        padded.push_str(".0");
    }
    Version::parse(&format!("{padded}{rest}")).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_parse_with_prefixes_and_short_forms() {
        assert_eq!(parse_tag("v1.2.3"), Some(Version::new(1, 2, 3)));
        assert_eq!(parse_tag("ivsr-v0.4.0"), Some(Version::new(0, 4, 0)));
        assert_eq!(parse_tag("v0.2"), Some(Version::new(0, 2, 0)));
        assert_eq!(parse_tag("v2.0.0-beta.1").unwrap().pre.as_str(), "beta.1");
        assert_eq!(parse_tag("nightly"), None);
    }
}

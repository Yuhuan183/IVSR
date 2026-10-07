//! GitHub Releases as a `ReleaseSource` (github.com or GitHub Enterprise).

use std::sync::Arc;

use serde::Deserialize;

use crate::http::{HttpClient, get_json};
use crate::release::parse_tag;
use crate::{Asset, Error, Release, ReleaseSource, Result};

pub const DEFAULT_API: &str = "https://api.github.com";

pub struct GitHubSource {
    owner: String,
    repo: String,
    api_base: String,
    token: Option<String>,
    http: Arc<dyn HttpClient>,
}

impl GitHubSource {
    /// `repository` is `owner/repo`.
    pub fn new(repository: &str, http: Arc<dyn HttpClient>) -> Result<Self> {
        let (owner, repo) = repository
            .trim()
            .trim_matches('/')
            .split_once('/')
            .filter(|(o, r)| !o.is_empty() && !r.is_empty() && !r.contains('/'))
            .ok_or(Error::NotConfigured)?;
        Ok(Self { owner: owner.into(), repo: repo.into(), api_base: DEFAULT_API.into(), token: None, http })
    }

    pub fn with_api_base(mut self, api_base: impl Into<String>) -> Self {
        self.api_base = api_base.into().trim_end_matches('/').to_string();
        self
    }

    pub fn with_token(mut self, token: Option<String>) -> Self {
        self.token = token.filter(|t| !t.trim().is_empty());
        self
    }

    fn headers(&self) -> Vec<(&str, String)> {
        let mut headers =
            vec![("Accept", "application/vnd.github+json".to_string()), ("X-GitHub-Api-Version", "2022-11-28".into())];
        if let Some(token) = &self.token {
            headers.push(("Authorization", format!("Bearer {token}")));
        }
        headers
    }

    fn get<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{}/repos/{}/{}{path}", self.api_base, self.owner, self.repo);
        let headers = self.headers();
        let refs: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
        get_json(self.http.as_ref(), &url, &refs)
    }
}

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    published_at: Option<String>,
    html_url: Option<String>,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Deserialize)]
struct GhAsset {
    name: String,
    size: u64,
    url: String,
    browser_download_url: String,
    digest: Option<String>,
}

impl From<GhRelease> for Release {
    fn from(r: GhRelease) -> Self {
        Release {
            version: parse_tag(&r.tag_name),
            name: r.name.filter(|n| !n.is_empty()).unwrap_or_else(|| r.tag_name.clone()),
            tag: r.tag_name,
            notes: r.body.unwrap_or_default(),
            prerelease: r.prerelease,
            published_at: r.published_at,
            page_url: r.html_url,
            assets: r
                .assets
                .into_iter()
                .map(|a| Asset {
                    name: a.name,
                    size: a.size,
                    download_url: a.browser_download_url,
                    api_url: Some(a.url),
                    digest: a.digest,
                })
                .collect(),
        }
    }
}

impl ReleaseSource for GitHubSource {
    fn describe(&self) -> String {
        format!("github:{}/{}", self.owner, self.repo)
    }

    fn releases(&self) -> Result<Vec<Release>> {
        let releases: Vec<GhRelease> = self.get("/releases?per_page=30")?;
        Ok(releases.into_iter().filter(|r| !r.draft).map(Release::from).collect())
    }

    fn release(&self, tag: &str) -> Result<Release> {
        match self.get::<GhRelease>(&format!("/releases/tags/{tag}")) {
            Err(Error::Http { status: 404, .. }) => Err(Error::NoRelease(format!(" tagged `{tag}` in {}", self.describe()))),
            other => other.map(Release::from),
        }
    }

    fn download_request(&self, asset: &Asset) -> (String, Vec<(String, String)>) {
        // Private repositories need the authenticated API endpoint.
        match (&self.token, &asset.api_url) {
            (Some(token), Some(api)) => (
                api.clone(),
                vec![
                    ("Accept".into(), "application/octet-stream".into()),
                    ("Authorization".into(), format!("Bearer {token}")),
                ],
            ),
            _ => (asset.download_url.clone(), Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::Mutex;

    use super::*;
    use crate::http::HttpResponse;

    type Headers = Vec<(String, String)>;

    /// Serves canned bodies by URL and records request headers.
    struct Canned {
        routes: Vec<(String, u16, String)>,
        seen: Mutex<Vec<(String, Headers)>>,
    }

    impl HttpClient for Canned {
        fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<HttpResponse> {
            self.seen.lock().unwrap().push((url.into(), headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()));
            let (_, status, body) = self.routes.iter().find(|(u, ..)| u == url).cloned().unwrap_or((url.into(), 404, "{}".into()));
            Ok(HttpResponse { status, content_length: Some(body.len() as u64), body: Box::new(Cursor::new(body.into_bytes())) })
        }
    }

    const LISTING: &str = r#"[
      {"tag_name": "v0.3.0-beta.1", "name": "", "draft": false, "prerelease": true, "assets": []},
      {"tag_name": "v0.2.0", "name": "ivsr 0.2.0", "body": "notes", "draft": false, "prerelease": false,
       "published_at": "2026-09-01T00:00:00Z", "html_url": "https://github.com/acme/ivsr/releases/tag/v0.2.0",
       "assets": [{"name": "ivsr-0.2.0-aarch64-apple-darwin.tar.gz", "size": 10,
                   "url": "https://api.github.com/repos/acme/ivsr/releases/assets/1",
                   "browser_download_url": "https://github.com/acme/ivsr/releases/download/v0.2.0/ivsr-0.2.0-aarch64-apple-darwin.tar.gz",
                   "digest": "sha256:abc"}]},
      {"tag_name": "v0.4.0", "draft": true, "prerelease": false, "assets": []}
    ]"#;

    fn source(token: Option<&str>) -> (GitHubSource, Arc<Canned>) {
        let http = Arc::new(Canned {
            routes: vec![("https://api.github.com/repos/acme/ivsr/releases?per_page=30".into(), 200, LISTING.into())],
            seen: Mutex::new(Vec::new()),
        });
        let src = GitHubSource::new("acme/ivsr", http.clone()).unwrap().with_token(token.map(String::from));
        (src, http)
    }

    #[test]
    fn listing_maps_fields_and_drops_drafts() {
        let (src, _) = source(None);
        let releases = src.releases().unwrap();
        let tags: Vec<_> = releases.iter().map(|r| r.tag.as_str()).collect();
        assert_eq!(tags, vec!["v0.3.0-beta.1", "v0.2.0"]);
        let stable = &releases[1];
        assert_eq!(stable.version, Some(semver::Version::new(0, 2, 0)));
        assert_eq!(stable.assets[0].digest.as_deref(), Some("sha256:abc"));
        assert_eq!(releases[0].name, "v0.3.0-beta.1", "empty name falls back to tag");
    }

    #[test]
    fn token_is_sent_and_switches_downloads_to_api_url() {
        let (src, http) = source(Some("t0k"));
        let release = src.releases().unwrap().remove(1);
        let (_, headers) = &http.seen.lock().unwrap()[0];
        assert!(headers.contains(&("Authorization".into(), "Bearer t0k".into())));
        let (url, headers) = src.download_request(&release.assets[0]);
        assert_eq!(url, "https://api.github.com/repos/acme/ivsr/releases/assets/1");
        assert!(headers.contains(&("Accept".into(), "application/octet-stream".into())));
    }

    #[test]
    fn missing_tag_is_no_release() {
        let (src, _) = source(None);
        assert!(matches!(src.release("v9.9.9"), Err(Error::NoRelease(_))));
    }

    #[test]
    fn malformed_repository_is_not_configured() {
        let http: Arc<dyn HttpClient> = Arc::new(Canned { routes: vec![], seen: Mutex::new(vec![]) });
        for bad in ["", "acme", "acme/", "a/b/c"] {
            assert!(matches!(GitHubSource::new(bad, http.clone()), Err(Error::NotConfigured)), "{bad}");
        }
    }
}

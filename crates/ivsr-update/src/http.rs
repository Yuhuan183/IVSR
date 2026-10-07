//! Minimal blocking HTTP abstraction, so sources and downloads can be tested
//! without a network and the transport can be swapped.

use std::io::Read;
use std::time::Duration;

use crate::{Error, Result};

pub struct HttpResponse {
    pub status: u16,
    pub content_length: Option<u64>,
    pub body: Box<dyn Read + Send>,
}

pub trait HttpClient: Send + Sync {
    /// Performs a GET, following redirects. Non-2xx statuses are returned, not errors.
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<HttpResponse>;
}

/// Reads a JSON body from a successful response.
pub fn get_json<T: serde::de::DeserializeOwned>(
    client: &dyn HttpClient,
    url: &str,
    headers: &[(&str, &str)],
) -> Result<T> {
    let resp = client.get(url, headers)?;
    if !(200..300).contains(&resp.status) {
        return Err(Error::Http { status: resp.status, url: url.to_string() });
    }
    // Release listings are small; cap the read to guard against hostile servers.
    let mut body = Vec::new();
    resp.body.take(16 * 1024 * 1024).read_to_end(&mut body).map_err(|e| Error::Network(e.to_string()))?;
    serde_json::from_slice(&body).map_err(|e| Error::Parse(e.to_string()))
}

pub struct UreqClient {
    agent: ureq::Agent,
}

impl UreqClient {
    pub fn new(user_agent: &str) -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .user_agent(user_agent)
            .build()
            .into();
        Self { agent }
    }
}

impl HttpClient for UreqClient {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<HttpResponse> {
        let mut req = self.agent.get(url);
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let resp = req.call().map_err(|e| Error::Network(e.to_string()))?;
        let status = resp.status().as_u16();
        let content_length = resp.body().content_length();
        let body = resp.into_body().into_reader();
        Ok(HttpResponse { status, content_length, body: Box::new(body) })
    }
}

//! HTTP behind a trait, so tests can stand in a fake GitHub.

use std::fs::File;
use std::io;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};

pub const USER_AGENT: &str = "packmule";

/// Largest JSON / text body read into memory (release lists can be a few MB).
const MAX_BODY: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    /// Header names in lowercase.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers.iter().find(|(k, _)| *k == name).map(|(_, v)| v.as_str())
    }
}

pub trait Http: Send + Sync {
    /// A GET that returns every status as a response; only transport failures are errors.
    fn get(&self, url: &str, headers: &[(&str, String)]) -> Result<Response>;

    /// Streams a successful GET to `dest`.
    fn download(&self, url: &str, dest: &Path) -> Result<()>;
}

pub struct UreqHttp {
    agent: ureq::Agent,
}

impl UreqHttp {
    pub fn new() -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(15)))
            .user_agent(USER_AGENT)
            .build();
        Self { agent: config.into() }
    }
}

impl Default for UreqHttp {
    fn default() -> Self {
        Self::new()
    }
}

impl Http for UreqHttp {
    fn get(&self, url: &str, headers: &[(&str, String)]) -> Result<Response> {
        let mut req = self.agent.get(url);
        for (k, v) in headers {
            req = req.header(*k, v);
        }
        let mut res = req.call().with_context(|| format!("Request failed: {url}"))?;
        let status = res.status().as_u16();
        let headers = res
            .headers()
            .iter()
            .filter_map(|(k, v)| Some((k.as_str().to_ascii_lowercase(), v.to_str().ok()?.to_string())))
            .collect();
        // A 304 carries no body but may still say it's gzipped; reading it would fail.
        let body = if matches!(status, 204 | 304) {
            Vec::new()
        } else {
            res.body_mut().with_config().limit(MAX_BODY).read_to_vec()?
        };
        Ok(Response { status, headers, body })
    }

    fn download(&self, url: &str, dest: &Path) -> Result<()> {
        let mut res = self.agent.get(url).call().with_context(|| format!("Download failed: {url}"))?;
        let status = res.status().as_u16();
        if !(200..300).contains(&status) {
            bail!("Download failed ({status}): {url}");
        }
        let mut out = File::create(dest)?;
        io::copy(&mut res.body_mut().as_reader(), &mut out)?;
        out.sync_all()?;
        Ok(())
    }
}

//! GitHub releases: finding the latest release and picking the right zip.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow, bail};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::http::Http;

pub const API: &str = "https://api.github.com";

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ReleaseAsset {
    pub id: u64,
    pub name: String,
    pub size: u64,
    pub browser_download_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub html_url: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<ReleaseAsset>,
}

/// "spoken/v3.8.4" -> "spoken/", "v1.2.3" -> "".
pub fn tag_stream(tag: &str) -> &str {
    tag.rfind('/').map_or("", |i| &tag[..=i])
}

/// A repository, optionally narrowed to one release stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRef {
    pub repo: String,
    /// Set when a release link selected a stream ("" = unprefixed tags).
    pub tag_prefix: Option<String>,
}

/// Accepts "owner/repo" or any github.com URL pointing into the repo. A link to
/// a release (".../releases/tag/spoken/v3.8.4") also selects that release's
/// stream.
pub fn parse_repo(input: &str) -> Result<RepoRef> {
    let mut s = input.trim();
    while let Some(t) = s.strip_suffix('/') {
        s = t;
    }
    let s = s.strip_suffix(".git").unwrap_or(s);
    let is_name = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c));

    let lower = s.to_ascii_lowercase();
    let rest = ["https://", "http://", ""]
        .iter()
        .flat_map(|scheme| ["www.", ""].map(move |www| format!("{scheme}{www}github.com/")))
        .find(|prefix| lower.starts_with(prefix.as_str()))
        .map(|prefix| &s[prefix.len()..]);

    let bad = || anyhow!("Not a GitHub repository URL: {input}");
    match rest {
        Some(rest) => {
            let mut parts = rest.splitn(3, '/');
            let owner = parts.next().unwrap_or("");
            let name: String = parts
                .next()
                .unwrap_or("")
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || "_.-".contains(*c))
                .collect();
            if !is_name(owner) || !is_name(&name) {
                return Err(bad());
            }
            let repo = format!("{owner}/{name}");
            let tag = parts.next().and_then(|tail| tail.strip_prefix("releases/tag/"));
            let tag_prefix = tag.map(|t| {
                let t = t.split(['?', '#']).next().unwrap_or("");
                tag_stream(&percent_decode(t)).to_string()
            });
            Ok(RepoRef { repo, tag_prefix })
        }
        None => match s.split_once('/') {
            Some((owner, name)) if is_name(owner) && is_name(name) => {
                Ok(RepoRef { repo: s.to_string(), tag_prefix: None })
            }
            _ => Err(bad()),
        },
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 3 <= bytes.len()
            && let Ok(b) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("zz"), 16)
        {
            out.push(b);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Shape of the release.json written by the BigWigsMods packager.
#[derive(Debug, Clone, Deserialize)]
struct PackagerManifest {
    releases: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestEntry {
    filename: String,
    #[serde(default)]
    nolib: bool,
    #[serde(default)]
    metadata: Vec<ManifestMeta>,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestMeta {
    flavor: String,
    interface: u32,
}

#[derive(Debug, Clone)]
pub struct AssetChoice {
    pub asset: ReleaseAsset,
    pub compat_warning: Option<String>,
    /// Some(true/false) when the release.json lists (or doesn't list) the interface; None without one.
    pub interface_match: Option<bool>,
}

pub struct GitHubClient {
    http: Arc<dyn Http>,
    token: Box<dyn Fn() -> String + Send + Sync>,
    /// ETag cache so unchanged releases cost no rate limit (304s are free).
    etags: Mutex<HashMap<String, (String, Vec<u8>)>>,
    manifests: Mutex<HashMap<String, PackagerManifest>>,
}

impl GitHubClient {
    pub fn new(http: Arc<dyn Http>, token: impl Fn() -> String + Send + Sync + 'static) -> Self {
        Self { http, token: Box::new(token), etags: Mutex::default(), manifests: Mutex::default() }
    }

    fn api<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = format!("{API}{path}");
        let mut headers = vec![
            ("Accept", "application/vnd.github+json".to_string()),
            ("X-GitHub-Api-Version", "2022-11-28".to_string()),
        ];
        let token = (self.token)();
        if !token.is_empty() {
            headers.push(("Authorization", format!("Bearer {token}")));
        }
        let cached = self.etags.lock().unwrap().get(&url).cloned();
        if let Some((etag, _)) = &cached {
            headers.push(("If-None-Match", etag.clone()));
        }

        let res = self.http.get(&url, &headers)?;
        let body = match (res.status, cached) {
            (304, Some((_, body))) => body,
            (404, _) => bail!("Repository not found or has no releases"),
            (403 | 429, _) => {
                let when = res
                    .header("x-ratelimit-reset")
                    .and_then(|r| r.parse::<i64>().ok())
                    .and_then(|r| chrono::DateTime::from_timestamp(r, 0))
                    .map(|t| t.with_timezone(&chrono::Local).format("%-I:%M %p").to_string())
                    .unwrap_or_else(|| "later".into());
                bail!("GitHub rate limit hit; try again after {when} or add a token in Settings")
            }
            (200..=299, _) => {
                if let Some(etag) = res.header("etag") {
                    self.etags.lock().unwrap().insert(url, (etag.to_string(), res.body.clone()));
                }
                res.body
            }
            (status, _) => bail!("GitHub API {status} for {path}"),
        };
        Ok(serde_json::from_slice(&body)?)
    }

    /// Newest published (non-draft, non-prerelease) release. With a tag prefix
    /// (including "" for unprefixed tags), only that stream counts: GitHub's
    /// /releases/latest is shared by every product a repo publishes.
    pub fn latest_release(&self, repo: &str, tag_prefix: Option<&str>) -> Result<Release> {
        let Some(prefix) = tag_prefix else {
            return self.api(&format!("/repos/{repo}/releases/latest"));
        };
        for page in 1..=5 {
            let list: Vec<Release> = self.api(&format!("/repos/{repo}/releases?per_page=100&page={page}"))?;
            if let Some(hit) = list.iter().find(|r| !r.draft && !r.prerelease && tag_stream(&r.tag_name) == prefix) {
                return Ok(hit.clone());
            }
            if list.len() < 100 {
                break;
            }
        }
        if prefix.is_empty() {
            bail!("No published release with an unprefixed tag")
        }
        bail!("No published release with a tag starting \"{prefix}\"")
    }

    /// Distinct tag streams among recent releases ("" = tags without a prefix), newest first.
    pub fn release_streams(&self, repo: &str) -> Result<Vec<String>> {
        let list: Vec<Release> = self.api(&format!("/repos/{repo}/releases?per_page=100"))?;
        let mut streams: Vec<String> = Vec::new();
        for r in list.iter().filter(|r| !r.draft) {
            let s = tag_stream(&r.tag_name).to_string();
            if !streams.contains(&s) {
                streams.push(s);
            }
        }
        Ok(streams)
    }

    fn manifest(&self, asset: &ReleaseAsset) -> Result<PackagerManifest> {
        // Release asset URLs are immutable per tag, so cache by URL for the session.
        if let Some(hit) = self.manifests.lock().unwrap().get(&asset.browser_download_url) {
            return Ok(hit.clone());
        }
        let res = self.http.get(&asset.browser_download_url, &[])?;
        if !(200..300).contains(&res.status) {
            bail!("Failed to fetch release.json ({})", res.status);
        }
        let manifest: PackagerManifest = serde_json::from_slice(&res.body)?;
        self.manifests.lock().unwrap().insert(asset.browser_download_url.clone(), manifest.clone());
        Ok(manifest)
    }

    /// Picks the zip for a game client. Prefers the packager's release.json,
    /// matching on interface number; otherwise falls back to the zip assets.
    pub fn choose_asset(&self, release: &Release, game_interface: u32) -> Result<AssetChoice> {
        let zips: Vec<&ReleaseAsset> =
            release.assets.iter().filter(|a| a.name.to_lowercase().ends_with(".zip")).collect();
        if zips.is_empty() {
            bail!("Release {} has no packaged .zip (source-only releases are not supported)", release.tag_name);
        }

        if let Some(manifest_asset) = release.assets.iter().find(|a| a.name == "release.json") {
            let mut entries = self.manifest(manifest_asset)?.releases;
            entries.sort_by_key(|e| e.nolib); // stable: full builds first
            let matched = entries.iter().find(|e| e.metadata.iter().any(|m| m.interface == game_interface));
            if let Some(chosen) = matched.or(entries.first())
                && let Some(asset) = zips.iter().find(|z| z.name == chosen.filename)
            {
                let mut listed: Vec<String> = Vec::new();
                for m in &chosen.metadata {
                    let s = format!("{} {}", m.flavor, m.interface);
                    if !listed.contains(&s) {
                        listed.push(s);
                    }
                }
                return Ok(AssetChoice {
                    asset: (*asset).clone(),
                    compat_warning: matched
                        .is_none()
                        .then(|| format!("No build lists interface {game_interface} (has: {})", listed.join(", "))),
                    interface_match: Some(matched.is_some()),
                });
            }
        }

        let full: Vec<&&ReleaseAsset> = zips.iter().filter(|z| !z.name.to_lowercase().contains("-nolib")).collect();
        let pool: Vec<&ReleaseAsset> = if full.is_empty() { zips.clone() } else { full.into_iter().copied().collect() };
        Ok(AssetChoice {
            asset: pool[0].clone(),
            compat_warning: (pool.len() > 1)
                .then(|| format!("No release.json; picked {} of {} zips", pool[0].name, pool.len())),
            interface_match: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{FakeHttp, FakeRelease, manifest};

    fn client(http: &Arc<FakeHttp>) -> GitHubClient {
        GitHubClient::new(http.clone(), String::new)
    }

    #[test]
    fn etag_cache_reuses_unchanged_answers() {
        let http = Arc::new(FakeHttp::default());
        http.releases(&[FakeRelease { repo: "o/r", tag: "v1", zips: vec![("r.zip", vec![1])], manifest: None }]);
        let gh = client(&http);
        assert_eq!(gh.latest_release("o/r", None).unwrap().tag_name, "v1");
        assert_eq!(gh.latest_release("o/r", None).unwrap().tag_name, "v1");
        assert_eq!(*http.not_modified.lock().unwrap(), 1);
    }

    #[test]
    fn api_errors() {
        let http = Arc::new(FakeHttp::default());
        http.serve_status(
            "https://api.github.com/repos/o/limited/releases/latest",
            403,
            &[("X-RateLimit-Reset", "1900000000")],
        );
        let gh = client(&http);
        let e = gh.latest_release("o/limited", None).unwrap_err().to_string();
        assert!(e.contains("rate limit") && e.contains("token"), "{e}");
        let e = gh.latest_release("o/missing", None).unwrap_err().to_string();
        assert!(e.contains("not found"), "{e}");
    }

    #[test]
    fn streams_pick_their_own_latest() {
        let http = Arc::new(FakeHttp::default());
        let rel = |tag| FakeRelease { repo: "o/multi", tag, zips: vec![("x.zip", vec![1])], manifest: None };
        http.releases(&[rel("quests-audio/v2.5.0"), rel("spoken/v3.8.4"), rel("spoken/v3.8.3"), rel("v0.9")]);
        let gh = client(&http);
        assert_eq!(gh.latest_release("o/multi", Some("spoken/")).unwrap().tag_name, "spoken/v3.8.4");
        assert_eq!(gh.latest_release("o/multi", Some("")).unwrap().tag_name, "v0.9");
        assert!(gh.latest_release("o/multi", Some("nope/")).is_err());
        assert_eq!(gh.release_streams("o/multi").unwrap(), ["quests-audio/", "spoken/", ""]);
    }

    #[test]
    fn chooses_build_for_the_client() {
        let http = Arc::new(FakeHttp::default());
        http.releases(&[FakeRelease {
            repo: "o/bw",
            tag: "v400",
            zips: vec![("BW-v400.zip", vec![1]), ("BW-v400-classic.zip", vec![2]), ("BW-v400-forever.zip", vec![3])],
            manifest: Some(manifest(&[
                ("BW-v400.zip", "mainline", 120001),
                ("BW-v400-classic.zip", "classic", 11507),
                ("BW-v400-forever.zip", "forever", 16001),
            ])),
        }]);
        let gh = client(&http);
        let rel = gh.latest_release("o/bw", None).unwrap();
        let c = gh.choose_asset(&rel, 16001).unwrap();
        assert_eq!(
            (c.asset.name.as_str(), c.interface_match, c.compat_warning.is_none()),
            ("BW-v400-forever.zip", Some(true), true)
        );
        let c = gh.choose_asset(&rel, 50000).unwrap();
        assert_eq!(c.asset.name, "BW-v400.zip");
        assert_eq!(c.interface_match, Some(false));
        assert!(c.compat_warning.unwrap().contains("mainline 120001"));
    }

    #[test]
    fn falls_back_without_release_json() {
        let http = Arc::new(FakeHttp::default());
        http.releases(&[FakeRelease {
            repo: "o/plain",
            tag: "1.0",
            zips: vec![("Addon-nolib.zip", vec![1]), ("Addon.zip", vec![2])],
            manifest: None,
        }]);
        let gh = client(&http);
        let rel = gh.latest_release("o/plain", None).unwrap();
        let c = gh.choose_asset(&rel, 16001).unwrap();
        assert_eq!((c.asset.name.as_str(), c.interface_match), ("Addon.zip", None));
        http.releases(&[FakeRelease { repo: "o/src", tag: "1.0", zips: vec![], manifest: None }]);
        let rel = gh.latest_release("o/src", None).unwrap();
        assert!(gh.choose_asset(&rel, 16001).unwrap_err().to_string().contains("no packaged .zip"));
    }

    fn rr(repo: &str, prefix: Option<&str>) -> RepoRef {
        RepoRef { repo: repo.into(), tag_prefix: prefix.map(Into::into) }
    }

    #[test]
    fn streams() {
        assert_eq!(tag_stream("spoken/v3.8.4"), "spoken/");
        assert_eq!(tag_stream("v1.2.3"), "");
        assert_eq!(tag_stream("a/b/c"), "a/b/");
    }

    #[test]
    fn parses_repo_links() {
        assert_eq!(parse_repo("hammackj/satchel").unwrap(), rr("hammackj/satchel", None));
        assert_eq!(parse_repo(" https://github.com/hammackj/satchel/ ").unwrap(), rr("hammackj/satchel", None));
        assert_eq!(parse_repo("https://github.com/hammackj/satchel.git").unwrap(), rr("hammackj/satchel", None));
        assert_eq!(parse_repo("github.com/BigWigsMods/BigWigs/issues/12").unwrap(), rr("BigWigsMods/BigWigs", None));
        assert_eq!(
            parse_repo("https://www.github.com/o/r/releases/tag/spoken/v3.8.4").unwrap(),
            rr("o/r", Some("spoken/"))
        );
        assert_eq!(parse_repo("https://github.com/o/r/releases/tag/v1.0?x=1").unwrap(), rr("o/r", Some("")));
        assert_eq!(
            parse_repo("https://github.com/o/r/releases/tag/quests-audio%2Fv2.5.0").unwrap(),
            rr("o/r", Some("quests-audio/"))
        );
        assert!(parse_repo("https://gitlab.com/o/r").is_err());
        assert!(parse_repo("https://github.com/hammackj").is_err());
        assert!(parse_repo("not a repo").is_err());
    }
}

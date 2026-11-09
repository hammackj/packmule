//! Test helpers: an in-memory HTTP server and zip builders.

use std::collections::HashMap;
use std::io::{Cursor, Write};
use std::path::Path;
use std::sync::Mutex;

use anyhow::{Result, bail};
use zip::write::SimpleFileOptions;

use crate::http::{Http, Response};

/// status, headers, body
type Route = (u16, Vec<(String, String)>, Vec<u8>);

#[derive(Default)]
pub struct FakeHttp {
    /// url -> (status, headers, body)
    routes: Mutex<HashMap<String, Route>>,
    pub requests: Mutex<Vec<String>>,
    /// How many 304 Not Modified answers were given.
    pub not_modified: Mutex<usize>,
}

impl FakeHttp {
    pub fn serve(&self, url: &str, body: Vec<u8>) {
        self.routes.lock().unwrap().insert(url.into(), (200, Vec::new(), body));
    }

    pub fn serve_json(&self, url: &str, json: serde_json::Value) {
        self.serve(url, serde_json::to_vec(&json).unwrap());
    }

    pub fn serve_status(&self, url: &str, status: u16, headers: &[(&str, &str)]) {
        let h = headers.iter().map(|(k, v)| (k.to_lowercase(), v.to_string())).collect();
        self.routes.lock().unwrap().insert(url.into(), (status, h, Vec::new()));
    }

    pub fn count(&self, url: &str) -> usize {
        self.requests.lock().unwrap().iter().filter(|u| *u == url).count()
    }
}

impl Http for FakeHttp {
    fn get(&self, url: &str, headers: &[(&str, String)]) -> Result<Response> {
        self.requests.lock().unwrap().push(url.to_string());
        let Some((status, hdrs, body)) = self.routes.lock().unwrap().get(url).cloned() else {
            return Ok(Response { status: 404, headers: Vec::new(), body: Vec::new() });
        };
        let header = |n: &str| headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)).map(|(_, v)| v.as_str());
        if let Some(range) = header("Range") {
            let (a, b) = range.trim_start_matches("bytes=").split_once('-').unwrap();
            let (a, b): (usize, usize) = (a.parse()?, b.parse()?); // suffix ranges fail, like GitHub
            return Ok(Response { status: 206, headers: Vec::new(), body: body[a..=b.min(body.len() - 1)].to_vec() });
        }
        // A simple deterministic ETag so the cache can be exercised.
        let etag =
            format!("\"{:x}\"", body.iter().fold(body.len() as u64, |h, b| h.wrapping_mul(31).wrapping_add(*b as u64)));
        if status == 200 && header("If-None-Match") == Some(etag.as_str()) {
            *self.not_modified.lock().unwrap() += 1;
            return Ok(Response { status: 304, headers: Vec::new(), body: Vec::new() });
        }
        let mut out_headers = hdrs;
        if status == 200 {
            out_headers.push(("etag".into(), etag));
        }
        Ok(Response { status, headers: out_headers, body })
    }

    fn download(&self, url: &str, dest: &Path) -> Result<()> {
        let res = self.get(url, &[])?;
        if res.status != 200 {
            bail!("Download failed ({}): {url}", res.status);
        }
        std::fs::write(dest, res.body)?;
        Ok(())
    }
}

/// A zip with the given files; names ending in "/" become directories.
/// `zip64` rewrites the end of the file into the Zip64 form (sentinel values in
/// the classic record, the real ones in a Zip64 record found via a locator),
/// which the zip crate only writes for archives over 4 GB.
pub fn make_zip(files: &[(&str, &[u8])], zip64: bool) -> Vec<u8> {
    let zip = make_plain_zip(files);
    if zip64 { to_zip64(zip) } else { zip }
}

fn to_zip64(mut zip: Vec<u8>) -> Vec<u8> {
    let eocd = zip.len() - 22; // no comment
    let le16 = |b: &[u8], i: usize| u16::from_le_bytes([b[i], b[i + 1]]) as u64;
    let le32 = |b: &[u8], i: usize| u32::from_le_bytes(b[i..i + 4].try_into().unwrap()) as u64;
    let (count, cd_size, offset) = (le16(&zip, eocd + 10), le32(&zip, eocd + 12), le32(&zip, eocd + 16));
    let record_at = eocd as u64;
    zip.truncate(eocd);

    let mut rec = Vec::new();
    rec.extend(0x0606_4b50u32.to_le_bytes());
    rec.extend(44u64.to_le_bytes()); // size of the rest of the record
    rec.extend(45u16.to_le_bytes());
    rec.extend(45u16.to_le_bytes());
    rec.extend(0u32.to_le_bytes());
    rec.extend(0u32.to_le_bytes());
    rec.extend(count.to_le_bytes());
    rec.extend(count.to_le_bytes());
    rec.extend(cd_size.to_le_bytes());
    rec.extend(offset.to_le_bytes());
    zip.extend(rec);

    zip.extend(0x0706_4b50u32.to_le_bytes());
    zip.extend(0u32.to_le_bytes());
    zip.extend(record_at.to_le_bytes());
    zip.extend(1u32.to_le_bytes());

    zip.extend(0x0605_4b50u32.to_le_bytes());
    zip.extend(0u16.to_le_bytes());
    zip.extend(0u16.to_le_bytes());
    zip.extend(0xffffu16.to_le_bytes());
    zip.extend(0xffffu16.to_le_bytes());
    zip.extend(0xffff_ffffu32.to_le_bytes());
    zip.extend(0xffff_ffffu32.to_le_bytes());
    zip.extend(0u16.to_le_bytes());
    zip
}

fn make_plain_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default();
    for (name, data) in files {
        if name.ends_with('/') {
            w.add_directory(*name, opts).unwrap();
        } else {
            w.start_file(*name, opts).unwrap();
            w.write_all(data).unwrap();
        }
    }
    w.finish().unwrap().into_inner()
}

pub fn make_zip_with_symlink(toc: &str, link: &str, target: &str) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default();
    w.start_file(toc, opts).unwrap();
    w.write_all(b"## Title: x").unwrap();
    w.add_symlink(link, target, opts).unwrap();
    w.finish().unwrap().into_inner()
}

/// Names for a zip whose central directory is bigger than the 64 KB tail
/// window, so reading it takes its own range request.
pub fn many_files(folder: &str, n: usize) -> Vec<String> {
    (0..n).map(|i| format!("{folder}/Media/Sounds/Long/Path/To/Make/The/Directory/Bigger/file_{i:05}.ogg")).collect()
}

/// A fake GitHub release: serves the API JSON for /releases/latest and the
/// release list, plus each asset. `manifest` becomes a release.json asset.
pub struct FakeRelease<'a> {
    pub repo: &'a str,
    pub tag: &'a str,
    pub zips: Vec<(&'a str, Vec<u8>)>,
    pub manifest: Option<serde_json::Value>,
}

impl FakeHttp {
    /// Publishes releases for one repo, newest first.
    pub fn releases(&self, releases: &[FakeRelease]) {
        let mut list = Vec::new();
        let mut id = 1000 * (self.routes.lock().unwrap().len() as u64 + 1);
        for r in releases {
            let mut assets = Vec::new();
            let mut files: Vec<(String, Vec<u8>)> = r.zips.iter().map(|(n, b)| (n.to_string(), b.clone())).collect();
            if let Some(m) = &r.manifest {
                files.push(("release.json".into(), serde_json::to_vec(m).unwrap()));
            }
            for (name, body) in files {
                id += 1;
                let url = format!("https://dl.example/{}/{}/{name}", r.repo, r.tag);
                use sha2::{Digest, Sha256};
                let digest: String = Sha256::digest(&body).iter().map(|b| format!("{b:02x}")).collect();
                assets.push(serde_json::json!({
                    "id": id, "name": name, "size": body.len(), "browser_download_url": url,
                    "digest": format!("sha256:{digest}")
                }));
                self.serve(&url, body);
            }
            list.push(serde_json::json!({
                "tag_name": r.tag,
                "html_url": format!("https://github.com/{}/releases/tag/{}", r.repo, r.tag),
                "draft": false, "prerelease": false, "assets": assets
            }));
        }
        let repo = releases[0].repo;
        self.serve_json(&format!("https://api.github.com/repos/{repo}/releases/latest"), list[0].clone());
        let all = serde_json::Value::Array(list);
        self.serve_json(&format!("https://api.github.com/repos/{repo}/releases?per_page=100"), all.clone());
        self.serve_json(&format!("https://api.github.com/repos/{repo}/releases?per_page=100&page=1"), all);
    }
}

/// release.json listing one zip per (filename, flavor, interface).
pub fn manifest(builds: &[(&str, &str, u32)]) -> serde_json::Value {
    serde_json::json!({ "releases": builds.iter().map(|(f, flavor, iface)| serde_json::json!({
        "filename": f, "nolib": false, "metadata": [{ "flavor": flavor, "interface": iface }]
    })).collect::<Vec<_>>() })
}

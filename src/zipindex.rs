//! Lists the addon folders inside a remote zip without downloading it: the
//! zip's central directory (its file index) sits at the end of the file, so
//! two HTTP range requests are enough even for zips of hundreds of MB.

use std::collections::BTreeSet;

use anyhow::{Result, bail};

use crate::http::Http;

const EOCD_SIG: u32 = 0x0605_4b50;
const ZIP64_LOCATOR_SIG: u32 = 0x0706_4b50;
const ZIP64_EOCD_SIG: u32 = 0x0606_4b50;
const CD_ENTRY_SIG: u32 = 0x0201_4b50;
/// End-of-central-directory record (22 bytes) plus the longest allowed comment,
/// plus the Zip64 locator in front of it.
const TAIL: u64 = 22 + 0xffff + 20;
/// Refuse absurd central directories (a corrupt or hostile size field).
const MAX_CD: u64 = 64 * 1024 * 1024;

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}
fn u64_at(b: &[u8], i: usize) -> Option<u64> {
    Some(u64::from_le_bytes(b.get(i..i + 8)?.try_into().ok()?))
}

/// GitHub's download servers reject suffix ranges ("bytes=-N"), so ranges are always explicit.
fn range(http: &dyn Http, url: &str, start: u64, end: u64) -> Result<Vec<u8>> {
    let res = http.get(url, &[("Range", format!("bytes={start}-{end}"))])?;
    if res.status != 206 {
        bail!("Range request not supported ({})", res.status);
    }
    Ok(res.body)
}

/// Names of every entry in the zip at `url`, which is `size` bytes long.
pub fn remote_zip_entries(http: &dyn Http, url: &str, size: u64) -> Result<Vec<String>> {
    if size < 22 {
        bail!("Not a zip file");
    }
    let tail_start = size.saturating_sub(TAIL);
    let tail = range(http, url, tail_start, size - 1)?;

    let eocd = (0..=tail.len().saturating_sub(22)).rev().find(|&i| u32_at(&tail, i) == Some(EOCD_SIG));
    let Some(eocd) = eocd else { bail!("Not a zip file") };
    let corrupt = || anyhow::anyhow!("Corrupt zip directory");

    let mut count = u64::from(u16_at(&tail, eocd + 10).ok_or_else(corrupt)?);
    let mut cd_size = u64::from(u32_at(&tail, eocd + 12).ok_or_else(corrupt)?);
    let mut offset = u64::from(u32_at(&tail, eocd + 16).ok_or_else(corrupt)?);

    // Zip64: the real values live in a separate record pointed to by a locator.
    if (offset == 0xffff_ffff || cd_size == 0xffff_ffff || count == 0xffff)
        && eocd >= 20
        && u32_at(&tail, eocd - 20) == Some(ZIP64_LOCATOR_SIG)
    {
        let record_at = u64_at(&tail, eocd - 20 + 8).ok_or_else(corrupt)?;
        let rec = if record_at >= tail_start {
            tail.get((record_at - tail_start) as usize..).ok_or_else(corrupt)?.to_vec()
        } else {
            range(http, url, record_at, record_at + 55)?
        };
        if u32_at(&rec, 0) == Some(ZIP64_EOCD_SIG) {
            count = u64_at(&rec, 32).ok_or_else(corrupt)?;
            cd_size = u64_at(&rec, 40).ok_or_else(corrupt)?;
            offset = u64_at(&rec, 48).ok_or_else(corrupt)?;
        }
    }
    if cd_size > MAX_CD || offset.saturating_add(cd_size) > size {
        return Err(corrupt());
    }

    let cd = if offset >= tail_start {
        let start = (offset - tail_start) as usize;
        tail.get(start..start + cd_size as usize).ok_or_else(corrupt)?.to_vec()
    } else if cd_size == 0 {
        Vec::new()
    } else {
        range(http, url, offset, offset + cd_size - 1)?
    };
    Ok(central_directory_names(&cd, count))
}

fn central_directory_names(cd: &[u8], count: u64) -> Vec<String> {
    let mut names = Vec::new();
    let mut p = 0usize;
    for _ in 0..count {
        if u32_at(cd, p) != Some(CD_ENTRY_SIG) {
            break;
        }
        let (Some(name_len), Some(extra_len), Some(comment_len)) =
            (u16_at(cd, p + 28), u16_at(cd, p + 30), u16_at(cd, p + 32))
        else {
            break;
        };
        let Some(name) = cd.get(p + 46..p + 46 + name_len as usize) else { break };
        names.push(String::from_utf8_lossy(name).into_owned());
        p += 46 + name_len as usize + extra_len as usize + comment_len as usize;
    }
    names
}

/// Addon folders in a list of zip entries: folders holding a .toc directly,
/// at the shallowest depth found (some zips wrap everything in one folder).
pub fn addon_folders_in(entries: &[String]) -> Vec<String> {
    let mut best: Option<(usize, BTreeSet<String>)> = None;
    for name in entries {
        let name = name.replace('\\', "/");
        let parts: Vec<&str> = name.split('/').filter(|p| !p.is_empty()).collect();
        if parts.len() < 2 || !parts[parts.len() - 1].to_lowercase().ends_with(".toc") {
            continue;
        }
        let depth = parts.len() - 2;
        let folder = parts[parts.len() - 2].to_string();
        match &mut best {
            Some((d, _)) if depth > *d => {}
            Some((d, folders)) if depth == *d => {
                folders.insert(folder);
            }
            _ => best = Some((depth, BTreeSet::from([folder]))),
        }
    }
    best.map(|(_, f)| f.into_iter().collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{FakeHttp, make_zip, many_files};

    fn strings(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn folders_at_shallowest_depth() {
        let e = strings(&[
            "BigWigs/BigWigs.toc",
            "BigWigs/Core.lua",
            "BigWigs_Core/BigWigs_Core.toc",
            "BigWigs/Libs/LibStub/LibStub.toc",
            "README.md",
        ]);
        assert_eq!(addon_folders_in(&e), strings(&["BigWigs", "BigWigs_Core"]));
        let wrapped = strings(&["repo-1.0/Addon/Addon.toc", "repo-1.0/Other/Other.TOC"]);
        assert_eq!(addon_folders_in(&wrapped), strings(&["Addon", "Other"]));
        assert!(addon_folders_in(&strings(&["top.toc"])).is_empty());
    }

    #[test]
    fn reads_remote_index() {
        let names = ["Satchel/", "Satchel/Satchel.toc", "Satchel/Window.lua"];
        let zip = make_zip(&names.iter().map(|n| (*n, b"x".as_slice())).collect::<Vec<_>>(), false);
        let http = FakeHttp::default();
        http.serve("https://dl/s.zip", zip.clone());
        let got = remote_zip_entries(&http, "https://dl/s.zip", zip.len() as u64).unwrap();
        assert_eq!(got, strings(&names));
    }

    #[test]
    fn reads_large_remote_index_with_separate_directory_request() {
        let mut names = vec!["Big/Big.toc".to_string()];
        names.extend(many_files("Big", 1500));
        let zip = make_zip(&names.iter().map(|n| (n.as_str(), b"x".as_slice())).collect::<Vec<_>>(), false);
        let http = FakeHttp::default();
        http.serve("https://dl/big.zip", zip.clone());
        let got = remote_zip_entries(&http, "https://dl/big.zip", zip.len() as u64).unwrap();
        assert_eq!(got, names);
        assert_eq!(http.count("https://dl/big.zip"), 2, "tail, then the directory");
    }

    #[test]
    fn reads_zip64_index() {
        let mut big = vec!["A/A.toc".to_string(), "B/B.toc".to_string()];
        big.extend(many_files("A", 1500));
        for (label, names) in [("small", vec!["A/A.toc".to_string(), "B/B.toc".to_string()]), ("big", big)] {
            let zip = make_zip(&names.iter().map(|n| (n.as_str(), b"x".as_slice())).collect::<Vec<_>>(), true);
            let eocd = zip.len() - 22;
            assert_eq!(u32_at(&zip, eocd - 20), Some(ZIP64_LOCATOR_SIG), "{label}: has a Zip64 locator");
            assert_eq!(u16_at(&zip, eocd + 10), Some(0xffff), "{label}: classic record holds sentinels");
            let real = zip::ZipArchive::new(std::io::Cursor::new(zip.clone())).unwrap();
            assert_eq!(real.len(), names.len(), "{label}: a valid zip to a real reader");

            let http = FakeHttp::default();
            http.serve("https://dl/64.zip", zip.clone());
            let got = remote_zip_entries(&http, "https://dl/64.zip", zip.len() as u64).unwrap();
            assert_eq!(got, names, "{label}");
            assert_eq!(addon_folders_in(&got), strings(&["A", "B"]), "{label}");
            let requests = http.count("https://dl/64.zip");
            assert_eq!(requests, if label == "big" { 2 } else { 1 }, "{label}: range requests");
        }
    }

    #[test]
    fn rejects_non_zip() {
        let http = FakeHttp::default();
        http.serve("https://dl/x", vec![0u8; 100]);
        assert!(remote_zip_entries(&http, "https://dl/x", 100).is_err());
    }
}

//! Refreshes each catalog.json entry from its latest release:
//!  - "flavors" from release.json (BigWigs packager). Entries whose releases
//!    have no release.json keep the flavors written by hand.
//!  - "folders": the addon folders the release installs, primary first, read
//!    from the zip's index without downloading it. Packmule uses these to
//!    recognise addons that were installed by hand.
//!
//!   GITHUB_TOKEN=$(gh auth token) cargo run --example update-catalog
//!
//! A token is optional but avoids the 60 requests/hour limit.

use std::collections::BTreeSet;
use std::sync::Arc;

use anyhow::{Context, Result};
use packmule::github::GitHubClient;
use packmule::http::{Http, UreqHttp};
use packmule::zipindex::{addon_folders_in, remote_zip_entries};
use serde_json::{Value, json};

/// catalog.json in the packmule-catalog checkout: $PACKMULE_CATALOG, else next to this repo.
fn catalog_file() -> std::path::PathBuf {
    std::env::var_os("PACKMULE_CATALOG")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../packmule-catalog"))
        .join("catalog.json")
}

/// Primary folder first: the one most others are named after (BigWigs for
/// BigWigs_Core), else the shortest.
fn order_folders(mut folders: Vec<String>) -> Vec<String> {
    let prefixes = |f: &str, all: &[String]| all.iter().filter(|o| o.as_str() != f && o.starts_with(f)).count();
    let all = folders.clone();
    folders.sort_by(|a, b| prefixes(b, &all).cmp(&prefixes(a, &all)).then(a.len().cmp(&b.len())).then(a.cmp(b)));
    folders
}

fn refresh(entry: &mut Value, gh: &GitHubClient, http: &dyn Http) -> Result<String> {
    let repo = entry["repo"].as_str().context("no repo")?.to_string();
    let prefix = entry.get("tagPrefix").and_then(Value::as_str).map(str::to_string);
    let release = gh.latest_release(&repo, prefix.as_deref())?;

    let zips: Vec<_> = release.assets.iter().filter(|a| a.name.to_lowercase().ends_with(".zip")).collect();
    if let Some(zip) = zips.iter().find(|a| !a.name.to_lowercase().contains("-nolib")).or(zips.first()) {
        let folders = addon_folders_in(&remote_zip_entries(http, &zip.browser_download_url, zip.size)?);
        entry["folders"] = json!(order_folders(folders));
    }
    let manifest = release.assets.iter().find(|a| a.name == "release.json");
    if let Some(m) = manifest {
        let res = http.get(&m.browser_download_url, &[])?;
        let data: Value = serde_json::from_slice(&res.body).context("bad release.json")?;
        let flavors: BTreeSet<&str> = data["releases"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|r| r["metadata"].as_array().into_iter().flatten())
            .filter_map(|m| m["flavor"].as_str())
            .collect();
        entry["flavors"] = json!(flavors);
    }
    let list = |k: &str| {
        entry[k]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "))
            .unwrap_or_default()
    };
    let note = if manifest.is_some() { "" } else { " (no release.json; flavors kept)" };
    Ok(format!("{} [{}]{note} folders: {}", release.tag_name, list("flavors"), list("folders")))
}

fn main() -> Result<()> {
    let http = Arc::new(UreqHttp::new());
    let token = std::env::var("GITHUB_TOKEN").unwrap_or_default();
    let gh = GitHubClient::new(http.clone(), move || token.clone());
    let mut catalog: Value = serde_json::from_str(
        &std::fs::read_to_string(catalog_file())
            .context("clone hammackj/packmule-catalog next to this repo or set PACKMULE_CATALOG")?,
    )?;
    let mut failed = false;
    for entry in catalog["addons"].as_array_mut().context("no addons list")? {
        let id = entry["id"].as_str().unwrap_or("?").to_string();
        match refresh(entry, &gh, http.as_ref()) {
            Ok(line) => println!("{id}: {line}"),
            Err(e) => {
                println!("{id}: ERROR {e:#}");
                failed = true;
            }
        }
    }
    std::fs::write(catalog_file(), serde_json::to_string_pretty(&catalog)? + "\n")?;
    if failed {
        std::process::exit(1);
    }
    Ok(())
}

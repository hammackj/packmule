//! Game clients inside a World of Warcraft folder.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// A game client found inside the WoW folder (`_retail_`, `_forever_`, ...).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GameInstall {
    pub dir: String,
    pub product: String,
    pub version: String,
    /// Interface number derived from the version, e.g. 1.60.1 -> 16001.
    pub interface: u32,
}

/// Reads a Blizzard "table" file (`.build.info` / `.flavor.info`): a header row
/// of `Name!TYPE:size` columns separated by `|`, followed by data rows.
pub fn parse_blizzard_table(text: &str) -> Vec<HashMap<String, String>> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let Some(header) = lines.next() else { return Vec::new() };
    let headers: Vec<&str> = header.split('|').map(|h| h.split('!').next().unwrap_or("")).collect();
    lines
        .map(|line| {
            let cells: Vec<&str> = line.split('|').collect();
            headers.iter().enumerate().map(|(i, h)| (h.to_string(), cells.get(i).unwrap_or(&"").to_string())).collect()
        })
        .collect()
}

/// "1.60.1.70009" -> 16001, "12.1.5.12345" -> 120105; 0 if unparseable.
pub fn version_to_interface(version: &str) -> u32 {
    let nums: Vec<Option<u32>> = version.split('.').take(3).map(|n| n.trim().parse().ok()).collect();
    match nums.as_slice() {
        [Some(major), Some(minor), Some(patch)] => major * 10000 + minor * 100 + patch,
        _ => 0,
    }
}

/// Finds game clients (folders named `_something_`) inside a WoW folder.
pub fn detect_installs(wow_path: &Path) -> Result<Vec<GameInstall>> {
    let builds = fs::read_to_string(wow_path.join(".build.info")).map(|t| parse_blizzard_table(&t)).unwrap_or_default();

    let mut installs = Vec::new();
    for entry in fs::read_dir(wow_path)? {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else { continue };
        if !entry.file_type()?.is_dir() || name.len() < 3 || !name.starts_with('_') || !name.ends_with('_') {
            continue;
        }
        let product = fs::read_to_string(entry.path().join(".flavor.info"))
            .ok()
            .and_then(|t| parse_blizzard_table(&t).into_iter().next())
            .and_then(|row| row.get("Product Flavor").cloned())
            .unwrap_or_default();
        let version = builds
            .iter()
            .find(|b| b.get("Product") == Some(&product))
            .and_then(|b| b.get("Version").cloned())
            .unwrap_or_default();
        installs.push(GameInstall { interface: version_to_interface(&version), dir: name, product, version });
    }
    installs.sort_by(|a, b| a.dir.cmp(&b.dir));
    Ok(installs)
}

pub fn addons_dir(wow_path: &Path, install_dir: &str) -> PathBuf {
    wow_path.join(install_dir).join("Interface").join("AddOns")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_numbers() {
        assert_eq!(version_to_interface("1.60.1.70009"), 16001);
        assert_eq!(version_to_interface("12.1.5.12345"), 120105);
        assert_eq!(version_to_interface("1.15.7"), 11507);
        assert_eq!(version_to_interface(""), 0);
        assert_eq!(version_to_interface("x.1.2"), 0);
    }

    #[test]
    fn detects_clients() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(
            root.join(".build.info"),
            "Branch!STRING:0|Active!DEC:1|Version!STRING:0|Product!STRING:0\n\
             us|1|1.60.1.70009|wowf\n\
             us|1|1.15.7.61582|wow_classic_era\n",
        )
        .unwrap();
        for (d, p) in [("_forever_", "wowf"), ("_classic_era_", "wow_classic_era")] {
            fs::create_dir(root.join(d)).unwrap();
            fs::write(root.join(d).join(".flavor.info"), format!("Product Flavor!STRING:0\n{p}\n")).unwrap();
        }
        fs::create_dir(root.join("Data")).unwrap();
        fs::create_dir(root.join("_noinfo_")).unwrap();

        let found = detect_installs(root).unwrap();
        let summary: Vec<_> = found.iter().map(|i| (i.dir.as_str(), i.product.as_str(), i.interface)).collect();
        assert_eq!(
            summary,
            [("_classic_era_", "wow_classic_era", 11507), ("_forever_", "wowf", 16001), ("_noinfo_", "", 0)]
        );
    }
}

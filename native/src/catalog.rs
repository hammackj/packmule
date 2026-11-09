//! The Browse tab's addon list (catalog.json).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::github::tag_stream;
use crate::http::Http;
use crate::store::AddonRecord;

/// The catalog shipped with the app is a fallback: the copy in the GitHub repo
/// is fetched at startup so new addons appear without an app release.
pub const REMOTE_URL: &str = "https://raw.githubusercontent.com/hammackj/packmule/master/catalog.json";
const BUNDLED: &str = include_str!("../../catalog.json");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    /// Stable id, e.g. "bigwigs".
    pub id: String,
    pub name: String,
    /// GitHub "owner/repo".
    pub repo: String,
    /// Release stream for repos that publish several products ("" = unprefixed tags).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag_prefix: Option<String>,
    /// Category, using the in-game AddOns list names where one exists.
    pub category: String,
    pub description: String,
    /// Game flavors the releases have builds for, as named in the packager's
    /// release.json ("forever", "mainline", "classic", "mists", ...).
    pub flavors: Vec<String>,
    /// Addon folders the release installs, primary first.
    #[serde(default)]
    pub folders: Vec<String>,
    /// Other catalog ids this addon needs.
    #[serde(default)]
    pub requires: Vec<String>,
}

fn valid_repo(repo: &str) -> bool {
    let ok = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c));
    matches!(repo.split_once('/'), Some((o, n)) if ok(o) && ok(n))
}

/// Entries of a catalog document; malformed entries are skipped, not fatal.
pub fn parse(data: &Value) -> Vec<CatalogEntry> {
    let Some(list) = data.get("addons").and_then(Value::as_array) else { return Vec::new() };
    list.iter()
        .filter_map(|e| serde_json::from_value::<CatalogEntry>(e.clone()).ok())
        .filter(|e| valid_repo(&e.repo))
        .collect()
}

pub struct Catalog {
    entries: Vec<CatalogEntry>,
}

impl Default for Catalog {
    fn default() -> Self {
        Self { entries: parse(&serde_json::from_str(BUNDLED).unwrap_or(Value::Null)) }
    }
}

impl Catalog {
    pub fn from_entries(entries: Vec<CatalogEntry>) -> Self {
        Self { entries }
    }

    pub fn entries(&self) -> &[CatalogEntry] {
        &self.entries
    }

    pub fn entry(&self, id: &str) -> Option<&CatalogEntry> {
        self.entries.iter().find(|e| e.id == id)
    }

    /// Replaces the bundled catalog with the published one; keeps the old list
    /// on any failure (offline, or not published yet). True if it changed.
    pub fn refresh(&mut self, http: &dyn Http) -> bool {
        let Ok(res) = http.get(REMOTE_URL, &[]) else { return false };
        if res.status != 200 {
            return false;
        }
        let Ok(doc) = serde_json::from_slice::<Value>(&res.body) else { return false };
        let remote = parse(&doc);
        if remote.is_empty() || remote == self.entries {
            return false;
        }
        self.entries = remote;
        true
    }
}

/// The packager's flavor name for a game client's product code (from
/// .flavor.info): wowf = WoW Forever, wow = retail, and so on.
pub fn product_flavor(product: &str) -> Option<&'static str> {
    if product.starts_with("wowf") {
        Some("forever")
    } else if product.starts_with("wow_classic_era") {
        Some("classic")
    } else if product.starts_with("wow_anniversary") {
        Some("bcc")
    } else if product.starts_with("wow_classic") {
        Some("mists")
    } else if matches!(product, "wow" | "wowt" | "wowxptr" | "wow_beta") {
        Some("mainline")
    } else {
        None
    }
}

/// The release stream a record follows ("" for unprefixed tags).
fn record_stream(r: &AddonRecord) -> Option<&str> {
    r.tag_prefix.as_deref().or_else(|| r.installed_tag.as_deref().map(tag_stream))
}

/// Whether a managed addon is this catalog entry (same repo and release stream).
pub fn matches_entry(r: &AddonRecord, e: &CatalogEntry) -> bool {
    if !r.repo.eq_ignore_ascii_case(&e.repo) {
        return false;
    }
    record_stream(r).is_none_or(|s| s == e.tag_prefix.as_deref().unwrap_or(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_catalog_parses() {
        let c = Catalog::default();
        assert!(c.entries().len() >= 20, "only {} entries", c.entries().len());
        let spoken = c.entry("spoken").unwrap();
        assert_eq!(spoken.tag_prefix.as_deref(), Some("spoken/"));
        assert_eq!(c.entry("satchel").unwrap().folders.first().map(String::as_str), Some("Satchel"));
    }

    #[test]
    fn skips_bad_entries() {
        let doc = serde_json::json!({ "addons": [
            { "id": "a", "name": "A", "repo": "o/a", "category": "X", "description": "", "flavors": [] },
            { "id": "b", "name": "B", "repo": "not a repo", "category": "X", "description": "", "flavors": [] },
            { "id": "c", "name": "C" }
        ]});
        let ids: Vec<_> = parse(&doc).into_iter().map(|e| e.id).collect();
        assert_eq!(ids, ["a"]);
    }

    #[test]
    fn flavors() {
        assert_eq!(product_flavor("wowf"), Some("forever"));
        assert_eq!(product_flavor("wow_classic_era"), Some("classic"));
        assert_eq!(product_flavor("wow_classic_era_ptr"), Some("classic"));
        assert_eq!(product_flavor("wow_anniversary"), Some("bcc"));
        assert_eq!(product_flavor("wow_classic"), Some("mists"));
        assert_eq!(product_flavor("wow"), Some("mainline"));
        assert_eq!(product_flavor("wowdev"), None);
    }

    #[test]
    fn matching_follows_streams() {
        let c = Catalog::default();
        let (spoken, quests) = (c.entry("spoken").unwrap(), c.entry("spoken-quests").unwrap());
        let mut r = AddonRecord::new(&spoken.repo.to_uppercase(), None, None);
        r.installed_tag = Some("quests-audio/v2.5.0".into());
        assert!(matches_entry(&r, quests));
        assert!(!matches_entry(&r, spoken));
        r.tag_prefix = Some("spoken/".into());
        assert!(matches_entry(&r, spoken));
        let fresh = AddonRecord::new(&spoken.repo, None, None);
        assert!(matches_entry(&fresh, spoken) && matches_entry(&fresh, quests));
    }
}

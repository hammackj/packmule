//! Reading an addon folder's `.toc` file.

use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TocInfo {
    pub title: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    /// "## Category" (11.1+ TOCs): the group the in-game AddOns list shows it under.
    pub category: Option<String>,
    /// A GitHub "owner/repo" named in the .toc's website fields, if any.
    pub github_repo: Option<String>,
}

/// .toc fields authors use for their project page.
const LINK_FIELDS: &[&str] = &["X-Website", "X-Github", "X-GitHub-Repo", "X-Repository", "X-Source", "X-URL", "URL"];

static GITHUB_LINK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)github\.com/([\w.-]+)/([\w.-]+?)(?:\.git)?(?:[/?#]|$)").unwrap());
static COLOR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\|c[0-9a-f]{8}").unwrap());
static FIELD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^##\s*([^:]+?)\s*:\s*(.+)$").unwrap());

/// "owner/repo" from a github.com link; profile links and other sites give None.
pub fn github_repo_from_url(text: &str) -> Option<String> {
    GITHUB_LINK.captures(text).map(|c| format!("{}/{}", &c[1], &c[2]))
}

/// Per-client .toc suffixes, most specific first (e.g. Questie_Forever.toc).
fn toc_suffixes(product: &str) -> &'static [&'static str] {
    match product {
        "wowf" => &["Forever"],
        "wow" => &["Mainline"],
        "wow_classic_era" => &["Vanilla", "Classic"],
        "wow_classic" => &["Mists", "Classic"],
        _ => &[],
    }
}

/// Removes WoW UI escape codes: |cAARRGGBB colors, |r resets, |n newlines.
fn strip_escapes(s: &str) -> String {
    COLOR.replace_all(s, "").replace("|r", "").replace("|n", "").trim().to_string()
}

/// Reads Title/Version/Author/Category and a GitHub link from an addon folder's
/// .toc, preferring the file for this client's flavor, then the plain
/// `<Folder>.toc`, then any .toc.
pub fn read_toc_info(addon_dir: &Path, product: &str) -> TocInfo {
    let mut files: Vec<String> = fs::read_dir(addon_dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok()?.file_name().into_string().ok())
                .filter(|f| f.to_lowercase().ends_with(".toc"))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    let folder = addon_dir.file_name().and_then(|f| f.to_str()).unwrap_or("").to_lowercase();
    let by_name = |name: &str| files.iter().find(|f| f.to_lowercase() == name).cloned();
    let candidates = toc_suffixes(product)
        .iter()
        .flat_map(|s| {
            let s = s.to_lowercase();
            [format!("{folder}_{s}.toc"), format!("{folder}-{s}.toc")]
        })
        .chain([format!("{folder}.toc")]);
    let Some(file) = candidates.filter_map(|n| by_name(&n)).next().or_else(|| files.first().cloned()) else {
        return TocInfo::default();
    };

    let bytes = fs::read(addon_dir.join(&file)).unwrap_or_default();
    let text = String::from_utf8_lossy(&bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    parse_toc(text)
}

pub fn parse_toc(text: &str) -> TocInfo {
    let field = |name: &str| -> Option<String> {
        let value = text.lines().find_map(|line| {
            let c = FIELD.captures(line.trim_end_matches('\r'))?;
            c[1].eq_ignore_ascii_case(name).then(|| strip_escapes(&c[2]))
        })?;
        // Unreplaced packager tokens like @project-version@ aren't useful to show.
        let token = value.len() > 2 && value.starts_with('@') && value.ends_with('@');
        (!value.is_empty() && !token).then_some(value)
    };
    TocInfo {
        title: field("Title"),
        version: field("Version"),
        author: field("Author"),
        category: field("Category"),
        github_repo: LINK_FIELDS.iter().filter_map(|f| field(f)).find_map(|v| github_repo_from_url(&v)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_links() {
        assert_eq!(github_repo_from_url("https://github.com/hammackj/satchel"), Some("hammackj/satchel".into()));
        assert_eq!(github_repo_from_url("https://github.com/a/b.git"), Some("a/b".into()));
        assert_eq!(github_repo_from_url("github.com/A/b-c.d/releases"), Some("A/b-c.d".into()));
        assert_eq!(github_repo_from_url("https://github.com/hammackj"), None);
        assert_eq!(github_repo_from_url("https://www.curseforge.com/wow/addons/x"), None);
    }

    #[test]
    fn parses_fields() {
        let toc = parse_toc(
            "## Interface: 16001\r\n## Title: |cff00ff00Satchel|r Bags\r\n## Version: @project-version@\r\n\
             ## Author: hammackj\n## Category: Inventory\n## X-Website: https://github.com/hammackj/satchel\n",
        );
        assert_eq!(toc.title.as_deref(), Some("Satchel Bags"));
        assert_eq!(toc.version, None);
        assert_eq!(toc.author.as_deref(), Some("hammackj"));
        assert_eq!(toc.category.as_deref(), Some("Inventory"));
        assert_eq!(toc.github_repo.as_deref(), Some("hammackj/satchel"));
    }

    #[test]
    fn prefers_flavor_toc() {
        let dir = tempfile::tempdir().unwrap();
        let addon = dir.path().join("Questie");
        fs::create_dir(&addon).unwrap();
        fs::write(addon.join("Questie.toc"), "## Version: plain\n").unwrap();
        fs::write(addon.join("Questie_Forever.toc"), "## Version: forever\n").unwrap();
        fs::write(addon.join("Questie-Classic.toc"), "## Version: classic\n").unwrap();
        assert_eq!(read_toc_info(&addon, "wowf").version.as_deref(), Some("forever"));
        assert_eq!(read_toc_info(&addon, "wow_classic_era").version.as_deref(), Some("classic"));
        assert_eq!(read_toc_info(&addon, "wow").version.as_deref(), Some("plain"));
        assert_eq!(read_toc_info(&dir.path().join("Missing"), "wow"), TocInfo::default());
    }
}

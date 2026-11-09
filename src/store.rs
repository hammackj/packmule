//! Persistent state: `~/.config/packmule/state.json`, in the same format the
//! Electron app writes, so either app can run on the same data.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Root of the WoW install, e.g. ".../World of Warcraft".
    pub wow_path: String,
    /// Game client folder inside wow_path, e.g. "_forever_".
    pub active_install: String,
    /// Hours between automatic update checks; 0 disables the timer.
    pub check_interval_hours: f64,
    /// Install updates automatically after a check finds them.
    pub auto_update: bool,
    /// Fields this version doesn't know, kept as they were.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            wow_path: String::new(),
            active_install: String::new(),
            check_interval_hours: 6.0,
            auto_update: false,
            extra: Map::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddonRecord {
    /// GitHub "owner/repo".
    pub repo: String,
    /// Follow releases whose tag has this prefix (e.g. "spoken/"; "" = tags
    /// without one), for repos that publish several products. None means
    /// GitHub's /releases/latest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag_prefix: Option<String>,
    /// Display name, when the source provides one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub installed_tag: Option<String>,
    #[serde(default)]
    pub installed_asset_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_asset_id: Option<u64>,
    /// Top-level AddOns folders this addon owns.
    #[serde(default)]
    pub folders: Vec<String>,
    #[serde(default)]
    pub installed_at: Option<String>,
    #[serde(default)]
    pub latest_tag: Option<String>,
    #[serde(default)]
    pub latest_asset_name: Option<String>,
    /// SHA-256 of the installed and the latest zip, when GitHub gave one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_sha256: Option<String>,
    #[serde(default)]
    pub last_checked: Option<String>,
    /// Set when the latest release has no build listed for this client's interface.
    #[serde(default)]
    pub compat_warning: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl AddonRecord {
    pub fn new(repo: &str, tag_prefix: Option<&str>, name: Option<&str>) -> Self {
        Self {
            repo: repo.to_string(),
            tag_prefix: tag_prefix.map(str::to_string),
            name: name.map(str::to_string),
            installed_tag: None,
            installed_asset_id: None,
            latest_asset_id: None,
            folders: Vec::new(),
            installed_at: None,
            latest_tag: None,
            latest_asset_name: None,
            installed_sha256: None,
            latest_sha256: None,
            last_checked: None,
            compat_warning: None,
            error: None,
            extra: Map::new(),
        }
    }

    /// Unique id of a managed addon within a client.
    pub fn key(&self) -> String {
        addon_key(&self.repo, self.tag_prefix.as_deref())
    }
}

pub fn addon_key(repo: &str, tag_prefix: Option<&str>) -> String {
    match tag_prefix {
        Some(p) if !p.is_empty() => format!("{repo}#{p}"),
        _ => repo.to_string(),
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct StoredData {
    settings: Settings,
    /// The Electron app's token: encrypted by Chromium (unreadable here) or
    /// "plain:<token>". Packmule native keeps tokens in the system keyring.
    token: String,
    /// Managed addons per game client folder ("_retail_", "_forever_", ...).
    addons: BTreeMap<String, Vec<AddonRecord>>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

/// Where the GitHub token lives.
pub trait SecretStore: Send {
    fn get(&self) -> Option<String>;
    /// Stores the token (empty = delete); false if no keyring is available.
    fn set(&self, token: &str) -> bool;
}

/// The desktop keyring (Secret Service: GNOME Keyring, KWallet...).
pub struct Keyring;

impl Keyring {
    fn entry() -> Option<keyring::Entry> {
        keyring::Entry::new("packmule", "github-token").ok()
    }
}

impl SecretStore for Keyring {
    fn get(&self) -> Option<String> {
        Self::entry()?.get_password().ok().filter(|t| !t.is_empty())
    }

    fn set(&self, token: &str) -> bool {
        let Some(entry) = Self::entry() else { return false };
        if token.is_empty() {
            return matches!(entry.delete_credential(), Ok(()) | Err(keyring::Error::NoEntry));
        }
        entry.set_password(token).is_ok()
    }
}

/// For tests, and machines without a keyring when no fallback is wanted.
#[derive(Default)]
pub struct MemorySecret(std::sync::Mutex<Option<String>>);

impl SecretStore for MemorySecret {
    fn get(&self) -> Option<String> {
        self.0.lock().unwrap().clone()
    }
    fn set(&self, token: &str) -> bool {
        *self.0.lock().unwrap() = (!token.is_empty()).then(|| token.to_string());
        true
    }
}

pub struct Store {
    file: PathBuf,
    data: StoredData,
    secrets: Box<dyn SecretStore>,
    /// Cached so the keyring isn't asked on every redraw.
    token: Option<String>,
}

impl Store {
    /// Loads `file`; a missing or unreadable file gives the defaults.
    pub fn load(file: &Path, secrets: Box<dyn SecretStore>) -> Self {
        let data =
            fs::read_to_string(file).ok().and_then(|t| serde_json::from_str::<StoredData>(&t).ok()).unwrap_or_default();
        Self { file: file.to_path_buf(), data, secrets, token: None }
    }

    fn save(&self) -> Result<()> {
        if let Some(dir) = self.file.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = self.file.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(&self.data)?)?;
        fs::rename(&tmp, &self.file).with_context(|| format!("Could not save {}", self.file.display()))
    }

    pub fn settings(&self) -> &Settings {
        &self.data.settings
    }

    pub fn set_settings(&mut self, f: impl FnOnce(&mut Settings)) -> Result<()> {
        f(&mut self.data.settings);
        self.save()
    }

    pub fn token(&mut self) -> String {
        if self.token.is_none() {
            let from_electron = self.data.token.strip_prefix("plain:").map(str::to_string);
            self.token = Some(self.secrets.get().or(from_electron).unwrap_or_default());
        }
        self.token.clone().unwrap_or_default()
    }

    /// Saves the token in the keyring, or as plain text in state.json when
    /// there is no keyring (as the Electron app did).
    pub fn set_token(&mut self, token: &str) -> Result<()> {
        let token = token.trim();
        let in_keyring = self.secrets.set(token);
        self.data.token = if token.is_empty() || in_keyring { String::new() } else { format!("plain:{token}") };
        self.token = Some(token.to_string());
        self.save()
    }

    pub fn addons(&self, install: &str) -> &[AddonRecord] {
        self.data.addons.get(install).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn addons_mut(&mut self, install: &str) -> &mut Vec<AddonRecord> {
        self.data.addons.entry(install.to_string()).or_default()
    }

    /// Writes the current addon lists to disk.
    pub fn persist(&self) -> Result<()> {
        self.save()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ELECTRON_STATE: &str = r#"{
      "settings": { "wowPath": "/games/wow", "activeInstall": "_forever_", "checkIntervalHours": 6, "autoUpdate": false },
      "token": "",
      "curseforgeKey": "",
      "addons": { "_forever_": [
        { "repo": "ATTWoWAddon/AllTheThings", "installedTag": "5.4.8", "installedAssetId": 724415449,
          "folders": ["AllTheThings"], "installedAt": "2026-11-08T21:16:56.455Z", "latestTag": "5.4.8",
          "latestAssetName": "AllTheThings-5.4.8.zip", "lastChecked": "2026-11-09T13:17:15.224Z",
          "compatWarning": null, "error": null, "latestAssetId": 724415449 },
        { "repo": "o/spoken", "tagPrefix": "spoken/", "name": "Spoken", "installedTag": null,
          "installedAssetId": null, "folders": [], "installedAt": null, "latestTag": null,
          "latestAssetName": null, "lastChecked": null, "compatWarning": null, "error": null }
      ] }
    }"#;

    #[test]
    fn reads_and_round_trips_electron_state() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("state.json");
        fs::write(&file, ELECTRON_STATE).unwrap();

        let mut store = Store::load(&file, Box::new(MemorySecret::default()));
        assert_eq!(store.settings().active_install, "_forever_");
        let list = store.addons("_forever_");
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].installed_asset_id, Some(724415449));
        assert_eq!(list[1].key(), "o/spoken#spoken/");
        assert_eq!(store.token(), "");

        store.set_settings(|s| s.auto_update = true).unwrap();
        let before: Value = serde_json::from_str(ELECTRON_STATE).unwrap();
        let mut after: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(after["settings"]["autoUpdate"], true);
        after["settings"]["autoUpdate"] = false.into();
        assert_eq!(after["settings"]["checkIntervalHours"], 6.0);
        after["settings"]["checkIntervalHours"] = 6.into();
        assert_eq!(before, after, "everything else, including unknown fields, is unchanged");
    }

    #[test]
    fn tokens() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("state.json");
        fs::write(&file, r#"{"token":"plain:abc"}"#).unwrap();
        let mut store = Store::load(&file, Box::new(MemorySecret::default()));
        assert_eq!(store.token(), "abc", "reads the Electron app's plain-text token");

        store.set_token(" xyz ").unwrap();
        assert_eq!(store.token(), "xyz");
        assert!(!fs::read_to_string(&file).unwrap().contains("xyz"), "kept out of state.json");

        struct NoKeyring;
        impl SecretStore for NoKeyring {
            fn get(&self) -> Option<String> {
                None
            }
            fn set(&self, _: &str) -> bool {
                false
            }
        }
        let mut store = Store::load(&file, Box::new(NoKeyring));
        store.set_token("fallback").unwrap();
        assert!(fs::read_to_string(&file).unwrap().contains("plain:fallback"));
        assert_eq!(Store::load(&file, Box::new(NoKeyring)).token(), "fallback");
    }

    #[test]
    fn missing_file_gives_defaults() {
        let store = Store::load(Path::new("/nonexistent/state.json"), Box::new(MemorySecret::default()));
        assert_eq!(store.settings().check_interval_hours, 6.0);
        assert!(store.addons("_retail_").is_empty());
    }
}

//! The addon manager: statuses, installs, update checks and adopting
//! hand-installed addons. It runs on the worker thread and is driven one
//! command at a time, so installs never overlap.

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result, anyhow, bail};

use crate::catalog::{Catalog, CatalogEntry, ReviewStatus, matches_entry, product_flavor};
use crate::game::{GameInstall, addons_dir, detect_installs};
use crate::github::{GitHubClient, RateLimit, ReleaseAsset, parse_repo, tag_stream};
use crate::http::Http;
use crate::installer::{install_asset, list_addon_folders, remove_folders, verify_folders};
use crate::selfupdate;
use crate::store::{AddonRecord, Settings, Store, addon_key};
use crate::toc::read_toc_info;
use crate::version::newer_version;
use crate::zipindex::{addon_folders_in, remote_zip_entries};

pub const APP_REPO: &str = "hammackj/packmule";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddonStatus {
    NotInstalled,
    UpToDate,
    UpdateAvailable,
    MissingFiles,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct AddonView {
    pub record: AddonRecord,
    pub key: String,
    /// The record's name, else the catalog's, else the repo.
    pub name: String,
    pub status: AddonStatus,
    pub busy: bool,
    /// From the addon's .toc "## Category", else the catalog.
    pub category: Option<String>,
    /// Catalog addons only: the source check, and the review status of the
    /// installed build and of the latest build.
    pub verified: bool,
    pub installed_review: Option<ReviewStatus>,
    pub latest_review: Option<ReviewStatus>,
}

/// An AddOns folder Packmule didn't install; never modified.
#[derive(Debug, Clone)]
pub struct ManualAddon {
    pub folder: String,
    pub title: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub category: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CatalogView {
    pub entry: CatalogEntry,
    pub installed: bool,
    /// Has a build for the active game client.
    pub available: bool,
    pub busy: bool,
    /// Review status of the installed build, or of the newest reviewed release.
    pub review: ReviewStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppUpdate {
    pub version: String,
    /// The release page.
    pub url: String,
    /// Packmule can install it itself (a .tar.gz install it can write to).
    pub can_install: bool,
    /// Installed; takes effect when Packmule restarts.
    pub installed: bool,
    pub installing: bool,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub settings: Settings,
    pub has_token: bool,
    pub installs: Vec<GameInstall>,
    pub active_install: Option<GameInstall>,
    pub addons: Vec<AddonView>,
    /// Folders in AddOns not owned by any managed addon.
    pub unmanaged: Vec<ManualAddon>,
    pub catalog: Vec<CatalogView>,
    /// Flavor of the active client ("forever", "mainline", ...).
    pub flavor: Option<String>,
    pub app_version: String,
    pub app_update: Option<AppUpdate>,
    /// GitHub API allowance as of the last request (None until one is made).
    pub rate_limit: Option<RateLimit>,
    pub checking: bool,
    pub last_check: Option<String>,
    pub path_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Error,
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub time: String,
    pub level: LogLevel,
    pub message: String,
}

/// What the manager tells the interface.
#[derive(Debug, Clone)]
pub enum Event {
    State(Box<AppState>),
    Log(LogEntry),
    /// A command failed; shown to the user.
    Failed(String),
    /// The worker finished a command sent by the interface.
    CommandDone,
}

/// A settings change; None leaves a field alone.
#[derive(Debug, Clone, Default)]
pub struct SettingsUpdate {
    pub wow_path: Option<String>,
    pub active_install: Option<String>,
    pub github_token: Option<String>,
    pub check_interval_hours: Option<f64>,
    pub auto_update: Option<bool>,
}

pub fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// "v1.2.3" / "spoken/v1.2.3+abc" / "1.2.3" -> "1.2.3", for comparing a .toc version to a tag.
fn bare_version(v: &str) -> String {
    let v = &v[tag_stream(v).len()..];
    let v = v.strip_prefix(['v', 'V']).unwrap_or(v);
    v.split('+').next().unwrap_or("").trim().to_lowercase()
}

pub struct Manager {
    store: Store,
    http: Arc<dyn Http>,
    github: GitHubClient,
    token: Arc<Mutex<String>>,
    catalog: Catalog,
    app_version: String,
    emit: Box<dyn Fn(Event) + Send>,
    installs: Vec<GameInstall>,
    path_error: Option<String>,
    busy: HashSet<String>,
    checking: bool,
    last_check: Option<String>,
    app_update: Option<AppUpdate>,
    /// The new release's tarball, for installing it.
    app_tarball: Option<ReleaseAsset>,
    /// The running binary, when it may be replaced in place (see selfupdate).
    self_update_target: Option<PathBuf>,
    /// "folder>owner/repo" pairs from .toc links already checked this session.
    link_checked: HashSet<String>,
}

impl Manager {
    pub fn new(
        mut store: Store,
        http: Arc<dyn Http>,
        catalog: Catalog,
        app_version: &str,
        emit: impl Fn(Event) + Send + 'static,
    ) -> Self {
        let token = Arc::new(Mutex::new(store.token()));
        let shared = token.clone();
        let github = GitHubClient::new(http.clone(), move || shared.lock().unwrap().clone());
        let mut m = Self {
            store,
            http,
            github,
            token,
            catalog,
            app_version: app_version.to_string(),
            emit: Box::new(emit),
            installs: Vec::new(),
            path_error: None,
            busy: HashSet::new(),
            checking: false,
            last_check: None,
            app_update: None,
            app_tarball: None,
            self_update_target: None,
            link_checked: HashSet::new(),
        };
        m.refresh_installs();
        m
    }

    // ---- small helpers ----

    fn log(&self, level: LogLevel, message: impl Into<String>) {
        (self.emit)(Event::Log(LogEntry { time: now(), level, message: message.into() }));
    }

    /// Sends an event to the interface.
    pub fn emit(&self, event: Event) {
        (self.emit)(event);
    }

    /// Sends the current state to the interface.
    pub fn changed(&mut self) {
        let state = self.state();
        (self.emit)(Event::State(Box::new(state)));
    }

    fn active_install(&self) -> Option<&GameInstall> {
        self.installs.iter().find(|i| i.dir == self.store.settings().active_install)
    }

    fn require_install(&self) -> Result<GameInstall> {
        self.active_install()
            .cloned()
            .ok_or_else(|| anyhow!(self.path_error.clone().unwrap_or_else(|| "No game client selected".into())))
    }

    fn addons_path(&self) -> PathBuf {
        let s = self.store.settings();
        addons_dir(Path::new(&s.wow_path), &s.active_install)
    }

    /// The AddOns folder of the active client, if there is one.
    pub fn current_addons_path(&self) -> Option<PathBuf> {
        self.active_install().map(|_| self.addons_path())
    }

    /// Hours between automatic update checks; 0 = never.
    pub fn check_interval_hours(&self) -> f64 {
        self.store.settings().check_interval_hours
    }

    pub fn has_active_install(&self) -> bool {
        self.active_install().is_some()
    }

    fn active_dir(&self) -> String {
        self.store.settings().active_install.clone()
    }

    fn records(&self) -> &[AddonRecord] {
        self.store.addons(&self.store.settings().active_install)
    }

    pub(crate) fn records_mut(&mut self) -> &mut Vec<AddonRecord> {
        let dir = self.active_dir();
        self.store.addons_mut(&dir)
    }

    fn record(&self, key: &str) -> Result<AddonRecord> {
        self.records().iter().find(|r| r.key() == key).cloned().ok_or_else(|| anyhow!("{key} is not managed"))
    }

    /// Writes a changed record back (matched by key) and saves.
    fn put(&mut self, record: AddonRecord) -> Result<()> {
        let key = record.key();
        let list = self.records_mut();
        match list.iter_mut().find(|r| r.key() == key) {
            Some(slot) => *slot = record,
            None => list.push(record),
        }
        self.store.persist()
    }

    fn refresh_installs(&mut self) {
        self.installs.clear();
        self.path_error = None;
        let s = self.store.settings().clone();
        if s.wow_path.is_empty() {
            self.path_error = Some("Set your World of Warcraft folder in Settings".into());
            return;
        }
        match detect_installs(Path::new(&s.wow_path)) {
            Ok(found) => self.installs = found,
            Err(e) => {
                self.path_error = Some(format!("Cannot read {}: {e}", s.wow_path));
                return;
            }
        }
        if self.installs.is_empty() {
            self.path_error = Some("No game clients (_retail_, _classic_, ...) found in that folder".into());
        } else if !self.installs.iter().any(|i| i.dir == s.active_install) {
            let first = self.installs[0].dir.clone();
            let _ = self.store.set_settings(|s| s.active_install = first);
        }
    }

    fn status(&self, r: &AddonRecord) -> AddonStatus {
        if r.installed_tag.is_none() {
            AddonStatus::NotInstalled
        } else if !verify_folders(&self.addons_path(), &r.folders) {
            AddonStatus::MissingFiles
        } else if r.latest_tag.is_some() && r.latest_tag != r.installed_tag {
            AddonStatus::UpdateAvailable
        } else if r.latest_tag.is_some() {
            AddonStatus::UpToDate
        } else {
            AddonStatus::Unknown
        }
    }

    // ---- state ----

    pub fn state(&mut self) -> AppState {
        let install = self.active_install().cloned();
        let addons_path = self.addons_path();
        let mut addons = Vec::new();
        let mut owned = HashSet::new();
        if let Some(install) = &install {
            for r in self.records() {
                owned.extend(r.folders.iter().cloned());
                let toc = r.folders.first().map(|f| read_toc_info(&addons_path.join(f), &install.product));
                let from_catalog = self.catalog.entries().iter().find(|e| matches_entry(r, e));
                addons.push(AddonView {
                    key: r.key(),
                    name: r
                        .name
                        .clone()
                        .or_else(|| from_catalog.map(|e| e.name.clone()))
                        .unwrap_or_else(|| r.repo.clone()),
                    status: self.status(r),
                    busy: self.busy.contains(&r.key()),
                    category: toc.and_then(|t| t.category).or_else(|| from_catalog.map(|e| e.category.clone())),
                    verified: from_catalog.is_some_and(|e| e.verified.is_some()),
                    installed_review: from_catalog
                        .filter(|_| r.installed_tag.is_some())
                        .map(|e| e.review_status(r.installed_sha256.as_deref())),
                    latest_review: from_catalog
                        .filter(|_| r.latest_tag.is_some())
                        .map(|e| e.review_status(r.latest_sha256.as_deref())),
                    record: r.clone(),
                });
            }
        }

        let mut unmanaged = Vec::new();
        if let Some(install) = &install {
            for folder in list_addon_folders(&addons_path) {
                if owned.contains(&folder) || folder.starts_with('.') {
                    continue;
                }
                let toc = read_toc_info(&addons_path.join(&folder), &install.product);
                unmanaged.push(ManualAddon {
                    folder,
                    title: toc.title,
                    version: toc.version,
                    author: toc.author,
                    category: toc.category,
                });
            }
        }

        let flavor = install.as_ref().and_then(|i| product_flavor(&i.product));
        let catalog = self
            .catalog
            .entries()
            .iter()
            .map(|e| {
                let record = self.records().iter().find(|r| matches_entry(r, e));
                CatalogView {
                    installed: install.is_some() && record.is_some_and(|r| r.installed_tag.is_some()),
                    available: flavor.is_some_and(|f| e.flavors.iter().any(|x| x == f)),
                    busy: record.is_some_and(|r| self.busy.contains(&r.key())),
                    review: e.review_status(record.and_then(|r| r.installed_sha256.as_deref())),
                    entry: e.clone(),
                }
            })
            .collect();

        AppState {
            settings: self.store.settings().clone(),
            has_token: !self.store.token().is_empty(),
            installs: self.installs.clone(),
            active_install: install,
            addons,
            unmanaged,
            catalog,
            flavor: flavor.map(str::to_string),
            app_version: self.app_version.clone(),
            app_update: self.app_update.clone(),
            rate_limit: self.github.rate_limit(),
            checking: self.checking,
            last_check: self.last_check.clone(),
            path_error: self.path_error.clone(),
        }
    }

    // ---- commands ----

    pub fn update_settings(&mut self, u: SettingsUpdate) -> Result<()> {
        if let Some(token) = &u.github_token {
            self.store.set_token(token)?;
            *self.token.lock().unwrap() = self.store.token();
            // A new token is a different allowance.
            let _ = self.github.fetch_rate_limit();
        }
        let reinstalls = u.wow_path.is_some() || u.active_install.is_some();
        self.store.set_settings(|s| {
            if let Some(v) = u.wow_path {
                s.wow_path = v.trim().to_string();
            }
            if let Some(v) = u.active_install {
                s.active_install = v;
            }
            if let Some(v) = u.check_interval_hours {
                s.check_interval_hours = v.max(0.0);
            }
            if let Some(v) = u.auto_update {
                s.auto_update = v;
            }
        })?;
        if reinstalls {
            self.refresh_installs();
        }
        self.changed();
        Ok(())
    }

    /// Fetches the published catalog; keeps the bundled one when offline.
    pub fn refresh_catalog(&mut self) {
        // Debug builds only: PACKMULE_BUNDLED_CATALOG=1 tries catalog.json edits
        // before they are pushed.
        if cfg!(debug_assertions) && std::env::var_os("PACKMULE_BUNDLED_CATALOG").is_some() {
            return;
        }
        if self.catalog.refresh(self.http.as_ref()) {
            self.changed();
        }
    }

    /// Refreshes the GitHub API allowance shown in Settings (free to ask).
    pub fn refresh_rate_limit(&mut self) {
        if self.github.fetch_rate_limit().is_ok() {
            self.changed();
        }
    }

    /// Looks for a newer Packmule release; shown as a banner, never installed automatically.
    pub fn check_app_update(&mut self) {
        let Ok(release) = self.github.latest_release(APP_REPO, None) else { return };
        let latest = release.tag_name.trim_start_matches('v').to_string();
        if !newer_version(&latest, &self.app_version) {
            return;
        }
        if self.app_update.as_ref().is_some_and(|u| u.version == latest) {
            return;
        }
        self.log(LogLevel::Info, format!("Packmule {latest} is available (you have {})", self.app_version));
        self.app_tarball = selfupdate::tarball(&release.assets).cloned();
        let can_install = self.self_update_target.is_some() && self.app_tarball.is_some();
        self.app_update = Some(AppUpdate {
            version: latest,
            url: release.html_url,
            can_install,
            installed: false,
            installing: false,
        });
        self.changed();
    }

    /// Where Packmule may replace its own binary (None: it can't, so updates
    /// only link to the release).
    pub fn set_self_update_target(&mut self, exe: Option<PathBuf>) {
        self.self_update_target = exe;
    }

    /// Installs the available Packmule update over the running binary.
    pub fn install_app_update(&mut self) -> Result<()> {
        let (Some(update), Some(asset), Some(exe)) =
            (self.app_update.clone(), self.app_tarball.clone(), self.self_update_target.clone())
        else {
            bail!("No Packmule update to install");
        };
        if update.installed {
            return Ok(());
        }
        self.set_app_update(|u| u.installing = true);
        self.log(LogLevel::Info, format!("Installing Packmule {} to {}", update.version, exe.display()));
        let result = selfupdate::install(self.http.as_ref(), &asset, &exe);
        self.set_app_update(|u| {
            u.installing = false;
            u.installed = result.is_ok();
        });
        match &result {
            Ok(()) => self.log(LogLevel::Info, format!("Installed Packmule {}; restart to use it", update.version)),
            Err(e) => self.log(LogLevel::Error, format!("Packmule update failed: {e:#}")),
        }
        result
    }

    fn set_app_update(&mut self, f: impl FnOnce(&mut AppUpdate)) {
        if let Some(u) = &mut self.app_update {
            f(u);
        }
        self.changed();
    }

    /// Finds the newest build of a record for this client and records it as latest.
    fn resolve_latest(&self, r: &mut AddonRecord, install: &GameInstall) -> Result<ReleaseAsset> {
        let release = self.github.latest_release(&r.repo, r.tag_prefix.as_deref())?;
        let choice = self.github.choose_asset(&release, install.interface)?;
        r.latest_tag = Some(release.tag_name);
        r.latest_asset_id = Some(choice.asset.id);
        r.latest_asset_name = Some(choice.asset.name.clone());
        r.latest_sha256 = choice.asset.sha256();
        // Installs from before hashes were kept: the same asset is the same zip.
        if r.installed_sha256.is_none() && r.installed_asset_id == Some(choice.asset.id) {
            r.installed_sha256 = r.latest_sha256.clone();
        }
        r.compat_warning = choice.compat_warning;
        r.last_checked = Some(now());
        Ok(choice.asset)
    }

    fn install(&mut self, key: &str) -> Result<()> {
        let install = self.require_install()?;
        let mut r = self.record(key)?;
        self.busy.insert(key.to_string());
        self.changed();

        let result = (|| -> Result<()> {
            let asset = self.resolve_latest(&mut r, &install)?;
            self.log(
                LogLevel::Info,
                format!("Installing {key} {} ({})", r.latest_tag.as_deref().unwrap_or(""), asset.name),
            );
            let foreign: HashSet<String> =
                self.records().iter().filter(|o| o.key() != key).flat_map(|o| o.folders.iter().cloned()).collect();
            r.folders = install_asset(self.http.as_ref(), &self.addons_path(), &asset, &r.folders, &foreign)?;
            r.installed_tag = r.latest_tag.clone();
            r.installed_asset_id = Some(asset.id);
            r.installed_sha256 = asset.sha256();
            r.installed_at = Some(now());
            r.error = None;
            self.log(
                LogLevel::Info,
                format!("Installed {key} {}: {}", r.installed_tag.as_deref().unwrap_or(""), r.folders.join(", ")),
            );
            Ok(())
        })();
        if let Err(e) = &result {
            r.error = Some(format!("{e:#}"));
            self.log(LogLevel::Error, format!("{key}: {e:#}"));
        }
        self.busy.remove(key);
        let saved = self.put(r);
        self.changed();
        result.and(saved)
    }

    /// Adds an addon from a pasted GitHub link and installs it.
    pub fn add_addon(&mut self, url: &str) -> Result<()> {
        let install = self.require_install()?;
        let gh = parse_repo(url)?;
        let mut tag_prefix = gh.tag_prefix;
        if tag_prefix.is_none() {
            // Repos that publish several products (tags like "spoken/v1", "manager/v1")
            // share one /releases/latest, so make the user pick a stream.
            let streams = self.github.release_streams(&gh.repo)?;
            if streams.len() > 1 {
                let names: Vec<&str> = streams
                    .iter()
                    .map(|s| s.strip_suffix('/').filter(|s| !s.is_empty()).unwrap_or("(no prefix)"))
                    .collect();
                bail!(
                    "{} publishes several release streams ({}). Paste the link to a release from the one you \
                     want, e.g. https://github.com/{}/releases/tag/<stream>/<version>",
                    gh.repo,
                    names.join(", "),
                    gh.repo
                );
            }
            if let [only] = streams.as_slice()
                && !only.is_empty()
            {
                tag_prefix = Some(only.clone());
            }
        }
        self.add_record(&install, &gh.repo, tag_prefix.as_deref(), None)
    }

    /// Installs an addon from the Browse tab's catalog.
    pub fn install_catalog(&mut self, id: &str) -> Result<()> {
        let install = self.require_install()?;
        let entry = self.catalog.entry(id).cloned().ok_or_else(|| anyhow!("Unknown catalog addon: {id}"))?;
        match self.records().iter().find(|r| matches_entry(r, &entry)).map(AddonRecord::key) {
            Some(key) => self.install(&key),
            None => self.add_record(&install, &entry.repo, entry.tag_prefix.as_deref(), Some(&entry.name)),
        }
    }

    fn add_record(
        &mut self,
        install: &GameInstall,
        repo: &str,
        tag_prefix: Option<&str>,
        name: Option<&str>,
    ) -> Result<()> {
        let key = addon_key(repo, tag_prefix);
        if self.records().iter().any(|r| r.key().eq_ignore_ascii_case(&key)) {
            bail!("{key} is already managed");
        }
        let mut record = AddonRecord::new(repo, tag_prefix, name);
        // Validate before saving: it must have a packaged release.
        self.resolve_latest(&mut record, install)?;
        self.put(record)?;
        self.log(LogLevel::Info, format!("Added {key}"));
        self.install(&key)
    }

    pub fn remove_addon(&mut self, key: &str) -> Result<()> {
        let Ok(r) = self.record(key) else { return Ok(()) };
        remove_folders(&self.addons_path(), &r.folders)?;
        self.records_mut().retain(|x| x.key() != key);
        self.store.persist()?;
        let folders = if r.folders.is_empty() { "no folders".into() } else { r.folders.join(", ") };
        self.log(LogLevel::Info, format!("Removed {key} ({folders})"));
        self.changed();
        Ok(())
    }

    /// Installs, repairs or updates one addon.
    pub fn install_addon(&mut self, key: &str) -> Result<()> {
        self.install(key)
    }

    /// Installs everything that is outdated, missing files, or not installed.
    pub fn update_all(&mut self) -> Result<()> {
        let todo: Vec<String> =
            self.records().iter().filter(|r| self.status(r) != AddonStatus::UpToDate).map(AddonRecord::key).collect();
        for key in todo {
            let _ = self.install(&key); // already logged
        }
        self.changed();
        Ok(())
    }

    /// Finds addons installed by hand that Packmule can manage and adds them:
    /// first by the catalog's folder lists, then by a GitHub link in the .toc
    /// (only when that repo's latest release really contains the folder).
    fn adopt_manual(&mut self, install: &GameInstall) {
        let addons_path = self.addons_path();
        let owned: HashSet<String> = self.records().iter().flat_map(|r| r.folders.iter().cloned()).collect();
        let mut present: BTreeSet<String> = list_addon_folders(&addons_path)
            .into_iter()
            .filter(|f| !owned.contains(f) && !f.starts_with('.'))
            .collect();
        if present.is_empty() {
            return;
        }
        let flavor = product_flavor(&install.product);

        let entries: Vec<CatalogEntry> = self.catalog.entries().to_vec();
        for e in entries {
            let Some(flavor) = flavor else { break };
            if !e.flavors.iter().any(|f| f == flavor) || e.folders.is_empty() {
                continue;
            }
            if !present.contains(&e.folders[0]) || self.records().iter().any(|r| matches_entry(r, &e)) {
                continue;
            }
            let folders: Vec<String> = e.folders.iter().filter(|f| present.contains(*f)).cloned().collect();
            if self.adopt(install, &e.repo, e.tag_prefix.as_deref(), &folders, Some(&e.name)) {
                folders.iter().for_each(|f| {
                    present.remove(f);
                });
            }
        }

        for folder in present.clone() {
            if !present.contains(&folder) {
                continue; // taken by an earlier adoption
            }
            let Some(repo) = read_toc_info(&addons_path.join(&folder), &install.product).github_repo else { continue };
            if !self.link_checked.insert(format!("{folder}>{repo}")) {
                continue;
            }
            if self.records().iter().any(|r| r.repo.eq_ignore_ascii_case(&repo)) {
                continue;
            }
            let found = (|| -> Result<Option<Vec<String>>> {
                let release = self.github.latest_release(&repo, None)?;
                let choice = self.github.choose_asset(&release, install.interface)?;
                // A release that lists its builds but none for this client would
                // "update" it to the wrong game version.
                if choice.interface_match == Some(false) {
                    return Ok(None);
                }
                let entries =
                    remote_zip_entries(self.http.as_ref(), &choice.asset.browser_download_url, choice.asset.size)?;
                let in_zip = addon_folders_in(&entries);
                if !in_zip.contains(&folder) {
                    return Ok(None);
                }
                Ok(Some(in_zip.into_iter().filter(|f| present.contains(f)).collect()))
            })();
            // Any failure (no usable release): it stays a manual addon.
            if let Ok(Some(folders)) = found
                && self.adopt(install, &repo, None, &folders, None)
            {
                folders.iter().for_each(|f| {
                    present.remove(f);
                });
            }
        }
    }

    /// Starts managing folders that are already installed; true on success.
    fn adopt(
        &mut self,
        install: &GameInstall,
        repo: &str,
        tag_prefix: Option<&str>,
        folders: &[String],
        name: Option<&str>,
    ) -> bool {
        let mut record = AddonRecord::new(repo, tag_prefix, name);
        record.folders = folders.to_vec();
        let label = name.unwrap_or(repo).to_string();
        if let Err(e) = self.resolve_latest(&mut record, install) {
            self.log(LogLevel::Error, format!("Found {label} in AddOns but couldn't check its releases: {e:#}"));
            return false;
        }
        // Up to date if the installed .toc version is the latest release's version.
        let toc = read_toc_info(&self.addons_path().join(&folders[0]), &install.product);
        let latest = record.latest_tag.clone().unwrap_or_default();
        let current = toc.version.as_deref().is_some_and(|v| bare_version(v) == bare_version(&latest));
        record.installed_tag = Some(if current {
            latest.clone()
        } else {
            toc.version.map_or_else(|| "installed by hand".into(), |v| format!("{v} (by hand)"))
        });
        record.installed_asset_id = if current { record.latest_asset_id } else { None };
        // Only trust the hash when the .toc version says it's that release.
        record.installed_sha256 = if current { record.latest_sha256.clone() } else { None };
        if let Err(e) = self.put(record) {
            self.log(LogLevel::Error, format!("Could not save {label}: {e:#}"));
            return false;
        }
        let tail = if current { "; up to date".to_string() } else { format!("; {latest} is available") };
        self.log(
            LogLevel::Info,
            format!("Now managing {label} ({}), found installed by hand{tail}", folders.join(", ")),
        );
        true
    }

    pub fn check_updates(&mut self) -> Result<()> {
        if self.checking {
            return Ok(());
        }
        let install = self.require_install()?;
        self.checking = true;
        self.changed();

        self.adopt_manual(&install);
        let keys: Vec<String> = self.records().iter().map(AddonRecord::key).collect();
        for key in keys {
            let Ok(mut r) = self.record(&key) else { continue };
            match self.resolve_latest(&mut r, &install) {
                Ok(_) => r.error = None,
                Err(e) => {
                    r.error = Some(format!("{e:#}"));
                    self.log(LogLevel::Error, format!("{key}: {e:#}"));
                }
            }
            let dir = self.active_dir();
            if let Some(slot) = self.store.addons_mut(&dir).iter_mut().find(|x| x.key() == key) {
                *slot = r;
            }
        }
        let saved = self.store.persist().context("Could not save state");
        self.last_check = Some(now());
        let outdated = self.records().iter().filter(|r| self.status(r) != AddonStatus::UpToDate).count();
        self.log(LogLevel::Info, format!("Update check done: {outdated} addon(s) need attention"));
        self.checking = false;

        if self.store.settings().auto_update {
            self.update_all()?;
        } else {
            self.changed();
        }
        saved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_versions() {
        assert_eq!(bare_version("v1.14.0"), "1.14.0");
        assert_eq!(bare_version("spoken/v3.8.4"), "3.8.4");
        assert_eq!(bare_version("1.2.3+abc"), "1.2.3");
        assert_eq!(bare_version("V2.0 "), "2.0");
    }
}

//! End-to-end tests of the manager against a fake WoW folder and a fake GitHub.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::catalog::{Catalog, CatalogEntry};
use crate::manager::{AddonStatus, AppState, Event, Manager, SettingsUpdate};
use crate::store::{MemorySecret, Store};
use crate::testutil::{FakeHttp, FakeRelease, make_zip, manifest};

struct Env {
    _dir: tempfile::TempDir,
    wow: PathBuf,
    state_file: PathBuf,
    http: Arc<FakeHttp>,
    events: Arc<Mutex<Vec<Event>>>,
}

impl Env {
    fn addons(&self) -> PathBuf {
        self.wow.join("_forever_/Interface/AddOns")
    }

    fn hand_install(&self, folder: &str, toc: &str) {
        let dir = self.addons().join(folder);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{folder}.toc")), toc).unwrap();
    }

    fn manager(&self, catalog: Vec<CatalogEntry>) -> Manager {
        let store = Store::load(&self.state_file, Box::new(MemorySecret::default()));
        let events = self.events.clone();
        Manager::new(store, self.http.clone(), Catalog::from_entries(catalog), "0.2.0", move |e| {
            events.lock().unwrap().push(e)
        })
    }

    fn logs(&self) -> String {
        let events = self.events.lock().unwrap();
        events
            .iter()
            .filter_map(|e| match e {
                Event::Log(l) => Some(l.message.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let wow = dir.path().join("World of Warcraft");
    fs::create_dir_all(wow.join("_forever_/Interface/AddOns")).unwrap();
    fs::write(wow.join(".build.info"), "Version!STRING:0|Product!STRING:0\n1.60.1.70009|wowf\n").unwrap();
    fs::write(wow.join("_forever_/.flavor.info"), "Product Flavor!STRING:0\nwowf\n").unwrap();
    let state_file = dir.path().join("config/state.json");
    fs::create_dir_all(state_file.parent().unwrap()).unwrap();
    fs::write(
        &state_file,
        serde_json::json!({ "settings": { "wowPath": wow, "activeInstall": "_forever_" } }).to_string(),
    )
    .unwrap();
    Env { _dir: dir, wow, state_file, http: Arc::new(FakeHttp::default()), events: Arc::default() }
}

fn entry(id: &str, repo: &str, prefix: Option<&str>, folders: &[&str], flavors: &[&str]) -> CatalogEntry {
    CatalogEntry {
        id: id.into(),
        name: id.to_uppercase(),
        repo: repo.into(),
        tag_prefix: prefix.map(Into::into),
        category: "Testing".into(),
        description: String::new(),
        flavors: flavors.iter().map(|s| s.to_string()).collect(),
        folders: folders.iter().map(|s| s.to_string()).collect(),
        requires: Vec::new(),
    }
}

fn zip_of(folders: &[(&str, &str)]) -> Vec<u8> {
    let files: Vec<(String, Vec<u8>)> =
        folders.iter().map(|(f, toc)| (format!("{f}/{f}.toc"), toc.as_bytes().to_vec())).collect();
    make_zip(&files.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect::<Vec<_>>(), false)
}

fn status_of(state: &AppState, key: &str) -> Option<AddonStatus> {
    state.addons.iter().find(|a| a.key == key).map(|a| a.status)
}

fn unmanaged(state: &AppState) -> Vec<&str> {
    state.unmanaged.iter().map(|m| m.folder.as_str()).collect()
}

fn toc_text(path: &Path) -> String {
    fs::read_to_string(path).unwrap()
}

#[test]
fn adopts_hand_installed_addons_and_updates_them() {
    let e = env();
    // Satchel: in the catalog, installed by hand at the latest version.
    e.http.releases(&[FakeRelease {
        repo: "hammackj/satchel",
        tag: "v0.2.0",
        zips: vec![("Satchel-v0.2.0.zip", zip_of(&[("Satchel", "## Version: v0.2.0\n")]))],
        manifest: Some(manifest(&[("Satchel-v0.2.0.zip", "forever", 16001)])),
    }]);
    e.hand_install("Satchel", "## Title: Satchel\n## Version: v0.2.0\n");

    // Mapster: not in the catalog, but its .toc links to GitHub; an older version.
    e.http.releases(&[FakeRelease {
        repo: "fake/Mapster",
        tag: "1.14.0",
        zips: vec![("Mapster-1.14.0.zip", zip_of(&[("Mapster", "## Version: 1.14.0\n"), ("Mapster_Extra", "x")]))],
        manifest: Some(manifest(&[("Mapster-1.14.0.zip", "forever", 16001)])),
    }]);
    e.hand_install("Mapster", "## Version: 1.13.0\n## X-Website: https://github.com/fake/Mapster\n");

    // OmniCC: links to GitHub, but the release only has a retail build.
    e.http.releases(&[FakeRelease {
        repo: "fake/OmniCC",
        tag: "12.0.0",
        zips: vec![("OmniCC.zip", zip_of(&[("OmniCC", "x")]))],
        manifest: Some(manifest(&[("OmniCC.zip", "mainline", 120001)])),
    }]);
    e.hand_install("OmniCC", "## Version: 11.0\n## X-Website: https://github.com/fake/OmniCC\n");

    // Helper: links to a repo whose release doesn't contain it.
    e.http.releases(&[FakeRelease {
        repo: "fake/Other",
        tag: "1.0",
        zips: vec![("Other.zip", zip_of(&[("Other", "x")]))],
        manifest: None,
    }]);
    e.hand_install("Helper", "## X-Website: https://github.com/fake/Other\n");

    // MyOwnThing: no link at all.
    e.hand_install("MyOwnThing", "## Title: Mine\n");

    let mut m = e.manager(vec![entry("satchel", "hammackj/satchel", None, &["Satchel"], &["forever"])]);
    let before = m.state();
    assert_eq!(unmanaged(&before), ["Helper", "Mapster", "MyOwnThing", "OmniCC", "Satchel"]);

    m.check_updates().unwrap();
    let s = m.state();
    assert_eq!(status_of(&s, "hammackj/satchel"), Some(AddonStatus::UpToDate));
    assert_eq!(status_of(&s, "fake/Mapster"), Some(AddonStatus::UpdateAvailable));
    assert_eq!(unmanaged(&s), ["Helper", "MyOwnThing", "OmniCC"]);
    let satchel = s.addons.iter().find(|a| a.key == "hammackj/satchel").unwrap();
    assert_eq!((satchel.name.as_str(), satchel.category.as_deref()), ("SATCHEL", Some("Testing")));
    let mapster = s.addons.iter().find(|a| a.key == "fake/Mapster").unwrap();
    assert_eq!(mapster.record.installed_tag.as_deref(), Some("1.13.0 (by hand)"));
    assert!(e.logs().contains("Now managing SATCHEL (Satchel), found installed by hand; up to date"), "{}", e.logs());

    // Updating replaces the hand-installed folder and brings the extra folder along.
    m.update_all().unwrap();
    let s = m.state();
    assert_eq!(status_of(&s, "fake/Mapster"), Some(AddonStatus::UpToDate));
    assert_eq!(toc_text(&e.addons().join("Mapster/Mapster.toc")), "## Version: 1.14.0\n");
    assert!(e.addons().join("Mapster_Extra").exists());
    assert_eq!(unmanaged(&s), ["Helper", "MyOwnThing", "OmniCC"]);

    // A second check costs nothing: every API answer is a 304.
    let before_304 = *e.http.not_modified.lock().unwrap();
    m.check_updates().unwrap();
    assert!(*e.http.not_modified.lock().unwrap() > before_304);
    assert_eq!(unmanaged(&m.state()), ["Helper", "MyOwnThing", "OmniCC"]);

    // Everything survives a restart.
    drop(m);
    let mut m = e.manager(vec![entry("satchel", "hammackj/satchel", None, &["Satchel"], &["forever"])]);
    let s = m.state();
    assert_eq!(s.addons.len(), 2);
    assert_eq!(status_of(&s, "fake/Mapster"), Some(AddonStatus::UpToDate));
}

#[test]
fn add_remove_streams_and_catalog() {
    let e = env();
    let rel = |tag: &'static str, folder: &'static str| FakeRelease {
        repo: "fake/spoken",
        tag,
        zips: vec![("pack.zip", zip_of(&[(folder, "x")]))],
        manifest: None,
    };
    e.http.releases(&[rel("quests-audio/v2.5.0", "SpokenQuests"), rel("spoken/v3.8.4", "Spoken")]);
    e.http.releases(&[FakeRelease {
        repo: "fake/bw",
        tag: "v400",
        zips: vec![("BW.zip", zip_of(&[("BigWigs", "x"), ("BigWigs_Core", "y")]))],
        manifest: Some(manifest(&[("BW.zip", "forever", 16001)])),
    }]);
    let catalog = vec![
        entry("spoken-quests", "fake/spoken", Some("quests-audio/"), &["SpokenQuests"], &["forever"]),
        entry("bigwigs", "fake/bw", None, &["BigWigs"], &["forever"]),
        entry("retailonly", "fake/retail", None, &["R"], &["mainline"]),
    ];
    let mut m = e.manager(catalog);

    // A repo with several streams needs a link to one of them.
    let err = m.add_addon("https://github.com/fake/spoken").unwrap_err().to_string();
    assert!(err.contains("several release streams (quests-audio, spoken)"), "{err}");
    m.add_addon("https://github.com/fake/spoken/releases/tag/spoken/v3.8.4").unwrap();
    assert!(e.addons().join("Spoken/Spoken.toc").exists());
    let err = m.add_addon("https://github.com/fake/spoken/releases/tag/spoken/v1").unwrap_err().to_string();
    assert!(err.contains("already managed"), "{err}");

    // The other stream is a separate catalog entry.
    let s = m.state();
    let view = |s: &AppState, id: &str| {
        s.catalog.iter().find(|c| c.entry.id == id).map(|c| (c.installed, c.available)).unwrap()
    };
    assert_eq!(view(&s, "spoken-quests"), (false, true));
    assert_eq!(view(&s, "retailonly"), (false, false));
    m.install_catalog("spoken-quests").unwrap();
    m.install_catalog("bigwigs").unwrap();
    let s = m.state();
    assert_eq!(view(&s, "spoken-quests"), (true, true));
    assert_eq!(view(&s, "bigwigs"), (true, true));
    assert!(s.unmanaged.is_empty());
    assert_eq!(s.addons.len(), 3);

    // Deleting a folder by hand shows up as missing files; reinstalling repairs it.
    fs::remove_dir_all(e.addons().join("BigWigs_Core")).unwrap();
    assert_eq!(status_of(&m.state(), "fake/bw"), Some(AddonStatus::MissingFiles));
    m.install_addon("fake/bw").unwrap();
    assert_eq!(status_of(&m.state(), "fake/bw"), Some(AddonStatus::UpToDate));

    // Removing deletes only that addon's folders.
    m.remove_addon("fake/bw").unwrap();
    assert!(!e.addons().join("BigWigs").exists() && !e.addons().join("BigWigs_Core").exists());
    assert!(e.addons().join("Spoken").exists() && e.addons().join("SpokenQuests").exists());
    assert_eq!(m.state().addons.len(), 2);
}

#[test]
fn settings_and_path_errors() {
    let e = env();
    let mut m = e.manager(Vec::new());
    assert!(m.state().path_error.is_none());
    assert_eq!(m.state().flavor.as_deref(), Some("forever"));

    m.update_settings(SettingsUpdate { wow_path: Some("/nonexistent/wow".into()), ..Default::default() }).unwrap();
    let s = m.state();
    assert!(s.path_error.unwrap().starts_with("Cannot read"));
    assert!(m.check_updates().is_err());

    m.update_settings(SettingsUpdate {
        wow_path: Some(e.wow.to_string_lossy().into()),
        github_token: Some("secret".into()),
        auto_update: Some(true),
        ..Default::default()
    })
    .unwrap();
    let s = m.state();
    assert!(s.path_error.is_none() && s.has_token && s.settings.auto_update);
    assert!(!fs::read_to_string(&e.state_file).unwrap().contains("secret"));
}

#[test]
fn packmule_update_banner() {
    let e = env();
    e.http.releases(&[FakeRelease { repo: "hammackj/packmule", tag: "v0.3.0", zips: vec![], manifest: None }]);
    let mut m = e.manager(Vec::new());
    m.check_app_update();
    let update = m.state().app_update.unwrap();
    assert_eq!(update.version, "0.3.0");
}

#[test]
fn worker_runs_commands_in_order_and_reports_failures() {
    use crate::worker::{Command, Worker};
    use std::sync::mpsc;
    use std::time::Duration;

    let e = env();
    e.http.releases(&[FakeRelease {
        repo: "fake/a",
        tag: "v1",
        zips: vec![("a.zip", zip_of(&[("A", "x")]))],
        manifest: None,
    }]);
    let (tx, rx) = mpsc::channel();
    let store = Store::load(&e.state_file, Box::new(MemorySecret::default()));
    let http = e.http.clone();
    let worker = Worker::spawn(
        move || {
            let tx = Mutex::new(tx);
            Manager::new(store, http, Catalog::from_entries(Vec::new()), "0.2.0", move |ev| {
                let _ = tx.lock().unwrap().send(ev);
            })
        },
        false,
    );
    worker.send(Command::AddAddon("not a link".into()));
    worker.send(Command::AddAddon("https://github.com/fake/a".into()));
    worker.send(Command::Refresh);

    let mut failures = Vec::new();
    let mut last_state = None;
    while let Ok(ev) = rx.recv_timeout(Duration::from_secs(5)) {
        match ev {
            Event::Failed(msg) => failures.push(msg),
            Event::State(s) => {
                let done = s.addons.iter().any(|a| a.status == AddonStatus::UpToDate);
                last_state = Some(s);
                if done && !failures.is_empty() {
                    break;
                }
            }
            Event::Log(_) | Event::CommandDone => {}
        }
    }
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert!(failures[0].contains("Not a GitHub repository URL"));
    assert_eq!(status_of(&last_state.unwrap(), "fake/a"), Some(AddonStatus::UpToDate));
    drop(worker);
}

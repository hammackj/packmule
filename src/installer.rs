//! Downloading and unpacking addons into Interface/AddOns, safely.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};

use crate::github::ReleaseAsset;
use crate::http::Http;

const MAX_UNPACKED_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// A relative path made only of normal components (no "..", no root, no drive).
fn safe_relative(name: &str) -> Option<PathBuf> {
    let name = name.replace('\\', "/");
    let path = Path::new(&name);
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

/// Extracts a zip, refusing symlinks and any entry that would land outside
/// `dest`. Zips come from arbitrary repos, so this is not optional.
pub fn extract_zip(zip_path: &Path, dest: &Path) -> Result<()> {
    let mut zip = zip::ZipArchive::new(File::open(zip_path)?).context("Not a valid zip file")?;
    let mut total: u64 = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        let name = entry.name()?.to_string();
        let Some(rel) = safe_relative(&name) else { bail!("Unsafe path in zip: {name}") };
        if entry.is_symlink() {
            bail!("Symlink in zip: {name}");
        }
        let target = dest.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        // The declared size can lie, so count what is actually written.
        let budget = MAX_UNPACKED_BYTES - total;
        let mut out = File::create(&target)?;
        let written = io::copy(&mut (&mut entry).take(budget + 1), &mut out)?;
        total += written;
        if written > budget {
            bail!("Zip unpacks to more than 2 GB");
        }
    }
    Ok(())
}

/// SHA-256 of a file, lowercase hex.
pub fn sha256_file(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut file = File::open(path)?;
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

fn has_toc(dir: &Path) -> bool {
    fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).any(|e| e.file_name().to_string_lossy().to_lowercase().ends_with(".toc")))
        .unwrap_or(false)
}

fn subdirs(dir: &Path) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir)? {
        let e = e?;
        if e.file_type()?.is_dir()
            && let Ok(name) = e.file_name().into_string()
        {
            out.push(name);
        }
    }
    out.sort();
    Ok(out)
}

/// Addon folders are the top-level dirs with a .toc; unwrap one extra level if needed.
fn find_addon_folders(root: &Path) -> Result<(PathBuf, Vec<String>)> {
    let top = subdirs(root)?;
    let with_toc: Vec<String> = top.iter().filter(|d| has_toc(&root.join(d))).cloned().collect();
    if !with_toc.is_empty() {
        return Ok((root.to_path_buf(), with_toc));
    }
    if top.len() == 1 {
        return find_addon_folders(&root.join(&top[0]));
    }
    bail!("No addon folders (with a .toc file) found in the zip")
}

/// True if every folder exists and still has a .toc file.
pub fn verify_folders(addons_path: &Path, folders: &[String]) -> bool {
    !folders.is_empty() && folders.iter().all(|f| has_toc(&addons_path.join(f)))
}

fn unique_name() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{}-{nanos:x}", std::process::id())
}

/// Downloads and installs an addon, replacing `previous_folders`.
/// Work happens in a staging dir on the same filesystem as AddOns so the
/// final swap is a set of renames, and old folders are restored on failure.
pub fn install_asset(
    http: &dyn Http,
    addons_path: &Path,
    asset: &ReleaseAsset,
    previous_folders: &[String],
    foreign_folders: &HashSet<String>,
) -> Result<Vec<String>> {
    // <client>/.packmule-tmp, next to Interface/, so renames stay on one filesystem.
    let client_dir = addons_path.parent().and_then(Path::parent).context("Unexpected AddOns path")?;
    let tmp_root = client_dir.join(".packmule-tmp");
    let staging = tmp_root.join(unique_name());
    let extracted = staging.join("extracted");
    let backup = staging.join("backup");
    fs::create_dir_all(&extracted)?;
    fs::create_dir_all(&backup)?;
    fs::create_dir_all(addons_path)?;

    let result = (|| -> Result<Vec<String>> {
        let zip_path = staging.join("addon.zip");
        http.download(&asset.browser_download_url, &zip_path)?;
        if let Some(expected) = asset.sha256() {
            let actual = sha256_file(&zip_path)?;
            if actual != expected {
                bail!(
                    "Download of {} is corrupt or was changed (SHA-256 {actual}, GitHub says {expected})",
                    asset.name
                );
            }
        }
        extract_zip(&zip_path, &extracted)?;
        let (base, folders) = find_addon_folders(&extracted)?;

        let clash: Vec<&str> = folders.iter().filter(|f| foreign_folders.contains(*f)).map(String::as_str).collect();
        if !clash.is_empty() {
            bail!("Folder(s) already owned by another managed addon: {}", clash.join(", "));
        }

        let mut moved: Vec<&str> = Vec::new();
        let mut backed_up: Vec<&str> = Vec::new();
        let mut seen = HashSet::new();
        let swap = (|| -> io::Result<()> {
            for f in previous_folders.iter().chain(&folders) {
                if seen.insert(f.as_str()) && addons_path.join(f).symlink_metadata().is_ok() {
                    fs::rename(addons_path.join(f), backup.join(f))?;
                    backed_up.push(f);
                }
            }
            for f in &folders {
                fs::rename(base.join(f), addons_path.join(f))?;
                moved.push(f);
            }
            Ok(())
        })();
        if let Err(e) = swap {
            for f in &moved {
                let _ = fs::remove_dir_all(addons_path.join(f));
            }
            for f in &backed_up {
                let _ = fs::rename(backup.join(f), addons_path.join(f));
            }
            return Err(e).context("Could not move the addon into AddOns; the previous version was restored");
        }
        Ok(folders)
    })();

    let _ = fs::remove_dir_all(&staging);
    let _ = fs::remove_dir(&tmp_root); // only succeeds once empty
    result
}

/// Deletes the given folders from AddOns. Names are checked so a corrupt
/// state file can never point outside the AddOns folder.
pub fn remove_folders(addons_path: &Path, folders: &[String]) -> Result<()> {
    for f in folders {
        let Some(rel) = safe_relative(f) else { continue };
        if rel.components().count() != 1 {
            continue;
        }
        match fs::remove_dir_all(addons_path.join(rel)) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("Could not remove {f}")),
        }
    }
    Ok(())
}

/// Folder names directly inside AddOns (empty if it doesn't exist).
pub fn list_addon_folders(addons_path: &Path) -> Vec<String> {
    subdirs(addons_path).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{FakeHttp, make_zip, make_zip_with_symlink};

    fn asset(url: &str) -> ReleaseAsset {
        ReleaseAsset { id: 1, name: "a.zip".into(), size: 0, browser_download_url: url.into(), digest: None }
    }

    fn setup() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let addons = dir.path().join("_forever_").join("Interface").join("AddOns");
        fs::create_dir_all(&addons).unwrap();
        (dir, addons)
    }

    #[test]
    fn rejects_unsafe_paths() {
        assert!(safe_relative("../evil").is_none());
        assert!(safe_relative("/etc/passwd").is_none());
        assert!(safe_relative("a/../../b").is_none());
        assert!(safe_relative("..\\evil").is_none());
        assert_eq!(safe_relative("./A/b.lua"), Some(PathBuf::from("A/b.lua")));
    }

    #[test]
    fn installs_and_replaces() {
        let (_dir, addons) = setup();
        let http = FakeHttp::default();
        http.serve(
            "https://dl/v1.zip",
            make_zip(&[("Foo/Foo.toc", b"v1".as_slice()), ("Foo_Opt/Foo_Opt.toc", b"o")], false),
        );
        http.serve("https://dl/v2.zip", make_zip(&[("wrap/Foo/Foo.toc", b"v2".as_slice())], false));

        let f = install_asset(&http, &addons, &asset("https://dl/v1.zip"), &[], &HashSet::new()).unwrap();
        assert_eq!(f, ["Foo", "Foo_Opt"]);
        assert!(verify_folders(&addons, &f));

        // v2 drops Foo_Opt and is wrapped in an extra folder.
        let f2 = install_asset(&http, &addons, &asset("https://dl/v2.zip"), &f, &HashSet::new()).unwrap();
        assert_eq!(f2, ["Foo"]);
        assert_eq!(fs::read_to_string(addons.join("Foo/Foo.toc")).unwrap(), "v2");
        assert!(!addons.join("Foo_Opt").exists());
        assert!(!addons.parent().unwrap().parent().unwrap().join(".packmule-tmp").exists());
    }

    #[test]
    fn refuses_zip_slip_and_symlinks() {
        let (dir, addons) = setup();
        let http = FakeHttp::default();
        http.serve(
            "https://dl/slip.zip",
            make_zip(&[("Foo/Foo.toc", b"x".as_slice()), ("../../escape.txt", b"x")], false),
        );
        http.serve("https://dl/link.zip", make_zip_with_symlink("Foo/Foo.toc", "Foo/link", "/etc/passwd"));

        let e = install_asset(&http, &addons, &asset("https://dl/slip.zip"), &[], &HashSet::new()).unwrap_err();
        assert!(e.to_string().contains("Unsafe path"), "{e}");
        assert!(!dir.path().join("escape.txt").exists());
        let e = install_asset(&http, &addons, &asset("https://dl/link.zip"), &[], &HashSet::new()).unwrap_err();
        assert!(e.to_string().contains("Symlink"), "{e}");
        assert!(list_addon_folders(&addons).is_empty());
    }

    #[test]
    fn checks_the_sha256() {
        let (_dir, addons) = setup();
        let http = FakeHttp::default();
        let zip = make_zip(&[("Foo/Foo.toc", b"x".as_slice())], false);
        http.serve("https://dl/a.zip", zip.clone());
        let tmp = tempfile::NamedTempFile::new().unwrap();
        fs::write(tmp.path(), &zip).unwrap();
        let good = sha256_file(tmp.path()).unwrap();

        let mut a = asset("https://dl/a.zip");
        a.digest = Some(format!("sha256:{}", "0".repeat(64)));
        let e = install_asset(&http, &addons, &a, &[], &HashSet::new()).unwrap_err();
        assert!(e.to_string().contains("corrupt or was changed"), "{e}");
        assert!(list_addon_folders(&addons).is_empty());

        a.digest = Some(format!("sha256:{}", good.to_uppercase()));
        install_asset(&http, &addons, &a, &[], &HashSet::new()).unwrap();
        assert!(addons.join("Foo/Foo.toc").exists());
    }

    #[test]
    fn refuses_folders_owned_by_others() {
        let (_dir, addons) = setup();
        let http = FakeHttp::default();
        http.serve("https://dl/a.zip", make_zip(&[("Shared/Shared.toc", b"x".as_slice())], false));
        let foreign = HashSet::from(["Shared".to_string()]);
        let e = install_asset(&http, &addons, &asset("https://dl/a.zip"), &[], &foreign).unwrap_err();
        assert!(e.to_string().contains("already owned"), "{e}");
    }

    #[test]
    fn restores_old_version_when_swap_fails() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, addons) = setup();
        let http = FakeHttp::default();
        http.serve("https://dl/v1.zip", make_zip(&[("Foo/Foo.toc", b"v1".as_slice())], false));
        let f = install_asset(&http, &addons, &asset("https://dl/v1.zip"), &[], &HashSet::new()).unwrap();

        // v2 also ships Zed. A hand-installed Zed that can't be moved (Linux needs
        // write access to a directory to move it elsewhere) makes the swap fail
        // after Foo was already moved to the backup.
        http.serve("https://dl/v2.zip", make_zip(&[("Foo/Foo.toc", b"v2".as_slice()), ("Zed/Zed.toc", b"z")], false));
        fs::create_dir_all(addons.join("Zed")).unwrap();
        fs::write(addons.join("Zed/Zed.toc"), "mine").unwrap();
        fs::set_permissions(addons.join("Zed"), fs::Permissions::from_mode(0o555)).unwrap();
        let result = install_asset(&http, &addons, &asset("https://dl/v2.zip"), &f, &HashSet::new());
        fs::set_permissions(addons.join("Zed"), fs::Permissions::from_mode(0o755)).unwrap();
        if is_root() {
            return; // root ignores permissions, so the failure can't be staged
        }

        assert!(result.is_err());
        assert_eq!(fs::read_to_string(addons.join("Foo/Foo.toc")).unwrap(), "v1");
        assert_eq!(fs::read_to_string(addons.join("Zed/Zed.toc")).unwrap(), "mine");
        assert!(!addons.parent().unwrap().parent().unwrap().join(".packmule-tmp").exists());
    }

    fn is_root() -> bool {
        use std::os::unix::fs::MetadataExt;
        fs::metadata("/proc/self").map(|m| m.uid() == 0).unwrap_or(false)
    }

    #[test]
    fn remove_stays_inside_addons() {
        let (dir, addons) = setup();
        fs::create_dir_all(addons.join("Foo")).unwrap();
        fs::create_dir_all(dir.path().join("outside")).unwrap();
        remove_folders(&addons, &["Foo".into(), "../../../outside".into(), "Missing".into()]).unwrap();
        assert!(!addons.join("Foo").exists());
        assert!(dir.path().join("outside").exists());
    }
}

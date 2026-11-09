//! Updating Packmule itself, for installs from the .tar.gz (in the user's home
//! or /usr/local). Package-manager installs (/usr/bin: pacman, AUR, .deb) are
//! left to the package manager, and builds run from a source checkout to git.

use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::github::ReleaseAsset;
use crate::http::Http;
use crate::installer::sha256_file;

/// No real build is anywhere near this.
const MAX_BINARY_BYTES: u64 = 512 << 20;

/// The running binary, if Packmule may replace it in place.
pub fn target() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    replaceable(&exe).then_some(exe)
}

fn replaceable(exe: &Path) -> bool {
    let packaged = exe.starts_with("/usr") && !exe.starts_with("/usr/local");
    let dev_build = exe.components().any(|c| c.as_os_str() == "target");
    let Some(dir) = exe.parent() else { return false };
    !packaged && !dev_build && writable(dir) && fs::metadata(exe).is_ok_and(|m| !m.permissions().readonly())
}

fn writable(dir: &Path) -> bool {
    tempfile::tempfile_in(dir).is_ok()
}

/// The release's Linux tarball, for this machine.
pub fn tarball(assets: &[ReleaseAsset]) -> Option<&ReleaseAsset> {
    assets.iter().find(|a| a.name.starts_with("packmule-") && a.name.ends_with("-x86_64-linux.tar.gz"))
}

/// Downloads the tarball, checks it against GitHub's SHA-256, and swaps its
/// `bin/packmule` in for `exe` with a rename, so a failure leaves the old
/// binary untouched. The running process keeps its copy until it restarts.
pub fn install(http: &dyn Http, asset: &ReleaseAsset, exe: &Path) -> Result<()> {
    let want = asset.sha256().context("GitHub publishes no SHA-256 for this download, so it can't be checked")?;
    let dir = exe.parent().context("no folder for the binary")?;
    // Work next to the binary: the final rename must stay on one file system.
    let work = tempfile::Builder::new().prefix(".packmule-update-").tempdir_in(dir)?;
    let tarball = work.path().join(&asset.name);
    http.download(&asset.browser_download_url, &tarball)?;
    let got = sha256_file(&tarball)?;
    if got != want {
        bail!("The update download is corrupt or was changed (SHA-256 {got}, GitHub says {want})");
    }

    let new = work.path().join("packmule");
    extract_binary(&tarball, &new)?;
    let mut head = [0u8; 4];
    File::open(&new)?.read_exact(&mut head).context("the update's binary is empty")?;
    if head != *b"\x7fELF" {
        bail!("The update's packmule isn't a Linux program");
    }
    fs::set_permissions(&new, fs::Permissions::from_mode(0o755))?;
    File::open(&new)?.sync_all()?;
    fs::rename(&new, exe).with_context(|| format!("replacing {}", exe.display()))?;
    Ok(())
}

/// Writes the archive's `<top>/bin/packmule` to `dest`.
fn extract_binary(tarball: &Path, dest: &Path) -> Result<()> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(File::open(tarball)?));
    for entry in archive.entries().context("not a .tar.gz")? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let parts: Vec<_> = path.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        if !(parts.len() == 3 && parts[1] == "bin" && parts[2] == "packmule") {
            continue;
        }
        if !entry.header().entry_type().is_file() {
            bail!("The update's packmule isn't a regular file");
        }
        let mut out = File::create(dest)?;
        let written = io::copy(&mut (&mut entry).take(MAX_BINARY_BYTES + 1), &mut out)?;
        if written > MAX_BINARY_BYTES {
            bail!("The update's packmule is implausibly large");
        }
        return Ok(());
    }
    bail!("The update has no bin/packmule")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{FakeHttp, tarball_with};

    fn asset(http: &FakeHttp, body: Vec<u8>, digest: Option<String>) -> ReleaseAsset {
        let url = "https://dl.example/packmule-9.9.9-x86_64-linux.tar.gz";
        let size = body.len() as u64;
        http.serve(url, body);
        ReleaseAsset {
            id: 1,
            name: "packmule-9.9.9-x86_64-linux.tar.gz".into(),
            size,
            browser_download_url: url.into(),
            digest,
        }
    }

    fn sha(b: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        format!("sha256:{}", Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect::<String>())
    }

    #[test]
    fn replaces_the_binary() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("packmule");
        fs::write(&exe, b"\x7fELF old").unwrap();
        let tgz = tarball_with(&[
            ("packmule-9.9.9-x86_64-linux/install.sh", b"#!/bin/sh"),
            ("packmule-9.9.9-x86_64-linux/bin/packmule", b"\x7fELF new"),
        ]);
        let http = FakeHttp::default();
        let a = asset(&http, tgz.clone(), Some(sha(&tgz)));
        install(&http, &a, &exe).unwrap();
        assert_eq!(fs::read(&exe).unwrap(), b"\x7fELF new");
        assert_eq!(fs::metadata(&exe).unwrap().permissions().mode() & 0o777, 0o755);
        // Nothing left behind.
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn refuses_bad_downloads() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("packmule");
        fs::write(&exe, b"\x7fELF old").unwrap();
        let http = FakeHttp::default();
        let good = tarball_with(&[("p/bin/packmule", b"\x7fELF new")]);

        let wrong_hash = asset(&http, good.clone(), Some(sha(b"something else")));
        assert!(format!("{:#}", install(&http, &wrong_hash, &exe).unwrap_err()).contains("corrupt"));
        let no_hash = asset(&http, good, None);
        assert!(install(&http, &no_hash, &exe).is_err());
        let script = tarball_with(&[("p/bin/packmule", b"#!/bin/sh\nrm -rf ~")]);
        let not_elf = asset(&http, script.clone(), Some(sha(&script)));
        assert!(install(&http, &not_elf, &exe).is_err());
        let empty = tarball_with(&[("p/README", b"hi")]);
        let missing = asset(&http, empty.clone(), Some(sha(&empty)));
        assert!(install(&http, &missing, &exe).is_err());

        assert_eq!(fs::read(&exe).unwrap(), b"\x7fELF old");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn only_replaces_its_own_installs() {
        assert!(!replaceable(Path::new("/usr/bin/packmule")));
        assert!(!replaceable(Path::new("/home/me/src/packmule/target/release/packmule")));
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("packmule");
        fs::write(&exe, b"x").unwrap();
        assert!(replaceable(&exe));
    }

    #[test]
    fn finds_the_tarball() {
        let a = |n: &str| ReleaseAsset {
            id: 1,
            name: n.into(),
            size: 1,
            browser_download_url: String::new(),
            digest: None,
        };
        let assets = [
            a("packmule_0.3.0_amd64.deb"),
            a("packmule-0.3.0-x86_64-linux.tar.gz"),
            a("packmule-0.3.0-1-x86_64.pkg.tar.zst"),
        ];
        assert_eq!(tarball(&assets).unwrap().name, "packmule-0.3.0-x86_64-linux.tar.gz");
    }
}

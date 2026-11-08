# WoW Addon Manager

A small Electron app that installs and updates World of Warcraft addons from
**GitHub releases** and **CurseForge**. Paste a repo or addon URL, it installs;
that's the whole model.

## Run

```sh
npm install      # also downloads the Electron binary (postinstall)
npm run dev      # dev mode with hot reload
npm run build && npm start
```

Open **Settings**, point it at the folder containing `_retail_`, `_classic_beta_`,
etc., then pick the game client from the dropdown.

## How it works

- **Release selection:** reads `releases/latest`. If the release has a
  `release.json` (BigWigs packager), picks the zip whose metadata lists the game
  client's interface number (read from `.build.info`, e.g. `1.60.1` → `16001`
  for WoW Forever). Otherwise falls back to the first non-`-nolib` zip.
  Source-only repos without a packaged zip are rejected.
- **Release streams:** some repos publish several products from one
  repository, with tags like `spoken/v3.8.4` and `quests-audio/v2.5.0`, so
  GitHub's single "latest release" is unreliable. Adding such a repo asks you
  to paste a link to a release from the stream you want
  (`.../releases/tag/spoken/v3.8.4`). The addon then follows only that stream,
  ignoring drafts and prereleases. Each stream is a separate entry.
- **CurseForge:** paste a `curseforge.com/wow/addons/<slug>` URL. Needs a free
  API key from [console.curseforge.com](https://console.curseforge.com) (Settings).
  Picks the newest *release* file tagged for the client's game version (e.g.
  `1.60.1`), falling back to the same major.minor with a warning; never to
  another flavor. Updates are detected by file id. Addons whose authors disable
  third-party downloads can't be installed this way; the app links to the page.
- **Install:** download and extract into `<client>/.wow-addon-mgr-tmp/`, find
  top-level folders containing a `.toc`, then swap them into `Interface/AddOns`
  via renames. Old folders are restored if anything fails. Zips containing
  symlinks or paths escaping the target are rejected.
- **Ownership:** each addon records the folders it installed. Removing an addon
  deletes only those; folders it didn't install are listed but never touched.
- **Updates:** checked on launch and every N hours (Settings), using ETags so
  unchanged repos don't consume GitHub rate limit. Optional auto-install.
  Missing folders are detected and can be repaired.
- **State:** `~/.config/wow-addon-mgr/state.json`. The GitHub token and
  CurseForge key are encrypted with Electron `safeStorage` when a keyring is available.

## Desktop launcher (Linux)

```sh
./packaging/install-launcher.sh
```

Installs the icon, a menu entry, and a desktop shortcut for the current user.
The launcher rebuilds automatically when `src/` is newer than the last build.
Re-run it after moving the repo.

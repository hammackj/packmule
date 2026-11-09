# Packmule

An addon manager for World of Warcraft that installs and updates addons
straight from their **GitHub releases** (and from CurseForge, with your own API
key). Linux desktop app.

- **Browse:** a catalog of addons that publish packaged GitHub releases,
  grouped by category like the in-game AddOns list. It only offers addons with a
  build for the game client you've selected (Retail, Classic, WoW Forever...).
- **Installed:** everything in your AddOns folder, grouped by category, with
  one-click updates. Addons you installed by hand are listed too, but never
  touched.
- **Any GitHub addon:** paste a repository link to add one that isn't in the
  catalog. It needs releases with a packaged `.zip`.

## Install

Download the latest build from
[Releases](https://github.com/hammackj/packmule/releases):

- **Arch / CachyOS / Manjaro:** `sudo pacman -U Packmule-<version>-*.pacman`
- **Debian / Ubuntu:** `sudo apt install ./Packmule-<version>-*.deb`
- **Anything else:** the `.AppImage` (needs `libfuse2`)

Open **Settings**, point Packmule at the folder that contains `_retail_`,
`_classic_era_`, `_forever_` and so on, then pick the game client at the top.
Packmule shows a banner when a newer version of itself is released.

## How it works

- **Picking the right build:** if a release has a `release.json` (written by
  the BigWigs packager most addons use), Packmule installs the zip listed for
  your client's interface number, read from the game's `.build.info`.
  Otherwise it uses the release's zip.
- **Release streams:** some repositories publish several products with tags
  like `spoken/v3.8.4` and `quests-audio/v2.5.0`. Add one by pasting a link to
  a release from the stream you want; the catalog already knows the streams.
- **Safe installs:** downloads unpack into a staging folder and are swapped
  into `Interface/AddOns` with renames, restoring the old folders if anything
  fails. Zips containing symlinks or paths outside the target are rejected.
  Removing an addon deletes only the folders Packmule installed for it.
- **Updates:** checked at launch and every few hours (Settings), using ETags so
  unchanged repositories cost no GitHub rate limit. Updates can install
  automatically. A GitHub token in Settings raises the limit if you manage many
  addons.
- **CurseForge:** paste a `curseforge.com/wow/addons/<name>` link. This needs a
  CurseForge API key with access to World of Warcraft (Settings).
- **Data:** `~/.config/packmule/state.json`. Tokens are encrypted with the
  system keyring when one is available.

## The catalog

`catalog.json` lists the Browse tab's addons. Packmule fetches the copy in this
repository at startup, so additions reach everyone without an app release.

To add an addon, add an entry and refresh the game versions it supports:

```sh
GITHUB_TOKEN=$(gh auth token) npm run catalog
```

That reads each addon's latest `release.json` and fills in `flavors`. Addons
whose releases have no `release.json` keep the flavors written by hand, so only
list versions you've checked.

## Development

```sh
npm install      # also downloads the Electron binary
npm run dev      # hot-reloading dev build
npm run dist     # build the Linux packages into dist/
```

`./packaging/install-launcher.sh` adds a menu entry that runs your working copy,
rebuilding when the source changes.

Releases: bump `version` in `package.json`, commit, then push a matching tag
(`v0.2.0`). GitHub Actions builds the packages and publishes the release.

## License

MIT

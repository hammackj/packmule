# Packmule

An addon manager for World of Warcraft that installs and updates addons
straight from their **GitHub releases**. A native Linux desktop app written in
Rust with [GPUI](https://gpui.rs) (Zed's GPU-accelerated interface library):
one small binary, no browser engine, and no CPU use while idle.

- **Browse:** a catalog of addons that publish packaged GitHub releases,
  grouped by category like the in-game AddOns list. It only offers addons with a
  build for the game client you've selected (Retail, Classic, WoW Forever...).
- **Installed:** everything in your AddOns folder, grouped by category, with
  one-click updates. Addons you installed by hand are listed too, but never
  touched.
- **Picks up hand-installed addons:** each update check scans AddOns for
  addons you installed yourself. Ones it recognises (from the catalog's folder
  lists, or a GitHub link in their `.toc` whose latest release contains that
  folder and has a build for your client) become managed: up to date if the
  `.toc` version matches the latest release, otherwise ready to update.
- **Any GitHub addon:** paste a repository link to add one that isn't in the
  catalog. It needs releases with a packaged `.zip`.

## Install

Download the latest build from
[Releases](https://github.com/hammackj/packmule/releases):

- **Arch / CachyOS / Manjaro:** `sudo pacman -U packmule-<version>-1-x86_64.pkg.tar.zst`
- **Debian / Ubuntu:** `sudo apt install ./packmule_<version>_amd64.deb`
- **Anything else:** unpack `packmule-<version>-x86_64-linux.tar.gz` and run
  `./install.sh` (installs to `~/.local`; `./install.sh --uninstall` removes it)

Needs glibc 2.35 or newer and a GPU driver with Vulkan (OpenGL works as a
fallback). Wayland and X11 are both supported.

Open **Settings**, point Packmule at the folder that contains `_retail_`,
`_classic_era_`, `_forever_` and so on, then pick the game client at the top.
Packmule shows a banner when a newer version of itself is released.

Coming from Packmule 0.1 (the Electron version)? Your addons and settings
carry over; only a saved GitHub token needs entering again.

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
- **Downloads are checked:** each zip's SHA-256 must match the one GitHub
  publishes for the release asset.
- **Data:** `~/.config/packmule/state.json`. The GitHub token is kept in the
  system keyring (KWallet, GNOME Keyring) when one is available.

## The catalog

The Browse tab's addons, and their reviews, live in their own repository:
[hammackj/packmule-catalog](https://github.com/hammackj/packmule-catalog).
Packmule fetches its `catalog.json` at startup, so additions reach everyone
without an app release. `catalog.json` here is a snapshot built into the app
as the offline fallback; `packaging/sync-catalog.sh` refreshes it, and
releases download the latest one when they build.

Browse and Installed show two kinds of badge:

- **Verified source:** the repository was checked to be the author's official
  one, noting whether its zips are built by CI or uploaded by hand.
- **Reviewed:** Claude and Codex both read the code of that exact release zip
  (matched by SHA-256) looking for malicious behaviour and passed it, or a
  maintainer signed off a reviewer's doubt after reading the report. Other
  builds show **Not reviewed**, and a build whose review didn't pass shows
  **Flagged**. Badges are information only; nothing blocks an install.

The tools that maintain the catalog are here, and work on a
`packmule-catalog` checkout next to this repository (or `$PACKMULE_CATALOG`):

```sh
GITHUB_TOKEN=$(gh auth token) cargo run --example update-catalog   # flavors and folders
GITHUB_TOKEN=$(gh auth token) cargo run --example review -- <id>   # Claude + Codex review
```

## Development

```sh
cargo run -p packmule-app              # the app
cargo test -p packmule                 # the core's tests (offline, against a fake GitHub)
packaging/build-packages.sh            # .tar.gz, .deb and pacman package into dist/
```

| Path | What |
| --- | --- |
| `src/` | the core: game clients, `.toc` files, GitHub releases, installs, the catalog, state |
| `app/` | the GPUI interface |
| `examples/cli.rs` | runs the core from a terminal: `cargo run --example cli -- --config <dir> status` |
| `packaging/` | package builder, desktop entry, icons |

Building needs the headers for `libxcb`, `libxkbcommon` and `wayland`
(Debian: `libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev`).

To test against a copy of your data: `XDG_CONFIG_HOME=/tmp/pm cargo run -p packmule-app`
with a `state.json` in `/tmp/pm/packmule/`.

`./packaging/install-launcher.sh` adds a menu entry that runs your working copy,
rebuilding when the source changes.

Releases: bump `version` in `app/Cargo.toml` (and `Cargo.toml`), commit, then
push a matching tag (`v0.3.0`). GitHub Actions builds the packages and
publishes the release.

## License

MIT

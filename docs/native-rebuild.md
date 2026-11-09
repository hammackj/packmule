# Plan: native Packmule (no Electron)

Status: proposal, not started. Written 2026-11-09 against the Electron app on
branch `packmule` (74e8bc4).

## Why

The Electron build is a 125 MB package that ships a whole Chromium, idles at
roughly 200–300 MB of RAM, and needs Node to build. Packmule is a small
utility that is idle most of the time, so that is all overhead.

A native app with a GPU-drawn immediate-mode interface (the Dear ImGui style)
would be:

- **One file:** a single 8–12 MB executable, no runtime to install.
- **Light:** about 30–60 MB of RAM, and no CPU use while idle (the interface
  only redraws on input or when work finishes).
- **Fine on integrated graphics:** it draws with OpenGL, so any iGPU handles it.

## Toolkit

| Option | Result | Cost | Look |
| --- | --- | --- | --- |
| **Rust + egui (recommended)** | ~10 MB single binary | Rewrite everything (~3,000 lines) | Clean but plain; themeable |
| C++ + Dear ImGui | ~5 MB single binary | Rewrite everything, plus hand-picking HTTP, JSON, zip and keyring libraries | Developer-tool look |
| Tauri (Rust + system webview) | ~10 MB, needs WebKitGTK installed | Rewrite only the back end; keep the React interface | Same as now |

**Recommendation: Rust + egui.** It is the ImGui model (immediate mode, GPU
drawn) with a mature ecosystem for everything Packmule does: HTTPS, JSON, zip,
the system keyring and file dialogs. Dear ImGui would mean assembling those
pieces by hand in C++. Tauri keeps today's interface but still depends on a
browser engine on the user's system, which is the thing we're removing.

### Crates

- `eframe` / `egui` (window + interface), `egui_extras` (tables)
- `ureq` with rustls (HTTPS; supports the Range requests the zip index uses)
- `serde` / `serde_json` (state, catalog, GitHub API, release.json)
- `zip` (extraction)
- `keyring` (GitHub token in the Secret Service / KWallet; plain-text fallback
  as today)
- `rfd` (folder picker through the XDG portal)
- `directories` (config path), `open` (open links and the AddOns folder)

## Architecture

Same design as the Electron app, minus the process split:

```
ui thread (egui)  --Command-->  worker thread  --Event-->  ui thread
  draws the latest AppState      one job at a time          StateChanged / Log
```

- The **worker** owns the store and does all network and disk work, one
  command at a time. That is today's `serial()` queue, so installs still never
  overlap.
- The **interface** only draws the latest `AppState` snapshot and sends
  commands. The worker asks egui to repaint when something changes.
- A timer thread sends `CheckUpdates` every N hours.

### Modules to port (1:1 with `src/main`)

| Electron | Rust | Notes |
| --- | --- | --- |
| `game.ts` | `game.rs` | `.build.info` / `.flavor.info` parsing, interface numbers |
| `toc.ts` | `toc.rs` | flavor-specific .toc choice, escapes, Category, GitHub links |
| `github.ts` | `github.rs` | ETag cache, release streams, release.json build choice, rate-limit errors |
| `zipindex.ts` | `zipindex.rs` | central-directory read over Range requests, Zip64 |
| `installer.ts` | `installer.rs` | staging dir, zip-slip and symlink rejection, rename swap with rollback |
| `catalog.ts` | `catalog.rs` | bundled `catalog.json` via `include_str!`, remote refresh, flavor mapping |
| `store.ts` | `store.rs` | **same `~/.config/packmule/state.json` format**, so either app can run on the same data |
| `manager.ts` | `worker.rs` | statuses, adoption scan, update checks, auto-update, self-update check |
| `index.ts` | `main.rs` | single-instance lock (lock file in the config dir), window setup |
| `App.tsx`, `Browse.tsx`, `Settings.tsx` | `ui/*.rs` | see below |

### Interface

- **Top bar:** game client picker, Check for updates, Update all, Settings,
  Installed / Browse tabs.
- **Installed:** a table grouped by category with collapsing headers (egui's
  `CollapsingHeader`), plus the add-by-URL bar and the manual rows.
- **Browse:** a category side panel, search box and addon cards with Install.
- **Settings:** a modal window with the same fields as now.
- **Log** panel, banners (path errors, Packmule update), and a theme matching
  today's dark and gold palette.

## Phases

1. **Core library, no interface** (largest part). Port every module above with
   unit tests:
   - Parsers: build.info, .toc, release.json choice, release streams.
   - Zip index against real release zips, including a Zip64 one.
   - Installer safety: zip slip, symlinks, rollback when a rename fails.
   - The fake-WoW-folder test used for the adoption feature, as a `cargo test`.
2. **Interface:** Installed, Browse, Settings, log and banners, wired to the
   worker.
3. **Packaging:** GitHub Actions builds the release binary and publishes a
   `.tar.gz`, a pacman package (PKGBUILD) and a `.deb` (`cargo-deb`). No
   AppImage needed: the binary is already self-contained. The `.desktop` file
   and icons carry over.
4. **Parity and cutover:** run both apps against the same `state.json` and
   check the list below. Then the Rust app replaces the Electron one in the
   same repository, so the release page, the catalog URL and the update banner
   keep working. The Electron code stays in git history.

### Parity checklist

- [ ] Detects `_retail_`, `_classic_era_`, `_forever_` and others, with correct interface numbers
- [ ] Add by URL, including release-stream links (`/releases/tag/spoken/v3.8.4`)
- [ ] Picks the right build from release.json; falls back to the zip without one
- [ ] Install, update, repair and remove, with rollback on failure
- [ ] Manual addons listed; hand-installed addons adopted
- [ ] Browse: flavor filtering, show-all, search, categories, install
- [ ] Category grouping and collapsing in Installed
- [ ] Update checks on launch and every N hours; auto-update; ETags
- [ ] GitHub token stored in the keyring; existing state.json read as-is
- [ ] Packmule update banner; single instance; opens links and the AddOns folder

## Repository layout during the rebuild

Develop in `native/` on a branch, sharing the root `catalog.json`. At cutover,
the Rust project moves to the root and the Electron sources are deleted.

## Open decisions

1. Rust + egui, or one of the alternatives above.
2. Publish the Electron version as v0.1.0 now, or wait for the native one to
   be the first public release.

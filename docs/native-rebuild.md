# Plan: native Packmule on GPUI (no Electron)

Status: **phase 1 done** (2026-11-09, branch `native`): the core library in
`native/` with 37 tests. Phase 2 (the GPUI interface) is next. The Electron
app was released as v0.1.0 first.

## Why

The Electron build is a 125 MB package that ships a whole Chromium, idles at
roughly 200–300 MB of RAM, and needs Node to build. Packmule is a small
utility that is idle most of the time, so that is all overhead.

A native app drawn on the GPU would be:

- **One file:** a single executable, no runtime to install.
- **Light:** tens of MB of RAM instead of hundreds, and no CPU use while idle
  (the window only redraws on input or when work finishes).
- **Fine on integrated graphics:** any iGPU with a Vulkan driver handles it.

## Toolkit

| Option | Result | Cost | Look |
| --- | --- | --- | --- |
| **Rust + GPUI (chosen direction)** | single binary, needs Vulkan | Rewrite everything (~3,000 lines) | Polished, styled like Tailwind CSS; Zed's look |
| Rust + egui | ~10 MB single binary, OpenGL | Rewrite everything | Clean but plain, Dear ImGui style |
| Tauri (Rust + system webview) | ~10 MB, needs WebKitGTK | Rewrite only the back end; keep the React interface | Same as now |

**GPUI** is the GPU-accelerated Rust interface library Zed Industries built for
the Zed editor. In November 2026 Discord announced it is rebuilding its desktop
client on GPUI instead of Electron, so it has serious backing beyond Zed.
On Linux it draws through Vulkan and supports both Wayland and X11.

Widgets come from **gpui-component** (Longbridge): tables, sidebar, inputs,
buttons, modals, tabs, checkboxes and theming, which covers everything the
current React interface uses.

Things to watch with GPUI:

- **API churn:** it is developed inside the Zed repository and changes
  without notice. Pin an exact version (or git revision) of `gpui` and
  `gpui-component` together and upgrade deliberately.
- **Thinner documentation** than egui; the Zed source and gpui-component's
  examples are the main reference.
- **Vulkan required:** fine for any machine that runs WoW, but no OpenGL
  fallback.
- **Heavier build:** a large dependency tree and long first compile.

egui stays the fallback if GPUI turns out to be too unstable to pin.

### Crates

- `gpui` + `gpui-component` (window, interface, theming)
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
ui (GPUI)  --Command-->  worker thread  --Event-->  ui
  renders the AppState     one job at a time     StateChanged / Log
```

- The **worker** owns the store and does all network and disk work, one
  command at a time. That is today's `serial()` queue, so installs still never
  overlap.
- The **interface** only draws the latest `AppState` snapshot and sends
  commands. The worker's events update a GPUI model, which re-renders only the views that
  depend on it.
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
- **Installed:** a table grouped by category with collapsible groups, plus the add-by-URL bar and the manual rows.
- **Browse:** a category side panel, search box and addon cards with Install.
- **Settings:** a modal dialog with the same fields as now.
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

### Phase 1 notes

- `cargo test` in `native/` runs everything offline: a fake GitHub
  (`testutil::FakeHttp`) serves releases, release.json files and zips,
  including range requests. `manager_tests.rs` is the fake-WoW-folder
  scenario (adoption, updates, streams, catalog, repair, removal, restart).
- `cargo run --example cli -- --config <dir> status|check|update --yes` runs the
  core against a real state.json and WoW folder. Point `--config` at a copy.
  Against the real `_forever_` install it matches the Electron app exactly.
- Found by the live run: GitHub answers a cached request with a 304 that
  claims gzip with no body; reading it failed. Bodies of 204/304 are skipped.
- The GitHub token moves to the system keyring (Secret Service via `keyring`,
  pure Rust). A token the Electron app encrypted can't be read; the user
  enters it again. `plain:` tokens are read.
- Unknown fields in state.json (like the old `curseforgeKey`) are kept.

## Repository layout during the rebuild

Develop in `native/` on a branch, sharing the root `catalog.json`. At cutover,
the Rust project moves to the root and the Electron sources are deleted.

## Open decisions

1. GPUI (current direction), or egui if pinning GPUI proves painful.
2. Publish the Electron version as v0.1.0 now, or wait for the native one to
   be the first public release.

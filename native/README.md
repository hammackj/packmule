# Packmule native

The Rust rebuild of Packmule (see `../docs/native-rebuild.md`): the core
library here, and the GPUI interface in `app/`.

```sh
cargo run --release -p packmule-app   # the app (close the Electron Packmule first)
XDG_CONFIG_HOME=/tmp/pm cargo run -p packmule-app   # on a copy: /tmp/pm/packmule/state.json
```

```sh
cargo test -p packmule                                  # the core, offline
cargo run --example cli -- --config <dir> status        # read a state.json
cargo run --example cli -- --config <dir> check         # live update check (network)
cargo run --example cli -- --config <dir> update --yes  # install updates (changes AddOns!)
```

`--config` defaults to `~/.config/packmule`, shared with the Electron app.
Use a copy of `state.json` while testing.

| Module | Does |
| --- | --- |
| `game` | finds `_retail_`, `_forever_`... and their interface numbers |
| `toc` | reads `.toc` fields, picking the client's flavor-specific file |
| `github` | releases, release streams, ETag cache, release.json build choice |
| `zipindex` | lists a remote zip's folders with range requests (Zip64 too) |
| `installer` | staged installs with rollback; rejects zip slip and symlinks |
| `catalog` | the Browse catalog, bundled and fetched |
| `store` | `state.json` (Electron-compatible) and the keyring token |
| `manager` | statuses, installs, update checks, adopting hand installs |
| `worker` | runs the manager on a thread, one command at a time |

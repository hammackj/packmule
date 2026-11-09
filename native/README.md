# Packmule native

The Rust rebuild of Packmule (see `../docs/native-rebuild.md`). Phase 1, the
core with no interface, is done; the GPUI interface comes next.

```sh
cargo test                                              # everything, offline
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

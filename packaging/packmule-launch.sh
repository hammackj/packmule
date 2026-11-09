#!/usr/bin/env bash
# Launch Packmule from this working copy (the menu entry installed by
# install-launcher.sh). Release users install a package instead.
#
# Started from a menu there is no terminal to read, so a missing toolchain or
# a failed build would show as a window that never appears. Each of those is
# reported in a dialog instead.
set -u

REPO="$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)"
BIN="$REPO/target/release/packmule"

fail() {
    if command -v kdialog >/dev/null 2>&1; then
        kdialog --title "Packmule" --error "$1"
    elif command -v zenity >/dev/null 2>&1; then
        zenity --error --title="Packmule" --text="$1"
    else
        printf 'Packmule: %s\n' "$1" >&2
    fi
    exit 1
}

cd "$REPO" || fail "Could not enter $REPO"

# Rebuild when the source is newer than the binary, so a `git pull` or local
# edit is picked up without remembering to run cargo.
if [ ! -x "$BIN" ] || [ -n "$(find src app/src Cargo.toml Cargo.lock catalog.json -newer "$BIN" -print -quit)" ]; then
    command -v cargo >/dev/null 2>&1 ||
        fail "cargo was not found on PATH.\n\nPackmule is built with Rust: https://rustup.rs"
    log="$(cargo build --release -p packmule-app 2>&1)" ||
        fail "Build failed:\n\n$(printf '%s' "$log" | tail -15)"
fi

# exec so the launcher does not linger as a parent of the app.
exec "$BIN" "$@"

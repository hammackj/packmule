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

notify() {
    command -v notify-send >/dev/null 2>&1 && notify-send -a Packmule -i packmule "Packmule" "$1"
}

stale() {
    [ -n "$(find src app/src Cargo.toml Cargo.lock catalog.json -newer "$BIN" -print -quit)" ]
}

if [ ! -x "$BIN" ]; then
    # First launch: nothing to run until it's built.
    command -v cargo >/dev/null 2>&1 ||
        fail "cargo was not found on PATH.\n\nPackmule is built with Rust: https://rustup.rs"
    notify "Building Packmule for the first time. This takes a few minutes."
    log="$(cargo build --release -p packmule-app 2>&1)" || fail "Build failed:\n\n$(printf '%s' "$log" | tail -15)"
elif command -v cargo >/dev/null 2>&1 && stale; then
    # A release build takes minutes, so don't hold the window for it: run the
    # build we have and rebuild in the background for the next launch.
    # (Replacing the binary while it runs is fine on Linux.)
    (
        if log="$(nice -n 10 cargo build --release -p packmule-app 2>&1)"; then
            notify "Packmule was updated from the source. Restart it to use the new build."
        else
            printf '%s\n' "$log" >"${XDG_CACHE_HOME:-$HOME/.cache}/packmule-build.log"
            notify "Packmule's background rebuild failed; see ~/.cache/packmule-build.log"
        fi
    ) >/dev/null 2>&1 </dev/null &
    disown
fi

# exec so the launcher does not linger as a parent of the app.
exec "$BIN" "$@"

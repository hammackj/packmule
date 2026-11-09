#!/usr/bin/env bash
# Launch the Packmule desktop app.
#
# Started from a menu there is no terminal to read, so a missing dependency or
# a failed build would show as a window that never appears. Each of those is
# checked for and reported in a dialog instead.
set -u

REPO="$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)"
ELECTRON="$REPO/node_modules/electron/dist/electron"
MAIN="$REPO/out/main/index.js"

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

command -v npm >/dev/null 2>&1 ||
    fail "npm was not found on PATH.\n\nThe app is an Electron app and needs Node installed."

[ -x "$ELECTRON" ] ||
    fail "Dependencies are not installed.\n\nRun this in $REPO:\n\n    npm install"

cd "$REPO" || fail "Could not enter $REPO"

# Rebuild when the source is newer than the build, so a `git pull` or local
# edit is picked up without remembering to run `npm run build`.
if [ ! -f "$MAIN" ] || [ -n "$(find src -newer "$MAIN" -print -quit)" ]; then
    log="$(npm run build 2>&1)" ||
        fail "Build failed:\n\n$(printf '%s' "$log" | tail -15)"
fi

# exec so the launcher does not linger as a parent of the app.
exec "$ELECTRON" "$MAIN" "$@"

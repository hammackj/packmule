#!/usr/bin/env bash
# Copies catalog.json from the packmule-catalog checkout (next to this repo,
# or $PACKMULE_CATALOG) into this repo. That copy is the app's offline
# fallback, and Packmule 0.2.0 and older still fetch it from here, so commit
# it after catalog changes. Releases download the latest one when they build.
set -euo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"
SRC="${PACKMULE_CATALOG:-$REPO/../packmule-catalog}/catalog.json"
[ -f "$SRC" ] || { echo "No catalog at $SRC: clone https://github.com/hammackj/packmule-catalog next to this repo" >&2; exit 1; }
cp "$SRC" "$REPO/catalog.json"
git -C "$REPO" diff --stat -- catalog.json

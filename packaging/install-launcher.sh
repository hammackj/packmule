#!/usr/bin/env bash
# Install the desktop launcher for the current user.
#
# Per-user rather than system-wide: this runs from a working copy, so the Exec
# path points at wherever the repo happens to live and only means anything for
# the person who checked it out. No root, nothing outside $HOME.
#
# Safe to re-run — every step overwrites rather than appends, so this doubles as
# the way to fix the launcher after moving the repo.
set -eu

REPO="$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)"
APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICONS="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor"

mkdir -p "$APPS"
chmod +x "$REPO/packaging/packmule-launch.sh"

# The icon goes in the hicolor theme under its own name, which is what
# `Icon=packmule` resolves against. Every size is installed so the menu, the
# task switcher and the desktop each pick their own rather than rescaling one.
for size in 32 48 64 128 256 512; do
    src="$REPO/packaging/packmule-$size.png"
    [ -f "$src" ] || continue
    dir="$ICONS/${size}x${size}/apps"
    mkdir -p "$dir"
    cp -f "$src" "$dir/packmule.png"
done
mkdir -p "$ICONS/scalable/apps"
cp -f "$REPO/packaging/packmule.svg" "$ICONS/scalable/apps/packmule.svg"

# The Exec path is only knowable at install time.
sed "s|__REPO__|$REPO|g" "$REPO/packaging/packmule.desktop" > "$APPS/packmule.desktop"
chmod +x "$APPS/packmule.desktop"

DESKTOP_DIR="$(xdg-user-dir DESKTOP 2>/dev/null || echo "$HOME/Desktop")"
if [ -d "$DESKTOP_DIR" ]; then
    cp -f "$APPS/packmule.desktop" "$DESKTOP_DIR/packmule.desktop"
    chmod +x "$DESKTOP_DIR/packmule.desktop"
    # KDE trusts an executable desktop file; GNOME wants this metadata flag.
    gio set "$DESKTOP_DIR/packmule.desktop" metadata::trusted true 2>/dev/null || true
fi

command -v update-desktop-database >/dev/null 2>&1 &&
    update-desktop-database "$APPS" >/dev/null 2>&1 || true
command -v gtk-update-icon-cache >/dev/null 2>&1 &&
    gtk-update-icon-cache -qtf "$ICONS" >/dev/null 2>&1 || true

echo "Installed:"
echo "  $APPS/packmule.desktop"
[ -d "$DESKTOP_DIR" ] && echo "  $DESKTOP_DIR/packmule.desktop"
echo "  icons under $ICONS"

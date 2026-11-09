#!/bin/sh
# Installs Packmule for the current user (~/.local), or system-wide with
# PREFIX=/usr/local (run as root). Uninstall: ./install.sh --uninstall
set -eu
PREFIX="${PREFIX:-$HOME/.local}"
here="$(cd "$(dirname "$0")" && pwd)"
files="bin/packmule share/applications/packmule.desktop share/icons/hicolor/scalable/apps/packmule.svg"
for s in 32 48 64 128 256 512; do files="$files share/icons/hicolor/${s}x${s}/apps/packmule.png"; done

if [ "${1:-}" = "--uninstall" ]; then
  for f in $files; do rm -f "$PREFIX/$f"; done
  echo "Removed Packmule from $PREFIX (settings in ~/.config/packmule are kept)."
  exit 0
fi
for f in $files; do install -Dm"$( [ "$f" = bin/packmule ] && echo 755 || echo 644)" "$here/$f" "$PREFIX/$f"; done
# The menu entry runs the binary by full path, so it works even if $PREFIX/bin isn't on PATH.
sed -i "s|^Exec=.*|Exec=$PREFIX/bin/packmule|" "$PREFIX/share/applications/packmule.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database -q "$PREFIX/share/applications" || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$PREFIX/share/icons/hicolor" || true
echo "Installed Packmule to $PREFIX."
case ":$PATH:" in *":$PREFIX/bin:"*) ;; *) echo "Note: $PREFIX/bin is not on your PATH; use the menu entry, or run $PREFIX/bin/packmule." ;; esac

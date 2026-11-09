#!/usr/bin/env bash
# Builds the Linux packages from a release binary:
#   packaging/build-packages.sh [path/to/packmule] [out-dir]
# Produces packmule-<ver>-x86_64-linux.tar.gz, packmule_<ver>_amd64.deb (if
# dpkg-deb exists) and packmule-<ver>-1-x86_64.pkg.tar.zst (if bsdtar and zstd
# exist).
set -euo pipefail
cd "$(dirname "$0")/.."
bin="${1:-target/release/packmule}"
out="${2:-dist}"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' app/Cargo.toml | head -1)"
repo_root="$(pwd)"
[ -x "$bin" ] || { echo "no binary at $bin (cargo build --release -p packmule-app)"; exit 1; }
mkdir -p "$out"
out="$(cd "$out" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# The installed file tree, shared by every format.
tree="$work/tree"
install -Dm755 "$bin" "$tree/usr/bin/packmule"
install -Dm644 packaging/packmule.desktop "$tree/usr/share/applications/packmule.desktop"
install -Dm644 "$repo_root/packaging/packmule.svg" "$tree/usr/share/icons/hicolor/scalable/apps/packmule.svg"
for s in 32 48 64 128 256 512; do
  install -Dm644 "$repo_root/packaging/packmule-$s.png" "$tree/usr/share/icons/hicolor/${s}x${s}/apps/packmule.png"
done
install -Dm644 "$repo_root/LICENSE" "$tree/usr/share/licenses/packmule/LICENSE"
find "$tree" -exec touch -h -d "@${SOURCE_DATE_EPOCH:-$(date +%s)}" {} +
size_kb=$(du -sk "$tree/usr" | cut -f1)

# --- tar.gz: unpack anywhere, run install.sh
name="packmule-$version-x86_64-linux"
mkdir -p "$work/$name"
cp -a "$tree/usr/bin" "$tree/usr/share" "$work/$name/"
cp packaging/install.sh "$work/$name/"
cp "$repo_root/LICENSE" "$work/$name/"
tar -C "$work" -czf "$out/$name.tar.gz" "$name"
echo "built $out/$name.tar.gz"

# --- .deb
if command -v dpkg-deb >/dev/null; then
  deb="$work/deb"
  cp -a "$tree" "$deb"
  mkdir -p "$deb/DEBIAN"
  # Debian keeps licenses in /usr/share/doc/<pkg>/copyright.
  install -Dm644 "$repo_root/LICENSE" "$deb/usr/share/doc/packmule/copyright"
  rm -rf "$deb/usr/share/licenses"
  cat > "$deb/DEBIAN/control" <<CONTROL
Package: packmule
Version: $version
Section: games
Priority: optional
Architecture: amd64
Maintainer: hammackj <hammackj@users.noreply.github.com>
Installed-Size: $size_kb
Depends: libc6 (>= 2.35), libgcc-s1, libxcb1, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libvulkan1 | libegl1
Homepage: https://github.com/hammackj/packmule
Description: World of Warcraft addon manager
 Installs and updates World of Warcraft addons from their GitHub releases,
 with a catalog grouped by category and support for WoW Forever, Retail and
 Classic clients.
CONTROL
  dpkg-deb --root-owner-group -Zxz --build "$deb" "$out/packmule_${version}_amd64.deb" >/dev/null
  echo "built $out/packmule_${version}_amd64.deb"
else
  echo "skipped .deb (no dpkg-deb)"
fi

# --- pacman package, laid out the way makepkg does it
if command -v bsdtar >/dev/null && command -v zstd >/dev/null; then
  pkg="$work/pkg"
  cp -a "$tree" "$pkg"
  cat > "$pkg/.PKGINFO" <<PKGINFO
pkgname = packmule
pkgbase = packmule
pkgver = $version-1
pkgdesc = World of Warcraft addon manager that installs and updates addons from GitHub releases
url = https://github.com/hammackj/packmule
builddate = ${SOURCE_DATE_EPOCH:-$(date +%s)}
packager = hammackj <hammackj@users.noreply.github.com>
size = $((size_kb * 1024))
arch = x86_64
license = MIT
depend = glibc
depend = gcc-libs
depend = libxcb
depend = libxkbcommon
depend = libxkbcommon-x11
depend = wayland
depend = vulkan-icd-loader
optdepend = libglvnd: OpenGL fallback when Vulkan is unavailable
PKGINFO
  (
    cd "$pkg"
    LANG=C bsdtar -czf .MTREE --format=mtree \
      --options='!all,use-set,type,uid,gid,mode,time,size,md5,sha256,link' .PKGINFO usr
    LANG=C bsdtar --uid 0 --gid 0 --uname root --gname root -cf - .MTREE .PKGINFO usr \
      | zstd -q -19 -T0 -o "$out/packmule-$version-1-x86_64.pkg.tar.zst" -f
  )
  echo "built $out/packmule-$version-1-x86_64.pkg.tar.zst"
else
  echo "skipped pacman package (needs bsdtar and zstd)"
fi

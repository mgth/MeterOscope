#!/usr/bin/env bash
# Fabrique les paquets Linux de MeterOscope à partir d'un exécutable déjà compilé :
#   - MeterOscope-<version>-linux-x86_64.tar.gz : archive universelle + install.sh (par utilisateur)
#   - meteroscope_<version>_amd64.deb            : paquet Debian/Ubuntu (installation système)
# Usage : packaging/linux/package.sh <racine des sources> <exécutable> <version> <dossier de sortie>
# Le .deb est assemblé avec ar + tar (pas besoin de dpkg-deb).
set -euo pipefail
src="$(cd "$1" && pwd)"; exe="$2"; version="$3"; mkdir -p "$4"; out="$(cd "$4" && pwd)"
work="$(mktemp -d "${TMPDIR:-$out}/meteroscope-pkg.XXXXXX")"
trap 'rm -rf "$work"' EXIT
sizes=(16 24 32 48 64 128 256 512)

# ---------- archive universelle ----------
name="MeterOscope-$version-linux-x86_64"
t="$work/$name"
install -Dm755 "$exe" "$t/meteroscope"
install -Dm644 "$src/LICENSE" "$t/LICENSE"
install -Dm644 "$src/README.md" "$t/README.md"
install -Dm755 "$src/packaging/linux/install.sh" "$t/packaging/linux/install.sh"
install -Dm644 "$src/packaging/linux/meteroscope.desktop" "$t/packaging/linux/meteroscope.desktop"
install -Dm644 "$src/packaging/linux/meteroscope-mime.xml" "$t/packaging/linux/meteroscope-mime.xml"
install -Dm644 "$src/assets/icon/meteroscope.svg" "$t/assets/icon/meteroscope.svg"
for s in "${sizes[@]}"; do install -Dm644 "$src/assets/icon/meteroscope-$s.png" "$t/assets/icon/meteroscope-$s.png"; done
cat > "$t/install.sh" <<'EOF'
#!/usr/bin/env bash
# Installe MeterOscope pour l'utilisateur courant (~/.local), sans droits administrateur.
#   ./install.sh               installer
#   ./install.sh --uninstall   désinstaller
here="$(cd "$(dirname "$0")" && pwd)"
if [[ "${1:-}" == "--uninstall" ]]; then exec "$here/packaging/linux/install.sh" --uninstall; fi
exec "$here/packaging/linux/install.sh" "$here/meteroscope"
EOF
chmod 755 "$t/install.sh"
tar -C "$work" --owner=0 --group=0 --numeric-owner -czf "$out/$name.tar.gz" "$name"

# ---------- paquet Debian ----------
d="$work/deb"
install -Dm755 "$exe" "$d/usr/bin/meteroscope"
install -Dm644 "$src/packaging/linux/meteroscope.desktop" "$d/usr/share/applications/meteroscope.desktop"
install -Dm644 "$src/packaging/linux/meteroscope-mime.xml" "$d/usr/share/mime/packages/meteroscope.xml"
install -Dm644 "$src/assets/icon/meteroscope.svg" "$d/usr/share/icons/hicolor/scalable/apps/meteroscope.svg"
for s in "${sizes[@]}"; do
  install -Dm644 "$src/assets/icon/meteroscope-$s.png" "$d/usr/share/icons/hicolor/${s}x${s}/apps/meteroscope.png"
  install -Dm644 "$src/assets/icon/meteroscope-$s.png" "$d/usr/share/icons/hicolor/${s}x${s}/mimetypes/application-x-meteroscope.png"
done
install -Dm644 "$src/LICENSE" "$d/usr/share/doc/meteroscope/copyright"
installed_kb=$(du -sk "$d" | cut -f1)
c="$work/control"; mkdir -p "$c"
cat > "$c/control" <<EOF
Package: meteroscope
Version: $version
Architecture: amd64
Maintainer: Mathieu GRENET <https://github.com/mgth/MeterOscope>
Installed-Size: $installed_kb
Depends: libc6 (>= 2.35), libgcc-s1, libxkbcommon0
Recommends: fonts-noto-cjk, libegl1, libvulkan1, libwayland-client0 | libx11-6
Section: graphics
Priority: optional
Homepage: https://github.com/mgth/MeterOscope
Description: Measure lengths, angles and areas on images
 MeterOscope opens a photo or captures from a camera or USB microscope,
 calibrates the scale on a segment of known length, then measures lengths,
 paths, angles, areas, rectangles and circles, and counts markers.
 Interface in 10 languages.
EOF
( cd "$d" && find usr -type f -print0 | sort -z | xargs -0 md5sum ) > "$c/md5sums"
echo "2.0" > "$work/debian-binary"
tar -C "$c" --owner=0 --group=0 --numeric-owner -cJf "$work/control.tar.xz" ./control ./md5sums
tar -C "$d" --owner=0 --group=0 --numeric-owner -cJf "$work/data.tar.xz" ./usr
deb="$out/meteroscope_${version}_amd64.deb"
rm -f "$deb"
( cd "$work" && ar rc "$deb" debian-binary control.tar.xz data.tar.xz )

echo "$out/$name.tar.gz"
echo "$deb"

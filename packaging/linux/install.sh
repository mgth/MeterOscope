#!/usr/bin/env bash
# Installe MeterOscope pour l'utilisateur courant (aucun droit administrateur) :
# exécutable dans ~/.local/bin, entrée de menu, icônes, type de fichier .meteroscope.
#   packaging/linux/install.sh [chemin/vers/meteroscope]   (défaut : target/release/meteroscope)
#   packaging/linux/install.sh --uninstall
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/../.." && pwd)"
bin_dir="${XDG_BIN_HOME:-$HOME/.local/bin}"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
sizes=(16 24 32 48 64 128 256 512)

refresh() {
  update-mime-database "$data/mime" >/dev/null 2>&1 || true
  update-desktop-database "$data/applications" >/dev/null 2>&1 || true
  gtk-update-icon-cache -q -t "$data/icons/hicolor" >/dev/null 2>&1 || true
  if command -v kbuildsycoca6 >/dev/null; then kbuildsycoca6 --noincremental >/dev/null 2>&1 || true
  elif command -v kbuildsycoca5 >/dev/null; then kbuildsycoca5 --noincremental >/dev/null 2>&1 || true; fi
}

if [[ "${1:-}" == "--uninstall" ]]; then
  rm -f "$bin_dir/meteroscope" "$data/applications/meteroscope.desktop" "$data/mime/packages/meteroscope.xml" \
        "$data/icons/hicolor/scalable/apps/meteroscope.svg"
  for s in "${sizes[@]}"; do rm -f "$data/icons/hicolor/${s}x${s}/apps/meteroscope.png" \
                                     "$data/icons/hicolor/${s}x${s}/mimetypes/application-x-meteroscope.png"; done
  refresh
  echo "MeterOscope désinstallé."
  exit 0
fi

exe="${1:-$root/target/release/meteroscope}"
[[ -x "$exe" ]] || { echo "Exécutable introuvable : $exe (lancez d'abord : cargo build --release -p meteroscope)" >&2; exit 1; }

install -Dm755 "$exe" "$bin_dir/meteroscope"
install -Dm644 "$here/meteroscope.desktop" "$data/applications/meteroscope.desktop"
install -Dm644 "$here/meteroscope-mime.xml" "$data/mime/packages/meteroscope.xml"
install -Dm644 "$root/assets/icon/meteroscope.svg" "$data/icons/hicolor/scalable/apps/meteroscope.svg"
for s in "${sizes[@]}"; do
  install -Dm644 "$root/assets/icon/meteroscope-$s.png" "$data/icons/hicolor/${s}x${s}/apps/meteroscope.png"
  install -Dm644 "$root/assets/icon/meteroscope-$s.png" "$data/icons/hicolor/${s}x${s}/mimetypes/application-x-meteroscope.png"
done
refresh
echo "MeterOscope installé : $bin_dir/meteroscope (menu Applications → Graphisme)."
case ":$PATH:" in *":$bin_dir:"*) ;; *) echo "Note : $bin_dir n'est pas dans votre PATH." ;; esac

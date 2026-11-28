# MeterOscope

Application de mesure sur image : ouvrez une photo ou capturez depuis une caméra, définissez l'échelle, mesurez.
Écrite en Rust avec egui/eframe.

Interface en 10 langues : anglais, français, allemand, espagnol, italien, polonais, portugais (Brésil),
japonais, coréen et chinois simplifié. La langue du système est choisie par défaut ; une autre se règle
dans la barre d'état. Les traductions sont dans `crates/i18n/locales/` (une clé → un texte par fichier
JSON, l'anglais sert de référence).

## Fonctionnalités

- **Sources** : photo (EXIF pris en compte), image collée, caméra ou microscope USB en direct
  (mesure sur le flux, réticule gradué, capture), série de plusieurs images dans un même projet.
- **Référence** : échelle sur un segment de longueur connue, préréglages nommés, redressement
  d'une photo inclinée, correction de perspective d'une photo prise de biais.
- **Mesures** : longueur, chemin, distance point-droite, écart entre parallèles, profil
  d'intensité (largeur entre deux bords au sous-pixel), angle, inclinaison entre deux segments,
  rectangle (droit ou incliné), surface, cercle (centre-bord ou 3 points), arc, ellipse, comptage
  par marqueurs. Statistiques par type et par groupe.
- **Annotations** : texte et flèche.
- **Aide au tracé** : loupe, aimantation aux points remarquables, contrainte avec Maj.
- **Détection automatique** d'objets par seuillage : une surface par objet.
- **Image** : quarts de tour, miroirs, luminosité/contraste, barre d'échelle.
- **Exports** : CSV, image annotée, SVG, rapport PDF, copie dans le presse-papiers.

## Formats

| | Formats |
|---|---|
| Ouverture | Images PNG, JPEG, BMP, TIFF, WebP, GIF ; projet `.meteroscope` (une image ou une série) |
| Export | CSV (mesures, série, courbe de profil), image annotée PNG (aussi pour toute une série), SVG (image intégrée ou liée), rapport PDF |

## Raccourcis

| Touche | Action |
|---|---|
| Ctrl+O | Ouvrir : les images s'ajoutent au travail en cours, un projet le remplace |
| Ctrl+N | Nouveau travail |
| Ctrl+V | Coller une image ou un fichier (ajouté au travail) |
| Ctrl+S / Ctrl+Maj+S | Enregistrer / enregistrer sous |
| Ctrl+E | Exporter le CSV |
| Ctrl+Z / Ctrl+Y (ou Ctrl+Maj+Z) | Annuler / rétablir (ajouts d'images et nouveau travail compris) |
| Ctrl+0 / Ctrl+1 | Ajuster à la fenêtre / taille réelle |
| Page préc. / Page suiv. | Image précédente / suivante de la série |
| Échap | Annuler le tracé, revenir à la sélection |
| Suppr | Supprimer les mesures sélectionnées |
| Entrée, double-clic | Terminer un chemin ou une surface |
| Maj / Alt (pendant le tracé) | Contraindre / sans aimantation |
| S, E, H, Q | Sélection, échelle, redresser, perspective |
| L, B, D, G, W | Longueur, chemin, point-droite, parallèles, profil |
| A, I | Angle, inclinaison |
| R, O, P | Rectangle, rectangle 3 points, surface |
| C, T, U, V | Cercle, cercle 3 points, arc, ellipse |
| M, X, F | Marqueur, texte, flèche |

Molette : zoom sous le curseur ; glisser dans le vide, au bouton du milieu ou avec Espace : déplacer la vue.

## Installer

**Windows** : téléchargez `MeterOscope-<version>-setup-x64.exe` depuis la page
[Releases](https://github.com/mgth/MeterOscope/releases) (installation pour vous seul ou pour tous les
utilisateurs, association des fichiers `.meteroscope`, entrée « Ouvrir avec » pour les images),
ou le zip portable, qui se lance sans installation.

**Linux** : sur la page Releases, `meteroscope_<version>_amd64.deb` (Debian, Ubuntu…) ou
`MeterOscope-<version>-linux-x86_64.tar.gz` (toute distribution : décompresser puis `./install.sh`,
installation dans `~/.local` sans droits administrateur). Depuis les sources : après `cargo build --release -p meteroscope`, `packaging/linux/install.sh` installe pour
l'utilisateur courant l'exécutable, l'entrée de menu, les icônes et le type de fichier `.meteroscope`
(`--uninstall` pour retirer).

## Compiler

Rust ≥ 1.95.

```bash
cargo run --release -p meteroscope
```

Installeur Windows : [Inno Setup 6](https://jrsoftware.org/isinfo.php), puis, après la compilation
(la version doit être celle de `Cargo.toml` ; l'exécutable est pris dans `target\release`) :

```bash
iscc /DAppVersion=1.2.0 installer\meteroscope.iss
```

L'icône se régénère depuis ses sources SVG avec `assets/icon/generate.sh`.

## Licence

GPL-3.0-or-later — voir [LICENSE](LICENSE). Historique des versions : [CHANGELOG.md](CHANGELOG.md).

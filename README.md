# MeterOscope

Application de mesure sur image : ouvrez une photo ou capturez depuis une caméra, définissez l'échelle, mesurez.
Écrite en Rust avec egui/eframe.

Interface en 10 langues : anglais, français, allemand, espagnol, italien, polonais, portugais (Brésil),
japonais, coréen et chinois simplifié. La langue du système est choisie par défaut ; une autre se règle
dans la barre d'état. Les traductions sont dans `crates/i18n/locales/` (une clé → un texte par fichier
JSON, l'anglais sert de référence).

## Installer

**Windows** : téléchargez `MeterOscope-<version>-setup-x64.exe` depuis la page
[Releases](https://github.com/mgth/MeterOscope/releases) (installation pour vous seul ou pour tous les
utilisateurs, association des fichiers `.meteroscope`, entrée « Ouvrir avec » pour les images),
ou le zip portable, qui se lance sans installation.

**Linux** : après `cargo build --release -p meteroscope`, `packaging/linux/install.sh` installe pour
l'utilisateur courant l'exécutable, l'entrée de menu, les icônes et le type de fichier `.meteroscope`
(`--uninstall` pour retirer).

## Compiler

Rust ≥ 1.95.


```bash
cargo run --release -p meteroscope
```

Installeur Windows : [Inno Setup 6](https://jrsoftware.org/isinfo.php), puis

```bash
iscc /DAppVersion=1.1.0 installer/meteroscope.iss
```

L'icône se régénère depuis ses sources SVG avec `assets/icon/generate.sh`.

## Licence

GPL-3.0-or-later — voir [LICENSE](LICENSE).

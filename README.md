# MeterOscope

Application de mesure sur image : ouvrez une photo ou capturez depuis une caméra, définissez l'échelle, mesurez.
Écrite en Rust avec egui/eframe. Interface en français.

## Installer

**Windows** : téléchargez `MeterOscope-<version>-setup-x64.exe` depuis la page
[Releases](https://github.com/mgth/MeterOscope/releases) (installation pour vous seul ou pour tous les
utilisateurs, association des fichiers `.meteroscope`, entrée « Ouvrir avec » pour les images),
ou le zip portable, qui se lance sans installation.

## Compiler

Rust ≥ 1.95.


```bash
cargo run --release -p meteroscope
```

Installeur Windows : [Inno Setup 6](https://jrsoftware.org/isinfo.php), puis

```bash
iscc /DAppVersion=1.0.0 installer/meteroscope.iss
```

L'icône se régénère depuis ses sources SVG avec `assets/icon/generate.sh`.

## Licence

GPL-3.0-or-later — voir [LICENSE](LICENSE).

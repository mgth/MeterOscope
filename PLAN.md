# MeterOscope — plan de développement

Application de mesure sur image (dans l'esprit de Digimizer) : on ouvre une photo
ou on capture depuis une caméra, on définit l'échelle, on mesure.
Rust + egui/eframe 0.37. Interface en 10 langues (anglais, français, allemand, espagnol,
italien, polonais, portugais du Brésil, japonais, coréen, chinois simplifié).

## Principe d'ergonomie (prioritaire sur tout le reste)

- Trois gestes suffisent : **ouvrir → définir l'échelle → cliquer pour mesurer**.
- Un seul écran : outils à gauche, image au centre, résultats à droite, aide contextuelle en bas.
- Chaque outil dit quoi faire à chaque étape (barre d'état). Aucun mode caché.
- Tout est annulable (Ctrl+Z), donc aucune boîte de confirmation.
- Molette = zoom sous le curseur ; glisser dans le vide = déplacer la vue.
- L'outil reste actif après une mesure (mesures en série) ; Échap revient à la sélection.
- Loupe près du curseur pendant le tracé, pour viser au pixel.

## Architecture (workspace Cargo)

| Crate | Rôle | Dépend de |
|---|---|---|
| `crates/i18n` (`meteroscope-i18n`) | Traductions : catalogues JSON clé → texte (`locales/`, anglais en secours), macro `tr!`, langue du système par défaut, changement à chaud | serde_json, sys-locale |
| `crates/core` (`meteroscope-core`) | Modèle pur (ni egui ni E/S) : géométrie, formes et valeurs, étalonnage, préréglages d'échelle, orientation, perspective (homographie), profil d'intensité (bords sur la courbe 1D), groupes, barre d'échelle, mise à l'échelle entre résolutions, série d'images, document, annuler/rétablir, stats | i18n, serde |
| `crates/canvas` (`meteroscope-canvas`) | Vue image : zoom/déplacement, tracé et édition des mesures et annotations, aimantation, loupe, barre d'échelle à l'écran | core, i18n, egui |
| `crates/panels` (`meteroscope-panels`) | Palette d'outils groupée, liste des mesures (par groupe, sélection multiple) et stats, dialogues d'échelle et de perspective, préréglages, profil, barre d'échelle | core, i18n, egui |
| `crates/io` (`meteroscope-io`) | Chargement d'image (EXIF), projet `.meteroscope` (une image ou série), CSV, image annotée PNG, SVG, rapport PDF, redressement (filtres de reconstruction), reprojection de perspective, échantillonnage du profil, détection par seuillage, rotation/miroir, luminosité/contraste, polices du système | core, i18n, image, tiny-skia, ab_glyph, fontique, krilla |
| `crates/camera` (`meteroscope-camera`) | Liste des caméras, flux en direct dans un fil dédié, décodage MJPEG. Linux : V4L2 direct (`src/linux.rs`, crate `v4l`, lecture avec délai d'attente) ; autres systèmes : nokhwa (`src/other.rs`) | i18n, zune-jpeg, v4l / nokhwa |
| `crates/app` (binaire `meteroscope`) | Coquille : barre d'outils, raccourcis, fichiers et récents, réglages, presse-papiers, série d'images (vignettes), mode caméra (mesure sur le flux, réticule), détection, profil, exports, assemblage | tous |

Le haut de chaque `lib.rs` porte le **contrat public** consommé par `crates/app`.
Toutes les coordonnées du modèle sont en pixels de l'image de travail (`f64`) : l'image
d'origine, ou l'image corrigée quand une correction de perspective est posée.

## Mesures et outils (`crates/core/src/measure.rs`)

Référence :
- **Échelle** (E) : segment de longueur connue ; préréglages nommés, option « garder l'échelle
  pour l'image suivante ».
- **Redresser** (H) : un segment tracé devient horizontal ou vertical. Seul l'affichage pivote
  (`Document::rotation`, enregistrée dans le projet, annulable) ; la photo est rééchantillonnée
  une fois avec le filtre choisi (bicubique, Lanczos-3, B-spline, trois cisaillements, ou rapide
  par la carte graphique), les coordonnées des mesures ne changent pas.
- **Perspective** (Q) : quatre coins d'un objet rectangulaire photographié de biais ; la photo
  est reprojetée (`Document::perspective`), dimensions réelles facultatives (rapport d'aspect et
  échelle). La photo d'origine n'est pas modifiée.

Mesures :
- Lignes : **longueur** (L), **chemin** (B, ligne brisée), **distance point-droite** (D),
  **écart entre parallèles** (G), **profil d'intensité** (W : courbe et largeur entre deux bords
  détectés au sous-pixel, mi-hauteur ou gradient, export CSV de la courbe).
- Angles : **angle** (A, 3 points), **inclinaison** (I, entre deux segments distants, 0° à 90°,
  prolongés en pointillés jusqu'à leur intersection).
- Surfaces : **rectangle** par deux coins (R) ou incliné par trois points (O), inclinable par
  sa poignée ronde, **surface** (P, polygone : aire + périmètre).
- Cercles, arcs et ellipses : **cercle** centre-bord (C), **cercle par 3 points** (T), **arc**
  par 3 points (U : rayon, longueur d'arc, angle au centre, corde), **ellipse** (V : axes, aire,
  périmètre).
- **Marqueur** (M, comptage).

Annotations (ni valeurs, ni statistiques, ni ligne au CSV) : **texte** (X), **flèche** (F)
avec un texte facultatif.

La palette regroupe les outils semblables en 8 boutons (Sélection, Référence, Lignes, Angles,
Surfaces, Cercles, arcs et ellipses, Marqueur, Annotations) ; chaque outil garde son raccourci,
et l'outil montré par chaque groupe est retenu d'un lancement à l'autre.
Statistiques par type, et par groupe s'il y en a : n, moyenne, écart-type, min, max.

## Fonctions

- **Aimantation** : pendant un tracé ou le glisser d'une poignée, le point s'accroche au point
  remarquable le plus proche (points de contrôle, coins, milieux, centres, intersections) ;
  Alt la désactive.
- **Groupes** : groupe facultatif par mesure, liste « par groupe », statistiques par groupe,
  colonne Groupe au CSV ; sélection multiple dans la liste (Ctrl/Maj+clic).
- **Série d'images** : plusieurs images dans un même projet (format version 2), bande de
  vignettes, échelle commune, CSV et statistiques consolidés, images annotées de toute la série.
- **Détection par seuillage** : seuil d'Otsu, objets clairs ou sombres, filtres d'aire, objets
  du bord écartés, trous bouchés ou gardés ; une surface par objet, en un seul pas d'annulation.
- **Mode caméra** : toutes les mesures sur le flux en direct, pause, réticule gradué, capture
  qui garde échelle et mesures, mise à l'échelle des mesures au changement de résolution.
- **Barre d'échelle** : longueur ronde, coin au choix, identique à l'écran et à l'export.
- **Exports** : CSV, image annotée PNG (fichier ou presse-papiers), SVG (image intégrée ou
  liée), rapport PDF (image, échelle, tableau, statistiques), tableau copié.
- **Coller** (Ctrl+V) une image ou un fichier ; remplacer l'image est annulable.
- **Réglages retenus** : fichiers récents, dernier dossier, unité, décimales, langue, filtre de
  redressement, épaisseur des traits, outil retenu par groupe de la palette, taille de la fenêtre.

## Commandes

```bash
cargo run -p meteroscope --release            # lancer
cargo run -p meteroscope --release -- photo.jpg
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

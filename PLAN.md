# MeterOscope — plan de développement

Application de mesure sur image (dans l'esprit de Digimizer) : on ouvre une photo
ou on capture depuis une caméra, on définit l'échelle, on mesure.
Rust + egui/eframe 0.37. Interface en français.

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
| `crates/core` (`meteroscope-core`) | Modèle pur : géométrie, formes, valeurs, étalonnage, document, annuler/rétablir, stats | serde |
| `crates/canvas` (`meteroscope-canvas`) | Vue image : zoom/déplacement, tracé et édition des mesures, loupe | core, egui |
| `crates/panels` (`meteroscope-panels`) | Palette d'outils, liste des mesures + stats, dialogue d'échelle | core, egui |
| `crates/io` (`meteroscope-io`) | Chargement d'image (EXIF), projet `.meteroscope`, CSV, image annotée, rotation/miroir, luminosité/contraste | core, image |
| `crates/camera` (`meteroscope-camera`) | Liste des caméras, flux en direct dans un fil dédié | nokhwa |
| `crates/app` (binaire `meteroscope`) | Coquille : barre d'outils, raccourcis, fichiers, vue caméra, assemblage | tous |

Le haut de chaque `lib.rs` porte le **contrat public** consommé par `crates/app`.
Toutes les coordonnées du modèle sont en pixels de l'image d'origine (`f64`).

## Mesures

Longueur, chemin (ligne brisée), angle (3 points), surface (polygone : aire + périmètre),
rectangle (largeur, hauteur, aire ; par deux coins, ou incliné par trois points, et
inclinable par sa poignée ronde), cercle centre-bord et cercle par 3 points
(diamètre, rayon, aire, circonférence), marqueur (comptage), inclinaison entre deux
segments distants (0° à 90°), distance orthogonale d'un point à un segment (ou à son
prolongement, tracé en pointillés).
Statistiques par type : n, moyenne, écart-type, min, max.

Redressement : l'outil Redresser (H) fait pivoter l'affichage pour qu'un segment tracé
devienne horizontal ou vertical. L'image et les coordonnées des mesures ne changent pas
(rotation d'affichage `Document::rotation`, enregistrée dans le projet, annulable).
Un rectangle tracé par deux coins sur une vue redressée en suit l'horizontale et la verticale.
L'inclinaison prolonge ses segments en pointillés jusqu'à leur intersection, cote sur l'arc.

## Lots de développement

| Lot | Contenu | Responsable |
|---|---|---|
| 0 | Workspace, modèle `core` testé, contrats des crates, coquille `app` qui compile | session principale |
| 1 | `canvas` : navigation, tracé, sélection/édition, étiquettes, loupe | agent Opus |
| 2 | `panels` : palette d'outils, liste/stats, dialogue d'échelle | agent Opus |
| 3 | `io` : EXIF, projet, CSV, image annotée, transformations, réglages | agent Opus |
| 4 | `camera` : détection, flux, capture | agent Opus |
| 5 | Intégration, compilation complète, tests, essai réel de l'app | session principale |

Les lots 1 à 4 sont indépendants (un crate chacun) et menés en parallèle.

## Pistes pour la suite (hors v1)

Détection automatique d'objets (seuillage), mesure de profil d'intensité, coller une image
depuis le presse-papiers, traduction anglaise, préréglages d'échelle par caméra/objectif,
paquet d'installation.

## Commandes

```bash
cargo run -p meteroscope --release            # lancer
cargo run -p meteroscope --release -- photo.jpg
cargo test --workspace
```

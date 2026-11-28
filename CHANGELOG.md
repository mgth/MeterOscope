# Historique des versions

## 1.2.0 — non publiée

### Nouveautés

**Mesures et outils**
- Quatre nouvelles mesures : distance d'un point à une droite (D), écart entre deux parallèles (G),
  ellipse (V) et arc de cercle par trois points (U).
- Profil d'intensité (W) : un segment tracé en travers d'un trait ou d'une fente donne la courbe
  d'intensité et la largeur entre deux bords repérés au sous-pixel ; courbe exportable en CSV.
- Inclinaison (I) : angle de 0 à 90° entre deux segments éloignés.
- Rectangles inclinés : poignée de rotation, et nouvel outil rectangle par trois points (O).
- Annotations : texte (X) et flèche (F), saisis directement sur l'image.
- Palette regroupée : les outils semblables partagent un bouton, chacun garde son raccourci ;
  l'outil montré par chaque groupe est retenu d'un lancement à l'autre.
- Épaisseur des traits réglable dans la barre d'état.
- Aimantation : pendant le tracé, le point s'accroche aux points remarquables des autres mesures
  (coins, milieux, centres, intersections) ; Alt pour s'en passer.

**Préparer l'image**
- Redresser (H) : un segment tracé devient horizontal ou vertical ; quatre filtres de
  reconstruction au choix (menu Image).
- Correction de perspective (Q) : on clique les quatre coins d'un objet rectangulaire photographié
  de biais, la photo est reprojetée et les mesures deviennent justes dans son plan.
- Détection automatique d'objets par seuillage (bouton Détecter) : réglage du seuil avec aperçu en
  direct, une surface créée par objet.
- Barre d'échelle sur l'image, dans le coin choisi, identique à l'écran et à l'export.

**Organiser les mesures**
- Série d'images : plusieurs images dans un même projet, bande de vignettes, échelle commune,
  CSV et statistiques de toute la série.
- Groupes de mesures, liste et statistiques par groupe ; sélection multiple dans la liste
  (Ctrl+clic, Maj+clic) pour changer groupe, couleur ou visibilité, ou supprimer en une fois.
- Préréglages d'échelle nommés, et option « garder l'échelle pour l'image suivante ».

**Caméra**
- Toutes les mesures fonctionnent sur le flux en direct ; pause, réticule gradué, capture qui garde
  l'échelle et les mesures. Les mesures suivent un changement de résolution.

**Fichiers et exports**
- Coller une image ou un fichier avec Ctrl+V ; copier l'image annotée dans le presse-papiers.
- Remplacer l'image (ouvrir, coller, capturer) peut s'annuler avec Ctrl+Z.
- Exports SVG (image intégrée ou liée) et rapport PDF (image, échelle, tableau, statistiques).
- Fichiers récents, dernier dossier, unité, nombre de décimales et taille de la fenêtre retenus
  d'un lancement à l'autre.

### Corrections

- Quarts de tour et miroirs s'annulent avec le reste : image, mesures et angle reviennent ensemble.
- Les exports (PNG, copie, images d'une série, SVG, PDF) montrent la photo redressée et corrigée
  de la perspective comme à l'écran.
- Redressement calculé hors de l'interface, avec une mémoire bornée.
- La distance point-droite et l'écart entre parallèles ne donnent plus de valeur quand les deux
  points de la droite sont confondus.
- Le panneau des mesures ne s'élargit plus sans fin quand une mesure est sélectionnée.
- La détection ne fait plus fermer l'application quand sa zone devient vide.
- Un fichier de préréglages ou de réglages partiellement abîmé ne fait plus perdre le reste.
- L'enregistrement d'une série n'écrase jamais une photo existante.

Les projets enregistrés par les versions précédentes s'ouvrent toujours. Un projet de plusieurs
images ou avec correction de perspective utilise la version 2 du format, que les versions 1.1 et
antérieures refusent.

## 1.1.0 — 2026-11-24

- Interface en 10 langues (anglais, français, allemand, espagnol, italien, polonais, portugais du
  Brésil, japonais, coréen, chinois simplifié), installeur Windows dans les mêmes langues.
- Linux : entrée de menu, icônes, type de fichier `.meteroscope`, paquets `.deb` et `tar.gz`.

## 1.0.0 — 2026-11-24

Première version : ouvrir une photo ou capturer depuis une caméra, définir l'échelle, mesurer
longueurs, chemins, angles, surfaces, rectangles, cercles et compter des marqueurs, avec
statistiques ; projet `.meteroscope`, export CSV et image annotée ; installeur Windows.

use crate::calibration::{Calibration, Scale};
use crate::geometry::Pt;
use crate::measure::{Measurement, Quantity, Shape, ShapeKind, PALETTE};
use crate::orientation::Orientation;
use crate::perspective::{Homography, Perspective, order_quad};
use crate::stats::Stats;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const UNDO_LIMIT: usize = 200;

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Snapshot {
    measurements: Vec<Measurement>,
    calibration: Calibration,
    rotation: f64,
    orientation: Orientation,
    perspective: Option<Perspective>,
}

/// L'ensemble des mesures posées sur une image, avec étalonnage,
/// sélection et historique annuler/rétablir.
///
/// Règle d'édition : appeler [`Document::checkpoint`] **une fois avant**
/// toute modification directe (`get_mut`, `calibration`, `measurements`),
/// p. ex. au début d'un glisser. `add`, `remove` et `clear` le font eux-mêmes.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Document {
    pub measurements: Vec<Measurement>,
    pub calibration: Calibration,
    /// Rotation d'affichage de la photo (redressement), en degrés, positif =
    /// sens horaire à l'écran. L'image et les coordonnées des mesures ne
    /// changent pas : seule la vue pivote.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub rotation: f64,
    /// Quarts de tour et miroirs appliqués à l'image depuis son chargement.
    /// Annulable comme le reste : l'application remet ses pixels dans cette
    /// orientation après une annulation. Non enregistrée, puisque le projet
    /// référence un fichier image déjà dans l'orientation mesurée.
    #[serde(skip)]
    pub orientation: Orientation,
    /// Correction de perspective. Quand elle est présente, l'image de travail
    /// est la photo reprojetée (reconstruite à partir du fichier d'origine,
    /// qui n'est pas modifié) et les coordonnées des mesures sont en pixels
    /// de cette image corrigée.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub perspective: Option<Perspective>,
    /// Identifiant de la mesure sélectionnée.
    #[serde(skip)]
    pub selected: Option<u64>,
    next_id: u64,
    counters: BTreeMap<ShapeKind, u32>,
    #[serde(skip)]
    undo: Vec<Snapshot>,
    #[serde(skip)]
    redo: Vec<Snapshot>,
    /// Incrémenté à chaque modification enregistrée (pour « modifié non sauvegardé »).
    #[serde(skip)]
    revision: u64,
}

impl Document {
    pub fn new() -> Self {
        Self::default()
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            measurements: self.measurements.clone(),
            calibration: self.calibration.clone(),
            rotation: self.rotation,
            orientation: self.orientation,
            perspective: self.perspective.clone(),
        }
    }

    /// Enregistre l'état courant dans l'historique. À appeler avant une modification.
    pub fn checkpoint(&mut self) {
        self.undo.push(self.snapshot());
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.revision += 1;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn restore(&mut self, s: Snapshot) {
        self.measurements = s.measurements;
        self.calibration = s.calibration;
        self.rotation = s.rotation;
        self.orientation = s.orientation;
        self.perspective = s.perspective;
        if self.selected.is_some_and(|id| self.get(id).is_none()) {
            self.selected = None;
        }
        self.revision += 1;
    }

    pub fn undo(&mut self) -> bool {
        let Some(s) = self.undo.pop() else { return false };
        self.redo.push(self.snapshot());
        self.restore(s);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(s) = self.redo.pop() else { return false };
        self.undo.push(self.snapshot());
        self.restore(s);
        true
    }

    /// Change la rotation d'affichage (annulable). Renvoie `false` si rien ne change.
    pub fn set_rotation(&mut self, deg: f64) -> bool {
        let deg = crate::geometry::normalize_deg(deg);
        // Valeurs « rondes » : évite d'afficher −0,00° après un aller-retour.
        let deg = if deg.abs() < 1e-9 { 0.0 } else { deg };
        if !deg.is_finite() || deg == self.rotation {
            return false;
        }
        self.checkpoint();
        self.rotation = deg;
        true
    }

    /// Remplace l'échelle (annulable), p. ex. par un préréglage. Renvoie
    /// `false` si elle ne change pas.
    pub fn set_scale(&mut self, scale: Option<crate::calibration::Scale>) -> bool {
        let same = match (&self.calibration.scale, &scale) {
            (Some(a), Some(b)) => crate::presets::same_scale(a, b),
            (None, None) => true,
            _ => false,
        };
        if same {
            return false;
        }
        self.checkpoint();
        self.calibration.scale = scale;
        true
    }

    /// Remplace la correction de perspective (annulable, un seul point
    /// d'historique). Les mesures suivent : leurs points sont reportés de
    /// l'ancienne image de travail vers la nouvelle (voir [`Shape::reproject`]).
    /// Le redressement, relatif à l'ancienne image, est remis à zéro, et
    /// l'échelle, qui valait pour les anciens pixels, est remplacée par `scale`.
    /// Si la correction est la même et que seule l'échelle diffère (dimensions
    /// réelles ressaisies sur les mêmes coins), seule l'échelle est remplacée.
    /// Renvoie `false` si rien ne change.
    pub fn set_perspective(&mut self, perspective: Option<Perspective>, scale: Option<Scale>) -> bool {
        if perspective == self.perspective {
            // Sans correction ni avant ni après, l'étalonnage n'est pas concerné.
            if perspective.is_none() || scale == self.calibration.scale {
                return false;
            }
            self.checkpoint();
            self.calibration.scale = scale;
            return true;
        }
        let homography = |p: &Option<Perspective>| p.as_ref().map_or(Some(Homography::IDENTITY), Perspective::homography);
        let (Some(new), Some(back)) = (homography(&perspective), homography(&self.perspective).and_then(|h| h.inverse())) else {
            return false;
        };
        let map = new.after(&back);
        self.checkpoint();
        for m in &mut self.measurements {
            m.shape.reproject(|p| {
                // Un point passé derrière la ligne de fuite n'a pas d'image : il reste en place.
                let q = map.apply(p);
                if q.x.is_finite() && q.y.is_finite() { q } else { p }
            });
        }
        self.perspective = perspective;
        self.rotation = 0.0;
        self.calibration.scale = scale;
        true
    }

    /// Corrige la perspective à partir des quatre coins `quad` d'un objet
    /// rectangulaire, cliqués dans un ordre quelconque sur l'image de travail
    /// (déjà corrigée ou non). `img` : taille de la photo d'origine. `real` :
    /// largeur et hauteur réelles de l'objet et leur unité, si elles sont
    /// connues ; elles fixent alors le rapport d'aspect et l'échelle, sinon le
    /// rapport est estimé et l'échelle reste à définir.
    /// Renvoie `false` (sans rien changer) si le quadrilatère est inutilisable.
    pub fn correct_perspective(&mut self, quad: [Pt; 4], img: (u32, u32), real: Option<(f64, f64, &str)>) -> bool {
        let Some(quad) = order_quad(quad, self.rotation) else { return false };
        // Coins dans la photo d'origine.
        let quad = match &self.perspective {
            Some(old) => match old.homography().and_then(|h| h.inverse()) {
                Some(back) => quad.map(|p| back.apply(p)),
                None => return false,
            },
            None => quad,
        };
        let positive = |v: f64| v.is_finite() && v > 0.0;
        let real = real.filter(|(w, h, unit)| positive(*w) && positive(*h) && !unit.trim().is_empty());
        let Some(p) = Perspective::from_quad(quad, img, real.map(|(w, h, _)| w / h)) else { return false };
        let scale = real.map(|(w, _, unit)| Scale { units_per_px: w / p.rect_size().0, unit: unit.trim().to_owned() });
        self.set_perspective(Some(p), scale)
    }

    /// Ajoute une mesure (nom et couleur automatiques), la sélectionne et renvoie son id.
    pub fn add(&mut self, shape: Shape) -> u64 {
        self.checkpoint();
        self.next_id += 1;
        let id = self.next_id;
        let kind = shape.kind();
        let n = self.counters.entry(kind).or_insert(0);
        *n += 1;
        let color = PALETTE[ShapeKind::ALL.iter().position(|k| *k == kind).unwrap_or(0) % PALETTE.len()];
        self.measurements.push(Measurement { id, name: format!("{} {}", kind.label(), n), shape, color, visible: true });
        self.selected = Some(id);
        id
    }

    pub fn remove(&mut self, id: u64) -> bool {
        if self.get(id).is_none() {
            return false;
        }
        self.checkpoint();
        self.measurements.retain(|m| m.id != id);
        if self.selected == Some(id) {
            self.selected = None;
        }
        true
    }

    /// Supprime toutes les mesures (l'étalonnage est conservé).
    pub fn clear(&mut self) {
        if self.measurements.is_empty() {
            return;
        }
        self.checkpoint();
        self.measurements.clear();
        self.selected = None;
    }

    pub fn get(&self, id: u64) -> Option<&Measurement> {
        self.measurements.iter().find(|m| m.id == id)
    }

    /// Accès mutable ; penser à [`Document::checkpoint`] avant de modifier.
    pub fn get_mut(&mut self, id: u64) -> Option<&mut Measurement> {
        self.measurements.iter_mut().find(|m| m.id == id)
    }

    pub fn selected(&self) -> Option<&Measurement> {
        self.selected.and_then(|id| self.get(id))
    }

    /// Nombre de marqueurs (comptage).
    pub fn marker_count(&self) -> usize {
        self.measurements.iter().filter(|m| m.shape.kind() == ShapeKind::Marker).count()
    }

    /// Statistiques par grandeur sur la valeur *principale* des mesures
    /// d'un même type : (type, grandeur, unité, stats).
    pub fn summary(&self) -> Vec<(ShapeKind, Quantity, String, Stats)> {
        let mut groups: BTreeMap<ShapeKind, (Quantity, String, Vec<f64>)> = BTreeMap::new();
        for m in &self.measurements {
            if let Some(v) = m.shape.values(&self.calibration).into_iter().next() {
                groups.entry(m.shape.kind()).or_insert_with(|| (v.quantity, v.unit.clone(), vec![])).2.push(v.value);
            }
        }
        groups
            .into_iter()
            .filter_map(|(k, (q, u, vals))| Stats::of(&vals).map(|s| (k, q, u, s)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Pt;

    #[test]
    fn add_undo_redo() {
        meteroscope_i18n::set_language("fr");
        let mut d = Document::new();
        let a = d.add(Shape::Length { a: Pt::new(0., 0.), b: Pt::new(3., 4.) });
        let b = d.add(Shape::Length { a: Pt::new(0., 0.), b: Pt::new(6., 8.) });
        assert_eq!(d.get(b).unwrap().name, "Longueur 2");
        assert_eq!(d.selected, Some(b));
        let s = d.summary();
        assert_eq!((s[0].3.n, s[0].3.mean), (2, 7.5));
        assert!(d.remove(a));
        assert_eq!(d.measurements.len(), 1);
        assert!(d.undo());
        assert_eq!(d.measurements.len(), 2);
        assert!(d.undo() && d.undo() && !d.undo());
        assert!(d.measurements.is_empty() && d.selected.is_none());
        assert!(d.redo() && d.redo());
        assert_eq!(d.measurements.len(), 2);
        assert!(d.set_rotation(-3.5) && !d.set_rotation(-3.5));
        assert!(d.undo());
        assert_eq!((d.rotation, d.measurements.len()), (0.0, 2));
        assert!(d.redo());
        assert_eq!(d.rotation, -3.5);
        assert!(d.set_rotation(363.5));
        assert_eq!(d.rotation, 3.5);
    }

    #[test]
    fn same_corners_new_dimensions_change_the_scale() {
        meteroscope_i18n::set_language("fr");
        // Rectangle vu de face, 200 × 100 px.
        let quad = [Pt::new(100., 100.), Pt::new(300., 100.), Pt::new(300., 200.), Pt::new(100., 200.)];
        let mut d = Document::new();
        let side = d.add(Shape::Length { a: quad[0], b: quad[1] });
        let value = |d: &Document| d.get(side).unwrap().shape.values(&d.calibration)[0].clone();
        assert!(d.correct_perspective(quad, (400, 300), Some((20.0, 10.0, "mm"))));
        assert!((value(&d).value - 20.0).abs() < 1e-9 && value(&d).unit == "mm");
        let dst = d.perspective.clone().unwrap().dst;
        d.set_rotation(3.0);

        // Mêmes coins, autre unité : la géométrie ne bouge pas, l'échelle si.
        let corrected = d.perspective.clone();
        assert!(d.correct_perspective(dst, (400, 300), Some((2.0, 1.0, "cm"))));
        assert_eq!(d.perspective, corrected);
        assert!((value(&d).value - 2.0).abs() < 1e-9 && value(&d).unit == "cm");
        assert_eq!(d.rotation, 3.0, "le redressement n'a pas de raison de changer");
        // Dimension deux fois trop petite, même rapport : on peut la corriger.
        assert!(d.correct_perspective(dst, (400, 300), Some((4.0, 2.0, "cm"))));
        assert!((value(&d).value - 4.0).abs() < 1e-9);
        // Rien de nouveau : rien à annuler.
        assert!(!d.correct_perspective(dst, (400, 300), Some((4.0, 2.0, "cm"))));

        // Chaque changement d'étalonnage s'annule seul.
        assert!(d.undo());
        assert!((value(&d).value - 2.0).abs() < 1e-9 && value(&d).unit == "cm");
        assert!(d.undo());
        assert!((value(&d).value - 20.0).abs() < 1e-9 && value(&d).unit == "mm");
        assert_eq!(d.perspective, corrected);

        // Retirer une correction absente ne touche pas à l'étalonnage.
        let mut plain = Document::new();
        plain.calibration.set_from_reference(10.0, 1.0, "mm");
        assert!(!plain.set_perspective(None, None));
        assert!(plain.calibration.is_calibrated());
    }

    #[test]
    fn perspective_correction_is_undoable() {
        meteroscope_i18n::set_language("fr");
        // Plaque de 200 × 100 mm photographiée de biais.
        let quad = [Pt::new(150., 120.), Pt::new(620., 180.), Pt::new(560., 470.), Pt::new(90., 380.)];
        let mut d = Document::new();
        d.calibration.set_from_reference(10.0, 1.0, "mm");
        d.set_rotation(4.0);
        // Diagonale de la plaque, et une mesure sur son grand côté.
        let diag = d.add(Shape::Length { a: quad[0], b: quad[2] });
        let side = d.add(Shape::Length { a: quad[0], b: quad[1] });
        let before = d.clone();

        // Quadrilatère inutilisable : rien ne change.
        assert!(!d.correct_perspective([quad[0]; 4], (800, 600), None));
        assert!(d.perspective.is_none() && d.measurements == before.measurements);

        // Coins cliqués dans le désordre, dimensions réelles connues.
        assert!(d.correct_perspective([quad[2], quad[0], quad[3], quad[1]], (800, 600), Some((200.0, 100.0, " mm "))));
        assert_eq!(d.rotation, 0.0);
        let value = |d: &Document, id| d.get(id).unwrap().shape.values(&d.calibration)[0].clone();
        assert!((value(&d, side).value - 200.0).abs() < 1e-6 && value(&d, side).unit == "mm");
        assert!((value(&d, diag).value - 200f64.hypot(100.0)).abs() < 1e-6);

        // Deuxième correction sur l'image déjà corrigée, sans dimensions :
        // la plaque reste un rectangle, l'échelle est à redéfinir.
        let dst = d.perspective.clone().unwrap().dst;
        assert!(d.correct_perspective(dst, (800, 600), None));
        assert!(!d.calibration.is_calibrated());
        let (s, g) = (value(&d, side).value, value(&d, diag).value);
        let p = d.perspective.clone().unwrap();
        let (w, h) = p.rect_size();
        assert!((s - w).abs() < 1e-6 && (g - w.hypot(h)).abs() < 1e-6);
        assert!(p.src.iter().zip(&quad).all(|(a, b)| a.dist(*b) < 1e-6), "coins ramenés dans la photo d'origine");

        // Retrait : les mesures reviennent à leur place dans la photo.
        assert!(d.set_perspective(None, None) && !d.set_perspective(None, None));
        let Shape::Length { a, b } = d.get(diag).unwrap().shape else { panic!() };
        assert!(a.dist(quad[0]) < 1e-6 && b.dist(quad[2]) < 1e-6);

        // Trois annulations ramènent l'état d'avant, redressement et échelle compris.
        assert!(d.undo() && d.undo() && d.undo());
        assert_eq!((d.measurements.clone(), d.rotation, d.perspective.clone()), (before.measurements.clone(), 4.0, None));
        assert_eq!(d.calibration, before.calibration);
        assert!(d.redo());
        assert!(d.perspective.is_some());
    }

    #[test]
    fn rectangle_follows_the_correction() {
        meteroscope_i18n::set_language("fr");
        // Un rectangle penché, déplacé par une similitude (quart de tour +
        // échelle 2) : il garde ses proportions et suit la rotation.
        let mut shape = Shape::Rect { a: Pt::new(10., 10.), b: Pt::new(50., 40.), angle: 20.0 };
        let (w, h) = crate::geometry::rect_size(Pt::new(10., 10.), Pt::new(50., 40.), 20.0);
        shape.reproject(|p| Pt::new(-2.0 * p.y, 2.0 * p.x));
        let Shape::Rect { a, b, angle } = shape else { panic!() };
        let (w2, h2) = crate::geometry::rect_size(a, b, angle);
        assert!((w2 - 2.0 * w).abs() < 1e-9 && (h2 - 2.0 * h).abs() < 1e-9, "{w} × {h} → {w2} × {h2}");
        let Shape::Rect { angle, .. } = shape else { panic!() };
        assert!((angle - 110.0).abs() < 1e-9, "{angle}");
    }
}

//! Changement de résolution d'une source d'images (flux caméra) : les
//! coordonnées des mesures sont en pixels image et doivent suivre.

use crate::document::Document;

/// Écart relatif toléré entre les rapports d'aspect (arrondis des modes
/// vidéo, p. ex. 854 × 480 face à 1280 × 720).
const ASPECT_TOLERANCE: f64 = 0.01;

/// Résultat de [`rescale`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Rescale {
    /// Même taille : rien à faire.
    Same,
    /// Même rapport d'aspect : mesures et échelle multipliées par ce facteur
    /// (nouvelle taille / ancienne).
    Scaled(f64),
    /// Rapport d'aspect différent (recadrage inconnu) : document inchangé,
    /// les mesures ne correspondent plus à l'image.
    Mismatch,
}

/// Facteur uniforme qui fait passer de `from` à `to` (largeur, hauteur),
/// `None` si les rapports d'aspect diffèrent ou si une taille est nulle.
pub fn scale_factor(from: (u32, u32), to: (u32, u32)) -> Option<f64> {
    if from.0 == 0 || from.1 == 0 || to.0 == 0 || to.1 == 0 {
        return None;
    }
    let sx = to.0 as f64 / from.0 as f64;
    let sy = to.1 as f64 / from.1 as f64;
    ((sx / sy - 1.0).abs() <= ASPECT_TOLERANCE).then_some((sx + sy) / 2.0)
}

/// Adapte le document à une image passée de la taille `from` à la taille
/// `to` : si le rapport d'aspect est le même, toutes les mesures sont mises à
/// l'échelle (annulable) et l'étalonnage aussi, de sorte que les valeurs en
/// unités réelles ne changent pas. Sinon le document reste tel quel.
pub fn rescale(doc: &mut Document, from: (u32, u32), to: (u32, u32)) -> Rescale {
    if from == to {
        return Rescale::Same;
    }
    let Some(s) = scale_factor(from, to) else { return Rescale::Mismatch };
    if !doc.measurements.is_empty() || doc.calibration.is_calibrated() {
        doc.checkpoint();
        for m in &mut doc.measurements {
            m.shape.map_points(|p| crate::Pt::new(p.x * s, p.y * s));
        }
        if let Some(scale) = &mut doc.calibration.scale {
            scale.units_per_px /= s;
        }
    }
    Rescale::Scaled(s)
}

/// Taille d'image à laquelle se rapportent les coordonnées d'un document
/// dont la source change de résolution (flux caméra). Après une
/// incompatibilité de rapport d'aspect, la taille de référence est gardée
/// tant que la géométrie du document (points, échelle) ne change pas — un
/// nom, une couleur ou une visibilité n'y comptent pas : revenir à une
/// résolution compatible avec elle remet mesures et échelle en place.
#[derive(Clone, Debug, Default)]
pub struct Reference {
    size: Option<(u32, u32)>,
    /// Taille incompatible affichée, et géométrie du document à ce moment.
    mismatch: Option<((u32, u32), Geometry)>,
}

/// Ce qui dépend de la résolution dans un document : points de chaque
/// mesure et échelle (un angle de rectangle n'en dépend pas).
#[derive(Clone, Debug, PartialEq)]
struct Geometry {
    points: Vec<(u64, Vec<crate::Pt>)>,
    units_per_px: Option<f64>,
}

impl Geometry {
    fn of(doc: &Document) -> Self {
        Self {
            points: doc.measurements.iter().map(|m| (m.id, m.shape.points())).collect(),
            units_per_px: doc.calibration.scale.as_ref().map(|s| s.units_per_px),
        }
    }
}

impl Reference {
    /// Taille à laquelle les mesures et l'échelle sont exprimées.
    pub fn size(&self) -> Option<(u32, u32)> {
        self.size
    }

    /// L'image vaut désormais `size` : adapte le document (voir [`rescale`]).
    pub fn follow(&mut self, doc: &mut Document, size: (u32, u32)) -> Rescale {
        // Géométrie modifiée pendant l'incompatibilité (mesure posée ou
        // déplacée, échelle redéfinie) : elle se rapporte désormais à la
        // taille qui était affichée.
        if let Some((shown, geo)) = self.mismatch.take()
            && Geometry::of(doc) != geo
        {
            self.size = Some(shown);
        }
        let Some(from) = self.size else {
            self.size = Some(size);
            return Rescale::Same;
        };
        let r = rescale(doc, from, size);
        let empty = doc.measurements.is_empty() && !doc.calibration.is_calibrated();
        if r == Rescale::Mismatch && !empty {
            self.mismatch = Some((size, Geometry::of(doc)));
        } else {
            self.size = Some(size);
        }
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Pt, Shape};

    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn facteur_selon_le_rapport_d_aspect() {
        assert_eq!(scale_factor((1920, 1080), (1280, 720)), Some(2.0 / 3.0));
        assert_eq!(scale_factor((640, 480), (1280, 960)), Some(2.0));
        // 854 × 480 n'est pas exactement du 16:9 : toléré.
        assert!(scale_factor((1280, 720), (854, 480)).is_some_and(|s| (s - 0.6669).abs() < 1e-3));
        assert_eq!(scale_factor((1920, 1080), (640, 480)), None);
        assert_eq!(scale_factor((0, 0), (640, 480)), None);
    }

    #[test]
    fn mesures_et_echelle_suivent_la_resolution() {
        meteroscope_i18n::set_language("fr");
        let mut d = Document::new();
        assert!(d.calibration.set_from_reference(100.0, 5.0, "mm"));
        let id = d.add(Shape::Length { a: Pt::new(300.0, 150.0), b: Pt::new(600.0, 550.0) });
        d.add(Shape::Rect { a: Pt::new(10.0, 20.0), b: Pt::new(110.0, 70.0), angle: 12.0 });
        let before: Vec<f64> = d.measurements.iter().map(|m| m.shape.values(&d.calibration)[0].value).collect();
        let rev = d.revision();

        assert_eq!(rescale(&mut d, (1920, 1080), (1280, 720)), Rescale::Scaled(2.0 / 3.0));
        let Shape::Length { a, b } = d.get(id).unwrap().shape else { panic!() };
        assert!(approx(a.x, 200.0) && approx(a.y, 100.0) && approx(b.x, 400.0) && approx(b.y, 550.0 * 2.0 / 3.0));
        // Les valeurs réelles sont conservées (longueur, aire du rectangle incliné).
        let after: Vec<f64> = d.measurements.iter().map(|m| m.shape.values(&d.calibration)[0].value).collect();
        for (x, y) in before.iter().zip(&after) {
            assert!(approx(*x, *y), "{x} ≠ {y}");
        }
        assert!(matches!(d.measurements[1].shape, Shape::Rect { angle, .. } if angle == 12.0));
        // Annulable en un seul pas.
        assert!(d.revision() > rev);
        assert!(d.undo());
        assert!(matches!(d.get(id).unwrap().shape, Shape::Length { a, .. } if a == Pt::new(300.0, 150.0)));
        assert!(approx(d.calibration.scale.as_ref().unwrap().units_per_px, 0.05));
    }

    #[test]
    fn rapport_different_ou_meme_taille() {
        meteroscope_i18n::set_language("fr");
        let mut d = Document::new();
        d.add(Shape::Marker { p: Pt::new(10.0, 10.0) });
        let copy = d.measurements.clone();
        let rev = d.revision();
        assert_eq!(rescale(&mut d, (1920, 1080), (640, 480)), Rescale::Mismatch);
        assert_eq!(rescale(&mut d, (640, 480), (640, 480)), Rescale::Same);
        assert_eq!((d.measurements.clone(), d.revision()), (copy, rev));
        // Document vide : rien d'enregistré dans l'historique.
        let mut e = Document::new();
        assert_eq!(rescale(&mut e, (640, 480), (320, 240)), Rescale::Scaled(0.5));
        assert!(!e.can_undo());
    }

    #[test]
    fn reference_gardee_apres_incompatibilite() {
        meteroscope_i18n::set_language("fr");
        let mut d = Document::new();
        let id = d.add(Shape::Marker { p: Pt::new(640.0, 360.0) });
        let mut r = Reference::default();
        assert_eq!(r.follow(&mut d, (1280, 720)), Rescale::Same);
        // 4:3 : incompatible, coordonnées inchangées, référence gardée.
        assert_eq!(r.follow(&mut d, (640, 480)), Rescale::Mismatch);
        assert_eq!(r.size(), Some((1280, 720)));
        // 640 × 360 est la moitié de la référence, pas une nouvelle incompatibilité.
        assert_eq!(r.follow(&mut d, (640, 360)), Rescale::Scaled(0.5));
        assert!(matches!(d.get(id).unwrap().shape, Shape::Marker { p } if p == Pt::new(320.0, 180.0)));
        assert_eq!(r.size(), Some((640, 360)));
    }

    #[test]
    fn renommer_ne_change_pas_la_reference() {
        meteroscope_i18n::set_language("fr");
        let mut d = Document::new();
        let id = d.add(Shape::Marker { p: Pt::new(640.0, 360.0) });
        let mut r = Reference::default();
        r.follow(&mut d, (1280, 720));
        assert_eq!(r.follow(&mut d, (640, 480)), Rescale::Mismatch);
        // Nom, couleur, visibilité : révision changée, géométrie identique.
        d.checkpoint();
        let m = d.get_mut(id).unwrap();
        m.name = "Repère".into();
        m.color = [1, 2, 3];
        m.visible = false;
        assert_eq!(r.follow(&mut d, (640, 360)), Rescale::Scaled(0.5));
        assert!(matches!(d.get(id).unwrap().shape, Shape::Marker { p } if p == Pt::new(320.0, 180.0)));
    }

    #[test]
    fn document_modifie_pendant_l_incompatibilite() {
        meteroscope_i18n::set_language("fr");
        let mut d = Document::new();
        d.add(Shape::Marker { p: Pt::new(10.0, 10.0) });
        let mut r = Reference::default();
        r.follow(&mut d, (1280, 720));
        assert_eq!(r.follow(&mut d, (640, 480)), Rescale::Mismatch);
        // Mesure posée en 640 × 480 : c'est désormais la référence.
        d.add(Shape::Marker { p: Pt::new(320.0, 240.0) });
        assert_eq!(r.follow(&mut d, (1280, 960)), Rescale::Scaled(2.0));
        assert_eq!(r.size(), Some((1280, 960)));
        // Document vide : la référence suit simplement l'image.
        let mut e = Document::new();
        let mut r = Reference::default();
        r.follow(&mut e, (1280, 720));
        assert_eq!(r.follow(&mut e, (640, 480)), Rescale::Mismatch);
        assert_eq!(r.size(), Some((640, 480)));
    }
}

//! Machine à états du tracé d'une nouvelle forme (logique pure, sans egui).
//!
//! Règles des gestes :
//! - le premier point est posé à l'appui ; si l'on glisse avant de relâcher,
//!   le point de relâchement devient le point suivant (presser-glisser-relâcher) ;
//! - les points suivants sont posés au relâchement d'un clic (clic puis clic) ;
//! - un point trop proche du précédent est ignoré (évite les doublons d'un double-clic) ;
//! - Inclinaison : deux segments, chacun tracé comme une longueur ; le second
//!   peut partir de l'extrémité du premier (sommet commun) ;
//! - Chemin/Surface se terminent par [`Draft::finish`] (double-clic, Entrée, clic
//!   droit) ou, pour la surface, par un clic sur le premier point.

use meteroscope_core::{Pt, ShapeKind};

/// Résultat d'une action sur le tracé.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    Continue,
    /// Forme terminée : ses points de contrôle (ordre de `Shape::from_points`).
    Done(Vec<Pt>),
}

#[derive(Clone, Debug)]
pub struct Draft {
    pub kind: ShapeKind,
    pub pts: Vec<Pt>,
    /// Un appui est en cours (reçu par la vue).
    pressing: bool,
    /// L'appui en cours a posé le premier point.
    press_started: bool,
}

impl Draft {
    pub fn new(kind: ShapeKind) -> Self {
        Self { kind, pts: Vec::new(), pressing: false, press_started: false }
    }

    pub fn is_active(&self) -> bool {
        !self.pts.is_empty()
    }

    pub fn reset(&mut self) {
        *self = Draft::new(self.kind);
    }

    fn variable(&self) -> bool {
        self.kind.point_count().is_none()
    }

    /// Le prochain point commence un segment (premier point, ou début du
    /// second segment d'une inclinaison) : il est posé à l'appui.
    fn starts_segment(&self) -> bool {
        self.pts.is_empty() || (self.kind == ShapeKind::LineAngle && self.pts.len() == 2)
    }

    /// Applique la contrainte Maj au point `p` par rapport au point précédent.
    /// `rot` : rotation d'affichage (degrés), pour que l'horizontale et la
    /// verticale soient celles de l'écran (carré compris).
    pub fn constrain(&self, p: Pt, shift: bool, rot: f64) -> Pt {
        let Some(&prev) = self.pts.last() else { return p };
        if !shift || self.starts_segment() {
            return p;
        }
        match self.kind {
            ShapeKind::Rect => square_in(prev, p, -rot),
            // Troisième point : largeur égale au premier côté (carré).
            ShapeKind::Rect3 if self.pts.len() == 2 => square3(self.pts[0], prev, p),
            ShapeKind::Circle | ShapeKind::Circle3 | ShapeKind::Marker => p,
            _ => snap45(prev, p, rot),
        }
    }

    /// Ajoute un point s'il n'est pas un doublon ; termine si le compte est atteint.
    fn push(&mut self, p: Pt, tol: f64) -> Step {
        // Le doublon se juge entre les deux extrémités d'un même segment : le
        // second segment d'une inclinaison peut partir du bout du premier.
        if !self.starts_segment() && self.pts.last().is_some_and(|q| q.dist(p) <= tol) {
            return Step::Continue;
        }
        // Surface : clic sur le premier point = fermer.
        if self.kind == ShapeKind::Area && self.pts.len() >= 3 && self.pts[0].dist(p) <= tol {
            return self.finish();
        }
        self.pts.push(p);
        if self.kind == ShapeKind::Circle3 && self.pts.len() == 3 {
            // Trois points alignés : on retire le dernier et on attend un autre clic.
            if meteroscope_core::geometry::circle_from_3_points(self.pts[0], self.pts[1], self.pts[2]).is_none() {
                self.pts.pop();
                return Step::Continue;
            }
        }
        if self.kind.point_count() == Some(self.pts.len()) {
            return self.take();
        }
        Step::Continue
    }

    fn take(&mut self) -> Step {
        let pts = std::mem::take(&mut self.pts);
        self.reset();
        Step::Done(pts)
    }

    /// Appui du bouton principal en `p` (déjà contraint). `tol` : tolérance en pixels image.
    pub fn press(&mut self, p: Pt, tol: f64) -> Step {
        self.pressing = true;
        self.press_started = self.starts_segment();
        if self.press_started {
            let step = self.push(p, tol);
            if matches!(step, Step::Done(_)) {
                // Marqueur : terminé dès l'appui, le relâchement est ignoré.
                self.pressing = false;
            }
            return step;
        }
        Step::Continue
    }

    /// Relâchement en `p` (déjà contraint). `dragged` : le pointeur a bougé
    /// au-delà du seuil de clic depuis l'appui.
    pub fn release(&mut self, p: Pt, dragged: bool, tol: f64) -> Step {
        if !std::mem::take(&mut self.pressing) {
            return Step::Continue;
        }
        if self.press_started && !dragged {
            return Step::Continue;
        }
        self.push(p, tol)
    }

    /// Terminer un chemin ou une surface (double-clic, Entrée, clic droit).
    pub fn finish(&mut self) -> Step {
        if self.variable() && self.pts.len() >= self.kind.min_points() {
            return self.take();
        }
        Step::Continue
    }

    /// Retour arrière : retire le dernier point.
    pub fn backspace(&mut self) {
        self.pts.pop();
        self.pressing = false;
    }

    /// Points à afficher en aperçu : points posés + point sous le curseur.
    pub fn preview(&self, hover: Option<Pt>) -> Vec<Pt> {
        let mut v = self.pts.clone();
        if let Some(h) = hover
            && !v.is_empty()
            && v.last().is_none_or(|q| q.dist(h) > 0.0)
        {
            v.push(h);
        }
        v
    }

    /// La surface se fermerait si l'on cliquait en `p`.
    pub fn would_close(&self, p: Pt, tol: f64) -> bool {
        self.kind == ShapeKind::Area && self.pts.len() >= 3 && self.pts[0].dist(p) <= tol
    }
}

/// Contraint `p` à l'horizontale, la verticale ou une diagonale à 45° depuis `o`
/// (projection sur la direction la plus proche), à l'écran : `rot` est la
/// rotation d'affichage en degrés (sens horaire).
pub fn snap45(o: Pt, p: Pt, rot: f64) -> Pt {
    let (dx, dy) = (p.x - o.x, p.y - o.y);
    if dx == 0.0 && dy == 0.0 {
        return p;
    }
    let step = std::f64::consts::FRAC_PI_4;
    let r = rot.to_radians();
    let a = ((dy.atan2(dx) + r) / step).round() * step - r;
    let (ux, uy) = (a.cos(), a.sin());
    let t = dx * ux + dy * uy;
    // Arrondi pour obtenir des coordonnées exactes sur les axes.
    let clean = |v: f64| if v.abs() < 1e-12 { 0.0 } else { v };
    Pt::new(o.x + clean(t * ux), o.y + clean(t * uy))
}

/// Contraint le coin opposé `p` pour former un carré depuis `o`.
pub fn square(o: Pt, p: Pt) -> Pt {
    let (dx, dy) = (p.x - o.x, p.y - o.y);
    let s = dx.abs().max(dy.abs());
    let sign = |v: f64| if v < 0.0 { -1.0 } else { 1.0 };
    Pt::new(o.x + s * sign(dx), o.y + s * sign(dy))
}

/// Contraint le troisième point d'un rectangle par trois points pour que
/// la largeur soit égale au côté `o→q` (du côté où se trouve `p`).
pub fn square3(o: Pt, q: Pt, p: Pt) -> Pt {
    let (dx, dy) = (q.x - o.x, q.y - o.y);
    let l = dx.hypot(dy);
    if l == 0.0 {
        return p;
    }
    // Normale unitaire, orientée vers p.
    let (mut nx, mut ny) = (-dy / l, dx / l);
    if (p.x - q.x) * nx + (p.y - q.y) * ny < 0.0 {
        (nx, ny) = (-nx, -ny);
    }
    Pt::new(q.x + nx * l, q.y + ny * l)
}

/// [`square`] pour un rectangle dont les côtés sont orientés à `deg` degrés.
pub fn square_in(o: Pt, p: Pt, deg: f64) -> Pt {
    use meteroscope_core::geometry::{from_frame, to_frame};
    if deg == 0.0 {
        return square(o, p);
    }
    from_frame(square(to_frame(o, deg), to_frame(p, deg)), deg)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOL: f64 = 2.0;
    fn p(x: f64, y: f64) -> Pt {
        Pt::new(x, y)
    }

    #[test]
    fn clic_puis_clic() {
        meteroscope_i18n::set_language("fr");
        let mut d = Draft::new(ShapeKind::Length);
        assert_eq!(d.press(p(0., 0.), TOL), Step::Continue);
        assert_eq!(d.release(p(0.5, 0.), false, TOL), Step::Continue);
        assert!(d.is_active());
        assert_eq!(d.press(p(10., 0.), TOL), Step::Continue);
        assert_eq!(d.release(p(10., 0.), false, TOL), Step::Done(vec![p(0., 0.), p(10., 0.)]));
        assert!(!d.is_active());
    }

    #[test]
    fn presser_glisser_relacher() {
        meteroscope_i18n::set_language("fr");
        let mut d = Draft::new(ShapeKind::Rect);
        d.press(p(1., 1.), TOL);
        assert_eq!(d.release(p(20., 30.), true, TOL), Step::Done(vec![p(1., 1.), p(20., 30.)]));
        // Relâchement orphelin (appui non reçu) : ignoré.
        assert_eq!(d.release(p(5., 5.), true, TOL), Step::Continue);
        assert!(!d.is_active());
    }

    #[test]
    fn angle_et_doublons() {
        meteroscope_i18n::set_language("fr");
        let mut d = Draft::new(ShapeKind::Angle);
        d.press(p(0., 0.), TOL);
        d.release(p(0., 0.), false, TOL);
        d.press(p(10., 0.), TOL);
        d.release(p(10., 0.), false, TOL);
        // Second clic d'un double-clic au même endroit : ignoré.
        d.press(p(10., 0.5), TOL);
        assert_eq!(d.release(p(10., 0.5), false, TOL), Step::Continue);
        assert_eq!(d.pts.len(), 2);
        d.press(p(10., 10.), TOL);
        assert!(matches!(d.release(p(10., 10.), false, TOL), Step::Done(v) if v.len() == 3));
    }

    #[test]
    fn chemin_et_surface() {
        meteroscope_i18n::set_language("fr");
        let mut d = Draft::new(ShapeKind::Path);
        for q in [p(0., 0.), p(10., 0.), p(10., 10.)] {
            d.press(q, TOL);
            d.release(q, false, TOL);
        }
        d.backspace();
        assert_eq!(d.pts.len(), 2);
        assert!(matches!(d.finish(), Step::Done(v) if v.len() == 2));

        let mut a = Draft::new(ShapeKind::Area);
        for q in [p(0., 0.), p(10., 0.), p(10., 10.)] {
            a.press(q, TOL);
            a.release(q, false, TOL);
        }
        assert!(a.would_close(p(1., 1.), TOL));
        a.press(p(1., 1.), TOL);
        assert!(matches!(a.release(p(1., 1.), false, TOL), Step::Done(v) if v.len() == 3));

        let mut b = Draft::new(ShapeKind::Area);
        b.press(p(0., 0.), TOL);
        b.release(p(0., 0.), false, TOL);
        assert_eq!(b.finish(), Step::Continue); // pas assez de points
    }

    #[test]
    fn marqueur_et_cercle3() {
        meteroscope_i18n::set_language("fr");
        let mut m = Draft::new(ShapeKind::Marker);
        assert_eq!(m.press(p(3., 4.), TOL), Step::Done(vec![p(3., 4.)]));
        assert_eq!(m.release(p(3., 4.), false, TOL), Step::Continue);
        assert!(!m.is_active());

        let mut c = Draft::new(ShapeKind::Circle3);
        for q in [p(0., 0.), p(10., 0.)] {
            c.press(q, TOL);
            c.release(q, false, TOL);
        }
        c.press(p(20., 0.), TOL);
        assert_eq!(c.release(p(20., 0.), false, TOL), Step::Continue); // alignés
        assert_eq!(c.pts.len(), 2);
    }

    #[test]
    fn rectangle_trois_points() {
        meteroscope_i18n::set_language("fr");
        let mut r = Draft::new(ShapeKind::Rect3);
        // Premier côté en glissant, puis un clic pour la largeur.
        r.press(p(0., 0.), TOL);
        assert_eq!(r.release(p(30., 40.), true, TOL), Step::Continue);
        assert_eq!(r.pts.len(), 2);
        // Maj sur le troisième point : carré, du côté du pointeur.
        let q = r.constrain(p(20., 60.), true, 0.0);
        assert!(q.dist(p(-10., 70.)) < 1e-9, "{q:?}");
        r.press(q, TOL);
        let Step::Done(pts) = r.release(q, false, TOL) else { panic!() };
        let s = meteroscope_core::Shape::from_points(ShapeKind::Rect3, &pts).unwrap();
        let v = s.values(&meteroscope_core::Calibration::default());
        assert!((v[1].value - 50.0).abs() < 1e-9 && (v[2].value - 50.0).abs() < 1e-9);
    }

    #[test]
    fn contrainte_maj() {
        meteroscope_i18n::set_language("fr");
        let o = p(10., 10.);
        assert_eq!(snap45(o, p(50., 13.), 0.0), p(50., 10.));
        assert_eq!(snap45(o, p(12., -30.), 0.0), p(10., -30.));
        let d = snap45(o, p(30., 28.), 0.0);
        assert!((d.x - o.x - (d.y - o.y)).abs() < 1e-9);
        assert_eq!(square(o, p(20., 4.)), p(20., 0.));
        assert_eq!(square(o, p(0., 13.)), p(0., 20.));

        let mut l = Draft::new(ShapeKind::Length);
        l.press(o, TOL);
        assert_eq!(l.constrain(p(50., 13.), true, 0.0), p(50., 10.));
        assert_eq!(l.constrain(p(50., 13.), false, 0.0), p(50., 13.));
        let mut r = Draft::new(ShapeKind::Rect);
        r.press(o, TOL);
        assert_eq!(r.constrain(p(20., 4.), true, 0.0), p(20., 0.));
        // Affichage pivoté de −10° : « horizontal » à l'écran = penché de +10° dans l'image.
        let q = snap45(o, p(50., 15.), -10.0);
        assert!(((q.y - o.y).atan2(q.x - o.x).to_degrees() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn inclinaison_deux_segments() {
        meteroscope_i18n::set_language("fr");
        let mut d = Draft::new(ShapeKind::LineAngle);
        // Premier segment glissé, second en clic puis clic.
        d.press(p(0., 0.), TOL);
        assert_eq!(d.release(p(10., 0.), true, TOL), Step::Continue);
        // Maj n'agit pas sur le début du second segment.
        assert_eq!(d.constrain(p(50., 13.), true, 0.0), p(50., 13.));
        d.press(p(50., 13.), TOL);
        d.release(p(50., 13.), false, TOL);
        assert_eq!(d.constrain(p(70., 14.), true, 0.0), p(70., 13.));
        d.press(p(70., 30.), TOL);
        let done = d.release(p(70., 30.), false, TOL);
        assert_eq!(done, Step::Done(vec![p(0., 0.), p(10., 0.), p(50., 13.), p(70., 30.)]));
        // Second segment glissé lui aussi.
        let mut g = Draft::new(ShapeKind::LineAngle);
        g.press(p(0., 0.), TOL);
        g.release(p(10., 0.), true, TOL);
        g.press(p(0., 20.), TOL);
        assert!(matches!(g.release(p(10., 25.), true, TOL), Step::Done(v) if v.len() == 4));
    }

    #[test]
    fn inclinaison_sommet_commun() {
        meteroscope_i18n::set_language("fr");
        // A→B glissé, puis B→C glissé depuis B : l'angle ABC est terminé au relâchement.
        let mut d = Draft::new(ShapeKind::LineAngle);
        d.press(p(0., 0.), TOL);
        d.release(p(10., 0.), true, TOL);
        assert_eq!(d.press(p(10., 0.), TOL), Step::Continue);
        assert_eq!(d.pts.len(), 3);
        assert_eq!(d.release(p(10., 20.), true, TOL), Step::Done(vec![p(0., 0.), p(10., 0.), p(10., 0.), p(10., 20.)]));
        // Départ dans la tolérance de clic de B, second segment en clic puis clic.
        let mut d = Draft::new(ShapeKind::LineAngle);
        d.press(p(0., 0.), TOL);
        d.release(p(10., 0.), true, TOL);
        d.press(p(10.5, 0.5), TOL);
        d.release(p(10.5, 0.5), false, TOL);
        d.press(p(30., 30.), TOL);
        assert!(matches!(d.release(p(30., 30.), false, TOL), Step::Done(v) if v.len() == 4 && v[2] == p(10.5, 0.5)));
        // Les deux extrémités d'un même segment restent distinctes : un
        // relâchement sur le début du second segment n'ajoute rien.
        let mut d = Draft::new(ShapeKind::LineAngle);
        d.press(p(0., 0.), TOL);
        d.release(p(10., 0.), true, TOL);
        d.press(p(10., 0.), TOL);
        assert_eq!(d.release(p(10.5, 0.), true, TOL), Step::Continue);
        assert_eq!(d.pts.len(), 3);
        // Ailleurs, le doublon d'un double-clic est toujours écarté.
        let mut l = Draft::new(ShapeKind::Path);
        l.press(p(0., 0.), TOL);
        l.release(p(0., 0.), false, TOL);
        l.press(p(10., 0.), TOL);
        l.release(p(10., 0.), false, TOL);
        l.press(p(10., 0.), TOL);
        l.release(p(10., 0.), false, TOL);
        assert_eq!(l.pts.len(), 2);
    }
}

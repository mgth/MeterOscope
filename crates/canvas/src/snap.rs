//! Aimantation aux points remarquables des mesures (logique pure, en pixels image).
//!
//! Ordre des contraintes appliquées au point tracé ou à la poignée déplacée :
//! 1. Alt maintenu : aucune aimantation, on passe directement à l'étape 3 ;
//! 2. un point remarquable à moins de la tolérance : le point s'y accroche
//!    exactement, et l'alignement Maj (45°, carré…) est ignoré ;
//! 3. sinon, la contrainte Maj habituelle s'applique.

use crate::edit::segments;
use meteroscope_core::geometry::{circle_from_3_points, line_angle_vertex, rect_corners};
use meteroscope_core::{Document, Pt, Shape};
use meteroscope_i18n::tr;

/// Nature du point d'accroche (ordre = priorité à distance égale).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapKind {
    /// Point de contrôle ou coin de rectangle.
    Point,
    /// Intersection des deux droites d'une inclinaison.
    Intersection,
    /// Centre d'un cercle, d'une ellipse ou d'un arc.
    Center,
    /// Milieu d'un segment.
    Midpoint,
}

impl SnapKind {
    /// Nom affiché dans l'aide contextuelle.
    pub fn label(self) -> &'static str {
        match self {
            SnapKind::Point => tr!("snap.point"),
            SnapKind::Intersection => tr!("snap.intersection"),
            SnapKind::Center => tr!("snap.center"),
            SnapKind::Midpoint => tr!("snap.midpoint"),
        }
    }
}

/// Point d'accroche trouvé.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snap {
    pub pt: Pt,
    pub kind: SnapKind,
}

/// Points remarquables d'une forme.
pub fn snap_points(shape: &Shape) -> Vec<Snap> {
    let mut v = Vec::new();
    let mut push = |pt: Pt, kind: SnapKind| v.push(Snap { pt, kind });
    let center = match shape {
        Shape::Ellipse { center, .. } => Some(*center),
        Shape::Arc { a, m, b } => circle_from_3_points(*a, *m, *b).map(|(c, _)| c),
        s => s.circle().map(|(c, _)| c),
    };
    match shape {
        Shape::Rect { a, b, angle } => rect_corners(*a, *b, *angle).into_iter().for_each(|c| push(c, SnapKind::Point)),
        // Le centre d'un cercle centre-bord ou d'une ellipse est annoncé comme
        // centre, pas comme point.
        s => s.points().into_iter().filter(|p| Some(*p) != center).for_each(|p| push(p, SnapKind::Point)),
    }
    if let Some(c) = center {
        push(c, SnapKind::Center);
    }
    if let Shape::LineAngle { a, b, c, d } = shape
        && let Some((x, _, _)) = line_angle_vertex(*a, *b, *c, *d)
    {
        push(x, SnapKind::Intersection);
    }
    for (a, b) in segments(shape) {
        push(a.lerp(b, 0.5), SnapKind::Midpoint);
    }
    v
}

/// Point remarquable des mesures visibles le plus proche de `p`, à moins de
/// `tol` pixels image. `exclude` : mesure qui ne s'aimante pas sur elle-même
/// (celle dont on déplace une poignée). À distance égale, l'ordre de
/// [`SnapKind`] départage.
pub fn find_snap(doc: &Document, p: Pt, tol: f64, exclude: Option<u64>) -> Option<Snap> {
    let rank = |k: SnapKind| k as u8;
    doc.measurements
        .iter()
        .filter(|m| m.visible && Some(m.id) != exclude)
        .flat_map(|m| snap_points(&m.shape))
        .map(|s| (s.pt.dist(p), s))
        .filter(|(d, _)| *d <= tol)
        .min_by(|(da, a), (db, b)| da.total_cmp(db).then(rank(a.kind).cmp(&rank(b.kind))))
        .map(|(_, s)| s)
}

/// Applique l'ordre des contraintes : l'aimantation (si `snap` est trouvé)
/// l'emporte sur la contrainte Maj `constrain`.
pub fn snap_or(snap: Option<Snap>, p: Pt, constrain: impl FnOnce(Pt) -> Pt) -> Pt {
    match snap {
        Some(s) => s.pt,
        None => constrain(p),
    }
}

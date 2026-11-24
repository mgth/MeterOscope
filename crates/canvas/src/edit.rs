//! Poignées et test de clic sur les mesures (logique pure, en pixels image).

use crate::draft::{snap45, square};
use mesura_core::geometry::{dist_point_segment, point_in_polygon};
use mesura_core::{Document, Pt, Shape};

/// Élément sous le pointeur.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    /// Poignée `idx` de la mesure `id`.
    Handle(u64, usize),
    /// Corps de la mesure `id`.
    Body(u64),
}

/// Positions des poignées d'une forme. Le rectangle a ses quatre coins
/// (a, coin haut-droit, b, coin bas-gauche), les autres leurs points de contrôle.
pub fn handles(shape: &Shape) -> Vec<Pt> {
    match shape {
        Shape::Rect { a, b } => vec![*a, Pt::new(b.x, a.y), *b, Pt::new(a.x, b.y)],
        s => s.points(),
    }
}

/// Déplace la poignée `i` en `p`. Le centre d'un cercle déplace tout le
/// cercle (le rayon est conservé).
pub fn set_handle(shape: &mut Shape, i: usize, p: Pt) {
    match shape {
        Shape::Rect { a, b } => {
            match i {
                0 => *a = p,
                1 => (b.x, a.y) = (p.x, p.y),
                2 => *b = p,
                _ => (a.x, b.y) = (p.x, p.y),
            }
            return;
        }
        Shape::Circle { center, edge } if i == 0 => {
            *edge = edge.offset(p.x - center.x, p.y - center.y);
            *center = p;
            return;
        }
        _ => {}
    }
    if let Some(q) = shape.points_mut().into_iter().nth(i) {
        *q = p;
    }
}

/// Contrainte Maj pendant le glisser d'une poignée : horizontale/verticale/45°
/// pour une longueur, carré pour un rectangle.
pub fn constrain_handle(shape: &Shape, i: usize, p: Pt, shift: bool) -> Pt {
    if !shift {
        return p;
    }
    match shape {
        Shape::Length { a, b } => snap45(if i == 0 { *b } else { *a }, p),
        Shape::Rect { .. } => square(handles(shape)[(i + 2) % 4], p),
        _ => p,
    }
}

/// Segments du contour d'une forme (hors cercles).
fn segments(shape: &Shape) -> Vec<(Pt, Pt)> {
    let open = |pts: &[Pt]| pts.windows(2).map(|w| (w[0], w[1])).collect::<Vec<_>>();
    let closed = |pts: &[Pt]| {
        let mut v = open(pts);
        if pts.len() > 2 {
            v.push((pts[pts.len() - 1], pts[0]));
        }
        v
    };
    match shape {
        Shape::Length { a, b } => vec![(*a, *b)],
        Shape::Path { pts } => open(pts),
        Shape::Angle { a, vertex, b } => vec![(*a, *vertex), (*vertex, *b)],
        Shape::Area { pts } => closed(pts),
        Shape::Rect { .. } => closed(&handles(shape)),
        _ => vec![],
    }
}

/// Distance du point `p` au tracé de la forme.
pub fn dist_to_outline(shape: &Shape, p: Pt) -> f64 {
    match shape {
        Shape::Marker { p: m } => m.dist(p),
        Shape::Circle { .. } | Shape::Circle3 { .. } => match shape.circle() {
            Some((c, r)) => (c.dist(p) - r).abs(),
            None => f64::INFINITY,
        },
        s => segments(s).iter().map(|(a, b)| dist_point_segment(p, *a, *b)).fold(f64::INFINITY, f64::min),
    }
}

/// Le point est à l'intérieur d'une forme fermée.
pub fn inside(shape: &Shape, p: Pt) -> bool {
    match shape {
        Shape::Area { pts } => point_in_polygon(p, pts),
        Shape::Rect { a, b } => {
            (a.x.min(b.x)..=a.x.max(b.x)).contains(&p.x) && (a.y.min(b.y)..=a.y.max(b.y)).contains(&p.y)
        }
        Shape::Circle { .. } | Shape::Circle3 { .. } => shape.circle().is_some_and(|(c, r)| c.dist(p) <= r),
        _ => false,
    }
}

/// Élément du document sous `p`. Tolérances en pixels image.
/// Priorité : poignées de la mesure sélectionnée, puis contours (la dernière
/// dessinée d'abord), puis intérieur de la mesure sélectionnée.
pub fn hit_test(doc: &Document, p: Pt, tol: f64, handle_tol: f64) -> Option<Hit> {
    let sel = doc.selected().filter(|m| m.visible);
    if let Some(m) = sel {
        let best = handles(&m.shape)
            .iter()
            .enumerate()
            .map(|(i, h)| (i, h.dist(p)))
            .filter(|(_, d)| *d <= handle_tol)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((i, _)) = best {
            return Some(Hit::Handle(m.id, i));
        }
    }
    let best = doc
        .measurements
        .iter()
        .rev()
        .filter(|m| m.visible)
        .map(|m| {
            let t = if matches!(m.shape, Shape::Marker { .. }) { handle_tol } else { tol };
            (m.id, dist_to_outline(&m.shape, p) - t)
        })
        .filter(|(_, d)| *d <= 0.0)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((id, _)) = best {
        return Some(Hit::Body(id));
    }
    sel.filter(|m| inside(&m.shape, p)).map(|m| Hit::Body(m.id))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> Pt {
        Pt::new(x, y)
    }

    #[test]
    fn poignees_rectangle() {
        let mut s = Shape::Rect { a: p(0., 0.), b: p(10., 5.) };
        assert_eq!(handles(&s), vec![p(0., 0.), p(10., 0.), p(10., 5.), p(0., 5.)]);
        set_handle(&mut s, 1, p(12., -1.));
        assert_eq!(s, Shape::Rect { a: p(0., -1.), b: p(12., 5.) });
        set_handle(&mut s, 3, p(2., 7.));
        assert_eq!(s, Shape::Rect { a: p(2., -1.), b: p(12., 7.) });
        // Carré par rapport au coin opposé (poignée 0 ↔ poignée 2).
        assert_eq!(constrain_handle(&s, 0, p(9., 0.), true), p(5., 0.));
        let mut l = Shape::Length { a: p(0., 0.), b: p(10., 0.) };
        set_handle(&mut l, 1, p(4., 4.));
        assert_eq!(l, Shape::Length { a: p(0., 0.), b: p(4., 4.) });
        assert_eq!(constrain_handle(&l, 1, p(20., 1.), true), p(20., 0.));
        let mut c = Shape::Circle { center: p(0., 0.), edge: p(5., 0.) };
        set_handle(&mut c, 0, p(1., 1.));
        assert_eq!(c, Shape::Circle { center: p(1., 1.), edge: p(6., 1.) });
        set_handle(&mut c, 1, p(1., 4.));
        assert_eq!(c, Shape::Circle { center: p(1., 1.), edge: p(1., 4.) });
    }

    #[test]
    fn test_de_clic() {
        let mut doc = Document::new();
        let a = doc.add(Shape::Length { a: p(0., 0.), b: p(100., 0.) });
        let c = doc.add(Shape::Circle { center: p(50., 50.), edge: p(70., 50.) });
        let m = doc.add(Shape::Marker { p: p(200., 200.) });
        doc.selected = None;
        assert_eq!(hit_test(&doc, p(50., 2.), 3., 5.), Some(Hit::Body(a)));
        assert_eq!(hit_test(&doc, p(50., 8.), 3., 5.), None);
        assert_eq!(hit_test(&doc, p(50., 31.), 3., 5.), Some(Hit::Body(c)));
        assert_eq!(hit_test(&doc, p(50., 50.), 3., 5.), None); // intérieur non sélectionné
        assert_eq!(hit_test(&doc, p(203., 200.), 3., 5.), Some(Hit::Body(m)));
        doc.selected = Some(c);
        assert_eq!(hit_test(&doc, p(50., 50.), 3., 5.), Some(Hit::Handle(c, 0)));
        assert_eq!(hit_test(&doc, p(55., 55.), 3., 5.), Some(Hit::Body(c)));
        assert_eq!(hit_test(&doc, p(69., 51.), 3., 5.), Some(Hit::Handle(c, 1)));
        doc.get_mut(c).unwrap().visible = false;
        assert_eq!(hit_test(&doc, p(50., 50.), 3., 5.), None);
        let area = Shape::Area { pts: vec![p(0., 0.), p(10., 0.), p(10., 10.)] };
        assert!(inside(&area, p(8., 2.)) && !inside(&area, p(2., 8.)));
        assert!((dist_to_outline(&area, p(2., 8.)) - (18f64).sqrt()).abs() < 1e-9);
    }
}

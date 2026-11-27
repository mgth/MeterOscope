//! Poignées et test de clic sur les mesures (logique pure, en pixels image).

use crate::draft::{snap45, square_in};
use meteroscope_core::geometry::{
    direction_deg, dist_point_segment, from_frame, normalize_deg, point_in_polygon, rect_corners, to_frame,
};
use meteroscope_core::{Document, Pt, Shape};

/// Élément sous le pointeur.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hit {
    /// Poignée `idx` de la mesure `id`.
    Handle(u64, usize),
    /// Corps de la mesure `id`.
    Body(u64),
}

/// Indice de la poignée de rotation d'un rectangle (après ses quatre coins).
pub const ROTATE_HANDLE: usize = 4;

/// Poignée de rotation d'un rectangle : (milieu du côté « haut » dans le
/// repère du rectangle, poignée à `gap` pixels image au-delà).
pub fn rotate_handle(shape: &Shape, gap: f64) -> Option<(Pt, Pt)> {
    let Shape::Rect { a, b, angle } = shape else { return None };
    let (la, lb) = (to_frame(*a, *angle), to_frame(*b, *angle));
    let (x, y) = ((la.x + lb.x) / 2.0, la.y.min(lb.y));
    Some((from_frame(Pt::new(x, y), *angle), from_frame(Pt::new(x, y - gap), *angle)))
}

/// Positions des poignées d'une forme. Le rectangle a ses quatre coins
/// (a, coin haut-droit, b, coin bas-gauche) puis sa poignée de rotation,
/// à `gap` pixels image de son bord ; les autres formes, leurs points de contrôle.
pub fn handles(shape: &Shape, gap: f64) -> Vec<Pt> {
    match shape {
        Shape::Rect { a, b, angle } => {
            let mut v = rect_corners(*a, *b, *angle).to_vec();
            v.extend(rotate_handle(shape, gap).map(|(_, h)| h));
            v
        }
        s => s.points(),
    }
}

/// Centre d'un rectangle.
fn rect_center(a: Pt, b: Pt) -> Pt {
    a.lerp(b, 0.5)
}

/// Déplace la poignée `i` en `p`. Le centre d'un cercle déplace tout le
/// cercle (le rayon est conservé).
pub fn set_handle(shape: &mut Shape, i: usize, p: Pt) {
    match shape {
        Shape::Rect { a, b, angle } if i == ROTATE_HANDLE => {
            // Rotation autour du centre : la poignée (côté −y du repère du
            // rectangle) vise le pointeur.
            let c = rect_center(*a, *b);
            if p.dist(c) == 0.0 {
                return;
            }
            let delta = direction_deg(c, p) - (*angle - 90.0);
            let turn = |q: Pt| {
                let r = to_frame(Pt::new(q.x - c.x, q.y - c.y), -delta);
                Pt::new(c.x + r.x, c.y + r.y)
            };
            (*a, *b) = (turn(*a), turn(*b));
            *angle = normalize_deg(*angle + delta);
            if angle.abs() < 1e-9 {
                *angle = 0.0;
            }
            return;
        }
        Shape::Rect { a, b, angle } => {
            // Dans le repère des côtés du rectangle.
            let (mut la, mut lb, lp) = (to_frame(*a, *angle), to_frame(*b, *angle), to_frame(p, *angle));
            match i {
                0 => la = lp,
                1 => (lb.x, la.y) = (lp.x, lp.y),
                2 => lb = lp,
                _ => (la.x, lb.y) = (lp.x, lp.y),
            }
            if *angle == 0.0 {
                (*a, *b) = (la, lb);
            } else {
                (*a, *b) = (from_frame(la, *angle), from_frame(lb, *angle));
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
/// à l'écran (`rot` : rotation d'affichage en degrés) pour une longueur ou un
/// segment d'inclinaison, carré pour un rectangle, pas de 15° à l'écran pour
/// sa rotation.
pub fn constrain_handle(shape: &Shape, i: usize, p: Pt, shift: bool, rot: f64) -> Pt {
    if !shift {
        return p;
    }
    match shape {
        Shape::Length { a, b } => snap45(if i == 0 { *b } else { *a }, p, rot),
        // L'autre extrémité du même segment.
        Shape::LineAngle { .. } => snap45(shape.points()[i ^ 1], p, rot),
        Shape::Rect { a, b, .. } if i == ROTATE_HANDLE => {
            let c = rect_center(*a, *b);
            let step = 15.0;
            let dir = (((direction_deg(c, p) + rot) / step).round() * step - rot).to_radians();
            let r = c.dist(p);
            Pt::new(c.x + r * dir.cos(), c.y + r * dir.sin())
        }
        Shape::Rect { a, b, angle } => square_in(rect_corners(*a, *b, *angle)[(i + 2) % 4], p, *angle),
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
        Shape::Rect { a, b, angle } => closed(&rect_corners(*a, *b, *angle)),
        Shape::LineAngle { a, b, c, d } => vec![(*a, *b), (*c, *d)],
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
        Shape::Rect { a, b, angle } => {
            let (a, b, p) = (to_frame(*a, *angle), to_frame(*b, *angle), to_frame(p, *angle));
            (a.x.min(b.x)..=a.x.max(b.x)).contains(&p.x) && (a.y.min(b.y)..=a.y.max(b.y)).contains(&p.y)
        }
        Shape::Circle { .. } | Shape::Circle3 { .. } => shape.circle().is_some_and(|(c, r)| c.dist(p) <= r),
        _ => false,
    }
}

/// Élément du document sous `p`. Tolérances en pixels image.
/// Priorité : poignées de la mesure sélectionnée, puis contours (la dernière
/// dessinée d'abord), puis intérieur de la mesure sélectionnée.
pub fn hit_test(doc: &Document, p: Pt, tol: f64, handle_tol: f64, gap: f64) -> Option<Hit> {
    let sel = doc.selected().filter(|m| m.visible);
    if let Some(m) = sel {
        let best = handles(&m.shape, gap)
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
        meteroscope_i18n::set_language("fr");
        let mut s = Shape::Rect { a: p(0., 0.), b: p(10., 5.), angle: 0.0 };
        assert_eq!(handles(&s, 3.), vec![p(0., 0.), p(10., 0.), p(10., 5.), p(0., 5.), p(5., -3.)]);
        set_handle(&mut s, 1, p(12., -1.));
        assert_eq!(s, Shape::Rect { a: p(0., -1.), b: p(12., 5.), angle: 0.0 });
        set_handle(&mut s, 3, p(2., 7.));
        assert_eq!(s, Shape::Rect { a: p(2., -1.), b: p(12., 7.), angle: 0.0 });
        // Carré par rapport au coin opposé (poignée 0 ↔ poignée 2).
        assert_eq!(constrain_handle(&s, 0, p(9., 0.), true, 0.0), p(5., 0.));
        // Rectangle à 90° : le coin « haut-droit » reste un coin, les côtés gardent l'orientation.
        let mut r = Shape::Rect { a: p(0., 0.), b: p(10., 5.), angle: 90.0 };
        let h = handles(&r, 3.);
        assert!(h[1].dist(p(0., 5.)) < 1e-9 && h[3].dist(p(10., 0.)) < 1e-9);
        set_handle(&mut r, 1, p(-1., 6.));
        let h = handles(&r, 3.);
        assert!(h[0].dist(p(-1., 0.)) < 1e-9 && h[2].dist(p(10., 6.)) < 1e-9, "{h:?}");
        assert!(inside(&r, p(5., 3.)) && !inside(&r, p(11., 3.)));
        // Poignée de rotation : vers la droite du centre = rectangle tourné d'un quart de tour horaire.
        let mut q = Shape::Rect { a: p(0., 0.), b: p(10., 4.), angle: 0.0 };
        set_handle(&mut q, ROTATE_HANDLE, p(20., 2.));
        let Shape::Rect { angle, .. } = q else { panic!() };
        assert!((angle - 90.0).abs() < 1e-9, "{angle}");
        let h = handles(&q, 3.);
        // Mêmes dimensions, même centre, poignée maintenant à droite.
        let v = q.values(&meteroscope_core::Calibration::default());
        assert!((v[1].value - 10.0).abs() < 1e-9 && (v[2].value - 4.0).abs() < 1e-9);
        assert!(h[0].lerp(h[2], 0.5).dist(p(5., 2.)) < 1e-9 && h[4].dist(p(10., 2.)) < 1e-9, "{h:?}");
        // Maj : pas de 15° à l'écran (vue redressée de 4°).
        let c = constrain_handle(&q, ROTATE_HANDLE, p(5. + 10. * 0.3f64.cos(), 2. + 10. * 0.3f64.sin()), true, 4.0);
        assert!((direction_deg(p(5., 2.), c) - 11.0).abs() < 1e-9 && (c.dist(p(5., 2.)) - 10.0).abs() < 1e-9);
        // Rectangle à 30° : intérieur dans le repère tourné.
        let t = 30f64.to_radians();
        let r = Shape::Rect { a: p(0., 0.), b: p(10. * (t.cos() - t.sin()), 10. * (t.sin() + t.cos())), angle: 30.0 };
        assert!(inside(&r, p(5. * t.cos(), 5. * t.sin()).offset(-0.5 * t.sin(), 0.5 * t.cos())));
        assert!(!inside(&r, p(5. * t.cos(), 5. * t.sin()).offset(0.5 * t.sin(), -0.5 * t.cos())));
        let mut l = Shape::Length { a: p(0., 0.), b: p(10., 0.) };
        set_handle(&mut l, 1, p(4., 4.));
        assert_eq!(l, Shape::Length { a: p(0., 0.), b: p(4., 4.) });
        assert_eq!(constrain_handle(&l, 1, p(20., 1.), true, 0.0), p(20., 0.));
        let mut c = Shape::Circle { center: p(0., 0.), edge: p(5., 0.) };
        set_handle(&mut c, 0, p(1., 1.));
        assert_eq!(c, Shape::Circle { center: p(1., 1.), edge: p(6., 1.) });
        set_handle(&mut c, 1, p(1., 4.));
        assert_eq!(c, Shape::Circle { center: p(1., 1.), edge: p(1., 4.) });
        let la = Shape::LineAngle { a: p(0., 0.), b: p(10., 0.), c: p(0., 20.), d: p(10., 20.) };
        assert_eq!(constrain_handle(&la, 3, p(30., 22.), true, 0.0), p(30., 20.));
        assert_eq!(constrain_handle(&la, 0, p(-9., 1.), true, 0.0), p(-9., 0.));
        assert_eq!(dist_to_outline(&la, p(5., 19.)), 1.0);
        assert_eq!(dist_to_outline(&la, p(5., 10.)), 10.0);
    }

    #[test]
    fn test_de_clic() {
        meteroscope_i18n::set_language("fr");
        let mut doc = Document::new();
        let a = doc.add(Shape::Length { a: p(0., 0.), b: p(100., 0.) });
        let c = doc.add(Shape::Circle { center: p(50., 50.), edge: p(70., 50.) });
        let m = doc.add(Shape::Marker { p: p(200., 200.) });
        doc.selected = None;
        assert_eq!(hit_test(&doc, p(50., 2.), 3., 5., 10.), Some(Hit::Body(a)));
        assert_eq!(hit_test(&doc, p(50., 8.), 3., 5., 10.), None);
        assert_eq!(hit_test(&doc, p(50., 31.), 3., 5., 10.), Some(Hit::Body(c)));
        assert_eq!(hit_test(&doc, p(50., 50.), 3., 5., 10.), None); // intérieur non sélectionné
        assert_eq!(hit_test(&doc, p(203., 200.), 3., 5., 10.), Some(Hit::Body(m)));
        doc.selected = Some(c);
        assert_eq!(hit_test(&doc, p(50., 50.), 3., 5., 10.), Some(Hit::Handle(c, 0)));
        assert_eq!(hit_test(&doc, p(55., 55.), 3., 5., 10.), Some(Hit::Body(c)));
        assert_eq!(hit_test(&doc, p(69., 51.), 3., 5., 10.), Some(Hit::Handle(c, 1)));
        doc.get_mut(c).unwrap().visible = false;
        assert_eq!(hit_test(&doc, p(50., 50.), 3., 5., 10.), None);
        let area = Shape::Area { pts: vec![p(0., 0.), p(10., 0.), p(10., 10.)] };
        assert!(inside(&area, p(8., 2.)) && !inside(&area, p(2., 8.)));
        assert!((dist_to_outline(&area, p(2., 8.)) - (18f64).sqrt()).abs() < 1e-9);
    }
}

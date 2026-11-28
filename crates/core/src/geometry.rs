use serde::{Deserialize, Serialize};

/// Point en coordonnées image (pixels, origine en haut à gauche).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

impl Pt {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    pub fn dist(self, o: Pt) -> f64 {
        (self.x - o.x).hypot(self.y - o.y)
    }
    pub fn offset(self, dx: f64, dy: f64) -> Pt {
        Pt::new(self.x + dx, self.y + dy)
    }
    pub fn lerp(self, o: Pt, t: f64) -> Pt {
        Pt::new(self.x + (o.x - self.x) * t, self.y + (o.y - self.y) * t)
    }
}

/// Longueur d'une ligne brisée ouverte.
pub fn polyline_length(pts: &[Pt]) -> f64 {
    pts.windows(2).map(|w| w[0].dist(w[1])).sum()
}

/// Périmètre d'un polygone fermé.
///
/// Une arête parcourue aussi en sens inverse (fente aller-retour qui raccorde
/// un trou au contour, en « trou de serrure ») ne compte pas : seuls le bord
/// extérieur et celui des trous font le périmètre.
pub fn polygon_perimeter(pts: &[Pt]) -> f64 {
    let n = pts.len();
    if n < 2 {
        return 0.0;
    }
    let key = |a: Pt, b: Pt| [a.x.to_bits(), a.y.to_bits(), b.x.to_bits(), b.y.to_bits()];
    let edges = (0..n).map(|i| (pts[i], pts[(i + 1) % n]));
    let mut seen: Vec<[u64; 4]> = edges.clone().map(|(a, b)| key(a, b)).collect();
    seen.sort_unstable();
    edges
        .filter(|&(a, b)| seen.binary_search(&key(b, a)).is_err())
        .map(|(a, b)| a.dist(b))
        .sum()
}

/// Aire (positive) d'un polygone simple, formule du lacet.
pub fn polygon_area(pts: &[Pt]) -> f64 {
    let n = pts.len();
    if n < 3 {
        return 0.0;
    }
    let mut s = 0.0;
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        s += a.x * b.y - b.x * a.y;
    }
    s.abs() / 2.0
}

/// Angle au sommet `v` entre les demi-droites `v→a` et `v→b`, en degrés, dans [0, 180].
pub fn angle_deg(a: Pt, v: Pt, b: Pt) -> f64 {
    let (ux, uy) = (a.x - v.x, a.y - v.y);
    let (wx, wy) = (b.x - v.x, b.y - v.y);
    let cross = ux * wy - uy * wx;
    let dot = ux * wx + uy * wy;
    cross.atan2(dot).abs().to_degrees()
}

/// Ramène un angle en degrés dans ]-180, 180].
pub fn normalize_deg(a: f64) -> f64 {
    let mut r = a % 360.0;
    if r > 180.0 {
        r -= 360.0;
    } else if r <= -180.0 {
        r += 360.0;
    }
    r
}

/// Direction du segment `a→b` en degrés (repère image, y vers le bas :
/// positif = sens horaire à l'écran).
pub fn direction_deg(a: Pt, b: Pt) -> f64 {
    (b.y - a.y).atan2(b.x - a.x).to_degrees()
}

/// Inclinaison entre les droites portant les segments `a→b` et `c→d`,
/// en degrés, dans [0, 90] (0 : parallèles, 90 : perpendiculaires).
pub fn line_angle_deg(a: Pt, b: Pt, c: Pt, d: Pt) -> f64 {
    if a.dist(b) == 0.0 || c.dist(d) == 0.0 {
        return 0.0;
    }
    let x = (direction_deg(c, d) - direction_deg(a, b)).abs() % 180.0;
    if x > 90.0 { 180.0 - x } else { x }
}

/// Rotation d'affichage (degrés, positif = sens horaire à l'écran) qui rend
/// le segment `a→b` horizontal ou vertical. On vise l'axe le plus proche de
/// l'orientation actuelle du segment à l'écran (rotation `current`), pour
/// une correction minimale. `None` si le segment est de longueur nulle.
pub fn straighten_rotation(a: Pt, b: Pt, current: f64) -> Option<f64> {
    if a.dist(b) == 0.0 {
        return None;
    }
    let phi = direction_deg(a, b);
    let target = ((phi + current) / 90.0).round() * 90.0;
    Some(normalize_deg(target - phi))
}

/// Point `p` exprimé dans le repère tourné de `deg` degrés (axes
/// `(cos, sin)` et `(-sin, cos)`, repère image).
pub fn to_frame(p: Pt, deg: f64) -> Pt {
    let (s, c) = deg.to_radians().sin_cos();
    Pt::new(c * p.x + s * p.y, -s * p.x + c * p.y)
}

/// Inverse de [`to_frame`].
pub fn from_frame(q: Pt, deg: f64) -> Pt {
    let (s, c) = deg.to_radians().sin_cos();
    Pt::new(c * q.x - s * q.y, s * q.x + c * q.y)
}

/// Coins d'un rectangle de coins opposés `a`, `b`, côtés orientés à `deg`
/// degrés : a, coin « haut-droit », b, coin « bas-gauche ».
pub fn rect_corners(a: Pt, b: Pt, deg: f64) -> [Pt; 4] {
    if deg == 0.0 {
        return [a, Pt::new(b.x, a.y), b, Pt::new(a.x, b.y)];
    }
    let (la, lb) = (to_frame(a, deg), to_frame(b, deg));
    [a, from_frame(Pt::new(lb.x, la.y), deg), b, from_frame(Pt::new(la.x, lb.y), deg)]
}

/// Largeur et hauteur (le long de ses côtés) d'un rectangle orienté.
pub fn rect_size(a: Pt, b: Pt, deg: f64) -> (f64, f64) {
    let (la, lb) = (to_frame(a, deg), to_frame(b, deg));
    ((la.x - lb.x).abs(), (la.y - lb.y).abs())
}

/// Rectangle par trois points : côté `p1→p2`, puis largeur jusqu'à la
/// droite parallèle passant par `p3`. Renvoie (a, b, angle) pour
/// `Shape::Rect` (coins opposés, orientation des côtés en degrés).
pub fn rect_from_3_points(p1: Pt, p2: Pt, p3: Pt) -> (Pt, Pt, f64) {
    if p1.dist(p2) == 0.0 {
        return (p1, p3, 0.0);
    }
    let angle = direction_deg(p1, p2);
    let (l2, l3) = (to_frame(p2, angle), to_frame(p3, angle));
    (p1, from_frame(Pt::new(l2.x, l3.y), angle), angle)
}

/// Intersection des droites `a→b` et `c→d`. `None` si parallèles.
pub fn line_intersection(a: Pt, b: Pt, c: Pt, d: Pt) -> Option<Pt> {
    let (rx, ry) = (b.x - a.x, b.y - a.y);
    let (sx, sy) = (d.x - c.x, d.y - c.y);
    let den = rx * sy - ry * sx;
    let scale = rx.hypot(ry) * sx.hypot(sy);
    if scale == 0.0 || den.abs() < 1e-9 * scale {
        return None;
    }
    let t = ((c.x - a.x) * sy - (c.y - a.y) * sx) / den;
    Some(Pt::new(a.x + t * rx, a.y + t * ry))
}

/// Sommet de l'angle d'inclinaison : intersection des deux droites et
/// directions unitaires des côtés de l'angle aigu, chacune tournée vers son
/// segment quand c'est possible. `None` si les droites sont presque
/// parallèles ou se coupent trop loin des segments pour être utiles.
pub fn line_angle_vertex(a: Pt, b: Pt, c: Pt, d: Pt) -> Option<(Pt, Pt, Pt)> {
    let x = line_intersection(a, b, c, d)?;
    let pts = [a, b, c, d];
    let center = Pt::new(pts.iter().map(|p| p.x).sum::<f64>() / 4.0, pts.iter().map(|p| p.y).sum::<f64>() / 4.0);
    let spread = pts.iter().map(|p| p.dist(center)).fold(0.0, f64::max);
    if x.dist(center) > 4.0 * spread.max(1e-9) {
        return None;
    }
    let unit = |v: (f64, f64)| {
        let l = v.0.hypot(v.1);
        Pt::new(v.0 / l, v.1 / l)
    };
    // Vers le milieu du segment, ou le long du segment si on est dessus.
    let side = |p: Pt, q: Pt| {
        let m = p.lerp(q, 0.5);
        if m.dist(x) > 1e-9 * p.dist(q) { unit((m.x - x.x, m.y - x.y)) } else { unit((q.x - p.x, q.y - p.y)) }
    };
    let u1 = side(a, b);
    let mut u2 = side(c, d);
    if u1.x * u2.x + u1.y * u2.y < 0.0 {
        u2 = Pt::new(-u2.x, -u2.y);
    }
    Some((x, u1, u2))
}

/// Cercle passant par trois points : (centre, rayon). `None` si alignés.
pub fn circle_from_3_points(a: Pt, b: Pt, c: Pt) -> Option<(Pt, f64)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-9 {
        return None;
    }
    let (a2, b2, c2) = (a.x * a.x + a.y * a.y, b.x * b.x + b.y * b.y, c.x * c.x + c.y * c.y);
    let ux = (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d;
    let uy = (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d;
    let center = Pt::new(ux, uy);
    Some((center, center.dist(a)))
}

/// Distance d'un point à un segment.
pub fn dist_point_segment(p: Pt, a: Pt, b: Pt) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return p.dist(a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0);
    p.dist(a.lerp(b, t))
}

/// Test point-dans-polygone (règle pair-impair).
pub fn point_in_polygon(p: Pt, pts: &[Pt]) -> bool {
    let n = pts.len();
    let mut inside = false;
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (pts[i], pts[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basics() {
        meteroscope_i18n::set_language("fr");
        let sq = [Pt::new(0., 0.), Pt::new(10., 0.), Pt::new(10., 10.), Pt::new(0., 10.)];
        assert_eq!(polygon_area(&sq), 100.0);
        assert_eq!(polygon_perimeter(&sq), 40.0);
        // Trou de serrure : carré 10 × 10 évidé de 4 × 4, la fente (0,0)–(3,3)
        // est parcourue deux fois et ne compte pas.
        let p = |x, y| Pt::new(x, y);
        let keyhole = [p(0., 0.), p(3., 3.), p(3., 7.), p(7., 7.), p(7., 3.), p(3., 3.), p(0., 0.), p(10., 0.), p(10., 10.), p(0., 10.)];
        assert_eq!((polygon_area(&keyhole), polygon_perimeter(&keyhole)), (84.0, 56.0));
        assert_eq!(polyline_length(&sq), 30.0);
        assert!((angle_deg(sq[1], sq[0], sq[3]) - 90.0).abs() < 1e-9);
        let (c, r) = circle_from_3_points(Pt::new(1., 0.), Pt::new(0., 1.), Pt::new(-1., 0.)).unwrap();
        assert!(c.dist(Pt::new(0., 0.)) < 1e-9 && (r - 1.0).abs() < 1e-9);
        assert!(circle_from_3_points(sq[0], Pt::new(1., 1.), Pt::new(2., 2.)).is_none());
        assert!(point_in_polygon(Pt::new(5., 5.), &sq));
        assert!(!point_in_polygon(Pt::new(15., 5.), &sq));
        assert_eq!(dist_point_segment(Pt::new(5., 3.), sq[0], sq[1]), 3.0);
    }

    #[test]
    fn inclinaisons() {
        let o = Pt::new(0., 0.);
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        // Parallèles quel que soit le sens de tracé.
        assert!(close(line_angle_deg(o, Pt::new(10., 0.), Pt::new(5., 5.), Pt::new(-5., 5.)), 0.0));
        assert!(close(line_angle_deg(o, Pt::new(10., 0.), o, Pt::new(0., 3.)), 90.0));
        assert!(close(line_angle_deg(o, Pt::new(10., 0.), o, Pt::new(-10., 10.)), 45.0));
        assert!(close(line_angle_deg(o, Pt::new(10., 0.), o, Pt::new(10., -1.)), 1f64.atan2(10.).to_degrees()));
        assert_eq!(line_angle_deg(o, o, o, Pt::new(1., 1.)), 0.0);

        // Segment presque horizontal penché de 5° (sens horaire) : on tourne de −5°.
        let b = Pt::new(5f64.to_radians().cos(), 5f64.to_radians().sin());
        assert!(close(straighten_rotation(o, b, 0.0).unwrap(), -5.0));
        // Tracé dans l'autre sens : même correction.
        assert!(close(straighten_rotation(b, o, 0.0).unwrap(), -5.0));
        // Presque vertical (penché de 3° vers la gauche en bas) : vers la verticale.
        let v = Pt::new(-(3f64.to_radians().sin()), 3f64.to_radians().cos());
        assert!(close(straighten_rotation(o, v, 0.0).unwrap(), -3.0));
        // Indépendant de la rotation courante quand l'axe visé reste le même.
        assert!(close(straighten_rotation(o, b, -2.0).unwrap(), -5.0));
        assert!(straighten_rotation(o, o, 0.0).is_none());
        assert!(close(normalize_deg(270.0), -90.0) && close(normalize_deg(-180.0), 180.0));
    }

    #[test]
    fn rectangle_oriente_et_intersection() {
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        let (a, b) = (Pt::new(0.0, 0.0), Pt::new(10.0, 5.0));
        assert_eq!(rect_corners(a, b, 0.0), [a, Pt::new(10.0, 0.0), b, Pt::new(0.0, 5.0)]);
        // Côtés à 90° : mêmes coins, largeur et hauteur échangées.
        let c = rect_corners(a, b, 90.0);
        assert!(c[1].dist(Pt::new(0.0, 5.0)) < 1e-9 && c[3].dist(Pt::new(10.0, 0.0)) < 1e-9);
        let (w, h) = rect_size(a, b, 90.0);
        assert!(close(w, 5.0) && close(h, 10.0));
        // Carré de côté 10 tourné de 30° : coins opposés sur la diagonale.
        let t = 30f64.to_radians();
        let b = Pt::new(10.0 * (t.cos() - t.sin()), 10.0 * (t.sin() + t.cos()));
        let (w, h) = rect_size(a, b, 30.0);
        assert!(close(w, 10.0) && close(h, 10.0));
        assert!(rect_corners(a, b, 30.0)[1].dist(Pt::new(10.0 * t.cos(), 10.0 * t.sin())) < 1e-9);
        assert!(to_frame(from_frame(Pt::new(3.0, -2.0), 17.0), 17.0).dist(Pt::new(3.0, -2.0)) < 1e-9);

        let p = |x, y| Pt::new(x, y);
        let (a, b, ang) = rect_from_3_points(p(0., 0.), p(10., 0.), p(3., 4.));
        assert!(a == p(0., 0.) && b.dist(p(10., 4.)) < 1e-9 && ang == 0.0);
        let (a, b, ang) = rect_from_3_points(p(0., 0.), p(0., 10.), p(-3., 2.));
        assert!(close(ang, 90.0) && rect_corners(a, b, ang)[2].dist(p(-3., 10.)) < 1e-9);
        assert!(line_intersection(p(0., 0.), p(10., 0.), p(5., 5.), p(5., 8.)).unwrap().dist(p(5., 0.)) < 1e-9);
        assert!(line_intersection(p(0., 0.), p(10., 0.), p(0., 1.), p(10., 1.)).is_none());
        // Segments disjoints : sommet en (5, 0), côtés vers chaque segment.
        let (x, u1, u2) = line_angle_vertex(p(10., 0.), p(20., 0.), p(5., 5.), p(5., 8.)).unwrap();
        assert!(x.dist(p(5., 0.)) < 1e-9 && u1.dist(p(1., 0.)) < 1e-9 && u2.dist(p(0., 1.)) < 1e-9);
        // Angle obtus entre les directions vers les segments : on prend l'aigu.
        let (_, u1, u2) = line_angle_vertex(p(10., 0.), p(20., 0.), p(-5., 5.), p(-10., 10.)).unwrap();
        assert!(u1.x * u2.x + u1.y * u2.y > 0.0);
        // Presque parallèles : pas de sommet.
        assert!(line_angle_vertex(p(0., 0.), p(10., 0.), p(0., 5.), p(10., 5.001)).is_none());
    }
}

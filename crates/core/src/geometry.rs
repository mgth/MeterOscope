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
pub fn polygon_perimeter(pts: &[Pt]) -> f64 {
    match pts.len() {
        0 | 1 => 0.0,
        n => polyline_length(pts) + pts[n - 1].dist(pts[0]),
    }
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

/// Pied de la perpendiculaire abaissée de `p` sur la droite `a→b`, et sa
/// position le long du segment (0 en `a`, 1 en `b`, hors de [0, 1] sur le
/// prolongement). `a` et `b` confondus : le pied est `a`, position 0.
pub fn foot_on_line(p: Pt, a: Pt, b: Pt) -> (Pt, f64) {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len2 = dx * dx + dy * dy;
    if len2 == 0.0 {
        return (a, 0.0);
    }
    let t = ((p.x - a.x) * dx + (p.y - a.y) * dy) / len2;
    (a.lerp(b, t), t)
}

/// Distance perpendiculaire de `p` à la droite (prolongée) `a→b` ; distance
/// à `a` si `a` et `b` sont confondus.
pub fn dist_point_line(p: Pt, a: Pt, b: Pt) -> f64 {
    p.dist(foot_on_line(p, a, b).0)
}

/// Droite parallèle à `a→b` passant par `p` : segment de même direction,
/// couvrant la projection de `a→b` et le point `p` lui-même. Renvoie
/// (début, fin, pied de la cote sur `a→b`, pied de la cote sur la parallèle) :
/// la cote, perpendiculaire aux deux droites, part du milieu de `a→b`.
/// `a` et `b` confondus : segment réduit au point `p`, cote de `a` à `p`.
pub fn parallel_through(a: Pt, b: Pt, p: Pt) -> (Pt, Pt, Pt, Pt) {
    let (f, t) = foot_on_line(p, a, b);
    if a.dist(b) == 0.0 {
        return (p, p, a, p);
    }
    let (dx, dy) = (p.x - f.x, p.y - f.y);
    let on = |s: f64| a.lerp(b, s).offset(dx, dy);
    let (lo, hi) = (t.min(0.0), t.max(1.0));
    let m = a.lerp(b, 0.5);
    (on(lo), on(hi), m, m.offset(dx, dy))
}

/// Demi-axes d'une ellipse de centre `c` : `a` est l'extrémité du premier
/// demi-axe, `b` celle du second (sa distance à la droite `c→a`, pour
/// tolérer un point qui ne serait pas exactement perpendiculaire).
/// Renvoie (premier demi-axe, second demi-axe).
pub fn ellipse_semi_axes(c: Pt, a: Pt, b: Pt) -> (f64, f64) {
    let ra = c.dist(a);
    if ra == 0.0 {
        return (0.0, c.dist(b));
    }
    (ra, dist_point_line(b, c, a))
}

/// Unitaire perpendiculaire au vecteur `c→a`, tourné vers `p` (vers le bas
/// de l'image si `p` est sur la droite). `None` si `c` et `a` sont confondus.
fn normal_toward(c: Pt, a: Pt, p: Pt) -> Option<Pt> {
    let l = c.dist(a);
    if l == 0.0 {
        return None;
    }
    let mut n = Pt::new(-(a.y - c.y) / l, (a.x - c.x) / l);
    let side = (p.x - c.x) * n.x + (p.y - c.y) * n.y;
    if side < 0.0 || (side == 0.0 && n.y < 0.0) {
        n = Pt::new(-n.x, -n.y);
    }
    Some(n)
}

/// Ellipse par trois points : centre `c`, extrémité `a` du premier demi-axe,
/// puis un point `p` dont la distance à cet axe fixe le second demi-axe.
/// Renvoie (c, a, b) avec `b` extrémité du second demi-axe, exactement
/// perpendiculaire au premier (du côté de `p`). Centre et premier point
/// confondus : `p` est gardé tel quel.
pub fn ellipse_from_3_points(c: Pt, a: Pt, p: Pt) -> (Pt, Pt, Pt) {
    match normal_toward(c, a, p) {
        Some(n) => {
            let rb = dist_point_line(p, c, a);
            (c, a, c.offset(n.x * rb, n.y * rb))
        }
        None => (c, a, p),
    }
}

/// Périmètre d'une ellipse de demi-axes `ra`, `rb` : seconde approximation
/// de Ramanujan, π(a+b)(1 + 3h / (10 + √(4 − 3h))) avec h = ((a−b)/(a+b))²,
/// exacte pour le cercle, erreur relative sous 5·10⁻⁴ même très aplatie.
pub fn ellipse_perimeter(ra: f64, rb: f64) -> f64 {
    let (ra, rb) = (ra.abs(), rb.abs());
    let s = ra + rb;
    if s == 0.0 {
        return 0.0;
    }
    let h = ((ra - rb) / s).powi(2);
    std::f64::consts::PI * s * (1.0 + 3.0 * h / (10.0 + (4.0 - 3.0 * h).sqrt()))
}

/// Points du contour d'une ellipse de centre `c` et d'extrémités de demi-axes
/// `a`, `b` : c + (a−c)·cos t + (b−c)·sin t, `n` points (polygone fermé).
pub fn ellipse_outline(c: Pt, a: Pt, b: Pt, n: usize) -> Vec<Pt> {
    let (u, v) = ((a.x - c.x, a.y - c.y), (b.x - c.x, b.y - c.y));
    (0..n)
        .map(|i| {
            let (s, co) = (std::f64::consts::TAU * i as f64 / n as f64).sin_cos();
            Pt::new(c.x + u.0 * co + v.0 * s, c.y + u.1 * co + v.1 * s)
        })
        .collect()
}

/// Arc de cercle passant par trois points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Arc {
    pub center: Pt,
    pub radius: f64,
    /// Direction du centre vers la première extrémité (radians, repère image).
    pub start: f64,
    /// Balayage signé (radians) de la première extrémité à la seconde en
    /// passant par le point intermédiaire ; |sweep| < 2π.
    pub sweep: f64,
}

impl Arc {
    /// Longueur de l'arc.
    pub fn length(&self) -> f64 {
        self.radius * self.sweep.abs()
    }

    /// Point de l'arc à la fraction `t` (0 : première extrémité, 1 : seconde).
    pub fn point(&self, t: f64) -> Pt {
        let (s, c) = (self.start + self.sweep * t).sin_cos();
        self.center.offset(self.radius * c, self.radius * s)
    }
}

/// Arc d'extrémités `a`, `b` passant par `m`. `None` si les trois points
/// sont alignés (ou deux d'entre eux confondus).
pub fn arc_from_3_points(a: Pt, m: Pt, b: Pt) -> Option<Arc> {
    let (center, radius) = circle_from_3_points(a, m, b)?;
    let tau = std::f64::consts::TAU;
    let dir = |p: Pt| (p.y - center.y).atan2(p.x - center.x);
    let start = dir(a);
    let to_b = (dir(b) - start).rem_euclid(tau);
    let to_m = (dir(m) - start).rem_euclid(tau);
    // Sens positif si le point intermédiaire est rencontré avant `b`, sinon négatif.
    let sweep = if to_m < to_b { to_b } else { to_b - tau };
    Some(Arc { center, radius, start, sweep })
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

    #[test]
    fn distance_point_droite_et_paralleles() {
        let p = |x, y| Pt::new(x, y);
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        // Pied dans le segment, puis sur le prolongement.
        let (f, t) = foot_on_line(p(3., 4.), p(0., 0.), p(10., 0.));
        assert!(f == p(3., 0.) && close(t, 0.3));
        let (f, t) = foot_on_line(p(15., -2.), p(0., 0.), p(10., 0.));
        assert!(f == p(15., 0.) && close(t, 1.5));
        assert!(close(dist_point_line(p(15., -2.), p(0., 0.), p(10., 0.)), 2.0));
        // Droite oblique : distance de l'origine à x + y = 10.
        assert!(close(dist_point_line(p(0., 0.), p(10., 0.), p(0., 10.)), 50f64.sqrt()));
        // Point sur la droite, droite dégénérée.
        assert_eq!(dist_point_line(p(20., 0.), p(0., 0.), p(10., 0.)), 0.0);
        assert_eq!(foot_on_line(p(3., 4.), p(1., 1.), p(1., 1.)), (p(1., 1.), 0.0));
        assert!(close(dist_point_line(p(4., 5.), p(1., 1.), p(1., 1.)), 5.0));

        // Parallèle par (15, 3) à (0,0)→(10,0) : couvre x = 0..15, cote au milieu.
        let (s, e, c0, c1) = parallel_through(p(0., 0.), p(10., 0.), p(15., 3.));
        assert!(s.dist(p(0., 3.)) < 1e-9 && e.dist(p(15., 3.)) < 1e-9);
        assert!(c0 == p(5., 0.) && c1.dist(p(5., 3.)) < 1e-9);
        // Point du côté de a : la parallèle s'étend avant a.
        let (s, e, _, _) = parallel_through(p(0., 0.), p(10., 0.), p(-4., -2.));
        assert!(s.dist(p(-4., -2.)) < 1e-9 && e.dist(p(10., -2.)) < 1e-9);
        // Point sur la droite de référence : parallèle confondue, cote nulle.
        let (_, _, c0, c1) = parallel_through(p(0., 0.), p(10., 0.), p(4., 0.));
        assert_eq!(c0.dist(c1), 0.0);
        assert_eq!(parallel_through(p(1., 1.), p(1., 1.), p(4., 5.)), (p(4., 5.), p(4., 5.), p(1., 1.), p(4., 5.)));
    }

    #[test]
    fn ellipses() {
        let p = |x, y| Pt::new(x, y);
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        // Ellipse inclinée à 45° : le troisième point n'a pas besoin d'être sur l'axe.
        let (c, a, b) = ellipse_from_3_points(p(0., 0.), p(3., 3.), p(-1., 3.));
        assert_eq!((c, a), (p(0., 0.), p(3., 3.)));
        let (ra, rb) = ellipse_semi_axes(c, a, b);
        assert!(close(ra, 18f64.sqrt()) && close(rb, 8f64.sqrt()), "{ra} {rb}");
        assert!(close((b.x - c.x) * (a.x - c.x) + (b.y - c.y) * (a.y - c.y), 0.0));
        assert!(b.dist(p(-2., 2.)) < 1e-9, "{b:?}");
        // Troisième point sur l'axe : ellipse plate (second demi-axe nul).
        let (_, _, b) = ellipse_from_3_points(p(0., 0.), p(4., 0.), p(9., 0.));
        assert_eq!(ellipse_semi_axes(p(0., 0.), p(4., 0.), b), (4.0, 0.0));
        // Centre et premier point confondus : rien à redresser.
        assert_eq!(ellipse_from_3_points(p(1., 1.), p(1., 1.), p(5., 4.)), (p(1., 1.), p(1., 1.), p(5., 4.)));
        assert_eq!(ellipse_semi_axes(p(1., 1.), p(1., 1.), p(5., 4.)), (0.0, 5.0));
        // Ramanujan : exact pour le cercle, bornes connues ailleurs.
        assert!(close(ellipse_perimeter(2.0, 2.0), 4.0 * std::f64::consts::PI));
        // Ellipse aplatie en segment : 4a, à 5·10⁻⁴ près en relatif.
        assert!((ellipse_perimeter(5.0, 0.0) - 20.0).abs() < 20.0 * 5e-4);
        // a = 2, b = 1 : 9,688448220547676 (valeur de référence, intégrale elliptique).
        assert!((ellipse_perimeter(2.0, 1.0) - 9.688448220547676).abs() < 1e-5);
        assert_eq!(ellipse_perimeter(0.0, 0.0), 0.0);
        // Contour : points sur l'ellipse, premier point à l'extrémité du premier axe.
        let o = ellipse_outline(p(1., 1.), p(5., 1.), p(1., 3.), 8);
        assert_eq!(o.len(), 8);
        assert!(o[0].dist(p(5., 1.)) < 1e-9 && o[2].dist(p(1., 3.)) < 1e-9);
        for q in o {
            assert!(close(((q.x - 1.) / 4.).powi(2) + ((q.y - 1.) / 2.).powi(2), 1.0));
        }
    }

    #[test]
    fn arcs_par_trois_points() {
        let p = |x, y| Pt::new(x, y);
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;
        let pi = std::f64::consts::PI;
        // Demi-cercle par le haut de l'image (y négatif), puis par le bas.
        let up = arc_from_3_points(p(1., 0.), p(0., -1.), p(-1., 0.)).unwrap();
        assert!(up.center.dist(p(0., 0.)) < 1e-9 && close(up.radius, 1.0));
        assert!(close(up.sweep, -pi) && close(up.length(), pi));
        assert!(up.point(0.5).dist(p(0., -1.)) < 1e-9 && up.point(1.0).dist(p(-1., 0.)) < 1e-9);
        let down = arc_from_3_points(p(1., 0.), p(0., 1.), p(-1., 0.)).unwrap();
        assert!(close(down.sweep, pi) && down.point(0.5).dist(p(0., 1.)) < 1e-9);
        // Petit arc d'un quart de tour, et grand arc (trois quarts) par l'autre côté.
        let s = std::f64::consts::FRAC_1_SQRT_2;
        let q = arc_from_3_points(p(1., 0.), p(s, s), p(0., 1.)).unwrap();
        assert!(close(q.sweep, pi / 2.0));
        let big = arc_from_3_points(p(1., 0.), p(-1., 0.), p(0., 1.)).unwrap();
        assert!(close(big.sweep, -1.5 * pi) && close(big.length(), 1.5 * pi));
        // Alignés ou confondus : pas d'arc.
        assert!(arc_from_3_points(p(0., 0.), p(1., 1.), p(2., 2.)).is_none());
        assert!(arc_from_3_points(p(0., 0.), p(1., 1.), p(0., 0.)).is_none());
        assert!(arc_from_3_points(p(0., 0.), p(0., 0.), p(3., 1.)).is_none());
    }
}

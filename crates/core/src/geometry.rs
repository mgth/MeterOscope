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
        assert_eq!(polyline_length(&sq), 30.0);
        assert!((angle_deg(sq[1], sq[0], sq[3]) - 90.0).abs() < 1e-9);
        let (c, r) = circle_from_3_points(Pt::new(1., 0.), Pt::new(0., 1.), Pt::new(-1., 0.)).unwrap();
        assert!(c.dist(Pt::new(0., 0.)) < 1e-9 && (r - 1.0).abs() < 1e-9);
        assert!(circle_from_3_points(sq[0], Pt::new(1., 1.), Pt::new(2., 2.)).is_none());
        assert!(point_in_polygon(Pt::new(5., 5.), &sq));
        assert!(!point_in_polygon(Pt::new(15., 5.), &sq));
        assert_eq!(dist_point_segment(Pt::new(5., 3.), sq[0], sq[1]), 3.0);
    }
}

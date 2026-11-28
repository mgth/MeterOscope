//! Correction de perspective : homographie entre la photo et le plan d'un
//! objet rectangulaire photographié de biais.
//!
//! L'utilisateur désigne les quatre coins d'un objet rectangulaire dans la
//! réalité ; l'homographie qui envoie ce quadrilatère sur un rectangle
//! redresse tout le plan de l'objet. La photo d'origine n'est pas modifiée :
//! [`Perspective`] (enregistrée dans le document) décrit l'image corrigée,
//! reconstruite à la demande, et c'est dans cette image corrigée que vivent
//! alors les coordonnées des mesures.

use crate::geometry::{Pt, to_frame};
use serde::{Deserialize, Serialize};

/// Part de la surface de la photo que l'image corrigée peut atteindre (les
/// zones proches de la ligne de fuite s'étirent sans limite).
const MAX_GROWTH: f64 = 1.5;
/// On ne reprojette pas au-delà du point où l'image est étirée de
/// `1 / HORIZON_MARGIN` fois plus que le coin le plus étiré du quadrilatère.
const HORIZON_MARGIN: f64 = 0.25;
/// Sinus minimal de l'angle à chaque coin du quadrilatère (≈ 1°).
const MIN_CORNER_SIN: f64 = 0.02;

/// Transformation projective du plan : `(x, y) ↦ (X / W, Y / W)` avec
/// `(X, Y, W) = M · (x, y, 1)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Homography(pub [[f64; 3]; 3]);

fn mul(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

/// Centre et échelle d'un nuage de points, pour conditionner le système.
fn normalizer(p: &[Pt; 4]) -> Option<(Pt, f64)> {
    let c = Pt::new(p.iter().map(|q| q.x).sum::<f64>() / 4.0, p.iter().map(|q| q.y).sum::<f64>() / 4.0);
    let s = p.iter().map(|q| q.dist(c)).sum::<f64>() / 4.0;
    (s.is_finite() && s > 0.0).then_some((c, s))
}

/// Résout `a · x = b` (8 × 8) par élimination de Gauss à pivot partiel.
fn solve8(mut a: [[f64; 9]; 8]) -> Option<[f64; 8]> {
    for col in 0..8 {
        let pivot = (col..8).max_by(|&i, &j| a[i][col].abs().total_cmp(&a[j][col].abs()))?;
        if a[pivot][col].abs() < 1e-10 {
            return None;
        }
        a.swap(col, pivot);
        let pivot_row = a[col];
        for (row, line) in a.iter_mut().enumerate() {
            let k = line[col] / pivot_row[col];
            if row != col && k != 0.0 {
                for (v, p) in line[col..].iter_mut().zip(&pivot_row[col..]) {
                    *v -= k * p;
                }
            }
        }
    }
    Some(std::array::from_fn(|i| a[i][8] / a[i][i]))
}

impl Homography {
    pub const IDENTITY: Self = Self([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);

    /// Homographie qui envoie chaque `src[i]` sur `dst[i]`. `None` si les
    /// points ne la déterminent pas (alignés, confondus) ou si elle
    /// retournerait une partie du quadrilatère (points de part et d'autre de
    /// la ligne de fuite : quadrilatère croisé ou non convexe).
    pub fn from_points(src: &[Pt; 4], dst: &[Pt; 4]) -> Option<Self> {
        // Coordonnées centrées et réduites : système bien conditionné, et
        // l'origine, à l'intérieur du nuage, ne part pas à l'infini (M₃₃ ≠ 0).
        let (cs, ss) = normalizer(src)?;
        let (cd, sd) = normalizer(dst)?;
        let mut a = [[0.0; 9]; 8];
        for i in 0..4 {
            let (x, y) = ((src[i].x - cs.x) / ss, (src[i].y - cs.y) / ss);
            let (u, v) = ((dst[i].x - cd.x) / sd, (dst[i].y - cd.y) / sd);
            a[2 * i] = [x, y, 1.0, 0.0, 0.0, 0.0, -u * x, -u * y, u];
            a[2 * i + 1] = [0.0, 0.0, 0.0, x, y, 1.0, -v * x, -v * y, v];
        }
        let h = solve8(a)?;
        let core = [[h[0], h[1], h[2]], [h[3], h[4], h[5]], [h[6], h[7], 1.0]];
        let to_norm = [[1.0 / ss, 0.0, -cs.x / ss], [0.0, 1.0 / ss, -cs.y / ss], [0.0, 0.0, 1.0]];
        let from_norm = [[sd, 0.0, cd.x], [0.0, sd, cd.y], [0.0, 0.0, 1.0]];
        let m = Self(mul(&from_norm, &mul(&core, &to_norm)));
        let d = src.map(|p| m.denom(p));
        let ok = d.iter().all(|w| w.is_finite() && *w > 1e-9) && m.0.iter().flatten().all(|v| v.is_finite());
        // Inversible (les points d'arrivée ne sont pas alignés non plus).
        (ok && m.inverse().is_some()).then_some(m)
    }

    /// Dénominateur projectif en `p` : il s'annule sur la ligne de fuite et
    /// change de signe au-delà.
    pub fn denom(&self, p: Pt) -> f64 {
        let m = &self.0;
        m[2][0] * p.x + m[2][1] * p.y + m[2][2]
    }

    /// Image du point `p` (non finie sur la ligne de fuite).
    pub fn apply(&self, p: Pt) -> Pt {
        let m = &self.0;
        let w = self.denom(p);
        Pt::new((m[0][0] * p.x + m[0][1] * p.y + m[0][2]) / w, (m[1][0] * p.x + m[1][1] * p.y + m[1][2]) / w)
    }

    /// Transformation réciproque. `None` si la matrice n'est pas inversible.
    pub fn inverse(&self) -> Option<Self> {
        let m = &self.0;
        let cof = |r: usize, c: usize| {
            let (r1, r2, c1, c2) = ((r + 1) % 3, (r + 2) % 3, (c + 1) % 3, (c + 2) % 3);
            m[r1][c1] * m[r2][c2] - m[r1][c2] * m[r2][c1]
        };
        let det = m[0][0] * cof(0, 0) + m[0][1] * cof(0, 1) + m[0][2] * cof(0, 2);
        let rows: f64 = m.iter().map(|r| r[0].hypot(r[1]).hypot(r[2])).product();
        if !det.is_finite() || det.abs() <= 1e-18 * rows {
            return None;
        }
        // Inverse = transposée de la comatrice / déterminant.
        Some(Self(std::array::from_fn(|i| std::array::from_fn(|j| cof(j, i) / det))))
    }

    /// `self ∘ first` : applique `first` puis `self`.
    pub fn after(&self, first: &Homography) -> Self {
        Self(mul(&self.0, &first.0))
    }
}

/// Remet quatre points cliqués dans un ordre quelconque dans l'ordre
/// haut-gauche, haut-droit, bas-droit, bas-gauche, tels qu'on les voit à
/// l'écran (`rot` : rotation d'affichage en degrés, 0 sans redressement).
/// `None` si les points ne forment pas un quadrilatère convexe franc (points
/// confondus ou alignés, un point à l'intérieur du triangle des trois autres).
pub fn order_quad(pts: [Pt; 4], rot: f64) -> Option<[Pt; 4]> {
    if pts.iter().any(|p| !(p.x.is_finite() && p.y.is_finite())) {
        return None;
    }
    // Repère de l'écran.
    let q = pts.map(|p| to_frame(p, -rot));
    let c = Pt::new(q.iter().map(|p| p.x).sum::<f64>() / 4.0, q.iter().map(|p| p.y).sum::<f64>() / 4.0);
    // Angle croissant autour du centre = sens horaire à l'écran (y vers le bas).
    let mut idx = [0usize, 1, 2, 3];
    idx.sort_by(|&i, &j| (q[i].y - c.y).atan2(q[i].x - c.x).total_cmp(&(q[j].y - c.y).atan2(q[j].x - c.x)));
    for k in 0..4 {
        let (a, b, d) = (q[idx[k]], q[idx[(k + 1) % 4]], q[idx[(k + 2) % 4]]);
        let (ux, uy, vx, vy) = (b.x - a.x, b.y - a.y, d.x - b.x, d.y - b.y);
        let (lu, lv) = (ux.hypot(uy), vx.hypot(vy));
        // Chaque coin tourne franchement dans le même sens.
        if !(lu > 0.0 && lv > 0.0 && ux * vy - uy * vx > MIN_CORNER_SIN * lu * lv) {
            return None;
        }
    }
    // Départ : le coin qui rapproche le plus le quadrilatère des coins de sa boîte.
    let (x0, x1) = (q.iter().map(|p| p.x).fold(f64::INFINITY, f64::min), q.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max));
    let (y0, y1) = (q.iter().map(|p| p.y).fold(f64::INFINITY, f64::min), q.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max));
    let corners = [Pt::new(x0, y0), Pt::new(x1, y0), Pt::new(x1, y1), Pt::new(x0, y1)];
    let cost = |s: usize| (0..4).map(|i| q[idx[(s + i) % 4]].dist(corners[i]).powi(2)).sum::<f64>();
    let start = (0..4).min_by(|&a, &b| cost(a).total_cmp(&cost(b)))?;
    Some(std::array::from_fn(|i| pts[idx[(start + i) % 4]]))
}

/// Rapport largeur / hauteur du rectangle réel dont `quad` (haut-gauche,
/// haut-droit, bas-droit, bas-gauche) est la photo, estimé à partir de la
/// seule perspective (Zhang et He, « Whiteboard scanning and image
/// enhancement ») en supposant des pixels carrés et un axe optique passant
/// par `center` (le centre d'une photo non recadrée).
///
/// Quand la perspective ne permet pas de retrouver la focale (côtés opposés
/// presque parallèles), on se rabat sur le rapport des côtés moyens.
pub fn estimate_aspect(quad: &[Pt; 4], center: Pt) -> f64 {
    let [tl, tr, br, bl] = *quad;
    let fallback = (tl.dist(tr) + bl.dist(br)) / (tl.dist(bl) + tr.dist(br));
    let h = |p: Pt| [p.x - center.x, p.y - center.y, 1.0];
    let cross = |a: [f64; 3], b: [f64; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let (m1, m2, m3, m4) = (h(tl), h(tr), h(bl), h(br));
    let k2 = dot(cross(m1, m4), m3) / dot(cross(m2, m4), m3);
    let k3 = dot(cross(m1, m4), m2) / dot(cross(m3, m4), m2);
    let n2: [f64; 3] = std::array::from_fn(|i| k2 * m2[i] - m1[i]);
    let n3: [f64; 3] = std::array::from_fn(|i| k3 * m3[i] - m1[i]);
    let size = tl.dist(br).max(tr.dist(bl));
    // Fuite négligeable sur l'un des axes : la focale n'est pas déterminée.
    let flat = |n: [f64; 3]| n[2].abs() * size < 1e-6 * n[0].hypot(n[1]);
    if flat(n2) || flat(n3) {
        return fallback;
    }
    let f2 = -(n2[0] * n3[0] + n2[1] * n3[1]) / (n2[2] * n3[2]);
    if !(f2.is_finite() && f2 > 0.0) {
        return fallback;
    }
    let len2 = |n: [f64; 3]| (n[0] * n[0] + n[1] * n[1]) / f2 + n[2] * n[2];
    let r = (len2(n2) / len2(n3)).sqrt();
    if r.is_finite() && r > 0.0 { r } else { fallback }
}

/// Correction de perspective appliquée à une photo : l'image corrigée, de
/// taille `size`, est la photo reprojetée par l'homographie qui envoie
/// `src[i]` (pixels de la photo) sur `dst[i]` (pixels de l'image corrigée).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Perspective {
    /// Coins de l'objet rectangulaire dans la photo d'origine : haut-gauche,
    /// haut-droit, bas-droit, bas-gauche.
    pub src: [Pt; 4],
    /// Les mêmes coins dans l'image corrigée : ils y forment un rectangle.
    pub dst: [Pt; 4],
    /// Taille de l'image corrigée, en pixels.
    pub size: (u32, u32),
}

impl Perspective {
    /// Correction qui redresse le quadrilatère `quad` (coins dans l'ordre de
    /// [`order_quad`], en pixels d'une photo de taille `img`) en un rectangle
    /// de rapport largeur / hauteur `aspect` (`None` : estimé, voir
    /// [`estimate_aspect`]).
    ///
    /// Le rectangle garde la résolution de son côté le mieux résolu dans la
    /// photo. L'image corrigée couvre toute la photo reprojetée, sauf ce qui
    /// s'approche trop de la ligne de fuite ou ferait exploser sa taille.
    pub fn from_quad(quad: [Pt; 4], img: (u32, u32), aspect: Option<f64>) -> Option<Self> {
        let (iw, ih) = (img.0 as f64, img.1 as f64);
        if iw < 1.0 || ih < 1.0 {
            return None;
        }
        let valid = |a: &f64| a.is_finite() && *a > 0.0;
        let aspect = aspect.filter(valid).unwrap_or_else(|| estimate_aspect(&quad, Pt::new(iw / 2.0, ih / 2.0)));
        if !valid(&aspect) {
            return None;
        }
        let [tl, tr, br, bl] = quad;
        let (wpx, hpx) = (tl.dist(tr).max(bl.dist(br)), tl.dist(bl).max(tr.dist(br)));
        let mut w = wpx.max(hpx * aspect);
        let mut h = w / aspect;
        let cap = MAX_GROWTH * iw * ih;
        if w * h > cap {
            let s = (cap / (w * h)).sqrt();
            (w, h) = (w * s, h * s);
        }
        if !(w >= 1.0 && h >= 1.0) {
            return None;
        }
        let rect = [Pt::new(0.0, 0.0), Pt::new(w, 0.0), Pt::new(w, h), Pt::new(0.0, h)];
        let hm = Homography::from_points(&quad, &rect)?;

        // Photo entière, rognée avant la ligne de fuite, puis reprojetée.
        let limit = HORIZON_MARGIN * quad.iter().map(|p| hm.denom(*p)).fold(f64::INFINITY, f64::min);
        let corners = [Pt::new(0.0, 0.0), Pt::new(iw, 0.0), Pt::new(iw, ih), Pt::new(0.0, ih)];
        let (mut x0, mut y0, mut x1, mut y1) = (0.0f64, 0.0f64, w, h);
        let mut grow = |p: Pt| {
            let q = hm.apply(p);
            (x0, y0, x1, y1) = (x0.min(q.x), y0.min(q.y), x1.max(q.x), y1.max(q.y));
        };
        for i in 0..4 {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            let (da, db) = (hm.denom(a) - limit, hm.denom(b) - limit);
            if da >= 0.0 {
                grow(a);
            }
            if (da >= 0.0) != (db >= 0.0) {
                grow(a.lerp(b, da / (da - db)));
            }
        }
        if ![x0, y0, x1, y1].iter().all(|v| v.is_finite()) {
            return None;
        }
        // Trop grand : on resserre le cadre vers le rectangle.
        let area = |t: f64| (w + t * (x1 - w - x0)) * (h + t * (y1 - h - y0));
        let mut t = 1.0;
        if area(1.0) > cap.max(w * h) {
            let (mut lo, mut hi) = (0.0, 1.0);
            for _ in 0..40 {
                let mid = (lo + hi) / 2.0;
                if area(mid) > cap.max(w * h) { hi = mid } else { lo = mid }
            }
            t = lo;
        }
        let (x0, y0) = ((t * x0 + 1e-6).floor(), (t * y0 + 1e-6).floor());
        let (x1, y1) = (w + t * (x1 - w), h + t * (y1 - h));
        let size = (((x1 - x0) - 1e-6).ceil().max(1.0), ((y1 - y0) - 1e-6).ceil().max(1.0));
        if size.0 > u32::MAX as f64 || size.1 > u32::MAX as f64 {
            return None;
        }
        Some(Self { src: quad, dst: rect.map(|p| p.offset(-x0, -y0)), size: (size.0 as u32, size.1 as u32) })
    }

    /// Photo → image corrigée.
    pub fn homography(&self) -> Option<Homography> {
        Homography::from_points(&self.src, &self.dst)
    }

    /// Largeur et hauteur du rectangle redressé, en pixels de l'image corrigée.
    pub fn rect_size(&self) -> (f64, f64) {
        (self.dst[0].dist(self.dst[1]), self.dst[0].dist(self.dst[3]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad() -> [Pt; 4] {
        [Pt::new(120.0, 80.0), Pt::new(520.0, 130.0), Pt::new(480.0, 420.0), Pt::new(60.0, 330.0)]
    }

    #[test]
    fn four_points_and_round_trip() {
        let dst = [Pt::new(0.0, 0.0), Pt::new(300.0, 0.0), Pt::new(300.0, 200.0), Pt::new(0.0, 200.0)];
        let h = Homography::from_points(&quad(), &dst).unwrap();
        for (s, d) in quad().iter().zip(&dst) {
            assert!(h.apply(*s).dist(*d) < 1e-8, "{:?} → {:?}", s, h.apply(*s));
        }
        let inv = h.inverse().unwrap();
        for p in [Pt::new(200.0, 200.0), Pt::new(0.0, 0.0), Pt::new(640.0, 480.0), Pt::new(-50.0, 700.0)] {
            assert!(inv.apply(h.apply(p)).dist(p) < 1e-7, "{p:?}");
            assert!(h.apply(inv.apply(p)).dist(p) < 1e-7, "{p:?}");
        }
        // Les droites restent des droites : un milieu reste aligné.
        let (a, b) = (Pt::new(100.0, 100.0), Pt::new(400.0, 300.0));
        let (pa, pm, pb) = (h.apply(a), h.apply(a.lerp(b, 0.5)), h.apply(b));
        let cross = (pm.x - pa.x) * (pb.y - pa.y) - (pm.y - pa.y) * (pb.x - pa.x);
        assert!(cross.abs() < 1e-6);
        // Composition.
        let id = h.after(&inv);
        assert!(id.apply(Pt::new(12.0, 34.0)).dist(Pt::new(12.0, 34.0)) < 1e-8);
        assert_eq!(Homography::IDENTITY.apply(Pt::new(3.0, 4.0)), Pt::new(3.0, 4.0));
    }

    #[test]
    fn translation_far_from_origin_is_well_conditioned() {
        // Coordonnées d'une photo de 24 Mpx : le système reste précis.
        let src = quad().map(|p| p.offset(5000.0, 3500.0));
        let dst = [Pt::new(0.0, 0.0), Pt::new(4000.0, 0.0), Pt::new(4000.0, 2500.0), Pt::new(0.0, 2500.0)];
        let h = Homography::from_points(&src, &dst).unwrap();
        for (s, d) in src.iter().zip(&dst) {
            assert!(h.apply(*s).dist(*d) < 1e-6);
        }
    }

    #[test]
    fn degenerate_points_are_refused() {
        let rect = [Pt::new(0.0, 0.0), Pt::new(1.0, 0.0), Pt::new(1.0, 1.0), Pt::new(0.0, 1.0)];
        let p = Pt::new;
        // Trois points alignés, points confondus, valeurs non finies.
        assert!(Homography::from_points(&[p(0.0, 0.0), p(1.0, 0.0), p(2.0, 0.0), p(0.0, 1.0)], &rect).is_none());
        assert!(Homography::from_points(&[p(0.0, 0.0), p(0.0, 0.0), p(1.0, 1.0), p(0.0, 1.0)], &rect).is_none());
        assert!(Homography::from_points(&[p(5.0, 5.0); 4], &rect).is_none());
        assert!(Homography::from_points(&[p(0.0, 0.0), p(f64::NAN, 0.0), p(1.0, 1.0), p(0.0, 1.0)], &rect).is_none());
        // Quadrilatère croisé (« nœud papillon ») ou non convexe, dans cet ordre.
        assert!(Homography::from_points(&[p(0.0, 0.0), p(10.0, 0.0), p(0.0, 10.0), p(10.0, 10.0)], &rect).is_none());
        assert!(Homography::from_points(&[p(0.0, 0.0), p(10.0, 0.0), p(2.0, 2.0), p(0.0, 10.0)], &rect).is_none());
        // Matrice singulière.
        assert!(Homography([[1.0, 2.0, 3.0], [2.0, 4.0, 6.0], [0.0, 0.0, 1.0]]).inverse().is_none());
    }

    #[test]
    fn clicks_in_any_order() {
        let want = quad();
        let perms = [[0, 1, 2, 3], [3, 2, 1, 0], [2, 0, 3, 1], [1, 3, 0, 2], [0, 2, 1, 3], [3, 0, 2, 1]];
        for perm in perms {
            let clicked = perm.map(|i| want[i]);
            assert_eq!(order_quad(clicked, 0.0), Some(want), "{perm:?}");
        }
        // Vue pivotée d'un quart de tour horaire : le coin en haut à gauche
        // de l'écran est celui qui est en bas à gauche dans l'image.
        let r = order_quad(want, 90.0).unwrap();
        assert_eq!(r, [want[3], want[0], want[1], want[2]]);
    }

    #[test]
    fn bad_quads_are_refused() {
        let p = Pt::new;
        // Un point à l'intérieur du triangle des trois autres.
        assert!(order_quad([p(0.0, 0.0), p(100.0, 0.0), p(50.0, 100.0), p(50.0, 30.0)], 0.0).is_none());
        // Trois points alignés, deux points confondus, quatre points alignés.
        assert!(order_quad([p(0.0, 0.0), p(50.0, 0.0), p(100.0, 0.0), p(50.0, 80.0)], 0.0).is_none());
        assert!(order_quad([p(0.0, 0.0), p(0.0, 0.0), p(100.0, 0.0), p(50.0, 80.0)], 0.0).is_none());
        assert!(order_quad([p(0.0, 0.0), p(1.0, 1.0), p(2.0, 2.0), p(3.0, 3.0)], 0.0).is_none());
        assert!(order_quad([p(0.0, 0.0), p(f64::INFINITY, 1.0), p(2.0, 5.0), p(3.0, 3.0)], 0.0).is_none());
        // Un quadrilatère convexe cliqué « en croix » est simplement remis en ordre.
        assert!(order_quad([p(0.0, 0.0), p(100.0, 100.0), p(100.0, 0.0), p(0.0, 100.0)], 0.0).is_some());
    }

    /// Photo (sténopé de focale `f`, axe optique en `c`) d'un rectangle
    /// `w` × `h` posé dans un plan incliné.
    fn photographed(w: f64, h: f64, f: f64, c: Pt, yaw: f64, pitch: f64) -> [Pt; 4] {
        let (sy, cy) = yaw.to_radians().sin_cos();
        let (sp, cp) = pitch.to_radians().sin_cos();
        [(-w / 2.0, -h / 2.0), (w / 2.0, -h / 2.0), (w / 2.0, h / 2.0), (-w / 2.0, h / 2.0)].map(|(x, y)| {
            // Rotation autour de l'axe vertical puis de l'axe horizontal, recul de 10.
            let (x1, z1) = (cy * x, -sy * x);
            let (y2, z2) = (cp * y - sp * z1, sp * y + cp * z1);
            let z = z2 + 10.0;
            Pt::new(c.x + f * x1 / z, c.y + f * y2 / z)
        })
    }

    #[test]
    fn aspect_is_recovered_from_perspective() {
        let c = Pt::new(2000.0, 1500.0);
        for (w, h, yaw, pitch) in [(4.0, 3.0, 25.0, 35.0), (2.0, 5.0, -40.0, 20.0), (3.0, 3.0, 15.0, -50.0), (6.0, 1.0, 30.0, 30.0)] {
            let q = photographed(w, h, 3200.0, c, yaw, pitch);
            let a = estimate_aspect(&q, c);
            assert!((a - w / h).abs() < 1e-6 * (w / h), "{w}×{h} : {a}");
        }
        // Vue de face ou fuite sur un seul axe : rapport des côtés moyens.
        let a = estimate_aspect(&photographed(4.0, 3.0, 3200.0, c, 0.0, 0.0), c);
        assert!((a - 4.0 / 3.0).abs() < 1e-9);
        let a = estimate_aspect(&photographed(4.0, 3.0, 3200.0, c, 0.0, 30.0), c);
        assert!(a.is_finite() && a > 0.5 && a < 3.0, "{a}");
    }

    #[test]
    fn frame_of_the_corrected_image() {
        let c = Pt::new(2000.0, 1500.0);
        let q = photographed(4.0, 3.0, 3200.0, c, 25.0, 35.0);
        let p = Perspective::from_quad(q, (4000, 3000), None).unwrap();
        let (w, h) = p.rect_size();
        assert!((w / h - 4.0 / 3.0).abs() < 1e-6, "{w} × {h}");
        // Le rectangle garde au moins la résolution de la photo.
        assert!(w >= q[0].dist(q[1]).max(q[3].dist(q[2])) - 1e-6 && h >= q[0].dist(q[3]).max(q[1].dist(q[2])) - 1e-6);
        // Rectangle droit, entièrement dans l'image corrigée, de taille raisonnable.
        let d = p.dst;
        assert!((d[0].y - d[1].y).abs() < 1e-9 && (d[0].x - d[3].x).abs() < 1e-9 && (d[2].x - d[1].x).abs() < 1e-9);
        assert!(d.iter().all(|v| v.x >= 0.0 && v.y >= 0.0 && v.x <= p.size.0 as f64 && v.y <= p.size.1 as f64));
        assert!(p.size.0 as f64 * p.size.1 as f64 <= MAX_GROWTH * 4000.0 * 3000.0 + 2.0 * (p.size.0 + p.size.1) as f64, "{:?}", p.size);
        let hm = p.homography().unwrap();
        assert!(hm.apply(q[2]).dist(d[2]) < 1e-6);
        // Rapport imposé par les dimensions réelles.
        let p = Perspective::from_quad(q, (4000, 3000), Some(2.0)).unwrap();
        let (w, h) = p.rect_size();
        assert!((w / h - 2.0).abs() < 1e-9);
        // Vue de face : la photo entière est conservée, sans marge.
        let q = [Pt::new(100.0, 100.0), Pt::new(300.0, 100.0), Pt::new(300.0, 200.0), Pt::new(100.0, 200.0)];
        let p = Perspective::from_quad(q, (400, 300), None).unwrap();
        assert_eq!((p.size, p.dst[0]), ((400, 300), Pt::new(100.0, 100.0)));
        // Quadrilatère inutilisable.
        assert!(Perspective::from_quad([Pt::new(1.0, 1.0); 4], (400, 300), None).is_none());
        assert!(Perspective::from_quad(q, (0, 300), None).is_none());
    }

    #[test]
    fn horizon_does_not_blow_up_the_frame() {
        // Plan très incliné : la ligne de fuite traverse la photo.
        let q = [Pt::new(450.0, 500.0), Pt::new(550.0, 500.0), Pt::new(700.0, 900.0), Pt::new(300.0, 900.0)];
        let p = Perspective::from_quad(q, (1000, 1000), None).unwrap();
        assert!(p.size.0 as f64 * p.size.1 as f64 <= MAX_GROWTH * 1e6 + 2.0 * (p.size.0 + p.size.1) as f64, "{:?}", p.size);
        assert!(p.dst.iter().all(|v| v.x >= 0.0 && v.y >= 0.0 && v.x <= p.size.0 as f64 && v.y <= p.size.1 as f64));
    }
}

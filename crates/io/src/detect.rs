//! Détection automatique d'objets par seuillage : particules, grains,
//! cellules, gouttes, trous ou pièces posés sur un fond contrasté.
//!
//! Chaîne de traitement (fonctions pures) :
//! 1. [`prepare`] : luminance (BT.601) de la zone utile, réduite si besoin
//!    par moyenne de blocs (aperçu), lissage 3×3 facultatif, masque de la
//!    zone de détection (polygone) ;
//! 2. seuil (initialisé par la méthode d'Otsu, [`Prepared::otsu`]) ;
//! 3. bouchage des trous facultatif, étiquetage en composantes connexes
//!    (8-connexité), filtres d'aire et de contact avec le bord ;
//! 4. pour chaque objet retenu : aire (pixels), centre de gravité, contour
//!    extérieur suivi le long des bords de pixels puis simplifié
//!    (Douglas-Peucker).
//!
//! Travaille sur l'image d'origine (sans le réglage luminosité/contraste) :
//! l'aperçu et le résultat sont ainsi calculés sur les mêmes valeurs.

use image::RgbaImage;
use meteroscope_core::Pt;
use meteroscope_core::geometry::point_in_polygon;
use std::sync::mpsc;

/// Nombre maximal de fils de calcul.
const MAX_THREADS: usize = 4;
/// Tolérance de simplification des contours (pixels image), réduite pour
/// les petits objets (voir [`simplify_tolerance`]).
pub const SIMPLIFY_TOLERANCE: f64 = 1.0;

/// Tolérance pour un objet de `area` pixels : un pixel au-delà d'environ
/// 250 px (rayon 9), moins en dessous pour que les petits objets ne
/// deviennent pas des carrés (aire sous-estimée de près de 20 % sinon).
pub fn simplify_tolerance(area: f64) -> f64 {
    (area.sqrt() / 16.0).clamp(0.25, SIMPLIFY_TOLERANCE)
}

/// Réglages de la détection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DetectParams {
    /// Seuil de luminance (0–255).
    pub threshold: u8,
    /// Objets clairs (luminance > seuil) ou sombres (luminance ≤ seuil).
    pub bright: bool,
    /// Aire minimale et maximale d'un objet, en pixels de l'image d'origine.
    pub min_area: f64,
    pub max_area: f64,
    /// Écarter les objets qui touchent le bord de l'image ou de la zone.
    pub exclude_border: bool,
    /// Boucher les trous des objets avant de les mesurer.
    pub fill_holes: bool,
}

impl Default for DetectParams {
    fn default() -> Self {
        Self {
            threshold: 128,
            bright: false,
            min_area: 20.0,
            max_area: f64::INFINITY,
            exclude_border: true,
            fill_holes: true,
        }
    }
}

/// Image en niveaux de gris prête pour la détection : zone utile, à
/// l'échelle `1 / scale` de l'image d'origine.
#[derive(Clone, Debug)]
pub struct Prepared {
    pub width: usize,
    pub height: usize,
    /// Luminance, ligne par ligne.
    pub luma: Vec<u8>,
    /// Pixels dans la zone de détection (`None` : tous).
    zone: Option<Vec<bool>>,
    /// Coin haut-gauche de la zone utile dans l'image d'origine.
    pub origin: (u32, u32),
    /// Côté d'un pixel préparé, en pixels de l'image d'origine (≥ 1).
    pub scale: u32,
}

/// Un objet détecté, en coordonnées de l'image d'origine.
#[derive(Clone, Debug, PartialEq)]
pub struct Object {
    /// Aire en pixels de l'image d'origine (nombre de pixels × scale²),
    /// trous compris s'ils sont bouchés.
    pub area: f64,
    /// Centre de gravité des pixels.
    pub centroid: Pt,
    /// Contour simplifié (polygone fermé, au moins 3 points) ; vide si les
    /// contours n'ont pas été demandés. Si les trous sont gardés, chacun y est
    /// raccordé en « trou de serrure » (voir [`bridge_hole`]) : l'aire du
    /// polygone suit alors `area`, trous déduits.
    pub polygon: Vec<Pt>,
}

/// Résultat d'une détection.
#[derive(Clone, Debug, Default)]
pub struct Detection {
    /// Objets retenus, dans l'ordre de lecture (haut en bas, gauche à droite).
    pub objects: Vec<Object>,
    /// Objets écartés par les filtres (aire, bord).
    pub rejected: usize,
}

// ---------- préparation ----------

/// Rectangle entier `(x0, y0, w, h)` de l'image couvrant la zone.
fn zone_bounds(img_w: u32, img_h: u32, zone: Option<&[Pt]>) -> (u32, u32, u32, u32) {
    let Some(z) = zone.filter(|z| z.len() >= 3) else { return (0, 0, img_w, img_h) };
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in z {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    let cx = |v: f64, max: u32| (v.max(0.0) as u32).min(max);
    let (x0, y0) = (cx(x0.floor(), img_w), cx(y0.floor(), img_h));
    let (x1, y1) = (cx(x1.ceil(), img_w), cx(y1.ceil(), img_h));
    (x0, y0, x1 - x0, y1 - y0)
}

/// Applique `f(ligne, pixels de la ligne)` à chaque ligne de `out`
/// (largeur `w`), sur quelques fils pour les grandes images.
fn par_rows<T: Send, F: Fn(usize, &mut [T]) + Sync>(out: &mut [T], w: usize, f: F) {
    if w == 0 || out.is_empty() {
        return;
    }
    let h = out.len() / w;
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(MAX_THREADS);
    if threads <= 1 || out.len() < 1 << 18 {
        for (y, row) in out.chunks_mut(w).enumerate() {
            f(y, row);
        }
        return;
    }
    let rows = h.div_ceil(threads);
    std::thread::scope(|s| {
        for (ci, chunk) in out.chunks_mut(rows * w).enumerate() {
            let f = &f;
            s.spawn(move || {
                for (i, row) in chunk.chunks_mut(w).enumerate() {
                    f(ci * rows + i, row);
                }
            });
        }
    });
}

/// Luminance BT.601 entière d'un pixel RGBA.
fn luma(p: &[u8; 4]) -> u32 {
    (77 * p[0] as u32 + 150 * p[1] as u32 + 29 * p[2] as u32 + 128) >> 8
}

/// Prépare la zone utile de `img` : la zone `zone` (polygone en pixels
/// image, `None` : image entière), réduite pour que son plus grand côté ne
/// dépasse pas `max_side` (moyenne de blocs entiers), lissée si `smooth`.
pub fn prepare(img: &RgbaImage, zone: Option<&[Pt]>, max_side: u32, smooth: bool) -> Prepared {
    let zone = zone.filter(|z| z.len() >= 3);
    let (x0, y0, zw, zh) = zone_bounds(img.width(), img.height(), zone);
    let scale = zw.max(zh).div_ceil(max_side.max(1)).max(1);
    let (w, h) = (zw.div_ceil(scale) as usize, zh.div_ceil(scale) as usize);
    let mut lum = vec![0u8; w * h];
    let (iw, raw) = (img.width() as usize, img.as_raw());
    let k = scale as usize;
    par_rows(&mut lum, w, |oy, row| {
        let sy0 = y0 as usize + oy * k;
        let sy1 = (sy0 + k).min((y0 + zh) as usize);
        for (ox, out) in row.iter_mut().enumerate() {
            let sx0 = x0 as usize + ox * k;
            let sx1 = (sx0 + k).min((x0 + zw) as usize);
            let mut sum = 0u32;
            for sy in sy0..sy1 {
                let line = &raw[(sy * iw + sx0) * 4..(sy * iw + sx1) * 4];
                sum += line.as_chunks::<4>().0.iter().map(luma).sum::<u32>();
            }
            let n = ((sy1 - sy0) * (sx1 - sx0)) as u32;
            *out = ((sum + n / 2) / n.max(1)) as u8;
        }
    });
    if smooth {
        lum = smooth3(&lum, w, h);
    }
    let zone = zone.map(|z| {
        let mut m = vec![false; w * h];
        par_rows(&mut m, w, |oy, row| {
            let cy = y0 as f64 + (oy as f64 + 0.5) * scale as f64;
            for (ox, v) in row.iter_mut().enumerate() {
                let cx = x0 as f64 + (ox as f64 + 0.5) * scale as f64;
                *v = point_in_polygon(Pt::new(cx, cy), z);
            }
        });
        m
    });
    Prepared { width: w, height: h, luma: lum, zone, origin: (x0, y0), scale }
}

/// Lissage 3×3 binomial ([1 2 1] ⊗ [1 2 1] / 16), bords répétés.
pub fn smooth3(src: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = vec![0u8; w * h];
    par_rows(&mut out, w, |y, row| {
        let r = |yy: usize| &src[yy * w..(yy + 1) * w];
        let (a, b, c) = (r(y.saturating_sub(1)), r(y), r((y + 1).min(h - 1)));
        for (x, o) in row.iter_mut().enumerate() {
            let (xl, xr) = (x.saturating_sub(1), (x + 1).min(w - 1));
            let col = |i: usize| a[i] as u32 + 2 * b[i] as u32 + c[i] as u32;
            *o = ((col(xl) + 2 * col(x) + col(xr) + 8) / 16) as u8;
        }
    });
    out
}

/// Seuil d'Otsu d'un histogramme : `t` sépare les classes `[0, t]` et
/// `[t+1, 255]` en maximisant la variance interclasse.
pub fn otsu(hist: &[u64; 256]) -> u8 {
    let total: u64 = hist.iter().sum();
    if total == 0 {
        return 128;
    }
    let sum_all: f64 = hist.iter().enumerate().map(|(i, &n)| i as f64 * n as f64).sum();
    let (mut w0, mut sum0) = (0u64, 0.0);
    let (mut best, mut best_t, mut best_end) = (-1.0, 0usize, 0usize);
    for (t, &n) in hist.iter().enumerate().take(255) {
        w0 += n;
        sum0 += t as f64 * n as f64;
        let w1 = total - w0;
        if w0 == 0 || w1 == 0 {
            continue;
        }
        let (m0, m1) = (sum0 / w0 as f64, (sum_all - sum0) / w1 as f64);
        let v = w0 as f64 * w1 as f64 * (m0 - m1) * (m0 - m1);
        if v > best * (1.0 + 1e-12) {
            (best, best_t, best_end) = (v, t, t);
        } else if (v - best).abs() <= best * 1e-12 {
            // Plateau (aucun pixel entre deux pics) : on prend son milieu.
            best_end = t;
        }
    }
    ((best_t + best_end) / 2) as u8
}

impl Prepared {
    /// Aucun pixel (zone de largeur ou hauteur nulle, ou hors de l'image).
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Le pixel `i` est-il dans la zone de détection ?
    fn in_zone(&self, i: usize) -> bool {
        self.zone.as_ref().is_none_or(|z| z[i])
    }

    /// Histogramme de luminance des pixels de la zone.
    pub fn histogram(&self) -> [u64; 256] {
        let mut h = [0u64; 256];
        for (i, &v) in self.luma.iter().enumerate() {
            if self.in_zone(i) {
                h[v as usize] += 1;
            }
        }
        h
    }

    /// Seuil automatique (méthode d'Otsu) sur la zone.
    pub fn otsu(&self) -> u8 {
        otsu(&self.histogram())
    }

    /// Masque binaire des pixels « objet » (dans la zone et du bon côté du seuil).
    pub fn binarize(&self, threshold: u8, bright: bool) -> Vec<bool> {
        self.luma
            .iter()
            .enumerate()
            .map(|(i, &v)| self.in_zone(i) && ((v > threshold) == bright))
            .collect()
    }
}

// ---------- composantes connexes ----------

/// Pixels de fond qu'on peut atteindre (en 4-connexité) depuis le bord ou
/// depuis l'extérieur de la zone ; les autres pixels de fond sont des trous.
fn outer_background(is_obj: impl Fn(usize) -> bool, w: usize, h: usize, zone: Option<&[bool]>) -> Vec<bool> {
    let mut seen = vec![false; w * h];
    let mut stack = Vec::new();
    let seed = |i: usize, seen: &mut [bool], stack: &mut Vec<usize>| {
        if !is_obj(i) && !seen[i] {
            seen[i] = true;
            stack.push(i);
        }
    };
    for x in 0..w {
        seed(x, &mut seen, &mut stack);
        seed((h - 1) * w + x, &mut seen, &mut stack);
    }
    for y in 0..h {
        seed(y * w, &mut seen, &mut stack);
        seed(y * w + w - 1, &mut seen, &mut stack);
    }
    if let Some(z) = zone {
        for (i, &inside) in z.iter().enumerate() {
            if !inside {
                seed(i, &mut seen, &mut stack);
            }
        }
    }
    while let Some(i) = stack.pop() {
        let (x, y) = (i % w, i / w);
        let mut push = |j: usize| {
            if !is_obj(j) && !seen[j] {
                seen[j] = true;
                stack.push(j);
            }
        };
        if x > 0 {
            push(i - 1);
        }
        if x + 1 < w {
            push(i + 1);
        }
        if y > 0 {
            push(i - w);
        }
        if y + 1 < h {
            push(i + w);
        }
    }
    seen
}

/// Bouche les trous : les pixels de fond qu'on ne peut atteindre (en
/// 4-connexité) ni depuis le bord, ni depuis l'extérieur de la zone,
/// deviennent objet.
pub fn fill_holes(mask: &mut [bool], w: usize, h: usize, zone: Option<&[bool]>) {
    let seen = outer_background(|i| mask[i], w, h, zone);
    for (m, s) in mask.iter_mut().zip(seen) {
        if !*m && !s {
            *m = true;
        }
    }
}

/// Trou : (étiquette de l'objet qui l'entoure, nombre de pixels, contour).
type Hole = (u32, u64, Vec<(i64, i64)>);

/// Trous des objets (composantes 4-connexes de fond fermées) : pour chacun,
/// l'étiquette de l'objet qui l'entoure, son nombre de pixels et son contour
/// (sommets de pixels, sens horaire, trou à droite). Un objet posé dans le
/// trou d'un autre fait partie du contour de ce trou.
fn hole_contours(labels: &[u32], w: usize, h: usize, zone: Option<&[bool]>) -> Vec<Hole> {
    let seen = outer_background(|i| labels[i] != 0, w, h, zone);
    let mut holes = vec![0u32; w * h];
    let mut out = Vec::new();
    let mut stack = Vec::new();
    for start in 0..w * h {
        if labels[start] != 0 || seen[start] || holes[start] != 0 {
            continue;
        }
        // Premier pixel du trou dans l'ordre de lecture : celui du dessus est
        // forcément objet (sinon il serait du même trou ou du fond extérieur).
        let id = out.len() as u32 + 1;
        let owner = labels[start - w];
        holes[start] = id;
        stack.push(start);
        let mut count = 0u64;
        while let Some(i) = stack.pop() {
            count += 1;
            let (x, y) = (i % w, i / w);
            for (ok, j) in [(x > 0, i.wrapping_sub(1)), (x + 1 < w, i + 1), (y > 0, i.wrapping_sub(w)), (y + 1 < h, i + w)] {
                if ok && labels[j] == 0 && !seen[j] && holes[j] == 0 {
                    holes[j] = id;
                    stack.push(j);
                }
            }
        }
        out.push((owner, count, (start % w, start / w)));
    }
    out.into_iter()
        .enumerate()
        .map(|(k, (owner, count, first))| (owner, count, trace_contour(&holes, w, h, k as u32 + 1, first)))
        .collect()
}

/// Raccorde un trou au polygone en « trou de serrure » : une fente aller-retour
/// relie le sommet du trou le plus proche d'un sommet du polygone. Le trou est
/// parcouru en sens inverse, si bien que l'aire du polygone (formule du lacet)
/// est celle de l'objet moins celle du trou ; la fente ne compte pas.
pub fn bridge_hole(poly: &mut Vec<Pt>, hole: &[Pt]) {
    if poly.is_empty() || hole.len() < 3 {
        return;
    }
    let (mut bi, mut bj, mut bd) = (0, 0, f64::INFINITY);
    for (i, p) in poly.iter().enumerate() {
        for (j, q) in hole.iter().enumerate() {
            let d = (p.x - q.x).powi(2) + (p.y - q.y).powi(2);
            if d < bd {
                (bi, bj, bd) = (i, j, d);
            }
        }
    }
    let n = hole.len();
    let ring = (0..=n).map(|k| hole[(bj + n - k % n) % n]);
    let tail: Vec<Pt> = poly.drain(bi..).collect();
    poly.push(tail[0]);
    poly.extend(ring);
    poly.extend(tail);
}

/// Statistiques d'une composante connexe.
#[derive(Clone, Debug, Default)]
struct Component {
    count: u64,
    sum_x: u64,
    sum_y: u64,
    /// Premier pixel dans l'ordre de lecture (départ du contour).
    first: (usize, usize),
    touches_border: bool,
}

fn find(parent: &mut [u32], mut a: u32) -> u32 {
    while parent[a as usize] != a {
        let p = parent[parent[a as usize] as usize];
        parent[a as usize] = p;
        a = p;
    }
    a
}

/// Étiquette les composantes 8-connexes de `mask` : étiquettes 1..=n par
/// pixel (0 = fond), numérotées dans l'ordre de lecture, et leurs statistiques
/// (`comps[l - 1]`). `zone` sert au contact avec le bord de la zone.
fn label(mask: &[bool], w: usize, h: usize, zone: Option<&[bool]>) -> (Vec<u32>, Vec<Component>) {
    let mut labels = vec![0u32; w * h];
    let mut parent: Vec<u32> = vec![0];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if !mask[i] {
                continue;
            }
            // Voisins déjà vus : ouest, nord-ouest, nord, nord-est.
            let mut best = 0u32;
            let mut neigh = [0u32; 4];
            if x > 0 {
                neigh[0] = labels[i - 1];
            }
            if y > 0 {
                let up = i - w;
                if x > 0 {
                    neigh[1] = labels[up - 1];
                }
                neigh[2] = labels[up];
                if x + 1 < w {
                    neigh[3] = labels[up + 1];
                }
            }
            for &n in &neigh {
                if n != 0 {
                    let r = find(&mut parent, n);
                    if best == 0 {
                        best = r;
                    } else if r != best {
                        let (lo, hi) = (best.min(r), best.max(r));
                        parent[hi as usize] = lo;
                        best = lo;
                    }
                }
            }
            if best == 0 {
                best = parent.len() as u32;
                parent.push(best);
            }
            labels[i] = best;
        }
    }
    // Racines numérotées 1..=n dans l'ordre de leur premier pixel.
    let mut remap = vec![0u32; parent.len()];
    let mut comps: Vec<Component> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if labels[i] == 0 {
                continue;
            }
            let r = find(&mut parent, labels[i]) as usize;
            if remap[r] == 0 {
                comps.push(Component { first: (x, y), ..Default::default() });
                remap[r] = comps.len() as u32;
            }
            let l = remap[r];
            labels[i] = l;
            let c = &mut comps[l as usize - 1];
            c.count += 1;
            c.sum_x += x as u64;
            c.sum_y += y as u64;
            if !c.touches_border {
                let outside = |j: usize| zone.is_some_and(|z| !z[j]);
                c.touches_border = x == 0
                    || y == 0
                    || x + 1 == w
                    || y + 1 == h
                    || outside(i - 1)
                    || outside(i + 1)
                    || outside(i - w)
                    || outside(i + w);
            }
        }
    }
    (labels, comps)
}

// ---------- contours ----------

/// Contour extérieur de la composante `lab` qui commence au pixel `start`
/// (son premier pixel dans l'ordre de lecture) : sommets (coins de pixels)
/// du bord, parcouru dans le sens horaire à l'écran, l'objet à droite. Aux
/// points-selles le contour suit la diagonale (8-connexité). L'aire du
/// polygone obtenu est exactement le nombre de pixels de la composante,
/// trous compris.
fn trace_contour(labels: &[u32], w: usize, h: usize, lab: u32, start: (usize, usize)) -> Vec<(i64, i64)> {
    const D: [(i64, i64); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)]; // E, S, O, N
    let at = |x: i64, y: i64| x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && labels[y as usize * w + x as usize] == lab;
    // Pixels devant à gauche et devant à droite en partant du sommet (x, y) dans la direction d.
    let ahead = |x: i64, y: i64, d: usize| match d {
        0 => ((x, y - 1), (x, y)),
        1 => ((x, y), (x - 1, y)),
        2 => ((x - 1, y), (x - 1, y - 1)),
        _ => ((x - 1, y - 1), (x, y - 1)),
    };
    let s = (start.0 as i64, start.1 as i64);
    let (mut v, mut d) = (s, 0usize);
    let mut pts = vec![s];
    let guard = 4 * (w + 1) * (h + 1);
    for _ in 0..guard {
        v = (v.0 + D[d].0, v.1 + D[d].1);
        let ((lx, ly), (rx, ry)) = ahead(v.0, v.1, d);
        let nd = if at(lx, ly) {
            (d + 3) % 4
        } else if at(rx, ry) {
            d
        } else {
            (d + 1) % 4
        };
        if v == s && nd == 0 {
            break;
        }
        if nd != d {
            pts.push(v);
        }
        d = nd;
    }
    pts
}

fn dist_to_segment(p: Pt, a: Pt, b: Pt) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let l2 = dx * dx + dy * dy;
    if l2 == 0.0 {
        return p.dist(a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / l2).clamp(0.0, 1.0);
    p.dist(Pt::new(a.x + t * dx, a.y + t * dy))
}

/// Douglas-Peucker sur une ligne ouverte : indices des points gardés.
fn dp_open(pts: &[Pt], tol: f64, keep: &mut [bool]) {
    let mut stack = vec![(0usize, pts.len() - 1)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (mut far, mut far_d) = (a, -1.0);
        for (i, p) in pts.iter().enumerate().take(b).skip(a + 1) {
            let d = dist_to_segment(*p, pts[a], pts[b]);
            if d > far_d {
                (far, far_d) = (i, d);
            }
        }
        if far_d > tol {
            keep[far] = true;
            stack.push((a, far));
            stack.push((far, b));
        }
    }
}

/// Simplifie un polygone fermé (Douglas-Peucker, tolérance `tol`). Garde au
/// moins 3 points : sinon le polygone est rendu tel quel.
pub fn simplify_closed(pts: &[Pt], tol: f64) -> Vec<Pt> {
    if pts.len() <= 3 {
        return pts.to_vec();
    }
    // Coupure entre le premier point et le plus éloigné de lui.
    let far = (1..pts.len()).max_by(|&i, &j| pts[0].dist(pts[i]).total_cmp(&pts[0].dist(pts[j]))).unwrap();
    let mut ring: Vec<Pt> = pts.to_vec();
    ring.push(pts[0]);
    let mut keep = vec![false; ring.len()];
    keep[0] = true;
    keep[far] = true;
    dp_open(&ring[..=far], tol, &mut keep[..=far]);
    dp_open(&ring[far..], tol, &mut keep[far..]);
    let out: Vec<Pt> = ring[..pts.len()].iter().zip(&keep).filter(|(_, k)| **k).map(|(p, _)| *p).collect();
    if out.len() >= 3 { out } else { pts.to_vec() }
}

// ---------- détection ----------

/// Étapes communes à la détection et à l'aperçu : étiquettes, composantes
/// et, pour chacune, si elle est retenue.
fn segment(p: &Prepared, params: &DetectParams) -> (Vec<u32>, Vec<Component>, Vec<bool>) {
    let (w, h) = (p.width, p.height);
    if w == 0 || h == 0 {
        return (vec![], vec![], vec![]);
    }
    let mut mask = p.binarize(params.threshold, params.bright);
    if params.fill_holes {
        fill_holes(&mut mask, w, h, p.zone.as_deref());
    }
    let (labels, comps) = label(&mask, w, h, p.zone.as_deref());
    let px = (p.scale as f64).powi(2);
    let keep = comps
        .iter()
        .map(|c| {
            let a = c.count as f64 * px;
            a >= params.min_area && a <= params.max_area && !(params.exclude_border && c.touches_border)
        })
        .collect();
    (labels, comps, keep)
}

/// Détecte les objets. `contours` : suivre et simplifier les contours
/// (sinon `Object::polygon` reste vide, pour compter vite).
pub fn detect(p: &Prepared, params: &DetectParams, contours: bool) -> Detection {
    if p.is_empty() {
        return Detection::default();
    }
    let (labels, comps, keep) = segment(p, params);
    let k = p.scale as f64;
    let (ox, oy) = (p.origin.0 as f64, p.origin.1 as f64);
    let to_img = |x: f64, y: f64| Pt::new(ox + x * k, oy + y * k);
    // Trous gardés : le polygone les contourne, pour que son aire soit celle
    // des pixels de l'objet (celle qui a servi au filtre).
    let holes = if contours && !params.fill_holes { hole_contours(&labels, p.width, p.height, p.zone.as_deref()) } else { vec![] };
    let mut by_owner = vec![vec![]; comps.len()];
    for (h, hole) in holes.iter().enumerate() {
        by_owner[hole.0 as usize - 1].push(h);
    }
    let mut out = Detection::default();
    for (i, c) in comps.iter().enumerate() {
        if !keep[i] {
            out.rejected += 1;
            continue;
        }
        let n = c.count as f64;
        let centroid = to_img(c.sum_x as f64 / n + 0.5, c.sum_y as f64 / n + 0.5);
        let polygon = if contours {
            let raw: Vec<Pt> = trace_contour(&labels, p.width, p.height, i as u32 + 1, c.first)
                .into_iter()
                .map(|(x, y)| to_img(x as f64, y as f64))
                .collect();
            let mut poly = simplify_closed(&raw, simplify_tolerance(n * k * k));
            for &h in &by_owner[i] {
                let (_, count, hole) = &holes[h];
                let hole: Vec<Pt> = hole.iter().map(|&(x, y)| to_img(x as f64, y as f64)).collect();
                bridge_hole(&mut poly, &simplify_closed(&hole, simplify_tolerance(*count as f64 * k * k)));
            }
            poly
        } else {
            vec![]
        };
        out.objects.push(Object { area: n * k * k, centroid, polygon });
    }
    out
}

/// Couleur de l'aperçu des objets retenus et écartés (RGBA non prémultiplié).
pub const PREVIEW_KEPT: [u8; 4] = [0, 210, 255, 130];
pub const PREVIEW_REJECTED: [u8; 4] = [255, 70, 50, 80];

/// Aperçu : image de la taille de `p` où les objets retenus sont colorés
/// (`PREVIEW_KEPT`), les écartés plus discrètement, le fond transparent.
/// Renvoie aussi le nombre d'objets retenus.
pub fn preview(p: &Prepared, params: &DetectParams) -> (RgbaImage, usize) {
    let (labels, _, keep) = segment(p, params);
    let mut img = RgbaImage::new(p.width as u32, p.height as u32);
    for (px, &l) in img.pixels_mut().zip(&labels) {
        if l != 0 {
            px.0 = if keep[l as usize - 1] { PREVIEW_KEPT } else { PREVIEW_REJECTED };
        }
    }
    (img, keep.iter().filter(|k| **k).count())
}

/// Lance la détection complète (pleine résolution) dans un fil dédié ; la
/// zone utile de l'image est copiée ici, le reste du calcul se fait dans le fil.
pub fn spawn_detection(img: &RgbaImage, zone: Option<Vec<Pt>>, smooth: bool, params: DetectParams) -> mpsc::Receiver<Detection> {
    let (x0, y0, w, h) = zone_bounds(img.width(), img.height(), zone.as_deref());
    let sub = image::imageops::crop_imm(img, x0, y0, w, h).to_image();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let zone = zone.map(|z| z.iter().map(|p| p.offset(-(x0 as f64), -(y0 as f64))).collect::<Vec<_>>());
        let mut prep = prepare(&sub, zone.as_deref(), u32::MAX, smooth);
        prep.origin = (x0 + prep.origin.0, y0 + prep.origin.1);
        let _ = tx.send(detect(&prep, &params, true));
    });
    rx
}

#[cfg(test)]
#[path = "detect_tests.rs"]
mod tests;

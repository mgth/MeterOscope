//! Redressement de l'image par rééchantillonnage : la photo pivotée est
//! reconstruite par un vrai filtre d'interpolation, au lieu de laisser la carte
//! graphique recopier les pixels d'origine (dont l'escalier d'antialiasing se
//! retrouve alors couché sur les bords devenus horizontaux).
//!
//! Les calculs se font en lumière linéaire, couleurs prémultipliées par
//! l'alpha : interpoler directement les valeurs sRGB assombrit les pixels
//! intermédiaires et laisse les bords irréguliers. Chaque valeur interpolée
//! est bornée par ses voisins immédiats (pas de halo ni de tirets noirs sur
//! les bords francs).
//!
//! Géométrie : celle de l'affichage (`Xf` du canevas). L'image pivote autour
//! de son centre, angle positif = sens horaire, et se place dans sa boîte
//! englobante dont le coin haut-gauche est l'origine.

use image::{RgbaImage, imageops};
use std::borrow::Cow;
use std::sync::OnceLock;

/// Filtre de reconstruction utilisé pour redresser l'image.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RotationFilter {
    /// Bicubique de Catmull-Rom (4 × 4 pixels).
    Bicubic,
    /// Sinus cardinal fenêtré de Lanczos, a = 3 (6 × 6 pixels).
    #[default]
    Lanczos3,
    /// B-spline cubique avec préfiltre (Unser) : interpolante, 4 × 4 pixels.
    BSpline,
    /// Trois cisaillements (Paeth), chacun interpolé en 1D par un sinus
    /// cardinal fenêtré large (16 pixels).
    Shear,
}

impl RotationFilter {
    pub const ALL: [Self; 4] = [Self::Bicubic, Self::Lanczos3, Self::BSpline, Self::Shear];

    /// Identifiant stable (préférences).
    pub fn id(self) -> &'static str {
        match self {
            Self::Bicubic => "bicubic",
            Self::Lanczos3 => "lanczos3",
            Self::BSpline => "bspline",
            Self::Shear => "shear",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.id() == id)
    }
}

/// Pixel de travail : RVB linéaire prémultiplié + alpha, dans [0, 1].
type Px = [f32; 4];

// ---------- sRGB ↔ linéaire ----------

struct Tables {
    to_linear: [f32; 256],
    /// `thresholds[v]` : valeur linéaire à partir de laquelle on arrondit à `v + 1`.
    thresholds: [f32; 255],
    /// Première estimation du code sRGB pour une valeur linéaire (pas de 1/4096).
    guess: Vec<u8>,
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn linear_to_srgb(l: f64) -> f64 {
    if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 }
}

fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| Tables {
        to_linear: std::array::from_fn(|v| srgb_to_linear(v as f64 / 255.0) as f32),
        thresholds: std::array::from_fn(|v| srgb_to_linear((v as f64 + 0.5) / 255.0) as f32),
        guess: (0..=4096).map(|i| (linear_to_srgb(i as f64 / 4096.0) * 255.0).round() as u8).collect(),
    })
}

impl Tables {
    /// Code sRGB le plus proche (exact : l'aller-retour d'un octet le redonne).
    fn encode(&self, x: f32) -> u8 {
        let x = if x > 0.0 { x.min(1.0) } else { 0.0 };
        let mut v = self.guess[(x * 4096.0) as usize] as usize;
        while v < 255 && x >= self.thresholds[v] {
            v += 1;
        }
        while v > 0 && x < self.thresholds[v - 1] {
            v -= 1;
        }
        v as u8
    }

    /// Écrit le pixel de travail `p` en RGBA 8 bits (alpha droit), atténué par
    /// la couverture `cov` du pixel par l'image.
    fn store(&self, p: Px, cov: f32, out: &mut [u8]) {
        let a = p[3];
        if a <= 1e-5 || cov <= 0.0 {
            out.fill(0);
            return;
        }
        for c in 0..3 {
            out[c] = self.encode(p[c] / a);
        }
        out[3] = (a.min(1.0) * cov * 255.0 + 0.5) as u8;
    }
}

fn to_working(image: &RgbaImage) -> Vec<Px> {
    let t = tables();
    image
        .as_raw()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let a = p[3] as f32 / 255.0;
            [t.to_linear[p[0] as usize] * a, t.to_linear[p[1] as usize] * a, t.to_linear[p[2] as usize] * a, a]
        })
        .collect()
}

// ---------- parallélisme ----------

/// Applique `f(numéro, ligne)` à chaque tranche de `line` éléments de `buf`,
/// sur quelques fils pour les grandes images.
fn par_lines<T: Send>(buf: &mut [T], line: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    if line == 0 || buf.is_empty() {
        return;
    }
    let n = buf.len() / line;
    let threads = std::thread::available_parallelism().map_or(1, |t| t.get()).min(8).min(n);
    if threads <= 1 || buf.len() < 1 << 16 {
        for (j, l) in buf.chunks_mut(line).enumerate() {
            f(j, l);
        }
        return;
    }
    let per = n.div_ceil(threads);
    std::thread::scope(|s| {
        for (c, chunk) in buf.chunks_mut(per * line).enumerate() {
            let f = &f;
            s.spawn(move || {
                for (j, l) in chunk.chunks_mut(line).enumerate() {
                    f(c * per + j, l);
                }
            });
        }
    });
}

// ---------- géométrie ----------

/// Rotation de `theta` (|theta| ≤ 45°) d'une image `w` × `h` vers sa boîte
/// englobante `ow` × `oh`.
struct Geo {
    w: usize,
    h: usize,
    theta: f64,
    cos: f64,
    sin: f64,
    /// Taille exacte (non arrondie) de la boîte englobante.
    vx: f64,
    vy: f64,
    ow: usize,
    oh: usize,
}

impl Geo {
    fn new(w: usize, h: usize, deg: f64) -> Self {
        let theta = deg.to_radians();
        let (sin, cos) = theta.sin_cos();
        let (wf, hf) = (w as f64, h as f64);
        let vx = cos.abs() * wf + sin.abs() * hf;
        let vy = sin.abs() * wf + cos.abs() * hf;
        // Tolérance : une boîte entière à l'arrondi près ne gagne pas un pixel.
        let up = |v: f64| ((v - 1e-6).ceil() as usize).max(1);
        Self { w, h, theta, cos, sin, vx, vy, ow: up(vx), oh: up(vy) }
    }

    /// Point de l'image d'origine (coordonnées continues) vu au centre du
    /// pixel (`i`, `j`) de l'image pivotée.
    fn source(&self, i: usize, j: usize) -> (f64, f64) {
        let dx = i as f64 + 0.5 - self.vx / 2.0;
        let dy = j as f64 + 0.5 - self.vy / 2.0;
        (
            self.cos * dx + self.sin * dy + self.w as f64 / 2.0,
            -self.sin * dx + self.cos * dy + self.h as f64 / 2.0,
        )
    }

    /// Part du pixel de sortie couverte par l'image (bords adoucis sur un pixel).
    fn coverage(&self, x: f64, y: f64) -> f32 {
        let edge = |v: f64, n: usize| (v + 0.5).clamp(0.0, 1.0) * (n as f64 - v + 0.5).clamp(0.0, 1.0);
        (edge(x, self.w) * edge(y, self.h)) as f32
    }
}

// ---------- noyaux ----------

fn lanczos(x: f64, a: f64) -> f64 {
    if x.abs() < 1e-9 {
        return 1.0;
    }
    if x.abs() >= a {
        return 0.0;
    }
    let px = std::f64::consts::PI * x;
    a * px.sin() * (px / a).sin() / (px * px)
}

/// Poids de Catmull-Rom pour la phase `f` ∈ [0, 1[ (pixels -1, 0, 1, 2).
fn catmull_rom(f: f32) -> [f32; 4] {
    [
        ((-f + 2.0) * f - 1.0) * f * 0.5,
        ((3.0 * f - 5.0) * f * f + 2.0) * 0.5,
        ((-3.0 * f + 4.0) * f + 1.0) * f * 0.5,
        (f - 1.0) * f * f * 0.5,
    ]
}

/// Poids de la B-spline cubique pour la phase `f` (pixels -1, 0, 1, 2).
fn bspline(f: f32) -> [f32; 4] {
    let g = 1.0 - f;
    [
        g * g * g / 6.0,
        ((3.0 * f - 6.0) * f * f + 4.0) / 6.0,
        ((-3.0 * f + 3.0) * f * f + 3.0 * f + 1.0) / 6.0,
        f * f * f / 6.0,
    ]
}

/// Nombre de phases sous-pixel de la table de Lanczos.
const PHASES: usize = 4096;

/// Poids de Lanczos-3 normalisés, par phase (pixels -2 … 3).
fn lanczos3_table() -> Vec<[f32; 6]> {
    (0..=PHASES)
        .map(|i| {
            let f = i as f64 / PHASES as f64;
            let w: [f64; 6] = std::array::from_fn(|t| lanczos(f - (t as f64 - 2.0), 3.0));
            let sum: f64 = w.iter().sum();
            w.map(|v| (v / sum) as f32)
        })
        .collect()
}

/// Indice replié en miroir sur [0, n[ (sans répéter le bord).
fn mirror(i: isize, n: usize) -> usize {
    if n == 1 {
        return 0;
    }
    let period = 2 * (n as isize - 1);
    let m = i.rem_euclid(period);
    (if m >= n as isize { period - m } else { m }) as usize
}

// ---------- filtres séparables N × N ----------

/// Rééchantillonne `src` selon `g` avec un noyau séparable de `N` pixels dont
/// `weights(phase)` donne les poids (premier pixel : `N/2 - 1` avant le pixel
/// contenant le point).
///
/// Le résultat est borné par les quatre échantillons de `bounds` (l'image
/// avant préfiltre) qui entourent le point : les noyaux à lobes négatifs
/// dépassent sur les bords francs, et ce dépassement, très visible côté
/// sombre en lumière linéaire, laisserait des tirets noirs le long des bords.
fn resample<const N: usize>(src: &[Px], bounds: &[Px], g: &Geo, weights: impl Fn(f32) -> [f32; N] + Sync) -> RgbaImage {
    let t = tables();
    let (w, h) = (g.w as isize, g.h as isize);
    let before = N as isize / 2 - 1;
    let mut out = vec![0u8; g.ow * g.oh * 4];
    par_lines(&mut out, g.ow * 4, |j, row| {
        for (i, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let (x, y) = g.source(i, j);
            let cov = g.coverage(x, y);
            if cov <= 0.0 {
                continue;
            }
            // Indices continus : le pixel k a son centre en k + 0,5.
            let (u, v) = (x - 0.5, y - 0.5);
            let (fu, fv) = (u.floor(), v.floor());
            let wx = weights((u - fu) as f32);
            let wy = weights((v - fv) as f32);
            let (x0, y0) = (fu as isize - before, fv as isize - before);
            let mut acc = [0f32; 4];
            let inside = x0 >= 0 && y0 >= 0 && x0 + N as isize <= w && y0 + N as isize <= h;
            for (ky, wyv) in wy.iter().enumerate() {
                let mut line = [0f32; 4];
                if inside {
                    let start = ((y0 + ky as isize) * w + x0) as usize;
                    for (p, wxv) in src[start..start + N].iter().zip(&wx) {
                        for c in 0..4 {
                            line[c] += p[c] * wxv;
                        }
                    }
                } else {
                    let base = mirror(y0 + ky as isize, g.h) * g.w;
                    for (kx, wxv) in wx.iter().enumerate() {
                        let p = src[base + mirror(x0 + kx as isize, g.w)];
                        for c in 0..4 {
                            line[c] += p[c] * wxv;
                        }
                    }
                }
                for c in 0..4 {
                    acc[c] += line[c] * wyv;
                }
            }
            let (bx, by) = (fu as isize, fv as isize);
            let near = if bx >= 0 && by >= 0 && bx + 1 < w && by + 1 < h {
                let k = (by * w + bx) as usize;
                [bounds[k], bounds[k + 1], bounds[k + g.w], bounds[k + g.w + 1]]
            } else {
                [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dy)| bounds[mirror(by + dy, g.h) * g.w + mirror(bx + dx, g.w)])
            };
            for c in 0..4 {
                let lo = near[0][c].min(near[1][c]).min(near[2][c]).min(near[3][c]);
                let hi = near[0][c].max(near[1][c]).max(near[2][c]).max(near[3][c]);
                acc[c] = acc[c].clamp(lo, hi);
            }
            t.store(acc, cov, px);
        }
    });
    RgbaImage::from_raw(g.ow as u32, g.oh as u32, out).expect("taille cohérente")
}

// ---------- préfiltre B-spline ----------

/// Pôle du filtre récursif de la B-spline cubique : √3 − 2.
const POLE: f32 = -0.267_949_2;
/// Nombre de termes pour l'amorce (|pôle|³² ≈ 5·10⁻¹⁹).
const HORIZON: usize = 32;

/// Remplace les échantillons par les coefficients B-spline cubiques qui les
/// interpolent (Unser, bords en miroir), ligne par ligne puis colonne par colonne.
fn bspline_prefilter(buf: &mut [Px], w: usize, h: usize) {
    let last = POLE / (POLE * POLE - 1.0);
    if w > 1 {
        par_lines(buf, w, |_, line| {
            for c in 0..4 {
                let mut zn = POLE;
                let mut sum = line[0][c];
                for p in &line[1..w.min(HORIZON)] {
                    sum += zn * p[c];
                    zn *= POLE;
                }
                line[0][c] = sum * 6.0;
                for k in 1..w {
                    line[k][c] = line[k][c] * 6.0 + POLE * line[k - 1][c];
                }
                line[w - 1][c] = last * (POLE * line[w - 2][c] + line[w - 1][c]);
                for k in (0..w - 1).rev() {
                    line[k][c] = POLE * (line[k + 1][c] - line[k][c]);
                }
            }
        });
    }
    if h > 1 {
        // Même récurrence sur les colonnes, menée ligne entière par ligne
        // entière (accès mémoire séquentiels).
        let mut zn = POLE;
        for k in 1..h.min(HORIZON) {
            let (first, rest) = buf.split_at_mut(w);
            for (a, b) in first.iter_mut().zip(&rest[(k - 1) * w..k * w]) {
                for c in 0..4 {
                    a[c] += zn * b[c];
                }
            }
            zn *= POLE;
        }
        for a in &mut buf[..w] {
            for v in a {
                *v *= 6.0;
            }
        }
        for y in 1..h {
            let (prev, cur) = buf.split_at_mut(y * w);
            for (a, b) in cur[..w].iter_mut().zip(&prev[(y - 1) * w..]) {
                for c in 0..4 {
                    a[c] = a[c] * 6.0 + POLE * b[c];
                }
            }
        }
        let (prev, cur) = buf.split_at_mut((h - 1) * w);
        for (a, b) in cur.iter_mut().zip(&prev[(h - 2) * w..]) {
            for c in 0..4 {
                a[c] = last * (POLE * b[c] + a[c]);
            }
        }
        for y in (0..h - 1).rev() {
            let (cur, next) = buf.split_at_mut((y + 1) * w);
            for (a, b) in cur[y * w..].iter_mut().zip(&next[..w]) {
                for c in 0..4 {
                    a[c] = POLE * (b[c] - a[c]);
                }
            }
        }
    }
}

// ---------- trois cisaillements ----------

/// Demi-largeur du sinus cardinal fenêtré des cisaillements.
const SHEAR_A: usize = 8;
const SHEAR_TAPS: usize = 2 * SHEAR_A;

/// Translation sous-pixel d'une ligne : `out[i]` = `src` interpolée en
/// `i + shift` (bords prolongés).
fn shift_line(src: &[Px], shift: f64, out: &mut [Px]) {
    let n = src.len() as isize;
    let base = shift.floor();
    let f = shift - base;
    let mut w = [0f32; SHEAR_TAPS];
    let mut sum = 0.0;
    for (t, v) in w.iter_mut().enumerate() {
        let k = lanczos(f - (t as f64 - (SHEAR_A as f64 - 1.0)), SHEAR_A as f64);
        *v = k as f32;
        sum += k;
    }
    for v in &mut w {
        *v /= sum as f32;
    }
    let first = base as isize - (SHEAR_A as isize - 1);
    for (i, o) in out.iter_mut().enumerate() {
        let b = i as isize + first;
        let mut acc = [0f32; 4];
        if b >= 0 && b + SHEAR_TAPS as isize <= n {
            for (p, wv) in src[b as usize..b as usize + SHEAR_TAPS].iter().zip(&w) {
                for c in 0..4 {
                    acc[c] += p[c] * wv;
                }
            }
        } else {
            for (t, wv) in w.iter().enumerate() {
                let p = src[(b + t as isize).clamp(0, n - 1) as usize];
                for c in 0..4 {
                    acc[c] += p[c] * wv;
                }
            }
        }
        // Même garde-fou que `resample` : borné par les deux voisins.
        let p0 = src[(b + SHEAR_A as isize - 1).clamp(0, n - 1) as usize];
        let p1 = src[(b + SHEAR_A as isize).clamp(0, n - 1) as usize];
        for c in 0..4 {
            acc[c] = acc[c].clamp(p0[c].min(p1[c]), p0[c].max(p1[c]));
        }
        *o = acc;
    }
}

/// Rotation de Paeth : R(θ) = Cx(a) · Cy(b) · Cx(a), avec a = −tan(θ/2) et
/// b = sin θ. Chaque cisaillement ne fait que translater des lignes (ou des
/// colonnes) d'une fraction de pixel : trois interpolations 1D.
fn rotate_shear(src: Vec<Px>, g: &Geo) -> RgbaImage {
    let t = tables();
    let (w, h) = (g.w, g.h);
    let (a, b) = (-(g.theta / 2.0).tan(), g.sin);
    let (wf, hf) = (w as f64, h as f64);

    // 1. Cisaillement horizontal : image w1 × h, centrée.
    let w1 = (wf + a.abs() * hf).ceil() as usize;
    let w1f = w1 as f64;
    let mut s1 = vec![[0f32; 4]; w1 * h];
    par_lines(&mut s1, w1, |j, out| {
        let yc = j as f64 + 0.5 - hf / 2.0;
        shift_line(&src[j * w..(j + 1) * w], (wf - w1f) / 2.0 - a * yc, out);
    });
    drop(src);

    // 2. Cisaillement vertical : colonnes de hauteur finale, rangées par
    // colonne (chaque fil écrit des tranches contiguës).
    let mut s2 = vec![[0f32; 4]; w1 * g.oh];
    par_lines(&mut s2, g.oh, |i, out| {
        let xc = i as f64 + 0.5 - w1f / 2.0;
        let column: Vec<Px> = (0..h).map(|y| s1[y * w1 + i]).collect();
        shift_line(&column, (hf - g.vy) / 2.0 - b * xc, out);
    });
    drop(s1);

    // 3. Cisaillement horizontal vers l'image finale.
    let mut out = vec![0u8; g.ow * g.oh * 4];
    par_lines(&mut out, g.ow * 4, |j, row| {
        let yc = j as f64 + 0.5 - g.vy / 2.0;
        let line: Vec<Px> = (0..w1).map(|i| s2[i * g.oh + j]).collect();
        let mut shifted = vec![[0f32; 4]; g.ow];
        shift_line(&line, (w1f - g.vx) / 2.0 - a * yc, &mut shifted);
        for (i, (px, p)) in row.as_chunks_mut::<4>().0.iter_mut().zip(shifted).enumerate() {
            let (x, y) = g.source(i, j);
            t.store(p, g.coverage(x, y), px);
        }
    });
    RgbaImage::from_raw(g.ow as u32, g.oh as u32, out).expect("taille cohérente")
}

// ---------- entrée ----------

/// Taille de l'image renvoyée par [`rotate_image`] pour une image `w` × `h`.
pub fn rotated_dimensions(w: u32, h: u32, deg: f64) -> (u32, u32) {
    let (quarters, rest) = split_angle(deg);
    let (w, h) = if quarters % 2 == 1 { (h, w) } else { (w, h) };
    if rest == 0.0 || w == 0 || h == 0 {
        return (w, h);
    }
    let g = Geo::new(w as usize, h as usize, rest);
    (g.ow as u32, g.oh as u32)
}

/// Décompose un angle en quarts de tour (0 à 3) et un reste dans [-45°, 45°].
fn split_angle(deg: f64) -> (u8, f64) {
    if !deg.is_finite() {
        return (0, 0.0);
    }
    let k = (deg / 90.0).round();
    (k.rem_euclid(4.0) as u8, deg - k * 90.0)
}

/// Image pivotée de `deg` degrés (positif = sens horaire) autour de son
/// centre, dans sa boîte englobante ; hors de l'image, les pixels sont
/// transparents. Les quarts de tour sont exacts, seul le reste est interpolé.
pub fn rotate_image(image: &RgbaImage, deg: f64, filter: RotationFilter) -> RgbaImage {
    let (quarters, rest) = split_angle(deg);
    let turned: Cow<RgbaImage> = match quarters {
        1 => Cow::Owned(imageops::rotate90(image)),
        2 => Cow::Owned(imageops::rotate180(image)),
        3 => Cow::Owned(imageops::rotate270(image)),
        _ => Cow::Borrowed(image),
    };
    if rest == 0.0 || turned.width() == 0 || turned.height() == 0 {
        return turned.into_owned();
    }
    let g = Geo::new(turned.width() as usize, turned.height() as usize, rest);
    let mut src = to_working(&turned);
    drop(turned);
    match filter {
        RotationFilter::Bicubic => resample(&src, &src, &g, catmull_rom),
        RotationFilter::Lanczos3 => {
            let table = lanczos3_table();
            resample(&src, &src, &g, |f| table[(f * PHASES as f32 + 0.5) as usize])
        }
        RotationFilter::BSpline => {
            // Les bornes viennent des échantillons, pas des coefficients.
            let orig = src.clone();
            bspline_prefilter(&mut src, g.w, g.h);
            resample(&src, &orig, &g, bspline)
        }
        RotationFilter::Shear => rotate_shear(src, &g),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn photo(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| Rgba([(x * 7 + y) as u8, (y * 13) as u8, ((x * y) % 251) as u8, 255]))
    }

    #[test]
    fn srgb_round_trip_is_exact() {
        let t = tables();
        for v in 0..=255u8 {
            assert_eq!(t.encode(t.to_linear[v as usize]), v);
        }
        assert_eq!((t.encode(-1.0), t.encode(2.0), t.encode(f32::NAN)), (0, 255, 0));
    }

    #[test]
    fn quarter_turns_are_exact() {
        let img = photo(23, 11);
        for f in RotationFilter::ALL {
            assert_eq!(rotate_image(&img, 0.0, f), img);
            assert_eq!(rotate_image(&img, 90.0, f), imageops::rotate90(&img));
            assert_eq!(rotate_image(&img, -90.0, f), imageops::rotate270(&img));
            assert_eq!(rotate_image(&img, 180.0, f), imageops::rotate180(&img));
        }
        assert_eq!(rotated_dimensions(200, 100, 90.0), (100, 200));
        assert_eq!(rotated_dimensions(200, 100, 0.0), (200, 100));
    }

    #[test]
    fn dimensions_match_bounding_box() {
        let img = photo(200, 100);
        for deg in [3.0, -12.5, 44.0, 97.0, -170.0] {
            let (s, c) = f64::to_radians(deg).sin_cos();
            let (bw, bh) = (c.abs() * 200.0 + s.abs() * 100.0, s.abs() * 200.0 + c.abs() * 100.0);
            for f in RotationFilter::ALL {
                let out = rotate_image(&img, deg, f);
                assert_eq!(out.dimensions(), rotated_dimensions(200, 100, deg));
                assert!((out.width() as f64 - bw).abs() <= 1.0 && (out.height() as f64 - bh).abs() <= 1.0, "{deg} {f:?}");
            }
        }
    }

    #[test]
    fn uniform_image_stays_uniform() {
        let img = RgbaImage::from_pixel(60, 40, Rgba([200, 90, 30, 255]));
        for f in RotationFilter::ALL {
            let out = rotate_image(&img, 17.0, f);
            // Hors de l'image : transparent ; dedans : couleur intacte.
            assert_eq!(out.get_pixel(0, 0)[3], 0, "{f:?}");
            let mut opaque = 0;
            for p in out.pixels() {
                if p[3] > 0 {
                    assert_eq!(&p.0[..3], &[200, 90, 30], "{f:?}");
                }
                opaque += (p[3] == 255) as u32;
            }
            assert!((opaque as i32 - 2400).abs() < 250, "{f:?} : {opaque}");
        }
    }

    /// Tache gaussienne centrée en (`cx`, `cy`).
    fn blob(w: u32, h: u32, cx: f64, cy: f64) -> RgbaImage {
        let t = tables();
        RgbaImage::from_fn(w, h, |x, y| {
            let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
            let v = t.encode((-(dx * dx + dy * dy) / (2.0 * 2.5 * 2.5)).exp() as f32);
            Rgba([v, v, v, 255])
        })
    }

    #[test]
    fn points_land_where_the_view_puts_them() {
        // Même formule que `Xf::rotated` du canevas : q = R·(p − c) + boîte/2.
        let (w, h) = (90u32, 60u32);
        let (px, py) = (61.3, 17.8);
        let img = blob(w, h, px, py);
        let t = tables();
        for deg in [7.5f64, -31.0, 40.0, 100.0, -160.0] {
            let (s, c) = deg.to_radians().sin_cos();
            let (bw, bh) = (c.abs() * 90.0 + s.abs() * 60.0, s.abs() * 90.0 + c.abs() * 60.0);
            let (dx, dy) = (px - 45.0, py - 30.0);
            let (ex, ey) = (c * dx - s * dy + bw / 2.0, s * dx + c * dy + bh / 2.0);
            for f in RotationFilter::ALL {
                let out = rotate_image(&img, deg, f);
                let (mut sx, mut sy, mut sm) = (0.0, 0.0, 0.0);
                for (x, y, p) in out.enumerate_pixels() {
                    let m = t.to_linear[p[0] as usize] as f64 * p[3] as f64;
                    sx += m * (x as f64 + 0.5);
                    sy += m * (y as f64 + 0.5);
                    sm += m;
                }
                let (gx, gy) = (sx / sm, sy / sm);
                assert!((gx - ex).abs() < 0.05 && (gy - ey).abs() < 0.05, "{deg}° {f:?} : ({gx:.3}, {gy:.3}) ≠ ({ex:.3}, {ey:.3})");
            }
        }
    }

    /// Bord antialiasé (flou gaussien d'écart-type `sigma`) penché de `deg`
    /// degrés, passant par le centre : clair au-dessus, sombre en dessous.
    fn tilted_edge(w: u32, h: u32, deg: f64, sigma: f64) -> RgbaImage {
        let t = tables();
        let (s, c) = deg.to_radians().sin_cos();
        RgbaImage::from_fn(w, h, |x, y| {
            let (dx, dy) = (x as f64 + 0.5 - w as f64 / 2.0, y as f64 + 0.5 - h as f64 / 2.0);
            // Distance signée au bord.
            let d = (-s * dx + c * dy) / sigma;
            let v = t.encode((0.05 + 0.9 / (1.0 + (1.7 * d).exp())) as f32);
            Rgba([v, v, v, 255])
        })
    }

    /// Écart maximal, le long du bord redressé, entre les lignes de l'image et
    /// leur moyenne : mesure l'ondulation laissée par le filtre.
    fn ripple(out: &RgbaImage) -> f64 {
        let (w, h) = out.dimensions();
        let cols = w / 4..w * 3 / 4;
        let mut worst = 0.0f64;
        for y in h / 2 - 3..h / 2 + 3 {
            let vals: Vec<f64> = cols.clone().map(|x| out.get_pixel(x, y)[0] as f64).collect();
            let mean = vals.iter().sum::<f64>() / vals.len() as f64;
            worst = vals.iter().fold(worst, |m, v| m.max((v - mean).abs()));
        }
        worst
    }

    #[test]
    fn straightened_edge_has_no_staircase() {
        let deg = 4.0;
        let img = tilted_edge(240, 120, deg, 0.8);
        // Référence : le bord incliné lui-même varie de toute l'amplitude.
        assert!(ripple(&img) > 60.0);
        for (f, max) in [
            (RotationFilter::Bicubic, 5.0),
            (RotationFilter::Lanczos3, 4.5),
            (RotationFilter::BSpline, 3.5),
            (RotationFilter::Shear, 3.5),
        ] {
            let r = ripple(&rotate_image(&img, -deg, f));
            assert!(r <= max, "{f:?} : ondulation de {r:.2} niveaux");
        }
    }

    #[test]
    fn hard_edges_get_no_halo() {
        // Bord franc penché, sans antialiasing : le pire cas pour le dépassement.
        let img = RgbaImage::from_fn(120, 80, |x, y| {
            let v = if (y as f64) < 40.0 + (x as f64 - 60.0) * 0.07 { 240 } else { 60 };
            Rgba([v, v, v, 255])
        });
        for f in RotationFilter::ALL {
            for p in rotate_image(&img, -4.0, f).pixels() {
                if p[3] > 0 {
                    assert!((60..=240).contains(&p[0]), "{f:?} : {p:?}");
                }
            }
        }
    }

    #[test]
    fn transparency_does_not_bleed() {
        // Moitié gauche transparente (rouge vif caché), moitié droite verte opaque.
        let img = RgbaImage::from_fn(40, 40, |x, _| if x < 20 { Rgba([255, 0, 0, 0]) } else { Rgba([0, 200, 0, 255]) });
        for f in RotationFilter::ALL {
            for p in rotate_image(&img, 10.0, f).pixels() {
                if p[3] > 40 {
                    assert!(p[0] < 8 && (p[1] as i32 - 200).abs() < 40, "{f:?} : {p:?}");
                }
            }
        }
    }

    #[test]
    fn tiny_images_do_not_panic() {
        for (w, h) in [(1u32, 1u32), (1, 7), (5, 1), (2, 2), (3, 20)] {
            let img = photo(w, h);
            for f in RotationFilter::ALL {
                let out = rotate_image(&img, 33.0, f);
                assert_eq!(out.dimensions(), rotated_dimensions(w, h, 33.0));
            }
        }
        let empty = RgbaImage::new(0, 0);
        assert_eq!(rotate_image(&empty, 12.0, RotationFilter::Shear).dimensions(), (0, 0));
    }
}

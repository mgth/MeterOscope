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
use meteroscope_core::{Pt, Shape};
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

/// Correspondance entre les pixels d'une image reconstruite et l'image
/// d'origine (rotation, correction de perspective).
pub(crate) trait Mapping: Sync {
    /// Taille de l'image d'origine.
    fn src(&self) -> (usize, usize);
    /// Taille de l'image reconstruite.
    fn out(&self) -> (usize, usize);
    /// Point de l'image d'origine (coordonnées continues) vu au centre du
    /// pixel (`i`, `j`) de l'image reconstruite.
    fn source(&self, i: usize, j: usize) -> (f64, f64);

    /// Part du pixel de sortie couverte par l'image (bords adoucis sur un pixel).
    fn coverage(&self, x: f64, y: f64) -> f32 {
        let (w, h) = self.src();
        let edge = |v: f64, n: usize| (v + 0.5).clamp(0.0, 1.0) * (n as f64 - v + 0.5).clamp(0.0, 1.0);
        (edge(x, w) * edge(y, h)) as f32
    }
}

impl Mapping for Geo {
    fn src(&self) -> (usize, usize) {
        (self.w, self.h)
    }
    fn out(&self) -> (usize, usize) {
        (self.ow, self.oh)
    }
    fn source(&self, i: usize, j: usize) -> (f64, f64) {
        Geo::source(self, i, j)
    }
    fn coverage(&self, x: f64, y: f64) -> f32 {
        Geo::coverage(self, x, y)
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
fn resample<const N: usize>(src: &[Px], bounds: &[Px], g: &impl Mapping, weights: impl Fn(f32) -> [f32; N] + Sync) -> RgbaImage {
    let t = tables();
    let ((gw, gh), (ow, oh)) = (g.src(), g.out());
    let (w, h) = (gw as isize, gh as isize);
    let before = N as isize / 2 - 1;
    let mut out = vec![0u8; ow * oh * 4];
    par_lines(&mut out, ow * 4, |j, row| {
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
                    let base = mirror(y0 + ky as isize, gh) * gw;
                    for (kx, wxv) in wx.iter().enumerate() {
                        let p = src[base + mirror(x0 + kx as isize, gw)];
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
                [bounds[k], bounds[k + 1], bounds[k + gw], bounds[k + gw + 1]]
            } else {
                [(0, 0), (1, 0), (0, 1), (1, 1)].map(|(dx, dy)| bounds[mirror(by + dy, gh) * gw + mirror(bx + dx, gw)])
            };
            for c in 0..4 {
                let lo = near[0][c].min(near[1][c]).min(near[2][c]).min(near[3][c]);
                let hi = near[0][c].max(near[1][c]).max(near[2][c]).max(near[3][c]);
                acc[c] = acc[c].clamp(lo, hi);
            }
            t.store(acc, cov, px);
        }
    });
    RgbaImage::from_raw(ow as u32, oh as u32, out).expect("taille cohérente")
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
                if w < HORIZON {
                    // Ligne courte : les termes réfléchis par le bord opposé
                    // comptent encore. Somme exacte sur une période du signal
                    // en miroir (2w − 2 échantillons), puis série géométrique.
                    for p in line[1..w - 1].iter().rev() {
                        sum += zn * p[c];
                        zn *= POLE;
                    }
                    sum /= 1.0 - zn;
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
        // Colonne courte : même amorce exacte que pour les lignes.
        let mut gain = 6.0;
        if h < HORIZON {
            for k in (1..h - 1).rev() {
                let (first, rest) = buf.split_at_mut(w);
                for (a, b) in first.iter_mut().zip(&rest[(k - 1) * w..k * w]) {
                    for c in 0..4 {
                        a[c] += zn * b[c];
                    }
                }
                zn *= POLE;
            }
            gain /= 1.0 - zn;
        }
        for a in &mut buf[..w] {
            for v in a {
                *v *= gain;
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

/// Largeur de l'image après le premier cisaillement.
fn shear_width(g: &Geo) -> usize {
    (g.w as f64 + (g.theta / 2.0).tan().abs() * g.h as f64).ceil() as usize
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
    let w1 = shear_width(g);
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

/// Octets de travail au plus fort de [`rotate_image`] pour une image `w` × `h`
/// (tampons en lumière linéaire, images intermédiaires et image produite),
/// sans rien allouer. Les trois cisaillements passent par une image
/// intermédiaire bien plus grande que la source sur un panorama très penché.
pub fn rotation_work_bytes(w: u32, h: u32, deg: f64, filter: RotationFilter) -> u64 {
    const PX: u64 = size_of::<Px>() as u64;
    let (quarters, rest) = split_angle(deg);
    let (w, h) = if quarters % 2 == 1 { (h, w) } else { (w, h) };
    let pixels = w as u64 * h as u64;
    let turned = if quarters != 0 { 4 * pixels } else { 0 };
    if rest == 0.0 || pixels == 0 {
        return turned;
    }
    let g = Geo::new(w as usize, h as usize, rest);
    let out = 4 * g.ow as u64 * g.oh as u64;
    let src = PX * pixels;
    let peak = match filter {
        RotationFilter::Bicubic | RotationFilter::Lanczos3 => src + out,
        // Copie des échantillons gardée pour les bornes.
        RotationFilter::BSpline => 2 * src + out,
        RotationFilter::Shear => {
            let w1 = shear_width(&g) as u64;
            let (s1, s2) = (PX * w1 * h as u64, PX * w1 * g.oh as u64);
            (src + s1).max(s1 + s2).max(s2 + out)
        }
    };
    peak.max(turned + src)
}

/// Budget de travail par défaut du redressement : large pour une photo de
/// 24 Mpx avec le filtre le plus gourmand, loin de ce qui ferme l'application
/// sur une machine peu dotée.
pub const WORK_BUDGET: u64 = 1536 << 20;

/// Taille d'une image `w` × `h` réduite d'un facteur `s` ≤ 1, et le facteur
/// réellement obtenu. Le grand côté fixe le facteur ; le petit côté est
/// arrondi au plus près de ce facteur, pour que les deux axes gardent la même
/// échelle à une fraction de pixel réduit près (un facteur pris sur le petit
/// côté d'un scan étroit décalerait le grand de plusieurs centaines de pixels).
pub fn reduced_size(w: u32, h: u32, s: f64) -> (u32, u32, f64) {
    if s.is_nan() || s >= 1.0 || w == 0 || h == 0 {
        return (w, h, 1.0);
    }
    let long = w.max(h);
    let reduced = ((long as f64 * s).floor() as u32).clamp(1, long);
    let k = reduced as f64 / long as f64;
    let side = |v: u32| ((v as f64 * k).round() as u32).max(1);
    (side(w), side(h), k)
}

/// Plus grand facteur de réduction (≤ 1) pour lequel l'image redressée tient
/// dans une texture de côté `max_side` et son calcul dans `budget` octets.
pub fn straighten_scale(w: u32, h: u32, deg: f64, filter: RotationFilter, max_side: u32, budget: u64) -> f64 {
    let fits = |s: f64| {
        let (rw, rh, _) = reduced_size(w, h, s);
        let (ow, oh) = rotated_dimensions(rw, rh, deg);
        ow.max(oh) <= max_side.max(1) && rotation_work_bytes(rw, rh, deg, filter) <= budget
    };
    if w == 0 || h == 0 || fits(1.0) {
        return 1.0;
    }
    let (bw, bh) = rotated_dimensions(w, h, deg);
    let by_side = max_side.max(1) as f64 / bw.max(bh) as f64;
    let by_memory = (budget as f64 / rotation_work_bytes(w, h, deg, filter).max(1) as f64).sqrt();
    let mut s = by_side.min(by_memory).min(1.0);
    while !fits(s) && s > 1e-4 {
        s *= 0.98;
    }
    s
}

/// Repère d'une image exportée ou affichée : où tombe un point de l'image
/// d'origine une fois celle-ci réduite puis pivotée dans sa boîte englobante.
/// Même formule que l'affichage (`Xf::rotated` du canevas),
/// q = k·R·(p − centre) + boîte/2. C'est le seul report de coordonnées des
/// exports : [`straighten`] le renvoie avec l'image, [`Frame::rotation`] le
/// donne pour un dessin vectoriel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    cos: f64,
    sin: f64,
    /// Translation : boîte/2 − k·R·centre.
    t: (f64, f64),
    /// Pixels de l'image redressée par pixel de l'image d'origine (≤ 1).
    pub scale: f64,
    /// Angle du redressement, en degrés.
    pub deg: f64,
}

impl Frame {
    /// Repère de l'image `size`, réduite à `reduced` (facteur `scale`) puis
    /// pivotée de `deg` degrés.
    pub(crate) fn new(size: (u32, u32), reduced: (u32, u32), scale: f64, deg: f64) -> Self {
        // Sans rotation : valeurs exactes, les points ne bougent pas d'un bit.
        let (sin, cos) = if deg == 0.0 { (0.0, 1.0) } else { deg.to_radians().sin_cos() };
        let (rw, rh) = (reduced.0 as f64, reduced.1 as f64);
        let (cx, cy) = (size.0 as f64 / 2.0, size.1 as f64 / 2.0);
        let half = ((cos.abs() * rw + sin.abs() * rh) / 2.0, (sin.abs() * rw + cos.abs() * rh) / 2.0);
        let t = (half.0 - scale * (cos * cx - sin * cy), half.1 - scale * (sin * cx + cos * cy));
        Self { cos, sin, t, scale, deg }
    }

    /// Repère de l'image `w` × `h` pivotée de `deg` degrés, sans réduction.
    pub fn rotation(w: u32, h: u32, deg: f64) -> Self {
        Self::new((w, h), (w, h), 1.0, deg)
    }

    /// Position, dans l'image redressée, du point `p` de l'image d'origine.
    pub fn map(&self, p: Pt) -> Pt {
        Pt::new(
            self.scale * (self.cos * p.x - self.sin * p.y) + self.t.0,
            self.scale * (self.sin * p.x + self.cos * p.y) + self.t.1,
        )
    }

    /// Forme reportée dans ce repère, à dessiner sur l'image redressée :
    /// points déplacés, rectangle réorienté, bande et bords d'un profil mis à
    /// l'échelle. Ses valeurs mesurées sont celles de l'image redressée, pas
    /// de l'image d'origine : les valeurs à afficher se lisent sur `shape`.
    pub fn place(&self, shape: &Shape) -> Shape {
        let mut placed = shape.clone();
        placed.map_similar(|p| self.map(p), self.deg, self.scale);
        placed
    }

    /// Matrice SVG `matrix(a b c d e f)` qui amène l'image d'origine dans ce repère.
    pub fn svg_matrix(&self) -> [f64; 6] {
        let k = self.scale;
        [k * self.cos, k * self.sin, -k * self.sin, k * self.cos, self.t.0, self.t.1]
    }
}

/// Image redressée et son repère.
pub struct Straightened {
    pub image: RgbaImage,
    pub frame: Frame,
}

/// Redresse `image` de `deg` degrés pour l'affichage ou l'export : comme
/// [`rotate_image`], mais l'image est d'abord réduite si le résultat ne
/// tiendrait pas dans une texture de côté `max_side` ou si le calcul
/// demanderait plus de `budget` octets.
pub fn straighten(image: &RgbaImage, deg: f64, filter: RotationFilter, max_side: u32, budget: u64) -> Straightened {
    let (w, h) = image.dimensions();
    let s = straighten_scale(w, h, deg, filter, max_side, budget);
    let (rw, rh, k) = reduced_size(w, h, s);
    let small;
    let source = if k < 1.0 {
        small = imageops::resize(image, rw, rh, imageops::FilterType::Triangle);
        &small
    } else {
        image
    };
    Straightened { image: rotate_image(source, deg, filter), frame: Frame::new((w, h), (rw, rh), k, deg) }
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
    let src = to_working(&turned);
    drop(turned);
    match filter {
        RotationFilter::Shear => rotate_shear(src, &g),
        f => resample_with(src, f, &g),
    }
}

/// Rééchantillonne `src` selon `g` avec le filtre séparable choisi (les
/// cisaillements, propres aux rotations, cèdent la place à Lanczos).
fn resample_with(mut src: Vec<Px>, filter: RotationFilter, g: &impl Mapping) -> RgbaImage {
    match filter {
        RotationFilter::Bicubic => resample(&src, &src, g, catmull_rom),
        RotationFilter::Lanczos3 | RotationFilter::Shear => {
            let table = lanczos3_table();
            resample(&src, &src, g, |f| table[(f * PHASES as f32 + 0.5) as usize])
        }
        RotationFilter::BSpline => {
            // Les bornes viennent des échantillons, pas des coefficients.
            let orig = src.clone();
            let (w, h) = g.src();
            bspline_prefilter(&mut src, w, h);
            resample(&src, &orig, g, bspline)
        }
    }
}

/// Reconstruit `image` selon la correspondance `g` (même traitement que le
/// redressement : lumière linéaire, alpha prémultiplié, valeurs bornées).
pub(crate) fn remap(image: &RgbaImage, filter: RotationFilter, g: &impl Mapping) -> RgbaImage {
    resample_with(to_working(image), filter, g)
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

    #[test]
    fn bspline_interpolates_short_lines_exactly() {
        // Préfiltre puis noyau aux positions entières : on doit retrouver les
        // échantillons, y compris quand la ligne est plus courte que l'amorce.
        for (w, h) in [(1usize, 1usize), (2, 1), (1, 2), (2, 2), (2, 40), (40, 2), (3, 5), (7, 4), (31, 33), (40, 40)] {
            let samples: Vec<Px> = (0..w * h).map(|i| [((i * 37 + 11) % 17) as f32 / 16.0, (i % 2) as f32, ((i / w) % 2) as f32, 1.0]).collect();
            let mut coef = samples.clone();
            bspline_prefilter(&mut coef, w, h);
            let k = bspline(0.0);
            for y in 0..h {
                for x in 0..w {
                    for c in 0..4 {
                        let mut v = 0.0;
                        for (ky, wy) in k.iter().enumerate() {
                            for (kx, wx) in k.iter().enumerate() {
                                let (sx, sy) = (mirror(x as isize + kx as isize - 1, w), mirror(y as isize + ky as isize - 1, h));
                                v += wx * wy * coef[sy * w + sx][c];
                            }
                        }
                        assert!((v - samples[y * w + x][c]).abs() < 2e-5, "{w}×{h} ({x}, {y}) : {v} ≠ {}", samples[y * w + x][c]);
                    }
                }
            }
        }
    }

    #[test]
    fn bspline_keeps_a_two_pixel_wide_image() {
        // Colonne gauche noire, colonne droite blanche : une rotation infime
        // ne doit rien changer (l'amorce tronquée donnait 38 au lieu de 0).
        let img = RgbaImage::from_fn(2, 40, |x, _| if x == 0 { Rgba([0, 0, 0, 255]) } else { Rgba([255, 255, 255, 255]) });
        let out = rotate_image(&img, 0.000_001, RotationFilter::BSpline);
        assert_eq!(out.dimensions(), (2, 40));
        for y in [1, 20, 38] {
            assert_eq!((out.get_pixel(0, y).0, out.get_pixel(1, y).0), ([0, 0, 0, 255], [255, 255, 255, 255]), "ligne {y}");
        }
        let tall = RgbaImage::from_fn(40, 2, |_, y| if y == 0 { Rgba([0, 0, 0, 255]) } else { Rgba([255, 255, 255, 255]) });
        let out = rotate_image(&tall, 0.000_001, RotationFilter::BSpline);
        assert_eq!((out.get_pixel(20, 0).0, out.get_pixel(20, 1).0), ([0, 0, 0, 255], [255, 255, 255, 255]));
    }

    #[test]
    fn shear_budget_is_known_without_allocating() {
        // Panorama de 16 000 × 100 px à 44° : 6 Mio de source, mais 2,67 Gio
        // pour l'image intermédiaire du deuxième cisaillement.
        let (w, h, deg) = (16_000u32, 100u32, 44.0);
        let shear = rotation_work_bytes(w, h, deg, RotationFilter::Shear);
        assert_eq!(rotated_dimensions(w, h, deg), (11_579, 11_187));
        assert!(shear >= 16_041 * 11_187 * 16, "{shear}");
        assert!(shear < 4 << 30);
        // Les filtres directs n'ont besoin que de la source et de la sortie.
        let direct = rotation_work_bytes(w, h, deg, RotationFilter::Lanczos3);
        assert_eq!(direct, 16_000 * 100 * 16 + 11_579 * 11_187 * 4);
        assert_eq!(rotation_work_bytes(w, h, deg, RotationFilter::BSpline), direct + 16_000 * 100 * 16);
        assert_eq!(rotation_work_bytes(w, h, 0.0, RotationFilter::Shear), 0);
        assert_eq!(rotation_work_bytes(w, h, 90.0, RotationFilter::Shear), 16_000 * 100 * 4);

        // Dans le budget : pleine résolution pour les filtres directs, réduite
        // juste ce qu'il faut pour les cisaillements.
        let side = 16_384;
        assert_eq!(straighten_scale(w, h, deg, RotationFilter::Lanczos3, side, WORK_BUDGET), 1.0);
        let s = straighten_scale(w, h, deg, RotationFilter::Shear, side, WORK_BUDGET);
        assert!(s < 1.0 && s > 0.6, "{s}");
        let (rw, rh, _) = reduced_size(w, h, s);
        let used = rotation_work_bytes(rw, rh, deg, RotationFilter::Shear);
        assert!(used <= WORK_BUDGET && used > WORK_BUDGET / 10 * 8, "{used}");
        // Une photo de 24 Mpx reste entière avec tous les filtres.
        for f in RotationFilter::ALL {
            assert_eq!(straighten_scale(6000, 4000, 6.0, f, side, WORK_BUDGET), 1.0, "{f:?}");
        }
        // Limite de texture seule.
        let s = straighten_scale(6000, 4000, 6.0, RotationFilter::Bicubic, 4096, WORK_BUDGET);
        let (rw, rh, _) = reduced_size(6000, 4000, s);
        let (ow, oh) = rotated_dimensions(rw, rh, 6.0);
        assert!(ow <= 4096 && oh <= 4096 && ow > 4000, "{ow} × {oh}");
    }

    #[test]
    fn reduced_size_keeps_one_scale_on_both_axes() {
        assert_eq!(reduced_size(101, 16_000, 1.0), (101, 16_000, 1.0));
        assert_eq!(reduced_size(101, 16_000, 2.0), (101, 16_000, 1.0));
        for (w, h, s) in [(101u32, 16_000u32, 0.5145), (16_000, 101, 0.5145), (3, 9000, 0.1), (4000, 3000, 0.37), (1, 5000, 0.3)] {
            let (rw, rh, k) = reduced_size(w, h, s);
            assert!(k <= s && k > s - 1.0 / w.max(h) as f64, "{w}×{h} : {k}");
            // Chaque axe à moins d'un demi-pixel réduit de l'échelle commune.
            assert!((rw as f64 - w as f64 * k).abs() <= 0.5 || rw == 1, "{w}×{h} → {rw}");
            assert!((rh as f64 - h as f64 * k).abs() <= 0.5 || rh == 1, "{w}×{h} → {rh}");
        }
    }

    #[test]
    fn marks_stay_under_measurements_on_a_reduced_narrow_scan() {
        // Scan étroit de 101 × 16 000 px redressé de 6° pour un GPU limité à
        // 8 192 px : un repère près du haut et un près du bas doivent tomber
        // là où le canevas dessine les mesures, soit, dans la texture étalée
        // sur (taille / échelle), en k·(R·(p − c) + boîte/2).
        let (w, h, deg) = (101u32, 16_000u32, 6.0f64);
        let marks = [(40.0, 300.0), (60.0, 15_700.0)];
        let t = tables();
        let img = RgbaImage::from_fn(w, h, |x, y| {
            let v = marks
                .iter()
                .map(|(cx, cy)| {
                    let (dx, dy) = (x as f64 + 0.5 - cx, y as f64 + 0.5 - cy);
                    (-(dx * dx + dy * dy) / (2.0 * 6.0 * 6.0)).exp()
                })
                .fold(0.0, f64::max);
            let v = t.encode(v as f32);
            Rgba([v, v, v, 255])
        });
        let out = straighten(&img, deg, RotationFilter::Lanczos3, 8192, WORK_BUDGET);
        let k = out.frame.scale;
        assert!(k < 0.52 && out.image.width().max(out.image.height()) <= 8192, "{k}");
        let (s, c) = deg.to_radians().sin_cos();
        let (bw, bh) = (c.abs() * w as f64 + s.abs() * h as f64, s.abs() * w as f64 + c.abs() * h as f64);
        let half = out.image.height() / 2;
        for (n, (px, py)) in marks.into_iter().enumerate() {
            let (dx, dy) = (px - w as f64 / 2.0, py - h as f64 / 2.0);
            let (ex, ey) = (k * (c * dx - s * dy + bw / 2.0), k * (s * dx + c * dy + bh / 2.0));
            // Barycentre du repère, cherché dans sa moitié de l'image.
            let (mut sx, mut sy, mut sm) = (0.0, 0.0, 0.0);
            let rows = if n == 0 { 0..half } else { half..out.image.height() };
            for y in rows {
                for x in 0..out.image.width() {
                    let p = out.image.get_pixel(x, y);
                    let m = t.to_linear[p[0] as usize] as f64 * p[3] as f64;
                    sx += m * (x as f64 + 0.5);
                    sy += m * (y as f64 + 0.5);
                    sm += m;
                }
            }
            let (gx, gy) = (sx / sm, sy / sm);
            assert!((gx - ex).abs() < 0.5 && (gy - ey).abs() < 0.5, "repère {n} : ({gx:.2}, {gy:.2}) ≠ ({ex:.2}, {ey:.2})");
            // Le repère du module donne la même position.
            let q = out.frame.map(Pt::new(px, py));
            assert!((q.x - ex).abs() < 0.5 && (q.y - ey).abs() < 0.5);
        }
    }

    #[test]
    fn formes_reportees_dans_le_repere() {
        meteroscope_i18n::set_language("fr");
        // Quart de tour horaire d'une image 200 × 100 : boîte 100 × 200,
        // le coin haut-gauche part en haut à droite.
        let f = Frame::rotation(200, 100, 90.0);
        assert!(f.map(Pt::new(0., 0.)).dist(Pt::new(100., 0.)) < 1e-9);
        assert!(f.map(Pt::new(200., 100.)).dist(Pt::new(0., 200.)) < 1e-9);
        let m = f.svg_matrix();
        let by_matrix = |p: Pt| Pt::new(m[0] * p.x + m[2] * p.y + m[4], m[1] * p.x + m[3] * p.y + m[5]);
        assert!(by_matrix(Pt::new(30., 70.)).dist(f.map(Pt::new(30., 70.))) < 1e-9);
        // Sans rotation ni réduction : rien ne bouge.
        let same = Frame::rotation(200, 100, 0.0);
        assert_eq!(same.map(Pt::new(12.3, 45.6)), Pt::new(12.3, 45.6));

        // Rotation seule : mêmes valeurs mesurées, rectangle réorienté.
        let cal = meteroscope_core::Calibration::default();
        let rect = Shape::Rect { a: Pt::new(10., 10.), b: Pt::new(50., 30.), angle: 0.0 };
        let length = Shape::Length { a: Pt::new(0., 0.), b: Pt::new(30., 40.) };
        for s in [&rect, &length] {
            for (a, b) in s.values(&cal).iter().zip(f.place(s).values(&cal)) {
                assert!((a.value - b.value).abs() < 1e-9, "{a:?} {b:?}");
            }
        }
        let Shape::Rect { angle, .. } = f.place(&rect) else { panic!() };
        assert_eq!(angle, 90.0);

        // Réduction de moitié : bords et bande d'un profil suivent.
        let half = Frame::new((200, 100), (100, 50), 0.5, 0.0);
        let mut profile = Shape::profile(Pt::new(20., 50.), Pt::new(180., 50.));
        if let Shape::Profile { params, result, .. } = &mut profile {
            params.band = 12.0;
            *result = Some(meteroscope_core::ProfileResult { edges: [40.0, 100.0], tilt: 0.0 });
        }
        let placed = half.place(&profile);
        assert_eq!(placed.profile_edges(), Some([Pt::new(30., 25.), Pt::new(60., 25.)]));
        assert_eq!(placed.profile_band().map(|b| b.0), Some(3.0));
    }
}

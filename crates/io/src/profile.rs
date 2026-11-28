//! Profil d'intensité : échantillonnage de l'image le long d'un segment,
//! puis détection des bords (`meteroscope_core::profile`).
//!
//! Choix de mesure :
//!
//! - **Grandeur** : la luminance *linéaire* (Rec. 709 après retrait du gamma
//!   sRGB). Le flou optique et l'intégration par les photosites additionnent
//!   de la lumière, pas des valeurs codées : c'est dans l'espace linéaire
//!   qu'un bord flouté est symétrique autour de sa vraie position. Sur les
//!   valeurs codées, le niveau médian correspond à ~21 % de la lumière et
//!   la mi-hauteur se décale d'environ 0,8 σ de flou vers le côté sombre :
//!   un trait clair paraît plus large, un trait sombre plus étroit. Le
//!   réglage `linear` permet de revenir aux valeurs codées pour une image
//!   déjà linéaire (caméra scientifique).
//! - **Échantillonnage** : interpolation bilinéaire, un point tous les
//!   [`STEP`] pixels le long du segment, sur des lignes parallèles espacées
//!   d'un pixel au plus couvrant la bande, puis moyenne des lignes. Le bruit
//!   diminue comme la racine du nombre de lignes.
//! - **Inclinaison** : si le segment n'est pas exactement perpendiculaire
//!   aux bords, la distance lue le long du segment est trop grande d'un
//!   facteur 1/cos θ (0,4 % à 5°). Les deux moitiés de la bande donnent la
//!   position de chaque bord en deux endroits, donc θ, et la largeur est
//!   ramenée à la perpendiculaire.
//! - L'image mesurée est l'image d'origine : luminosité et contraste ne sont
//!   qu'un réglage d'affichage et ne touchent pas la mesure.

use crate::table::{csv_field, csv_separator, format_precise};
use image::RgbaImage;
use meteroscope_core::profile::{Analysis, ProfileParams, analyze};
use meteroscope_core::{Calibration, Pt};
use meteroscope_i18n::tr;

/// Pas d'échantillonnage le long du segment, en pixels.
pub const STEP: f64 = 0.5;
/// Longueur minimale d'un segment mesurable, en pixels.
const MIN_LENGTH: f64 = 4.0;
/// Nombre minimal de lignes dans la bande pour estimer l'inclinaison des bords.
const TILT_MIN_LINES: usize = 4;
/// Inclinaison au-delà de laquelle l'estimation n'est plus crédible (degrés).
const TILT_MAX: f64 = 45.0;

/// Table valeur codée sRGB (0–255) → intensité linéaire (0–1).
fn srgb_to_linear() -> [f32; 256] {
    std::array::from_fn(|i| {
        let c = i as f32 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    })
}

/// Lecture de la luminance d'une image, avec interpolation bilinéaire.
struct Luminance<'a> {
    image: &'a RgbaImage,
    lut: [f32; 256],
}

impl Luminance<'_> {
    fn new(image: &RgbaImage, linear: bool) -> Luminance<'_> {
        let lut = if linear { srgb_to_linear() } else { std::array::from_fn(|i| i as f32 / 255.0) };
        Luminance { image, lut }
    }

    fn pixel(&self, x: i64, y: i64) -> f64 {
        let (w, h) = self.image.dimensions();
        // Hors de l'image : le pixel du bord est prolongé.
        let p = self.image.get_pixel(x.clamp(0, w as i64 - 1) as u32, y.clamp(0, h as i64 - 1) as u32);
        let c = |i: usize| self.lut[p[i] as usize] as f64;
        0.2126 * c(0) + 0.7152 * c(1) + 0.0722 * c(2)
    }

    /// Luminance au point `p` (coordonnées continues : le centre du pixel
    /// (i, j) est en (i + 0,5 ; j + 0,5)).
    fn at(&self, p: Pt) -> f64 {
        let (x, y) = (p.x - 0.5, p.y - 0.5);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (i, j) = (x0 as i64, y0 as i64);
        let top = self.pixel(i, j) * (1.0 - fx) + self.pixel(i + 1, j) * fx;
        let bottom = self.pixel(i, j + 1) * (1.0 - fx) + self.pixel(i + 1, j + 1) * fx;
        top * (1.0 - fy) + bottom * fy
    }
}

/// Moyenne de plusieurs lignes d'échantillons.
fn mean(lines: &[Vec<f64>]) -> Vec<f64> {
    let n = lines.first().map_or(0, Vec::len);
    (0..n).map(|i| lines.iter().map(|l| l[i]).sum::<f64>() / lines.len() as f64).collect()
}

/// Mesure le profil d'intensité de `image` le long du segment `a→b`.
///
/// Renvoie l'analyse complète (courbe, bords, bords retenus, inclinaison).
/// Un segment trop court ou une image vide donnent une analyse sans bords.
pub fn measure_profile(image: &RgbaImage, a: Pt, b: Pt, params: &ProfileParams) -> Analysis {
    let len = a.dist(b);
    if !(len >= MIN_LENGTH && len.is_finite()) || image.width() == 0 || image.height() == 0 {
        return Analysis { step: STEP, ..Default::default() };
    }
    let lum = Luminance::new(image, params.linear);
    let (ux, uy) = ((b.x - a.x) / len, (b.y - a.y) / len);
    // Échantillons : les deux extrémités comprises, pas voisin de STEP.
    let n = (len / STEP).round() as usize + 1;
    let step = len / (n - 1) as f64;
    // Lignes parallèles réparties sur la bande, espacées d'un pixel au plus.
    let band = if params.band.is_finite() { params.band.max(0.0) } else { 0.0 };
    let count = (band.ceil() as usize).max(1);
    let spacing = band / count as f64;
    let offsets: Vec<f64> = (0..count).map(|k| (k as f64 - (count - 1) as f64 / 2.0) * spacing).collect();
    let lines: Vec<Vec<f64>> = offsets
        .iter()
        .map(|o| {
            let origin = a.offset(-uy * o, ux * o);
            (0..n).map(|i| lum.at(origin.offset(ux * step * i as f64, uy * step * i as f64))).collect()
        })
        .collect();
    let mut out = analyze(&mean(&lines), step, params.method, params.threshold, params.pick);
    if params.perpendicular && count >= TILT_MIN_LINES {
        out.tilt = tilt(&out, &lines, &offsets, params).unwrap_or(0.0);
    }
    out
}

/// Inclinaison (degrés) des bords retenus par rapport à la perpendiculaire
/// au segment : chaque bord est relocalisé dans les deux moitiés de la
/// bande ; son décalage, rapporté à la distance entre les moitiés, donne la
/// tangente de l'angle. `None` si un bord ne se retrouve pas des deux côtés.
fn tilt(whole: &Analysis, lines: &[Vec<f64>], offsets: &[f64], params: &ProfileParams) -> Option<f64> {
    let half = lines.len() / 2;
    // La ligne centrale d'une bande impaire est laissée de côté.
    let (left, right) = (0..half, lines.len() - half..lines.len());
    let center = |r: std::ops::Range<usize>| offsets[r.clone()].iter().sum::<f64>() / r.len() as f64;
    let gap = center(right.clone()) - center(left.clone());
    let sides = [left, right].map(|r| analyze(&mean(&lines[r]), whole.step, params.method, Some(whole.threshold), None));
    let reach = gap / 2.0 * TILT_MAX.to_radians().tan() + 2.0;
    let mut slope = 0.0;
    for i in whole.chosen? {
        let e = whole.edges[i];
        let found = |side: &Analysis| {
            let j = side.nearest_edge(e.pos)?;
            let f = side.edges[j];
            (f.rising == e.rising && (f.pos - e.pos).abs() <= reach).then_some(f.pos)
        };
        slope += (found(&sides[1])? - found(&sides[0])?) / gap / 2.0;
    }
    let deg = slope.atan().to_degrees();
    (deg.abs() <= TILT_MAX).then_some(deg)
}

/// Courbe d'un profil en CSV (mêmes conventions que le tableau des mesures) :
/// bords détectés et largeur en tête, puis une ligne par échantillon
/// (position dans l'unité de l'échelle, position en pixels, intensité 0–1).
pub fn profile_csv_string(name: &str, analysis: &Analysis, cal: &Calibration) -> String {
    let sep = csv_separator();
    let unit = cal.length_unit();
    let mut out = String::from("\u{feff}");
    let mut push = |row: &[String]| {
        out.push_str(&row.iter().map(|f| csv_field(f, sep)).collect::<Vec<_>>().join(&sep.to_string()));
        out.push_str("\r\n");
    };
    push(&[tr!("table.name").to_owned(), name.to_owned()]);
    if let Some(r) = analysis.result() {
        push(&[format!("{} ({unit})", tr!("quantity.width")), format_precise(cal.length(r.width_px()))]);
        if r.tilt != 0.0 {
            push(&[format!("{} (°)", tr!("profile.tilt")), format_precise(r.tilt)]);
        }
    }
    for (i, e) in analysis.edges.iter().enumerate() {
        let chosen = analysis.chosen.is_some_and(|c| c.contains(&i));
        push(&[
            format!("{} {} ({unit})", tr!("profile.edge"), i + 1),
            format_precise(cal.length(e.pos)),
            if chosen { tr!("profile.edge_used").to_owned() } else { String::new() },
        ]);
    }
    push(&[]);
    push(&[format!("{} ({unit})", tr!("item.position")), format!("{} (px)", tr!("item.position")), tr!("profile.intensity").to_owned()]);
    for (i, v) in analysis.values.iter().enumerate() {
        let px = i as f64 * analysis.step;
        push(&[format_precise(cal.length(px)), format_precise(px), format_precise(*v)]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Transform;
    use image::Rgba;
    use meteroscope_core::profile::EdgeMethod;
    use meteroscope_core::{Document, Shape};

    /// Fonction de répartition de la loi normale (Abramowitz et Stegun 7.1.26).
    fn phi(x: f64) -> f64 {
        let z = x.abs() / std::f64::consts::SQRT_2;
        let t = 1.0 / (1.0 + 0.3275911 * z);
        let poly = t * (0.254829592 + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
        let erf = 1.0 - poly * (-z * z).exp();
        if x >= 0.0 { 0.5 * (1.0 + erf) } else { 0.5 * (1.0 - erf) }
    }

    /// Générateur pseudo-aléatoire reproductible (xorshift) et bruit gaussien.
    struct Rng(u64);
    impl Rng {
        fn uniform(&mut self) -> f64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            ((self.0 >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        }
        fn gauss(&mut self) -> f64 {
            (-2.0 * self.uniform().ln()).sqrt() * (std::f64::consts::TAU * self.uniform()).cos()
        }
    }

    /// Image de synthèse : une barre claire de largeur `width` (pixels, non
    /// entière), d'axe incliné de `axis` degrés, centrée en `center`, floutée
    /// par une gaussienne `sigma`, intégrée sur la surface de chaque pixel,
    /// codée en sRGB 8 bits puis bruitée (`noise` : écart-type en niveaux).
    #[derive(Clone, Copy)]
    struct Scene {
        size: u32,
        center: Pt,
        axis: f64,
        width: f64,
        sigma: f64,
        noise: f64,
        seed: u64,
        /// Barre sombre sur fond clair.
        dark_bar: bool,
    }

    impl Default for Scene {
        fn default() -> Self {
            Scene { size: 96, center: Pt::new(48.3, 47.6), axis: 90.0, width: 17.37, sigma: 1.0, noise: 0.0, seed: 1, dark_bar: false }
        }
    }

    const DARK: f64 = 0.04;
    const BRIGHT: f64 = 0.55;

    impl Scene {
        /// Normale unitaire à la barre (direction de la largeur).
        fn normal(&self) -> (f64, f64) {
            let t = self.axis.to_radians();
            (-t.sin(), t.cos())
        }

        fn image(&self) -> RgbaImage {
            let (nx, ny) = self.normal();
            let mut rng = Rng(self.seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
            // Sur-échantillonnage du pixel : sa surface intègre la lumière.
            let sub = if self.sigma < 0.5 { 12 } else { 4 };
            RgbaImage::from_fn(self.size, self.size, |x, y| {
                let mut cover = 0.0;
                for sy in 0..sub {
                    for sx in 0..sub {
                        let px = x as f64 + (sx as f64 + 0.5) / sub as f64 - self.center.x;
                        let py = y as f64 + (sy as f64 + 0.5) / sub as f64 - self.center.y;
                        let d = px * nx + py * ny;
                        cover += if self.sigma > 0.0 {
                            phi((d + self.width / 2.0) / self.sigma) - phi((d - self.width / 2.0) / self.sigma)
                        } else if d.abs() <= self.width / 2.0 {
                            1.0
                        } else {
                            0.0
                        };
                    }
                }
                cover /= (sub * sub) as f64;
                if self.dark_bar {
                    cover = 1.0 - cover;
                }
                let lin = DARK + (BRIGHT - DARK) * cover;
                let coded = if lin <= 0.0031308 { 12.92 * lin } else { 1.055 * lin.powf(1.0 / 2.4) - 0.055 };
                let v = (coded * 255.0 + self.noise * rng.gauss()).round().clamp(0.0, 255.0) as u8;
                Rgba([v, v, v, 255])
            })
        }

        /// Segment de longueur `len` centré sur la barre, écarté de `skew`
        /// degrés de la perpendiculaire aux bords.
        fn segment(&self, len: f64, skew: f64) -> (Pt, Pt) {
            let t = (self.axis + 90.0 + skew).to_radians();
            let (ux, uy) = (t.cos() * len / 2.0, t.sin() * len / 2.0);
            (self.center.offset(-ux, -uy), self.center.offset(ux, uy))
        }
    }

    fn params(method: EdgeMethod, band: f64) -> ProfileParams {
        ProfileParams { method, band, ..Default::default() }
    }

    /// Erreur sur la largeur (pixels) pour une scène, un segment et des réglages.
    fn width_error(scene: &Scene, skew: f64, p: &ProfileParams) -> f64 {
        let (a, b) = scene.segment(scene.width + 42.6, skew);
        match measure_profile(&scene.image(), a, b, p).result() {
            Some(r) => r.width_px() - scene.width,
            None => f64::NAN,
        }
    }

    /// (erreur maximale, écart-type) de la largeur sur plusieurs positions
    /// sous-pixel de la barre et tirages de bruit.
    fn spread(base: Scene, skew: f64, p: &ProfileParams) -> (f64, f64) {
        let mut errs = vec![];
        for k in 0..12u64 {
            let shift = k as f64 * 0.083;
            let scene = Scene { center: base.center.offset(shift, 0.37 * shift), seed: base.seed + k, ..base };
            errs.push(width_error(&scene, skew, p));
        }
        let max = errs.iter().fold(0.0f64, |m, e| if e.is_nan() { f64::INFINITY } else { m.max(e.abs()) });
        let mean = errs.iter().sum::<f64>() / errs.len() as f64;
        let sd = (errs.iter().map(|e| (e - mean).powi(2)).sum::<f64>() / (errs.len() - 1) as f64).sqrt();
        (max, sd)
    }

    /// Tableau de précision (à lire avec `cargo test -p meteroscope-io
    /// tableau_de_precision -- --ignored --nocapture`) : il a servi à choisir
    /// la méthode par défaut et à fixer les tolérances des tests ci-dessous.
    #[test]
    #[ignore = "tableau d'exploration, sans assertion"]
    fn tableau_de_precision() {
        let methods = [EdgeMethod::HalfHeight, EdgeMethod::Gradient];
        println!("erreur max / écart-type sur la largeur (px), 12 positions sous-pixel");
        println!("{:<46} {:>17} {:>17}", "", "mi-hauteur", "gradient");
        let line = |label: String, scene: Scene, skew: f64, band: f64, linear: bool| {
            let cells: Vec<String> = methods
                .iter()
                .map(|m| {
                    let (max, sd) = spread(scene, skew, &ProfileParams { linear, ..params(*m, band) });
                    format!("{max:>8.3} / {sd:<6.3}")
                })
                .collect();
            println!("{label:<46} {} {}", cells[0], cells[1]);
        };
        let wide = Scene { width: 40.37, size: 160, center: Pt::new(80.3, 79.6), ..Default::default() };
        for axis in [90.0, 83.0, 67.0, 45.0] {
            for sigma in [0.0, 0.7, 1.5, 3.0] {
                line(format!("axe {axis}° flou {sigma} sans bruit bande 9"), Scene { axis, sigma, ..Default::default() }, 0.0, 9.0, true);
            }
        }
        for sigma in [3.0, 5.0] {
            for noise in [0.0, 1.0] {
                line(format!("barre de 40 px, flou {sigma} bruit {noise}"), Scene { axis: 67.0, sigma, noise, ..wide }, 0.0, 9.0, true);
            }
        }
        for noise in [1.0, 2.0, 5.0, 10.0, 20.0] {
            for band in [1.0, 9.0, 31.0] {
                line(format!("axe 67° flou 1,5 bruit {noise} bande {band}"), Scene { axis: 67.0, sigma: 1.5, noise, ..Default::default() }, 0.0, band, true);
            }
        }
        for sigma in [1.5, 3.0] {
            line(format!("valeurs codées (gamma non retiré), flou {sigma}"), Scene { axis: 67.0, sigma, ..Default::default() }, 0.0, 9.0, false);
        }
        for skew in [2.0, 5.0, 10.0, 20.0] {
            line(format!("segment de travers de {skew}°, bande 21"), Scene { axis: 67.0, sigma: 1.5, noise: 2.0, ..Default::default() }, skew, 21.0, true);
        }
        for width in [2.0, 3.0, 5.0, 8.0, 12.0] {
            line(format!("trait étroit {width} px, flou 1,5"), Scene { axis: 67.0, sigma: 1.5, width, ..Default::default() }, 0.0, 9.0, true);
        }
    }

    const HALF: EdgeMethod = EdgeMethod::HalfHeight;
    const GRAD: EdgeMethod = EdgeMethod::Gradient;

    #[test]
    fn bord_net_peu_bruite_mieux_que_le_dixieme_de_pixel() {
        // Barre de 17,37 px, segment incliné (axe à 67° ou 45°) ou aligné sur
        // les pixels, flou de 0,7 à 1,5 px, bruit d'un niveau sur 255, bande
        // de 9 px : la largeur est retrouvée à mieux que 0,1 px par les deux
        // méthodes, quelle que soit la position sous-pixel de la barre.
        for axis in [90.0, 67.0, 45.0] {
            for sigma in [0.7, 1.5] {
                let scene = Scene { axis, sigma, noise: 1.0, ..Default::default() };
                let (half, _) = spread(scene, 0.0, &params(HALF, 9.0));
                let (grad, _) = spread(scene, 0.0, &params(GRAD, 9.0));
                assert!(half < 0.06, "mi-hauteur, axe {axis}° flou {sigma} : {half}");
                assert!(grad < 0.1, "gradient, axe {axis}° flou {sigma} : {grad}");
            }
        }
        // Barre sombre sur fond clair : même précision.
        let (dark, _) = spread(Scene { axis: 67.0, sigma: 1.0, noise: 1.0, dark_bar: true, ..Default::default() }, 0.0, &params(HALF, 9.0));
        assert!(dark < 0.06, "{dark}");
    }

    #[test]
    fn bord_parfaitement_net() {
        // Sans aucun flou, la seule information sous-pixel est le niveau du
        // pixel traversé par le bord. Segment incliné : les lignes de la
        // bande voient le bord sous des phases différentes, l'erreur reste
        // sous 0,03 px. Bord exactement aligné sur les colonnes de pixels :
        // toutes les lignes voient la même phase et l'interpolation
        // bilinéaire laisse jusqu'à ~0,1 px sur la largeur (cas le pire).
        let (inclined, _) = spread(Scene { axis: 67.0, sigma: 0.0, ..Default::default() }, 0.0, &params(HALF, 9.0));
        assert!(inclined < 0.03, "{inclined}");
        let (aligned, _) = spread(Scene { axis: 90.0, sigma: 0.0, ..Default::default() }, 0.0, &params(HALF, 9.0));
        assert!(aligned < 0.12, "{aligned}");
    }

    #[test]
    fn degradation_avec_le_bruit_et_le_flou() {
        // Écart-type de la largeur mesurée (répétabilité), contraste de
        // ~140 niveaux, flou 1,5 px. Le bruit se paie en proportion ; élargir
        // la bande le réduit (comme la racine du nombre de lignes).
        let noisy = |noise: f64| Scene { axis: 67.0, sigma: 1.5, noise, ..Default::default() };
        for (noise, band, limit) in [(2.0, 9.0, 0.05), (5.0, 9.0, 0.12), (10.0, 9.0, 0.22), (20.0, 9.0, 0.4), (10.0, 31.0, 0.12), (20.0, 31.0, 0.25)] {
            let (_, sd) = spread(noisy(noise), 0.0, &params(HALF, band));
            assert!(sd < limit, "bruit {noise} bande {band} : écart-type {sd}");
        }
        let (_, single) = spread(noisy(5.0), 0.0, &params(HALF, 1.0));
        let (_, wide) = spread(noisy(5.0), 0.0, &params(HALF, 31.0));
        assert!(wide < single / 2.0, "{wide} contre {single}");
        // À bruit égal, le gradient est environ deux fois moins répétable.
        let (_, half) = spread(noisy(10.0), 0.0, &params(HALF, 9.0));
        let (_, grad) = spread(noisy(10.0), 0.0, &params(GRAD, 9.0));
        assert!(grad > 1.4 * half && grad < 0.4, "{grad} contre {half}");
        // Flou important (σ = 5 px, transition étalée sur ~20 px) et bruité :
        // la mi-hauteur tient encore le dixième de pixel sur une barre large.
        let blurred = Scene { axis: 67.0, sigma: 5.0, noise: 1.0, width: 40.37, size: 160, center: Pt::new(80.3, 79.6), ..Default::default() };
        let (max, _) = spread(blurred, 0.0, &params(HALF, 9.0));
        assert!(max < 0.12, "{max}");
    }

    #[test]
    fn trait_etroit_devant_le_flou() {
        // Quand le trait n'est pas nettement plus large que le flou, les
        // plateaux disparaissent : la mi-hauteur surestime la largeur
        // (+0,1 px à 5 σ, +0,7 px à 3,3 σ), le gradient résiste mieux.
        let narrow = |width: f64| Scene { axis: 67.0, sigma: 1.5, width, ..Default::default() };
        let err = |width: f64, m: EdgeMethod| width_error(&narrow(width), 0.0, &params(m, 9.0));
        assert!((0.05..0.15).contains(&err(8.0, HALF)), "{}", err(8.0, HALF));
        assert!(err(8.0, GRAD).abs() < 0.03, "{}", err(8.0, GRAD));
        assert!((0.5..0.8).contains(&err(5.0, HALF)), "{}", err(5.0, HALF));
        assert!((0.15..0.35).contains(&err(5.0, GRAD)), "{}", err(5.0, GRAD));
    }

    #[test]
    fn gamma_non_retire_fausse_la_largeur() {
        // Sur les valeurs codées sRGB, la mi-hauteur tombe à ~21 % de la
        // lumière : la barre claire paraît plus large de ~0,9 σ de chaque côté.
        let scene = Scene { axis: 67.0, sigma: 1.5, ..Default::default() };
        let coded = width_error(&scene, 0.0, &ProfileParams { linear: false, ..params(HALF, 9.0) });
        assert!((1.1..1.5).contains(&coded), "{coded}");
        assert!(width_error(&scene, 0.0, &params(HALF, 9.0)).abs() < 0.03);
    }

    #[test]
    fn segment_de_travers_ramene_a_la_perpendiculaire() {
        let scene = Scene { axis: 67.0, sigma: 1.5, noise: 2.0, ..Default::default() };
        for skew in [-10.0, 5.0, 10.0, 20.0] {
            let (max, _) = spread(scene, skew, &params(HALF, 21.0));
            assert!(max < 0.1, "travers {skew}° : {max}");
            let (a, b) = scene.segment(60.0, skew);
            let tilt = measure_profile(&scene.image(), a, b, &params(HALF, 21.0)).tilt;
            assert!((tilt.abs() - skew.abs()).abs() < 0.5, "travers {skew}° : inclinaison estimée {tilt}°");
        }
        // Sans la correction : l'erreur de cosinus, 17,37 × (1/cos 10° − 1) = 0,27 px.
        let raw = width_error(&scene, 10.0, &ProfileParams { perpendicular: false, ..params(HALF, 21.0) });
        assert!((0.2..0.35).contains(&raw), "{raw}");
        // Bande trop étroite pour estimer l'inclinaison : pas de correction.
        let (a, b) = scene.segment(60.0, 10.0);
        assert_eq!(measure_profile(&scene.image(), a, b, &params(HALF, 2.0)).tilt, 0.0);
    }

    #[test]
    fn rotations_et_miroirs_conservent_la_mesure() {
        meteroscope_i18n::set_language("fr");
        let scene = Scene { axis: 67.0, sigma: 1.2, noise: 2.0, ..Default::default() };
        let mut image = scene.image();
        let (a, b) = scene.segment(60.0, 4.0);
        let p = params(HALF, 9.0);
        let reference = measure_profile(&image, a, b, &p).result().unwrap();
        let mut doc = Document::new();
        doc.add(Shape::Profile { a, b, params: p.clone(), result: Some(reference) });
        for t in [Transform::RotateRight, Transform::FlipHorizontal, Transform::RotateLeft, Transform::FlipVertical, Transform::RotateRight] {
            image = crate::apply_transform(&image, &mut doc, t);
            let Shape::Profile { a, b, params, .. } = &doc.measurements[0].shape else { panic!() };
            // Les pixels et les points tournent ensemble : mêmes bords, au
            // signe de l'inclinaison près (un miroir l'inverse).
            let r = measure_profile(&image, *a, *b, params).result().unwrap();
            assert!((r.edges[0] - reference.edges[0]).abs() < 1e-6 && (r.edges[1] - reference.edges[1]).abs() < 1e-6, "{t:?}");
            assert!((r.width_px() - reference.width_px()).abs() < 1e-6, "{t:?}");
        }
    }

    #[test]
    fn cas_limites_et_csv() {
        meteroscope_i18n::set_language("fr");
        let scene = Scene::default();
        let image = scene.image();
        // Segment trop court, image vide, segment qui sort de l'image : pas de panique.
        assert!(measure_profile(&image, Pt::new(5., 5.), Pt::new(6., 5.), &ProfileParams::default()).result().is_none());
        assert!(measure_profile(&RgbaImage::new(0, 0), Pt::new(0., 0.), Pt::new(50., 0.), &ProfileParams::default()).result().is_none());
        assert!(measure_profile(&image, Pt::new(-40., 47.), Pt::new(-10., 47.), &ProfileParams::default()).result().is_none());
        let out = measure_profile(&image, Pt::new(-20., 47.6), Pt::new(140., 47.6), &ProfileParams::default());
        assert!((out.result().unwrap().width_px() - scene.width).abs() < 0.1);
        // Courbe exportée : en-tête, bords, puis un échantillon par ligne.
        let (a, b) = scene.segment(60.0, 0.0);
        let analysis = measure_profile(&image, a, b, &ProfileParams::default());
        let mut cal = Calibration::default();
        cal.set_from_reference(10.0, 1.0, "mm");
        let csv = profile_csv_string("Profil 1", &analysis, &cal);
        let lines: Vec<&str> = csv.trim_start_matches('\u{feff}').split("\r\n").collect();
        assert_eq!(lines[0], "Nom;Profil 1");
        assert!(lines[1].starts_with("Largeur (mm);1,73"), "{}", lines[1]);
        assert!(lines[2].starts_with("Bord 1 (mm);2,13") && lines[2].ends_with(";retenu"), "{}", lines[2]);
        assert_eq!(lines[5], "Position (mm);Position (px);Intensité");
        assert_eq!(lines[6].split(';').count(), 3);
        assert_eq!(lines.len(), 6 + analysis.values.len() + 1);
        assert_eq!(analysis.values.len(), 121);
    }
}

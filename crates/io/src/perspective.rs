//! Correction de perspective : reprojection de la photo dans le plan de
//! l'objet désigné (voir `meteroscope_core::perspective`), avec les filtres
//! de reconstruction du redressement.

use crate::rotate::{Mapping, RotationFilter, remap};
use image::RgbaImage;
use meteroscope_core::{Homography, Perspective, Pt};

/// Chaque pixel de l'image corrigée va chercher sa couleur dans la photo.
struct Warp {
    src: (usize, usize),
    out: (usize, usize),
    /// Image corrigée → photo, dénominateur positif du côté visible.
    back: Homography,
}

impl Mapping for Warp {
    fn src(&self) -> (usize, usize) {
        self.src
    }
    fn out(&self) -> (usize, usize) {
        self.out
    }
    fn source(&self, i: usize, j: usize) -> (f64, f64) {
        let p = Pt::new(i as f64 + 0.5, j as f64 + 0.5);
        // Au-delà de la ligne de fuite, la formule renverrait l'autre moitié
        // du plan, retournée : ces pixels restent vides.
        if self.back.denom(p) <= 0.0 {
            return (f64::NEG_INFINITY, f64::NEG_INFINITY);
        }
        let q = self.back.apply(p);
        (q.x, q.y)
    }
}

/// Image corrigée de la perspective `p` : la photo `image` reprojetée, de
/// taille `p.size` ; hors de la photo, les pixels sont transparents.
/// `None` si la correction n'est pas applicable (coins dégénérés, image vide).
///
/// Le filtre `Shear`, propre aux rotations, est remplacé par Lanczos.
pub fn warp_perspective(image: &RgbaImage, p: &Perspective, filter: RotationFilter) -> Option<RgbaImage> {
    if image.width() == 0 || image.height() == 0 || p.size.0 == 0 || p.size.1 == 0 {
        return None;
    }
    let mut back = p.homography()?.inverse()?;
    let center = Pt::new((p.dst[0].x + p.dst[2].x) / 2.0, (p.dst[0].y + p.dst[2].y) / 2.0);
    if back.denom(center) < 0.0 {
        back = Homography(back.0.map(|r| r.map(|v| -v)));
    }
    let warp = Warp {
        src: (image.width() as usize, image.height() as usize),
        out: (p.size.0 as usize, p.size.1 as usize),
        back,
    };
    Some(remap(image, filter, &warp))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    use meteroscope_core::{Document, Shape};

    /// Damier de 8 × 6 cases de 50 mm (400 × 300 mm), sur fond blanc.
    const SQUARE: f64 = 50.0;
    const BOARD: (f64, f64) = (400.0, 300.0);
    const PHOTO: (u32, u32) = (800, 600);

    fn srgb(l: f64) -> u8 {
        let v = if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
        (v * 255.0).round() as u8
    }

    fn linear(v: u8) -> f64 {
        let c = v as f64 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    }

    /// Luminance (linéaire) du damier au point (x, y) du plan, en mm.
    fn board(x: f64, y: f64) -> f64 {
        let inside = x >= 0.0 && y >= 0.0 && x < BOARD.0 && y < BOARD.1;
        if inside && ((x / SQUARE).floor() + (y / SQUARE).floor()) % 2.0 == 0.0 { 0.0 } else { 1.0 }
    }

    /// Coins du damier vus par un appareil (sténopé, axe optique au centre
    /// de la photo) qui le regarde de biais.
    fn corners_in_photo() -> [Pt; 4] {
        let (f, dist) = (800.0, 700.0);
        let (sy, cy) = 25f64.to_radians().sin_cos();
        let (sp, cp) = 30f64.to_radians().sin_cos();
        [(0.0, 0.0), (BOARD.0, 0.0), (BOARD.0, BOARD.1), (0.0, BOARD.1)].map(|(x, y)| {
            let (x, y) = (x - BOARD.0 / 2.0, y - BOARD.1 / 2.0);
            let (x1, z1) = (cy * x, -sy * x);
            let (y2, z2) = (cp * y - sp * z1, sp * y + cp * z1);
            let z = z2 + dist;
            Pt::new(PHOTO.0 as f64 / 2.0 + f * x1 / z, PHOTO.1 as f64 / 2.0 + f * y2 / z)
        })
    }

    /// Photo du damier : homographie connue, 4 × 4 échantillons par pixel.
    fn photo(corners: &[Pt; 4]) -> RgbaImage {
        let plane = [Pt::new(0.0, 0.0), Pt::new(BOARD.0, 0.0), Pt::new(BOARD.0, BOARD.1), Pt::new(0.0, BOARD.1)];
        let to_plane = Homography::from_points(corners, &plane).unwrap();
        RgbaImage::from_fn(PHOTO.0, PHOTO.1, |i, j| {
            let mut sum = 0.0;
            for a in 0..4 {
                for b in 0..4 {
                    let q = to_plane.apply(Pt::new(i as f64 + (a as f64 + 0.5) / 4.0, j as f64 + (b as f64 + 0.5) / 4.0));
                    sum += board(q.x, q.y);
                }
            }
            let v = srgb(sum / 16.0);
            Rgba([v, v, v, 255])
        })
    }

    /// Positions (au sous-pixel) des bords noir/blanc le long d'un segment
    /// horizontal (`dx` = 1) ou vertical (`dx` = 0) partant de `from`.
    fn edges(img: &RgbaImage, from: Pt, len: usize, dx: u32) -> Vec<f64> {
        let (x0, y0) = (from.x.floor() as u32, from.y.floor() as u32);
        let v: Vec<f64> = (0..len as u32).map(|k| linear(img.get_pixel(x0 + k * dx, y0 + k * (1 - dx))[0])).collect();
        let start = if dx == 1 { x0 } else { y0 } as f64;
        v.windows(2)
            .enumerate()
            .filter(|(_, w)| (w[0] - 0.5) * (w[1] - 0.5) < 0.0)
            // Le pixel k a son centre en k + 0,5.
            .map(|(k, w)| start + k as f64 + 0.5 + (0.5 - w[0]) / (w[1] - w[0]))
            .collect()
    }

    #[test]
    fn checkerboard_measures_true_after_correction() {
        meteroscope_i18n::set_language("fr");
        let corners = corners_in_photo();
        let photo = photo(&corners);
        // De biais, les cases ne sont ni carrées ni égales dans la photo.
        assert!((corners[0].dist(corners[1]) - corners[3].dist(corners[2])).abs() > 20.0);

        for filter in RotationFilter::ALL {
            // Coins cliqués dans le désordre, dimensions réelles saisies.
            let mut doc = Document::new();
            let diag = doc.add(Shape::Length { a: corners[0], b: corners[2] });
            assert!(doc.correct_perspective([corners[2], corners[0], corners[1], corners[3]], PHOTO, Some((BOARD.0, BOARD.1, "mm"))));
            let p = doc.perspective.clone().unwrap();
            let img = warp_perspective(&photo, &p, filter).unwrap();
            assert_eq!(img.dimensions(), p.size);
            let upp = doc.calibration.scale.as_ref().unwrap().units_per_px;
            let (origin, px) = (p.dst[0], SQUARE / upp);
            assert!(px > 40.0, "résolution conservée : {px} px par case");

            // La mesure posée avant la correction donne la diagonale réelle.
            let d = doc.get(diag).unwrap().shape.values(&doc.calibration)[0].value;
            assert!((d - BOARD.0.hypot(BOARD.1)).abs() < 1e-6, "{d}");

            // Bords des cases le long d'une rangée puis d'une colonne, mesurés
            // dans l'image corrigée : tous les 50 mm, à 0,5 mm près. (La rangée
            // finit par une case blanche, la colonne commence par une blanche.)
            let margin = 0.4 * px;
            let row = edges(&img, origin.offset(-margin, 2.5 * px), (BOARD.0 / upp + 2.0 * margin) as usize, 1);
            let col = edges(&img, origin.offset(3.5 * px, -margin), (BOARD.1 / upp + 2.0 * margin) as usize, 0);
            assert_eq!((row.len(), col.len()), (8, 6), "{filter:?} : {row:?} {col:?}");
            for (k, x) in row.iter().enumerate() {
                let mm = (x - origin.x) * upp;
                assert!((mm - k as f64 * SQUARE).abs() < 0.5, "{filter:?} : bord {k} à {mm} mm");
            }
            for (k, y) in col.iter().enumerate() {
                let mm = (y - origin.y) * upp;
                assert!((mm - (k + 1) as f64 * SQUARE).abs() < 0.5, "{filter:?} : bord {k} à {mm} mm");
            }
            // Une distance entre deux bords éloignés : 6 cases = 300 mm.
            assert!(((row[7] - row[1]) * upp - 300.0).abs() < 0.5);
        }
    }

    #[test]
    fn checkerboard_is_square_without_real_dimensions() {
        meteroscope_i18n::set_language("fr");
        let corners = corners_in_photo();
        let photo = photo(&corners);
        let mut doc = Document::new();
        assert!(doc.correct_perspective(corners, PHOTO, None));
        assert!(!doc.calibration.is_calibrated(), "sans dimensions, l'échelle reste à définir");
        let p = doc.perspective.clone().unwrap();
        // Rapport d'aspect estimé d'après la seule perspective.
        let (w, h) = p.rect_size();
        assert!((w / h - BOARD.0 / BOARD.1).abs() < 1e-6, "{}", w / h);
        let img = warp_perspective(&photo, &p, RotationFilter::Lanczos3).unwrap();
        let px = w / 8.0;
        let row = edges(&img, p.dst[0].offset(-0.4 * px, 2.5 * px), (w + 0.8 * px) as usize, 1);
        let col = edges(&img, p.dst[0].offset(3.5 * px, -0.4 * px), (h + 0.8 * px) as usize, 0);
        assert_eq!((row.len(), col.len()), (8, 6));
        // Les cases sont redevenues carrées et égales.
        for w in row.windows(2).chain(col.windows(2)) {
            assert!((w[1] - w[0] - px).abs() < 0.6, "case de {} px au lieu de {px}", w[1] - w[0]);
        }
    }

    #[test]
    fn outside_the_photo_is_transparent() {
        // Vue de face : la correction ne change rien à l'image.
        let img = RgbaImage::from_fn(40, 30, |x, y| Rgba([(x * 6) as u8, (y * 8) as u8, 90, 255]));
        let q = [Pt::new(10.0, 5.0), Pt::new(30.0, 5.0), Pt::new(30.0, 25.0), Pt::new(10.0, 25.0)];
        let p = Perspective::from_quad(q, (40, 30), None).unwrap();
        for f in RotationFilter::ALL {
            assert_eq!(warp_perspective(&img, &p, f).unwrap(), img, "{f:?}");
        }
        // Plan très incliné : la ligne de fuite traverse la photo, rien ne
        // déborde ni ne se replie de l'autre côté.
        let q = [Pt::new(18.0, 14.0), Pt::new(22.0, 14.0), Pt::new(30.0, 28.0), Pt::new(10.0, 28.0)];
        let p = Perspective::from_quad(q, (40, 30), None).unwrap();
        let out = warp_perspective(&img, &p, RotationFilter::Bicubic).unwrap();
        assert_eq!(out.dimensions(), p.size);
        assert!(out.pixels().any(|px| px[3] == 255) && out.pixels().all(|px| px[3] == 0 || px[2].abs_diff(90) <= 2));
        // Entrées inutilisables.
        assert!(warp_perspective(&RgbaImage::new(0, 0), &p, RotationFilter::Bicubic).is_none());
        let flat = Perspective { src: [Pt::new(1.0, 1.0); 4], ..p };
        assert!(warp_perspective(&img, &flat, RotationFilter::Bicubic).is_none());
    }

    #[test]
    fn quarter_turn_keeps_the_correction() {
        meteroscope_i18n::set_language("fr");
        let corners = corners_in_photo();
        let photo = photo(&corners);
        let mut doc = Document::new();
        doc.add(Shape::Length { a: corners[0], b: corners[1] });
        assert!(doc.correct_perspective(corners, PHOTO, Some((BOARD.0, BOARD.1, "mm"))));
        let before = warp_perspective(&photo, doc.perspective.as_ref().unwrap(), RotationFilter::Bicubic).unwrap();
        let value = |d: &Document| d.measurements[0].shape.values(&d.calibration)[0].value;
        for t in [crate::Transform::RotateRight, crate::Transform::FlipHorizontal] {
            let mut turned = doc.clone();
            let photo2 = crate::apply_transform(&photo, &mut turned, t);
            let p = turned.perspective.clone().unwrap();
            let after = warp_perspective(&photo2, &p, RotationFilter::Bicubic).unwrap();
            // L'image corrigée subit la même transformation que la photo…
            let expected = match t {
                crate::Transform::RotateRight => image::imageops::rotate90(&before),
                _ => image::imageops::flip_horizontal(&before),
            };
            assert_eq!(after.dimensions(), expected.dimensions());
            let worst = after.pixels().zip(expected.pixels()).map(|(a, b)| a[0].abs_diff(b[0])).max().unwrap();
            assert!(worst <= 2, "{t:?} : écart {worst}");
            // … et la mesure garde sa valeur.
            assert!((value(&turned) - BOARD.0).abs() < 1e-6);
        }
    }
}

//! Rotations / miroirs (image + mesures) et réglage luminosité/contraste.

use crate::Transform;
use image::{RgbaImage, imageops};
use meteroscope_core::{Document, Pt};

/// Image du point `p` (coordonnées continues : le pixel (i, j) couvre
/// [i, i+1] × [j, j+1]) pour une image de taille `w`×`h` *avant* transformation.
pub fn transform_point(p: Pt, w: f64, h: f64, t: Transform) -> Pt {
    match t {
        // Sens horaire : le bord gauche devient le haut.
        Transform::RotateRight => Pt::new(h - p.y, p.x),
        Transform::RotateLeft => Pt::new(p.y, w - p.x),
        Transform::FlipHorizontal => Pt::new(w - p.x, p.y),
        Transform::FlipVertical => Pt::new(p.x, h - p.y),
    }
}

pub fn apply_transform(image: &RgbaImage, doc: &mut Document, t: Transform) -> RgbaImage {
    let out = match t {
        Transform::RotateRight => imageops::rotate90(image),
        Transform::RotateLeft => imageops::rotate270(image),
        Transform::FlipHorizontal => imageops::flip_horizontal(image),
        Transform::FlipVertical => imageops::flip_vertical(image),
    };
    let (w, h) = (image.width() as f64, image.height() as f64);
    if !doc.measurements.is_empty() {
        doc.checkpoint();
        for m in &mut doc.measurements {
            m.shape.map_points(|p| transform_point(p, w, h, t));
        }
    }
    out
}

/// Table de correspondance luminosité/contraste.
fn lut(brightness: f32, contrast: f32) -> [u8; 256] {
    let b = brightness.clamp(-1.0, 1.0) * 0.5;
    // Pente : 0 (gris uniforme) à -1, 1 à 0, très forte vers +1.
    let c = contrast.clamp(-1.0, 0.98);
    let slope = ((c + 1.0) * std::f32::consts::FRAC_PI_4).tan();
    let mut t = [0u8; 256];
    for (i, v) in t.iter_mut().enumerate() {
        let x = i as f32 / 255.0;
        let y = (x - 0.5) * slope + 0.5 + b;
        *v = (y * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    t
}

pub fn adjust(image: &RgbaImage, brightness: f32, contrast: f32) -> RgbaImage {
    let mut out = image.clone();
    if brightness == 0.0 && contrast == 0.0 {
        return out;
    }
    let t = lut(brightness, contrast);
    let apply = |chunk: &mut [u8]| {
        for px in chunk.as_chunks_mut::<4>().0 {
            px[0] = t[px[0] as usize];
            px[1] = t[px[1] as usize];
            px[2] = t[px[2] as usize];
        }
    };
    let buf: &mut [u8] = &mut out;
    // Découpage en quelques fils pour les grandes photos (curseur fluide).
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).min(4);
    if threads > 1 && buf.len() > 1 << 20 {
        let per = buf.len().div_ceil(threads * 4) * 4;
        std::thread::scope(|s| {
            for chunk in buf.chunks_mut(per) {
                s.spawn(move || apply(chunk));
            }
        });
    } else {
        apply(buf);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    use meteroscope_core::Shape;

    fn test_image(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| Rgba([(x * 7) as u8, (y * 13) as u8, ((x * y) % 251) as u8, 200]))
    }

    #[test]
    fn transforms_follow_pixels() {
        meteroscope_i18n::set_language("fr");
        let (w, h) = (7u32, 4u32);
        let img = test_image(w, h);
        // Repère : centre du pixel (5, 1), marqué d'une couleur unique.
        let mut img2 = img.clone();
        img2.put_pixel(5, 1, Rgba([1, 2, 3, 4]));
        for t in [Transform::RotateLeft, Transform::RotateRight, Transform::FlipHorizontal, Transform::FlipVertical] {
            let mut doc = Document::new();
            doc.add(Shape::Marker { p: Pt::new(5.5, 1.5) });
            let undo_before = doc.can_undo();
            let out = apply_transform(&img2, &mut doc, t);
            assert!(undo_before && doc.can_undo());
            let Shape::Marker { p } = doc.measurements[0].shape else { panic!() };
            assert_eq!(*out.get_pixel(p.x.floor() as u32, p.y.floor() as u32), Rgba([1, 2, 3, 4]), "{t:?}");
            // Dimensions échangées pour une rotation.
            let rot = matches!(t, Transform::RotateLeft | Transform::RotateRight);
            assert_eq!(out.dimensions(), if rot { (h, w) } else { (w, h) });
            assert!(doc.undo());
            assert_eq!(doc.measurements[0].shape, Shape::Marker { p: Pt::new(5.5, 1.5) });
        }
    }

    #[test]
    fn four_rotations_identity_and_values_kept() {
        meteroscope_i18n::set_language("fr");
        let img = test_image(9, 5);
        let mut doc = Document::new();
        doc.add(Shape::Length { a: Pt::new(1.0, 2.0), b: Pt::new(8.0, 4.5) });
        doc.add(Shape::Angle { a: Pt::new(1.0, 1.0), vertex: Pt::new(4.0, 2.0), b: Pt::new(2.0, 4.0) });
        doc.add(Shape::Rect { a: Pt::new(1.0, 1.0), b: Pt::new(6.0, 3.0) });
        doc.add(Shape::Circle3 { a: Pt::new(1.0, 1.0), b: Pt::new(4.0, 0.5), c: Pt::new(3.0, 4.0) });
        let before: Vec<_> = doc.measurements.iter().map(|m| m.shape.values(&doc.calibration)).collect();
        let shapes0: Vec<_> = doc.measurements.iter().map(|m| m.shape.clone()).collect();
        for t in [Transform::RotateRight, Transform::RotateLeft] {
            let mut im = img.clone();
            for _ in 0..4 {
                im = apply_transform(&im, &mut doc, t);
                for (m, v) in doc.measurements.iter().zip(&before) {
                    // Valeurs triées : un quart de tour échange largeur et hauteur d'un rectangle.
                    let sorted = |vals: &[meteroscope_core::Value]| {
                        let mut x: Vec<f64> = vals.iter().map(|v| v.value).collect();
                        x.sort_by(f64::total_cmp);
                        x
                    };
                    let (a, b) = (sorted(&m.shape.values(&doc.calibration)), sorted(v));
                    assert_eq!(a.len(), b.len());
                    assert!(a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-9), "{:?}", m.shape);
                }
            }
            assert_eq!(im, img);
            for (m, s) in doc.measurements.iter().zip(&shapes0) {
                for (p, q) in m.shape.points().iter().zip(s.points()) {
                    assert!(p.dist(q) < 1e-12);
                }
            }
        }
        let mut im = img.clone();
        for t in [Transform::FlipHorizontal, Transform::FlipHorizontal, Transform::FlipVertical, Transform::FlipVertical] {
            im = apply_transform(&im, &mut doc, t);
        }
        assert_eq!(im, img);
        // Gauche puis droite = identité.
        let im = apply_transform(&apply_transform(&img, &mut doc, Transform::RotateLeft), &mut doc, Transform::RotateRight);
        assert_eq!(im, img);
    }

    #[test]
    fn adjust_identity_and_alpha() {
        meteroscope_i18n::set_language("fr");
        let img = test_image(64, 64);
        assert_eq!(adjust(&img, 0.0, 0.0), img);
        assert_eq!(lut(0.0, 0.0).to_vec(), (0..=255u8).collect::<Vec<_>>());
        let brighter = adjust(&img, 0.5, 0.0);
        let darker = adjust(&img, -0.5, 0.3);
        for ((a, b), c) in img.pixels().zip(brighter.pixels()).zip(darker.pixels()) {
            assert_eq!((a[3], b[3], c[3]), (a[3], a[3], a[3]));
            assert!(b[0] >= a[0]);
        }
        // Contraste minimal : gris uniforme.
        assert!(lut(0.0, -1.0).iter().all(|&v| v == 128));
        // Grande image (chemin multi-fil) identique au chemin simple.
        let big = test_image(800, 600);
        let small_path: Vec<u8> = big.as_raw().iter().enumerate().map(|(i, &v)| if i % 4 == 3 { v } else { lut(0.2, 0.4)[v as usize] }).collect();
        assert_eq!(adjust(&big, 0.2, 0.4).into_raw(), small_path);
    }
}

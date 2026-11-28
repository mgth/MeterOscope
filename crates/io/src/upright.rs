//! Vue redressée pour les exports : l'image pivotée de `Document::rotation`
//! dans sa boîte englobante, comme à l'écran et comme [`crate::rotate_image`].

use meteroscope_core::{Document, Pt, Shape};

/// Rotation de `deg` degrés (sens horaire) autour du centre de l'image
/// `w` × `h`, puis translation dans la boîte englobante : p' = R·p + t.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub deg: f64,
    pub cos: f64,
    pub sin: f64,
    pub t: (f64, f64),
    /// Taille exacte de la boîte englobante.
    pub vw: f64,
    pub vh: f64,
}

impl Frame {
    pub fn new(w: f64, h: f64, deg: f64) -> Frame {
        let (sin, cos) = if deg == 0.0 { (0.0, 1.0) } else { deg.to_radians().sin_cos() };
        let vw = cos.abs() * w + sin.abs() * h;
        let vh = sin.abs() * w + cos.abs() * h;
        let (cx, cy) = (w / 2.0, h / 2.0);
        let t = (vw / 2.0 - (cos * cx - sin * cy), vh / 2.0 - (sin * cx + cos * cy));
        Frame { deg, cos, sin, t, vw, vh }
    }

    pub fn apply(&self, p: Pt) -> Pt {
        Pt::new(self.cos * p.x - self.sin * p.y + self.t.0, self.sin * p.x + self.cos * p.y + self.t.1)
    }

    /// Matrice SVG `matrix(a b c d e f)` de la transformation.
    pub fn svg_matrix(&self) -> [f64; 6] {
        [self.cos, self.sin, -self.sin, self.cos, self.t.0, self.t.1]
    }
}

/// Copie du document dans le repère de l'image redressée : points pivotés,
/// rectangles réorientés, rotation remise à zéro. Les valeurs mesurées ne
/// changent pas (rotation rigide).
pub fn upright_document(doc: &Document, w: f64, h: f64) -> Document {
    let mut out = doc.clone();
    if doc.rotation == 0.0 {
        return out;
    }
    let f = Frame::new(w, h, doc.rotation);
    for m in &mut out.measurements {
        m.shape.map_points(|p| f.apply(p));
        if let Shape::Rect { angle, .. } = &m.shape {
            let a = *angle + doc.rotation;
            m.shape = m.shape.clone().oriented(a);
        }
    }
    out.rotation = 0.0;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repere_redresse() {
        meteroscope_i18n::set_language("fr");
        // Quart de tour horaire d'une image 200 × 100 : boîte 100 × 200,
        // le coin haut-gauche part en haut à droite.
        let f = Frame::new(200.0, 100.0, 90.0);
        assert!((f.vw - 100.0).abs() < 1e-9 && (f.vh - 200.0).abs() < 1e-9);
        assert!(f.apply(Pt::new(0., 0.)).dist(Pt::new(100., 0.)) < 1e-9);
        assert!(f.apply(Pt::new(200., 100.)).dist(Pt::new(0., 200.)) < 1e-9);

        let mut doc = Document::new();
        doc.add(Shape::Rect { a: Pt::new(10., 10.), b: Pt::new(50., 30.), angle: 0.0 });
        doc.add(Shape::Length { a: Pt::new(0., 0.), b: Pt::new(30., 40.) });
        doc.rotation = 90.0;
        let up = upright_document(&doc, 200.0, 100.0);
        assert_eq!(up.rotation, 0.0);
        for (m, n) in doc.measurements.iter().zip(&up.measurements) {
            let (v, w) = (m.shape.values(&doc.calibration), n.shape.values(&up.calibration));
            for (a, b) in v.iter().zip(&w) {
                assert!((a.value - b.value).abs() < 1e-9, "{a:?} {b:?}");
            }
        }
        let Shape::Rect { angle, .. } = up.measurements[0].shape else { panic!() };
        assert_eq!(angle, 90.0);
    }
}

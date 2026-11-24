use meteroscope_i18n::tr;
use crate::calibration::Calibration;
use crate::geometry::{self as geo, Pt};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// Couleurs proposées pour les mesures (RGB), bien visibles sur une photo.
pub const PALETTE: [[u8; 3]; 8] = [
    [255, 214, 10],
    [255, 69, 58],
    [48, 209, 88],
    [10, 132, 255],
    [255, 159, 10],
    [191, 90, 242],
    [100, 210, 255],
    [255, 255, 255],
];

/// Type de mesure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ShapeKind {
    Length,
    Path,
    Angle,
    Area,
    Rect,
    Circle,
    Circle3,
    Marker,
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 8] = [
        ShapeKind::Length,
        ShapeKind::Path,
        ShapeKind::Angle,
        ShapeKind::Area,
        ShapeKind::Rect,
        ShapeKind::Circle,
        ShapeKind::Circle3,
        ShapeKind::Marker,
    ];

    /// Nom affiché (français).
    pub fn label(self) -> &'static str {
        match self {
            ShapeKind::Length => tr!("shape.length"),
            ShapeKind::Path => tr!("shape.path"),
            ShapeKind::Angle => tr!("shape.angle"),
            ShapeKind::Area => tr!("shape.area"),
            ShapeKind::Rect => tr!("shape.rect"),
            ShapeKind::Circle => tr!("shape.circle"),
            ShapeKind::Circle3 => tr!("shape.circle3"),
            ShapeKind::Marker => tr!("shape.marker"),
        }
    }

    /// Nombre de points fixe nécessaire, ou `None` si variable (Path, Area :
    /// terminé par double-clic / Entrée).
    pub fn point_count(self) -> Option<usize> {
        match self {
            ShapeKind::Length | ShapeKind::Rect | ShapeKind::Circle => Some(2),
            ShapeKind::Angle | ShapeKind::Circle3 => Some(3),
            ShapeKind::Marker => Some(1),
            ShapeKind::Path | ShapeKind::Area => None,
        }
    }

    /// Nombre minimal de points pour une forme valide.
    pub fn min_points(self) -> usize {
        match self {
            ShapeKind::Path => 2,
            ShapeKind::Area => 3,
            k => k.point_count().unwrap_or(1),
        }
    }
}

/// Outil actif dans la vue image.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tool {
    /// Sélectionner, déplacer, modifier les mesures ; déplacer la vue.
    Select,
    /// Tracer un segment de référence pour définir l'échelle.
    Calibrate,
    /// Créer une mesure du type donné.
    Draw(ShapeKind),
}

/// Géométrie d'une mesure, en pixels image.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    Length { a: Pt, b: Pt },
    /// Ligne brisée ouverte.
    Path { pts: Vec<Pt> },
    /// Angle au sommet `vertex`.
    Angle { a: Pt, vertex: Pt, b: Pt },
    /// Polygone fermé.
    Area { pts: Vec<Pt> },
    /// Rectangle aligné sur les axes, coins opposés.
    Rect { a: Pt, b: Pt },
    /// Cercle défini par son centre et un point du bord.
    Circle { center: Pt, edge: Pt },
    /// Cercle passant par trois points.
    Circle3 { a: Pt, b: Pt, c: Pt },
    Marker { p: Pt },
}

impl Shape {
    /// Construit une forme à partir de ses points de contrôle, dans l'ordre
    /// de [`Shape::points`]. `None` si le nombre de points ne convient pas.
    pub fn from_points(kind: ShapeKind, p: &[Pt]) -> Option<Shape> {
        if p.len() < kind.min_points() || kind.point_count().is_some_and(|n| n != p.len()) {
            return None;
        }
        Some(match kind {
            ShapeKind::Length => Shape::Length { a: p[0], b: p[1] },
            ShapeKind::Path => Shape::Path { pts: p.to_vec() },
            ShapeKind::Angle => Shape::Angle { a: p[0], vertex: p[1], b: p[2] },
            ShapeKind::Area => Shape::Area { pts: p.to_vec() },
            ShapeKind::Rect => Shape::Rect { a: p[0], b: p[1] },
            ShapeKind::Circle => Shape::Circle { center: p[0], edge: p[1] },
            ShapeKind::Circle3 => Shape::Circle3 { a: p[0], b: p[1], c: p[2] },
            ShapeKind::Marker => Shape::Marker { p: p[0] },
        })
    }

    pub fn kind(&self) -> ShapeKind {
        match self {
            Shape::Length { .. } => ShapeKind::Length,
            Shape::Path { .. } => ShapeKind::Path,
            Shape::Angle { .. } => ShapeKind::Angle,
            Shape::Area { .. } => ShapeKind::Area,
            Shape::Rect { .. } => ShapeKind::Rect,
            Shape::Circle { .. } => ShapeKind::Circle,
            Shape::Circle3 { .. } => ShapeKind::Circle3,
            Shape::Marker { .. } => ShapeKind::Marker,
        }
    }

    /// Points de contrôle (poignées), dans l'ordre de saisie.
    pub fn points(&self) -> Vec<Pt> {
        match self {
            Shape::Length { a, b } | Shape::Rect { a, b } => vec![*a, *b],
            Shape::Path { pts } | Shape::Area { pts } => pts.clone(),
            Shape::Angle { a, vertex, b } => vec![*a, *vertex, *b],
            Shape::Circle { center, edge } => vec![*center, *edge],
            Shape::Circle3 { a, b, c } => vec![*a, *b, *c],
            Shape::Marker { p } => vec![*p],
        }
    }

    /// Accès mutable aux points de contrôle, même ordre que [`Shape::points`].
    pub fn points_mut(&mut self) -> Vec<&mut Pt> {
        match self {
            Shape::Length { a, b } | Shape::Rect { a, b } => vec![a, b],
            Shape::Path { pts } | Shape::Area { pts } => pts.iter_mut().collect(),
            Shape::Angle { a, vertex, b } => vec![a, vertex, b],
            Shape::Circle { center, edge } => vec![center, edge],
            Shape::Circle3 { a, b, c } => vec![a, b, c],
            Shape::Marker { p } => vec![p],
        }
    }

    /// Applique une transformation à tous les points (déplacement, rotation d'image…).
    pub fn map_points(&mut self, f: impl Fn(Pt) -> Pt) {
        for p in self.points_mut() {
            *p = f(*p);
        }
    }

    pub fn translate(&mut self, dx: f64, dy: f64) {
        self.map_points(|p| p.offset(dx, dy));
    }

    /// Centre et rayon (pixels) pour les deux types de cercle.
    pub fn circle(&self) -> Option<(Pt, f64)> {
        match self {
            Shape::Circle { center, edge } => Some((*center, center.dist(*edge))),
            Shape::Circle3 { a, b, c } => geo::circle_from_3_points(*a, *b, *c),
            _ => None,
        }
    }

    /// Valeurs mesurées, converties selon l'étalonnage. La première est la
    /// valeur principale (celle affichée sur l'image).
    pub fn values(&self, cal: &Calibration) -> Vec<Value> {
        let len = |q, px| Value { quantity: q, value: cal.length(px), unit: cal.length_unit().to_owned() };
        let area = |px2| Value { quantity: Quantity::Area, value: cal.area(px2), unit: cal.area_unit() };
        match self {
            Shape::Length { a, b } => vec![len(Quantity::Length, a.dist(*b))],
            Shape::Path { pts } => vec![len(Quantity::Length, geo::polyline_length(pts))],
            Shape::Angle { a, vertex, b } => vec![Value {
                quantity: Quantity::Angle,
                value: geo::angle_deg(*a, *vertex, *b),
                unit: "°".to_owned(),
            }],
            Shape::Area { pts } => {
                vec![area(geo::polygon_area(pts)), len(Quantity::Perimeter, geo::polygon_perimeter(pts))]
            }
            Shape::Rect { a, b } => {
                let (w, h) = ((a.x - b.x).abs(), (a.y - b.y).abs());
                vec![
                    area(w * h),
                    len(Quantity::Width, w),
                    len(Quantity::Height, h),
                    len(Quantity::Perimeter, 2.0 * (w + h)),
                ]
            }
            Shape::Circle { .. } | Shape::Circle3 { .. } => match self.circle() {
                Some((_, r)) => vec![
                    len(Quantity::Diameter, 2.0 * r),
                    len(Quantity::Radius, r),
                    area(PI * r * r),
                    len(Quantity::Perimeter, 2.0 * PI * r),
                ],
                None => vec![],
            },
            Shape::Marker { .. } => vec![],
        }
    }
}

/// Grandeur mesurée.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Quantity {
    Length,
    Angle,
    Area,
    Perimeter,
    Width,
    Height,
    Diameter,
    Radius,
}

impl Quantity {
    pub fn label(self) -> &'static str {
        match self {
            Quantity::Length => tr!("quantity.length"),
            Quantity::Angle => tr!("quantity.angle"),
            Quantity::Area => tr!("quantity.area"),
            Quantity::Perimeter => tr!("quantity.perimeter"),
            Quantity::Width => tr!("quantity.width"),
            Quantity::Height => tr!("quantity.height"),
            Quantity::Diameter => tr!("quantity.diameter"),
            Quantity::Radius => tr!("quantity.radius"),
        }
    }
}

/// Une valeur mesurée, déjà convertie dans l'unité d'affichage.
#[derive(Clone, Debug, PartialEq)]
pub struct Value {
    pub quantity: Quantity,
    pub value: f64,
    pub unit: String,
}

impl Value {
    /// Texte court, p. ex. « 12,35 mm » (4 chiffres significatifs environ).
    pub fn text(&self) -> String {
        format!("{} {}", format_number(self.value), self.unit)
    }
}

/// Formate un nombre avec une précision adaptée à sa grandeur, séparateur décimal de la langue.
pub fn format_number(v: f64) -> String {
    let a = v.abs();
    let decimals = if a >= 1000.0 {
        0
    } else if a >= 100.0 {
        1
    } else if a >= 1.0 {
        2
    } else {
        3
    };
    let s = format!("{v:.decimals$}");
    match meteroscope_i18n::decimal_separator() {
        '.' => s,
        sep => s.replace('.', &sep.to_string()),
    }
}

/// Une mesure posée sur l'image.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Measurement {
    pub id: u64,
    pub name: String,
    pub shape: Shape,
    pub color: [u8; 3],
    pub visible: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_and_calibration() {
        meteroscope_i18n::set_language("fr");
        let mut cal = Calibration::default();
        let s = Shape::Length { a: Pt::new(0., 0.), b: Pt::new(30., 40.) };
        assert_eq!(s.values(&cal)[0].text(), "50,00 px");
        assert!(cal.set_from_reference(100.0, 10.0, "mm"));
        assert_eq!(s.values(&cal)[0].text(), "5,00 mm");
        let r = Shape::Rect { a: Pt::new(0., 0.), b: Pt::new(100., 50.) };
        let v = r.values(&cal);
        assert_eq!((v[0].value, v[0].unit.as_str()), (50.0, "mm²"));
        assert!(Shape::from_points(ShapeKind::Angle, &[Pt::default(); 2]).is_none());
        for k in ShapeKind::ALL {
            let n = k.min_points();
            let pts: Vec<Pt> = (0..n).map(|i| Pt::new(i as f64, (i * i) as f64)).collect();
            let s = Shape::from_points(k, &pts).unwrap();
            assert_eq!((s.kind(), s.points()), (k, pts));
        }
    }
}

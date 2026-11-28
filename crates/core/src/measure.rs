use meteroscope_i18n::tr;
use crate::calibration::Calibration;
use crate::geometry::{self as geo, Pt};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

fn is_zero(v: &f64) -> bool {
    *v == 0.0
}

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
    /// Inclinaison entre deux segments distants.
    LineAngle,
    /// Rectangle tracé par trois points (un côté, puis la largeur) : outil
    /// de tracé seulement, il produit un [`Shape::Rect`] incliné.
    Rect3,
    /// Annotation : texte libre posé sur l'image (pas une mesure).
    Text,
    /// Annotation : flèche, avec un texte facultatif à sa base (pas une mesure).
    Arrow,
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 12] = [
        ShapeKind::Length,
        ShapeKind::Path,
        ShapeKind::Angle,
        ShapeKind::Area,
        ShapeKind::Rect,
        ShapeKind::Circle,
        ShapeKind::Circle3,
        ShapeKind::Marker,
        ShapeKind::LineAngle,
        ShapeKind::Rect3,
        ShapeKind::Text,
        ShapeKind::Arrow,
    ];

    /// Annotation (texte, flèche) : ni valeurs, ni statistiques, ni ligne au CSV.
    pub fn is_annotation(self) -> bool {
        matches!(self, ShapeKind::Text | ShapeKind::Arrow)
    }

    /// Type de la mesure produite par ce tracé (Rect3 → Rect).
    pub fn produces(self) -> ShapeKind {
        match self {
            ShapeKind::Rect3 => ShapeKind::Rect,
            k => k,
        }
    }

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
            ShapeKind::LineAngle => tr!("shape.line_angle"),
            ShapeKind::Rect3 => tr!("shape.rect3"),
            ShapeKind::Text => tr!("shape.text"),
            ShapeKind::Arrow => tr!("shape.arrow"),
        }
    }

    /// Nombre de points fixe nécessaire, ou `None` si variable (Path, Area :
    /// terminé par double-clic / Entrée).
    pub fn point_count(self) -> Option<usize> {
        match self {
            ShapeKind::Length | ShapeKind::Rect | ShapeKind::Circle | ShapeKind::Arrow => Some(2),
            ShapeKind::Angle | ShapeKind::Circle3 | ShapeKind::Rect3 => Some(3),
            ShapeKind::Marker | ShapeKind::Text => Some(1),
            ShapeKind::LineAngle => Some(4),
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
    /// Tracer un segment qui doit être horizontal ou vertical : l'affichage
    /// de la photo pivote en conséquence (l'image n'est pas modifiée).
    Straighten,
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
    /// Rectangle de coins opposés `a`, `b`, côtés orientés à `angle` degrés
    /// dans l'image (0 : alignés sur les bords ; tracé sur une vue redressée,
    /// il suit l'horizontale et la verticale de la vue).
    Rect {
        a: Pt,
        b: Pt,
        #[serde(default, skip_serializing_if = "is_zero")]
        angle: f64,
    },
    /// Cercle défini par son centre et un point du bord.
    Circle { center: Pt, edge: Pt },
    /// Cercle passant par trois points.
    Circle3 { a: Pt, b: Pt, c: Pt },
    Marker { p: Pt },
    /// Inclinaison entre le segment `a→b` et le segment `c→d`.
    LineAngle { a: Pt, b: Pt, c: Pt, d: Pt },
    /// Annotation : texte (une ou plusieurs lignes) centré sur `p`.
    Text { p: Pt, text: String },
    /// Annotation : flèche de la base `a` vers la pointe `b`, texte facultatif à la base.
    Arrow {
        a: Pt,
        b: Pt,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        text: String,
    },
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
            ShapeKind::Rect => Shape::Rect { a: p[0], b: p[1], angle: 0.0 },
            ShapeKind::Circle => Shape::Circle { center: p[0], edge: p[1] },
            ShapeKind::Circle3 => Shape::Circle3 { a: p[0], b: p[1], c: p[2] },
            ShapeKind::Marker => Shape::Marker { p: p[0] },
            ShapeKind::LineAngle => Shape::LineAngle { a: p[0], b: p[1], c: p[2], d: p[3] },
            ShapeKind::Rect3 => {
                let (a, b, angle) = geo::rect_from_3_points(p[0], p[1], p[2]);
                Shape::Rect { a, b, angle }
            }
            ShapeKind::Text => Shape::Text { p: p[0], text: String::new() },
            ShapeKind::Arrow => Shape::Arrow { a: p[0], b: p[1], text: String::new() },
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
            Shape::LineAngle { .. } => ShapeKind::LineAngle,
            Shape::Text { .. } => ShapeKind::Text,
            Shape::Arrow { .. } => ShapeKind::Arrow,
        }
    }

    /// Points de contrôle (poignées), dans l'ordre de saisie.
    pub fn points(&self) -> Vec<Pt> {
        match self {
            Shape::Length { a, b } | Shape::Rect { a, b, .. } | Shape::Arrow { a, b, .. } => vec![*a, *b],
            Shape::Path { pts } | Shape::Area { pts } => pts.clone(),
            Shape::Angle { a, vertex, b } => vec![*a, *vertex, *b],
            Shape::Circle { center, edge } => vec![*center, *edge],
            Shape::Circle3 { a, b, c } => vec![*a, *b, *c],
            Shape::Marker { p } | Shape::Text { p, .. } => vec![*p],
            Shape::LineAngle { a, b, c, d } => vec![*a, *b, *c, *d],
        }
    }

    /// Accès mutable aux points de contrôle, même ordre que [`Shape::points`].
    pub fn points_mut(&mut self) -> Vec<&mut Pt> {
        match self {
            Shape::Length { a, b } | Shape::Rect { a, b, .. } | Shape::Arrow { a, b, .. } => vec![a, b],
            Shape::Path { pts } | Shape::Area { pts } => pts.iter_mut().collect(),
            Shape::Angle { a, vertex, b } => vec![a, vertex, b],
            Shape::Circle { center, edge } => vec![center, edge],
            Shape::Circle3 { a, b, c } => vec![a, b, c],
            Shape::Marker { p } | Shape::Text { p, .. } => vec![p],
            Shape::LineAngle { a, b, c, d } => vec![a, b, c, d],
        }
    }

    /// Applique une transformation à tous les points (déplacement, rotation d'image…).
    pub fn map_points(&mut self, f: impl Fn(Pt) -> Pt) {
        for p in self.points_mut() {
            *p = f(*p);
        }
    }

    /// Oriente un rectangle à `deg` degrés (sans effet sur les autres formes).
    pub fn oriented(mut self, deg: f64) -> Self {
        if let Shape::Rect { angle, .. } = &mut self {
            *angle = geo::normalize_deg(deg);
            if angle.abs() < 1e-9 {
                *angle = 0.0;
            }
        }
        self
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

    /// Texte d'une annotation (vide pour une flèche sans texte), `None` pour une mesure.
    pub fn annotation_text(&self) -> Option<&str> {
        match self {
            Shape::Text { text, .. } | Shape::Arrow { text, .. } => Some(text),
            _ => None,
        }
    }

    /// Accès mutable au texte d'une annotation.
    pub fn annotation_text_mut(&mut self) -> Option<&mut String> {
        match self {
            Shape::Text { text, .. } | Shape::Arrow { text, .. } => Some(text),
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
            Shape::Rect { a, b, angle } => {
                let (w, h) = geo::rect_size(*a, *b, *angle);
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
            Shape::Marker { .. } | Shape::Text { .. } | Shape::Arrow { .. } => vec![],
            Shape::LineAngle { a, b, c, d } => vec![Value {
                quantity: Quantity::Angle,
                value: geo::line_angle_deg(*a, *b, *c, *d),
                unit: "°".to_owned(),
            }],
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
        let r = Shape::Rect { a: Pt::new(0., 0.), b: Pt::new(100., 50.), angle: 0.0 };
        let v = r.values(&cal);
        assert_eq!((v[0].value, v[0].unit.as_str()), (50.0, "mm²"));
        // Orienté à 90° : largeur et hauteur échangées, même aire.
        let v = r.clone().oriented(90.0).values(&cal);
        assert!((v[0].value - 50.0).abs() < 1e-9 && (v[1].value - 5.0).abs() < 1e-9 && (v[2].value - 10.0).abs() < 1e-9);
        // L'angle nul n'est pas écrit dans le projet ; un ancien projet se relit.
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("angle"));
        assert_eq!(serde_json::from_str::<Shape>(&json).unwrap(), r);
        let la = Shape::LineAngle { a: Pt::new(0., 0.), b: Pt::new(10., 0.), c: Pt::new(50., 50.), d: Pt::new(60., 60.) };
        assert_eq!(la.values(&cal)[0].text(), "45,00 °");
        assert!(Shape::from_points(ShapeKind::Angle, &[Pt::default(); 2]).is_none());
        // Rectangle par trois points : côté (0,0)→(10,10), largeur jusqu'à (0,20).
        let r = Shape::from_points(ShapeKind::Rect3, &[Pt::new(0., 0.), Pt::new(10., 10.), Pt::new(0., 20.)]).unwrap();
        assert_eq!(r.kind(), ShapeKind::Rect);
        let v = r.values(&Calibration::default());
        assert!((v[1].value - 200f64.sqrt()).abs() < 1e-9 && (v[2].value - 200f64.sqrt()).abs() < 1e-9, "{v:?}");
        for k in ShapeKind::ALL {
            if k == ShapeKind::Rect3 {
                continue;
            }
            let n = k.min_points();
            let pts: Vec<Pt> = (0..n).map(|i| Pt::new(i as f64, (i * i) as f64)).collect();
            let s = Shape::from_points(k, &pts).unwrap();
            assert_eq!((s.kind(), s.points()), (k, pts));
        }
    }

    #[test]
    fn annotations() {
        meteroscope_i18n::set_language("fr");
        let cal = Calibration::default();
        let t = Shape::Text { p: Pt::new(1., 2.), text: "Fissure — 裂缝".to_owned() };
        let a = Shape::Arrow { a: Pt::new(0., 0.), b: Pt::new(10., 5.), text: String::new() };
        assert!(t.values(&cal).is_empty() && a.values(&cal).is_empty());
        assert!(t.kind().is_annotation() && a.kind().is_annotation() && !ShapeKind::Marker.is_annotation());
        assert_eq!(t.annotation_text(), Some("Fissure — 裂缝"));
        assert_eq!(Shape::Marker { p: Pt::default() }.annotation_text(), None);
        // Aller-retour JSON ; le texte vide d'une flèche n'est pas écrit.
        for s in [&t, &a] {
            let json = serde_json::to_string(s).unwrap();
            assert_eq!(&serde_json::from_str::<Shape>(&json).unwrap(), s);
        }
        assert!(!serde_json::to_string(&a).unwrap().contains("text"));
        let mut b = a.clone();
        *b.annotation_text_mut().unwrap() = "ici".to_owned();
        b.translate(1., 1.);
        assert_eq!(b, Shape::Arrow { a: Pt::new(1., 1.), b: Pt::new(11., 6.), text: "ici".to_owned() });
    }
}

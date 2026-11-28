use meteroscope_i18n::tr;
use crate::calibration::Calibration;
use crate::geometry::{self as geo, Pt};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;
use std::sync::atomic::{AtomicU8, Ordering};

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
    /// Distance d'un point à une droite.
    PointLine,
    /// Écart entre deux droites parallèles.
    Parallel,
    /// Ellipse par son centre et ses deux demi-axes.
    Ellipse,
    /// Arc de cercle par trois points.
    Arc,
    /// Profil d'intensité : largeur entre deux bords détectés au sous-pixel
    /// le long d'un segment (voir [`crate::profile`]).
    Profile,
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 15] = [
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
        ShapeKind::PointLine,
        ShapeKind::Parallel,
        ShapeKind::Ellipse,
        ShapeKind::Arc,
        ShapeKind::Profile,
    ];

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
            ShapeKind::PointLine => tr!("shape.point_line"),
            ShapeKind::Parallel => tr!("shape.parallel"),
            ShapeKind::Ellipse => tr!("shape.ellipse"),
            ShapeKind::Arc => tr!("shape.arc"),
            ShapeKind::Profile => tr!("shape.profile"),
        }
    }

    /// Nombre de points fixe nécessaire, ou `None` si variable (Path, Area :
    /// terminé par double-clic / Entrée).
    pub fn point_count(self) -> Option<usize> {
        match self {
            ShapeKind::Length | ShapeKind::Rect | ShapeKind::Circle | ShapeKind::Profile => Some(2),
            ShapeKind::Angle | ShapeKind::Circle3 | ShapeKind::Rect3 => Some(3),
            ShapeKind::PointLine | ShapeKind::Parallel | ShapeKind::Ellipse | ShapeKind::Arc => Some(3),
            ShapeKind::Marker => Some(1),
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
    /// Cliquer les quatre coins d'un objet rectangulaire photographié de
    /// biais : la photo est reprojetée pour le redresser (`Document::perspective`).
    Perspective,
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
    /// Distance du point `p` à la droite portant `a→b`.
    PointLine { a: Pt, b: Pt, p: Pt },
    /// Écart entre la droite `a→b` et sa parallèle passant par `p`.
    Parallel { a: Pt, b: Pt, p: Pt },
    /// Ellipse de centre `center` ; `a` et `b` : extrémités de ses deux
    /// demi-axes (perpendiculaires), ce qui fixe aussi son inclinaison.
    Ellipse { center: Pt, a: Pt, b: Pt },
    /// Arc de cercle d'extrémités `a`, `b`, passant par `m`.
    Arc { a: Pt, m: Pt, b: Pt },
    /// Profil d'intensité le long de `a→b`. `result` est un cache : il dépend
    /// des pixels et c'est l'application qui le recalcule.
    Profile {
        a: Pt,
        b: Pt,
        #[serde(default)]
        params: crate::profile::ProfileParams,
        #[serde(default)]
        result: Option<crate::profile::ProfileResult>,
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
            ShapeKind::PointLine => Shape::PointLine { a: p[0], b: p[1], p: p[2] },
            ShapeKind::Parallel => Shape::Parallel { a: p[0], b: p[1], p: p[2] },
            ShapeKind::Ellipse => {
                // Second demi-axe rendu perpendiculaire au premier.
                let (center, a, b) = geo::ellipse_from_3_points(p[0], p[1], p[2]);
                Shape::Ellipse { center, a, b }
            }
            ShapeKind::Arc => Shape::Arc { a: p[0], m: p[1], b: p[2] },
            ShapeKind::Profile => Shape::profile(p[0], p[1]),
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
            Shape::PointLine { .. } => ShapeKind::PointLine,
            Shape::Parallel { .. } => ShapeKind::Parallel,
            Shape::Ellipse { .. } => ShapeKind::Ellipse,
            Shape::Arc { .. } => ShapeKind::Arc,
            Shape::Profile { .. } => ShapeKind::Profile,
        }
    }

    /// Points de contrôle (poignées), dans l'ordre de saisie.
    pub fn points(&self) -> Vec<Pt> {
        match self {
            Shape::Length { a, b } | Shape::Rect { a, b, .. } | Shape::Profile { a, b, .. } => vec![*a, *b],
            Shape::Path { pts } | Shape::Area { pts } => pts.clone(),
            Shape::Angle { a, vertex, b } => vec![*a, *vertex, *b],
            Shape::Circle { center, edge } => vec![*center, *edge],
            Shape::Circle3 { a, b, c } => vec![*a, *b, *c],
            Shape::Marker { p } => vec![*p],
            Shape::LineAngle { a, b, c, d } => vec![*a, *b, *c, *d],
            Shape::PointLine { a, b, p } | Shape::Parallel { a, b, p } => vec![*a, *b, *p],
            Shape::Ellipse { center, a, b } => vec![*center, *a, *b],
            Shape::Arc { a, m, b } => vec![*a, *m, *b],
        }
    }

    /// Accès mutable aux points de contrôle, même ordre que [`Shape::points`].
    pub fn points_mut(&mut self) -> Vec<&mut Pt> {
        match self {
            Shape::Length { a, b } | Shape::Rect { a, b, .. } | Shape::Profile { a, b, .. } => vec![a, b],
            Shape::Path { pts } | Shape::Area { pts } => pts.iter_mut().collect(),
            Shape::Angle { a, vertex, b } => vec![a, vertex, b],
            Shape::Circle { center, edge } => vec![center, edge],
            Shape::Circle3 { a, b, c } => vec![a, b, c],
            Shape::Marker { p } => vec![p],
            Shape::LineAngle { a, b, c, d } => vec![a, b, c, d],
            Shape::PointLine { a, b, p } | Shape::Parallel { a, b, p } => vec![a, b, p],
            Shape::Ellipse { center, a, b } => vec![center, a, b],
            Shape::Arc { a, m, b } => vec![a, m, b],
        }
    }

    /// Applique une transformation à tous les points (déplacement, rotation d'image…).
    pub fn map_points(&mut self, f: impl Fn(Pt) -> Pt) {
        for p in self.points_mut() {
            *p = f(*p);
        }
    }

    /// Reporte la forme dans une image reprojetée par `f` (correction de
    /// perspective). Les droites restent des droites : longueurs, chemins,
    /// angles, surfaces et cercles par trois points suivent exactement leurs
    /// points. Un rectangle garde ses deux coins et s'aligne sur l'image de
    /// son premier côté ; un cercle garde son centre et son point de bord
    /// (leur image exacte serait un quadrilatère et une ellipse).
    pub fn reproject(&mut self, f: impl Fn(Pt) -> Pt) {
        if let Shape::Rect { a, b, angle } = self {
            let side = geo::rect_corners(*a, *b, *angle)[1];
            // Le premier côté part de `a` dans le sens de l'axe du rectangle, ou à l'opposé.
            let backwards = geo::to_frame(*b, *angle).x < geo::to_frame(*a, *angle).x;
            let (na, ns) = (f(*a), f(side));
            (*a, *b) = (na, f(*b));
            if na.dist(ns) > 0.0 {
                *angle = geo::normalize_deg(geo::direction_deg(na, ns) + if backwards { 180.0 } else { 0.0 });
            }
            return;
        }
        self.map_points(f);
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
            Shape::Marker { .. } => vec![],
            Shape::LineAngle { a, b, c, d } => vec![Value {
                quantity: Quantity::Angle,
                value: geo::line_angle_deg(*a, *b, *c, *d),
                unit: "°".to_owned(),
            }],
            // Extrémités confondues : plus de droite, distance indéfinie (les
            // poignées restent pour corriger la mesure).
            Shape::PointLine { a, b, .. } | Shape::Parallel { a, b, .. } if a == b => vec![],
            Shape::PointLine { a, b, p } | Shape::Parallel { a, b, p } => {
                vec![len(Quantity::Distance, geo::dist_point_line(*p, *a, *b))]
            }
            Shape::Ellipse { center, a, b } => {
                let (ra, rb) = geo::ellipse_semi_axes(*center, *a, *b);
                vec![
                    len(Quantity::MajorAxis, 2.0 * ra.max(rb)),
                    len(Quantity::MinorAxis, 2.0 * ra.min(rb)),
                    area(PI * ra * rb),
                    len(Quantity::Perimeter, geo::ellipse_perimeter(ra, rb)),
                ]
            }
            Shape::Arc { a, m, b } => match geo::arc_from_3_points(*a, *m, *b) {
                Some(arc) => vec![
                    len(Quantity::Radius, arc.radius),
                    len(Quantity::ArcLength, arc.length()),
                    Value { quantity: Quantity::Angle, value: arc.sweep.abs().to_degrees(), unit: "°".to_owned() },
                    len(Quantity::Chord, a.dist(*b)),
                ],
                None => vec![],
            },
            // Sans bords détectés : pas de valeur.
            Shape::Profile { result, .. } => result.iter().map(|r| len(Quantity::Width, r.width_px())).collect(),
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
    /// Distance perpendiculaire (point-droite, écart entre parallèles).
    Distance,
    MajorAxis,
    MinorAxis,
    ArcLength,
    Chord,
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
            Quantity::Distance => tr!("quantity.distance"),
            Quantity::MajorAxis => tr!("quantity.major_axis"),
            Quantity::MinorAxis => tr!("quantity.minor_axis"),
            Quantity::ArcLength => tr!("quantity.arc_length"),
            Quantity::Chord => tr!("quantity.chord"),
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

/// Nombre de décimales affichées choisi par l'utilisateur (`AUTO` : selon la grandeur).
/// Réglage global, comme la langue : une seule interface par processus.
static DECIMALS: AtomicU8 = AtomicU8::new(AUTO);
const AUTO: u8 = u8::MAX;

/// Plus grand nombre de décimales proposé.
pub const MAX_DECIMALS: u8 = 4;

/// Fixe le nombre de décimales des valeurs affichées (`None` : automatique).
pub fn set_decimals(decimals: Option<u8>) {
    DECIMALS.store(decimals.map_or(AUTO, |d| d.min(MAX_DECIMALS)), Ordering::Relaxed);
}

/// Nombre de décimales des valeurs affichées (`None` : automatique).
pub fn decimals() -> Option<u8> {
    Some(DECIMALS.load(Ordering::Relaxed)).filter(|&d| d != AUTO)
}

/// Formate un nombre avec le nombre de décimales réglé ([`set_decimals`]),
/// séparateur décimal de la langue.
pub fn format_number(v: f64) -> String {
    format_number_with(v, decimals())
}

/// Formate un nombre avec `decimals` décimales, ou une précision adaptée à sa
/// grandeur (`None`), séparateur décimal de la langue.
pub fn format_number_with(v: f64, decimals: Option<u8>) -> String {
    let a = v.abs();
    let decimals = match decimals {
        Some(d) => usize::from(d),
        None if a >= 1000.0 => 0,
        None if a >= 100.0 => 1,
        None if a >= 1.0 => 2,
        None => 3,
    };
    let mut s = format!("{v:.decimals$}");
    // « -0,00 » pour une valeur négative arrondie à zéro.
    if s.starts_with('-') && s.bytes().all(|b| matches!(b, b'-' | b'0' | b'.')) {
        s.remove(0);
    }
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
    fn decimales_choisies() {
        meteroscope_i18n::set_language("fr");
        assert_eq!(format_number_with(1234.567, None), "1235");
        assert_eq!(format_number_with(0.12345, None), "0,123");
        assert_eq!(format_number_with(1234.567, Some(2)), "1234,57");
        assert_eq!(format_number_with(0.12345, Some(0)), "0");
        assert_eq!(format_number_with(12.5, Some(4)), "12,5000");
        // Pas de « -0 » pour une petite valeur négative arrondie.
        assert_eq!(format_number_with(-0.004, Some(2)), "0,00");
        assert_eq!(format_number_with(-0.4, Some(0)), "0");
        assert_eq!(format_number_with(-1.5, Some(1)), "-1,5");
    }

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
            // Formes dont le tracé recalcule des points (rectangle incliné, ellipse).
            if matches!(k, ShapeKind::Rect3 | ShapeKind::Ellipse) {
                continue;
            }
            let n = k.min_points();
            let pts: Vec<Pt> = (0..n).map(|i| Pt::new(i as f64, (i * i) as f64)).collect();
            let s = Shape::from_points(k, &pts).unwrap();
            assert_eq!((s.kind(), s.points()), (k, pts));
        }
    }

    #[test]
    fn nouvelles_formes() {
        meteroscope_i18n::set_language("fr");
        let p = |x, y| Pt::new(x, y);
        let mut cal = Calibration::default();
        assert!(cal.set_from_reference(10.0, 1.0, "mm"));
        let close = |a: f64, b: f64| (a - b).abs() < 1e-9;

        // Distance point-droite : la droite est prolongée au-delà du segment.
        let s = Shape::from_points(ShapeKind::PointLine, &[p(0., 0.), p(10., 0.), p(30., -40.)]).unwrap();
        let v = s.values(&cal);
        assert_eq!((v.len(), v[0].quantity, v[0].text()), (1, Quantity::Distance, "4,00 mm".to_owned()));
        // Extrémités de référence confondues : pas de droite, pas de valeur.
        for k in [ShapeKind::PointLine, ShapeKind::Parallel] {
            let s = Shape::from_points(k, &[p(0., 0.), p(0., 0.), p(3., 4.)]).unwrap();
            assert!(s.values(&cal).is_empty() && s.points().len() == 3);
        }
        // Écart entre parallèles : même grandeur, autre type.
        let s = Shape::from_points(ShapeKind::Parallel, &[p(0., 0.), p(0., 10.), p(25., 3.)]).unwrap();
        assert_eq!(s.kind(), ShapeKind::Parallel);
        assert!(close(s.values(&cal)[0].value, 2.5));

        // Ellipse 4 × 2 (demi-axes), troisième point pas exactement sur l'axe.
        let s = Shape::from_points(ShapeKind::Ellipse, &[p(0., 0.), p(0., 20.), p(-10., 7.)]).unwrap();
        assert_eq!(s, Shape::Ellipse { center: p(0., 0.), a: p(0., 20.), b: p(-10., 0.) });
        let v = s.values(&cal);
        let q: Vec<Quantity> = v.iter().map(|v| v.quantity).collect();
        assert_eq!(q, [Quantity::MajorAxis, Quantity::MinorAxis, Quantity::Area, Quantity::Perimeter]);
        assert!(close(v[0].value, 4.0) && close(v[1].value, 2.0) && close(v[2].value, 2.0 * PI));
        assert!((v[3].value - 9.688448220547676).abs() < 1e-5);
        assert_eq!(v[2].unit, "mm²");

        // Arc : demi-cercle de rayon 10 px = 1 mm.
        let s = Shape::from_points(ShapeKind::Arc, &[p(10., 0.), p(0., 10.), p(-10., 0.)]).unwrap();
        let v = s.values(&cal);
        let q: Vec<Quantity> = v.iter().map(|v| v.quantity).collect();
        assert_eq!(q, [Quantity::Radius, Quantity::ArcLength, Quantity::Angle, Quantity::Chord]);
        assert!(close(v[0].value, 1.0) && close(v[1].value, PI) && close(v[2].value, 180.0) && close(v[3].value, 2.0));
        assert_eq!(v[2].unit, "°");
        // Arc dégénéré (alignés) : pas de valeur, comme un cercle impossible.
        assert!(Shape::Arc { a: p(0., 0.), m: p(1., 0.), b: p(2., 0.) }.values(&cal).is_empty());

        // Aller-retour JSON de chaque nouvelle forme.
        for s in [
            Shape::PointLine { a: p(0., 0.), b: p(10., 0.), p: p(3., 4.) },
            Shape::Parallel { a: p(1., 2.), b: p(3., 4.), p: p(5., 0.) },
            Shape::Ellipse { center: p(5., 5.), a: p(9., 5.), b: p(5., 7.) },
            Shape::Arc { a: p(1., 0.), m: p(0., 1.), b: p(-1., 0.) },
        ] {
            let json = serde_json::to_string(&s).unwrap();
            assert_eq!(serde_json::from_str::<Shape>(&json).unwrap(), s, "{json}");
        }
        // Un ancien projet (formes existantes seulement) se relit toujours.
        let old = r#"[{"Length":{"a":{"x":0.0,"y":0.0},"b":{"x":1.0,"y":0.0}}},{"LineAngle":{"a":{"x":0.0,"y":0.0},"b":{"x":1.0,"y":0.0},"c":{"x":0.0,"y":1.0},"d":{"x":1.0,"y":2.0}}}]"#;
        assert_eq!(serde_json::from_str::<Vec<Shape>>(old).unwrap().len(), 2);

        // Statistiques par type : chaque nouvelle forme a son groupe, sur sa valeur principale.
        let mut doc = crate::Document::new();
        doc.add(Shape::PointLine { a: p(0., 0.), b: p(10., 0.), p: p(3., 4.) });
        doc.add(Shape::PointLine { a: p(0., 0.), b: p(10., 0.), p: p(3., 6.) });
        doc.add(Shape::Parallel { a: p(0., 0.), b: p(10., 0.), p: p(3., 6.) });
        doc.add(Shape::Arc { a: p(1., 0.), m: p(0., 1.), b: p(-1., 0.) });
        let sum = doc.summary();
        let pl = sum.iter().find(|g| g.0 == ShapeKind::PointLine).unwrap();
        assert!(pl.1 == Quantity::Distance && pl.3.n == 2 && close(pl.3.mean, 5.0));
        assert_eq!(sum.iter().find(|g| g.0 == ShapeKind::Parallel).unwrap().3.n, 1);
        assert_eq!(sum.iter().find(|g| g.0 == ShapeKind::Arc).unwrap().1, Quantity::Radius);
    }
}

//! Tableau des mesures : CSV (séparateurs selon la langue) et texte tabulé (presse-papiers).

use meteroscope_i18n::tr;
use meteroscope_core::{Document, Quantity, Shape};

/// Colonnes de grandeurs, dans l'ordre du tableau.
const COLUMNS: [Quantity; 13] = [
    Quantity::Length,
    Quantity::Angle,
    Quantity::Area,
    Quantity::Perimeter,
    Quantity::Width,
    Quantity::Height,
    Quantity::Diameter,
    Quantity::Radius,
    Quantity::Distance,
    Quantity::MajorAxis,
    Quantity::MinorAxis,
    Quantity::ArcLength,
    Quantity::Chord,
];

/// Nombre avec ~6 chiffres significatifs, virgule décimale, sans notation
/// scientifique ni zéros inutiles.
pub fn format_precise(v: f64) -> String {
    if !v.is_finite() {
        return String::new();
    }
    if v == 0.0 {
        return "0".to_owned();
    }
    let mag = v.abs().log10().floor() as i32;
    let decimals = (5 - mag).clamp(0, 15) as usize;
    let mut s = format!("{v:.decimals$}");
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_owned();
    }
    if s == "-0" {
        s = "0".to_owned();
    }
    match meteroscope_i18n::decimal_separator() {
        '.' => s,
        sep => s.replace('.', &sep.to_string()),
    }
}

/// Unité d'une grandeur pour l'étalonnage du document.
fn unit_of(doc: &Document, q: Quantity) -> String {
    match q {
        Quantity::Angle => "°".to_owned(),
        Quantity::Area => doc.calibration.area_unit(),
        _ => doc.calibration.length_unit().to_owned(),
    }
}

/// Lignes du tableau des mesures (en-tête compris), cellules brutes.
pub(crate) fn measurement_rows(doc: &Document) -> Vec<Vec<String>> {
    let mut header = vec![tr!("table.name").to_owned(), tr!("item.type").to_owned()];
    header.extend(COLUMNS.iter().map(|q| format!("{} ({})", q.label(), unit_of(doc, *q))));
    header.extend(["X (px)".to_owned(), "Y (px)".to_owned()]);
    let mut rows = vec![header];
    for m in &doc.measurements {
        let mut row = vec![m.name.clone(), m.shape.kind().label().to_owned()];
        let values = m.shape.values(&doc.calibration);
        for q in COLUMNS {
            row.push(values.iter().find(|v| v.quantity == q).map(|v| format_precise(v.value)).unwrap_or_default());
        }
        match &m.shape {
            Shape::Marker { p } => row.extend([format_precise(p.x), format_precise(p.y)]),
            _ => row.extend([String::new(), String::new()]),
        }
        rows.push(row);
    }
    rows
}

/// Bloc statistiques + échelle.
fn stats_rows(doc: &Document) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = vec![
        [tr!("stats.title"), tr!("table.quantity"), tr!("calib.unit"), "n", tr!("stats.mean"), tr!("stats.std_dev"), tr!("stats.min"), tr!("stats.max")]
            .map(str::to_owned)
            .to_vec(),
    ];
    for (kind, q, unit, s) in doc.summary() {
        rows.push(vec![
            kind.label().to_owned(),
            q.label().to_owned(),
            unit,
            s.n.to_string(),
            format_precise(s.mean),
            format_precise(s.sd),
            format_precise(s.min),
            format_precise(s.max),
        ]);
    }
    rows.push(vec![tr!("table.marker_count").to_owned(), doc.marker_count().to_string()]);
    rows.push(vec![]);
    rows.push(match &doc.calibration.scale {
        Some(s) => vec![tr!("panel.scale").to_owned(), format_precise(s.units_per_px), format!("{}/px", s.unit)],
        None => vec![tr!("panel.scale").to_owned(), tr!("table.uncalibrated").to_owned()],
    });
    rows
}

/// Séparateur de champs CSV adapté au séparateur décimal de la langue.
pub fn csv_separator() -> char {
    if meteroscope_i18n::decimal_separator() == ',' { ';' } else { ',' }
}

/// Champ CSV : entre guillemets si nécessaire, guillemets doublés.
pub(crate) fn csv_field(s: &str, sep: char) -> String {
    if s.contains([sep, '"', '\n', '\r']) || s.starts_with(' ') || s.ends_with(' ') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_owned()
    }
}

/// Champ tabulé : tabulations et retours à la ligne remplacés par des espaces.
pub(crate) fn tsv_field(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

pub fn csv_string(doc: &Document) -> String {
    // Virgule décimale → « ; » entre les champs (tableurs européens), sinon « , ».
    let sep = csv_separator();
    let mut out = String::from("\u{feff}");
    let mut push = |rows: Vec<Vec<String>>| {
        for r in rows {
            out.push_str(&r.iter().map(|f| csv_field(f, sep)).collect::<Vec<_>>().join(&sep.to_string()));
            out.push_str("\r\n");
        }
    };
    push(measurement_rows(doc));
    push(vec![vec![]]);
    push(stats_rows(doc));
    out
}

pub fn clipboard_table(doc: &Document) -> String {
    let mut out = String::new();
    for r in measurement_rows(doc) {
        out.push_str(&r.iter().map(|f| tsv_field(f)).collect::<Vec<_>>().join("\t"));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use meteroscope_core::Pt;

    #[test]
    fn precise_numbers() {
        meteroscope_i18n::set_language("fr");
        assert_eq!(format_precise(0.0), "0");
        assert_eq!(format_precise(-0.0), "0");
        assert_eq!(format_precise(5.0), "5");
        assert_eq!(format_precise(1.0 / 3.0), "0,333333");
        assert_eq!(format_precise(123.456789), "123,457");
        assert_eq!(format_precise(1234567.89), "1234568");
        assert_eq!(format_precise(-0.000123456789), "-0,000123457");
        assert_eq!(format_precise(f64::NAN), "");
    }

    #[test]
    fn csv_escaping() {
        meteroscope_i18n::set_language("fr");
        assert_eq!(csv_field("a;b", ';'), "\"a;b\"");
        assert_eq!(csv_field("dit \"x\"", ';'), "\"dit \"\"x\"\"\"");
        assert_eq!(csv_field("simple", ';'), "simple");
        // Point décimal : virgule entre les champs.
        assert_eq!(csv_field("a,b", ','), "\"a,b\"");
        assert_eq!(csv_field("a;b", ','), "a;b");
        assert_eq!(tsv_field("a\tb\nc"), "a b c");
    }

    fn sample() -> Document {
        let mut d = Document::new();
        d.add(Shape::Length { a: Pt::new(0., 0.), b: Pt::new(30., 40.) });
        d.add(Shape::Length { a: Pt::new(0., 0.), b: Pt::new(0., 100.) });
        d.add(Shape::Rect { a: Pt::new(0., 0.), b: Pt::new(20., 10.), angle: 0.0 });
        d.add(Shape::Angle { a: Pt::new(10., 0.), vertex: Pt::new(0., 0.), b: Pt::new(0., 10.) });
        d.add(Shape::Marker { p: Pt::new(12.5, 7.0) });
        let id = d.add(Shape::Marker { p: Pt::new(3.0, 4.0) });
        d.get_mut(id).unwrap().name = "Point; \"spécial\"".to_owned();
        d.calibration.set_from_reference(10.0, 1.0, "mm");
        d
    }

    #[test]
    fn csv_exact() {
        meteroscope_i18n::set_language("fr");
        let expected = "\u{feff}\
Nom;Type;Longueur (mm);Angle (°);Aire (mm²);Périmètre (mm);Largeur (mm);Hauteur (mm);Diamètre (mm);Rayon (mm);Distance (mm);Grand axe (mm);Petit axe (mm);Longueur d'arc (mm);Corde (mm);X (px);Y (px)\r\n\
Longueur 1;Longueur;5;;;;;;;;;;;;;;\r\n\
Longueur 2;Longueur;10;;;;;;;;;;;;;;\r\n\
Rectangle 1;Rectangle;;;2;6;2;1;;;;;;;;;\r\n\
Angle 1;Angle;;90;;;;;;;;;;;;;\r\n\
Marqueur 1;Marqueur;;;;;;;;;;;;;;12,5;7\r\n\
\"Point; \"\"spécial\"\"\";Marqueur;;;;;;;;;;;;;;3;4\r\n\
\r\n\
Statistiques;Grandeur;Unité;n;Moyenne;Écart-type;Min;Max\r\n\
Longueur;Longueur;mm;2;7,5;3,53553;5;10\r\n\
Angle;Angle;°;1;90;0;90;90\r\n\
Rectangle;Aire;mm²;1;2;0;2;2\r\n\
Nombre de marqueurs;2\r\n\
\r\n\
Échelle;0,1;mm/px\r\n";
        assert_eq!(csv_string(&sample()), expected);
    }

    #[test]
    fn clipboard_exact() {
        meteroscope_i18n::set_language("fr");
        let t = clipboard_table(&sample());
        assert!(!t.starts_with('\u{feff}'));
        let lines: Vec<&str> = t.lines().collect();
        assert_eq!(lines.len(), 7);
        assert_eq!(lines[1], "Longueur 1\tLongueur\t5\t\t\t\t\t\t\t\t\t\t\t\t\t\t");
        assert_eq!(lines[6], "Point; \"spécial\"\tMarqueur\t\t\t\t\t\t\t\t\t\t\t\t\t\t3\t4");
    }

    #[test]
    fn uncalibrated() {
        meteroscope_i18n::set_language("fr");
        let mut d = Document::new();
        d.add(Shape::Circle { center: Pt::new(0., 0.), edge: Pt::new(10., 0.) });
        let s = csv_string(&d);
        assert!(s.contains("Diamètre (px)") && s.contains("Aire (px²)"));
        assert!(s.contains("Cercle 1;Cercle;;;314,159;62,8319;;;20;10;;;;;;;\r\n"));
        assert!(s.ends_with("Échelle;non étalonnée (mesures en pixels)\r\n"));
    }

    #[test]
    fn colonnes_nouvelles_formes() {
        meteroscope_i18n::set_language("fr");
        let mut d = Document::new();
        d.add(Shape::PointLine { a: Pt::new(0., 0.), b: Pt::new(10., 0.), p: Pt::new(20., 30.) });
        d.add(Shape::Parallel { a: Pt::new(0., 0.), b: Pt::new(0., 10.), p: Pt::new(4., 3.) });
        d.add(Shape::Ellipse { center: Pt::new(0., 0.), a: Pt::new(10., 0.), b: Pt::new(0., 5.) });
        d.add(Shape::Arc { a: Pt::new(10., 0.), m: Pt::new(0., 10.), b: Pt::new(-10., 0.) });
        let s = csv_string(&d);
        // Distance, puis grand axe, petit axe, aire et périmètre, puis rayon, longueur d'arc, angle et corde.
        assert!(s.contains("Distance point-droite 1;Distance point-droite;;;;;;;;;30;;;;;;\r\n"), "{s}");
        assert!(s.contains("Écart entre parallèles 1;Écart entre parallèles;;;;;;;;;4;;;;;;\r\n"), "{s}");
        assert!(s.contains("Ellipse 1;Ellipse;;;157,08;48,4422;;;;;;20;10;;;;\r\n"), "{s}");
        assert!(s.contains("Arc 1;Arc;;180;;;;;;10;;;;31,4159;20;;\r\n"), "{s}");
        // Statistiques par type sur la valeur principale.
        assert!(s.contains("Ellipse;Grand axe;px;1;20;0;20;20\r\n") && s.contains("Arc;Rayon;px;1;10;0;10;10\r\n"), "{s}");
    }
}

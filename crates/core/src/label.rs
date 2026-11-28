//! Texte des étiquettes posées sur l'image : le même à l'écran et dans tous
//! les exports (image annotée, SVG, rapport PDF).

use crate::calibration::Calibration;
use crate::measure::{Measurement, Shape, format_number};
use std::collections::HashMap;

/// Texte de l'étiquette d'une forme. `marker_no` : numéro du marqueur (les
/// exports lui préfèrent [`marker_labels`], qui reste lisible après un
/// renommage). Vide quand il n'y a rien à afficher.
pub fn label_text(shape: &Shape, cal: &Calibration, marker_no: usize) -> String {
    let v = shape.values(cal);
    match shape {
        Shape::Marker { .. } => marker_no.to_string(),
        Shape::Text { text, .. } | Shape::Arrow { text, .. } => text.clone(),
        Shape::Rect { .. } if v.len() >= 3 => {
            format!("{} × {} {}\n{}", format_number(v[1].value), format_number(v[2].value), v[1].unit, v[0].text())
        }
        Shape::Circle { .. } | Shape::Circle3 { .. } if !v.is_empty() => format!("Ø {}", v[0].text()),
        Shape::Angle { .. } | Shape::LineAngle { .. } if !v.is_empty() => format!("{}°", format_number(v[0].value)),
        Shape::Ellipse { .. } if v.len() >= 2 => {
            format!("{} × {} {}", format_number(v[0].value), format_number(v[1].value), v[0].unit)
        }
        Shape::Arc { .. } if !v.is_empty() => format!("R {}", v[0].text()),
        _ => v.first().map(|v| v.text()).unwrap_or_default(),
    }
}

/// Numéro que porte le nom d'une mesure (« Marqueur 3 » → 3).
fn name_number(name: &str) -> Option<usize> {
    name.rsplit(char::is_whitespace).next()?.parse().ok().filter(|n| *n > 0)
}

/// Étiquette dessinée à côté de chaque marqueur, par identifiant de mesure,
/// pour que l'image et les tableaux désignent le même point : le numéro de
/// son nom (« Marqueur 3 » → « 3 », même après une suppression) tant qu'il
/// ne désigne que lui. Si deux marqueurs portent le même numéro (« Lot A 1 »
/// et « Lot B 1 ») ou si le nom n'en a pas, c'est le nom entier ; un
/// marqueur sans nom garde son rang dans la liste.
pub fn marker_labels(measurements: &[Measurement]) -> HashMap<u64, String> {
    let markers = || measurements.iter().filter(|m| matches!(m.shape, Shape::Marker { .. }));
    let mut uses: HashMap<usize, usize> = HashMap::new();
    for n in markers().filter_map(|m| name_number(&m.name)) {
        *uses.entry(n).or_default() += 1;
    }
    markers()
        .enumerate()
        .map(|(rank, m)| {
            let name = m.name.trim();
            let label = match name_number(name) {
                Some(n) if uses[&n] == 1 => n.to_string(),
                _ if name.is_empty() => (rank + 1).to_string(),
                _ => name.to_owned(),
            };
            (m.id, label)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, Pt};

    #[test]
    fn etiquettes_des_marqueurs() {
        meteroscope_i18n::set_language("fr");
        let mut doc = Document::new();
        let ids: Vec<u64> = (0..4).map(|i| doc.add(Shape::Marker { p: Pt::new(i as f64, 0.) })).collect();
        doc.add(Shape::Length { a: Pt::new(0., 0.), b: Pt::new(1., 0.) });
        let labels = |doc: &Document| {
            let all = marker_labels(&doc.measurements);
            ids.iter().map(|id| all.get(id).cloned().unwrap_or_default()).collect::<Vec<_>>()
        };
        assert_eq!(labels(&doc), ["1", "2", "3", "4"]);
        // Suppression : les suivants gardent le numéro de leur nom.
        doc.remove(ids[1]);
        assert_eq!(labels(&doc), ["1", "", "3", "4"]);
        // Renommé sans numéro : son nom, que l'on retrouve dans le tableau.
        doc.get_mut(ids[2]).unwrap().name = "Repère".to_owned();
        assert_eq!(labels(&doc), ["1", "", "Repère", "4"]);
        doc.get_mut(ids[0]).unwrap().name = "Trou n° 12".to_owned();
        assert_eq!(labels(&doc), ["12", "", "Repère", "4"]);
        // Deux noms distincts qui finissent par le même numéro : jamais la
        // même étiquette, chacun affiche son nom.
        doc.get_mut(ids[0]).unwrap().name = "Lot A 1".to_owned();
        doc.get_mut(ids[2]).unwrap().name = "Lot B 1".to_owned();
        assert_eq!(labels(&doc), ["Lot A 1", "", "Lot B 1", "4"]);
        // Sans nom : le rang dans la liste des marqueurs.
        doc.get_mut(ids[3]).unwrap().name = "  ".to_owned();
        assert_eq!(labels(&doc)[3], "3");
    }

    #[test]
    fn textes_des_etiquettes() {
        meteroscope_i18n::set_language("fr");
        let cal = Calibration::default();
        let r = Shape::Rect { a: Pt::new(0., 0.), b: Pt::new(20., 10.), angle: 0.0 };
        assert_eq!(label_text(&r, &cal, 0), "20,00 × 10,00 px\n200,0 px²");
        let c = Shape::Circle { center: Pt::new(0., 0.), edge: Pt::new(5., 0.) };
        assert_eq!(label_text(&c, &cal, 0), "Ø 10,00 px");
        assert_eq!(label_text(&Shape::Marker { p: Pt::default() }, &cal, 3), "3");
        // Droite réduite à un point : pas de valeur, pas d'étiquette.
        let pl = Shape::PointLine { a: Pt::new(1., 1.), b: Pt::new(1., 1.), p: Pt::new(5., 5.) };
        assert_eq!(label_text(&pl, &cal, 0), "");
    }
}

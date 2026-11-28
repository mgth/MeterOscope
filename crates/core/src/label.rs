//! Texte des étiquettes posées sur l'image : le même à l'écran et dans tous
//! les exports (image annotée, SVG, rapport PDF).

use crate::calibration::Calibration;
use crate::measure::{Measurement, Shape, format_number};
use std::collections::{HashMap, HashSet};

/// Texte de l'étiquette d'une forme. `marker_no` : numéro du marqueur
/// (voir [`marker_numbers`]). Vide quand il n'y a rien à afficher.
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

/// Numéro dessiné à côté de chaque marqueur, par identifiant de mesure :
/// celui de son nom (« Marqueur 3 » → 3), pour que l'image et les tableaux
/// désignent le même point même après une suppression. Un marqueur renommé
/// sans numéro prend le plus petit numéro libre, dans l'ordre de la liste.
pub fn marker_numbers(measurements: &[Measurement]) -> HashMap<u64, usize> {
    let markers = || measurements.iter().filter(|m| matches!(m.shape, Shape::Marker { .. }));
    let named: HashSet<usize> = markers().filter_map(|m| name_number(&m.name)).collect();
    let mut free = (1..).filter(|n| !named.contains(n));
    markers().map(|m| (m.id, name_number(&m.name).or_else(|| free.next()).unwrap_or(0))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Document, Pt};

    #[test]
    fn numeros_des_marqueurs() {
        meteroscope_i18n::set_language("fr");
        let mut doc = Document::new();
        let ids: Vec<u64> = (0..4).map(|i| doc.add(Shape::Marker { p: Pt::new(i as f64, 0.) })).collect();
        doc.add(Shape::Length { a: Pt::new(0., 0.), b: Pt::new(1., 0.) });
        let numbers = |doc: &Document| ids.iter().map(|id| marker_numbers(&doc.measurements).get(id).copied()).collect::<Vec<_>>();
        assert_eq!(numbers(&doc), [Some(1), Some(2), Some(3), Some(4)]);
        // Suppression : les suivants gardent le numéro de leur nom.
        doc.remove(ids[1]);
        assert_eq!(numbers(&doc), [Some(1), None, Some(3), Some(4)]);
        // Renommé sans numéro : le premier numéro libre.
        doc.get_mut(ids[2]).unwrap().name = "Repère".to_owned();
        assert_eq!(numbers(&doc), [Some(1), None, Some(2), Some(4)]);
        doc.get_mut(ids[0]).unwrap().name = "Trou n° 12".to_owned();
        assert_eq!(numbers(&doc), [Some(12), None, Some(1), Some(4)]);
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

//! Logique pure : lecture des nombres saisis et mise en forme de l'échelle.

use meteroscope_i18n::tr;
use meteroscope_core::Scale;

/// Lit un nombre saisi par l'utilisateur : accepte la virgule ou le point
/// décimal et ignore les espaces (y compris insécables). `None` si le texte
/// n'est pas un nombre fini.
pub fn parse_number(text: &str) -> Option<f64> {
    let cleaned: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '\u{202F}' && *c != '\u{00A0}')
        .map(|c| if c == ',' { '.' } else { c })
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    cleaned.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Formate un nombre avec `sig` chiffres significatifs (voir
/// [`meteroscope_core::measure::format_significant`]) ; « — » s'il n'est pas fini.
pub fn format_significant(v: f64, sig: usize) -> String {
    if v.is_finite() { meteroscope_core::measure::format_significant(v, sig) } else { "—".to_owned() }
}

/// Textes décrivant une échelle : (« 1 px = 0,04082 mm », « 24,5 px par mm »).
pub fn scale_texts(scale: &Scale) -> (String, String) {
    scale_texts_raw(scale.units_per_px, &scale.unit)
}

/// Comme [`scale_texts`], à partir du nombre d'unités par pixel.
pub fn scale_texts_raw(units_per_px: f64, unit: &str) -> (String, String) {
    let main = format!("1 px = {} {unit}", format_significant(units_per_px, 4));
    let inverse = if units_per_px > 0.0 {
        tr!("scale.px_per_unit", px = format_significant(1.0 / units_per_px, 4), unit = unit)
    } else {
        String::new()
    };
    (main, inverse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lecture_des_nombres() {
        meteroscope_i18n::set_language("fr");
        assert_eq!(parse_number("12,5"), Some(12.5));
        assert_eq!(parse_number("12.5"), Some(12.5));
        assert_eq!(parse_number("  7 "), Some(7.0));
        assert_eq!(parse_number("1 000,5"), Some(1000.5));
        assert_eq!(parse_number("1\u{202F}000"), Some(1000.0));
        assert_eq!(parse_number(",5"), Some(0.5));
        assert_eq!(parse_number("-3"), Some(-3.0));
        assert_eq!(parse_number(""), None);
        assert_eq!(parse_number("abc"), None);
        assert_eq!(parse_number("1,2,3"), None);
        assert_eq!(parse_number("1.234,5"), None);
        assert_eq!(parse_number("inf"), None);
        assert_eq!(parse_number("NaN"), None);
    }

    #[test]
    fn chiffres_significatifs() {
        meteroscope_i18n::set_language("fr");
        assert_eq!(format_significant(0.0123456, 4), "0,01235");
        assert_eq!(format_significant(81.3, 4), "81,3");
        assert_eq!(format_significant(120.0, 4), "120");
        assert_eq!(format_significant(12345.6, 4), "12346");
        assert_eq!(format_significant(2.5, 4), "2,5");
        assert_eq!(format_significant(0.0, 4), "0");
        assert_eq!(format_significant(-0.5, 3), "-0,5");
    }

    #[test]
    fn texte_de_l_echelle() {
        meteroscope_i18n::set_language("fr");
        let s = Scale { units_per_px: 0.1, unit: "mm".to_owned() };
        assert_eq!(scale_texts(&s), ("1 px = 0,1 mm".to_owned(), "10 px par mm".to_owned()));
        let (a, b) = scale_texts_raw(10.0 / 245.0, "µm");
        assert_eq!(a, "1 px = 0,04082 µm");
        assert_eq!(b, "24,5 px par µm");
    }
}

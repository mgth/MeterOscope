//! Barre d'échelle (comme sur une image de microscopie) : un trait de
//! longueur ronde (1, 2 ou 5 × 10ⁿ dans l'unité de l'échelle) et son libellé.

use serde::{Deserialize, Serialize};

/// Coin où se place la barre d'échelle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    #[default]
    BottomRight,
}

impl Corner {
    pub const ALL: [Corner; 4] = [Corner::TopLeft, Corner::TopRight, Corner::BottomLeft, Corner::BottomRight];

    pub fn is_left(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::BottomLeft)
    }
    pub fn is_top(self) -> bool {
        matches!(self, Corner::TopLeft | Corner::TopRight)
    }

    /// Coin haut-gauche d'une boîte `bw` × `bh` placée dans ce coin d'une zone
    /// `x0..x0 + w`, `y0..y0 + h`, à `margin` des bords. Si la marge ne tient
    /// pas, elle se réduit pour que la boîte reste entière dans la zone ; une
    /// boîte plus grande que la zone est collée en haut à gauche.
    pub fn place(self, (x0, y0, w, h): (f32, f32, f32, f32), (bw, bh): (f32, f32), margin: f32) -> (f32, f32) {
        let x = if self.is_left() { x0 + margin } else { x0 + w - bw - margin };
        let y = if self.is_top() { y0 + margin } else { y0 + h - bh - margin };
        (x.min(x0 + w - bw).max(x0), y.min(y0 + h - bh).max(y0))
    }
}

/// Réglages de la barre d'échelle, enregistrés avec le document.
/// Affichée par défaut, mais seulement quand l'échelle est définie.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScaleBarSettings {
    pub visible: bool,
    pub corner: Corner,
}

impl Default for ScaleBarSettings {
    fn default() -> Self {
        Self { visible: true, corner: Corner::default() }
    }
}

/// Part de la largeur visée par la barre (milieu de la fourchette 15–20 %).
pub const TARGET_FRACTION: f64 = 0.175;

/// Barre calculée : valeur ronde, longueur en pixels (de la vue considérée)
/// et libellé « valeur unité ».
#[derive(Clone, Debug, PartialEq)]
pub struct ScaleBar {
    pub value: f64,
    pub px: f64,
    pub label: String,
}

/// Valeur ronde (1, 2 ou 5 × 10ⁿ) la plus proche de `v` ; `None` si `v`
/// n'est pas un réel strictement positif. Renvoie aussi l'exposant n.
fn nice_value(v: f64) -> Option<(f64, i32)> {
    if !(v > 0.0 && v.is_finite()) {
        return None;
    }
    let n = v.log10().floor() as i32;
    // Candidats autour de v, y compris 10 × 10ⁿ (= 1 × 10ⁿ⁺¹).
    [(1.0, n), (2.0, n), (5.0, n), (1.0, n + 1)]
        .into_iter()
        .map(|(m, e)| (m * 10f64.powi(e), e))
        .filter(|(c, _)| *c > 0.0 && c.is_finite())
        .min_by(|a, b| (a.0 - v).abs().total_cmp(&(b.0 - v).abs()))
}

/// Nombre rond écrit sans zéros inutiles, avec le séparateur décimal de la langue.
fn format_round(v: f64, exp: i32) -> String {
    let decimals = (-exp).max(0) as usize;
    let s = format!("{v:.decimals$}");
    match meteroscope_i18n::decimal_separator() {
        '.' => s,
        sep => s.replace('.', &sep.to_string()),
    }
}

/// Barre d'échelle dont la longueur à l'écran (ou dans l'image) est la plus
/// proche de `target_px`, sachant qu'un pixel vaut `units_per_px` unités.
pub fn scale_bar(units_per_px: f64, unit: &str, target_px: f64) -> Option<ScaleBar> {
    if !(units_per_px > 0.0 && units_per_px.is_finite() && target_px > 0.0) {
        return None;
    }
    let (value, exp) = nice_value(target_px * units_per_px)?;
    let px = value / units_per_px;
    if !(px.is_finite() && px > 0.0) {
        return None;
    }
    let unit = unit.trim();
    let label = if unit.is_empty() { format_round(value, exp) } else { format!("{} {unit}", format_round(value, exp)) };
    Some(ScaleBar { value, px, label })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valeurs_rondes() {
        meteroscope_i18n::set_language("fr");
        let v = |x| nice_value(x).map(|(v, _)| v);
        assert_eq!(v(1.3), Some(1.0));
        assert_eq!(v(1.6), Some(2.0));
        assert_eq!(v(3.4), Some(2.0));
        assert_eq!(v(3.6), Some(5.0));
        assert_eq!(v(37.0), Some(50.0));
        assert_eq!(v(8.0), Some(10.0));
        assert_eq!(v(0.07), Some(0.05));
        assert_eq!(v(5.0), Some(5.0));
        assert_eq!(v(0.0), None);
        assert_eq!(v(-2.0), None);
        assert_eq!(v(f64::NAN), None);
    }

    #[test]
    fn barres_selon_echelle_et_largeur() {
        meteroscope_i18n::set_language("fr");
        // 0,01 mm/px, image de 4000 px : 17,5 % = 700 px = 7 mm → 5 mm = 500 px.
        let b = scale_bar(0.01, "mm", 4000.0 * TARGET_FRACTION).unwrap();
        assert_eq!((b.value, b.px, b.label.as_str()), (5.0, 500.0, "5 mm"));
        // Même échelle, image de 1000 px : 1,75 mm → 2 mm = 200 px.
        let b = scale_bar(0.01, "mm", 1000.0 * TARGET_FRACTION).unwrap();
        assert_eq!((b.value, b.label.as_str()), (2.0, "2 mm"));
        assert!((b.px - 200.0).abs() < 1e-9);
        // Microscopie : 0,0625 µm/px, 2048 px → 22,4 µm → 20 µm = 320 px.
        let b = scale_bar(0.0625, "µm", 2048.0 * TARGET_FRACTION).unwrap();
        assert_eq!((b.value, b.px, b.label.as_str()), (20.0, 320.0, "20 µm"));
        // Petites valeurs : décimales justes, virgule en français.
        let b = scale_bar(0.0002, "mm", 1500.0 * TARGET_FRACTION).unwrap();
        assert_eq!(b.label, "0,05 mm");
        assert!((b.px - 250.0).abs() < 1e-9);
        // Sans étalonnage réel (pixels) : 640 px → 112 px → 100 px.
        let b = scale_bar(1.0, "px", 640.0 * TARGET_FRACTION).unwrap();
        assert_eq!((b.value, b.px, b.label.as_str()), (100.0, 100.0, "100 px"));
        // Grandes valeurs : pas de décimales.
        let b = scale_bar(2.5, "m", 3000.0 * TARGET_FRACTION).unwrap();
        assert_eq!(b.label, "1000 m");
        // Entrées invalides.
        assert!(scale_bar(0.0, "mm", 100.0).is_none());
        assert!(scale_bar(0.1, "mm", 0.0).is_none());
        assert!(scale_bar(f64::INFINITY, "mm", 100.0).is_none());
    }

    #[test]
    fn longueur_proche_de_la_cible() {
        meteroscope_i18n::set_language("fr");
        // Pour toute échelle, la barre reste dans un facteur raisonnable de la cible.
        for k in 0..200 {
            let upp = 10f64.powf(-5.0 + k as f64 * 0.05);
            let b = scale_bar(upp, "mm", 175.0).unwrap();
            assert!(b.px > 175.0 / 1.8 && b.px < 175.0 * 1.6, "{upp} → {}", b.px);
        }
    }

    #[test]
    fn placement_dans_les_coins() {
        let zone = (10.0, 20.0, 200.0, 100.0);
        assert_eq!(Corner::TopLeft.place(zone, (50.0, 30.0), 5.0), (15.0, 25.0));
        assert_eq!(Corner::BottomRight.place(zone, (50.0, 30.0), 5.0), (155.0, 85.0));
        assert_eq!(Corner::TopRight.place(zone, (50.0, 30.0), 5.0), (155.0, 25.0));
        assert_eq!(Corner::BottomLeft.place(zone, (50.0, 30.0), 5.0), (15.0, 85.0));
        // Boîte plus grande que la zone : collée en haut à gauche.
        assert_eq!(Corner::BottomRight.place(zone, (300.0, 300.0), 5.0), (10.0, 20.0));
        // Zone très plate : la boîte tient, pas la marge → la marge se réduit,
        // dans tous les coins (le bas de la boîte ne sort pas de la zone).
        let flat = (0.0, 0.0, 1000.0, 45.0);
        for c in Corner::ALL {
            let (x, y) = c.place(flat, (200.0, 44.0), 17.0);
            assert!(y >= 0.0 && y + 44.0 <= 45.0, "{c:?} : y = {y}");
            assert!(x >= 0.0 && x + 200.0 <= 1000.0);
        }
        // Idem en largeur.
        let (x, _) = Corner::TopLeft.place((0.0, 0.0, 210.0, 500.0), (200.0, 44.0), 17.0);
        assert_eq!(x, 10.0);
    }
}

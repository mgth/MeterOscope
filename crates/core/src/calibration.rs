use serde::{Deserialize, Serialize};

/// Échelle : combien d'unités réelles représente un pixel.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scale {
    pub units_per_px: f64,
    /// Unité de longueur affichée, p. ex. "mm".
    pub unit: String,
}

/// Étalonnage du document. Sans échelle, tout est exprimé en pixels.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    pub scale: Option<Scale>,
}

impl Calibration {
    /// Définit l'échelle à partir d'une longueur en pixels et de sa longueur réelle.
    /// Renvoie `false` (sans rien changer) si les valeurs sont invalides.
    pub fn set_from_reference(&mut self, px_len: f64, real_len: f64, unit: &str) -> bool {
        if !(px_len > 0.0 && real_len > 0.0 && px_len.is_finite() && real_len.is_finite()) {
            return false;
        }
        self.scale = Some(Scale { units_per_px: real_len / px_len, unit: unit.trim().to_owned() });
        true
    }
    pub fn is_calibrated(&self) -> bool {
        self.scale.is_some()
    }
    fn k(&self) -> f64 {
        self.scale.as_ref().map_or(1.0, |s| s.units_per_px)
    }
    /// Convertit une longueur en pixels vers l'unité courante.
    pub fn length(&self, px: f64) -> f64 {
        px * self.k()
    }
    /// Convertit une aire en pixels² vers l'unité courante au carré.
    pub fn area(&self, px2: f64) -> f64 {
        px2 * self.k() * self.k()
    }
    pub fn length_unit(&self) -> &str {
        self.scale.as_ref().map_or("px", |s| s.unit.as_str())
    }
    pub fn area_unit(&self) -> String {
        format!("{}²", self.length_unit())
    }
}

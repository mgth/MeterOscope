//! Préréglages d'échelle : des échelles nommées (« objectif ×10 »,
//! « caméra atelier ») réappliquées d'une image à l'autre, et l'option
//! « garder l'échelle pour l'image suivante ».
//!
//! Le modèle est pur ; la lecture et l'écriture du fichier JSON se font dans
//! `meteroscope-io`, le fichier lui-même dans le dossier de configuration.

use crate::calibration::{Calibration, Scale};
use serde::{Deserialize, Serialize};

/// Une échelle enregistrée sous un nom.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScalePreset {
    pub name: String,
    pub scale: Scale,
}

/// Liste des préréglages et option de conservation de l'échelle.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ScalePresets {
    /// Garder l'échelle courante quand on ouvre une nouvelle image ou qu'on
    /// capture depuis la caméra (un projet reprend toujours la sienne).
    #[serde(default)]
    pub keep_scale: bool,
    #[serde(default)]
    pub presets: Vec<ScalePreset>,
}

/// Échelle utilisable : facteur fini strictement positif et unité non vide.
fn valid_scale(s: &Scale) -> bool {
    s.units_per_px.is_finite() && s.units_per_px > 0.0 && !s.unit.trim().is_empty()
}

/// Deux échelles identiques (même unité, facteur égal aux arrondis près).
pub fn same_scale(a: &Scale, b: &Scale) -> bool {
    a.unit == b.unit && (a.units_per_px - b.units_per_px).abs() <= 1e-9 * a.units_per_px.abs().max(b.units_per_px.abs())
}

impl ScalePresets {
    /// Position du préréglage portant ce nom (casse et espaces ignorés).
    pub fn find(&self, name: &str) -> Option<usize> {
        let key = name.trim().to_lowercase();
        self.presets.iter().position(|p| p.name.trim().to_lowercase() == key)
    }

    /// Enregistre `scale` sous `name`. Un préréglage de même nom est mis à
    /// jour sur place. Renvoie sa position, ou `None` (rien changé) si le nom
    /// est vide ou l'échelle invalide.
    pub fn save(&mut self, name: &str, scale: &Scale) -> Option<usize> {
        let name = name.trim();
        if name.is_empty() || !valid_scale(scale) {
            return None;
        }
        let preset = ScalePreset { name: name.to_owned(), scale: scale.clone() };
        match self.find(name) {
            Some(i) => {
                self.presets[i] = preset;
                Some(i)
            }
            None => {
                self.presets.push(preset);
                Some(self.presets.len() - 1)
            }
        }
    }

    /// Renomme le préréglage `i`. Refusé (`false`) si le nom est vide ou déjà
    /// pris par un autre préréglage.
    pub fn rename(&mut self, i: usize, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() || i >= self.presets.len() || self.find(name).is_some_and(|j| j != i) {
            return false;
        }
        self.presets[i].name = name.to_owned();
        true
    }

    /// Supprime le préréglage `i` et le renvoie.
    pub fn remove(&mut self, i: usize) -> Option<ScalePreset> {
        (i < self.presets.len()).then(|| self.presets.remove(i))
    }

    /// Premier préréglage identique à cette échelle (pour afficher son nom).
    pub fn matching(&self, scale: &Scale) -> Option<&ScalePreset> {
        self.presets.iter().find(|p| same_scale(&p.scale, scale))
    }

    /// Étalonnage d'une nouvelle image : celui en cours si l'option « garder
    /// l'échelle » est cochée, sinon aucun.
    pub fn next_calibration(&self, current: &Calibration) -> Calibration {
        if self.keep_scale { current.clone() } else { Calibration::default() }
    }

    /// Écarte les entrées inutilisables (fichier modifié à la main) : nom
    /// vide, échelle invalide, doublons de nom (le premier est gardé).
    pub fn sanitize(&mut self) {
        let mut kept: Vec<ScalePreset> = Vec::with_capacity(self.presets.len());
        for mut p in std::mem::take(&mut self.presets) {
            p.name = p.name.trim().to_owned();
            p.scale.unit = p.scale.unit.trim().to_owned();
            let dup = kept.iter().any(|k| k.name.to_lowercase() == p.name.to_lowercase());
            if !p.name.is_empty() && valid_scale(&p.scale) && !dup {
                kept.push(p);
            }
        }
        self.presets = kept;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Document;

    fn mm(k: f64) -> Scale {
        Scale { units_per_px: k, unit: "mm".to_owned() }
    }

    #[test]
    fn enregistrer_renommer_supprimer() {
        let mut p = ScalePresets::default();
        assert_eq!(p.save("  ", &mm(0.1)), None);
        assert_eq!(p.save("x", &mm(0.0)), None);
        assert_eq!(p.save("x", &Scale { units_per_px: 1.0, unit: " ".into() }), None);
        assert!(p.presets.is_empty());

        assert_eq!(p.save(" objectif ×10 ", &mm(0.01)), Some(0));
        assert_eq!(p.save("caméra atelier", &mm(0.2)), Some(1));
        assert_eq!(p.presets[0].name, "objectif ×10");
        // Même nom (casse ignorée) : mise à jour sur place.
        assert_eq!(p.save("Objectif ×10", &mm(0.011)), Some(0));
        assert_eq!(p.presets.len(), 2);
        assert_eq!(p.presets[0].scale, mm(0.011));
        assert_eq!(p.presets[0].name, "Objectif ×10");

        assert!(!p.rename(0, "Caméra atelier"), "nom déjà pris");
        assert!(!p.rename(0, " "));
        assert!(!p.rename(5, "z"));
        assert!(p.rename(0, "objectif ×10 (Nikon)"));
        assert!(p.rename(1, "CAMÉRA atelier"), "changer la casse de son propre nom");
        assert_eq!(p.find("objectif ×10 (nikon)"), Some(0));

        assert_eq!(p.remove(0).map(|x| x.name), Some("objectif ×10 (Nikon)".to_owned()));
        assert_eq!(p.remove(3), None);
        assert_eq!(p.presets.len(), 1);
    }

    #[test]
    fn reconnaitre_l_echelle_courante() {
        let mut p = ScalePresets::default();
        p.save("a", &mm(0.1));
        p.save("b", &Scale { units_per_px: 0.1, unit: "cm".into() });
        assert_eq!(p.matching(&mm(0.1 + 1e-14)).map(|x| x.name.as_str()), Some("a"));
        assert_eq!(p.matching(&Scale { units_per_px: 0.1, unit: "cm".into() }).map(|x| x.name.as_str()), Some("b"));
        assert!(p.matching(&mm(0.1001)).is_none());
    }

    #[test]
    fn garder_l_echelle_pour_l_image_suivante() {
        let mut current = Calibration::default();
        current.set_from_reference(100.0, 5.0, "mm");
        let mut p = ScalePresets::default();
        assert!(!p.next_calibration(&current).is_calibrated());
        p.keep_scale = true;
        assert_eq!(p.next_calibration(&current), current);
    }

    #[test]
    fn nettoyage_apres_lecture() {
        let mut p = ScalePresets {
            keep_scale: false,
            presets: vec![
                ScalePreset { name: " a ".into(), scale: mm(0.1) },
                ScalePreset { name: "".into(), scale: mm(0.1) },
                ScalePreset { name: "b".into(), scale: mm(f64::NAN) },
                ScalePreset { name: "c".into(), scale: mm(-1.0) },
                ScalePreset { name: "A".into(), scale: mm(0.3) },
                ScalePreset { name: "d".into(), scale: Scale { units_per_px: 2.0, unit: " µm ".into() } },
            ],
        };
        p.sanitize();
        let names: Vec<_> = p.presets.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["a", "d"]);
        assert_eq!(p.presets[0].scale, mm(0.1));
        assert_eq!(p.presets[1].scale.unit, "µm");
    }

    #[test]
    fn appliquer_est_annulable() {
        let mut doc = Document::new();
        let mut p = ScalePresets::default();
        p.save("a", &mm(0.1));
        assert!(doc.set_scale(Some(p.presets[0].scale.clone())));
        assert!(!doc.set_scale(Some(mm(0.1))), "même échelle : rien à annuler");
        assert_eq!(doc.calibration.scale, Some(mm(0.1)));
        assert!(doc.undo());
        assert!(!doc.calibration.is_calibrated());
        assert!(!doc.can_undo());
        assert!(!doc.set_scale(None));
    }
}

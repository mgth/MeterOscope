//! Fichier JSON des préréglages d'échelle (dossier de configuration).

use meteroscope_core::{ScalePreset, ScalePresets};
use serde_json::Value;

/// Texte JSON indenté, lisible et modifiable à la main.
pub fn scale_presets_to_json(presets: &ScalePresets) -> String {
    serde_json::to_string_pretty(presets).unwrap_or_default()
}

/// Lit le fichier ; un texte illisible donne une liste vide. Chaque entrée
/// est lue à part : une entrée abîmée (champ manquant, mauvais type) est
/// écartée sans faire perdre les autres ni l'option « garder l'échelle ».
pub fn scale_presets_from_json(text: &str) -> ScalePresets {
    let Ok(Value::Object(root)) = serde_json::from_str::<Value>(text) else {
        return ScalePresets::default();
    };
    let keep_scale = root.get("keep_scale").and_then(Value::as_bool).unwrap_or(false);
    let presets = match root.get("presets") {
        Some(Value::Array(items)) => {
            items.iter().filter_map(|v| serde_json::from_value::<ScalePreset>(v.clone()).ok()).collect()
        }
        _ => Vec::new(),
    };
    let mut presets = ScalePresets { keep_scale, presets };
    presets.sanitize();
    presets
}

#[cfg(test)]
mod tests {
    use super::*;
    use meteroscope_core::Scale;

    #[test]
    fn aller_retour_json() {
        let mut p = ScalePresets { keep_scale: true, ..Default::default() };
        p.save("objectif ×10", &Scale { units_per_px: 0.0125, unit: "µm".into() });
        p.save("caméra atelier", &Scale { units_per_px: 0.2, unit: "mm".into() });
        let text = scale_presets_to_json(&p);
        assert!(text.contains("\"objectif ×10\"") && text.contains('\n'), "lisible : {text}");
        assert_eq!(scale_presets_from_json(&text), p);
    }

    #[test]
    fn fichier_abime_ou_incomplet() {
        assert_eq!(scale_presets_from_json("{ pas du json"), ScalePresets::default());
        assert_eq!(scale_presets_from_json(""), ScalePresets::default());
        let p = scale_presets_from_json(r#"{"presets":[{"name":"a","scale":{"units_per_px":0.5,"unit":"mm"}},{"name":"b","scale":{"units_per_px":0,"unit":"mm"}}]}"#);
        assert!(!p.keep_scale);
        assert_eq!(p.presets.len(), 1);
        assert_eq!(p.presets[0].name, "a");
    }

    #[test]
    fn une_entree_abimee_ne_fait_pas_perdre_les_autres() {
        let p = scale_presets_from_json(
            r#"{
              "keep_scale": true,
              "presets": [
                {"name": "a", "scale": {"units_per_px": 0.5, "unit": "mm"}},
                {"name": "sans échelle"},
                {"name": 12, "scale": {"units_per_px": 1, "unit": "mm"}},
                "texte",
                {"name": "b", "scale": {"units_per_px": 2, "unit": "µm"}}
              ]
            }"#,
        );
        assert!(p.keep_scale, "l'option est conservée");
        let names: Vec<_> = p.presets.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);

        // Liste absente ou mal typée : l'option reste lue.
        let p = scale_presets_from_json(r#"{"keep_scale": true, "presets": {"a": 1}}"#);
        assert!(p.keep_scale && p.presets.is_empty());
        assert_eq!(scale_presets_from_json("[1, 2]"), ScalePresets::default());
    }
}

//! Fichier JSON des préréglages d'échelle (dossier de configuration).

use meteroscope_core::ScalePresets;

/// Texte JSON indenté, lisible et modifiable à la main.
pub fn scale_presets_to_json(presets: &ScalePresets) -> String {
    serde_json::to_string_pretty(presets).unwrap_or_default()
}

/// Lit le fichier ; un texte illisible donne une liste vide, les entrées
/// invalides sont écartées.
pub fn scale_presets_from_json(text: &str) -> ScalePresets {
    let mut presets: ScalePresets = serde_json::from_str(text).unwrap_or_default();
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
}

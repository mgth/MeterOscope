//! Préférences retenues dans le dossier de configuration (à côté de la langue).

use meteroscope_io::RotationFilter;
use std::path::PathBuf;

/// Valeur enregistrée pour « sans filtre » (rotation par la carte graphique).
const GPU: &str = "gpu";

fn filter_file() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("meteroscope").join("rotation_filter"))
}

/// Filtre de redressement choisi (`None` : rotation par la carte graphique).
/// Sans préférence enregistrée : le filtre par défaut.
pub fn load_filter() -> Option<RotationFilter> {
    let saved = filter_file().and_then(|f| std::fs::read_to_string(f).ok());
    match saved.as_deref().map(str::trim) {
        Some(GPU) => None,
        Some(id) => Some(RotationFilter::from_id(id).unwrap_or_default()),
        None => Some(RotationFilter::default()),
    }
}

pub fn save_filter(filter: Option<RotationFilter>) {
    let Some(file) = filter_file() else { return };
    let _ = file.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&file, filter.map_or(GPU, RotationFilter::id)));
}

/// Préréglages d'échelle (JSON lisible, à côté des autres préférences).
fn presets_file() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("meteroscope").join("scale_presets.json"))
}

pub fn load_scale_presets() -> meteroscope_core::ScalePresets {
    let saved = presets_file().and_then(|f| std::fs::read_to_string(f).ok());
    saved.map(|s| meteroscope_io::scale_presets_from_json(&s)).unwrap_or_default()
}

pub fn save_scale_presets(presets: &meteroscope_core::ScalePresets) {
    let Some(file) = presets_file() else { return };
    let _ = file.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&file, meteroscope_io::scale_presets_to_json(presets)));
}

/// Dernier outil choisi dans chaque groupe de la palette (JSON lisible :
/// identifiant de groupe → outil).
fn group_tools_file() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("meteroscope").join("tool_groups.json"))
}

pub fn load_group_tools() -> meteroscope_panels::GroupTools {
    let saved = group_tools_file().and_then(|f| std::fs::read_to_string(f).ok());
    saved.map(|s| group_tools_from_json(&s)).unwrap_or_default()
}

pub fn save_group_tools(memory: &meteroscope_panels::GroupTools) {
    let Some(file) = group_tools_file() else { return };
    let _ = file.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&file, group_tools_to_json(memory)));
}

fn group_tools_to_json(memory: &meteroscope_panels::GroupTools) -> String {
    let map: std::collections::BTreeMap<&str, meteroscope_core::Tool> = memory.entries().collect();
    serde_json::to_string_pretty(&map).unwrap_or_default()
}

/// Lecture tolérante : une entrée illisible (outil inconnu d'une autre
/// version) est ignorée sans perdre les autres.
fn group_tools_from_json(s: &str) -> meteroscope_panels::GroupTools {
    let map: std::collections::BTreeMap<String, serde_json::Value> = serde_json::from_str(s).unwrap_or_default();
    map.into_iter().filter_map(|(id, v)| Some((id, serde_json::from_value(v).ok()?))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use meteroscope_core::{ShapeKind, Tool};
    use meteroscope_panels::{GroupTools, tool_group};

    #[test]
    fn outils_des_groupes_aller_retour() {
        let mut memory = GroupTools::default();
        memory.remember(Tool::Draw(ShapeKind::Arc));
        memory.remember(Tool::Draw(ShapeKind::Parallel));
        memory.remember(Tool::Perspective);
        let json = group_tools_to_json(&memory);
        let back = group_tools_from_json(&json);
        assert_eq!(back, memory);
        assert_eq!(back.shown(tool_group(Tool::Draw(ShapeKind::Circle))), Tool::Draw(ShapeKind::Arc));
        assert_eq!(back.shown(tool_group(Tool::Calibrate)), Tool::Perspective);
    }

    #[test]
    fn outils_des_groupes_lecture_tolerante() {
        let back = group_tools_from_json(r#"{"lines": {"Draw": "Parallel"}, "circles": {"Draw": "Inconnu"}}"#);
        assert_eq!(back.entries().collect::<Vec<_>>(), [("lines", Tool::Draw(ShapeKind::Parallel))]);
        assert_eq!(group_tools_from_json("pas du json"), GroupTools::default());
    }
}

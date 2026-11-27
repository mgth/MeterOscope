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

//! Entrées/sorties : chargement d'images, projet, export CSV, image annotée,
//! transformations d'image.
//!
//! CONTRAT PUBLIC (utilisé par `crates/app`) — ne pas changer les signatures
//! existantes ; on peut en ajouter.

mod image_io;
mod perspective;
mod presets;
mod profile;
mod project;
mod render;
mod rotate;
mod series;
mod table;
mod transform;

use meteroscope_i18n::tr;
use anyhow::{Context, Result, anyhow};
use image::RgbaImage;
use meteroscope_core::Document;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub use image_io::can_save_extension;
pub use perspective::warp_perspective;
pub use presets::{scale_presets_from_json, scale_presets_to_json};
pub use profile::{measure_profile, profile_csv_string};
pub use project::PROJECT_FORMAT_VERSION;
pub use series::{
    SERIES_FORMAT_VERSION, SeriesImage, SeriesProject, export_series_csv, image_dimensions, load_series, save_series, series_clipboard_table,
    series_csv_string, thumbnail,
};
pub use rotate::{RotationFilter, rotate_image, rotated_dimensions};
pub use table::format_precise;
pub use transform::transform_point;

/// Extensions d'image acceptées à l'ouverture (minuscules, sans point).
pub const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "bmp", "tif", "tiff", "webp", "gif"];
/// Extension des fichiers projet.
pub const PROJECT_EXTENSION: &str = "meteroscope";

/// Charge une image en RGBA en respectant l'orientation EXIF.
pub fn load_image(path: &Path) -> Result<RgbaImage> {
    image_io::load_image(path)
}

/// Enregistre une image (format selon l'extension ; JPEG sans alpha).
pub fn save_image(image: &RgbaImage, path: &Path) -> Result<()> {
    image_io::save_image(image, path)
}

/// Fichier projet : les mesures + la référence à l'image.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Project {
    /// Chemin de l'image (absolu, ou relatif au fichier projet à l'enregistrement).
    pub image_path: Option<PathBuf>,
    pub document: Document,
}

pub fn save_project(project: &Project, path: &Path) -> Result<()> {
    project::save_project(project, path)
}

/// Charge un projet ; `image_path` est renvoyé résolu en chemin absolu.
pub fn load_project(path: &Path) -> Result<Project> {
    project::load_project(path)
}

/// Tableau des mesures en CSV (séparateur `;`, virgule décimale, UTF-8 avec BOM
/// pour Excel/LibreOffice) : une ligne par mesure, colonnes nom, type, puis
/// valeurs et unités ; suivi d'un bloc de statistiques.
pub fn csv_string(doc: &Document) -> String {
    table::csv_string(doc)
}

/// Même contenu en texte tabulé, pour coller dans un tableur.
pub fn clipboard_table(doc: &Document) -> String {
    table::clipboard_table(doc)
}

pub fn export_csv(doc: &Document, path: &Path) -> Result<()> {
    std::fs::write(path, csv_string(doc))
        .map_err(|e| anyhow!(image_io::describe_io(&e)))
        .with_context(|| tr!("csv.export_failed", file = image_io::file_name(path)))
}

/// Enregistre la courbe d'un profil d'intensité en CSV ([`profile_csv_string`]).
pub fn export_profile_csv(name: &str, analysis: &meteroscope_core::profile::Analysis, doc: &Document, path: &Path) -> Result<()> {
    std::fs::write(path, profile_csv_string(name, analysis, &doc.calibration))
        .map_err(|e| anyhow!(image_io::describe_io(&e)))
        .with_context(|| tr!("csv.export_failed", file = image_io::file_name(path)))
}

/// Dessine les mesures visibles (traits, poignées discrètes, étiquettes de
/// valeur) sur une copie de l'image, à la résolution de l'image.
pub fn render_annotated(image: &RgbaImage, doc: &Document) -> RgbaImage {
    render::render_annotated(image, doc)
}

/// Transformations géométriques de l'image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transform {
    RotateLeft,
    RotateRight,
    FlipHorizontal,
    FlipVertical,
}

/// Applique la transformation à l'image ET aux points des mesures du document
/// (avec `checkpoint`), pour que les mesures restent en place.
pub fn apply_transform(image: &RgbaImage, doc: &mut Document, t: Transform) -> RgbaImage {
    transform::apply_transform(image, doc, t)
}

/// Réglage non destructif luminosité/contraste (chacun dans [-1, 1], 0 = neutre).
pub fn adjust(image: &RgbaImage, brightness: f32, contrast: f32) -> RgbaImage {
    transform::adjust(image, brightness, contrast)
}

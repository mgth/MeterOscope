//! Entrées/sorties : chargement d'images, projet, export CSV, image annotée,
//! transformations d'image.
//!
//! CONTRAT PUBLIC (utilisé par `crates/app`) — ne pas changer les signatures
//! existantes ; on peut en ajouter.

mod fonts;
pub mod detect;
mod image_io;
mod perspective;
mod presets;
mod profile;
mod pdf;
mod project;
mod render;
mod rotate;
mod svg;
mod table;
mod transform;
mod upright;

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
pub use pdf::{ReportInfo, today};
pub use svg::SvgBackground;
pub use project::PROJECT_FORMAT_VERSION;
pub use rotate::{
    Frame, RotationFilter, Straightened, WORK_BUDGET, reduced_size, rotate_image, rotated_dimensions, rotation_work_bytes, straighten,
    straighten_scale,
};
pub use table::format_precise;
pub use transform::{orient_image, sync_orientation, transform_point};

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

/// Image annotée telle que la vue la montre : la photo est redressée de
/// `doc.rotation` avec `filter` et les mesures y sont reportées (une ligne
/// redressée à l'horizontale l'est aussi dans le fichier exporté). Sans
/// redressement, identique à [`render_annotated`].
pub fn render_view(image: &RgbaImage, doc: &Document, filter: RotationFilter) -> RgbaImage {
    if doc.rotation == 0.0 {
        return render::render_annotated(image, doc);
    }
    let s = rotate::straighten(image, doc.rotation, filter, u32::MAX, WORK_BUDGET);
    render::render_annotated_in(&s.image, doc, Some(&s.frame))
}

/// Image annotée en SVG : l'image en fond (intégrée ou liée) et les mesures,
/// étiquettes et annotations en vectoriel, vue redressée comprise.
/// `image` : l'image telle qu'affichée (réglages compris), non pivotée.
pub fn svg_string(image: &RgbaImage, doc: &Document, background: &SvgBackground) -> Result<String> {
    svg::svg_string(image, doc, background)
}

pub fn export_svg(image: &RgbaImage, doc: &Document, background: &SvgBackground, path: &Path) -> Result<()> {
    let ctx = || tr!("export.failed", file = image_io::file_name(path));
    let s = svg_string(image, doc, background).with_context(ctx)?;
    std::fs::write(path, s).map_err(|e| anyhow!(image_io::describe_io(&e))).with_context(ctx)
}

/// Lien vers `image` depuis un SVG enregistré en `svg` : chemin relatif avec
/// des `/` si possible, sinon URL `file:`, caractères spéciaux encodés.
pub fn link_href(image: &Path, svg: &Path) -> String {
    let img = project::absolute_clean(image);
    let dir = project::absolute_clean(svg.parent().unwrap_or(Path::new(".")));
    let encode = |s: &str| {
        let mut o = String::new();
        for b in s.bytes() {
            if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) {
                o.push(b as char);
            } else {
                o.push_str(&format!("%{b:02X}"));
            }
        }
        o
    };
    match project::relative_to(&img, &dir) {
        Some(rel) => encode(&rel),
        None => {
            let s = img.to_string_lossy().replace('\\', "/");
            let s = if s.starts_with('/') { s } else { format!("/{s}") };
            format!("file://{}", encode(&s).replacen("%3A", ":", 1))
        }
    }
}

/// Rapport PDF (A4) : en-tête, image annotée redressée, échelle, tableaux
/// des mesures et des statistiques. `filter` : filtre de redressement.
pub fn pdf_report(image: &RgbaImage, doc: &Document, filter: RotationFilter, info: &ReportInfo) -> Result<Vec<u8>> {
    pdf::pdf_report(image, doc, filter, info)
}

pub fn export_pdf(image: &RgbaImage, doc: &Document, filter: RotationFilter, info: &ReportInfo, path: &Path) -> Result<()> {
    let ctx = || tr!("export.failed", file = image_io::file_name(path));
    let bytes = pdf_report(image, doc, filter, info).with_context(ctx)?;
    std::fs::write(path, bytes).map_err(|e| anyhow!(image_io::describe_io(&e))).with_context(ctx)
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
/// (avec `checkpoint`), pour que les mesures restent en place. L'orientation
/// du document suit : après une annulation, [`sync_orientation`] remet les
/// pixels en accord avec lui.
pub fn apply_transform(image: &RgbaImage, doc: &mut Document, t: Transform) -> RgbaImage {
    transform::apply_transform(image, doc, t)
}

/// Réglage non destructif luminosité/contraste (chacun dans [-1, 1], 0 = neutre).
pub fn adjust(image: &RgbaImage, brightness: f32, contrast: f32) -> RgbaImage {
    transform::adjust(image, brightness, contrast)
}

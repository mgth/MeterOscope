//! Modèle de données de MeterOscope : géométrie, mesures, étalonnage, document.
//!
//! Ce crate est pur (pas d'egui, pas d'E/S). Toutes les coordonnées sont en
//! pixels image (origine en haut à gauche, y vers le bas), en `f64`.

pub mod calibration;
pub mod document;
pub mod geometry;
pub mod groups;
pub mod label;
pub mod measure;
pub mod orientation;
pub mod perspective;
pub mod presets;
pub mod profile;
pub mod rescale;
pub mod scale_bar;
pub mod series;
pub mod stats;

pub use calibration::{Calibration, Scale};
pub use document::Document;
pub use geometry::Pt;
pub use label::{label_text, marker_labels};
pub use orientation::Orientation;
pub use profile::{EdgeMethod, ProfileParams, ProfileResult};
pub use measure::{Measurement, Quantity, Shape, ShapeKind, Tool, Value, PALETTE};
pub use perspective::{Homography, Perspective};
pub use presets::{ScalePreset, ScalePresets};
pub use scale_bar::{Corner, ScaleBar, ScaleBarSettings, scale_bar};
pub use series::{Series, Undone, series_summary};
pub use stats::Stats;

//! Panneaux de l'interface : palette d'outils, liste des mesures et
//! statistiques, boîtes de dialogue d'étalonnage et de correction de perspective.
//!
//! CONTRAT PUBLIC (utilisé par `crates/app`) — ne pas changer les signatures
//! existantes ; on peut en ajouter.

mod calibration;
mod format;
mod group_ui;
mod icons;
mod list;
mod palette;
mod perspective;
mod presets;
mod profile;
mod scale_bar;

pub use calibration::{CalibrationDialog, CalibrationInput, UNITS, read_calibration_input};
pub use format::{format_significant, parse_number, scale_texts, scale_texts_raw};
pub use icons::paint_tool_icon;
pub use list::{measurements_panel, measurements_panel_with_presets, selection};
pub use scale_bar::{corner_name, scale_bar_menu};
pub use perspective::{PerspectiveDialog, PerspectiveInput, read_perspective_input};
pub use profile::{ProfilePanelOutput, edge_method_name, profile_panel};
pub use palette::{
    GROUPS, GroupTools, TOOLS, ToolGroup, tool_description, tool_group, tool_name, tool_palette, tool_shortcut, tool_tooltip,
};

/// Actions demandées depuis le panneau des mesures.
#[derive(Clone, Debug, Default)]
pub struct PanelOutput {
    /// L'utilisateur veut (re)définir l'échelle → activer l'outil Échelle.
    pub start_calibration: bool,
    /// Copier le tableau dans le presse-papiers.
    pub copy_table: bool,
    /// Exporter en CSV.
    pub export_csv: bool,
    /// Les préréglages d'échelle ou l'option « garder l'échelle » ont changé
    /// (à enregistrer).
    pub presets_changed: bool,
}

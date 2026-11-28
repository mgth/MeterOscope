//! Menu de la barre d'échelle : affichage et coin (annulables).

use egui::Ui;
use meteroscope_core::{Corner, Document, ScaleBarSettings};
use meteroscope_i18n::tr;

pub fn corner_name(c: Corner) -> &'static str {
    match c {
        Corner::TopLeft => tr!("scale_bar.top_left"),
        Corner::TopRight => tr!("scale_bar.top_right"),
        Corner::BottomLeft => tr!("scale_bar.bottom_left"),
        Corner::BottomRight => tr!("scale_bar.bottom_right"),
    }
}

/// Sous-menu « Barre d'échelle » : case à cocher et choix du coin. Chaque
/// changement passe par [`Document::set_scale_bar`], donc s'annule par Ctrl+Z.
pub fn scale_bar_menu(ui: &mut Ui, doc: &mut Document) {
    ui.menu_button(tr!("scale_bar.menu"), |ui| {
        let mut s: ScaleBarSettings = doc.scale_bar;
        let calibrated = doc.calibration.is_calibrated();
        let check = ui.checkbox(&mut s.visible, tr!("scale_bar.show"));
        if calibrated {
            check.on_hover_text(tr!("scale_bar.show_tip"));
        } else {
            check.on_hover_text(tr!("scale_bar.needs_scale"));
        }
        ui.separator();
        ui.add_enabled_ui(s.visible, |ui| {
            for c in Corner::ALL {
                ui.radio_value(&mut s.corner, c, corner_name(c));
            }
        });
        doc.set_scale_bar(s);
    });
}

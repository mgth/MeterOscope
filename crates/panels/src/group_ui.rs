//! Saisie du groupe d'une ou plusieurs mesures, avec proposition des groupes existants.

use egui::{Align, Id, Key, Layout, TextEdit, Ui};
use meteroscope_i18n::tr;

/// Champ de groupe : texte libre (validé par Entrée ou en quittant le champ)
/// et menu « … » des groupes existants. `current` : groupe actuel (vide :
/// aucun, ou valeurs différentes pour une sélection multiple). Renvoie le
/// groupe choisi quand il est validé.
pub fn group_editor(
    ui: &mut Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    current: &str,
    groups: &[String],
) -> Option<String> {
    let id = Id::unique(("meteroscope_groupe", salt));
    let mut chosen = None;
    let mut picked = None;
    // De droite à gauche : le bouton « … » d'abord, puis le champ dans la
    // place qui reste. Réserver une largeur fixe au bouton débordait de
    // quelques pixels, ce qui élargissait le panneau à chaque image.
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        ui.menu_button("…", |ui| {
            if groups.is_empty() {
                ui.label(egui::RichText::new(tr!("group.no_groups")).weak());
            }
            for g in groups {
                if ui.selectable_label(g == current, g).clicked() {
                    picked = Some(g.clone());
                }
            }
            ui.separator();
            if ui.selectable_label(current.is_empty(), tr!("group.none")).clicked() {
                picked = Some(String::new());
            }
        })
        .response
        .on_hover_text(tr!("group.pick_tip"));
        let mut text: String = ui.data(|d| d.get_temp(id)).unwrap_or_else(|| current.to_owned());
        let resp = ui.add(
            TextEdit::singleline(&mut text)
                .id(id.with("champ"))
                .desired_width(ui.available_width().max(60.0))
                .hint_text(tr!("group.none")),
        );
        if resp.lost_focus() {
            if !ui.input(|i| i.key_pressed(Key::Escape)) && text.trim() != current {
                chosen = Some(text.trim().to_owned());
            }
            ui.data_mut(|d| d.remove::<String>(id));
        } else if resp.has_focus() {
            ui.data_mut(|d| d.insert_temp(id, text));
        } else {
            // Hors saisie : le champ suit la valeur du document (annuler, autre mesure…).
            ui.data_mut(|d| d.remove::<String>(id));
        }
    });
    // Un groupe pris dans le menu l'emporte sur la saisie qu'il interrompt.
    if picked.is_some() {
        chosen = picked;
    }
    chosen
}

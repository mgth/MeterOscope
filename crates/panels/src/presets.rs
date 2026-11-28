//! Menu « Préréglages » de la carte Échelle : appliquer, enregistrer,
//! renommer, supprimer une échelle nommée, et garder l'échelle d'une image
//! à l'autre.

use crate::format::scale_texts;
use crate::icons::paint_cross;
use crate::list::icon_button;
use egui::containers::menu::{MenuButton, MenuConfig};
use egui::{Button, Id, Key, PopupCloseBehavior, RichText, TextEdit, Ui};
use meteroscope_core::{Document, ScalePresets};
use meteroscope_i18n::tr;

/// Saisies en cours dans le menu (mémoire temporaire d'egui).
#[derive(Clone, Default)]
struct MenuState {
    /// Nom tapé pour enregistrer l'échelle courante.
    new_name: String,
    /// Renommage en cours : (position, texte saisi, focus à demander).
    renaming: Option<(usize, String, bool)>,
}

/// Action choisie dans la liste, appliquée après son parcours.
enum Action {
    Apply(usize),
    StartRename(usize),
    CommitRename(usize, String),
    Remove(usize),
}

/// Bouton « Préréglages ▾ » et son menu. Appliquer un préréglage modifie le
/// document (annulable) ; renvoie `true` si la liste ou l'option ont changé
/// (à enregistrer).
pub(crate) fn presets_menu(ui: &mut Ui, doc: &mut Document, presets: &mut ScalePresets) -> bool {
    let state_id = Id::unique("meteroscope_prereglages_echelle");
    let mut st: MenuState = ui.data(|d| d.get_temp(state_id)).unwrap_or_default();
    let mut changed = false;

    // Le menu reste ouvert pendant la saisie d'un nom ; il se ferme après
    // l'application d'un préréglage.
    let config = MenuConfig::new().close_behavior(PopupCloseBehavior::CloseOnClickOutside);
    let (resp, _) = MenuButton::new(tr!("presets.menu")).config(config).ui(ui, |ui| {
        ui.set_min_width(260.0);
        changed |= menu_contents(ui, doc, presets, &mut st);
    });
    resp.on_hover_text(tr!("presets.menu_tip"));

    ui.data_mut(|d| d.insert_temp(state_id, st));
    changed
}

fn menu_contents(ui: &mut Ui, doc: &mut Document, presets: &mut ScalePresets, st: &mut MenuState) -> bool {
    let mut changed = false;
    let mut action = None;

    if presets.presets.is_empty() {
        ui.label(RichText::new(tr!("presets.none")).weak());
    }
    for (i, p) in presets.presets.iter().enumerate() {
        ui.horizontal(|ui| {
            if let Some((j, text, focus)) = &mut st.renaming
                && *j == i
            {
                let resp = ui.add(TextEdit::singleline(text).desired_width(170.0).hint_text(tr!("presets.name_hint")));
                if std::mem::take(focus) {
                    resp.request_focus();
                }
                if ui.input(|inp| inp.key_pressed(Key::Escape)) {
                    st.renaming = None;
                } else if resp.lost_focus() {
                    action = Some(Action::CommitRename(i, text.clone()));
                }
                return;
            }
            let current = doc.calibration.scale.as_ref().is_some_and(|s| meteroscope_core::presets::same_scale(s, &p.scale));
            let (main, _) = scale_texts(&p.scale);
            let tip = format!("{main}\n{}", tr!("presets.apply_tip"));
            if ui.selectable_label(current, &p.name).on_hover_text(tip).clicked() {
                action = Some(Action::Apply(i));
            }
            ui.label(RichText::new(main).weak().small());
            if ui.small_button(tr!("presets.rename_tip")).clicked() {
                action = Some(Action::StartRename(i));
            }
            let error = ui.visuals().error_fg_color;
            if icon_button(ui, tr!("presets.delete_tip"), paint_cross, Some(error)).on_hover_text(tr!("presets.delete_tip")).clicked() {
                action = Some(Action::Remove(i));
            }
        });
    }

    match action {
        Some(Action::Apply(i)) => {
            doc.set_scale(Some(presets.presets[i].scale.clone()));
            st.renaming = None;
            ui.close();
        }
        Some(Action::StartRename(i)) => st.renaming = Some((i, presets.presets[i].name.clone(), true)),
        Some(Action::CommitRename(i, name)) => {
            changed |= presets.presets[i].name != name.trim() && presets.rename(i, &name);
            st.renaming = None;
        }
        Some(Action::Remove(i)) => {
            changed |= presets.remove(i).is_some();
            st.renaming = None;
        }
        None => {}
    }

    // Enregistrer l'échelle courante.
    ui.separator();
    match doc.calibration.scale.clone() {
        None => {
            ui.label(RichText::new(tr!("presets.need_scale")).weak());
        }
        Some(scale) => {
            ui.label(tr!("presets.save_label"));
            ui.horizontal(|ui| {
                let field = ui.add(
                    TextEdit::singleline(&mut st.new_name)
                        .id(Id::unique("meteroscope_prereglage_nom"))
                        .desired_width(170.0)
                        .hint_text(tr!("presets.name_hint")),
                );
                let name_ok = !st.new_name.trim().is_empty();
                let tip = if presets.find(&st.new_name).is_some() { tr!("presets.replace_tip") } else { tr!("presets.save_tip") };
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                let clicked = ui.add_enabled(name_ok, Button::new(tr!("presets.save"))).on_hover_text(tip).clicked();
                if (clicked || enter) && presets.save(&st.new_name, &scale).is_some() {
                    st.new_name.clear();
                    changed = true;
                }
            });
        }
    }

    ui.separator();
    if ui.checkbox(&mut presets.keep_scale, tr!("presets.keep")).on_hover_text(tr!("presets.keep_tip")).changed() {
        changed = true;
    }
    changed
}

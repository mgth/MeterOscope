//! Panneau de droite : échelle, liste des mesures, statistiques, actions.

use meteroscope_i18n::tr;
use crate::PanelOutput;
use crate::format::scale_texts;
use crate::icons::{paint_cross, paint_eye};
use egui::text::CCursorRange;
use egui::{
    Align, Button, CollapsingHeader, Color32, Frame, Grid, Key, Label, Layout, Popup, Rect, RichText, ScrollArea,
    Sense, Stroke, TextEdit, Ui, UiBuilder, Vec2, vec2,
};
use meteroscope_core::measure::format_number;
use meteroscope_core::{Document, Measurement, PALETTE, Quantity, Shape, ShapeKind, Stats};

const ROW_H: f32 = 30.0;
const ICON_BTN: f32 = 22.0;

/// État d'interface conservé d'une frame à l'autre (mémoire temporaire d'egui).
#[derive(Clone, Default)]
struct ListState {
    /// Renommage en cours : (id, texte saisi, focus à demander).
    renaming: Option<(u64, String, bool)>,
    /// Sélection vue à la frame précédente (pour défiler quand elle change).
    last_selected: Option<u64>,
    /// Hauteur de la zone du bas (statistiques + boutons) à la frame précédente.
    bottom_h: f32,
}

/// Action demandée sur une ligne, appliquée après le parcours de la liste.
enum RowAction {
    Select,
    StartRename,
    /// Saisie en cours (conservée pour la frame suivante).
    EditRename(String),
    CommitRename(String),
    CancelRename,
    SetColor([u8; 3]),
    ToggleVisible,
    Remove,
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// Panneau latéral : état de l'échelle, liste des mesures (sélection,
/// renommage, couleur, visibilité, suppression), valeurs de la mesure
/// sélectionnée, statistiques par type.
pub fn measurements_panel(ui: &mut Ui, doc: &mut Document) -> PanelOutput {
    let mut out = PanelOutput::default();
    let state_id = ui.scope_id().with("meteroscope_panneau_mesures");
    let mut st: ListState = ui.data(|d| d.get_temp(state_id)).unwrap_or_default();

    ui.add_space(6.0);
    scale_card(ui, doc, &mut out);
    ui.add_space(10.0);

    ui.horizontal(|ui| {
        ui.label(RichText::new(tr!("panel.measurements")).strong());
        if !doc.measurements.is_empty() {
            ui.label(RichText::new(format!("({})", doc.measurements.len())).weak());
        }
    });

    // La liste prend toute la hauteur restante, moins la zone du bas.
    let list_h = (ui.available_height() - st.bottom_h - ui.spacing().item_spacing.y).max(90.0);
    let selection_changed = doc.selected != st.last_selected;
    let mut actions: Vec<(u64, RowAction)> = Vec::new();
    ScrollArea::vertical()
        .id_salt("meteroscope_liste_mesures")
        .auto_shrink([false, false])
        .max_height(list_h)
        .show(ui, |ui| {
            if doc.measurements.is_empty() {
                empty_hint(ui);
            }
            for m in &doc.measurements {
                measurement_row(ui, doc, m.id, &st, selection_changed, &mut actions);
            }
        });
    st.last_selected = doc.selected;
    apply_actions(doc, &mut st, actions);
    if st.renaming.as_ref().is_some_and(|(id, ..)| doc.get(*id).is_none()) {
        st.renaming = None;
    }

    // Zone du bas : statistiques puis boutons ; sa hauteur sert à la frame suivante.
    let top = ui.cursor().top();
    ui.separator();
    stats_section(ui, doc);
    ui.add_space(4.0);
    actions_bar(ui, doc, &mut out);
    ui.add_space(4.0);
    let h = ui.cursor().top() - top;
    if (h - st.bottom_h).abs() > 0.5 {
        st.bottom_h = h;
        ui.ctx().request_repaint();
    }

    ui.data_mut(|d| d.insert_temp(state_id, st));
    out
}

/// Carte « Échelle » en tête du panneau.
fn scale_card(ui: &mut Ui, doc: &mut Document, out: &mut PanelOutput) {
    Frame::group(ui.style()).inner_margin(10.0).corner_radius(6.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(tr!("panel.scale")).strong());
        ui.add_space(2.0);
        match doc.calibration.scale.clone() {
            None => {
                ui.label(RichText::new(tr!("panel.scale_unset")).color(ui.visuals().warn_fg_color));
                ui.add_space(6.0);
                let sel = ui.visuals().selection;
                let button = Button::new(RichText::new(tr!("calib.title")).strong().color(sel.stroke.color))
                    .fill(sel.bg_fill)
                    .min_size(vec2(0.0, 32.0));
                let clicked = ui
                    .vertical_centered_justified(|ui| ui.add(button))
                    .inner
                    .on_hover_text(tr!("panel.scale_set_tip"))
                    .clicked();
                if clicked {
                    out.start_calibration = true;
                }
            }
            Some(scale) => {
                let (main, inverse) = scale_texts(&scale);
                ui.label(RichText::new(main).size(15.0).strong());
                ui.label(RichText::new(inverse).weak());
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if ui.button(tr!("panel.scale_change")).on_hover_text(tr!("panel.scale_change_tip")).clicked() {
                        out.start_calibration = true;
                    }
                    if ui.button(tr!("panel.scale_remove")).on_hover_text(tr!("panel.scale_remove_tip")).clicked() {
                        doc.checkpoint();
                        doc.calibration.scale = None;
                    }
                });
            }
        }
    });
}

fn empty_hint(ui: &mut Ui) {
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(tr!("panel.empty")).weak());
        ui.add_space(4.0);
        ui.add(Label::new(RichText::new(tr!("panel.empty_help")).weak()).wrap());
    });
}

/// Valeur principale affichée à droite de la ligne.
fn main_value(doc: &Document, shape: &Shape) -> String {
    match shape {
        Shape::Marker { p } => format!("x {:.0} · y {:.0}", p.x, p.y),
        s => s.values(&doc.calibration).first().map(|v| v.text()).unwrap_or_else(|| "—".to_owned()),
    }
}

fn measurement_row(
    ui: &mut Ui,
    doc: &Document,
    id: u64,
    st: &ListState,
    selection_changed: bool,
    actions: &mut Vec<(u64, RowAction)>,
) {
    let selected = doc.selected == Some(id);
    // Ligne hors de la zone visible (longues listes, p. ex. après une
    // détection) : on réserve sa place sans la dessiner.
    let row_rect = Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), ROW_H));
    if !selected && !ui.is_rect_visible(row_rect) {
        ui.allocate_space(row_rect.size());
        return;
    }
    let Some(m) = doc.get(id) else { return };
    let visuals = ui.visuals().clone();
    let (rect, row) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    row.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, &m.name));

    // Fond : sélection bien visible, survol discret.
    if selected {
        ui.painter().rect_filled(rect, 5.0, visuals.selection.bg_fill);
    } else if row.hovered() {
        ui.painter().rect_filled(rect, 5.0, visuals.widgets.hovered.weak_bg_fill);
    }

    // Contenu de la ligne, par-dessus la zone cliquable (prioritaire au clic).
    let mut content = ui.new_child(
        UiBuilder::new().max_rect(rect.shrink2(vec2(6.0, 0.0))).layout(Layout::left_to_right(Align::Center)),
    );
    let renaming = st.renaming.as_ref().filter(|(rid, ..)| *rid == id);
    row_content(&mut content, doc, m, selected, renaming, actions);

    if row.clicked() {
        actions.push((id, RowAction::Select));
    }
    if row.double_clicked() {
        actions.push((id, RowAction::StartRename));
    }
    if selected {
        let details = values_box(ui, doc, m);
        ui.add_space(4.0);
        if selection_changed {
            // Amène la ligne et ses valeurs dans la zone visible.
            ui.scroll_to_rect(row.rect.union(details.rect), None);
        }
    }
}

fn row_content(
    ui: &mut Ui,
    doc: &Document,
    m: &Measurement,
    selected: bool,
    renaming: Option<&(u64, String, bool)>,
    actions: &mut Vec<(u64, RowAction)>,
) {
    let id = m.id;
    let visuals = ui.visuals().clone();
    let text_color = if selected {
        visuals.selection.stroke.color
    } else if m.visible {
        visuals.text_color()
    } else {
        visuals.weak_text_color()
    };
    ui.spacing_mut().item_spacing.x = 6.0;

    // Pastille de couleur → choix dans la palette.
    let swatch = color_swatch(ui, rgb(m.color), m.visible).on_hover_text(tr!("item.color_tip"));
    Popup::menu(&swatch).show(|ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            for (i, c) in PALETTE.into_iter().enumerate() {
                if color_choice(ui, rgb(c), c == m.color, i).clicked() {
                    actions.push((id, RowAction::SetColor(c)));
                }
            }
        });
    });

    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if icon_button(ui, tr!("item.delete"), paint_cross, Some(visuals.error_fg_color))
            .on_hover_text(tr!("item.delete_tip"))
            .clicked()
        {
            actions.push((id, RowAction::Remove));
        }
        let visible = m.visible;
        let eye_tip = if visible { tr!("item.hide") } else { tr!("item.show") };
        if icon_button(ui, eye_tip, move |p, r, c| paint_eye(p, r, c, visible), None).on_hover_text(eye_tip).clicked() {
            actions.push((id, RowAction::ToggleVisible));
        }

        if renaming.is_none() {
            ui.add(Label::new(RichText::new(main_value(doc, &m.shape)).color(text_color)).selectable(false));
        }

        // Nom (ou champ de renommage) dans l'espace restant, aligné à gauche.
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| match renaming {
            Some((_, text, focus)) => {
                let mut text = text.clone();
                let mut output =
                    TextEdit::singleline(&mut text).desired_width(f32::INFINITY).hint_text(tr!("item.name_hint")).show(ui);
                let resp = output.response.response.clone();
                if *focus {
                    resp.request_focus();
                    output.state.cursor.set_char_range(Some(CCursorRange::select_all_str(&text)));
                    output.state.store(ui.ctx(), resp.id);
                }
                if resp.lost_focus() {
                    if ui.input(|i| i.key_pressed(Key::Escape)) {
                        actions.push((id, RowAction::CancelRename));
                    } else {
                        actions.push((id, RowAction::CommitRename(text)));
                    }
                } else {
                    actions.push((id, RowAction::EditRename(text)));
                }
            }
            None => {
                let mut name = RichText::new(&m.name).color(text_color);
                if selected {
                    name = name.strong();
                }
                ui.add(Label::new(name).truncate().selectable(false));
            }
        });
    });
}

/// Encart des valeurs de la mesure sélectionnée.
fn values_box(ui: &mut Ui, doc: &Document, m: &Measurement) -> egui::Response {
    let visuals = ui.visuals().clone();
    Frame::new()
        .fill(visuals.faint_bg_color)
        .stroke(Stroke::new(1.0, visuals.selection.bg_fill))
        .corner_radius(5.0)
        .inner_margin(egui::Margin { left: 28, right: 8, top: 6, bottom: 6 })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            Grid::new(("meteroscope_valeurs", m.id)).num_columns(2).spacing([14.0, 3.0]).show(ui, |ui| {
                ui.label(RichText::new(tr!("item.type")).weak());
                ui.label(m.shape.kind().label());
                ui.end_row();
                match &m.shape {
                    Shape::Marker { p } => {
                        ui.label(RichText::new(tr!("item.position")).weak());
                        ui.label(format!("x {} · y {} px", format_number(p.x), format_number(p.y)));
                        ui.end_row();
                    }
                    s => {
                        for v in s.values(&doc.calibration) {
                            ui.label(RichText::new(v.quantity.label()).weak());
                            ui.label(RichText::new(v.text()).strong());
                            ui.end_row();
                        }
                    }
                }
                if !m.visible {
                    ui.label("");
                    ui.label(RichText::new(tr!("item.hidden")).weak().italics());
                    ui.end_row();
                }
            });
        })
        .response
}

fn apply_actions(doc: &mut Document, st: &mut ListState, actions: Vec<(u64, RowAction)>) {
    for (id, action) in actions {
        match action {
            RowAction::Select => doc.selected = Some(id),
            RowAction::StartRename => {
                if let Some(m) = doc.get(id) {
                    st.renaming = Some((id, m.name.clone(), true));
                    doc.selected = Some(id);
                }
            }
            RowAction::EditRename(text) => {
                if st.renaming.as_ref().is_some_and(|r| r.0 == id) {
                    st.renaming = Some((id, text, false));
                }
            }
            RowAction::CommitRename(text) => {
                if st.renaming.as_ref().is_some_and(|r| r.0 == id) {
                    st.renaming = None;
                }
                let name = text.trim();
                if !name.is_empty() && doc.get(id).is_some_and(|m| m.name != name) {
                    doc.checkpoint();
                    if let Some(m) = doc.get_mut(id) {
                        m.name = name.to_owned();
                    }
                }
            }
            RowAction::CancelRename => {
                if st.renaming.as_ref().is_some_and(|r| r.0 == id) {
                    st.renaming = None;
                }
            }
            RowAction::SetColor(c) => {
                if doc.get(id).is_some_and(|m| m.color != c) {
                    doc.checkpoint();
                    if let Some(m) = doc.get_mut(id) {
                        m.color = c;
                    }
                }
            }
            RowAction::ToggleVisible => {
                if doc.get(id).is_some() {
                    doc.checkpoint();
                    if let Some(m) = doc.get_mut(id) {
                        m.visible = !m.visible;
                    }
                }
            }
            RowAction::Remove => {
                doc.remove(id);
            }
        }
    }
}

/// Pastille ronde cliquable.
fn color_swatch(ui: &mut Ui, color: Color32, filled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tr!("item.color")));
    let painter = ui.painter();
    let r = if resp.hovered() { 7.5 } else { 6.5 };
    let outline = ui.visuals().widgets.noninteractive.fg_stroke.color.gamma_multiply(0.6);
    if filled {
        painter.circle(rect.center(), r, color, Stroke::new(1.0, outline));
    } else {
        // Mesure masquée : simple anneau.
        painter.circle_stroke(rect.center(), r - 1.0, Stroke::new(2.0, color));
    }
    resp
}

/// Case de couleur dans le choix de la palette.
fn color_choice(ui: &mut Ui, color: Color32, current: bool, index: usize) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, current, tr!("item.color_n", n = index + 1)));
    let painter = ui.painter();
    let outline = ui.visuals().widgets.noninteractive.fg_stroke.color;
    painter.circle(rect.center(), if resp.hovered() { 10.5 } else { 9.0 }, color, Stroke::new(1.0, outline.gamma_multiply(0.5)));
    if current {
        painter.circle_stroke(rect.center(), 11.5, Stroke::new(2.0, ui.visuals().selection.stroke.color));
    }
    resp
}

/// Petit bouton carré avec icône dessinée.
fn icon_button(
    ui: &mut Ui,
    label: &str,
    paint: impl FnOnce(&egui::Painter, Rect, Color32),
    hover_color: Option<Color32>,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(ICON_BTN), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let visuals = ui.style().interact(&resp);
    let mut color = visuals.fg_stroke.color;
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, visuals.weak_bg_fill);
        if let Some(c) = hover_color {
            color = c;
        }
    }
    paint(ui.painter(), rect.shrink(3.0), color);
    resp
}

/// Statistiques par type (repliable) : un tableau compact, une ligne par type.
fn stats_section(ui: &mut Ui, doc: &Document) {
    CollapsingHeader::new(RichText::new(tr!("stats.title")).strong())
        .id_salt("meteroscope_statistiques")
        .default_open(true)
        .show(ui, |ui| {
            let summary = doc.summary();
            let markers = doc.marker_count();
            if summary.is_empty() && markers == 0 {
                ui.label(RichText::new(tr!("stats.empty")).weak());
                return;
            }
            // Hauteur plafonnée : la liste des mesures reste prioritaire.
            let max_h = (ui.ctx().content_rect().height() * 0.3).max(110.0);
            ScrollArea::vertical().id_salt("meteroscope_stats_defilement").max_height(max_h).show(ui, |ui| {
                if !summary.is_empty() {
                    stats_table(ui, &summary);
                }
                if markers > 0 {
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(tr!("stats.markers")).strong());
                        ui.label(RichText::new(format!("n = {markers}")).weak());
                    });
                }
            });
        });
}

fn stats_table(ui: &mut Ui, summary: &[(ShapeKind, Quantity, String, Stats)]) {
    // Colonnes de largeur minimale commune : les grilles successives restent alignées.
    const COL_W: f32 = 54.0;
    let grid = |salt: &str| Grid::new(("meteroscope_stats", salt.to_owned())).num_columns(4).min_col_width(COL_W).spacing([8.0, 0.0]);
    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
    grid("entete").show(ui, |ui| {
        for h in [tr!("stats.mean"), tr!("stats.std_dev"), tr!("stats.min"), tr!("stats.max")] {
            ui.label(RichText::new(h).small().weak());
        }
        ui.end_row();
    });
    for (kind, quantity, unit, s) in summary {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(kind.label()).strong());
            let what = if quantity.label() == kind.label() {
                format!("({unit})")
            } else {
                format!("{} ({unit})", quantity.label())
            };
            ui.label(RichText::new(what).weak());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(format!("n = {}", s.n)).weak());
            });
        });
        grid(kind.label()).show(ui, |ui| {
            let sd = if s.n < 2 { "—".to_owned() } else { format_number(s.sd) };
            for v in [format_number(s.mean), sd, format_number(s.min), format_number(s.max)] {
                ui.label(v);
            }
            ui.end_row();
        });
    }
}

/// Boutons du bas : copier, exporter ; « Tout effacer » à l'écart, à droite.
fn actions_bar(ui: &mut Ui, doc: &mut Document, out: &mut PanelOutput) {
    let empty = doc.measurements.is_empty();
    ui.add_enabled_ui(!empty, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().button_padding = vec2(8.0, 5.0);
            if ui.button(tr!("panel.copy")).on_hover_text(tr!("panel.copy_tip")).clicked() {
                out.copy_table = true;
            }
            if ui.button(tr!("panel.export_csv")).on_hover_text(tr!("panel.export_csv_tip")).clicked() {
                out.export_csv = true;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(tr!("panel.clear_all")).on_hover_text(tr!("panel.clear_all_tip")).clicked() {
                    doc.clear();
                }
            });
        });
    });
}

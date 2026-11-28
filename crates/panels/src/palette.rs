//! Palette d'outils verticale et raccourcis clavier.

use meteroscope_i18n::tr;
use crate::icons::paint_tool_icon;
use egui::{Align2, FontId, Key, Rect, RichText, ScrollArea, Sense, Ui, WidgetInfo, pos2, vec2};
use meteroscope_core::{ShapeKind, Tool};

/// Tous les outils, dans l'ordre de la palette.
pub const TOOLS: [Tool; 19] = [
    Tool::Select,
    Tool::Calibrate,
    Tool::Straighten,
    Tool::Draw(ShapeKind::Length),
    Tool::Draw(ShapeKind::Path),
    Tool::Draw(ShapeKind::PointLine),
    Tool::Draw(ShapeKind::Parallel),
    Tool::Draw(ShapeKind::Angle),
    Tool::Draw(ShapeKind::LineAngle),
    Tool::Draw(ShapeKind::Area),
    Tool::Draw(ShapeKind::Rect),
    Tool::Draw(ShapeKind::Rect3),
    Tool::Draw(ShapeKind::Ellipse),
    Tool::Draw(ShapeKind::Circle),
    Tool::Draw(ShapeKind::Circle3),
    Tool::Draw(ShapeKind::Arc),
    Tool::Draw(ShapeKind::Marker),
    Tool::Draw(ShapeKind::Text),
    Tool::Draw(ShapeKind::Arrow),
];

/// Largeur fixe des boutons (et donc du panneau).
const BUTTON_W: f32 = 76.0;
const BUTTON_H: f32 = 50.0;
const ICON: f32 = 22.0;

/// Nom complet de l'outil.
pub fn tool_name(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => tr!("tool.select"),
        Tool::Calibrate => tr!("tool.scale"),
        Tool::Straighten => tr!("tool.straighten"),
        Tool::Draw(k) => k.label(),
    }
}

/// Nom court affiché sous l'icône (tient dans la largeur du bouton).
fn short_name(tool: Tool) -> &'static str {
    match tool {
        Tool::Draw(ShapeKind::Circle3) => tr!("tool.circle3_short"),
        Tool::Draw(ShapeKind::Rect3) => tr!("tool.rect3_short"),
        Tool::Draw(ShapeKind::PointLine) => tr!("tool.point_line_short"),
        Tool::Draw(ShapeKind::Parallel) => tr!("tool.parallel_short"),
        t => tool_name(t),
    }
}

/// Description d'une ligne (infobulle).
pub fn tool_description(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => tr!("tool_help.select"),
        Tool::Calibrate => tr!("tool_help.scale"),
        Tool::Straighten => tr!("tool_help.straighten"),
        Tool::Draw(k) => match k {
            ShapeKind::Length => tr!("tool_help.length"),
            ShapeKind::Path => tr!("tool_help.path"),
            ShapeKind::Angle => tr!("tool_help.angle"),
            ShapeKind::Area => tr!("tool_help.area"),
            ShapeKind::Rect => tr!("tool_help.rect"),
            ShapeKind::Circle => tr!("tool_help.circle"),
            ShapeKind::Circle3 => tr!("tool_help.circle3"),
            ShapeKind::Marker => tr!("tool_help.marker"),
            ShapeKind::LineAngle => tr!("tool_help.line_angle"),
            ShapeKind::Rect3 => tr!("tool_help.rect3"),
            ShapeKind::PointLine => tr!("tool_help.point_line"),
            ShapeKind::Parallel => tr!("tool_help.parallel"),
            ShapeKind::Ellipse => tr!("tool_help.ellipse"),
            ShapeKind::Arc => tr!("tool_help.arc"),
            ShapeKind::Text => tr!("tool_help.text"),
            ShapeKind::Arrow => tr!("tool_help.arrow"),
        },
    }
}

/// Raccourci clavier (une lettre, sans modificateur) associé à un outil.
///
/// S Sélection · E Échelle · H Horizon (redresser) · L Longueur · B ligne
/// Brisée (chemin) · A Angle · I Inclinaison · P Polygone (surface) ·
/// R Rectangle · O rectangle Orienté (trois points) · C Cercle · T cercle Trois points · M Marqueur ·
/// D Distance point-droite · G écart (« Gap ») entre parallèles · V ellipse (oVale) · U arc (en U).
/// X teXte · F Flèche.
pub fn tool_shortcut(tool: Tool) -> Option<Key> {
    Some(match tool {
        Tool::Select => Key::S,
        Tool::Calibrate => Key::E,
        Tool::Straighten => Key::H,
        Tool::Draw(k) => match k {
            ShapeKind::Length => Key::L,
            ShapeKind::Path => Key::B,
            ShapeKind::Angle => Key::A,
            ShapeKind::Area => Key::P,
            ShapeKind::Rect => Key::R,
            ShapeKind::Circle => Key::C,
            ShapeKind::Circle3 => Key::T,
            ShapeKind::Marker => Key::M,
            ShapeKind::LineAngle => Key::I,
            ShapeKind::Rect3 => Key::O,
            ShapeKind::PointLine => Key::D,
            ShapeKind::Parallel => Key::G,
            ShapeKind::Ellipse => Key::V,
            ShapeKind::Arc => Key::U,
            ShapeKind::Text => Key::X,
            ShapeKind::Arrow => Key::F,
        },
    })
}

/// Texte d'infobulle complet : description + raccourci.
pub fn tool_tooltip(tool: Tool) -> String {
    match tool_shortcut(tool) {
        Some(k) => format!("{}\n{}", tool_description(tool), tr!("tool.shortcut", key = k.name())),
        None => tool_description(tool).to_owned(),
    }
}

/// Palette d'outils verticale (gros boutons avec icône dessinée + nom,
/// raccourci en infobulle). Modifie `tool` au clic. Les outils sont groupés
/// par famille, séparées d'un trait ; si la hauteur ne suffit pas pour une
/// seule colonne, chaque famille se range deux par deux (aucun outil caché).
pub fn tool_palette(ui: &mut Ui, tool: &mut Tool) {
    let columns = palette_columns(ui.available_height());
    let gap = ui.spacing().item_spacing.x;
    ui.set_width(BUTTON_W * columns as f32 + gap * (columns - 1) as f32);
    ScrollArea::vertical()
        .id_salt("meteroscope_palette_outils")
        .auto_shrink([true, true])
        .content_margin(0.0)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = ROW_GAP;
            ui.add_space(4.0);
            for (i, group) in families().iter().enumerate() {
                if i > 0 {
                    ui.add_space(2.0);
                    ui.separator();
                    ui.add_space(2.0);
                }
                for row in group.chunks(columns) {
                    ui.horizontal(|ui| {
                        for t in row {
                            if tool_button(ui, *t, *tool == *t).clicked() {
                                *tool = *t;
                            }
                        }
                    });
                }
            }
            ui.add_space(4.0);
        });
}

/// Espace vertical entre deux boutons.
const ROW_GAP: f32 = 3.0;
/// Hauteur prise par un séparateur de familles (marges comprises).
const SEPARATOR_H: f32 = 12.0;

/// Famille d'un outil : réglages de l'image, longueurs et distances, angles,
/// surfaces, cercles et arcs, comptage.
fn family(tool: Tool) -> u8 {
    match tool {
        Tool::Select | Tool::Calibrate | Tool::Straighten => 0,
        Tool::Draw(k) => match k {
            ShapeKind::Length | ShapeKind::Path | ShapeKind::PointLine | ShapeKind::Parallel => 1,
            ShapeKind::Angle | ShapeKind::LineAngle => 2,
            ShapeKind::Area | ShapeKind::Rect | ShapeKind::Rect3 | ShapeKind::Ellipse => 3,
            ShapeKind::Circle | ShapeKind::Circle3 | ShapeKind::Arc => 4,
            ShapeKind::Marker => 5,
            ShapeKind::Text | ShapeKind::Arrow => 6,
        },
    }
}

/// Outils groupés par famille, dans l'ordre de la palette.
fn families() -> Vec<Vec<Tool>> {
    TOOLS.chunk_by(|a, b| family(*a) == family(*b)).map(<[Tool]>::to_vec).collect()
}

/// Hauteur de la palette sur `columns` colonnes.
fn palette_height(columns: usize) -> f32 {
    let groups = families();
    let rows: usize = groups.iter().map(|g| g.len().div_ceil(columns)).sum();
    8.0 + rows as f32 * (BUTTON_H + ROW_GAP) + (groups.len() - 1) as f32 * SEPARATOR_H
}

/// Une colonne si tout tient dans la hauteur `available`, sinon deux.
fn palette_columns(available: f32) -> usize {
    if palette_height(1) <= available { 1 } else { 2 }
}

/// Un bouton de la palette : icône au-dessus, nom dessous, lettre du
/// raccourci discrète dans le coin.
fn tool_button(ui: &mut Ui, tool: Tool, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(BUTTON_W, BUTTON_H), Sense::click());
    response.widget_info(|| WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), selected, tool_name(tool)));

    if ui.is_rect_visible(rect) {
        let visuals = ui.style().interact_selectable(&response, selected);
        let painter = ui.painter();
        if selected || response.hovered() || response.has_focus() {
            painter.rect_filled(rect, 6.0, visuals.weak_bg_fill);
        }
        if selected {
            painter.rect_stroke(rect, 6.0, ui.visuals().selection.stroke, egui::StrokeKind::Inside);
        }
        let color = if selected { ui.visuals().selection.stroke.color } else { visuals.fg_stroke.color };

        let icon_rect = Rect::from_center_size(pos2(rect.center().x, rect.top() + 6.0 + ICON / 2.0), vec2(ICON, ICON));
        paint_tool_icon(painter, icon_rect, tool, color);

        painter.text(
            pos2(rect.center().x, rect.bottom() - 6.0),
            Align2::CENTER_BOTTOM,
            short_name(tool),
            FontId::proportional(11.5),
            color,
        );
        if let Some(k) = tool_shortcut(tool) {
            painter.text(
                rect.right_top() + vec2(-5.0, 3.0),
                Align2::RIGHT_TOP,
                k.name(),
                FontId::proportional(9.5),
                ui.visuals().weak_text_color(),
            );
        }
    }

    response.on_hover_ui(|ui| {
        ui.label(RichText::new(tool_name(tool)).strong());
        ui.label(tool_description(tool));
        if let Some(k) = tool_shortcut(tool) {
            ui.label(RichText::new(tr!("tool.shortcut", key = k.name())).weak());
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn raccourcis_sans_collision() {
        meteroscope_i18n::set_language("fr");
        let mut seen = HashSet::new();
        for t in TOOLS {
            let k = tool_shortcut(t).expect("chaque outil a un raccourci");
            assert!(seen.insert(k), "raccourci {k:?} en double");
            // Une lettre simple, sans conflit avec Échap / Suppr / Retour arrière.
            assert_eq!(k.name().len(), 1, "{k:?} n'est pas une lettre");
            assert!(k.name().chars().all(|c| c.is_ascii_uppercase()));
        }
        assert_eq!(seen.len(), 19);
    }

    #[test]
    fn tous_les_types_sont_dans_la_palette() {
        meteroscope_i18n::set_language("fr");
        for k in ShapeKind::ALL {
            assert!(TOOLS.contains(&Tool::Draw(k)));
        }
        assert!(tool_tooltip(Tool::Draw(ShapeKind::Length)).ends_with("Raccourci : L"));
    }

    #[test]
    fn familles_sans_outil_cache() {
        meteroscope_i18n::set_language("fr");
        // Chaque outil apparaît une fois, les familles sont contiguës dans TOOLS.
        let flat: Vec<Tool> = families().concat();
        assert_eq!(flat, TOOLS.to_vec());
        let ids: Vec<u8> = families().iter().map(|g| family(g[0])).collect();
        assert_eq!(ids, [0, 1, 2, 3, 4, 5, 6]);
        assert_eq!(families()[6], [Tool::Draw(ShapeKind::Text), Tool::Draw(ShapeKind::Arrow)]);
        assert!(families()[1].contains(&Tool::Draw(ShapeKind::PointLine)));
        assert!(families()[4].contains(&Tool::Draw(ShapeKind::Arc)));
        // Deux colonnes quand la hauteur manque : bien moins haut, mêmes outils.
        assert_eq!(palette_columns(2000.0), 1);
        assert_eq!(palette_columns(700.0), 2);
        assert!(palette_height(2) < 0.65 * palette_height(1));
        assert!(palette_height(2) < 700.0);
    }
}

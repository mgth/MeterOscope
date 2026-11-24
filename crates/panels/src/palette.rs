//! Palette d'outils verticale et raccourcis clavier.

use crate::icons::paint_tool_icon;
use egui::{Align2, FontId, Key, Rect, RichText, ScrollArea, Sense, Ui, WidgetInfo, pos2, vec2};
use meteroscope_core::{ShapeKind, Tool};

/// Tous les outils, dans l'ordre de la palette.
pub const TOOLS: [Tool; 10] = [
    Tool::Select,
    Tool::Calibrate,
    Tool::Draw(ShapeKind::Length),
    Tool::Draw(ShapeKind::Path),
    Tool::Draw(ShapeKind::Angle),
    Tool::Draw(ShapeKind::Area),
    Tool::Draw(ShapeKind::Rect),
    Tool::Draw(ShapeKind::Circle),
    Tool::Draw(ShapeKind::Circle3),
    Tool::Draw(ShapeKind::Marker),
];

/// Largeur fixe des boutons (et donc du panneau).
const BUTTON_W: f32 = 76.0;
const BUTTON_H: f32 = 50.0;
const ICON: f32 = 22.0;

/// Nom complet de l'outil.
pub fn tool_name(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "Sélection",
        Tool::Calibrate => "Échelle",
        Tool::Draw(k) => k.label(),
    }
}

/// Nom court affiché sous l'icône (tient dans la largeur du bouton).
fn short_name(tool: Tool) -> &'static str {
    match tool {
        Tool::Draw(ShapeKind::Circle3) => "Cercle 3 pts",
        t => tool_name(t),
    }
}

/// Description d'une ligne (infobulle).
pub fn tool_description(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => "Sélectionner, déplacer ou modifier une mesure.",
        Tool::Calibrate => "Tracer un segment de longueur connue pour définir l'échelle.",
        Tool::Draw(k) => match k {
            ShapeKind::Length => "Distance entre deux points.",
            ShapeKind::Path => "Longueur d'une ligne brisée ; double-clic pour terminer.",
            ShapeKind::Angle => "Angle par trois points : un côté, le sommet, l'autre côté.",
            ShapeKind::Area => "Aire et périmètre d'un contour ; double-clic pour fermer.",
            ShapeKind::Rect => "Largeur, hauteur et aire, par deux coins opposés.",
            ShapeKind::Circle => "Cercle par son centre, puis un point du bord.",
            ShapeKind::Circle3 => "Cercle passant par trois points de son bord.",
            ShapeKind::Marker => "Pointer et compter des éléments.",
        },
    }
}

/// Raccourci clavier (une lettre, sans modificateur) associé à un outil.
///
/// S Sélection · E Échelle · L Longueur · B ligne Brisée (chemin) · A Angle ·
/// P Polygone (surface) · R Rectangle · C Cercle · T cercle Trois points · M Marqueur.
pub fn tool_shortcut(tool: Tool) -> Option<Key> {
    Some(match tool {
        Tool::Select => Key::S,
        Tool::Calibrate => Key::E,
        Tool::Draw(k) => match k {
            ShapeKind::Length => Key::L,
            ShapeKind::Path => Key::B,
            ShapeKind::Angle => Key::A,
            ShapeKind::Area => Key::P,
            ShapeKind::Rect => Key::R,
            ShapeKind::Circle => Key::C,
            ShapeKind::Circle3 => Key::T,
            ShapeKind::Marker => Key::M,
        },
    })
}

/// Texte d'infobulle complet : description + raccourci.
pub fn tool_tooltip(tool: Tool) -> String {
    match tool_shortcut(tool) {
        Some(k) => format!("{}\nRaccourci : {}", tool_description(tool), k.name()),
        None => tool_description(tool).to_owned(),
    }
}

/// Palette d'outils verticale (gros boutons avec icône dessinée + nom,
/// raccourci en infobulle). Modifie `tool` au clic.
pub fn tool_palette(ui: &mut Ui, tool: &mut Tool) {
    ui.set_width(BUTTON_W);
    ScrollArea::vertical()
        .id_salt("meteroscope_palette_outils")
        .auto_shrink([true, true])
        .content_margin(0.0)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.add_space(4.0);
            for t in TOOLS {
                if t == Tool::Draw(ShapeKind::Length) {
                    ui.add_space(2.0);
                    ui.separator();
                    ui.add_space(2.0);
                }
                if tool_button(ui, t, *tool == t).clicked() {
                    *tool = t;
                }
            }
            ui.add_space(4.0);
        });
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
            ui.label(RichText::new(format!("Raccourci : {}", k.name())).weak());
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn raccourcis_sans_collision() {
        let mut seen = HashSet::new();
        for t in TOOLS {
            let k = tool_shortcut(t).expect("chaque outil a un raccourci");
            assert!(seen.insert(k), "raccourci {k:?} en double");
            // Une lettre simple, sans conflit avec Échap / Suppr / Retour arrière.
            assert_eq!(k.name().len(), 1, "{k:?} n'est pas une lettre");
            assert!(k.name().chars().all(|c| c.is_ascii_uppercase()));
        }
        assert_eq!(seen.len(), 10);
    }

    #[test]
    fn tous_les_types_sont_dans_la_palette() {
        for k in ShapeKind::ALL {
            assert!(TOOLS.contains(&Tool::Draw(k)));
        }
        assert!(tool_tooltip(Tool::Draw(ShapeKind::Length)).ends_with("Raccourci : L"));
    }
}

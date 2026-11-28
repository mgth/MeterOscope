//! Palette d'outils verticale et raccourcis clavier.

use meteroscope_i18n::tr;
use crate::icons::paint_tool_icon;
use egui::{
    Align2, FontId, Id, Key, Popup, PopupCloseBehavior, PopupKind, Rect, RectAlign, RichText, ScrollArea, Sense,
    SetOpenCommand, Shape, Stroke, Ui, WidgetInfo, pos2, vec2,
};
use meteroscope_core::{ShapeKind, Tool};

/// Tous les outils, dans l'ordre de la palette.
pub const TOOLS: [Tool; 17] = [
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
];

/// Groupe d'outils semblables : un seul bouton dans la palette, qui montre
/// le dernier outil utilisé du groupe ; les autres s'ouvrent à côté.
#[derive(Clone, Copy, Debug)]
pub struct ToolGroup {
    /// Nom traduit du groupe (`None` pour un outil seul).
    label: Option<fn() -> &'static str>,
    pub tools: &'static [Tool],
}

impl ToolGroup {
    /// Nom du groupe, ou celui de son outil s'il est seul.
    pub fn name(&self) -> &'static str {
        match self.label {
            Some(label) => label(),
            None => tool_name(self.tools[0]),
        }
    }
}

/// Groupes de la palette, dans l'ordre ; ensemble, ils reprennent
/// exactement [`TOOLS`].
pub const GROUPS: [ToolGroup; 7] = [
    ToolGroup { label: None, tools: &[Tool::Select] },
    ToolGroup { label: Some(|| tr!("tool_group.reference")), tools: &[Tool::Calibrate, Tool::Straighten] },
    ToolGroup {
        label: Some(|| tr!("tool_group.lines")),
        tools: &[
            Tool::Draw(ShapeKind::Length),
            Tool::Draw(ShapeKind::Path),
            Tool::Draw(ShapeKind::PointLine),
            Tool::Draw(ShapeKind::Parallel),
        ],
    },
    ToolGroup {
        label: Some(|| tr!("tool_group.angles")),
        tools: &[Tool::Draw(ShapeKind::Angle), Tool::Draw(ShapeKind::LineAngle)],
    },
    ToolGroup {
        label: Some(|| tr!("tool_group.surfaces")),
        tools: &[
            Tool::Draw(ShapeKind::Area),
            Tool::Draw(ShapeKind::Rect),
            Tool::Draw(ShapeKind::Rect3),
            Tool::Draw(ShapeKind::Ellipse),
        ],
    },
    ToolGroup {
        label: Some(|| tr!("tool_group.circles")),
        tools: &[Tool::Draw(ShapeKind::Circle), Tool::Draw(ShapeKind::Circle3), Tool::Draw(ShapeKind::Arc)],
    },
    ToolGroup { label: None, tools: &[Tool::Draw(ShapeKind::Marker)] },
];

/// Groupe qui contient `tool`.
pub fn tool_group(tool: Tool) -> &'static ToolGroup {
    GROUPS.iter().find(|g| g.tools.contains(&tool)).expect("chaque outil a un groupe")
}

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
        },
    }
}

/// Raccourci clavier (une lettre, sans modificateur) associé à un outil.
///
/// S Sélection · E Échelle · H Horizon (redresser) · L Longueur · B ligne
/// Brisée (chemin) · A Angle · I Inclinaison · P Polygone (surface) ·
/// R Rectangle · O rectangle Orienté (trois points) · C Cercle · T cercle Trois points · M Marqueur ·
/// D Distance point-droite · G écart (« Gap ») entre parallèles · V ellipse (oVale) · U arc (en U).
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
/// raccourci en infobulle). Modifie `tool` au clic.
///
/// Les outils semblables sont regroupés ([`GROUPS`]) : le bouton montre
/// l'outil actif du groupe, ou le dernier utilisé ; un clic droit, un appui
/// long ou un nouveau clic sur le bouton déjà actif ouvre les autres outils
/// du groupe à côté. Les raccourcis clavier gardent un outil chacun. Si la
/// hauteur ne suffit pas pour une seule colonne, les boutons se rangent deux
/// par deux.
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
            for (s, section) in sections().iter().enumerate() {
                if s > 0 {
                    ui.add_space(2.0);
                    ui.separator();
                    ui.add_space(2.0);
                }
                for row in section.chunks(columns) {
                    ui.horizontal(|ui| {
                        for (index, group) in row {
                            group_button(ui, *index, group, tool);
                        }
                    });
                }
            }
            ui.add_space(4.0);
        });
}

/// Bouton d'un groupe et son volet d'outils.
fn group_button(ui: &mut Ui, index: usize, group: &ToolGroup, tool: &mut Tool) {
    let memory_id = Id::unique(("meteroscope_groupe_outils", index));
    let active = group.tools.contains(tool);
    let shown = if active {
        ui.data_mut(|d| d.insert_temp(memory_id, *tool));
        *tool
    } else {
        ui.data(|d| d.get_temp::<Tool>(memory_id)).filter(|t| group.tools.contains(t)).unwrap_or(group.tools[0])
    };
    let multiple = group.tools.len() > 1;

    let response = tool_button(ui, shown, active, multiple.then_some(group));
    if !multiple {
        if response.clicked() {
            *tool = shown;
        }
        return;
    }

    let open_flyout = response.secondary_clicked() || response.long_touched() || (response.clicked() && active);
    if response.clicked() && !active {
        *tool = shown;
    }
    Popup::from_response(&response)
        .kind(PopupKind::Menu)
        .align(RectAlign::RIGHT_START)
        .gap(4.0)
        .open_memory(open_flyout.then_some(SetOpenCommand::Bool(true)))
        .close_behavior(PopupCloseBehavior::CloseOnClick)
        .show(|ui| {
            ui.label(RichText::new(group.name()).small().weak());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                for &t in group.tools {
                    if tool_button(ui, t, *tool == t, None).clicked() {
                        *tool = t;
                    }
                }
            });
        });
}

/// Espace vertical entre deux boutons.
const ROW_GAP: f32 = 3.0;
/// Hauteur prise par un séparateur de familles (marges comprises).
const SEPARATOR_H: f32 = 12.0;

/// Boutons de la palette par section (séparées d'un trait) : réglages de
/// l'image, puis les mesures. Chaque bouton garde l'indice de son groupe.
fn sections() -> [Vec<(usize, &'static ToolGroup)>; 2] {
    let first_measure = GROUPS.iter().position(|g| matches!(g.tools[0], Tool::Draw(_))).unwrap_or(GROUPS.len());
    let all: Vec<_> = GROUPS.iter().enumerate().collect();
    [all[..first_measure].to_vec(), all[first_measure..].to_vec()]
}

/// Hauteur de la palette sur `columns` colonnes.
fn palette_height(columns: usize) -> f32 {
    let sections = sections();
    let rows: usize = sections.iter().map(|g| g.len().div_ceil(columns)).sum();
    8.0 + rows as f32 * (BUTTON_H + ROW_GAP) + (sections.len() - 1) as f32 * SEPARATOR_H
}

/// Une colonne si tout tient dans la hauteur `available`, sinon deux.
fn palette_columns(available: f32) -> usize {
    if palette_height(1) <= available { 1 } else { 2 }
}

/// Un bouton de la palette : icône au-dessus, nom dessous, lettre du
/// raccourci discrète dans le coin.
/// `group` : groupe à plusieurs outils dont ce bouton est le représentant
/// dans la palette (petit triangle dans le coin, aide en infobulle).
fn tool_button(ui: &mut Ui, tool: Tool, selected: bool, group: Option<&ToolGroup>) -> egui::Response {
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
        if group.is_some() {
            // Petit triangle en bas à droite : d'autres outils sont rangés ici.
            let c = rect.right_bottom() + vec2(-4.0, -4.0);
            painter.add(Shape::convex_polygon(
                vec![c, c + vec2(0.0, -6.0), c + vec2(-6.0, 0.0)],
                ui.visuals().weak_text_color(),
                Stroke::NONE,
            ));
        }
    }

    response.on_hover_ui(|ui| {
        ui.label(RichText::new(tool_name(tool)).strong());
        ui.label(tool_description(tool));
        if let Some(k) = tool_shortcut(tool) {
            ui.label(RichText::new(tr!("tool.shortcut", key = k.name())).weak());
        }
        if let Some(g) = group {
            let others: Vec<&str> = g.tools.iter().filter(|&&t| t != tool).map(|&t| tool_name(t)).collect();
            ui.separator();
            ui.label(RichText::new(tr!("tool_group.hint", tools = others.join(", "))).weak());
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
        assert_eq!(seen.len(), 17);
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
    fn les_groupes_reprennent_tous_les_outils_dans_l_ordre() {
        meteroscope_i18n::set_language("fr");
        let flat: Vec<Tool> = GROUPS.iter().flat_map(|g| g.tools.iter().copied()).collect();
        assert_eq!(flat, TOOLS);
        for g in &GROUPS {
            assert!(!g.tools.is_empty());
            assert_eq!(g.label.is_some(), g.tools.len() > 1, "{}", g.name());
        }
        assert_eq!(tool_group(Tool::Draw(ShapeKind::Rect3)).name(), "Surfaces");
        assert_eq!(tool_group(Tool::Draw(ShapeKind::Arc)).name(), "Cercles et arcs");
        assert_eq!(tool_group(Tool::Draw(ShapeKind::Parallel)).name(), "Lignes et distances");
        // Deux colonnes quand la hauteur manque : bien moins haut, mêmes outils.
        assert_eq!(palette_columns(2000.0), 1);
        assert_eq!(palette_columns(300.0), 2);
        assert!(palette_height(2) < 0.7 * palette_height(1));
    }
}

//! Panneau du profil d'intensité : courbe de la mesure sélectionnée, bords
//! détectés, réglages (bande, méthode, seuil) et choix des bords au clic.

use egui::{Align2, Color32, CursorIcon, FontId, Pos2, Rect, RichText, Sense, Shape as EShape, Stroke, Ui, pos2, vec2};
use meteroscope_core::measure::format_number;
use meteroscope_core::profile::{Analysis, BAND_RANGE, EdgeMethod};
use meteroscope_core::{Calibration, Document, Shape};
use meteroscope_i18n::tr;

/// Distance écran (points) sous laquelle un clic désigne un bord.
const PICK_RADIUS: f32 = 14.0;

/// Actions demandées depuis le panneau du profil.
#[derive(Clone, Debug, Default)]
pub struct ProfilePanelOutput {
    /// Exporter la courbe en CSV.
    pub export_csv: bool,
    /// Les réglages de la mesure ont changé (déjà appliqués au document).
    pub changed: bool,
}

/// Nom affiché d'une méthode de localisation des bords.
pub fn edge_method_name(m: EdgeMethod) -> &'static str {
    match m {
        EdgeMethod::HalfHeight => tr!("profile.method.half"),
        EdgeMethod::Gradient => tr!("profile.method.gradient"),
    }
}

fn edge_method_tip(m: EdgeMethod) -> &'static str {
    match m {
        EdgeMethod::HalfHeight => tr!("profile.method.half_tip"),
        EdgeMethod::Gradient => tr!("profile.method.gradient_tip"),
    }
}

/// Panneau du profil de la mesure `id` (un [`Shape::Profile`]), dont
/// `analysis` est l'analyse à jour. Les réglages modifiés sont écrits dans
/// le document, une étape d'annulation par geste.
pub fn profile_panel(ui: &mut Ui, doc: &mut Document, id: u64, analysis: &Analysis) -> ProfilePanelOutput {
    let mut out = ProfilePanelOutput::default();
    let Some(m) = doc.get(id) else { return out };
    let Shape::Profile { params, .. } = &m.shape else { return out };
    let (name, color, before) = (m.name.clone(), Color32::from_rgb(m.color[0], m.color[1], m.color[2]), params.clone());
    let cal = doc.calibration.clone();
    let mut p = before.clone();

    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(250.0);
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.horizontal(|ui| {
                ui.label(RichText::new(&name).strong());
                match analysis.result() {
                    Some(r) => {
                        let text = format!("{} {}", format_number(cal.length(r.width_px())), cal.length_unit());
                        ui.label(RichText::new(text).strong().color(color));
                    }
                    None => {
                        ui.label(RichText::new("—").weak());
                    }
                }
            });
            match analysis.result() {
                None => {
                    ui.add(egui::Label::new(RichText::new(tr!("profile.no_edges")).color(ui.visuals().warn_fg_color)).wrap());
                }
                Some(r) if r.tilt != 0.0 => {
                    ui.label(RichText::new(tr!("profile.tilt_info", angle = format_number(r.tilt.abs()))).weak())
                        .on_hover_text(tr!("profile.perpendicular_tip"));
                }
                Some(_) => {}
            }
            egui::Grid::new("meteroscope_profil_reglages").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                ui.label(tr!("profile.band")).on_hover_text(tr!("profile.band_tip"));
                ui.add(egui::DragValue::new(&mut p.band).range(BAND_RANGE).speed(0.25).max_decimals(0).suffix(" px"))
                    .on_hover_text(tr!("profile.band_tip"));
                ui.end_row();

                ui.label(tr!("profile.method"));
                ui.horizontal(|ui| {
                    for m in [EdgeMethod::HalfHeight, EdgeMethod::Gradient] {
                        ui.selectable_value(&mut p.method, m, edge_method_name(m)).on_hover_text(edge_method_tip(m));
                    }
                });
                ui.end_row();

                ui.label(tr!("profile.threshold")).on_hover_text(tr!("profile.threshold_tip"));
                ui.horizontal(|ui| {
                    let mut auto = p.threshold.is_none();
                    if ui.checkbox(&mut auto, tr!("profile.auto")).on_hover_text(tr!("profile.threshold_tip")).changed() {
                        p.threshold = if auto { None } else { Some(analysis.threshold) };
                    }
                    let mut pct = (p.threshold.unwrap_or(analysis.threshold) * 100.0).round();
                    let slider = egui::DragValue::new(&mut pct).range(2.0..=95.0).speed(0.5).max_decimals(0).suffix(" %");
                    if ui.add_enabled(!auto, slider).on_hover_text(tr!("profile.threshold_tip")).changed() {
                        p.threshold = Some(pct / 100.0);
                    }
                });
                ui.end_row();
            });
            ui.checkbox(&mut p.linear, tr!("profile.linear")).on_hover_text(tr!("profile.linear_tip"));
            ui.checkbox(&mut p.perpendicular, tr!("profile.perpendicular")).on_hover_text(tr!("profile.perpendicular_tip"));
            ui.horizontal(|ui| {
                if ui.add_enabled(p.pick.is_some(), egui::Button::new(tr!("profile.strongest"))).on_hover_text(tr!("profile.strongest_tip")).clicked() {
                    p.pick = None;
                }
                if ui.button(tr!("profile.export")).on_hover_text(tr!("profile.export_tip")).clicked() {
                    out.export_csv = true;
                }
            });
        });
        ui.separator();
        ui.vertical(|ui| {
            if let Some(pos) = plot(ui, analysis, &cal, color, p.method)
                && let Some(pick) = analysis.pick_at(pos)
            {
                p.pick = Some(pick);
            }
            ui.label(RichText::new(tr!("profile.pick_hint")).weak().small());
        });
    });

    // Une seule étape d'annulation par geste (glisser d'une valeur compris).
    let editing = egui::Id::unique("meteroscope_profil_edition");
    let down = ui.input(|i| i.pointer.any_down());
    let in_gesture: Option<u64> = ui.data(|d| d.get_temp(editing));
    if p != before {
        if !(down && in_gesture == Some(id)) {
            doc.checkpoint();
        }
        if let Some(Shape::Profile { params, .. }) = doc.get_mut(id).map(|m| &mut m.shape) {
            *params = p;
        }
        ui.data_mut(|d| d.insert_temp(editing, id));
        out.changed = true;
        ui.ctx().request_repaint();
    } else if !down && in_gesture.is_some() {
        ui.data_mut(|d| d.remove::<u64>(editing));
    }
    out
}

/// Trace la courbe et les bords. Renvoie la position (pixels le long du
/// segment) d'un clic qui désigne un bord.
fn plot(ui: &mut Ui, analysis: &Analysis, cal: &Calibration, color: Color32, method: EdgeMethod) -> Option<f64> {
    let size = vec2(ui.available_width().max(120.0), (ui.available_height() - 18.0).max(90.0));
    let (outer, resp) = ui.allocate_exact_size(size, Sense::click());
    let visuals = ui.visuals().clone();
    let painter = ui.painter_at(outer);
    painter.rect_filled(outer, 4.0, visuals.extreme_bg_color);
    let font = FontId::proportional(11.0);
    let weak = visuals.weak_text_color();
    // Zone de tracé : marge à gauche pour l'intensité, en bas pour les positions.
    let area = Rect::from_min_max(outer.min + vec2(38.0, 16.0), outer.max - vec2(8.0, 16.0));
    let length = analysis.length();
    if analysis.values.len() < 2 || length <= 0.0 || area.width() < 10.0 || area.height() < 10.0 {
        return None;
    }
    let (vmin, vmax) = analysis.values.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    let span = (vmax - vmin).max(1e-6);
    let (lo, hi) = (vmin - 0.06 * span, vmax + 0.06 * span);
    let x_of = |px: f64| area.left() + (px / length) as f32 * area.width();
    let y_of = |v: f64| area.bottom() - ((v - lo) / (hi - lo)) as f32 * area.height();
    let at = |px: f64, v: f64| pos2(x_of(px), y_of(v));

    // Axes : intensité en %, longueur dans l'unité de l'échelle.
    painter.rect_stroke(area, 0.0, Stroke::new(1.0, visuals.widgets.noninteractive.bg_stroke.color), egui::StrokeKind::Outside);
    for v in [vmin, vmax] {
        painter.text(pos2(area.left() - 4.0, y_of(v)), Align2::RIGHT_CENTER, format!("{:.0} %", v * 100.0), font.clone(), weak);
    }
    painter.text(pos2(area.left(), area.bottom() + 2.0), Align2::LEFT_TOP, "0", font.clone(), weak);
    let total = format!("{} {}", format_number(cal.length(length)), cal.length_unit());
    painter.text(pos2(area.right(), area.bottom() + 2.0), Align2::RIGHT_TOP, total, font.clone(), weak);

    // Largeur mesurée : zone teintée entre les deux bords retenus.
    let chosen = analysis.chosen.map(|c| c.map(|i| analysis.edges[i]));
    if let Some([e0, e1]) = chosen {
        let band = Rect::from_min_max(pos2(x_of(e0.pos), area.top()), pos2(x_of(e1.pos), area.bottom()));
        painter.rect_filled(band, 0.0, color.gamma_multiply(0.12));
        let width = format!("{} {}", format_number(cal.length(e1.pos - e0.pos)), cal.length_unit());
        painter.text(pos2(band.center().x, area.top() - 2.0), Align2::CENTER_BOTTOM, width, font.clone(), color);
    }

    // Courbe.
    let pts: Vec<Pos2> = analysis.values.iter().enumerate().map(|(i, v)| at(i as f64 * analysis.step, *v)).collect();
    painter.add(EShape::line(pts, Stroke::new(1.5, visuals.text_color())));

    // Bords : retenus en trait plein, autres candidats en pointillés.
    let hover = resp.hover_pos().filter(|h| area.expand(4.0).contains(*h));
    let near = hover.and_then(|h| {
        let px = ((h.x - area.left()) / area.width()) as f64 * length;
        let i = analysis.nearest_edge(px)?;
        ((x_of(analysis.edges[i].pos) - h.x).abs() <= PICK_RADIUS).then_some(i)
    });
    for (i, e) in analysis.edges.iter().enumerate() {
        let x = x_of(e.pos);
        let (top, bottom) = (pos2(x, area.top()), pos2(x, area.bottom()));
        if analysis.chosen.is_some_and(|c| c.contains(&i)) {
            painter.line_segment([top, bottom], Stroke::new(2.0, color));
            // Repère du niveau où le bord est pris.
            let level = match method {
                EdgeMethod::HalfHeight => (e.low + e.high) / 2.0,
                EdgeMethod::Gradient => {
                    let t = (e.pos / analysis.step).clamp(0.0, (analysis.values.len() - 1) as f64);
                    let j = (t.floor() as usize).min(analysis.values.len() - 2);
                    analysis.values[j] + (analysis.values[j + 1] - analysis.values[j]) * (t - j as f64)
                }
            };
            painter.circle(pos2(x, y_of(level)), 3.5, color, Stroke::new(1.0, visuals.extreme_bg_color));
            let label = format_number(cal.length(e.pos));
            painter.text(pos2(x, area.bottom() + 2.0), Align2::CENTER_TOP, label, font.clone(), color);
        } else {
            let strong = near == Some(i);
            let c = if strong { color } else { weak };
            painter.extend(EShape::dashed_line(&[top, bottom], Stroke::new(if strong { 2.0 } else { 1.0 }, c), 4.0, 3.0));
        }
    }

    // Lecture sous le pointeur.
    if let Some(h) = hover {
        let px = (((h.x - area.left()) / area.width()) as f64 * length).clamp(0.0, length);
        let i = ((px / analysis.step).round() as usize).min(analysis.values.len() - 1);
        painter.line_segment([pos2(x_of(px), area.top()), pos2(x_of(px), area.bottom())], Stroke::new(1.0, weak.gamma_multiply(0.6)));
        let text = format!("{} {}  ·  {:.1} %", format_number(cal.length(px)), cal.length_unit(), analysis.values[i] * 100.0);
        let align = if h.x > area.center().x { Align2::RIGHT_TOP } else { Align2::LEFT_TOP };
        let anchor = pos2(if h.x > area.center().x { area.right() - 4.0 } else { area.left() + 4.0 }, area.top() + 3.0);
        painter.text(anchor, align, text, font, visuals.strong_text_color());
        if near.is_some_and(|i| !analysis.chosen.is_some_and(|c| c.contains(&i))) {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
    }
    if resp.clicked()
        && let (Some(i), Some(_)) = (near, hover)
    {
        return Some(analysis.edges[i].pos);
    }
    None
}

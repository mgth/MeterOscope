//! Dessin des mesures, étiquettes, poignées, loupe et barre d'échelle.

use crate::view::Xf;
use egui::{
    Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Shape as EShape, Stroke, StrokeKind, TextureId, Vec2,
    pos2, vec2,
};
use meteroscope_core::measure::format_number;
use meteroscope_core::{Calibration, Pt, Shape};
use std::f32::consts::TAU;

/// Contour sombre sous les traits, pour rester lisible sur toute photo.
const HALO: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 170);
pub const LABEL_FONT: f32 = 13.0;
pub const MARKER_RADIUS: f32 = 9.0;
pub const HANDLE_SIZE: f32 = 8.0;

pub fn color32(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// Ligne (ouverte ou fermée) avec halo sombre.
pub fn outlined(p: &Painter, pts: Vec<Pos2>, closed: bool, color: Color32, width: f32) {
    if pts.len() < 2 {
        return;
    }
    let mk = |s: Stroke| if closed { EShape::closed_line(pts.clone(), s) } else { EShape::line(pts.clone(), s) };
    p.add(mk(Stroke::new(width + 2.5, HALO)));
    p.add(mk(Stroke::new(width, color)));
}

fn circle_outlined(p: &Painter, c: Pos2, r: f32, color: Color32, width: f32) {
    p.circle_stroke(c, r, Stroke::new(width + 2.5, HALO));
    p.circle_stroke(c, r, Stroke::new(width, color));
}

/// Petit point (sommet posé).
pub fn dot(p: &Painter, c: Pos2, color: Color32) {
    p.circle_filled(c, 3.5, HALO);
    p.circle_filled(c, 2.2, color);
}

/// Petite croix (centre de cercle).
fn cross(p: &Painter, c: Pos2, color: Color32, r: f32) {
    for d in [vec2(r, 0.0), vec2(0.0, r)] {
        outlined(p, vec![c - d, c + d], false, color, 1.2);
    }
}

/// Arc de cercle écran de l'angle `a0` sur `sweep` radians.
fn arc_points(c: Pos2, r: f32, a0: f32, sweep: f32) -> Vec<Pos2> {
    let n = ((sweep.abs() * r / 4.0).ceil() as usize).clamp(4, 64);
    (0..=n).map(|i| c + Vec2::angled(a0 + sweep * i as f32 / n as f32) * r).collect()
}

/// Géométrie de l'arc d'un angle en écran : (angle de départ, balayage, rayon).
fn angle_arc(a: Pos2, v: Pos2, b: Pos2) -> (f32, f32, f32) {
    let (u, w) = (a - v, b - v);
    let a0 = u.angle();
    let mut sweep = w.angle() - a0;
    while sweep > std::f32::consts::PI {
        sweep -= TAU;
    }
    while sweep <= -std::f32::consts::PI {
        sweep += TAU;
    }
    let r = (u.length().min(w.length()) * 0.4).clamp(6.0, 32.0);
    (a0, sweep, r)
}

/// Texte de l'étiquette d'une forme. `marker_no` : numéro du marqueur.
pub fn label_text(shape: &Shape, cal: &Calibration, marker_no: usize) -> String {
    let v = shape.values(cal);
    match shape {
        Shape::Marker { .. } => marker_no.to_string(),
        Shape::Rect { .. } if v.len() >= 3 => {
            format!("{} × {} {}\n{}", format_number(v[1].value), format_number(v[2].value), v[1].unit, v[0].text())
        }
        Shape::Circle { .. } | Shape::Circle3 { .. } if !v.is_empty() => format!("Ø {}", v[0].text()),
        Shape::Angle { .. } if !v.is_empty() => format!("{}°", format_number(v[0].value)),
        _ => v.first().map(|v| v.text()).unwrap_or_default(),
    }
}

/// Position (ancre) de l'étiquette en écran.
pub fn label_anchor(shape: &Shape, xf: Xf) -> (Pos2, Align2) {
    let s = |p: Pt| xf.to_screen(p);
    let beside_segment = |a: Pos2, b: Pos2| {
        let mid = a + (b - a) / 2.0;
        let d = b - a;
        let mut n = if d.length() > 0.0 { vec2(d.y, -d.x).normalized() } else { vec2(0.0, -1.0) };
        if n.y > 0.0 {
            n = -n; // toujours du côté « haut »
        }
        let align = if n.x.abs() > 0.7 {
            if n.x > 0.0 { Align2::LEFT_CENTER } else { Align2::RIGHT_CENTER }
        } else {
            Align2::CENTER_BOTTOM
        };
        (mid + n * 8.0, align)
    };
    match shape {
        Shape::Length { a, b } => beside_segment(s(*a), s(*b)),
        Shape::Path { pts } => {
            let i = (pts.len().saturating_sub(1)) / 2;
            beside_segment(s(pts[i]), s(pts[(i + 1).min(pts.len() - 1)]))
        }
        Shape::Angle { a, vertex, b } => {
            let (a0, sweep, r) = angle_arc(s(*a), s(*vertex), s(*b));
            let dir = Vec2::angled(a0 + sweep / 2.0);
            let align = Align2([
                if dir.x > 0.3 { egui::Align::Min } else if dir.x < -0.3 { egui::Align::Max } else { egui::Align::Center },
                if dir.y > 0.3 { egui::Align::Min } else if dir.y < -0.3 { egui::Align::Max } else { egui::Align::Center },
            ]);
            (s(*vertex) + dir * (r + 6.0), align)
        }
        Shape::Area { pts } => {
            let n = pts.len().max(1) as f64;
            let c = Pt::new(pts.iter().map(|p| p.x).sum::<f64>() / n, pts.iter().map(|p| p.y).sum::<f64>() / n);
            (s(c), Align2::CENTER_CENTER)
        }
        Shape::Rect { a, b } => (s(a.lerp(*b, 0.5)), Align2::CENTER_CENTER),
        Shape::Circle { .. } | Shape::Circle3 { .. } => match shape.circle() {
            Some((c, _)) => (s(c) + vec2(0.0, 6.0), Align2::CENTER_TOP),
            None => (s(shape.points()[0]), Align2::LEFT_BOTTOM),
        },
        Shape::Marker { p } => (s(*p), Align2::CENTER_CENTER),
    }
}

/// Étiquette sur fond sombre. Renvoie son rectangle écran.
pub fn label(p: &Painter, pos: Pos2, align: Align2, text: &str, color: Color32, strong: bool) -> Rect {
    let galley = p.layout_no_wrap(text.to_owned(), FontId::proportional(LABEL_FONT), color);
    let pad = vec2(5.0, 2.0);
    let rect = align.anchor_size(pos, galley.size() + pad * 2.0);
    let bg = if strong { Color32::from_black_alpha(220) } else { Color32::from_black_alpha(165) };
    p.rect(rect, CornerRadius::same(4), bg, Stroke::new(1.0, color.gamma_multiply(0.7)), StrokeKind::Inside);
    p.galley(rect.min + pad, galley, color);
    rect
}

/// Dessine une forme (traits seulement). `width` : épaisseur du trait.
pub fn paint_shape(p: &Painter, xf: Xf, shape: &Shape, color: Color32, width: f32, marker_no: usize) {
    let s = |q: Pt| xf.to_screen(q);
    match shape {
        Shape::Length { a, b } => {
            let (a, b) = (s(*a), s(*b));
            outlined(p, vec![a, b], false, color, width);
            // Petits traits perpendiculaires aux extrémités.
            let d = b - a;
            if d.length() > 0.0 {
                let n = vec2(-d.y, d.x).normalized() * 5.0;
                for e in [a, b] {
                    outlined(p, vec![e - n, e + n], false, color, width * 0.75);
                }
            }
        }
        Shape::Path { pts } => {
            outlined(p, pts.iter().map(|q| s(*q)).collect(), false, color, width);
            for q in pts {
                dot(p, s(*q), color);
            }
        }
        Shape::Area { pts } => {
            let v: Vec<Pos2> = pts.iter().map(|q| s(*q)).collect();
            outlined(p, v.clone(), true, color, width);
            for q in v {
                dot(p, q, color);
            }
        }
        Shape::Angle { a, vertex, b } => {
            let (a, v, b) = (s(*a), s(*vertex), s(*b));
            outlined(p, vec![a, v, b], false, color, width);
            let (a0, sweep, r) = angle_arc(a, v, b);
            outlined(p, arc_points(v, r, a0, sweep), false, color, width * 0.75);
        }
        Shape::Rect { a, b } => {
            let r = Rect::from_two_pos(s(*a), s(*b));
            outlined(p, vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()], true, color, width);
        }
        Shape::Circle { .. } | Shape::Circle3 { .. } => {
            if let Some((c, r)) = shape.circle() {
                let (cs, rs) = (s(c), r as f32 * xf.zoom);
                circle_outlined(p, cs, rs, color, width);
                cross(p, cs, color, 4.0);
                if let Shape::Circle { edge, .. } = shape {
                    // Diamètre en pointillés passant par le point du bord.
                    let e = s(*edge);
                    let opp = cs - (e - cs);
                    p.extend(EShape::dashed_line(&[opp, e], Stroke::new(1.0, color), 5.0, 4.0));
                } else {
                    for q in shape.points() {
                        dot(p, s(q), color);
                    }
                }
            }
        }
        Shape::Marker { p: m } => {
            let c = s(*m);
            p.circle_filled(c, MARKER_RADIUS + 1.5, HALO);
            p.circle_filled(c, MARKER_RADIUS, color);
            let text_color = if color.r() as u32 + color.g() as u32 + color.b() as u32 > 380 {
                Color32::BLACK
            } else {
                Color32::WHITE
            };
            p.text(c, Align2::CENTER_CENTER, marker_no.to_string(), FontId::proportional(11.0), text_color);
        }
    }
}

/// Poignées carrées blanches ; `hot` : poignée survolée ou saisie.
pub fn paint_handles(p: &Painter, xf: Xf, pts: &[Pt], hot: Option<usize>) {
    for (i, q) in pts.iter().enumerate() {
        let size = if hot == Some(i) { HANDLE_SIZE + 3.0 } else { HANDLE_SIZE };
        let r = Rect::from_center_size(xf.to_screen(*q), Vec2::splat(size));
        p.rect(r, CornerRadius::same(1), Color32::WHITE, Stroke::new(1.5, Color32::BLACK), StrokeKind::Outside);
    }
}

/// Loupe : zoom de la texture autour de `center` (pixels image) avec réticule.
/// `extra` dessine des formes dans le repère de la loupe.
#[allow(clippy::too_many_arguments)]
pub fn paint_loupe(
    p: &Painter,
    tex: TextureId,
    img: [usize; 2],
    center: Pt,
    cursor_screen: Pos2,
    view: Rect,
    mag: f32,
    extra: impl FnOnce(&Painter, Xf),
) {
    const SIZE: f32 = 140.0;
    const GAP: f32 = 28.0;
    // En haut à droite du curseur, ou de l'autre côté s'il n'y a pas la place.
    let mut min = cursor_screen + vec2(GAP, -GAP - SIZE);
    if min.x + SIZE > view.right() {
        min.x = cursor_screen.x - GAP - SIZE;
    }
    if min.y < view.top() {
        min.y = cursor_screen.y + GAP;
    }
    let rect = Rect::from_min_size(min, Vec2::splat(SIZE));
    let lp = p.with_clip_rect(rect.intersect(p.clip_rect()));
    lp.rect_filled(rect, 0.0, Color32::from_gray(20));

    // Partie visible de l'image dans la loupe.
    let xf = Xf { origin: rect.center() - vec2(center.x as f32, center.y as f32) * mag, zoom: mag };
    let (w, h) = (img[0] as f64, img[1] as f64);
    let tl = xf.to_image(rect.min);
    let br = xf.to_image(rect.max);
    let (x0, y0, x1, y1) = (tl.x.max(0.0), tl.y.max(0.0), br.x.min(w), br.y.min(h));
    if x1 > x0 && y1 > y0 && w > 0.0 && h > 0.0 {
        let screen = Rect::from_min_max(xf.to_screen(Pt::new(x0, y0)), xf.to_screen(Pt::new(x1, y1)));
        let uv = Rect::from_min_max(
            pos2((x0 / w) as f32, (y0 / h) as f32),
            pos2((x1 / w) as f32, (y1 / h) as f32),
        );
        lp.image(tex, screen, uv, Color32::WHITE);
    }
    extra(&lp, xf);

    // Réticule avec un trou au centre.
    let c = rect.center();
    for (d0, d1) in [(vec2(-SIZE, 0.0), vec2(-5.0, 0.0)), (vec2(5.0, 0.0), vec2(SIZE, 0.0))] {
        for k in [Vec2::X, Vec2::Y] {
            let f = |v: Vec2| if k == Vec2::X { v } else { vec2(v.y, v.x) };
            outlined(&lp, vec![c + f(d0), c + f(d1)], false, Color32::from_rgb(255, 60, 60), 1.0);
        }
    }
    p.rect_stroke(rect, 0.0, Stroke::new(1.0, Color32::BLACK), StrokeKind::Outside);
    p.rect_stroke(rect.expand(1.0), 0.0, Stroke::new(1.0, Color32::from_gray(200)), StrokeKind::Outside);
}

/// Longueur « ronde » (1, 2 ou 5 × 10ⁿ) la plus proche par défaut de `v`.
pub fn nice_length(v: f64) -> f64 {
    if !(v > 0.0 && v.is_finite()) {
        return 0.0;
    }
    let e = 10f64.powf(v.log10().floor());
    let m = v / e;
    let k = if m >= 5.0 {
        5.0
    } else if m >= 2.0 {
        2.0
    } else {
        1.0
    };
    k * e
}

/// Nombre sans zéros décimaux inutiles (« 0,5 », « 1000 »).
fn short_number(v: f64) -> String {
    let s = format_number(v);
    if s.contains(',') { s.trim_end_matches('0').trim_end_matches(',').to_owned() } else { s }
}

/// Barre d'échelle en bas à gauche de la vue.
pub fn paint_scale_bar(p: &Painter, view: Rect, zoom: f32, cal: &Calibration) {
    let Some(scale) = &cal.scale else { return };
    let units_per_screen_px = scale.units_per_px / zoom as f64;
    let len = nice_length(units_per_screen_px * 120.0);
    let px = (len / units_per_screen_px) as f32;
    if !(px.is_finite() && px > 4.0) {
        return;
    }
    let text = format!("{} {}", short_number(len), scale.unit);
    let galley = p.layout_no_wrap(text, FontId::proportional(12.0), Color32::WHITE);
    let pad = 6.0;
    let size = vec2(px.max(galley.size().x) + 2.0 * pad, galley.size().y + 14.0 + pad);
    let bg = Rect::from_min_size(pos2(view.left() + 12.0, view.bottom() - 12.0 - size.y), size);
    p.rect_filled(bg, CornerRadius::same(4), Color32::from_black_alpha(150));
    let y = bg.bottom() - pad - 3.0;
    let x0 = bg.left() + pad;
    let st = Stroke::new(2.0, Color32::WHITE);
    p.line_segment([pos2(x0, y), pos2(x0 + px, y)], st);
    for x in [x0, x0 + px] {
        p.line_segment([pos2(x, y - 5.0), pos2(x, y + 2.0)], st);
    }
    p.galley(pos2(x0, bg.top() + pad * 0.5), galley, Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn longueurs_rondes() {
        meteroscope_i18n::set_language("fr");
        assert_eq!(nice_length(1.3), 1.0);
        assert_eq!(nice_length(37.0), 20.0);
        assert_eq!(nice_length(0.07), 0.05);
        assert_eq!(nice_length(5.0), 5.0);
        assert_eq!(nice_length(0.0), 0.0);
        assert_eq!(short_number(1000.0), "1000");
        assert_eq!(short_number(0.5), "0,5");
        assert_eq!(short_number(20.0), "20");
    }

    #[test]
    fn textes_etiquettes() {
        meteroscope_i18n::set_language("fr");
        let cal = Calibration::default();
        let r = Shape::Rect { a: Pt::new(0., 0.), b: Pt::new(20., 10.) };
        assert_eq!(label_text(&r, &cal, 0), "20,00 × 10,00 px\n200,0 px²");
        let c = Shape::Circle { center: Pt::new(0., 0.), edge: Pt::new(5., 0.) };
        assert_eq!(label_text(&c, &cal, 0), "Ø 10,00 px");
        let a = Shape::Angle { a: Pt::new(1., 0.), vertex: Pt::new(0., 0.), b: Pt::new(0., 1.) };
        assert_eq!(label_text(&a, &cal, 0), "90,00°");
        assert_eq!(label_text(&Shape::Marker { p: Pt::default() }, &cal, 3), "3");
    }

    #[test]
    fn arc_angle_court() {
        meteroscope_i18n::set_language("fr");
        let (_, sweep, _) = angle_arc(pos2(10.0, 0.0), pos2(0.0, 0.0), pos2(0.0, -10.0));
        assert!((sweep.abs() - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    }
}

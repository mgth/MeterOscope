//! Icônes dessinées au pinceau (aucune police d'icônes, aucun emoji).

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2};
use meteroscope_core::{ShapeKind, Tool};
use std::f32::consts::TAU;

/// Dessine l'icône de `tool` centrée dans `rect` (carré conseillé, 20 à 28 px).
pub fn paint_tool_icon(painter: &Painter, rect: Rect, tool: Tool, color: Color32) {
    let s = rect.width().min(rect.height());
    let r = Rect::from_center_size(rect.center(), vec2(s, s));
    // Coordonnées normalisées (0..1) → écran.
    let p = |x: f32, y: f32| pos2(r.left() + x * s, r.top() + y * s);
    let w = (s / 13.0).max(1.2);
    let stroke = Stroke::new(w, color);
    let dot = |c: Pos2| {
        painter.circle_filled(c, w * 1.35, color);
    };
    let light = color.gamma_multiply(0.22);

    match tool {
        Tool::Select => {
            // Flèche de pointeur.
            let pts = vec![
                p(0.28, 0.08),
                p(0.28, 0.82),
                p(0.44, 0.67),
                p(0.56, 0.93),
                p(0.68, 0.88),
                p(0.56, 0.62),
                p(0.78, 0.62),
            ];
            // Remplissage léger en deux triangles convexes (la flèche ne l'est pas).
            painter.add(Shape::convex_polygon(vec![p(0.28, 0.08), p(0.28, 0.82), p(0.44, 0.67)], light, Stroke::NONE));
            painter.add(Shape::convex_polygon(vec![p(0.28, 0.08), p(0.44, 0.67), p(0.78, 0.62)], light, Stroke::NONE));
            painter.add(Shape::closed_line(pts, stroke));
        }
        Tool::Calibrate => {
            // Règle graduée.
            let ruler = Rect::from_min_max(p(0.05, 0.32), p(0.95, 0.70));
            painter.rect_filled(ruler, 1.5, light);
            painter.rect_stroke(ruler, 1.5, stroke, StrokeKind::Middle);
            for i in 1..8 {
                let x = 0.05 + i as f32 * 0.1125;
                let len = if i % 2 == 0 { 0.17 } else { 0.10 };
                painter.line_segment([p(x, 0.32), p(x, 0.32 + len)], Stroke::new(w * 0.8, color));
            }
        }
        Tool::Straighten => {
            // Horizon penché et flèche de rotation qui le redresse.
            let c = p(0.5, 0.56);
            let t: f32 = -0.30;
            let d = vec2(t.cos(), t.sin()) * 0.44 * s;
            painter.line_segment([c - d, c + d], Stroke::new(w * 0.8, light.gamma_multiply(3.0)));
            painter.line_segment([p(0.06, 0.56), p(0.94, 0.56)], stroke);
            let radius = 0.36 * s;
            let arc: Vec<Pos2> = (0..=10)
                .map(|i| {
                    let a = -2.55 + 1.0 * i as f32 / 10.0;
                    c + radius * vec2(a.cos(), a.sin())
                })
                .collect();
            let tip = *arc.last().unwrap();
            painter.add(Shape::line(arc, Stroke::new(w * 0.85, color)));
            // Pointe de flèche (sens horaire).
            let dir = vec2(-(-1.55f32).sin(), (-1.55f32).cos());
            let side = vec2(-dir.y, dir.x);
            let h = 0.16 * s;
            painter.add(Shape::convex_polygon(
                vec![tip + dir * h * 0.6, tip - dir * h * 0.5 + side * h * 0.55, tip - dir * h * 0.5 - side * h * 0.55],
                color,
                Stroke::NONE,
            ));
        }
        Tool::Perspective => {
            // Quadrilatère fuyant (pâle) ramené à un rectangle droit.
            let quad = vec![p(0.30, 0.16), p(0.70, 0.16), p(0.96, 0.84), p(0.04, 0.84)];
            painter.add(Shape::closed_line(quad, Stroke::new(w * 0.8, light.gamma_multiply(3.0))));
            let rr = Rect::from_min_max(p(0.20, 0.16), p(0.80, 0.84));
            painter.rect_filled(rr, 0.0, light);
            painter.rect_stroke(rr, 0.0, stroke, StrokeKind::Middle);
            for c in [rr.left_top(), rr.right_top(), rr.right_bottom(), rr.left_bottom()] {
                dot(c);
            }
        }
        Tool::Draw(kind) => match kind {
            ShapeKind::Length => {
                let (a, b) = (p(0.14, 0.80), p(0.86, 0.20));
                painter.line_segment([a, b], stroke);
                dot(a);
                dot(b);
            }
            ShapeKind::Path => {
                let pts = [p(0.08, 0.76), p(0.34, 0.28), p(0.62, 0.64), p(0.92, 0.22)];
                painter.add(Shape::line(pts.to_vec(), stroke));
                for q in pts {
                    dot(q);
                }
            }
            ShapeKind::Angle => {
                let v = p(0.14, 0.82);
                let (a, b) = (p(0.92, 0.82), p(0.62, 0.12));
                painter.line_segment([v, a], stroke);
                painter.line_segment([v, b], stroke);
                let end = (b - v).angle();
                let radius = 0.36 * s;
                let arc: Vec<Pos2> = (0..=12)
                    .map(|i| {
                        let t = end * i as f32 / 12.0;
                        v + radius * vec2(t.cos(), t.sin())
                    })
                    .collect();
                painter.add(Shape::line(arc, Stroke::new(w * 0.85, color)));
                dot(v);
            }
            ShapeKind::Area => {
                let pts = vec![p(0.22, 0.18), p(0.72, 0.10), p(0.92, 0.52), p(0.56, 0.90), p(0.10, 0.66)];
                painter.add(Shape::convex_polygon(pts, light, stroke));
            }
            ShapeKind::Rect => {
                let rr = Rect::from_min_max(p(0.10, 0.24), p(0.90, 0.76));
                painter.rect_filled(rr, 0.0, light);
                painter.rect_stroke(rr, 0.0, stroke, StrokeKind::Middle);
            }
            ShapeKind::Circle => {
                let c = p(0.5, 0.5);
                let radius = 0.40 * s;
                painter.circle(c, radius, light, stroke);
                painter.line_segment([c, c + radius * vec2(0.8, -0.6)], Stroke::new(w * 0.8, color));
                dot(c);
            }
            ShapeKind::Circle3 => {
                let c = p(0.5, 0.5);
                let radius = 0.38 * s;
                painter.circle_stroke(c, radius, stroke);
                for i in 0..3 {
                    let t = -TAU / 4.0 + i as f32 * TAU / 3.0;
                    painter.circle_filled(c + radius * vec2(t.cos(), t.sin()), w * 1.6, color);
                }
            }
            ShapeKind::LineAngle => {
                // Deux segments distants prolongés en pointillés jusqu'à leur
                // intersection, avec l'arc de l'angle.
                let v = p(0.94, 0.84);
                let (a, b) = (p(0.04, 0.84), p(0.52, 0.84));
                let (c, d) = (p(0.76, 0.50), p(0.48, 0.0));
                painter.line_segment([a, b], stroke);
                painter.line_segment([c, d], stroke);
                let dash = Stroke::new(w * 0.7, color);
                painter.extend(Shape::dashed_line(&[b, v], dash, w * 1.6, w * 1.4));
                painter.extend(Shape::dashed_line(&[c, v], dash, w * 1.6, w * 1.4));
                let radius = 0.30 * s;
                let (t0, t1) = (std::f32::consts::PI, (d - v).angle() + TAU);
                let arc: Vec<Pos2> = (0..=10)
                    .map(|i| {
                        let t = t0 + (t1 - t0) * i as f32 / 10.0;
                        v + radius * vec2(t.cos(), t.sin())
                    })
                    .collect();
                painter.add(Shape::line(arc, Stroke::new(w * 0.85, color)));
            }
            ShapeKind::Rect3 => {
                // Rectangle incliné : deux points pour un côté, un troisième pour la largeur.
                let pts = [p(0.10, 0.55), p(0.62, 0.17), p(0.856, 0.493), p(0.336, 0.873)];
                painter.add(Shape::convex_polygon(pts.to_vec(), light, stroke));
                for q in &pts[..3] {
                    dot(*q);
                }
            }
            ShapeKind::Marker => {
                painter.line_segment([p(0.5, 0.06), p(0.5, 0.94)], stroke);
                painter.line_segment([p(0.06, 0.5), p(0.94, 0.5)], stroke);
                painter.circle_stroke(p(0.5, 0.5), 0.22 * s, stroke);
            }
        },
    }
}

/// Œil ouvert (visible) ou barré (masqué).
pub fn paint_eye(painter: &Painter, rect: Rect, color: Color32, open: bool) {
    let c = rect.center();
    let hw = rect.width().min(rect.height()) * 0.42;
    let hh = hw * 0.55;
    let w = (hw / 6.0).max(1.1);
    let stroke = Stroke::new(w, color);
    // Paupières : deux arcs paraboliques.
    let lid = |sign: f32| -> Vec<Pos2> {
        (0..=12)
            .map(|i| {
                let x = -1.0 + 2.0 * i as f32 / 12.0;
                c + vec2(x * hw, sign * hh * (1.0 - x * x))
            })
            .collect()
    };
    painter.add(Shape::line(lid(-1.0), stroke));
    painter.add(Shape::line(lid(1.0), stroke));
    if open {
        painter.circle_filled(c, hh * 0.55, color);
    } else {
        painter.line_segment([c + vec2(-hw * 0.8, hh * 1.2), c + vec2(hw * 0.8, -hh * 1.2)], stroke);
    }
}

/// Croix de suppression.
pub fn paint_cross(painter: &Painter, rect: Rect, color: Color32) {
    let c = rect.center();
    let h = rect.width().min(rect.height()) * 0.26;
    let stroke = Stroke::new((h / 3.5).max(1.3), color);
    painter.line_segment([c + vec2(-h, -h), c + vec2(h, h)], stroke);
    painter.line_segment([c + vec2(-h, h), c + vec2(h, -h)], stroke);
}

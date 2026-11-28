//! Dessin des mesures, étiquettes, poignées, loupe et barre d'échelle.

use crate::view::Xf;
use egui::{
    Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Shape as EShape, Stroke, StrokeKind, TextureId, Vec2,
    pos2, vec2,
};
use meteroscope_core::geometry::{
    arc_from_3_points, ellipse_outline, ellipse_semi_axes, foot_on_line, line_angle_vertex, parallel_through,
    rect_corners,
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

/// Rayon écran de l'arc d'une inclinaison.
const LINE_ANGLE_RADIUS: f32 = 34.0;

/// Arc d'une inclinaison en écran : (sommet, angle de départ, balayage,
/// rayon), au croisement des droites portant les deux segments. `None` si
/// elles ne se coupent pas assez près (presque parallèles).
pub fn line_angle_arc(xf: Xf, a: Pt, b: Pt, c: Pt, d: Pt) -> Option<(Pos2, f32, f32, f32)> {
    let (x, u1, u2) = line_angle_vertex(a, b, c, d)?;
    let v = xf.to_screen(x);
    // Directions écran (la vue peut être pivotée), sur ~1 pixel écran.
    let k = 1.0 / xf.zoom as f64;
    let dir = |u: Pt| (xf.to_screen(x.offset(u.x * k, u.y * k)) - v).normalized();
    let a0 = dir(u1).angle();
    let mut sweep = dir(u2).angle() - a0;
    while sweep > std::f32::consts::PI {
        sweep -= TAU;
    }
    while sweep <= -std::f32::consts::PI {
        sweep += TAU;
    }
    Some((v, a0, sweep, LINE_ANGLE_RADIUS))
}

/// Parties à ajouter en pointillés au segment `p0→p1` pour qu'il atteigne
/// les points `reach` (pris sur sa droite).
fn extensions(p0: Pos2, p1: Pos2, reach: &[Pos2]) -> Vec<(Pos2, Pos2)> {
    let d = p1 - p0;
    let l2 = d.length_sq();
    if l2 == 0.0 {
        return vec![];
    }
    let ts = reach.iter().map(|q| (*q - p0).dot(d) / l2);
    let (lo, hi) = ts.fold((0.0f32, 1.0f32), |(lo, hi), t| (lo.min(t), hi.max(t)));
    let mut v = vec![];
    if lo < 0.0 {
        v.push((p0 + d * lo, p0));
    }
    if hi > 1.0 {
        v.push((p1, p0 + d * hi));
    }
    v
}

/// Trait en pointillés avec halo.
fn dashed(p: &Painter, a: Pos2, b: Pos2, color: Color32) {
    p.extend(EShape::dashed_line(&[a, b], Stroke::new(3.0, HALO), 5.0, 4.0));
    p.extend(EShape::dashed_line(&[a, b], Stroke::new(1.2, color), 5.0, 4.0));
}

/// Alignement d'une étiquette placée vers l'extérieur dans la direction `dir`.
fn align_out(dir: Vec2) -> Align2 {
    let side = |v: f32| if v > 0.3 { egui::Align::Min } else if v < -0.3 { egui::Align::Max } else { egui::Align::Center };
    Align2([side(dir.x), side(dir.y)])
}

/// Texte de l'étiquette d'une forme. `marker_no` : numéro du marqueur.
pub fn label_text(shape: &Shape, cal: &Calibration, marker_no: usize) -> String {
    let v = shape.values(cal);
    match shape {
        Shape::Marker { .. } => marker_no.to_string(),
        Shape::Text { text, .. } | Shape::Arrow { text, .. } => text.clone(),
        Shape::Rect { .. } if v.len() >= 3 => {
            format!("{} × {} {}\n{}", format_number(v[1].value), format_number(v[2].value), v[1].unit, v[0].text())
        }
        Shape::Circle { .. } | Shape::Circle3 { .. } if !v.is_empty() => format!("Ø {}", v[0].text()),
        Shape::Angle { .. } | Shape::LineAngle { .. } if !v.is_empty() => format!("{}°", format_number(v[0].value)),
        Shape::Ellipse { .. } if v.len() >= 2 => {
            format!("{} × {} {}", format_number(v[0].value), format_number(v[1].value), v[0].unit)
        }
        Shape::Arc { .. } if !v.is_empty() => format!("R {}", v[0].text()),
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
        Shape::Profile { a, b, .. } => {
            // Entre les deux bords détectés, au-delà des traits qui les marquent.
            let [e0, e1] = shape.profile_edges().unwrap_or([*a, *b]);
            let (pos, align) = beside_segment(s(e0), s(e1));
            let mid = s(e0.lerp(e1, 0.5));
            let out = (pos - mid).normalized();
            (pos + out * (profile_tick(shape, xf) - 4.0), align)
        }
        Shape::Path { pts } => {
            let i = (pts.len().saturating_sub(1)) / 2;
            beside_segment(s(pts[i]), s(pts[(i + 1).min(pts.len() - 1)]))
        }
        Shape::Angle { a, vertex, b } => {
            let (a0, sweep, r) = angle_arc(s(*a), s(*vertex), s(*b));
            let dir = Vec2::angled(a0 + sweep / 2.0);
            (s(*vertex) + dir * (r + 6.0), align_out(dir))
        }
        Shape::Area { pts } => {
            let n = pts.len().max(1) as f64;
            let c = Pt::new(pts.iter().map(|p| p.x).sum::<f64>() / n, pts.iter().map(|p| p.y).sum::<f64>() / n);
            (s(c), Align2::CENTER_CENTER)
        }
        Shape::Rect { a, b, .. } => (s(a.lerp(*b, 0.5)), Align2::CENTER_CENTER),
        Shape::Circle { .. } | Shape::Circle3 { .. } => match shape.circle() {
            Some((c, _)) => (s(c) + vec2(0.0, 6.0), Align2::CENTER_TOP),
            None => (s(shape.points()[0]), Align2::LEFT_BOTTOM),
        },
        Shape::Marker { p } | Shape::Text { p, .. } => (s(*p), Align2::CENTER_CENTER),
        // À la base, du côté opposé à la pointe.
        Shape::Arrow { a, b, .. } => {
            let d = s(*a) - s(*b);
            let dir = if d.length() > 0.0 { d.normalized() } else { vec2(0.0, -1.0) };
            (s(*a) + dir * 6.0, align_out(dir))
        }
        // À côté de la cote perpendiculaire.
        Shape::PointLine { a, b, p } => beside_segment(s(foot_on_line(*p, *a, *b).0), s(*p)),
        Shape::Parallel { a, b, p } => {
            let (_, _, c0, c1) = parallel_through(*a, *b, *p);
            beside_segment(s(c0), s(c1))
        }
        Shape::Ellipse { center, .. } => (s(*center) + vec2(0.0, 6.0), Align2::CENTER_TOP),
        // Au milieu de l'arc, du côté extérieur.
        Shape::Arc { a, m, b } => match arc_from_3_points(*a, *m, *b) {
            Some(arc) => {
                let (c, mid) = (s(arc.center), s(arc.point(0.5)));
                let dir = (mid - c).normalized();
                (mid + dir * 8.0, align_out(dir))
            }
            None => beside_segment(s(*a), s(*b)),
        },
        Shape::LineAngle { a, b, c, d } => match line_angle_arc(xf, *a, *b, *c, *d) {
            // Sur l'arc, du côté extérieur.
            Some((v, a0, sweep, r)) => {
                let dir = Vec2::angled(a0 + sweep / 2.0);
                (v + dir * (r + 6.0), align_out(dir))
            }
            // Au milieu du trait qui relie les deux segments.
            None => {
                let (m1, m2) = (s(a.lerp(*b, 0.5)), s(c.lerp(*d, 0.5)));
                (m1 + (m2 - m1) / 2.0, Align2::CENTER_CENTER)
            }
        },
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
/// Segment avec petits traits perpendiculaires aux extrémités (écran).
pub fn segment(p: &Painter, a: Pos2, b: Pos2, color: Color32, width: f32) {
    outlined(p, vec![a, b], false, color, width);
    let d = b - a;
    if d.length() > 0.0 {
        let n = vec2(-d.y, d.x).normalized() * 5.0;
        for e in [a, b] {
            outlined(p, vec![e - n, e + n], false, color, width * 0.75);
        }
    }
}

/// Demi-longueur écran des traits qui marquent les bords d'un profil : la
/// demi-largeur de la bande, sans descendre sous une taille lisible.
fn profile_tick(shape: &Shape, xf: Xf) -> f32 {
    shape.profile_band().map_or(0.0, |(h, _)| h as f32 * xf.zoom).max(7.0)
}

/// Dessine l'image (taille `img` en pixels) selon `xf`, rotation comprise.
/// `baked` : la texture contient déjà l'image pivotée dans sa boîte
/// englobante, dont c'est la taille en pixels image ; elle se dessine droite.
pub fn paint_image(p: &Painter, tex: TextureId, img: [usize; 2], xf: Xf, baked: Option<Vec2>) {
    let (w, h) = (img[0] as f64, img[1] as f64);
    let mut mesh = egui::Mesh::with_texture(tex);
    for (x, y) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        let pos = match baked {
            Some(size) => xf.origin + vec2(x as f32 * size.x, y as f32 * size.y) * xf.zoom,
            None => xf.to_screen(Pt::new(x * w, y * h)),
        };
        mesh.vertices.push(egui::epaint::Vertex { pos, uv: pos2(x as f32, y as f32), color: Color32::WHITE });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    p.add(EShape::mesh(mesh));
}

/// Contour écran de l'image (quadrilatère, rotation comprise).
pub fn image_outline(xf: Xf, img: [usize; 2]) -> Vec<Pos2> {
    let (w, h) = (img[0] as f64, img[1] as f64);
    [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)].map(|(x, y)| xf.to_screen(Pt::new(x, y))).to_vec()
}

pub fn paint_shape(p: &Painter, xf: Xf, shape: &Shape, color: Color32, width: f32, marker_no: usize) {
    let s = |q: Pt| xf.to_screen(q);
    match shape {
        Shape::Length { a, b } => segment(p, s(*a), s(*b), color, width),
        Shape::Profile { a, b, .. } => {
            let (sa, sb) = (s(*a), s(*b));
            let d = sb - sa;
            let n = if d.length() > 0.0 { vec2(-d.y, d.x).normalized() } else { vec2(0.0, 1.0) };
            // Bande moyennée, en pointillés fins de part et d'autre du segment.
            let half = shape.profile_band().map_or(0.0, |(h, _)| h as f32 * xf.zoom);
            if half >= 3.0 {
                for side in [-half, half] {
                    p.extend(EShape::dashed_line(&[sa + n * side, sb + n * side], Stroke::new(1.0, color.gamma_multiply(0.7)), 4.0, 4.0));
                }
            }
            outlined(p, vec![sa, sb], false, color, width * 0.75);
            dot(p, sa, color);
            dot(p, sb, color);
            // Un trait en travers sur chaque bord détecté.
            let t = profile_tick(shape, xf);
            for e in shape.profile_edges().into_iter().flatten() {
                outlined(p, vec![s(e) - n * t, s(e) + n * t], false, color, width);
            }
        }
        Shape::LineAngle { a: pa, b: pb, c: pc, d: pd } => {
            let (a, b, c, d) = (s(*pa), s(*pb), s(*pc), s(*pd));
            match line_angle_arc(xf, *pa, *pb, *pc, *pd) {
                Some((v, a0, sweep, r)) => {
                    // Segments prolongés en pointillés jusqu'à leur intersection
                    // (et un peu au-delà, jusqu'au bout de l'arc), puis l'arc.
                    let reach = r + 8.0;
                    for (p0, p1, dir) in [(a, b, Vec2::angled(a0)), (c, d, Vec2::angled(a0 + sweep))] {
                        for (e0, e1) in extensions(p0, p1, &[v, v + dir * reach]) {
                            dashed(p, e0, e1, color);
                        }
                    }
                    outlined(p, arc_points(v, r, a0, sweep), false, color, width * 0.75);
                }
                // Presque parallèles : lien entre les milieux.
                None => dashed(p, a + (b - a) / 2.0, c + (d - c) / 2.0, color),
            }
            segment(p, a, b, color, width);
            segment(p, c, d, color, width);
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
        Shape::Rect { a, b, angle } => {
            // Coins en pixels image, côtés orientés selon le rectangle.
            let corners = rect_corners(*a, *b, *angle);
            outlined(p, corners.iter().map(|q| s(*q)).collect(), true, color, width);
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
        Shape::PointLine { a, b, p: q } => paint_point_line(p, xf, *a, *b, *q, color, width),
        Shape::Parallel { a, b, p: q } => paint_parallel(p, xf, *a, *b, *q, color, width),
        Shape::Ellipse { center, a, b } => paint_ellipse(p, xf, *center, *a, *b, color, width),
        Shape::Arc { a, m, b } => paint_arc(p, xf, *a, *m, *b, color, width),
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
        // Le texte est dessiné comme une étiquette ; sans texte, un simple point.
        Shape::Text { p: q, text } => {
            if text.is_empty() {
                dot(p, s(*q), color);
            }
        }
        Shape::Arrow { a, b, .. } => arrow(p, s(*a), s(*b), color, width),
    }
}

/// Flèche de `a` vers la pointe `b` (écran), pointe pleine avec halo.
pub fn arrow(p: &Painter, a: Pos2, b: Pos2, color: Color32, width: f32) {
    let d = b - a;
    let len = d.length();
    if len == 0.0 {
        dot(p, a, color);
        return;
    }
    let (u, n) = (d / len, vec2(-d.y, d.x) / len);
    let head = (4.0 * width + 6.0).min(len * 0.6);
    let base = b - u * head;
    let tip = vec![b, base + n * head * 0.45, base - n * head * 0.45];
    // Le trait s'arrête dans la pointe pour ne pas dépasser de son sommet.
    outlined(p, vec![a, b - u * head * 0.5], false, color, width);
    p.add(EShape::convex_polygon(tip.clone(), HALO, Stroke::new(2.5, HALO)));
    p.add(EShape::convex_polygon(tip, color, Stroke::NONE));
}

/// Petit symbole d'angle droit au pied `f` d'une perpendiculaire : un côté
/// le long de la droite (vers `along`), l'autre vers le point `toward`.
fn right_angle(p: &Painter, f: Pos2, along: Pos2, toward: Pos2, color: Color32) {
    const SIZE: f32 = 8.0;
    let (u, w) = (along - f, toward - f);
    if u.length() < 1e-3 || w.length() < SIZE {
        return;
    }
    let (u, w) = (u.normalized() * SIZE, w.normalized() * SIZE);
    outlined(p, vec![f + u, f + u + w, f + w], false, color, 1.2);
}

/// Distance d'un point à une droite : la droite (prolongée en pointillés
/// jusqu'au pied de la perpendiculaire s'il tombe hors du segment), la
/// perpendiculaire cotée et le symbole d'angle droit.
fn paint_point_line(p: &Painter, xf: Xf, pa: Pt, pb: Pt, pp: Pt, color: Color32, width: f32) {
    let s = |q: Pt| xf.to_screen(q);
    let (a, b, q) = (s(pa), s(pb), s(pp));
    let (foot, t) = foot_on_line(pp, pa, pb);
    let f = s(foot);
    for (e0, e1) in extensions(a, b, &[f]) {
        dashed(p, e0, e1, color);
    }
    outlined(p, vec![a, b], false, color, width);
    // Le côté de l'angle droit le long de la droite va vers le segment.
    let toward_segment = if t > 0.5 { a } else { b };
    right_angle(p, f, toward_segment, q, color);
    segment(p, f, q, color, width * 0.75);
    for e in [a, b, q] {
        dot(p, e, color);
    }
}

/// Écart entre parallèles : les deux droites et la cote perpendiculaire.
fn paint_parallel(p: &Painter, xf: Xf, pa: Pt, pb: Pt, pp: Pt, color: Color32, width: f32) {
    let s = |q: Pt| xf.to_screen(q);
    let (start, end, c0, c1) = parallel_through(pa, pb, pp);
    outlined(p, vec![s(pa), s(pb)], false, color, width);
    outlined(p, vec![s(start), s(end)], false, color, width);
    segment(p, s(c0), s(c1), color, width * 0.75);
    for e in [pa, pb, pp] {
        dot(p, s(e), color);
    }
}

/// Ellipse : contour, croix au centre, demi-axes en pointillés.
fn paint_ellipse(p: &Painter, xf: Xf, c: Pt, a: Pt, b: Pt, color: Color32, width: f32) {
    let s = |q: Pt| xf.to_screen(q);
    let (ra, rb) = ellipse_semi_axes(c, a, b);
    let n = ((ra + rb) * xf.zoom as f64).clamp(24.0, 180.0) as usize;
    outlined(p, ellipse_outline(c, a, b, n).into_iter().map(s).collect(), true, color, width);
    cross(p, s(c), color, 4.0);
    for e in [a, b] {
        p.extend(EShape::dashed_line(&[s(c), s(e)], Stroke::new(1.0, color), 5.0, 4.0));
    }
}

/// Arc par trois points : l'arc seul, et un rayon en pointillés vers le centre.
fn paint_arc(p: &Painter, xf: Xf, a: Pt, m: Pt, b: Pt, color: Color32, width: f32) {
    let s = |q: Pt| xf.to_screen(q);
    match arc_from_3_points(a, m, b) {
        Some(arc) => {
            let n = ((arc.length() * xf.zoom as f64) / 4.0).clamp(8.0, 256.0) as usize;
            let pts: Vec<Pos2> = (0..=n).map(|i| s(arc.point(i as f64 / n as f64))).collect();
            dashed(p, s(arc.point(0.5)), s(arc.center), color);
            cross(p, s(arc.center), color, 4.0);
            outlined(p, pts, false, color, width);
        }
        None => outlined(p, vec![s(a), s(m), s(b)], false, color, width),
    }
    for e in [a, m, b] {
        dot(p, s(e), color);
    }
}

/// Poignée de rotation (ronde) reliée au bord en `base`.
pub fn paint_rotate_handle(p: &Painter, base: Pos2, h: Pos2, hot: bool) {
    outlined(p, vec![base, h], false, Color32::WHITE, 1.2);
    let r = if hot { HANDLE_SIZE / 2.0 + 2.5 } else { HANDLE_SIZE / 2.0 + 1.0 };
    p.circle(h, r, Color32::WHITE, Stroke::new(1.5, Color32::BLACK));
}

/// Couleur du repère d'aimantation.
const SNAP_COLOR: Color32 = Color32::from_rgb(255, 220, 60);

/// Repère d'aimantation en `c` (écran), dont la forme dit la nature de
/// l'accroche : carré pour un point ou un coin, triangle pour un milieu,
/// cercle pour un centre, croix pour une intersection.
pub fn paint_snap(p: &Painter, c: Pos2, kind: crate::snap::SnapKind) {
    use crate::snap::SnapKind;
    const R: f32 = 6.0;
    match kind {
        SnapKind::Point => {
            let q = [vec2(-R, -R), vec2(R, -R), vec2(R, R), vec2(-R, R)];
            outlined(p, q.iter().map(|d| c + *d).collect(), true, SNAP_COLOR, 1.5);
        }
        SnapKind::Midpoint => {
            let q = [vec2(0.0, -R * 1.15), vec2(R, R * 0.7), vec2(-R, R * 0.7)];
            outlined(p, q.iter().map(|d| c + *d).collect(), true, SNAP_COLOR, 1.5);
        }
        SnapKind::Center => circle_outlined(p, c, R + 0.75, SNAP_COLOR, 1.5),
        SnapKind::Intersection => {
            for d in [vec2(R, R), vec2(R, -R)] {
                outlined(p, vec![c - d, c + d], false, SNAP_COLOR, 1.5);
            }
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
/// `baked` : voir [`paint_image`]. `base` : transformation de la vue (pour la
/// rotation). `extra` dessine des
/// formes dans le repère de la loupe.
#[allow(clippy::too_many_arguments)]
pub fn paint_loupe(
    p: &Painter,
    tex: TextureId,
    img: [usize; 2],
    baked: Option<Vec2>,
    base: Xf,
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

    // Image entière, découpée par la zone de la loupe.
    let xf = base.centered_on(center, rect.center(), mag);
    if img[0] > 0 && img[1] > 0 {
        paint_image(&lp, tex, img, xf, baked);
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
        let r = Shape::Rect { a: Pt::new(0., 0.), b: Pt::new(20., 10.), angle: 0.0 };
        assert_eq!(label_text(&r, &cal, 0), "20,00 × 10,00 px\n200,0 px²");
        let c = Shape::Circle { center: Pt::new(0., 0.), edge: Pt::new(5., 0.) };
        assert_eq!(label_text(&c, &cal, 0), "Ø 10,00 px");
        let a = Shape::Angle { a: Pt::new(1., 0.), vertex: Pt::new(0., 0.), b: Pt::new(0., 1.) };
        assert_eq!(label_text(&a, &cal, 0), "90,00°");
        assert_eq!(label_text(&Shape::Marker { p: Pt::default() }, &cal, 3), "3");
        let la = Shape::LineAngle { a: Pt::new(0., 0.), b: Pt::new(10., 0.), c: Pt::new(0., 5.), d: Pt::new(0., 9.) };
        assert_eq!(label_text(&la, &cal, 0), "90,00°");
        let t = Shape::Text { p: Pt::default(), text: "Défaut".to_owned() };
        assert_eq!(label_text(&t, &cal, 0), "Défaut");
        assert_eq!(label_text(&Shape::Arrow { a: Pt::default(), b: Pt::new(5., 0.), text: String::new() }, &cal, 0), "");
        // Étiquette d'une flèche : à sa base, du côté opposé à la pointe.
        let xf = Xf::new(Pos2::ZERO, 1.0);
        let (pos, align) = label_anchor(&Shape::Arrow { a: Pt::new(50., 50.), b: Pt::new(100., 50.), text: "x".into() }, xf);
        assert!(pos.x < 50.0 && align == Align2::RIGHT_CENTER, "{pos:?} {align:?}");
    }

    #[test]
    fn inclinaison_arc_et_prolongements() {
        meteroscope_i18n::set_language("fr");
        let xf = Xf::new(Pos2::ZERO, 2.0);
        // Horizontale de 10 à 20 et verticale de y = 5 à 8 en x = 5 : sommet en (5, 0).
        let (a, b, c, d) = (Pt::new(10., 0.), Pt::new(20., 0.), Pt::new(5., 5.), Pt::new(5., 8.));
        let (v, a0, sweep, _) = line_angle_arc(xf, a, b, c, d).unwrap();
        assert!(v.distance(pos2(10.0, 0.0)) < 1e-4);
        assert!(a0.abs() < 1e-4 && (sweep - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
        let (pos, _) = label_anchor(&Shape::LineAngle { a, b, c, d }, xf);
        assert!(pos.x > v.x && pos.y > v.y);
        // Prolongement jusqu'au sommet, d'un seul côté.
        let e = extensions(pos2(20.0, 0.0), pos2(40.0, 0.0), &[pos2(10.0, 0.0), pos2(14.0, 0.0)]);
        assert_eq!(e, vec![(pos2(10.0, 0.0), pos2(20.0, 0.0))]);
        assert!(extensions(pos2(0.0, 0.0), pos2(40.0, 0.0), &[pos2(10.0, 0.0)]).is_empty());
        // Parallèles : pas d'arc.
        assert!(line_angle_arc(xf, a, b, Pt::new(10., 5.), Pt::new(20., 5.)).is_none());
    }

    #[test]
    fn arc_angle_court() {
        meteroscope_i18n::set_language("fr");
        let (_, sweep, _) = angle_arc(pos2(10.0, 0.0), pos2(0.0, 0.0), pos2(0.0, -10.0));
        assert!((sweep.abs() - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
    }

    #[test]
    fn etiquettes_nouvelles_formes() {
        meteroscope_i18n::set_language("fr");
        let cal = Calibration::default();
        let e = Shape::Ellipse { center: Pt::new(0., 0.), a: Pt::new(10., 0.), b: Pt::new(0., 4.) };
        assert_eq!(label_text(&e, &cal, 0), "20,00 × 8,00 px");
        let arc = Shape::Arc { a: Pt::new(5., 0.), m: Pt::new(0., 5.), b: Pt::new(-5., 0.) };
        assert_eq!(label_text(&arc, &cal, 0), "R 5,00 px");
        let pl = Shape::PointLine { a: Pt::new(0., 0.), b: Pt::new(10., 0.), p: Pt::new(20., -3.) };
        assert_eq!(label_text(&pl, &cal, 0), "3,00 px");
        let xf = Xf::new(Pos2::ZERO, 1.0);
        // Étiquette de la distance à côté de la perpendiculaire (x = 20), pas de la droite.
        let (pos, _) = label_anchor(&pl, xf);
        assert!((pos.x - 20.0).abs() > 1.0 && (pos.x - 20.0).abs() < 12.0 && pos.y < 0.0 && pos.y > -3.0, "{pos:?}");
        // Arc par le bas (y positif) : étiquette sous l'arc.
        let (pos, _) = label_anchor(&arc, xf);
        assert!(pos.y > 5.0 && pos.x.abs() < 1e-3, "{pos:?}");
        let (pos, _) = label_anchor(&Shape::Parallel { a: Pt::new(0., 0.), b: Pt::new(10., 0.), p: Pt::new(3., 6.) }, xf);
        assert!((pos.y - 3.0).abs() < 1.0 && pos.x != 5.0, "{pos:?}");
    }
}

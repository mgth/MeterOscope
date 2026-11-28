//! Dessin des mesures, commun à tous les exports : l'image annotée (PNG, et
//! par elle le rapport PDF) et le SVG tracent exactement les mêmes traits et
//! les mêmes étiquettes, chacun sur sa [`Surface`].
//!
//! Le texte des étiquettes est celui de l'écran ([`label_text`]), toujours
//! calculé sur la mesure d'origine ; seule la géométrie est reportée dans le
//! repère de l'image écrite ([`Frame::place`]), qui peut être pivotée et
//! réduite. Les valeurs affichées ne dépendent donc jamais de la taille du
//! fichier produit.

use crate::render::text_width;
use crate::rotate::Frame;
use meteroscope_core::geometry as geo;
use meteroscope_core::scale_bar::TARGET_FRACTION;
use meteroscope_core::{Corner, Document, Measurement, Pt, Shape, label_text, marker_labels};

/// Point de la surface (pixels).
pub(crate) type P2 = (f32, f32);
pub(crate) type Rgba = [u8; 4];

const SHADOW: Rgba = [0, 0, 0, 190];
const LABEL_BG: Rgba = [0, 0, 0, 185];
pub(crate) const WHITE: Rgba = [255, 255, 255, 255];
/// Segments des arcs d'angle.
const ARC_STEPS: usize = 48;

fn rgb(c: [u8; 3], a: u8) -> Rgba {
    [c[0], c[1], c[2], a]
}

fn p32(p: Pt) -> P2 {
    (p.x as f32, p.y as f32)
}

fn mid(a: P2, b: P2) -> P2 {
    ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)
}

/// Direction unitaire de `a` vers `b`, vers le haut si les points sont confondus.
fn unit(a: P2, b: P2) -> P2 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l = dx.hypot(dy);
    if l > 1e-6 { (dx / l, dy / l) } else { (0.0, -1.0) }
}

/// Normale unitaire au segment `a→b`, du côté haut de l'image.
fn normal_up(a: P2, b: P2) -> P2 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx.hypot(dy).max(1e-6);
    let n = (-dy / len, dx / len);
    if n.1 > 0.0 || (n.1 == 0.0 && n.0 > 0.0) { (-n.0, -n.1) } else { n }
}

/// Ligne brisée, fermée ou non.
pub(crate) struct Line {
    pub pts: Vec<P2>,
    pub closed: bool,
}

fn open(pts: &[P2]) -> Line {
    Line { pts: pts.to_vec(), closed: false }
}

fn poly(pts: &[Pt], closed: bool) -> Line {
    Line { pts: pts.iter().map(|p| p32(*p)).collect(), closed }
}

/// Ce qu'une surface sait tracer.
pub(crate) enum Geom<'a> {
    Lines(&'a [Line]),
    /// Centre, rayon.
    Circle(P2, f32),
    /// Rectangle droit aux coins arrondis de rayon `r` (0 : coins vifs).
    Rect { x: f32, y: f32, w: f32, h: f32, r: f32 },
}

/// Trait : couleur, épaisseur, et longueur des tirets pour un pointillé.
#[derive(Clone, Copy)]
pub(crate) struct Pen {
    pub color: Rgba,
    pub width: f32,
    pub dash: Option<f32>,
}

fn pen(color: Rgba, width: f32) -> Option<Pen> {
    Some(Pen { color, width, dash: None })
}

/// Dimensions de référence d'un dessin, proportionnelles à l'image.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Metrics {
    pub w: f32,
    pub h: f32,
    /// Épaisseur du trait coloré.
    pub line: f32,
    /// Largeur du contour sombre de chaque côté.
    pub halo: f32,
    /// Taille du texte (em, pixels).
    pub text: f32,
}

impl Metrics {
    pub fn new(w: f32, h: f32) -> Metrics {
        // Unité : 1 pour une image de 1000 px de grand côté.
        let u = (w.max(h) / 1000.0).max(0.6);
        Metrics { w, h, line: (2.4 * u).max(1.8), halo: (1.2 * u).max(1.0), text: (17.0 * u).max(13.0) }
    }
}

/// Support de dessin d'un export : calque de pixels ou texte SVG.
pub(crate) trait Surface {
    fn metrics(&self) -> Metrics;
    /// Trace `g`, rempli de `fill` et/ou bordé de `pen` (extrémités rondes).
    fn paint(&mut self, g: &Geom, fill: Option<Rgba>, pen: Option<Pen>);
    /// Texte blanc d'une ligne, ligne de base en (x, y), de largeur `width`.
    fn text(&mut self, s: &str, x: f32, y: f32, size: f32, width: f32);
    /// Début du dessin d'une mesure (groupe SVG).
    fn open(&mut self, _m: &Measurement) {}
    /// Fin du dessin de la mesure ouverte.
    fn close(&mut self) {}
}

/// Géométrie d'une flèche : longueur de la pointe et ses trois sommets
/// (pointe, puis les deux coins de la base), pour un trait d'épaisseur `line`.
fn arrow_head(a: P2, b: P2, line: f32) -> Option<(f32, [P2; 3])> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx.hypot(dy);
    if len < 1e-6 {
        return None;
    }
    let (ux, uy) = (dx / len, dy / len);
    let head = (line * 5.0 + 4.0).min(len * 0.6);
    let (bx, by) = (b.0 - ux * head, b.1 - uy * head);
    let w = head * 0.45;
    Some((head, [b, (bx - uy * w, by + ux * w), (bx + uy * w, by - ux * w)]))
}

/// Dimensions (largeur, hauteur) de la boîte d'une étiquette, d'une ou
/// plusieurs lignes.
fn label_size(s: &str, size: f32) -> (f32, f32) {
    let pad = size * 0.35;
    let width = s.lines().map(|l| text_width(l, size)).fold(0.0, f32::max);
    let extra = s.lines().count().saturating_sub(1) as f32;
    (width + 2.0 * pad, size * 1.1 + pad + extra * size * LINE_PITCH)
}

/// Interligne des étiquettes de plusieurs lignes, en em.
const LINE_PITCH: f32 = 1.2;

/// Le dessin lui-même, écrit une fois pour toutes les surfaces.
struct Pencil<'a, S: Surface> {
    s: &'a mut S,
    m: Metrics,
}

impl<'a, S: Surface> Pencil<'a, S> {
    fn new(s: &'a mut S) -> Self {
        let m = s.metrics();
        Pencil { s, m }
    }

    /// Trait d'épaisseur `k` × le trait normal, avec contour sombre.
    fn stroked(&mut self, lines: &[Line], color: [u8; 3], k: f32, dash: Option<f32>) {
        let w = self.m.line * k;
        let g = Geom::Lines(lines);
        self.s.paint(&g, None, Some(Pen { color: SHADOW, width: w + 2.0 * self.m.halo, dash }));
        self.s.paint(&g, None, Some(Pen { color: rgb(color, 255), width: w, dash }));
    }

    /// Trait coloré avec contour sombre.
    fn outlined(&mut self, lines: &[Line], color: [u8; 3]) {
        self.stroked(lines, color, 1.0, None);
    }

    /// Traits en pointillés fins.
    fn dashed(&mut self, lines: &[Line], color: [u8; 3]) {
        if !lines.is_empty() {
            self.stroked(lines, color, 0.6, Some(self.m.line * 2.0));
        }
    }

    /// Forme pleine, légèrement teintée, avec son contour.
    fn filled(&mut self, g: &Geom, color: [u8; 3], alpha: u8) {
        self.s.paint(g, Some(rgb(color, alpha)), pen(SHADOW, self.m.line + 2.0 * self.m.halo));
        self.s.paint(g, None, pen(rgb(color, 255), self.m.line));
    }

    /// Petite poignée ronde sur un point de contrôle.
    fn handle(&mut self, p: Pt, color: [u8; 3]) {
        self.s.paint(&Geom::Circle(p32(p), self.m.line * 1.3), Some(rgb(color, 255)), pen(SHADOW, self.m.halo));
    }

    /// Petite croix (centre de cercle, d'ellipse ou d'arc). Renvoie sa demi-taille.
    fn cross(&mut self, c: P2, color: [u8; 3]) -> f32 {
        let t = self.m.line * 3.0;
        self.outlined(&[open(&[(c.0 - t, c.1), (c.0 + t, c.1)]), open(&[(c.0, c.1 - t), (c.0, c.1 + t)])], color);
        t
    }

    /// Segment avec petits traits perpendiculaires aux extrémités. Renvoie
    /// la normale unitaire orientée vers le haut de l'image.
    fn segment(&mut self, pa: P2, pb: P2, color: [u8; 3]) -> P2 {
        let n = normal_up(pa, pb);
        let t = self.m.line * 3.0;
        let tick = |p: P2| open(&[(p.0 - n.0 * t, p.1 - n.1 * t), (p.0 + n.0 * t, p.1 + n.1 * t)]);
        self.outlined(&[open(&[pa, pb]), tick(pa), tick(pb)], color);
        n
    }

    /// Arc d'angle de centre `c`, un peu plus fin que les traits.
    fn arc(&mut self, c: Pt, r: f32, start: f64, sweep: f64, color: [u8; 3]) {
        self.stroked(&[poly(&geo::arc_points(c, r as f64, start, sweep, ARC_STEPS), false)], color, 0.8, None);
    }

    /// Étiquette : texte blanc sur fond sombre arrondi, liseré de couleur,
    /// centrée en (cx, cy) et ramenée dans l'image. Sans texte, rien.
    fn label(&mut self, s: &str, cx: f32, cy: f32, size: f32, accent: [u8; 3]) {
        if s.is_empty() {
            return;
        }
        let (bw, bh) = label_size(s, size);
        let Metrics { w, h, halo: m, line, .. } = self.m;
        let x0 = (cx - bw / 2.0).clamp(m, (w - bw - m).max(m));
        let y0 = (cy - bh / 2.0).clamp(m, (h - bh - m).max(m));
        // Hauteur d'une boîte d'une ligne : elle fixe l'arrondi et la première ligne.
        let one = size * 1.1 + size * 0.35;
        let frame = Geom::Rect { x: x0, y: y0, w: bw, h: bh, r: one * 0.22 };
        self.s.paint(&frame, Some(LABEL_BG), pen(rgb(accent, 230), (line * 0.5).max(1.0)));
        for (i, l) in s.lines().enumerate() {
            // Centrage vertical sur la hauteur des capitales/chiffres (~0,7 em).
            let baseline = y0 + one / 2.0 + size * 0.35 + i as f32 * size * LINE_PITCH;
            let tw = text_width(l, size);
            self.s.text(l, x0 + (bw - tw) / 2.0, baseline, size, tw);
        }
    }

    /// Place une étiquette à côté d'un point, décalée selon la direction `n`
    /// d'une distance qui tient compte de la taille de la boîte.
    fn label_beside(&mut self, s: &str, at: P2, n: P2, gap: f32, size: f32, accent: [u8; 3]) {
        let (bw, bh) = label_size(s, size);
        let d = n.0.abs() * bw / 2.0 + n.1.abs() * bh / 2.0 + gap;
        self.label(s, at.0 + n.0 * d, at.1 + n.1 * d, size, accent);
    }

    /// Dessine une forme (déjà dans le repère de la surface) et son étiquette.
    fn draw(&mut self, shape: &Shape, color: [u8; 3], label: &str) {
        let Metrics { line, halo, text: size, .. } = self.m;
        let gap = line * 2.0 + halo;
        match shape {
            Shape::Length { a, b } => {
                let (pa, pb) = (p32(*a), p32(*b));
                let n = self.segment(pa, pb, color);
                self.label_beside(label, mid(pa, pb), n, gap + line * 3.0, size, color);
            }
            Shape::LineAngle { a, b, c, d } => {
                let mut dashes = vec![];
                let (at, n) = match geo::line_angle_vertex(*a, *b, *c, *d) {
                    Some((x, u1, u2)) => {
                        // Segments prolongés jusqu'à leur intersection, arc au sommet.
                        let r = size * 2.6;
                        let reach = (r + size * 0.6) as f64;
                        for (p0, p1, u) in [(a, b, u1), (c, d, u2)] {
                            let tip = x.offset(u.x * reach, u.y * reach);
                            dashes.extend(geo::extensions(*p0, *p1, &[x, tip]).iter().map(|(e0, e1)| poly(&[*e0, *e1], false)));
                        }
                        let (start, sweep) = geo::angle_arc(x.offset(u1.x, u1.y), x, x.offset(u2.x, u2.y));
                        self.arc(x, r, start, sweep, color);
                        let (sin, cos) = ((start + sweep / 2.0) as f32).sin_cos();
                        let v = p32(x);
                        ((v.0 + cos * r, v.1 + sin * r), (cos, sin))
                    }
                    None => {
                        // Presque parallèles : lien entre les milieux des deux segments.
                        let (m1, m2) = (a.lerp(*b, 0.5), c.lerp(*d, 0.5));
                        dashes.push(poly(&[m1, m2], false));
                        (p32(m1.lerp(m2, 0.5)), (0.0, 0.0))
                    }
                };
                self.dashed(&dashes, color);
                self.segment(p32(*a), p32(*b), color);
                self.segment(p32(*c), p32(*d), color);
                if n == (0.0, 0.0) {
                    self.label(label, at.0, at.1, size, color);
                } else {
                    self.label_beside(label, at, n, gap, size, color);
                }
            }
            Shape::Path { pts } => {
                self.outlined(&[poly(pts, false)], color);
                for p in pts {
                    self.handle(*p, color);
                }
                if !pts.is_empty() {
                    // Comme à l'écran : à côté du segment du milieu.
                    let i = (pts.len() - 1) / 2;
                    let (pa, pb) = (p32(pts[i]), p32(pts[(i + 1).min(pts.len() - 1)]));
                    self.label_beside(label, mid(pa, pb), normal_up(pa, pb), gap, size, color);
                }
            }
            Shape::Angle { a, vertex, b } => {
                let (start, sweep) = geo::angle_arc(*a, *vertex, *b);
                let r = (0.35 * a.dist(*vertex).min(b.dist(*vertex)) as f32).min(size * 3.0).max(line * 4.0);
                self.outlined(&[poly(&[*a, *vertex, *b], false)], color);
                self.arc(*vertex, r, start, sweep, color);
                for p in [*a, *vertex, *b] {
                    self.handle(p, color);
                }
                let (sin, cos) = ((start + sweep / 2.0) as f32).sin_cos();
                let v = p32(*vertex);
                self.label_beside(label, (v.0 + cos * r, v.1 + sin * r), (cos, sin), gap, size, color);
            }
            Shape::Area { pts } => {
                self.filled(&Geom::Lines(&[poly(pts, true)]), color, 40);
                for p in pts {
                    self.handle(*p, color);
                }
                let n = pts.len().max(1) as f64;
                let c = p32(pts.iter().fold(Pt::default(), |acc, p| acc.offset(p.x / n, p.y / n)));
                self.label(label, c.0, c.1, size, color);
            }
            Shape::Rect { a, b, angle } => {
                self.filled(&Geom::Lines(&[poly(&geo::rect_corners(*a, *b, *angle), true)]), color, 40);
                for p in [*a, *b] {
                    self.handle(p, color);
                }
                let c = p32(a.lerp(*b, 0.5));
                self.label(label, c.0, c.1, size, color);
            }
            Shape::Circle { .. } | Shape::Circle3 { .. } => {
                if let Some((c, r)) = shape.circle() {
                    let c = p32(c);
                    self.filled(&Geom::Circle(c, r as f32), color, 35);
                    let t = self.cross(c, color);
                    // Comme à l'écran : sous le centre.
                    self.label_beside(label, c, (0.0, 1.0), gap + t, size, color);
                }
                for p in shape.points() {
                    if !matches!(shape, Shape::Circle { center, .. } if *center == p) {
                        self.handle(p, color);
                    }
                }
            }
            Shape::Profile { a, b, .. } => {
                // Segment, puis un trait en travers sur chaque bord détecté,
                // de la largeur de la bande moyennée.
                let (pa, pb) = (p32(*a), p32(*b));
                let mut lines = vec![open(&[pa, pb])];
                let mut at = mid(pa, pb);
                let (half, nrm) = shape.profile_band().unwrap_or((0.0, Pt::default()));
                let t = (half as f32).max(line * 3.0);
                let n = (nrm.x as f32, nrm.y as f32);
                if let Some([e0, e1]) = shape.profile_edges().map(|e| e.map(p32)) {
                    for e in [e0, e1] {
                        lines.push(open(&[(e.0 - n.0 * t, e.1 - n.1 * t), (e.0 + n.0 * t, e.1 + n.1 * t)]));
                    }
                    at = mid(e0, e1);
                }
                self.outlined(&lines, color);
                // Étiquette du côté haut de l'image, au-delà des traits de bord.
                self.label_beside(label, at, normal_up(pa, pb), gap + t, size, color);
            }
            Shape::PointLine { a, b, p } => {
                let (foot, t) = geo::foot_on_line(*p, *a, *b);
                // Droite prolongée en pointillés jusqu'au pied de la perpendiculaire.
                let ext: Vec<Line> = geo::extensions(*a, *b, &[foot]).iter().map(|(e0, e1)| poly(&[*e0, *e1], false)).collect();
                self.dashed(&ext, color);
                self.outlined(&[poly(&[*a, *b], false)], color);
                // Symbole d'angle droit, un côté vers le segment, l'autre vers le point.
                let along = if t > 0.5 { *a } else { *b };
                if let Some(mark) = geo::right_angle_mark(foot, along, *p, (line * 4.0) as f64) {
                    self.stroked(&[poly(&mark, false)], color, 0.6, None);
                }
                let (pf, pp) = (p32(foot), p32(*p));
                let n = self.segment(pf, pp, color);
                for q in [*a, *b, *p] {
                    self.handle(q, color);
                }
                self.label_beside(label, mid(pf, pp), n, gap + line * 3.0, size, color);
            }
            Shape::Parallel { a, b, p } => {
                let (start, end, c0, c1) = geo::parallel_through(*a, *b, *p);
                for l in [[*a, *b], [start, end]] {
                    self.outlined(&[poly(&l, false)], color);
                }
                let (q0, q1) = (p32(c0), p32(c1));
                let n = self.segment(q0, q1, color);
                for q in [*a, *b, *p] {
                    self.handle(q, color);
                }
                self.label_beside(label, mid(q0, q1), n, gap + line * 3.0, size, color);
            }
            Shape::Ellipse { center, a, b } => {
                self.filled(&Geom::Lines(&[poly(&geo::ellipse_outline(*center, *a, *b, 180), true)]), color, 35);
                let c = p32(*center);
                let t = self.cross(c, color);
                self.dashed(&[poly(&[*center, *a], false), poly(&[*center, *b], false)], color);
                // Comme à l'écran : sous le centre.
                self.label_beside(label, c, (0.0, 1.0), gap + t, size, color);
            }
            Shape::Arc { a, m, b } => {
                match geo::arc_from_3_points(*a, *m, *b) {
                    Some(arc) => {
                        let pts: Vec<Pt> = (0..=180).map(|i| arc.point(i as f64 / 180.0)).collect();
                        let (top, c) = (arc.point(0.5), arc.center);
                        // Rayon en pointillés du milieu de l'arc vers le centre.
                        self.dashed(&[poly(&[top, c], false)], color);
                        self.cross(p32(c), color);
                        self.outlined(&[poly(&pts, false)], color);
                        let n = if top.dist(c) > 0.0 { unit(p32(c), p32(top)) } else { (0.0, -1.0) };
                        self.label_beside(label, p32(top), n, gap, size, color);
                    }
                    None => self.outlined(&[poly(&[*a, *m, *b], false)], color),
                }
                for q in [*a, *m, *b] {
                    self.handle(q, color);
                }
            }
            Shape::Marker { p } => {
                let at = p32(*p);
                let r = line * 2.2;
                self.s.paint(&Geom::Circle(at, r), Some(rgb(color, 255)), pen(SHADOW, halo * 1.5));
                let d = std::f32::consts::FRAC_1_SQRT_2;
                self.label_beside(label, at, (d, -d), r + halo, size * 0.85, color);
            }
            Shape::Text { p, text } => {
                if text.is_empty() {
                    self.handle(*p, color);
                } else {
                    let at = p32(*p);
                    self.label(label, at.0, at.1, size, color);
                }
            }
            Shape::Arrow { a, b, .. } => {
                let (pa, pb) = (p32(*a), p32(*b));
                if let Some((head, tri)) = arrow_head(pa, pb, line) {
                    // Le trait s'arrête dans la pointe pour ne pas dépasser de son sommet.
                    let u = unit(pa, pb);
                    self.outlined(&[open(&[pa, (pb.0 - u.0 * head * 0.5, pb.1 - u.1 * head * 0.5)])], color);
                    let tip = [Line { pts: tri.to_vec(), closed: true }];
                    self.s.paint(&Geom::Lines(&tip), None, pen(SHADOW, 2.0 * halo));
                    self.s.paint(&Geom::Lines(&tip), Some(rgb(color, 255)), None);
                }
                // À la base, du côté opposé à la pointe.
                self.label_beside(label, pa, unit(pb, pa), gap, size, color);
            }
        }
    }

    /// Barre d'échelle dans le coin choisi : longueur ronde visant ~17,5 %
    /// de la largeur de l'image, sur fond sombre pour rester lisible.
    fn scale_bar(&mut self, units_per_px: f64, unit: &str, corner: Corner) {
        let Metrics { w, h, line, halo, text: size } = self.m;
        let Some(bar) = meteroscope_core::scale_bar(units_per_px, unit, w as f64 * TARGET_FRACTION) else { return };
        let len = bar.px as f32;
        let pad = size * 0.5;
        let bar_h = line * 1.6;
        let tw = text_width(&bar.label, size);
        let bw = len.max(tw) + 2.0 * pad;
        let bh = size * 1.1 + bar_h + 2.5 * pad;
        let (x0, y0) = corner.place((0.0, 0.0, w, h), (bw, bh), size);
        self.s.paint(&Geom::Rect { x: x0, y: y0, w: bw, h: bh, r: pad * 0.6 }, Some(LABEL_BG), None);
        let bx = x0 + (bw - len) / 2.0;
        let by = y0 + bh - pad - bar_h;
        self.s.paint(&Geom::Rect { x: bx, y: by, w: len, h: bar_h, r: 0.0 }, Some(WHITE), None);
        // Graduations aux extrémités.
        for x in [bx, bx + len - halo] {
            self.s.paint(&Geom::Rect { x, y: by - bar_h, w: halo.max(1.0), h: bar_h * 2.0, r: 0.0 }, Some(WHITE), None);
        }
        self.s.text(&bar.label, x0 + (bw - tw) / 2.0, by - pad * 0.9, size, tw);
    }
}

/// Dessine les mesures visibles de `doc` sur une surface dont le repère est
/// `frame` (l'image d'origine, pivotée et éventuellement réduite).
pub(crate) fn draw_measurements<S: Surface>(s: &mut S, doc: &Document, frame: &Frame) {
    let mut markers = marker_labels(&doc.measurements);
    for m in doc.measurements.iter().filter(|m| m.visible) {
        let label = markers.remove(&m.id).unwrap_or_else(|| label_text(&m.shape, &doc.calibration, 0));
        s.open(m);
        Pencil::new(s).draw(&frame.place(&m.shape), m.color, &label);
        s.close();
    }
}

/// Dessine la barre d'échelle de `doc` si elle est affichée. Elle est
/// horizontale dans l'image écrite, dont un pixel vaut `1 / frame.scale`
/// pixel de l'image d'origine.
pub(crate) fn draw_scale_bar<S: Surface>(s: &mut S, doc: &Document, frame: &Frame) {
    if let Some(sc) = &doc.calibration.scale
        && doc.scale_bar.visible
    {
        Pencil::new(s).scale_bar(sc.units_per_px / frame.scale, &sc.unit, doc.scale_bar.corner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Surface qui ne retient que les textes : ce qu'un export affiche.
    struct Texts(Metrics, Vec<String>);

    impl Surface for Texts {
        fn metrics(&self) -> Metrics {
            self.0
        }
        fn paint(&mut self, _g: &Geom, _fill: Option<Rgba>, _pen: Option<Pen>) {}
        fn text(&mut self, s: &str, _x: f32, _y: f32, _size: f32, _width: f32) {
            self.1.push(s.to_owned());
        }
    }

    /// Les valeurs dessinées sont celles du document, quel que soit le
    /// repère (pivoté, réduit) et que le document soit étalonné ou non.
    #[test]
    fn valeurs_independantes_du_repere() {
        meteroscope_i18n::set_language("fr");
        let mut doc = Document::new();
        doc.add(Shape::Length { a: Pt::new(400., 300.), b: Pt::new(3400., 300.) });
        doc.add(Shape::Rect { a: Pt::new(100., 100.), b: Pt::new(900., 500.), angle: 0.0 });
        let mut profile = Shape::profile(Pt::new(1000., 400.), Pt::new(2000., 400.));
        if let Shape::Profile { result, .. } = &mut profile {
            *result = Some(meteroscope_core::ProfileResult { edges: [200.0, 700.0], tilt: 0.0 });
        }
        doc.add(profile);
        let texts = |doc: &Document, frame: &Frame| {
            let mut s = Texts(Metrics::new(2400.0, 300.0), vec![]);
            draw_measurements(&mut s, doc, frame);
            s.1
        };
        let full = Frame::rotation(4800, 600, 0.0);
        let small = Frame::new((4800, 600), (2400, 300), 0.5, 12.0);
        for calibrated in [false, true] {
            if calibrated {
                doc.calibration.set_from_reference(1000.0, 1.0, "mm");
            }
            let expected = texts(&doc, &full);
            assert_eq!(expected[0], if calibrated { "3,00 mm" } else { "3000 px" });
            assert_eq!(expected[3], if calibrated { "0,500 mm" } else { "500,0 px" }, "profil");
            assert_eq!(texts(&doc, &small), expected);
        }
    }

    #[test]
    fn etiquette_de_plusieurs_lignes() {
        meteroscope_i18n::set_language("fr");
        let (w1, h1) = label_size("20,00 × 10,00 px", 20.0);
        let (w2, h2) = label_size("20,00 × 10,00 px\n200,0 px²", 20.0);
        assert_eq!(w1, w2);
        assert!((h2 - h1 - 20.0 * LINE_PITCH).abs() < 1e-4);
    }
}

//! Image annotée : les mesures visibles dessinées à la résolution de l'image.
//!
//! Le dessin se fait sur un calque transparent (tiny-skia, antialiasé) puis est
//! composé sur une copie de l'image : les pixels non touchés restent identiques.

use ab_glyph::{Font, FontRef, OutlineCurve};
use image::RgbaImage;
use mesura_core::{Document, Pt, Shape};
use std::f32::consts::PI;
use std::sync::OnceLock;
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

/// Police embarquée (Ubuntu Light, licence libre UFL, fournie par egui).
fn font() -> &'static FontRef<'static> {
    static FONT: OnceLock<FontRef<'static>> = OnceLock::new();
    FONT.get_or_init(|| FontRef::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).expect("police embarquée valide"))
}

type Rgba = [u8; 4];
const SHADOW: Rgba = [0, 0, 0, 190];
const LABEL_BG: Rgba = [0, 0, 0, 185];
const WHITE: Rgba = [255, 255, 255, 255];

fn paint(c: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c[0], c[1], c[2], c[3]);
    p.anti_alias = true;
    p
}

fn stroke(width: f32) -> Stroke {
    Stroke { width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() }
}

fn rgb(c: [u8; 3], a: u8) -> Rgba {
    [c[0], c[1], c[2], a]
}

fn p32(p: Pt) -> (f32, f32) {
    (p.x as f32, p.y as f32)
}

/// Peintre : calque + dimensions de référence proportionnelles à l'image.
struct Painter {
    pm: Pixmap,
    w: f32,
    h: f32,
    /// Épaisseur du trait coloré.
    line: f32,
    /// Largeur du contour sombre de chaque côté.
    halo: f32,
    /// Taille du texte (em, pixels).
    text: f32,
}

impl Painter {
    fn new(w: u32, h: u32) -> Option<Painter> {
        let pm = Pixmap::new(w, h)?;
        // Unité : 1 pour une image de 1000 px de grand côté.
        let u = (w.max(h) as f32 / 1000.0).max(0.6);
        Some(Painter {
            pm,
            w: w as f32,
            h: h as f32,
            line: (2.4 * u).max(1.8),
            halo: (1.2 * u).max(1.0),
            text: (17.0 * u).max(13.0),
        })
    }

    fn fill(&mut self, path: &Path, c: Rgba) {
        self.pm.fill_path(path, &paint(c), FillRule::Winding, Transform::identity(), None);
    }

    fn stroke(&mut self, path: &Path, c: Rgba, width: f32) {
        self.pm.stroke_path(path, &paint(c), &stroke(width), Transform::identity(), None);
    }

    /// Trait coloré avec contour sombre.
    fn outlined(&mut self, path: &Path, color: [u8; 3]) {
        self.stroke(path, SHADOW, self.line + 2.0 * self.halo);
        self.stroke(path, rgb(color, 255), self.line);
    }

    /// Petite poignée ronde sur un point de contrôle.
    fn handle(&mut self, p: Pt, color: [u8; 3]) {
        let (x, y) = p32(p);
        if let Some(c) = PathBuilder::from_circle(x, y, self.line * 1.3) {
            self.fill(&c, rgb(color, 255));
            self.stroke(&c, SHADOW, self.halo);
        }
    }

    // ----- texte -----

    fn scale(&self, size: f32) -> f32 {
        size / font().units_per_em().unwrap_or(1000.0)
    }

    /// Largeur du texte en pixels.
    fn text_width(&self, s: &str, size: f32) -> f32 {
        let f = font();
        let k = self.scale(size);
        let mut x = 0.0;
        let mut prev = None;
        for ch in s.chars() {
            let id = f.glyph_id(ch);
            if let Some(p) = prev {
                x += f.kern_unscaled(p, id) * k;
            }
            x += f.h_advance_unscaled(id) * k;
            prev = Some(id);
        }
        x
    }

    /// Contours du texte, ligne de base en (x, y).
    fn text_path(&self, s: &str, x: f32, y: f32, size: f32) -> Option<Path> {
        let f = font();
        let k = self.scale(size);
        let mut pb = PathBuilder::new();
        let mut caret = x;
        let mut prev = None;
        for ch in s.chars() {
            let id = f.glyph_id(ch);
            if let Some(p) = prev {
                caret += f.kern_unscaled(p, id) * k;
            }
            if let Some(outline) = f.outline(id) {
                // Unités de police (y vers le haut) → pixels image (y vers le bas).
                let tr = |p: ab_glyph::Point| (caret + p.x * k, y - p.y * k);
                let mut last: Option<(f32, f32)> = None;
                for c in &outline.curves {
                    let start = match c {
                        OutlineCurve::Line(a, _) | OutlineCurve::Quad(a, _, _) | OutlineCurve::Cubic(a, _, _, _) => tr(*a),
                    };
                    if last.is_none_or(|l| (l.0 - start.0).abs() > 1e-3 || (l.1 - start.1).abs() > 1e-3) {
                        if last.is_some() {
                            pb.close();
                        }
                        pb.move_to(start.0, start.1);
                    }
                    let end = match c {
                        OutlineCurve::Line(_, b) => {
                            let b = tr(*b);
                            pb.line_to(b.0, b.1);
                            b
                        }
                        OutlineCurve::Quad(_, c1, b) => {
                            let (c1, b) = (tr(*c1), tr(*b));
                            pb.quad_to(c1.0, c1.1, b.0, b.1);
                            b
                        }
                        OutlineCurve::Cubic(_, c1, c2, b) => {
                            let (c1, c2, b) = (tr(*c1), tr(*c2), tr(*b));
                            pb.cubic_to(c1.0, c1.1, c2.0, c2.1, b.0, b.1);
                            b
                        }
                    };
                    last = Some(end);
                }
                if last.is_some() {
                    pb.close();
                }
            }
            caret += f.h_advance_unscaled(id) * k;
            prev = Some(id);
        }
        pb.finish()
    }

    /// Dimensions (largeur, hauteur) de la boîte d'étiquette.
    fn label_size(&self, s: &str, size: f32) -> (f32, f32) {
        let pad = size * 0.35;
        (self.text_width(s, size) + 2.0 * pad, size * 1.1 + pad)
    }

    /// Étiquette : texte blanc sur fond sombre arrondi, liseré de couleur,
    /// centrée en (cx, cy) et ramenée dans l'image.
    fn label(&mut self, s: &str, cx: f32, cy: f32, size: f32, accent: [u8; 3]) {
        let (bw, bh) = self.label_size(s, size);
        let m = self.halo;
        let x0 = (cx - bw / 2.0).clamp(m, (self.w - bw - m).max(m));
        let y0 = (cy - bh / 2.0).clamp(m, (self.h - bh - m).max(m));
        if let Some(r) = rounded_rect(x0, y0, bw, bh, bh * 0.22) {
            self.fill(&r, LABEL_BG);
            self.stroke(&r, rgb(accent, 230), (self.line * 0.5).max(1.0));
        }
        // Centrage vertical sur la hauteur des capitales/chiffres (~0,7 em).
        let baseline = y0 + bh / 2.0 + size * 0.35;
        let tw = self.text_width(s, size);
        if let Some(p) = self.text_path(s, x0 + (bw - tw) / 2.0, baseline, size) {
            self.fill(&p, WHITE);
            // Léger épaississement : la police Light seule est un peu fine.
            self.stroke(&p, WHITE, size * 0.035);
        }
    }

    /// Place une étiquette à côté d'un point, décalée selon la normale (nx, ny)
    /// d'une distance qui tient compte de la taille de la boîte.
    fn label_beside(&mut self, s: &str, at: (f32, f32), n: (f32, f32), gap: f32, size: f32, accent: [u8; 3]) {
        let (bw, bh) = self.label_size(s, size);
        let d = n.0.abs() * bw / 2.0 + n.1.abs() * bh / 2.0 + gap;
        self.label(s, at.0 + n.0 * d, at.1 + n.1 * d, size, accent);
    }

    // ----- mesures -----

    fn draw(&mut self, shape: &Shape, color: [u8; 3], main: Option<String>, marker_no: usize) {
        let gap = self.line * 2.0 + self.halo;
        let size = self.text;
        match shape {
            Shape::Length { a, b } => {
                let (pa, pb_) = (p32(*a), p32(*b));
                let (dx, dy) = (pb_.0 - pa.0, pb_.1 - pa.1);
                let len = dx.hypot(dy).max(1e-6);
                // Normale orientée vers le haut de l'image.
                let mut n = (-dy / len, dx / len);
                if n.1 > 0.0 || (n.1 == 0.0 && n.0 > 0.0) {
                    n = (-n.0, -n.1);
                }
                let mut pbld = PathBuilder::new();
                pbld.move_to(pa.0, pa.1);
                pbld.line_to(pb_.0, pb_.1);
                // Petits traits perpendiculaires aux extrémités.
                let t = self.line * 3.0;
                for p in [pa, pb_] {
                    pbld.move_to(p.0 - n.0 * t, p.1 - n.1 * t);
                    pbld.line_to(p.0 + n.0 * t, p.1 + n.1 * t);
                }
                if let Some(p) = pbld.finish() {
                    self.outlined(&p, color);
                }
                if let Some(s) = main {
                    let mid = ((pa.0 + pb_.0) / 2.0, (pa.1 + pb_.1) / 2.0);
                    self.label_beside(&s, mid, n, gap + t, size, color);
                }
            }
            Shape::Path { pts } => {
                if let Some(p) = polyline(pts, false) {
                    self.outlined(&p, color);
                }
                for p in pts {
                    self.handle(*p, color);
                }
                if let (Some(s), Some(last)) = (main, pts.last()) {
                    // Dans le prolongement du dernier segment, pour ne pas le masquer.
                    let at = p32(*last);
                    let prev = p32(pts[pts.len().saturating_sub(2)]);
                    let (dx, dy) = (at.0 - prev.0, at.1 - prev.1);
                    let l = dx.hypot(dy);
                    let n = if l > 1e-6 { (dx / l, dy / l) } else { (0.0, -1.0) };
                    self.label_beside(&s, at, n, gap, size, color);
                }
            }
            Shape::Angle { a, vertex, b } => {
                let (pa, pv, pb_) = (p32(*a), p32(*vertex), p32(*b));
                let la = (pa.0 - pv.0).hypot(pa.1 - pv.1);
                let lb = (pb_.0 - pv.0).hypot(pb_.1 - pv.1);
                let ta = (pa.1 - pv.1).atan2(pa.0 - pv.0);
                let tb = (pb_.1 - pv.1).atan2(pb_.0 - pv.0);
                // Balayage du plus petit angle, de a vers b.
                let mut sweep = tb - ta;
                while sweep > PI {
                    sweep -= 2.0 * PI;
                }
                while sweep <= -PI {
                    sweep += 2.0 * PI;
                }
                let r = (0.35 * la.min(lb)).min(self.text * 3.0).max(self.line * 4.0);
                let mut rays = PathBuilder::new();
                rays.move_to(pa.0, pa.1);
                rays.line_to(pv.0, pv.1);
                rays.line_to(pb_.0, pb_.1);
                let steps = 48;
                let mut arc = PathBuilder::new();
                for i in 0..=steps {
                    let t = ta + sweep * i as f32 / steps as f32;
                    let (x, y) = (pv.0 + r * t.cos(), pv.1 + r * t.sin());
                    if i == 0 { arc.move_to(x, y) } else { arc.line_to(x, y) }
                }
                if let Some(p) = rays.finish() {
                    self.outlined(&p, color);
                }
                if let Some(p) = arc.finish() {
                    self.stroke(&p, SHADOW, self.line * 0.8 + 2.0 * self.halo);
                    self.stroke(&p, rgb(color, 255), self.line * 0.8);
                }
                for p in [*a, *vertex, *b] {
                    self.handle(p, color);
                }
                if let Some(s) = main {
                    let tm = ta + sweep / 2.0;
                    let n = (tm.cos(), tm.sin());
                    let at = (pv.0 + n.0 * r, pv.1 + n.1 * r);
                    self.label_beside(&s, at, n, gap, size, color);
                }
            }
            Shape::Area { pts } => {
                if let Some(p) = polyline(pts, true) {
                    self.fill(&p, rgb(color, 40));
                    self.outlined(&p, color);
                }
                for p in pts {
                    self.handle(*p, color);
                }
                if let Some(s) = main {
                    let n = pts.len().max(1) as f64;
                    let c = pts.iter().fold(Pt::default(), |acc, p| acc.offset(p.x / n, p.y / n));
                    let c = p32(c);
                    self.label(&s, c.0, c.1, size, color);
                }
            }
            Shape::Rect { a, b } => {
                let r = tiny_skia::Rect::from_ltrb(
                    a.x.min(b.x) as f32,
                    a.y.min(b.y) as f32,
                    a.x.max(b.x) as f32,
                    a.y.max(b.y) as f32,
                );
                if let Some(r) = r {
                    let p = PathBuilder::from_rect(r);
                    self.fill(&p, rgb(color, 40));
                    self.outlined(&p, color);
                }
                for p in [*a, *b] {
                    self.handle(p, color);
                }
                if let Some(s) = main {
                    self.label(&s, ((a.x + b.x) / 2.0) as f32, ((a.y + b.y) / 2.0) as f32, size, color);
                }
            }
            Shape::Circle { .. } | Shape::Circle3 { .. } => {
                if let Some((c, r)) = shape.circle() {
                    let (cx, cy) = p32(c);
                    if let Some(p) = PathBuilder::from_circle(cx, cy, r as f32) {
                        self.fill(&p, rgb(color, 35));
                        self.outlined(&p, color);
                    }
                    // Croix au centre.
                    let t = self.line * 3.0;
                    let mut cross = PathBuilder::new();
                    cross.move_to(cx - t, cy);
                    cross.line_to(cx + t, cy);
                    cross.move_to(cx, cy - t);
                    cross.line_to(cx, cy + t);
                    if let Some(p) = cross.finish() {
                        self.outlined(&p, color);
                    }
                    if let Some(s) = main {
                        self.label_beside(&s, (cx, cy), (0.0, -1.0), gap + t, size, color);
                    }
                }
                for p in shape.points() {
                    if !matches!(shape, Shape::Circle { center, .. } if *center == p) {
                        self.handle(p, color);
                    }
                }
            }
            Shape::Marker { p } => {
                let (x, y) = p32(*p);
                let r = self.line * 2.2;
                if let Some(c) = PathBuilder::from_circle(x, y, r) {
                    self.fill(&c, rgb(color, 255));
                    self.stroke(&c, SHADOW, self.halo * 1.5);
                }
                let s = marker_no.to_string();
                let size = size * 0.85;
                let d = std::f32::consts::FRAC_1_SQRT_2;
                self.label_beside(&s, (x, y), (d, -d), r + self.halo, size, color);
            }
        }
    }

    /// Barre d'échelle en bas à droite.
    fn scale_bar(&mut self, units_per_px: f64, unit: &str) {
        if !(units_per_px > 0.0 && units_per_px.is_finite()) {
            return;
        }
        let target = 0.2 * self.w as f64 * units_per_px;
        let base = 10f64.powf(target.log10().floor());
        let m = target / base;
        let nice = base * if m >= 5.0 { 5.0 } else if m >= 2.0 { 2.0 } else { 1.0 };
        let len = (nice / units_per_px) as f32;
        let text = format!("{} {unit}", crate::table::format_precise(nice));
        let size = self.text;
        let pad = size * 0.5;
        let bar_h = self.line * 1.6;
        let tw = self.text_width(&text, size);
        let bw = len.max(tw) + 2.0 * pad;
        let bh = size * 1.1 + bar_h + 2.5 * pad;
        let margin = self.text;
        let x0 = (self.w - bw - margin).max(0.0);
        let y0 = (self.h - bh - margin).max(0.0);
        if let Some(r) = rounded_rect(x0, y0, bw, bh, pad * 0.6) {
            self.fill(&r, LABEL_BG);
        }
        let bx = x0 + (bw - len) / 2.0;
        let by = y0 + bh - pad - bar_h;
        if let Some(r) = tiny_skia::Rect::from_xywh(bx, by, len, bar_h) {
            self.fill(&PathBuilder::from_rect(r), WHITE);
        }
        // Graduations aux extrémités.
        for x in [bx, bx + len - self.halo] {
            if let Some(r) = tiny_skia::Rect::from_xywh(x, by - bar_h, self.halo.max(1.0), bar_h * 2.0) {
                self.fill(&PathBuilder::from_rect(r), WHITE);
            }
        }
        let baseline = by - pad * 0.9;
        if let Some(p) = self.text_path(&text, x0 + (bw - tw) / 2.0, baseline, size) {
            self.fill(&p, WHITE);
            self.stroke(&p, WHITE, size * 0.035);
        }
    }
}

fn polyline(pts: &[Pt], closed: bool) -> Option<Path> {
    let mut pb = PathBuilder::new();
    for (i, p) in pts.iter().enumerate() {
        let (x, y) = p32(*p);
        if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
    }
    if closed {
        pb.close();
    }
    pb.finish()
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<Path> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut pb = PathBuilder::new();
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

/// Compose le calque (prémultiplié) sur l'image (alpha droit), « source over ».
fn composite(dst: &mut RgbaImage, overlay: &Pixmap) {
    for (d, s) in dst.as_mut().as_chunks_mut::<4>().0.iter_mut().zip(overlay.data().as_chunks::<4>().0) {
        let sa = s[3] as u32;
        if sa == 0 {
            continue;
        }
        let da = d[3] as u32;
        // Alpha résultant (sur 255) et couleurs prémultipliées.
        let inv = 255 - sa;
        let oa = sa * 255 + da * inv; // ×255
        if oa == 0 {
            continue;
        }
        for c in 0..3 {
            let premul = s[c] as u32 * 255 * 255 + d[c] as u32 * da * inv; // ×255²
            d[c] = ((premul + oa / 2) / oa).min(255) as u8;
        }
        d[3] = ((oa + 127) / 255) as u8;
    }
}

pub fn render_annotated(image: &RgbaImage, doc: &Document) -> RgbaImage {
    let mut out = image.clone();
    let Some(mut p) = Painter::new(image.width(), image.height()) else { return out };
    let mut marker_no = 0;
    for m in &doc.measurements {
        if matches!(m.shape, Shape::Marker { .. }) {
            marker_no += 1;
        }
        if !m.visible {
            continue;
        }
        let main = m.shape.values(&doc.calibration).first().map(|v| v.text());
        p.draw(&m.shape, m.color, main, marker_no);
    }
    if let Some(s) = &doc.calibration.scale {
        p.scale_bar(s.units_per_px, &s.unit);
    }
    composite(&mut out, &p.pm);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn font_has_needed_glyphs() {
        let f = font();
        for ch in ['°', '²', 'µ', '×', ',', '0', 'é'] {
            assert_ne!(f.glyph_id(ch).0, 0, "glyphe manquant : {ch}");
        }
    }

    #[test]
    fn draws_where_measurements_are() {
        for (w, h) in [(640u32, 480u32), (2000, 1500)] {
            let img = RgbaImage::from_pixel(w, h, Rgba([90, 90, 90, 255]));
            let mut doc = Document::new();
            let k = w as f64 / 640.0;
            doc.add(Shape::Length { a: Pt::new(50.0 * k, 400.0 * k), b: Pt::new(300.0 * k, 400.0 * k) });
            let hidden = doc.add(Shape::Rect { a: Pt::new(400.0 * k, 50.0 * k), b: Pt::new(600.0 * k, 150.0 * k) });
            doc.get_mut(hidden).unwrap().visible = false;
            doc.calibration.set_from_reference(100.0, 1.0, "mm");
            let out = render_annotated(&img, &doc);
            assert_eq!(out.dimensions(), img.dimensions());
            // Sur le trait : couleur de la mesure.
            let px = out.get_pixel((100.0 * k) as u32, (400.0 * k) as u32);
            assert_eq!(&px.0[..3], &doc.measurements[0].color[..]);
            // Loin de tout : inchangé ; mesure masquée : inchangée.
            assert_eq!(out.get_pixel(w / 2, h / 4 * 3 - (20.0 * k) as u32), img.get_pixel(0, 0));
            assert_eq!(out.get_pixel((400.0 * k) as u32, (100.0 * k) as u32), img.get_pixel(0, 0));
            // Barre d'échelle : des pixels blancs dans le coin inférieur droit.
            let white = (w * 3 / 4..w).flat_map(|x| (h * 3 / 4..h).map(move |y| (x, y)))
                .filter(|&(x, y)| out.get_pixel(x, y).0 == [255, 255, 255, 255])
                .count();
            assert!(white > 50);
        }
    }

    #[test]
    fn transparent_pixels_untouched() {
        let img = RgbaImage::from_pixel(50, 40, Rgba([10, 20, 30, 0]));
        let out = render_annotated(&img, &Document::new());
        assert_eq!(out, img);
    }
}

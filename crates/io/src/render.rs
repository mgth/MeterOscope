//! Image annotée : les mesures visibles dessinées à la résolution de l'image.
//!
//! Le dessin (commun à tous les exports, voir [`crate::draw`]) se fait sur un
//! calque transparent (tiny-skia, antialiasé) puis est composé sur une copie
//! de l'image : les pixels non touchés restent identiques.

use crate::draw::{Geom, Metrics, Pen, Rgba, Surface, WHITE, draw_measurements, draw_scale_bar};
use crate::fonts;
use crate::rotate::{Frame, RotationFilter, WORK_BUDGET, straighten};
use ab_glyph::{Font, OutlineCurve};
use image::RgbaImage;
use meteroscope_core::Document;
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, Path, PathBuilder, Pixmap, Stroke, Transform};

/// Police d'un caractère : Ubuntu Light embarquée (licence libre UFL, fournie
/// par egui), ou une police du système pour les écritures qu'elle n'a pas
/// (japonais, coréen, chinois…), voir [`crate::fonts`].
fn font_for(ch: char) -> &'static ab_glyph::FontRef<'static> {
    &fonts::get(fonts::index_for(ch)).font
}

fn paint(c: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c[0], c[1], c[2], c[3]);
    p.anti_alias = true;
    p
}

/// Calque de pixels sur lequel se dessinent les mesures.
struct Painter {
    pm: Pixmap,
    m: Metrics,
}

impl Painter {
    fn new(w: u32, h: u32) -> Option<Painter> {
        Some(Painter { pm: Pixmap::new(w, h)?, m: Metrics::new(w as f32, h as f32) })
    }
}

fn path_of(g: &Geom) -> Option<Path> {
    match *g {
        Geom::Lines(lines) => {
            let mut pb = PathBuilder::new();
            for l in lines {
                for (i, p) in l.pts.iter().enumerate() {
                    if i == 0 { pb.move_to(p.0, p.1) } else { pb.line_to(p.0, p.1) }
                }
                if l.closed {
                    pb.close();
                }
            }
            pb.finish()
        }
        Geom::Circle(c, r) => PathBuilder::from_circle(c.0, c.1, r),
        Geom::Rect { x, y, w, h, r } if r <= 0.0 => tiny_skia::Rect::from_xywh(x, y, w, h).map(PathBuilder::from_rect),
        Geom::Rect { x, y, w, h, r } => rounded_rect(x, y, w, h, r),
    }
}

impl Surface for Painter {
    fn metrics(&self) -> Metrics {
        self.m
    }

    fn paint(&mut self, g: &Geom, fill: Option<Rgba>, pen: Option<Pen>) {
        let Some(path) = path_of(g) else { return };
        if let Some(c) = fill {
            self.pm.fill_path(&path, &paint(c), FillRule::Winding, Transform::identity(), None);
        }
        if let Some(pen) = pen {
            let mut st = Stroke { width: pen.width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
            st.dash = pen.dash.and_then(|d| tiny_skia::StrokeDash::new(vec![d, d], 0.0));
            self.pm.stroke_path(&path, &paint(pen.color), &st, Transform::identity(), None);
        }
    }

    fn text(&mut self, s: &str, x: f32, y: f32, size: f32, _width: f32) {
        // Léger épaississement : la police Light seule est un peu fine.
        let outline = Pen { color: WHITE, width: size * 0.035, dash: None };
        if let Some(p) = text_path(s, x, y, size) {
            self.pm.fill_path(&p, &paint(WHITE), FillRule::Winding, Transform::identity(), None);
            let st = Stroke { width: outline.width, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
            self.pm.stroke_path(&p, &paint(outline.color), &st, Transform::identity(), None);
        }
    }
}

/// Contours du texte, ligne de base en (x, y).
fn text_path(s: &str, x: f32, y: f32, size: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let mut caret = x;
    let mut prev = None;
    for ch in s.chars() {
        let f = font_for(ch);
        let k = scale(f, size);
        let id = f.glyph_id(ch);
        if let Some((pf, p)) = prev
            && std::ptr::eq(pf, f)
        {
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
        prev = Some((f, id));
    }
    pb.finish()
}

fn scale(f: &ab_glyph::FontRef<'static>, size: f32) -> f32 {
    size / f.units_per_em().unwrap_or(1000.0)
}

/// Largeur d'un texte en pixels (une ligne), polices de repli comprises.
pub(crate) fn text_width(s: &str, size: f32) -> f32 {
    let mut x = 0.0;
    let mut prev: Option<(&ab_glyph::FontRef<'static>, ab_glyph::GlyphId)> = None;
    for ch in s.chars() {
        let f = font_for(ch);
        let k = scale(f, size);
        let id = f.glyph_id(ch);
        if let Some((pf, p)) = prev
            && std::ptr::eq(pf, f)
        {
            x += f.kern_unscaled(p, id) * k;
        }
        x += f.h_advance_unscaled(id) * k;
        prev = Some((f, id));
    }
    x
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

/// Dessine les mesures de `doc` sur `image`, la photo telle quelle (le
/// redressement du document n'est pas appliqué, voir [`render_view`]).
pub fn render_annotated(image: &RgbaImage, doc: &Document) -> RgbaImage {
    render_annotated_in(image, doc, &Frame::rotation(image.width(), image.height(), 0.0))
}

/// Dessine les mesures de `doc` sur `image`, qui est la photo dans le repère
/// `frame` (redressée, éventuellement réduite) : les formes y sont reportées,
/// les valeurs restent celles des mesures d'origine.
pub fn render_annotated_in(image: &RgbaImage, doc: &Document, frame: &Frame) -> RgbaImage {
    let mut out = image.clone();
    let Some(mut p) = Painter::new(image.width(), image.height()) else { return out };
    draw_measurements(&mut p, doc, frame);
    draw_scale_bar(&mut p, doc, frame);
    composite(&mut out, &p.pm);
    out
}

/// Image annotée telle que la vue la montre : la photo est redressée de
/// `doc.rotation` avec `filter`, réduite si son grand côté dépasserait
/// `max_side`, et les mesures y sont reportées.
pub fn render_view(image: &RgbaImage, doc: &Document, filter: RotationFilter, max_side: u32) -> RgbaImage {
    if doc.rotation == 0.0 && image.width().max(image.height()) <= max_side {
        return render_annotated(image, doc);
    }
    let s = straighten(image, doc.rotation, filter, max_side, WORK_BUDGET);
    render_annotated_in(&s.image, doc, &s.frame)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    use meteroscope_core::{Pt, Shape};

    #[test]
    fn font_has_needed_glyphs() {
        meteroscope_i18n::set_language("fr");
        let f = &fonts::main().font;
        for ch in ['°', '²', 'µ', '×', ',', '0', 'é', 'ł', 'ã', 'ß', 'ñ'] {
            assert_ne!(f.glyph_id(ch).0, 0, "glyphe manquant : {ch}");
        }
    }

    #[test]
    fn draws_where_measurements_are() {
        meteroscope_i18n::set_language("fr");
        for (w, h) in [(640u32, 480u32), (2000, 1500)] {
            let img = RgbaImage::from_pixel(w, h, Rgba([90, 90, 90, 255]));
            let mut doc = Document::new();
            let k = w as f64 / 640.0;
            doc.add(Shape::Length { a: Pt::new(50.0 * k, 400.0 * k), b: Pt::new(300.0 * k, 400.0 * k) });
            let hidden = doc.add(Shape::Rect { a: Pt::new(400.0 * k, 50.0 * k), b: Pt::new(600.0 * k, 150.0 * k), angle: 0.0 });
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
    fn annotations_drawn() {
        meteroscope_i18n::set_language("fr");
        let img = RgbaImage::from_pixel(640, 480, Rgba([90, 90, 90, 255]));
        let mut doc = Document::new();
        doc.add(Shape::Arrow { a: Pt::new(50., 400.), b: Pt::new(300., 400.), text: "base".into() });
        doc.add(Shape::Text { p: Pt::new(400., 100.), text: "Fissure 裂缝 균열".into() });
        let out = render_annotated(&img, &doc);
        assert_eq!(&out.get_pixel(150, 400).0[..3], &doc.measurements[0].color[..]);
        // Texte : fond d'étiquette sombre et pixels blancs (le texte) autour du point.
        let white = (330..470u32).flat_map(|x| (85..115u32).map(move |y| (x, y)))
            .filter(|&(x, y)| out.get_pixel(x, y).0[0] > 200 && out.get_pixel(x, y).0[1] > 200)
            .count();
        assert!(white > 30, "{white}");
        // Les idéogrammes ont des contours (repli sur une police du système).
        if fonts::index_for('裂') != 0 {
            assert!(text_width("裂缝", 20.0) > 30.0);
        }
    }

    #[test]
    fn nouvelles_formes_dessinees() {
        meteroscope_i18n::set_language("fr");
        let img = RgbaImage::from_pixel(640, 480, Rgba([90, 90, 90, 255]));
        let mut doc = Document::new();
        let pl = doc.add(Shape::PointLine { a: Pt::new(50., 100.), b: Pt::new(150., 100.), p: Pt::new(250., 40.) });
        let par = doc.add(Shape::Parallel { a: Pt::new(50., 200.), b: Pt::new(150., 200.), p: Pt::new(60., 260.) });
        let el = doc.add(Shape::Ellipse { center: Pt::new(450., 120.), a: Pt::new(550., 120.), b: Pt::new(450., 80.) });
        let arc = doc.add(Shape::Arc { a: Pt::new(350., 400.), m: Pt::new(450., 300.), b: Pt::new(550., 400.) });
        let out = render_annotated(&img, &doc);
        let color = |id| doc.get(id).unwrap().color;
        let at = |x: u32, y: u32| out.get_pixel(x, y).0;
        // Perpendiculaire en x = 250, entre la droite prolongée (y = 100) et le point.
        assert_eq!(&at(250, 75)[..3], &color(pl)[..]);
        // Prolongement en pointillés entre x = 150 et 250.
        assert!((155..245).filter(|&x| at(x, 99) != img.get_pixel(0, 0).0).count() > 20);
        // Seconde droite parallèle en y = 260, qui s'étend jusqu'à x = 150.
        assert_eq!(&at(140, 260)[..3], &color(par)[..]);
        // Contour de l'ellipse au bout du grand axe, intérieur teinté mais pas plein.
        assert_eq!(&at(450, 160)[..3], &color(el)[..]);
        assert_ne!(at(500, 140), img.get_pixel(0, 0).0);
        // Arc : passe par son point intermédiaire ; la corde n'est pas tracée.
        assert_eq!(&at(450, 300)[..3], &color(arc)[..]);
        assert_eq!(at(400, 400), img.get_pixel(0, 0).0);
    }

    #[test]
    fn transparent_pixels_untouched() {
        meteroscope_i18n::set_language("fr");
        let img = RgbaImage::from_pixel(50, 40, Rgba([10, 20, 30, 0]));
        let out = render_annotated(&img, &Document::new());
        assert_eq!(out, img);
    }

    /// Profil sur une image réduite : les traits de bord suivent les bords.
    #[test]
    fn profil_reporte_sur_une_image_reduite() {
        use meteroscope_core::profile::ProfileResult;
        meteroscope_i18n::set_language("fr");
        let img = RgbaImage::from_pixel(600, 400, Rgba([90, 90, 90, 255]));
        let mut doc = Document::new();
        let mut shape = Shape::profile(Pt::new(100., 200.), Pt::new(500., 200.));
        if let Shape::Profile { params, result, .. } = &mut shape {
            params.band = 80.0;
            *result = Some(ProfileResult { edges: [100.0, 300.0], tilt: 0.0 });
        }
        let id = doc.add(shape);
        let color = doc.get(id).unwrap().color;
        // Réduite de moitié : bords en (100, 100) et (200, 100), bande de ± 20.
        let s = crate::rotate::straighten(&img, 0.0, crate::rotate::RotationFilter::Bicubic, 300, u64::MAX);
        assert_eq!(s.image.dimensions(), (300, 200));
        let out = render_annotated_in(&s.image, &doc, &s.frame);
        assert_eq!(&out.get_pixel(100, 85).0[..3], &color[..], "trait du premier bord");
        assert_eq!(&out.get_pixel(200, 115).0[..3], &color[..], "trait du second bord");
        assert_eq!(out.get_pixel(150, 85), s.image.get_pixel(150, 85), "pas de trait hors des bords");
    }
}

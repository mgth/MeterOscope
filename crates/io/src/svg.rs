//! Export SVG : l'image en fond (intégrée en PNG base64, ou liée) et les
//! mesures, étiquettes et annotations en vectoriel.
//!
//! Écrit à la main, sans dépendance. Le dessin reprend celui de l'image
//! annotée (`render.rs`) : mêmes épaisseurs, contours sombres, étiquettes et
//! barre d'échelle, à la même échelle (tailles proportionnelles à l'image).
//! Il se fait dans le repère de la vue redressée (`Document::rotation`,
//! voir [`crate::upright`]) : l'image de fond pivote par une matrice SVG, les
//! étiquettes restent droites, comme à l'écran. Le texte des étiquettes garde
//! la largeur calculée avec nos polices (`textLength`), quelle que soit la
//! police du lecteur : il tient toujours dans sa boîte.

use crate::fonts;
use crate::render::{arrow_head, text_width};
use crate::upright::{Frame, upright_document};
use anyhow::{Context, Result};
use image::RgbaImage;
use meteroscope_core::{Document, Measurement, Pt, Shape};
use std::f32::consts::PI;
use std::fmt::Write as _;

/// Image de fond du SVG.
#[derive(Clone, Debug, PartialEq)]
pub enum SvgBackground {
    /// Intégrée au fichier (PNG en base64) : le SVG se suffit à lui-même.
    Embedded,
    /// Liée : chemin ou URL de l'image, tel quel (relatif au SVG de préférence).
    Linked(String),
    /// Sans image (calque des annotations seul).
    None,
}

const SHADOW: (&str, f32) = ("#000000", 190.0 / 255.0);
const LABEL_BG: f32 = 185.0 / 255.0;

/// Nombre court pour le SVG (2 décimales au plus, sans zéros inutiles).
fn n(v: f32) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".to_owned() } else { s.to_owned() }
}

fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// Échappe le texte pour XML (contenu et attributs).
pub fn xml_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            // Caractères interdits en XML 1.0.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => o.push(c),
        }
    }
    o
}

/// Base64 standard (RFC 4648, avec remplissage).
pub fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let v = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        for i in 0..4 {
            if i <= c.len() {
                o.push(T[(v >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                o.push('=');
            }
        }
    }
    o
}

/// Tracé SVG (attribut `d`).
#[derive(Default)]
struct D(String);

impl D {
    fn m(&mut self, p: (f32, f32)) -> &mut Self {
        let _ = write!(self.0, "M{} {}", n(p.0), n(p.1));
        self
    }
    fn l(&mut self, p: (f32, f32)) -> &mut Self {
        let _ = write!(self.0, "L{} {}", n(p.0), n(p.1));
        self
    }
    fn z(&mut self) -> &mut Self {
        self.0.push('Z');
        self
    }
    fn poly(pts: &[(f32, f32)], closed: bool) -> D {
        let mut d = D::default();
        for (i, p) in pts.iter().enumerate() {
            if i == 0 { d.m(*p) } else { d.l(*p) };
        }
        if closed {
            d.z();
        }
        d
    }
}

fn p32(p: Pt) -> (f32, f32) {
    (p.x as f32, p.y as f32)
}

/// Écrivain SVG : mêmes dimensions de référence que le peintre de l'image annotée.
struct Svg {
    out: String,
    w: f32,
    h: f32,
    line: f32,
    halo: f32,
    text: f32,
}

impl Svg {
    fn new(w: f32, h: f32) -> Svg {
        let u = (w.max(h) / 1000.0).max(0.6);
        Svg {
            out: String::new(),
            w,
            h,
            line: (2.4 * u).max(1.8),
            halo: (1.2 * u).max(1.0),
            text: (17.0 * u).max(13.0),
        }
    }

    fn stroke(&mut self, d: &D, color: &str, opacity: f32, width: f32, extra: &str) {
        let op = if opacity < 1.0 { format!(" stroke-opacity=\"{}\"", n(opacity)) } else { String::new() };
        let _ = writeln!(self.out, "<path d=\"{}\" stroke=\"{color}\"{op} stroke-width=\"{}\"{extra}/>", d.0, n(width));
    }

    fn fill(&mut self, d: &D, color: &str, opacity: f32) {
        let op = if opacity < 1.0 { format!(" fill-opacity=\"{}\"", n(opacity)) } else { String::new() };
        let _ = writeln!(self.out, "<path d=\"{}\" fill=\"{color}\"{op} stroke=\"none\"/>", d.0);
    }

    /// Trait coloré avec contour sombre.
    fn outlined(&mut self, d: &D, color: [u8; 3]) {
        self.stroke(d, SHADOW.0, SHADOW.1, self.line + 2.0 * self.halo, "");
        self.stroke(d, &hex(color), 1.0, self.line, "");
    }

    fn circle(&mut self, c: (f32, f32), r: f32, attrs: &str) {
        let _ = writeln!(self.out, "<circle cx=\"{}\" cy=\"{}\" r=\"{}\" {attrs}/>", n(c.0), n(c.1), n(r));
    }

    fn handle(&mut self, p: (f32, f32), color: [u8; 3]) {
        let a = format!(
            "fill=\"{}\" stroke=\"{}\" stroke-opacity=\"{}\" stroke-width=\"{}\"",
            hex(color),
            SHADOW.0,
            n(SHADOW.1),
            n(self.halo)
        );
        self.circle(p, self.line * 1.3, &a);
    }

    fn segment(&mut self, pa: (f32, f32), pb: (f32, f32), color: [u8; 3]) -> (f32, f32) {
        let (dx, dy) = (pb.0 - pa.0, pb.1 - pa.1);
        let len = dx.hypot(dy).max(1e-6);
        let mut nn = (-dy / len, dx / len);
        if nn.1 > 0.0 || (nn.1 == 0.0 && nn.0 > 0.0) {
            nn = (-nn.0, -nn.1);
        }
        let mut d = D::default();
        d.m(pa).l(pb);
        let t = self.line * 3.0;
        for p in [pa, pb] {
            d.m((p.0 - nn.0 * t, p.1 - nn.1 * t)).l((p.0 + nn.0 * t, p.1 + nn.1 * t));
        }
        self.outlined(&d, color);
        nn
    }

    fn label_size(&self, s: &str, size: f32) -> (f32, f32) {
        let pad = size * 0.35;
        (text_width(s, size) + 2.0 * pad, size * 1.1 + pad)
    }

    /// Étiquette : texte blanc sur fond sombre arrondi, liseré de couleur,
    /// centrée en (cx, cy) et ramenée dans l'image.
    fn label(&mut self, s: &str, cx: f32, cy: f32, size: f32, accent: [u8; 3]) {
        let (bw, bh) = self.label_size(s, size);
        let m = self.halo;
        let x0 = (cx - bw / 2.0).clamp(m, (self.w - bw - m).max(m));
        let y0 = (cy - bh / 2.0).clamp(m, (self.h - bh - m).max(m));
        let _ = writeln!(
            self.out,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" fill=\"#000000\" fill-opacity=\"{}\" stroke=\"{}\" stroke-opacity=\"{}\" stroke-width=\"{}\"/>",
            n(x0),
            n(y0),
            n(bw),
            n(bh),
            n(bh * 0.22),
            n(LABEL_BG),
            hex(accent),
            n(230.0 / 255.0),
            n((self.line * 0.5).max(1.0))
        );
        let tw = text_width(s, size);
        self.text_at(s, x0 + (bw - tw) / 2.0, y0 + bh / 2.0 + size * 0.35, size, tw);
    }

    /// Texte blanc, ligne de base en (x, y), de largeur imposée `tw`.
    fn text_at(&mut self, s: &str, x: f32, y: f32, size: f32, tw: f32) {
        let fit = if tw > 0.0 { format!(" textLength=\"{}\" lengthAdjust=\"spacingAndGlyphs\"", n(tw)) } else { String::new() };
        let _ = writeln!(
            self.out,
            "<text x=\"{}\" y=\"{}\" font-family=\"{}\" font-size=\"{}\" fill=\"#ffffff\" stroke=\"#ffffff\" stroke-width=\"{}\"{fit}>{}</text>",
            n(x),
            n(y),
            xml_escape(&fonts::css_families(s)),
            n(size),
            n(size * 0.035),
            xml_escape(s)
        );
    }

    fn label_beside(&mut self, s: &str, at: (f32, f32), nn: (f32, f32), gap: f32, size: f32, accent: [u8; 3]) {
        let (bw, bh) = self.label_size(s, size);
        let d = nn.0.abs() * bw / 2.0 + nn.1.abs() * bh / 2.0 + gap;
        self.label(s, at.0 + nn.0 * d, at.1 + nn.1 * d, size, accent);
    }

    fn dashed(&mut self, d: &D, color: [u8; 3]) {
        let dash = n(self.line * 2.0);
        let extra = format!(" stroke-dasharray=\"{dash} {dash}\" stroke-linecap=\"round\"");
        self.stroke(d, SHADOW.0, SHADOW.1, self.line * 0.6 + 2.0 * self.halo, &extra);
        self.stroke(d, &hex(color), 1.0, self.line * 0.6, &extra);
    }

    fn arc(&mut self, v: (f32, f32), r: f32, t0: f32, sweep: f32, color: [u8; 3]) {
        let steps = 48;
        let pts: Vec<(f32, f32)> = (0..=steps)
            .map(|i| {
                let t = t0 + sweep * i as f32 / steps as f32;
                (v.0 + r * t.cos(), v.1 + r * t.sin())
            })
            .collect();
        let d = D::poly(&pts, false);
        self.stroke(&d, SHADOW.0, SHADOW.1, self.line * 0.8 + 2.0 * self.halo, "");
        self.stroke(&d, &hex(color), 1.0, self.line * 0.8, "");
    }

    /// Même dessin que `render::Painter::draw`.
    fn draw(&mut self, shape: &Shape, color: [u8; 3], main: Option<String>, marker_no: usize) {
        let gap = self.line * 2.0 + self.halo;
        let size = self.text;
        let short = |mut s: f32| {
            while s > PI {
                s -= 2.0 * PI;
            }
            while s <= -PI {
                s += 2.0 * PI;
            }
            s
        };
        match shape {
            Shape::Length { a, b } => {
                let (pa, pb) = (p32(*a), p32(*b));
                let nn = self.segment(pa, pb, color);
                if let Some(s) = main {
                    let mid = ((pa.0 + pb.0) / 2.0, (pa.1 + pb.1) / 2.0);
                    self.label_beside(&s, mid, nn, gap + self.line * 3.0, size, color);
                }
            }
            Shape::LineAngle { a, b, c, d } => {
                let (pa, pb, pc, pd) = (p32(*a), p32(*b), p32(*c), p32(*d));
                let mut dashes = D::default();
                let (at, nn) = match meteroscope_core::geometry::line_angle_vertex(*a, *b, *c, *d) {
                    Some((x, u1, u2)) => {
                        let v = p32(x);
                        let r = self.text * 2.6;
                        let reach = r + self.text * 0.6;
                        for (p0, p1, u) in [(pa, pb, u1), (pc, pd, u2)] {
                            let tip = (v.0 + u.x as f32 * reach, v.1 + u.y as f32 * reach);
                            for (e0, e1) in extensions(p0, p1, &[v, tip]) {
                                dashes.m(e0).l(e1);
                            }
                        }
                        let (t1, t2) = ((u1.y as f32).atan2(u1.x as f32), (u2.y as f32).atan2(u2.x as f32));
                        let sweep = short(t2 - t1);
                        self.arc(v, r, t1, sweep, color);
                        let tm = t1 + sweep / 2.0;
                        ((v.0 + tm.cos() * r, v.1 + tm.sin() * r), (tm.cos(), tm.sin()))
                    }
                    None => {
                        let m1 = ((pa.0 + pb.0) / 2.0, (pa.1 + pb.1) / 2.0);
                        let m2 = ((pc.0 + pd.0) / 2.0, (pc.1 + pd.1) / 2.0);
                        dashes.m(m1).l(m2);
                        (((m1.0 + m2.0) / 2.0, (m1.1 + m2.1) / 2.0), (0.0, 0.0))
                    }
                };
                if !dashes.0.is_empty() {
                    self.dashed(&dashes, color);
                }
                self.segment(pa, pb, color);
                self.segment(pc, pd, color);
                if let Some(s) = main {
                    if nn == (0.0, 0.0) {
                        self.label(&s, at.0, at.1, size, color);
                    } else {
                        self.label_beside(&s, at, nn, gap, size, color);
                    }
                }
            }
            Shape::Path { pts } => {
                let v: Vec<(f32, f32)> = pts.iter().map(|p| p32(*p)).collect();
                self.outlined(&D::poly(&v, false), color);
                for p in &v {
                    self.handle(*p, color);
                }
                if let (Some(s), Some(last)) = (main, v.last()) {
                    let prev = v[v.len().saturating_sub(2)];
                    let (dx, dy) = (last.0 - prev.0, last.1 - prev.1);
                    let l = dx.hypot(dy);
                    let nn = if l > 1e-6 { (dx / l, dy / l) } else { (0.0, -1.0) };
                    self.label_beside(&s, *last, nn, gap, size, color);
                }
            }
            Shape::Angle { a, vertex, b } => {
                let (pa, pv, pb) = (p32(*a), p32(*vertex), p32(*b));
                let la = (pa.0 - pv.0).hypot(pa.1 - pv.1);
                let lb = (pb.0 - pv.0).hypot(pb.1 - pv.1);
                let ta = (pa.1 - pv.1).atan2(pa.0 - pv.0);
                let tb = (pb.1 - pv.1).atan2(pb.0 - pv.0);
                let sweep = short(tb - ta);
                let r = (0.35 * la.min(lb)).min(self.text * 3.0).max(self.line * 4.0);
                self.outlined(&D::poly(&[pa, pv, pb], false), color);
                self.arc(pv, r, ta, sweep, color);
                for p in [pa, pv, pb] {
                    self.handle(p, color);
                }
                if let Some(s) = main {
                    let tm = ta + sweep / 2.0;
                    let nn = (tm.cos(), tm.sin());
                    self.label_beside(&s, (pv.0 + nn.0 * r, pv.1 + nn.1 * r), nn, gap, size, color);
                }
            }
            Shape::Area { pts } => {
                let v: Vec<(f32, f32)> = pts.iter().map(|p| p32(*p)).collect();
                let d = D::poly(&v, true);
                self.fill(&d, &hex(color), 40.0 / 255.0);
                self.outlined(&d, color);
                for p in &v {
                    self.handle(*p, color);
                }
                if let Some(s) = main {
                    let k = pts.len().max(1) as f64;
                    let c = pts.iter().fold(Pt::default(), |acc, p| acc.offset(p.x / k, p.y / k));
                    self.label(&s, c.x as f32, c.y as f32, size, color);
                }
            }
            Shape::Rect { a, b, angle } => {
                let corners: Vec<(f32, f32)> =
                    meteroscope_core::geometry::rect_corners(*a, *b, *angle).iter().map(|p| p32(*p)).collect();
                let d = D::poly(&corners, true);
                self.fill(&d, &hex(color), 40.0 / 255.0);
                self.outlined(&d, color);
                for p in [*a, *b] {
                    self.handle(p32(p), color);
                }
                if let Some(s) = main {
                    self.label(&s, ((a.x + b.x) / 2.0) as f32, ((a.y + b.y) / 2.0) as f32, size, color);
                }
            }
            Shape::Circle { .. } | Shape::Circle3 { .. } => {
                if let Some((c, r)) = shape.circle() {
                    let (cx, cy) = p32(c);
                    let col = hex(color);
                    let (sh, sw) = (SHADOW.1, self.line + 2.0 * self.halo);
                    self.circle((cx, cy), r as f32, &format!("fill=\"{col}\" fill-opacity=\"{}\" stroke=\"none\"", n(35.0 / 255.0)));
                    self.circle((cx, cy), r as f32, &format!("fill=\"none\" stroke=\"#000000\" stroke-opacity=\"{}\" stroke-width=\"{}\"", n(sh), n(sw)));
                    self.circle((cx, cy), r as f32, &format!("fill=\"none\" stroke=\"{col}\" stroke-width=\"{}\"", n(self.line)));
                    let t = self.line * 3.0;
                    let mut d = D::default();
                    d.m((cx - t, cy)).l((cx + t, cy)).m((cx, cy - t)).l((cx, cy + t));
                    self.outlined(&d, color);
                    if let Some(s) = main {
                        self.label_beside(&s, (cx, cy), (0.0, -1.0), gap + t, size, color);
                    }
                }
                for p in shape.points() {
                    if !matches!(shape, Shape::Circle { center, .. } if *center == p) {
                        self.handle(p32(p), color);
                    }
                }
            }
            Shape::Marker { p } => {
                let (x, y) = p32(*p);
                let r = self.line * 2.2;
                let a = format!(
                    "fill=\"{}\" stroke=\"#000000\" stroke-opacity=\"{}\" stroke-width=\"{}\"",
                    hex(color),
                    n(SHADOW.1),
                    n(self.halo * 1.5)
                );
                self.circle((x, y), r, &a);
                let d = std::f32::consts::FRAC_1_SQRT_2;
                self.label_beside(&marker_no.to_string(), (x, y), (d, -d), r + self.halo, size * 0.85, color);
            }
            Shape::Text { p, text } => {
                let (x, y) = p32(*p);
                if text.is_empty() {
                    self.handle((x, y), color);
                } else {
                    self.label(text, x, y, size, color);
                }
            }
            Shape::Arrow { a, b, text } => {
                let (pa, pb) = (p32(*a), p32(*b));
                if let Some((head, tri)) = arrow_head(pa, pb, self.line) {
                    let (dx, dy) = (pb.0 - pa.0, pb.1 - pa.1);
                    let len = dx.hypot(dy);
                    let mut shaft = D::default();
                    shaft.m(pa).l((pb.0 - dx / len * head * 0.5, pb.1 - dy / len * head * 0.5));
                    self.outlined(&shaft, color);
                    let tip = D::poly(&tri, true);
                    self.stroke(&tip, SHADOW.0, SHADOW.1, 2.0 * self.halo, "");
                    self.fill(&tip, &hex(color), 1.0);
                }
                if !text.is_empty() {
                    let (dx, dy) = (pa.0 - pb.0, pa.1 - pb.1);
                    let l = dx.hypot(dy);
                    let nn = if l > 1e-6 { (dx / l, dy / l) } else { (0.0, -1.0) };
                    self.label_beside(text, pa, nn, gap, size, color);
                }
            }
        }
    }

    /// Barre d'échelle en bas à droite (comme l'image annotée).
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
        let tw = text_width(&text, size);
        let bw = len.max(tw) + 2.0 * pad;
        let bh = size * 1.1 + bar_h + 2.5 * pad;
        let margin = self.text;
        let x0 = (self.w - bw - margin).max(0.0);
        let y0 = (self.h - bh - margin).max(0.0);
        let _ = writeln!(self.out, "<g id=\"echelle\">");
        let _ = writeln!(
            self.out,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" fill=\"#000000\" fill-opacity=\"{}\"/>",
            n(x0),
            n(y0),
            n(bw),
            n(bh),
            n(pad * 0.6),
            n(LABEL_BG)
        );
        let bx = x0 + (bw - len) / 2.0;
        let by = y0 + bh - pad - bar_h;
        let mut rects = vec![(bx, by, len, bar_h)];
        for x in [bx, bx + len - self.halo] {
            rects.push((x, by - bar_h, self.halo.max(1.0), bar_h * 2.0));
        }
        for (x, y, w, h) in rects {
            let _ = writeln!(self.out, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#ffffff\"/>", n(x), n(y), n(w), n(h));
        }
        self.text_at(&text, x0 + (bw - tw) / 2.0, by - pad * 0.9, size, tw);
        let _ = writeln!(self.out, "</g>");
    }
}

/// Voir `render::extensions`.
fn extensions(p0: (f32, f32), p1: (f32, f32), reach: &[(f32, f32)]) -> Vec<((f32, f32), (f32, f32))> {
    let d = (p1.0 - p0.0, p1.1 - p0.1);
    let l2 = d.0 * d.0 + d.1 * d.1;
    if l2 == 0.0 {
        return vec![];
    }
    let at = |t: f32| (p0.0 + d.0 * t, p0.1 + d.1 * t);
    let (lo, hi) = reach
        .iter()
        .map(|q| ((q.0 - p0.0) * d.0 + (q.1 - p0.1) * d.1) / l2)
        .fold((0.0f32, 1.0f32), |(lo, hi), t| (lo.min(t), hi.max(t)));
    let mut v = vec![];
    if lo < 0.0 {
        v.push((at(lo), p0));
    }
    if hi > 1.0 {
        v.push((p1, at(hi)));
    }
    v
}

/// Identifiant XML d'une mesure (classe = type, pour un tri dans un éditeur).
fn group_open(m: &Measurement) -> String {
    let kind = format!("{:?}", m.shape.kind()).to_lowercase();
    let group = if m.group.is_empty() { String::new() } else { format!(" data-groupe=\"{}\"", xml_escape(&m.group)) };
    format!("<g id=\"m{}\" class=\"{kind}\"{group}>\n<title>{}</title>\n", m.id, xml_escape(&m.name))
}

fn png_bytes(image: &RgbaImage) -> Result<Vec<u8>> {
    use image::ImageEncoder;
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(image.as_raw(), image.width(), image.height(), image::ExtendedColorType::Rgba8)
        .context("PNG")?;
    Ok(out)
}

/// SVG de l'image annotée. `image` : l'image telle qu'affichée (réglages de
/// luminosité/contraste compris), non pivotée.
pub fn svg_string(image: &RgbaImage, doc: &Document, background: &SvgBackground) -> Result<String> {
    let (w, h) = (image.width() as f64, image.height() as f64);
    let frame = Frame::new(w, h, doc.rotation);
    let (ow, oh) = crate::rotate::rotated_dimensions(image.width(), image.height(), doc.rotation);
    let up = upright_document(doc, w, h);
    let mut s = Svg::new(ow as f32, oh as f32);

    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" version=\"1.1\" width=\"{ow}\" height=\"{oh}\" viewBox=\"0 0 {ow} {oh}\">"
    );
    out.push_str("<title>MeterOscope</title>\n");
    let href = match background {
        SvgBackground::Embedded => Some(format!("data:image/png;base64,{}", base64(&png_bytes(image)?))),
        SvgBackground::Linked(p) => Some(xml_escape(p)),
        SvgBackground::None => None,
    };
    if let Some(href) = href {
        let mtx = if doc.rotation == 0.0 {
            String::new()
        } else {
            let m = frame.svg_matrix();
            format!(" transform=\"matrix({} {} {} {} {} {})\"", m[0], m[1], m[2], m[3], m[4], m[5])
        };
        let _ = writeln!(
            out,
            "<image id=\"image\" x=\"0\" y=\"0\" width=\"{w}\" height=\"{h}\" preserveAspectRatio=\"none\"{mtx} xlink:href=\"{href}\" href=\"{href}\"/>"
        );
    }
    out.push_str("<g id=\"mesures\" fill=\"none\" stroke-linecap=\"round\" stroke-linejoin=\"round\">\n");
    let mut marker_no = 0;
    for m in &up.measurements {
        if matches!(m.shape, Shape::Marker { .. }) {
            marker_no += 1;
        }
        if !m.visible {
            continue;
        }
        let main = m.shape.values(&up.calibration).first().map(|v| v.text());
        s.out.push_str(&group_open(m));
        s.draw(&m.shape, m.color, main, marker_no);
        s.out.push_str("</g>\n");
    }
    out.push_str(&std::mem::take(&mut s.out));
    out.push_str("</g>\n");
    if let Some(sc) = &up.calibration.scale {
        s.scale_bar(sc.units_per_px, &sc.unit);
        out.push_str(&s.out);
    }
    out.push_str("</svg>\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_rfc4648() {
        meteroscope_i18n::set_language("fr");
        for (i, o) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(i.as_bytes()), o);
        }
        assert_eq!(xml_escape("a<b & \"c\"\u{1}"), "a&lt;b &amp; &quot;c&quot;");
        assert_eq!(n(1.0), "1");
        assert_eq!(n(-0.001), "0");
        assert_eq!((n(2.25), n(3.1)), ("2.25".to_owned(), "3.1".to_owned()));
    }
}

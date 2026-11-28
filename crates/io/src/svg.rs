//! Export SVG : l'image en fond (intégrée en PNG base64, ou liée) et les
//! mesures, étiquettes et annotations en vectoriel.
//!
//! Écrit à la main, sans dépendance. Le dessin est celui de l'image annotée
//! (voir [`crate::draw`]) : mêmes traits, contours sombres, étiquettes et
//! barre d'échelle, à la même échelle (tailles proportionnelles à l'image).
//! Il se fait dans le repère de la vue redressée (`Document::rotation`,
//! voir [`crate::rotate::Frame`]) : l'image de fond pivote par une matrice SVG, les
//! étiquettes restent droites, comme à l'écran. Le texte des étiquettes garde
//! la largeur calculée avec nos polices (`textLength`), quelle que soit la
//! police du lecteur : il tient toujours dans sa boîte.

use crate::draw::{Geom, Metrics, Pen, Rgba, Surface, draw_measurements, draw_scale_bar};
use crate::fonts;
use crate::rotate::Frame;
use anyhow::{Context, Result};
use image::RgbaImage;
use meteroscope_core::{Document, Measurement};
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

/// Nombre court pour le SVG (2 décimales au plus, sans zéros inutiles).
fn n(v: f32) -> String {
    let s = format!("{v:.2}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".to_owned() } else { s.to_owned() }
}

/// Attributs `nom="#rrggbb"` et, si la couleur est translucide, `nom-opacity`.
fn color_attrs(name: &str, c: Rgba) -> String {
    let op = if c[3] < 255 { format!(" {name}-opacity=\"{}\"", n(c[3] as f32 / 255.0)) } else { String::new() };
    format!(" {name}=\"#{:02x}{:02x}{:02x}\"{op}", c[0], c[1], c[2])
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

/// Écrivain SVG : mêmes dimensions de référence que l'image annotée.
struct Svg {
    out: String,
    m: Metrics,
}

impl Surface for Svg {
    fn metrics(&self) -> Metrics {
        self.m
    }

    fn paint(&mut self, g: &Geom, fill: Option<Rgba>, pen: Option<Pen>) {
        let mut attrs = fill.map_or_else(|| " fill=\"none\"".to_owned(), |c| color_attrs("fill", c));
        if let Some(pen) = pen {
            let _ = write!(attrs, "{} stroke-width=\"{}\"", color_attrs("stroke", pen.color), n(pen.width));
            if let Some(d) = pen.dash {
                let _ = write!(attrs, " stroke-dasharray=\"{0} {0}\"", n(d));
            }
        }
        match *g {
            Geom::Lines(lines) => {
                let mut d = String::new();
                for l in lines {
                    for (i, p) in l.pts.iter().enumerate() {
                        let _ = write!(d, "{}{} {}", if i == 0 { 'M' } else { 'L' }, n(p.0), n(p.1));
                    }
                    if l.closed {
                        d.push('Z');
                    }
                }
                if !d.is_empty() {
                    let _ = writeln!(self.out, "<path d=\"{d}\"{attrs}/>");
                }
            }
            Geom::Circle(c, r) => {
                let _ = writeln!(self.out, "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"{attrs}/>", n(c.0), n(c.1), n(r));
            }
            Geom::Rect { x, y, w, h, r } => {
                let rx = if r > 0.0 { format!(" rx=\"{}\"", n(r)) } else { String::new() };
                let _ = writeln!(self.out, "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{rx}{attrs}/>", n(x), n(y), n(w), n(h));
            }
        }
    }

    /// Le texte garde la largeur calculée avec nos polices (`textLength`).
    fn text(&mut self, s: &str, x: f32, y: f32, size: f32, width: f32) {
        let fit = if width > 0.0 { format!(" textLength=\"{}\" lengthAdjust=\"spacingAndGlyphs\"", n(width)) } else { String::new() };
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

    fn open(&mut self, m: &Measurement) {
        self.out.push_str(&group_open(m));
    }

    fn close(&mut self) {
        self.out.push_str("</g>\n");
    }
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
    let (w, h) = image.dimensions();
    let frame = Frame::rotation(w, h, doc.rotation);
    let (ow, oh) = crate::rotate::rotated_dimensions(w, h, doc.rotation);
    let mut s = Svg { out: String::new(), m: Metrics::new(ow as f32, oh as f32) };

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
    draw_measurements(&mut s, doc, &frame);
    out.push_str(&std::mem::take(&mut s.out));
    out.push_str("</g>\n");
    // Vue redressée : la barre reste horizontale, comme à l'écran.
    draw_scale_bar(&mut s, doc, &frame);
    if !s.out.is_empty() {
        let _ = write!(out, "<g id=\"echelle\">\n{}</g>\n", s.out);
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

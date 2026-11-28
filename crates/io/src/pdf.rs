//! Rapport PDF : pages A4 avec titre, date, nom de l'image, image annotée
//! ajustée à la page, échelle, tableau des mesures puis des statistiques.
//!
//! Écrit avec `krilla` (MIT/Apache-2.0, pur Rust, maintenu par l'équipe de
//! Typst) : polices intégrées en sous-ensemble, images compressées. Police :
//! Ubuntu Light embarquée pour les écritures latines ; les caractères
//! qu'elle n'a pas (japonais, coréen, chinois…) sont écrits avec une police
//! du système qui les a, intégrée elle aussi au PDF (voir [`crate::fonts`]).
//! Sans telle police installée, ils restent des boîtes vides.
//!
//! L'image est l'image annotée (mêmes traits et étiquettes que l'export PNG),
//! redressée selon `Document::rotation`, réduite à 2400 pixels au plus.

use crate::fonts;
use crate::render::text_width;
use crate::rotate::{RotationFilter, rotate_image};
use crate::table::format_precise;
use crate::upright::upright_document;
use anyhow::{Result, anyhow};
use image::RgbaImage;
use krilla::color::rgb;
use krilla::geom::{PathBuilder, Point, Rect, Size, Transform};
use krilla::num::NormalizedF32;
use krilla::paint::{Fill, Stroke};
use krilla::page::PageSettings;
use krilla::text::{Font, TextDirection};
use meteroscope_core::{Document, Measurement, Shape, Stats};
use meteroscope_i18n::tr;
use std::collections::HashMap;

/// A4 en points.
const PAGE_W: f32 = 595.28;
const PAGE_H: f32 = 841.89;
const MARGIN: f32 = 42.0;
const BODY: f32 = 9.0;
const ROW_H: f32 = 13.0;
/// Grand côté maximal de l'image intégrée (≈ 300 dpi sur la largeur de la page).
const MAX_IMAGE_PX: u32 = 2400;

type Rgb = [u8; 3];
const INK: Rgb = [20, 20, 20];
const GREY: Rgb = [110, 110, 110];
const RULE: Rgb = [200, 200, 200];
const HEAD_BG: Rgb = [235, 237, 240];

/// Informations de l'en-tête du rapport.
#[derive(Clone, Debug)]
pub struct ReportInfo {
    pub title: String,
    /// Nom du fichier image (vide : image sans fichier, caméra ou presse-papiers).
    pub image_name: String,
    /// Date affichée, p. ex. « 2026-11-28 ».
    pub date: String,
}

/// Date du jour (UTC) au format ISO 8601, sans dépendance.
pub fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Jour civil (grégorien) d'un nombre de jours depuis le 1970-01-01 (H. Hinnant).
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Opération de dessin d'une page (coordonnées en points, y vers le bas).
enum Op {
    /// Texte, ligne de base en (x, y).
    Text { x: f32, y: f32, size: f32, s: String, color: Rgb },
    Rect { x: f32, y: f32, w: f32, h: f32, color: Rgb },
    Line { x0: f32, y0: f32, x1: f32, y1: f32, color: Rgb },
    Image { x: f32, y: f32, w: f32, h: f32 },
}

/// Mise en page : les pages et la position courante.
struct Layout {
    pages: Vec<Vec<Op>>,
    y: f32,
}

impl Layout {
    fn new() -> Layout {
        Layout { pages: vec![vec![]], y: MARGIN }
    }

    fn ops(&mut self) -> &mut Vec<Op> {
        self.pages.last_mut().expect("au moins une page")
    }

    fn new_page(&mut self) {
        self.pages.push(vec![]);
        self.y = MARGIN;
    }

    /// Passe à la page suivante si `h` points ne tiennent plus. Renvoie vrai si nouvelle page.
    fn ensure(&mut self, h: f32) -> bool {
        if self.y + h > PAGE_H - MARGIN - 14.0 {
            self.new_page();
            return true;
        }
        false
    }

    fn text(&mut self, x: f32, y: f32, size: f32, s: impl Into<String>, color: Rgb) {
        self.ops().push(Op::Text { x, y, size, s: s.into(), color });
    }
}

/// Coupe `s` en lignes de largeur `w` au plus (aux espaces ; un mot trop long est coupé).
fn wrap(s: &str, size: f32, w: f32) -> Vec<String> {
    let mut lines = vec![];
    let mut cur = String::new();
    for word in s.split(' ') {
        let cand = if cur.is_empty() { word.to_owned() } else { format!("{cur} {word}") };
        if text_width(&cand, size) <= w || cur.is_empty() && text_width(word, size) <= w {
            cur = cand;
            continue;
        }
        if !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
        }
        // Mot plus long que la colonne (ou écriture sans espaces) : coupé au caractère.
        for ch in word.chars() {
            let mut c2 = cur.clone();
            c2.push(ch);
            if text_width(&c2, size) > w && !cur.is_empty() {
                lines.push(std::mem::replace(&mut cur, ch.to_string()));
            } else {
                cur = c2;
            }
        }
    }
    if !cur.is_empty() || lines.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Tableau : en-tête, puis lignes (cellules coupées en plusieurs lignes au
/// besoin) ; l'en-tête est répété en haut de chaque nouvelle page.
fn table(l: &mut Layout, widths: &[f32], header: &[String], rows: &[Vec<String>], right: &[bool]) {
    let pad = 3.0;
    let draw_header = |l: &mut Layout| {
        let y = l.y;
        l.ops().push(Op::Rect { x: MARGIN, y, w: widths.iter().sum(), h: ROW_H + 2.0, color: HEAD_BG });
        let mut x = MARGIN;
        for (i, (h, w)) in header.iter().zip(widths).enumerate() {
            let s = wrap(h, BODY, w - 2.0 * pad).remove(0);
            let tx = if right[i] { x + w - pad - text_width(&s, BODY) } else { x + pad };
            l.text(tx, y + ROW_H - 2.5, BODY, s, INK);
            x += w;
        }
        l.y += ROW_H + 2.0;
    };
    l.ensure(2.0 * ROW_H + 2.0);
    draw_header(l);
    for row in rows {
        let cells: Vec<Vec<String>> = row.iter().zip(widths).map(|(c, w)| wrap(c, BODY, w - 2.0 * pad)).collect();
        let n = cells.iter().map(Vec::len).max().unwrap_or(1).max(1);
        let h = n as f32 * ROW_H;
        if l.ensure(h) {
            draw_header(l);
        }
        let mut x = MARGIN;
        for (i, (lines, w)) in cells.iter().zip(widths).enumerate() {
            for (k, s) in lines.iter().enumerate() {
                let tx = if right[i] { x + w - pad - text_width(s, BODY) } else { x + pad };
                l.text(tx, l.y + (k as f32 + 1.0) * ROW_H - 3.5, BODY, s.clone(), INK);
            }
            x += w;
        }
        l.y += h;
        let y = l.y;
        l.ops().push(Op::Line { x0: MARGIN, y0: y, x1: MARGIN + widths.iter().sum::<f32>(), y1: y, color: RULE });
    }
}

fn heading(l: &mut Layout, s: &str) {
    l.ensure(40.0);
    l.y += 14.0;
    l.text(MARGIN, l.y + 12.0, 13.0, s, INK);
    l.y += 20.0;
}

/// Valeurs d'une mesure en une ligne (« Aire 2 mm² · Largeur 2 mm … »).
fn values_text(doc: &Document, m: &Measurement) -> String {
    match &m.shape {
        Shape::Marker { p } => format!("x {} · y {} px", format_precise(p.x), format_precise(p.y)),
        s => s
            .values(&doc.calibration)
            .iter()
            .map(|v| format!("{} {} {}", v.quantity.label(), format_precise(v.value), v.unit))
            .collect::<Vec<_>>()
            .join(" · "),
    }
}

fn stats_rows(summary: &[(meteroscope_core::ShapeKind, meteroscope_core::Quantity, String, Stats)]) -> Vec<Vec<String>> {
    summary
        .iter()
        .map(|(k, q, u, s)| {
            let sd = if s.n < 2 { "—".to_owned() } else { format_precise(s.sd) };
            vec![
                k.label().to_owned(),
                q.label().to_owned(),
                u.clone(),
                s.n.to_string(),
                format_precise(s.mean),
                sd,
                format_precise(s.min),
                format_precise(s.max),
            ]
        })
        .collect()
}

/// Image annotée redressée, réduite, et sa taille.
fn annotated_image(image: &RgbaImage, doc: &Document, filter: RotationFilter) -> RgbaImage {
    let (w, h) = (image.width() as f64, image.height() as f64);
    let mut up = upright_document(doc, w, h);
    let mut img = if doc.rotation == 0.0 { image.clone() } else { rotate_image(image, doc.rotation, filter) };
    let side = img.width().max(img.height());
    if side > MAX_IMAGE_PX {
        let k = MAX_IMAGE_PX as f64 / side as f64;
        let nw = ((img.width() as f64 * k).round() as u32).max(1);
        let nh = ((img.height() as f64 * k).round() as u32).max(1);
        // Échelle effective (arrondi de la largeur), la même sur les deux axes.
        let k = nw as f64 / img.width() as f64;
        img = image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Triangle);
        for m in &mut up.measurements {
            m.shape.map_points(|p| meteroscope_core::Pt::new(p.x * k, p.y * k));
        }
        // Mêmes valeurs mesurées : un pixel réduit vaut 1/k pixel d'origine.
        if let Some(s) = &mut up.calibration.scale {
            s.units_per_px /= k;
        }
    }
    crate::render::render_annotated(&img, &up)
}

/// Rapport PDF de l'image mesurée. `image` : l'image telle qu'affichée
/// (réglages compris), non pivotée ; `filter` : filtre de redressement.
pub fn pdf_report(image: &RgbaImage, doc: &Document, filter: RotationFilter, info: &ReportInfo) -> Result<Vec<u8>> {
    let mut l = Layout::new();
    let content_w = PAGE_W - 2.0 * MARGIN;

    // En-tête.
    l.text(MARGIN, l.y + 18.0, 20.0, info.title.clone(), INK);
    l.y += 30.0;
    let mut meta = vec![format!("{} : {}", tr!("report.date"), info.date)];
    if !info.image_name.is_empty() {
        meta.push(format!("{} : {}", tr!("report.image"), info.image_name));
    }
    meta.push(format!("{} × {} px", image.width(), image.height()));
    for line in wrap(&meta.join("   ·   "), 10.0, content_w) {
        l.text(MARGIN, l.y + 10.0, 10.0, line, GREY);
        l.y += 14.0;
    }
    let scale = match &doc.calibration.scale {
        Some(s) => tr!("report.scale_value", value = format_precise(s.units_per_px), unit = s.unit),
        None => tr!("table.uncalibrated").to_owned(),
    };
    l.text(MARGIN, l.y + 10.0, 10.0, format!("{} : {scale}", tr!("panel.scale")), GREY);
    l.y += 14.0;
    if doc.rotation != 0.0 {
        let r = tr!("report.rotation", angle = meteroscope_core::measure::format_number(doc.rotation));
        l.text(MARGIN, l.y + 10.0, 10.0, r, GREY);
        l.y += 14.0;
    }
    l.y += 8.0;

    // Image annotée, ajustée à la largeur et à ce qui reste de la page.
    let annotated = annotated_image(image, doc, filter);
    let (iw, ih) = (annotated.width() as f32, annotated.height() as f32);
    let max_h = PAGE_H - MARGIN - 14.0 - l.y;
    let k = (content_w / iw).min(max_h / ih);
    let (w, h) = (iw * k, ih * k);
    let x = MARGIN + (content_w - w) / 2.0;
    let y = l.y;
    l.ops().push(Op::Rect { x: x - 0.5, y: y - 0.5, w: w + 1.0, h: h + 1.0, color: RULE });
    l.ops().push(Op::Image { x, y, w, h });
    l.y += h + 6.0;

    // Mesures (les annotations n'en sont pas).
    heading(&mut l, tr!("report.measurements"));
    let rows: Vec<Vec<String>> = doc
        .measurements
        .iter()
        .filter(|m| !m.shape.kind().is_annotation())
        .map(|m| vec![m.name.clone(), m.shape.kind().label().to_owned(), m.group.clone(), values_text(doc, m)])
        .collect();
    if rows.is_empty() {
        l.text(MARGIN, l.y + 10.0, BODY, tr!("report.no_measurement"), GREY);
        l.y += ROW_H;
    } else {
        let header = [tr!("table.name"), tr!("item.type"), tr!("table.group"), tr!("report.values")].map(str::to_owned);
        table(&mut l, &[130.0, 85.0, 80.0, content_w - 295.0], &header, &rows, &[false; 4]);
    }

    // Statistiques, par groupe s'il y en a.
    let header = [
        tr!("item.type"),
        tr!("table.quantity"),
        tr!("calib.unit"),
        "n",
        tr!("stats.mean"),
        tr!("stats.std_dev"),
        tr!("stats.min"),
        tr!("stats.max"),
    ]
    .map(str::to_owned);
    let widths = [90.0, 75.0, 40.0, 30.0, 70.0, 70.0, 68.0, content_w - 443.0];
    let right = [false, false, false, true, true, true, true, true];
    let sections: Vec<(String, Vec<_>)> =
        if doc.groups().is_empty() { vec![(String::new(), doc.summary())] } else { doc.summary_by_group() };
    heading(&mut l, tr!("stats.title"));
    let mut any = false;
    for (g, summary) in sections {
        if summary.is_empty() {
            continue;
        }
        any = true;
        if !doc.groups().is_empty() {
            l.ensure(3.0 * ROW_H);
            let name = if g.is_empty() { tr!("group.ungrouped") } else { g.as_str() };
            l.text(MARGIN, l.y + 12.0, 10.5, name, INK);
            l.y += 17.0;
        }
        table(&mut l, &widths, &header, &stats_rows(&summary), &right);
        l.y += 6.0;
    }
    if !any {
        l.text(MARGIN, l.y + 10.0, BODY, tr!("stats.empty"), GREY);
        l.y += ROW_H;
    }
    let markers = doc.marker_count();
    if markers > 0 {
        l.ensure(ROW_H + 4.0);
        l.text(MARGIN, l.y + 12.0, BODY, format!("{} : {markers}", tr!("table.marker_count")), INK);
        l.y += ROW_H + 4.0;
    }

    render(&l, &annotated, info)
}

/// Écrit les pages avec krilla.
fn render(l: &Layout, annotated: &RgbaImage, info: &ReportInfo) -> Result<Vec<u8>> {
    let mut document = krilla::Document::new();
    document.set_metadata(
        krilla::metadata::Metadata::new()
            .title(info.title.clone())
            .creator("MeterOscope".to_owned())
            .language(meteroscope_i18n::language().tag.to_owned()),
    );
    let image = krilla::image::Image::from_rgba8(annotated.as_raw().clone(), annotated.width(), annotated.height());
    let mut kfonts: HashMap<usize, Font> = HashMap::new();
    let total = l.pages.len();
    for (no, ops) in l.pages.iter().enumerate() {
        let settings = PageSettings::from_wh(PAGE_W, PAGE_H).ok_or_else(|| anyhow!("page"))?;
        let mut page = document.start_page_with(settings);
        let mut surface = page.surface();
        let fill = |c: Rgb| Fill { paint: rgb::Color::new(c[0], c[1], c[2]).into(), opacity: NormalizedF32::ONE, rule: Default::default() };
        let footer = Op::Text {
            x: 0.0,
            y: PAGE_H - MARGIN / 2.0,
            size: 8.0,
            s: format!("{} — {}", info.title, tr!("report.page", n = no + 1, total = total)),
            color: GREY,
        };
        for op in ops.iter().chain(std::iter::once(&footer)) {
            match op {
                Op::Text { x, y, size, s, color } => {
                    // Pied de page centré.
                    let x = if std::ptr::eq(op, &footer) { (PAGE_W - text_width(s, *size)) / 2.0 } else { *x };
                    surface.set_stroke(None);
                    surface.set_fill(Some(fill(*color)));
                    let mut cx = x;
                    for (i, run) in fonts::runs(s) {
                        let font = match kfonts.get(&i) {
                            Some(f) => f.clone(),
                            None => {
                                let lf = fonts::get(i);
                                let f = Font::new(lf.data.into(), lf.index).ok_or_else(|| anyhow!("police {}", lf.family))?;
                                kfonts.insert(i, f.clone());
                                f
                            }
                        };
                        surface.draw_text(Point::from_xy(cx, *y), font, *size, &run, false, TextDirection::Auto);
                        cx += text_width(&run, *size);
                    }
                }
                Op::Rect { x, y, w, h, color } => {
                    let mut pb = PathBuilder::new();
                    if let Some(r) = Rect::from_xywh(*x, *y, *w, *h) {
                        pb.push_rect(r);
                    }
                    if let Some(p) = pb.finish() {
                        surface.set_stroke(None);
                        surface.set_fill(Some(fill(*color)));
                        surface.draw_path(&p);
                    }
                }
                Op::Line { x0, y0, x1, y1, color } => {
                    let mut pb = PathBuilder::new();
                    pb.move_to(*x0, *y0);
                    pb.line_to(*x1, *y1);
                    if let Some(p) = pb.finish() {
                        surface.set_fill(None);
                        surface.set_stroke(Some(Stroke {
                            paint: rgb::Color::new(color[0], color[1], color[2]).into(),
                            width: 0.5,
                            ..Default::default()
                        }));
                        surface.draw_path(&p);
                    }
                }
                Op::Image { x, y, w, h } => {
                    surface.push_transform(&Transform::from_translate(*x, *y));
                    if let Some(size) = Size::from_wh(*w, *h) {
                        surface.draw_image(image.clone(), size);
                    }
                    surface.pop();
                }
            }
        }
        surface.finish();
        page.finish();
    }
    document.finish().map_err(|e| anyhow!("{e:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_et_coupures() {
        meteroscope_i18n::set_language("fr");
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_785), (2026, 11, 28));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(today().len(), 10);
        let lines = wrap("Longueur 12,5 mm · Aire 3 mm² · Périmètre 18 mm", BODY, 80.0);
        assert!(lines.len() > 1 && lines.iter().all(|s| text_width(s, BODY) <= 80.0), "{lines:?}");
        assert_eq!(wrap("", BODY, 50.0), vec![String::new()]);
    }
}

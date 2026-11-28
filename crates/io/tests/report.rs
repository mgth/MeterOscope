//! Exports SVG et PDF. Avec `METEROSCOPE_REPORT_DEMO=<dossier>`, les fichiers
//! de démonstration y sont écrits pour être ouverts à la main.

use image::{Rgba, RgbaImage};
use meteroscope_core::{Document, Pt, Shape};
use meteroscope_io::*;

fn photo(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let v = (60.0 + 90.0 * (x as f64 / w as f64) + 50.0 * (y as f64 / h as f64)) as u8;
        // Une « pièce » plus claire au milieu, pour voir l'image sous les mesures.
        let inside = (x as i64 - w as i64 / 2).pow(2) + (y as i64 - h as i64 / 2).pow(2) < (h as i64 / 4).pow(2);
        let v = if inside { v.saturating_add(60) } else { v };
        Rgba([v, v.saturating_add(10), v.saturating_add(25), 255])
    })
}

fn sample() -> Document {
    let mut d = Document::new();
    let a = d.add(Shape::Length { a: Pt::new(40., 60.), b: Pt::new(260., 110.) });
    let r = d.add(Shape::Rect { a: Pt::new(300., 150.), b: Pt::new(450., 240.), angle: 0.0 });
    d.add(Shape::Circle { center: Pt::new(150., 220.), edge: Pt::new(200., 220.) });
    d.add(Shape::Angle { a: Pt::new(80., 300.), vertex: Pt::new(40., 420.), b: Pt::new(220., 420.) });
    d.add(Shape::LineAngle { a: Pt::new(330., 330.), b: Pt::new(430., 330.), c: Pt::new(480., 380.), d: Pt::new(520., 440.) });
    d.add(Shape::Marker { p: Pt::new(560., 60.) });
    d.add(Shape::Text { p: Pt::new(470., 40.), text: "Pièce <A> & \"B\"".into() });
    d.add(Shape::Arrow { a: Pt::new(560., 260.), b: Pt::new(460., 200.), text: "Défaut 欠陥 결함 缺陷".into() });
    let hidden = d.add(Shape::Length { a: Pt::new(10., 470.), b: Pt::new(600., 470.) });
    d.get_mut(hidden).unwrap().visible = false;
    d.set_group(&[a, r], "Zone 1");
    d.calibration.set_from_reference(100.0, 2.5, "mm");
    d
}

#[test]
fn svg_bien_forme() {
    meteroscope_i18n::set_language("fr");
    let img = photo(640, 480);
    let doc = sample();
    let svg = svg_string(&img, &doc, &SvgBackground::Embedded).unwrap();
    let x = roxmltree::Document::parse(&svg).expect("XML bien formé");
    let root = x.root_element();
    assert_eq!(root.tag_name().name(), "svg");
    assert_eq!((root.attribute("width"), root.attribute("height")), (Some("640"), Some("480")));
    let image = x.descendants().find(|n| n.has_tag_name("image")).unwrap();
    assert!(image.attribute("href").unwrap().starts_with("data:image/png;base64,iVBORw0KGgo"));
    assert!(image.attribute("transform").is_none());
    // Une entrée par mesure visible, pas pour la mesure masquée.
    let groups: Vec<_> = x.descendants().filter(|n| n.has_tag_name("g") && n.attribute("id").is_some_and(|i| i.len() > 1 && i.starts_with('m') && i[1..].bytes().all(|b| b.is_ascii_digit()))).collect();
    assert_eq!(groups.len(), doc.measurements.len() - 1);
    assert_eq!(groups[0].attribute("data-groupe"), Some("Zone 1"));
    let texts: Vec<String> = x.descendants().filter(|n| n.has_tag_name("text")).filter_map(|n| n.text().map(str::to_owned)).collect();
    for expected in ["Pièce <A> & \"B\"", "Défaut 欠陥 결함 缺陷", "1", "71,57 °"] {
        assert!(texts.iter().any(|t| t == expected), "{expected:?} absent de {texts:?}");
    }
    assert!(texts.iter().any(|t| t.ends_with(" mm")), "barre d'échelle");
    assert!(x.descendants().any(|n| n.has_tag_name("circle")));

    // Image liée, vue redressée : l'image pivote par une matrice, la toile grandit.
    let mut doc = doc;
    doc.rotation = 10.0;
    let svg = svg_string(&img, &doc, &SvgBackground::Linked("photos/pièce 1.png".into())).unwrap();
    let x = roxmltree::Document::parse(&svg).unwrap();
    let (ow, oh) = rotated_dimensions(640, 480, 10.0);
    assert_eq!(x.root_element().attribute("width"), Some(ow.to_string().as_str()));
    assert_eq!(x.root_element().attribute("height"), Some(oh.to_string().as_str()));
    let image = x.descendants().find(|n| n.has_tag_name("image")).unwrap();
    assert_eq!(image.attribute("href"), Some("photos/pièce 1.png"));
    assert!(image.attribute("transform").unwrap().starts_with("matrix(0.98"));
    assert!(svg_string(&img, &doc, &SvgBackground::None).unwrap().matches("<image").count() == 0);
}

#[test]
fn lien_relatif() {
    meteroscope_i18n::set_language("fr");
    let base = std::path::absolute("dossier").unwrap();
    assert_eq!(link_href(&base.join("img/pièce 1.png"), &base.join("out.svg")), "img/pi%C3%A8ce%201.png");
    assert_eq!(link_href(&base.join("a.png"), &base.join("sous/out.svg")), "../a.png");
}

#[test]
fn pdf_produit() {
    meteroscope_i18n::set_language("fr");
    let img = photo(640, 480);
    let mut doc = sample();
    let info = ReportInfo { title: "Rapport de mesures".into(), image_name: "pièce.png".into(), date: today() };
    let pdf = pdf_report(&img, &doc, RotationFilter::Bicubic, &info).unwrap();
    assert!(pdf.starts_with(b"%PDF-1."), "{:?}", &pdf[..8]);
    assert!(pdf.len() > 20_000, "{}", pdf.len());
    assert!(pdf.windows(5).rev().take(64).any(|w| w == b"%%EOF"));
    // Beaucoup de mesures, image redressée : plusieurs pages.
    for i in 0..120 {
        doc.add(Shape::Length { a: Pt::new(10., i as f64), b: Pt::new(200., i as f64 * 2.) });
    }
    doc.rotation = -7.5;
    let long = pdf_report(&img, &doc, RotationFilter::Bicubic, &info).unwrap();
    assert!(long.starts_with(b"%PDF-") && long.len() > pdf.len());

    if let Ok(dir) = std::env::var("METEROSCOPE_REPORT_DEMO") {
        let dir = std::path::Path::new(&dir);
        let doc = sample();
        let big = photo(3000, 2250);
        let mut d2 = doc.clone();
        d2.measurements.iter_mut().for_each(|m| m.shape.map_points(|p| Pt::new(p.x * 4.6875, p.y * 4.6875)));
        d2.calibration.set_from_reference(468.75, 2.5, "mm");
        export_pdf(&big, &d2, RotationFilter::Bicubic, &info, &dir.join("rapport.pdf")).unwrap();
        export_svg(&img, &doc, &SvgBackground::Embedded, &dir.join("annotee.svg")).unwrap();
        let mut r = doc.clone();
        r.rotation = 10.0;
        export_svg(&img, &r, &SvgBackground::Embedded, &dir.join("annotee-redressee.svg")).unwrap();
        save_image(&render_annotated(&img, &doc), &dir.join("annotee.png")).unwrap();
        let mut long = doc.clone();
        for i in 0..120 {
            long.add(Shape::Length { a: Pt::new(10., i as f64), b: Pt::new(200., i as f64 * 2.) });
        }
        long.rotation = -7.5;
        export_pdf(&img, &long, RotationFilter::Bicubic, &info, &dir.join("rapport-long.pdf")).unwrap();
    }
}

/// Démo seulement (`METEROSCOPE_REPORT_DEMO`, à lancer avec `--test-threads=1` :
/// change la langue) : rapport en japonais, noms de mesures en japonais,
/// coréen et chinois.
#[test]
fn pdf_cjk_demo() {
    let Ok(dir) = std::env::var("METEROSCOPE_REPORT_DEMO") else { return };
    meteroscope_i18n::set_language("ja");
    let mut doc = sample();
    let names = ["亀裂の長さ", "균열 폭", "缺陷面积"];
    for (m, n) in doc.measurements.iter_mut().zip(names) {
        m.name = n.to_owned();
    }
    doc.set_group(&[doc.measurements[2].id], "試料 B");
    let info = ReportInfo { title: meteroscope_i18n::tr("report.title").to_owned(), image_name: "試料.png".into(), date: today() };
    export_pdf(&photo(640, 480), &doc, RotationFilter::Bicubic, &info, &std::path::Path::new(&dir).join("rapport-ja.pdf")).unwrap();
    meteroscope_i18n::set_language("fr");
}

#[test]
fn svg_nouvelles_formes() {
    meteroscope_i18n::set_language("fr");
    let img = photo(640, 480);
    let mut doc = Document::new();
    doc.add(Shape::PointLine { a: Pt::new(50., 100.), b: Pt::new(150., 100.), p: Pt::new(250., 40.) });
    doc.add(Shape::Parallel { a: Pt::new(50., 200.), b: Pt::new(150., 200.), p: Pt::new(60., 260.) });
    doc.add(Shape::Ellipse { center: Pt::new(450., 120.), a: Pt::new(550., 120.), b: Pt::new(450., 80.) });
    doc.add(Shape::Arc { a: Pt::new(350., 400.), m: Pt::new(450., 300.), b: Pt::new(550., 400.) });
    let svg = svg_string(&img, &doc, &SvgBackground::None).unwrap();
    let x = roxmltree::Document::parse(&svg).expect("XML bien formé");
    // Une étiquette (texte) et au moins un tracé par forme.
    for m in &doc.measurements {
        let g = x.descendants().find(|n| n.attribute("id") == Some(format!("m{}", m.id).as_str())).unwrap();
        assert!(g.descendants().any(|n| n.has_tag_name("path")) && g.descendants().any(|n| n.has_tag_name("text")));
    }
    if let Some(dir) = std::env::var_os("METEROSCOPE_REPORT_DEMO") {
        std::fs::write(std::path::Path::new(&dir).join("nouvelles-formes.svg"), &svg).unwrap();
        meteroscope_io::render_annotated(&img, &doc).save(std::path::Path::new(&dir).join("nouvelles-formes.png")).unwrap();
    }
}

/// Après une correction de perspective, les mesures sont dans le repère de
/// l'image corrigée : c'est elle qui sert de fond (comme l'application le fait
/// avec son image de travail), pas la photo brute.
#[test]
fn exports_apres_correction_de_perspective() {
    meteroscope_i18n::set_language("fr");
    let img = photo(800, 600);
    let mut doc = Document::new();
    let quad = [Pt::new(150., 120.), Pt::new(620., 180.), Pt::new(560., 470.), Pt::new(90., 380.)];
    assert!(doc.correct_perspective(quad, img.dimensions(), Some((200.0, 100.0, "mm"))));
    let flat = warp_perspective(&img, doc.perspective.as_ref().unwrap(), RotationFilter::Bicubic).unwrap();
    let (w, h) = flat.dimensions();
    assert_ne!((w, h), (800, 600));
    // Une mesure près du coin opposé de l'image corrigée.
    doc.add(Shape::Length { a: Pt::new(w as f64 - 60.0, h as f64 - 20.0), b: Pt::new(w as f64 - 10.0, h as f64 - 20.0) });

    let svg = svg_string(&flat, &doc, &SvgBackground::Embedded).unwrap();
    let x = roxmltree::Document::parse(&svg).unwrap();
    assert_eq!(x.root_element().attribute("viewBox"), Some(format!("0 0 {w} {h}").as_str()));
    let image = x.descendants().find(|n| n.has_tag_name("image")).unwrap();
    assert_eq!((image.attribute("width"), image.attribute("height")), (Some(w.to_string().as_str()), Some(h.to_string().as_str())));

    let info = ReportInfo { title: "Rapport".into(), image_name: "biais.png".into(), date: today() };
    let pdf = pdf_report(&flat, &doc, RotationFilter::Bicubic, &info).unwrap();
    assert!(pdf.starts_with(b"%PDF-1."));
}

/// La barre d'échelle du SVG suit les réglages du document, comme l'image annotée.
#[test]
fn svg_barre_d_echelle_reglee() {
    use meteroscope_core::{Corner, ScaleBarSettings};
    meteroscope_i18n::set_language("fr");
    let img = photo(640, 480);
    let mut doc = sample();
    let bar_rect = |doc: &Document| {
        let svg = svg_string(&img, doc, &SvgBackground::None).unwrap();
        let x = roxmltree::Document::parse(&svg).unwrap();
        x.descendants().find(|n| n.attribute("id") == Some("echelle")).map(|g| {
            let r = g.descendants().find(|n| n.has_tag_name("rect")).unwrap();
            (r.attribute("x").unwrap().parse::<f32>().unwrap(), r.attribute("y").unwrap().parse::<f32>().unwrap())
        })
    };
    let (x, y) = bar_rect(&doc).expect("barre par défaut");
    assert!(x > 320.0 && y > 240.0, "en bas à droite : {x} {y}");
    assert!(doc.set_scale_bar(ScaleBarSettings { visible: true, corner: Corner::TopLeft }));
    let (x, y) = bar_rect(&doc).unwrap();
    assert!(x < 100.0 && y < 100.0, "en haut à gauche : {x} {y}");
    assert!(doc.set_scale_bar(ScaleBarSettings { visible: false, corner: Corner::TopLeft }));
    assert_eq!(bar_rect(&doc), None);
}

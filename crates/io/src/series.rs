//! Série d'images : dimensions et vignettes, tableau consolidé de toute la
//! série. Le fichier projet (une image ou plusieurs) est dans `project.rs`.

use crate::image_io::{describe_io, file_name};
use crate::table::{csv_field, csv_separator, format_precise, measurement_rows, quantity_columns, tsv_field};
use anyhow::{Context, Result, anyhow};
use image::RgbaImage;
use meteroscope_core::{Document, series_summary};
use meteroscope_i18n::tr;
use std::path::Path;

/// Dimensions d'une image lues dans son en-tête (orientation EXIF comprise),
/// sans la décoder. `None` si le fichier est illisible.
pub fn image_dimensions(path: &Path) -> Option<(u32, u32)> {
    use image::ImageDecoder;
    use image::metadata::Orientation as O;
    let mut decoder = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.into_decoder().ok()?;
    let (w, h) = decoder.dimensions();
    match decoder.orientation().unwrap_or(O::NoTransforms) {
        O::Rotate90 | O::Rotate270 | O::Rotate90FlipH | O::Rotate270FlipH => Some((h, w)),
        _ => Some((w, h)),
    }
}

/// Vignette tenant dans un carré de `max_side` pixels (proportions gardées).
pub fn thumbnail(image: &RgbaImage, max_side: u32) -> RgbaImage {
    let side = image.width().max(image.height()).max(1);
    let s = (max_side as f32 / side as f32).min(1.0);
    let (w, h) = ((image.width() as f32 * s).round() as u32, (image.height() as f32 * s).round() as u32);
    image::imageops::thumbnail(image, w.max(1), h.max(1))
}

// ---------- tableau consolidé ----------

/// Les documents n'ont pas tous la même unité de longueur.
fn mixed_units(images: &[(String, &Document)]) -> bool {
    images.windows(2).any(|w| w[0].1.calibration.length_unit() != w[1].1.calibration.length_unit())
}

/// Lignes du tableau de toute la série (en-tête compris) : celles de chaque
/// image, précédées d'une colonne « Image ». Si les unités diffèrent d'une
/// image à l'autre, elles quittent l'en-tête pour une colonne « Unité ».
fn series_rows(images: &[(String, &Document)]) -> Vec<Vec<String>> {
    let mixed = mixed_units(images);
    let mut out: Vec<Vec<String>> = vec![];
    for (name, doc) in images {
        let mut rows = measurement_rows(doc).into_iter();
        let mut header = rows.next().unwrap_or_default();
        if out.is_empty() {
            if mixed {
                // Seules les colonnes de grandeurs perdent leur unité : X et
                // Y restent en pixels, quelle que soit l'unité de l'image.
                let columns = quantity_columns();
                let units = [format!(" ({})", doc.calibration.length_unit()), format!(" ({})", doc.calibration.area_unit())];
                for cell in header.iter_mut().take(columns.end).skip(columns.start) {
                    if let Some(short) = units.iter().find_map(|u| cell.strip_suffix(u.as_str())) {
                        *cell = short.to_owned();
                    }
                }
                header.insert(0, tr!("calib.unit").to_owned());
            }
            header.insert(0, tr!("table.image").to_owned());
            out.push(header);
        }
        for mut row in rows {
            if mixed {
                row.insert(0, doc.calibration.length_unit().to_owned());
            }
            row.insert(0, name.clone());
            out.push(row);
        }
    }
    out
}

/// Bloc statistiques de la série + échelle(s).
fn series_stats_rows(images: &[(String, &Document)]) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = vec![
        [tr!("stats.title"), tr!("table.quantity"), tr!("calib.unit"), "n", tr!("stats.mean"), tr!("stats.std_dev"), tr!("stats.min"), tr!("stats.max")]
            .map(str::to_owned)
            .to_vec(),
    ];
    for (kind, q, unit, s) in series_summary(images.iter().map(|(_, d)| *d)) {
        rows.push(vec![
            kind.label().to_owned(),
            q.label().to_owned(),
            unit,
            s.n.to_string(),
            format_precise(s.mean),
            format_precise(s.sd),
            format_precise(s.min),
            format_precise(s.max),
        ]);
    }
    let markers: usize = images.iter().map(|(_, d)| d.marker_count()).sum();
    rows.push(vec![tr!("table.marker_count").to_owned(), markers.to_string()]);
    rows.push(vec![tr!("table.image_count").to_owned(), images.len().to_string()]);
    rows.push(vec![]);
    let scale = |doc: &Document| match &doc.calibration.scale {
        Some(s) => vec![format_precise(s.units_per_px), format!("{}/px", s.unit)],
        None => vec![tr!("table.uncalibrated").to_owned()],
    };
    let same = images.windows(2).all(|w| w[0].1.calibration == w[1].1.calibration);
    for (name, doc) in images.iter().take(if same { 1 } else { images.len() }) {
        let mut row = vec![tr!("panel.scale").to_owned()];
        if !same {
            row.push(name.clone());
        }
        row.extend(scale(doc));
        rows.push(row);
    }
    rows
}

/// Tableau de toute la série en CSV, comme [`crate::csv_string`] avec une
/// colonne « Image » en tête ; `images` : (nom affiché, document).
pub fn series_csv_string(images: &[(String, &Document)]) -> String {
    let sep = csv_separator();
    let mut out = String::from("\u{feff}");
    let mut push = |rows: Vec<Vec<String>>| {
        for r in rows {
            out.push_str(&r.iter().map(|f| csv_field(f, sep)).collect::<Vec<_>>().join(&sep.to_string()));
            out.push_str("\r\n");
        }
    };
    push(series_rows(images));
    push(vec![vec![]]);
    push(series_stats_rows(images));
    out
}

/// Même tableau en texte tabulé, pour coller dans un tableur.
pub fn series_clipboard_table(images: &[(String, &Document)]) -> String {
    let mut out = String::new();
    for r in series_rows(images) {
        out.push_str(&r.iter().map(|f| tsv_field(f)).collect::<Vec<_>>().join("\t"));
        out.push('\n');
    }
    out
}

pub fn export_series_csv(images: &[(String, &Document)], path: &Path) -> Result<()> {
    std::fs::write(path, series_csv_string(images))
        .map_err(|e| anyhow!(describe_io(&e)))
        .with_context(|| tr!("csv.export_failed", file = file_name(path)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::absolute_clean;
    use crate::{SeriesImage, SeriesProject, load_series, save_series};
    use meteroscope_core::{Pt, Shape};
    use std::path::PathBuf;

    /// Dossier de test propre à chaque essai, dans le dossier de construction.
    fn scratch(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-series").join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn doc(lengths: &[f64], real: Option<(f64, &str)>) -> Document {
        let mut d = Document::new();
        for l in lengths {
            d.add(Shape::Length { a: Pt::new(0., 0.), b: Pt::new(*l, 0.) });
        }
        if let Some((r, unit)) = real {
            d.calibration.set_from_reference(100.0, r, unit);
        }
        d
    }

    fn image(path: &Path, document: Document) -> SeriesImage {
        SeriesImage { image_path: Some(path.to_owned()), size: Some((4, 3)), own_scale: false, missing: false, document }
    }

    fn write_png(path: &Path) {
        crate::save_image(&RgbaImage::new(4, 3), path).unwrap();
    }

    #[test]
    fn projet_v1_relu_comme_serie() {
        meteroscope_i18n::set_language("fr");
        let dir = scratch("v1");
        write_png(&dir.join("photo.png"));
        // Fichier tel qu'écrit par la version 1.
        let v1 = r#"{ "format": "meteroscope", "version": 1, "image": "photo.png",
            "document": { "measurements": [], "calibration": { "scale": { "units_per_px": 0.5, "unit": "mm" } }, "next_id": 0, "counters": {} } }"#;
        let path = dir.join("ancien.meteroscope");
        std::fs::write(&path, v1).unwrap();
        let p = load_series(&path).unwrap();
        assert_eq!((p.images.len(), p.active, p.shared_scale), (1, 0, true));
        let i = &p.images[0];
        assert!(!i.missing && !i.own_scale);
        assert_eq!(i.image_path.as_deref(), Some(absolute_clean(&dir.join("photo.png")).as_path()));
        assert_eq!(i.document.calibration.scale.as_ref().unwrap().units_per_px, 0.5);
    }

    #[test]
    fn une_image_s_ecrit_en_v1() {
        meteroscope_i18n::set_language("fr");
        let dir = scratch("une");
        write_png(&dir.join("a.png"));
        let path = dir.join("p.meteroscope");
        let project = SeriesProject { images: vec![image(&dir.join("a.png"), doc(&[30.0], Some((10.0, "mm"))))], active: 0, shared_scale: true };
        save_series(&project, &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"version\": 1") && !text.contains("\"images\""));
        // Lisible par l'ancienne fonction comme par la nouvelle.
        assert_eq!(crate::load_project(&path).unwrap().document.measurements.len(), 1);
        assert_eq!(load_series(&path).unwrap().images[0].document.measurements.len(), 1);
    }

    #[test]
    fn aller_retour_v2_et_image_manquante() {
        meteroscope_i18n::set_language("fr");
        let dir = scratch("v2");
        std::fs::create_dir_all(dir.join("photos")).unwrap();
        write_png(&dir.join("photos/a.png"));
        write_png(&dir.join("photos/b.png"));
        let mut c = image(&dir.join("photos/c.png"), doc(&[1.0, 2.0, 3.0], Some((5.0, "µm"))));
        c.own_scale = true;
        c.size = Some((800, 600));
        let project = SeriesProject {
            images: vec![image(&dir.join("photos/a.png"), doc(&[30.0], Some((10.0, "mm")))), image(&dir.join("photos/b.png"), doc(&[], Some((10.0, "mm")))), c],
            active: 2,
            shared_scale: false,
        };
        let path = dir.join("lot.meteroscope");
        save_series(&project, &path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("\"version\": 2") && text.contains("\"image\": \"photos/a.png\""));

        let p = load_series(&path).unwrap();
        assert_eq!((p.images.len(), p.active, p.shared_scale), (3, 2, false));
        assert_eq!(p.images.iter().map(|i| i.document.measurements.len()).collect::<Vec<_>>(), [1, 0, 3]);
        assert_eq!(p.images.iter().map(|i| i.own_scale).collect::<Vec<_>>(), [false, false, true]);
        assert_eq!((p.images[0].size, p.images[2].size), (Some((4, 3)), Some((800, 600))));
        assert_eq!(p.images[2].document.calibration.length_unit(), "µm");
        // c.png n'existe pas : signalée, mesures conservées, les autres intactes.
        assert_eq!(p.images.iter().map(|i| i.missing).collect::<Vec<_>>(), [false, false, true]);
        assert_eq!(p.images[2].image_path.as_deref(), Some(absolute_clean(&dir.join("photos/c.png")).as_path()));

        // Un projet d'une image en version 2 (clé `document`, sans `images`)
        // n'est pas pris pour une série : il va au lecteur de projet simple.
        let single_v2 = serde_json::json!({ "format": "meteroscope", "version": 2, "image": "a.png", "document": Document::new() });
        let single = dir.join("seule-v2.meteroscope");
        std::fs::write(&single, single_v2.to_string()).unwrap();
        assert_eq!(load_series(&single).unwrap().images.len(), 1);

        // Un champ de document inconnu et renseigné (écrit par une version
        // plus récente) fait refuser le projet plutôt que de le perdre ; le
        // même champ vide ne gêne pas.
        let newer = dir.join("plus-recent.meteroscope");
        let with = |extra: &str| text.replacen("\"document\": {", &format!("\"document\": {{ {extra},"), 1);
        assert_ne!(with("\"x\": 1"), text);
        std::fs::write(&newer, with("\"champ_futur\": { \"a\": 1 }")).unwrap();
        let err = format!("{:#}", load_series(&newer).unwrap_err());
        assert!(err.contains("champ_futur"), "{err}");
        std::fs::write(&newer, with("\"champ_futur\": null")).unwrap();
        assert_eq!(load_series(&newer).unwrap().images.len(), 3);

        // Le lecteur d'une seule image refuse une série plutôt que d'en perdre.
        assert!(crate::load_project(&path).is_err());
        // Version inconnue.
        std::fs::write(&path, text.replace("\"version\": 2", "\"version\": 3")).unwrap();
        assert!(load_series(&path).is_err());
    }

    /// Un projet d'une image ne reste en version 1 que si la version 1.1.0,
    /// qui ignore les champs qu'elle ne connaît pas, le relirait sans rien
    /// perdre ni fausser.
    #[test]
    fn version_2_des_que_la_1_1_0_lirait_de_travers() {
        meteroscope_i18n::set_language("fr");
        let dir = scratch("compat");
        write_png(&dir.join("a.png"));
        let path = dir.join("p.meteroscope");
        let version = |d: Document| {
            save_series(&SeriesProject { images: vec![image(&dir.join("a.png"), d.clone())], active: 0, shared_scale: true }, &path).unwrap();
            let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            // Relu à l'identique quelle que soit la version.
            let back = load_series(&path).unwrap();
            assert_eq!(serde_json::to_value(&back.images[0].document).unwrap(), serde_json::to_value(&d).unwrap());
            json["version"].as_u64().unwrap()
        };
        let plain = || {
            let mut d = doc(&[30.0], Some((10.0, "mm")));
            d.add(Shape::Rect { a: Pt::new(1., 1.), b: Pt::new(5., 4.), angle: 0.0 });
            d.add(Shape::Marker { p: Pt::new(3.0, 4.0) });
            d
        };
        assert_eq!(version(plain()), 1, "rien que la 1.1.0 ne connaisse");
        let mut d = plain();
        d.add(Shape::Rect { a: Pt::new(1., 1.), b: Pt::new(5., 4.), angle: 30.0 });
        assert_eq!(version(d), 2, "rectangle incliné");
        let mut d = plain();
        d.set_rotation(12.0);
        assert_eq!(version(d), 2, "redressement");
        let mut d = plain();
        d.set_scale_bar(meteroscope_core::ScaleBarSettings { visible: false, ..Default::default() });
        assert_eq!(version(d), 2, "barre d'échelle");
        let mut d = plain();
        d.measurements[0].group = "lot A".to_owned();
        assert_eq!(version(d), 2, "groupe");
        let mut d = plain();
        d.add(Shape::Text { p: Pt::new(1., 1.), text: "note".to_owned() });
        assert_eq!(version(d), 2, "type de mesure inconnu de la 1.1.0");
    }

    /// Le garde-fou des champs inconnus vaut aussi pour un projet d'une image.
    #[test]
    fn champ_inconnu_refuse_dans_un_projet_d_une_image() {
        meteroscope_i18n::set_language("fr");
        let dir = scratch("inconnu");
        let path = dir.join("p.meteroscope");
        let file = |extra: &str| format!(r#"{{ "format": "meteroscope", "version": 1, "document": {{ {extra} "measurements": [], "calibration": {{ "scale": null }}, "next_id": 0, "counters": {{}} }} }}"#);
        std::fs::write(&path, file("")).unwrap();
        assert_eq!(load_series(&path).unwrap().images.len(), 1);
        std::fs::write(&path, file(r#""champ_futur": { "a": 1 },"#)).unwrap();
        let err = format!("{:#}", load_series(&path).unwrap_err());
        assert!(err.contains("champ_futur"), "{err}");
    }

    #[test]
    fn dimensions_et_vignette() {
        let dir = scratch("dims");
        let p = dir.join("i.png");
        crate::save_image(&RgbaImage::new(40, 30), &p).unwrap();
        assert_eq!(image_dimensions(&p), Some((40, 30)));
        assert_eq!(image_dimensions(&dir.join("absent.png")), None);
        let t = thumbnail(&RgbaImage::new(4000, 3000), 100);
        assert_eq!((t.width(), t.height()), (100, 75));
        let t = thumbnail(&RgbaImage::new(40, 30), 100);
        assert_eq!((t.width(), t.height()), (40, 30));
    }

    #[test]
    fn csv_consolide_exact() {
        meteroscope_i18n::set_language("fr");
        let a = doc(&[50.0, 100.0], Some((10.0, "mm")));
        let mut b = doc(&[150.0], Some((10.0, "mm")));
        b.add(Shape::Marker { p: Pt::new(3.0, 4.0) });
        let vide = doc(&[], Some((10.0, "mm")));
        let images = [("a.jpg".to_owned(), &a), ("vide.jpg".to_owned(), &vide), ("b; 2.jpg".to_owned(), &b)];
        let expected = "\u{feff}\
Image;Nom;Type;Longueur (mm);Angle (°);Aire (mm²);Périmètre (mm);Largeur (mm);Hauteur (mm);Diamètre (mm);Rayon (mm);Distance (mm);Grand axe (mm);Petit axe (mm);Longueur d'arc (mm);Corde (mm);X (px);Y (px);Groupe\r\n\
a.jpg;Longueur 1;Longueur;5;;;;;;;;;;;;;;;\r\n\
a.jpg;Longueur 2;Longueur;10;;;;;;;;;;;;;;;\r\n\
\"b; 2.jpg\";Longueur 1;Longueur;15;;;;;;;;;;;;;;;\r\n\
\"b; 2.jpg\";Marqueur 1;Marqueur;;;;;;;;;;;;;;3;4;\r\n\
\r\n\
Statistiques;Grandeur;Unité;n;Moyenne;Écart-type;Min;Max\r\n\
Longueur;Longueur;mm;3;10;5;5;15\r\n\
Nombre de marqueurs;1\r\n\
Nombre d'images;3\r\n\
\r\n\
Échelle;0,1;mm/px\r\n";
        assert_eq!(series_csv_string(&images), expected);
        let t = series_clipboard_table(&images);
        assert_eq!(t.lines().count(), 5);
        assert_eq!(t.lines().nth(1).unwrap(), "a.jpg\tLongueur 1\tLongueur\t5\t\t\t\t\t\t\t\t\t\t\t\t\t\t\t");
    }

    #[test]
    fn csv_consolide_unites_differentes() {
        meteroscope_i18n::set_language("fr");
        let a = doc(&[50.0], Some((10.0, "mm")));
        let b = doc(&[50.0], None);
        let images = [("a.jpg".to_owned(), &a), ("b.jpg".to_owned(), &b)];
        let s = series_csv_string(&images);
        let lines: Vec<&str> = s.trim_start_matches('\u{feff}').split("\r\n").collect();
        assert_eq!(lines[0], "Image;Unité;Nom;Type;Longueur;Angle (°);Aire;Périmètre;Largeur;Hauteur;Diamètre;Rayon;Distance;Grand axe;Petit axe;Longueur d'arc;Corde;X (px);Y (px);Groupe");
        assert_eq!(lines[1], "a.jpg;mm;Longueur 1;Longueur;5;;;;;;;;;;;;;;;");
        assert_eq!(lines[2], "b.jpg;px;Longueur 1;Longueur;50;;;;;;;;;;;;;;;");
        // Dans l'autre ordre (image en pixels d'abord), même en-tête : X
        // garde son unité.
        let inverse = [("b.jpg".to_owned(), &b), ("a.jpg".to_owned(), &a)];
        let t = series_csv_string(&inverse);
        assert_eq!(t.trim_start_matches('\u{feff}').split("\r\n").next(), Some(lines[0]));
        assert!(lines[0].ends_with(";X (px);Y (px);Groupe"));
        assert_eq!(series_clipboard_table(&inverse).lines().next(), series_clipboard_table(&images).lines().next());
        // Une ligne de statistiques par unité, une ligne d'échelle par image.
        assert!(s.contains("Longueur;Longueur;mm;1;5;0;5;5\r\nLongueur;Longueur;px;1;50;0;50;50\r\n"));
        assert!(s.ends_with("Échelle;a.jpg;0,1;mm/px\r\nÉchelle;b.jpg;non étalonnée (mesures en pixels)\r\n"));
    }
}

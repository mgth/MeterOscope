//! Série d'images : projet version 2 (plusieurs images, chacune avec son
//! document), tableau consolidé de toute la série, vignettes.
//!
//! Le format version 1 (une image, voir `project.rs`) reste lu, et reste
//! écrit tant que le projet ne contient qu'une image : une ancienne version
//! de l'application peut donc toujours l'ouvrir.

use crate::image_io::{describe_io, file_name};
use crate::project::{FORMAT_ID, absolute_clean, relative_to, resolve_image, write_atomic};
use crate::table::{csv_field, csv_separator, format_precise, measurement_rows, tsv_field};
use crate::Project;
use anyhow::{Context, Result, anyhow, bail};
use image::RgbaImage;
use meteroscope_core::{Document, series_summary};
use meteroscope_i18n::tr;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Version du format à plusieurs images.
pub const SERIES_FORMAT_VERSION: u32 = 2;

/// Une image d'un projet et son document.
#[derive(Clone, Debug)]
pub struct SeriesImage {
    /// Chemin de l'image (résolu en absolu à la lecture).
    pub image_path: Option<PathBuf>,
    /// Dimensions de l'image à l'enregistrement, si connues.
    pub size: Option<(u32, u32)>,
    /// L'image a sa propre échelle (elle ne suit pas celle de la série).
    pub own_scale: bool,
    /// À la lecture : le fichier image est introuvable. Les mesures sont là quand même.
    pub missing: bool,
    pub document: Document,
}

/// Projet complet : la série d'images.
#[derive(Clone, Debug)]
pub struct SeriesProject {
    pub images: Vec<SeriesImage>,
    /// Image affichée à l'enregistrement.
    pub active: usize,
    /// « Même échelle pour toute la série ».
    pub shared_scale: bool,
}

fn yes() -> bool {
    true
}
fn is_false(v: &bool) -> bool {
    !*v
}

/// Une image sur disque (version 2) : mêmes champs que le projet version 1.
#[derive(Serialize, Deserialize)]
struct ImageFile {
    /// Chemin relatif au projet (séparateurs `/`) si possible, sinon absolu.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    image: Option<String>,
    /// Chemin absolu à l'enregistrement (secours si le projet a été déplacé).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    image_absolute: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    height: Option<u32>,
    #[serde(default, skip_serializing_if = "is_false")]
    own_scale: bool,
    document: Document,
}

/// Représentation sur disque (version 2).
#[derive(Serialize, Deserialize)]
struct SeriesFile {
    format: String,
    version: u32,
    #[serde(default = "yes")]
    shared_scale: bool,
    #[serde(default)]
    active: usize,
    images: Vec<ImageFile>,
}

/// Enregistre la série. Un projet d'une seule image s'écrit en version 1.
pub fn save_series(project: &SeriesProject, path: &Path) -> Result<()> {
    if let [only] = project.images.as_slice() {
        return crate::save_project(&Project { image_path: only.image_path.clone(), document: only.document.clone() }, path);
    }
    let ctx = || tr!("project.save_failed", file = file_name(path));
    let proj_abs = absolute_clean(path);
    let dir = proj_abs.parent().unwrap_or(Path::new("/"));
    let images = project
        .images
        .iter()
        .map(|i| {
            let abs = i.image_path.as_deref().map(absolute_clean);
            let abs_str = abs.as_ref().map(|a| a.to_string_lossy().into_owned());
            ImageFile {
                image: abs.as_ref().and_then(|a| relative_to(a, dir)).or_else(|| abs_str.clone()),
                image_absolute: abs_str,
                width: i.size.map(|s| s.0),
                height: i.size.map(|s| s.1),
                own_scale: i.own_scale,
                document: i.document.clone(),
            }
        })
        .collect();
    let file = SeriesFile {
        format: FORMAT_ID.to_owned(),
        version: SERIES_FORMAT_VERSION,
        shared_scale: project.shared_scale,
        active: project.active,
        images,
    };
    let mut json = serde_json::to_string_pretty(&file).context(tr!("project.serialize")).with_context(ctx)?;
    json.push('\n');
    write_atomic(path, json.as_bytes()).map_err(|e| anyhow!(describe_io(&e))).with_context(ctx)?;
    Ok(())
}

/// Vrai si le fichier projet a la forme d'une série (liste `images`), faux
/// pour un projet d'une image (clé `document`), quelle que soit sa version.
fn is_series(value: &serde_json::Value) -> bool {
    value.get("images").is_some()
}

/// Premier champ d'un document du fichier que cette version ne connaît pas
/// et qui porte une valeur (par exemple une correction de perspective écrite
/// par une version plus récente). L'ignorer afficherait des mesures fausses
/// et le ferait disparaître à l'enregistrement : mieux vaut refuser.
fn unknown_document_field(value: &serde_json::Value) -> Option<String> {
    use serde_json::Value as V;
    let empty = |v: &V| match v {
        V::Null => true,
        V::Bool(b) => !b,
        V::Number(n) => n.as_f64() == Some(0.0),
        V::String(s) => s.is_empty(),
        V::Array(a) => a.is_empty(),
        V::Object(o) => o.is_empty(),
    };
    for image in value.get("images")?.as_array()? {
        let Some(written) = image.get("document").and_then(V::as_object) else { continue };
        // Un document illisible est signalé plus loin, avec son erreur.
        let Ok(doc) = serde_json::from_value::<Document>(V::Object(written.clone())) else { continue };
        let Ok(V::Object(known)) = serde_json::to_value(&doc) else { continue };
        if let Some((key, _)) = written.iter().find(|(k, v)| !known.contains_key(*k) && !empty(v)) {
            return Some(key.clone());
        }
    }
    None
}

/// Charge un projet version 1 (lu comme une série d'une image) ou 2.
/// Une image introuvable ne fait pas échouer la lecture : elle est signalée
/// par [`SeriesImage::missing`].
pub fn load_series(path: &Path) -> Result<SeriesProject> {
    let ctx = || tr!("project.open_failed", file = file_name(path));
    let text = std::fs::read(path).map_err(|e| anyhow!(describe_io(&e))).with_context(ctx)?;
    let text = String::from_utf8(text).map_err(|_| anyhow!(tr!("project.not_utf8"))).with_context(ctx)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| anyhow!(tr!("project.bad_json", line = e.line(), column = e.column())))
        .with_context(ctx)?;
    let missing = |p: &Option<PathBuf>| p.as_ref().is_some_and(|p| !p.exists());
    // La forme du fichier décide du lecteur, pas son numéro de version : un
    // projet d'une image (clé `document`) peut lui aussi évoluer au-delà de
    // la version 1 et reste l'affaire de `load_project`, qui le valide.
    if !is_series(&value) {
        let p = crate::load_project(path)?;
        let image = SeriesImage { missing: missing(&p.image_path), image_path: p.image_path, size: None, own_scale: false, document: p.document };
        return Ok(SeriesProject { images: vec![image], active: 0, shared_scale: true });
    }
    let check = || -> Result<()> {
        if value.get("format").and_then(|f| f.as_str()) != Some(FORMAT_ID) {
            bail!(tr!("project.not_project"));
        }
        match value.get("version").and_then(|v| v.as_u64()) {
            None => bail!(tr!("project.bad_version")),
            Some(v) if v < SERIES_FORMAT_VERSION as u64 => bail!(tr!("project.bad_version")),
            Some(v) if v > SERIES_FORMAT_VERSION as u64 => bail!(tr!("project.unknown_version", version = v, max = SERIES_FORMAT_VERSION)),
            Some(_) => Ok(()),
        }
    };
    check().with_context(ctx)?;
    if let Some(field) = unknown_document_field(&value) {
        return Err(anyhow!(tr!("project.newer_data", field = field))).with_context(ctx);
    }
    let file: SeriesFile =
        serde_json::from_value(value).map_err(|e| anyhow!(tr!("project.bad_content", error = e))).with_context(ctx)?;
    let proj_abs = absolute_clean(path);
    let dir = proj_abs.parent().unwrap_or(Path::new("/"));
    let images: Vec<SeriesImage> = file
        .images
        .into_iter()
        .map(|i| {
            let image_path = i.image.as_deref().map(|s| resolve_image(s, i.image_absolute.as_deref(), dir));
            SeriesImage { missing: missing(&image_path), image_path, size: i.width.zip(i.height), own_scale: i.own_scale, document: i.document }
        })
        .collect();
    Ok(SeriesProject { active: file.active.min(images.len().saturating_sub(1)), shared_scale: file.shared_scale, images })
}

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
                // Les deux dernières colonnes (X, Y) sont toujours en pixels.
                let n = header.len().saturating_sub(2);
                let units = [format!(" ({})", doc.calibration.length_unit()), format!(" ({})", doc.calibration.area_unit())];
                for cell in &mut header[..n] {
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
    use meteroscope_core::{Pt, Shape};

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
        assert!(!is_series(&single_v2));
        assert!(is_series(&serde_json::from_str(&text).unwrap()));
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
        // Une ligne de statistiques par unité, une ligne d'échelle par image.
        assert!(s.contains("Longueur;Longueur;mm;1;5;0;5;5\r\nLongueur;Longueur;px;1;50;0;50;50\r\n"));
        assert!(s.ends_with("Échelle;a.jpg;0,1;mm/px\r\nÉchelle;b.jpg;non étalonnée (mesures en pixels)\r\n"));
    }
}

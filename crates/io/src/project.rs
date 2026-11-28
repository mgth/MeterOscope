//! Fichier projet `.meteroscope` : JSON lisible, versionné.
//!
//! Un seul lecteur et un seul écrivain ([`load_series`], [`save_series`]),
//! pour un projet d'une image comme pour une série. Deux formes sur disque :
//!
//! - version 1, `document` à la racine : celle de la version 1.1.0 du
//!   logiciel. On ne l'écrit que pour un projet d'une image que la 1.1.0
//!   relirait sans rien perdre ni fausser ([`readable_by_v1`]) ;
//! - version 2, tableau `images` : tout le reste. La 1.1.0 refuse ce numéro
//!   de version au lieu d'ignorer en silence ce qu'elle ne connaît pas
//!   (rectangle incliné affiché droit, redressement, groupes, perspective…).

use crate::Project;
use crate::image_io::{describe_io, file_name};
use anyhow::{Context, Result, anyhow, bail};
use meteroscope_core::Document;
use meteroscope_i18n::tr;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

/// Identifiant du format, pour reconnaître nos fichiers.
const FORMAT_ID: &str = "meteroscope";
/// Version du format à une image, `document` à la racine, que lit la version
/// 1.1.0 du logiciel.
pub const PROJECT_FORMAT_VERSION: u32 = 1;
/// Version du format à tableau `images` ; la plus récente que cette version lit.
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

/// Projet complet : la série d'images (une seule pour un projet simple).
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

/// Une image sur disque : la racine d'un projet version 1 (sans les champs
/// propres aux séries), ou un élément du tableau `images` de la version 2.
#[derive(Serialize, Deserialize)]
struct ImageFile {
    /// Chemin relatif au projet (séparateurs `/`) si possible, sinon absolu.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    image: Option<String>,
    /// Chemin absolu à l'enregistrement (secours si le projet a été déplacé
    /// sans l'image).
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

/// Forme à une image : `document` à la racine.
#[derive(Serialize, Deserialize)]
struct SingleFile {
    format: String,
    version: u32,
    #[serde(flatten)]
    image: ImageFile,
}

/// Forme à tableau `images`.
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

// ---------- ce que lit la version 1.1.0 ----------

/// Champs du document, d'une mesure et de chaque forme tels que la version
/// 1.1.0 les connaît. Elle ignore tout autre champ sans rien dire.
const V1_DOCUMENT: &[&str] = &["measurements", "calibration", "next_id", "counters"];
const V1_MEASUREMENT: &[&str] = &["id", "name", "shape", "color", "visible"];
const V1_SHAPES: &[(&str, &[&str])] = &[
    ("Length", &["a", "b"]),
    ("Path", &["pts"]),
    ("Angle", &["a", "vertex", "b"]),
    ("Area", &["pts"]),
    ("Rect", &["a", "b"]),
    ("Circle", &["center", "edge"]),
    ("Circle3", &["a", "b", "c"]),
    ("Marker", &["p"]),
];

/// Tous les champs de l'objet `v` sont parmi `known`.
fn keys_within(v: &Value, known: &[&str]) -> bool {
    v.as_object().is_some_and(|o| o.keys().all(|k| known.contains(&k.as_str())))
}

/// La version 1.1.0 relirait ce document sans rien perdre ni fausser : tout
/// ce qu'il écrit, elle le connaît. Sinon (rectangle incliné, redressement,
/// barre d'échelle réglée, groupe, perspective, type de mesure plus récent…)
/// le projet doit porter un numéro de version qu'elle refuse.
fn readable_by_v1(document: &Document) -> bool {
    let Ok(v) = serde_json::to_value(document) else { return false };
    let shape = |s: &Value| s.as_object().is_some_and(|o| o.iter().all(|(name, fields)| V1_SHAPES.iter().any(|(n, f)| n == name && keys_within(fields, f))));
    let measurement = |m: &Value| keys_within(m, V1_MEASUREMENT) && m.get("shape").is_some_and(shape);
    let kinds: Vec<&str> = V1_SHAPES.iter().map(|(n, _)| *n).collect();
    let scale = &v["calibration"]["scale"];
    keys_within(&v, V1_DOCUMENT)
        && v["measurements"].as_array().is_some_and(|a| a.iter().all(measurement))
        && keys_within(&v["counters"], &kinds)
        && keys_within(&v["calibration"], &["scale"])
        && (scale.is_null() || keys_within(scale, &["units_per_px", "unit"]))
}

// ---------- chemins ----------

/// Rend un chemin absolu sans accéder au disque et résout `.` / `..` lexicalement.
pub(crate) fn absolute_clean(p: &Path) -> PathBuf {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                // Ne remonte pas au-delà de la racine.
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// Chemin de `target` relatif au dossier `base` (tous deux absolus et nettoyés),
/// avec des `/`. `None` si aucune racine commune (autre lecteur sous Windows)
/// ou si un composant n'est pas de l'UTF-8.
pub(crate) fn relative_to(target: &Path, base: &Path) -> Option<String> {
    let t: Vec<Component> = target.components().collect();
    let b: Vec<Component> = base.components().collect();
    let common = t.iter().zip(&b).take_while(|(x, y)| x == y).count();
    // Préfixe/racine différents : pas de chemin relatif possible.
    let roots = |v: &[Component]| v.iter().take_while(|c| matches!(c, Component::Prefix(_) | Component::RootDir)).count();
    if common < roots(&t).max(roots(&b)) {
        return None;
    }
    let mut parts: Vec<String> = vec!["..".to_owned(); b.len() - common];
    for c in &t[common..] {
        parts.push(c.as_os_str().to_str()?.to_owned());
    }
    if parts.is_empty() {
        return None;
    }
    Some(parts.join("/"))
}

/// Écrit `data` de façon atomique (fichier temporaire puis renommage).
fn write_atomic(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

// ---------- écriture ----------

/// Enregistre le projet. Une seule image que la version 1.1.0 relirait
/// fidèlement, avec les réglages de série par défaut, s'écrit en version 1 ;
/// tout le reste en version 2.
pub fn save_series(project: &SeriesProject, path: &Path) -> Result<()> {
    let ctx = || tr!("project.save_failed", file = file_name(path));
    let proj_abs = absolute_clean(path);
    let dir = proj_abs.parent().unwrap_or(Path::new("/"));
    let entry = |i: &SeriesImage| {
        // Un chemin relatif fourni par l'appelant est compris par rapport au dossier courant.
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
    };
    let format = FORMAT_ID.to_owned();
    let json = match project.images.as_slice() {
        // La forme de la 1.1.0 n'a pas de champs de série : le lecteur les
        // reconstitue (échelle commune active, image liée). Des réglages
        // différents, gardés d'une série ramenée à une image, imposent la v2.
        [only] if project.shared_scale && !only.own_scale && readable_by_v1(&only.document) => {
            let image = ImageFile { width: None, height: None, own_scale: false, ..entry(only) };
            serde_json::to_string_pretty(&SingleFile { format, version: PROJECT_FORMAT_VERSION, image })
        }
        images => serde_json::to_string_pretty(&SeriesFile {
            format,
            version: SERIES_FORMAT_VERSION,
            shared_scale: project.shared_scale,
            active: project.active,
            images: images.iter().map(entry).collect(),
        }),
    };
    let mut json = json.context(tr!("project.serialize")).with_context(ctx)?;
    json.push('\n');
    write_atomic(path, json.as_bytes()).map_err(|e| anyhow!(describe_io(&e))).with_context(ctx)?;
    Ok(())
}

/// Enregistre un projet d'une image (même écrivain que [`save_series`]).
pub fn save_project(project: &Project, path: &Path) -> Result<()> {
    let image = SeriesImage { image_path: project.image_path.clone(), size: None, own_scale: false, missing: false, document: project.document.clone() };
    save_series(&SeriesProject { images: vec![image], active: 0, shared_scale: true }, path)
}

// ---------- lecture ----------

/// Résout le chemin d'image stocké, relativement au dossier du projet.
fn resolve_image(stored: &str, absolute: Option<&str>, dir: &Path) -> PathBuf {
    let p = Path::new(stored);
    if p.is_absolute() {
        return absolute_clean(p);
    }
    let rel: PathBuf = stored.split('/').collect();
    let candidate = absolute_clean(&dir.join(rel));
    if candidate.exists() {
        return candidate;
    }
    // Projet déplacé sans l'image : on tente l'emplacement absolu d'origine.
    match absolute.map(PathBuf::from) {
        Some(a) if a.exists() => a,
        _ => candidate,
    }
}

/// Premier champ d'un document du fichier que cette version ne connaît pas
/// et qui porte une valeur (écrit par une version plus récente). L'ignorer
/// afficherait des mesures fausses et le ferait disparaître à
/// l'enregistrement : mieux vaut refuser.
fn unknown_document_field<'a>(documents: impl Iterator<Item = &'a Value>) -> Option<String> {
    let empty = |v: &Value| match v {
        Value::Null => true,
        Value::Bool(b) => !b,
        Value::Number(n) => n.as_f64() == Some(0.0),
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
    };
    for written in documents.filter_map(Value::as_object) {
        // Un document illisible est signalé plus loin, avec son erreur.
        let Ok(doc) = serde_json::from_value::<Document>(Value::Object(written.clone())) else { continue };
        let Ok(known) = serde_json::to_value(&doc) else { continue };
        // Les champs par défaut ne sont pas tous réécrits : on compare aux
        // champs que le type sait lire, pas à ceux qu'il vient d'écrire.
        let readable = |k: &str| known.get(k).is_some() || DOCUMENT_FIELDS.contains(&k);
        if let Some((key, _)) = written.iter().find(|(k, v)| !readable(k) && !empty(v)) {
            return Some(key.clone());
        }
    }
    None
}

/// Champs du document absents du JSON quand ils valent leur défaut.
const DOCUMENT_FIELDS: &[&str] = &["rotation", "scale_bar", "perspective"];

/// Charge un projet, quelle que soit sa forme. Une image introuvable ne fait
/// pas échouer la lecture : elle est signalée par [`SeriesImage::missing`].
pub fn load_series(path: &Path) -> Result<SeriesProject> {
    let ctx = || tr!("project.open_failed", file = file_name(path));
    let text = std::fs::read(path).map_err(|e| anyhow!(describe_io(&e))).with_context(ctx)?;
    let text = String::from_utf8(text).map_err(|_| anyhow!(tr!("project.not_utf8"))).with_context(ctx)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let value: Value = serde_json::from_str(text)
        .map_err(|e| anyhow!(tr!("project.bad_json", line = e.line(), column = e.column())))
        .with_context(ctx)?;
    // La forme se reconnaît au tableau `images`, pas au numéro : un projet
    // version 2 à `document` racine a existé brièvement et reste lu.
    let several = value.get("images").is_some();
    let check = || -> Result<()> {
        if value.get("format").and_then(|f| f.as_str()) != Some(FORMAT_ID) {
            bail!(tr!("project.not_project"));
        }
        match value.get("version").and_then(|v| v.as_u64()) {
            None | Some(0) => bail!(tr!("project.bad_version")),
            // La version 1 n'a jamais eu de tableau `images`.
            Some(v) if several && v < SERIES_FORMAT_VERSION as u64 => bail!(tr!("project.bad_version")),
            Some(v) if v > SERIES_FORMAT_VERSION as u64 => bail!(tr!("project.unknown_version", version = v, max = SERIES_FORMAT_VERSION)),
            Some(_) => {}
        }
        let documents: Vec<&Value> = match value.get("images").and_then(Value::as_array) {
            Some(images) => images.iter().filter_map(|i| i.get("document")).collect(),
            None => value.get("document").into_iter().collect(),
        };
        match unknown_document_field(documents.into_iter()) {
            Some(field) => bail!(tr!("project.newer_data", field = field)),
            None => Ok(()),
        }
    };
    check().with_context(ctx)?;
    let bad = |e: serde_json::Error| anyhow!(tr!("project.bad_content", error = e));
    let (images, active, shared_scale) = if several {
        let file: SeriesFile = serde_json::from_value(value).map_err(bad).with_context(ctx)?;
        (file.images, file.active, file.shared_scale)
    } else {
        (vec![serde_json::from_value::<SingleFile>(value).map_err(bad).with_context(ctx)?.image], 0, true)
    };
    let proj_abs = absolute_clean(path);
    let dir = proj_abs.parent().unwrap_or(Path::new("/"));
    let images: Vec<SeriesImage> = images
        .into_iter()
        .map(|i| {
            let image_path = i.image.as_deref().map(|s| resolve_image(s, i.image_absolute.as_deref(), dir));
            let missing = image_path.as_ref().is_some_and(|p| !p.exists());
            SeriesImage { missing, image_path, size: i.width.zip(i.height), own_scale: i.own_scale, document: i.document }
        })
        .collect();
    Ok(SeriesProject { active: active.min(images.len().saturating_sub(1)), shared_scale, images })
}

/// Charge un projet d'une image (même lecteur que [`load_series`]) ; une
/// série est refusée plutôt que d'en perdre des images.
pub fn load_project(path: &Path) -> Result<Project> {
    let mut images = load_series(path)?.images;
    match images.pop() {
        Some(only) if images.is_empty() => Ok(Project { image_path: only.image_path, document: only.document }),
        _ => Err(anyhow!(tr!("project.several_images"))).with_context(|| tr!("project.open_failed", file = file_name(path))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths() {
        meteroscope_i18n::set_language("fr");
        let r = |t: &str, b: &str| relative_to(Path::new(t), Path::new(b));
        assert_eq!(r("/a/b/img.png", "/a/b").as_deref(), Some("img.png"));
        assert_eq!(r("/a/b/c/img.png", "/a/b").as_deref(), Some("c/img.png"));
        assert_eq!(r("/a/x/img.png", "/a/b").as_deref(), Some("../x/img.png"));
        assert_eq!(r("/img.png", "/a/b").as_deref(), Some("../../img.png"));
        // Sous Windows, « /a » est relatif au lecteur courant : on compare donc
        // à la forme absolue attendue plutôt qu'à une chaîne Unix.
        let abs = |s: &str| std::path::absolute(s).unwrap();
        assert_eq!(absolute_clean(Path::new("/a/b/../c/./d")), abs("/a/c/d"));
        assert_eq!(absolute_clean(Path::new("/../a")), abs("/a"));
    }

    #[cfg(windows)]
    #[test]
    fn relative_paths_windows() {
        meteroscope_i18n::set_language("fr");
        let r = |t: &str, b: &str| relative_to(Path::new(t), Path::new(b));
        assert_eq!(r(r"C:\a\b\c\img.png", r"C:\a\b").as_deref(), Some("c/img.png"));
        assert_eq!(r(r"C:\a\x\img.png", r"C:\a\b").as_deref(), Some("../x/img.png"));
        // Autre lecteur ou partage réseau : pas de chemin relatif.
        assert_eq!(r(r"D:\img.png", r"C:\a"), None);
        assert_eq!(r(r"\\srv\photos\img.png", r"C:\a"), None);
        assert_eq!(absolute_clean(Path::new(r"C:\a\b\..\c\.\d")), PathBuf::from(r"C:\a\c\d"));
    }
}

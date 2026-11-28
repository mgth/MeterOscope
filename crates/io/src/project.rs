//! Fichier projet `.meteroscope` : JSON lisible, versionné.

use meteroscope_i18n::tr;
use crate::Project;
use crate::image_io::{describe_io, file_name};
use anyhow::{Context, Result, anyhow, bail};
use meteroscope_core::Document;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// Identifiant du format, pour reconnaître nos fichiers.
const FORMAT_ID: &str = "meteroscope";
/// Version la plus récente du format que l'on sait lire.
pub const PROJECT_FORMAT_VERSION: u32 = 2;
/// Version écrite pour un document sans correction de perspective : ces
/// projets restent lisibles par les versions antérieures du logiciel.
const PLAIN_FORMAT_VERSION: u32 = 1;

/// Version à écrire pour `doc`. Avec une correction de perspective, les
/// mesures et l'échelle sont exprimées dans l'image corrigée : un lecteur qui
/// ignorerait le champ les superposerait à la photo brute, et perdrait la
/// correction en réenregistrant. La version 2 lui fait refuser le projet.
fn format_version(doc: &Document) -> u32 {
    if doc.perspective.is_some() { PROJECT_FORMAT_VERSION } else { PLAIN_FORMAT_VERSION }
}

/// Représentation sur disque (versions 1 et 2 : la 2 ajoute `document.perspective`).
#[derive(Serialize, Deserialize)]
struct ProjectFile {
    format: String,
    version: u32,
    /// Chemin de l'image relatif au projet (séparateurs `/`) si possible, sinon absolu.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    image: Option<String>,
    /// Chemin absolu de l'image au moment de l'enregistrement (secours si
    /// le projet a été déplacé sans l'image).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    image_absolute: Option<String>,
    document: Document,
}

/// Rend un chemin absolu sans accéder au disque et résout `.` / `..` lexicalement.
fn absolute_clean(p: &Path) -> PathBuf {
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
fn relative_to(target: &Path, base: &Path) -> Option<String> {
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

pub fn save_project(project: &Project, path: &Path) -> Result<()> {
    let ctx = || tr!("project.save_failed", file = file_name(path));
    let proj_abs = absolute_clean(path);
    let dir = proj_abs.parent().unwrap_or(Path::new("/"));
    let (image, image_absolute) = match &project.image_path {
        None => (None, None),
        Some(img) => {
            // Un chemin relatif fourni par l'appelant est compris par rapport au dossier courant.
            let img_abs = absolute_clean(img);
            let abs_str = img_abs.to_string_lossy().into_owned();
            let rel = relative_to(&img_abs, dir);
            (Some(rel.unwrap_or_else(|| abs_str.clone())), Some(abs_str))
        }
    };
    let file = ProjectFile {
        format: FORMAT_ID.to_owned(),
        version: format_version(&project.document),
        image,
        image_absolute,
        document: project.document.clone(),
    };
    let mut json = serde_json::to_string_pretty(&file).context(tr!("project.serialize")).with_context(ctx)?;
    json.push('\n');
    write_atomic(path, json.as_bytes()).map_err(|e| anyhow!(describe_io(&e))).with_context(ctx)?;
    Ok(())
}

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

pub fn load_project(path: &Path) -> Result<Project> {
    let ctx = || tr!("project.open_failed", file = file_name(path));
    let text = std::fs::read(path).map_err(|e| anyhow!(describe_io(&e))).with_context(ctx)?;
    let text = String::from_utf8(text).map_err(|_| anyhow!(tr!("project.not_utf8"))).with_context(ctx)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let value: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| anyhow!(tr!("project.bad_json", line = e.line(), column = e.column())))
        .with_context(ctx)?;
    let check = || -> Result<()> {
        if value.get("format").and_then(|f| f.as_str()) != Some(FORMAT_ID) {
            bail!(tr!("project.not_project"));
        }
        match value.get("version").and_then(|v| v.as_u64()) {
            None => bail!(tr!("project.bad_version")),
            Some(v) if v == 0 || v > PROJECT_FORMAT_VERSION as u64 => bail!(tr!("project.unknown_version", version = v, max = PROJECT_FORMAT_VERSION)),
            Some(_) => Ok(()),
        }
    };
    check().with_context(ctx)?;
    let file: ProjectFile =
        serde_json::from_value(value).map_err(|e| anyhow!(tr!("project.bad_content", error = e))).with_context(ctx)?;
    let proj_abs = absolute_clean(path);
    let dir = proj_abs.parent().unwrap_or(Path::new("/"));
    let image_path = file.image.as_deref().map(|s| resolve_image(s, file.image_absolute.as_deref(), dir));
    Ok(Project { image_path, document: file.document })
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

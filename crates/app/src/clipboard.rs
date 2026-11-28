//! Coller une image depuis le presse-papiers (Ctrl+V).
//!
//! egui ne lit l'image du presse-papiers que s'il n'y a aucun texte, et
//! n'émet rien quand le presse-papiers est vide : on repère donc le geste
//! Ctrl+V (événement `Paste`/`PasteImage`, ou à défaut le relâchement de V
//! avec Ctrl), puis on lit nous-mêmes le presse-papiers avec `arboard`, par
//! ordre de préférence : liste de fichiers, image, texte contenant un chemin.
//! La conversion des données reçues est faite par des fonctions pures.

use egui::{Event, Key};
use image::RgbaImage;
use meteroscope_io as io;
use std::cell::RefCell;
use std::path::{Path, PathBuf};

/// Ce que le presse-papiers permet d'ouvrir.
#[derive(Debug, PartialEq)]
pub enum Pasted {
    /// Image sans fichier d'origine (capture d'écran, image copiée d'un navigateur).
    Image(RgbaImage),
    /// Fichier image ou projet à ouvrir.
    File(PathBuf),
    /// Rien d'utilisable.
    Nothing,
}

/// Ce qu'egui a déjà lu du presse-papiers, en secours si `arboard` échoue.
#[derive(Default)]
pub struct Fallback {
    text: Option<String>,
    image: Option<RgbaImage>,
}

/// Repère un Ctrl+V destiné à l'application (pas à un champ de saisie).
/// `enabled` est faux quand un dialogue occupe le clavier.
pub fn paste_requested(ctx: &egui::Context, enabled: bool) -> Option<Fallback> {
    let id = egui::Id::unique("meteroscope_paste_seen");
    let mut seen: bool = ctx.data(|d| d.get_temp(id)).unwrap_or(false);
    let mut request = None;
    for event in ctx.input(|i| i.events.clone()) {
        match event {
            Event::Paste(text) => {
                seen = true;
                request = Some(Fallback { text: Some(text), image: None });
            }
            Event::PasteImage(img) => {
                seen = true;
                request = Some(Fallback { text: None, image: image_from_rgba(img.width(), img.height(), img.as_raw().to_vec(), cfg!(windows)) });
            }
            // Presse-papiers vide : egui n'a émis aucun `Paste`, seul le
            // relâchement de V arrive encore.
            Event::Key { key: Key::V, pressed: false, modifiers, .. } => {
                if !seen && modifiers.command_only() {
                    request = Some(Fallback::default());
                }
                seen = false;
            }
            _ => {}
        }
    }
    ctx.data_mut(|d| d.insert_temp(id, seen));
    // Un champ de saisie qui a le focus garde le collage de texte.
    request.filter(|_| enabled && !ctx.egui_wants_keyboard_input())
}

/// Confirmation avant qu'un Ctrl+V remplace une image aux mesures non
/// enregistrées. `Some(true)` : coller (bouton ou Entrée) ; `Some(false)` :
/// renoncer (bouton, Échap ou clic hors de la boîte) ; `None` : en attente.
pub fn confirm_replace(ctx: &egui::Context) -> Option<bool> {
    use meteroscope_i18n::tr;
    let mut answer = None;
    let modal = egui::Modal::new(egui::Id::unique("meteroscope_confirmer_coller")).show(ctx, |ui| {
        ui.set_width(360.0);
        ui.heading(tr!("paste.confirm_title"));
        ui.add_space(6.0);
        ui.label(tr!("paste.confirm_text"));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            let ok = ui.button(tr!("paste.confirm_ok"));
            if ok.clicked() || ui.input(|i| i.key_pressed(Key::Enter)) {
                answer = Some(true);
            }
            if ui.button(tr!("common.cancel")).clicked() {
                answer = Some(false);
            }
        });
    });
    if answer.is_none() && modal.should_close() {
        answer = Some(false);
    }
    answer
}

thread_local! {
    /// Connexion au presse-papiers, ouverte au premier collage.
    static CLIPBOARD: RefCell<Option<arboard::Clipboard>> = const { RefCell::new(None) };
}

/// Lit le presse-papiers et choisit quoi ouvrir.
pub fn read(fallback: Fallback) -> Pasted {
    let (files, image, text) = CLIPBOARD.with_borrow_mut(|slot| {
        if slot.is_none() {
            *slot = arboard::Clipboard::new().ok();
        }
        match slot {
            Some(c) => (
                c.get().file_list().unwrap_or_default(),
                c.get().image().ok().and_then(|i| image_from_rgba(i.width, i.height, i.bytes.into_owned(), cfg!(windows))),
                c.get().text().ok(),
            ),
            None => (Vec::new(), None, None),
        }
    });
    let image = image.or(fallback.image);
    let text = text.or(fallback.text);
    choose(&files, image, text.as_deref())
}

/// Ordre de préférence : fichier de la liste copiée par un gestionnaire de
/// fichiers, puis image, puis chemin écrit dans le texte.
pub fn choose(files: &[PathBuf], image: Option<RgbaImage>, text: Option<&str>) -> Pasted {
    if let Some(p) = first_openable(files) {
        return Pasted::File(p.to_owned());
    }
    if let Some(img) = image {
        return Pasted::Image(img);
    }
    let from_text = text.map(paths_from_text).unwrap_or_default();
    match first_openable(&from_text) {
        Some(p) => Pasted::File(p.to_owned()),
        None => Pasted::Nothing,
    }
}

/// Premier chemin que l'application sait ouvrir (image ou projet).
pub fn first_openable(paths: &[PathBuf]) -> Option<&Path> {
    paths.iter().map(PathBuf::as_path).find(|p| {
        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        ext == io::PROJECT_EXTENSION || io::IMAGE_EXTENSIONS.contains(&ext.as_str())
    })
}

/// Image RGBA brute (non prémultipliée). Avec `no_alpha_fix` (Windows, où un
/// bitmap 32 bits sans canal alpha arrive entièrement transparent), une
/// image entièrement transparente est rendue opaque ; ailleurs, l'alpha
/// d'un PNG est conservé tel quel.
pub fn image_from_rgba(width: usize, height: usize, mut bytes: Vec<u8>, no_alpha_fix: bool) -> Option<RgbaImage> {
    if width == 0 || height == 0 || bytes.len() != width.checked_mul(height)?.checked_mul(4)? {
        return None;
    }
    if no_alpha_fix && bytes.as_chunks::<4>().0.iter().all(|px| px[3] == 0) {
        bytes.as_chunks_mut::<4>().0.iter_mut().for_each(|px| px[3] = 255);
    }
    RgbaImage::from_raw(width.try_into().ok()?, height.try_into().ok()?, bytes)
}

/// Chemins absolus contenus dans un texte : une ligne par chemin, sous forme
/// d'URI `file://` (`text/uri-list`) ou de chemin brut, éventuellement entre
/// guillemets (« Copier en tant que chemin » de Windows).
pub fn paths_from_text(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|line| {
            let line = line.strip_prefix('"').and_then(|l| l.strip_suffix('"')).unwrap_or(line);
            let path = match line.strip_prefix("file://") {
                Some(rest) => path_from_file_uri(rest, cfg!(windows))?,
                None => PathBuf::from(line),
            };
            path.is_absolute().then_some(path)
        })
        .collect()
}

/// Partie d'une URI `file://` après le schéma : hôte (vide ou `localhost`),
/// puis chemin avec échappements `%XX`.
fn path_from_file_uri(rest: &str, windows: bool) -> Option<PathBuf> {
    let path = match rest.find('/') {
        Some(0) => rest,
        Some(i) if rest[..i].eq_ignore_ascii_case("localhost") => &rest[i..],
        _ => return None,
    };
    let decoded = percent_decode(path)?;
    // `file:///C:/dossier` : la barre initiale précède la lettre de lecteur.
    let b = decoded.as_bytes();
    if windows && b.len() >= 3 && b[0] == b'/' && b[1].is_ascii_alphabetic() && b[2] == b':' {
        return Some(PathBuf::from(&decoded[1..]));
    }
    Some(PathBuf::from(decoded))
}

/// Décode les échappements `%XX` (octets UTF-8) ; un `%` mal formé reste tel quel.
fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |j: usize| b.get(j).and_then(|&c| (c as char).to_digit(16));
        if b[i] == b'%'
            && let (Some(h), Some(l)) = (hex(i + 1), hex(i + 2))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn rgba_brut() {
        let img = image_from_rgba(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8], false).unwrap();
        assert_eq!(img.dimensions(), (2, 1));
        assert_eq!(img.get_pixel(1, 0).0, [5, 6, 7, 8]);
    }

    #[test]
    fn rgba_taille_incoherente() {
        assert!(image_from_rgba(2, 2, vec![0; 12], false).is_none());
        assert!(image_from_rgba(0, 0, vec![], false).is_none());
        assert!(image_from_rgba(usize::MAX, 2, vec![], false).is_none());
    }

    #[test]
    fn rgba_sans_alpha_devient_opaque() {
        let img = image_from_rgba(2, 1, vec![10, 20, 30, 0, 40, 50, 60, 0], true).unwrap();
        assert_eq!(img.get_pixel(0, 0).0, [10, 20, 30, 255]);
        // Une transparence partielle est conservée.
        let img = image_from_rgba(2, 1, vec![10, 20, 30, 0, 40, 50, 60, 128], true).unwrap();
        assert_eq!(img.get_pixel(0, 0).0[3], 0);
    }

    #[test]
    fn png_transparent_conserve() {
        // Hors Windows, une image entièrement transparente reste transparente.
        let img = image_from_rgba(1, 1, vec![10, 20, 30, 0], false).unwrap();
        assert_eq!(img.get_pixel(0, 0).0, [10, 20, 30, 0]);
    }

    #[cfg(unix)]
    #[test]
    fn uri_list_avec_echappements() {
        let text = "# commentaire\r\nfile:///home/moi/Images/plan%20%C3%A9t%C3%A9.png\r\nfile://localhost/tmp/b.jpg\r\n";
        assert_eq!(paths_from_text(text), vec![p("/home/moi/Images/plan été.png"), p("/tmp/b.jpg")]);
    }

    #[cfg(windows)]
    #[test]
    fn uri_list_windows() {
        let text = "file:///C:/Users/moi/plan%20%C3%A9t%C3%A9.png\r\nfile://localhost/D:/b.jpg\r\n";
        assert_eq!(paths_from_text(text), vec![p("C:/Users/moi/plan été.png"), p("D:/b.jpg")]);
    }

    #[test]
    fn uri_hote_distant_ignoree() {
        assert!(paths_from_text("file://serveur/partage/a.png").is_empty());
    }

    #[test]
    fn pourcent_mal_forme_conserve() {
        assert_eq!(percent_decode("/a%2"), Some("/a%2".to_owned()));
        assert_eq!(percent_decode("/a%zz%41"), Some("/a%zzA".to_owned()));
        assert_eq!(percent_decode("/%ff"), None);
    }

    #[test]
    fn lettre_de_lecteur_windows() {
        assert_eq!(path_from_file_uri("/C:/Users/moi/a%20b.png", true), Some(p("C:/Users/moi/a b.png")));
        assert_eq!(path_from_file_uri("/C:/x.png", false), Some(p("/C:/x.png")));
    }

    #[cfg(unix)]
    #[test]
    fn chemin_brut_et_guillemets() {
        assert_eq!(paths_from_text("  /home/moi/a.png  "), vec![p("/home/moi/a.png")]);
        assert_eq!(paths_from_text("\"/home/moi/b c.png\""), vec![p("/home/moi/b c.png")]);
        // Texte ordinaire, chemin relatif et format « copy » de Nautilus.
        assert_eq!(paths_from_text("copy\nfile:///home/moi/a.png"), vec![p("/home/moi/a.png")]);
        assert!(paths_from_text("Bonjour le monde").is_empty());
        assert!(paths_from_text("images/a.png").is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn chemin_windows_entre_guillemets() {
        assert_eq!(paths_from_text("\"C:\\Users\\moi\\a.png\""), vec![p("C:\\Users\\moi\\a.png")]);
    }

    #[test]
    fn premier_fichier_ouvrable() {
        let list = [p("/a/notes.txt"), p("/a/photo.JPG"), p("/a/b.meteroscope")];
        assert_eq!(first_openable(&list), Some(Path::new("/a/photo.JPG")));
        assert_eq!(first_openable(&list[2..]), Some(Path::new("/a/b.meteroscope")));
        assert_eq!(first_openable(&list[..1]), None);
    }

    #[test]
    fn ordre_de_preference() {
        let img = || image_from_rgba(1, 1, vec![1, 2, 3, 255], false);
        let files = [p("/a/photo.png")];
        assert_eq!(choose(&files, img(), Some("/a/autre.png")), Pasted::File(p("/a/photo.png")));
        assert_eq!(choose(&[p("/a/x.txt")], img(), None), Pasted::Image(img().unwrap()));
        #[cfg(unix)]
        assert_eq!(choose(&[], None, Some("file:///a/autre.png")), Pasted::File(p("/a/autre.png")));
        assert_eq!(choose(&[], None, Some("du texte")), Pasted::Nothing);
        assert_eq!(choose(&[], None, None), Pasted::Nothing);
    }
}

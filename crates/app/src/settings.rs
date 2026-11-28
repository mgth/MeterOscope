//! Réglages retenus d'un lancement à l'autre : fichiers récents, dernier
//! dossier, unité d'échelle, nombre de décimales. Un fichier JSON du dossier
//! de configuration, lu avec tolérance (fichier absent ou abîmé : valeurs par
//! défaut, champ par champ).

use egui::{ComboBox, Ui};
use meteroscope_core::measure::{MAX_DECIMALS, format_number_with};
use meteroscope_i18n::tr;
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Nombre de fichiers récents retenus.
pub const MAX_RECENT: usize = 10;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Settings {
    /// Images et projets ouverts, les plus récents d'abord, sans doublon.
    pub recent: Vec<PathBuf>,
    /// Dernier dossier où un fichier a été ouvert ou enregistré.
    pub last_dir: Option<PathBuf>,
    /// Dernière unité saisie dans le dialogue d'échelle.
    pub calibration_unit: Option<String>,
    /// Décimales des valeurs affichées (`None` : automatique).
    pub decimals: Option<u8>,
}

fn settings_file() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("meteroscope").join("settings.json"))
}

/// Fichier de la taille et de la position de la fenêtre (tenu par eframe).
pub fn window_file() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("meteroscope").join("window.ron"))
}

/// Lit un champ, ou `None` s'il manque ou n'a pas le bon type.
fn field<T: serde::de::DeserializeOwned>(obj: &Value, key: &str) -> Option<T> {
    serde_json::from_value(obj.get(key)?.clone()).ok()
}

impl Settings {
    /// Analyse le contenu du fichier ; ce qui est illisible prend la valeur par défaut.
    pub fn parse(text: &str) -> Self {
        let obj: Value = serde_json::from_str(text).unwrap_or(Value::Null);
        let mut s = Self::default();
        for p in field::<Vec<PathBuf>>(&obj, "recent").unwrap_or_default().into_iter().rev() {
            push_recent(&mut s.recent, p);
        }
        s.last_dir = field(&obj, "last_dir");
        s.calibration_unit = field::<String>(&obj, "calibration_unit").map(|u| u.trim().to_owned()).filter(|u| !u.is_empty());
        s.decimals = field::<u8>(&obj, "decimals").filter(|&d| d <= MAX_DECIMALS);
        s
    }

    /// Échoue si un chemin n'est pas représentable en JSON (non UTF-8).
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }

    pub fn load_from(file: &Path) -> Self {
        std::fs::read_to_string(file).map(|t| Self::parse(&t)).unwrap_or_default()
    }

    /// Écrit dans un fichier voisin puis le renomme : une coupure ne laisse pas un fichier tronqué.
    /// Si les réglages ne peuvent pas être mis en JSON, le fichier existant reste intact.
    pub fn save_to(&self, file: &Path) -> std::io::Result<()> {
        let json = self.to_json()?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = file.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, file)
    }

    /// Réglages du dossier de configuration de l'utilisateur.
    pub fn load() -> Self {
        settings_file().map(|f| Self::load_from(&f)).unwrap_or_default()
    }

    /// Enregistre dans le dossier de configuration (les erreurs sont ignorées :
    /// un réglage perdu n'empêche pas de travailler).
    pub fn save(&self) {
        if let Some(f) = settings_file() {
            let _ = self.save_to(&f);
        }
    }

    /// Retient un fichier ouvert ou enregistré (récents et dernier dossier), puis enregistre.
    pub fn add_recent(&mut self, path: &Path) {
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
        self.remember(path);
        self.save();
    }

    /// Met `path` en tête des récents et retient son dossier.
    fn remember(&mut self, path: PathBuf) {
        self.remember_dir_of(&path);
        push_recent(&mut self.recent, path);
    }

    /// Retient le dossier d'un fichier enregistré (export), puis enregistre.
    pub fn set_last_dir_of(&mut self, path: &Path) {
        if self.remember_dir_of(path) {
            self.save();
        }
    }

    /// Retient le dossier de `path` ; renvoie `true` s'il a changé. Un dossier
    /// vide ou non UTF-8 (non enregistrable en JSON) est ignoré.
    fn remember_dir_of(&mut self, path: &Path) -> bool {
        let dir = path.parent().filter(|d| d.to_str().is_some_and(|s| !s.is_empty())).map(Path::to_owned);
        let changed = dir.is_some() && dir != self.last_dir;
        if changed {
            self.last_dir = dir;
        }
        changed
    }

    /// Dossier proposé par les dialogues, s'il existe encore.
    pub fn dialog_dir(&self) -> Option<&Path> {
        self.last_dir.as_deref().filter(|d| d.is_dir())
    }

    /// Retient l'unité d'échelle si elle a changé.
    pub fn set_calibration_unit(&mut self, unit: &str) {
        let unit = Some(unit.trim().to_owned()).filter(|u| !u.is_empty());
        if unit.is_some() && unit != self.calibration_unit {
            self.calibration_unit = unit;
            self.save();
        }
    }
}

/// Place `path` en tête de la liste, sans doublon, au plus [`MAX_RECENT`].
/// Les chemins non UTF-8 (non enregistrables en JSON) sont ignorés.
pub fn push_recent(list: &mut Vec<PathBuf>, path: PathBuf) {
    if path.to_str().is_none_or(str::is_empty) {
        return;
    }
    list.retain(|p| *p != path);
    list.insert(0, path);
    list.truncate(MAX_RECENT);
}

/// Récents encore présents sur le disque (`exists` : test d'existence).
pub fn existing_recent(list: &[PathBuf], exists: impl Fn(&Path) -> bool) -> Vec<&Path> {
    list.iter().map(PathBuf::as_path).filter(|p| exists(p)).collect()
}

/// Libellé d'un récent : icône selon le type, puis nom du fichier.
fn recent_label(p: &Path) -> String {
    let is_project = p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(meteroscope_io::PROJECT_EXTENSION));
    let icon = if is_project { "📏" } else { "🖼" };
    format!("{icon}  {}", p.file_name().map_or_else(|| p.to_string_lossy(), |n| n.to_string_lossy()))
}

/// Boutons des fichiers récents ; renvoie celui qui a été cliqué.
fn recent_buttons(ui: &mut Ui, settings: &Settings) -> Option<PathBuf> {
    let mut picked = None;
    for p in existing_recent(&settings.recent, Path::exists) {
        if ui.button(recent_label(p)).on_hover_text(p.to_string_lossy()).clicked() {
            picked = Some(p.to_owned());
        }
    }
    picked
}

/// Fichiers récents (liste et « effacer »), dans le menu du bouton
/// « Ouvrir » ; renvoie le fichier choisi.
pub fn recent_items(ui: &mut Ui, settings: &mut Settings) -> Option<PathBuf> {
    if existing_recent(&settings.recent, Path::exists).is_empty() {
        ui.weak(tr!("recent.none"));
        return None;
    }
    let picked = recent_buttons(ui, settings);
    ui.separator();
    if ui.button(tr!("recent.clear")).clicked() {
        settings.recent.clear();
        settings.save();
    }
    picked
}

/// Liste des récents de l'écran d'accueil ; renvoie le fichier choisi.
pub fn welcome_recent(ui: &mut Ui, settings: &Settings) -> Option<PathBuf> {
    if existing_recent(&settings.recent, Path::exists).is_empty() {
        return None;
    }
    ui.add_space(24.0);
    ui.label(egui::RichText::new(tr!("recent.title")).strong());
    ui.add_space(4.0);
    recent_buttons(ui, settings)
}

/// Libellé d'un choix de décimales, p. ex. « 0,00 » ou « Automatique ».
fn decimals_text(d: Option<u8>) -> String {
    match d {
        None => tr!("decimals.auto").to_owned(),
        Some(d) => format_number_with(0.0, Some(d)),
    }
}

/// Liste déroulante du nombre de décimales (barre d'état). Applique et enregistre le choix.
pub fn decimals_selector(ui: &mut Ui, settings: &mut Settings) {
    let current = settings.decimals;
    let mut wanted = current;
    ComboBox::from_id_salt("meteroscope_decimals")
        .selected_text(tr!("decimals.label", value = decimals_text(current)))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut wanted, None, decimals_text(None));
            ui.separator();
            for d in 0..=MAX_DECIMALS {
                ui.selectable_value(&mut wanted, Some(d), decimals_text(Some(d)));
            }
        })
        .response
        .on_hover_text(tr!("decimals.tip"));
    if wanted != current {
        settings.decimals = wanted;
        meteroscope_core::measure::set_decimals(wanted);
        settings.save();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn liste_des_recents() {
        let mut l = Vec::new();
        for i in 0..12 {
            push_recent(&mut l, p(&format!("/img/{i}.jpg")));
        }
        assert_eq!(l.len(), MAX_RECENT);
        assert_eq!(l[0], p("/img/11.jpg"));
        assert_eq!(l[9], p("/img/2.jpg"));
        // Un fichier rouvert remonte en tête sans doublon.
        push_recent(&mut l, p("/img/5.jpg"));
        assert_eq!((l.len(), l[0].as_path(), l[1].as_path()), (MAX_RECENT, Path::new("/img/5.jpg"), Path::new("/img/11.jpg")));
        assert_eq!(l.iter().filter(|x| **x == p("/img/5.jpg")).count(), 1);
        push_recent(&mut l, p(""));
        assert_eq!(l[0], p("/img/5.jpg"), "chemin vide ignoré");
        // Les fichiers disparus ne sont pas affichés.
        let shown = existing_recent(&l, |x| x != Path::new("/img/11.jpg"));
        assert_eq!(shown.len(), MAX_RECENT - 1);
        assert!(!shown.contains(&Path::new("/img/11.jpg")));
    }

    #[test]
    fn lecture_tolerante() {
        assert_eq!(Settings::parse(""), Settings::default());
        assert_eq!(Settings::parse("{ pas du json"), Settings::default());
        assert_eq!(Settings::parse("[1, 2]"), Settings::default());
        // Un champ abîmé n'empêche pas de lire les autres.
        let s = Settings::parse(r#"{"recent": 3, "decimals": 2, "calibration_unit": "cm", "inconnu": true}"#);
        assert_eq!((s.recent.len(), s.decimals, s.calibration_unit.as_deref()), (0, Some(2), Some("cm")));
        // Valeurs hors limites ou vides écartées, doublons et excédent retirés.
        let s = Settings::parse(r#"{"decimals": 9, "calibration_unit": "  ", "recent": ["/a", "/b", "/a", "", 5]}"#);
        assert_eq!((s.decimals, s.calibration_unit), (None, None));
        assert_eq!(s.recent, Vec::<PathBuf>::new(), "liste au mauvais type (nombre) ignorée");
        let s = Settings::parse(r#"{"recent": ["/a", "/b", "/a", ""]}"#);
        assert_eq!(s.recent, vec![p("/a"), p("/b")]);
        let many: Vec<String> = (0..15).map(|i| format!("/f{i}")).collect();
        let s = Settings::parse(&serde_json::json!({ "recent": many }).to_string());
        assert_eq!((s.recent.len(), s.recent[0].as_path()), (MAX_RECENT, Path::new("/f0")));
    }

    #[test]
    fn aller_retour_fichier() {
        // Dossier de test propre, jamais le vrai dossier de configuration.
        let dir = std::env::temp_dir().join(format!("meteroscope-settings-test-{}", std::process::id()));
        let file = dir.join("sous-dossier").join("settings.json");
        assert_eq!(Settings::load_from(&file), Settings::default(), "fichier absent");
        let s = Settings {
            recent: vec![p("/photos/été.jpg"), p("/photos/pièce.meteroscope")],
            last_dir: Some(p("/photos")),
            calibration_unit: Some("µm".to_owned()),
            decimals: Some(0),
        };
        s.save_to(&file).unwrap();
        assert_eq!(Settings::load_from(&file), s);
        assert!(!file.with_extension("json.tmp").exists());
        std::fs::write(&file, "\0\0 abîmé").unwrap();
        assert_eq!(Settings::load_from(&file), Settings::default(), "fichier corrompu");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn chemin_non_utf8() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let bad_dir = Path::new("/photos").join(OsStr::from_bytes(b"caf\xe9"));
        let mut s = Settings::default();
        s.remember(p("/photos/a.jpg"));
        // Ni le fichier ni son dossier ne sont retenus ; les réglages restent enregistrables.
        s.remember(bad_dir.join("b.jpg"));
        assert!(!s.remember_dir_of(&bad_dir.join("c.png")));
        assert_eq!((s.recent.clone(), s.last_dir.clone()), (vec![p("/photos/a.jpg")], Some(p("/photos"))));
        assert!(s.to_json().is_ok());

        // Des réglages non enregistrables ne remplacent pas le fichier existant.
        let dir = std::env::temp_dir().join(format!("meteroscope-settings-utf8-{}", std::process::id()));
        let file = dir.join("settings.json");
        s.save_to(&file).unwrap();
        let bad = Settings { last_dir: Some(bad_dir), ..s.clone() };
        assert!(bad.save_to(&file).is_err());
        assert_eq!(Settings::load_from(&file), s);
        assert!(!file.with_extension("json.tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

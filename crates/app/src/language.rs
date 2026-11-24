//! Choix de la langue : celle du système par défaut, ou une langue imposée,
//! retenue dans le dossier de configuration (comme le fait LittleBigMouse).

use egui::{ComboBox, Ui};
use meteroscope_i18n::{LANGUAGES, set_language, system_language, tr};
use std::path::PathBuf;
use std::sync::Mutex;

/// Langue imposée par l'utilisateur (`None` : celle du système).
static CHOICE: Mutex<Option<&'static str>> = Mutex::new(None);

fn pref_file() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("meteroscope").join("language"))
}

fn apply(choice: Option<&'static str>) {
    set_language(choice.unwrap_or(system_language().tag));
    *CHOICE.lock().unwrap_or_else(|e| e.into_inner()) = choice;
}

/// À appeler au démarrage, avant la première image.
pub fn init() {
    let saved = pref_file().and_then(|f| std::fs::read_to_string(f).ok());
    let choice = saved.and_then(|s| LANGUAGES.iter().find(|l| l.tag == s.trim()).map(|l| l.tag));
    apply(choice);
}

fn save(choice: Option<&'static str>) {
    let Some(file) = pref_file() else { return };
    let _ = match choice {
        Some(tag) => file.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&file, tag)),
        None => std::fs::remove_file(&file),
    };
}

/// Liste déroulante des langues (barre d'état).
pub fn selector(ui: &mut Ui) {
    let current = *CHOICE.lock().unwrap_or_else(|e| e.into_inner());
    let auto_label = tr!("language.auto", language = system_language().native_name);
    let shown = match current {
        None => auto_label.clone(),
        Some(tag) => LANGUAGES.iter().find(|l| l.tag == tag).map_or(tag, |l| l.native_name).to_owned(),
    };
    let mut wanted = current;
    ComboBox::from_id_salt("meteroscope_language").selected_text(shown).show_ui(ui, |ui| {
        ui.selectable_value(&mut wanted, None, auto_label);
        ui.separator();
        for l in LANGUAGES {
            ui.selectable_value(&mut wanted, Some(l.tag), l.native_name);
        }
    })
    .response
    .on_hover_text(tr!("language.tip"));
    if wanted != current {
        apply(wanted);
        save(wanted);
    }
}

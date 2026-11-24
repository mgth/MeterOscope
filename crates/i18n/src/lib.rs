//! Traductions de l'interface : mêmes langues et mêmes règles de choix que
//! LittleBigMouse (catalogues JSON clé → texte, anglais en secours).
//!
//! ```ignore
//! ui.label(tr!("panel.measurements"));
//! ui.label(tr!("calib.segment", px = 12.5));   // « Segment tracé : {px} px »
//! ```
//!
//! La langue courante est globale (une seule interface par processus) ; elle
//! se change à chaud, l'interface egui étant redessinée à chaque image.

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Une langue proposée : étiquette BCP 47 et nom dans la langue elle-même.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Language {
    pub tag: &'static str,
    pub native_name: &'static str,
    /// Virgule décimale (sinon point).
    pub decimal_comma: bool,
}

/// Langues prises en charge, l'anglais (secours) en premier.
pub const LANGUAGES: [Language; 10] = [
    Language { tag: "en", native_name: "English", decimal_comma: false },
    Language { tag: "fr", native_name: "Français", decimal_comma: true },
    Language { tag: "de", native_name: "Deutsch", decimal_comma: true },
    Language { tag: "es", native_name: "Español", decimal_comma: true },
    Language { tag: "it", native_name: "Italiano", decimal_comma: true },
    Language { tag: "pl", native_name: "Polski", decimal_comma: true },
    Language { tag: "pt-BR", native_name: "Português (Brasil)", decimal_comma: true },
    Language { tag: "ja", native_name: "日本語", decimal_comma: false },
    Language { tag: "ko", native_name: "한국어", decimal_comma: false },
    Language { tag: "zh-CN", native_name: "简体中文", decimal_comma: false },
];

/// Catalogues, dans l'ordre de [`LANGUAGES`].
const SOURCES: [&str; 10] = [
    include_str!("../locales/en.json"),
    include_str!("../locales/fr.json"),
    include_str!("../locales/de.json"),
    include_str!("../locales/es.json"),
    include_str!("../locales/it.json"),
    include_str!("../locales/pl.json"),
    include_str!("../locales/pt-BR.json"),
    include_str!("../locales/ja.json"),
    include_str!("../locales/ko.json"),
    include_str!("../locales/zh-CN.json"),
];

type Catalog = HashMap<&'static str, &'static str>;

fn catalogs() -> &'static [Catalog] {
    static PARSED: OnceLock<Vec<Catalog>> = OnceLock::new();
    PARSED.get_or_init(|| {
        SOURCES
            .iter()
            .enumerate()
            .map(|(i, src)| {
                let map: serde_json::Map<String, serde_json::Value> = serde_json::from_str(src)
                    .unwrap_or_else(|e| panic!("catalogue {} invalide : {e}", LANGUAGES[i].tag));
                map.into_iter()
                    .filter_map(|(k, v)| {
                        let v = v.as_str()?.to_owned();
                        // Chargés une fois pour toute la durée du programme.
                        Some((&*Box::leak(k.into_boxed_str()), &*Box::leak(v.into_boxed_str())))
                    })
                    .collect()
            })
            .collect()
    })
}

static CURRENT: AtomicUsize = AtomicUsize::new(0);

/// Langue courante.
pub fn language() -> Language {
    LANGUAGES[CURRENT.load(Ordering::Relaxed)]
}

/// Change la langue courante ; `tag` est résolu comme [`resolve`].
pub fn set_language(tag: &str) {
    let lang = resolve(tag);
    let i = LANGUAGES.iter().position(|l| l.tag == lang.tag).unwrap_or(0);
    CURRENT.store(i, Ordering::Relaxed);
}

/// Langue du système, ramenée à une langue prise en charge.
pub fn system_language() -> Language {
    resolve(&sys_locale::get_locale().unwrap_or_default())
}

/// Ramène une étiquette quelconque (« fr-CA », « pt_PT », « zh-Hant-TW »…) à
/// une langue prise en charge, comme LittleBigMouse : sous-étiquette
/// principale, tout le chinois vers le simplifié, anglais par défaut.
pub fn resolve(tag: &str) -> Language {
    let lower = tag.trim().replace('_', "-").to_ascii_lowercase();
    // « fr-FR.UTF-8 », « de_DE@euro » (variables d'environnement POSIX).
    let lower = lower.split(['.', '@']).next().unwrap_or("");
    let primary = lower.split('-').next().unwrap_or("");
    let wanted = match primary {
        "zh" => "zh-CN",
        "pt" => "pt-BR",
        p => p,
    };
    LANGUAGES.iter().copied().find(|l| l.tag.eq_ignore_ascii_case(wanted)).unwrap_or(LANGUAGES[0])
}

/// Texte de `key` dans la langue courante, sinon en anglais, sinon la clé.
pub fn tr(key: &'static str) -> &'static str {
    let cats = catalogs();
    cats[CURRENT.load(Ordering::Relaxed)].get(key).or_else(|| cats[0].get(key)).copied().unwrap_or(key)
}

/// Remplace les paramètres nommés `{nom}` de `template`.
pub fn fill(template: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = template.to_owned();
    for (name, value) in args {
        out = out.replace(&format!("{{{name}}}"), &value.to_string());
    }
    out
}

/// Séparateur décimal de la langue courante.
pub fn decimal_separator() -> char {
    if language().decimal_comma { ',' } else { '.' }
}

/// `tr!("clé")` → `&'static str` ; `tr!("clé", n = 3, nom = x)` → `String`.
#[macro_export]
macro_rules! tr {
    ($key:literal) => {
        $crate::tr($key)
    };
    ($key:literal, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::fill($crate::tr($key), &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),+])
    };
}

#[cfg(test)]
mod tests;

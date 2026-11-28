//! Polices des exports (image annotée, SVG, PDF).
//!
//! Police principale embarquée : Ubuntu Light (licence libre UFL, fournie par
//! egui), qui couvre les écritures latines de l'interface (français, anglais,
//! allemand, espagnol, italien, polonais, portugais). Elle n'a ni idéogrammes,
//! ni kana, ni hangul : pour un caractère qu'elle n'a pas (texte d'annotation
//! ou nom de mesure en japonais, coréen, chinois…), on cherche une police du
//! système qui l'a, avec `fontique` (fontconfig sous Linux, DirectWrite sous
//! Windows), comme le fait l'interface elle-même (eframe `system_fonts`).
//! Sans police adaptée installée, le caractère reste une boîte vide (glyphe
//! `.notdef`) : sous Linux, installer p. ex. `noto-fonts-cjk`.

use ab_glyph::{Font, FontRef};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// Une police chargée : ses octets (fichier entier) et son indice dans une
/// collection `.ttc`, plus la vue `ab_glyph`.
pub struct LoadedFont {
    pub data: &'static [u8],
    pub index: u32,
    pub font: FontRef<'static>,
    /// Nom de famille (système), ou « Ubuntu » pour la police embarquée.
    pub family: String,
}

impl LoadedFont {
    pub fn has(&self, c: char) -> bool {
        self.font.glyph_id(c).0 != 0
    }
}

struct Registry {
    fonts: Vec<&'static LoadedFont>,
    by_char: HashMap<char, usize>,
    system: Option<(fontique::Collection, fontique::SourceCache)>,
}

fn registry() -> &'static Mutex<Registry> {
    static R: OnceLock<Mutex<Registry>> = OnceLock::new();
    R.get_or_init(|| {
        let data = epaint_default_fonts::UBUNTU_LIGHT;
        let font = FontRef::try_from_slice(data).expect("police embarquée valide");
        let main: &'static LoadedFont = Box::leak(Box::new(LoadedFont { data, index: 0, font, family: "Ubuntu".into() }));
        Mutex::new(Registry { fonts: vec![main], by_char: HashMap::new(), system: None })
    })
}

/// Police d'indice `i` (0 : la police principale).
pub fn get(i: usize) -> &'static LoadedFont {
    registry().lock().unwrap_or_else(|e| e.into_inner()).fonts[i]
}

/// Police principale embarquée.
pub fn main() -> &'static LoadedFont {
    get(0)
}

/// Écriture et langues à essayer pour un caractère que la police principale n'a pas.
fn script_of(c: char) -> Option<&'static [u8; 4]> {
    Some(match c as u32 {
        0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7AF => b"Hang",
        0x3040..=0x309F => b"Hira",
        0x30A0..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9F => b"Kana",
        0x2E80..=0x2FDF | 0x3000..=0x303F | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFF65 => {
            b"Hani"
        }
        _ => return None,
    })
}

/// Langues des polices d'idéogrammes, celle de l'interface d'abord (les
/// glyphes des mêmes caractères diffèrent entre chinois, japonais et coréen).
fn han_locales() -> Vec<&'static str> {
    // zh-SG plutôt que zh-CN : fontique prend zh-CN pour la langue Han par défaut.
    let mut v = vec!["zh-SG", "ja", "ko", "zh-TW"];
    let lang = meteroscope_i18n::language().tag;
    let first = if lang.starts_with("ja") {
        "ja"
    } else if lang.starts_with("ko") {
        "ko"
    } else {
        "zh-SG"
    };
    v.retain(|l| *l != first);
    v.insert(0, first);
    v
}

/// Cherche dans les polices du système une police qui a le caractère `c`.
fn find_system(reg: &mut Registry, c: char) -> Option<LoadedFont> {
    use fontique::{FallbackKey, GenericFamily, Language, QueryFamily, QueryStatus, Script};
    let (collection, cache) = reg.system.get_or_insert_with(|| {
        let coll = fontique::Collection::new(fontique::CollectionOptions { shared: false, system_fonts: true });
        (coll, fontique::SourceCache::new(Default::default()))
    });
    let mut keys: Vec<Option<FallbackKey>> = match script_of(c) {
        Some(b"Hani") => han_locales()
            .into_iter()
            .map(|l| Some(FallbackKey::new(Script::from_bytes(*b"Hani"), Language::parse(l).ok().as_ref())))
            .collect(),
        Some(s) => vec![Some(FallbackKey::new(Script::from_bytes(*s), None))],
        None => vec![],
    };
    // En dernier recours : les familles génériques (symboles, autres écritures).
    keys.push(None);
    for key in keys {
        let mut found = None;
        let mut q = collection.query(cache);
        match &key {
            Some(k) => {
                q.set_families([QueryFamily::Generic(GenericFamily::SansSerif)]);
                q.set_fallbacks(*k);
            }
            None => q.set_families(
                [GenericFamily::SansSerif, GenericFamily::SystemUi, GenericFamily::Serif].map(QueryFamily::Generic),
            ),
        }
        q.matches_with(|f| match FontRef::try_from_slice_and_index(f.blob.as_ref(), f.index) {
            Ok(font) if font.glyph_id(c).0 != 0 => {
                found = Some((f.blob.clone(), f.index, f.family.0));
                QueryStatus::Stop
            }
            _ => QueryStatus::Continue,
        });
        drop(q);
        if let Some((blob, index, fam)) = found {
            // Gardée jusqu'à la fin du programme (quelques polices au plus).
            let blob: &'static fontique::Blob<u8> = Box::leak(Box::new(blob));
            let font = FontRef::try_from_slice_and_index(blob.as_ref(), index).ok()?;
            let family = collection.family_name(fam).unwrap_or("sans-serif").to_owned();
            return Some(LoadedFont { data: blob.as_ref(), index, font, family });
        }
    }
    None
}

/// Indice de la police à utiliser pour `c` : la principale si elle l'a,
/// sinon une police du système (cherchée une fois par caractère).
pub fn index_for(c: char) -> usize {
    let main = main();
    if c.is_control() || main.has(c) {
        return 0;
    }
    let mut reg = registry().lock().unwrap_or_else(|e| e.into_inner());
    if let Some(&i) = reg.by_char.get(&c) {
        return i;
    }
    // Une police déjà chargée qui l'a ?
    let i = match reg.fonts.iter().position(|f| f.has(c)) {
        Some(i) => i,
        None => match find_system(&mut reg, c) {
            Some(f) => {
                // Même fichier déjà chargé (autre caractère) : on le réutilise.
                match reg.fonts.iter().position(|g| g.family == f.family && g.index == f.index && g.data.len() == f.data.len()) {
                    Some(i) => i,
                    None => {
                        reg.fonts.push(Box::leak(Box::new(f)));
                        reg.fonts.len() - 1
                    }
                }
            }
            None => 0,
        },
    };
    reg.by_char.insert(c, i);
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn principale_et_repli() {
        meteroscope_i18n::set_language("fr");
        assert_eq!(index_for('é'), 0);
        // Repli système : dépend des polices installées (noto-fonts-cjk ici).
        let i = index_for('日');
        if i == 0 {
            eprintln!("aucune police CJK sur ce système : repli non vérifié");
            return;
        }
        assert!(get(i).has('日'));
        let k = index_for('한');
        assert!(k != 0 && get(k).has('한'));
    }
}

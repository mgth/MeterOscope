use super::*;
use std::collections::BTreeSet;
use std::path::Path;

fn placeholders(s: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut rest = s;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else { break };
        out.insert(after[..end].to_owned());
        rest = &after[end + 1..];
    }
    out
}

#[test]
fn catalogs_have_same_keys_and_placeholders() {
    let cats = catalogs();
    let en = &cats[0];
    for (lang, cat) in LANGUAGES.iter().zip(cats) {
        let missing: Vec<_> = en.keys().filter(|k| !cat.contains_key(*k)).collect();
        let extra: Vec<_> = cat.keys().filter(|k| !en.contains_key(*k)).collect();
        assert!(missing.is_empty() && extra.is_empty(), "{} : manquantes {missing:?}, en trop {extra:?}", lang.tag);
        for (k, v) in cat {
            assert!(!v.trim().is_empty(), "{} : « {k} » vide", lang.tag);
            assert_eq!(placeholders(v), placeholders(en[k]), "{} : paramètres de « {k} »", lang.tag);
        }
    }
}

/// Clés citées dans le code par `tr!("…")`.
fn used_keys() -> BTreeSet<String> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut keys = BTreeSet::new();
    let mut stack = vec![crates.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap().flatten() {
            let p = entry.path();
            if p.is_dir() {
                if !p.ends_with("target") && !p.ends_with("target-tests") {
                    stack.push(p);
                }
            } else if p.extension().is_some_and(|e| e == "rs") && !p.starts_with(crates.join("i18n")) {
                let text = std::fs::read_to_string(&p).unwrap();
                // Les commentaires (exemples dans la doc) ne comptent pas.
                for line in text.lines().filter(|l| !l.trim_start().starts_with("//")) {
                    for (i, _) in line.match_indices("tr!(") {
                        if let Some(rest) = line[i + 4..].trim_start().strip_prefix('"') {
                            keys.insert(rest[..rest.find('"').unwrap()].to_owned());
                        }
                    }
                }
            }
        }
    }
    keys
}

#[test]
fn every_used_key_exists_and_every_key_is_used() {
    let en = &catalogs()[0];
    let used = used_keys();
    let missing: Vec<_> = used.iter().filter(|k| !en.contains_key(k.as_str())).collect();
    assert!(missing.is_empty(), "clés absentes de en.json : {missing:?}");
    let unused: Vec<_> = en.keys().filter(|k| !used.contains(**k)).collect();
    assert!(unused.is_empty(), "clés inutilisées : {unused:?}");
}

#[test]
fn resolve_like_lbm() {
    let r = |t| resolve(t).tag;
    assert_eq!(r("fr-CA"), "fr");
    assert_eq!(r("fr_FR.UTF-8"), "fr");
    assert_eq!(r("de_DE@euro"), "de");
    assert_eq!(r("pt-PT"), "pt-BR");
    assert_eq!(r("zh-Hant-TW"), "zh-CN");
    assert_eq!(r("zh"), "zh-CN");
    assert_eq!(r("ja-JP"), "ja");
    assert_eq!(r("ko"), "ko");
    assert_eq!(r("ru-RU"), "en");
    assert_eq!(r(""), "en");
}

#[test]
fn fill_named_parameters() {
    assert_eq!(fill("{a} × {b} px", &[("a", &3), ("b", &"4")]), "3 × 4 px");
}

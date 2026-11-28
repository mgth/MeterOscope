//! Projet avec annotations et groupes ; lecture d'un projet de la version 1.1.0.

use meteroscope_core::{Document, Pt, Shape, ShapeKind};
use meteroscope_io::*;
use std::path::{Path, PathBuf};

fn test_dir(name: &str) -> PathBuf {
    let d = std::path::absolute(Path::new(env!("CARGO_MANIFEST_DIR")).join("target-tests").join(name)).unwrap();
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn cleanup(d: &Path) {
    let _ = std::fs::remove_dir_all(d);
    if let Some(p) = d.parent() {
        let _ = std::fs::remove_dir(p);
    }
}

/// Projet écrit par MeterOscope 1.1.0 : ni groupes, ni annotations.
const OLD_PROJECT: &str = r#"{
  "format": "meteroscope",
  "version": 1,
  "image": "photo.png",
  "document": {
    "measurements": [
      {"id": 1, "name": "Longueur 1", "shape": {"Length": {"a": {"x": 0.0, "y": 0.0}, "b": {"x": 30.0, "y": 40.0}}},
       "color": [255, 214, 10], "visible": true},
      {"id": 2, "name": "Marqueur 1", "shape": {"Marker": {"p": {"x": 5.0, "y": 6.0}}},
       "color": [191, 90, 242], "visible": false}
    ],
    "calibration": {"scale": {"units_per_px": 0.1, "unit": "mm"}},
    "next_id": 2,
    "counters": {"Length": 1, "Marker": 1}
  }
}
"#;

#[test]
fn ancien_projet_lisible() {
    meteroscope_i18n::set_language("fr");
    let d = test_dir("ancien_projet");
    let path = d.join("old.meteroscope");
    std::fs::write(&path, OLD_PROJECT).unwrap();
    let p = load_project(&path).unwrap();
    let doc = &p.document;
    assert_eq!(doc.measurements.len(), 2);
    assert!(doc.measurements.iter().all(|m| m.group.is_empty()));
    assert!(doc.groups().is_empty());
    assert_eq!(doc.measurements[0].shape.values(&doc.calibration)[0].value, 5.0);
    // Les numéros continuent : la mesure suivante est la troisième.
    let mut doc = p.document.clone();
    let id = doc.add(Shape::Length { a: Pt::new(0., 0.), b: Pt::new(1., 0.) });
    assert_eq!((id, doc.get(id).unwrap().name.as_str()), (3, "Longueur 2"));
    cleanup(&d);
}

#[test]
fn projet_annotations_et_groupes() {
    meteroscope_i18n::set_language("fr");
    let d = test_dir("annotations_groupes");
    let mut doc = Document::new();
    let a = doc.add(Shape::Length { a: Pt::new(0., 0.), b: Pt::new(10., 0.) });
    let t = doc.add(Shape::Text { p: Pt::new(5., 5.), text: "Défaut — 欠陥 결함".into() });
    let f = doc.add(Shape::Arrow { a: Pt::new(1., 1.), b: Pt::new(9., 9.), text: "ici".into() });
    doc.set_group(&[a, f], "Zone 1");
    doc.get_mut(t).unwrap().visible = false;
    let path = d.join("p.meteroscope");
    save_project(&Project { image_path: None, document: doc.clone() }, &path).unwrap();
    let back = load_project(&path).unwrap().document;
    assert_eq!(back.measurements, doc.measurements);
    assert_eq!(back.groups(), vec!["Zone 1"]);
    assert_eq!(back.get(t).unwrap().shape.kind(), ShapeKind::Text);
    // Les annotations restent hors statistiques.
    assert_eq!(back.summary().len(), 1);
    cleanup(&d);
}

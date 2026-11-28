//! Vérifie sans fenêtre que les panneaux se construisent et réagissent.

use egui::Key;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use meteroscope_core::{Document, Pt, Scale, ScalePresets, Shape, ShapeKind, Tool};
use meteroscope_panels::{CalibrationDialog, PanelOutput, measurements_panel, measurements_panel_with_presets, tool_palette, GroupTools};

fn length(x: f64) -> Shape {
    Shape::Length { a: Pt::new(0.0, 0.0), b: Pt::new(x, 0.0) }
}

fn palette_harness(memory: GroupTools) -> Harness<'static, (Tool, GroupTools)> {
    Harness::new_ui_state(
        |ui, (tool, memory): &mut (Tool, GroupTools)| {
            tool_palette(ui, tool, memory);
        },
        (Tool::Select, memory),
    )
}

#[test]
fn palette_change_l_outil() {
    meteroscope_i18n::set_language("fr");
    let mut harness = palette_harness(GroupTools::default());
    harness.run();
    harness.get_by_label("Longueur").click();
    harness.run();
    assert_eq!(harness.state().0, Tool::Draw(ShapeKind::Length));
    harness.get_by_label("Échelle").click();
    harness.run();
    assert_eq!(harness.state().0, Tool::Calibrate);
}

#[test]
fn palette_groupe_les_outils_semblables() {
    meteroscope_i18n::set_language("fr");
    let mut harness = palette_harness(GroupTools::default());
    harness.run();
    // Seul le premier outil du groupe « Cercles » est visible.
    assert!(harness.query_by_label("Cercle 3 points").is_none());
    // Le clic droit ouvre le volet du groupe sans changer d'outil.
    harness.get_by_label("Cercle").click_secondary();
    harness.run();
    assert_eq!(harness.state().0, Tool::Select);
    harness.get_by_label("Cercle 3 points").click();
    harness.run();
    assert_eq!(harness.state().0, Tool::Draw(ShapeKind::Circle3));
    // Le volet se referme et le bouton du groupe montre désormais l'outil choisi.
    harness.run();
    assert!(harness.query_by_label("Cercle").is_none());
    harness.get_by_label("Sélection").click();
    harness.run();
    harness.get_by_label("Cercle 3 points").click();
    harness.run();
    assert_eq!(harness.state().0, Tool::Draw(ShapeKind::Circle3));
}

#[test]
fn palette_montre_l_outil_retenu_au_lancement() {
    meteroscope_i18n::set_language("fr");
    let mut memory = GroupTools::default();
    memory.remember(Tool::Draw(ShapeKind::Circle3));
    let mut harness = palette_harness(memory.clone());
    harness.run();
    // Le bouton du groupe montre l'outil retenu, pas le premier du groupe.
    assert!(harness.query_by_label("Cercle").is_none());
    harness.get_by_label("Cercle 3 points").click();
    harness.run();
    assert_eq!(harness.state().0, Tool::Draw(ShapeKind::Circle3));
    let circles = meteroscope_panels::tool_group(Tool::Draw(ShapeKind::Circle));
    assert_eq!(harness.state().1.shown(circles), memory.shown(circles));
}

struct PanelState {
    doc: Document,
    out: PanelOutput,
}

fn panel_harness(doc: Document) -> Harness<'static, PanelState> {
    Harness::builder().with_size(egui::vec2(320.0, 700.0)).with_step_dt(0.05).build_ui_state(
        |ui, st: &mut PanelState| {
            let out = measurements_panel(ui, &mut st.doc);
            st.out.start_calibration |= out.start_calibration;
            st.out.copy_table |= out.copy_table;
            st.out.export_csv |= out.export_csv;
        },
        PanelState { doc, out: PanelOutput::default() },
    )
}

#[test]
fn panneau_vide() {
    meteroscope_i18n::set_language("fr");
    let mut harness = panel_harness(Document::new());
    harness.run();
    harness.get_by_label_contains("Choisissez un outil à gauche");
    harness.get_by_label("Définir l'échelle").click();
    harness.run();
    assert!(harness.state().out.start_calibration);
    assert!(!harness.state().doc.can_undo());
}

#[test]
fn liste_selection_visibilite_suppression() {
    meteroscope_i18n::set_language("fr");
    let mut doc = Document::new();
    let a = doc.add(length(30.0));
    let b = doc.add(length(40.0));
    doc.selected = None;
    let mut harness = panel_harness(doc);
    harness.run();

    // Valeurs principales et statistiques affichées.
    harness.get_by_label("30,00 px");
    harness.get_by_label("n = 2");

    // Clic sur la ligne : sélection, et les valeurs détaillées apparaissent.
    harness.get_by_role_and_label(egui::Role::Button, "Longueur 1").click();
    harness.run();
    assert_eq!(harness.state().doc.selected, Some(a));

    // Œil : masque la mesure, avec un seul point d'historique.
    let undo_before = count_undo(&harness.state().doc);
    harness.get_all_by_label("Masquer sur l'image").next().unwrap().click();
    harness.run();
    let doc = &harness.state().doc;
    assert!(!doc.get(a).unwrap().visible);
    assert_eq!(count_undo(doc), undo_before + 1);

    // Croix : suppression de la seconde mesure.
    harness.get_all_by_label("Supprimer").nth(1).unwrap().click();
    harness.run();
    assert!(harness.state().doc.get(b).is_none());

    // Boutons du bas.
    harness.get_by_label("Copier").click();
    harness.run();
    assert!(harness.state().out.copy_table);
    harness.get_by_label("Exporter CSV").click();
    harness.run();
    assert!(harness.state().out.export_csv);
    harness.get_by_label("Tout effacer").click();
    harness.run();
    assert!(harness.state().doc.measurements.is_empty());
    assert!(harness.state().doc.can_undo());
}

#[test]
fn renommer_par_double_clic() {
    meteroscope_i18n::set_language("fr");
    let mut doc = Document::new();
    let a = doc.add(length(10.0));
    let mut harness = panel_harness(doc);
    harness.run();
    let undo_before = count_undo(&harness.state().doc);

    // Double-clic : deux clics rapprochés (pas de 50 ms).
    harness.get_by_role_and_label(egui::Role::Button, "Longueur 1").click();
    harness.step();
    harness.get_by_role_and_label(egui::Role::Button, "Longueur 1").click();
    harness.run();
    // Le champ de saisie a le focus, texte sélectionné : la frappe remplace le nom.
    // Premier champ : le nom (le champ Groupe est dans l'encart des valeurs, dessous).
    harness.query_all_by_role(egui::Role::TextInput).next().unwrap().type_text("Pièce A");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    let doc = &harness.state().doc;
    assert_eq!(doc.get(a).unwrap().name, "Pièce A");
    assert_eq!(count_undo(doc), undo_before + 1);
}

#[test]
fn echelle_definie_puis_retiree() {
    meteroscope_i18n::set_language("fr");
    let mut doc = Document::new();
    doc.calibration.set_from_reference(100.0, 10.0, "mm");
    doc.add(length(50.0));
    let mut harness = panel_harness(doc);
    harness.run();
    harness.get_by_label("1 px = 0,1 mm");
    harness.get_by_label("10 px par mm");
    assert!(harness.get_all_by_label("5,00 mm").count() >= 1);
    harness.get_by_label("Retirer").click();
    harness.run();
    assert!(!harness.state().doc.calibration.is_calibrated());
    assert!(harness.get_all_by_label("50,00 px").count() >= 1);
    harness.get_by_label("Définir l'échelle");
}

struct DialogState {
    dlg: CalibrationDialog,
    doc: Document,
    closed: usize,
}

fn dialog_harness() -> Harness<'static, DialogState> {
    let mut dlg = CalibrationDialog::default();
    dlg.open(250.0);
    Harness::builder().with_size(egui::vec2(600.0, 400.0)).build_ui_state(
        |ui, st: &mut DialogState| {
            if st.dlg.show(ui.ctx(), &mut st.doc) {
                st.closed += 1;
            }
        },
        DialogState { dlg, doc: Document::new(), closed: 0 },
    )
}

#[test]
fn dialogue_saisie_et_entree() {
    meteroscope_i18n::set_language("fr");
    let mut harness = dialog_harness();
    harness.run();
    harness.get_by_label_contains("250,0 px");
    // Le champ a le focus dès l'ouverture : on tape directement.
    harness.get_by_role(egui::Role::TextInput).type_text("12,5");
    harness.run();
    harness.get_by_label("1 px = 0,05 mm");
    harness.key_press(Key::Enter);
    harness.run();
    let st = harness.state();
    assert_eq!(st.closed, 1);
    assert!(!st.dlg.is_open());
    let s = st.doc.calibration.scale.clone().unwrap();
    assert!((s.units_per_px - 0.05).abs() < 1e-12);
    assert_eq!(s.unit, "mm");
}

#[test]
fn dialogue_invalide_puis_echap() {
    meteroscope_i18n::set_language("fr");
    let mut harness = dialog_harness();
    harness.run();
    harness.get_by_role(egui::Role::TextInput).type_text("abc");
    harness.run();
    harness.get_by_label_contains("Nombre invalide");
    harness.key_press(Key::Enter);
    harness.run();
    assert!(harness.state().dlg.is_open(), "saisie invalide : la boîte reste ouverte");
    harness.key_press(Key::Escape);
    harness.run();
    let st = harness.state();
    assert_eq!(st.closed, 1);
    assert!(!st.dlg.is_open() && !st.doc.calibration.is_calibrated() && !st.doc.can_undo());
}

/// Nombre de pas d'annulation disponibles (sans modifier le document d'origine).
fn count_undo(doc: &Document) -> usize {
    let mut d = doc.clone();
    let mut n = 0;
    while d.undo() {
        n += 1;
    }
    n
}

#[test]
fn renommer_puis_echap_annule() {
    meteroscope_i18n::set_language("fr");
    let mut doc = Document::new();
    let a = doc.add(length(10.0));
    let mut harness = panel_harness(doc);
    harness.run();
    let undo_before = count_undo(&harness.state().doc);
    harness.get_by_role_and_label(egui::Role::Button, "Longueur 1").click();
    harness.step();
    harness.get_by_role_and_label(egui::Role::Button, "Longueur 1").click();
    harness.run();
    harness.query_all_by_role(egui::Role::TextInput).next().unwrap().type_text("Autre");
    harness.run();
    harness.key_press(Key::Escape);
    harness.run();
    let doc = &harness.state().doc;
    assert_eq!(doc.get(a).unwrap().name, "Longueur 1");
    assert_eq!(count_undo(doc), undo_before);
    // Reste le seul champ Groupe.
    assert_eq!(harness.query_all_by_role(egui::Role::TextInput).count(), 1);
}

#[test]
fn changer_la_couleur() {
    meteroscope_i18n::set_language("fr");
    let mut doc = Document::new();
    let a = doc.add(length(10.0));
    let mut harness = panel_harness(doc);
    harness.run();
    let before = harness.state().doc.get(a).unwrap().color;
    harness.get_by_label("Couleur").click();
    harness.run();
    // La palette s'ouvre : 8 pastilles ; on prend la dernière (blanc).
    let choices: Vec<_> = harness.get_all_by_label_contains("Couleur ").collect();
    assert_eq!(choices.len(), meteroscope_core::PALETTE.len());
    choices.last().unwrap().click();
    harness.run();
    let after = harness.state().doc.get(a).unwrap().color;
    assert_ne!(before, after);
    assert_eq!(after, meteroscope_core::PALETTE[7]);
}

#[test]
fn selection_multiple_groupe_et_suppression() {
    meteroscope_i18n::set_language("fr");
    let mut doc = Document::new();
    let a = doc.add(length(10.0));
    let b = doc.add(length(20.0));
    let c = doc.add(length(30.0));
    let mut harness = panel_harness(doc);
    harness.run();
    harness.get_by_role_and_label(egui::Role::Button, "Longueur 1").click();
    harness.run();
    harness.get_by_role_and_label(egui::Role::Button, "Longueur 3").click_modifiers(egui::Modifiers::COMMAND);
    harness.run();
    assert_eq!(meteroscope_panels::selection(&harness.ctx, &harness.state().doc), vec![a, c]);
    harness.get_by_label_contains("2 sélectionnées");

    // Groupe pour les deux, en un pas d'annulation.
    let undo_before = count_undo(&harness.state().doc);
    harness.query_all_by_role(egui::Role::TextInput).next().unwrap().focus();
    harness.run();
    harness.query_all_by_role(egui::Role::TextInput).next().unwrap().type_text("Pièce A");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    let d = &harness.state().doc;
    let g = |id| d.get(id).unwrap().group.as_str();
    assert_eq!((g(a), g(b), g(c)), ("Pièce A", "", "Pièce A"));
    assert_eq!(count_undo(d), undo_before + 1);

    // Présentation par groupe : en-tête « Sans groupe » dans la liste et dans les statistiques.
    harness.get_by_label("Par groupe").click();
    harness.run();
    assert_eq!(harness.query_all_by_label("Sans groupe").count(), 2);

    // Maj + clic : plage dans l'ordre affiché (Pièce A : 1, 3 ; puis 2).
    harness.get_by_role_and_label(egui::Role::Button, "Longueur 1").click();
    harness.run();
    harness.get_by_role_and_label(egui::Role::Button, "Longueur 2").click_modifiers(egui::Modifiers::SHIFT);
    harness.run();
    assert_eq!(meteroscope_panels::selection(&harness.ctx, &harness.state().doc).len(), 3);
    harness.get_by_role_and_label(egui::Role::Button, "Supprimer la sélection").click();
    harness.run();
    assert!(harness.state().doc.measurements.is_empty());
    assert!(harness.state_mut().doc.undo());
    assert_eq!(harness.state().doc.measurements.len(), 3);
}

/// Profil d'une barre claire (bords en 30 et 70 px) et son document.
fn profile_state() -> (Document, u64, meteroscope_core::profile::Analysis) {
    use meteroscope_core::profile::{EdgeMethod, analyze};
    let values: Vec<f64> = (0..200).map(|i| if (60..140).contains(&i) { 0.8 } else { 0.2 }).collect();
    let analysis = analyze(&values, 0.5, EdgeMethod::HalfHeight, None, None);
    let mut doc = Document::new();
    let id = doc.add(Shape::profile(Pt::new(0.0, 0.0), Pt::new(99.5, 0.0)));
    if let Shape::Profile { result, .. } = &mut doc.get_mut(id).unwrap().shape {
        *result = analysis.result();
    }
    (doc, id, analysis)
}

#[test]
fn panneau_du_profil_reglages_annulables() {
    meteroscope_i18n::set_language("fr");
    let (doc, id, analysis) = profile_state();
    assert!((analysis.result().unwrap().width_px() - 40.0).abs() < 0.05);
    let undo_before = doc.clone();
    let mut harness = Harness::builder().with_size(egui::vec2(900.0, 230.0)).build_ui_state(
        move |ui, st: &mut (Document, bool)| {
            let out = meteroscope_panels::profile_panel(ui, &mut st.0, id, &analysis);
            st.1 |= out.export_csv;
        },
        (doc, false),
    );
    harness.run();
    // Nom de la mesure, largeur en pixels (sans échelle) et réglages visibles.
    harness.get_by_label("Profil 1");
    harness.get_by_label("40,00 px");
    harness.get_by_label("Mi-hauteur");
    // Décocher la correction d'inclinaison : appliqué, en une étape d'annulation.
    harness.get_by_label("Mesurer perpendiculairement aux bords").click();
    harness.run();
    let params = |d: &Document| match &d.get(id).unwrap().shape {
        Shape::Profile { params, .. } => params.clone(),
        _ => panic!(),
    };
    assert!(!params(&harness.state().0).perpendicular);
    harness.get_by_label("Gradient").click();
    harness.run();
    assert_eq!(params(&harness.state().0).method, meteroscope_core::EdgeMethod::Gradient);
    let st = harness.state_mut();
    assert!(st.0.undo() && st.0.undo());
    assert_eq!(st.0.measurements, undo_before.measurements);
    assert!(!st.1);
    harness.get_by_label("Exporter la courbe…").click();
    harness.run();
    assert!(harness.state().1);
}

struct PresetState {
    doc: Document,
    presets: ScalePresets,
    changed: usize,
}

fn presets_harness(doc: Document, presets: ScalePresets) -> Harness<'static, PresetState> {
    Harness::builder().with_size(egui::vec2(420.0, 700.0)).with_step_dt(0.05).build_ui_state(
        |ui, st: &mut PresetState| {
            let out = measurements_panel_with_presets(ui, &mut st.doc, &mut st.presets);
            if out.presets_changed {
                st.changed += 1;
            }
        },
        PresetState { doc, presets, changed: 0 },
    )
}

fn mm(k: f64) -> Scale {
    Scale { units_per_px: k, unit: "mm".to_owned() }
}

#[test]
fn prereglage_applique_en_un_clic_puis_annule() {
    meteroscope_i18n::set_language("fr");
    let mut presets = ScalePresets::default();
    presets.save("objectif ×10", &mm(0.01));
    presets.save("caméra atelier", &mm(0.2));
    let mut harness = presets_harness(Document::new(), presets);
    harness.run();
    harness.get_by_label("Préréglages").click();
    harness.run();
    harness.get_by_label("caméra atelier").click();
    harness.run();
    let st = harness.state();
    assert_eq!(st.doc.calibration.scale, Some(mm(0.2)));
    assert_eq!(st.changed, 0, "appliquer ne modifie pas la liste");
    // La carte rappelle le nom du préréglage reconnu.
    harness.get_by_label("Préréglage : caméra atelier");
    harness.state_mut().doc.undo();
    harness.run();
    assert!(!harness.state().doc.calibration.is_calibrated());
}

#[test]
fn prereglage_enregistre_renomme_supprime() {
    meteroscope_i18n::set_language("fr");
    let mut doc = Document::new();
    doc.calibration.set_from_reference(100.0, 10.0, "mm");
    let mut harness = presets_harness(doc, ScalePresets::default());
    harness.run();
    harness.get_by_label("Préréglages").click();
    harness.run();
    harness.get_by_label("Aucun préréglage enregistré.");

    // Enregistrer : taper un nom puis Entrée.
    harness.get_by_role(egui::Role::TextInput).click();
    harness.run();
    harness.get_by_role(egui::Role::TextInput).type_text("objectif ×10");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    {
        let st = harness.state();
        assert_eq!(st.presets.presets.len(), 1);
        assert_eq!(st.presets.presets[0].name, "objectif ×10");
        assert_eq!(st.presets.presets[0].scale, mm(0.1));
        assert!(st.changed >= 1);
        assert!(!st.doc.can_undo(), "enregistrer ne touche pas au document");
    }

    // Renommer : crayon, nouveau nom, Entrée.
    harness.get_by_label("Renommer").click();
    harness.run();
    harness.run();
    let changed = harness.state().changed;
    let field = harness.get_all_by_role(egui::Role::TextInput).next().unwrap();
    field.type_text(" Nikon");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    assert_eq!(harness.state().presets.presets[0].name, "objectif ×10 Nikon");
    assert!(harness.state().changed > changed);

    // Option « garder l'échelle ».
    harness.get_by_label("Garder l'échelle pour l'image suivante").click();
    harness.run();
    assert!(harness.state().presets.keep_scale);

    // Supprimer.
    harness.get_by_label("Supprimer ce préréglage").click();
    harness.run();
    assert!(harness.state().presets.presets.is_empty());
}

/// Le panneau, dans un volet latéral redimensionnable, garde sa largeur :
/// le champ « Groupe » de la mesure sélectionnée débordait de quelques
/// pixels, et le volet s'élargissait à chaque image jusqu'à tout occuper.
#[test]
fn panneau_garde_sa_largeur() {
    meteroscope_i18n::set_language("fr");
    let mut doc = Document::new();
    let ids: Vec<u64> = (0..3).map(|i| doc.add(length(10.0 + i as f64))).collect();
    doc.set_group(&ids[..2], "Lot A");
    doc.selected = Some(ids[0]);
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(
        |ui, st: &mut (Document, ScalePresets, Vec<f32>)| {
            let r = egui::Panel::right("mesures").default_size(300.0).show(ui, |ui| {
                measurements_panel_with_presets(ui, &mut st.0, &mut st.1);
            });
            st.2.push(r.response.rect.width());
        },
        (doc, ScalePresets::default(), Vec::new()),
    );
    for _ in 0..30 {
        harness.step();
    }
    let widths = &harness.state().2;
    let last = *widths.last().unwrap();
    assert!((last - widths[0]).abs() < 0.5, "le volet s'est élargi : {} → {last}", widths[0]);
}

/// Changer de document (autre image d'une série, caméra) n'emporte ni la
/// sélection multiple ni le renommage en cours : les identifiants de mesures
/// se recoupent d'un document à l'autre.
#[test]
fn selection_multiple_propre_au_document() {
    meteroscope_i18n::set_language("fr");
    let image = |l: f64| {
        let mut doc = Document::new();
        for i in 1..=3 {
            doc.add(length(l * i as f64));
        }
        doc
    };
    let mut harness = panel_harness(image(10.0));
    harness.run();
    harness.get_by_role_and_label(egui::Role::Button, "Longueur 1").click();
    harness.run();
    harness.get_by_role_and_label(egui::Role::Button, "Longueur 3").click_modifiers(egui::Modifiers::COMMAND);
    harness.run();
    let ids: Vec<u64> = harness.state().doc.measurements.iter().map(|m| m.id).collect();
    assert_eq!(meteroscope_panels::selection(&harness.ctx, &harness.state().doc), vec![ids[0], ids[2]]);

    // Image 2 : mêmes identifiants, la dernière mesure sélectionnée. Suppr
    // (lu par l'application avant le panneau) ne doit viser qu'elle.
    let other = image(15.0);
    let last = other.selected.unwrap();
    assert_eq!(last, ids[2]);
    let first = std::mem::replace(&mut harness.state_mut().doc, other);
    assert_eq!(meteroscope_panels::selection(&harness.ctx, &harness.state().doc), vec![last]);
    harness.run();
    assert_eq!(meteroscope_panels::selection(&harness.ctx, &harness.state().doc), vec![last]);
    assert!(harness.query_by_label_contains("sélectionnées").is_none());

    // Retour à l'image 1 : la sélection multiple n'y revient pas non plus.
    harness.state_mut().doc = first;
    harness.run();
    assert_eq!(meteroscope_panels::selection(&harness.ctx, &harness.state().doc), vec![ids[2]]);

    // Image 2 dont la longueur 1 est identique à celle de l'image 1 (même
    // identifiant, nom et géométrie), la 3 différente : pas confondue non plus.
    harness.get_by_role_and_label(egui::Role::Button, "Longueur 1").click();
    harness.run();
    harness.get_by_role_and_label(egui::Role::Button, "Longueur 3").click_modifiers(egui::Modifiers::COMMAND);
    harness.run();
    assert_eq!(meteroscope_panels::selection(&harness.ctx, &harness.state().doc).len(), 2);
    let mut twin = image(15.0);
    twin.measurements[0].shape = harness.state().doc.measurements[0].shape.clone();
    let first = std::mem::replace(&mut harness.state_mut().doc, twin);
    assert_eq!(meteroscope_panels::selection(&harness.ctx, &harness.state().doc), vec![last]);
    harness.run();
    assert_eq!(meteroscope_panels::selection(&harness.ctx, &harness.state().doc), vec![last]);
    harness.state_mut().doc = first;
    harness.run();

    // Renommage en cours sur l'image 1, puis image 2 : le champ disparaît.
    let fields = harness.query_all_by_role(egui::Role::TextInput).count();
    harness.get_by_role_and_label(egui::Role::Button, "Longueur 2").click();
    harness.step();
    harness.get_by_role_and_label(egui::Role::Button, "Longueur 2").click();
    harness.run();
    assert_eq!(harness.query_all_by_role(egui::Role::TextInput).count(), fields + 1);
    harness.state_mut().doc = image(15.0);
    harness.run();
    assert_eq!(harness.query_all_by_role(egui::Role::TextInput).count(), fields);
}

/// Modifier une mesure de la sélection multiple (glisser une poignée dans la
/// vue) ne passe pas pour un changement de document.
#[test]
fn selection_multiple_survit_a_une_edition() {
    meteroscope_i18n::set_language("fr");
    let mut doc = Document::new();
    let a = doc.add(length(10.0));
    doc.add(length(20.0));
    let c = doc.add(length(30.0));
    let mut harness = panel_harness(doc);
    harness.run();
    harness.get_by_role_and_label(egui::Role::Button, "Longueur 1").click();
    harness.run();
    harness.get_by_role_and_label(egui::Role::Button, "Longueur 3").click_modifiers(egui::Modifiers::COMMAND);
    harness.run();
    for x in [31.0, 32.0, 33.0] {
        harness.state_mut().doc.get_mut(c).unwrap().shape = length(x);
        harness.run();
    }
    harness.state_mut().doc.get_mut(a).unwrap().name = "Bord".into();
    harness.run();
    assert_eq!(meteroscope_panels::selection(&harness.ctx, &harness.state().doc), vec![a, c]);
}

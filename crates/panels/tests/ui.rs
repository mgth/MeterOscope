//! Vérifie sans fenêtre que les panneaux se construisent et réagissent.

use egui::Key;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use meteroscope_core::{Document, Pt, Shape, ShapeKind, Tool};
use meteroscope_panels::{CalibrationDialog, PanelOutput, measurements_panel, tool_palette};

fn length(x: f64) -> Shape {
    Shape::Length { a: Pt::new(0.0, 0.0), b: Pt::new(x, 0.0) }
}

#[test]
fn palette_change_l_outil() {
    let mut harness = Harness::new_ui_state(|ui, tool: &mut Tool| tool_palette(ui, tool), Tool::Select);
    harness.run();
    harness.get_by_label("Longueur").click();
    harness.run();
    assert_eq!(*harness.state(), Tool::Draw(ShapeKind::Length));
    harness.get_by_label("Échelle").click();
    harness.run();
    assert_eq!(*harness.state(), Tool::Calibrate);
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
    harness.get_by_role(egui::Role::TextInput).type_text("Pièce A");
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    let doc = &harness.state().doc;
    assert_eq!(doc.get(a).unwrap().name, "Pièce A");
    assert_eq!(count_undo(doc), undo_before + 1);
}

#[test]
fn echelle_definie_puis_retiree() {
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
    let mut doc = Document::new();
    let a = doc.add(length(10.0));
    let mut harness = panel_harness(doc);
    harness.run();
    let undo_before = count_undo(&harness.state().doc);
    harness.get_by_role_and_label(egui::Role::Button, "Longueur 1").click();
    harness.step();
    harness.get_by_role_and_label(egui::Role::Button, "Longueur 1").click();
    harness.run();
    harness.get_by_role(egui::Role::TextInput).type_text("Autre");
    harness.run();
    harness.key_press(Key::Escape);
    harness.run();
    let doc = &harness.state().doc;
    assert_eq!(doc.get(a).unwrap().name, "Longueur 1");
    assert_eq!(count_undo(doc), undo_before);
    assert!(harness.query_by_role(egui::Role::TextInput).is_none());
}

#[test]
fn changer_la_couleur() {
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

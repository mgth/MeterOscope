//! Tests d'intégration de la vue avec egui_kittest (sans fenêtre).

use crate::{Canvas, CanvasOutput};
use egui::{Color32, ColorImage, Event, Key, Modifiers, PointerButton, Pos2, TextureHandle, TextureOptions, vec2};
use egui_kittest::Harness;
use meteroscope_core::{Document, Pt, Shape, ShapeKind, Tool};

struct St {
    canvas: Canvas,
    doc: Document,
    tex: Option<TextureHandle>,
    outs: Vec<CanvasOutput>,
}

fn harness() -> Harness<'static, St> {
    let st = St { canvas: Canvas::default(), doc: Document::new(), tex: None, outs: Vec::new() };
    let mut h = Harness::builder().with_size(vec2(800.0, 600.0)).with_step_dt(1.0 / 60.0).build_ui_state(
        |ui, st: &mut St| {
            let tex = st
                .tex
                .get_or_insert_with(|| {
                    ui.ctx().load_texture("img", ColorImage::filled([200, 100], Color32::GRAY), TextureOptions::NEAREST)
                })
                .clone();
            let out = st.canvas.show(ui, &mut st.doc, Some((&tex, [200, 100])));
            st.outs.push(out);
        },
        st,
    );
    h.step();
    h
}

/// Position écran d'un point image.
fn scr(h: &Harness<'_, St>, x: f64, y: f64) -> Pos2 {
    h.state().canvas.xf().to_screen(Pt::new(x, y))
}

fn button(h: &mut Harness<'_, St>, pos: Pos2, button: PointerButton, pressed: bool) {
    h.event(Event::PointerMoved(pos));
    h.event(Event::PointerButton { pos, button, pressed, modifiers: Modifiers::NONE });
    h.step();
}

fn click(h: &mut Harness<'_, St>, x: f64, y: f64) {
    let p = scr(h, x, y);
    button(h, p, PointerButton::Primary, true);
    button(h, p, PointerButton::Primary, false);
}

fn drag(h: &mut Harness<'_, St>, from: Pos2, to: Pos2, b: PointerButton) {
    button(h, from, b, true);
    h.event(Event::PointerMoved(from.lerp(to, 0.5)));
    h.event(Event::PointerMoved(to));
    h.step();
    button(h, to, b, false);
}

fn near(a: Pt, b: Pt) -> bool {
    a.dist(b) < 0.05
}

#[test]
fn ajuste_a_la_vue() {
    meteroscope_i18n::set_language("fr");
    let h = harness();
    let c = &h.state().canvas;
    // 200×100 dans environ 800×600 : la largeur limite.
    assert!(c.zoom() > 3.5 && c.zoom() < 4.0, "zoom = {}", c.zoom());
    assert!(c.last_rect.contains(scr(&h, 0.0, 0.0)) && c.last_rect.contains(scr(&h, 200.0, 100.0)));
}

#[test]
fn longueur_clic_clic_et_glisser() {
    meteroscope_i18n::set_language("fr");
    let mut h = harness();
    h.state_mut().canvas.set_tool(Tool::Draw(ShapeKind::Length));
    click(&mut h, 10.0, 10.0);
    assert!(h.state().canvas.is_drawing());
    assert!(h.state().canvas.hint().contains("deuxième"));
    click(&mut h, 60.0, 10.0);
    let st = h.state();
    assert!(!st.canvas.is_drawing());
    assert_eq!(st.doc.measurements.len(), 1);
    assert!(st.outs.iter().any(|o| o.created.is_some()));
    let Shape::Length { a, b } = st.doc.measurements[0].shape else { panic!() };
    assert!(near(a, Pt::new(10.0, 10.0)) && near(b, Pt::new(60.0, 10.0)));
    // L'outil reste actif : presser-glisser-relâcher.
    assert_eq!(st.canvas.tool(), Tool::Draw(ShapeKind::Length));
    let (p, q) = (scr(&h, 20.0, 50.0), scr(&h, 120.0, 80.0));
    drag(&mut h, p, q, PointerButton::Primary);
    assert_eq!(h.state().doc.measurements.len(), 2);
    let Shape::Length { a, b } = h.state().doc.measurements[1].shape else { panic!() };
    assert!(near(a, Pt::new(20.0, 50.0)) && near(b, Pt::new(120.0, 80.0)));
}

#[test]
fn maj_contraint_et_echelle() {
    meteroscope_i18n::set_language("fr");
    let mut h = harness();
    h.state_mut().canvas.set_tool(Tool::Calibrate);
    click(&mut h, 10.0, 10.0);
    let p = scr(&h, 90.0, 14.0);
    h.event(Event::ModifiersChanged(Modifiers::SHIFT));
    for pressed in [true, false] {
        h.event(Event::PointerMoved(p));
        h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers: Modifiers::SHIFT });
        h.step();
    }
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.step();
    let st = h.state();
    assert!(st.doc.measurements.is_empty(), "l'échelle n'ajoute pas de mesure");
    let (a, b) = st.outs.iter().find_map(|o| o.calibration_segment).expect("segment d'étalonnage");
    assert!(near(a, Pt::new(10.0, 10.0)) && near(b, Pt::new(90.0, 10.0)), "{a:?} {b:?}");
    assert_eq!(st.canvas.calibration_segment(), Some((a, b)));
    h.state_mut().canvas.set_tool(Tool::Select);
    assert_eq!(h.state().canvas.calibration_segment(), None);
}

#[test]
fn surface_double_clic_entree_retour_arriere() {
    meteroscope_i18n::set_language("fr");
    let mut h = harness();
    h.state_mut().canvas.set_tool(Tool::Draw(ShapeKind::Area));
    for (x, y) in [(10.0, 10.0), (80.0, 10.0), (80.0, 60.0)] {
        click(&mut h, x, y);
    }
    click(&mut h, 10.0, 60.0);
    h.key_press(Key::Backspace);
    h.step();
    assert_eq!(h.state().canvas.draft.pts.len(), 3);
    h.key_press(Key::Enter);
    h.step();
    assert_eq!(h.state().doc.measurements.len(), 1);
    assert!(matches!(&h.state().doc.measurements[0].shape, Shape::Area { pts } if pts.len() == 3));

    // Double-clic sur le dernier point.
    h.state_mut().canvas.set_tool(Tool::Draw(ShapeKind::Path));
    for (x, y) in [(10.0, 90.0), (50.0, 90.0), (90.0, 70.0), (90.0, 70.0)] {
        click(&mut h, x, y);
    }
    assert!(!h.state().canvas.is_drawing());
    assert!(matches!(&h.state().doc.measurements[1].shape, Shape::Path { pts } if pts.len() == 3));

    // Clic sur le premier point pour fermer la surface.
    h.state_mut().canvas.set_tool(Tool::Draw(ShapeKind::Area));
    for (x, y) in [(110.0, 10.0), (190.0, 10.0), (190.0, 90.0), (110.5, 10.5)] {
        click(&mut h, x, y);
    }
    assert!(matches!(&h.state().doc.measurements[2].shape, Shape::Area { pts } if pts.len() == 3));
}

#[test]
fn selection_poignee_et_deplacement() {
    meteroscope_i18n::set_language("fr");
    let mut h = harness();
    let id = h.state_mut().doc.add(Shape::Length { a: Pt::new(10.0, 10.0), b: Pt::new(60.0, 10.0) });
    h.state_mut().doc.selected = None;
    h.step();
    // Clic dans le vide : rien ; clic sur le trait : sélection.
    click(&mut h, 30.0, 40.0);
    assert_eq!(h.state().doc.selected, None);
    click(&mut h, 30.0, 10.0);
    assert_eq!(h.state().doc.selected, Some(id));
    // Glisser la poignée b.
    let (p, q) = (scr(&h, 60.0, 10.0), scr(&h, 70.0, 30.0));
    drag(&mut h, p, q, PointerButton::Primary);
    let Shape::Length { a, b } = h.state().doc.measurements[0].shape else { panic!() };
    assert!(near(a, Pt::new(10.0, 10.0)) && near(b, Pt::new(70.0, 30.0)), "{b:?}");
    // Glisser le corps : translation.
    let (p, q) = (scr(&h, 40.0, 20.0), scr(&h, 50.0, 25.0));
    drag(&mut h, p, q, PointerButton::Primary);
    let Shape::Length { a, b } = h.state().doc.measurements[0].shape else { panic!() };
    assert!(near(a, Pt::new(20.0, 15.0)) && near(b, Pt::new(80.0, 35.0)), "{a:?} {b:?}");
    // Un point d'annulation par glisser.
    assert!(h.state_mut().doc.undo());
    let Shape::Length { b, .. } = h.state().doc.measurements[0].shape else { panic!() };
    assert!(near(b, Pt::new(70.0, 30.0)));
    // Clic dans le vide : désélection.
    click(&mut h, 150.0, 80.0);
    assert_eq!(h.state().doc.selected, None);
}

#[test]
fn deplacer_la_vue_et_zoomer() {
    meteroscope_i18n::set_language("fr");
    let mut h = harness();
    let before = h.state().canvas.view.pan;
    let from = scr(&h, 100.0, 50.0);
    drag(&mut h, from, from + vec2(30.0, 20.0), PointerButton::Secondary);
    let after = h.state().canvas.view.pan;
    assert!((after - before - vec2(30.0, 20.0)).length() < 0.01);
    assert!(h.state().doc.selected.is_none());
    let z = h.state().canvas.zoom();
    h.state_mut().canvas.zoom_by(2.0);
    assert!((h.state().canvas.zoom() - 2.0 * z).abs() < 1e-4);
    h.state_mut().canvas.zoom_actual();
    assert_eq!(h.state().canvas.zoom(), 1.0);
    h.state_mut().canvas.fit();
    h.step();
    assert!((h.state().canvas.zoom() - z).abs() < 1e-4);
    // Molette sous le curseur : le point visé reste fixe.
    let p = scr(&h, 50.0, 25.0);
    h.event(Event::PointerMoved(p));
    h.step();
    h.event(Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: vec2(0.0, 1.0), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel, modifiers: Modifiers::NONE });
    for _ in 0..30 {
        h.step();
    }
    assert!(h.state().canvas.zoom() > z * 1.05, "zoom {} vs {z}", h.state().canvas.zoom());
    let ip = h.state().canvas.xf().to_image(p);
    assert!(near(ip, Pt::new(50.0, 25.0)), "{ip:?}");
}

#[test]
fn marqueurs_et_annulation() {
    meteroscope_i18n::set_language("fr");
    let mut h = harness();
    h.state_mut().canvas.set_tool(Tool::Draw(ShapeKind::Marker));
    click(&mut h, 10.0, 10.0);
    click(&mut h, 50.0, 50.0);
    assert_eq!(h.state().doc.marker_count(), 2);
    h.state_mut().canvas.set_tool(Tool::Draw(ShapeKind::Angle));
    click(&mut h, 10.0, 10.0);
    assert!(h.state().canvas.is_drawing());
    h.state_mut().canvas.cancel();
    assert!(!h.state().canvas.is_drawing());
    h.step();
    assert_eq!(h.state().doc.measurements.len(), 2);
}

#[test]
fn redresser_puis_mesurer_sur_la_vue_pivotee() {
    meteroscope_i18n::set_language("fr");
    let mut h = harness();
    // Ligne penchée de atan(10/100) ≈ 5,71° dans l'image.
    h.state_mut().canvas.set_tool(Tool::Straighten);
    let (p, q) = (scr(&h, 50.0, 40.0), scr(&h, 150.0, 50.0));
    drag(&mut h, p, q, PointerButton::Primary);
    let expected = -(10f64).atan2(100.0).to_degrees();
    let st = h.state();
    assert!(st.doc.measurements.is_empty(), "redresser n'ajoute pas de mesure");
    assert!((st.doc.rotation - expected).abs() < 1e-6, "rotation = {}", st.doc.rotation);
    assert!(st.outs.iter().any(|o| o.straightened.is_some()));
    // À la frame suivante, la vue est pivotée : la ligne est horizontale à l'écran
    // et l'image entière reste visible (réajustement).
    h.step();
    let (a, b) = (scr(&h, 50.0, 40.0), scr(&h, 150.0, 50.0));
    assert!((a.y - b.y).abs() < 0.01, "{a:?} {b:?}");
    let c = &h.state().canvas;
    for (x, y) in [(0.0, 0.0), (200.0, 0.0), (200.0, 100.0), (0.0, 100.0)] {
        assert!(c.last_rect.contains(scr(&h, x, y)), "coin ({x}, {y}) hors de la vue");
    }
    // Un clic au même endroit de l'écran tombe toujours au bon pixel de l'image.
    h.state_mut().canvas.set_tool(Tool::Draw(ShapeKind::Marker));
    click(&mut h, 120.0, 30.0);
    let Shape::Marker { p: m } = h.state().doc.measurements[0].shape else { panic!() };
    assert!(near(m, Pt::new(120.0, 30.0)), "{m:?}");
    // Un rectangle tracé sur la vue redressée reste droit à l'écran.
    h.state_mut().canvas.set_tool(Tool::Draw(ShapeKind::Rect));
    let p = scr(&h, 60.0, 40.0);
    drag(&mut h, p, p + egui::vec2(50.0, 30.0), PointerButton::Primary);
    let r = h.state().doc.measurements[1].shape.clone();
    let Shape::Rect { angle, .. } = r else { panic!("{r:?}") };
    assert!((angle + expected).abs() < 1e-6, "angle = {angle}");
    let c: Vec<_> = crate::edit::handles(&r, 1.0).into_iter().map(|q| scr(&h, q.x, q.y)).collect();
    assert!((c[0].y - c[1].y).abs() < 0.01 && (c[0].x - c[3].x).abs() < 0.01, "{c:?}");
    assert!((c[1].x - c[0].x - 50.0).abs() < 0.01 && (c[3].y - c[0].y - 30.0).abs() < 0.01, "{c:?}");
    // Redresser est annulable comme le reste.
    h.state_mut().doc.undo();
    h.state_mut().doc.undo();
    h.state_mut().doc.undo();
    h.step();
    assert_eq!(h.state().doc.rotation, 0.0);
    let (a, b) = (scr(&h, 0.0, 0.0), scr(&h, 200.0, 0.0));
    assert!((a.y - b.y).abs() < 0.01);
}

#[test]
fn inclinaison_entre_deux_segments() {
    meteroscope_i18n::set_language("fr");
    let mut h = harness();
    h.state_mut().canvas.set_tool(Tool::Draw(ShapeKind::LineAngle));
    let (p, q) = (scr(&h, 10.0, 10.0), scr(&h, 90.0, 10.0));
    drag(&mut h, p, q, PointerButton::Primary);
    assert!(h.state().canvas.is_drawing());
    assert!(h.state().canvas.hint().contains("second segment"));
    click(&mut h, 100.0, 60.0);
    click(&mut h, 180.0, 80.0);
    let st = h.state();
    assert_eq!(st.doc.measurements.len(), 1);
    let m = &st.doc.measurements[0];
    assert_eq!(m.shape.kind(), ShapeKind::LineAngle);
    let v = m.shape.values(&st.doc.calibration)[0].value;
    assert!((v - (20f64).atan2(80.0).to_degrees()).abs() < 0.1, "{v}");
}

fn type_text(h: &mut Harness<'_, St>, s: &str) {
    h.event(Event::Text(s.to_owned()));
    h.step();
}

fn press_key(h: &mut Harness<'_, St>, key: Key) {
    for pressed in [true, false] {
        h.event(Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE });
    }
    h.step();
    h.step();
}

#[test]
fn texte_saisi_sur_place() {
    meteroscope_i18n::set_language("fr");
    let mut h = harness();
    h.state_mut().canvas.set_tool(Tool::Draw(ShapeKind::Text));
    click(&mut h, 50.0, 40.0);
    h.step();
    assert!(h.state().canvas.is_editing_text() && h.state().doc.measurements.is_empty());
    type_text(&mut h, "Fissure à gauche");
    press_key(&mut h, Key::Enter);
    let st = h.state();
    assert!(!st.canvas.is_editing_text());
    assert_eq!(st.doc.measurements.len(), 1);
    let Shape::Text { p, text } = &st.doc.measurements[0].shape else { panic!() };
    assert!(near(*p, Pt::new(50.0, 40.0)) && text == "Fissure à gauche", "{text:?}");

    // Double-clic (outil Sélection) : modifier ; Échap abandonne.
    h.state_mut().canvas.set_tool(Tool::Select);
    click(&mut h, 50.0, 40.0);
    click(&mut h, 50.0, 40.0);
    h.step();
    assert!(h.state().canvas.is_editing_text());
    type_text(&mut h, "autre");
    press_key(&mut h, Key::Escape);
    assert!(!h.state().canvas.is_editing_text());
    assert_eq!(h.state().doc.measurements[0].shape.annotation_text(), Some("Fissure à gauche"));
}

#[test]
fn fleche_et_texte_facultatif() {
    meteroscope_i18n::set_language("fr");
    let mut h = harness();
    h.state_mut().canvas.set_tool(Tool::Draw(ShapeKind::Arrow));
    click(&mut h, 20.0, 20.0);
    click(&mut h, 80.0, 60.0);
    h.step();
    assert_eq!(h.state().doc.measurements.len(), 1);
    assert!(h.state().canvas.is_editing_text());
    type_text(&mut h, "ici");
    press_key(&mut h, Key::Enter);
    let Shape::Arrow { a, b, text } = &h.state().doc.measurements[0].shape else { panic!() };
    assert!(near(*a, Pt::new(20.0, 20.0)) && near(*b, Pt::new(80.0, 60.0)) && text == "ici");
    // Un seul pas d'annulation pour la flèche et son texte.
    assert!(h.state_mut().doc.undo());
    assert!(h.state().doc.measurements.is_empty() && !h.state().doc.can_undo());
}

#[test]
fn perspective_quatre_coins() {
    meteroscope_i18n::set_language("fr");
    let mut h = harness();
    h.state_mut().canvas.set_tool(Tool::Perspective);
    assert!(h.state().canvas.hint().contains("premier coin"));
    let corners = [Pt::new(30.0, 20.0), Pt::new(160.0, 30.0), Pt::new(150.0, 80.0), Pt::new(20.0, 70.0)];
    for (i, c) in corners.iter().enumerate() {
        assert!(h.state().outs.iter().all(|o| o.perspective_quad.is_none()));
        click(&mut h, c.x, c.y);
        assert_eq!(h.state().canvas.is_drawing(), i < 3, "le quatrième coin termine le tracé");
    }
    // Un double-clic ou Entrée en cours de route ne termine pas le quadrilatère.
    let st = h.state();
    let quad = st.outs.iter().find_map(|o| o.perspective_quad).expect("quadrilatère signalé");
    assert!(quad.iter().zip(&corners).all(|(a, b)| near(*a, *b)), "{quad:?}");
    assert!(st.doc.measurements.is_empty() && !st.doc.can_undo(), "la vue ne modifie pas le document");
    // Le quadrilatère reste affiché pendant le dialogue, puis s'efface.
    assert!(st.canvas.hint().contains("dimensions"));
    h.state_mut().canvas.clear_perspective_quad();
    assert!(h.state().canvas.hint().contains("premier coin"));
    // Échap (annulation) en cours de tracé : on repart de zéro.
    click(&mut h, 30.0, 20.0);
    click(&mut h, 160.0, 30.0);
    h.state_mut().canvas.cancel();
    assert!(!h.state().canvas.is_drawing());
    click(&mut h, 30.0, 20.0);
    h.key_press(Key::Enter);
    h.step();
    assert!(h.state().canvas.is_drawing(), "Entrée ne valide pas un quadrilatère incomplet");
}

#[test]
fn points_d_accroche() {
    use crate::snap::{Snap, SnapKind, find_snap, snap_or};
    meteroscope_i18n::set_language("fr");
    let p = Pt::new;
    let mut doc = Document::new();
    // Rectangle incliné à 30° : ses quatre coins, pas seulement a et b.
    let t = 30f64.to_radians();
    let b = p(10. * (t.cos() - t.sin()), 10. * (t.sin() + t.cos()));
    let rect = doc.add(Shape::Rect { a: p(0., 0.), b, angle: 30.0 });
    let corner = p(10. * t.cos(), 10. * t.sin());
    let s = find_snap(&doc, corner.offset(0.5, 0.5), 2.0, None).unwrap();
    assert_eq!(s.kind, SnapKind::Point);
    assert!(s.pt.dist(corner) < 1e-9, "{s:?}");
    // Milieu d'un côté du rectangle.
    let s = find_snap(&doc, p(0., 0.).lerp(corner, 0.5).offset(0.3, 0.), 2.0, None).unwrap();
    assert_eq!(s.kind, SnapKind::Midpoint);
    // Une mesure ne s'aimante pas sur elle-même ; hors tolérance : rien.
    assert_eq!(find_snap(&doc, corner, 2.0, Some(rect)), None);
    assert_eq!(find_snap(&doc, corner.offset(3., 0.), 2.0, None), None);

    let len = doc.add(Shape::Length { a: p(100., 100.), b: p(140., 100.) });
    let s = find_snap(&doc, p(119., 101.), 2.0, None).unwrap();
    assert_eq!(s, Snap { pt: p(120., 100.), kind: SnapKind::Midpoint });
    assert_eq!(find_snap(&doc, p(141., 99.), 2.0, None).map(|s| s.pt), Some(p(140., 100.)));
    // Mesure masquée : ignorée.
    doc.get_mut(len).unwrap().visible = false;
    assert_eq!(find_snap(&doc, p(141., 99.), 2.0, None), None);

    // Centre des deux types de cercle.
    doc.add(Shape::Circle { center: p(200., 0.), edge: p(210., 0.) });
    doc.add(Shape::Circle3 { a: p(290., 0.), b: p(310., 0.), c: p(300., 10.) });
    assert_eq!(find_snap(&doc, p(201., 1.), 2.0, None), Some(Snap { pt: p(200., 0.), kind: SnapKind::Center }));
    let s = find_snap(&doc, p(300.5, 0.5), 2.0, None).unwrap();
    assert_eq!(s.kind, SnapKind::Center);
    assert!(s.pt.dist(p(300., 0.)) < 1e-9);
    assert_eq!(find_snap(&doc, p(210., 1.), 2.0, None).map(|s| s.kind), Some(SnapKind::Point));

    // Intersection des droites d'une inclinaison, hors des segments.
    doc.add(Shape::LineAngle { a: p(400., 0.), b: p(410., 0.), c: p(400., 20.), d: p(410., 10.) });
    let s = find_snap(&doc, p(419., 1.), 2.0, None).unwrap();
    assert_eq!(s.kind, SnapKind::Intersection);
    assert!(s.pt.dist(p(420., 0.)) < 1e-9, "{s:?}");

    // Centre d'une ellipse et d'un arc.
    doc.add(Shape::Ellipse { center: p(500., 0.), a: p(520., 0.), b: p(500., 10.) });
    assert_eq!(find_snap(&doc, p(501., 1.), 2.0, None), Some(Snap { pt: p(500., 0.), kind: SnapKind::Center }));
    doc.add(Shape::Arc { a: p(590., 50.), m: p(600., 40.), b: p(610., 50.) });
    let s = find_snap(&doc, p(600.5, 50.5), 2.0, None).unwrap();
    assert_eq!(s.kind, SnapKind::Center);
    assert!(s.pt.dist(p(600., 50.)) < 1e-9, "{s:?}");

    // À distance égale, un point l'emporte sur un milieu.
    let mut d2 = Document::new();
    d2.add(Shape::Length { a: p(0., 0.), b: p(20., 0.) });
    d2.add(Shape::Marker { p: p(10., 4.) });
    assert_eq!(find_snap(&d2, p(10., 2.), 3.0, None).map(|s| s.kind), Some(SnapKind::Point));

    // Ordre des contraintes : l'aimantation l'emporte sur la contrainte Maj.
    let snap = Some(Snap { pt: p(5., 5.), kind: SnapKind::Point });
    assert_eq!(snap_or(snap, p(4., 6.), |_| p(4., 0.)), p(5., 5.));
    assert_eq!(snap_or(None, p(4., 6.), |_| p(4., 0.)), p(4., 0.));
}

/// Clic (appui + relâchement) avec des touches de modification maintenues.
fn click_mod(h: &mut Harness<'_, St>, x: f64, y: f64, modifiers: Modifiers) {
    let p = scr(h, x, y);
    h.event(Event::ModifiersChanged(modifiers));
    for pressed in [true, false] {
        h.event(Event::PointerMoved(p));
        h.event(Event::PointerButton { pos: p, button: PointerButton::Primary, pressed, modifiers });
        h.step();
    }
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.step();
}

#[test]
fn aimantation_pendant_le_trace() {
    meteroscope_i18n::set_language("fr");
    let mut h = harness();
    h.state_mut().doc.add(Shape::Rect { a: Pt::new(20.0, 20.0), b: Pt::new(80.0, 60.0), angle: 0.0 });
    h.state_mut().canvas.set_tool(Tool::Draw(ShapeKind::Length));
    // Le curseur à environ un pixel image (≈ 5 px écran) du coin haut-droit.
    h.event(Event::PointerMoved(scr(&h, 81.0, 19.0)));
    h.step();
    assert!(h.state().canvas.hint().contains("Aimanté : point"), "{}", h.state().canvas.hint());
    click(&mut h, 81.0, 19.0);
    assert_eq!(h.state().canvas.draft.pts, vec![Pt::new(80.0, 20.0)]);
    // Maj et un point remarquable : l'aimantation l'emporte sur les 45°
    // (le milieu du côté bas n'est pas sur une direction à 45° du coin).
    click_mod(&mut h, 50.5, 60.5, Modifiers::SHIFT);
    let Shape::Length { a, b } = h.state().doc.measurements[1].shape else { panic!() };
    assert_eq!((a, b), (Pt::new(80.0, 20.0), Pt::new(50.0, 60.0)));
    // Alt : pas d'aimantation, le point suit le curseur.
    click_mod(&mut h, 20.5, 59.5, Modifiers::ALT);
    click(&mut h, 150.0, 90.0);
    let Shape::Length { a, .. } = h.state().doc.measurements[2].shape else { panic!() };
    assert!(near(a, Pt::new(20.5, 59.5)), "{a:?}");
    // Maj seul loin de tout point : alignement habituel.
    click(&mut h, 120.0, 10.0);
    click_mod(&mut h, 170.0, 13.0, Modifiers::SHIFT);
    let Shape::Length { b, .. } = h.state().doc.measurements[3].shape else { panic!() };
    assert!(near(b, Pt::new(170.0, 10.0)), "{b:?}");

    // L'étalonnage s'aimante aussi.
    h.state_mut().canvas.set_tool(Tool::Calibrate);
    click(&mut h, 19.5, 20.5);
    click(&mut h, 79.5, 59.5);
    let seg = h.state().canvas.calibration_segment();
    assert_eq!(seg, Some((Pt::new(20.0, 20.0), Pt::new(80.0, 60.0))));
}

#[test]
fn aimantation_d_une_poignee() {
    meteroscope_i18n::set_language("fr");
    let mut h = harness();
    h.state_mut().doc.add(Shape::Circle { center: Pt::new(100.0, 50.0), edge: Pt::new(120.0, 50.0) });
    let id = h.state_mut().doc.add(Shape::Length { a: Pt::new(10.0, 10.0), b: Pt::new(60.0, 10.0) });
    // Poignée b vers le centre du cercle : accrochée exactement.
    let (p, q) = (scr(&h, 60.0, 10.0), scr(&h, 101.0, 49.0));
    drag(&mut h, p, q, PointerButton::Primary);
    let Shape::Length { b, .. } = h.state().doc.get(id).unwrap().shape else { panic!() };
    assert_eq!(b, Pt::new(100.0, 50.0));
    // Pas d'aimantation sur la mesure elle-même (son autre extrémité, son milieu).
    let (p, q) = (scr(&h, 100.0, 50.0), scr(&h, 11.0, 11.0));
    drag(&mut h, p, q, PointerButton::Primary);
    let Shape::Length { a, b } = h.state().doc.get(id).unwrap().shape else { panic!() };
    assert_eq!(a, Pt::new(10.0, 10.0));
    assert!(near(b, Pt::new(11.0, 11.0)), "{b:?}");
    // Les marqueurs ne s'aimantent pas (pas de doublon invisible au comptage).
    h.state_mut().canvas.set_tool(Tool::Draw(ShapeKind::Marker));
    click(&mut h, 101.0, 49.0);
    let Shape::Marker { p: m } = h.state().doc.measurements[2].shape else { panic!() };
    assert!(near(m, Pt::new(101.0, 49.0)), "{m:?}");
    // Ni quand on déplace un marqueur, même près d'un autre marqueur.
    let other = h.state_mut().doc.add(Shape::Marker { p: Pt::new(150.0, 80.0) });
    let moved = h.state().doc.measurements[2].id;
    h.state_mut().canvas.set_tool(Tool::Select);
    h.state_mut().doc.selected = Some(moved);
    h.step();
    let (p, q) = (scr(&h, 101.0, 49.0), scr(&h, 149.0, 79.0));
    drag(&mut h, p, q, PointerButton::Primary);
    let Shape::Marker { p: m } = h.state().doc.get(moved).unwrap().shape else { panic!() };
    assert!(near(m, Pt::new(149.0, 79.0)), "{m:?}");
    assert!(matches!(h.state().doc.get(other).unwrap().shape, Shape::Marker { p } if p == Pt::new(150.0, 80.0)));
}

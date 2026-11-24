//! Tests d'intégration de la vue avec egui_kittest (sans fenêtre).

use crate::{Canvas, CanvasOutput};
use egui::{Color32, ColorImage, Event, Key, Modifiers, PointerButton, Pos2, TextureHandle, TextureOptions, vec2};
use egui_kittest::Harness;
use mesura_core::{Document, Pt, Shape, ShapeKind, Tool};

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
    let h = harness();
    let c = &h.state().canvas;
    // 200×100 dans environ 800×600 : la largeur limite.
    assert!(c.zoom() > 3.5 && c.zoom() < 4.0, "zoom = {}", c.zoom());
    assert!(c.last_rect.contains(scr(&h, 0.0, 0.0)) && c.last_rect.contains(scr(&h, 200.0, 100.0)));
}

#[test]
fn longueur_clic_clic_et_glisser() {
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

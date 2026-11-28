//! Annulation d'un quart de tour ou d'un miroir au niveau de l'application :
//! pixels, mesures et angle de redressement reviennent ensemble.

use super::{MeterOscopeApp, Mode};
use image::{Rgba, RgbaImage};
use meteroscope_core::{Orientation, Pt, Shape};
use meteroscope_io as io;

/// Photo sans symétrie, avec un pixel unique servant de repère.
fn photo() -> RgbaImage {
    let mut img = RgbaImage::from_fn(9, 5, |x, y| Rgba([(x * 7) as u8, (y * 13) as u8, ((x * y) % 251) as u8, 255]));
    img.put_pixel(7, 1, Rgba([1, 2, 3, 255]));
    img
}

fn pixel_under_marker(app: &MeterOscopeApp) -> Rgba<u8> {
    let Shape::Marker { p } = app.doc.measurements[0].shape else { panic!() };
    *app.image.as_ref().unwrap().get_pixel(p.x.floor() as u32, p.y.floor() as u32)
}

#[test]
fn annuler_un_miroir_ramene_pixels_mesures_et_angle() {
    meteroscope_i18n::set_language("fr");
    let ctx = egui::Context::default();
    for t in [io::Transform::FlipHorizontal, io::Transform::FlipVertical, io::Transform::RotateLeft, io::Transform::RotateRight] {
        let mut app = MeterOscopeApp::blank();
        app.set_image(&ctx, photo(), None);
        app.doc.set_rotation(6.0);
        app.doc.add(Shape::Marker { p: Pt::new(7.5, 1.5) });
        let measurements = app.doc.measurements.clone();
        let generation = app.image_gen;

        app.transform(t);
        let flip = matches!(t, io::Transform::FlipHorizontal | io::Transform::FlipVertical);
        assert_eq!(app.doc.rotation, if flip { -6.0 } else { 6.0 }, "{t:?}");
        assert_ne!(**app.image.as_ref().unwrap(), photo(), "{t:?}");
        assert_eq!(pixel_under_marker(&app), Rgba([1, 2, 3, 255]), "{t:?}");
        assert!(app.image_modified() && app.image_gen > generation);
        let transformed = (app.image.clone(), app.doc.measurements.clone(), app.doc.rotation);

        // Ctrl+Z, puis ce que fait la frame suivante.
        assert!(app.doc.undo());
        app.sync_orientation();
        assert_eq!(**app.image.as_ref().unwrap(), photo(), "{t:?}");
        assert_eq!((app.doc.rotation, &app.doc.measurements), (6.0, &measurements), "{t:?}");
        assert_eq!(pixel_under_marker(&app), Rgba([1, 2, 3, 255]), "{t:?}");
        assert!(!app.image_modified(), "l'image est de nouveau celle du fichier");
        // L'image redressée gardée pour l'ancienne orientation n'est plus valable.
        assert!(app.straight.is_none() && !app.have_straight());

        // Rétablir.
        assert!(app.doc.redo());
        app.sync_orientation();
        assert_eq!((app.image.clone(), app.doc.measurements.clone(), app.doc.rotation), transformed, "{t:?}");
        assert_eq!(pixel_under_marker(&app), Rgba([1, 2, 3, 255]), "{t:?}");
    }
}

#[test]
fn le_document_du_flux_ne_reoriente_pas_la_photo() {
    meteroscope_i18n::set_language("fr");
    let ctx = egui::Context::default();
    let mut app = MeterOscopeApp::blank();
    app.set_image(&ctx, photo(), None);
    app.transform(io::Transform::RotateRight);
    let turned = app.image.clone();
    // Mode caméra : `doc` est celui du flux, jamais pivoté.
    std::mem::swap(&mut app.doc, &mut app.camera.other_doc);
    app.mode = Mode::Camera;
    assert_eq!(app.doc.orientation, Orientation::IDENTITY);
    app.sync_orientation();
    assert_eq!(app.image, turned);
    // De retour sur l'image, rien à refaire non plus.
    std::mem::swap(&mut app.doc, &mut app.camera.other_doc);
    app.mode = Mode::Image;
    app.sync_orientation();
    assert_eq!(app.image, turned);
}

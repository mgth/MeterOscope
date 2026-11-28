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

#[test]
fn annuler_un_miroir_avec_une_perspective_corrigee() {
    meteroscope_i18n::set_language("fr");
    let ctx = egui::Context::default();
    let mut app = MeterOscopeApp::blank();
    let img = RgbaImage::from_fn(60, 40, |x, y| Rgba([(x * 4) as u8, (y * 6) as u8, ((x * y) % 251) as u8, 255]));
    app.set_image(&ctx, img, None);
    let quad = [Pt::new(8.0, 6.0), Pt::new(50.0, 9.0), Pt::new(54.0, 33.0), Pt::new(5.0, 30.0)];
    let p = meteroscope_core::Perspective::from_quad(quad, (60, 40), None).expect("quadrilatère valide");
    assert!(app.doc.set_perspective(Some(p.clone()), None));
    app.sync_flat();
    let flat = app.work_image().unwrap().clone();
    assert_eq!(flat.dimensions(), p.size);
    app.doc.add(Shape::Marker { p: Pt::new(12.0, 7.0) });
    let measurements = app.doc.measurements.clone();

    // Miroir : la photo, la correction et l'image corrigée suivent.
    app.transform(io::Transform::FlipHorizontal);
    assert_ne!(app.doc.perspective.as_ref(), Some(&p));
    assert_ne!(*app.work_image().unwrap(), flat);

    // Ctrl+Z, puis ce que fait la frame suivante.
    assert!(app.doc.undo());
    app.sync_orientation();
    app.sync_flat();
    assert_eq!(app.doc.perspective.as_ref(), Some(&p));
    assert_eq!(app.doc.measurements, measurements);
    assert_eq!(*app.work_image().unwrap(), flat, "l'image corrigée est reconstruite sur la photo remise à l'endroit");
    assert!(!app.image_modified());
}

/// Dossier de test, supprimé à la fin.
struct TestDir(std::path::PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let d = std::path::absolute(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target-tests").join(name)).unwrap();
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Self(d)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn la_photo_d_origine_survit_a_deux_enregistrements() {
    meteroscope_i18n::set_language("fr");
    let ctx = egui::Context::default();
    let dir = TestDir::new("deux_enregistrements");
    let (source, project) = (dir.0.join("photo.png"), dir.0.join("photo.meteroscope"));
    io::save_image(&photo(), &source).unwrap();
    let mut app = MeterOscopeApp::blank();
    app.set_image(&ctx, io::load_image(&source).unwrap(), Some(source.clone()));

    // Miroir, quart de tour, premier enregistrement : l'image part dans un fichier dérivé.
    app.transform(io::Transform::FlipHorizontal);
    app.transform(io::Transform::RotateRight);
    assert!(app.write_project(&project));
    let derived = dir.0.join("photo-image.png");
    assert_eq!(app.image_path.as_deref(), Some(derived.as_path()));
    assert_eq!(io::load_image(&source).unwrap(), photo());
    assert_eq!(io::load_image(&derived).unwrap(), **app.image.as_ref().unwrap());

    // Quart de tour annulé, second enregistrement : le fichier dérivé est
    // réécrit, la photo d'origine reste intacte.
    assert!(app.doc.undo());
    assert!(app.write_project(&project));
    assert_eq!(io::load_image(&source).unwrap(), photo(), "la photo d'origine a été écrasée");
    assert_eq!(app.image_path.as_deref(), Some(derived.as_path()));
    let mirrored = image::imageops::flip_horizontal(&photo());
    assert_eq!(io::load_image(&derived).unwrap(), mirrored);
    assert!(!app.image_modified());

    // Tout annulé : l'image est de nouveau celle de la photo, mais le projet
    // pointe vers son fichier dérivé, réécrit à l'identique de la photo.
    assert!(app.doc.undo());
    assert!(app.write_project(&project));
    assert_eq!(io::load_image(&source).unwrap(), photo());
    assert_eq!(io::load_image(&derived).unwrap(), photo());
    // Le projet relu retrouve l'image mesurée.
    let saved = io::load_project(&project).unwrap();
    assert_eq!(saved.image_path.as_deref(), Some(derived.as_path()));
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 3, "aucun fichier en trop");
}

#[test]
fn un_projet_rouvert_n_ecrase_aucun_fichier_existant() {
    meteroscope_i18n::set_language("fr");
    let ctx = egui::Context::default();
    let dir = TestDir::new("projet_rouvert");
    // Session précédente : photo.png d'origine, photo-image.png dérivé, projet enregistré.
    let (source, derived, project) = (dir.0.join("photo.png"), dir.0.join("photo-image.png"), dir.0.join("photo.meteroscope"));
    io::save_image(&photo(), &source).unwrap();
    let old = image::imageops::flip_horizontal(&photo());
    io::save_image(&old, &derived).unwrap();
    // Nouvelle session : le projet est rouvert sur son fichier dérivé, puis l'image est retournée.
    let mut app = MeterOscopeApp::blank();
    app.set_image(&ctx, io::load_image(&derived).unwrap(), Some(derived.clone()));
    app.project_path = Some(project.clone());
    app.transform(io::Transform::FlipVertical);
    assert!(app.write_project(&project));
    // Ni la photo d'origine ni l'ancien fichier dérivé ne sont touchés.
    assert_eq!(io::load_image(&source).unwrap(), photo());
    assert_eq!(io::load_image(&derived).unwrap(), old);
    assert_eq!(app.image_path.as_deref(), Some(dir.0.join("photo-image-2.png").as_path()));

    // Capture sans fichier : `<projet>.png` s'il est libre.
    let mut app = MeterOscopeApp::blank();
    app.set_image(&ctx, photo(), None);
    let capture = dir.0.join("capture.meteroscope");
    assert!(app.write_project(&capture));
    assert_eq!(app.image_path.as_deref(), Some(dir.0.join("capture.png").as_path()));
    // Enregistrée de nouveau sans changement : rien de plus à écrire.
    assert!(app.write_project(&capture));
    assert_eq!(app.image_path.as_deref(), Some(dir.0.join("capture.png").as_path()));
}

#[test]
fn enregistrer_sous_ne_touche_pas_l_image_du_premier_projet() {
    meteroscope_i18n::set_language("fr");
    let ctx = egui::Context::default();
    let dir = TestDir::new("enregistrer_sous");
    let (a, b) = (dir.0.join("A.meteroscope"), dir.0.join("B.meteroscope"));
    let image_of = |project: &std::path::Path| io::load_image(io::load_project(project).unwrap().image_path.as_deref().unwrap()).unwrap();

    // Image collée (sans fichier), retournée, enregistrée dans A.
    let mut app = MeterOscopeApp::blank();
    app.set_image(&ctx, photo(), None);
    app.transform(io::Transform::FlipHorizontal);
    assert!(app.write_project(&a));
    let mirrored = image::imageops::flip_horizontal(&photo());
    assert_eq!(image_of(&a), mirrored);

    // Quart de tour, puis « Enregistrer sous » B dans le même dossier.
    app.transform(io::Transform::RotateRight);
    assert!(app.write_project(&b));
    let turned = image::imageops::rotate90(&mirrored);
    assert_eq!(image_of(&b), turned);
    assert_eq!(image_of(&a), mirrored, "l'image du projet A a été réécrite");
    assert_ne!(io::load_project(&a).unwrap().image_path, io::load_project(&b).unwrap().image_path);

    // B enregistré de nouveau après une autre transformation : son propre fichier, réécrit.
    app.transform(io::Transform::FlipVertical);
    assert!(app.write_project(&b));
    assert_eq!(image_of(&b), image::imageops::flip_vertical(&turned));
    assert_eq!(image_of(&a), mirrored);
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 4, "deux projets, deux images");

    // « Enregistrer sous » d'abord, transformation ensuite : A reste intact aussi.
    let dir = TestDir::new("enregistrer_sous_puis_transformer");
    let (a, b) = (dir.0.join("A.meteroscope"), dir.0.join("B.meteroscope"));
    let mut app = MeterOscopeApp::blank();
    app.set_image(&ctx, photo(), None);
    assert!(app.write_project(&a));
    assert!(app.write_project(&b));
    // Sans changement, les deux projets partagent le même fichier image.
    assert_eq!(io::load_project(&a).unwrap().image_path, io::load_project(&b).unwrap().image_path);
    app.transform(io::Transform::RotateLeft);
    assert!(app.write_project(&b));
    assert_eq!(image_of(&a), photo(), "l'image du projet A a été réécrite");
    assert_eq!(image_of(&b), image::imageops::rotate270(&photo()));

    // Fichier partagé par deux projets : réenregistrer sous A après une
    // transformation ne le réécrit pas non plus sous les pieds de B.
    let dir = TestDir::new("fichier_partage");
    let (a, b) = (dir.0.join("A.meteroscope"), dir.0.join("B.meteroscope"));
    let mut app = MeterOscopeApp::blank();
    app.set_image(&ctx, photo(), None);
    assert!(app.write_project(&a));
    assert!(app.write_project(&b));
    app.transform(io::Transform::FlipHorizontal);
    assert!(app.write_project(&a));
    assert_eq!(image_of(&b), photo(), "l'image partagée avec B a été réécrite");
    assert_eq!(image_of(&a), image::imageops::flip_horizontal(&photo()));
}

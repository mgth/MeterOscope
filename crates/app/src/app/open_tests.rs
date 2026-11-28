//! Ouverture d'images autour de la série : chaque chemin (Ouvrir, glisser,
//! coller, capture) ajoute à ce qui est ouvert, tout s'annule, et l'échelle
//! suit une seule règle.

use super::{MeterOscopeApp, Mode};
use crate::clipboard::Pasted;
use image::RgbaImage;
use meteroscope_core::{Pt, Shape, Tool};
use meteroscope_io as io;
use std::path::PathBuf;

/// Dossier de test, supprimé à la fin.
struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let d = std::path::absolute(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target-tests").join(name)).unwrap();
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Self(d)
    }

    /// Écrit une image `w` × `h` et rend son chemin.
    fn png(&self, name: &str, w: u32, h: u32) -> PathBuf {
        let p = self.0.join(name);
        io::save_image(&RgbaImage::new(w, h), &p).unwrap();
        p
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn start() -> (egui::Context, MeterOscopeApp) {
    meteroscope_i18n::set_language("fr");
    (egui::Context::default(), MeterOscopeApp::blank())
}

fn widths(app: &MeterOscopeApp) -> Vec<u32> {
    app.series.model.entries().iter().map(|e| e.size.unwrap().0).collect()
}

fn scale(app: &MeterOscopeApp) -> Option<f64> {
    app.doc.calibration.scale.as_ref().map(|s| s.units_per_px)
}

/// Une frappe de touche, vue par les raccourcis de l'application.
fn press(ctx: &egui::Context, app: &mut MeterOscopeApp, key: egui::Key) {
    let mut raw = egui::RawInput::default();
    raw.events.push(egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE });
    let mut out = ctx.run_pass(raw, |ctx| app.shortcuts(ctx));
    // Les vignettes créées par le test n'ont pas de moteur de rendu à qui aller.
    out.textures_delta.clear();
}

/// Ouvrir, puis ouvrir encore : la deuxième image fait une série, la
/// troisième l'allonge ; chaque ouverture s'annule, la première exceptée.
#[test]
fn ouvrir_ajoute_et_s_annule() {
    let (ctx, mut app) = start();
    let dir = TestDir::new("ouvrir");
    let (a, b, c, d) = (dir.png("a.png", 40, 30), dir.png("b.png", 30, 30), dir.png("c.png", 20, 20), dir.png("d.png", 10, 10));
    app.open_paths(&ctx, vec![a.clone()]);
    app.series_frame(&ctx);
    assert_eq!(widths(&app), [40]);
    assert!(!app.is_dirty() && !app.can_undo(), "ouvrir la première image n'est pas une modification");
    app.doc.add(Shape::Marker { p: Pt::new(1.0, 1.0) });
    app.open_paths(&ctx, vec![b.clone()]);
    app.series_frame(&ctx);
    assert_eq!((widths(&app), app.series.model.active()), (vec![40, 30], 1));
    assert_eq!(app.image_path.as_deref(), Some(b.as_path()));
    assert!(app.doc.measurements.is_empty() && app.project_path.is_none());
    // Glisser-déposer de deux fichiers d'un coup : même chemin de code.
    app.open_paths(&ctx, vec![c.clone(), d.clone()]);
    app.series_frame(&ctx);
    assert_eq!((widths(&app), app.series.model.active()), (vec![40, 30, 20, 10], 2));
    app.undo(&ctx);
    assert_eq!((widths(&app), app.series.model.active()), (vec![40, 30], 1));
    app.undo(&ctx);
    assert_eq!((widths(&app), app.doc.measurements.len()), (vec![40], 1));
    assert_eq!(app.image.as_ref().map(|i| i.width()), Some(40));
    app.redo(&ctx);
    app.redo(&ctx);
    assert_eq!(widths(&app), [40, 30, 20, 10]);
    // Un fichier illisible ne change rien et le dit.
    let bad = dir.0.join("faux.png");
    std::fs::write(&bad, b"pas une image").unwrap();
    app.open_paths(&ctx, vec![bad]);
    assert_eq!(widths(&app).len(), 4);
    assert!(app.toast.as_ref().is_some_and(|t| t.1));
}

/// Un projet remplace toujours le travail en cours, sans demander : Ctrl+Z
/// le rend, Ctrl+Y revient au projet.
#[test]
fn un_projet_remplace_le_travail_et_s_annule() {
    let (ctx, mut app) = start();
    let dir = TestDir::new("projet");
    let (a, b) = (dir.png("a.png", 40, 30), dir.png("b.png", 20, 20));
    let project = dir.0.join("lot.meteroscope");
    // Projet enregistré : une image, une mesure, une échelle.
    app.open_paths(&ctx, vec![b.clone()]);
    assert!(app.doc.calibration.set_from_reference(100.0, 10.0, "mm"));
    app.doc.add(Shape::Marker { p: Pt::new(2.0, 2.0) });
    assert!(app.series_write(&project));

    let (ctx, mut app) = start();
    app.open_paths(&ctx, vec![a.clone()]);
    app.doc.add(Shape::Marker { p: Pt::new(1.0, 1.0) });
    app.doc.add(Shape::Marker { p: Pt::new(3.0, 1.0) });
    app.add_image(&ctx, RgbaImage::new(40, 30), None);
    app.series_frame(&ctx);
    assert!(app.is_dirty());
    // Le projet, mêlé à des images dans la sélection, l'emporte.
    app.open_paths(&ctx, vec![a.clone(), project.clone()]);
    app.series_frame(&ctx);
    assert_eq!((widths(&app), app.doc.measurements.len(), scale(&app)), (vec![20], 1, Some(0.1)));
    assert_eq!(app.project_path.as_deref(), Some(project.as_path()));
    assert!(!app.is_dirty());
    app.undo(&ctx);
    assert_eq!((widths(&app), app.series.model.active()), (vec![40, 40], 1));
    assert!(app.project_path.is_none() && app.is_dirty());
    app.series_step(&ctx, -1);
    assert_eq!(app.doc.measurements.len(), 2);
    app.series_step(&ctx, 1);
    app.redo(&ctx);
    assert_eq!((widths(&app), app.project_path.as_deref()), (vec![20], Some(project.as_path())));
    // Une image ouverte ensuite s'ajoute au projet.
    app.open_paths(&ctx, vec![a]);
    assert_eq!((widths(&app), app.project_path.as_deref()), (vec![20, 40], Some(project.as_path())));
    assert!(app.is_dirty());
}

#[test]
fn coller_ajoute_a_la_serie() {
    let (ctx, mut app) = start();
    let dir = TestDir::new("coller");
    app.add_image(&ctx, RgbaImage::new(40, 30), None);
    app.doc.add(Shape::Marker { p: Pt::new(1.0, 1.0) });
    app.series_frame(&ctx);
    // Image du presse-papiers, puis fichier copié dans un gestionnaire de fichiers.
    app.apply_paste(&ctx, Pasted::Image(RgbaImage::new(20, 20)));
    app.series_frame(&ctx);
    assert_eq!((widths(&app), app.series.model.active()), (vec![40, 20], 1));
    app.apply_paste(&ctx, Pasted::File(dir.png("c.png", 10, 10)));
    app.series_frame(&ctx);
    assert_eq!((widths(&app), app.series.model.active()), (vec![40, 20, 10], 2));
    assert_eq!(app.image.as_ref().map(|i| i.width()), Some(10));
    // Chaque collage s'annule séparément ; la première image a gardé ses mesures.
    app.undo(&ctx);
    assert_eq!(widths(&app), [40, 20]);
    app.undo(&ctx);
    assert_eq!((widths(&app), app.doc.measurements.len()), (vec![40], 1));
    assert_eq!(app.image.as_ref().map(|i| i.width()), Some(40));
    app.redo(&ctx);
    assert_eq!((widths(&app), app.image.as_ref().map(|i| i.width())), (vec![40, 20], Some(20)));
}

/// « Garder l'échelle pour l'image suivante » et l'échelle commune de la
/// série : une seule règle, la même pour une image, plusieurs, ou un ajout.
#[test]
fn garder_l_echelle_vaut_pour_toutes_les_ouvertures() {
    let (ctx, mut app) = start();
    let dir = TestDir::new("garder");
    let (a, b, c) = (dir.png("a.png", 40, 30), dir.png("b.png", 40, 30), dir.png("c.png", 20, 20));
    app.scale_presets.keep_scale = true;
    app.open_paths(&ctx, vec![a.clone()]);
    assert!(app.doc.calibration.set_from_reference(100.0, 10.0, "mm"));
    app.series_frame(&ctx);
    // Ajout d'une image d'autres dimensions : elle ne suit pas l'échelle
    // commune, mais l'option lui donne l'échelle en cours.
    app.open_paths(&ctx, vec![c.clone()]);
    app.series_frame(&ctx);
    assert_eq!(scale(&app), Some(0.1), "l'option vaut aussi pour un ajout");
    // Option décochée : l'image d'autres dimensions arrive en pixels, celle
    // de mêmes dimensions suit toujours l'échelle commune.
    app.scale_presets.keep_scale = false;
    app.series_step(&ctx, -1);
    app.open_paths(&ctx, vec![c.clone(), b.clone()]);
    app.series_frame(&ctx);
    assert_eq!((app.series.model.active(), scale(&app)), (2, None));
    app.series_step(&ctx, 1);
    assert_eq!(scale(&app), Some(0.1));

    // Nouveau travail : option cochée, la première image reprend l'échelle
    // du travail précédent, qu'on en ouvre une ou plusieurs ; décochée, non.
    app.series_step(&ctx, -3);
    app.scale_presets.keep_scale = true;
    app.new_work(&ctx);
    app.open_paths(&ctx, vec![c.clone(), a.clone()]);
    app.series_frame(&ctx);
    assert_eq!((widths(&app), scale(&app)), (vec![20, 40], Some(0.1)));
    app.series_step(&ctx, 1);
    assert_eq!(scale(&app), Some(0.1), "l'image d'autres dimensions aussi");
    app.scale_presets.keep_scale = false;
    app.new_work(&ctx);
    app.open_paths(&ctx, vec![a.clone(), b.clone()]);
    assert_eq!(scale(&app), None);
    // Une image à perspective corrigée ne transmet pas son échelle, qui ne
    // vaut que pour elle.
    app.scale_presets.keep_scale = true;
    assert!(app.doc.calibration.set_from_reference(100.0, 10.0, "mm"));
    app.series_frame(&ctx);
    let quad = [Pt::new(2., 2.), Pt::new(30., 3.), Pt::new(32., 25.), Pt::new(3., 24.)];
    assert!(app.doc.correct_perspective(quad, (40, 30), Some((50.0, 40.0, "cm"))));
    app.series_frame(&ctx);
    app.open_paths(&ctx, vec![c.clone()]);
    assert_eq!((app.doc.calibration.length_unit().to_owned(), scale(&app)), ("mm".to_owned(), Some(0.1)));
    app.series_step(&ctx, -1);
    assert_eq!(scale(&app), Some(0.1), "l'image liée garde l'échelle commune");
}

/// Les fichiers ouverts à plusieurs ou ajoutés comptent parmi les récents et
/// fixent le dossier proposé par les dialogues.
#[test]
fn ouvrir_plusieurs_images_retient_dossier_et_recents() {
    let (ctx, mut app) = start();
    let dir = TestDir::new("recents");
    let (a, b, c) = (dir.png("a.png", 40, 30), dir.png("b.png", 40, 30), dir.png("c.png", 40, 30));
    app.open_paths(&ctx, vec![a.clone(), b.clone()]);
    let canon = |p: &PathBuf| std::fs::canonicalize(p).unwrap();
    assert!(app.settings.recent.contains(&canon(&a)) && app.settings.recent.contains(&canon(&b)), "{:?}", app.settings.recent);
    assert_eq!(app.settings.last_dir.as_deref(), canon(&a).parent());
    app.open_paths(&ctx, vec![c.clone()]);
    assert_eq!(app.settings.recent.first(), Some(&canon(&c)));
    // Tous les dialogues (ouvrir, enregistrer, exports de la série) partent de là.
    assert_eq!(app.dialog_dir(), canon(&a).parent());
}

/// Dialogue de perspective ouvert : changer d'image appliquerait le
/// quadrilatère à une autre photo. Les raccourcis attendent, comme pour le
/// dialogue d'échelle.
#[test]
fn dialogue_ouvert_les_raccourcis_attendent() {
    let (ctx, mut app) = start();
    app.add_image(&ctx, RgbaImage::new(40, 30), None);
    app.add_image(&ctx, RgbaImage::new(40, 30), None);
    app.series_frame(&ctx);
    assert_eq!(app.series.model.active(), 1);
    press(&ctx, &mut app, egui::Key::PageUp);
    assert_eq!(app.series.model.active(), 0, "sans dialogue, la touche change d'image");
    let quad = [Pt::new(2., 2.), Pt::new(30., 3.), Pt::new(32., 25.), Pt::new(3., 24.)];
    app.perspective.open(quad, (40, 30));
    press(&ctx, &mut app, egui::Key::PageDown);
    assert_eq!(app.series.model.active(), 0, "dialogue de perspective ouvert");
    app.perspective.cancel();
    app.calibration.open(10.0);
    press(&ctx, &mut app, egui::Key::PageDown);
    assert_eq!(app.series.model.active(), 0, "dialogue d'échelle ouvert");
    // Changer d'image par un autre chemin (bande, dépôt, collage) referme le
    // dialogue : il ne s'appliquera pas à une autre photo.
    app.perspective.open(quad, (40, 30));
    app.canvas.set_tool(Tool::Perspective);
    app.apply_paste(&ctx, Pasted::Image(RgbaImage::new(20, 20)));
    assert!(!app.dialog_open() && app.mode == Mode::Image);
    assert_eq!(app.series.model.active(), 2);
}

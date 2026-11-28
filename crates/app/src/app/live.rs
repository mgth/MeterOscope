//! Mesure sur le flux caméra : le flux s'affiche dans la même vue que les
//! images fixes, avec son propre document (mesures, échelle) et sa propre
//! vue, échangés avec ceux de l'image à l'entrée et à la sortie du mode.

use super::{MeterOscopeApp, Mode};
use egui::{Key, Ui};
use image::RgbaImage;
use meteroscope_core::{Pt, ShapeKind, Tool};
use meteroscope_core::rescale::Rescale;
use meteroscope_i18n::tr;

impl MeterOscopeApp {
    /// Entre dans le mode caméra : le document et la vue du flux prennent la
    /// place de ceux de l'image (qui sont gardés pour le retour).
    pub(super) fn enter_camera(&mut self, ctx: &egui::Context) {
        self.camera_mode();
        self.camera.start(ctx);
    }

    /// Passage en mode caméra, sans démarrer le périphérique.
    fn camera_mode(&mut self) {
        std::mem::swap(&mut self.doc, &mut self.camera.other_doc);
        std::mem::swap(&mut self.canvas, &mut self.camera.other_canvas);
        // Les identifiants de mesures se recoupent d'un document à l'autre.
        self.profiles.invalidate();
        // La détection lit les pixels d'une image fixe : son panneau se
        // ferme, et un calcul en cours appartient à la photo, pas au flux.
        if self.detect.is_open() {
            self.detect.toggle();
        }
        self.detect.invalidate();
        self.mode = Mode::Camera;
    }

    /// Les outils qui lisent les pixels d'une image fixe (perspective, profil
    /// d'intensité) sont refusés sur le flux dès qu'on les choisit, avec la
    /// marche à suivre, plutôt que de produire une mesure sans résultat.
    fn refuse_still_tools(&mut self) {
        let msg = match self.canvas.tool() {
            Tool::Perspective => tr!("msg.perspective_live"),
            Tool::Draw(ShapeKind::Profile) => tr!("msg.profile_live"),
            _ => return,
        };
        self.canvas.set_tool(Tool::Select);
        self.error(msg);
    }

    /// Quitte le mode caméra et rend à l'image son document et sa vue.
    pub(super) fn leave_camera(&mut self) {
        self.camera.stop();
        std::mem::swap(&mut self.doc, &mut self.camera.other_doc);
        std::mem::swap(&mut self.canvas, &mut self.camera.other_canvas);
        self.profiles.invalidate();
        self.detect.invalidate();
        self.mode = Mode::Image;
    }

    /// Fige l'image du flux : elle devient l'image de travail, avec une copie
    /// des mesures et de l'échelle du flux, et la même vue (zoom, outil).
    pub(super) fn freeze(&mut self, ctx: &egui::Context, img: RgbaImage) {
        // Les mesures du flux, sans son historique : Ctrl+Z défait la
        // capture, pas ce qui a été tracé sur le flux avant elle.
        let doc = self.doc.detached();
        self.leave_camera();
        // La capture rejoint ce qui est ouvert (ou devient la première image).
        // Un flux sans échelle prend celle de la série ; un flux étalonné
        // autrement garde la sienne pour cette image.
        self.series_add(ctx, vec![super::series::Incoming { path: None, size: img.dimensions(), pixels: Some(img), doc: Some(doc) }]);
        // La vue du flux devient celle de l'image ; le flux retrouvera une
        // vue ajustée à son retour.
        std::mem::swap(&mut self.canvas, &mut self.camera.other_canvas);
        self.camera.other_canvas.fit();
        self.info(tr!("msg.image_captured"));
    }

    /// Les coordonnées des mesures sont en pixels du flux : à un changement
    /// de résolution elles suivent si le rapport d'aspect est le même.
    fn follow_resolution(&mut self, size: (u32, u32), device: String) {
        let other_camera = self.camera.doc_device.as_ref().is_some_and(|d| *d != device);
        if other_camera && self.doc.calibration.is_calibrated() {
            self.error(tr!("msg.live_other_camera"));
        }
        let before = self.doc.revision();
        let r = self.camera.reference.follow(&mut self.doc, size);
        // Les mesures ont suivi la résolution ; l'historique suit aussi, et
        // ce n'est pas une étape à annuler : Ctrl+Z ramènerait sinon des
        // coordonnées de l'ancienne résolution sur le flux de la nouvelle.
        if let Rescale::Scaled(s) = r {
            self.doc.rescale_history(s, self.doc.revision() != before);
        }
        if !self.doc.measurements.is_empty() || self.doc.calibration.is_calibrated() {
            match r {
                Rescale::Same => {}
                Rescale::Scaled(_) => self.info(tr!("msg.live_rescaled", w = size.0, h = size.1)),
                Rescale::Mismatch => self.error(tr!("msg.live_mismatch", w = size.0, h = size.1)),
            }
        }
        self.camera.doc_device = Some(device);
    }

    /// Zone centrale du mode caméra. Renvoie la position du curseur (pixels image).
    pub(super) fn live_view(&mut self, ui: &mut Ui) -> Option<Pt> {
        let ctx = ui.ctx().clone();
        ui.add_space(4.0);
        self.camera.controls(ui);
        ui.add_space(4.0);
        let capture = self.camera.capture_bar(ui);
        if let Some(r) = self.camera.poll(&ctx) {
            self.follow_resolution(r.size, r.device);
        }

        let rect = ui.available_rect_before_wrap();
        self.refuse_still_tools();
        // Le flux est toujours affiché droit, pivoté par la carte graphique.
        self.canvas.set_baked(None);
        let out = self.canvas.show(ui, &mut self.doc, self.camera.texture());
        self.camera.overlay(ui, rect, &self.canvas, &self.doc.calibration);
        if let Some(id) = out.created {
            self.profiles.created(&mut self.doc, id);
        }
        if let Some((a, b)) = out.calibration_segment {
            self.calibration.open(a.dist(b));
        }

        // Entrée capture (hors tracé : Entrée y termine un chemin, déjà consommée).
        let enter = !ctx.egui_wants_keyboard_input()
            && !self.dialog_open()
            && !self.canvas.is_drawing()
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter));
        if (capture || enter)
            && let Some(img) = self.camera.snapshot()
        {
            self.freeze(&ctx, img);
        }
        out.cursor
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meteroscope_core::Shape;

    fn photo(ctx: &egui::Context, app: &mut MeterOscopeApp, w: u32) {
        app.add_image(ctx, RgbaImage::new(w, 10), None);
    }

    /// Mode caméra sans ouvrir de vraie caméra.
    fn enter_without_device(app: &mut MeterOscopeApp) {
        app.camera_mode();
    }

    /// Sur le flux, la détection et les outils qui lisent une image fixe
    /// (profil d'intensité, perspective) sont refusés de la même façon.
    #[test]
    fn outils_d_image_fixe_refuses_sur_le_flux() {
        meteroscope_i18n::set_language("fr");
        let ctx = egui::Context::default();
        let mut app = MeterOscopeApp::blank();
        photo(&ctx, &mut app, 20);
        app.detect.toggle();
        enter_without_device(&mut app);
        assert!(!app.detect.is_open(), "le panneau de détection se ferme en entrant sur le flux");
        for tool in [Tool::Draw(ShapeKind::Profile), Tool::Perspective] {
            app.toast = None;
            app.canvas.set_tool(tool);
            app.refuse_still_tools();
            assert_eq!(app.canvas.tool(), Tool::Select, "{tool:?}");
            assert!(app.toast.as_ref().is_some_and(|t| t.1), "{tool:?} : refus expliqué");
        }
        // Les autres outils restent disponibles.
        app.toast = None;
        app.canvas.set_tool(Tool::Draw(ShapeKind::Length));
        app.refuse_still_tools();
        assert_eq!(app.canvas.tool(), Tool::Draw(ShapeKind::Length));
        assert!(app.toast.is_none());
    }

    #[test]
    fn ouvrir_une_photo_depuis_la_camera() {
        meteroscope_i18n::set_language("fr");
        let ctx = egui::Context::default();
        let mut app = MeterOscopeApp::blank();
        photo(&ctx, &mut app, 20);
        app.doc.add(Shape::Marker { p: Pt::new(1.0, 1.0) });
        enter_without_device(&mut app);
        let live = app.doc.add(Shape::Marker { p: Pt::new(2.0, 2.0) });
        // Ctrl+O d'une autre photo pendant le mode caméra.
        photo(&ctx, &mut app, 30);
        assert!(app.mode == Mode::Image);
        assert!(app.doc.measurements.is_empty(), "la nouvelle photo part d'un document vierge");
        assert_eq!(app.image.as_ref().map(|i| i.width()), Some(30));
        // Le document du flux est intact pour le retour à la caméra.
        assert_eq!(app.camera.other_doc.measurements.len(), 1);
        assert!(app.camera.other_doc.get(live).is_some());
        enter_without_device(&mut app);
        assert!(app.doc.get(live).is_some());
    }

    #[test]
    fn capture_garde_les_mesures_du_flux() {
        meteroscope_i18n::set_language("fr");
        let ctx = egui::Context::default();
        let mut app = MeterOscopeApp::blank();
        enter_without_device(&mut app);
        assert!(app.doc.calibration.set_from_reference(100.0, 1.0, "mm"));
        app.doc.add(Shape::Marker { p: Pt::new(2.0, 2.0) });
        app.freeze(&ctx, RgbaImage::new(40, 30));
        assert!(app.mode == Mode::Image);
        assert_eq!(app.doc.measurements.len(), 1);
        assert!(app.doc.calibration.is_calibrated());
        // Le flux garde aussi les siennes.
        assert_eq!(app.camera.other_doc.measurements.len(), 1);
    }

    /// Juste après une capture, Ctrl+Z défait la capture, pas les mesures
    /// posées sur le flux avant elle.
    #[test]
    fn annuler_apres_une_capture_retire_la_capture() {
        meteroscope_i18n::set_language("fr");
        let ctx = egui::Context::default();
        let mut app = MeterOscopeApp::blank();
        photo(&ctx, &mut app, 20);
        app.series_frame(&ctx);
        enter_without_device(&mut app);
        app.doc.add(Shape::Marker { p: Pt::new(2.0, 2.0) });
        app.doc.add(Shape::Marker { p: Pt::new(3.0, 3.0) });
        app.freeze(&ctx, RgbaImage::new(40, 30));
        app.series_frame(&ctx);
        assert_eq!((app.series.model.len(), app.doc.measurements.len()), (2, 2));
        app.undo(&ctx);
        app.series_frame(&ctx);
        assert_eq!(app.series.model.len(), 1, "la capture est retirée");
        assert_eq!(app.image.as_ref().map(|i| i.width()), Some(20));
        // Le flux a gardé ses mesures et son historique.
        assert_eq!(app.camera.other_doc.measurements.len(), 2);
        app.redo(&ctx);
        assert_eq!((app.series.model.len(), app.doc.measurements.len()), (2, 2));
    }

    /// Le flux change de résolution : les mesures suivent, et ce n'est pas
    /// une étape à annuler. Ctrl+Z défait la dernière vraie modification,
    /// dans les coordonnées de la résolution affichée.
    #[test]
    fn annuler_apres_un_changement_de_resolution() {
        meteroscope_i18n::set_language("fr");
        let ctx = egui::Context::default();
        let mut app = MeterOscopeApp::blank();
        enter_without_device(&mut app);
        app.follow_resolution((640, 480), "cam".to_owned());
        let first = app.doc.add(Shape::Marker { p: Pt::new(10.0, 20.0) });
        let second = app.doc.add(Shape::Marker { p: Pt::new(30.0, 40.0) });
        app.follow_resolution((1280, 960), "cam".to_owned());
        let at = |app: &MeterOscopeApp, id| app.doc.get(id).map(|m| m.shape.points()[0]);
        assert_eq!((at(&app, first), at(&app, second)), (Some(Pt::new(20.0, 40.0)), Some(Pt::new(60.0, 80.0))));
        // Ctrl+Z : la seconde mesure part, la première reste à sa place sur le flux.
        app.undo(&ctx);
        assert_eq!((at(&app, first), at(&app, second)), (Some(Pt::new(20.0, 40.0)), None));
        assert_eq!(app.camera.reference.size(), Some((1280, 960)));
        app.redo(&ctx);
        assert_eq!(at(&app, second), Some(Pt::new(60.0, 80.0)));
        app.undo(&ctx);
        app.undo(&ctx);
        assert!(app.doc.measurements.is_empty() && !app.can_undo());
    }
}

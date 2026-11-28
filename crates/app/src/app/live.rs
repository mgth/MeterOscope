//! Mesure sur le flux caméra : le flux s'affiche dans la même vue que les
//! images fixes, avec son propre document (mesures, échelle) et sa propre
//! vue, échangés avec ceux de l'image à l'entrée et à la sortie du mode.

use super::{MeterOscopeApp, Mode};
use egui::{Key, Ui};
use image::RgbaImage;
use meteroscope_core::{Pt, Tool};
use meteroscope_core::rescale::Rescale;
use meteroscope_i18n::tr;

impl MeterOscopeApp {
    /// Entre dans le mode caméra : le document et la vue du flux prennent la
    /// place de ceux de l'image (qui sont gardés pour le retour).
    pub(super) fn enter_camera(&mut self, ctx: &egui::Context) {
        std::mem::swap(&mut self.doc, &mut self.camera.other_doc);
        std::mem::swap(&mut self.canvas, &mut self.camera.other_canvas);
        // Les identifiants de mesures se recoupent d'un document à l'autre.
        self.profiles.invalidate();
        self.mode = Mode::Camera;
        self.camera.start(ctx);
    }

    /// Quitte le mode caméra et rend à l'image son document et sa vue.
    pub(super) fn leave_camera(&mut self) {
        self.camera.stop();
        std::mem::swap(&mut self.doc, &mut self.camera.other_doc);
        std::mem::swap(&mut self.canvas, &mut self.camera.other_canvas);
        self.profiles.invalidate();
        self.mode = Mode::Image;
    }

    /// Fige l'image du flux : elle devient l'image de travail, avec une copie
    /// des mesures et de l'échelle du flux, et la même vue (zoom, outil).
    pub(super) fn freeze(&mut self, ctx: &egui::Context, img: RgbaImage) {
        let doc = self.doc.clone();
        self.leave_camera();
        // La capture rejoint la série ouverte (ou devient la première image).
        self.series_capture(ctx, img);
        let shared = self.doc.calibration.clone();
        self.doc = doc;
        // Un flux sans échelle n'efface pas celle de la série.
        if !self.doc.calibration.is_calibrated() {
            self.doc.calibration = shared;
        }
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
        let r = self.camera.reference.follow(&mut self.doc, size);
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
        if out.perspective_quad.is_some() {
            // Pas de correction de perspective sur le flux : elle se fait sur l'image figée.
            self.canvas.clear_perspective_quad();
            self.canvas.set_tool(Tool::Select);
            self.error(tr!("msg.perspective_live"));
        }

        // Entrée capture (hors tracé : Entrée y termine un chemin, déjà consommée).
        let enter = !ctx.egui_wants_keyboard_input()
            && !self.calibration.is_open()
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
        app.set_image(ctx, RgbaImage::new(w, 10), None);
    }

    /// Mode caméra sans ouvrir de vraie caméra.
    fn enter_without_device(app: &mut MeterOscopeApp) {
        std::mem::swap(&mut app.doc, &mut app.camera.other_doc);
        std::mem::swap(&mut app.canvas, &mut app.camera.other_canvas);
        app.mode = Mode::Camera;
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
}

//! Mesure sur le flux caméra : le flux s'affiche dans la même vue que les
//! images fixes, avec son propre document (mesures, échelle) et sa propre
//! vue, échangés avec ceux de l'image à l'entrée et à la sortie du mode.

use super::{MeterOscopeApp, Mode};
use egui::{Key, Ui};
use image::RgbaImage;
use meteroscope_core::Pt;
use meteroscope_core::rescale::{Rescale, rescale};
use meteroscope_i18n::tr;

impl MeterOscopeApp {
    /// Entre dans le mode caméra : le document et la vue du flux prennent la
    /// place de ceux de l'image (qui sont gardés pour le retour).
    pub(super) fn enter_camera(&mut self, ctx: &egui::Context) {
        std::mem::swap(&mut self.doc, &mut self.camera.other_doc);
        std::mem::swap(&mut self.canvas, &mut self.camera.other_canvas);
        self.mode = Mode::Camera;
        self.camera.start(ctx);
    }

    /// Quitte le mode caméra et rend à l'image son document et sa vue.
    pub(super) fn leave_camera(&mut self) {
        self.camera.stop();
        std::mem::swap(&mut self.doc, &mut self.camera.other_doc);
        std::mem::swap(&mut self.canvas, &mut self.camera.other_canvas);
        self.mode = Mode::Image;
    }

    /// Fige l'image du flux : elle devient l'image de travail, avec une copie
    /// des mesures et de l'échelle du flux, et la même vue (zoom, outil).
    fn freeze(&mut self, ctx: &egui::Context, img: RgbaImage) {
        let doc = self.doc.clone();
        self.leave_camera();
        self.set_image(ctx, img, None);
        self.doc = doc;
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
        if let Some(from) = self.camera.doc_size {
            match rescale(&mut self.doc, from, size) {
                Rescale::Same => {}
                Rescale::Scaled(_) => {
                    if !self.doc.measurements.is_empty() || self.doc.calibration.is_calibrated() {
                        self.info(tr!("msg.live_rescaled", w = size.0, h = size.1));
                    }
                }
                Rescale::Mismatch => {
                    if !self.doc.measurements.is_empty() || self.doc.calibration.is_calibrated() {
                        self.error(tr!("msg.live_mismatch", w = size.0, h = size.1));
                    }
                }
            }
        }
        self.camera.doc_size = Some(size);
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
        if let Some((a, b)) = out.calibration_segment {
            self.calibration.open(a.dist(b));
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

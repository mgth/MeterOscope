//! Vue caméra en direct : choix de l'appareil, aperçu, capture.

use egui::{Color32, ColorImage, Rect, RichText, TextureHandle, TextureOptions, Ui, pos2};
use image::RgbaImage;
use meteroscope_camera::{CameraStream, DeviceInfo, VideoMode, list_devices, list_modes};
use std::path::PathBuf;

#[derive(Default)]
pub struct CameraView {
    devices: Vec<DeviceInfo>,
    current: Option<DeviceInfo>,
    stream: Option<CameraStream>,
    texture: Option<TextureHandle>,
    error: Option<String>,
    modes: Vec<VideoMode>,
    /// Le flux courant a déjà fourni au moins une image.
    got_frame: bool,
    /// Appareils déjà essayés sans succès lors de la recherche automatique.
    failed: Vec<DeviceInfo>,
    /// Vrai tant que l'utilisateur n'a pas choisi lui-même une caméra.
    auto: bool,
}

/// Fichier où l'on retient la dernière caméra qui a servi à capturer.
fn memory_file() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("meteroscope").join("camera"))
}

impl CameraView {
    /// Appelé à l'entrée dans le mode caméra.
    pub fn start(&mut self, ctx: &egui::Context) {
        self.devices = list_devices();
        self.error = None;
        self.failed.clear();
        self.auto = true;
        let remembered = memory_file().and_then(|f| std::fs::read_to_string(f).ok());
        let dev = self
            .current
            .clone()
            .filter(|d| self.devices.contains(d))
            .or_else(|| self.devices.iter().find(|d| Some(d.name.as_str()) == remembered.as_deref().map(str::trim)).cloned())
            .or_else(|| self.devices.first().cloned());
        match dev {
            Some(d) => self.open(ctx, d),
            None => self.error = Some("Aucune caméra détectée.".into()),
        }
    }

    /// Appelé à la sortie du mode caméra : libère l'appareil.
    pub fn stop(&mut self) {
        self.stream = None;
        self.texture = None;
    }

    fn open(&mut self, ctx: &egui::Context, dev: DeviceInfo) {
        self.modes = list_modes(&dev);
        self.open_at(ctx, dev, None);
    }

    fn open_at(&mut self, ctx: &egui::Context, dev: DeviceInfo, size: Option<(u32, u32)>) {
        self.stream = None;
        self.texture = None;
        self.got_frame = false;
        let repaint = ctx.clone();
        let on_frame = move || repaint.request_repaint();
        let opened = match size {
            Some((w, h)) => CameraStream::open_with_resolution(&dev, w, h, on_frame),
            None => CameraStream::open(&dev, on_frame),
        };
        match opened {
            Ok(s) => {
                self.stream = Some(s);
                self.error = None;
            }
            Err(e) => self.error = Some(e.to_string()),
        }
        self.current = Some(dev);
    }

    /// Affiche l'aperçu. Renvoie l'image quand l'utilisateur capture
    /// (bouton ou barre d'espace).
    pub fn show(&mut self, ui: &mut Ui) -> Option<RgbaImage> {
        let ctx = ui.ctx().clone();
        let mut captured = None;

        ui.horizontal(|ui| {
            ui.label("Caméra :");
            let mut pick = None;
            egui::ComboBox::from_id_salt("camera_device")
                .selected_text(self.current.as_ref().map_or("—", |d| d.name.as_str()))
                .show_ui(ui, |ui| {
                    for d in &self.devices {
                        if ui.selectable_label(self.current.as_ref() == Some(d), &d.name).clicked() {
                            pick = Some(d.clone());
                        }
                    }
                });
            if let Some(d) = pick {
                self.auto = false;
                self.open(&ctx, d);
            }
            let res = self.stream.as_ref().and_then(|s| s.resolution());
            if self.modes.len() > 1 {
                let mut pick = None;
                egui::ComboBox::from_id_salt("camera_mode")
                    .selected_text(res.map_or("—".to_owned(), |(w, h)| format!("{w} × {h}")))
                    .show_ui(ui, |ui| {
                        for m in &self.modes {
                            let label = format!("{} × {}  ({:.0} i/s)", m.width, m.height, m.fps);
                            if ui.selectable_label(res == Some((m.width, m.height)), label).clicked() {
                                pick = Some((m.width, m.height));
                            }
                        }
                    });
                if let (Some(size), Some(d)) = (pick, self.current.clone()) {
                    self.auto = false;
                    self.open_at(&ctx, d, Some(size));
                }
            } else if let Some((w, h)) = res {
                ui.weak(format!("{w} × {h}"));
            }
            if ui.button("⟳").on_hover_text("Rechercher les caméras").clicked() {
                self.start(&ctx);
            }
        });

        if let Some(s) = &self.stream {
            self.error = s.error();
            if let Some(f) = s.take_frame() {
                self.got_frame = true;
                let img = ColorImage::from_rgba_unmultiplied([f.width as usize, f.height as usize], &f.rgba);
                match &mut self.texture {
                    Some(t) => t.set(img, TextureOptions::LINEAR),
                    None => self.texture = Some(ctx.load_texture("camera", img, TextureOptions::LINEAR)),
                }
            }
        }

        // Recherche automatique : une caméra qui échoue sans avoir donné
        // d'image (scanner 3D, flux inactif…) cède la place à la suivante.
        if self.auto && self.error.is_some() && !self.got_frame {
            if let Some(cur) = self.current.clone()
                && !self.failed.contains(&cur) {
                    self.failed.push(cur);
                }
            if let Some(next) = self.devices.iter().find(|d| !self.failed.contains(d)).cloned() {
                self.open(&ctx, next);
            } else {
                self.auto = false;
            }
        }

        let ready = self.texture.is_some() && self.stream.is_some();
        let space = ui.input(|i| i.key_pressed(egui::Key::Space));
        let mut clicked = false;
        egui::Panel::bottom("camera_capture").show(ui, |ui| {
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                let b = egui::Button::new(RichText::new("⏺  Capturer").size(20.0)).min_size(egui::vec2(220.0, 44.0));
                clicked = ui.add_enabled(ready, b).on_hover_text("Barre d'espace").clicked();
            });
            ui.add_space(8.0);
        });

        let rect = ui.available_rect_before_wrap();
        ui.painter().rect_filled(rect, 0.0, Color32::from_gray(28));
        if let Some(t) = &self.texture {
            let [w, h] = t.size();
            let s = (rect.width() / w as f32).min(rect.height() / h as f32);
            let r = Rect::from_center_size(rect.center(), egui::vec2(w as f32 * s, h as f32 * s));
            ui.painter().image(t.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        if let Some(e) = &self.error {
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, e, egui::FontId::proportional(18.0), Color32::from_rgb(255, 120, 110));
        } else if self.texture.is_none() {
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "Démarrage de la caméra…", egui::FontId::proportional(18.0), Color32::GRAY);
        }

        if ready && (clicked || space)
            && let Some(f) = self.stream.as_ref().and_then(|s| s.latest()) {
                captured = RgbaImage::from_raw(f.width, f.height, f.rgba.clone());
                if let (Some(file), Some(d)) = (memory_file(), &self.current) {
                    if let Some(dir) = file.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    let _ = std::fs::write(file, &d.name);
                }
            }
        captured
    }
}

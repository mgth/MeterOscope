//! Vue caméra en direct : choix de l'appareil, flux affiché dans la vue de
//! mesure (`Canvas`), pause, réticule, capture.

use crate::reticle::{self, Reticle};
use meteroscope_i18n::tr;
use egui::{Color32, ColorImage, Rect, TextureHandle, TextureOptions, Ui};
use image::RgbaImage;
use meteroscope_camera::{CameraStream, DeviceInfo, Frame, VideoMode, list_devices, list_modes};
use meteroscope_canvas::Canvas;
use meteroscope_core::Document;
use meteroscope_core::rescale::Reference;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Pixels nets au fort zoom, comme pour les images fixes.
const TEXTURE_OPTIONS: TextureOptions = TextureOptions { magnification: egui::TextureFilter::Nearest, ..TextureOptions::LINEAR };

#[derive(Default)]
pub struct CameraView {
    devices: Vec<DeviceInfo>,
    current: Option<DeviceInfo>,
    stream: Option<CameraStream>,
    texture: Option<TextureHandle>,
    /// Image actuellement dans la texture (celle que fige la capture).
    shown: Option<Arc<Frame>>,
    error: Option<String>,
    modes: Vec<VideoMode>,
    /// Le flux courant a déjà fourni au moins une image.
    got_frame: bool,
    /// Flux tout juste ouvert : sa première image est signalée par `poll`.
    fresh: bool,
    /// Appareils déjà essayés sans succès lors de la recherche automatique.
    failed: Vec<DeviceInfo>,
    /// Vrai tant que l'utilisateur n'a pas choisi lui-même une caméra.
    auto: bool,
    /// Flux gelé : la texture ne suit plus la caméra (partagé avec le fil de
    /// capture pour ne plus demander de rafraîchissement).
    paused: Arc<AtomicBool>,
    reticle: Option<Reticle>,
    /// L'autre document : celui du flux hors du mode caméra, celui de l'image
    /// fixe pendant le mode caméra (l'application les échange).
    pub other_doc: Document,
    /// Vue (zoom, outil, tracé) de l'autre mode, échangée de même.
    pub other_canvas: Canvas,
    /// Taille d'image et caméra auxquelles se rapportent les coordonnées des
    /// mesures du flux.
    pub reference: Reference,
    pub doc_device: Option<String>,
}

/// Première image d'un flux ouvert, ou image de taille différente de la
/// précédente : la résolution ou la caméra a pu changer.
pub struct Resized {
    pub size: (u32, u32),
    pub device: String,
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
        self.set_paused(false);
        let remembered = memory_file().and_then(|f| std::fs::read_to_string(f).ok());
        let dev = self
            .current
            .clone()
            .filter(|d| self.devices.contains(d))
            .or_else(|| self.devices.iter().find(|d| Some(d.name.as_str()) == remembered.as_deref().map(str::trim)).cloned())
            .or_else(|| self.devices.first().cloned());
        match dev {
            Some(d) => self.open(ctx, d),
            None => self.error = Some(tr!("camera.none").into()),
        }
    }

    /// Appelé à la sortie du mode caméra : libère l'appareil.
    pub fn stop(&mut self) {
        self.stream = None;
        self.texture = None;
        self.shown = None;
    }

    fn open(&mut self, ctx: &egui::Context, dev: DeviceInfo) {
        self.modes = list_modes(&dev);
        self.open_at(ctx, dev, None);
    }

    fn open_at(&mut self, ctx: &egui::Context, dev: DeviceInfo, size: Option<(u32, u32)>) {
        self.stream = None;
        self.got_frame = false;
        self.fresh = true;
        self.set_paused(false);
        let repaint = ctx.clone();
        let paused = self.paused.clone();
        // Redessin seulement à l'arrivée d'une image, et pas pendant la pause.
        let on_frame = move || {
            if !paused.load(Ordering::Relaxed) {
                repaint.request_repaint();
            }
        };
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

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    fn set_paused(&mut self, p: bool) {
        self.paused.store(p, Ordering::Relaxed);
    }

    pub fn reticle(&mut self) -> Reticle {
        *self.reticle.get_or_insert_with(reticle::load)
    }

    /// Texture du flux et taille de l'image, pour `Canvas::show`.
    pub fn texture(&self) -> Option<(&TextureHandle, [usize; 2])> {
        let f = self.shown.as_ref()?;
        Some((self.texture.as_ref()?, [f.width as usize, f.height as usize]))
    }

    /// Barre du haut : caméra, résolution, pause, réticule.
    pub fn controls(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| {
            ui.add_space(6.0);
            ui.label(tr!("camera.label"));
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
            if ui.button("⟳").on_hover_text(tr!("camera.rescan_tip")).clicked() {
                self.start(&ctx);
            }
            ui.separator();
            let paused = self.is_paused();
            let text = if paused { format!("▶  {}", tr!("camera.resume")) } else { format!("⏸  {}", tr!("camera.pause")) };
            if ui.add_enabled(self.stream.is_some(), egui::Button::new(text).selected(paused)).on_hover_text(tr!("camera.pause_tip")).clicked() {
                self.set_paused(!paused);
                ctx.request_repaint();
            }
            ui.separator();
            let mut r = self.reticle();
            ui.label(tr!("reticle.label"));
            egui::ComboBox::from_id_salt("camera_reticle").selected_text(r.label()).show_ui(ui, |ui| {
                for k in Reticle::ALL {
                    ui.selectable_value(&mut r, k, k.label());
                }
            });
            if Some(r) != self.reticle {
                self.reticle = Some(r);
                reticle::save(r);
            }
            if let Some(fps) = self.stream.as_ref().and_then(|s| s.measured_fps()).filter(|_| !paused) {
                ui.weak(format!("{fps:.0} i/s"));
            }
        });
    }

    /// Prend en compte la dernière image du flux (sauf en pause) et met la
    /// texture à jour sans la réallouer tant que la taille ne change pas.
    pub fn poll(&mut self, ctx: &egui::Context) -> Option<Resized> {
        let mut resized = None;
        if let Some(s) = &self.stream {
            self.error = s.error();
            if !self.is_paused()
                && let Some(f) = s.take_frame()
            {
                self.got_frame = true;
                let size = [f.width as usize, f.height as usize];
                // Images opaques : prémultipliée = non prémultipliée, simple copie.
                let img = ColorImage::from_rgba_premultiplied(size, &f.rgba);
                match &mut self.texture {
                    Some(t) if t.size() == size => t.set_partial([0, 0], img, TEXTURE_OPTIONS),
                    Some(t) => t.set(img, TEXTURE_OPTIONS),
                    None => self.texture = Some(ctx.load_texture("camera", img, TEXTURE_OPTIONS)),
                }
                let prev = self.shown.as_ref().map(|p| (p.width, p.height));
                if self.fresh || prev != Some((f.width, f.height)) {
                    self.fresh = false;
                    let device = self.current.as_ref().map(|d| d.name.clone()).unwrap_or_default();
                    resized = Some(Resized { size: (f.width, f.height), device });
                }
                self.shown = Some(f);
            }
        }

        // Recherche automatique : une caméra qui échoue sans avoir donné
        // d'image (scanner 3D, flux inactif…) cède la place à la suivante.
        if self.auto && self.error.is_some() && !self.got_frame {
            if let Some(cur) = self.current.clone()
                && !self.failed.contains(&cur)
            {
                self.failed.push(cur);
            }
            if let Some(next) = self.devices.iter().find(|d| !self.failed.contains(d)).cloned() {
                self.open(ctx, next);
            } else {
                self.auto = false;
            }
        }
        resized
    }

    /// Barre du bas : bouton de capture. Renvoie vrai s'il est cliqué.
    pub fn capture_bar(&mut self, ui: &mut Ui) -> bool {
        let ready = self.shown.is_some();
        let mut clicked = false;
        egui::Panel::bottom("camera_capture").show(ui, |ui| {
            ui.add_space(6.0);
            ui.vertical_centered(|ui| {
                let b = egui::Button::new(egui::RichText::new(format!("⏺  {}", tr!("camera.capture"))).size(18.0)).min_size(egui::vec2(220.0, 38.0));
                clicked = ui.add_enabled(ready, b).on_hover_text(tr!("camera.capture_tip")).clicked();
            });
            ui.add_space(6.0);
        });
        clicked
    }

    /// Réticule, mention de pause et messages (erreur, démarrage) par-dessus
    /// la vue `rect`.
    pub fn overlay(&mut self, ui: &Ui, rect: Rect, canvas: &Canvas, cal: &meteroscope_core::Calibration) {
        let p = ui.painter_at(rect);
        let kind = self.reticle();
        if let Some((_, size)) = self.texture() {
            reticle::paint(&p, canvas, size, cal, kind);
        }
        if self.is_paused() {
            let pos = rect.center_top() + egui::vec2(0.0, 12.0);
            let galley = p.layout_no_wrap(format!("⏸  {}", tr!("camera.paused")), egui::FontId::proportional(15.0), Color32::WHITE);
            let r = egui::Align2::CENTER_TOP.anchor_size(pos, galley.size()).expand2(egui::vec2(10.0, 4.0));
            p.rect_filled(r, 4.0, Color32::from_black_alpha(170));
            p.galley(r.min + egui::vec2(10.0, 4.0), galley, Color32::WHITE);
        }
        if let Some(e) = &self.error {
            p.text(rect.center(), egui::Align2::CENTER_CENTER, e, egui::FontId::proportional(18.0), Color32::from_rgb(255, 120, 110));
        } else if self.shown.is_none() {
            p.text(rect.center(), egui::Align2::CENTER_CENTER, tr!("camera.starting"), egui::FontId::proportional(18.0), Color32::GRAY);
        }
    }

    /// Copie de l'image affichée (figée par la capture), et mémorisation de
    /// la caméra utilisée.
    pub fn snapshot(&self) -> Option<RgbaImage> {
        let f = self.shown.as_ref()?;
        let img = RgbaImage::from_raw(f.width, f.height, f.rgba.clone())?;
        if let (Some(file), Some(d)) = (memory_file(), &self.current) {
            if let Some(dir) = file.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(file, &d.name);
        }
        Some(img)
    }
}

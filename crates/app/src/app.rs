//! Coquille de l'application : barre d'outils, panneaux, raccourcis, fichiers.

use crate::camera_view::CameraView;
use egui::{Align, Color32, ColorImage, Key, KeyboardShortcut, Layout, Modifiers, RichText, TextureHandle, TextureOptions, Ui};
use image::RgbaImage;
use mesura_canvas::Canvas;
use mesura_core::{Document, Tool};
use mesura_io as io;
use mesura_panels::CalibrationDialog;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[derive(PartialEq)]
enum Mode {
    Image,
    Camera,
}

pub struct MesuraApp {
    doc: Document,
    /// Image d'origine (pleine résolution, non réglée).
    image: Option<RgbaImage>,
    texture: Option<TextureHandle>,
    image_path: Option<PathBuf>,
    /// L'image en mémoire diffère du fichier `image_path` (pivotée, retournée).
    image_modified: bool,
    project_path: Option<PathBuf>,
    /// Révision du document au dernier enregistrement.
    saved_revision: u64,
    brightness: f32,
    contrast: f32,
    canvas: Canvas,
    calibration: CalibrationDialog,
    mode: Mode,
    camera: CameraView,
    toast: Option<(String, bool, Instant)>,
}

const CTRL: Modifiers = Modifiers::COMMAND;
const CTRL_SHIFT: Modifiers = Modifiers { shift: true, ..Modifiers::COMMAND };

impl MesuraApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        let mut app = Self {
            doc: Document::new(),
            image: None,
            texture: None,
            image_path: None,
            image_modified: false,
            project_path: None,
            saved_revision: 0,
            brightness: 0.0,
            contrast: 0.0,
            canvas: Canvas::default(),
            calibration: CalibrationDialog::default(),
            mode: Mode::Image,
            camera: CameraView::default(),
            toast: None,
        };
        if let Some(p) = initial {
            app.open_path(&cc.egui_ctx, &p);
        }
        app
    }

    // ---------- messages ----------

    fn info(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), false, Instant::now()));
    }
    fn error(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), true, Instant::now()));
    }
    fn report<T>(&mut self, r: anyhow::Result<T>, ok: &str) -> Option<T> {
        match r {
            Ok(v) => {
                if !ok.is_empty() {
                    self.info(ok);
                }
                Some(v)
            }
            Err(e) => {
                self.error(format!("{e:#}"));
                None
            }
        }
    }

    // ---------- image ----------

    /// (Re)construit la texture affichée à partir de l'image et des réglages.
    fn refresh_texture(&mut self, ctx: &egui::Context) {
        let Some(img) = &self.image else {
            self.texture = None;
            return;
        };
        let adjusted;
        let mut shown = if self.brightness != 0.0 || self.contrast != 0.0 {
            adjusted = io::adjust(img, self.brightness, self.contrast);
            &adjusted
        } else {
            img
        };
        // Les très grandes photos dépassent la taille de texture du GPU :
        // on affiche une version réduite, les mesures restent en pixels réels.
        let max = ctx.input(|i| i.max_texture_side).max(1024) as u32;
        let reduced;
        if shown.width().max(shown.height()) > max {
            let s = max as f32 / shown.width().max(shown.height()) as f32;
            let (w, h) = ((shown.width() as f32 * s) as u32, (shown.height() as f32 * s) as u32);
            reduced = image::imageops::resize(shown, w.max(1), h.max(1), image::imageops::FilterType::Triangle);
            shown = &reduced;
        }
        let ci = ColorImage::from_rgba_unmultiplied([shown.width() as usize, shown.height() as usize], shown.as_raw());
        let opts = TextureOptions { magnification: egui::TextureFilter::Nearest, ..TextureOptions::LINEAR };
        self.texture = Some(ctx.load_texture("image", ci, opts));
    }

    /// Installe une nouvelle image avec un document vierge.
    fn set_image(&mut self, ctx: &egui::Context, img: RgbaImage, path: Option<PathBuf>) {
        self.image = Some(img);
        self.image_path = path;
        self.image_modified = false;
        self.project_path = None;
        self.doc = Document::new();
        self.saved_revision = 0;
        self.brightness = 0.0;
        self.contrast = 0.0;
        self.mode_image();
        self.canvas.set_tool(Tool::Select);
        self.canvas.fit();
        self.refresh_texture(ctx);
    }

    fn mode_image(&mut self) {
        if self.mode == Mode::Camera {
            self.camera.stop();
            self.mode = Mode::Image;
        }
    }

    fn is_dirty(&self) -> bool {
        self.doc.revision() != self.saved_revision
    }

    // ---------- fichiers ----------

    fn open_path(&mut self, ctx: &egui::Context, path: &Path) {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if ext == io::PROJECT_EXTENSION {
            let Some(project) = self.report(io::load_project(path), "") else { return };
            let img = match &project.image_path {
                Some(p) => self.report(io::load_image(p), ""),
                None => None,
            };
            if let Some(img) = img {
                self.set_image(ctx, img, project.image_path.clone());
            }
            self.doc = project.document;
            self.saved_revision = self.doc.revision();
            self.project_path = Some(path.to_owned());
        } else if let Some(img) = self.report(io::load_image(path), "") {
            self.set_image(ctx, img, Some(path.to_owned()));
        }
    }

    fn open_dialog(&mut self, ctx: &egui::Context) {
        let mut all: Vec<&str> = io::IMAGE_EXTENSIONS.to_vec();
        all.push(io::PROJECT_EXTENSION);
        let picked = rfd::FileDialog::new()
            .set_title("Ouvrir une image")
            .add_filter("Images et projets", &all)
            .add_filter("Tous les fichiers", &["*"])
            .pick_file();
        if let Some(p) = picked {
            self.open_path(ctx, &p);
        }
    }

    fn default_stem(&self) -> String {
        self.image_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map_or_else(|| "capture".to_owned(), |s| s.to_string_lossy().into_owned())
    }

    fn save_dialog(&self, title: &str, suffix: &str, ext: &str) -> Option<PathBuf> {
        let mut d = rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(format!("{}{suffix}.{ext}", self.default_stem()))
            .add_filter(ext.to_uppercase(), &[ext]);
        if let Some(dir) = self.image_path.as_ref().and_then(|p| p.parent()) {
            d = d.set_directory(dir);
        }
        d.save_file()
    }

    /// Enregistre le projet (mesures). Une capture caméra sans fichier est
    /// d'abord enregistrée en PNG à côté du projet.
    fn save_project(&mut self, save_as: bool) {
        if self.image.is_none() {
            return;
        }
        let path = match (&self.project_path, save_as) {
            (Some(p), false) => p.clone(),
            _ => match self.save_dialog("Enregistrer le projet", "", io::PROJECT_EXTENSION) {
                Some(p) => p,
                None => return,
            },
        };
        // Capture caméra ou image pivotée/retournée : le projet doit pointer
        // vers un fichier identique à l'image mesurée, sans écraser l'original.
        if self.image_path.is_none() || self.image_modified {
            let mut png = path.with_extension("png");
            if self.image_modified && self.image_path.as_deref() == Some(png.as_path()) {
                png = path.with_file_name(format!("{}-image.png", self.default_stem()));
            }
            let r = io::save_image(self.image.as_ref().unwrap(), &png);
            if self.report(r, "").is_none() {
                return;
            }
            self.image_path = Some(png);
            self.image_modified = false;
        }
        let project = io::Project { image_path: self.image_path.clone(), document: self.doc.clone() };
        if self.report(io::save_project(&project, &path), "Projet enregistré").is_some() {
            self.saved_revision = self.doc.revision();
            self.project_path = Some(path);
        }
    }

    fn export_csv(&mut self) {
        if let Some(p) = self.save_dialog("Exporter les mesures", "-mesures", "csv") {
            let r = io::export_csv(&self.doc, &p);
            self.report(r, "Mesures exportées");
        }
    }

    fn export_image(&mut self) {
        let Some(img) = &self.image else { return };
        if let Some(p) = self.save_dialog("Exporter l'image annotée", "-mesures", "png") {
            let base = io::adjust(img, self.brightness, self.contrast);
            let r = io::save_image(&io::render_annotated(&base, &self.doc), &p);
            self.report(r, "Image exportée");
        }
    }

    fn transform(&mut self, ctx: &egui::Context, t: io::Transform) {
        if let Some(img) = &self.image {
            self.image = Some(io::apply_transform(img, &mut self.doc, t));
            self.image_modified = true;
            self.canvas.fit();
            self.refresh_texture(ctx);
        }
    }

    fn toggle_camera(&mut self, ctx: &egui::Context) {
        if self.mode == Mode::Camera {
            self.mode_image();
        } else {
            self.mode = Mode::Camera;
            self.camera.start(ctx);
        }
    }

    // ---------- raccourcis ----------

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let hit = |m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(m, k)));
        if hit(CTRL, Key::O) {
            self.open_dialog(ctx);
        }
        if hit(CTRL_SHIFT, Key::S) {
            self.save_project(true);
        } else if hit(CTRL, Key::S) {
            self.save_project(false);
        }
        if hit(CTRL, Key::E) {
            self.export_csv();
        }
        if hit(CTRL_SHIFT, Key::Z) || hit(CTRL, Key::Y) {
            self.doc.redo();
        } else if hit(CTRL, Key::Z) {
            self.doc.undo();
        }
        if hit(CTRL, Key::Num0) {
            self.canvas.fit();
        }
        if hit(CTRL, Key::Num1) {
            self.canvas.zoom_actual();
        }

        // Touches simples : seulement si aucun champ de texte n'a le focus.
        if ctx.egui_wants_keyboard_input() || self.calibration.is_open() || self.mode == Mode::Camera {
            if self.mode == Mode::Camera && ctx.input(|i| i.key_pressed(Key::Escape)) {
                self.mode_image();
            }
            return;
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            if self.canvas.is_drawing() {
                self.canvas.cancel();
            } else if self.canvas.tool() != Tool::Select {
                self.canvas.set_tool(Tool::Select);
            } else {
                self.doc.selected = None;
            }
        }
        if ctx.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace)) && !self.canvas.is_drawing()
            && let Some(id) = self.doc.selected {
                self.doc.remove(id);
            }
        if ctx.input(|i| i.modifiers.is_none()) {
            let tools = std::iter::once(Tool::Select)
                .chain(std::iter::once(Tool::Calibrate))
                .chain(mesura_core::ShapeKind::ALL.into_iter().map(Tool::Draw));
            for t in tools {
                if mesura_panels::tool_shortcut(t).is_some_and(|k| ctx.input(|i| i.key_pressed(k))) {
                    self.canvas.set_tool(t);
                }
            }
        }
    }

    // ---------- interface ----------

    fn toolbar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let has_image = self.image.is_some() && self.mode == Mode::Image;
        ui.horizontal(|ui| {
            ui.spacing_mut().button_padding = egui::vec2(10.0, 6.0);
            if ui.button("📂  Ouvrir").on_hover_text("Ouvrir une image ou un projet (Ctrl+O)").clicked() {
                self.open_dialog(&ctx);
            }
            if ui.selectable_label(self.mode == Mode::Camera, "📷  Caméra").on_hover_text("Capturer depuis une caméra").clicked() {
                self.toggle_camera(&ctx);
            }
            ui.separator();
            ui.add_enabled_ui(has_image, |ui| {
                let dirty = if self.is_dirty() { " •" } else { "" };
                if ui.button(format!("💾  Enregistrer{dirty}")).on_hover_text("Enregistrer le projet (Ctrl+S)").clicked() {
                    self.save_project(false);
                }
                ui.menu_button("⬇  Exporter", |ui| {
                    if ui.button("Mesures en CSV…  (Ctrl+E)").clicked() {
                        self.export_csv();
                    }
                    if ui.button("Image annotée…").clicked() {
                        self.export_image();
                    }
                    if ui.button("Copier le tableau").clicked() {
                        ui.ctx().copy_text(io::clipboard_table(&self.doc));
                        self.info("Tableau copié");
                    }
                });
                ui.separator();
                if ui.add_enabled(self.doc.can_undo(), egui::Button::new("↩")).on_hover_text("Annuler (Ctrl+Z)").clicked() {
                    self.doc.undo();
                }
                if ui.add_enabled(self.doc.can_redo(), egui::Button::new("↪")).on_hover_text("Rétablir (Ctrl+Y)").clicked() {
                    self.doc.redo();
                }
                ui.separator();
                ui.menu_button("🖼  Image", |ui| {
                    for (label, t) in [
                        ("⟲  Pivoter à gauche", io::Transform::RotateLeft),
                        ("⟳  Pivoter à droite", io::Transform::RotateRight),
                        ("⇆  Miroir horizontal", io::Transform::FlipHorizontal),
                        ("⇅  Miroir vertical", io::Transform::FlipVertical),
                    ] {
                        if ui.button(label).clicked() {
                            self.transform(&ctx, t);
                        }
                    }
                    ui.separator();
                    let mut changed = false;
                    changed |= ui.add(egui::Slider::new(&mut self.brightness, -1.0..=1.0).text("Luminosité")).changed();
                    changed |= ui.add(egui::Slider::new(&mut self.contrast, -1.0..=1.0).text("Contraste")).changed();
                    if ui.button("Réinitialiser").clicked() {
                        self.brightness = 0.0;
                        self.contrast = 0.0;
                        changed = true;
                    }
                    if changed {
                        self.refresh_texture(&ctx);
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("1:1").on_hover_text("Taille réelle (Ctrl+1)").clicked() {
                        self.canvas.zoom_actual();
                    }
                    if ui.button("Ajuster").on_hover_text("Voir toute l'image (Ctrl+0)").clicked() {
                        self.canvas.fit();
                    }
                    if ui.button("➕").on_hover_text("Zoom avant (molette)").clicked() {
                        self.canvas.zoom_by(1.25);
                    }
                    ui.label(format!("{:.0} %", self.canvas.zoom() * 100.0));
                    if ui.button("➖").on_hover_text("Zoom arrière (molette)").clicked() {
                        self.canvas.zoom_by(0.8);
                    }
                });
            });
        });
    }

    fn status_bar(&mut self, ui: &mut Ui, cursor: Option<mesura_core::Pt>) {
        ui.horizontal(|ui| {
            match &self.toast {
                Some((msg, is_err, t)) if t.elapsed() < Duration::from_secs(5) => {
                    let color = if *is_err { Color32::from_rgb(255, 120, 110) } else { ui.visuals().strong_text_color() };
                    ui.label(RichText::new(msg).color(color));
                    ui.ctx().request_repaint_after(Duration::from_secs(1));
                }
                _ => {
                    ui.label(self.canvas.hint());
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(img) = &self.image {
                    ui.label(format!("{} × {} px", img.width(), img.height()));
                    ui.separator();
                }
                if let Some(p) = cursor {
                    ui.label(format!("x {:.0}  y {:.0}", p.x, p.y));
                }
            });
        });
    }

    fn welcome(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.vertical_centered(|ui| {
            ui.add_space((ui.available_height() * 0.3).max(20.0));
            ui.label(RichText::new("Mesura").size(34.0).strong());
            ui.label("Mesurez des longueurs, angles et surfaces sur une image.");
            ui.add_space(24.0);
            let big = |text: &str| egui::Button::new(RichText::new(text).size(18.0)).min_size(egui::vec2(260.0, 48.0));
            if ui.add(big("📂  Ouvrir une image")).clicked() {
                self.open_dialog(&ctx);
            }
            ui.add_space(8.0);
            if ui.add(big("📷  Capturer avec la caméra")).clicked() {
                self.toggle_camera(&ctx);
            }
            ui.add_space(16.0);
            ui.weak("… ou glissez une image dans la fenêtre");
        });
    }
}

impl eframe::App for MesuraApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Fichier glissé-déposé.
        if let Some(p) = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_owned())) {
            self.open_path(&ctx, &p);
        }
        self.shortcuts(&ctx);

        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.add_space(4.0);
            self.toolbar(ui);
            ui.add_space(4.0);
        });

        let show_tools = self.image.is_some() && self.mode == Mode::Image;
        let mut cursor = None;
        // La barre d'état est déclarée avant le centre mais affiche le curseur de la frame précédente.
        let cursor_id = egui::Id::unique("mesura_cursor");
        let last_cursor: Option<mesura_core::Pt> = ctx.data(|d| d.get_temp(cursor_id)).flatten();
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui, last_cursor));

        if show_tools {
            egui::Panel::left("tools").resizable(false).show(ui, |ui| {
                let mut tool = self.canvas.tool();
                mesura_panels::tool_palette(ui, &mut tool);
                if tool != self.canvas.tool() {
                    self.canvas.set_tool(tool);
                }
            });
            egui::Panel::right("measurements").default_size(300.0).show(ui, |ui| {
                let out = mesura_panels::measurements_panel(ui, &mut self.doc);
                if out.start_calibration {
                    self.canvas.set_tool(Tool::Calibrate);
                }
                if out.copy_table {
                    ui.ctx().copy_text(io::clipboard_table(&self.doc));
                    self.info("Tableau copié");
                }
                if out.export_csv {
                    self.export_csv();
                }
            });
        }

        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| match self.mode {
            Mode::Camera => {
                if let Some(img) = self.camera.show(ui) {
                    self.set_image(&ctx, img, None);
                    self.info("Image capturée");
                }
            }
            Mode::Image if self.image.is_none() => self.welcome(ui),
            Mode::Image => {
                let size = self.image.as_ref().map(|i| [i.width() as usize, i.height() as usize]).unwrap();
                let out = self.canvas.show(ui, &mut self.doc, self.texture.as_ref().map(|t| (t, size)));
                cursor = out.cursor;
                if let Some((a, b)) = out.calibration_segment {
                    self.calibration.open(a.dist(b));
                }
            }
        });
        ctx.data_mut(|d| d.insert_temp(cursor_id, cursor));

        if self.calibration.show(&ctx, &mut self.doc) {
            self.canvas.set_tool(Tool::Select);
        }

        // Titre de fenêtre.
        let name = self.project_path.as_ref().or(self.image_path.as_ref()).and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());
        let title = match name {
            Some(n) => format!("{n}{} — Mesura", if self.is_dirty() { " •" } else { "" }),
            None => "Mesura".to_owned(),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
    }
}

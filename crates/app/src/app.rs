//! Coquille de l'application : barre d'outils, panneaux, raccourcis, fichiers.

use meteroscope_i18n::tr;
use crate::camera_view::CameraView;
use egui::{Align, Color32, ColorImage, Key, KeyboardShortcut, Layout, Modifiers, RichText, TextureHandle, TextureOptions, Ui};
use image::RgbaImage;
use meteroscope_canvas::Canvas;
use meteroscope_core::{Document, Orientation, Tool};
use meteroscope_io as io;
use meteroscope_panels::CalibrationDialog;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

#[derive(PartialEq)]
enum Mode {
    Image,
    Camera,
}

pub struct MeterOscopeApp {
    doc: Document,
    /// Image d'origine (pleine résolution, non réglée). Partagée avec le fil
    /// qui calcule le redressement.
    image: Option<Arc<RgbaImage>>,
    /// Incrémenté chaque fois que les pixels de `image` changent.
    image_gen: u64,
    /// Orientation (quarts de tour, miroirs) dans laquelle se trouvent les
    /// pixels de `image` ; celle du document fait foi après une annulation.
    applied: Orientation,
    /// Orientation de l'image telle qu'elle est dans le fichier `image_path`.
    file_orientation: Orientation,
    texture: Option<TextureHandle>,
    /// Filtre de redressement (`None` : rotation par la carte graphique).
    filter: Option<io::RotationFilter>,
    /// Ce que contient `texture` : génération de l'image, et rotation et
    /// filtre si elle est redressée (`NO_KEY` : droite, pivotée à l'affichage).
    texture_state: (u64, TexKey),
    /// Image redressée (avant luminosité/contraste), gardée tant que l'image,
    /// l'angle et le filtre ne changent pas.
    straight: Option<Straight>,
    /// Redressement en cours de calcul hors du fil d'interface.
    job: Option<Job>,
    /// Calcul qui a échoué : on ne le relance pas, l'aperçu GPU reste.
    failed: Option<(u64, TexKey)>,
    image_path: Option<PathBuf>,
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
    /// Réglages retenus d'un lancement à l'autre (récents, unité, décimales).
    settings: crate::settings::Settings,
}

/// Rotation et filtre d'une texture redressée.
type TexKey = (f64, Option<io::RotationFilter>);
/// Texture non redressée.
const NO_KEY: TexKey = (0.0, None);

struct Straight {
    key: TexKey,
    generation: u64,
    image: RgbaImage,
    /// Taille de l'image redressée, en pixels de l'image d'origine.
    size: egui::Vec2,
}

struct Job {
    key: TexKey,
    generation: u64,
    rx: mpsc::Receiver<io::Straightened>,
}

const CTRL: Modifiers = Modifiers::COMMAND;
const CTRL_SHIFT: Modifiers = Modifiers { shift: true, ..Modifiers::COMMAND };

impl MeterOscopeApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        let mut app = Self {
            doc: Document::new(),
            image: None,
            image_gen: 0,
            applied: Orientation::IDENTITY,
            file_orientation: Orientation::IDENTITY,
            texture: None,
            filter: crate::prefs::load_filter(),
            texture_state: (0, NO_KEY),
            straight: None,
            job: None,
            failed: None,
            image_path: None,
            project_path: None,
            saved_revision: 0,
            brightness: 0.0,
            contrast: 0.0,
            canvas: Canvas::default(),
            calibration: CalibrationDialog::default(),
            mode: Mode::Image,
            camera: CameraView::default(),
            toast: None,
            settings: crate::settings::Settings::load(),
        };
        meteroscope_core::measure::set_decimals(app.settings.decimals);
        if let Some(u) = &app.settings.calibration_unit {
            app.calibration.set_unit(u);
        }
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

    /// Rotation et filtre dont dépend la texture : sans filtre, la texture
    /// reste droite et c'est l'affichage qui la fait pivoter.
    fn wanted_key(&self) -> TexKey {
        match self.filter {
            Some(f) if self.doc.rotation != 0.0 => (self.doc.rotation, Some(f)),
            _ => NO_KEY,
        }
    }

    /// L'image redressée demandée est prête.
    fn have_straight(&self) -> bool {
        let key = self.wanted_key();
        self.straight.as_ref().is_some_and(|s| s.key == key && s.generation == self.image_gen)
    }

    /// Ce que la texture devrait contenir avec ce qui est disponible.
    fn target_state(&self) -> (u64, TexKey) {
        (self.image_gen, if self.have_straight() { self.wanted_key() } else { NO_KEY })
    }

    /// Un redressement est à calculer et aucun calcul n'occupe le fil de travail.
    fn should_start_job(&self) -> bool {
        let key = self.wanted_key();
        self.image.is_some() && key.1.is_some() && !self.have_straight() && self.job.is_none() && self.failed != Some((self.image_gen, key))
    }

    /// La texture n'est pas (encore) celle qu'il faut.
    fn texture_pending(&self) -> bool {
        self.target_state() != self.texture_state || self.should_start_job()
    }

    /// Les pixels de `image` viennent de changer.
    fn pixels_changed(&mut self) {
        self.image_gen += 1;
        self.straight = None;
    }

    /// Remet les pixels dans l'orientation du document : un quart de tour ou
    /// un miroir annulé (ou rétabli) revient avec les mesures et l'angle.
    fn sync_orientation(&mut self) {
        let Some(img) = &self.image else { return };
        if let Some(out) = io::sync_orientation(img, &mut self.applied, &self.doc) {
            self.image = Some(Arc::new(out));
            self.pixels_changed();
            self.canvas.fit();
        }
    }

    /// L'image en mémoire diffère du fichier `image_path` (pivotée, retournée).
    fn image_modified(&self) -> bool {
        self.applied != self.file_orientation
    }

    /// Lance le calcul de l'image redressée hors du fil d'interface.
    fn start_job(&mut self, ctx: &egui::Context) {
        let key = self.wanted_key();
        let (Some(img), (deg, Some(filter))) = (&self.image, key) else { return };
        let max = ctx.input(|i| i.max_texture_side).max(1024) as u32;
        let (tx, rx) = mpsc::channel();
        let (img, ctx) = (Arc::clone(img), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(io::straighten(&img, deg, filter, max, io::WORK_BUDGET));
            ctx.request_repaint();
        });
        self.job = Some(Job { key, generation: self.image_gen, rx });
    }

    /// Tient la texture à jour sans bloquer l'interface : le redressement
    /// filtré se calcule en arrière-plan, et en attendant c'est la carte
    /// graphique qui fait pivoter la photo. Le résultat n'est retenu que si
    /// l'image, l'angle et le filtre sont toujours ceux de la demande.
    fn update_texture(&mut self, ctx: &egui::Context) {
        if let Some(job) = &self.job {
            match job.rx.try_recv() {
                Ok(s) => {
                    let job = self.job.take().unwrap();
                    if job.generation == self.image_gen && job.key == self.wanted_key() {
                        let size = egui::vec2(s.image.width() as f32, s.image.height() as f32) / s.frame.scale as f32;
                        self.straight = Some(Straight { key: job.key, generation: job.generation, image: s.image, size });
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => {
                    let job = self.job.take().unwrap();
                    self.failed = Some((job.generation, job.key));
                }
            }
        }
        // Un seul calcul à la fois : une demande périmée se termine d'abord.
        if self.should_start_job() {
            self.start_job(ctx);
        }
        if self.target_state() != self.texture_state {
            self.refresh_texture(ctx);
        }
    }

    /// (Re)construit la texture affichée à partir de l'image, du redressement
    /// disponible et des réglages.
    fn refresh_texture(&mut self, ctx: &egui::Context) {
        self.texture_state = self.target_state();
        let Some(img) = &self.image else {
            self.texture = None;
            self.straight = None;
            self.canvas.set_baked(None);
            return;
        };
        let img: &RgbaImage = img;
        let reduced;
        let base = match &self.straight {
            Some(s) if self.texture_state.1 != NO_KEY => {
                self.canvas.set_baked(Some(s.size));
                &s.image
            }
            _ => {
                self.canvas.set_baked(None);
                // Les très grandes photos dépassent la taille de texture du GPU :
                // on affiche une version réduite, les mesures restent en pixels réels.
                let max = ctx.input(|i| i.max_texture_side).max(1024) as f64;
                let (w, h, k) = io::reduced_size(img.width(), img.height(), max / img.width().max(img.height()) as f64);
                if k < 1.0 {
                    reduced = image::imageops::resize(img, w, h, image::imageops::FilterType::Triangle);
                    &reduced
                } else {
                    img
                }
            }
        };
        let adjusted;
        let shown = if self.brightness != 0.0 || self.contrast != 0.0 {
            adjusted = io::adjust(base, self.brightness, self.contrast);
            &adjusted
        } else {
            base
        };
        let ci = ColorImage::from_rgba_unmultiplied([shown.width() as usize, shown.height() as usize], shown.as_raw());
        let opts = TextureOptions { magnification: egui::TextureFilter::Nearest, ..TextureOptions::LINEAR };
        self.texture = Some(ctx.load_texture("image", ci, opts));
    }

    fn set_filter(&mut self, filter: Option<io::RotationFilter>) {
        if filter != self.filter {
            self.filter = filter;
            crate::prefs::save_filter(filter);
        }
    }

    /// Installe une nouvelle image avec un document vierge.
    fn set_image(&mut self, ctx: &egui::Context, img: RgbaImage, path: Option<PathBuf>) {
        self.image = Some(Arc::new(img));
        self.pixels_changed();
        self.image_path = path;
        self.project_path = None;
        self.doc = Document::new();
        self.applied = Orientation::IDENTITY;
        self.file_orientation = Orientation::IDENTITY;
        self.saved_revision = 0;
        self.brightness = 0.0;
        self.contrast = 0.0;
        self.mode_image();
        self.canvas.set_tool(Tool::Select);
        self.canvas.fit();
        // La texture se construit à la prochaine frame de la vue image.
        ctx.request_repaint();
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
            // Le fichier image du projet est déjà dans l'orientation mesurée.
            self.applied = self.doc.orientation;
            self.file_orientation = self.doc.orientation;
            self.saved_revision = self.doc.revision();
            self.project_path = Some(path.to_owned());
            self.settings.add_recent(path);
        } else if let Some(img) = self.report(io::load_image(path), "") {
            self.set_image(ctx, img, Some(path.to_owned()));
            self.settings.add_recent(path);
        }
    }

    fn open_dialog(&mut self, ctx: &egui::Context) {
        let mut all: Vec<&str> = io::IMAGE_EXTENSIONS.to_vec();
        all.push(io::PROJECT_EXTENSION);
        let mut dialog = rfd::FileDialog::new();
        if let Some(dir) = self.settings.dialog_dir() {
            dialog = dialog.set_directory(dir);
        }
        let picked = dialog
            .set_title(tr!("dialog.open_title"))
            .add_filter(tr!("dialog.filter_images"), &all)
            .add_filter(tr!("dialog.filter_all"), &["*"])
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

    /// Demande où enregistrer, dans le dernier dossier utilisé (sinon celui de l'image).
    fn save_dialog(&mut self, title: &str, suffix: &str, ext: &str) -> Option<PathBuf> {
        let mut d = rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(format!("{}{suffix}.{ext}", self.default_stem()))
            .add_filter(ext.to_uppercase(), &[ext]);
        if let Some(dir) = self.settings.dialog_dir().or(self.image_path.as_ref().and_then(|p| p.parent())) {
            d = d.set_directory(dir);
        }
        let picked = d.save_file();
        if let Some(p) = &picked {
            self.settings.set_last_dir_of(p);
        }
        picked
    }

    /// Enregistre le projet (mesures). Une capture caméra sans fichier est
    /// d'abord enregistrée en PNG à côté du projet.
    fn save_project(&mut self, save_as: bool) {
        self.sync_orientation();
        if self.image.is_none() {
            return;
        }
        let path = match (&self.project_path, save_as) {
            (Some(p), false) => p.clone(),
            _ => match self.save_dialog(tr!("dialog.save_project"), "", io::PROJECT_EXTENSION) {
                Some(p) => p,
                None => return,
            },
        };
        // Capture caméra ou image pivotée/retournée : le projet doit pointer
        // vers un fichier identique à l'image mesurée, sans écraser l'original.
        if self.image_path.is_none() || self.image_modified() {
            let mut png = path.with_extension("png");
            if self.image_modified() && self.image_path.as_deref() == Some(png.as_path()) {
                png = path.with_file_name(format!("{}-image.png", self.default_stem()));
            }
            let r = io::save_image(self.image.as_ref().unwrap(), &png);
            if self.report(r, "").is_none() {
                return;
            }
            self.image_path = Some(png);
            self.file_orientation = self.applied;
        }
        let project = io::Project { image_path: self.image_path.clone(), document: self.doc.clone() };
        if self.report(io::save_project(&project, &path), tr!("msg.project_saved")).is_some() {
            self.saved_revision = self.doc.revision();
            self.settings.add_recent(&path);
            self.project_path = Some(path);
        }
    }

    fn export_csv(&mut self) {
        if let Some(p) = self.save_dialog(tr!("dialog.export_csv"), tr!("file.measurements_suffix"), "csv") {
            let r = io::export_csv(&self.doc, &p);
            self.report(r, tr!("msg.csv_exported"));
        }
    }

    /// Exporte l'image annotée telle que la vue la montre, redressement compris
    /// (avec le filtre choisi, ou celui par défaut si l'affichage pivote par
    /// la carte graphique).
    fn export_image(&mut self) {
        self.sync_orientation();
        if self.image.is_none() {
            return;
        }
        if let Some(p) = self.save_dialog(tr!("dialog.export_image"), tr!("file.measurements_suffix"), "png") {
            let Some(img) = &self.image else { return };
            let base = io::adjust(img, self.brightness, self.contrast);
            let r = io::save_image(&io::render_view(&base, &self.doc, self.filter.unwrap_or_default()), &p);
            self.report(r, tr!("msg.image_exported"));
        }
    }

    /// Quart de tour ou miroir : pixels, mesures et redressement changent
    /// ensemble, en une seule étape d'historique.
    fn transform(&mut self, t: io::Transform) {
        self.sync_orientation();
        if let Some(img) = &self.image {
            self.image = Some(Arc::new(io::apply_transform(img, &mut self.doc, t)));
            self.applied = self.doc.orientation;
            self.pixels_changed();
            self.canvas.fit();
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
            for t in meteroscope_panels::TOOLS {
                if meteroscope_panels::tool_shortcut(t).is_some_and(|k| ctx.input(|i| i.key_pressed(k))) {
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
            if ui.button(format!("📂  {}", tr!("toolbar.open"))).on_hover_text(tr!("toolbar.open_tip")).clicked() {
                self.open_dialog(&ctx);
            }
            if let Some(p) = crate::settings::recent_menu(ui, &mut self.settings) {
                self.open_path(&ctx, &p);
            }
            if ui.selectable_label(self.mode == Mode::Camera, format!("📷  {}", tr!("toolbar.camera"))).on_hover_text(tr!("toolbar.camera_tip")).clicked() {
                self.toggle_camera(&ctx);
            }
            ui.separator();
            ui.add_enabled_ui(has_image, |ui| {
                let dirty = if self.is_dirty() { " •" } else { "" };
                if ui.button(format!("💾  {}{dirty}", tr!("toolbar.save"))).on_hover_text(tr!("toolbar.save_tip")).clicked() {
                    self.save_project(false);
                }
                ui.menu_button(format!("⬇  {}", tr!("toolbar.export")), |ui| {
                    if ui.button(tr!("toolbar.export_csv")).clicked() {
                        self.export_csv();
                    }
                    if ui.button(tr!("toolbar.export_image")).clicked() {
                        self.export_image();
                    }
                    if ui.button(tr!("toolbar.copy_table")).clicked() {
                        ui.ctx().copy_text(io::clipboard_table(&self.doc));
                        self.info(tr!("msg.table_copied"));
                    }
                });
                ui.separator();
                if ui.add_enabled(self.doc.can_undo(), egui::Button::new("↩")).on_hover_text(tr!("toolbar.undo_tip")).clicked() {
                    self.doc.undo();
                }
                if ui.add_enabled(self.doc.can_redo(), egui::Button::new("↪")).on_hover_text(tr!("toolbar.redo_tip")).clicked() {
                    self.doc.redo();
                }
                ui.separator();
                ui.menu_button(format!("🖼  {}", tr!("toolbar.image")), |ui| {
                    for (label, t) in [
                        (format!("⟲  {}", tr!("image.rotate_left")), io::Transform::RotateLeft),
                        (format!("⟳  {}", tr!("image.rotate_right")), io::Transform::RotateRight),
                        (format!("⇆  {}", tr!("image.flip_h")), io::Transform::FlipHorizontal),
                        (format!("⇅  {}", tr!("image.flip_v")), io::Transform::FlipVertical),
                    ] {
                        if ui.button(label).clicked() {
                            self.transform(t);
                        }
                    }
                    ui.separator();
                    if ui.button(format!("📐  {}", tr!("image.straighten"))).on_hover_text(tr!("tool_help.straighten")).clicked() {
                        self.canvas.set_tool(Tool::Straighten);
                        ui.close();
                    }
                    let reset = ui.add_enabled(self.doc.rotation != 0.0, egui::Button::new(tr!("image.straighten_reset")));
                    if reset.clicked() {
                        self.doc.set_rotation(0.0);
                    }
                    ui.menu_button(tr!("image.filter"), |ui| {
                        let mut wanted = self.filter;
                        ui.radio_value(&mut wanted, None, tr!("filter.gpu")).on_hover_text(tr!("filter.gpu_tip"));
                        ui.separator();
                        for f in io::RotationFilter::ALL {
                            let (label, tip) = filter_texts(f);
                            ui.radio_value(&mut wanted, Some(f), label).on_hover_text(tip);
                        }
                        self.set_filter(wanted);
                    });
                    ui.separator();
                    let mut changed = false;
                    changed |= ui.add(egui::Slider::new(&mut self.brightness, -1.0..=1.0).text(tr!("image.brightness"))).changed();
                    changed |= ui.add(egui::Slider::new(&mut self.contrast, -1.0..=1.0).text(tr!("image.contrast"))).changed();
                    if ui.button(tr!("image.reset")).clicked() {
                        self.brightness = 0.0;
                        self.contrast = 0.0;
                        changed = true;
                    }
                    if changed {
                        self.refresh_texture(&ctx);
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("1:1").on_hover_text(tr!("zoom.actual_tip")).clicked() {
                        self.canvas.zoom_actual();
                    }
                    if ui.button(tr!("zoom.fit")).on_hover_text(tr!("zoom.fit_tip")).clicked() {
                        self.canvas.fit();
                    }
                    if ui.button("➕").on_hover_text(tr!("zoom.in_tip")).clicked() {
                        self.canvas.zoom_by(1.25);
                    }
                    ui.label(format!("{:.0} %", self.canvas.zoom() * 100.0));
                    if ui.button("➖").on_hover_text(tr!("zoom.out_tip")).clicked() {
                        self.canvas.zoom_by(0.8);
                    }
                });
            });
        });
    }

    fn status_bar(&mut self, ui: &mut Ui, cursor: Option<meteroscope_core::Pt>) {
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
                crate::language::selector(ui);
                crate::settings::decimals_selector(ui, &mut self.settings);
                ui.separator();
                if let Some(img) = &self.image {
                    ui.label(format!("{} × {} px", img.width(), img.height()));
                    ui.separator();
                }
                if self.image.is_some() && self.doc.rotation != 0.0 {
                    let r = self.doc.rotation;
                    let text = tr!("status.rotation", angle = meteroscope_core::measure::format_number(r.abs()));
                    let resp = ui.add(egui::Label::new(text).sense(egui::Sense::click())).on_hover_text(tr!("status.rotation_tip"));
                    if resp.double_clicked() {
                        self.doc.set_rotation(0.0);
                    }
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
            ui.label(RichText::new("MeterOscope").size(34.0).strong());
            ui.label(tr!("welcome.tagline"));
            ui.add_space(24.0);
            let big = |text: &str| egui::Button::new(RichText::new(text).size(18.0)).min_size(egui::vec2(260.0, 48.0));
            if ui.add(big(&format!("📂  {}", tr!("welcome.open")))).clicked() {
                self.open_dialog(&ctx);
            }
            ui.add_space(8.0);
            if ui.add(big(&format!("📷  {}", tr!("welcome.camera")))).clicked() {
                self.toggle_camera(&ctx);
            }
            ui.add_space(16.0);
            ui.weak(tr!("welcome.drop"));
            if let Some(p) = crate::settings::welcome_recent(ui, &self.settings) {
                self.open_path(&ctx, &p);
            }
        });
    }
}

/// Nom et description d'un filtre de redressement.
fn filter_texts(f: io::RotationFilter) -> (&'static str, &'static str) {
    use io::RotationFilter as F;
    match f {
        F::Bicubic => (tr!("filter.bicubic"), tr!("filter.bicubic_tip")),
        F::Lanczos3 => (tr!("filter.lanczos3"), tr!("filter.lanczos3_tip")),
        F::BSpline => (tr!("filter.bspline"), tr!("filter.bspline_tip")),
        F::Shear => (tr!("filter.shear"), tr!("filter.shear_tip")),
    }
}

impl eframe::App for MeterOscopeApp {
    /// Seule la fenêtre est retenue par eframe, pas l'état des widgets.
    fn persist_egui_memory(&self) -> bool {
        false
    }

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
        let cursor_id = egui::Id::unique("meteroscope_cursor");
        let last_cursor: Option<meteroscope_core::Pt> = ctx.data(|d| d.get_temp(cursor_id)).flatten();
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui, last_cursor));

        if show_tools {
            egui::Panel::left("tools").resizable(false).show(ui, |ui| {
                let mut tool = self.canvas.tool();
                meteroscope_panels::tool_palette(ui, &mut tool);
                if tool != self.canvas.tool() {
                    self.canvas.set_tool(tool);
                }
            });
            egui::Panel::right("measurements").default_size(300.0).show(ui, |ui| {
                let out = meteroscope_panels::measurements_panel(ui, &mut self.doc);
                if out.start_calibration {
                    self.canvas.set_tool(Tool::Calibrate);
                }
                if out.copy_table {
                    ui.ctx().copy_text(io::clipboard_table(&self.doc));
                    self.info(tr!("msg.table_copied"));
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
                    self.info(tr!("msg.image_captured"));
                }
            }
            Mode::Image if self.image.is_none() => self.welcome(ui),
            Mode::Image => {
                // Quart de tour ou miroir annulé, redressement ou filtre changé
                // (outil, annulation, projet ouvert…).
                self.sync_orientation();
                self.update_texture(&ctx);
                let size = self.image.as_ref().map(|i| [i.width() as usize, i.height() as usize]).unwrap();
                let out = self.canvas.show(ui, &mut self.doc, self.texture.as_ref().map(|t| (t, size)));
                cursor = out.cursor;
                if self.texture_pending() {
                    ctx.request_repaint();
                }
                if let Some((a, b)) = out.calibration_segment {
                    self.calibration.open(a.dist(b));
                }
            }
        });
        ctx.data_mut(|d| d.insert_temp(cursor_id, cursor));

        if self.calibration.show(&ctx, &mut self.doc) {
            self.canvas.set_tool(Tool::Select);
            self.settings.set_calibration_unit(self.calibration.unit());
        }

        // Titre de fenêtre.
        let name = self.project_path.as_ref().or(self.image_path.as_ref()).and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());
        let title = match name {
            Some(n) => format!("{n}{} — MeterOscope", if self.is_dirty() { " •" } else { "" }),
            None => "MeterOscope".to_owned(),
        };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
    }
}

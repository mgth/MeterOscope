//! Coquille de l'application : barre d'outils, panneaux, raccourcis, fichiers.

use meteroscope_i18n::tr;
use crate::camera_view::CameraView;
use crate::detect_view::DetectView;
use egui::{Align, Color32, ColorImage, Key, KeyboardShortcut, Layout, Modifiers, RichText, TextureHandle, TextureOptions, Ui};
use image::RgbaImage;
use meteroscope_canvas::Canvas;
use meteroscope_core::{Document, Orientation, Tool};
use meteroscope_io as io;
use meteroscope_panels::{CalibrationDialog, PerspectiveDialog};
use crate::clipboard::Pasted;
use crate::history::{ImageHistory, Workspace};
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

mod live;
mod export;
#[cfg(test)]
mod undo_tests;

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
    /// Incrémenté chaque fois que l'image de travail change (pixels de
    /// `image`, ou image corrigée de la perspective).
    image_gen: u64,
    /// Orientation (quarts de tour, miroirs) dans laquelle se trouvent les
    /// pixels de `image` ; celle du document fait foi après une annulation.
    applied: Orientation,
    /// Orientation de l'image telle qu'elle est dans le fichier `image_path`.
    file_orientation: Orientation,
    /// Image corrigée de la perspective (`Document::perspective`), sur
    /// laquelle on travaille alors ; reconstruite à partir de `image`.
    flat: Option<Flat>,
    /// `flat` vient de changer : texture à refaire, vue à réajuster.
    flat_changed: bool,
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
    /// Fichier image que cette session a écrit (image sans fichier, ou
    /// transformée) et projet auquel il appartient : le seul fichier qu'un
    /// enregistrement peut réécrire, et seulement pour ce projet.
    owned_image: Option<(PathBuf, PathBuf)>,
    project_path: Option<PathBuf>,
    /// Révision du document au dernier enregistrement.
    saved_revision: u64,
    brightness: f32,
    contrast: f32,
    canvas: Canvas,
    calibration: CalibrationDialog,
    perspective: PerspectiveDialog,
    /// Préréglages d'échelle et option « garder l'échelle pour l'image suivante ».
    scale_presets: meteroscope_core::ScalePresets,
    mode: Mode,
    camera: CameraView,
    detect: DetectView,
    /// Profils d'intensité : résultats calculés sur l'image, panneau de la courbe.
    profiles: crate::profile_view::ProfileView,
    toast: Option<(String, bool, Instant)>,
    /// Images remplacées, rétablies par Ctrl+Z.
    history: ImageHistory<Workspace, String>,
    /// Collage (Ctrl+V) en attente de confirmation.
    pending_paste: Option<Pasted>,
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

struct Flat {
    /// Correction et filtre pour lesquels `image` a été construite.
    key: (meteroscope_core::Perspective, io::RotationFilter),
    /// `None` : correction inapplicable (projet abîmé), on montre la photo.
    image: Option<Arc<RgbaImage>>,
}

const CTRL: Modifiers = Modifiers::COMMAND;
const CTRL_SHIFT: Modifiers = Modifiers { shift: true, ..Modifiers::COMMAND };

impl MeterOscopeApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        let mut app = Self::blank();
        meteroscope_core::measure::set_decimals(app.settings.decimals);
        if let Some(u) = &app.settings.calibration_unit {
            app.calibration.set_unit(u);
        }
        if let Some(p) = initial {
            app.open_path(&cc.egui_ctx, &p);
        }
        app
    }

    /// Application sans image, en mode Image.
    fn blank() -> Self {
        Self {
            doc: Document::new(),
            image: None,
            image_gen: 0,
            applied: Orientation::IDENTITY,
            file_orientation: Orientation::IDENTITY,
            flat: None,
            flat_changed: false,
            texture: None,
            filter: crate::prefs::load_filter(),
            texture_state: (0, NO_KEY),
            straight: None,
            job: None,
            failed: None,
            image_path: None,
            owned_image: None,
            project_path: None,
            saved_revision: 0,
            brightness: 0.0,
            contrast: 0.0,
            canvas: Canvas::default(),
            calibration: CalibrationDialog::default(),
            perspective: PerspectiveDialog::default(),
            scale_presets: crate::prefs::load_scale_presets(),
            mode: Mode::Image,
            camera: CameraView::default(),
            detect: DetectView::default(),
            profiles: Default::default(),
            toast: None,
            history: ImageHistory::default(),
            pending_paste: None,
            settings: crate::settings::Settings::load(),
        }
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

    /// Image de travail : la photo, ou sa version corrigée de la perspective.
    /// C'est elle que l'on affiche, mesure et exporte.
    fn work_image(&self) -> Option<&RgbaImage> {
        self.flat.as_ref().and_then(|f| f.image.as_deref()).or(self.image.as_deref())
    }

    /// Met l'image corrigée en accord avec `Document::perspective` (outil,
    /// annulation, projet ouvert, changement de filtre…).
    fn sync_flat(&mut self) {
        // Sans filtre choisi (rotation par la carte graphique) : Lanczos.
        let filter = self.filter.unwrap_or_default();
        let wanted = self.image.as_ref().and(self.doc.perspective.as_ref()).map(|p| (p, filter));
        if self.flat.as_ref().map(|f| (&f.key.0, f.key.1)) == wanted {
            return;
        }
        self.flat = match (wanted, &self.image) {
            (Some((p, filter)), Some(img)) => Some(Flat { key: (p.clone(), filter), image: io::warp_perspective(img, p, filter).map(Arc::new) }),
            _ => None,
        };
        self.flat_changed = true;
        // L'image de travail a changé : redressement, profils et détection sont à refaire.
        self.image_gen += 1;
        self.straight = None;
        self.profiles.invalidate();
        self.detect.invalidate();
    }

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
        // L'image corrigée de la perspective en dérive : à reconstruire.
        self.flat = None;
        // Les profils d'intensité se lisent dans ces pixels.
        self.profiles.invalidate();
        // Une détection en cours appartient aux anciens pixels.
        self.detect.invalidate();
    }

    /// Remet les pixels dans l'orientation du document : un quart de tour ou
    /// un miroir annulé (ou rétabli) revient avec les mesures et l'angle.
    fn sync_orientation(&mut self) {
        // En mode caméra, `self.doc` est le document du flux, pas celui de l'image.
        if self.mode == Mode::Camera {
            return;
        }
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
        // On redresse l'image de travail : la photo, ou sa version corrigée de la perspective.
        let work = self.flat.as_ref().and_then(|f| f.image.as_ref()).or(self.image.as_ref());
        let (Some(img), (deg, Some(filter))) = (work, key) else { return };
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
        let Some(img) = self.flat.as_ref().and_then(|f| f.image.as_deref()).or(self.image.as_deref()) else {
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

    /// Installe une nouvelle image avec un document vierge (qui garde
    /// l'échelle en cours si l'option est cochée). L'image précédente reste
    /// récupérable par Ctrl+Z.
    fn set_image(&mut self, ctx: &egui::Context, img: RgbaImage, path: Option<PathBuf>) {
        // D'abord rendre à l'image son document, pour ne pas écraser celui du
        // flux ; l'échelle à garder se lit avant que ce document parte dans
        // l'historique des images.
        self.mode_image();
        let calibration = self.scale_presets.next_calibration(&self.doc.calibration);
        if let Some(old) = self.take_workspace() {
            self.history.replaced(old);
        }
        self.image = Some(Arc::new(img));
        self.pixels_changed();
        self.image_path = path;
        self.owned_image = None;
        self.project_path = None;
        self.doc = Document::new();
        self.applied = Orientation::IDENTITY;
        self.file_orientation = Orientation::IDENTITY;
        self.doc.calibration = calibration;
        self.saved_revision = 0;
        self.brightness = 0.0;
        self.contrast = 0.0;
        self.canvas.set_tool(Tool::Select);
        self.canvas.fit();
        // La texture se construit à la prochaine frame de la vue image.
        ctx.request_repaint();
    }

    /// Retire l'espace de travail courant (l'image reste vide jusqu'à `put_workspace`).
    fn take_workspace(&mut self) -> Option<Workspace> {
        Some(Workspace {
            image: self.image.take()?,
            image_path: self.image_path.take(),
            owned_image: self.owned_image.take(),
            applied: self.applied,
            file_orientation: self.file_orientation,
            project_path: self.project_path.take(),
            doc: std::mem::take(&mut self.doc),
            saved_revision: self.saved_revision,
            brightness: self.brightness,
            contrast: self.contrast,
        })
    }

    fn put_workspace(&mut self, ctx: &egui::Context, w: Workspace) {
        self.image = Some(w.image);
        self.pixels_changed();
        self.image_path = w.image_path;
        self.owned_image = w.owned_image;
        self.applied = w.applied;
        self.file_orientation = w.file_orientation;
        self.project_path = w.project_path;
        self.doc = w.doc;
        self.saved_revision = w.saved_revision;
        self.brightness = w.brightness;
        self.contrast = w.contrast;
        self.mode_image();
        // L'image corrigée de la perspective suit l'image rétablie.
        self.sync_flat();
        self.flat_changed = false;
        self.canvas.set_tool(Tool::Select);
        self.canvas.fit();
        self.refresh_texture(ctx);
    }

    /// L'historique des images ne sert qu'en mode image : en mode caméra,
    /// `doc` est celui du flux.
    fn images_undoable(&self) -> bool {
        self.image.is_some() && self.mode == Mode::Image
    }

    fn can_undo(&self) -> bool {
        self.doc.can_undo() || (self.images_undoable() && self.history.can_undo())
    }
    fn can_redo(&self) -> bool {
        self.doc.can_redo()
            || (self.images_undoable() && self.history.can_redo_with(self.doc.revision(), || crate::history::doc_key(&self.doc)))
    }

    /// Annule dans le document, puis, son historique épuisé, le remplacement d'image.
    fn undo(&mut self, ctx: &egui::Context) {
        if self.doc.undo() || !self.images_undoable() || !self.history.can_undo() {
            return;
        }
        let Some(current) = self.take_workspace() else { return };
        match self.history.undo(current, |w| crate::history::doc_key(&w.doc)) {
            Ok(previous) => {
                self.put_workspace(ctx, previous);
                self.info(tr!("msg.image_restored"));
            }
            Err(current) => self.put_workspace(ctx, current),
        }
    }

    fn redo(&mut self, ctx: &egui::Context) {
        if self.doc.redo() || !self.images_undoable() {
            return;
        }
        // Empreinte recalculée : un glisser modifie le document sans changer sa révision.
        let key = crate::history::doc_key(&self.doc);
        if !self.history.can_redo(&key) {
            return;
        }
        let Some(current) = self.take_workspace() else { return };
        match self.history.redo(current, &key) {
            Ok(next) | Err(next) => self.put_workspace(ctx, next),
        }
    }

    fn mode_image(&mut self) {
        if self.mode == Mode::Camera {
            self.leave_camera();
        }
    }

    /// L'image fixe a des modifications non enregistrées (en mode caméra, son
    /// document est mis de côté : les mesures du flux ne comptent pas).
    fn is_dirty(&self) -> bool {
        let doc = if self.mode == Mode::Camera { &self.camera.other_doc } else { &self.doc };
        doc.revision() != self.saved_revision
    }

    // ---------- fichiers ----------

    fn open_path(&mut self, ctx: &egui::Context, path: &Path) {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if ext == io::PROJECT_EXTENSION {
            let Some(project) = self.report(io::load_project(path), "") else { return };
            self.mode_image();
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
            self.detect.invalidate();
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
        if self.image.is_none() || self.mode == Mode::Camera {
            return;
        }
        let path = match (&self.project_path, save_as) {
            (Some(p), false) => p.clone(),
            _ => match self.save_dialog(tr!("dialog.save_project"), "", io::PROJECT_EXTENSION) {
                Some(p) => p,
                None => return,
            },
        };
        if self.write_project(&path) {
            self.settings.add_recent(&path);
        }
    }

    /// Fichier où écrire l'image mesurée quand elle n'a pas de fichier
    /// identique (capture, image collée, pivotée ou retournée). Jamais un
    /// fichier existant, la photo d'origine en premier lieu : on reprend le
    /// fichier que cette session a déjà écrit pour ce même projet, sinon le
    /// premier nom libre. « Enregistrer sous » un autre projet lui réserve
    /// donc son propre fichier, sans toucher à celui du premier.
    fn image_file_for(&self, project: &Path) -> PathBuf {
        if let Some((image, _)) = self.owned_image.as_ref().filter(|(_, owner)| owner == project) {
            return image.clone();
        }
        let stem = project.file_stem().map_or_else(|| self.default_stem(), |s| s.to_string_lossy().into_owned());
        let named = |suffix: &str| project.with_file_name(format!("{stem}{suffix}.png"));
        std::iter::once(String::new())
            .chain(std::iter::once("-image".to_owned()))
            .chain((2..).map(|n| format!("-image-{n}")))
            .map(|suffix| named(&suffix))
            .find(|p| !p.exists())
            .expect("suite de noms sans fin")
    }

    /// Écrit le projet dans `path`, et d'abord l'image si aucun fichier ne
    /// lui est identique. Renvoie `false` en cas d'échec (déjà signalé).
    fn write_project(&mut self, path: &Path) -> bool {
        self.sync_orientation();
        // Capture caméra ou image pivotée/retournée : le projet doit pointer
        // vers un fichier identique à l'image mesurée, sans écraser l'original.
        if self.image_path.is_none() || self.image_modified() {
            let png = self.image_file_for(path);
            let r = io::save_image(self.image.as_ref().unwrap(), &png);
            if self.report(r, "").is_none() {
                return false;
            }
            self.owned_image = Some((png.clone(), path.to_owned()));
            self.image_path = Some(png);
            self.file_orientation = self.applied;
        } else if self.owned_image.as_ref().is_some_and(|(image, owner)| owner != path && Some(image) == self.image_path.as_ref()) {
            // Un autre projet va référencer ce fichier tel quel : désormais
            // partagé, il ne sera plus réécrit pour aucun des deux.
            self.owned_image = None;
        }
        let project = io::Project { image_path: self.image_path.clone(), document: self.doc.clone() };
        if self.report(io::save_project(&project, path), tr!("msg.project_saved")).is_none() {
            return false;
        }
        self.saved_revision = self.doc.revision();
        self.project_path = Some(path.to_owned());
        true
    }

    fn export_csv(&mut self) {
        if let Some(p) = self.save_dialog(tr!("dialog.export_csv"), tr!("file.measurements_suffix"), "csv") {
            let r = io::export_csv(&self.doc, &p);
            self.report(r, tr!("msg.csv_exported"));
        }
    }

    /// Exporte la courbe du profil d'intensité sélectionné.
    fn export_profile(&mut self) {
        let Some((id, _)) = self.profiles.selected(&self.doc) else { return };
        let name = self.doc.get(id).map(|m| m.name.clone()).unwrap_or_default();
        // Le dialogue retient le dossier (emprunt exclusif) : l'analyse est reprise après.
        if let Some(p) = self.save_dialog(tr!("dialog.export_profile"), &format!("-{name}"), "csv")
            && let Some((_, analysis)) = self.profiles.selected(&self.doc)
        {
            let r = io::export_profile_csv(&name, analysis, &self.doc, &p);
            self.report(r, tr!("msg.profile_exported"));
        }
    }

    /// Exporte l'image annotée telle que la vue la montre, redressement compris
    /// (avec le filtre choisi, ou celui par défaut si l'affichage pivote par
    /// la carte graphique).
    fn export_image(&mut self) {
        self.sync_orientation();
        self.sync_flat();
        if self.work_image().is_none() {
            return;
        }
        if let Some(p) = self.save_dialog(tr!("dialog.export_image"), tr!("file.measurements_suffix"), "png") {
            let Some(img) = self.work_image() else { return };
            let base = io::adjust(img, self.brightness, self.contrast);
            let r = io::save_image(&io::render_view(&base, &self.doc, self.filter.unwrap_or_default()), &p);
            self.report(r, tr!("msg.image_exported"));
        }
    }

    /// Colle l'image du presse-papiers, traitée comme une capture caméra
    /// (sans fichier d'origine), ou ouvre le fichier image copié. Avec
    /// `confirm` (Ctrl+V), des mesures non enregistrées font d'abord demander.
    fn paste(&mut self, ctx: &egui::Context, fallback: crate::clipboard::Fallback, confirm: bool) {
        let pasted = crate::clipboard::read(fallback);
        if confirm && !matches!(pasted, Pasted::Nothing) && self.image.is_some() && self.is_dirty() {
            self.pending_paste = Some(pasted);
        } else {
            self.apply_paste(ctx, pasted);
        }
    }

    fn apply_paste(&mut self, ctx: &egui::Context, pasted: Pasted) {
        match pasted {
            Pasted::Image(img) => {
                self.set_image(ctx, img, None);
                self.info(tr!("msg.image_pasted"));
            }
            Pasted::File(p) => self.open_path(ctx, &p),
            Pasted::Nothing => self.info(tr!("msg.paste_nothing")),
        }
    }

    /// Copie l'image annotée telle que l'export l'écrit : image de travail
    /// (perspective corrigée comprise), redressement compris.
    fn copy_image(&mut self, ctx: &egui::Context) {
        self.sync_orientation();
        self.sync_flat();
        let Some(img) = self.work_image() else { return };
        let out = io::render_view(&io::adjust(img, self.brightness, self.contrast), &self.doc, self.filter.unwrap_or_default());
        ctx.copy_image(ColorImage::from_rgba_unmultiplied([out.width() as usize, out.height() as usize], out.as_raw()));
        self.info(tr!("msg.image_copied"));
    }

    /// Quart de tour ou miroir : pixels, mesures et redressement changent
    /// ensemble, en une seule étape d'historique.
    fn transform(&mut self, t: io::Transform) {
        self.sync_orientation();
        if let Some(img) = &self.image {
            self.image = Some(Arc::new(io::apply_transform(img, &mut self.doc, t)));
            self.applied = self.doc.orientation;
            self.pixels_changed();
            // L'image corrigée suit la photo.
            self.sync_flat();
            self.flat_changed = false;
            self.canvas.fit();
        }
    }

    fn toggle_camera(&mut self, ctx: &egui::Context) {
        if self.mode == Mode::Camera {
            self.mode_image();
        } else {
            self.enter_camera(ctx);
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
            self.redo(ctx);
        } else if hit(CTRL, Key::Z) {
            self.undo(ctx);
        }
        if hit(CTRL, Key::Num0) {
            self.canvas.fit();
        }
        if hit(CTRL, Key::Num1) {
            self.canvas.zoom_actual();
        }

        // Touches simples : seulement si aucun champ de texte n'a le focus.
        if ctx.egui_wants_keyboard_input() || self.calibration.is_open() || self.perspective.is_open() {
            return;
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            if self.canvas.is_drawing() {
                self.canvas.cancel();
            } else if self.canvas.tool() != Tool::Select {
                self.canvas.set_tool(Tool::Select);
            } else if self.doc.selected.is_some() || self.mode == Mode::Image {
                self.doc.selected = None;
            } else {
                self.mode_image();
            }
        }
        if ctx.input(|i| i.key_pressed(Key::Delete) || i.key_pressed(Key::Backspace)) && !self.canvas.is_drawing()
            && self.doc.selected.is_some() {
                // Toute la sélection multiple de la liste, en un seul pas d'annulation.
                self.doc.remove_many(&meteroscope_panels::selection(ctx, &self.doc));
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
            // Bouton partagé : « Ouvrir » ouvre un fichier, la flèche propose les autres sources.
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.x = 1.0;
                if ui.button(format!("📂  {}", tr!("toolbar.open"))).on_hover_text(tr!("toolbar.open_tip")).clicked() {
                    self.open_dialog(&ctx);
                }
                ui.menu_button("⏷", |ui| {
                    if ui.button(format!("📂  {}", tr!("open.file"))).clicked() {
                        self.open_dialog(&ctx);
                    }
                    if ui.button(format!("📋  {}", tr!("open.paste"))).clicked() {
                        self.paste(&ctx, crate::clipboard::Fallback::default(), false);
                    }
                    if ui.button(format!("📷  {}", tr!("toolbar.camera"))).clicked() {
                        if self.mode != Mode::Camera {
                            self.toggle_camera(&ctx);
                        }
                        ui.close();
                    }
                    // Fichiers récents.
                    ui.separator();
                    if let Some(p) = crate::settings::recent_items(ui, &mut self.settings) {
                        self.open_path(&ctx, &p);
                    }
                })
                .response
                .on_hover_text(tr!("toolbar.open_more_tip"));
            });
            if ui.selectable_label(self.mode == Mode::Camera, format!("📷  {}", tr!("toolbar.camera"))).on_hover_text(tr!("toolbar.camera_tip")).clicked() {
                self.toggle_camera(&ctx);
            }
            ui.separator();
            // En mode caméra : annuler et zoom servent sur le flux ; enregistrer,
            // exporter et le menu Image ne concernent que l'image fixe.
            ui.add_enabled_ui(has_image || self.mode == Mode::Camera, |ui| {
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
                        self.export_menu_items(ui);
                        if ui.button(tr!("toolbar.copy_image")).clicked() {
                            self.copy_image(&ctx);
                        }
                        if ui.button(tr!("toolbar.copy_table")).clicked() {
                            ui.ctx().copy_text(io::clipboard_table(&self.doc));
                            self.info(tr!("msg.table_copied"));
                        }
                    });
                });
                ui.separator();
                if ui.add_enabled(self.can_undo(), egui::Button::new("↩")).on_hover_text(tr!("toolbar.undo_tip")).clicked() {
                    self.undo(&ctx);
                }
                if ui.add_enabled(self.can_redo(), egui::Button::new("↪")).on_hover_text(tr!("toolbar.redo_tip")).clicked() {
                    self.redo(&ctx);
                }
                ui.separator();
                ui.add_enabled_ui(has_image, |ui| {
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
                        if ui.button(format!("▱  {}", tr!("image.perspective"))).on_hover_text(tr!("tool_help.perspective")).clicked() {
                            self.canvas.set_tool(Tool::Perspective);
                            ui.close();
                        }
                        let reset = ui.add_enabled(self.doc.perspective.is_some(), egui::Button::new(tr!("image.perspective_reset")));
                        if reset.clicked() {
                            self.doc.set_perspective(None, None);
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
                        ui.separator();
                        meteroscope_panels::scale_bar_menu(ui, &mut self.doc);
                    });
                });
                let detect = ui.selectable_label(self.detect.is_open(), format!("🔍  {}", tr!("toolbar.detect")));
                if detect.on_hover_text(tr!("toolbar.detect_tip")).clicked() {
                    self.detect.toggle();
                }
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
                if let Some(img) = self.work_image().filter(|_| self.mode == Mode::Image) {
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
                if self.image.is_some() && self.doc.perspective.is_some() {
                    let resp = ui.add(egui::Label::new(tr!("status.perspective")).sense(egui::Sense::click())).on_hover_text(tr!("status.perspective_tip"));
                    if resp.double_clicked() {
                        self.doc.set_perspective(None, None);
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
            ui.add_space(8.0);
            if ui.add(big(&format!("📋  {}", tr!("welcome.paste")))).on_hover_text(tr!("welcome.paste_tip")).clicked() {
                self.paste(&ctx, crate::clipboard::Fallback::default(), false);
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
        let free = !self.calibration.is_open() && self.pending_paste.is_none();
        if let Some(fallback) = crate::clipboard::paste_requested(&ctx, free) {
            self.paste(&ctx, fallback, true);
        }

        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.add_space(4.0);
            self.toolbar(ui);
            ui.add_space(4.0);
        });

        let show_tools = self.image.is_some() || self.mode == Mode::Camera;
        let mut cursor = None;
        // La barre d'état est déclarée avant le centre mais affiche le curseur de la frame précédente.
        let cursor_id = egui::Id::unique("meteroscope_cursor");
        let last_cursor: Option<meteroscope_core::Pt> = ctx.data(|d| d.get_temp(cursor_id)).flatten();
        egui::Panel::bottom("status").show(ui, |ui| self.status_bar(ui, last_cursor));

        if show_tools {
            // Profils d'intensité : résultats recalculés sur l'image d'origine,
            // remise d'abord dans l'orientation du document (annulation d'un miroir),
            // ou sur l'image corrigée en perspective, repère des mesures
            // (pas en mode caméra : `self.doc` est alors celui du flux).
            self.sync_orientation();
            if self.mode == Mode::Image {
                self.sync_flat();
            }
            let work = self.flat.as_ref().and_then(|f| f.image.as_deref()).or(self.image.as_deref());
            if let Some(img) = work.filter(|_| self.mode == Mode::Image) {
                self.profiles.sync(&mut self.doc, img);
            }
            egui::Panel::left("tools").resizable(false).show(ui, |ui| {
                let mut tool = self.canvas.tool();
                meteroscope_panels::tool_palette(ui, &mut tool);
                if tool != self.canvas.tool() {
                    self.canvas.set_tool(tool);
                }
            });
            egui::Panel::right("measurements").default_size(300.0).show(ui, |ui| {
                let out = meteroscope_panels::measurements_panel_with_presets(ui, &mut self.doc, &mut self.scale_presets);
                if out.presets_changed {
                    crate::prefs::save_scale_presets(&self.scale_presets);
                }
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
            // Courbe du profil sélectionné, sous l'image.
            if self.profiles.selected(&self.doc).is_some() {
                let export = egui::Panel::bottom("profile").resizable(true).default_size(210.0).show(ui, |ui| self.profiles.panel(ui, &mut self.doc));
                if export.inner {
                    self.export_profile();
                }
            }
        }

        // La détection travaille sur l'image de travail (corrigée de la
        // perspective s'il y a lieu) : à refaire quand celle-ci change.
        // `sync_flat` et la remise en orientation l'invalident alors.
        if self.mode == Mode::Image {
            self.sync_orientation();
            self.sync_flat();
        }
        let work = self.flat.as_ref().and_then(|f| f.image.as_deref()).or(self.image.as_deref());
        let image = work.filter(|_| self.mode == Mode::Image);
        if let Some((msg, err)) = self.detect.frame(&ctx, image, &mut self.doc, &mut self.canvas) {
            if err { self.error(msg) } else { self.info(msg) }
        }

        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| match self.mode {
            Mode::Camera => cursor = self.live_view(ui),
            Mode::Image if self.image.is_none() => self.welcome(ui),
            Mode::Image => {
                // Quart de tour ou miroir annulé, perspective, redressement ou
                // filtre changés (outil, annulation, projet ouvert…).
                self.sync_orientation();
                self.sync_flat();
                if std::mem::take(&mut self.flat_changed) {
                    self.canvas.fit();
                }
                self.update_texture(&ctx);
                let size = self.work_image().map(|i| [i.width() as usize, i.height() as usize]).unwrap();
                let out = self.canvas.show(ui, &mut self.doc, self.texture.as_ref().map(|t| (t, size)));
                cursor = out.cursor;
                if self.texture_pending() {
                    ctx.request_repaint();
                }
                if let Some(id) = out.created {
                    self.profiles.created(&mut self.doc, id);
                }
                if let Some((a, b)) = out.calibration_segment {
                    self.calibration.open(a.dist(b));
                }
                if let (Some(quad), Some(img)) = (out.perspective_quad, &self.image) {
                    if meteroscope_core::perspective::order_quad(quad, self.doc.rotation).is_some() {
                        self.perspective.open(quad, img.dimensions());
                    } else {
                        self.canvas.clear_perspective_quad();
                        self.error(tr!("msg.perspective_invalid"));
                    }
                }
            }
        });
        ctx.data_mut(|d| d.insert_temp(cursor_id, cursor));

        if self.pending_paste.is_some()
            && let Some(ok) = crate::clipboard::confirm_replace(&ctx)
            && let Some(pasted) = self.pending_paste.take()
            && ok
        {
            self.apply_paste(&ctx, pasted);
        }

        if self.calibration.show(&ctx, &mut self.doc) {
            self.canvas.set_tool(Tool::Select);
            self.settings.set_calibration_unit(self.calibration.unit());
        }
        let corrected = self.doc.perspective.clone();
        if self.perspective.show(&ctx, &mut self.doc) {
            self.canvas.set_tool(Tool::Select);
            if self.doc.perspective != corrected && !self.doc.calibration.is_calibrated() {
                self.info(tr!("msg.perspective_no_scale"));
            }
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

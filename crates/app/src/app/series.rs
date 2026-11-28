//! Série d'images dans la coquille : passage d'une image à l'autre, ajout,
//! retrait, bande de vignettes, enregistrement et sorties de toute la série.
//!
//! Le reste de l'application continue de travailler sur « l'image
//! courante » : `app.image`, `app.image_path`, `app.image_modified` et
//! `app.doc` sont ceux de l'image affichée. Ce module les range et les
//! ressort quand on change d'image ([`MeterOscopeApp::series_show`]).
//! Seule l'image affichée est décodée en mémoire ; les autres sont relues
//! de leur fichier à l'affichage (les captures sans fichier restent en
//! mémoire jusqu'à l'enregistrement du projet).

use super::{MeterOscopeApp, Mode};
use crate::thumbs::{Thumbs, load_transformed};
use egui::{Align, Align2, Color32, CornerRadius, FontId, Key, KeyboardShortcut, Modifiers, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui};
use image::RgbaImage;
use meteroscope_core::series::Entry;
use meteroscope_core::{Document, Series, Tool, Undone, series_summary};
use meteroscope_i18n::tr;
use meteroscope_io as io;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

/// Hauteur des vignettes de la bande.
const THUMB_H: f32 = 64.0;
/// Taille de l'image de remplacement quand celle d'origine est inconnue.
const PLACEHOLDER: (u32, u32) = (1600, 1200);

/// Ce que l'application sait d'une image en plus du modèle (par identifiant d'entrée).
#[derive(Default)]
struct Extra {
    /// Pixels gardés en mémoire quand aucun fichier ne les contient
    /// (capture, collage) et que l'image n'est pas affichée.
    pixels: Option<RgbaImage>,
    /// Rotations et miroirs appliqués depuis la lecture du fichier : rejoués
    /// à la relecture, ce qui évite de garder l'image modifiée en mémoire.
    transforms: Vec<io::Transform>,
    /// L'image diffère de son fichier (`image_modified` quand elle est affichée).
    modified: bool,
    /// Fichier introuvable ou illisible : une image grise le remplace.
    missing: bool,
}

/// Export des images annotées en cours (dans un fil à part).
struct ExportJob {
    rx: Receiver<(usize, usize, Option<String>)>,
    dir: String,
    failed: usize,
}

pub(super) struct SeriesState {
    model: Series,
    extras: HashMap<u64, Extra>,
    thumbs: Thumbs,
    /// Entrée dont l'image est actuellement dans `app.image`.
    shown: Option<u64>,
    /// Révision de la série au dernier enregistrement.
    saved: u64,
    strip_open: bool,
    /// Entrée vers laquelle la bande a déjà défilé.
    scrolled: Option<u64>,
    export: Option<ExportJob>,
}

impl Default for SeriesState {
    fn default() -> Self {
        Self {
            model: Series::new(),
            extras: HashMap::new(),
            thumbs: Thumbs::default(),
            shown: None,
            saved: 0,
            strip_open: true,
            scrolled: None,
            export: None,
        }
    }
}

/// Action demandée depuis la bande ou le menu, exécutée après le dessin.
enum Act {
    Go(usize),
    Remove(usize),
    Move(usize, usize),
    Add,
    Shared(bool),
    Linked(usize, bool),
}

const CTRL_SHIFT: Modifiers = Modifiers { shift: true, ..Modifiers::COMMAND };

fn is_project(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case(io::PROJECT_EXTENSION))
}

/// Premier nom libre parmi `base` + `tail`, puis `base-2` + `tail`,
/// `base-3` + `tail`… `free` dit si un nom est disponible.
fn free_name(base: &str, tail: &str, free: impl Fn(&str) -> bool) -> String {
    let first = format!("{base}{tail}");
    if free(&first) {
        return first;
    }
    (2u32..).map(|k| format!("{base}-{k}{tail}")).find(|name| free(name)).unwrap_or(first)
}

fn placeholder(size: Option<(u32, u32)>) -> RgbaImage {
    let (w, h) = size.unwrap_or(PLACEHOLDER);
    RgbaImage::from_pixel(w.max(1), h.max(1), image::Rgba([72, 72, 78, 255]))
}

impl MeterOscopeApp {
    // ---------- état ----------

    /// Nouvelle série d'une seule image : appelé par `set_image`.
    pub(super) fn series_restart(&mut self, ctx: &egui::Context) {
        let size = self.image.as_ref().map(|i| i.dimensions());
        let s = &mut self.series;
        s.model = Series::new();
        s.extras.clear();
        s.thumbs.clear();
        s.shown = s.model.add(vec![(self.image_path.clone(), size)], &mut self.doc).first().copied();
        if let (Some(id), Some(img)) = (s.shown, &self.image) {
            s.thumbs.set(ctx, id, img);
        }
        s.saved = s.model.revision(&self.doc);
    }

    /// Modifications non enregistrées, sur n'importe quelle image de la série.
    pub(super) fn series_dirty(&self) -> bool {
        self.series.model.revision(&self.doc) != self.series.saved
    }

    /// Reporte dans la série l'état de l'image affichée (chemin, dimensions)
    /// et lui fait suivre le document actif (historique, échelle commune).
    fn series_sync(&mut self) {
        let s = &mut self.series;
        if let Some(id) = s.shown
            && let Some(e) = s.model.active_entry_mut().filter(|e| e.id() == id)
        {
            if e.path != self.image_path {
                e.path = self.image_path.clone();
            }
            if let Some(img) = &self.image
                && !s.extras.get(&id).is_some_and(|x| x.missing)
            {
                e.size = Some(img.dimensions());
            }
        }
        s.model.observe(&mut self.doc);
    }

    /// À chaque image d'interface : suivi du document, vignettes, export.
    pub(super) fn series_frame(&mut self, ctx: &egui::Context) {
        self.series_sync();
        self.series.thumbs.poll(ctx);
        self.series_export_poll();
    }

    /// Met dans `app.image` l'image de l'entrée active, après avoir rangé
    /// celle qui s'y trouvait. Sans effet si c'est déjà la bonne.
    fn series_show(&mut self, ctx: &egui::Context) {
        let target = self.series.model.active_entry().map(|e| e.id());
        if target == self.series.shown {
            return;
        }
        let old_size = self.image.as_ref().map(|i| i.dimensions());
        if let Some(old) = self.series.shown.take() {
            let x = self.series.extras.entry(old).or_default();
            x.modified = self.image_modified;
            // Sans fichier fidèle, les pixels sont le seul exemplaire.
            let keep = !x.missing && (self.image_path.is_none() || (self.image_modified && x.transforms.is_empty()));
            x.pixels = if keep { self.image.take() } else { None };
        }
        self.image = None;
        self.straight = None;
        self.canvas.cancel();
        let Some(entry) = self.series.model.active_entry() else {
            self.image_path = None;
            self.image_modified = false;
            self.refresh_texture(ctx);
            return;
        };
        let (id, path, size) = (entry.id(), entry.path.clone(), entry.size);
        let x = self.series.extras.entry(id).or_default();
        let mut failure = None;
        let img = match (x.pixels.take(), &path) {
            (Some(px), _) => Some(px),
            (None, Some(p)) => load_transformed(p, &x.transforms).map_err(|e| failure = Some(format!("{e:#}"))).ok(),
            (None, None) => None,
        };
        x.missing = img.is_none();
        self.image_modified = x.modified;
        self.image_path = path;
        let img = match img {
            Some(img) => {
                if !self.series.thumbs.has(id) {
                    self.series.thumbs.set(ctx, id, &img);
                }
                img
            }
            // Image introuvable : les mesures restent visibles sur un fond gris.
            None => placeholder(size),
        };
        if old_size != Some(img.dimensions()) {
            self.canvas.fit();
        }
        self.image = Some(img);
        self.series.shown = Some(id);
        self.mode_image();
        self.refresh_texture(ctx);
        if let Some(f) = failure {
            self.error(tr!("msg.image_missing", detail = f));
        }
    }

    /// Demande les vignettes qui manquent.
    fn series_request_thumbs(&mut self, ctx: &egui::Context) {
        let s = &mut self.series;
        for e in s.model.entries() {
            let x = s.extras.get(&e.id());
            if x.is_some_and(|x| x.missing)
                || s.thumbs.has(e.id())
            {
                continue;
            }
            if let Some(px) = x.and_then(|x| x.pixels.as_ref()) {
                s.thumbs.set(ctx, e.id(), px);
            } else if let Some(p) = &e.path {
                s.thumbs.request(ctx, e.id(), p.clone(), x.map(|x| x.transforms.clone()).unwrap_or_default());
            }
        }
    }

    /// Après une opération qui a pu changer l'image affichée ou la liste.
    fn series_after_op(&mut self, ctx: &egui::Context) {
        self.series_show(ctx);
        self.series_request_thumbs(ctx);
    }

    /// Installe une série toute faite (projet relu, plusieurs images ouvertes).
    fn series_install(&mut self, ctx: &egui::Context, entries: Vec<Entry>, missing: Vec<bool>, active: usize, shared_scale: bool, project: Option<PathBuf>) {
        self.mode_image();
        self.image = None;
        let s = &mut self.series;
        s.extras.clear();
        s.thumbs.clear();
        s.shown = None;
        s.model = Series::from_entries(entries, active, shared_scale, &mut self.doc);
        for (e, m) in s.model.entries().iter().zip(missing) {
            s.extras.entry(e.id()).or_default().missing = m;
        }
        s.saved = s.model.revision(&self.doc);
        self.project_path = project;
        self.brightness = 0.0;
        self.contrast = 0.0;
        self.canvas.set_tool(Tool::Select);
        self.series_after_op(ctx);
        self.canvas.fit();
    }

    // ---------- ouverture, ajout ----------

    /// Ouvre un projet (version 1 ou 2). Les images introuvables sont
    /// signalées sans bloquer : leurs mesures sont conservées.
    pub(super) fn series_open_project(&mut self, ctx: &egui::Context, path: &Path) {
        let Some(p) = self.report(io::load_series(path), "") else { return };
        let missing: Vec<bool> = p.images.iter().map(|i| i.missing).collect();
        let n = missing.iter().filter(|m| **m).count();
        let several = missing.len() > 1;
        let entries = p.images.into_iter().map(|i| Entry::new(i.image_path, i.size, !i.own_scale, i.document)).collect();
        self.series_install(ctx, entries, missing, p.active, p.shared_scale, Some(path.to_owned()));
        if n > 0 && several {
            self.error(tr!("msg.images_missing", n = n));
        }
    }

    /// Ouvre ce qui a été choisi : un projet, une image, ou plusieurs images
    /// qui forment une nouvelle série.
    pub(super) fn series_open_paths(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        if let Some(p) = paths.iter().find(|p| is_project(p)) {
            self.open_path(ctx, p);
            return;
        }
        if let [only] = paths.as_slice() {
            self.open_path(ctx, only);
            return;
        }
        let total = paths.len();
        let found: Vec<(PathBuf, (u32, u32))> = paths.into_iter().filter_map(|p| io::image_dimensions(&p).map(|d| (p, d))).collect();
        let bad = total - found.len();
        match found.as_slice() {
            [] => {}
            [(only, _)] => self.open_path(ctx, only),
            [(_, first), ..] => {
                let first = *first;
                let entries = found.into_iter().map(|(p, d)| Entry::new(Some(p), Some(d), d == first, Document::new())).collect();
                self.series_install(ctx, entries, vec![], 0, true, None);
            }
        }
        if bad > 0 {
            self.error(tr!("msg.images_unreadable", n = bad));
        }
    }

    /// Ajoute des images à la série en cours (ou ouvre, s'il n'y a rien).
    fn series_add_paths(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        if self.image.is_none() || paths.iter().any(|p| is_project(p)) {
            self.series_open_paths(ctx, paths);
            return;
        }
        let total = paths.len();
        let list: Vec<_> = paths.into_iter().filter_map(|p| io::image_dimensions(&p).map(|d| (Some(p), Some(d)))).collect();
        let (n, bad) = (list.len(), total - list.len());
        if n > 0 {
            self.series_sync();
            self.series.model.add(list, &mut self.doc);
            self.series_after_op(ctx);
            self.info(tr!("msg.images_added", n = n));
        }
        if bad > 0 {
            self.error(tr!("msg.images_unreadable", n = bad));
        }
    }

    fn series_add_dialog(&mut self, ctx: &egui::Context) {
        let mut d = rfd::FileDialog::new().set_title(tr!("dialog.add_images")).add_filter(tr!("dialog.filter_only_images"), io::IMAGE_EXTENSIONS);
        if let Some(dir) = self.image_path.as_ref().and_then(|p| p.parent()) {
            d = d.set_directory(dir);
        }
        if let Some(paths) = d.pick_files() {
            self.series_add_paths(ctx, paths);
        }
    }

    /// Fichiers glissés-déposés : un projet s'ouvre, des images s'ajoutent à
    /// la série en cours (ou en commencent une).
    pub(super) fn series_dropped(&mut self, ctx: &egui::Context) {
        let paths: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_owned()).collect());
        if !paths.is_empty() {
            self.series_add_paths(ctx, paths);
        }
    }

    /// Capture caméra (ou toute image sans fichier) : nouvelle image de la
    /// série en cours, ou première image s'il n'y a rien d'ouvert.
    pub(super) fn series_capture(&mut self, ctx: &egui::Context, img: RgbaImage) {
        if self.image.is_none() {
            self.set_image(ctx, img, None);
            return;
        }
        self.series_sync();
        let ids = self.series.model.add(vec![(None, Some(img.dimensions()))], &mut self.doc);
        if let Some(id) = ids.first() {
            self.series.thumbs.set(ctx, *id, &img);
            self.series.extras.entry(*id).or_default().pixels = Some(img);
        }
        self.series_after_op(ctx);
    }

    /// Rotation ou miroir appliqué à l'image affichée : appelé par `transform`.
    pub(super) fn series_transformed(&mut self, ctx: &egui::Context, t: io::Transform) {
        if let (Some(id), Some(img)) = (self.series.shown, &self.image) {
            self.series.extras.entry(id).or_default().transforms.push(t);
            self.series.thumbs.set(ctx, id, img);
        }
    }

    // ---------- navigation et opérations ----------

    fn series_do(&mut self, ctx: &egui::Context, act: Act) {
        if let Act::Add = act {
            self.series_add_dialog(ctx);
            return;
        }
        self.series_sync();
        let (m, doc) = (&mut self.series.model, &mut self.doc);
        match act {
            Act::Go(i) => m.activate(i, doc),
            Act::Remove(i) => m.remove(i, doc),
            Act::Move(from, to) => m.move_entry(from, to, doc),
            Act::Shared(on) => m.set_shared_scale(on, doc),
            Act::Linked(i, on) => m.set_linked(i, on, doc),
            Act::Add => false,
        };
        self.series_after_op(ctx);
    }

    /// Image précédente (−1) ou suivante (+1).
    fn series_step(&mut self, ctx: &egui::Context, delta: isize) {
        let m = &self.series.model;
        if let Some(i) = m.active().checked_add_signed(delta).filter(|i| *i < m.len()) {
            self.series_do(ctx, Act::Go(i));
        }
    }

    pub(super) fn can_undo(&self) -> bool {
        self.series.model.can_undo(&self.doc)
    }
    pub(super) fn can_redo(&self) -> bool {
        self.series.model.can_redo(&self.doc)
    }

    /// Ctrl+Z : la plus récente entre la dernière modification de l'image
    /// affichée et la dernière opération de série.
    pub(super) fn undo(&mut self, ctx: &egui::Context) {
        self.series_sync();
        if self.series.model.undo(&mut self.doc) == Undone::Series {
            self.series_after_op(ctx);
        }
    }

    pub(super) fn redo(&mut self, ctx: &egui::Context) {
        self.series_sync();
        if self.series.model.redo(&mut self.doc) == Undone::Series {
            self.series_after_op(ctx);
        }
    }

    /// Raccourcis de la série : Ctrl+Maj+O ajoute des images, Page
    /// précédente / suivante change d'image.
    pub(super) fn series_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(CTRL_SHIFT, Key::O))) {
            self.series_add_dialog(ctx);
        }
        if ctx.egui_wants_keyboard_input() || self.calibration.is_open() || self.mode != Mode::Image || self.series.model.len() < 2 {
            return;
        }
        if ctx.input(|i| i.key_pressed(Key::PageUp)) {
            self.series_step(ctx, -1);
        }
        if ctx.input(|i| i.key_pressed(Key::PageDown)) {
            self.series_step(ctx, 1);
        }
    }

    // ---------- enregistrement ----------

    /// Enregistre toute la série dans `path`. Les images sans fichier fidèle
    /// (captures, images pivotées ou retournées) sont d'abord écrites en PNG
    /// à côté du projet, sans écraser l'original.
    pub(super) fn series_save(&mut self, path: &Path) {
        self.series_sync();
        let n = self.series.model.len();
        let active = self.series.model.active();
        let stem = path.file_stem().map_or_else(|| "projet".to_owned(), |s| s.to_string_lossy().into_owned());
        let mut failure: Option<anyhow::Error> = None;
        // Fichiers à ne jamais écraser : les sources de toutes les images.
        let mut reserved: HashSet<PathBuf> = self.series.model.entries().iter().filter_map(|e| e.path.clone()).collect();
        reserved.extend(self.image_path.clone());
        for i in 0..n {
            let entry = &self.series.model.entries()[i];
            let (id, entry_path) = (entry.id(), entry.path.clone());
            let x = self.series.extras.entry(id).or_default();
            let (current, modified) = if i == active { (self.image_path.clone(), self.image_modified) } else { (entry_path, x.modified) };
            if x.missing || !(current.is_none() || modified) {
                continue;
            }
            // Nom libre : ni la source d'une image de la série (même pas
            // encore traitée), ni un fichier déjà présent, ni un nom déjà
            // attribué pendant cet enregistrement.
            let base = if n == 1 { stem.clone() } else { format!("{stem}-{:02}", i + 1) };
            let png = path.with_file_name(free_name(&base, ".png", |name| {
                let candidate = path.with_file_name(name);
                !reserved.contains(&candidate) && !candidate.exists()
            }));
            reserved.insert(png.clone());
            let loaded;
            let img = if i == active {
                self.image.as_ref()
            } else if x.pixels.is_some() {
                x.pixels.as_ref()
            } else {
                loaded = current.as_deref().map(|p| load_transformed(p, &x.transforms));
                match &loaded {
                    Some(Ok(img)) => Some(img),
                    _ => None,
                }
            };
            let r = match img {
                Some(img) => io::save_image(img, &png),
                None => Err(anyhow::anyhow!(tr!("msg.image_unavailable", n = i + 1))),
            };
            if let Err(e) = r {
                failure = Some(e);
                break;
            }
            x.transforms.clear();
            x.modified = false;
            if i == active {
                self.image_path = Some(png.clone());
                self.image_modified = false;
            } else {
                // Désormais sur disque : plus besoin de la garder en mémoire.
                x.pixels = None;
            }
            if let Some(e) = self.series.model.entry_mut(i) {
                e.path = Some(png);
            }
        }
        if let Some(e) = failure {
            self.error(format!("{e:#}"));
            return;
        }
        let m = &self.series.model;
        let project = io::SeriesProject {
            images: m
                .docs(&self.doc)
                .map(|(e, d)| io::SeriesImage { image_path: e.path.clone(), size: e.size, own_scale: !e.linked, missing: false, document: d.clone() })
                .collect(),
            active: m.active(),
            shared_scale: m.shared_scale(),
        };
        if self.report(io::save_series(&project, path), tr!("msg.project_saved")).is_some() {
            self.series.saved = self.series.model.revision(&self.doc);
            self.project_path = Some(path.to_owned());
        }
    }

    // ---------- sorties de toute la série ----------

    /// Nom affiché de chaque image (colonne « Image » du tableau).
    fn series_names(&self) -> Vec<String> {
        let mut captures = 0;
        self.series
            .model
            .entries()
            .iter()
            .map(|e| match e.path.as_ref().and_then(|p| p.file_name()) {
                Some(n) => n.to_string_lossy().into_owned(),
                None => {
                    captures += 1;
                    tr!("series.capture_name", n = captures)
                }
            })
            .collect()
    }

    /// (nom, document) de chaque image, dans l'ordre de la série.
    fn series_tables(&self) -> Vec<(String, &Document)> {
        self.series_names().into_iter().zip(self.series.model.docs(&self.doc).map(|(_, d)| d)).collect()
    }

    /// Nom de fichier proposé pour une sortie de toute la série.
    fn series_stem(&self) -> String {
        self.project_path.as_ref().and_then(|p| p.file_stem()).map_or_else(|| tr!("file.series_stem").to_owned(), |s| s.to_string_lossy().into_owned())
    }

    fn series_dir(&self) -> Option<&Path> {
        self.project_path.as_ref().or(self.image_path.as_ref()).and_then(|p| p.parent())
    }

    fn series_export_csv(&mut self) {
        self.series_sync();
        let mut d = rfd::FileDialog::new()
            .set_title(tr!("dialog.export_series_csv"))
            .set_file_name(format!("{}{}.csv", self.series_stem(), tr!("file.measurements_suffix")))
            .add_filter("CSV", &["csv"]);
        if let Some(dir) = self.series_dir() {
            d = d.set_directory(dir);
        }
        if let Some(p) = d.save_file() {
            let r = io::export_series_csv(&self.series_tables(), &p);
            self.report(r, tr!("msg.csv_exported"));
        }
    }

    /// Images annotées de toute la série dans un dossier, une par image,
    /// calculées dans un fil à part (la progression s'affiche en bas).
    fn series_export_images(&mut self, ctx: &egui::Context) {
        if self.series.export.is_some() {
            return;
        }
        let mut d = rfd::FileDialog::new().set_title(tr!("dialog.export_series_images"));
        if let Some(dir) = self.series_dir() {
            d = d.set_directory(dir);
        }
        let Some(dir) = d.pick_folder() else { return };
        self.series_sync();
        enum Source {
            File(PathBuf, Vec<io::Transform>),
            Pixels(RgbaImage),
        }
        let suffix = tr!("file.measurements_suffix");
        let active = self.series.model.active();
        let mut used = HashSet::new();
        let mut items = vec![];
        for (i, (e, doc)) in self.series.model.docs(&self.doc).enumerate() {
            let x = self.series.extras.get(&e.id());
            if x.is_some_and(|x| x.missing) {
                continue;
            }
            let transforms = x.map(|x| x.transforms.clone()).unwrap_or_default();
            let unfaithful = i == active && (self.image_path.is_none() || (self.image_modified && transforms.is_empty()));
            let source = match (&e.path, x.and_then(|x| x.pixels.as_ref())) {
                _ if unfaithful => self.image.clone().map(Source::Pixels),
                (_, Some(px)) => Some(Source::Pixels(px.clone())),
                (Some(p), None) => Some(Source::File(p.clone(), transforms)),
                (None, None) => None,
            };
            let Some(source) = source else { continue };
            let stem = e.path.as_ref().and_then(|p| p.file_stem()).map_or_else(|| format!("capture-{:02}", i + 1), |s| s.to_string_lossy().into_owned());
            let name = free_name(&stem, &format!("{suffix}.png"), |name| !used.contains(name));
            used.insert(name.clone());
            items.push((source, doc.clone(), dir.join(name)));
        }
        let total = items.len();
        if total == 0 {
            return;
        }
        let (tx, rx) = channel();
        let (brightness, contrast, ctx) = (self.brightness, self.contrast, ctx.clone());
        let spawned = std::thread::Builder::new().name("export-serie".to_owned()).spawn(move || {
            for (k, (source, doc, out)) in items.into_iter().enumerate() {
                let r = match source {
                    Source::File(p, t) => load_transformed(&p, &t),
                    Source::Pixels(px) => Ok(px),
                }
                .and_then(|img| {
                    let base = if brightness != 0.0 || contrast != 0.0 { io::adjust(&img, brightness, contrast) } else { img };
                    io::save_image(&io::render_annotated(&base, &doc), &out)
                });
                if tx.send((k + 1, total, r.err().map(|e| format!("{e:#}")))).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        if spawned.is_ok() {
            self.series.export = Some(ExportJob { rx, dir: dir.to_string_lossy().into_owned(), failed: 0 });
            self.info(tr!("msg.series_export_progress", done = 0, total = total));
        }
    }

    fn series_export_poll(&mut self) {
        let Some(job) = &mut self.series.export else { return };
        let mut last = None;
        let mut error = None;
        while let Ok((done, total, e)) = job.rx.try_recv() {
            if e.is_some() {
                job.failed += 1;
                error = e;
            }
            last = Some((done, total));
        }
        let Some((done, total)) = last else { return };
        if done < total {
            self.info(tr!("msg.series_export_progress", done = done, total = total));
            return;
        }
        let job = self.series.export.take().unwrap();
        match error {
            Some(e) if job.failed > 0 => self.error(tr!("msg.series_export_failed", n = job.failed, detail = e)),
            _ if job.failed > 0 => self.error(tr!("msg.series_export_failed", n = job.failed, detail = "")),
            _ => self.info(tr!("msg.series_exported", n = total, dir = job.dir)),
        }
    }

    /// Entrées « toute la série » du menu Exporter (rien avec une seule image).
    pub(super) fn series_export_menu(&mut self, ui: &mut Ui) {
        if self.series.model.len() < 2 {
            return;
        }
        ui.separator();
        ui.label(RichText::new(tr!("series.whole", n = self.series.model.len())).weak());
        if ui.button(tr!("series.export_csv")).clicked() {
            self.series_export_csv();
        }
        if ui.button(tr!("series.copy_table")).clicked() {
            self.series_sync();
            ui.ctx().copy_text(io::series_clipboard_table(&self.series_tables()));
            self.info(tr!("msg.table_copied"));
        }
        if ui.add_enabled(self.series.export.is_none(), egui::Button::new(tr!("series.export_images"))).clicked() {
            let ctx = ui.ctx().clone();
            self.series_export_images(&ctx);
        }
    }

    // ---------- interface ----------

    /// Menu « Série » de la barre d'outils.
    pub(super) fn series_menu(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let mut act = None;
        ui.menu_button(format!("🗀  {}", tr!("series.menu")), |ui| {
            let m = &self.series.model;
            let (n, active) = (m.len(), m.active());
            if ui.button(format!("➕  {}", tr!("series.add"))).on_hover_text(tr!("series.add_tip")).clicked() {
                act = Some(Act::Add);
            }
            ui.add_enabled_ui(n > 1, |ui| {
                if ui.add_enabled(active > 0, egui::Button::new(format!("⏴  {}", tr!("series.previous")))).on_hover_text(tr!("series.previous_tip")).clicked() {
                    act = Some(Act::Go(active - 1));
                }
                if ui.add_enabled(active + 1 < n, egui::Button::new(format!("⏵  {}", tr!("series.next")))).on_hover_text(tr!("series.next_tip")).clicked() {
                    act = Some(Act::Go(active + 1));
                }
                if ui.button(tr!("series.remove")).on_hover_text(tr!("series.remove_tip")).clicked() {
                    act = Some(Act::Remove(active));
                }
                ui.separator();
                act = scale_options(ui, m, active).or(act.take());
                ui.separator();
                ui.checkbox(&mut self.series.strip_open, tr!("series.show_strip"));
            });
        });
        if let Some(a) = act {
            self.series_do(&ctx, a);
        }
    }

    /// Bande de vignettes, sous l'image. Absente tant qu'il n'y a qu'une image.
    pub(super) fn series_strip(&mut self, ui: &mut Ui) {
        if self.series.model.len() < 2 || self.mode != Mode::Image || self.image.is_none() {
            return;
        }
        let ctx = ui.ctx().clone();
        let mut act = None;
        egui::Panel::bottom("series").resizable(false).show(ui, |ui| {
            ui.add_space(2.0);
            act = self.series_strip_header(ui);
            if self.series.strip_open {
                act = self.series_strip_thumbs(ui).or(act.take());
            }
            ui.add_space(2.0);
        });
        if let Some(a) = act {
            self.series_do(&ctx, a);
        }
    }

    fn series_strip_header(&mut self, ui: &mut Ui) -> Option<Act> {
        let mut act = None;
        ui.horizontal(|ui| {
            let m = &self.series.model;
            let (n, active) = (m.len(), m.active());
            let open = self.series.strip_open;
            let (arrow, tip) = if open { ("⏷", tr!("series.hide_tip")) } else { ("⏶", tr!("series.show_tip")) };
            if ui.small_button(arrow).on_hover_text(tip).clicked() {
                self.series.strip_open = !open;
            }
            if ui.add_enabled(active > 0, egui::Button::new("⏴").small()).on_hover_text(tr!("series.previous_tip")).clicked() {
                act = Some(Act::Go(active - 1));
            }
            ui.label(RichText::new(tr!("series.position", i = active + 1, n = n)).strong());
            if ui.add_enabled(active + 1 < n, egui::Button::new("⏵").small()).on_hover_text(tr!("series.next_tip")).clicked() {
                act = Some(Act::Go(active + 1));
            }
            if let Some(name) = self.series_names().into_iter().nth(active) {
                ui.label(name);
            }
            if self.series.shown.is_some_and(|id| self.series.extras.get(&id).is_some_and(|x| x.missing)) {
                ui.label(RichText::new(format!("⚠ {}", tr!("series.missing"))).color(Color32::from_rgb(255, 170, 60)));
            }
            ui.separator();
            act = scale_options(ui, m, active).or(act.take());
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button(format!("➕ {}", tr!("series.add"))).on_hover_text(tr!("series.add_tip")).clicked() {
                    act = Some(Act::Add);
                }
            });
        });
        act
    }

    fn series_strip_thumbs(&mut self, ui: &mut Ui) -> Option<Act> {
        let mut act = None;
        let s = &mut self.series;
        let (n, active, shared) = (s.model.len(), s.model.active(), s.model.shared_scale());
        let names = {
            let mut captures = 0;
            s.model
                .entries()
                .iter()
                .map(|e| match e.path.as_ref().and_then(|p| p.file_name()) {
                    Some(n) => n.to_string_lossy().into_owned(),
                    None => {
                        captures += 1;
                        tr!("series.capture_name", n = captures)
                    }
                })
                .collect::<Vec<_>>()
        };
        egui::ScrollArea::horizontal().id_salt("meteroscope_serie").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let mut rects: Vec<Rect> = Vec::with_capacity(n);
                // (image saisie, position du pointeur, relâchée).
                let mut drag: Option<(usize, Pos2, bool)> = None;
                for (i, (e, doc)) in s.model.docs(&self.doc).enumerate() {
                    let id = e.id();
                    let tex = s.thumbs.get(id);
                    let missing = s.extras.get(&id).is_some_and(|x| x.missing) || s.thumbs.failed(id);
                    let aspect = tex.map(|t| t.aspect_ratio()).or(e.size.map(|(w, h)| w as f32 / h.max(1) as f32)).unwrap_or(4.0 / 3.0).clamp(0.5, 2.5);
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2((THUMB_H * aspect).round(), THUMB_H), Sense::click_and_drag());
                    rects.push(rect);
                    let p = ui.painter();
                    let inner = rect.shrink(2.0);
                    p.rect_filled(inner, CornerRadius::same(2), ui.visuals().extreme_bg_color);
                    match tex {
                        Some(t) => {
                            p.image(t.id(), inner, Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
                        }
                        None if missing => {
                            p.text(inner.center(), Align2::CENTER_CENTER, "⚠", FontId::proportional(22.0), Color32::from_rgb(255, 170, 60));
                        }
                        None => {
                            p.text(inner.center(), Align2::CENTER_CENTER, "…", FontId::proportional(18.0), ui.visuals().weak_text_color());
                        }
                    }
                    let badge = |pos: Pos2, align: Align2, text: String| {
                        let galley = p.layout_no_wrap(text, FontId::proportional(10.5), Color32::WHITE);
                        let r = align.anchor_size(pos, galley.size() + egui::vec2(6.0, 2.0));
                        p.rect_filled(r, CornerRadius::same(3), Color32::from_black_alpha(170));
                        p.galley(r.min + egui::vec2(3.0, 1.0), galley, Color32::WHITE);
                    };
                    badge(inner.left_top() + egui::vec2(2.0, 2.0), Align2::LEFT_TOP, (i + 1).to_string());
                    if !doc.measurements.is_empty() {
                        badge(inner.right_bottom() - egui::vec2(2.0, 2.0), Align2::RIGHT_BOTTOM, doc.measurements.len().to_string());
                    }
                    if shared && !e.linked {
                        badge(inner.left_bottom() + egui::vec2(2.0, -2.0), Align2::LEFT_BOTTOM, "📏".to_owned());
                    }
                    if missing && tex.is_some() {
                        badge(inner.right_top() + egui::vec2(-2.0, 2.0), Align2::RIGHT_TOP, "⚠".to_owned());
                    }
                    let stroke = if i == active {
                        Stroke::new(2.0, ui.visuals().selection.bg_fill)
                    } else if resp.hovered() {
                        Stroke::new(1.0, ui.visuals().widgets.hovered.bg_stroke.color)
                    } else {
                        Stroke::NONE
                    };
                    p.rect_stroke(rect.shrink(1.0), CornerRadius::same(3), stroke, StrokeKind::Inside);

                    if i == active && s.scrolled != Some(id) {
                        resp.scroll_to_me(Some(Align::Center));
                        s.scrolled = Some(id);
                    }
                    if resp.clicked() {
                        act = Some(Act::Go(i));
                    }
                    if (resp.dragged() || resp.drag_stopped())
                        && let Some(pos) = ui.ctx().pointer_interact_pos()
                    {
                        drag = Some((i, pos, resp.drag_stopped()));
                    }
                    resp.context_menu(|ui| {
                        if ui.add_enabled(i > 0, egui::Button::new(tr!("series.move_before"))).clicked() {
                            act = Some(Act::Move(i, i - 1));
                            ui.close();
                        }
                        if ui.add_enabled(i + 1 < n, egui::Button::new(tr!("series.move_after"))).clicked() {
                            act = Some(Act::Move(i, i + 1));
                            ui.close();
                        }
                        let mut own = !e.linked;
                        if ui.add_enabled(shared, egui::Checkbox::new(&mut own, tr!("series.own_scale"))).on_hover_text(tr!("series.own_scale_tip")).clicked() {
                            act = Some(Act::Linked(i, !own));
                            ui.close();
                        }
                        ui.separator();
                        if ui.button(tr!("series.remove")).on_hover_text(tr!("series.remove_tip")).clicked() {
                            act = Some(Act::Remove(i));
                            ui.close();
                        }
                    });
                    resp.on_hover_ui(|ui| {
                        ui.label(RichText::new(&names[i]).strong());
                        if let Some((w, h)) = e.size {
                            ui.label(format!("{w} × {h} px"));
                        }
                        ui.label(tr!("series.measure_count", n = doc.measurements.len()));
                        if shared && !e.linked {
                            ui.label(tr!("series.own_scale"));
                        }
                        if missing {
                            ui.label(RichText::new(tr!("series.missing")).color(Color32::from_rgb(255, 170, 60)));
                        }
                    });
                }
                // Glisser une vignette la déplace : un trait montre où elle ira.
                if let Some((from, pos, released)) = drag {
                    let to = rects.iter().position(|r| pos.x < r.right()).unwrap_or(n - 1);
                    if to != from {
                        let r = rects[to];
                        let x = if to > from { r.right() + 1.0 } else { r.left() - 1.0 };
                        ui.painter().vline(x, r.y_range(), Stroke::new(3.0, ui.visuals().selection.bg_fill));
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                        if released {
                            act = Some(Act::Move(from, to));
                        }
                    }
                }
            });
        });
        act
    }

    /// Statistiques par type sur toute la série, sous celles de l'image
    /// (dans le panneau des mesures). Rien avec une seule image.
    pub(super) fn series_stats(&mut self, ui: &mut Ui) {
        let m = &self.series.model;
        if m.len() < 2 {
            return;
        }
        egui::Panel::bottom("series_stats").resizable(false).show(ui, |ui| {
            egui::CollapsingHeader::new(RichText::new(tr!("series.stats_title", n = m.len())).strong()).id_salt("meteroscope_stats_serie").default_open(false).show(ui, |ui| {
                let summary = series_summary(m.docs(&self.doc).map(|(_, d)| d));
                let markers: usize = m.docs(&self.doc).map(|(_, d)| d.marker_count()).sum();
                if summary.is_empty() && markers == 0 {
                    ui.label(RichText::new(tr!("stats.empty")).weak());
                    return;
                }
                let number = meteroscope_core::measure::format_number;
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                egui::ScrollArea::horizontal().id_salt("meteroscope_stats_serie_defilement").show(ui, |ui| {
                    egui::Grid::new("meteroscope_stats_serie_grille").num_columns(6).spacing([8.0, 2.0]).show(ui, |ui| {
                        for h in ["", "n", tr!("stats.mean"), tr!("stats.std_dev"), tr!("stats.min"), tr!("stats.max")] {
                            ui.label(RichText::new(h).small().weak());
                        }
                        ui.end_row();
                        for (kind, _, unit, s) in &summary {
                            ui.label(RichText::new(format!("{} ({unit})", kind.label())).strong());
                            ui.label(s.n.to_string());
                            let sd = if s.n < 2 { "—".to_owned() } else { number(s.sd) };
                            for v in [number(s.mean), sd, number(s.min), number(s.max)] {
                                ui.label(v);
                            }
                            ui.end_row();
                        }
                        if markers > 0 {
                            ui.label(RichText::new(tr!("stats.markers")).strong());
                            ui.label(markers.to_string());
                            ui.end_row();
                        }
                    });
                });
            });
        });
    }
}

/// Cases « même échelle pour toute la série » et « échelle propre à cette image ».
fn scale_options(ui: &mut Ui, m: &Series, active: usize) -> Option<Act> {
    let mut act = None;
    let mut shared = m.shared_scale();
    if ui.checkbox(&mut shared, tr!("series.shared_scale")).on_hover_text(tr!("series.shared_scale_tip")).changed() {
        act = Some(Act::Shared(shared));
    }
    let mut own = m.active_entry().is_some_and(|e| !e.linked);
    if ui.add_enabled(m.shared_scale(), egui::Checkbox::new(&mut own, tr!("series.own_scale"))).on_hover_text(tr!("series.own_scale_tip")).changed() {
        act = Some(Act::Linked(active, !own));
    }
    act
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trois images de même nom de base, dont une qui porte déjà le nom de
    /// repli d'une autre : trois sorties distinctes.
    #[test]
    fn free_names_never_collide() {
        let mut used = HashSet::new();
        let mut out = vec![];
        for stem in ["a-2", "a", "a", "a", "a-2"] {
            let name = free_name(stem, "-mesures.png", |n| !used.contains(n));
            assert!(used.insert(name.clone()), "{name} attribué deux fois");
            out.push(name);
        }
        assert_eq!(out, ["a-2-mesures.png", "a-mesures.png", "a-3-mesures.png", "a-4-mesures.png", "a-2-2-mesures.png"]);
    }

    /// Une capture en première position ne prend pas le nom d'une photo
    /// d'origine placée plus loin dans la série.
    #[test]
    fn free_name_skips_reserved_sources() {
        let reserved: HashSet<PathBuf> = [PathBuf::from("/d/lot-01.png")].into();
        let name = free_name("lot-01", ".png", |n| !reserved.contains(&Path::new("/d").join(n)));
        assert_eq!(name, "lot-01-2.png");
    }
}

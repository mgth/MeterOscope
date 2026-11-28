//! Série d'images dans la coquille : ouverture, passage d'une image à
//! l'autre, retrait, bande de vignettes, enregistrement et sorties de toute
//! la série.
//!
//! Tout ce qui s'ouvre passe par ici ([`MeterOscopeApp::open_paths`],
//! [`MeterOscopeApp::series_add`]) : une image s'ajoute à ce qui est ouvert
//! (une série à partir de la deuxième), un projet remplace le travail en
//! cours. Les deux s'annulent par Ctrl+Z.
//!
//! Le reste de l'application continue de travailler sur « l'image
//! courante » : `app.image`, `app.image_path`, son orientation
//! (`app.applied`, `app.file_orientation`) et `app.doc` sont ceux de l'image affichée. Ce module les range et les
//! ressort quand on change d'image ([`MeterOscopeApp::series_show`]).
//! Seule l'image affichée est décodée en mémoire ; les autres sont relues
//! de leur fichier à l'affichage (les captures sans fichier restent en
//! mémoire jusqu'à l'enregistrement du projet).

use super::{MeterOscopeApp, Mode};
use crate::thumbs::{Thumbs, load_transformed};
use egui::{Align, Align2, Color32, CornerRadius, FontId, Key, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui};
use image::RgbaImage;
use meteroscope_core::series::Entry;
use meteroscope_core::{Document, Orientation, Series, Tool, Undone, series_summary};
use meteroscope_i18n::tr;
use meteroscope_io as io;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
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
    pixels: Option<Arc<RgbaImage>>,
    /// Orientation des pixels mesurés et celle du fichier (`app.applied` et
    /// `app.file_orientation` quand l'image est affichée).
    applied: Orientation,
    file_orientation: Orientation,
    /// Rotations et miroirs qui mènent du fichier aux pixels mesurés : rejoués
    /// à la relecture, ce qui évite de garder l'image modifiée en mémoire.
    transforms: Vec<io::Transform>,
    /// L'image diffère de son fichier.
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

pub(crate) struct SeriesState {
    pub(super) model: Series,
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
    /// Les pixels de l'image affichée ont changé : vignette à refaire.
    thumb_stale: bool,
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
            thumb_stale: false,
        }
    }
}

/// Action demandée depuis la bande ou le menu, exécutée après le dessin.
enum Act {
    Go(usize),
    Remove(usize),
    Move(usize, usize),
    Shared(bool),
    Linked(usize, bool),
}

/// Image à ouvrir : son fichier, ou ses pixels quand aucun fichier ne les
/// contient (collage, capture), et le document qu'elle apporte (capture du
/// flux caméra, avec ses mesures).
pub(super) struct Incoming {
    pub path: Option<PathBuf>,
    pub size: (u32, u32),
    pub pixels: Option<RgbaImage>,
    pub doc: Option<Document>,
}

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

/// Nom affiché de chaque image (colonne « Image » du tableau, bande).
fn names(model: &Series) -> Vec<String> {
    let mut captures = 0;
    model
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

fn placeholder(size: Option<(u32, u32)>) -> RgbaImage {
    let (w, h) = size.unwrap_or(PLACEHOLDER);
    RgbaImage::from_pixel(w.max(1), h.max(1), image::Rgba([72, 72, 78, 255]))
}

impl MeterOscopeApp {
    // ---------- état ----------

    /// Modifications non enregistrées, sur n'importe quelle image de la série.
    pub(super) fn series_dirty(&self) -> bool {
        self.series.model.revision(self.image_doc()) != self.series.saved
    }

    /// Document de l'image affichée : `self.doc`, ou celui mis de côté
    /// pendant le flux caméra.
    fn image_doc(&self) -> &Document {
        if self.mode == Mode::Camera { &self.camera.other_doc } else { &self.doc }
    }

    /// Reporte dans la série l'état de l'image affichée (chemin, dimensions)
    /// et lui fait suivre le document actif (historique, échelle commune).
    pub(super) fn series_sync(&mut self) {
        // En mode caméra `self.doc` est le document du flux : on rend
        // d'abord à l'image le sien, seul connu de la série.
        self.mode_image();
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
        // Pendant le flux caméra, le document de l'image est de côté et ne change pas.
        if self.mode == Mode::Image {
            self.series_sync();
        }
        if std::mem::take(&mut self.series.thumb_stale)
            && let (Some(id), Some(img)) = (self.series.shown, &self.image)
            && !self.series.extras.get(&id).is_some_and(|x| x.missing)
        {
            self.series.thumbs.set(ctx, id, img);
        }
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
        // Un dialogue ouvert se rapportait à l'image qui s'en va.
        self.close_dialogs();
        let old_size = self.image.as_ref().map(|i| i.dimensions());
        if let Some(old) = self.series.shown.take() {
            let x = self.series.extras.entry(old).or_default();
            x.applied = self.applied;
            x.file_orientation = self.file_orientation;
            x.modified = self.applied != self.file_orientation;
            x.transforms = io::orientation_transforms(self.file_orientation.to(self.applied));
            // Sans fichier, les pixels sont le seul exemplaire ; une image
            // pivotée ou retournée se retrouve en rejouant son orientation.
            let keep = !x.missing && self.image_path.is_none();
            x.pixels = if keep { self.image.take() } else { None };
        }
        self.image = None;
        self.pixels_changed();
        // L'image corrigée de la perspective appartenait à l'ancienne photo,
        // comme une détection en cours et les profils (les identifiants de
        // mesures se recoupent d'un document à l'autre).
        self.flat = None;
        self.detect.invalidate();
        self.profiles.invalidate();
        self.straight = None;
        self.canvas.cancel();
        let Some(entry) = self.series.model.active_entry() else {
            self.image_path = None;
            self.applied = Orientation::IDENTITY;
            self.file_orientation = Orientation::IDENTITY;
            self.refresh_texture(ctx);
            return;
        };
        let (id, path, size) = (entry.id(), entry.path.clone(), entry.size);
        let x = self.series.extras.entry(id).or_default();
        let mut failure = None;
        let img = match (x.pixels.take(), &path) {
            (Some(px), _) => Some(px),
            (None, Some(p)) => load_transformed(p, &x.transforms).map(Arc::new).map_err(|e| failure = Some(format!("{e:#}"))).ok(),
            (None, None) => None,
        };
        x.missing = img.is_none();
        self.applied = x.applied;
        self.file_orientation = x.file_orientation;
        self.image_path = path;
        let img = match img {
            Some(img) => {
                if !self.series.thumbs.has(id) {
                    self.series.thumbs.set(ctx, id, &img);
                }
                img
            }
            // Image introuvable : les mesures restent visibles sur un fond gris.
            None => Arc::new(placeholder(size)),
        };
        if old_size != Some(img.dimensions()) {
            self.canvas.fit();
        }
        self.image = Some(img);
        self.pixels_changed();
        self.series.thumb_stale = false;
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

    // ---------- ouverture ----------

    /// Ouvre ce qui a été choisi, déposé, collé ou repris des récents. Un
    /// projet remplace le travail en cours ; des images s'y ajoutent (la
    /// première ajoutée est affichée). Les fichiers ouverts comptent parmi
    /// les récents et fixent le dossier des dialogues.
    pub(super) fn open_paths(&mut self, ctx: &egui::Context, paths: Vec<PathBuf>) {
        if let Some(p) = paths.iter().find(|p| is_project(p)) {
            self.series_open_project(ctx, p);
            return;
        }
        let total = paths.len();
        let mut unreadable = None;
        let mut found = vec![];
        for p in paths {
            match io::image_dimensions(&p) {
                Some(size) => found.push(Incoming { path: Some(p), size, pixels: None, doc: None }),
                None => unreadable = Some(p),
            }
        }
        let bad = total - found.len();
        // En tête des récents : l'image qui sera affichée.
        for p in found.iter().rev().filter_map(|i| i.path.as_deref()) {
            self.settings.add_recent(p);
        }
        self.series_add(ctx, found);
        match unreadable {
            // Un seul fichier : l'erreur précise plutôt qu'un décompte.
            Some(p) if total == 1 => match io::load_image(&p) {
                Err(e) => self.error(format!("{e:#}")),
                Ok(_) => self.error(tr!("msg.images_unreadable", n = 1)),
            },
            Some(_) => self.error(tr!("msg.images_unreadable", n = bad)),
            None => {}
        }
    }

    /// Ouvre un projet (version 1 ou 2) à la place du travail en cours, qui
    /// reste récupérable par Ctrl+Z. Les images introuvables sont signalées
    /// sans bloquer : leurs mesures sont conservées.
    pub(super) fn series_open_project(&mut self, ctx: &egui::Context, path: &Path) {
        let Some(p) = self.report(io::load_series(path), "") else { return };
        let missing: Vec<bool> = p.images.iter().map(|i| i.missing).collect();
        let n = missing.iter().filter(|m| **m).count();
        let several = missing.len() > 1;
        let entries = p.images.into_iter().map(|i| Entry::new(i.image_path, i.size, !i.own_scale, i.document)).collect();
        self.clear_workspace(ctx);
        let s = &mut self.series;
        s.model = Series::from_entries(entries, p.active, p.shared_scale, &mut self.doc);
        for (e, m) in s.model.entries().iter().zip(missing) {
            s.extras.entry(e.id()).or_default().missing = m;
        }
        s.saved = s.model.revision(&self.doc);
        self.project_path = Some(path.to_owned());
        self.series_after_op(ctx);
        self.canvas.fit();
        self.settings.add_recent(path);
        if n > 0 && several {
            self.error(tr!("msg.images_missing", n = n));
        }
    }

    /// Ajoute des images à ce qui est ouvert et affiche la première : c'est
    /// le seul chemin d'ouverture d'une image (fichier, collage, capture).
    /// La première image d'un travail n'est pas une étape à annuler ; les
    /// suivantes se retirent par Ctrl+Z.
    ///
    /// Échelle : une image qui suit l'échelle commune de la série la reçoit ;
    /// les autres, et la première d'un nouveau travail, reçoivent l'échelle
    /// en cours si « garder l'échelle pour l'image suivante » est cochée.
    pub(super) fn series_add(&mut self, ctx: &egui::Context, images: Vec<Incoming>) {
        if images.is_empty() {
            return;
        }
        // En mode caméra, l'image retrouve d'abord son document.
        self.series_sync();
        let was_empty = self.series.model.is_empty();
        let keep = self.scale_presets.keep_scale;
        let kept = keep.then(|| if was_empty { self.kept_scale.clone() } else { self.series.model.carried(&self.doc) });
        let n = images.len();
        let brought = images.iter().any(|i| i.doc.is_some());
        let (entries, pixels): (Vec<_>, Vec<_>) = images.into_iter().map(|i| ((i.path, Some(i.size), i.doc), i.pixels)).unzip();
        let ids = self.series.model.add_with(entries, kept, &mut self.doc);
        for (id, px) in ids.into_iter().zip(pixels) {
            if let Some(px) = px {
                self.series.thumbs.set(ctx, id, &px);
                self.series.extras.entry(id).or_default().pixels = Some(Arc::new(px));
            }
        }
        if was_empty {
            // Ouvrir n'est pas modifier ; des mesures apportées (capture) le sont.
            if !brought {
                self.series.saved = self.series.model.revision(&self.doc);
            }
            self.canvas.set_tool(Tool::Select);
        } else {
            self.info(tr!("msg.images_added", n = n));
        }
        self.series_after_op(ctx);
        // La texture se construit à la prochaine frame de la vue image.
        ctx.request_repaint();
    }

    /// Série vide, comme au lancement (l'export en cours reste suivi).
    pub(super) fn series_reset(&mut self) {
        drop(self.series_take());
    }

    /// Fichiers glissés-déposés : un projet s'ouvre, des images s'ajoutent à
    /// ce qui est ouvert.
    pub(super) fn series_dropped(&mut self, ctx: &egui::Context) {
        let paths: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_owned()).collect());
        if !paths.is_empty() {
            self.open_paths(ctx, paths);
        }
    }

    /// Les pixels de l'image affichée ont changé (rotation, miroir, ou leur
    /// annulation) : sa vignette est refaite à la prochaine image d'interface.
    pub(super) fn series_pixels_changed(&mut self) {
        self.series.thumb_stale = true;
    }

    // ---------- navigation et opérations ----------

    fn series_do(&mut self, ctx: &egui::Context, act: Act) {
        self.series_sync();
        let (m, doc) = (&mut self.series.model, &mut self.doc);
        match act {
            Act::Go(i) => m.activate(i, doc),
            Act::Remove(i) => m.remove(i, doc),
            Act::Move(from, to) => m.move_entry(from, to, doc),
            Act::Shared(on) => m.set_shared_scale(on, doc),
            Act::Linked(i, on) => m.set_linked(i, on, doc),
        };
        self.series_after_op(ctx);
    }

    /// Image précédente (−1) ou suivante (+1).
    pub(super) fn series_step(&mut self, ctx: &egui::Context, delta: isize) {
        let m = &self.series.model;
        if let Some(i) = m.active().checked_add_signed(delta).filter(|i| *i < m.len()) {
            self.series_do(ctx, Act::Go(i));
        }
    }

    pub(super) fn series_can_undo(&self) -> bool {
        if self.mode == Mode::Camera { self.doc.can_undo() } else { self.series.model.can_undo(&self.doc) }
    }
    pub(super) fn series_can_redo(&self) -> bool {
        if self.mode == Mode::Camera { self.doc.can_redo() } else { self.series.model.can_redo(&self.doc) }
    }

    /// Ctrl+Z : la plus récente entre la dernière modification de l'image
    /// affichée et la dernière opération de série. Faux s'il n'y avait rien
    /// à annuler (l'appelant passe alors au remplacement d'image).
    pub(super) fn series_undo(&mut self, ctx: &egui::Context) -> bool {
        // Sur le flux caméra : l'historique est celui du document du flux.
        if self.mode == Mode::Camera {
            return self.doc.undo();
        }
        self.series_sync();
        let done = self.series.model.undo(&mut self.doc);
        if done == Undone::Series {
            self.series_after_op(ctx);
        }
        done != Undone::Nothing
    }

    pub(super) fn series_redo(&mut self, ctx: &egui::Context) -> bool {
        if self.mode == Mode::Camera {
            return self.doc.redo();
        }
        self.series_sync();
        let done = self.series.model.redo(&mut self.doc);
        if done == Undone::Series {
            self.series_after_op(ctx);
        }
        done != Undone::Nothing
    }

    /// Retire la série de l'application avec l'espace de travail (image
    /// remplacée, voir `history`) ; un export en cours reste suivi ici.
    pub(super) fn series_take(&mut self) -> SeriesState {
        let mut taken = std::mem::take(&mut self.series);
        self.series.export = taken.export.take();
        self.series.strip_open = taken.strip_open;
        taken
    }

    /// Remet une série retirée par `series_take`.
    pub(super) fn series_put(&mut self, mut series: SeriesState) {
        series.export = self.series.export.take();
        self.series = series;
    }

    /// Empreinte de l'espace de travail pour le rétablissement d'une image
    /// remplacée : il n'a lieu que si rien n'a changé dans toute la série.
    pub(super) fn series_key(series: &SeriesState, doc: &Document) -> String {
        format!("{}|{}", series.model.revision(doc), crate::history::doc_key(doc))
    }

    /// Révision de toute la série (document affiché compris).
    pub(super) fn series_revision(&self) -> u64 {
        self.series.model.revision(self.image_doc())
    }

    /// Raccourcis de la série : Page précédente / suivante change d'image
    /// (jamais sous un dialogue ouvert, qui se rapporte à l'image affichée).
    pub(super) fn series_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() || self.dialog_open() || self.mode != Mode::Image || self.series.model.len() < 2 {
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
        if self.series_write(path) {
            self.settings.add_recent(path);
        }
    }

    /// Écrit la série dans `path` (images sans fichier fidèle d'abord).
    /// Aucun fichier existant n'est réécrit : ni les sources, ni l'image
    /// d'un autre projet, ni un fichier écrit par un enregistrement précédent.
    /// Renvoie `false` en cas d'échec (déjà signalé).
    pub(super) fn series_write(&mut self, path: &Path) -> bool {
        // Un quart de tour ou un miroir annulé : les pixels suivent d'abord.
        self.sync_orientation();
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
            let (current, modified) = if i == active { (self.image_path.clone(), self.applied != self.file_orientation) } else { (entry_path, x.modified) };
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
                self.image.as_deref()
            } else if x.pixels.is_some() {
                x.pixels.as_deref()
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
            x.file_orientation = x.applied;
            if i == active {
                self.image_path = Some(png.clone());
                self.file_orientation = self.applied;
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
            return false;
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
        if self.report(io::save_series(&project, path), tr!("msg.project_saved")).is_none() {
            return false;
        }
        self.series.saved = self.series.model.revision(&self.doc);
        self.project_path = Some(path.to_owned());
        true
    }

    // ---------- sorties de toute la série ----------

    /// (nom, document) de chaque image, dans l'ordre de la série.
    fn series_tables(&self) -> Vec<(String, &Document)> {
        names(&self.series.model).into_iter().zip(self.series.model.docs(self.image_doc()).map(|(_, d)| d)).collect()
    }

    /// Nom de fichier proposé pour une sortie de toute la série.
    fn series_stem(&self) -> String {
        self.project_path.as_ref().and_then(|p| p.file_stem()).map_or_else(|| tr!("file.series_stem").to_owned(), |s| s.to_string_lossy().into_owned())
    }

    fn series_export_csv(&mut self) {
        self.series_sync();
        let name = format!("{}{}.csv", self.series_stem(), tr!("file.measurements_suffix"));
        if let Some(p) = self.save_dialog_named(tr!("dialog.export_series_csv"), &name, "csv") {
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
        let Some(dir) = self.file_dialog(tr!("dialog.export_series_images")).pick_folder() else { return };
        // Le dossier choisi est retenu, comme celui d'un fichier enregistré.
        self.settings.set_last_dir_of(&dir.join("x"));
        self.sync_orientation();
        self.series_sync();
        enum Source {
            File(PathBuf, Vec<io::Transform>),
            Pixels(RgbaImage),
        }
        let suffix = tr!("file.measurements_suffix");
        let active = self.series.model.active();
        let mut used = HashSet::new();
        let mut items = vec![];
        for (i, (e, doc)) in self.series.model.docs(self.image_doc()).enumerate() {
            let x = self.series.extras.get(&e.id());
            if x.is_some_and(|x| x.missing) {
                continue;
            }
            // L'image affichée : son orientation est celle de l'application.
            let transforms = if i == active {
                io::orientation_transforms(self.file_orientation.to(self.applied))
            } else {
                x.map(|x| x.transforms.clone()).unwrap_or_default()
            };
            let unfaithful = i == active && self.image_path.is_none();
            let source = match (&e.path, x.and_then(|x| x.pixels.as_deref())) {
                _ if unfaithful => self.image.as_deref().cloned().map(Source::Pixels),
                (_, Some(px)) => Some(Source::Pixels(px.clone())),
                (Some(p), None) => Some(Source::File(p.clone(), transforms)),
                (None, None) => None,
            };
            let Some(source) = source else { continue };
            let stem = e.path.as_ref().and_then(|p| p.file_stem()).map_or_else(|| format!("capture-{:02}", i + 1), |s| s.to_string_lossy().into_owned());
            // Jamais sur un fichier déjà présent : ce pourrait être une photo
            // d'origine, sous ce nom, par un lien symbolique ou un lien physique.
            let name = free_name(&stem, &format!("{suffix}.png"), |name| !used.contains(name) && !dir.join(name).exists());
            used.insert(name.clone());
            items.push((source, doc.clone(), dir.join(name)));
        }
        let total = items.len();
        if total == 0 {
            return;
        }
        let (tx, rx) = channel();
        let (brightness, contrast, ctx) = (self.brightness, self.contrast, ctx.clone());
        let filter = self.filter.unwrap_or_default();
        let spawned = std::thread::Builder::new().name("export-serie".to_owned()).spawn(move || {
            for (k, (source, doc, out)) in items.into_iter().enumerate() {
                let r = match source {
                    Source::File(p, t) => load_transformed(&p, &t),
                    Source::Pixels(px) => Ok(px),
                }
                .and_then(|img| {
                    // Les mesures sont dans l'image corrigée de la perspective.
                    let img = match &doc.perspective {
                        Some(p) => io::warp_perspective(&img, p, filter).unwrap_or(img),
                        None => img,
                    };
                    let base = if brightness != 0.0 || contrast != 0.0 { io::adjust(&img, brightness, contrast) } else { img };
                    // Comme l'export d'une image : redressement compris.
                    io::save_image(&io::render_view(&base, &doc, filter), &out)
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
            let scale = ScaleOptions::of(m, self.image_doc());
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
                act = scale.show(ui).or(act.take());
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
            if let Some(name) = names(m).into_iter().nth(active) {
                ui.label(name);
            }
            if self.series.shown.is_some_and(|id| self.series.extras.get(&id).is_some_and(|x| x.missing)) {
                ui.label(RichText::new(format!("⚠ {}", tr!("series.missing"))).color(Color32::from_rgb(255, 170, 60)));
            }
            ui.separator();
            act = ScaleOptions::of(m, &self.doc).show(ui).or(act.take());
        });
        act
    }

    fn series_strip_thumbs(&mut self, ui: &mut Ui) -> Option<Act> {
        let mut act = None;
        let s = &mut self.series;
        let (n, active, shared) = (s.model.len(), s.model.active(), s.model.shared_scale());
        let names = names(&s.model);
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
                    // Échelle propre : détachée, ou à perspective corrigée.
                    let corrected = doc.perspective.is_some();
                    let own = !e.linked || corrected;
                    if shared && own {
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
                        let mut own = own;
                        if ui.add_enabled(shared && !corrected, egui::Checkbox::new(&mut own, tr!("series.own_scale"))).on_hover_text(tr!("series.own_scale_tip")).clicked() {
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
                        if shared && own {
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
                let summary = series_summary(m.docs(self.image_doc()).map(|(_, d)| d));
                let markers: usize = m.docs(self.image_doc()).map(|(_, d)| d.marker_count()).sum();
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

/// Cases « même échelle pour toute la série » et « échelle propre à cette
/// image » (toujours propre, et non modifiable, si sa perspective est corrigée).
struct ScaleOptions {
    active: usize,
    shared: bool,
    own: bool,
    corrected: bool,
}

impl ScaleOptions {
    /// `doc` : document de l'image affichée.
    fn of(m: &Series, doc: &Document) -> Self {
        Self { active: m.active(), shared: m.shared_scale(), own: m.own_scale(m.active(), doc), corrected: doc.perspective.is_some() }
    }

    fn show(&self, ui: &mut Ui) -> Option<Act> {
        let mut act = None;
        let mut shared = self.shared;
        if ui.checkbox(&mut shared, tr!("series.shared_scale")).on_hover_text(tr!("series.shared_scale_tip")).changed() {
            act = Some(Act::Shared(shared));
        }
        let mut own = self.own;
        let free = self.shared && !self.corrected;
        if ui.add_enabled(free, egui::Checkbox::new(&mut own, tr!("series.own_scale"))).on_hover_text(tr!("series.own_scale_tip")).changed() {
            act = Some(Act::Linked(self.active, !own));
        }
        act
    }
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

    /// Export dans le dossier des photos : aucun nom de sortie ne désigne un
    /// fichier déjà présent, même atteint par un lien symbolique ou physique.
    #[cfg(unix)]
    #[test]
    fn export_names_spare_existing_files() {
        let root = std::env::temp_dir().join(format!("meteroscope-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let photos = root.join("photos");
        std::fs::create_dir_all(&photos).unwrap();
        std::fs::write(photos.join("a.png"), b"x").unwrap();
        std::fs::write(photos.join("original.png"), b"x").unwrap();
        // Lien physique : même fichier que original.png sous un autre nom.
        std::fs::hard_link(photos.join("original.png"), photos.join("a-mesures.png")).unwrap();
        let alias = root.join("alias");
        std::os::unix::fs::symlink(&photos, &alias).unwrap();
        for dir in [&photos, &alias] {
            let name = free_name("a", "-mesures.png", |n| !dir.join(n).exists());
            assert_eq!(name, "a-2-mesures.png");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Capture du flux caméra avec une image ouverte : elle s'ajoute à la
    /// série avec les mesures du flux, garde l'échelle de la série si le flux
    /// n'en a pas, et la première image retrouve son document.
    #[test]
    fn capture_joins_series_from_camera() {
        use meteroscope_core::{Pt, Shape};
        meteroscope_i18n::set_language("fr");
        let ctx = egui::Context::default();
        let mut app = MeterOscopeApp::blank();
        app.add_image(&ctx, RgbaImage::new(40, 30), None);
        assert!(app.doc.calibration.set_from_reference(100.0, 1.0, "mm"));
        app.doc.add(Shape::Marker { p: Pt::new(1.0, 1.0) });
        app.series_frame(&ctx);
        // Mode caméra sans ouvrir de vraie caméra.
        std::mem::swap(&mut app.doc, &mut app.camera.other_doc);
        std::mem::swap(&mut app.canvas, &mut app.camera.other_canvas);
        app.mode = Mode::Camera;
        app.doc.add(Shape::Marker { p: Pt::new(2.0, 2.0) });
        app.doc.add(Shape::Marker { p: Pt::new(3.0, 3.0) });
        // Le flux ne compte pas dans la série ; annuler y agit sur le flux.
        app.series_frame(&ctx);
        assert!(app.is_dirty());
        app.undo(&ctx);
        assert_eq!(app.doc.measurements.len(), 1);
        assert_eq!(app.camera.other_doc.measurements.len(), 1);

        app.freeze(&ctx, RgbaImage::new(40, 30));
        app.series_frame(&ctx);
        assert!(app.mode == Mode::Image);
        assert_eq!((app.series.model.len(), app.series.model.active()), (2, 1));
        assert_eq!(app.doc.measurements.len(), 1);
        assert!(app.doc.calibration.is_calibrated(), "l'échelle de la série est gardée");
        app.series_do(&ctx, Act::Go(0));
        assert_eq!(app.doc.measurements.len(), 1);
        assert!(app.doc.get(app.doc.measurements[0].id).is_some() && app.doc.calibration.is_calibrated());
    }

    /// « Nouveau travail » retire toute la série sans demander : Ctrl+Z la
    /// rend entière, Ctrl+Y revient à l'application vide.
    #[test]
    fn new_work_comes_back_whole() {
        use meteroscope_core::{Pt, Shape};
        meteroscope_i18n::set_language("fr");
        let ctx = egui::Context::default();
        let mut app = MeterOscopeApp::blank();
        app.add_image(&ctx, RgbaImage::new(40, 30), None);
        app.doc.add(Shape::Marker { p: Pt::new(1.0, 1.0) });
        app.add_image(&ctx, RgbaImage::new(40, 30), None);
        app.series_frame(&ctx);
        assert_eq!((app.series.model.len(), app.series.model.active()), (2, 1));

        app.new_work(&ctx);
        app.series_frame(&ctx);
        assert!(app.image.is_none() && app.series.model.is_empty() && app.doc.measurements.is_empty());
        assert!(!app.is_dirty() && app.can_undo());
        app.undo(&ctx);
        assert_eq!((app.series.model.len(), app.series.model.active()), (2, 1));
        assert_eq!(app.image.as_ref().map(|i| i.width()), Some(40));
        app.series_do(&ctx, Act::Go(0));
        assert_eq!(app.doc.measurements.len(), 1);
        app.series_do(&ctx, Act::Go(1));
        assert!(app.can_redo());
        app.redo(&ctx);
        assert!(app.image.is_none() && app.series.model.is_empty());
        // Vide de nouveau : la première image ouverte repart d'un travail neuf.
        app.add_image(&ctx, RgbaImage::new(20, 20), None);
        assert_eq!(app.series.model.len(), 1);
        assert!(!app.is_dirty());
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

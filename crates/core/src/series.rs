//! Série d'images : plusieurs images mesurées dans un même projet.
//!
//! La série est une couche **au-dessus** de [`Document`], qui reste le
//! document d'une seule image. Elle tient une liste d'entrées (référence à
//! l'image + document) et une entrée active.
//!
//! Le document de l'entrée active est « sorti » de la série : l'application
//! le garde chez elle (`app.doc`) et travaille dessus comme s'il était seul.
//! La case correspondante de la série contient pendant ce temps un document
//! vide. C'est pourquoi les méthodes qui touchent à toute la série reçoivent
//! le document actif en paramètre.
//!
//! Annuler / rétablir :
//! - chaque document garde sa propre pile (celle de [`Document`]) ;
//! - les opérations de série (ajout, retrait, déplacement, échelle commune)
//!   ont leur pile ici ;
//! - [`Series::undo`] annule **la plus récente** entre la dernière
//!   modification de l'image affichée et la dernière opération de série.
//!   Les modifications des autres images ne sont jamais annulées d'ici :
//!   on ne change d'image que si l'opération de série annulée retire ou
//!   remet l'image affichée.
//!
//! Échelle commune : quand elle est active, les documents des entrées
//! « liées » portent tous la même [`Calibration`]. Définir l'échelle sur
//! l'image active (par n'importe quel moyen) est repéré par
//! [`Series::observe`], qui la propage ; l'annulation d'un document lié ne
//! ressuscite jamais une ancienne échelle (elle appartient à la série).

use crate::calibration::Calibration;
use crate::document::Document;
use crate::measure::{Quantity, ShapeKind};
use crate::stats::Stats;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Nombre maximal d'opérations de série annulables.
const UNDO_LIMIT: usize = 200;

/// Image à ajouter : fichier (ou `None` pour une capture) et dimensions connues.
pub type NewImage = (Option<PathBuf>, Option<(u32, u32)>);

/// Une image de la série.
#[derive(Clone, Debug)]
pub struct Entry {
    id: u64,
    /// Fichier image (`None` : capture ou collage pas encore enregistrés).
    pub path: Option<PathBuf>,
    /// Dimensions de l'image en pixels, si elles sont connues.
    pub size: Option<(u32, u32)>,
    /// Suit l'échelle commune de la série (sinon : échelle propre).
    pub linked: bool,
    /// Document de l'image. Pour l'entrée active, c'est un document vide :
    /// le vrai est chez l'application.
    doc: Document,
    /// Horodatage de chaque étape annulable du document (même ordre que sa pile).
    undo_marks: Vec<u64>,
    redo_marks: Vec<u64>,
    /// Révision du document déjà prise en compte par [`Series::observe`].
    seen: u64,
}

impl Entry {
    /// Entrée telle que relue d'un projet.
    pub fn new(path: Option<PathBuf>, size: Option<(u32, u32)>, linked: bool, doc: Document) -> Self {
        let seen = doc.revision();
        Self { id: 0, path, size, linked, doc, undo_marks: vec![], redo_marks: vec![], seen }
    }

    /// Identifiant stable de l'entrée (ne change pas quand on réordonne).
    pub fn id(&self) -> u64 {
        self.id
    }
}

/// État d'échelle de toute la série, pour annuler une propagation.
#[derive(Clone, Debug, PartialEq)]
struct ScaleState {
    shared_scale: bool,
    shared: Calibration,
    /// (identifiant, liée, étalonnage) de chaque entrée.
    entries: Vec<(u64, bool, Calibration)>,
}

/// Opération de série **à appliquer** ; l'appliquer rend son inverse.
#[derive(Clone, Debug)]
enum Op {
    /// Remet ces entrées à ces positions (ordre croissant) et affiche la première.
    Insert(Vec<(usize, Entry)>),
    /// Retire ces entrées.
    Remove(Vec<u64>),
    Move { from: usize, to: usize },
    /// Restaure l'état d'échelle. Le couple (entrée, horodatage) désigne
    /// l'étape du document où l'échelle a été saisie : elle est annulée ou
    /// rétablie du même geste si elle est au sommet de sa pile.
    Scale(ScaleState, Option<(u64, u64)>),
}

/// Ce qu'a fait [`Series::undo`] ou [`Series::redo`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Undone {
    Nothing,
    /// Une étape du document de l'image affichée.
    Document,
    /// Une opération de série (l'image affichée a pu changer).
    Series,
}

/// La série : entrées, entrée active, échelle commune, historique.
#[derive(Clone, Debug)]
pub struct Series {
    entries: Vec<Entry>,
    active: usize,
    shared_scale: bool,
    /// Échelle commune (celle des entrées liées).
    shared: Calibration,
    next_id: u64,
    /// Horloge logique : ordonne les étapes des documents et les opérations de série.
    clock: u64,
    undo: Vec<(u64, Op)>,
    redo: Vec<(u64, Op)>,
    /// Incrémenté à chaque opération de série (pour « modifié non enregistré »).
    revision: u64,
}

impl Default for Series {
    fn default() -> Self {
        Self {
            entries: vec![],
            active: 0,
            shared_scale: true,
            shared: Calibration::default(),
            next_id: 0,
            clock: 0,
            undo: vec![],
            redo: vec![],
            revision: 0,
        }
    }
}

impl Series {
    pub fn new() -> Self {
        Self::default()
    }

    /// Série relue d'un projet. `doc` reçoit le document de l'entrée active.
    pub fn from_entries(mut entries: Vec<Entry>, active: usize, shared_scale: bool, doc: &mut Document) -> Self {
        let mut s = Self { shared_scale, ..Self::default() };
        for e in &mut entries {
            s.next_id += 1;
            e.id = s.next_id;
        }
        s.entries = entries;
        s.active = active.min(s.entries.len().saturating_sub(1));
        // L'échelle commune est celle de la première entrée liée.
        if let Some(e) = s.entries.iter().find(|e| e.linked) {
            s.shared = e.doc.calibration.clone();
        }
        *doc = Document::new();
        s.checkout(doc);
        s
    }

    // ---------- lecture ----------

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn active(&self) -> usize {
        self.active
    }
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
    pub fn active_entry(&self) -> Option<&Entry> {
        self.entries.get(self.active)
    }
    /// Accès à l'entrée active (chemin, dimensions), tenue à jour par l'application.
    pub fn active_entry_mut(&mut self) -> Option<&mut Entry> {
        self.entries.get_mut(self.active)
    }
    pub fn entry_mut(&mut self, index: usize) -> Option<&mut Entry> {
        self.entries.get_mut(index)
    }
    pub fn index_of(&self, id: u64) -> Option<usize> {
        self.entries.iter().position(|e| e.id == id)
    }
    pub fn shared_scale(&self) -> bool {
        self.shared_scale
    }

    /// Les entrées avec leur document ; `doc` est celui de l'entrée active.
    pub fn docs<'a>(&'a self, doc: &'a Document) -> impl Iterator<Item = (&'a Entry, &'a Document)> {
        self.entries.iter().enumerate().map(move |(i, e)| (e, if i == self.active { doc } else { &e.doc }))
    }

    /// Compteur qui change à chaque modification de la série ou d'un de ses
    /// documents : à comparer à sa valeur au dernier enregistrement.
    pub fn revision(&self, doc: &Document) -> u64 {
        self.revision + self.docs(doc).map(|(_, d)| d.revision()).sum::<u64>()
    }

    // ---------- document actif sorti / rangé ----------

    /// Range le document actif dans sa case (qui contenait un document vide).
    fn park(&mut self, doc: &mut Document) {
        match self.entries.get_mut(self.active) {
            Some(e) => std::mem::swap(doc, &mut e.doc),
            None => *doc = Document::new(),
        }
    }

    /// Sort le document de l'entrée active.
    fn checkout(&mut self, doc: &mut Document) {
        match self.entries.get_mut(self.active) {
            Some(e) => std::mem::swap(doc, &mut e.doc),
            None => *doc = Document::new(),
        }
    }

    /// Exécute `f` avec tous les documents rangés dans la série, puis ressort
    /// celui de l'entrée active (qui a pu changer).
    fn with_all<R>(&mut self, doc: &mut Document, f: impl FnOnce(&mut Self) -> R) -> R {
        self.park(doc);
        let r = f(self);
        self.active = self.active.min(self.entries.len().saturating_sub(1));
        self.checkout(doc);
        r
    }

    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    /// Enregistre l'inverse d'une opération qui vient d'être faite.
    fn record(&mut self, inverse: Op) {
        let t = self.tick();
        self.undo.push((t, inverse));
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    // ---------- navigation ----------

    /// Affiche une autre image (ce n'est pas une modification : rien à annuler).
    pub fn activate(&mut self, index: usize, doc: &mut Document) -> bool {
        if index >= self.entries.len() || index == self.active {
            return false;
        }
        self.observe(doc);
        self.park(doc);
        self.active = index;
        self.checkout(doc);
        true
    }

    // ---------- opérations de série (annulables) ----------

    /// Ajoute des images à la fin et affiche la première ajoutée.
    ///
    /// Une image de mêmes dimensions que l'image affichée (ou que la première
    /// ajoutée si la série est vide) suit l'échelle commune ; sinon elle
    /// garde une échelle propre.
    pub fn add(&mut self, images: Vec<NewImage>, doc: &mut Document) -> Vec<u64> {
        if images.is_empty() {
            return vec![];
        }
        self.observe(doc);
        let was_empty = self.entries.is_empty();
        let reference = self.active_entry().map_or(images[0].1, |e| e.size);
        let ids = self.with_all(doc, |s| {
            let first = s.entries.len();
            let mut ids = vec![];
            for (path, size) in images {
                let linked = size.is_none() || reference.is_none() || size == reference;
                let mut d = Document::new();
                if linked && s.shared_scale {
                    d.calibration = s.shared.clone();
                }
                s.next_id += 1;
                ids.push(s.next_id);
                s.entries.push(Entry { id: s.next_id, ..Entry::new(path, size, linked, d) });
            }
            s.active = first;
            ids
        });
        self.revision += 1;
        // Ouvrir la première image n'est pas une opération à annuler.
        if !was_empty {
            self.record(Op::Remove(ids.clone()));
        }
        ids
    }

    /// Retire une image de la série (ses mesures reviennent avec Ctrl+Z).
    pub fn remove(&mut self, index: usize, doc: &mut Document) -> bool {
        let Some(id) = self.entries.get(index).map(|e| e.id) else { return false };
        self.observe(doc);
        let inverse = self.with_all(doc, |s| s.apply(Op::Remove(vec![id]), true));
        self.record(inverse);
        true
    }

    /// Déplace une image dans la série ; l'image affichée reste la même.
    pub fn move_entry(&mut self, from: usize, to: usize, doc: &mut Document) -> bool {
        if from == to || from >= self.entries.len() || to >= self.entries.len() {
            return false;
        }
        self.observe(doc);
        let inverse = self.with_all(doc, |s| s.apply(Op::Move { from, to }, true));
        self.record(inverse);
        true
    }

    /// Active ou coupe « même échelle pour toute la série ». À l'activation,
    /// l'échelle de l'image affichée devient celle de toutes les images liées.
    pub fn set_shared_scale(&mut self, on: bool, doc: &mut Document) -> bool {
        if on == self.shared_scale {
            return false;
        }
        self.observe(doc);
        let before = self.scale_state(doc);
        self.shared_scale = on;
        if on {
            self.shared = doc.calibration.clone();
            if let Some(e) = self.entries.get_mut(self.active) {
                e.linked = true;
            }
            self.spread(doc);
        }
        self.revision += 1;
        self.record(Op::Scale(before, None));
        true
    }

    /// Lie une image à l'échelle commune (elle la reçoit) ou l'en détache
    /// (elle garde son échelle actuelle et pourra en changer seule).
    pub fn set_linked(&mut self, index: usize, linked: bool, doc: &mut Document) -> bool {
        if self.entries.get(index).is_none_or(|e| e.linked == linked) {
            return false;
        }
        self.observe(doc);
        let before = self.scale_state(doc);
        self.entries[index].linked = linked;
        self.spread(doc);
        self.revision += 1;
        self.record(Op::Scale(before, None));
        true
    }

    /// Copie l'échelle commune dans tous les documents liés.
    fn spread(&mut self, doc: &mut Document) {
        if !self.shared_scale {
            return;
        }
        for (i, e) in self.entries.iter_mut().enumerate() {
            if e.linked {
                let d = if i == self.active { &mut *doc } else { &mut e.doc };
                d.calibration = self.shared.clone();
            }
        }
    }

    fn scale_state(&self, doc: &Document) -> ScaleState {
        ScaleState {
            shared_scale: self.shared_scale,
            shared: self.shared.clone(),
            entries: self.docs(doc).map(|(e, d)| (e.id, e.linked, d.calibration.clone())).collect(),
        }
    }

    /// Applique une opération (tous les documents étant rangés) et rend son
    /// inverse. `undoing` : sens de l'étape de document accompagnant une échelle.
    fn apply(&mut self, op: Op, undoing: bool) -> Op {
        let active_id = self.entries.get(self.active).map(|e| e.id);
        let mut show = None;
        let inverse = match op {
            Op::Insert(items) => {
                let ids: Vec<u64> = items.iter().map(|(_, e)| e.id).collect();
                show = ids.first().copied();
                for (i, e) in items {
                    self.entries.insert(i.min(self.entries.len()), e);
                }
                Op::Remove(ids)
            }
            Op::Remove(ids) => {
                let (mut items, mut kept) = (vec![], vec![]);
                for (i, e) in std::mem::take(&mut self.entries).into_iter().enumerate() {
                    if ids.contains(&e.id) {
                        items.push((i, e));
                    } else {
                        kept.push(e);
                    }
                }
                self.entries = kept;
                // Les révisions des documents retirés restent comptées.
                self.revision += items.iter().map(|(_, e)| e.doc.revision()).sum::<u64>();
                Op::Insert(items)
            }
            Op::Move { from, to } => {
                let e = self.entries.remove(from);
                self.entries.insert(to, e);
                Op::Move { from: to, to: from }
            }
            Op::Scale(state, companion) => {
                let inverse = ScaleState {
                    shared_scale: self.shared_scale,
                    shared: self.shared.clone(),
                    entries: self.entries.iter().map(|e| (e.id, e.linked, e.doc.calibration.clone())).collect(),
                };
                if let Some((id, mark)) = companion
                    && let Some(e) = self.entries.iter_mut().find(|e| e.id == id)
                {
                    let (from, to) = if undoing { (&mut e.undo_marks, &mut e.redo_marks) } else { (&mut e.redo_marks, &mut e.undo_marks) };
                    if from.last() == Some(&mark) && (if undoing { e.doc.undo() } else { e.doc.redo() }) {
                        from.pop();
                        to.push(mark);
                        e.seen = e.doc.revision();
                    }
                }
                self.shared_scale = state.shared_scale;
                self.shared = state.shared;
                for (id, linked, calibration) in state.entries {
                    if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
                        e.linked = linked;
                        e.doc.calibration = calibration;
                    }
                }
                Op::Scale(inverse, companion)
            }
        };
        // L'image affichée reste la même, sauf si elle vient d'être retirée
        // (on affiche sa voisine) ou si des images viennent d'être remises.
        if let Some(i) = show.or(active_id).and_then(|id| self.index_of(id)) {
            self.active = i;
        }
        self.revision += 1;
        inverse
    }

    // ---------- suivi du document actif ----------

    /// À appeler à chaque image d'interface avec le document actif : date ses
    /// nouvelles étapes annulables et propage une échelle qui vient d'y être
    /// définie si l'image suit l'échelle commune.
    pub fn observe(&mut self, doc: &mut Document) {
        let active = self.active;
        let single = self.entries.len() == 1;
        let Some(e) = self.entries.get_mut(active) else { return };
        let rev = doc.revision();
        let mut mark = None;
        if rev != e.seen {
            let steps = rev.saturating_sub(e.seen).clamp(1, UNDO_LIMIT as u64);
            for _ in 0..steps {
                self.clock += 1;
                e.undo_marks.push(self.clock);
            }
            let extra = e.undo_marks.len().saturating_sub(UNDO_LIMIT);
            e.undo_marks.drain(..extra);
            e.redo_marks.clear();
            e.seen = rev;
            mark = (steps == 1).then_some((e.id, self.clock));
        }
        if !self.shared_scale || !e.linked || doc.calibration == self.shared {
            return;
        }
        if single {
            // Une seule image : son échelle est celle de la série, sans opération à part.
            self.shared = doc.calibration.clone();
            return;
        }
        // Avant : l'image active portait encore l'échelle commune.
        let mut before = self.scale_state(doc);
        before.entries[active].2 = self.shared.clone();
        self.shared = doc.calibration.clone();
        self.spread(doc);
        self.revision += 1;
        self.record(Op::Scale(before, mark));
    }

    // ---------- annuler / rétablir ----------

    pub fn can_undo(&self, doc: &Document) -> bool {
        doc.can_undo() || !self.undo.is_empty()
    }
    pub fn can_redo(&self, doc: &Document) -> bool {
        doc.can_redo() || !self.redo.is_empty()
    }

    /// Annule la plus récente entre la dernière modification de l'image
    /// affichée et la dernière opération de série.
    pub fn undo(&mut self, doc: &mut Document) -> Undone {
        self.observe(doc);
        let series = self.undo.last().map(|(t, _)| *t);
        let own = doc.can_undo().then(|| self.active_entry().and_then(|e| e.undo_marks.last().copied()).unwrap_or(0));
        match (series, own) {
            (None, None) => Undone::Nothing,
            (Some(s), own) if own.is_none_or(|d| s > d) => {
                let (t, op) = self.undo.pop().unwrap();
                let inverse = self.with_all(doc, |s| s.apply(op, true));
                self.redo.push((t, inverse));
                Undone::Series
            }
            _ => {
                doc.undo();
                self.after_step(doc, true);
                Undone::Document
            }
        }
    }

    /// Rétablit, dans l'ordre d'origine, la plus ancienne étape annulée entre
    /// celles de l'image affichée et celles de la série.
    pub fn redo(&mut self, doc: &mut Document) -> Undone {
        self.observe(doc);
        let series = self.redo.last().map(|(t, _)| *t);
        let own = doc.can_redo().then(|| self.active_entry().and_then(|e| e.redo_marks.last().copied()).unwrap_or(u64::MAX));
        // L'étape de document qui accompagne une échelle se rétablit avec elle.
        let companion = match (self.redo.last(), self.active_entry()) {
            (Some((_, Op::Scale(_, Some((id, mark))))), Some(e)) => *id == e.id && Some(*mark) == own,
            _ => false,
        };
        match (series, own) {
            (None, None) => Undone::Nothing,
            (Some(s), own) if companion || own.is_none_or(|d| s < d) => {
                let (t, op) = self.redo.pop().unwrap();
                let inverse = self.with_all(doc, |s| s.apply(op, false));
                self.undo.push((t, inverse));
                Undone::Series
            }
            _ => {
                doc.redo();
                self.after_step(doc, false);
                Undone::Document
            }
        }
    }

    /// Après une étape annulée ou rétablie dans le document actif.
    fn after_step(&mut self, doc: &mut Document, undoing: bool) {
        let shared = self.shared_scale.then(|| self.shared.clone());
        let single = self.entries.len() == 1;
        let Some(e) = self.entries.get_mut(self.active) else { return };
        let (from, to) = if undoing { (&mut e.undo_marks, &mut e.redo_marks) } else { (&mut e.redo_marks, &mut e.undo_marks) };
        let mark = from.pop().unwrap_or(0);
        to.push(mark);
        e.seen = doc.revision();
        // L'échelle commune appartient à la série : l'historique d'un
        // document lié ne la change pas.
        if e.linked && !single && let Some(shared) = shared {
            doc.calibration = shared;
        }
    }
}

/// Statistiques par type sur plusieurs documents, comme
/// [`Document::summary`] : (type, grandeur, unité, stats). Les valeurs
/// d'unités différentes ne sont pas mélangées (une ligne par unité).
pub fn series_summary<'a>(docs: impl IntoIterator<Item = &'a Document>) -> Vec<(ShapeKind, Quantity, String, Stats)> {
    let mut groups: BTreeMap<(ShapeKind, String), (Quantity, Vec<f64>)> = BTreeMap::new();
    for doc in docs {
        for m in &doc.measurements {
            if let Some(v) = m.shape.values(&doc.calibration).into_iter().next() {
                groups.entry((m.shape.kind(), v.unit.clone())).or_insert_with(|| (v.quantity, vec![])).1.push(v.value);
            }
        }
    }
    groups.into_iter().filter_map(|((k, u), (q, vals))| Stats::of(&vals).map(|s| (k, q, u, s))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Pt;
    use crate::measure::Shape;

    fn line(len: f64) -> Shape {
        Shape::Length { a: Pt::new(0., 0.), b: Pt::new(len, 0.) }
    }

    fn img(name: &str) -> NewImage {
        (Some(PathBuf::from(name)), Some((4000, 3000)))
    }

    /// Série de trois images de mêmes dimensions, la première affichée.
    fn three() -> (Series, Document) {
        let mut doc = Document::new();
        let mut s = Series::new();
        s.add(vec![img("a.jpg"), img("b.jpg"), img("c.jpg")], &mut doc);
        (s, doc)
    }

    fn names(s: &Series) -> Vec<String> {
        s.entries().iter().map(|e| e.path.as_ref().unwrap().to_string_lossy().into_owned()).collect()
    }

    /// Définit l'échelle comme le fait le dialogue (avec `checkpoint`).
    fn calibrate(doc: &mut Document, real: f64) {
        doc.checkpoint();
        doc.calibration.set_from_reference(100.0, real, "mm");
    }

    fn scales(s: &Series, doc: &Document) -> Vec<Option<f64>> {
        s.docs(doc).map(|(_, d)| d.calibration.scale.as_ref().map(|c| c.units_per_px)).collect()
    }

    #[test]
    fn un_document_par_image() {
        let (mut s, mut doc) = three();
        assert_eq!((s.len(), s.active()), (3, 0));
        doc.add(line(10.0));
        assert!(s.activate(1, &mut doc));
        assert!(doc.measurements.is_empty());
        doc.add(line(20.0));
        doc.add(line(30.0));
        assert!(s.activate(0, &mut doc) && !s.activate(0, &mut doc) && !s.activate(9, &mut doc));
        assert_eq!(doc.measurements.len(), 1);
        let counts: Vec<usize> = s.docs(&doc).map(|(_, d)| d.measurements.len()).collect();
        assert_eq!(counts, [1, 2, 0]);
    }

    #[test]
    fn annuler_ne_touche_que_l_image_affichee() {
        let (mut s, mut doc) = three();
        doc.add(line(10.0));
        s.activate(1, &mut doc);
        doc.add(line(20.0));
        doc.add(line(30.0));
        s.activate(0, &mut doc);
        // Sur la première image : sa seule mesure, puis l'ajout des images.
        assert_eq!(s.undo(&mut doc), Undone::Document);
        assert!(doc.measurements.is_empty());
        s.activate(1, &mut doc);
        assert_eq!(doc.measurements.len(), 2, "les mesures de l'autre image sont intactes");
        assert_eq!(s.undo(&mut doc), Undone::Document);
        assert_eq!(doc.measurements.len(), 1);
        assert_eq!(s.redo(&mut doc), Undone::Document);
        assert_eq!(doc.measurements.len(), 2);
        s.activate(0, &mut doc);
        assert_eq!(s.redo(&mut doc), Undone::Document);
        assert_eq!(doc.measurements.len(), 1);
    }

    #[test]
    fn annuler_ajout_et_retrait() {
        let mut doc = Document::new();
        let mut s = Series::new();
        s.add(vec![img("a.jpg")], &mut doc);
        doc.add(line(10.0));
        s.add(vec![img("b.jpg"), img("c.jpg")], &mut doc);
        assert_eq!((s.len(), s.active()), (3, 1), "la première image ajoutée est affichée");
        doc.add(line(20.0));
        // Sur b : la mesure de b, puis l'ajout de b et c ; on retombe sur a.
        assert_eq!(s.undo(&mut doc), Undone::Document);
        assert_eq!(s.undo(&mut doc), Undone::Series);
        assert_eq!((names(&s), s.active(), doc.measurements.len()), (vec!["a.jpg".to_owned()], 0, 1));
        // Rétablir remet les images, puis la mesure de b.
        assert_eq!(s.redo(&mut doc), Undone::Series);
        assert_eq!((s.len(), s.active(), doc.measurements.len()), (3, 1, 0));
        assert_eq!(s.redo(&mut doc), Undone::Document);
        assert_eq!(doc.measurements.len(), 1);

        // Retrait de l'image affichée : sa voisine la remplace ; Ctrl+Z la
        // remet à sa place, affichée, avec ses mesures et sa propre pile.
        assert!(s.remove(1, &mut doc));
        assert_eq!((names(&s), s.active()), (vec!["a.jpg".to_owned(), "c.jpg".to_owned()], 1));
        assert!(doc.measurements.is_empty());
        assert_eq!(s.undo(&mut doc), Undone::Series);
        assert_eq!((names(&s)[1].as_str(), s.active(), doc.measurements.len()), ("b.jpg", 1, 1));
        assert_eq!(s.undo(&mut doc), Undone::Document);
        assert!(doc.measurements.is_empty());
    }

    #[test]
    fn annuler_deplacement() {
        let (mut s, mut doc) = three();
        doc.add(line(10.0));
        assert!(s.move_entry(0, 2, &mut doc) && !s.move_entry(1, 1, &mut doc) && !s.move_entry(0, 3, &mut doc));
        assert_eq!((names(&s), s.active()), (vec!["b.jpg".to_owned(), "c.jpg".to_owned(), "a.jpg".to_owned()], 2));
        assert_eq!(doc.measurements.len(), 1, "l'image affichée suit son déplacement");
        assert_eq!(s.undo(&mut doc), Undone::Series);
        assert_eq!((names(&s)[0].as_str(), s.active(), doc.measurements.len()), ("a.jpg", 0, 1));
        assert_eq!(s.redo(&mut doc), Undone::Series);
        assert_eq!((names(&s)[2].as_str(), s.active()), ("a.jpg", 2));
    }

    #[test]
    fn echelle_propagee_et_annulee_d_un_geste() {
        let (mut s, mut doc) = three();
        calibrate(&mut doc, 10.0);
        s.observe(&mut doc);
        assert_eq!(scales(&s, &doc), [Some(0.1); 3]);
        // Une seule annulation retire l'échelle partout, sans étape morte.
        assert_eq!(s.undo(&mut doc), Undone::Series);
        assert_eq!(scales(&s, &doc), [None; 3]);
        assert!(!doc.can_undo() && !s.can_undo(&doc), "l'étape du document est partie avec l'opération");
        assert_eq!(s.redo(&mut doc), Undone::Series);
        assert_eq!(scales(&s, &doc), [Some(0.1); 3]);

        // Modifiée depuis une autre image : toutes suivent.
        s.activate(2, &mut doc);
        calibrate(&mut doc, 20.0);
        s.observe(&mut doc);
        assert_eq!(scales(&s, &doc), [Some(0.2); 3]);
        // Annulée depuis une troisième image : l'échelle revient partout.
        s.activate(1, &mut doc);
        assert_eq!(s.undo(&mut doc), Undone::Series);
        assert_eq!(scales(&s, &doc), [Some(0.1); 3]);
    }

    #[test]
    fn annuler_une_mesure_ne_ressuscite_pas_une_ancienne_echelle() {
        let (mut s, mut doc) = three();
        calibrate(&mut doc, 10.0);
        s.observe(&mut doc);
        doc.add(line(50.0));
        s.activate(1, &mut doc);
        calibrate(&mut doc, 20.0);
        s.observe(&mut doc);
        s.activate(0, &mut doc);
        // Depuis la première image : l'échelle (plus récente) part d'abord,
        // puis la mesure posée sous l'ancienne échelle.
        assert_eq!(s.undo(&mut doc), Undone::Series);
        assert_eq!(scales(&s, &doc), [Some(0.1); 3]);
        assert_eq!(s.undo(&mut doc), Undone::Document);
        assert!(doc.measurements.is_empty());
        // Entre-temps, une autre échelle est définie ailleurs.
        s.activate(1, &mut doc);
        calibrate(&mut doc, 30.0);
        s.observe(&mut doc);
        s.activate(0, &mut doc);
        // Rétablir la mesure ne ramène pas l'échelle qu'elle avait connue.
        assert_eq!(s.redo(&mut doc), Undone::Document);
        assert_eq!(doc.measurements.len(), 1);
        assert_eq!(scales(&s, &doc), [Some(0.3); 3]);
    }

    #[test]
    fn image_detachee_et_dimensions_differentes() {
        let (mut s, mut doc) = three();
        calibrate(&mut doc, 10.0);
        s.observe(&mut doc);
        // Autres dimensions : échelle propre par défaut.
        s.add(vec![(Some(PathBuf::from("d.jpg")), Some((800, 600)))], &mut doc);
        assert!(!s.entries()[3].linked && doc.calibration.scale.is_none());
        calibrate(&mut doc, 50.0);
        s.observe(&mut doc);
        assert_eq!(scales(&s, &doc), [Some(0.1), Some(0.1), Some(0.1), Some(0.5)]);
        // On détache b : elle ne suit plus.
        assert!(s.set_linked(1, false, &mut doc) && !s.set_linked(1, false, &mut doc));
        s.activate(0, &mut doc);
        calibrate(&mut doc, 30.0);
        s.observe(&mut doc);
        assert_eq!(scales(&s, &doc), [Some(0.3), Some(0.1), Some(0.3), Some(0.5)]);
        // On la rattache : elle reçoit l'échelle commune ; annulable.
        assert!(s.set_linked(1, true, &mut doc));
        assert_eq!(scales(&s, &doc)[1], Some(0.3));
        assert_eq!(s.undo(&mut doc), Undone::Series);
        assert_eq!((scales(&s, &doc)[1], s.entries()[1].linked), (Some(0.1), false));

        // Option coupée : plus de propagation ; rétablie : l'image affichée fait foi.
        assert!(s.set_shared_scale(false, &mut doc));
        calibrate(&mut doc, 70.0);
        s.observe(&mut doc);
        assert_eq!(scales(&s, &doc), [Some(0.7), Some(0.1), Some(0.3), Some(0.5)]);
        assert!(s.set_shared_scale(true, &mut doc));
        assert_eq!(scales(&s, &doc), [Some(0.7), Some(0.1), Some(0.7), Some(0.5)]);
        assert_eq!(s.undo(&mut doc), Undone::Series);
        assert_eq!(scales(&s, &doc), [Some(0.7), Some(0.1), Some(0.3), Some(0.5)]);
    }

    #[test]
    fn image_seule_comme_avant() {
        let mut doc = Document::new();
        let mut s = Series::new();
        s.add(vec![img("a.jpg")], &mut doc);
        calibrate(&mut doc, 10.0);
        s.observe(&mut doc);
        doc.add(line(10.0));
        // Deux étapes de document ; ouvrir l'image ne s'annule pas.
        assert_eq!(s.undo(&mut doc), Undone::Document);
        assert_eq!(s.undo(&mut doc), Undone::Document);
        assert!(doc.calibration.scale.is_none());
        assert_eq!((s.undo(&mut doc), s.len()), (Undone::Nothing, 1));
        assert_eq!(s.redo(&mut doc), Undone::Document);
        assert!(doc.calibration.scale.is_some());
        // Les images ajoutées ensuite héritent de l'échelle.
        s.observe(&mut doc);
        s.add(vec![img("b.jpg")], &mut doc);
        assert_eq!(scales(&s, &doc), [Some(0.1); 2]);
    }

    #[test]
    fn revision_pour_l_indicateur_de_modification() {
        let (mut s, mut doc) = three();
        let mut last = s.revision(&doc);
        let mut changed = |s: &Series, doc: &Document| {
            let r = s.revision(doc);
            let c = r != last;
            last = r;
            c
        };
        s.activate(1, &mut doc);
        assert!(!changed(&s, &doc), "changer d'image ne modifie rien");
        doc.add(line(1.0));
        doc.add(line(2.0));
        assert!(changed(&s, &doc));
        // Retirer une image modifiée puis mesurer ailleurs ne retombe jamais
        // sur une valeur déjà vue.
        let saved = s.revision(&doc);
        s.remove(1, &mut doc);
        assert!(s.revision(&doc) > saved);
        s.move_entry(0, 1, &mut doc);
        assert!(changed(&s, &doc));
        s.undo(&mut doc);
        assert!(changed(&s, &doc));
        s.set_linked(0, false, &mut doc);
        assert!(changed(&s, &doc));
    }

    #[test]
    fn statistiques_de_la_serie() {
        let (mut s, mut doc) = three();
        calibrate(&mut doc, 10.0);
        s.observe(&mut doc);
        doc.add(line(50.0));
        s.activate(1, &mut doc);
        doc.add(line(100.0));
        s.activate(2, &mut doc);
        doc.add(line(150.0));
        let sum = series_summary(s.docs(&doc).map(|(_, d)| d));
        assert_eq!(sum.len(), 1);
        assert_eq!((sum[0].3.n, sum[0].3.mean, sum[0].2.as_str()), (3, 10.0, "mm"));
        // Une image à échelle propre, en pixels : ligne séparée.
        s.set_linked(2, false, &mut doc);
        doc.checkpoint();
        doc.calibration = Calibration::default();
        let sum = series_summary(s.docs(&doc).map(|(_, d)| d));
        assert_eq!(sum.iter().map(|x| (x.2.as_str(), x.3.n)).collect::<Vec<_>>(), [("mm", 2), ("px", 1)]);
    }

    #[test]
    fn serie_relue() {
        let mut a = Document::new();
        a.calibration.set_from_reference(100.0, 10.0, "mm");
        a.add(line(1.0));
        let mut b = Document::new();
        b.add(line(2.0));
        b.add(line(3.0));
        let entries = vec![Entry::new(None, None, true, a), Entry::new(None, None, false, b)];
        let mut doc = Document::new();
        let mut s = Series::from_entries(entries, 1, true, &mut doc);
        assert_eq!((s.active(), doc.measurements.len()), (1, 2));
        assert_ne!(s.entries()[0].id(), s.entries()[1].id());
        // Rien à annuler juste après l'ouverture.
        assert_eq!(s.undo(&mut doc), Undone::Document, "les étapes du document relu restent les siennes");
        s.activate(0, &mut doc);
        s.add(vec![img("c.jpg")], &mut doc);
        assert_eq!(doc.calibration.scale.as_ref().unwrap().units_per_px, 0.1);
    }
}

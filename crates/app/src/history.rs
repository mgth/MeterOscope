//! Annulation du remplacement de tout le travail (projet ouvert, nouveau
//! travail).
//!
//! Ouvrir, coller, déposer ou capturer une image l'ajoute à la série, ce que
//! l'historique de la série sait annuler. Seuls un projet ouvert et « Nouveau
//! travail » remplacent tout : l'espace de travail précédent (images,
//! mesures, chemins, réglages) est alors gardé ici. Une fois l'historique du
//! nouveau travail épuisé, Ctrl+Z le rétablit, et Ctrl+Y revient au
//! remplaçant tant que le travail rétabli n'a pas changé.

use image::RgbaImage;
use meteroscope_core::Document;
use std::cell::Cell;
use std::path::PathBuf;

/// Nombre de travaux remplacés gardés en mémoire.
const LIMIT: usize = 4;

/// Tout ce qui accompagne une image de travail.
pub struct Workspace {
    /// `None` : rien d'ouvert (état d'après « Nouveau travail »).
    pub image: Option<std::sync::Arc<RgbaImage>>,
    pub image_path: Option<PathBuf>,
    /// Orientation des pixels de `image`, et celle du fichier `image_path`.
    pub applied: meteroscope_core::Orientation,
    pub file_orientation: meteroscope_core::Orientation,
    pub project_path: Option<PathBuf>,
    pub doc: Document,
    /// Série dont l'image fait partie (autres images, vignettes, état enregistré).
    pub series: crate::app::SeriesState,
    pub brightness: f32,
    pub contrast: f32,
}

/// Empreinte du contenu d'un document (mesures, échelle, rotation), pour
/// savoir s'il a changé depuis qu'il a été rétabli.
pub fn doc_key(doc: &Document) -> String {
    serde_json::to_string(doc).unwrap_or_default()
}

/// Pile d'annulation des remplacements d'image. `K` est l'empreinte du
/// document rétabli, qui conditionne le rétablissement.
pub struct ImageHistory<W, K> {
    undo: Vec<W>,
    redo: Vec<(W, K)>,
    /// Réponse de `can_redo_with` et révision du document pour laquelle elle
    /// vaut, oubliée à chaque changement de la pile.
    redo_cache: Cell<Option<(u64, bool)>>,
}

impl<W, K> Default for ImageHistory<W, K> {
    fn default() -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), redo_cache: Cell::new(None) }
    }
}

impl<W, K: PartialEq> ImageHistory<W, K> {
    /// Comme `can_redo`, pour l'affichage à chaque image : rien n'est calculé
    /// quand la pile est vide, et la réponse est gardée tant que la révision
    /// du document et la pile ne changent pas. Le clic sur Rétablir refait la
    /// vérification complète avec `can_redo`/`redo`.
    pub fn can_redo_with(&self, revision: u64, key: impl FnOnce() -> K) -> bool {
        if self.redo.is_empty() {
            return false;
        }
        if let Some((r, answer)) = self.redo_cache.get()
            && r == revision
        {
            return answer;
        }
        let answer = self.can_redo(&key());
        self.redo_cache.set(Some((revision, answer)));
        answer
    }

    /// Une nouvelle image remplace `old`, qui devient annulable.
    pub fn replaced(&mut self, old: W) {
        self.redo_cache.take();
        self.undo.push(old);
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Rétablissement possible si le document courant n'a pas changé depuis
    /// l'annulation.
    pub fn can_redo(&self, current_key: &K) -> bool {
        self.redo.last().is_some_and(|(_, k)| k == current_key)
    }

    /// Rend l'espace précédent à la place de `current` (rendu tel quel en
    /// `Err` s'il n'y a rien à annuler). `key` donne l'empreinte de l'espace
    /// rétabli.
    pub fn undo(&mut self, current: W, key: impl FnOnce(&W) -> K) -> Result<W, W> {
        let Some(previous) = self.undo.pop() else { return Err(current) };
        self.redo_cache.take();
        self.redo.push((current, key(&previous)));
        Ok(previous)
    }

    /// Rend l'espace remplaçant annulé, ou `current` en `Err` si le document
    /// a changé entre-temps (le rétablissement est alors abandonné).
    pub fn redo(&mut self, current: W, current_key: &K) -> Result<W, W> {
        if !self.can_redo(current_key) {
            self.redo.clear();
            self.redo_cache.take();
            return Err(current);
        }
        let (next, _) = self.redo.pop().expect("vérifié par can_redo");
        self.redo_cache.take();
        self.undo.push(current);
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Espace de travail réduit à son nom et à son empreinte.
    type H = ImageHistory<(&'static str, u32), u32>;
    fn key(w: &(&'static str, u32)) -> u32 {
        w.1
    }

    #[test]
    fn annuler_puis_retablir() {
        let mut h = H::default();
        assert!(!h.can_undo());
        h.replaced(("a", 1));
        h.replaced(("b", 2));
        let cur = ("c", 3);
        let b = h.undo(cur, key).unwrap();
        assert_eq!(b, ("b", 2));
        let a = h.undo(b, key).unwrap();
        assert_eq!(a, ("a", 1));
        assert_eq!(h.undo(a, key), Err(("a", 1)));
        assert!(h.can_redo(&1));
        let b = h.redo(a, &1).unwrap();
        assert_eq!(h.redo(b, &2), Ok(("c", 3)));
        assert!(!h.can_redo(&3));
        assert!(h.can_undo());
    }

    #[test]
    fn document_modifie_abandonne_le_retablissement() {
        let mut h = H::default();
        h.replaced(("a", 1));
        let a = h.undo(("b", 2), key).unwrap();
        // Une mesure ajoutée sur « a » change son empreinte.
        assert!(!h.can_redo(&9));
        assert_eq!(h.redo(("a", 9), &9), Err(("a", 9)));
        assert!(!h.can_redo(&a.1), "le rétablissement est oublié");
    }

    #[test]
    fn nouvelle_image_vide_le_retablissement() {
        let mut h = H::default();
        h.replaced(("a", 1));
        let a = h.undo(("b", 2), key).unwrap();
        h.replaced(a);
        assert!(!h.can_redo(&1));
    }

    #[test]
    fn empreinte_calculee_une_fois_par_revision() {
        let mut h = H::default();
        let calls = std::cell::Cell::new(0);
        let k = || {
            calls.set(calls.get() + 1);
            7
        };
        // Pile vide : aucune empreinte calculée.
        assert!(!h.can_redo_with(1, k));
        assert_eq!(calls.get(), 0);
        h.replaced(("a", 7));
        let _ = h.undo(("b", 2), key).unwrap();
        assert!(h.can_redo_with(1, k));
        assert!(h.can_redo_with(1, k));
        assert_eq!(calls.get(), 1);
        assert!(h.can_redo_with(2, k));
        assert_eq!(calls.get(), 2);
    }

    #[test]
    fn nombre_limite() {
        let mut h = H::default();
        for i in 0..10 {
            h.replaced(("x", i));
        }
        let mut cur = ("courant", 99);
        let mut n = 0;
        while let Ok(prev) = h.undo(cur, key) {
            cur = prev;
            n += 1;
        }
        assert_eq!(n, LIMIT);
        assert_eq!(cur, ("x", 6), "les plus anciennes sont oubliées");
    }

    #[test]
    fn empreinte_ignore_la_selection() {
        let mut doc = Document::new();
        let k = doc_key(&doc);
        doc.selected = Some(3);
        assert_eq!(doc_key(&doc), k);
        doc.rotation = 12.0;
        assert_ne!(doc_key(&doc), k);
    }
}

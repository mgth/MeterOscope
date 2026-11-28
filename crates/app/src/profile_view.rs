//! Profils d'intensité : calcul et cache des résultats, panneau de la courbe.
//!
//! Le modèle (`meteroscope-core`) ne connaît pas les pixels : le résultat
//! d'une mesure par profil (`Shape::Profile::result`) est un cache que ce
//! module tient à jour à partir de l'image d'origine, chaque fois que le
//! segment, les réglages ou l'image changent. L'écriture du cache n'est pas
//! une modification de l'utilisateur : elle ne crée pas d'étape d'annulation.

use meteroscope_core::profile::{Analysis, ProfileParams};
use meteroscope_core::{Document, Pt, Shape};
use meteroscope_io as io;
use std::collections::HashMap;

/// Analyse d'un profil et ce dont elle dépend.
struct Entry {
    key: (Pt, Pt, ProfileParams),
    analysis: Analysis,
}

#[derive(Default)]
pub struct ProfileView {
    cache: HashMap<u64, Entry>,
    /// Réglages donnés aux nouveaux profils : les derniers choisis.
    defaults: ProfileParams,
}

impl ProfileView {
    /// L'image a changé (nouvelle image, quart de tour, miroir) : tout est à recalculer.
    pub fn invalidate(&mut self) {
        self.cache.clear();
    }

    /// Une mesure vient d'être tracée : un profil reprend les derniers réglages
    /// choisis. Seul le tracé les applique ; les réglages d'un profil
    /// enregistré dans un projet ne sont jamais remplacés.
    pub fn created(&mut self, doc: &mut Document, id: u64) {
        if let Some(Shape::Profile { params, .. }) = doc.get_mut(id).map(|m| &mut m.shape) {
            *params = self.defaults.clone();
        }
    }

    /// Met à jour les résultats de tous les profils du document à partir de
    /// l'image d'origine (jamais l'image réglée en luminosité/contraste).
    pub fn sync(&mut self, doc: &mut Document, image: &image::RgbaImage) {
        self.cache.retain(|id, _| doc.get(*id).is_some());
        for m in &mut doc.measurements {
            let Shape::Profile { a, b, params, result } = &mut m.shape else { continue };
            let key = (*a, *b, params.clone());
            let entry = match self.cache.get(&m.id) {
                Some(e) if e.key == key => e,
                _ => {
                    let analysis = io::measure_profile(image, *a, *b, params);
                    self.cache.insert(m.id, Entry { key, analysis });
                    &self.cache[&m.id]
                }
            };
            let fresh = entry.analysis.result();
            if *result != fresh {
                *result = fresh;
            }
        }
    }

    /// Analyse à jour de la mesure sélectionnée, si c'est un profil.
    pub fn selected<'a>(&'a self, doc: &Document) -> Option<(u64, &'a Analysis)> {
        let m = doc.selected().filter(|m| matches!(m.shape, Shape::Profile { .. }))?;
        Some((m.id, &self.cache.get(&m.id)?.analysis))
    }

    /// Panneau de la courbe du profil sélectionné. Renvoie `true` si
    /// l'export CSV de la courbe est demandé.
    pub fn panel(&mut self, ui: &mut egui::Ui, doc: &mut Document) -> bool {
        let Some((id, analysis)) = self.selected(doc) else { return false };
        let out = meteroscope_panels::profile_panel(ui, doc, id, analysis);
        if out.changed
            && let Some(Shape::Profile { params, .. }) = doc.get(id).map(|m| &m.shape)
        {
            // Le choix manuel des bords est propre à une mesure.
            self.defaults = ProfileParams { pick: None, ..params.clone() };
        }
        out.export_csv
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    /// Barre claire verticale de 20 px (bords en x = 30 et x = 50).
    fn bar() -> RgbaImage {
        RgbaImage::from_fn(80, 40, |x, _| if (30..50).contains(&x) { Rgba([200, 200, 200, 255]) } else { Rgba([40, 40, 40, 255]) })
    }

    fn width(doc: &Document, id: u64) -> Option<f64> {
        doc.get(id).unwrap().shape.values(&doc.calibration).first().map(|v| v.value)
    }

    #[test]
    fn resultat_suit_le_segment_l_image_et_l_annulation() {
        meteroscope_i18n::set_language("fr");
        let mut view = ProfileView::default();
        let mut doc = Document::new();
        let mut image = bar();
        let id = doc.add(Shape::profile(Pt::new(10.0, 20.0), Pt::new(70.0, 20.0)));
        assert_eq!(width(&doc, id), None);
        let revision = doc.revision();
        view.sync(&mut doc, &image);
        assert!((width(&doc, id).unwrap() - 20.0).abs() < 0.05);
        // Le cache n'est ni une modification ni une étape d'annulation.
        assert_eq!(doc.revision(), revision);
        assert!(view.selected(&doc).is_some_and(|(i, a)| i == id && a.edges.len() == 2));

        // Segment déplacé hors de la barre : plus de bords, plus de valeur.
        doc.checkpoint();
        doc.get_mut(id).unwrap().shape.translate(-45.0, 0.0);
        view.sync(&mut doc, &image);
        assert_eq!(width(&doc, id), None);
        // Annuler rend le segment et son résultat ; rétablir se recalcule.
        assert!(doc.undo());
        assert!(width(&doc, id).is_some());
        assert!(doc.redo());
        view.sync(&mut doc, &image);
        assert_eq!(width(&doc, id), None);
        assert!(doc.undo());
        view.sync(&mut doc, &image);
        assert!((width(&doc, id).unwrap() - 20.0).abs() < 0.05);

        // Quart de tour : points et pixels tournent ensemble, même largeur.
        image = io::apply_transform(&image, &mut doc, io::Transform::RotateRight);
        view.invalidate();
        view.sync(&mut doc, &image);
        assert!((width(&doc, id).unwrap() - 20.0).abs() < 0.05);
        // Image changée sous un segment inchangé : le résultat suit l'image.
        image = RgbaImage::from_pixel(40, 80, Rgba([90, 90, 90, 255]));
        view.invalidate();
        view.sync(&mut doc, &image);
        assert_eq!(width(&doc, id), None);
    }

    #[test]
    fn nouveaux_profils_reprennent_les_derniers_reglages() {
        meteroscope_i18n::set_language("fr");
        let mut view = ProfileView { defaults: ProfileParams { band: 25.0, ..Default::default() }, ..Default::default() };
        let mut doc = Document::new();
        let band = |doc: &Document, id| match &doc.get(id).unwrap().shape {
            Shape::Profile { params, .. } => params.band,
            _ => panic!(),
        };
        // Profil d'un projet ouvert, réglages standard et aucun bord trouvé
        // (segment hors de la barre) : ses réglages enregistrés sont gardés.
        let saved = doc.add(Shape::profile(Pt::new(2.0, 20.0), Pt::new(22.0, 20.0)));
        view.sync(&mut doc, &bar());
        assert_eq!(width(&doc, saved), None);
        assert_eq!(band(&doc, saved), ProfileParams::default().band);
        // Profil tout juste tracé : les derniers réglages, sans étape
        // d'annulation de plus (annuler retire la mesure, rétablir la rend
        // avec ses réglages).
        let id = doc.add(Shape::profile(Pt::new(10.0, 20.0), Pt::new(70.0, 20.0)));
        view.created(&mut doc, id);
        view.sync(&mut doc, &bar());
        assert_eq!(band(&doc, id), 25.0);
        assert_eq!(band(&doc, saved), ProfileParams::default().band);
        assert!(doc.undo());
        assert!(doc.get(id).is_none());
        assert!(doc.redo());
        assert_eq!(band(&doc, id), 25.0);
        // Mesure supprimée : son analyse est oubliée.
        doc.remove(id);
        doc.remove(saved);
        view.sync(&mut doc, &bar());
        assert!(view.cache.is_empty());
    }
}

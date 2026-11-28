//! Saisie sur place du texte des annotations (outil Texte, texte d'une flèche).
//!
//! Un clic de l'outil Texte ouvre un champ à l'endroit cliqué ; l'annotation
//! n'est ajoutée au document qu'à la validation (Entrée ou clic ailleurs), avec
//! son texte : un seul pas d'annulation. Une flèche tracée ouvre le champ de
//! son texte facultatif à sa base. Double-clic sur une annotation (outil
//! Sélection) : modifier son texte. Échap abandonne la saisie.

use crate::Canvas;
use crate::render::{self, LABEL_FONT};
use crate::view::Xf;
use egui::text::CCursorRange;
use egui::{Align2, Color32, FontId, Frame, Key, Pos2, Rect, TextEdit, Ui, vec2};
use meteroscope_core::{Document, Pt, Shape};
use meteroscope_i18n::tr;

/// Annotation dont on saisit le texte.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// Texte pas encore créé, à poser en ce point.
    New(Pt),
    /// Annotation existante. `fresh` : révision du document juste après sa
    /// création (flèche tracée à l'instant) ; tant qu'elle n'a pas changé, le
    /// texte s'ajoute sans nouveau point d'annulation.
    Existing { id: u64, fresh: Option<u64> },
}

#[derive(Clone, Debug)]
pub struct Editing {
    pub target: Target,
    pub text: String,
    /// Demander le focus (première frame).
    focus: bool,
}

/// État de la saisie, conservé dans la vue.
#[derive(Clone, Debug, Default)]
pub struct Annotations {
    pub editing: Option<Editing>,
    /// Rectangle écran du champ à la frame précédente : les clics qui y
    /// tombent sont pour lui, pas pour la vue.
    pub field: Option<Rect>,
}

impl Annotations {
    /// Identifiant de l'annotation en cours de saisie (son étiquette n'est pas dessinée).
    pub fn editing_id(&self) -> Option<u64> {
        self.editing_id_of(self.editing.as_ref()?)
    }

    fn editing_id_of(&self, e: &Editing) -> Option<u64> {
        match e.target {
            Target::Existing { id, .. } => Some(id),
            Target::New(_) => None,
        }
    }

    /// Le point écran `pos` tombe dans le champ de saisie.
    pub fn covers(&self, pos: Pos2) -> bool {
        self.editing.is_some() && self.field.is_some_and(|r| r.contains(pos))
    }
}

/// Applique le texte saisi au document. Renvoie l'id d'un texte créé.
pub fn commit(doc: &mut Document, target: &Target, text: &str) -> Option<u64> {
    let text = text.trim();
    match *target {
        Target::New(p) => (!text.is_empty()).then(|| doc.add(Shape::Text { p, text: text.to_owned() })),
        Target::Existing { id, fresh } => {
            let m = doc.get(id)?;
            if m.shape.annotation_text() == Some(text) {
                return None;
            }
            if text.is_empty() && matches!(m.shape, Shape::Text { .. }) {
                // Texte vidé : l'annotation n'a plus de raison d'être.
                doc.remove(id);
                return None;
            }
            if fresh != Some(doc.revision()) {
                doc.checkpoint();
            }
            if let Some(t) = doc.get_mut(id).and_then(|m| m.shape.annotation_text_mut()) {
                *t = text.to_owned();
            }
            None
        }
    }
}

impl Canvas {
    /// Ouvre la saisie (valide d'abord une saisie en cours).
    pub(crate) fn begin_edit(&mut self, target: Target, doc: &mut Document) {
        self.finish_edit(doc);
        let text = match &target {
            Target::Existing { id, .. } => {
                doc.get(*id).and_then(|m| m.shape.annotation_text()).unwrap_or_default().to_owned()
            }
            Target::New(_) => String::new(),
        };
        self.annot.editing = Some(Editing { target, text, focus: true });
        self.annot.field = None;
    }

    /// Valide la saisie en cours, s'il y en a une.
    pub(crate) fn finish_edit(&mut self, doc: &mut Document) {
        if let Some(e) = self.annot.editing.take()
            && let Some(id) = commit(doc, &e.target, &e.text)
        {
            self.out.created = Some(id);
        }
    }

    /// Une saisie de texte d'annotation est ouverte.
    pub fn is_editing_text(&self) -> bool {
        self.annot.editing.is_some()
    }

    /// Affiche le champ de saisie à l'emplacement de l'étiquette.
    pub(crate) fn show_editor(&mut self, ui: &mut Ui, view: Rect, xf: Xf, doc: &mut Document) {
        let Some(mut e) = self.annot.editing.take() else {
            self.annot.field = None;
            return;
        };
        let anchor = match &e.target {
            Target::New(p) => Some((xf.to_screen(*p), Align2::CENTER_CENTER)),
            Target::Existing { id, .. } => doc.get(*id).map(|m| render::label_anchor(&m.shape, xf)),
        };
        // Annotation disparue (annulée pendant la saisie) : on abandonne.
        let Some((pos, align)) = anchor else {
            self.annot.field = None;
            return;
        };
        let font = FontId::proportional(LABEL_FONT);
        let arrow = self.annot.editing_id_of(&e).and_then(|id| doc.get(id)).is_some_and(|m| matches!(m.shape, Shape::Arrow { .. }));
        let hint = if arrow { tr!("annot.arrow_hint") } else { tr!("annot.text_hint") };
        let shown = if e.text.is_empty() { hint } else { e.text.as_str() };
        let w = ui.ctx().fonts_mut(|f| f.layout_no_wrap(shown.to_owned(), font.clone(), Color32::WHITE).size().x);
        let size = vec2((w + 28.0).clamp(90.0, view.width().max(90.0)), LABEL_FONT + 14.0);
        let rect = align.anchor_size(pos, size);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        let mut output = Frame::new()
            .fill(Color32::from_black_alpha(230))
            .corner_radius(4.0)
            .show(&mut child, |ui| {
                TextEdit::singleline(&mut e.text)
                    .font(font)
                    .text_color(Color32::WHITE)
                    .hint_text(hint)
                    .desired_width(rect.width() - 10.0)
                    .show(ui)
            })
            .inner;
        let resp = output.response.response.clone();
        // Pas pendant le clic qui ouvre la saisie : egui retirerait aussitôt
        // le focus d'un champ qui n'est pas sous le pointeur.
        if e.focus && !ui.input(|i| i.pointer.any_click() || i.pointer.any_down()) {
            e.focus = false;
            resp.request_focus();
            output.state.cursor.set_char_range(Some(CCursorRange::select_all_str(&e.text)));
            output.state.store(ui.ctx(), resp.id);
        }
        self.annot.field = Some(rect);
        if resp.lost_focus() {
            self.annot.field = None;
            if !ui.input(|i| i.key_pressed(Key::Escape))
                && let Some(id) = commit(doc, &e.target, &e.text)
            {
                self.out.created = Some(id);
            }
        } else {
            self.annot.editing = Some(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saisie_et_annulation() {
        meteroscope_i18n::set_language("fr");
        let mut doc = Document::new();
        // Texte vide : rien n'est créé.
        assert_eq!(commit(&mut doc, &Target::New(Pt::new(5., 5.)), "  "), None);
        assert!(doc.measurements.is_empty() && !doc.can_undo());
        let id = commit(&mut doc, &Target::New(Pt::new(5., 5.)), " Fissure ").unwrap();
        assert_eq!(doc.get(id).unwrap().shape, Shape::Text { p: Pt::new(5., 5.), text: "Fissure".into() });
        // Modification : un pas d'annulation.
        commit(&mut doc, &Target::Existing { id, fresh: None }, "Fissure B");
        assert!(doc.undo());
        assert_eq!(doc.get(id).unwrap().shape.annotation_text(), Some("Fissure"));
        // Texte vidé : l'annotation est supprimée (annulable).
        commit(&mut doc, &Target::Existing { id, fresh: None }, "");
        assert!(doc.get(id).is_none());
        assert!(doc.undo() && doc.get(id).is_some());

        // Flèche fraîchement tracée : son texte rejoint le même pas d'annulation.
        let a = doc.add(Shape::Arrow { a: Pt::new(0., 0.), b: Pt::new(9., 9.), text: String::new() });
        let rev = doc.revision();
        commit(&mut doc, &Target::Existing { id: a, fresh: Some(rev) }, "ici");
        assert_eq!(doc.get(a).unwrap().shape.annotation_text(), Some("ici"));
        assert!(doc.undo() && doc.get(a).is_none());
        // Flèche vidée : elle reste.
        assert!(doc.redo());
        commit(&mut doc, &Target::Existing { id: a, fresh: None }, "");
        assert_eq!(doc.get(a).unwrap().shape.annotation_text(), Some(""));
    }
}

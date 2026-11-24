//! Vue image interactive : zoom/déplacement, dessin et édition des mesures.
//!
//! CONTRAT PUBLIC (utilisé par `crates/app`) — ne pas changer les signatures
//! existantes ; on peut en ajouter.
//!
//! Gestes :
//! - molette / pincement : zoom sous le curseur ; glisser au bouton du milieu
//!   ou droit, Espace + glisser, ou glisser dans le vide (outil Sélection) :
//!   déplacer la vue ;
//! - outils de tracé : clic puis clic, ou presser-glisser-relâcher ; Maj
//!   contraint ; Entrée / double-clic / clic droit terminent un chemin ou une
//!   surface, Retour arrière retire le dernier point ;
//! - outil Sélection : clic pour sélectionner, glisser une poignée ou le corps.

mod draft;
mod edit;
mod render;
mod view;
#[cfg(test)]
mod tests;

use draft::{Draft, Step};
use edit::Hit;
use egui::{
    Color32, CursorIcon, Event, Key, Modifiers, PointerButton, Pos2, Rect, Sense, Stroke, TextureHandle, Ui, Vec2,
    pos2, vec2,
};
use mesura_core::{Calibration, Document, PALETTE, Pt, Shape, ShapeKind, Tool};
use view::{View, Xf};

pub use view::{MAX_ZOOM, MIN_ZOOM};

/// Seuil (pixels écran) au-delà duquel un appui devient un glisser.
const DRAG_THRESHOLD: f32 = 4.0;
/// Tolérance de clic sur un trait (pixels écran).
const HIT_TOL: f32 = 6.0;
/// Tolérance de clic sur une poignée ou un point (pixels écran).
const HANDLE_TOL: f32 = 9.0;
/// Délai et distance maximaux d'un double-clic.
const DOUBLE_CLICK_DELAY: f64 = 0.4;
const DOUBLE_CLICK_DIST: f32 = 6.0;
/// Au-delà de ce zoom, la loupe n'apporte plus rien.
const LOUPE_MAX_ZOOM: f32 = 12.0;
/// Couleur du segment d'étalonnage.
const CALIB_COLOR: Color32 = Color32::from_rgb(255, 64, 170);

/// Ce que la vue signale à l'application après une frame.
#[derive(Clone, Debug, Default)]
pub struct CanvasOutput {
    /// Position du curseur en pixels image, s'il survole l'image.
    pub cursor: Option<Pt>,
    /// L'utilisateur vient de tracer un segment de référence avec l'outil
    /// Échelle : l'application doit ouvrir la boîte de dialogue d'étalonnage.
    pub calibration_segment: Option<(Pt, Pt)>,
    /// Une mesure vient d'être créée (id).
    pub created: Option<u64>,
}

#[derive(Clone, Copy, Debug)]
enum Pending {
    Fit,
    Actual,
}

/// Nature du geste souris en cours.
#[derive(Clone, Copy, Debug, PartialEq)]
enum GestureKind {
    /// Déplacement de la vue.
    Pan,
    /// Appui de l'outil de tracé.
    Draw,
    /// Poignée `idx` de la mesure `id`.
    Handle(u64, usize),
    /// Corps de la mesure `id`.
    Body(u64),
    /// Appui gauche dans le vide (outil Sélection) : clic = désélection, glisser = vue.
    Void,
    /// Appui droit : clic = terminer le tracé, glisser = vue.
    Secondary,
}

#[derive(Clone, Debug)]
struct Gesture {
    button: PointerButton,
    kind: GestureKind,
    start: Pos2,
    last: Pos2,
    dragged: bool,
    /// Position image de l'appui.
    start_img: Pt,
    /// Décalage poignée − pointeur, pour que la poignée ne saute pas.
    grab: (f64, f64),
    /// Forme d'origine (déplacement du corps).
    orig: Option<Shape>,
}

/// État persistant de la vue (zoom, outil, tracé en cours…).
pub struct Canvas {
    tool: Tool,
    pending: Option<Pending>,
    view: View,
    /// La vue est « ajustée » : on réajuste si la zone change de taille.
    fitted: bool,
    last_rect: Rect,
    img_size: Vec2,
    draft: Draft,
    /// Dernier segment d'étalonnage tracé (affiché tant que l'outil Échelle est actif).
    calib_seg: Option<(Pt, Pt)>,
    gesture: Option<Gesture>,
    last_click: Option<(f64, Pos2)>,
    /// Rectangles des étiquettes de la dernière frame (clic sur une étiquette).
    label_rects: Vec<(u64, Rect)>,
    has_selection: bool,
    hover_hit: Option<Hit>,
    shift: bool,
    out: CanvasOutput,
}

impl Default for Canvas {
    fn default() -> Self {
        Self {
            tool: Tool::Select,
            pending: Some(Pending::Fit),
            view: View::default(),
            fitted: true,
            last_rect: Rect::NOTHING,
            img_size: Vec2::ZERO,
            draft: Draft::new(ShapeKind::Length),
            calib_seg: None,
            gesture: None,
            last_click: None,
            label_rects: Vec::new(),
            has_selection: false,
            hover_hit: None,
            shift: false,
            out: CanvasOutput::default(),
        }
    }
}

/// Type de forme tracé par un outil (l'Échelle trace une longueur).
fn draft_kind(tool: Tool) -> Option<ShapeKind> {
    match tool {
        Tool::Select => None,
        Tool::Calibrate => Some(ShapeKind::Length),
        Tool::Draw(k) => Some(k),
    }
}

/// Couleur attribuée par `Document::add` à un type de forme.
fn kind_color(kind: ShapeKind) -> Color32 {
    let i = ShapeKind::ALL.iter().position(|k| *k == kind).unwrap_or(0);
    render::color32(PALETTE[i % PALETTE.len()])
}

impl Canvas {
    pub fn tool(&self) -> Tool {
        self.tool
    }

    /// Change d'outil (annule un tracé en cours).
    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
        self.draft = Draft::new(draft_kind(tool).unwrap_or(ShapeKind::Length));
        if self.gesture.as_ref().is_some_and(|g| g.kind == GestureKind::Draw) {
            self.gesture = None;
        }
        if tool != Tool::Calibrate {
            self.calib_seg = None;
        }
    }

    /// Ajuste l'image à la vue à la prochaine frame.
    pub fn fit(&mut self) {
        self.pending = Some(Pending::Fit);
    }

    fn known(&self) -> bool {
        self.last_rect.is_positive() && self.img_size.x > 0.0 && self.img_size.y > 0.0
    }

    /// Zoom 1:1 centré.
    pub fn zoom_actual(&mut self) {
        if self.known() {
            self.view = View::actual(self.last_rect.size(), self.img_size);
            self.fitted = false;
        } else {
            self.pending = Some(Pending::Actual);
        }
    }

    /// Zoom relatif (×factor) autour du centre de la vue.
    pub fn zoom_by(&mut self, factor: f32) {
        // Avant le premier affichage, la taille de la vue est inconnue : on ignore.
        if !(factor.is_finite() && factor > 0.0 && self.known()) {
            return;
        }
        self.view.zoom_about(self.last_rect.size() / 2.0, factor);
        self.view.clamp(self.last_rect.size(), self.img_size);
        self.fitted = false;
    }

    /// Zoom courant (pixels écran par pixel image).
    pub fn zoom(&self) -> f32 {
        self.view.zoom
    }

    /// Vrai si un tracé est en cours (Échap doit alors l'annuler plutôt que
    /// de revenir à l'outil Sélection).
    pub fn is_drawing(&self) -> bool {
        self.draft.is_active()
    }

    /// Annule le tracé en cours.
    pub fn cancel(&mut self) {
        self.draft.reset();
        if self.gesture.as_ref().is_some_and(|g| g.kind == GestureKind::Draw) {
            self.gesture = None;
        }
    }

    /// Dernier segment d'étalonnage tracé, tant que l'outil Échelle est actif.
    pub fn calibration_segment(&self) -> Option<(Pt, Pt)> {
        self.calib_seg
    }

    /// Aide contextuelle d'une ligne pour la barre d'état, selon l'outil et
    /// l'avancement du tracé (« Cliquez le premier point », …).
    pub fn hint(&self) -> String {
        let n = self.draft.pts.len();
        let s = match self.tool {
            Tool::Select => {
                if matches!(self.gesture.as_ref().map(|g| g.kind), Some(GestureKind::Handle(..))) {
                    "Relâchez pour placer le point — Maj : contraindre"
                } else if self.has_selection {
                    "Glissez une poignée pour modifier, le trait pour déplacer — Suppr : supprimer, Échap : désélectionner"
                } else {
                    "Cliquez une mesure pour la sélectionner — glisser dans le vide : déplacer la vue, molette : zoom"
                }
            }
            Tool::Calibrate => match (n, self.calib_seg.is_some()) {
                (0, false) => "Échelle : cliquez le début d'une longueur connue (ou glissez d'un bout à l'autre)",
                (0, true) => "Indiquez la longueur réelle dans la boîte de dialogue — ou tracez un autre segment",
                _ => "Cliquez la fin de la longueur connue — Maj : horizontale/verticale",
            },
            Tool::Draw(kind) => match (kind, n) {
                (ShapeKind::Length, 0) => "Cliquez le premier point (ou glissez d'un bout à l'autre)",
                (ShapeKind::Length, _) => "Cliquez le deuxième point — Maj : horizontale/verticale/45°",
                (ShapeKind::Rect, 0) => "Cliquez un premier coin (ou glissez en diagonale)",
                (ShapeKind::Rect, _) => "Cliquez le coin opposé — Maj : carré",
                (ShapeKind::Circle, 0) => "Cliquez le centre du cercle (ou glissez du centre vers le bord)",
                (ShapeKind::Circle, _) => "Cliquez un point du bord du cercle",
                (ShapeKind::Circle3, 0) => "Cliquez un premier point sur le bord du cercle",
                (ShapeKind::Circle3, 1) => "Cliquez un deuxième point sur le bord",
                (ShapeKind::Circle3, _) => "Cliquez un troisième point sur le bord",
                (ShapeKind::Angle, 0) => "Cliquez un point sur le premier côté de l'angle",
                (ShapeKind::Angle, 1) => "Cliquez le sommet de l'angle — Maj : horizontale/verticale/45°",
                (ShapeKind::Angle, _) => "Cliquez un point sur le second côté — Maj : horizontale/verticale/45°",
                (ShapeKind::Path, 0) => "Cliquez le premier point du chemin",
                (ShapeKind::Path, 1) => "Cliquez le point suivant — Retour arrière : retirer le dernier point",
                (ShapeKind::Path, _) => {
                    "Cliquez le point suivant — double-clic, Entrée ou clic droit pour terminer, Retour arrière : retirer un point"
                }
                (ShapeKind::Area, 0) => "Cliquez le premier sommet de la surface",
                (ShapeKind::Area, 1 | 2) => "Cliquez le sommet suivant — Retour arrière : retirer le dernier point",
                (ShapeKind::Area, _) => {
                    "Cliquez le sommet suivant — double-clic, Entrée ou clic sur le premier point pour fermer"
                }
                (ShapeKind::Marker, _) => "Cliquez pour poser un marqueur (comptage)",
            },
        };
        s.to_owned()
    }

    fn xf(&self) -> Xf {
        self.view.xf(self.last_rect.min)
    }

    /// Affiche la vue dans tout l'espace disponible de `ui` et gère les
    /// interactions. `image` : texture et taille en pixels de l'image.
    pub fn show(&mut self, ui: &mut Ui, doc: &mut Document, image: Option<(&TextureHandle, [usize; 2])>) -> CanvasOutput {
        self.out = CanvasOutput::default();
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::from_gray(28));
        self.has_selection = doc.selected().is_some();
        let Some((tex, [w, h])) = image else {
            self.gesture = None;
            return std::mem::take(&mut self.out);
        };
        let img = vec2(w as f32, h as f32);

        // Ajustement initial, ou réajustement si la zone change de taille.
        let resized = rect.size() != self.last_rect.size() || img != self.img_size;
        self.last_rect = rect;
        self.img_size = img;
        match self.pending.take() {
            Some(Pending::Fit) => self.fit_now(),
            Some(Pending::Actual) => {
                self.view = View::actual(rect.size(), img);
                self.fitted = false;
            }
            None if resized && self.fitted => self.fit_now(),
            None => {}
        }

        // Entrées de la frame.
        let kb_ok = !ui.ctx().egui_wants_keyboard_input();
        let hovering = resp.contains_pointer();
        let (events, shift, space, time, scroll, pinch, hover) = ui.input(|i| {
            (
                i.events.clone(),
                i.modifiers.shift,
                i.key_down(Key::Space),
                i.time,
                i.smooth_scroll_delta,
                i.zoom_delta(),
                i.pointer.latest_pos(),
            )
        });
        self.shift = shift;
        let space = space && kb_ok && (hovering || self.gesture.is_some());

        // Zoom (molette, pincement) sous le curseur ; défilement horizontal = déplacement.
        if hovering {
            let factor = pinch * (scroll.y * 0.0025).exp();
            if factor != 1.0
                && let Some(hp) = hover
            {
                self.view.zoom_about(hp - rect.min, factor);
                self.fitted = false;
            }
            if scroll.x != 0.0 {
                self.view.pan.x += scroll.x;
                self.fitted = false;
            }
            if scroll != Vec2::ZERO {
                ui.input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
            }
            self.view.clamp(rect.size(), img);
        }

        // Souris, dans l'ordre des événements.
        for ev in &events {
            match ev {
                Event::PointerMoved(pos) => self.pointer_moved(*pos, doc),
                Event::PointerButton { pos, button, pressed: true, .. } => {
                    if self.gesture.is_none() && hovering && rect.contains(*pos) {
                        self.press(*pos, *button, space, doc);
                    }
                }
                Event::PointerButton { pos, button, pressed: false, .. } => self.release(*pos, *button, time, doc),
                _ => {}
            }
        }
        // Relâchement perdu (hors fenêtre…) : on termine le geste.
        if let Some(g) = &self.gesture
            && !ui.input(|i| i.pointer.button_down(g.button))
        {
            let (pos, button) = (g.last, g.button);
            self.release(pos, button, time, doc);
        }

        // Clavier pendant un tracé.
        if self.draft.is_active() && kb_ok {
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter)) {
                let step = self.draft.finish();
                self.apply(step, doc);
            }
            if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Backspace)) {
                self.draft.backspace();
            }
        }

        // Survol.
        let xf = self.xf();
        let hover_in = hover.filter(|p| rect.contains(*p) && hovering);
        self.hover_hit = match (self.tool, &self.gesture, hover_in) {
            (Tool::Select, None, Some(p)) if !space => self.hit(doc, p, xf),
            _ => None,
        };
        if let Some(p) = hover_in {
            let ip = xf.to_image(p);
            if ip.x >= 0.0 && ip.y >= 0.0 && ip.x <= w as f64 && ip.y <= h as f64 {
                self.out.cursor = Some(ip);
            }
        }
        if hover_in.is_some() || self.gesture.is_some() {
            ui.ctx().set_cursor_icon(self.cursor_icon(space));
        }

        // Rendu.
        self.has_selection = doc.selected().is_some();
        let img_rect = Rect::from_min_size(xf.origin, img * xf.zoom);
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        painter.rect_filled(img_rect.expand(1.0), 0.0, Color32::from_gray(10));
        painter.image(tex.id(), img_rect, uv, Color32::WHITE);
        self.paint_measurements(&painter, xf, doc);

        // Aperçu du tracé (le point sous le curseur suit la contrainte Maj).
        let pointer = if self.gesture.is_some() { hover } else { hover_in };
        let draw_tool = draft_kind(self.tool).is_some();
        let preview_pt = pointer.filter(|_| draw_tool).map(|p| self.draft.constrain(xf.to_image(p), shift));
        if self.tool == Tool::Calibrate
            && !self.draft.is_active()
            && let Some((a, b)) = self.calib_seg
        {
            self.paint_draft(&painter, xf, &[a, b], Some(&doc.calibration));
        }
        if draw_tool {
            let pts = self.draft.preview(preview_pt);
            self.paint_draft(&painter, xf, &pts, Some(&doc.calibration));
            if let Some(p) = preview_pt
                && self.draft.would_close(p, xf.img_len(HANDLE_TOL))
            {
                painter.circle_stroke(xf.to_screen(self.draft.pts[0]), 8.0, Stroke::new(2.0, Color32::WHITE));
            }
        }
        render::paint_scale_bar(&painter, rect, xf.zoom, &doc.calibration);

        // Loupe : pendant un tracé ou le glisser d'une poignée.
        if xf.zoom < LOUPE_MAX_ZOOM
            && let Some(cur) = pointer
        {
            let mag = (xf.zoom * 4.0).clamp(2.0, 48.0);
            let panning = space || self.gesture.as_ref().is_some_and(|g| g.dragged && !matches!(g.kind, GestureKind::Handle(..) | GestureKind::Draw));
            if draw_tool && !panning && let Some(p) = preview_pt {
                let pts = self.draft.preview(Some(p));
                render::paint_loupe(&painter, tex.id(), [w, h], p, cur, rect, mag, |lp, lxf| {
                    self.paint_draft(lp, lxf, &pts, None)
                });
            } else if let Some(Gesture { kind: GestureKind::Handle(id, i), dragged: true, .. }) = &self.gesture
                && let Some(m) = doc.get(*id)
            {
                let hp = edit::handles(&m.shape)[*i];
                let color = render::color32(m.color);
                render::paint_loupe(&painter, tex.id(), [w, h], hp, cur, rect, mag, |lp, lxf| {
                    render::paint_shape(lp, lxf, &m.shape, color, 1.5, 0);
                });
            }
        }

        std::mem::take(&mut self.out)
    }

    fn fit_now(&mut self) {
        self.view = View::fit(self.last_rect.size(), self.img_size);
        self.fitted = true;
    }

    /// Élément sous le point écran `pos` : poignées, puis étiquettes, puis traits.
    fn hit(&self, doc: &Document, pos: Pos2, xf: Xf) -> Option<Hit> {
        let h = edit::hit_test(doc, xf.to_image(pos), xf.img_len(HIT_TOL), xf.img_len(HANDLE_TOL));
        if matches!(h, Some(Hit::Handle(..))) {
            return h;
        }
        if let Some((id, _)) = self.label_rects.iter().rev().find(|(_, r)| r.contains(pos)) {
            return Some(Hit::Body(*id));
        }
        h
    }

    fn cursor_icon(&self, space: bool) -> CursorIcon {
        if let Some(g) = &self.gesture {
            return match g.kind {
                GestureKind::Pan => CursorIcon::Grabbing,
                GestureKind::Void | GestureKind::Secondary if g.dragged => CursorIcon::Grabbing,
                GestureKind::Handle(..) => CursorIcon::Crosshair,
                GestureKind::Body(_) => CursorIcon::Move,
                GestureKind::Draw => CursorIcon::Crosshair,
                _ => CursorIcon::Default,
            };
        }
        if space {
            return CursorIcon::Grab;
        }
        match (self.tool, self.hover_hit) {
            (Tool::Select, Some(Hit::Handle(..))) => CursorIcon::Grab,
            (Tool::Select, Some(Hit::Body(_))) => CursorIcon::Move,
            (Tool::Select, None) => CursorIcon::Default,
            _ => CursorIcon::Crosshair,
        }
    }

    /// Tolérance de doublon/fermeture d'un tracé, en pixels image.
    fn draft_tol(&self, xf: Xf) -> f64 {
        xf.img_len(HANDLE_TOL).min(xf.img_len(DOUBLE_CLICK_DIST))
    }

    fn press(&mut self, pos: Pos2, button: PointerButton, space: bool, doc: &mut Document) {
        let xf = self.xf();
        let ip = xf.to_image(pos);
        let mut grab = (0.0, 0.0);
        let kind = match button {
            PointerButton::Middle => GestureKind::Pan,
            PointerButton::Secondary => GestureKind::Secondary,
            PointerButton::Primary if space => GestureKind::Pan,
            PointerButton::Primary => match self.tool {
                Tool::Select => match self.hit(doc, pos, xf) {
                    Some(Hit::Handle(id, i)) => {
                        if let Some(m) = doc.get(id) {
                            let h = edit::handles(&m.shape)[i];
                            grab = (h.x - ip.x, h.y - ip.y);
                        }
                        GestureKind::Handle(id, i)
                    }
                    Some(Hit::Body(id)) => {
                        doc.selected = Some(id);
                        GestureKind::Body(id)
                    }
                    None => GestureKind::Void,
                },
                _ => {
                    if !self.draft.is_active() {
                        self.calib_seg = None;
                    }
                    let p = self.draft.constrain(ip, self.shift);
                    let step = self.draft.press(p, self.draft_tol(xf));
                    self.apply(step, doc);
                    GestureKind::Draw
                }
            },
            _ => return,
        };
        self.gesture = Some(Gesture {
            button,
            kind,
            start: pos,
            last: pos,
            dragged: false,
            start_img: ip,
            grab,
            orig: None,
        });
    }

    fn pointer_moved(&mut self, pos: Pos2, doc: &mut Document) {
        let Some(mut g) = self.gesture.take() else { return };
        let xf = self.xf();
        if !g.dragged && pos.distance(g.start) > DRAG_THRESHOLD {
            g.dragged = true;
            // Début d'une modification : un seul point d'annulation par glisser.
            if let GestureKind::Handle(id, _) | GestureKind::Body(id) = g.kind {
                g.orig = doc.get(id).map(|m| m.shape.clone());
                if g.orig.is_some() {
                    doc.checkpoint();
                }
            }
        }
        let delta = pos - g.last;
        g.last = pos;
        if g.dragged {
            let ip = xf.to_image(pos);
            match g.kind {
                GestureKind::Pan | GestureKind::Void | GestureKind::Secondary => {
                    self.view.pan += delta;
                    self.fitted = false;
                    self.view.clamp(self.last_rect.size(), self.img_size);
                }
                GestureKind::Handle(id, i) => match doc.get_mut(id) {
                    Some(m) => {
                        let p = Pt::new(ip.x + g.grab.0, ip.y + g.grab.1);
                        let p = edit::constrain_handle(&m.shape, i, p, self.shift);
                        edit::set_handle(&mut m.shape, i, p);
                    }
                    None => return, // mesure disparue (annuler pendant le glisser)
                },
                GestureKind::Body(id) => match (doc.get_mut(id), &g.orig) {
                    (Some(m), Some(orig)) => {
                        m.shape = orig.clone();
                        m.shape.translate(ip.x - g.start_img.x, ip.y - g.start_img.y);
                    }
                    _ => return,
                },
                GestureKind::Draw => {}
            }
        }
        self.gesture = Some(g);
    }

    fn release(&mut self, pos: Pos2, button: PointerButton, time: f64, doc: &mut Document) {
        if self.gesture.as_ref().is_none_or(|g| g.button != button) {
            return;
        }
        let Some(g) = self.gesture.take() else { return };
        let xf = self.xf();
        match g.kind {
            GestureKind::Draw => {
                let p = self.draft.constrain(xf.to_image(pos), self.shift);
                let step = self.draft.release(p, g.dragged, self.draft_tol(xf));
                let done = matches!(step, Step::Done(_));
                self.apply(step, doc);
                if !g.dragged && !done {
                    let double = self
                        .last_click
                        .is_some_and(|(t, q)| time - t <= DOUBLE_CLICK_DELAY && q.distance(pos) <= DOUBLE_CLICK_DIST);
                    self.last_click = if double { None } else { Some((time, pos)) };
                    if double {
                        let step = self.draft.finish();
                        self.apply(step, doc);
                    }
                }
            }
            GestureKind::Secondary if !g.dragged => {
                let step = self.draft.finish();
                self.apply(step, doc);
            }
            GestureKind::Void if !g.dragged => doc.selected = None,
            _ => {}
        }
    }

    /// Traite la fin d'un tracé : ajout au document ou segment d'étalonnage.
    fn apply(&mut self, step: Step, doc: &mut Document) {
        let Step::Done(pts) = step else { return };
        if self.tool == Tool::Calibrate {
            if let [a, b] = pts[..] {
                self.calib_seg = Some((a, b));
                self.out.calibration_segment = Some((a, b));
            }
        } else if let Some(shape) = Shape::from_points(self.draft.kind, &pts) {
            self.out.created = Some(doc.add(shape));
        }
        self.last_click = None;
    }

    fn paint_measurements(&mut self, p: &egui::Painter, xf: Xf, doc: &Document) {
        let hovered = match self.hover_hit {
            Some(Hit::Handle(id, _) | Hit::Body(id)) => Some(id),
            None => None,
        };
        let mut labels = Vec::new();
        let mut marker_no = 0;
        for m in &doc.measurements {
            if matches!(m.shape, Shape::Marker { .. }) {
                marker_no += 1;
            }
            if !m.visible {
                continue;
            }
            let color = render::color32(m.color);
            let strong = doc.selected == Some(m.id) || hovered == Some(m.id);
            render::paint_shape(p, xf, &m.shape, color, if strong { 3.0 } else { 2.0 }, marker_no);
            labels.push((m, color, strong, marker_no));
        }
        // Étiquettes au-dessus de tous les traits.
        self.label_rects.clear();
        for (m, color, strong, no) in labels {
            if let Shape::Marker { p: c } = m.shape {
                let r = Rect::from_center_size(xf.to_screen(c), Vec2::splat(2.0 * render::MARKER_RADIUS));
                self.label_rects.push((m.id, r));
                continue;
            }
            let text = render::label_text(&m.shape, &doc.calibration, no);
            if text.is_empty() {
                continue;
            }
            let (pos, align) = render::label_anchor(&m.shape, xf);
            let r = render::label(p, pos, align, &text, color, strong);
            self.label_rects.push((m.id, r));
        }
        // Poignées de la mesure sélectionnée.
        if self.tool == Tool::Select
            && let Some(m) = doc.selected().filter(|m| m.visible)
        {
            let hot = match (&self.gesture, self.hover_hit) {
                (Some(Gesture { kind: GestureKind::Handle(id, i), .. }), _) if *id == m.id => Some(*i),
                (None, Some(Hit::Handle(id, i))) if id == m.id => Some(i),
                _ => None,
            };
            render::paint_handles(p, xf, &edit::handles(&m.shape), hot);
        }
    }

    /// Dessine un tracé en cours (ou le segment d'étalonnage) avec sa valeur.
    /// `cal` : étalonnage pour l'étiquette (`None` : pas d'étiquette).
    fn paint_draft(&self, p: &egui::Painter, xf: Xf, pts: &[Pt], cal: Option<&Calibration>) {
        if pts.is_empty() {
            return;
        }
        let calibrate = self.tool == Tool::Calibrate;
        let kind = self.draft.kind;
        let color = if calibrate { CALIB_COLOR } else { kind_color(kind) };
        match Shape::from_points(kind, pts) {
            Some(shape) => {
                render::paint_shape(p, xf, &shape, color, 2.0, 0);
                if let Some(cal) = cal {
                    // Le segment d'étalonnage s'exprime toujours en pixels.
                    let px = Calibration::default();
                    let text = render::label_text(&shape, if calibrate { &px } else { cal }, 0);
                    if !text.is_empty() {
                        let (pos, align) = render::label_anchor(&shape, xf);
                        render::label(p, pos, align, &text, color, true);
                    }
                }
            }
            None => render::outlined(p, pts.iter().map(|q| xf.to_screen(*q)).collect(), false, color, 2.0),
        }
        for q in &self.draft.pts {
            render::dot(p, xf.to_screen(*q), color);
        }
    }
}

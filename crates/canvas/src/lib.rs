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
//!   contraint ; près d'un point remarquable d'une mesure, le point s'y
//!   aimante (Alt maintenu : sans aimantation) ; Entrée / double-clic / clic
//!   droit terminent un chemin ou une surface, Retour arrière retire le
//!   dernier point ;
//! - outil Sélection : clic pour sélectionner, glisser une poignée ou le corps ;
//! - outil Redresser : tracer un segment le long d'une ligne qui doit être
//!   horizontale ou verticale ; la vue pivote (`Document::rotation`), l'image
//!   et les coordonnées des mesures restent inchangées ;
//! - outil Perspective : cliquer les quatre coins d'un objet rectangulaire ;
//!   la vue signale le quadrilatère, l'application corrige la perspective.

mod draft;
mod edit;
mod render;
mod snap;
mod view;
#[cfg(test)]
mod tests;

use meteroscope_i18n::tr;
use draft::{Draft, Step};
use edit::Hit;
use egui::{
    Color32, CursorIcon, Event, Key, Modifiers, PointerButton, Pos2, Rect, Sense, Stroke, TextureHandle, Ui, Vec2,
    vec2,
};
use meteroscope_core::geometry::straighten_rotation;
use meteroscope_core::measure::format_number;
use meteroscope_core::{Calibration, Document, PALETTE, Pt, Shape, ShapeKind, Tool};
use view::{View, Xf};

pub use view::{MAX_ZOOM, MIN_ZOOM};

/// Seuil (pixels écran) au-delà duquel un appui devient un glisser.
const DRAG_THRESHOLD: f32 = 4.0;
/// Tolérance de clic sur un trait (pixels écran).
const HIT_TOL: f32 = 6.0;
/// Tolérance de clic sur une poignée ou un point (pixels écran).
const HANDLE_TOL: f32 = 9.0;
/// Distance d'aimantation aux points remarquables (pixels écran).
const SNAP_TOL: f32 = 8.0;
/// Distance écran entre le bord d'un rectangle et sa poignée de rotation.
const ROTATE_GAP: f32 = 24.0;
/// Délai et distance maximaux d'un double-clic.
const DOUBLE_CLICK_DELAY: f64 = 0.4;
const DOUBLE_CLICK_DIST: f32 = 6.0;
/// Au-delà de ce zoom, la loupe n'apporte plus rien.
const LOUPE_MAX_ZOOM: f32 = 12.0;
/// Couleur du segment d'étalonnage.
const CALIB_COLOR: Color32 = Color32::from_rgb(255, 64, 170);
/// Couleur du segment de redressement.
const STRAIGHTEN_COLOR: Color32 = Color32::from_rgb(64, 224, 255);
/// Couleur du quadrilatère de correction de perspective.
const PERSPECTIVE_COLOR: Color32 = Color32::from_rgb(255, 196, 48);

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
    /// La vue vient d'être redressée : nouvelle rotation (degrés).
    pub straightened: Option<f64>,
    /// L'utilisateur vient de cliquer les quatre coins d'un objet
    /// rectangulaire avec l'outil Perspective (dans l'ordre des clics) :
    /// l'application doit corriger la perspective.
    pub perspective_quad: Option<[Pt; 4]>,
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
    /// Rotation d'affichage appliquée (copie de `Document::rotation`).
    rotation: f64,
    /// La texture fournie est déjà pivotée : taille de sa boîte (pixels image).
    baked: Option<Vec2>,
    draft: Draft,
    /// Dernier segment d'étalonnage tracé (affiché tant que l'outil Échelle est actif).
    calib_seg: Option<(Pt, Pt)>,
    /// Dernier quadrilatère de perspective cliqué (affiché tant que
    /// l'application ne l'a pas traité : boîte de dialogue ouverte).
    persp_quad: Option<[Pt; 4]>,
    gesture: Option<Gesture>,
    last_click: Option<(f64, Pos2)>,
    /// Rectangles des étiquettes de la dernière frame (clic sur une étiquette).
    label_rects: Vec<(u64, Rect)>,
    has_selection: bool,
    hover_hit: Option<Hit>,
    shift: bool,
    /// Alt maintenu : aimantation désactivée.
    alt: bool,
    /// Point d'accroche courant (tracé ou poignée), pour le repère et l'aide.
    snap: Option<snap::Snap>,
    /// La poignée saisie peut s'aimanter (voir [`handle_snaps`]).
    handle_snaps: bool,
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
            rotation: 0.0,
            baked: None,
            draft: Draft::new(ShapeKind::Length),
            calib_seg: None,
            persp_quad: None,
            gesture: None,
            last_click: None,
            label_rects: Vec::new(),
            has_selection: false,
            hover_hit: None,
            shift: false,
            alt: false,
            snap: None,
            handle_snaps: false,
            out: CanvasOutput::default(),
        }
    }
}

/// Type de forme tracé par un outil (l'Échelle trace une longueur).
fn draft_kind(tool: Tool) -> Option<ShapeKind> {
    match tool {
        Tool::Select => None,
        Tool::Calibrate | Tool::Straighten => Some(ShapeKind::Length),
        // Quadrilatère : un polygone de quatre points.
        Tool::Perspective => Some(ShapeKind::Area),
        Tool::Draw(k) => Some(k),
    }
}

/// Tracé vierge de l'outil.
fn new_draft(tool: Tool) -> Draft {
    match tool {
        Tool::Perspective => Draft::polygon(4),
        t => Draft::new(draft_kind(t).unwrap_or(ShapeKind::Length)),
    }
}

/// La poignée `i` de la mesure `id` peut s'aimanter. Ni la rotation d'un
/// rectangle (seul l'angle compte), ni un marqueur (deux marqueurs superposés
/// se compteraient sans se voir).
fn handle_snaps(doc: &Document, id: u64, i: usize) -> bool {
    doc.get(id).is_some_and(|m| match m.shape {
        Shape::Rect { .. } => i != edit::ROTATE_HANDLE,
        Shape::Marker { .. } => false,
        _ => true,
    })
}

/// Couleur attribuée par `Document::add` à un type de forme.
fn kind_color(kind: ShapeKind) -> Color32 {
    let kind = kind.produces();
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
        self.draft = new_draft(tool);
        self.persp_quad = None;
        if self.gesture.as_ref().is_some_and(|g| g.kind == GestureKind::Draw) {
            self.gesture = None;
        }
        if tool != Tool::Calibrate {
            self.calib_seg = None;
        }
    }

    /// Indique que la texture passée à [`Canvas::show`] contient l'image déjà
    /// redressée (pivotée de `Document::rotation` dans sa boîte englobante,
    /// coin haut-gauche à l'origine) ; `size` : taille de cette boîte en pixels
    /// de l'image d'origine. `None` : texture droite, pivotée à l'affichage.
    pub fn set_baked(&mut self, size: Option<Vec2>) {
        self.baked = size;
    }

    /// Ajuste l'image à la vue à la prochaine frame.
    pub fn fit(&mut self) {
        self.pending = Some(Pending::Fit);
    }

    /// Taille de l'image pivotée (boîte englobante), utilisée pour ajuster
    /// et borner la vue.
    fn vimg(&self) -> Vec2 {
        view::rotated_size(self.img_size, self.rotation)
    }

    fn known(&self) -> bool {
        self.last_rect.is_positive() && self.img_size.x > 0.0 && self.img_size.y > 0.0
    }

    /// Zoom 1:1 centré.
    pub fn zoom_actual(&mut self) {
        if self.known() {
            self.view = View::actual(self.last_rect.size(), self.vimg());
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
        self.view.clamp(self.last_rect.size(), self.vimg());
        self.fitted = false;
    }

    /// Zoom courant (pixels écran par pixel image).
    pub fn zoom(&self) -> f32 {
        self.view.zoom
    }

    /// Position à l'écran d'un point image selon la vue de la dernière frame
    /// (pour superposer un dessin à l'image, p. ex. un réticule).
    pub fn image_to_screen(&self, p: Pt) -> Pos2 {
        self.xf().to_screen(p)
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

    /// Oublie le quadrilatère de perspective affiché (correction appliquée
    /// ou abandonnée par l'application).
    pub fn clear_perspective_quad(&mut self) {
        self.persp_quad = None;
    }

    /// Aide contextuelle d'une ligne pour la barre d'état, selon l'outil et
    /// l'avancement du tracé (« Cliquez le premier point », …).
    pub fn hint(&self) -> String {
        let n = self.draft.pts.len();
        let s = match self.tool {
            Tool::Select => {
                if matches!(self.gesture.as_ref().map(|g| g.kind), Some(GestureKind::Handle(..))) {
                    tr!("hint.select.drag_handle")
                } else if self.has_selection {
                    tr!("hint.select.selected")
                } else {
                    tr!("hint.select.idle")
                }
            }
            Tool::Calibrate => match (n, self.calib_seg.is_some()) {
                (0, false) => tr!("hint.scale.start"),
                (0, true) => tr!("hint.scale.dialog"),
                _ => tr!("hint.scale.end"),
            },
            Tool::Straighten => match n {
                0 => tr!("hint.straighten.start"),
                _ => tr!("hint.straighten.end"),
            },
            Tool::Perspective => match (n, self.persp_quad.is_some()) {
                (0, true) => tr!("hint.perspective.dialog"),
                (0, false) => tr!("hint.perspective.first"),
                (1, _) => tr!("hint.perspective.second"),
                (2, _) => tr!("hint.perspective.third"),
                _ => tr!("hint.perspective.fourth"),
            },
            Tool::Draw(kind) => match (kind, n) {
                (ShapeKind::Length, 0) => tr!("hint.length.first"),
                (ShapeKind::Length, _) => tr!("hint.length.second"),
                (ShapeKind::Rect, 0) => tr!("hint.rect.first"),
                (ShapeKind::Rect, _) => tr!("hint.rect.second"),
                (ShapeKind::Rect3, 0) => tr!("hint.rect3.first"),
                (ShapeKind::Rect3, 1) => tr!("hint.rect3.second"),
                (ShapeKind::Rect3, _) => tr!("hint.rect3.third"),
                (ShapeKind::Circle, 0) => tr!("hint.circle.center"),
                (ShapeKind::Circle, _) => tr!("hint.circle.edge"),
                (ShapeKind::Circle3, 0) => tr!("hint.circle3.first"),
                (ShapeKind::Circle3, 1) => tr!("hint.circle3.second"),
                (ShapeKind::Circle3, _) => tr!("hint.circle3.third"),
                (ShapeKind::Angle, 0) => tr!("hint.angle.first"),
                (ShapeKind::Angle, 1) => tr!("hint.angle.vertex"),
                (ShapeKind::Angle, _) => tr!("hint.angle.second"),
                (ShapeKind::Path, 0) => tr!("hint.path.first"),
                (ShapeKind::Path, 1) => tr!("hint.path.second"),
                (ShapeKind::Path, _) => {
                    tr!("hint.path.next")
                }
                (ShapeKind::Area, 0) => tr!("hint.area.first"),
                (ShapeKind::Area, 1 | 2) => tr!("hint.area.second"),
                (ShapeKind::Area, _) => {
                    tr!("hint.area.next")
                }
                (ShapeKind::Marker, _) => tr!("hint.marker"),
                (ShapeKind::LineAngle, 0) => tr!("hint.line_angle.first"),
                (ShapeKind::LineAngle, 1) => tr!("hint.line_angle.first_end"),
                (ShapeKind::LineAngle, 2) => tr!("hint.line_angle.second"),
                (ShapeKind::LineAngle, _) => tr!("hint.line_angle.second_end"),
                (ShapeKind::PointLine, 0) => tr!("hint.point_line.first"),
                (ShapeKind::PointLine, 1) => tr!("hint.point_line.second"),
                (ShapeKind::PointLine, _) => tr!("hint.point_line.point"),
                (ShapeKind::Parallel, 0) => tr!("hint.parallel.first"),
                (ShapeKind::Parallel, 1) => tr!("hint.parallel.second"),
                (ShapeKind::Parallel, _) => tr!("hint.parallel.point"),
                (ShapeKind::Ellipse, 0) => tr!("hint.ellipse.center"),
                (ShapeKind::Ellipse, 1) => tr!("hint.ellipse.axis"),
                (ShapeKind::Ellipse, _) => tr!("hint.ellipse.width"),
                (ShapeKind::Arc, 0) => tr!("hint.arc.first"),
                (ShapeKind::Arc, 1) => tr!("hint.arc.middle"),
                (ShapeKind::Arc, _) => tr!("hint.arc.last"),
                (ShapeKind::Profile, 0) => tr!("hint.profile.first"),
                (ShapeKind::Profile, _) => tr!("hint.profile.second"),
            },
        };
        // Aimantation : partout où l'on pose un point ou déplace une poignée.
        let snapping = match self.tool {
            Tool::Select => {
                self.handle_snaps && matches!(self.gesture.as_ref().map(|g| g.kind), Some(GestureKind::Handle(..)))
            }
            Tool::Calibrate => n > 0 || self.calib_seg.is_none(),
            Tool::Draw(k) => k != ShapeKind::Marker,
            Tool::Straighten => true,
            // Les coins s'aimantent ; plus rien à poser tant que le dialogue est ouvert.
            Tool::Perspective => self.persp_quad.is_none(),
        };
        match (snapping, self.snap) {
            (false, _) => s.to_owned(),
            (true, Some(sn)) => format!("{s} — {}", tr!("hint.snapped", target = sn.kind.label())),
            (true, None) => format!("{s} — {}", tr!("hint.snap")),
        }
    }

    fn xf(&self) -> Xf {
        self.view.xf(self.last_rect.min).rotated(self.rotation, self.img_size)
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
        self.sync_rotation(doc.rotation);
        match self.pending.take() {
            Some(Pending::Fit) => self.fit_now(),
            Some(Pending::Actual) => {
                self.view = View::actual(rect.size(), self.vimg());
                self.fitted = false;
            }
            None if resized && self.fitted => self.fit_now(),
            None => {}
        }

        // Entrées de la frame.
        let kb_ok = !ui.ctx().egui_wants_keyboard_input();
        let hovering = resp.contains_pointer();
        let (events, shift, alt, space, time, scroll, pinch, hover) = ui.input(|i| {
            (
                i.events.clone(),
                i.modifiers.shift,
                i.modifiers.alt,
                i.key_down(Key::Space),
                i.time,
                i.smooth_scroll_delta,
                i.zoom_delta(),
                i.pointer.latest_pos(),
            )
        });
        self.shift = shift;
        self.alt = alt;
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
            self.view.clamp(rect.size(), self.vimg());
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
        let outline = render::image_outline(xf, [w, h]);
        painter.add(egui::Shape::convex_polygon(outline, Color32::from_gray(10), Stroke::new(1.0, Color32::from_gray(10))));
        render::paint_image(&painter, tex.id(), [w, h], xf, self.baked);
        self.paint_measurements(&painter, xf, doc);

        // Aperçu du tracé (le point sous le curseur suit la contrainte Maj).
        let pointer = if self.gesture.is_some() { hover } else { hover_in };
        let draw_tool = draft_kind(self.tool).is_some();
        // Repère d'aimantation : celui de la poignée déplacée (calculé au
        // glisser) ou celui du point sous le curseur pendant un tracé.
        if !matches!(self.gesture, Some(Gesture { kind: GestureKind::Handle(..), dragged: true, .. })) {
            self.snap = None;
        }
        let preview_pt = pointer.filter(|_| draw_tool).map(|p| {
            let (q, s) = self.draft_point(doc, xf.to_image(p), xf);
            self.snap = s;
            q
        });
        if self.tool == Tool::Calibrate
            && !self.draft.is_active()
            && let Some((a, b)) = self.calib_seg
        {
            self.paint_draft(&painter, xf, &[a, b], Some(&doc.calibration));
        }
        if !self.draft.is_active()
            && let Some(q) = self.persp_quad
        {
            self.paint_draft(&painter, xf, &q, None);
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
        let snap_mark = self.snap;
        if let Some(s) = snap_mark {
            render::paint_snap(&painter, xf.to_screen(s.pt), s.kind);
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
                render::paint_loupe(&painter, tex.id(), [w, h], self.baked, xf, p, cur, rect, mag, |lp, lxf| {
                    self.paint_draft(lp, lxf, &pts, None);
                    if let Some(s) = snap_mark {
                        render::paint_snap(lp, lxf.to_screen(s.pt), s.kind);
                    }
                });
            } else if let Some(Gesture { kind: GestureKind::Handle(id, i), dragged: true, .. }) = &self.gesture
                && let Some(m) = doc.get(*id)
                // Pas de loupe pour une rotation : la position exacte n'y compte pas.
                && !edit::is_rotate_handle(&m.shape, *i)
            {
                let hp = edit::handles(&m.shape, xf.img_len(ROTATE_GAP))[*i];
                let color = render::color32(m.color);
                render::paint_loupe(&painter, tex.id(), [w, h], self.baked, xf, hp, cur, rect, mag, |lp, lxf| {
                    render::paint_shape(lp, lxf, &m.shape, color, 1.5, 0);
                    if let Some(s) = snap_mark {
                        render::paint_snap(lp, lxf.to_screen(s.pt), s.kind);
                    }
                });
            }
        }

        std::mem::take(&mut self.out)
    }

    /// Forme d'un tracé. Un rectangle par deux coins suit l'horizontale et
    /// la verticale de la vue (redressée).
    fn shape_from_draft(&self, kind: ShapeKind, pts: &[Pt]) -> Option<Shape> {
        let shape = Shape::from_points(kind, pts)?;
        Some(if kind == ShapeKind::Rect { shape.oriented(-self.rotation) } else { shape })
    }

    fn fit_now(&mut self) {
        self.view = View::fit(self.last_rect.size(), self.vimg());
        self.fitted = true;
    }

    /// Applique une nouvelle rotation d'affichage. Vue ajustée : on réajuste ;
    /// sinon le point de l'image au centre de la vue y reste.
    fn sync_rotation(&mut self, rotation: f64) {
        if rotation == self.rotation {
            return;
        }
        let center = self.last_rect.center();
        let keep = self.known().then(|| self.xf().to_image(center));
        self.rotation = rotation;
        if self.fitted {
            self.pending = Some(Pending::Fit);
        } else if let Some(p) = keep {
            self.view.pan += center - self.xf().to_screen(p);
            self.view.clamp(self.last_rect.size(), self.vimg());
        }
    }

    /// Élément sous le point écran `pos` : poignées, puis étiquettes, puis traits.
    fn hit(&self, doc: &Document, pos: Pos2, xf: Xf) -> Option<Hit> {
        let h = edit::hit_test(doc, xf.to_image(pos), xf.img_len(HIT_TOL), xf.img_len(HANDLE_TOL), xf.img_len(ROTATE_GAP));
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

    /// Point d'accroche près de `p` (pixels image), sauf si Alt est maintenu.
    fn find_snap(&self, doc: &Document, p: Pt, xf: Xf, exclude: Option<u64>) -> Option<snap::Snap> {
        if self.alt {
            return None;
        }
        snap::find_snap(doc, p, xf.img_len(SNAP_TOL), exclude)
    }

    /// Point du tracé sous le pointeur `ip` : aimanté, sinon contraint par Maj.
    /// Les marqueurs (comptage) ne s'aimantent pas : deux marqueurs
    /// superposés se compteraient sans se voir.
    fn draft_point(&self, doc: &Document, ip: Pt, xf: Xf) -> (Pt, Option<snap::Snap>) {
        let s = if self.draft.kind == ShapeKind::Marker { None } else { self.find_snap(doc, ip, xf, None) };
        (snap::snap_or(s, ip, |p| self.draft.constrain(p, self.shift, self.rotation)), s)
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
                            let h = edit::handles(&m.shape, xf.img_len(ROTATE_GAP))[i];
                            grab = (h.x - ip.x, h.y - ip.y);
                        }
                        self.handle_snaps = handle_snaps(doc, id, i);
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
                        self.persp_quad = None;
                    }
                    let (p, _) = self.draft_point(doc, ip, xf);
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
                    self.view.clamp(self.last_rect.size(), self.vimg());
                }
                GestureKind::Handle(id, i) => {
                    let p = Pt::new(ip.x + g.grab.0, ip.y + g.grab.1);
                    // Une mesure ne s'aimante jamais sur elle-même.
                    self.snap = if handle_snaps(doc, id, i) { self.find_snap(doc, p, xf, Some(id)) } else { None };
                    match doc.get_mut(id) {
                        Some(m) => {
                            let (shift, rot) = (self.shift, self.rotation);
                            let p = snap::snap_or(self.snap, p, |p| edit::constrain_handle(&m.shape, i, p, shift, rot));
                            edit::set_handle(&mut m.shape, i, p);
                        }
                        None => return, // mesure disparue (annuler pendant le glisser)
                    }
                }
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
                let (p, _) = self.draft_point(doc, xf.to_image(pos), xf);
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

    /// Traite la fin d'un tracé : ajout au document, segment d'étalonnage
    /// ou redressement de la vue.
    fn apply(&mut self, step: Step, doc: &mut Document) {
        let Step::Done(pts) = step else { return };
        if self.tool == Tool::Straighten {
            if let [a, b] = pts[..]
                && let Some(r) = straighten_rotation(a, b, doc.rotation)
                && doc.set_rotation(r)
            {
                self.out.straightened = Some(doc.rotation);
            }
        } else if self.tool == Tool::Perspective {
            if let [a, b, c, d] = pts[..] {
                self.persp_quad = Some([a, b, c, d]);
                self.out.perspective_quad = self.persp_quad;
            }
        } else if self.tool == Tool::Calibrate {
            if let [a, b] = pts[..] {
                self.calib_seg = Some((a, b));
                self.out.calibration_segment = Some((a, b));
            }
        } else if let Some(shape) = self.shape_from_draft(self.draft.kind, &pts) {
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
            let gap = xf.img_len(ROTATE_GAP);
            let mut hs = edit::handles(&m.shape, gap);
            if let Some((base, h)) = edit::rotate_handle(&m.shape, gap) {
                hs.pop();
                render::paint_rotate_handle(p, xf.to_screen(base), xf.to_screen(h), hot == Some(hs.len()));
            }
            render::paint_handles(p, xf, &hs, hot);
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
        let color = match self.tool {
            Tool::Calibrate => CALIB_COLOR,
            Tool::Straighten => STRAIGHTEN_COLOR,
            Tool::Perspective => PERSPECTIVE_COLOR,
            _ => kind_color(kind),
        };
        if self.tool == Tool::Perspective {
            // Quadrilatère en cours : contour refermé dès le troisième coin.
            render::outlined(p, pts.iter().map(|q| xf.to_screen(*q)).collect(), pts.len() >= 3, color, 2.0);
            if pts.len() == 4 && !self.draft.is_active() {
                for q in pts {
                    render::dot(p, xf.to_screen(*q), color);
                }
            }
        } else if let (Tool::Straighten, [a, b]) = (self.tool, pts) {
            // Segment de redressement : on affiche son écart à l'horizontale ou
            // à la verticale de l'écran, c.-à-d. la rotation qui sera appliquée.
            render::segment(p, xf.to_screen(*a), xf.to_screen(*b), color, 2.0);
            if cal.is_some()
                && let Some(r) = straighten_rotation(*a, *b, self.rotation)
            {
                let delta = meteroscope_core::geometry::normalize_deg(r - self.rotation);
                let text = format!("{}°", format_number(delta.abs()));
                let (pos, align) = render::label_anchor(&Shape::Length { a: *a, b: *b }, xf);
                render::label(p, pos, align, &text, color, true);
            }
        } else if let (ShapeKind::LineAngle, [a, b, c]) = (kind, pts) {
            // Premier segment posé, début du second.
            render::segment(p, xf.to_screen(*a), xf.to_screen(*b), color, 2.0);
            render::dot(p, xf.to_screen(*c), color);
        } else {
            match self.shape_from_draft(kind, pts) {
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
        }
        for q in &self.draft.pts {
            render::dot(p, xf.to_screen(*q), color);
        }
    }
}

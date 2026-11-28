//! Panneau « Détecter » : détection automatique d'objets par seuillage.
//!
//! L'aperçu se calcule à chaque réglage sur une version réduite de l'image
//! et colore sur la vue ce qui sera retenu ; « Ajouter » lance le calcul en
//! pleine résolution dans un fil dédié, puis crée une mesure de surface par
//! objet, annulable d'un seul Ctrl+Z.
//!
//! La détection travaille sur l'image d'origine : le réglage
//! luminosité/contraste, purement visuel, n'en change pas le résultat, et
//! l'aperçu est calculé sur les mêmes valeurs que le résultat.

use egui::{ColorImage, RichText, TextureHandle, TextureOptions, Ui};
use image::RgbaImage;
use meteroscope_canvas::Canvas;
use meteroscope_core::geometry::rect_corners;
use meteroscope_core::measure::format_number;
use meteroscope_core::{Document, Pt, Shape};
use meteroscope_i18n::tr;
use meteroscope_io::detect::{self, DetectParams, Detection, Prepared};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

/// Plus grand côté de l'image réduite servant à l'aperçu.
const PREVIEW_SIDE: u32 = 1600;
/// Au-delà, on n'ajoute rien : la liste et l'historique deviendraient lourds,
/// et c'est presque toujours le signe d'un seuil mal réglé.
pub const MAX_OBJECTS: usize = 5000;

/// Zone de détection et lissage : ce dont dépend l'image préparée.
#[derive(Clone, PartialEq)]
struct Key {
    zone: Option<Vec<Pt>>,
    smooth: bool,
}

pub struct DetectView {
    open: bool,
    params: DetectParams,
    smooth: bool,
    /// Aire maximale saisie (appliquée si `use_max`).
    max_area: f64,
    use_max: bool,
    /// Seuil et polarité déjà initialisés pour cette image.
    initialized: bool,
    prep: Option<(Key, Prepared)>,
    texture: Option<TextureHandle>,
    /// Réglages de l'aperçu affiché, et nombre d'objets retenus.
    shown: Option<(DetectParams, usize)>,
    pending: Option<Receiver<Detection>>,
}

impl Default for DetectView {
    fn default() -> Self {
        Self {
            open: false,
            params: DetectParams::default(),
            smooth: true,
            max_area: 100_000.0,
            use_max: false,
            initialized: false,
            prep: None,
            texture: None,
            shown: None,
            pending: None,
        }
    }
}

/// Polygone de la zone de détection : la mesure sélectionnée si c'est un
/// rectangle, une surface ou un cercle ; sinon l'image entière (`None`).
pub fn zone_of(doc: &Document) -> Option<(String, Vec<Pt>)> {
    let m = doc.selected()?;
    let pts = match &m.shape {
        Shape::Rect { a, b, angle } => rect_corners(*a, *b, *angle).to_vec(),
        Shape::Area { pts } if pts.len() >= 3 => pts.clone(),
        s => {
            let (c, r) = s.circle()?;
            (0..72).map(|i| (i as f64 * 5.0).to_radians()).map(|t| Pt::new(c.x + r * t.cos(), c.y + r * t.sin())).collect()
        }
    };
    Some((m.name.clone(), pts))
}

/// Mesures créées à partir d'une détection : une surface par objet.
pub fn shapes_of(det: Detection) -> Vec<Shape> {
    det.objects.into_iter().filter(|o| o.polygon.len() >= 3).map(|o| Shape::Area { pts: o.polygon }).collect()
}

impl DetectView {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// L'image a changé (ouverture, rotation, retournement) : tout est à refaire.
    pub fn invalidate(&mut self) {
        self.prep = None;
        self.shown = None;
        self.pending = None;
        self.initialized = false;
    }

    fn effective(&self) -> DetectParams {
        DetectParams { max_area: if self.use_max { self.max_area } else { f64::INFINITY }, ..self.params }
    }

    /// Une frame : panneau, aperçu sur la vue, résultat du calcul complet.
    /// Renvoie un message pour la barre d'état (texte, erreur ?).
    pub fn frame(&mut self, ctx: &egui::Context, image: Option<&RgbaImage>, doc: &mut Document, canvas: &mut Canvas) -> Option<(String, bool)> {
        // Sans image (mode caméra…), `doc` n'est pas celui de la photo qui a
        // lancé le calcul : son résultat est abandonné.
        if image.is_none() {
            self.pending = None;
        }
        let msg = self.poll(ctx, doc);
        let Some(img) = image.filter(|_| self.open) else {
            canvas.set_overlay(None);
            return msg;
        };
        let zone = zone_of(doc);
        self.update_preview(ctx, img, zone.as_ref().map(|z| z.1.clone()));

        let mut open = self.open;
        let mut run = false;
        egui::Window::new(tr!("detect.title"))
            .open(&mut open)
            .resizable(false)
            .collapsible(false)
            .default_pos(ctx.content_rect().right_top() + egui::vec2(-640.0, 70.0))
            .show(ctx, |ui| run = self.window(ui, doc, zone.as_ref().map(|z| z.0.as_str())));
        self.open = open;
        if run {
            let p = self.effective();
            self.pending = Some(detect::spawn_detection(img, zone.map(|z| z.1), self.smooth, p));
        }

        match (&self.texture, &self.prep) {
            (Some(t), Some((_, p))) if self.open && !p.is_empty() => {
                let (x0, y0, k) = (p.origin.0 as f64, p.origin.1 as f64, p.scale as f64);
                canvas.set_overlay(Some((t.id(), [x0, y0, x0 + p.width as f64 * k, y0 + p.height as f64 * k])));
            }
            _ => canvas.set_overlay(None),
        }
        msg
    }

    /// (Re)prépare l'image réduite si la zone ou le lissage changent, puis
    /// recalcule l'aperçu si un réglage a changé.
    fn update_preview(&mut self, ctx: &egui::Context, img: &RgbaImage, zone: Option<Vec<Pt>>) {
        let key = Key { zone, smooth: self.smooth };
        if self.prep.as_ref().is_none_or(|(k, _)| *k != key) {
            let p = detect::prepare(img, key.zone.as_deref(), PREVIEW_SIDE, key.smooth);
            if !self.initialized {
                // Seuil d'Otsu ; les objets sont la classe minoritaire.
                let t = p.otsu();
                let h = p.histogram();
                let above: u64 = h[t as usize + 1..].iter().sum();
                self.params.threshold = t;
                self.params.bright = 2 * above < h.iter().sum::<u64>();
                self.initialized = true;
            }
            self.prep = Some((key, p));
            self.shown = None;
        }
        let params = self.effective();
        if self.shown.is_some_and(|(p, _)| p == params) {
            return;
        }
        let prep = &self.prep.as_ref().unwrap().1;
        // Zone vide (rectangle aplati, hors de l'image) : rien à montrer, et
        // une texture de taille nulle ferait planter le rendu (wgpu).
        if prep.is_empty() {
            self.shown = Some((params, 0));
            return;
        }
        let (overlay, n) = detect::preview(prep, &params);
        let ci = ColorImage::from_rgba_unmultiplied([overlay.width() as usize, overlay.height() as usize], overlay.as_raw());
        match &mut self.texture {
            Some(t) => t.set(ci, TextureOptions::NEAREST),
            None => self.texture = Some(ctx.load_texture("detection", ci, TextureOptions::NEAREST)),
        }
        self.shown = Some((params, n));
    }

    /// Contenu du panneau ; renvoie `true` si l'utilisateur lance l'ajout.
    fn window(&mut self, ui: &mut Ui, doc: &Document, zone: Option<&str>) -> bool {
        ui.spacing_mut().item_spacing.y = 6.0;
        let p = &mut self.params;
        ui.horizontal(|ui| {
            ui.label(tr!("detect.objects"));
            ui.radio_value(&mut p.bright, false, tr!("detect.dark")).on_hover_text(tr!("detect.dark_tip"));
            ui.radio_value(&mut p.bright, true, tr!("detect.bright")).on_hover_text(tr!("detect.bright_tip"));
        });
        ui.horizontal(|ui| {
            ui.add(egui::Slider::new(&mut p.threshold, 0..=255).text(tr!("detect.threshold")));
            if ui.button(tr!("detect.auto")).on_hover_text(tr!("detect.auto_tip")).clicked()
                && let Some((_, prep)) = &self.prep
            {
                p.threshold = prep.otsu();
            }
        });
        ui.checkbox(&mut self.smooth, tr!("detect.smooth")).on_hover_text(tr!("detect.smooth_tip"));
        let cal = &doc.calibration;
        let real = |px: f64| {
            if cal.is_calibrated() { format!("≈ {} {}", format_number(cal.area(px)), cal.area_unit()) } else { String::new() }
        };
        egui::Grid::new("meteroscope_detection_aires").num_columns(3).show(ui, |ui| {
            ui.label(tr!("detect.min_area"));
            ui.add(egui::DragValue::new(&mut p.min_area).range(1.0..=1e9).speed(1.0).suffix(" px²"));
            ui.label(RichText::new(real(p.min_area)).weak());
            ui.end_row();
            ui.checkbox(&mut self.use_max, tr!("detect.max_area"));
            ui.add_enabled(self.use_max, egui::DragValue::new(&mut self.max_area).range(1.0..=1e12).speed(10.0).suffix(" px²"));
            ui.label(RichText::new(if self.use_max { real(self.max_area) } else { String::new() }).weak());
            ui.end_row();
        });
        ui.checkbox(&mut p.exclude_border, tr!("detect.exclude_border")).on_hover_text(tr!("detect.exclude_border_tip"));
        ui.checkbox(&mut p.fill_holes, tr!("detect.fill_holes")).on_hover_text(tr!("detect.fill_holes_tip"));
        ui.separator();
        match zone {
            Some(name) => ui.label(tr!("detect.zone_selection", name = name)),
            None => ui.label(tr!("detect.zone_image")),
        }
        .on_hover_text(tr!("detect.zone_tip"));
        let n = self.shown.map_or(0, |s| s.1);
        let approx = self.prep.as_ref().is_some_and(|(_, p)| p.scale > 1);
        let count = if approx { tr!("detect.count_approx", n = n) } else { tr!("detect.count", n = n) };
        ui.label(RichText::new(count).strong());
        ui.label(RichText::new(tr!("detect.original_note")).weak().small());
        let mut run = false;
        ui.horizontal(|ui| {
            if self.pending.is_some() {
                ui.spinner();
                ui.label(tr!("detect.running"));
            // Aperçu réduit : de petits objets peuvent n'apparaître qu'à
            // pleine résolution, le calcul complet reste donc possible.
            } else if ui.add_enabled(n > 0 || approx, egui::Button::new(tr!("detect.add"))).on_hover_text(tr!("detect.add_tip")).clicked() {
                run = true;
            }
        });
        run
    }

    /// Récupère le résultat du calcul complet et crée les mesures.
    fn poll(&mut self, ctx: &egui::Context, doc: &mut Document) -> Option<(String, bool)> {
        let rx = self.pending.as_ref()?;
        let det = match rx.try_recv() {
            Ok(det) => det,
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(Duration::from_millis(50));
                return None;
            }
            Err(TryRecvError::Disconnected) => {
                self.pending = None;
                return Some((tr!("detect.failed").to_owned(), true));
            }
        };
        self.pending = None;
        let n = det.objects.len();
        if n > MAX_OBJECTS {
            return Some((tr!("detect.too_many", n = n, max = MAX_OBJECTS), true));
        }
        let ids = doc.add_many(shapes_of(det));
        Some(if ids.is_empty() { (tr!("detect.none").to_owned(), false) } else { (tr!("detect.added", n = ids.len()), false) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resultat_abandonne_sans_image() {
        // Calcul lancé sur la photo, puis passage en mode caméra : le document
        // reçu (celui du flux) ne doit pas recevoir les contours de la photo.
        meteroscope_i18n::set_language("fr");
        let (ctx, mut canvas, img) = (egui::Context::default(), Canvas::default(), RgbaImage::new(8, 8));
        let pending = || {
            let (tx, rx) = std::sync::mpsc::channel();
            let square = vec![Pt::new(1., 1.), Pt::new(3., 1.), Pt::new(3., 3.), Pt::new(1., 3.)];
            tx.send(Detection { objects: vec![detect::Object { area: 4.0, centroid: Pt::new(2., 2.), polygon: square }], rejected: 0 }).unwrap();
            Some(rx)
        };
        let mut view = DetectView { pending: pending(), ..Default::default() };
        let mut camera_doc = Document::new();
        assert!(view.frame(&ctx, None, &mut camera_doc, &mut canvas).is_none());
        assert!(camera_doc.measurements.is_empty() && view.pending.is_none());
        // Avec la photo, le résultat est bien ajouté.
        let mut photo_doc = Document::new();
        view.pending = pending();
        assert!(view.frame(&ctx, Some(&img), &mut photo_doc, &mut canvas).is_some());
        assert_eq!(photo_doc.measurements.len(), 1);
    }

    #[test]
    fn zone_aplatie() {
        // Rectangle de largeur nulle choisi comme zone : ni aperçu, ni
        // texture vide (qui ferait planter le rendu).
        meteroscope_i18n::set_language("fr");
        let (ctx, img) = (egui::Context::default(), RgbaImage::new(64, 64));
        let mut doc = Document::new();
        doc.add(Shape::Rect { a: Pt::new(10., 10.), b: Pt::new(10., 40.), angle: 0.0 });
        let mut view = DetectView { open: true, ..Default::default() };
        view.update_preview(&ctx, &img, zone_of(&doc).map(|z| z.1));
        assert!(view.texture.is_none());
        assert_eq!(view.shown.map(|s| s.1), Some(0));
    }

    #[test]
    fn zone_et_mesures() {
        meteroscope_i18n::set_language("fr");
        let mut doc = Document::new();
        assert!(zone_of(&doc).is_none());
        let id = doc.add(Shape::Rect { a: Pt::new(10., 10.), b: Pt::new(50., 30.), angle: 0.0 });
        let (name, z) = zone_of(&doc).unwrap();
        assert_eq!((name.as_str(), z.len()), ("Rectangle 1", 4));
        doc.add(Shape::Circle { center: Pt::new(0., 0.), edge: Pt::new(10., 0.) });
        assert_eq!(zone_of(&doc).unwrap().1.len(), 72);
        doc.add(Shape::Length { a: Pt::new(0., 0.), b: Pt::new(1., 1.) });
        assert!(zone_of(&doc).is_none(), "une longueur ne délimite pas de zone");
        doc.selected = Some(id);

        // Image : deux carrés sombres sur fond clair ; un seul Ctrl+Z.
        let img = RgbaImage::from_fn(200, 100, |x, y| {
            let dark = (20..40).contains(&y) && ((20..40).contains(&x) || (120..150).contains(&x));
            let v = if dark { 30 } else { 220 };
            image::Rgba([v, v, v, 255])
        });
        let prep = detect::prepare(&img, None, u32::MAX, false);
        let det = detect::detect(&prep, &DetectParams { threshold: prep.otsu(), ..Default::default() }, true);
        let before = doc.measurements.len();
        let ids = doc.add_many(shapes_of(det));
        assert_eq!(ids.len(), 2);
        let areas: Vec<f64> = ids.iter().map(|&i| doc.get(i).unwrap().shape.values(&doc.calibration)[0].value).collect();
        assert_eq!(areas, vec![400.0, 600.0]);
        assert_eq!(doc.get(ids[1]).unwrap().name, "Surface 2");
        assert!(doc.undo());
        assert_eq!(doc.measurements.len(), before);
    }
}

/// Temps d'une frame de la liste des mesures et de la vue image avec
/// beaucoup d'objets détectés (`cargo test --release -p meteroscope -- --ignored --nocapture fluidite`).
#[cfg(test)]
mod perf {
    use super::*;
    use std::time::Instant;

    #[test]
    #[ignore]
    fn fluidite() {
        meteroscope_i18n::set_language("fr");
        for n in [300, 1000, 5000] {
            let mut doc = Document::new();
            let mut objs = vec![];
            for i in 0..n {
                let (cx, cy) = (20.0 + (i % 100) as f64 * 40.0, 20.0 + (i / 100) as f64 * 40.0);
                objs.push(Shape::Area { pts: (0..24).map(|k| (k as f64 * 15.0).to_radians()).map(|t| Pt::new(cx + 12.0 * t.cos(), cy + 12.0 * t.sin())).collect() });
            }
            doc.add_many(objs);
            let ctx = egui::Context::default();
            let input = || egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0))),
                ..Default::default()
            };
            let mut canvas = Canvas::default();
            let tex = ctx.load_texture("t", ColorImage::filled([4000, 2000], egui::Color32::GRAY), TextureOptions::LINEAR);
            let (mut t_list, mut t_canvas) = (Duration::ZERO, Duration::ZERO);
            for frame in 0..12 {
                let _ = ctx.run_ui(input(), |ui| {
                    let t = Instant::now();
                    egui::Panel::right("m").default_size(300.0).show(ui, |ui| {
                        meteroscope_panels::measurements_panel(ui, &mut doc);
                    });
                    let t1 = Instant::now();
                    egui::CentralPanel::default().show(ui, |ui| {
                        canvas.show(ui, &mut doc, Some((&tex, [4000, 2000])));
                    });
                    if frame >= 2 {
                        t_list += t1 - t;
                        t_canvas += t1.elapsed();
                    }
                });
            }
            println!("{n:>5} objets : liste {:?}/frame, vue {:?}/frame", t_list / 10, t_canvas / 10);
        }
    }
}

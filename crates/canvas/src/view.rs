//! Transformation image ↔ écran, zoom et déplacement de la vue.

use egui::{Pos2, Vec2, pos2, vec2};
use mesura_core::Pt;

/// Bornes du zoom (pixels écran par pixel image).
pub const MIN_ZOOM: f32 = 0.01;
pub const MAX_ZOOM: f32 = 64.0;
/// Partie de l'image (pixels écran) qui reste toujours visible.
const KEEP_VISIBLE: f32 = 48.0;
/// Marge autour de l'image ajustée à la vue.
const FIT_MARGIN: f32 = 8.0;

/// Transformation affine image → écran : `écran = origin + p × zoom`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xf {
    pub origin: Pos2,
    pub zoom: f32,
}

impl Xf {
    pub fn to_screen(self, p: Pt) -> Pos2 {
        pos2(self.origin.x + p.x as f32 * self.zoom, self.origin.y + p.y as f32 * self.zoom)
    }

    pub fn to_image(self, s: Pos2) -> Pt {
        Pt::new(((s.x - self.origin.x) / self.zoom) as f64, ((s.y - self.origin.y) / self.zoom) as f64)
    }

    /// Convertit une distance écran en distance image.
    pub fn img_len(self, screen: f32) -> f64 {
        (screen / self.zoom) as f64
    }
}

/// État de la vue, relatif au coin haut-gauche de la zone d'affichage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub zoom: f32,
    /// Position écran de l'origine de l'image, relative au coin de la vue.
    pub pan: Vec2,
}

impl Default for View {
    fn default() -> Self {
        Self { zoom: 1.0, pan: Vec2::ZERO }
    }
}

impl View {
    pub fn xf(&self, view_min: Pos2) -> Xf {
        Xf { origin: view_min + self.pan, zoom: self.zoom }
    }

    /// Ajuste l'image entière dans la vue, centrée.
    pub fn fit(view: Vec2, img: Vec2) -> View {
        if img.x <= 0.0 || img.y <= 0.0 || view.x <= 0.0 || view.y <= 0.0 {
            return View::default();
        }
        let avail = (view - Vec2::splat(2.0 * FIT_MARGIN)).max(Vec2::splat(1.0));
        let zoom = (avail.x / img.x).min(avail.y / img.y).clamp(MIN_ZOOM, MAX_ZOOM);
        View { zoom, pan: (view - img * zoom) / 2.0 }
    }

    /// Zoom 1:1 centré.
    pub fn actual(view: Vec2, img: Vec2) -> View {
        View { zoom: 1.0, pan: (view - img) / 2.0 }
    }

    /// Multiplie le zoom par `factor` en gardant fixe le point écran `anchor`
    /// (relatif au coin de la vue).
    pub fn zoom_about(&mut self, anchor: Vec2, factor: f32) {
        let new = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let k = new / self.zoom;
        self.pan = anchor - (anchor - self.pan) * k;
        self.zoom = new;
    }

    /// Empêche l'image de sortir entièrement de la vue.
    pub fn clamp(&mut self, view: Vec2, img: Vec2) {
        let size = img * self.zoom;
        let m = vec2(KEEP_VISIBLE.min(size.x), KEEP_VISIBLE.min(size.y));
        let clamp1 = |v: f32, lo: f32, hi: f32| if lo > hi { (lo + hi) / 2.0 } else { v.clamp(lo, hi) };
        self.pan.x = clamp1(self.pan.x, m.x - size.x, view.x - m.x);
        self.pan.y = clamp1(self.pan.y, m.y - size.y, view.y - m.y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aller_retour() {
        let xf = Xf { origin: pos2(10.0, 20.0), zoom: 2.5 };
        let p = Pt::new(13.2, 7.5);
        let back = xf.to_image(xf.to_screen(p));
        assert!(back.dist(p) < 1e-4);
        assert_eq!(xf.to_screen(Pt::new(0.0, 0.0)), pos2(10.0, 20.0));
        assert!((xf.img_len(5.0) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn ajuster_et_zoomer() {
        let v = View::fit(vec2(816.0, 616.0), vec2(400.0, 100.0));
        assert_eq!(v.zoom, 2.0);
        assert_eq!(v.pan, vec2(8.0, 208.0));
        // Le point sous l'ancre ne bouge pas.
        let mut z = v;
        let anchor = vec2(300.0, 250.0);
        let before = z.xf(Pos2::ZERO).to_image(anchor.to_pos2());
        z.zoom_about(anchor, 1.7);
        let after = z.xf(Pos2::ZERO).to_image(anchor.to_pos2());
        assert!(before.dist(after) < 1e-3);
        // Bornes.
        z.zoom_about(anchor, 1e6);
        assert_eq!(z.zoom, MAX_ZOOM);
        z.zoom_about(anchor, 1e-9);
        assert_eq!(z.zoom, MIN_ZOOM);
        let a = View::actual(vec2(800.0, 600.0), vec2(200.0, 100.0));
        assert_eq!((a.zoom, a.pan), (1.0, vec2(300.0, 250.0)));
    }

    #[test]
    fn image_jamais_perdue() {
        let mut v = View { zoom: 1.0, pan: vec2(5000.0, -5000.0) };
        v.clamp(vec2(800.0, 600.0), vec2(200.0, 100.0));
        assert_eq!(v.pan, vec2(800.0 - 48.0, 48.0 - 100.0));
    }
}

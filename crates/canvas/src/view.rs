//! Transformation image ↔ écran, zoom et déplacement de la vue.

use egui::{Pos2, Vec2, pos2, vec2};
use meteroscope_core::Pt;

/// Bornes du zoom (pixels écran par pixel image).
pub const MIN_ZOOM: f32 = 0.01;
pub const MAX_ZOOM: f32 = 64.0;
/// Partie de l'image (pixels écran) qui reste toujours visible.
const KEEP_VISIBLE: f32 = 48.0;
/// Marge autour de l'image ajustée à la vue.
const FIT_MARGIN: f32 = 8.0;

/// Transformation image → écran : `écran = origin + (R·p + t) × zoom`, où
/// `R` est la rotation d'affichage (redressement) et `t` place l'image
/// pivotée dans sa boîte englobante (coin haut-gauche en 0).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xf {
    pub origin: Pos2,
    pub zoom: f32,
    cos: f64,
    sin: f64,
    t: (f64, f64),
}

/// Taille de la boîte englobante d'une image `img` pivotée de `deg` degrés.
pub fn rotated_size(img: Vec2, deg: f64) -> Vec2 {
    if deg == 0.0 {
        return img;
    }
    let (s, c) = deg.to_radians().sin_cos();
    let (s, c) = (s.abs() as f32, c.abs() as f32);
    vec2(c * img.x + s * img.y, s * img.x + c * img.y)
}

impl Xf {
    /// Sans rotation.
    pub fn new(origin: Pos2, zoom: f32) -> Self {
        Self { origin, zoom, cos: 1.0, sin: 0.0, t: (0.0, 0.0) }
    }

    /// Même transformation avec l'image (taille `img`) pivotée de `deg`
    /// degrés (positif = sens horaire) autour de son centre.
    pub fn rotated(self, deg: f64, img: Vec2) -> Self {
        if deg == 0.0 {
            return Self::new(self.origin, self.zoom);
        }
        let (sin, cos) = deg.to_radians().sin_cos();
        let v = rotated_size(img, deg);
        let (cx, cy) = (img.x as f64 / 2.0, img.y as f64 / 2.0);
        let t = (v.x as f64 / 2.0 - (cos * cx - sin * cy), v.y as f64 / 2.0 - (sin * cx + cos * cy));
        Self { cos, sin, t, ..self }
    }

    /// Même rotation, zoom `zoom`, le point image `p` placé en `screen`.
    pub fn centered_on(self, p: Pt, screen: Pos2, zoom: f32) -> Self {
        let mut x = Self { origin: Pos2::ZERO, zoom, ..self };
        x.origin = screen - x.to_screen(p).to_vec2();
        x
    }

    pub fn to_screen(self, p: Pt) -> Pos2 {
        let x = self.cos * p.x - self.sin * p.y + self.t.0;
        let y = self.sin * p.x + self.cos * p.y + self.t.1;
        let z = self.zoom as f64;
        pos2((self.origin.x as f64 + x * z) as f32, (self.origin.y as f64 + y * z) as f32)
    }

    pub fn to_image(self, s: Pos2) -> Pt {
        let z = self.zoom as f64;
        let qx = (s.x - self.origin.x) as f64 / z - self.t.0;
        let qy = (s.y - self.origin.y) as f64 / z - self.t.1;
        Pt::new(self.cos * qx + self.sin * qy, -self.sin * qx + self.cos * qy)
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
        Xf::new(view_min + self.pan, self.zoom)
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
        meteroscope_i18n::set_language("fr");
        let xf = Xf::new(pos2(10.0, 20.0), 2.5);
        let p = Pt::new(13.2, 7.5);
        let back = xf.to_image(xf.to_screen(p));
        assert!(back.dist(p) < 1e-4);
        assert_eq!(xf.to_screen(Pt::new(0.0, 0.0)), pos2(10.0, 20.0));
        assert!((xf.img_len(5.0) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn rotation() {
        meteroscope_i18n::set_language("fr");
        let img = vec2(200.0, 100.0);
        // Quart de tour horaire : boîte 100 × 200, le coin haut-gauche part en haut à droite.
        let xf = Xf::new(Pos2::ZERO, 1.0).rotated(90.0, img);
        assert_eq!(rotated_size(img, 90.0).round(), vec2(100.0, 200.0));
        let tl = xf.to_screen(Pt::new(0.0, 0.0));
        assert!(tl.distance(pos2(100.0, 0.0)) < 1e-3, "{tl:?}");
        assert!(xf.to_screen(Pt::new(200.0, 100.0)).distance(pos2(0.0, 200.0)) < 1e-3);
        // Aller-retour, angle quelconque.
        let xf = Xf::new(pos2(30.0, -12.0), 1.7).rotated(-7.5, img);
        let p = Pt::new(13.2, 87.5);
        assert!(xf.to_image(xf.to_screen(p)).dist(p) < 1e-3);
        // Un segment penché de +7,5° dans l'image devient horizontal à l'écran.
        let b = Pt::new(p.x + 7.5f64.to_radians().cos() * 50.0, p.y + 7.5f64.to_radians().sin() * 50.0);
        assert!((xf.to_screen(p).y - xf.to_screen(b).y).abs() < 1e-3);
        // Recentrage : le point visé arrive au centre demandé.
        let c = xf.centered_on(p, pos2(70.0, 70.0), 8.0);
        assert!(c.to_screen(p).distance(pos2(70.0, 70.0)) < 1e-3 && c.zoom == 8.0);
    }

    #[test]
    fn ajuster_et_zoomer() {
        meteroscope_i18n::set_language("fr");
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
        meteroscope_i18n::set_language("fr");
        let mut v = View { zoom: 1.0, pan: vec2(5000.0, -5000.0) };
        v.clamp(vec2(800.0, 600.0), vec2(200.0, 100.0));
        assert_eq!(v.pan, vec2(800.0 - 48.0, 48.0 - 100.0));
    }
}

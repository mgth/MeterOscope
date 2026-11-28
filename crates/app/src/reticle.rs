//! Réticule superposé au flux caméra : croix centrale, et au choix cercles
//! concentriques ou grille, gradués dans l'unité de l'échelle. Purement
//! visuel (jamais enregistré comme mesure) ; le choix est mémorisé.

use egui::{Align2, Color32, FontId, Painter, Pos2, Stroke};
use meteroscope_canvas::Canvas;
use meteroscope_core::measure::format_number;
use meteroscope_core::{Calibration, Pt};
use meteroscope_i18n::tr;
use std::path::PathBuf;

/// Écart visé entre deux graduations, en pixels écran.
const TARGET_SPACING: f32 = 80.0;
/// Nombre maximal de cercles ou de lignes par demi-axe (garde-fou).
const MAX_STEPS: usize = 400;
/// Longueur des graduations de la croix (pixels écran).
const TICK: f32 = 5.0;
const COLOR: Color32 = Color32::from_rgba_premultiplied(230, 210, 0, 230);
const SHADOW: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 110);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Reticle {
    #[default]
    Off,
    Cross,
    Circles,
    Grid,
}

impl Reticle {
    pub const ALL: [Reticle; 4] = [Reticle::Off, Reticle::Cross, Reticle::Circles, Reticle::Grid];

    pub fn label(self) -> &'static str {
        match self {
            Reticle::Off => tr!("reticle.off"),
            Reticle::Cross => tr!("reticle.cross"),
            Reticle::Circles => tr!("reticle.circles"),
            Reticle::Grid => tr!("reticle.grid"),
        }
    }

    fn id(self) -> &'static str {
        match self {
            Reticle::Off => "off",
            Reticle::Cross => "cross",
            Reticle::Circles => "circles",
            Reticle::Grid => "grid",
        }
    }

    fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.id() == id)
    }
}

fn pref_file() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("meteroscope").join("reticle"))
}

pub fn load() -> Reticle {
    let saved = pref_file().and_then(|f| std::fs::read_to_string(f).ok());
    saved.as_deref().map(str::trim).and_then(Reticle::from_id).unwrap_or_default()
}

pub fn save(r: Reticle) {
    let Some(file) = pref_file() else { return };
    let _ = file.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&file, r.id()));
}

/// Plus petite valeur « ronde » (1, 2 ou 5 × 10ⁿ) supérieure ou égale à `v`.
pub fn nice_step(v: f64) -> f64 {
    if !(v.is_finite() && v > 0.0) {
        return 1.0;
    }
    let p = 10f64.powf(v.log10().floor());
    let m = v / p;
    // Petite marge : 2,0000001 reste 2 malgré les arrondis.
    let n = [1.0, 2.0, 5.0, 10.0].into_iter().find(|n| m <= n * (1.0 + 1e-9)).unwrap_or(10.0);
    n * p
}

/// Pas de graduation pour un zoom donné : (pas en unités réelles, pas en
/// pixels image). Sans échelle, l'unité est le pixel et le pas est entier.
pub fn graduation(zoom: f32, cal: &Calibration) -> (f64, f64) {
    let k = cal.length(1.0);
    let min_px = (TARGET_SPACING / zoom.max(1e-6)) as f64;
    let mut units = nice_step(min_px * k);
    if !cal.is_calibrated() {
        units = units.max(1.0);
    }
    (units, units / k)
}

fn line(p: &Painter, a: Pos2, b: Pos2) {
    p.line_segment([a, b], Stroke::new(3.0, SHADOW));
    p.line_segment([a, b], Stroke::new(1.0, COLOR));
}

fn text(p: &Painter, pos: Pos2, align: Align2, s: &str) {
    let font = FontId::proportional(12.0);
    p.text(pos + egui::vec2(1.0, 1.0), align, s, font.clone(), Color32::BLACK);
    p.text(pos, align, s, font, COLOR);
}

/// Dessine le réticule centré sur l'image `[w, h]` affichée par `canvas`.
pub fn paint(p: &Painter, canvas: &Canvas, size: [usize; 2], cal: &Calibration, kind: Reticle) {
    if kind == Reticle::Off {
        return;
    }
    let (w, h) = (size[0] as f64, size[1] as f64);
    let c = Pt::new(w / 2.0, h / 2.0);
    let s = |x: f64, y: f64| canvas.image_to_screen(Pt::new(x, y));
    // Rien en dehors de l'image (boîte englobante si la vue est redressée).
    let bbox = egui::Rect::from_points(&[s(0.0, 0.0), s(w, 0.0), s(w, h), s(0.0, h)]);
    let clipped = p.with_clip_rect(p.clip_rect().intersect(bbox));
    let p = &clipped;
    let zoom = canvas.zoom();
    let (units, step) = graduation(zoom, cal);
    let unit = cal.length_unit();
    let n = (((w * w + h * h).sqrt() / 2.0 / step).ceil() as usize).min(MAX_STEPS);
    let label = |k: usize| format!("{} {unit}", format_number(units * k as f64));

    // Croix sur toute l'image.
    line(p, s(0.0, c.y), s(w, c.y));
    line(p, s(c.x, 0.0), s(c.x, h));
    // Directions écran des deux bras (la vue peut être redressée).
    let sc = s(c.x, c.y);
    let ux = (s(c.x + 1.0, c.y) - sc).normalized();
    let uy = (s(c.x, c.y + 1.0) - sc).normalized();
    for k in 1..=n {
        let d = k as f64 * step;
        for (q, u) in [(s(c.x + d, c.y), uy), (s(c.x - d, c.y), uy), (s(c.x, c.y + d), ux), (s(c.x, c.y - d), ux)] {
            line(p, q - u * TICK, q + u * TICK);
        }
    }

    match kind {
        Reticle::Circles => {
            for k in 1..=n {
                let r = (k as f64 * step) as f32 * zoom;
                p.circle_stroke(sc, r, Stroke::new(3.0, SHADOW));
                p.circle_stroke(sc, r, Stroke::new(1.0, COLOR));
                let at = s(c.x + k as f64 * step, c.y);
                text(p, at + uy * (TICK + 2.0), Align2::LEFT_TOP, &label(k));
            }
        }
        Reticle::Grid => {
            for k in 1..=n {
                let d = k as f64 * step;
                for x in [c.x + d, c.x - d] {
                    if (0.0..=w).contains(&x) {
                        line(p, s(x, 0.0), s(x, h));
                    }
                }
                for y in [c.y + d, c.y - d] {
                    if (0.0..=h).contains(&y) {
                        line(p, s(0.0, y), s(w, y));
                    }
                }
                if c.x + d <= w {
                    text(p, s(c.x + d, c.y) + uy * (TICK + 2.0) + ux * 3.0, Align2::LEFT_TOP, &label(k));
                }
            }
        }
        _ => {
            // Croix seule : graduations numérotées sur le bras droit.
            for k in 1..=n {
                let d = k as f64 * step;
                if c.x + d <= w {
                    text(p, s(c.x + d, c.y) + uy * (TICK + 2.0), Align2::CENTER_TOP, &label(k));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pas_ronds() {
        assert_eq!(nice_step(0.7), 1.0);
        assert_eq!(nice_step(1.0), 1.0);
        assert_eq!(nice_step(1.3), 2.0);
        assert_eq!(nice_step(3.0), 5.0);
        assert_eq!(nice_step(7.0), 10.0);
        assert!((nice_step(0.023) - 0.05).abs() < 1e-12);
        assert_eq!(nice_step(120.0), 200.0);
        assert_eq!(nice_step(0.0), 1.0);
    }

    #[test]
    fn graduation_dans_l_unite_de_l_echelle() {
        // Sans échelle, zoom 1 : 80 px → pas de 100 px.
        let cal = Calibration::default();
        assert_eq!(graduation(1.0, &cal), (100.0, 100.0));
        // Fort zoom sans échelle : jamais moins d'un pixel.
        assert_eq!(graduation(200.0, &cal), (1.0, 1.0));
        // 0,01 mm/px, zoom 2 : 40 px = 0,4 mm → pas de 0,5 mm = 50 px.
        let mut cal = Calibration::default();
        assert!(cal.set_from_reference(100.0, 1.0, "mm"));
        let (u, px) = graduation(2.0, &cal);
        assert!((u - 0.5).abs() < 1e-12 && (px - 50.0).abs() < 1e-9);
    }

    #[test]
    fn choix_memorisable() {
        for r in Reticle::ALL {
            assert_eq!(Reticle::from_id(r.id()), Some(r));
        }
        assert_eq!(Reticle::from_id("?"), None);
    }
}

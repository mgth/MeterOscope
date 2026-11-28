//! Profil d'intensité : détection des bords au sous-pixel sur une courbe 1D.
//!
//! Ce module est pur : il ne connaît pas l'image. L'échantillonnage des
//! pixels le long du segment se fait dans `meteroscope-io` (`profile`), qui
//! fournit ici une suite de valeurs régulièrement espacées. Le résultat
//! retenu (positions des deux bords) est rangé dans la mesure
//! ([`ProfileResult`], cache recalculé par l'application quand le segment,
//! les réglages ou l'image changent), si bien que `Shape::values` reste
//! une fonction de la seule géométrie enregistrée.
//!
//! Deux localisations d'un bord sont proposées ([`EdgeMethod`]) :
//!
//! - **mi-hauteur** (par défaut) : franchissement du niveau moyen entre les
//!   deux plateaux qui encadrent la transition. C'est la définition
//!   habituelle d'une largeur de trait (largeur à mi-hauteur) ; elle utilise
//!   le signal lui-même, peu amplifié par le bruit, et reste juste quel que
//!   soit le flou tant que les plateaux existent ;
//! - **gradient** : extremum de la dérivée lissée, affiné par une parabole
//!   sur trois points. Elle ne dépend pas des plateaux (éclairage non
//!   uniforme, bords très rapprochés) mais elle est plus sensible au bruit.
//!
//! Dans les deux cas les bords candidats sont d'abord repérés par les
//! extremums du gradient lissé qui dépassent le seuil de détection.

use crate::geometry::Pt;
use crate::measure::Shape;
use serde::{Deserialize, Serialize};

/// Écart-type (pixels) de la gaussienne de lissage du gradient.
pub const GRADIENT_SIGMA: f64 = 1.0;
/// Écart-type (pixels) du lissage léger appliqué avant la mi-hauteur.
pub const LEVEL_SIGMA: f64 = 0.6;
/// Longueur maximale (pixels) d'un plateau pris en compte de chaque côté d'un bord.
pub const PLATEAU_LENGTH: f64 = 12.0;
/// Début d'un plateau, en demi-largeurs de transition depuis l'extremum du gradient.
pub const PLATEAU_START: f64 = 1.5;
/// Seuil automatique : fraction du gradient le plus fort.
pub const AUTO_THRESHOLD: f64 = 0.25;
/// Largeur de bande par défaut (pixels).
pub const DEFAULT_BAND: f64 = 9.0;
/// Bornes de la largeur de bande réglable (pixels).
pub const BAND_RANGE: std::ops::RangeInclusive<f64> = 1.0..=200.0;

/// Manière de localiser un bord dans la transition.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeMethod {
    /// Franchissement à mi-hauteur entre les deux plateaux.
    #[default]
    HalfHeight,
    /// Extremum du gradient lissé (ajustement parabolique).
    Gradient,
}

fn default_true() -> bool {
    true
}
fn default_band() -> f64 {
    DEFAULT_BAND
}

/// Réglages d'une mesure par profil (enregistrés avec la mesure).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileParams {
    /// Largeur (pixels) de la bande perpendiculaire au segment sur laquelle
    /// l'intensité est moyennée.
    #[serde(default = "default_band")]
    pub band: f64,
    #[serde(default)]
    pub method: EdgeMethod,
    /// Seuil de détection, en fraction du gradient le plus fort (0 à 1).
    /// `None` : automatique.
    #[serde(default)]
    pub threshold: Option<f64>,
    /// Mesurer la luminance linéaire (gamma sRGB retiré) plutôt que les
    /// valeurs codées du fichier.
    #[serde(default = "default_true")]
    pub linear: bool,
    /// Ramener la largeur à la perpendiculaire des bords quand le segment
    /// n'est pas tracé exactement en travers.
    #[serde(default = "default_true")]
    pub perpendicular: bool,
    /// Bords choisis à la main : positions en fraction de la longueur du
    /// segment. `None` : les deux bords les plus marqués.
    #[serde(default)]
    pub pick: Option<[f64; 2]>,
}

impl Default for ProfileParams {
    fn default() -> Self {
        Self { band: DEFAULT_BAND, method: EdgeMethod::default(), threshold: None, linear: true, perpendicular: true, pick: None }
    }
}

/// Résultat mis en cache dans la mesure : les deux bords retenus.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProfileResult {
    /// Positions des deux bords le long du segment (pixels depuis `a`), croissantes.
    pub edges: [f64; 2],
    /// Inclinaison des bords par rapport à la perpendiculaire au segment,
    /// en degrés (0 : segment tracé exactement en travers, ou non corrigé).
    #[serde(default)]
    pub tilt: f64,
}

impl ProfileResult {
    /// Distance entre les deux bords, en pixels, ramenée à la perpendiculaire.
    pub fn width_px(&self) -> f64 {
        (self.edges[1] - self.edges[0]).abs() * self.tilt.to_radians().cos()
    }
}

impl Shape {
    /// Nouveau profil d'intensité sur le segment `a→b`, réglages par défaut.
    pub fn profile(a: Pt, b: Pt) -> Shape {
        Shape::Profile { a, b, params: ProfileParams::default(), result: None }
    }

    /// Points image des deux bords détectés d'un profil.
    pub fn profile_edges(&self) -> Option<[Pt; 2]> {
        let Shape::Profile { a, b, result: Some(r), .. } = self else { return None };
        let len = a.dist(*b);
        (len > 0.0).then(|| r.edges.map(|t| a.lerp(*b, t / len)))
    }

    /// Demi-largeur de la bande d'un profil et normale unitaire au segment.
    pub fn profile_band(&self) -> Option<(f64, Pt)> {
        let Shape::Profile { a, b, params, .. } = self else { return None };
        let len = a.dist(*b);
        (len > 0.0).then(|| (params.band / 2.0, Pt::new(-(b.y - a.y) / len, (b.x - a.x) / len)))
    }
}

/// Un bord détecté sur le profil.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    /// Position le long du segment, en pixels depuis le premier échantillon.
    pub pos: f64,
    /// Valeur absolue du gradient lissé à son extremum (intensité par pixel).
    pub strength: f64,
    /// Intensité croissante dans le sens du segment.
    pub rising: bool,
    /// Niveaux des plateaux de part et d'autre (côté sombre, côté clair).
    pub low: f64,
    pub high: f64,
}

/// Analyse complète d'un profil : de quoi tracer la courbe et choisir les bords.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Analysis {
    /// Pas entre deux échantillons, en pixels.
    pub step: f64,
    /// Intensités échantillonnées (0 à 1).
    pub values: Vec<f64>,
    /// Bords détectés au-dessus du seuil, par position croissante.
    pub edges: Vec<Edge>,
    /// Indices dans `edges` des deux bords retenus (croissants).
    pub chosen: Option<[usize; 2]>,
    /// Seuil appliqué, en fraction du gradient le plus fort.
    pub threshold: f64,
    /// Inclinaison estimée des bords (degrés), renseignée par l'échantillonnage.
    pub tilt: f64,
}

impl Analysis {
    /// Longueur couverte par le profil, en pixels.
    pub fn length(&self) -> f64 {
        self.step * self.values.len().saturating_sub(1) as f64
    }

    /// Résultat à ranger dans la mesure.
    pub fn result(&self) -> Option<ProfileResult> {
        let [i, j] = self.chosen?;
        Some(ProfileResult { edges: [self.edges[i].pos, self.edges[j].pos], tilt: self.tilt })
    }

    /// Indice du bord détecté le plus proche de la position `pos` (pixels).
    pub fn nearest_edge(&self, pos: f64) -> Option<usize> {
        (0..self.edges.len()).min_by(|i, j| (self.edges[*i].pos - pos).abs().total_cmp(&(self.edges[*j].pos - pos).abs()))
    }

    /// Choix obtenu en cliquant près de la position `pos` : le bord le plus
    /// proche remplace celui des deux bords retenus dont il est le plus
    /// voisin. Renvoie les positions en fraction de la longueur
    /// ([`ProfileParams::pick`]), ou `None` si rien ne change.
    pub fn pick_at(&self, pos: f64) -> Option<[f64; 2]> {
        let k = self.nearest_edge(pos)?;
        let [i, j] = self.chosen?;
        if k == i || k == j {
            return None;
        }
        let d = |e: usize| (self.edges[e].pos - self.edges[k].pos).abs();
        let (keep, new) = if d(i) <= d(j) { (j, k) } else { (i, k) };
        let len = self.length();
        let mut f = [self.edges[keep].pos / len, self.edges[new].pos / len];
        f.sort_by(f64::total_cmp);
        Some(f)
    }
}

/// Convolution avec bords prolongés (la première et la dernière valeur sont répétées).
fn convolve(v: &[f64], kernel: &[f64]) -> Vec<f64> {
    let r = (kernel.len() / 2) as isize;
    let last = v.len() as isize - 1;
    (0..v.len() as isize)
        .map(|i| kernel.iter().enumerate().map(|(k, w)| w * v[(i + k as isize - r).clamp(0, last) as usize]).sum())
        .collect()
}

/// Noyau gaussien normalisé, `sigma` en échantillons.
fn gaussian(sigma: f64) -> Vec<f64> {
    let r = (4.0 * sigma).ceil().max(1.0) as isize;
    let k: Vec<f64> = (-r..=r).map(|j| (-(j * j) as f64 / (2.0 * sigma * sigma)).exp()).collect();
    let sum: f64 = k.iter().sum();
    k.into_iter().map(|w| w / sum).collect()
}

/// Dérivée de gaussienne, normalisée pour rendre la pente d'une rampe
/// (par échantillon). Le noyau est écrit pour une corrélation.
fn gaussian_derivative(sigma: f64) -> Vec<f64> {
    let r = (4.0 * sigma).ceil().max(1.0) as isize;
    let k: Vec<f64> = (-r..=r).map(|j| j as f64 * (-(j * j) as f64 / (2.0 * sigma * sigma)).exp()).collect();
    let norm: f64 = k.iter().zip(-r..=r).map(|(w, j)| w * j as f64).sum();
    k.into_iter().map(|w| w / norm).collect()
}

fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(f64::total_cmp);
    let n = v.len();
    if n % 2 == 1 { v[n / 2] } else { (v[n / 2 - 1] + v[n / 2]) / 2.0 }
}

/// Extremum du gradient retenu comme bord candidat.
struct Peak {
    i: usize,
    /// Position affinée, en échantillons.
    x: f64,
    g: f64,
}

/// Analyse un profil régulièrement échantillonné (`step` pixels entre deux valeurs).
///
/// `threshold` : fraction du gradient le plus fort au-dessus de laquelle un
/// extremum est un bord (`None` : automatique, [`AUTO_THRESHOLD`] relevé si
/// le bruit l'exige). `pick` : bords choisis à la main, en fraction de la
/// longueur ; sinon les deux bords les plus marqués.
pub fn analyze(values: &[f64], step: f64, method: EdgeMethod, threshold: Option<f64>, pick: Option<[f64; 2]>) -> Analysis {
    let mut out = Analysis { step, values: values.to_vec(), threshold: threshold.unwrap_or(AUTO_THRESHOLD), ..Default::default() };
    let n = values.len();
    if n < 8 || step.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return out;
    }
    // Gradient lissé, en intensité par pixel.
    let grad: Vec<f64> = convolve(values, &gaussian_derivative(GRADIENT_SIGMA / step)).into_iter().map(|g| g / step).collect();
    let gmax = grad.iter().fold(0.0f64, |m, g| m.max(g.abs()));
    if gmax <= 0.0 {
        return out;
    }
    // Seuil automatique : au moins 5 écarts-types du bruit du gradient
    // (estimé par l'écart absolu médian, insensible aux bords eux-mêmes),
    // sans jamais écarter le bord le plus fort ni son vis-à-vis.
    let fraction = match threshold {
        Some(f) => f.clamp(0.01, 1.0),
        None => {
            let med = median(grad.clone());
            let noise = 1.4826 * median(grad.iter().map(|g| (g - med).abs()).collect());
            AUTO_THRESHOLD.max((5.0 * noise / gmax).min(0.5))
        }
    };
    out.threshold = fraction;
    let limit = fraction * gmax;

    let mut peaks = vec![];
    for i in 1..n - 1 {
        let (l, c, r) = (grad[i - 1].abs(), grad[i].abs(), grad[i + 1].abs());
        let same_sign = grad[i - 1] * grad[i] >= 0.0 && grad[i + 1] * grad[i] >= 0.0;
        if c >= limit && c > l && c >= r && same_sign {
            // Sommet de la parabole passant par les trois points.
            let den = l - 2.0 * c + r;
            let d = if den < 0.0 { (0.5 * (l - r) / den).clamp(-0.5, 0.5) } else { 0.0 };
            peaks.push(Peak { i, x: i as f64 + d, g: grad[i] });
        }
    }

    // Un bord très flou et bruité donne plusieurs extremums voisins de même
    // signe : ils ne font qu'un bord tant que le gradient ne retombe pas
    // nettement (sous la moitié du plus petit) entre eux.
    let mut merged: Vec<Peak> = vec![];
    for p in peaks {
        if let Some(q) = merged.last_mut()
            && q.g * p.g > 0.0
        {
            let dip = grad[q.i..=p.i].iter().fold(f64::INFINITY, |m, g| m.min(g.abs()));
            if dip > 0.5 * q.g.abs().min(p.g.abs()) {
                if p.g.abs() > q.g.abs() {
                    *q = p;
                }
                continue;
            }
        }
        merged.push(p);
    }
    let peaks = merged;

    let smooth = convolve(values, &gaussian(LEVEL_SIGMA / step));
    let spans = transitions(&grad, &peaks);
    let plateau = (PLATEAU_LENGTH / step).round().max(1.0) as usize;
    for (k, p) in peaks.iter().enumerate() {
        let (jl, jr) = spans[k];
        let prev = (k > 0).then(|| (peaks[k - 1].i, spans[k - 1].1));
        let next = (k + 1 < peaks.len()).then(|| (peaks[k + 1].i, spans[k + 1].0));
        let left = plateau_level(&smooth, p.i, jl, prev, plateau);
        let right = plateau_level(&smooth, p.i, jr, next, plateau);
        let rising = p.g > 0.0;
        let (low, high) = if rising { (left, right) } else { (right, left) };
        let x = match method {
            EdgeMethod::Gradient => p.x,
            EdgeMethod::HalfHeight => crossing(&smooth, (low + high) / 2.0, jl, jr, p.x).unwrap_or(p.x),
        };
        out.edges.push(Edge { pos: x * step, strength: p.g.abs(), rising, low, high });
    }
    out.edges.sort_by(|a, b| a.pos.total_cmp(&b.pos));
    out.chosen = choose(&out.edges, pick.map(|f| f.map(|t| t * out.length())));
    out
}

/// Étendue (indices) de la transition de chaque extremum : là où le gradient
/// garde son signe et dépasse 10 % de l'extremum, sans franchir le milieu
/// de l'intervalle avec l'extremum voisin.
fn transitions(grad: &[f64], peaks: &[Peak]) -> Vec<(usize, usize)> {
    let n = grad.len();
    let inside = |j: usize, p: &Peak| grad[j] * p.g > 0.0 && grad[j].abs() >= 0.1 * p.g.abs();
    peaks
        .iter()
        .enumerate()
        .map(|(k, p)| {
            let lo = if k > 0 { (peaks[k - 1].i + p.i).div_ceil(2) } else { 0 };
            let hi = if k + 1 < peaks.len() { (p.i + peaks[k + 1].i) / 2 } else { n - 1 };
            let (mut jl, mut jr) = (p.i, p.i);
            while jl > lo && inside(jl - 1, p) {
                jl -= 1;
            }
            while jr < hi && inside(jr + 1, p) {
                jr += 1;
            }
            (jl, jr)
        })
        .collect()
}

/// Niveau du plateau d'un côté d'un bord : médiane du profil sur une
/// fenêtre d'au plus `length` échantillons.
///
/// `peak` est l'extremum du bord, `edge` le bout de sa transition de ce
/// côté, `neighbour` l'extremum et le bout de transition du bord voisin de
/// ce côté (sinon le profil va jusqu'à son extrémité). La fenêtre commence
/// à deux demi-transitions de l'extremum, là où la queue du flou est
/// négligeable (≈ 4,3 σ pour une gaussienne), et s'arrête à la même distance
/// du voisin. S'il ne reste pas de place (trait à peine plus large que le
/// flou, ou plus étroit), le plateau se réduit au milieu des deux bords :
/// c'est alors la vraie « largeur à mi-hauteur » qui est rendue.
fn plateau_level(smooth: &[f64], peak: usize, edge: usize, neighbour: Option<(usize, usize)>, length: usize) -> f64 {
    let last = smooth.len() as isize - 1;
    let (peak, edge) = (peak as isize, edge as isize);
    let dir = if edge >= peak { 1 } else { -1 };
    let start = peak + (PLATEAU_START * (edge - peak) as f64).round() as isize;
    // Dernier indice utilisable, en s'éloignant du bord.
    let end = match neighbour {
        Some((np, ne)) => np as isize + (PLATEAU_START * (ne as isize - np as isize) as f64).round() as isize,
        None => if dir > 0 { last } else { 0 },
    };
    let (from, to) = if (end - start) * dir >= 0 {
        (start, start + dir * ((end - start).abs().min(length as isize)))
    } else {
        // Pas de plateau : le milieu entre les deux bords (ou le bout du profil).
        let mid = neighbour.map_or(end, |(np, _)| (peak + np as isize) / 2);
        (mid - 1, mid + 1)
    };
    let (from, to) = (from.min(to).clamp(0, last) as usize, from.max(to).clamp(0, last) as usize);
    median(smooth[from..=to].to_vec())
}

/// Franchissement du niveau `half` le plus proche de `near` (échantillons),
/// cherché dans la transition `jl..=jr`, par interpolation linéaire.
fn crossing(smooth: &[f64], half: f64, jl: usize, jr: usize, near: f64) -> Option<f64> {
    let (from, to) = (jl.saturating_sub(1), (jr + 1).min(smooth.len() - 1));
    let mut best: Option<f64> = None;
    for j in from..to {
        let (u, v) = (smooth[j] - half, smooth[j + 1] - half);
        if u == v || u * v > 0.0 {
            continue;
        }
        let x = j as f64 + u / (u - v);
        if best.is_none_or(|b| (x - near).abs() < (b - near).abs()) {
            best = Some(x);
        }
    }
    best
}

/// Bords retenus : les plus proches des positions demandées, sinon les deux
/// plus marqués. Indices croissants.
fn choose(edges: &[Edge], pick: Option<[f64; 2]>) -> Option<[usize; 2]> {
    if edges.len() < 2 {
        return None;
    }
    let nearest = |pos: f64, skip: Option<usize>| {
        (0..edges.len()).filter(|i| Some(*i) != skip).min_by(|i, j| (edges[*i].pos - pos).abs().total_cmp(&(edges[*j].pos - pos).abs()))
    };
    let (i, j) = match pick {
        Some([p, q]) => {
            let i = nearest(p, None)?;
            (i, nearest(q, Some(i))?)
        }
        None => {
            let mut order: Vec<usize> = (0..edges.len()).collect();
            order.sort_by(|a, b| edges[*b].strength.total_cmp(&edges[*a].strength));
            (order[0], order[1])
        }
    };
    Some([i.min(j), i.max(j)])
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Fonction de répartition de la loi normale (Abramowitz et Stegun 7.1.26, erreur < 1,5e-7).
    pub fn phi(x: f64) -> f64 {
        let z = x.abs() / std::f64::consts::SQRT_2;
        let t = 1.0 / (1.0 + 0.3275911 * z);
        let poly = t * (0.254829592 + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
        let erf = 1.0 - poly * (-z * z).exp();
        if x >= 0.0 { 0.5 * (1.0 + erf) } else { 0.5 * (1.0 - erf) }
    }

    /// Barre claire de bords `e0`, `e1` (pixels), floutée par une gaussienne `sigma`.
    fn bar(n: usize, step: f64, e0: f64, e1: f64, sigma: f64) -> Vec<f64> {
        (0..n).map(|i| i as f64 * step).map(|x| 0.2 + 0.6 * (phi((x - e0) / sigma) - phi((x - e1) / sigma))).collect()
    }

    #[test]
    fn deux_bords_au_centieme_de_pixel() {
        // Bords non entiers, flou de 1 à 4 px : les deux méthodes retrouvent
        // les positions à mieux que 0,01 px sur un profil sans bruit.
        for method in [EdgeMethod::HalfHeight, EdgeMethod::Gradient] {
            for sigma in [1.0, 2.0, 4.0] {
                for phase in [0.0, 0.13, 0.37, 0.5, 0.81] {
                    let (e0, e1) = (30.0 + phase, 71.3 + 2.0 * phase);
                    let a = analyze(&bar(240, 0.5, e0, e1, sigma), 0.5, method, None, None);
                    let r = a.result().unwrap_or_else(|| panic!("{method:?} σ={sigma} : pas de bords"));
                    assert_eq!(a.edges.len(), 2);
                    assert!(a.edges[0].rising && !a.edges[1].rising);
                    assert!((r.edges[0] - e0).abs() < 0.01 && (r.edges[1] - e1).abs() < 0.01, "{method:?} σ={sigma} φ={phase} : {:?}", r.edges);
                    assert!((r.width_px() - (e1 - e0)).abs() < 0.01);
                    assert!((a.edges[0].low - 0.2).abs() < 0.01 && (a.edges[0].high - 0.8).abs() < 0.01);
                }
            }
        }
    }

    #[test]
    fn seuil_et_choix_des_bords() {
        // Une barre marquée et une marche faible (15 % de la barre).
        let mut v = bar(400, 0.5, 40.0, 90.0, 1.5);
        for (i, x) in v.iter_mut().enumerate() {
            *x += 0.09 * phi((i as f64 * 0.5 - 150.0) / 1.5);
        }
        // Seuil automatique (25 %) : la marche faible est ignorée.
        let auto = analyze(&v, 0.5, EdgeMethod::HalfHeight, None, None);
        assert_eq!(auto.edges.len(), 2);
        assert_eq!(auto.threshold, AUTO_THRESHOLD);
        // Seuil abaissé : elle apparaît, mais les deux plus marqués restent retenus.
        let low = analyze(&v, 0.5, EdgeMethod::HalfHeight, Some(0.05), None);
        assert_eq!(low.edges.len(), 3);
        assert_eq!(low.chosen, Some([0, 1]));
        assert!((low.edges[2].pos - 150.0).abs() < 0.02);
        // Clic près de la marche : elle remplace le bord retenu le plus proche (90).
        let pick = low.pick_at(148.0).unwrap();
        assert!(low.pick_at(41.0).is_none());
        let picked = analyze(&v, 0.5, EdgeMethod::HalfHeight, Some(0.05), Some(pick));
        assert_eq!(picked.chosen, Some([0, 2]));
        assert!((picked.result().unwrap().width_px() - 110.0).abs() < 0.03);
        // Seuil très haut ou profil plat : pas de mesure.
        assert!(analyze(&[0.5; 100], 0.5, EdgeMethod::Gradient, None, None).result().is_none());
        assert!(analyze(&v[..4], 0.5, EdgeMethod::Gradient, None, None).result().is_none());
    }

    #[test]
    fn trait_etroit_largeur_a_mi_hauteur() {
        // Trait plus étroit que le flou : les plateaux n'existent plus. La
        // mi-hauteur rend alors la largeur à mi-hauteur de la tache
        // (2,355 σ pour une gaussienne pure), pas la largeur du trait.
        let sigma = 3.0;
        let v = bar(240, 0.5, 59.5, 60.5, sigma);
        let a = analyze(&v, 0.5, EdgeMethod::HalfHeight, None, None);
        let w = a.result().unwrap().width_px();
        assert!((w - 2.355 * (sigma * sigma + 1.0 / 12.0 + LEVEL_SIGMA * LEVEL_SIGMA).sqrt()).abs() < 0.1, "{w}");
    }

    #[test]
    fn inclinaison_et_ancien_projet() {
        let r = ProfileResult { edges: [10.0, 30.0], tilt: 60.0 };
        assert!((r.width_px() - 10.0).abs() < 1e-9);
        // Réglages absents du fichier : valeurs par défaut.
        let s: Shape = serde_json::from_str(r#"{"Profile":{"a":{"x":0.0,"y":0.0},"b":{"x":10.0,"y":0.0},"params":{},"result":null}}"#).unwrap();
        assert_eq!(s, Shape::profile(Pt::new(0., 0.), Pt::new(10., 0.)));
        let s = Shape::Profile { a: Pt::new(0., 0.), b: Pt::new(0., 40.), params: ProfileParams::default(), result: Some(r) };
        assert_eq!(s.profile_edges(), Some([Pt::new(0., 10.), Pt::new(0., 30.)]));
        assert_eq!(s.profile_band(), Some((DEFAULT_BAND / 2.0, Pt::new(-1.0, 0.0))));
        assert_eq!(serde_json::from_str::<Shape>(&serde_json::to_string(&s).unwrap()).unwrap(), s);
    }
}

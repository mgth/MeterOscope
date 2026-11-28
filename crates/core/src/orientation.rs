//! Orientation de l'image de travail par rapport à l'image chargée : quarts
//! de tour et miroirs cumulés (les huit éléments du groupe du carré).
//!
//! Elle fait partie de l'état annulable du document : après une annulation,
//! l'application compare l'orientation du document à celle de ses pixels et
//! leur applique la différence ([`Orientation::to`]), de sorte que l'image,
//! les mesures et le redressement reviennent ensemble.

use serde::{Deserialize, Serialize};

/// Un miroir horizontal éventuel, **puis** `quarters` quarts de tour horaires.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Orientation {
    quarters: u8,
    mirrored: bool,
}

impl Orientation {
    pub const IDENTITY: Self = Self { quarters: 0, mirrored: false };
    /// Quart de tour horaire.
    pub const ROTATE_RIGHT: Self = Self { quarters: 1, mirrored: false };
    /// Quart de tour anti-horaire.
    pub const ROTATE_LEFT: Self = Self { quarters: 3, mirrored: false };
    pub const FLIP_HORIZONTAL: Self = Self { quarters: 0, mirrored: true };
    /// Miroir vertical = miroir horizontal puis demi-tour.
    pub const FLIP_VERTICAL: Self = Self { quarters: 2, mirrored: true };

    /// Nombre de quarts de tour horaires (0 à 3), appliqués après le miroir.
    pub fn quarters(self) -> u8 {
        self.quarters
    }

    /// Miroir horizontal appliqué en premier.
    pub fn mirrored(self) -> bool {
        self.mirrored
    }

    /// `self`, puis `next`.
    pub fn then(self, next: Self) -> Self {
        // Un miroir traversé par une rotation en inverse le sens : H·Rᵇ = R⁻ᵇ·H.
        let carried = if next.mirrored { (4 - self.quarters) % 4 } else { self.quarters };
        Self { quarters: (next.quarters + carried) % 4, mirrored: next.mirrored ^ self.mirrored }
    }

    pub fn inverse(self) -> Self {
        // Une orientation avec miroir est une réflexion : elle est son propre inverse.
        if self.mirrored { self } else { Self { quarters: (4 - self.quarters) % 4, mirrored: false } }
    }

    /// Ce qu'il faut appliquer à une image orientée `self` pour l'amener à `target`.
    pub fn to(self, target: Self) -> Self {
        self.inverse().then(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEPS: [Orientation; 4] =
        [Orientation::ROTATE_RIGHT, Orientation::ROTATE_LEFT, Orientation::FLIP_HORIZONTAL, Orientation::FLIP_VERTICAL];

    /// Les huit orientations, atteintes depuis l'identité.
    fn all() -> Vec<Orientation> {
        let mut seen = vec![Orientation::IDENTITY];
        let mut i = 0;
        while i < seen.len() {
            for s in STEPS {
                let o = seen[i].then(s);
                if !seen.contains(&o) {
                    seen.push(o);
                }
            }
            i += 1;
        }
        seen
    }

    #[test]
    fn group_laws() {
        let all = all();
        assert_eq!(all.len(), 8);
        let id = Orientation::IDENTITY;
        for &a in &all {
            assert_eq!((a.then(id), id.then(a)), (a, a));
            assert_eq!((a.then(a.inverse()), a.inverse().then(a)), (id, id));
            for &b in &all {
                assert_eq!(a.then(a.to(b)), b);
                for &c in &all {
                    assert_eq!(a.then(b).then(c), a.then(b.then(c)));
                }
            }
        }
        let (r, l, h, v) = (STEPS[0], STEPS[1], STEPS[2], STEPS[3]);
        assert_eq!((r.then(l), h.then(h), v.then(v)), (id, id, id));
        assert_eq!(r.then(r), h.then(v));
        assert_eq!(r.then(r).then(r), l);
        // Miroir puis quart de tour ≠ quart de tour puis miroir.
        assert_ne!(h.then(r), r.then(h));
        assert_eq!(h.then(r), r.then(v));
    }
}

use crate::calibration::Calibration;
use crate::measure::{Measurement, Quantity, Shape, ShapeKind, PALETTE};
use crate::stats::Stats;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const UNDO_LIMIT: usize = 200;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Snapshot {
    measurements: Vec<Measurement>,
    calibration: Calibration,
}

/// L'ensemble des mesures posées sur une image, avec étalonnage,
/// sélection et historique annuler/rétablir.
///
/// Règle d'édition : appeler [`Document::checkpoint`] **une fois avant**
/// toute modification directe (`get_mut`, `calibration`, `measurements`),
/// p. ex. au début d'un glisser. `add`, `remove` et `clear` le font eux-mêmes.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Document {
    pub measurements: Vec<Measurement>,
    pub calibration: Calibration,
    /// Identifiant de la mesure sélectionnée.
    #[serde(skip)]
    pub selected: Option<u64>,
    next_id: u64,
    counters: BTreeMap<ShapeKind, u32>,
    #[serde(skip)]
    undo: Vec<Snapshot>,
    #[serde(skip)]
    redo: Vec<Snapshot>,
    /// Incrémenté à chaque modification enregistrée (pour « modifié non sauvegardé »).
    #[serde(skip)]
    revision: u64,
}

impl Document {
    pub fn new() -> Self {
        Self::default()
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot { measurements: self.measurements.clone(), calibration: self.calibration.clone() }
    }

    /// Enregistre l'état courant dans l'historique. À appeler avant une modification.
    pub fn checkpoint(&mut self) {
        self.undo.push(self.snapshot());
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.revision += 1;
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn restore(&mut self, s: Snapshot) {
        self.measurements = s.measurements;
        self.calibration = s.calibration;
        if self.selected.is_some_and(|id| self.get(id).is_none()) {
            self.selected = None;
        }
        self.revision += 1;
    }

    pub fn undo(&mut self) -> bool {
        let Some(s) = self.undo.pop() else { return false };
        self.redo.push(self.snapshot());
        self.restore(s);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(s) = self.redo.pop() else { return false };
        self.undo.push(self.snapshot());
        self.restore(s);
        true
    }

    /// Ajoute une mesure (nom et couleur automatiques), la sélectionne et renvoie son id.
    pub fn add(&mut self, shape: Shape) -> u64 {
        self.checkpoint();
        self.next_id += 1;
        let id = self.next_id;
        let kind = shape.kind();
        let n = self.counters.entry(kind).or_insert(0);
        *n += 1;
        let color = PALETTE[ShapeKind::ALL.iter().position(|k| *k == kind).unwrap_or(0) % PALETTE.len()];
        self.measurements.push(Measurement { id, name: format!("{} {}", kind.label(), n), shape, color, visible: true });
        self.selected = Some(id);
        id
    }

    pub fn remove(&mut self, id: u64) -> bool {
        if self.get(id).is_none() {
            return false;
        }
        self.checkpoint();
        self.measurements.retain(|m| m.id != id);
        if self.selected == Some(id) {
            self.selected = None;
        }
        true
    }

    /// Supprime toutes les mesures (l'étalonnage est conservé).
    pub fn clear(&mut self) {
        if self.measurements.is_empty() {
            return;
        }
        self.checkpoint();
        self.measurements.clear();
        self.selected = None;
    }

    pub fn get(&self, id: u64) -> Option<&Measurement> {
        self.measurements.iter().find(|m| m.id == id)
    }

    /// Accès mutable ; penser à [`Document::checkpoint`] avant de modifier.
    pub fn get_mut(&mut self, id: u64) -> Option<&mut Measurement> {
        self.measurements.iter_mut().find(|m| m.id == id)
    }

    pub fn selected(&self) -> Option<&Measurement> {
        self.selected.and_then(|id| self.get(id))
    }

    /// Nombre de marqueurs (comptage).
    pub fn marker_count(&self) -> usize {
        self.measurements.iter().filter(|m| m.shape.kind() == ShapeKind::Marker).count()
    }

    /// Statistiques par grandeur sur la valeur *principale* des mesures
    /// d'un même type : (type, grandeur, unité, stats).
    pub fn summary(&self) -> Vec<(ShapeKind, Quantity, String, Stats)> {
        let mut groups: BTreeMap<ShapeKind, (Quantity, String, Vec<f64>)> = BTreeMap::new();
        for m in &self.measurements {
            if let Some(v) = m.shape.values(&self.calibration).into_iter().next() {
                groups.entry(m.shape.kind()).or_insert_with(|| (v.quantity, v.unit.clone(), vec![])).2.push(v.value);
            }
        }
        groups
            .into_iter()
            .filter_map(|(k, (q, u, vals))| Stats::of(&vals).map(|s| (k, q, u, s)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Pt;

    #[test]
    fn add_undo_redo() {
        meteroscope_i18n::set_language("fr");
        let mut d = Document::new();
        let a = d.add(Shape::Length { a: Pt::new(0., 0.), b: Pt::new(3., 4.) });
        let b = d.add(Shape::Length { a: Pt::new(0., 0.), b: Pt::new(6., 8.) });
        assert_eq!(d.get(b).unwrap().name, "Longueur 2");
        assert_eq!(d.selected, Some(b));
        let s = d.summary();
        assert_eq!((s[0].3.n, s[0].3.mean), (2, 7.5));
        assert!(d.remove(a));
        assert_eq!(d.measurements.len(), 1);
        assert!(d.undo());
        assert_eq!(d.measurements.len(), 2);
        assert!(d.undo() && d.undo() && !d.undo());
        assert!(d.measurements.is_empty() && d.selected.is_none());
        assert!(d.redo() && d.redo());
        assert_eq!(d.measurements.len(), 2);
    }
}

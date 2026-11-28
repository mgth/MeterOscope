//! Groupes de mesures et actions sur plusieurs mesures à la fois.
//!
//! Le groupe est un simple nom porté par chaque mesure ([`Measurement::group`],
//! vide : sans groupe). Chaque action ci-dessous est un seul pas d'annulation,
//! et ne fait rien (ni point d'annulation) si elle ne change rien.

use crate::document::Document;
use crate::measure::{Measurement, Quantity, ShapeKind};
use crate::stats::Stats;
use std::collections::BTreeMap;

/// Statistiques d'un type de mesure : (type, grandeur, unité, stats).
pub type KindStats = (ShapeKind, Quantity, String, Stats);

impl Document {
    /// Noms des groupes utilisés, triés, sans doublon (le groupe vide n'en est pas un).
    pub fn groups(&self) -> Vec<String> {
        let mut v: Vec<String> = self.measurements.iter().map(|m| m.group.clone()).filter(|g| !g.is_empty()).collect();
        v.sort_by_key(|g| g.to_lowercase());
        v.dedup();
        v
    }

    /// Applique `f` aux mesures `ids` qu'il changerait, avec un seul point
    /// d'annulation. Renvoie `true` si quelque chose a changé.
    fn update_many(&mut self, ids: &[u64], changes: impl Fn(&Measurement) -> bool, f: impl Fn(&mut Measurement)) -> bool {
        let touched: Vec<u64> = ids.iter().copied().filter(|id| self.get(*id).is_some_and(&changes)).collect();
        if touched.is_empty() {
            return false;
        }
        self.checkpoint();
        for id in touched {
            if let Some(m) = self.get_mut(id) {
                f(m);
            }
        }
        true
    }

    /// Range les mesures `ids` dans le groupe `group` (espaces de bord retirés ; vide : aucun groupe).
    pub fn set_group(&mut self, ids: &[u64], group: &str) -> bool {
        let g = group.trim().to_owned();
        self.update_many(ids, |m| m.group != g, |m| m.group = g.clone())
    }

    pub fn set_color(&mut self, ids: &[u64], color: [u8; 3]) -> bool {
        self.update_many(ids, |m| m.color != color, |m| m.color = color)
    }

    pub fn set_visible(&mut self, ids: &[u64], visible: bool) -> bool {
        self.update_many(ids, |m| m.visible != visible, |m| m.visible = visible)
    }

    /// Supprime les mesures `ids` en une fois.
    pub fn remove_many(&mut self, ids: &[u64]) -> bool {
        if !ids.iter().any(|id| self.get(*id).is_some()) {
            return false;
        }
        self.checkpoint();
        self.measurements.retain(|m| !ids.contains(&m.id));
        if self.selected.is_some_and(|id| ids.contains(&id)) {
            self.selected = None;
        }
        true
    }

    /// Identifiants des mesures du groupe `group` ("" : sans groupe).
    pub fn group_members(&self, group: &str) -> Vec<u64> {
        self.measurements.iter().filter(|m| m.group == group).map(|m| m.id).collect()
    }

    /// Statistiques par groupe puis par type, comme [`Document::summary`] :
    /// groupes triés, « sans groupe » ("") en dernier, groupes sans valeur omis.
    pub fn summary_by_group(&self) -> Vec<(String, Vec<KindStats>)> {
        let mut names = self.groups();
        names.push(String::new());
        names
            .into_iter()
            .filter_map(|g| {
                let mut sub = Document::new();
                sub.measurements = self.measurements.iter().filter(|m| m.group == g).cloned().collect();
                sub.calibration = self.calibration.clone();
                let s = sub.summary();
                (!s.is_empty()).then_some((g, s))
            })
            .collect()
    }

    /// Mesures dans l'ordre de la liste regroupée : par groupe (ordre de
    /// [`Document::groups`]), puis sans groupe ; ordre de création dans chaque groupe.
    pub fn grouped_order(&self) -> Vec<(String, Vec<u64>)> {
        let mut by: BTreeMap<String, Vec<u64>> = BTreeMap::new();
        for m in &self.measurements {
            by.entry(m.group.clone()).or_default().push(m.id);
        }
        let mut out: Vec<(String, Vec<u64>)> = Vec::new();
        for g in self.groups() {
            let ids = by.remove(&g).unwrap_or_default();
            out.push((g, ids));
        }
        if let Some(ids) = by.remove("") {
            out.push((String::new(), ids));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use crate::{Document, Pt, Shape};

    fn len(d: &mut Document, l: f64) -> u64 {
        d.add(Shape::Length { a: Pt::new(0., 0.), b: Pt::new(l, 0.) })
    }

    #[test]
    fn groupes_et_actions_groupees() {
        meteroscope_i18n::set_language("fr");
        let mut d = Document::new();
        let (a, b, c) = (len(&mut d, 10.), len(&mut d, 20.), len(&mut d, 60.));
        let m = d.add(Shape::Marker { p: Pt::new(1., 1.) });
        assert!(d.groups().is_empty());
        assert!(d.set_group(&[a, b], " Pièce B "));
        assert!(!d.set_group(&[a, b], "Pièce B"), "rien ne change : pas de point d'annulation");
        assert!(d.set_group(&[c], "pièce a"));
        assert_eq!(d.groups(), vec!["pièce a", "Pièce B"]);
        assert_eq!(d.get(a).unwrap().group, "Pièce B");
        // Un seul pas d'annulation pour deux mesures.
        assert!(d.undo());
        assert_eq!(d.get(c).unwrap().group, "");
        assert!(d.undo());
        assert!(d.get(a).unwrap().group.is_empty() && d.get(b).unwrap().group.is_empty());
        assert!(d.redo() && d.redo());

        let s = d.summary_by_group();
        assert_eq!(s.iter().map(|(g, _)| g.as_str()).collect::<Vec<_>>(), vec!["pièce a", "Pièce B"]);
        assert_eq!((s[1].1[0].3.n, s[1].1[0].3.mean), (2, 15.0));
        // Le marqueur (sans valeur) n'a pas de statistiques, mais il est dans la liste.
        assert_eq!(d.grouped_order(), vec![("pièce a".into(), vec![c]), ("Pièce B".into(), vec![a, b]), (String::new(), vec![m])]);

        assert!(d.set_color(&[a, c], [1, 2, 3]) && d.get(c).unwrap().color == [1, 2, 3]);
        assert!(d.set_visible(&d.group_members("Pièce B"), false));
        assert!(!d.get(a).unwrap().visible && !d.get(b).unwrap().visible && d.get(c).unwrap().visible);
        d.selected = Some(b);
        assert!(d.remove_many(&[a, b]));
        assert_eq!((d.measurements.len(), d.selected), (2, None));
        assert!(d.undo() && d.measurements.len() == 4);
        assert!(!d.remove_many(&[999]));
    }

    #[test]
    fn groupe_dans_le_projet() {
        meteroscope_i18n::set_language("fr");
        let mut d = Document::new();
        let a = len(&mut d, 10.);
        len(&mut d, 20.);
        d.set_group(&[a], "Lot 3");
        let json = serde_json::to_string(&d.measurements).unwrap();
        // Sans groupe : le champ n'est pas écrit.
        assert_eq!(json.matches("\"group\"").count(), 1);
        let back: Vec<crate::Measurement> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, d.measurements);
        // Mesure d'un ancien projet (sans champ « group ») : sans groupe.
        let old = r#"{"id":1,"name":"Longueur 1","shape":{"Length":{"a":{"x":0.0,"y":0.0},"b":{"x":3.0,"y":4.0}}},"color":[255,214,10],"visible":true}"#;
        let m: crate::Measurement = serde_json::from_str(old).unwrap();
        assert_eq!(m.group, "");
    }
}

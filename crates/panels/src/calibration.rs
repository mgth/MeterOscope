//! Boîte de dialogue « Définir l'échelle ».

use meteroscope_i18n::tr;
use crate::format::{format_significant, parse_number, scale_texts_raw};
use egui::text::CCursorRange;
use egui::{Align, Button, ComboBox, Context, Id, Key, Layout, Modal, RichText, TextEdit, vec2};
use meteroscope_core::Document;
use meteroscope_core::measure::format_number;

/// Unités proposées dans la liste.
pub const UNITS: [&str; 5] = ["µm", "mm", "cm", "m", "in"];

/// Résultat de la lecture du formulaire.
#[derive(Clone, Debug, PartialEq)]
pub enum CalibrationInput {
    /// Longueur réelle et unité valides.
    Valid { real_len: f64, unit: String },
    /// Rien n'est encore saisi.
    Empty,
    /// Le texte n'est pas un nombre strictement positif.
    BadNumber,
    /// L'unité est vide.
    NoUnit,
}

/// Analyse la saisie (longueur réelle + unité).
pub fn read_calibration_input(value: &str, unit: &str) -> CalibrationInput {
    if value.trim().is_empty() {
        return CalibrationInput::Empty;
    }
    let Some(real_len) = parse_number(value).filter(|v| *v > 0.0) else {
        return CalibrationInput::BadNumber;
    };
    let unit = unit.trim();
    if unit.is_empty() {
        return CalibrationInput::NoUnit;
    }
    CalibrationInput::Valid { real_len, unit: unit.to_owned() }
}

/// Boîte de dialogue « Définir l'échelle » : l'utilisateur saisit la longueur
/// réelle du segment tracé et son unité.
pub struct CalibrationDialog {
    /// Longueur du segment tracé (pixels) ; `Some` quand la boîte est ouverte.
    px_len: Option<f64>,
    /// Texte du champ « Longueur réelle ».
    value: String,
    /// Unité choisie (mémorisée d'une ouverture à l'autre).
    unit: String,
    /// Unité saisie librement plutôt que choisie dans la liste.
    custom_unit: bool,
    /// Dernière longueur validée, proposée à l'ouverture suivante.
    last_value: Option<f64>,
    /// Donner le focus au champ à la prochaine frame.
    focus: bool,
    /// Une validation a été tentée avec une saisie invalide.
    tried: bool,
    /// Première frame après l'ouverture : le clavier est ignoré.
    fresh: bool,
}

impl Default for CalibrationDialog {
    fn default() -> Self {
        Self {
            px_len: None,
            value: String::new(),
            unit: "mm".to_owned(),
            custom_unit: false,
            last_value: None,
            focus: false,
            tried: false,
            fresh: false,
        }
    }
}

impl CalibrationDialog {
    /// Ouvre la boîte pour un segment de `px_len` pixels. Un segment de
    /// longueur nulle ou invalide est ignoré (la boîte reste fermée).
    pub fn open(&mut self, px_len: f64) {
        if !(px_len.is_finite() && px_len > 0.0) {
            return;
        }
        self.px_len = Some(px_len);
        self.value = self.last_value.map(|v| format_significant(v, 8)).unwrap_or_default();
        self.focus = true;
        self.tried = false;
        self.fresh = true;
    }

    pub fn is_open(&self) -> bool {
        self.px_len.is_some()
    }

    /// Longueur du segment en cours d'étalonnage, en pixels.
    pub fn px_len(&self) -> Option<f64> {
        self.px_len
    }

    /// Unité mémorisée (dernière utilisée).
    pub fn unit(&self) -> &str {
        &self.unit
    }

    /// Propose `unit` à la prochaine ouverture (unité retenue d'un lancement
    /// à l'autre). Une unité vide est ignorée.
    pub fn set_unit(&mut self, unit: &str) {
        let unit = unit.trim();
        if !unit.is_empty() {
            self.custom_unit = !UNITS.contains(&unit);
            self.unit = unit.to_owned();
        }
    }

    /// Ferme la boîte sans rien appliquer.
    pub fn cancel(&mut self) {
        self.px_len = None;
    }

    /// Remplit le formulaire (utile aux tests et à une saisie programmée).
    pub fn set_input(&mut self, value: &str, unit: &str) {
        self.value = value.to_owned();
        self.custom_unit = !UNITS.contains(&unit);
        self.unit = unit.to_owned();
    }

    /// Valide le formulaire : applique l'échelle (avec `checkpoint`) et ferme.
    /// Renvoie `false` (boîte laissée ouverte) si la saisie est invalide.
    pub fn validate(&mut self, doc: &mut Document) -> bool {
        let Some(px) = self.px_len else { return false };
        match read_calibration_input(&self.value, &self.unit) {
            CalibrationInput::Valid { real_len, unit } => {
                doc.checkpoint();
                if !doc.calibration.set_from_reference(px, real_len, &unit) {
                    // Impossible ici (valeurs déjà vérifiées) : on annule le point d'historique.
                    doc.undo();
                    self.tried = true;
                    return false;
                }
                self.last_value = Some(real_len);
                self.unit = unit;
                self.px_len = None;
                true
            }
            _ => {
                self.tried = true;
                self.focus = true;
                false
            }
        }
    }

    /// Affiche la boîte si elle est ouverte ; applique l'échelle au document
    /// à la validation (avec `checkpoint`). Renvoie `true` quand elle vient
    /// d'être fermée (validée ou annulée).
    pub fn show(&mut self, ctx: &Context, doc: &mut Document) -> bool {
        let Some(px) = self.px_len else { return false };
        let mut validate = false;
        let mut cancel = false;

        let modal = Modal::new(Id::unique("meteroscope_dialogue_echelle")).show(ctx, |ui| {
            ui.set_width(330.0);
            ui.heading(tr!("calib.title"));
            ui.add_space(4.0);
            ui.label(RichText::new(tr!("calib.segment", px = format_number(px))).weak());
            ui.add_space(10.0);
            ui.label(tr!("calib.real_length"));
            ui.add_space(2.0);

            ui.horizontal(|ui| {
                let mut output = TextEdit::singleline(&mut self.value)
                    .id(Id::unique("meteroscope_echelle_valeur"))
                    .desired_width(120.0)
                    .hint_text(tr!("calib.value_hint"))
                    .show(ui);
                let resp = output.response.response.clone();
                if self.focus {
                    resp.request_focus();
                    output.state.cursor.set_char_range(Some(CCursorRange::select_all_str(&self.value)));
                    output.state.store(ui.ctx(), resp.id);
                    self.focus = false;
                }

                let shown = if self.custom_unit { tr!("calib.other_unit") } else { self.unit.as_str() };
                let combo = ComboBox::from_id_salt("meteroscope_echelle_unite").selected_text(shown.to_owned()).width(72.0).show_ui(
                    ui,
                    |ui| {
                        for u in UNITS {
                            if ui.selectable_label(!self.custom_unit && self.unit == u, u).clicked() {
                                self.unit = u.to_owned();
                                self.custom_unit = false;
                            }
                        }
                        if ui.selectable_label(self.custom_unit, tr!("calib.other_unit")).clicked() && !self.custom_unit {
                            self.custom_unit = true;
                            self.unit.clear();
                        }
                    },
                );
                combo.response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, tr!("calib.unit")));
                if self.custom_unit {
                    ui.add(TextEdit::singleline(&mut self.unit).desired_width(64.0).hint_text(tr!("calib.unit_hint")));
                }
            });

            // Aperçu en direct.
            ui.add_space(10.0);
            let input = read_calibration_input(&self.value, &self.unit);
            let error = ui.visuals().error_fg_color;
            match &input {
                CalibrationInput::Valid { real_len, unit } => {
                    let (main, inverse) = scale_texts_raw(real_len / px, unit);
                    ui.label(RichText::new(main).size(15.0).strong());
                    ui.label(RichText::new(inverse).weak());
                }
                CalibrationInput::Empty => {
                    let t = RichText::new(tr!("calib.enter_length"));
                    ui.label(if self.tried { t.color(error) } else { t.weak() });
                    ui.label("");
                }
                CalibrationInput::BadNumber => {
                    ui.label(RichText::new(tr!("calib.invalid_number")).color(error));
                    ui.label("");
                }
                CalibrationInput::NoUnit => {
                    ui.label(RichText::new(tr!("calib.need_unit")).color(error));
                    ui.label("");
                }
            }

            ui.add_space(12.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let valid = matches!(input, CalibrationInput::Valid { .. });
                let sel = ui.visuals().selection;
                let ok = Button::new(RichText::new(tr!("calib.apply")).strong().color(sel.stroke.color))
                    .fill(sel.bg_fill)
                    .min_size(vec2(90.0, 30.0));
                if ui.add_enabled(valid, ok).on_hover_text(tr!("calib.apply_tip")).clicked() {
                    validate = true;
                }
                if ui.add(Button::new(tr!("common.cancel")).min_size(vec2(90.0, 30.0))).on_hover_text(tr!("key.escape")).clicked() {
                    cancel = true;
                }
            });
        });

        // Clavier : Entrée valide, Échap annule (sauf si une liste déroulante est ouverte).
        let fresh = std::mem::take(&mut self.fresh);
        if !modal.any_popup_open && !fresh {
            if ctx.input(|i| i.key_pressed(Key::Enter)) {
                validate = true;
            }
            if ctx.input(|i| i.key_pressed(Key::Escape)) {
                cancel = true;
            }
        }

        if cancel {
            self.px_len = None;
            return true;
        }
        validate && self.validate(doc)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lecture_du_formulaire() {
        meteroscope_i18n::set_language("fr");
        assert_eq!(
            read_calibration_input("12,5", " mm "),
            CalibrationInput::Valid { real_len: 12.5, unit: "mm".to_owned() }
        );
        assert_eq!(read_calibration_input("  ", "mm"), CalibrationInput::Empty);
        assert_eq!(read_calibration_input("0", "mm"), CalibrationInput::BadNumber);
        assert_eq!(read_calibration_input("-2", "mm"), CalibrationInput::BadNumber);
        assert_eq!(read_calibration_input("douze", "mm"), CalibrationInput::BadNumber);
        assert_eq!(read_calibration_input("3", " "), CalibrationInput::NoUnit);
    }

    #[test]
    fn validation_et_memoire() {
        meteroscope_i18n::set_language("fr");
        let mut doc = Document::new();
        let mut dlg = CalibrationDialog::default();
        dlg.open(0.0);
        assert!(!dlg.is_open(), "segment nul ignoré");

        dlg.open(200.0);
        assert!(dlg.is_open());
        dlg.set_input("abc", "mm");
        assert!(!dlg.validate(&mut doc));
        assert!(dlg.is_open() && !doc.calibration.is_calibrated() && !doc.can_undo());

        dlg.set_input("10,0", "cm");
        assert!(dlg.validate(&mut doc));
        assert!(!dlg.is_open());
        let s = doc.calibration.scale.clone().unwrap();
        assert_eq!((s.units_per_px, s.unit.as_str()), (0.05, "cm"));
        assert!(doc.can_undo());

        // Réouverture : dernière unité et dernière valeur proposées.
        dlg.open(50.0);
        assert_eq!(dlg.unit(), "cm");
        assert_eq!(dlg.value, "10");
        dlg.cancel();
        assert!(!dlg.is_open());

        doc.undo();
        assert!(!doc.calibration.is_calibrated());
    }

    #[test]
    fn unite_retenue() {
        let mut dlg = CalibrationDialog::default();
        dlg.set_unit("  ");
        assert_eq!(dlg.unit(), "mm", "unité vide ignorée");
        dlg.set_unit("cm");
        assert_eq!((dlg.unit(), dlg.custom_unit), ("cm", false));
        dlg.set_unit(" pouce ");
        assert_eq!((dlg.unit(), dlg.custom_unit), ("pouce", true));
    }
}

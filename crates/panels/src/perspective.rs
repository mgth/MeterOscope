//! Boîte de dialogue « Corriger la perspective ».

use meteroscope_i18n::tr;
use crate::calibration::UNITS;
use crate::format::parse_number;
use egui::{Align, Button, ComboBox, Context, Id, Key, Layout, Modal, RichText, TextEdit, vec2};
use meteroscope_core::{Document, Pt};

/// Résultat de la lecture du formulaire.
#[derive(Clone, Debug, PartialEq)]
pub enum PerspectiveInput {
    /// Aucune dimension saisie : le rapport d'aspect sera estimé et
    /// l'échelle restera à définir.
    Estimated,
    /// Largeur, hauteur et unité valides : elles fixent le rapport d'aspect
    /// et l'échelle.
    Valid { width: f64, height: f64, unit: String },
    /// Une seule des deux dimensions est saisie.
    Incomplete,
    /// Un texte n'est pas un nombre strictement positif.
    BadNumber,
    /// L'unité est vide.
    NoUnit,
}

/// Analyse la saisie (largeur et hauteur réelles facultatives + unité).
pub fn read_perspective_input(width: &str, height: &str, unit: &str) -> PerspectiveInput {
    let (we, he) = (width.trim().is_empty(), height.trim().is_empty());
    if we && he {
        return PerspectiveInput::Estimated;
    }
    if we || he {
        return PerspectiveInput::Incomplete;
    }
    let number = |s: &str| parse_number(s).filter(|v| *v > 0.0);
    let (Some(width), Some(height)) = (number(width), number(height)) else {
        return PerspectiveInput::BadNumber;
    };
    let unit = unit.trim();
    if unit.is_empty() {
        return PerspectiveInput::NoUnit;
    }
    PerspectiveInput::Valid { width, height, unit: unit.to_owned() }
}

/// Boîte de dialogue « Corriger la perspective » : l'utilisateur vient de
/// cliquer les quatre coins d'un objet rectangulaire et peut en donner les
/// dimensions réelles.
pub struct PerspectiveDialog {
    /// Coins cliqués (image de travail) ; `Some` quand la boîte est ouverte.
    quad: Option<[Pt; 4]>,
    /// Taille de la photo d'origine.
    img: (u32, u32),
    width: String,
    height: String,
    /// Unité choisie (mémorisée d'une ouverture à l'autre).
    unit: String,
    /// Unité saisie librement plutôt que choisie dans la liste.
    custom_unit: bool,
    /// Donner le focus au premier champ à la prochaine frame.
    focus: bool,
    /// Une validation a été tentée avec une saisie invalide.
    tried: bool,
    /// La correction a été refusée (quadrilatère inutilisable).
    refused: bool,
    /// Première frame après l'ouverture : le clavier est ignoré.
    fresh: bool,
}

impl Default for PerspectiveDialog {
    fn default() -> Self {
        Self {
            quad: None,
            img: (0, 0),
            width: String::new(),
            height: String::new(),
            unit: "mm".to_owned(),
            custom_unit: false,
            focus: false,
            tried: false,
            refused: false,
            fresh: false,
        }
    }
}

impl PerspectiveDialog {
    /// Ouvre la boîte pour le quadrilatère `quad` cliqué sur l'image de
    /// travail ; `img` : taille de la photo d'origine.
    pub fn open(&mut self, quad: [Pt; 4], img: (u32, u32)) {
        self.quad = Some(quad);
        self.img = img;
        self.width.clear();
        self.height.clear();
        self.focus = true;
        self.tried = false;
        self.refused = false;
        self.fresh = true;
    }

    pub fn is_open(&self) -> bool {
        self.quad.is_some()
    }

    /// Ferme la boîte sans rien appliquer.
    pub fn cancel(&mut self) {
        self.quad = None;
    }

    /// Remplit le formulaire (utile aux tests et à une saisie programmée).
    pub fn set_input(&mut self, width: &str, height: &str, unit: &str) {
        self.width = width.to_owned();
        self.height = height.to_owned();
        self.custom_unit = !UNITS.contains(&unit);
        self.unit = unit.to_owned();
    }

    /// Valide le formulaire : corrige la perspective du document (un seul
    /// point d'annulation) et ferme. Renvoie `false` (boîte laissée ouverte)
    /// si la saisie est invalide ou le quadrilatère inutilisable.
    pub fn validate(&mut self, doc: &mut Document) -> bool {
        let Some(quad) = self.quad else { return false };
        let real = match read_perspective_input(&self.width, &self.height, &self.unit) {
            PerspectiveInput::Estimated => None,
            PerspectiveInput::Valid { width, height, unit } => Some((width, height, unit)),
            _ => {
                self.tried = true;
                self.focus = true;
                return false;
            }
        };
        if !doc.correct_perspective(quad, self.img, real.as_ref().map(|(w, h, u)| (*w, *h, u.as_str()))) {
            self.refused = true;
            return false;
        }
        if let Some((_, _, unit)) = real {
            self.unit = unit;
        }
        self.quad = None;
        true
    }

    /// Affiche la boîte si elle est ouverte ; corrige la perspective du
    /// document à la validation. Renvoie `true` quand elle vient d'être
    /// fermée (validée ou annulée).
    pub fn show(&mut self, ctx: &Context, doc: &mut Document) -> bool {
        if self.quad.is_none() {
            return false;
        }
        let mut validate = false;
        let mut cancel = false;

        let modal = Modal::new(Id::unique("meteroscope_dialogue_perspective")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.heading(tr!("persp.title"));
            ui.add_space(4.0);
            ui.label(RichText::new(tr!("persp.intro")).weak());
            ui.add_space(10.0);
            ui.label(tr!("persp.real_size"));
            ui.add_space(2.0);

            ui.horizontal(|ui| {
                let w = ui.add(
                    TextEdit::singleline(&mut self.width)
                        .id(Id::unique("meteroscope_perspective_largeur"))
                        .desired_width(84.0)
                        .hint_text(tr!("persp.width")),
                );
                if std::mem::take(&mut self.focus) {
                    w.request_focus();
                }
                ui.label("×");
                ui.add(
                    TextEdit::singleline(&mut self.height)
                        .id(Id::unique("meteroscope_perspective_hauteur"))
                        .desired_width(84.0)
                        .hint_text(tr!("persp.height")),
                );
                let shown = if self.custom_unit { tr!("calib.other_unit") } else { self.unit.as_str() };
                let combo = ComboBox::from_id_salt("meteroscope_perspective_unite").selected_text(shown.to_owned()).width(72.0).show_ui(
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
                    ui.add(TextEdit::singleline(&mut self.unit).desired_width(56.0).hint_text(tr!("calib.unit_hint")));
                }
            });

            // Ce que fera la validation.
            ui.add_space(10.0);
            let input = read_perspective_input(&self.width, &self.height, &self.unit);
            let error = ui.visuals().error_fg_color;
            let (text, bad) = match &input {
                _ if self.refused => (tr!("msg.perspective_invalid"), true),
                PerspectiveInput::Valid { .. } => (tr!("persp.exact"), false),
                PerspectiveInput::Estimated => (tr!("persp.estimated"), false),
                PerspectiveInput::Incomplete => (tr!("persp.incomplete"), self.tried),
                PerspectiveInput::BadNumber => (tr!("calib.invalid_number"), true),
                PerspectiveInput::NoUnit => (tr!("calib.need_unit"), true),
            };
            let text = RichText::new(text);
            ui.label(if bad { text.color(error) } else { text.weak() });

            ui.add_space(12.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let valid = matches!(input, PerspectiveInput::Valid { .. } | PerspectiveInput::Estimated) && !self.refused;
                let sel = ui.visuals().selection;
                let ok = Button::new(RichText::new(tr!("persp.apply")).strong().color(sel.stroke.color))
                    .fill(sel.bg_fill)
                    .min_size(vec2(90.0, 30.0));
                if ui.add_enabled(valid, ok).on_hover_text(tr!("persp.apply_tip")).clicked() {
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
            self.quad = None;
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
        assert_eq!(read_perspective_input(" ", "", "mm"), PerspectiveInput::Estimated);
        assert_eq!(
            read_perspective_input("29,7", " 21 ", " cm "),
            PerspectiveInput::Valid { width: 29.7, height: 21.0, unit: "cm".to_owned() }
        );
        assert_eq!(read_perspective_input("297", "", "mm"), PerspectiveInput::Incomplete);
        assert_eq!(read_perspective_input("", "210", "mm"), PerspectiveInput::Incomplete);
        assert_eq!(read_perspective_input("297", "0", "mm"), PerspectiveInput::BadNumber);
        assert_eq!(read_perspective_input("large", "210", "mm"), PerspectiveInput::BadNumber);
        assert_eq!(read_perspective_input("297", "210", " "), PerspectiveInput::NoUnit);
    }

    #[test]
    fn validation() {
        meteroscope_i18n::set_language("fr");
        let quad = [Pt::new(150., 120.), Pt::new(620., 180.), Pt::new(560., 470.), Pt::new(90., 380.)];
        let mut doc = Document::new();
        let mut dlg = PerspectiveDialog::default();
        assert!(!dlg.validate(&mut doc), "boîte fermée");

        // Saisie incomplète : rien n'est appliqué, la boîte reste ouverte.
        dlg.open(quad, (800, 600));
        dlg.set_input("297", "", "mm");
        assert!(!dlg.validate(&mut doc));
        assert!(dlg.is_open() && doc.perspective.is_none() && !doc.can_undo());

        // Dimensions réelles : rapport d'aspect et échelle.
        dlg.set_input("297", "210", "mm");
        assert!(dlg.validate(&mut doc) && !dlg.is_open());
        let p = doc.perspective.clone().unwrap();
        let (w, h) = p.rect_size();
        assert!((w / h - 297.0 / 210.0).abs() < 1e-9);
        let s = doc.calibration.scale.clone().unwrap();
        assert!((s.units_per_px * w - 297.0).abs() < 1e-9 && s.unit == "mm");

        // Un seul point d'annulation.
        assert!(doc.undo() && !doc.can_undo());
        assert!(doc.perspective.is_none() && !doc.calibration.is_calibrated());

        // Sans dimensions : proportions estimées, échelle à définir.
        dlg.open(quad, (800, 600));
        assert!(dlg.width.is_empty(), "les dimensions ne sont pas reprises d'un objet à l'autre");
        assert!(dlg.validate(&mut doc));
        assert!(doc.perspective.is_some() && !doc.calibration.is_calibrated());

        // Quadrilatère inutilisable : refus, la boîte reste ouverte.
        let mut doc = Document::new();
        dlg.open([quad[0], quad[1], quad[0].lerp(quad[1], 0.5), quad[3]], (800, 600));
        assert!(!dlg.validate(&mut doc) && dlg.is_open() && !doc.can_undo());
        dlg.cancel();
        assert!(!dlg.is_open());
    }
}

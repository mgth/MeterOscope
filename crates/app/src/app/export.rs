//! Exports SVG et rapport PDF (menu « Exporter »).

use super::MeterOscopeApp;
use meteroscope_i18n::tr;
use meteroscope_io as io;

impl MeterOscopeApp {
    /// Image annotée en SVG. `linked` : l'image de fond est liée (son fichier
    /// tel qu'il est sur le disque) au lieu d'être intégrée. Le fond est
    /// l'image de travail (corrigée de la perspective le cas échéant), dans le
    /// repère des mesures ; la photo brute ne peut donc être liée que sans
    /// correction de perspective.
    pub(super) fn export_svg(&mut self, linked: bool) {
        self.sync_orientation();
        self.sync_flat();
        if self.work_image().is_none() {
            return;
        }
        let Some(path) = self.save_dialog(tr!("dialog.export_svg"), tr!("file.measurements_suffix"), "svg") else {
            return;
        };
        let Some(img) = self.work_image() else { return };
        let base = io::adjust(img, self.brightness, self.contrast);
        let background = match (&self.image_path, linked && self.can_link_svg()) {
            (Some(src), true) => io::SvgBackground::Linked(io::link_href(src, &path)),
            _ => io::SvgBackground::Embedded,
        };
        let r = io::export_svg(&base, &self.doc, &background, &path);
        self.report(r, tr!("msg.svg_exported"));
    }

    /// Rapport PDF (A4).
    pub(super) fn export_pdf(&mut self) {
        self.sync_orientation();
        self.sync_flat();
        if self.work_image().is_none() {
            return;
        }
        let Some(path) = self.save_dialog(tr!("dialog.export_pdf"), tr!("file.report_suffix"), "pdf") else {
            return;
        };
        let Some(img) = self.work_image() else { return };
        let base = io::adjust(img, self.brightness, self.contrast);
        let info = io::ReportInfo {
            title: tr!("report.title").to_owned(),
            image_name: self
                .image_path
                .as_ref()
                .and_then(|p| p.file_name())
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            date: io::today(),
        };
        // Filtre de redressement : celui choisi, ou bicubique si c'est la carte graphique.
        let filter = self.filter.unwrap_or(io::RotationFilter::Bicubic);
        let r = io::export_pdf(&base, &self.doc, filter, &info, &path);
        self.report(r, tr!("msg.pdf_exported"));
    }

    /// Le fichier de la photo représente exactement le fond des mesures :
    /// enregistrée telle quelle et sans correction de perspective.
    fn can_link_svg(&self) -> bool {
        self.image_path.is_some() && !self.image_modified() && self.doc.perspective.is_none()
    }

    /// Entrées SVG et PDF du menu d'export.
    pub(super) fn export_menu_items(&mut self, ui: &mut egui::Ui) {
        if ui.button(tr!("toolbar.export_svg")).on_hover_text(tr!("toolbar.export_svg_tip")).clicked() {
            self.export_svg(false);
        }
        let can_link = self.can_link_svg();
        let linked = ui
            .add_enabled(can_link, egui::Button::new(tr!("toolbar.export_svg_linked")))
            .on_hover_text(tr!("toolbar.export_svg_linked_tip"))
            .on_disabled_hover_text(tr!("toolbar.export_svg_linked_off"));
        if linked.clicked() {
            self.export_svg(true);
        }
        if ui.button(tr!("toolbar.export_pdf")).on_hover_text(tr!("toolbar.export_pdf_tip")).clicked() {
            self.export_pdf();
        }
    }
}

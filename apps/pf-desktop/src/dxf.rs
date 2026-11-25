//! Export DXF de type AAMA / ASTM (US-12.1) : l'export est calculé à l'ouverture de la fenêtre
//! (lecture seule, aucune `Command`), le rapport est affiché **avant** l'écriture (pièces,
//! couches, éléments non exportés, remarques) et le fichier n'est écrit qu'après
//! « Enregistrer le DXF… ». Un échec ou un refus ne laisse aucun fichier.

use crate::app::App;
use crate::palette::pal;
use eframe::egui::{self, RichText};
use pf_export::dxf::{export_dxf_with, DxfError, DxfExport, DxfIssue, DxfIssueKind, LayerRole};
use pf_i18n::{tr, tr_args};
use std::path::Path;

/// Fenêtre « Exporter en DXF » : l'export déjà calculé (état d'interface, jamais enregistré).
pub struct DxfDialog {
    pub export: DxfExport,
}

/// Nom d'une couche d'après son rôle.
pub fn layer_label(role: LayerRole) -> String {
    match role {
        LayerRole::Boundary => tr("dxf.layer.boundary"),
        LayerRole::TurnPoints => tr("dxf.layer.turn_points"),
        LayerRole::CurvePoints => tr("dxf.layer.curve_points"),
        LayerRole::Notches => tr("dxf.layer.notches"),
        LayerRole::Grainline => tr("dxf.layer.grainline"),
        LayerRole::InternalLines => tr("dxf.layer.internal_lines"),
        LayerRole::InternalCutouts => tr("dxf.layer.internal_cutouts"),
        LayerRole::Mirror => tr("dxf.layer.mirror"),
        LayerRole::Drill => tr("dxf.layer.drill"),
        LayerRole::Sewing => tr("dxf.layer.sewing"),
        LayerRole::Text => tr("dxf.layer.text"),
        LayerRole::DartLegs => tr("dxf.layer.dart_legs"),
        LayerRole::FoldLine => tr("dxf.layer.fold_line"),
    }
}

/// Phrase d'un point signalé.
pub fn issue_text(i: &DxfIssue) -> String {
    let args = [("piece", i.piece.as_str()), ("count", &i.count.to_string())];
    match i.kind {
        DxfIssueKind::EmptyOutline => tr_args("dxf.issue.empty_outline", &args),
        DxfIssueKind::OutlineUnreadable => tr_args("dxf.issue.outline_unreadable", &args),
        DxfIssueKind::OutlineOpen => tr_args("dxf.issue.outline_open", &args),
        DxfIssueKind::DegenerateOutline => tr_args("dxf.issue.degenerate_outline", &args),
        DxfIssueKind::NotchUnreadable => tr_args("dxf.issue.notch_unreadable", &args),
        DxfIssueKind::GrainUnreadable => tr_args("dxf.issue.grain_unreadable", &args),
        DxfIssueKind::InternalUnreadable => tr_args("dxf.issue.internal_unreadable", &args),
        DxfIssueKind::MarkingUnreadable => tr_args("dxf.issue.marking_unreadable", &args),
        DxfIssueKind::ButtonsUnreadable => tr_args("dxf.issue.buttons_unreadable", &args),
        DxfIssueKind::TextTruncated => tr_args("dxf.issue.text_truncated", &args),
        DxfIssueKind::NoCutContour => tr_args("dxf.issue.no_cut_contour", &args),
        DxfIssueKind::BoundaryNotCut => tr_args("dxf.issue.boundary_not_cut", &args),
        DxfIssueKind::SelfIntersecting => tr_args("dxf.issue.self_intersecting", &args),
        DxfIssueKind::MissingGrain => tr_args("dxf.issue.missing_grain", &args),
        DxfIssueKind::CutContourFailed => tr_args("dxf.issue.cut_contour_failed", &args),
        DxfIssueKind::FoldNotchesFailed => tr_args("dxf.issue.fold_notches_failed", &args),
        DxfIssueKind::DartUnreadable => tr_args("dxf.issue.dart_unreadable", &args),
    }
}

/// Message d'un refus d'export (aucun fichier écrit).
pub fn dxf_error(e: &DxfError) -> String {
    match e {
        DxfError::NoPieces => tr("error.dxf.no_pieces"),
        DxfError::NothingExported(issues) => {
            let detail: Vec<String> = issues.iter().map(issue_text).collect();
            tr_args("error.dxf.nothing", &[("detail", &detail.join("\n"))])
        }
        DxfError::NonFinite => tr("error.dxf.non_finite"),
        DxfError::OutOfRange => tr("error.dxf.range"),
        DxfError::Core(c) => crate::text::core_error(c),
    }
}

impl App {
    /// Menu Fichier › « Exporter … en DXF » : calcule l'export et ouvre la fenêtre de rapport ;
    /// un refus est affiché comme erreur, sans fenêtre ni fichier.
    pub fn open_dxf_export(&mut self) -> bool {
        // US-09.8 : le projet évalué de la taille exportée (le projet lui-même pour la base).
        let project = match self.export_project() {
            Ok(p) => p,
            Err(e) => {
                self.editor.error(crate::print_setup::export_error(&e));
                return false;
            }
        };
        // Même choix couture / coupe que le PDF, le SVG et le HPGL (US-05.4).
        match export_dxf_with(&project, self.output_content.piece_mode) {
            Ok(export) => {
                drop(project);
                self.dxf_dialog = Some(DxfDialog { export });
                true
            }
            Err(e) => {
                drop(project);
                self.editor.error(dxf_error(&e));
                false
            }
        }
    }

    /// Écrit le DXF calculé dans `path`. Renvoie `true` si le fichier est écrit ; un échec
    /// d'écriture ouvre la boîte d'erreur.
    pub fn write_dxf_to(&mut self, path: &Path) -> bool {
        let Some(d) = &self.dxf_dialog else {
            return false;
        };
        let pieces = d.export.report.pieces.len().to_string();
        match std::fs::write(path, d.export.bytes()) {
            Ok(()) => {
                let note = tr_args("export.note.dxf", &[("pieces", &pieces)]);
                self.editor.status.text = tr_args(
                    "export.done",
                    &[
                        ("what", "DXF"),
                        ("note", &note),
                        ("path", &path.display().to_string()),
                    ],
                );
                self.editor.status.error = false;
                true
            }
            Err(e) => {
                self.error_dialog = Some(tr_args("export.failed", &[("detail", &e.to_string())]));
                false
            }
        }
    }

    fn save_dxf_dialog(&mut self) -> bool {
        let stem = self.export_stem(&Self::safe_file_stem(&self.editor.project().name));
        let Some(path) = rfd::FileDialog::new()
            .add_filter("DXF", &["dxf"])
            .set_file_name(format!("{stem}.dxf"))
            .save_file()
        else {
            return false;
        };
        self.write_dxf_to(&path)
    }

    pub fn dxf_window(&mut self, ctx: &egui::Context) {
        let Some(d) = self.dxf_dialog.take() else {
            return;
        };
        let mut open = true;
        let mut save = false;
        let mut close = false;
        let mut chosen_size = None;
        let report = &d.export.report;
        egui::Window::new(tr("dxf.title"))
            .id(egui::Id::new("dxf_window"))
            .open(&mut open)
            .collapsible(false)
            .default_width(520.0)
            .show(ctx, |ui| {
                // US-09.8 : taille exportée ; changer de taille recalcule l'export (rapport compris).
                if let Some(size) = self.export_size_section(ui, true) {
                    chosen_size = Some(size);
                }
                crate::panels::note_paragraph(ui, "dxf.intro", &tr("dxf.intro"));
                let entities: usize = report.layers.iter().map(|l| l.entities).sum();
                crate::a11y::label(
                    ui,
                    tr_args(
                        "dxf.summary",
                        &[
                            ("pieces", &report.pieces.len().to_string()),
                            ("layers", &report.layers.len().to_string()),
                            ("entities", &entities.to_string()),
                        ],
                    ),
                );
                egui::ScrollArea::vertical()
                    .max_height(260.0)
                    .id_salt("dxf_report")
                    .show(ui, |ui| {
                        crate::a11y::label(ui, RichText::new(tr("dxf.layers.title")).strong());
                        for l in &report.layers {
                            crate::a11y::label(
                                ui,
                                tr_args(
                                    "dxf.layer.line",
                                    &[
                                        ("number", &l.layer.number.to_string()),
                                        ("name", &layer_label(l.layer.role)),
                                        ("count", &l.entities.to_string()),
                                    ],
                                ),
                            );
                        }
                        crate::a11y::label(ui, RichText::new(tr("dxf.omitted.title")).strong());
                        let mut none = true;
                        for i in report.omissions() {
                            none = false;
                            crate::a11y::label(ui, RichText::new(issue_text(i)).color(pal().error));
                        }
                        if none {
                            crate::a11y::label(ui, tr("dxf.omitted.none"));
                        }
                        let notes: Vec<&DxfIssue> = report.notes().collect();
                        if !notes.is_empty() {
                            crate::a11y::label(ui, RichText::new(tr("dxf.notes.title")).strong());
                            for i in notes {
                                crate::a11y::label(
                                    ui,
                                    RichText::new(issue_text(i)).color(pal().muted),
                                );
                            }
                        }
                    });
                ui.horizontal(|ui| {
                    if ui.button(tr("dxf.save")).clicked() {
                        save = true;
                    }
                    if ui.button(tr("dxf.close")).clicked() {
                        close = true;
                    }
                });
            });
        self.dxf_dialog = Some(d);
        if let Some(size) = chosen_size {
            // Un refus (taille non exportable) laisse la fenêtre et le choix d'avant.
            let previous = self.export_size;
            self.export_size = Some(size);
            if !self.open_dxf_export() {
                self.export_size = previous;
            }
        }
        if save && self.save_dxf_dialog() {
            close = true;
        }
        if close || !open {
            self.dxf_dialog = None;
        }
    }
}

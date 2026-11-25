//! Rapport de contrôle (US-08.5) : fenêtre « Exporter le rapport de contrôle… » (menu Fichier).
//! Le rapport est calculé à l'ouverture (lecture seule, aucune `Command`, rien n'entre dans le
//! projet) ; la fenêtre montre le nombre d'anomalies par type, puis « Exporter… » écrit un PDF
//! ou un CSV. Un refus (destination illisible) est une erreur propre : aucun fichier, projet et
//! préférences inchangés. Aucun calcul métier ici : tout vient de `pf_core::control_report` et
//! `pf_export::report`.

use crate::app::App;
use eframe::egui::{self, RichText};
use pf_core::control_report::{AnomalyKind, ControlReport};
use pf_export::report::{format_control_report, ReportFormat};
use pf_i18n::{lang, tr, tr_args};
use std::path::Path;

/// Hauteur maximale de la fenêtre (points) : elle défile au-delà.
const WINDOW_MAX_HEIGHT: f32 = 600.0;
/// Hauteur (points) de la partie fixe de la fenêtre : titre, consigne, format, boutons.
const FIXED_PART: f32 = 240.0;

/// Fenêtre du rapport : l'instantané calculé à l'ouverture et le format choisi (état
/// d'interface, jamais enregistré).
pub struct ReportDialog {
    pub report: ControlReport,
    pub format: ReportFormat,
}

fn kind_label(k: AnomalyKind) -> String {
    match k {
        AnomalyKind::SeamOutOfTolerance => tr("report.kind.seam_out"),
        AnomalyKind::SeamNotComputable => tr("report.kind.seam_na"),
        AnomalyKind::NotchMissing => tr("report.kind.notch_missing"),
        AnomalyKind::NotchShifted => tr("report.kind.notch_shifted"),
        AnomalyKind::NotchNotComputable => tr("report.kind.notch_na"),
        AnomalyKind::JunctionBroken => tr("report.kind.junction_broken"),
        AnomalyKind::JunctionNotComputable => tr("report.kind.junction_na"),
        AnomalyKind::PieceIssue => tr("report.kind.piece_issue"),
    }
}

impl App {
    /// Menu Fichier › « Exporter le rapport de contrôle… » : calcule le rapport et ouvre la
    /// fenêtre (aucun fichier n'est écrit avant « Exporter… »).
    pub fn open_report_export(&mut self) {
        let report = self.editor.project().control_report();
        self.report_dialog = Some(ReportDialog {
            report,
            format: ReportFormat::Pdf,
        });
    }

    /// Écrit le rapport affiché dans `path`, au format choisi. Renvoie `true` si le fichier est
    /// écrit ; un échec ouvre la boîte d'erreur, sans fichier ni changement du projet.
    pub fn write_report_to(&mut self, path: &Path) -> bool {
        let Some(d) = &self.report_dialog else {
            return false;
        };
        let out = format_control_report(&d.report, d.format, lang());
        let count = d.report.anomalies.len();
        let what = match d.format {
            ReportFormat::Pdf => "PDF",
            ReportFormat::Csv => "CSV",
        };
        match out.write_to(path) {
            Ok(()) => {
                let note = tr_args("export.note.report", &[("count", &count.to_string())]);
                self.editor.status.text = tr_args(
                    "export.done",
                    &[
                        ("what", what),
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

    fn save_report_dialog(&mut self) -> bool {
        let Some(format) = self.report_dialog.as_ref().map(|d| d.format) else {
            return false;
        };
        let stem = Self::safe_file_stem(&self.editor.project().name);
        let ext = format.extension();
        let Some(path) = rfd::FileDialog::new()
            .add_filter(ext.to_uppercase(), &[ext])
            .set_file_name(format!("{stem}-controle.{ext}"))
            .save_file()
        else {
            return false;
        };
        self.write_report_to(&path)
    }

    pub fn report_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.report_dialog.take() else {
            return;
        };
        let mut open = true;
        let mut save = false;
        let mut close = false;
        let slot = crate::dock::slot(ctx, &self.dock, crate::dock::ToolWin::Report);
        let width = self.dock.content_width(crate::dock::ToolWin::Report);
        egui::Window::new(tr("report.win.title"))
            .id(egui::Id::new("report_window"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_pos(slot.pos)
            .show(ctx, |ui| {
                ui.set_width(width);
                crate::panels::note_paragraph(ui, "report.win.intro", &tr("report.win.intro"));
                ui.separator();
                crate::a11y::label(ui, RichText::new(tr("report.win.format")).strong());
                ui.radio_value(&mut d.format, ReportFormat::Pdf, tr("report.win.pdf"));
                ui.radio_value(&mut d.format, ReportFormat::Csv, tr("report.win.csv"));
                ui.separator();
                let r = &d.report;
                // Le résumé défile ; les boutons restent visibles (fenêtre de 600 points au plus).
                let room = WINDOW_MAX_HEIGHT.min(slot.max_height) - FIXED_PART;
                egui::ScrollArea::vertical()
                    .id_salt("report_summary")
                    .max_height(room.max(60.0))
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        crate::a11y::label(
                            ui,
                            tr_args(
                                "report.win.counts",
                                &[
                                    ("pieces", &r.pieces.len().to_string()),
                                    ("pairs", &r.seams.len().to_string()),
                                ],
                            ),
                        );
                        crate::a11y::label(
                            ui,
                            RichText::new(tr_args(
                                "report.win.anomalies",
                                &[("count", &r.anomalies.len().to_string())],
                            ))
                            .strong(),
                        );
                        if r.anomalies.is_empty() {
                            crate::a11y::label(ui, tr("report.win.none"));
                        }
                        for k in AnomalyKind::ALL {
                            let n = r.count(k);
                            if n > 0 {
                                crate::a11y::label(
                                    ui,
                                    tr_args(
                                        "report.win.kind_line",
                                        &[("kind", &kind_label(k)), ("count", &n.to_string())],
                                    ),
                                );
                            }
                        }
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(tr("report.win.export")).clicked() {
                        save = true;
                    }
                    if ui.button(tr("report.win.close")).clicked() {
                        close = true;
                    }
                });
            });
        self.report_dialog = Some(d);
        if save && self.save_report_dialog() {
            close = true;
        }
        if close || !open {
            self.report_dialog = None;
        }
    }
}

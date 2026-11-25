//! US-11.2 / US-11.3 - fenêtre « Nomenclature » du dock (menu Fichier, sans raccourci) : pièces,
//! matières, découpes et surface nette par taille, relus par `Project::bill_of_materials` (calcul
//! dérivé, jamais stocké, aucune `Command`). Lecture seule. Les boutons écrivent la nomenclature
//! (CSV, PDF) ou la fiche technique (PDF, avec une note saisie ici et jamais enregistrée) dans le
//! seul fichier choisi, projet inchangé (`pf_export::bom_export`).
//!
//! Aucun calcul métier ici : tout vient du cœur. Le tableau est recalculé seulement quand le
//! projet ou la gamme change.

use crate::app::App;
use crate::dock::ToolWin;
use eframe::egui::{self, RichText};
use pf_core::{Bom, Project, SizeRangeId, VariantId};
use pf_export::bom_export::{export_bom_variant, export_tech_sheet};
use pf_export::report::ReportFormat;
use pf_i18n::{lang, tr, tr_args};
use pf_ui::components::{button, ButtonKind};
use std::path::Path;

#[derive(Debug, Clone)]
struct Cache {
    project: Project,
    range: Option<SizeRangeId>,
    variant: Option<VariantId>,
    bom: Option<Bom>,
}

/// État de la fenêtre (interface seulement, jamais enregistré).
#[derive(Debug, Clone, Default)]
pub struct BomDialog {
    /// Gamme choisie ; `None` = la première du projet, ou le projet tel quel sans gamme.
    pub range: Option<SizeRangeId>,
    /// Variante choisie (US-11.1) ; `None` = tout le projet (défaut, jamais suivie automatiquement).
    pub variant: Option<VariantId>,
    /// Note de la fiche technique (non enregistrée).
    pub note: String,
    cache: Option<Cache>,
}

fn area_text(v: Option<f64>) -> String {
    v.filter(|v| v.is_finite())
        .map_or_else(|| "—".to_string(), |v| format!("{v:.0}"))
}

/// Ce que l'utilisateur veut écrire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Out {
    Csv,
    Pdf,
    Tech,
}

impl App {
    /// Menu Fichier › « Nomenclature et fiche technique… ».
    pub fn open_bom(&mut self) {
        if self.bom.is_none() {
            self.bom = Some(BomDialog::default());
        }
    }

    /// Écrit la nomenclature ou la fiche technique dans `path`, dans la langue courante. Lecture
    /// seule : le projet n'est pas modifié et seul `path` est écrit ; un échec ouvre la boîte
    /// d'erreur, sans fichier.
    /// `variant` : `None` = tout le projet (US-11.1) ; la fiche technique ne dépend pas de la
    /// variante.
    pub fn write_bom_to(
        &mut self,
        range: Option<SizeRangeId>,
        variant: Option<VariantId>,
        format: ReportFormat,
        tech_note: Option<&str>,
        path: &Path,
    ) -> bool {
        let project = self.editor.project();
        let what = match format {
            ReportFormat::Pdf => "PDF",
            ReportFormat::Csv => "CSV",
        };
        let result = match tech_note {
            Some(note) => export_tech_sheet(project, range, Some(note), lang()),
            None => export_bom_variant(project, range, variant, format, lang()),
        };
        let out = match result {
            Ok(o) => o,
            Err(e) => {
                self.error_dialog = Some(tr_args("export.failed", &[("detail", &e.to_string())]));
                return false;
            }
        };
        let counts = project
            .bill_of_materials_variant(range, variant)
            .ok()
            .map(|b| (b.lines.len().to_string(), b.materials.len().to_string()));
        match out.write_to(path) {
            Ok(()) => {
                let note = counts.map_or_else(String::new, |(p, m)| {
                    tr_args("export.note.bom", &[("pieces", &p), ("materials", &m)])
                });
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

    fn save_bom_dialog(
        &mut self,
        range: Option<SizeRangeId>,
        variant: Option<VariantId>,
        what: Out,
        note: &str,
    ) -> bool {
        let stem = Self::safe_file_stem(&self.editor.project().name);
        let (format, suffix) = match what {
            Out::Csv => (ReportFormat::Csv, "nomenclature"),
            Out::Pdf => (ReportFormat::Pdf, "nomenclature"),
            Out::Tech => (ReportFormat::Pdf, "fiche-technique"),
        };
        let ext = format.extension();
        let Some(path) = rfd::FileDialog::new()
            .add_filter(ext.to_uppercase(), &[ext])
            .set_file_name(format!("{stem}-{suffix}.{ext}"))
            .save_file()
        else {
            return false;
        };
        let tech = (what == Out::Tech).then_some(note);
        self.write_bom_to(range, variant, format, tech, &path)
    }

    pub fn bom_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.bom.take() else {
            return;
        };
        let secondary = pf_ui::theme::tokens(ctx).text_secondary;
        let ranges: Vec<(SizeRangeId, String)> = self
            .editor
            .project()
            .sizes
            .iter()
            .map(|r| (r.id, r.name.clone()))
            .collect();
        let range = d
            .range
            .filter(|id| ranges.iter().any(|(r, _)| r == id))
            .or(ranges.first().map(|(r, _)| *r));
        let variants: Vec<(VariantId, String)> = self
            .editor
            .project()
            .variants
            .iter()
            .map(|v| (v.id, v.name.clone()))
            .collect();
        // Une variante disparue (suppression, annulation) est oubliée : retour à « tout le projet ».
        let variant = d.variant.filter(|id| variants.iter().any(|(v, _)| v == id));
        let stale = d.cache.as_ref().is_none_or(|c| {
            c.range != range || c.variant != variant || &c.project != self.editor.project()
        });
        if stale {
            d.cache = Some(Cache {
                project: self.editor.project().clone(),
                range,
                variant,
                bom: self
                    .editor
                    .project()
                    .bill_of_materials_variant(range, variant)
                    .ok(),
            });
        }
        let mut open = true;
        let mut close = false;
        let mut export: Option<Out> = None;
        let mut chosen: Option<Option<SizeRangeId>> = None;
        let mut chosen_variant: Option<Option<VariantId>> = None;
        let slot = crate::dock::slot(ctx, &self.dock, ToolWin::Bom);
        let width = self.dock.content_width(ToolWin::Bom);
        let bom = d.cache.as_ref().and_then(|c| c.bom.clone());
        egui::Window::new(tr("bomwin.title"))
            .id(ToolWin::Bom.id())
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_pos(slot.pos)
            .default_height(slot.max_height)
            .max_height(slot.max_height)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(slot.max_height)
                    .show(ui, |ui| {
                        ui.set_width(width);
                        crate::a11y::label(ui, RichText::new(tr("bomwin.intro")).color(secondary));
                        crate::a11y::label(ui, RichText::new(tr("bomwin.readonly")).strong());
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("bomwin.variants")).color(secondary),
                        );
                        ui.add_space(4.0);
                        if !ranges.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                crate::a11y::label(ui, tr("bomwin.range"));
                                for (id, name) in &ranges {
                                    if ui.selectable_label(Some(*id) == range, name).clicked() {
                                        chosen = Some(Some(*id));
                                    }
                                }
                            });
                        } else {
                            crate::a11y::label(
                                ui,
                                RichText::new(tr("bomwin.no_range")).color(secondary),
                            );
                        }
                        if !variants.is_empty() {
                            ui.horizontal_wrapped(|ui| {
                                crate::a11y::label(ui, tr("bomwin.variant"));
                                if ui
                                    .selectable_label(variant.is_none(), tr("bomwin.variant.all"))
                                    .clicked()
                                {
                                    chosen_variant = Some(None);
                                }
                                for (id, name) in &variants {
                                    if ui.selectable_label(Some(*id) == variant, name).clicked() {
                                        chosen_variant = Some(Some(*id));
                                    }
                                }
                            });
                        }
                        match &bom {
                            Some(b) if !b.lines.is_empty() => draw_tables(ui, b, secondary),
                            _ => {
                                crate::a11y::label(
                                    ui,
                                    RichText::new(tr("bomwin.empty")).color(secondary),
                                );
                            }
                        }
                        ui.add_space(6.0);
                        crate::a11y::label(ui, tr("bomwin.note_label"));
                        crate::a11y::add_text(
                            ui,
                            egui::TextEdit::multiline(&mut d.note)
                                .desired_rows(2)
                                .desired_width(f32::INFINITY),
                        );
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            if button(ui, &tr("bomwin.export_csv"), ButtonKind::Secondary).clicked()
                            {
                                export = Some(Out::Csv);
                            }
                            if button(ui, &tr("bomwin.export_pdf"), ButtonKind::Secondary).clicked()
                            {
                                export = Some(Out::Pdf);
                            }
                            if button(ui, &tr("bomwin.export_tech"), ButtonKind::Secondary)
                                .clicked()
                            {
                                export = Some(Out::Tech);
                            }
                        });
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("bomwin.export_note")).color(secondary),
                        );
                        ui.add_space(6.0);
                        close = button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                    });
            });
        if let Some(id) = chosen {
            d.range = id;
        }
        if let Some(v) = chosen_variant {
            d.variant = v;
        }
        let note = d.note.clone();
        if open && !close {
            self.bom = Some(d);
        }
        if let Some(what) = export {
            self.save_bom_dialog(range, variant, what, &note);
        }
    }
}

fn draw_tables(ui: &mut egui::Ui, bom: &Bom, secondary: egui::Color32) {
    ui.add_space(4.0);
    crate::a11y::label(ui, RichText::new(tr("bomwin.pieces")).strong());
    egui::Grid::new("bom_pieces").striped(true).show(ui, |ui| {
        for h in [
            "bom.col.piece",
            "bom.col.material",
            "bom.col.qty",
            "bom.col.cuts",
        ] {
            crate::a11y::label(ui, RichText::new(tr(h)).small().color(secondary));
        }
        ui.end_row();
        for l in &bom.lines {
            match &l.option {
                Some(o) => crate::a11y::label(ui, format!("{} [{o}]", l.name)),
                None => crate::a11y::label(ui, &l.name),
            };
            crate::a11y::label(ui, l.material.clone().unwrap_or_else(|| tr("bom.unset")));
            crate::a11y::label(ui, l.quantity.to_string());
            crate::a11y::label(ui, l.cuts.to_string());
            ui.end_row();
        }
    });
    ui.add_space(6.0);
    crate::a11y::label(ui, RichText::new(tr("bomwin.materials")).strong());
    let labels: Vec<String> = if bom.labels.is_empty() {
        vec![tr("bom.size.base")]
    } else {
        bom.labels.clone()
    };
    egui::ScrollArea::horizontal()
        .id_salt("bom_materials_scroll")
        .show(ui, |ui| {
            egui::Grid::new("bom_materials")
                .striped(true)
                .show(ui, |ui| {
                    for h in ["bom.col.material", "bom.col.pieces", "bom.col.cuts"] {
                        crate::a11y::label(ui, RichText::new(tr(h)).small().color(secondary));
                    }
                    for l in &labels {
                        crate::a11y::label(ui, RichText::new(l).small().color(secondary));
                    }
                    ui.end_row();
                    for m in &bom.materials {
                        crate::a11y::label(
                            ui,
                            m.material.clone().unwrap_or_else(|| tr("bom.unset")),
                        );
                        crate::a11y::label(ui, m.pieces.to_string());
                        crate::a11y::label(ui, m.cuts.to_string());
                        for a in &m.net_area_mm2 {
                            crate::a11y::label(ui, area_text(*a));
                        }
                        ui.end_row();
                    }
                });
        });
}

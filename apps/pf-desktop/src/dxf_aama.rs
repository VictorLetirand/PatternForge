//! Import de pièces d'un DXF de type AAMA / ASTM (US-12.2) : fenêtre « Importer des pièces »
//! (fichier, unité, aperçu des pièces, rapport de ce qui n'est pas importé, remarques).
//!
//! L'import passe par **une seule** `Command::ImportAamaPieces` (une étape annulable) calculée
//! d'après l'aperçu affiché : ce que l'on voit est ce qui est créé. Un fichier illisible ou un
//! refus de la commande ne laisse aucune trace dans le projet. La gradation (`.rul`) n'est pas lue.

use crate::app::App;
use crate::dxf_generic::import_error;
use crate::palette::pal;
use eframe::egui::{self, RichText};
use pf_export::dxf_aama_import::{AamaBuild, AamaDocument, AamaNote, AamaNoteKind, AamaReason};
use pf_export::dxf_import::{DeclaredUnit, DxfImportError, DxfUnit, UnitChoice, MAX_FILE_BYTES};
use pf_i18n::{tr, tr_args};
use std::path::Path;

/// Fenêtre « Importer des pièces » : état d'interface, jamais enregistré.
pub struct AamaDialog {
    pub file_name: String,
    pub doc: AamaDocument,
    /// `None` tant que l'utilisatrice n'a pas choisi (fichier sans unité reconnue).
    pub choice: Option<UnitChoice>,
    cache: Option<(UnitChoice, Result<AamaBuild, DxfImportError>)>,
}

impl AamaDialog {
    pub fn new(file_name: String, doc: AamaDocument) -> AamaDialog {
        let choice = match doc.declared_unit() {
            DeclaredUnit::Known(_) => Some(UnitChoice::Declared),
            _ => None,
        };
        AamaDialog {
            file_name,
            doc,
            choice,
            cache: None,
        }
    }

    /// Aperçu pour le choix courant (calculé une fois par choix).
    pub fn preview(&mut self) -> Option<&Result<AamaBuild, DxfImportError>> {
        let choice = self.choice?;
        if self.cache.as_ref().is_none_or(|(c, _)| *c != choice) {
            self.cache = Some((choice, self.doc.build_choice(choice)));
        }
        self.cache.as_ref().map(|(_, r)| r)
    }
}

fn unit_name(u: DxfUnit) -> String {
    tr(match u {
        DxfUnit::Millimetre => "dxfi.unit.name.mm",
        DxfUnit::Centimetre => "dxfi.unit.name.cm",
        DxfUnit::Decimetre => "dxfi.unit.name.dm",
        DxfUnit::Metre => "dxfi.unit.name.m",
        DxfUnit::Inch => "dxfi.unit.name.in",
        DxfUnit::Foot => "dxfi.unit.name.ft",
        DxfUnit::Yard => "dxfi.unit.name.yd",
    })
}

fn unit_choice_label(u: DxfUnit) -> String {
    match u {
        DxfUnit::Millimetre => tr("dxfi.unit.mm"),
        DxfUnit::Centimetre => tr("dxfi.unit.cm"),
        DxfUnit::Metre => tr("dxfi.unit.m"),
        DxfUnit::Inch => tr("dxfi.unit.in"),
        DxfUnit::Foot => tr("dxfi.unit.ft"),
        other => unit_name(other),
    }
}

fn reason_text(r: AamaReason) -> String {
    tr(match r {
        AamaReason::NotImported => "aama.reason.not_imported",
        AamaReason::UnknownLayer => "aama.reason.unknown_layer",
        AamaReason::Unsupported => "aama.reason.unsupported",
        AamaReason::Invalid => "aama.reason.invalid",
        AamaReason::Degenerate => "aama.reason.degenerate",
        AamaReason::OpenContour => "aama.reason.open_contour",
        AamaReason::ExtraContour => "aama.reason.extra_contour",
        AamaReason::NotOnContour => "aama.reason.not_on_contour",
        AamaReason::GrainMark => "aama.reason.grain_mark",
        AamaReason::TextLine => "aama.reason.text_line",
        AamaReason::NoBlock => "aama.reason.no_block",
        AamaReason::NoContour => "aama.reason.no_contour",
        AamaReason::BlockNotInserted => "aama.reason.block_not_inserted",
        AamaReason::Transformed => "aama.reason.transformed",
        AamaReason::UnknownBlock => "aama.reason.unknown_block",
        AamaReason::Nested => "aama.reason.nested",
        AamaReason::OutOfRange => "aama.reason.out_of_range",
        AamaReason::TooComplex => "aama.reason.too_complex",
        AamaReason::Refused => "aama.reason.refused",
    })
}

fn note_text(n: &AamaNote) -> String {
    let c = n.count.to_string();
    let a = [("count", c.as_str())];
    match n.kind {
        AamaNoteKind::DuplicateVertices => tr_args("aama.note.duplicate_vertices", &a),
        AamaNoteKind::ArcsFlattened => tr_args("aama.note.arcs_flattened", &a),
        AamaNoteKind::ClosedByRepeat => tr_args("aama.note.closed_by_repeat", &a),
        AamaNoteKind::LegacyEncoding => tr("aama.note.legacy_encoding"),
    }
}

fn preview_body(ui: &mut egui::Ui, b: &AamaBuild) {
    if b.pieces.is_empty() {
        crate::a11y::label(ui, RichText::new(tr("aama.nothing")).color(pal().error));
    } else {
        crate::a11y::label(
            ui,
            tr_args(
                "aama.preview.pieces",
                &[
                    ("count", &b.pieces.len().to_string()),
                    ("notches", &b.report.notches().to_string()),
                ],
            ),
        );
    }
    for p in &b.report.pieces {
        let grain = tr(if p.grain {
            "aama.grain.yes"
        } else {
            "aama.grain.no"
        });
        let sewing = if p.sewing {
            tr("aama.piece.sewing")
        } else {
            String::new()
        };
        crate::a11y::label(
            ui,
            tr_args(
                "aama.piece.line",
                &[
                    ("name", &p.name),
                    ("vertices", &p.vertices.to_string()),
                    ("notches", &p.notches.to_string()),
                    ("grain", &grain),
                    ("sewing", &sewing),
                ],
            ),
        );
    }
    crate::a11y::label(ui, RichText::new(tr("aama.ignored.title")).strong());
    if b.report.ignored.is_empty() {
        crate::a11y::label(ui, tr("aama.ignored.none"));
    }
    for i in &b.report.ignored {
        let place = if i.piece.is_empty() {
            tr_args("aama.where.top", &[("layer", &i.layer)])
        } else {
            tr_args(
                "aama.where.block",
                &[("piece", &i.piece), ("layer", &i.layer)],
            )
        };
        let detail = if i.detail.is_empty() {
            String::new()
        } else {
            format!(" ({})", i.detail)
        };
        crate::a11y::label(
            ui,
            RichText::new(tr_args(
                "aama.ignored.line",
                &[
                    ("kind", &i.kind),
                    ("count", &i.count.to_string()),
                    ("where", &place),
                    ("reason", &reason_text(i.reason)),
                    ("detail", &detail),
                ],
            ))
            .color(pal().error),
        );
    }
    if !b.report.notes.is_empty() {
        crate::a11y::label(ui, RichText::new(tr("aama.notes.title")).strong());
        for n in &b.report.notes {
            crate::a11y::label(ui, note_text(n));
        }
    }
    crate::a11y::label(ui, RichText::new(tr("aama.rul")).color(pal().muted));
}

impl App {
    /// Menu Fichier > « Importer des pièces d'un DXF de type AAMA / ASTM… ».
    pub fn open_aama_import(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("DXF", &["dxf"])
            .pick_file()
        else {
            return;
        };
        self.load_aama_import(&path);
    }

    /// Lit le fichier et ouvre la fenêtre. Un fichier trop gros, illisible ou refusé affiche une
    /// erreur : aucune fenêtre, aucun changement du projet.
    pub fn load_aama_import(&mut self, path: &Path) {
        let too_large = std::fs::metadata(path)
            .map(|m| m.len() > MAX_FILE_BYTES as u64)
            .unwrap_or(false);
        if too_large {
            let bytes = std::fs::metadata(path).map_or(0, |m| m.len() as usize);
            self.editor
                .error(import_error(&DxfImportError::TooLarge { bytes }));
            return;
        }
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                self.error_dialog = Some(tr_args("dxfi.err.read", &[("detail", &e.to_string())]));
                return;
            }
        };
        match AamaDocument::parse(&bytes) {
            Ok(doc) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                self.aama_import = Some(AamaDialog::new(name, doc));
            }
            Err(e) => self.editor.error(import_error(&e)),
        }
    }

    /// Importe d'après l'aperçu affiché : une seule commande ; un refus laisse le projet intact
    /// et la fenêtre ouverte.
    pub fn run_aama_import(&mut self) -> bool {
        let Some(d) = self.aama_import.as_mut() else {
            return false;
        };
        let name = d.file_name.clone();
        let Some(Ok(build)) = d.preview().cloned() else {
            return false;
        };
        let Some(command) = build.command() else {
            return false;
        };
        let Some(out) = self.editor.exec(command) else {
            return false;
        };
        if let Some(sheet) = out.sheet {
            self.editor.set_sheet(sheet);
        }
        self.editor.info(tr_args(
            "aama.status.done",
            &[("count", &out.pieces.len().to_string()), ("name", &name)],
        ));
        self.fit_pending = true;
        self.aama_import = None;
        true
    }

    pub fn aama_import_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.aama_import.take() else {
            return;
        };
        let mut open = true;
        let mut import = false;
        let mut close = false;
        egui::Window::new(tr("aama.title"))
            .id(egui::Id::new("aama_import_window"))
            .open(&mut open)
            .collapsible(false)
            .default_width(600.0)
            .show(ctx, |ui| {
                crate::panels::note_paragraph(ui, "aama.intro", &tr("aama.intro"));
                crate::a11y::label(
                    ui,
                    tr_args(
                        "aama.file",
                        &[
                            ("name", &d.file_name),
                            ("blocks", &d.doc.block_count().to_string()),
                            ("inserts", &d.doc.insert_count().to_string()),
                        ],
                    ),
                );
                ui.separator();
                crate::a11y::label(ui, RichText::new(tr("dxfi.unit.title")).strong());
                match d.doc.declared_unit() {
                    DeclaredUnit::Known(u) => {
                        let label = tr_args("dxfi.unit.declared", &[("unit", &unit_name(u))]);
                        ui.radio_value(&mut d.choice, Some(UnitChoice::Declared), label);
                    }
                    DeclaredUnit::Unitless => {
                        crate::a11y::label(ui, tr("dxfi.unit.declared_unitless"));
                    }
                    DeclaredUnit::Missing => {
                        crate::a11y::label(ui, tr("dxfi.unit.declared_missing"));
                    }
                    DeclaredUnit::Unsupported(code) => {
                        crate::a11y::label(
                            ui,
                            tr_args(
                                "dxfi.unit.declared_unsupported",
                                &[("code", &code.to_string())],
                            ),
                        );
                    }
                }
                for u in DxfUnit::CHOICES {
                    ui.radio_value(
                        &mut d.choice,
                        Some(UnitChoice::Fixed(u)),
                        unit_choice_label(u),
                    );
                }
                ui.separator();
                crate::a11y::label(ui, RichText::new(tr("aama.preview.title")).strong());
                let mut importable = false;
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .id_salt("aama_import_preview")
                    .show(ui, |ui| match d.preview() {
                        None => {
                            crate::a11y::label(
                                ui,
                                RichText::new(tr("dxfi.unit.pick")).color(pal().error),
                            );
                        }
                        Some(Err(e)) => {
                            crate::a11y::label(
                                ui,
                                RichText::new(import_error(e)).color(pal().error),
                            );
                        }
                        Some(Ok(b)) => {
                            importable = !b.pieces.is_empty();
                            preview_body(ui, b);
                        }
                    });
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(importable, egui::Button::new(tr("aama.import")))
                        .clicked()
                    {
                        import = true;
                    }
                    if ui.button(tr("aama.close")).clicked() {
                        close = true;
                    }
                });
            });
        self.aama_import = Some(d);
        if import {
            self.run_aama_import();
        }
        if close || !open {
            self.aama_import = None;
        }
    }
}

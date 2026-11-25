//! DXF générique (US-12.3) : fenêtre « Importer un DXF » (fichier, unité, feuille cible, aperçu
//! des dimensions, rapport, bouton d'import) et fenêtre « Exporter la construction en DXF ».
//!
//! L'import passe par **une seule** `Command::ImportGeometry` (une étape annulable) calculée
//! d'après l'aperçu affiché : ce que l'on voit est ce qui est créé. Un fichier illisible ou un
//! refus de la commande ne laisse aucune trace dans le projet. L'export est en lecture seule.

use crate::a11y::ComboExt;
use crate::app::App;
use crate::editor::Selection;
use crate::palette::pal;
use crate::text::role_label;
use eframe::egui::{self, RichText};
use pf_core::{input::format_number, Command, SheetId};
use pf_export::dxf_construction::{
    export_construction_dxf, ConstructionError, ConstructionExport, ConstructionNote,
    ConstructionNoteKind, ConstructionScope,
};
use pf_export::dxf_import::{
    DeclaredUnit, DxfDocument, DxfImportError, DxfUnit, IgnoreReason, ImportBuild, ImportNote,
    ImportNoteKind, UnitChoice, MAX_FILE_BYTES,
};
use pf_i18n::{tr, tr_args};
use std::path::Path;

/// Fenêtre « Importer un DXF » : état d'interface, jamais enregistré.
pub struct ImportDialog {
    pub file_name: String,
    pub doc: DxfDocument,
    /// `None` tant que l'utilisatrice n'a pas choisi (fichier sans unité reconnue).
    pub choice: Option<UnitChoice>,
    pub sheet: SheetId,
    /// Aperçu du choix courant, recalculé quand le choix change.
    cache: Option<(UnitChoice, Result<ImportBuild, DxfImportError>)>,
}

impl ImportDialog {
    pub fn new(file_name: String, doc: DxfDocument, sheet: SheetId) -> ImportDialog {
        let choice = match doc.declared_unit() {
            DeclaredUnit::Known(_) => Some(UnitChoice::Declared),
            _ => None,
        };
        ImportDialog {
            file_name,
            doc,
            choice,
            sheet,
            cache: None,
        }
    }

    /// Aperçu pour le choix courant (calculé une fois par choix).
    pub fn preview(&mut self) -> Option<&Result<ImportBuild, DxfImportError>> {
        let choice = self.choice?;
        if self.cache.as_ref().is_none_or(|(c, _)| *c != choice) {
            self.cache = Some((choice, self.doc.build_choice(choice)));
        }
        self.cache.as_ref().map(|(_, r)| r)
    }
}

/// Fenêtre « Exporter la construction en DXF » : l'export déjà calculé, jamais enregistré.
pub struct ConstructionDialog {
    pub all_sheets: bool,
    pub export: Result<ConstructionExport, ConstructionError>,
}

fn unit_name(u: DxfUnit) -> String {
    match u {
        DxfUnit::Millimetre => tr("dxfi.unit.name.mm"),
        DxfUnit::Centimetre => tr("dxfi.unit.name.cm"),
        DxfUnit::Decimetre => tr("dxfi.unit.name.dm"),
        DxfUnit::Metre => tr("dxfi.unit.name.m"),
        DxfUnit::Inch => tr("dxfi.unit.name.in"),
        DxfUnit::Foot => tr("dxfi.unit.name.ft"),
        DxfUnit::Yard => tr("dxfi.unit.name.yd"),
    }
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

/// Message d'un fichier refusé (aucun objet créé).
pub fn import_error(e: &DxfImportError) -> String {
    match e {
        DxfImportError::TooLarge { bytes } => tr_args(
            "dxfi.err.too_large",
            &[
                ("bytes", &bytes.to_string()),
                ("max", &MAX_FILE_BYTES.to_string()),
            ],
        ),
        DxfImportError::BinaryDxf => tr("dxfi.err.binary"),
        DxfImportError::NotDxf => tr("dxfi.err.not_dxf"),
        DxfImportError::InvalidGroupCode { line } => {
            tr_args("dxfi.err.group_code", &[("line", &line.to_string())])
        }
        DxfImportError::Truncated { line } => {
            tr_args("dxfi.err.truncated", &[("line", &line.to_string())])
        }
        DxfImportError::Structure { line } => {
            tr_args("dxfi.err.structure", &[("line", &line.to_string())])
        }
        DxfImportError::TooManyEntities => tr("dxfi.err.too_many_entities"),
        DxfImportError::TooManyVertices { max } => {
            tr_args("dxfi.err.too_many_vertices", &[("max", &max.to_string())])
        }
        DxfImportError::TooManyObjects { count } => tr_args(
            "dxfi.err.too_many_objects",
            &[
                ("count", &count.to_string()),
                ("max", &pf_core::MAX_IMPORT_OBJECTS.to_string()),
            ],
        ),
        DxfImportError::UnitRequired => tr("dxfi.err.unit_required"),
    }
}

fn reason_text(r: IgnoreReason) -> String {
    match r {
        IgnoreReason::Unsupported => tr("dxfi.reason.unsupported"),
        IgnoreReason::Invalid => tr("dxfi.reason.invalid"),
        IgnoreReason::Degenerate => tr("dxfi.reason.degenerate"),
        IgnoreReason::Polyline3d => tr("dxfi.reason.polyline_3d"),
        IgnoreReason::Extrusion => tr("dxfi.reason.extrusion"),
        IgnoreReason::OutOfRange => tr("dxfi.reason.out_of_range"),
        IgnoreReason::TooComplex => tr("dxfi.reason.too_complex"),
    }
}

fn note_text(n: &ImportNote) -> String {
    let c = n.count.to_string();
    let a = [("count", c.as_str())];
    match n.kind {
        ImportNoteKind::ZIgnored => tr_args("dxfi.note.z_ignored", &a),
        ImportNoteKind::SplineSegments => tr_args("dxfi.note.spline_segments", &a),
        ImportNoteKind::SplineFitPoints => tr_args("dxfi.note.spline_fit_points", &a),
        ImportNoteKind::ClosedCurveSplit => tr_args("dxfi.note.closed_curve_split", &a),
        ImportNoteKind::BulgeFlattened => tr_args("dxfi.note.bulge_flattened", &a),
        ImportNoteKind::PolylineFit => tr_args("dxfi.note.polyline_fit", &a),
        ImportNoteKind::ZeroLengthParts => tr_args("dxfi.note.zero_length_parts", &a),
        ImportNoteKind::DuplicatesRemoved => tr_args("dxfi.note.duplicates_removed", &a),
        ImportNoteKind::LegacyEncoding => tr("dxfi.note.legacy_encoding"),
        ImportNoteKind::BlockContents => tr_args("dxfi.note.block_contents", &a),
        ImportNoteKind::LayerRenamed => tr_args("dxfi.note.layer_renamed", &a),
    }
}

fn construction_note_text(n: &ConstructionNote) -> String {
    let c = n.count.to_string();
    let a = [("count", c.as_str())];
    match n.kind {
        ConstructionNoteKind::Notches => tr_args("dxfc.note.notches", &a),
        ConstructionNoteKind::Dimensions => tr_args("dxfc.note.dimensions", &a),
        ConstructionNoteKind::FullDimensions => tr_args("dxfc.note.full_dimensions", &a),
        ConstructionNoteKind::Guides => tr_args("dxfc.note.guides", &a),
        ConstructionNoteKind::Groups => tr_args("dxfc.note.groups", &a),
        ConstructionNoteKind::Unreadable => tr_args("dxfc.note.unreadable", &a),
        ConstructionNoteKind::EntityNames => tr_args("dxfc.note.entity_names", &a),
        ConstructionNoteKind::PointLabels => tr_args("dxfc.note.point_labels", &a),
        ConstructionNoteKind::PiecesAsPaths => tr_args("dxfc.note.pieces_as_paths", &a),
        ConstructionNoteKind::CurvesDiscretized => tr_args("dxfc.note.curves_discretized", &a),
        ConstructionNoteKind::SplinesDiscretized => tr_args("dxfc.note.splines_discretized", &a),
        ConstructionNoteKind::HiddenLayers => tr_args("dxfc.note.hidden_layers", &a),
    }
}

/// Message d'un refus d'export de construction (aucun fichier écrit).
pub fn construction_error(e: &ConstructionError) -> String {
    match e {
        ConstructionError::NothingToExport => tr("dxfc.err.nothing"),
        ConstructionError::NonFinite => tr("dxfc.err.non_finite"),
        ConstructionError::OutOfRange => tr("dxfc.err.range"),
        ConstructionError::Core(c) => crate::text::core_error(c),
    }
}

impl App {
    /// Menu Fichier › « Importer un DXF… » : choix du fichier puis fenêtre d'aperçu.
    pub fn open_dxf_import(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("DXF", &["dxf"])
            .pick_file()
        else {
            return;
        };
        self.load_dxf_import(&path);
    }

    /// Lit le fichier et ouvre la fenêtre. Un fichier trop gros, illisible ou refusé affiche une
    /// erreur : aucune fenêtre, aucun changement du projet.
    pub fn load_dxf_import(&mut self, path: &Path) {
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
        match DxfDocument::parse(&bytes) {
            Ok(doc) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                self.dxf_import = Some(ImportDialog::new(name, doc, self.editor.sheet));
            }
            Err(e) => self.editor.error(import_error(&e)),
        }
    }

    /// Importe d'après l'aperçu affiché : une seule commande ; un refus laisse le projet intact
    /// et la fenêtre ouverte.
    pub fn run_dxf_import(&mut self) -> bool {
        let Some(d) = self.dxf_import.as_mut() else {
            return false;
        };
        let (sheet, name) = (d.sheet, d.file_name.clone());
        let Some(Ok(build)) = d.preview().cloned() else {
            return false;
        };
        if build.objects.is_empty() {
            return false;
        }
        let Some(out) = self.editor.exec(Command::ImportGeometry {
            sheet,
            objects: build.objects,
        }) else {
            return false;
        };
        self.editor.set_sheet(sheet);
        self.editor.selection = Selection {
            points: out.points.clone(),
            entities: out.entities.clone(),
            ..Selection::default()
        };
        let sheet_name = self
            .editor
            .project()
            .sheet(sheet)
            .map(|s| s.name.clone())
            .unwrap_or_default();
        if out.notes.is_empty() {
            self.editor.info(tr_args(
                "dxfi.status.done",
                &[
                    ("count", &out.entities.len().to_string()),
                    ("name", &name),
                    ("sheet", &sheet_name),
                ],
            ));
        }
        self.fit_pending = true;
        self.dxf_import = None;
        true
    }

    pub fn dxf_import_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.dxf_import.take() else {
            return;
        };
        let mut open = true;
        let mut import = false;
        let mut close = false;
        let sheets: Vec<(SheetId, String)> = self
            .editor
            .project()
            .sheets
            .iter()
            .map(|s| (s.id, s.name.clone()))
            .collect();
        egui::Window::new(tr("dxfi.title"))
            .id(egui::Id::new("dxf_import_window"))
            .open(&mut open)
            .collapsible(false)
            .default_width(560.0)
            .show(ctx, |ui| {
                crate::panels::note_paragraph(ui, "dxfi.intro", &tr("dxfi.intro"));
                crate::a11y::label(
                    ui,
                    tr_args(
                        "dxfi.file",
                        &[
                            ("name", &d.file_name),
                            ("entities", &d.doc.entity_count().to_string()),
                        ],
                    ),
                );
                if let Some(v) = d.doc.version() {
                    crate::a11y::label(ui, tr_args("dxfi.version", &[("version", v)]));
                }
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
                ui.horizontal(|ui| {
                    crate::a11y::label(ui, tr("dxfi.sheet"));
                    let current = sheets
                        .iter()
                        .find(|(id, _)| *id == d.sheet)
                        .map(|(_, n)| n.clone())
                        .unwrap_or_default();
                    egui::ComboBox::from_id_salt("dxf_import_sheet")
                        .selected_text(current)
                        .show_ui_named(ui, |ui| {
                            for (id, name) in &sheets {
                                ui.selectable_value(&mut d.sheet, *id, name);
                            }
                        });
                });
                ui.separator();
                crate::a11y::label(ui, RichText::new(tr("dxfi.preview.title")).strong());
                let mut importable = false;
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .id_salt("dxf_import_preview")
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
                            importable = !b.objects.is_empty();
                            preview_body(ui, b);
                        }
                    });
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(importable, egui::Button::new(tr("dxfi.import")))
                        .clicked()
                    {
                        import = true;
                    }
                    if ui.button(tr("dxfi.close")).clicked() {
                        close = true;
                    }
                });
            });
        self.dxf_import = Some(d);
        if import {
            self.run_dxf_import();
        }
        if close || !open {
            self.dxf_import = None;
        }
    }

    // ----- export de la construction -------------------------------------------------------------

    fn construction_scope(&self, all: bool) -> ConstructionScope {
        if all {
            ConstructionScope::AllSheets
        } else {
            ConstructionScope::Sheet(self.editor.sheet)
        }
    }

    /// Menu Fichier › « Exporter la construction en DXF… » : calcule l'export et ouvre la
    /// fenêtre ; un refus (feuille vide) est une erreur, sans fenêtre ni fichier.
    pub fn open_construction_export(&mut self) {
        match export_construction_dxf(self.editor.project(), self.construction_scope(false)) {
            Ok(export) => {
                self.dxf_construction = Some(ConstructionDialog {
                    all_sheets: false,
                    export: Ok(export),
                });
            }
            Err(e) => self.editor.error(construction_error(&e)),
        }
    }

    /// Écrit le DXF calculé dans `path`.
    pub fn write_construction_dxf_to(&mut self, path: &Path) -> bool {
        let Some(Ok(export)) = self.dxf_construction.as_ref().map(|d| &d.export) else {
            return false;
        };
        let entities = export.report.total_entities().to_string();
        match std::fs::write(path, export.bytes()) {
            Ok(()) => {
                let note = tr_args("export.note.dxf_construction", &[("entities", &entities)]);
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

    fn save_construction_dialog(&mut self) -> bool {
        let stem = Self::safe_file_stem(&self.editor.project().name);
        let Some(path) = rfd::FileDialog::new()
            .add_filter("DXF", &["dxf"])
            .set_file_name(format!("{stem}.dxf"))
            .save_file()
        else {
            return false;
        };
        self.write_construction_dxf_to(&path)
    }

    pub fn dxf_construction_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.dxf_construction.take() else {
            return;
        };
        let was_all = d.all_sheets;
        let mut open = true;
        let mut save = false;
        let mut close = false;
        let sheet_name = self
            .editor
            .project()
            .sheet(self.editor.sheet)
            .map(|s| s.name.clone())
            .unwrap_or_default();
        egui::Window::new(tr("dxfc.title"))
            .id(egui::Id::new("dxf_construction_window"))
            .open(&mut open)
            .collapsible(false)
            .default_width(540.0)
            .show(ctx, |ui| {
                crate::panels::note_paragraph(ui, "dxfc.intro", &tr("dxfc.intro"));
                ui.radio_value(
                    &mut d.all_sheets,
                    false,
                    tr_args("dxfc.scope.sheet", &[("name", &sheet_name)]),
                );
                ui.radio_value(&mut d.all_sheets, true, tr("dxfc.scope.all"));
                match &d.export {
                    Err(e) => {
                        crate::a11y::label(
                            ui,
                            RichText::new(construction_error(e)).color(pal().error),
                        );
                    }
                    Ok(x) => {
                        egui::ScrollArea::vertical()
                            .max_height(280.0)
                            .id_salt("dxf_construction_report")
                            .show(ui, |ui| construction_body(ui, x));
                    }
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(d.export.is_ok(), egui::Button::new(tr("dxfc.save")))
                        .clicked()
                    {
                        save = true;
                    }
                    if ui.button(tr("dxfc.close")).clicked() {
                        close = true;
                    }
                });
            });
        if d.all_sheets != was_all {
            d.export = export_construction_dxf(
                self.editor.project(),
                self.construction_scope(d.all_sheets),
            );
        }
        self.dxf_construction = Some(d);
        if save && self.save_construction_dialog() {
            close = true;
        }
        if close || !open {
            self.dxf_construction = None;
        }
    }
}

fn num(v: f64) -> String {
    format_number(v, 2)
}

fn preview_body(ui: &mut egui::Ui, b: &ImportBuild) {
    match b.bbox_mm {
        Some(bb) => {
            crate::a11y::label(
                ui,
                tr_args(
                    "dxfi.preview.size",
                    &[
                        ("w", &num(bb.width())),
                        ("h", &num(bb.height())),
                        ("x0", &num(bb.min.x)),
                        ("y0", &num(bb.min.y)),
                        ("x1", &num(bb.max.x)),
                        ("y1", &num(bb.max.y)),
                    ],
                ),
            );
        }
        None => {
            crate::a11y::label(
                ui,
                RichText::new(tr("dxfi.preview.nothing")).color(pal().error),
            );
        }
    }
    crate::a11y::label(
        ui,
        tr_args("dxfi.preview.scale", &[("k", &format_number(b.scale, 4))]),
    );
    crate::a11y::label(
        ui,
        tr_args(
            "dxfi.preview.objects",
            &[("count", &b.report.objects.to_string())],
        ),
    );
    let list: Vec<String> = b
        .report
        .imported
        .iter()
        .map(|(k, n)| format!("{k} {n}"))
        .collect();
    if !list.is_empty() {
        crate::a11y::label(
            ui,
            tr_args("dxfi.preview.counts", &[("list", &list.join(", "))]),
        );
    }
    crate::a11y::label(ui, RichText::new(tr("dxfi.ignored.title")).strong());
    if b.report.ignored.is_empty() {
        crate::a11y::label(ui, tr("dxfi.ignored.none"));
    }
    for i in &b.report.ignored {
        crate::a11y::label(
            ui,
            RichText::new(tr_args(
                "dxfi.ignored.line",
                &[
                    ("kind", &i.kind),
                    ("count", &i.count.to_string()),
                    ("reason", &reason_text(i.reason)),
                ],
            ))
            .color(pal().error),
        );
    }
    if b.report.ignored_of("INSERT") > 0 {
        crate::a11y::label(ui, RichText::new(tr("dxfi.insert_hint")).color(pal().muted));
    }
    if !b.report.notes.is_empty() {
        crate::a11y::label(ui, RichText::new(tr("dxfi.notes.title")).strong());
        for n in &b.report.notes {
            crate::a11y::label(ui, RichText::new(note_text(n)).color(pal().muted));
        }
    }
    if !b.report.layers.is_empty() {
        crate::a11y::label(ui, RichText::new(tr("dxfi.layers.title")).strong());
        for l in &b.report.layers {
            let calque = l.calque.clone().unwrap_or_else(|| tr("dxfi.layer.default"));
            crate::a11y::label(
                ui,
                tr_args(
                    "dxfi.layer.line",
                    &[
                        ("dxf", &l.dxf_name),
                        ("calque", &calque),
                        ("count", &l.objects.to_string()),
                    ],
                ),
            );
        }
    }
}

fn construction_body(ui: &mut egui::Ui, x: &ConstructionExport) {
    let r = &x.report;
    if let Some(bb) = r.bbox_mm {
        crate::a11y::label(
            ui,
            tr_args(
                "dxfc.summary",
                &[
                    ("entities", &r.total_entities().to_string()),
                    ("layers", &r.layers.len().to_string()),
                    ("w", &num(bb.width())),
                    ("h", &num(bb.height())),
                ],
            ),
        );
    }
    let kinds: Vec<String> = r.entities.iter().map(|(k, n)| format!("{k} {n}")).collect();
    crate::a11y::label(ui, tr_args("dxfc.entities", &[("list", &kinds.join(", "))]));
    if r.sheets.len() > 1 {
        for s in &r.sheets {
            crate::a11y::label(
                ui,
                tr_args(
                    "dxfc.sheet.line",
                    &[
                        ("name", &s.name),
                        ("count", &s.entities.to_string()),
                        ("dx", &num(s.offset_x_mm)),
                    ],
                ),
            );
        }
    }
    crate::a11y::label(ui, RichText::new(tr("dxfc.layers.title")).strong());
    for l in &r.layers {
        let line = tr_args(
            "dxfc.layer.line",
            &[("name", &l.name), ("count", &l.entities.to_string())],
        );
        match l.role {
            Some(role) => crate::a11y::label(
                ui,
                format!(
                    "{line}{}",
                    tr_args("dxfc.layer.role", &[("role", &role_label(role))])
                ),
            ),
            None => crate::a11y::label(ui, line),
        };
    }
    crate::a11y::label(ui, RichText::new(tr("dxfc.omitted.title")).strong());
    let mut none = true;
    for n in r.omissions() {
        none = false;
        crate::a11y::label(
            ui,
            RichText::new(construction_note_text(n)).color(pal().error),
        );
    }
    if none {
        crate::a11y::label(ui, tr("dxfc.omitted.none"));
    }
    let remarks: Vec<&ConstructionNote> = r.remarks().collect();
    if !remarks.is_empty() {
        crate::a11y::label(ui, RichText::new(tr("dxfc.remarks.title")).strong());
        for n in remarks {
            crate::a11y::label(
                ui,
                RichText::new(construction_note_text(n)).color(pal().muted),
            );
        }
    }
}

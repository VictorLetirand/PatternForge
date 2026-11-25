//! Import SVG et PDF vectoriel (US-12.4) : fenêtre « Importer un SVG / PDF » (fichier, unité de
//! lecture avec aperçu des dimensions, échelle, feuille cible nouvelle ou existante, rapport de ce
//! qui est ignoré, bouton d'import).
//!
//! L'import passe par **une seule** commande (`ImportGeometry` ou `ImportGeometryNewSheet`)
//! calculée d'après l'aperçu affiché : ce que l'on voit est ce qui est créé. Un fichier illisible
//! ou un refus de la commande ne laisse aucune trace dans le projet. Rien de ce module n'est
//! enregistré : c'est un état d'interface.

use crate::a11y::ComboExt;
use crate::app::App;
use crate::editor::Selection;
use crate::palette::pal;
use eframe::egui::{self, RichText};
use pf_core::{input::format_number, Command, SheetId};
use pf_export::pdf_import::PdfDocument;
use pf_export::svg_import::{SvgDocument, SvgUnit, SvgUnitChoice};
use pf_export::vector_import::{
    IgnoreReason, NoteKind, VectorBuild, VectorImportError, MAX_FILE_BYTES, MAX_SCALE, MIN_SCALE,
};
use pf_i18n::{tr, tr_args};
use std::path::Path;

pub enum Source {
    Svg(SvgDocument),
    Pdf(PdfDocument),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Target {
    New,
    Existing(SheetId),
}

/// Fenêtre « Importer un SVG / PDF » : état d'interface, jamais enregistré.
pub struct VectorDialog {
    pub file_name: String,
    pub source: Source,
    pub unit: SvgUnitChoice,
    /// Échelle saisie en pourcentage (texte tel que tapé).
    pub scale_text: String,
    pub page: usize,
    pub target: Target,
    pub new_name: String,
    /// Feuille proposée quand l'utilisatrice bascule sur « existante ».
    pub existing: SheetId,
    cache: Option<(Key, Result<VectorBuild, VectorImportError>)>,
}

#[derive(Clone, Copy, PartialEq)]
struct Key {
    unit: SvgUnitChoice,
    scale: u64,
    page: usize,
}

/// Pourcentage saisi -> facteur ; `None` si illisible ou hors bornes.
pub fn parse_scale(text: &str) -> Option<f64> {
    let t = text.trim().replace(',', ".");
    let pct: f64 = t.trim_end_matches('%').trim().parse().ok()?;
    let k = pct / 100.0;
    (k.is_finite() && (MIN_SCALE..=MAX_SCALE).contains(&k)).then_some(k)
}

impl VectorDialog {
    pub fn new(file_name: String, source: Source, sheet: SheetId) -> VectorDialog {
        let stem = Path::new(&file_name)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| tr("vimp.target.new"));
        VectorDialog {
            file_name,
            source,
            unit: SvgUnitChoice::Declared,
            scale_text: "100".into(),
            page: 1,
            target: Target::New,
            new_name: stem,
            existing: sheet,
            cache: None,
        }
    }

    /// Aperçu pour les choix courants (calculé une fois par choix) ; `None` si l'échelle saisie
    /// est invalide.
    pub fn preview(&mut self) -> Option<&Result<VectorBuild, VectorImportError>> {
        let scale = parse_scale(&self.scale_text)?;
        let key = Key {
            unit: self.unit,
            scale: scale.to_bits(),
            page: self.page,
        };
        if self.cache.as_ref().is_none_or(|(k, _)| *k != key) {
            let built = match &self.source {
                Source::Svg(d) => d.build_choice(self.unit, scale),
                Source::Pdf(d) => d.build(self.page, scale),
            };
            self.cache = Some((key, built));
        }
        self.cache.as_ref().map(|(_, r)| r)
    }

    fn is_pdf(&self) -> bool {
        matches!(self.source, Source::Pdf(_))
    }
}

/// Message d'un fichier refusé (aucun objet créé).
pub fn import_error(e: &VectorImportError) -> String {
    match e {
        VectorImportError::TooLarge { bytes } => tr_args(
            "vimp.err.too_large",
            &[
                ("bytes", &bytes.to_string()),
                ("max", &MAX_FILE_BYTES.to_string()),
            ],
        ),
        VectorImportError::NotSvg => tr("vimp.err.not_svg"),
        VectorImportError::NotPdf => tr("vimp.err.not_pdf"),
        VectorImportError::Xml { offset } => {
            tr_args("vimp.err.xml", &[("offset", &offset.to_string())])
        }
        VectorImportError::Truncated => tr("vimp.err.truncated"),
        VectorImportError::Structure(what) => tr_args("vimp.err.structure", &[("what", what)]),
        VectorImportError::Encrypted => tr("vimp.err.encrypted"),
        VectorImportError::Unsupported(which) => match *which {
            "filter" => tr("vimp.err.unsupported.filter"),
            "predictor" => tr("vimp.err.unsupported.predictor"),
            "object_streams" => tr("vimp.err.unsupported.object_streams"),
            "total_too_big" => tr("vimp.err.unsupported.total_too_big"),
            _ => tr("vimp.err.unsupported.too_big"),
        },
        VectorImportError::TooManyObjects { count } => tr_args(
            "vimp.err.too_many_objects",
            &[
                ("count", &count.to_string()),
                ("max", &pf_core::MAX_IMPORT_OBJECTS.to_string()),
            ],
        ),
        VectorImportError::BadScale => tr("vimp.err.bad_scale"),
        VectorImportError::PageOutOfRange { page, pages } => tr_args(
            "vimp.err.page",
            &[("page", &page.to_string()), ("pages", &pages.to_string())],
        ),
        VectorImportError::NoPage => tr("vimp.err.no_page"),
        VectorImportError::TooManyElements { max } => {
            tr_args("vimp.err.too_many_elements", &[("max", &max.to_string())])
        }
    }
}

fn reason_text(r: IgnoreReason) -> String {
    match r {
        IgnoreReason::Unsupported => tr("vimp.reason.unsupported"),
        IgnoreReason::Invalid => tr("vimp.reason.invalid"),
        IgnoreReason::Degenerate => tr("vimp.reason.degenerate"),
        IgnoreReason::OutOfRange => tr("vimp.reason.out_of_range"),
        IgnoreReason::TooComplex => tr("vimp.reason.too_complex"),
        IgnoreReason::Unterminated => tr("vimp.reason.unterminated"),
    }
}

fn note_text(kind: NoteKind, count: usize) -> String {
    let c = count.to_string();
    let a = [("count", c.as_str())];
    match kind {
        NoteKind::StyleIgnored => tr_args("vimp.note.style_ignored", &a),
        NoteKind::ClipIgnored => tr_args("vimp.note.clip_ignored", &a),
        NoteKind::QuadraticRaised => tr_args("vimp.note.quadratic_raised", &a),
        NoteKind::Flattened => tr_args("vimp.note.flattened", &a),
        NoteKind::SemicircleSnapped => tr_args("vimp.note.semicircle_snapped", &a),
        NoteKind::PageRotation => tr("vimp.note.page_rotation"),
        NoteKind::ClipPathDropped => tr_args("vimp.note.clip_path_dropped", &a),
        NoteKind::UserUnit => tr("vimp.note.user_unit"),
    }
}

fn unit_choice_label(u: SvgUnit) -> String {
    match u {
        SvgUnit::Millimetre => tr("vimp.unit.mm"),
        SvgUnit::Centimetre => tr("vimp.unit.cm"),
        SvgUnit::Inch => tr("vimp.unit.in"),
        SvgUnit::Point => tr("vimp.unit.pt"),
        SvgUnit::Pica | SvgUnit::Pixel => tr("vimp.unit.px"),
    }
}

fn num(v: f64) -> String {
    format_number(v, 2)
}

impl App {
    /// Menu Fichier > « Importer un SVG / PDF… » : choix du fichier puis fenêtre d'aperçu.
    pub fn open_vector_import(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("SVG / PDF", &["svg", "pdf"])
            .pick_file()
        else {
            return;
        };
        self.load_vector_import(&path);
    }

    /// Lit le fichier et ouvre la fenêtre. Un fichier trop gros, illisible ou refusé affiche une
    /// erreur : aucune fenêtre, aucun changement du projet.
    pub fn load_vector_import(&mut self, path: &Path) {
        if let Ok(m) = std::fs::metadata(path) {
            if m.len() > MAX_FILE_BYTES as u64 {
                self.editor
                    .error(import_error(&VectorImportError::TooLarge {
                        bytes: m.len() as usize,
                    }));
                return;
            }
        }
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                self.error_dialog = Some(tr_args("vimp.err.read", &[("detail", &e.to_string())]));
                return;
            }
        };
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let is_pdf = ext == "pdf" || (ext != "svg" && bytes.starts_with(b"%PDF"));
        let source = if is_pdf {
            PdfDocument::parse(&bytes).map(Source::Pdf)
        } else {
            SvgDocument::parse(&bytes).map(Source::Svg)
        };
        match source {
            Ok(source) => {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                self.vector_import = Some(VectorDialog::new(name, source, self.editor.sheet));
            }
            Err(e) => self.editor.error(import_error(&e)),
        }
    }

    /// Importe d'après l'aperçu affiché : une seule commande ; un refus laisse le projet intact
    /// et la fenêtre ouverte.
    pub fn run_vector_import(&mut self) -> bool {
        let Some(d) = self.vector_import.as_mut() else {
            return false;
        };
        let (target, name, new_name) = (d.target, d.file_name.clone(), d.new_name.clone());
        let Some(Ok(build)) = d.preview().cloned() else {
            return false;
        };
        if build.objects.is_empty() {
            return false;
        }
        let command = match target {
            Target::New => Command::ImportGeometryNewSheet {
                name: new_name,
                objects: build.objects,
            },
            Target::Existing(sheet) => Command::ImportGeometry {
                sheet,
                objects: build.objects,
            },
        };
        let Some(out) = self.editor.exec(command) else {
            return false;
        };
        if let Some(sheet) = out.sheet {
            self.editor.set_sheet(sheet);
        }
        self.editor.selection = Selection {
            points: out.points.clone(),
            entities: out.entities.clone(),
            ..Selection::default()
        };
        let sheet_name = out
            .sheet
            .and_then(|s| self.editor.project().sheet(s).ok())
            .map(|s| s.name.clone())
            .unwrap_or_default();
        if out.notes.is_empty() {
            self.editor.info(tr_args(
                "vimp.status.done",
                &[
                    ("count", &out.entities.len().to_string()),
                    ("name", &name),
                    ("sheet", &sheet_name),
                ],
            ));
        }
        self.fit_pending = true;
        self.vector_import = None;
        true
    }

    pub fn vector_import_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.vector_import.take() else {
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
        if !sheets.iter().any(|(id, _)| *id == d.existing) {
            d.existing = self.editor.sheet;
        }
        egui::Window::new(tr("vimp.title"))
            .id(egui::Id::new("vector_import_window"))
            .open(&mut open)
            .collapsible(false)
            .default_width(580.0)
            .show(ctx, |ui| {
                crate::panels::note_paragraph(ui, "vimp.intro", &tr("vimp.intro"));
                let (kind, count) = match &d.source {
                    Source::Svg(s) => (tr("vimp.kind.svg"), s.element_count()),
                    Source::Pdf(p) => (tr("vimp.kind.pdf"), p.page_count()),
                };
                let args = [
                    ("name", d.file_name.as_str()),
                    ("kind", kind.as_str()),
                    ("count", &count.to_string()),
                ];
                let text = if d.is_pdf() {
                    tr_args("vimp.file.pdf", &args)
                } else {
                    tr_args("vimp.file", &args)
                };
                crate::a11y::label(ui, text);
                ui.separator();
                self.unit_section(ui, &mut d);
                ui.horizontal(|ui| {
                    crate::a11y::label(ui, tr("vimp.scale"));
                    crate::a11y::add_text(
                        ui,
                        egui::TextEdit::singleline(&mut d.scale_text).desired_width(80.0),
                    );
                });
                if parse_scale(&d.scale_text).is_none() {
                    crate::a11y::label(ui, RichText::new(tr("vimp.scale.bad")).color(pal().error));
                }
                ui.separator();
                crate::a11y::label(ui, RichText::new(tr("vimp.target.title")).strong());
                ui.radio_value(&mut d.target, Target::New, tr("vimp.target.new"));
                if d.target == Target::New {
                    ui.horizontal(|ui| {
                        crate::a11y::label(ui, tr("vimp.target.new_name"));
                        crate::a11y::add_text(
                            ui,
                            egui::TextEdit::singleline(&mut d.new_name).desired_width(220.0),
                        );
                    });
                }
                let existing = Target::Existing(d.existing);
                if ui
                    .radio(
                        matches!(d.target, Target::Existing(_)),
                        tr("vimp.target.existing"),
                    )
                    .clicked()
                {
                    d.target = existing;
                }
                if matches!(d.target, Target::Existing(_)) {
                    ui.horizontal(|ui| {
                        crate::a11y::label(ui, tr("vimp.target.existing_pick"));
                        let current = sheets
                            .iter()
                            .find(|(id, _)| *id == d.existing)
                            .map(|(_, n)| n.clone())
                            .unwrap_or_default();
                        let mut chosen = d.existing;
                        egui::ComboBox::from_id_salt("vector_import_sheet")
                            .selected_text(current)
                            .show_ui_named(ui, |ui| {
                                for (id, name) in &sheets {
                                    ui.selectable_value(&mut chosen, *id, name);
                                }
                            });
                        d.existing = chosen;
                        d.target = Target::Existing(chosen);
                    });
                }
                ui.separator();
                crate::a11y::label(ui, RichText::new(tr("vimp.preview.title")).strong());
                let mut importable = false;
                let name_ok = d.target != Target::New || !d.new_name.trim().is_empty();
                egui::ScrollArea::vertical()
                    .max_height(300.0)
                    .id_salt("vector_import_preview")
                    .show(ui, |ui| match d.preview() {
                        None => {}
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
                        .add_enabled(importable && name_ok, egui::Button::new(tr("vimp.import")))
                        .clicked()
                    {
                        import = true;
                    }
                    if ui.button(tr("vimp.close")).clicked() {
                        close = true;
                    }
                });
            });
        self.vector_import = Some(d);
        if import {
            self.run_vector_import();
        }
        if close || !open {
            self.vector_import = None;
        }
    }

    fn unit_section(&self, ui: &mut egui::Ui, d: &mut VectorDialog) {
        match &d.source {
            Source::Svg(s) => {
                crate::a11y::label(ui, RichText::new(tr("vimp.unit.title")).strong());
                let label = match s.declared_mm_per_unit() {
                    Some(mm) => tr_args("vimp.unit.declared", &[("mm", &format_number(mm, 4))]),
                    None => tr("vimp.unit.default_px"),
                };
                ui.radio_value(&mut d.unit, SvgUnitChoice::Declared, label);
                for u in SvgUnit::CHOICES {
                    ui.radio_value(&mut d.unit, SvgUnitChoice::Fixed(u), unit_choice_label(u));
                }
            }
            Source::Pdf(p) => {
                crate::a11y::label(ui, tr("vimp.pdf.units"));
                let pages = p.page_count();
                ui.horizontal(|ui| {
                    crate::a11y::label(
                        ui,
                        tr_args("vimp.pdf.page", &[("count", &pages.to_string())]),
                    );
                    crate::a11y::add_drag(
                        ui,
                        egui::DragValue::new(&mut d.page).range(1..=pages.max(1)),
                    );
                });
            }
        }
    }
}

fn preview_body(ui: &mut egui::Ui, b: &VectorBuild) {
    match b.bbox_mm {
        Some(bb) => {
            crate::a11y::label(
                ui,
                tr_args(
                    "vimp.preview.size",
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
                RichText::new(tr("vimp.preview.nothing")).color(pal().error),
            );
        }
    }
    crate::a11y::label(
        ui,
        tr_args(
            "vimp.preview.scale",
            &[("k", &format_number(b.mm_per_unit, 4))],
        ),
    );
    crate::a11y::label(
        ui,
        tr_args(
            "vimp.preview.tolerance",
            &[("tol", &format_number(b.tolerance_mm, 2))],
        ),
    );
    crate::a11y::label(
        ui,
        tr_args(
            "vimp.preview.objects",
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
            tr_args("vimp.preview.counts", &[("list", &list.join(", "))]),
        );
    }
    crate::a11y::label(ui, RichText::new(tr("vimp.ignored.title")).strong());
    if b.report.ignored.is_empty() {
        crate::a11y::label(ui, tr("vimp.ignored.none"));
    }
    for i in &b.report.ignored {
        crate::a11y::label(
            ui,
            RichText::new(tr_args(
                "vimp.ignored.line",
                &[
                    ("kind", &i.kind),
                    ("count", &i.count.to_string()),
                    ("reason", &reason_text(i.reason)),
                ],
            ))
            .color(pal().error),
        );
    }
    if !b.report.notes.is_empty() {
        crate::a11y::label(ui, RichText::new(tr("vimp.notes.title")).strong());
        for n in &b.report.notes {
            crate::a11y::label(
                ui,
                RichText::new(note_text(n.kind, n.count)).color(pal().muted),
            );
        }
    }
}

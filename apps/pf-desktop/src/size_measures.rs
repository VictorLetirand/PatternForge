//! US-09.6a — fenêtre « Mesures par taille » du dock (menu Affichage, sans raccourci) : le
//! tableau **mesures × tailles** d'une gamme, relu par `Project::size_measures` (calcul dérivé,
//! jamais stocké, aucune `Command`). Lecture seule.
//!
//! Trois sections repliées au départ (pièces, coutures, tracés). Chaque ligne donne la valeur dans
//! chaque taille (colonne de la base marquée par une icône dessinée), une colonne « Évol. » (pas
//! moyen entre tailles consécutives ; le détail en mm et en % est dans l'infobulle) et, quand
//! l'évolution est irrégulière, une ligne d'alerte avec une icône dessinée **et** un texte. Un clic
//! sur une ligne sélectionne l'objet (tracé ou contour de la pièce dans le plan, couple dans la
//! fenêtre « Coutures »). « Copier en CSV » met le texte dans le presse-papiers (aucun fichier) ;
//! « Exporter en CSV… » / « Exporter en PDF… » (US-09.8b, `pf_export::size_measures_export`)
//! écrivent le tableau de la gamme dans le seul fichier choisi, projet inchangé.
//!
//! Aucun calcul métier ici : tout vient du cœur. Le tableau est recalculé seulement quand le
//! projet change (le projet est comparé à celui de la dernière image).

use crate::app::App;
use crate::diagnostic::{object_bbox, severity_icon};
use crate::dock::ToolWin;
use crate::grading::section_header;
use crate::sizes::{cell, marker_icon};
use eframe::egui::{self, RichText};
use pf_core::input::format_number;
use pf_core::{
    DiagnosticObject, DiagnosticSeverity, MeasureGroup, MeasureKind, MeasureUnit, Project,
    SizeMeasureRow, SizeMeasures, SizeMeasuresError, SizeRangeId,
};
use pf_export::report::{csv_number, csv_text, ReportFormat};
use pf_export::size_measures_export::{format_size_measures, size_alert_text, size_row_label};
use pf_geometry::Bbox;
use pf_i18n::{lang, tr, tr_args};
use pf_ui::components::{button, ButtonKind};
use std::path::Path;

const W_LABEL: f32 = 140.0;
/// Largeur minimale d'une colonne de valeurs et marge ajoutée au texte le plus large.
const MIN_VALUE: f32 = 40.0;
const PAD: f32 = 8.0;
/// Largeur de l'icône de base dessinée dans l'en-tête de colonne.
const BASE_ICON: f32 = 18.0;

/// Dernier tableau calculé et le projet dont il vient.
#[derive(Debug, Clone)]
struct Cache {
    project: Project,
    range: SizeRangeId,
    table: Result<SizeMeasures, SizeMeasuresError>,
}

/// État de la fenêtre (interface seulement, jamais enregistré).
#[derive(Debug, Clone, Default)]
pub struct SizeMeasuresDialog {
    /// Gamme affichée ; `None` = la première du projet.
    pub range: Option<SizeRangeId>,
    /// Sections dépliées (repliées au départ).
    pub pieces_open: bool,
    pub seams_open: bool,
    pub paths_open: bool,
    /// Dernière confirmation de copie, localisée.
    notice: Option<String>,
    cache: Option<Cache>,
}

/// Nombre à 0,1 près, sans décimale quand il est entier (« 1960 », « 30,5 »).
fn compact(x: f64) -> String {
    let r = (x * 10.0).round() / 10.0;
    if r == 0.0 {
        "0".to_string()
    } else if r.fract() == 0.0 {
        format!("{r:.0}")
    } else {
        format_number(r, 1)
    }
}

/// Nombre d'une ligne : une longueur suit l'unité d'affichage (US-16.5), une aire reste en mm².
fn compact_for(kind: &MeasureKind, x: f64) -> String {
    use pf_core::units::{display_units, len_number, LengthUnit};
    if kind.unit() == MeasureUnit::Mm && display_units().unit != LengthUnit::Mm {
        len_number(x, 1)
    } else {
        compact(x)
    }
}

fn signed(kind: &MeasureKind, x: f64) -> String {
    let text = compact_for(kind, x);
    if x > 0.0 && text != "0" {
        format!("+{text}")
    } else {
        text
    }
}

fn unit_text(kind: &MeasureKind) -> &'static str {
    match kind.unit() {
        MeasureUnit::Mm => pf_core::units::display_units().unit.suffix(),
        MeasureUnit::Mm2 => "mm²",
    }
}

/// Libellé d'une ligne (le même que celui des exports CSV et PDF : `pf_export`).
fn row_label(project: &Project, kind: &MeasureKind) -> String {
    size_row_label(lang(), project, kind)
}

fn size_labels(project: &Project, range: SizeRangeId) -> Vec<String> {
    project
        .size_range(range)
        .map(|r| r.sizes.iter().map(|s| s.label.clone()).collect())
        .unwrap_or_default()
}

/// Valeur d'une cellule : les aires en mm² entiers, les longueurs à 0,1 mm près.
fn value_text(kind: &MeasureKind, v: f64) -> String {
    match kind.unit() {
        MeasureUnit::Mm2 => format!("{v:.0}"),
        MeasureUnit::Mm => compact_for(kind, v),
    }
}

fn percent_text(p: Option<f64>) -> String {
    p.map_or_else(|| "—".to_string(), |p| format!("{} %", signed_pct(p)))
}

fn signed_pct(p: f64) -> String {
    if p > 0.0 {
        format!("+{}", format_number(p, 1))
    } else {
        format_number(p, 1)
    }
}

fn alert_text(row: &SizeMeasureRow, labels: &[String]) -> Option<String> {
    size_alert_text(lang(), row, labels)
}

/// Détail des pas d'une ligne (infobulle de la colonne « Évol. »).
fn steps_tip(row: &SizeMeasureRow, labels: &[String]) -> String {
    let unit = unit_text(&row.kind);
    row.steps
        .iter()
        .enumerate()
        .map(|(i, s)| match s {
            Some(s) => tr_args(
                "sizemeas.step",
                &[
                    ("from", labels.get(i).map_or("?", String::as_str)),
                    ("to", labels.get(i + 1).map_or("?", String::as_str)),
                    ("delta", &signed(&row.kind, s.delta)),
                    ("unit", unit),
                    ("percent", &percent_text(s.percent)),
                ],
            ),
            None => tr("sizemeas.cell.unreadable"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Tableau en texte CSV : séparateur « ; », point décimal, 2 décimales, cellule illisible vide,
/// une ligne par mesure (mêmes libellés que la fenêtre), aucun fichier.
pub(crate) fn csv(project: &Project, table: &SizeMeasures) -> String {
    let labels = size_labels(project, table.range);
    let mut head = vec![tr("sizemeas.csv.measure"), tr("sizemeas.csv.unit")];
    head.extend(labels);
    head.push(tr("sizemeas.csv.trend"));
    head.push(tr("sizemeas.csv.alert"));
    let mut out = head
        .iter()
        .map(|h| csv_text(h))
        .collect::<Vec<_>>()
        .join(";");
    out.push_str("\r\n");
    for row in &table.rows {
        let mut cells = vec![
            csv_text(&row_label(project, &row.kind)),
            unit_text(&row.kind).to_string(),
        ];
        cells.extend(row.values.iter().map(|v| csv_number(*v)));
        cells.push(csv_number(row.mean_step));
        cells.push(if row.irregularity.is_some() {
            csv_text(&tr("sizemeas.csv.irregular"))
        } else {
            String::new()
        });
        out.push_str(&cells.join(";"));
        out.push_str("\r\n");
    }
    out
}

/// Emprise d'une pièce : celle de ses tracés de contour.
fn entities_bbox(
    project: &Project,
    sheet: pf_core::SheetId,
    entities: &[pf_core::EntityId],
) -> Option<Bbox> {
    let s = project.sheet(sheet).ok()?;
    let mut out: Option<Bbox> = None;
    for e in entities {
        if let Some(b) = object_bbox(s, &DiagnosticObject::Entity(*e)) {
            match out.as_mut() {
                Some(acc) => acc.union(b),
                None => out = Some(b),
            }
        }
    }
    out
}

impl App {
    /// Menu Affichage › « Mesures par taille… » (sans raccourci).
    pub fn open_size_measures(&mut self) {
        if self.size_measures.is_none() {
            self.size_measures = Some(SizeMeasuresDialog::default());
        }
    }

    /// Clic sur une ligne : sélectionne l'objet (feuille de l'objet active, vue cadrée) ; un
    /// couple de coutures est choisi dans la fenêtre « Coutures ».
    pub fn activate_measure(&mut self, kind: MeasureKind) {
        let target = match kind {
            MeasureKind::SeamLength { pair, .. } | MeasureKind::SeamGap { pair } => {
                if !self.editor.seam.open {
                    self.open_seams();
                }
                self.editor.seam.selected = Some(pair);
                return;
            }
            MeasureKind::PathLength { sheet, entity } => Some((sheet, vec![entity])),
            MeasureKind::PiecePerimeter { piece }
            | MeasureKind::PieceArea { piece }
            | MeasureKind::PieceCutPerimeter { piece } => {
                let project = self.editor.project();
                match (project.piece_sheet(piece), project.piece(piece)) {
                    (Ok(s), Ok(p)) => Some((s.id, p.outline.iter().map(|e| e.entity).collect())),
                    _ => None,
                }
            }
        };
        let Some((sheet, entities)) = target else {
            return;
        };
        if self.plan.is_some() {
            self.leave_plan();
        }
        self.editor.set_sheet(sheet);
        self.editor.selection.clear();
        self.editor
            .selection
            .entities
            .extend(entities.iter().copied());
        if let Some(bbox) = entities_bbox(self.editor.project(), sheet, &entities) {
            self.focus_on(bbox);
        }
    }

    /// Écrit le tableau des mesures de la gamme `range` dans `path` (CSV ou PDF), dans la langue
    /// courante. Lecture seule : le projet n'est pas modifié et seul `path` est écrit ; un échec
    /// (gamme disparue, destination illisible) ouvre la boîte d'erreur, sans fichier.
    pub fn write_size_measures_to(
        &mut self,
        range: SizeRangeId,
        format: ReportFormat,
        path: &Path,
    ) -> bool {
        let project = self.editor.project();
        let table = match project.size_measures(range) {
            Ok(t) => t,
            Err(e) => {
                self.error_dialog = Some(tr_args("export.failed", &[("detail", &e.to_string())]));
                return false;
            }
        };
        let out = format_size_measures(project, &table, format, lang());
        let range_name = project
            .size_range(range)
            .map(|r| r.name.clone())
            .unwrap_or_default();
        let what = match format {
            ReportFormat::Pdf => "PDF",
            ReportFormat::Csv => "CSV",
        };
        match out.write_to(path) {
            Ok(()) => {
                let note = tr_args(
                    "export.note.size_measures",
                    &[
                        ("rows", &table.rows.len().to_string()),
                        ("range", &range_name),
                    ],
                );
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

    fn save_size_measures_dialog(&mut self, range: SizeRangeId, format: ReportFormat) -> bool {
        let stem = Self::safe_file_stem(&self.editor.project().name);
        let ext = format.extension();
        let Some(path) = rfd::FileDialog::new()
            .add_filter(ext.to_uppercase(), &[ext])
            .set_file_name(format!("{stem}-mesures.{ext}"))
            .save_file()
        else {
            return false;
        };
        self.write_size_measures_to(range, format, &path)
    }

    pub fn size_measures_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.size_measures.take() else {
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
        // Gamme affichée : celle choisie si elle existe encore, sinon la première.
        let range = d
            .range
            .filter(|id| ranges.iter().any(|(r, _)| r == id))
            .or(ranges.first().map(|(r, _)| *r));
        d.range = range;
        // Tableau recalculé seulement si le projet ou la gamme a changé.
        if let Some(range) = range {
            let stale = d
                .cache
                .as_ref()
                .is_none_or(|c| c.range != range || &c.project != self.editor.project());
            if stale {
                d.cache = Some(Cache {
                    project: self.editor.project().clone(),
                    range,
                    table: self.editor.project().size_measures(range),
                });
            }
        } else {
            d.cache = None;
        }
        let mut open = true;
        let mut close = false;
        let mut copy = false;
        let mut export: Option<ReportFormat> = None;
        let mut clicked: Option<MeasureKind> = None;
        let mut chosen: Option<SizeRangeId> = None;
        let slot = crate::dock::slot(ctx, &self.dock, ToolWin::SizeMeasures);
        let width = self.dock.content_width(ToolWin::SizeMeasures);
        let project = self.editor.project();
        let cache = d.cache.take();
        egui::Window::new(tr("sizemeas.title"))
            .id(ToolWin::SizeMeasures.id())
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
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("sizemeas.intro")).color(secondary),
                        );
                        crate::a11y::label(ui, RichText::new(tr("sizemeas.readonly")).strong());
                        ui.add_space(4.0);
                        let table = cache.as_ref().and_then(|c| c.table.as_ref().ok());
                        match (range, table) {
                            (None, _) => {
                                crate::a11y::label(
                                    ui,
                                    RichText::new(tr("sizemeas.no_sizes")).color(secondary),
                                );
                            }
                            (Some(range), Some(table)) => {
                                if ranges.len() > 1 {
                                    ui.horizontal_wrapped(|ui| {
                                        crate::a11y::label(ui, tr("sizemeas.range"));
                                        for (id, name) in &ranges {
                                            if ui.selectable_label(*id == range, name).clicked() {
                                                chosen = Some(*id);
                                            }
                                        }
                                    });
                                }
                                draw_table(ui, project, table, &mut d, secondary, &mut clicked);
                                ui.add_space(4.0);
                                ui.horizontal_wrapped(|ui| {
                                    copy = button(ui, &tr("sizemeas.copy"), ButtonKind::Secondary)
                                        .clicked();
                                    if button(ui, &tr("sizemeas.export_csv"), ButtonKind::Secondary)
                                        .clicked()
                                    {
                                        export = Some(ReportFormat::Csv);
                                    }
                                    if button(ui, &tr("sizemeas.export_pdf"), ButtonKind::Secondary)
                                        .clicked()
                                    {
                                        export = Some(ReportFormat::Pdf);
                                    }
                                });
                                crate::a11y::label(
                                    ui,
                                    RichText::new(tr("sizemeas.export_note")).color(secondary),
                                );
                                if let Some(n) = &d.notice {
                                    crate::a11y::label(ui, RichText::new(n).color(secondary));
                                }
                            }
                            (Some(_), None) => {
                                crate::a11y::label(
                                    ui,
                                    RichText::new(tr("sizemeas.no_sizes")).color(secondary),
                                );
                            }
                        }
                        ui.add_space(6.0);
                        crate::a11y::label(ui, RichText::new(tr("sizemeas.note")).color(secondary));
                        ui.add_space(6.0);
                        close = button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                    });
            });
        d.cache = cache;
        if let Some(id) = chosen {
            d.range = Some(id);
            d.notice = None;
        }
        if copy {
            if let Some(Cache {
                table: Ok(table), ..
            }) = &d.cache
            {
                ctx.copy_text(csv(self.editor.project(), table));
                d.notice = Some(tr_args(
                    "sizemeas.copied",
                    &[("rows", &table.rows.len().to_string())],
                ));
            }
        }
        if open && !close {
            self.size_measures = Some(d);
        }
        if let (Some(format), Some(range)) = (export, range) {
            self.save_size_measures_dialog(range, format);
        }
        if let Some(kind) = clicked {
            self.activate_measure(kind);
        }
    }
}

fn text_width(ui: &egui::Ui, text: &str) -> f32 {
    let font = egui::TextStyle::Body.resolve(ui.style());
    ui.fonts(|f| {
        f.layout_no_wrap(text.to_string(), font, egui::Color32::PLACEHOLDER)
            .size()
            .x
    })
}

/// Largeurs des colonnes d'une section : celles du texte le plus large (marge comprise), avec un
/// minimum ; un nombre ne déborde jamais sur son voisin.
struct Cols {
    values: Vec<f32>,
    trend: f32,
}

fn columns(
    ui: &egui::Ui,
    rows: &[&SizeMeasureRow],
    table: &SizeMeasures,
    labels: &[String],
) -> Cols {
    let mut values: Vec<f32> = labels
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let icon = if table.sizes.get(i) == Some(&table.base) {
                BASE_ICON
            } else {
                0.0
            };
            (text_width(ui, l) + icon + PAD).max(MIN_VALUE)
        })
        .collect();
    let mut trend = (text_width(ui, &tr("sizemeas.col.trend")) + PAD).max(MIN_VALUE);
    for r in rows {
        for (i, v) in r.values.iter().enumerate() {
            if let (Some(w), Some(v)) = (values.get_mut(i), v) {
                *w = w.max(text_width(ui, &value_text(&r.kind, *v)) + PAD);
            }
        }
        if let Some(m) = r.mean_step {
            trend = trend.max(text_width(ui, &signed(&r.kind, m)) + PAD);
        }
    }
    Cols { values, trend }
}

fn draw_table(
    ui: &mut egui::Ui,
    project: &Project,
    table: &SizeMeasures,
    d: &mut SizeMeasuresDialog,
    secondary: egui::Color32,
    clicked: &mut Option<MeasureKind>,
) {
    let labels = size_labels(project, table.range);
    let alerts = table.irregular_rows().count();
    crate::a11y::label(
        ui,
        tr_args(
            "sizemeas.summary",
            &[
                ("rows", &table.rows.len().to_string()),
                ("alerts", &alerts.to_string()),
            ],
        ),
    );
    crate::a11y::label(ui, RichText::new(tr("sizemeas.units")).color(secondary));
    ui.horizontal(|ui| {
        marker_icon(ui, false, true);
        crate::a11y::label(ui, RichText::new(tr("sizes.base")).color(secondary));
    });
    for (group, key) in [
        (MeasureGroup::Pieces, "sizemeas.group.pieces"),
        (MeasureGroup::Seams, "sizemeas.group.seams"),
        (MeasureGroup::Paths, "sizemeas.group.paths"),
    ] {
        let mut rows: Vec<&SizeMeasureRow> = table
            .rows
            .iter()
            .filter(|r| r.kind.group() == group)
            .collect();
        if rows.is_empty() {
            continue;
        }
        let n_alerts = rows.iter().filter(|r| r.irregularity.is_some()).count();
        let mut title = tr_args(key, &[("count", &rows.len().to_string())]);
        if n_alerts > 0 {
            title = format!(
                "{title} · {}",
                tr_args("sizemeas.group.alerts", &[("count", &n_alerts.to_string())])
            );
        }
        let open = match group {
            MeasureGroup::Pieces => &mut d.pieces_open,
            MeasureGroup::Seams => &mut d.seams_open,
            MeasureGroup::Paths => &mut d.paths_open,
        };
        ui.add_space(4.0);
        section_header(ui, open, title);
        if !*open {
            continue;
        }
        // Les lignes en alerte d'abord (tri stable : l'ordre du cœur est gardé pour le reste).
        rows.sort_by_key(|r| r.irregularity.is_none());
        let cols = columns(ui, &rows, table, &labels);
        egui::ScrollArea::horizontal()
            .id_salt(("sizemeas_table", key))
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                header_row(ui, table, &labels, &cols);
                for row in rows {
                    draw_row(ui, project, row, &labels, &cols, clicked);
                }
            });
    }
}

fn header_row(ui: &mut egui::Ui, table: &SizeMeasures, labels: &[String], cols: &Cols) {
    let right = egui::Layout::right_to_left(egui::Align::Center);
    ui.horizontal(|ui| {
        cell(ui, W_LABEL, |ui| {
            crate::a11y::label(ui, RichText::new(tr("sizemeas.csv.measure")).strong());
        });
        for (i, label) in labels.iter().enumerate() {
            let is_base = table.sizes.get(i) == Some(&table.base);
            cell(ui, cols.values[i], |ui| {
                ui.with_layout(right, |ui| {
                    let r = crate::a11y::label(ui, RichText::new(label).strong());
                    if is_base {
                        marker_icon(ui, false, true);
                        r.on_hover_text(tr_args("size.base_mark", &[("label", label)]));
                    }
                });
            });
        }
        cell(ui, cols.trend, |ui| {
            ui.with_layout(right, |ui| {
                crate::a11y::label(ui, RichText::new(tr("sizemeas.col.trend")).strong());
            });
        });
    });
}

fn draw_row(
    ui: &mut egui::Ui,
    project: &Project,
    row: &SizeMeasureRow,
    labels: &[String],
    cols: &Cols,
    clicked: &mut Option<MeasureKind>,
) {
    let right = egui::Layout::right_to_left(egui::Align::Center);
    let text = row_label(project, &row.kind);
    let flagged: Vec<usize> = row
        .irregularity
        .as_ref()
        .map(|i| i.steps.iter().flat_map(|s| [*s, *s + 1]).collect())
        .unwrap_or_default();
    ui.horizontal(|ui| {
        cell(ui, W_LABEL, |ui| {
            let r = ui
                .add(egui::Button::new(text.as_str()).frame(false).truncate())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(&text);
            if r.clicked() {
                *clicked = Some(row.kind);
            }
        });
        for (i, v) in row.values.iter().enumerate() {
            cell(ui, cols.values[i], |ui| {
                ui.with_layout(right, |ui| {
                    let mut t = RichText::new(
                        v.map_or_else(|| "—".to_string(), |v| value_text(&row.kind, v)),
                    );
                    if flagged.contains(&i) {
                        t = t.strong();
                    }
                    let r = crate::a11y::label(ui, t);
                    if v.is_none() {
                        r.on_hover_text(tr("sizemeas.cell.unreadable"));
                    }
                });
            });
        }
        cell(ui, cols.trend, |ui| {
            ui.with_layout(right, |ui| {
                let r = crate::a11y::label(
                    ui,
                    row.mean_step
                        .map_or_else(|| "—".to_string(), |m| signed(&row.kind, m)),
                );
                if !row.steps.is_empty() {
                    r.on_hover_text(steps_tip(row, labels));
                }
            });
        });
    });
    if let Some(alert) = alert_text(row, labels) {
        ui.horizontal(|ui| {
            severity_icon(ui, DiagnosticSeverity::Warning);
            crate::a11y::label(ui, alert);
        });
    }
}

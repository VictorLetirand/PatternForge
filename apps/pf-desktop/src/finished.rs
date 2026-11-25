//! US-10.3 et US-10.4 — fenêtre « Retouches et mesures finies » du dock (menu Affichage, sans
//! raccourci).
//!
//! Retouches : liste, ajout, renommage, cible, delta, suppression (une commande chacune). Elles
//! s'ajoutent à la valeur du barème **à la lecture** : jamais le tracé.
//!
//! Mesures finies : définition (nom, mesure du barème, couture ou cote, tolérances −/+) et tableau
//! de contrôle **mesures × tailles** lu par `Project::finished_measure_table` (valeur mesurée sur
//! la géométrie de chaque taille, comparée au barème). Chaque cellule dit son état **en toutes
//! lettres** (OK / hors tolérance / non comparable), jamais par la seule couleur. Aucun calcul
//! métier ici ; la case « Avec les retouches » est un réglage d'affichage, jamais enregistré.

use crate::a11y::ComboExt;
use crate::app::App;
use crate::dock::ToolWin;
use crate::grading::section_header;
use crate::measurements::{
    edit, field_u, parse_value, pick_range, range_selector, show_u, unit_label, unit_of, Buffers,
};
use crate::sizes::cell;
use eframe::egui::{self, RichText};
use pf_core::{
    AlterationId, Command, FinishedMeasureId, FinishedReading, FinishedReason, FinishedSource,
    FinishedStatus, MeasurementId, MeasurementUnit, Project, SizeRangeId,
};
use pf_i18n::{tr, tr_args};
use pf_ui::components::{button, ButtonKind};

const W_NAME: f32 = 96.0;
const W_SMALL: f32 = 56.0;
const W_CELL: f32 = 66.0;
const K_ALT_NAME: u8 = 10;
const K_ALT_DELTA: u8 = 11;
const K_FIN_NAME: u8 = 12;
const K_FIN_TM: u8 = 13;
const K_FIN_TP: u8 = 14;

/// État de la fenêtre (interface seulement, jamais enregistré).
#[derive(Debug, Clone)]
pub struct FinishedDialog {
    pub range: Option<SizeRangeId>,
    buffers: Buffers,
    pub alt_open: bool,
    pub def_open: bool,
    pub table_open: bool,
    /// Réglage d'affichage : la référence du tableau est le barème retouché.
    pub with_alterations: bool,
    alt_name: String,
    alt_target: Option<MeasurementId>,
    alt_delta: String,
    fin_name: String,
    fin_measure: Option<MeasurementId>,
    fin_source: Option<FinishedSource>,
    fin_tm: String,
    fin_tp: String,
    error: Option<String>,
}

impl Default for FinishedDialog {
    fn default() -> Self {
        FinishedDialog {
            range: None,
            buffers: Buffers::new(),
            alt_open: true,
            def_open: true,
            table_open: true,
            with_alterations: true,
            alt_name: String::new(),
            alt_target: None,
            alt_delta: String::new(),
            fin_name: String::new(),
            fin_measure: None,
            fin_source: None,
            fin_tm: String::new(),
            fin_tp: String::new(),
            error: None,
        }
    }
}

enum FAction {
    AltAdd,
    AltRename(AlterationId, String),
    AltDelta(AlterationId, f64),
    AltTarget(AlterationId, MeasurementId),
    AltRemove(AlterationId),
    FinAdd,
    FinEdit(FinishedMeasureId, Option<String>, Option<f64>, Option<f64>),
    FinRemove(FinishedMeasureId),
}

/// Libellé d'une source : couture (rang, pièces) ou cote (feuille, valeur lue).
fn source_label(p: &Project, s: &FinishedSource) -> String {
    match *s {
        FinishedSource::Seam { seam } => match p.seams.iter().position(|x| x.id == seam) {
            Some(i) => {
                let name = |piece| p.piece(piece).map(|x| x.name.clone()).unwrap_or_default();
                tr_args(
                    "finished.src.seam",
                    &[
                        ("n", &(i + 1).to_string()),
                        ("a", &name(p.seams[i].a.piece)),
                        ("b", &name(p.seams[i].b.piece)),
                    ],
                )
            }
            None => tr("finished.src.missing"),
        },
        FinishedSource::FullDimension { sheet, dimension } => match p.sheet(sheet) {
            Ok(sh) => {
                let value = sh
                    .full_dimension_geometry(dimension)
                    .map(|g| g.value_text)
                    .unwrap_or_else(|_| "?".into());
                tr_args(
                    "finished.src.dim",
                    &[("sheet", &sh.name), ("value", &value)],
                )
            }
            Err(_) => tr("finished.src.missing"),
        },
    }
}

/// Sources disponibles pour l'unité donnée (couture = mm ; cote d'angle = degrés).
fn sources(p: &Project, unit: Option<MeasurementUnit>) -> Vec<FinishedSource> {
    use pf_core::FullDimensionKind;
    let mut out = Vec::new();
    if unit != Some(MeasurementUnit::Deg) {
        out.extend(p.seams.iter().map(|s| FinishedSource::Seam { seam: s.id }));
    }
    for sh in &p.sheets {
        for d in &sh.full_dimensions {
            let deg = matches!(d.kind, FullDimensionKind::Angle { .. });
            if unit.is_none_or(|u| (u == MeasurementUnit::Deg) == deg) {
                out.push(FinishedSource::FullDimension {
                    sheet: sh.id,
                    dimension: d.id,
                });
            }
        }
    }
    out
}

fn reason_text(r: FinishedReason) -> String {
    tr(match r {
        FinishedReason::SourceDegenerate => "report.fin.reason.source",
        FinishedReason::NoReferenceValue => "report.fin.reason.no_reference",
        FinishedReason::UnitMismatch => "report.fin.reason.unit",
        FinishedReason::AlterationOutOfRange => "report.fin.reason.alteration",
        FinishedReason::DeadMeasurement => "report.fin.reason.dead",
    })
}

/// Valeur signée affichée (avec `+` pour un écart positif).
fn signed(unit: MeasurementUnit, v: f64) -> String {
    if v > 0.0 {
        format!("+{}", show_u(unit, v))
    } else {
        show_u(unit, v)
    }
}

/// Texte exact d'un champ d'écart (signe `+` conservé, valeur non arrondie).
fn signed_field(unit: MeasurementUnit, v: f64) -> String {
    if v > 0.0 {
        format!("+{}", field_u(unit, v))
    } else {
        field_u(unit, v)
    }
}

fn measure_combo(
    ui: &mut egui::Ui,
    salt: &str,
    project: &Project,
    current: &mut Option<MeasurementId>,
) -> bool {
    let name = |id: Option<MeasurementId>| {
        id.and_then(|m| project.measurement(m).ok())
            .map(|m| format!("{} ({})", m.name, unit_label(m.unit)))
            .unwrap_or_else(|| tr("finished.choose"))
    };
    let mut changed = false;
    egui::ComboBox::from_id_salt(salt)
        .selected_text(name(*current))
        .show_ui_named(ui, |ui| {
            for m in &project.measurements {
                let label = format!("{} ({})", m.name, unit_label(m.unit));
                if ui.selectable_label(*current == Some(m.id), label).clicked() {
                    *current = Some(m.id);
                    changed = true;
                }
            }
        });
    changed
}

impl App {
    /// Menu Affichage › « Retouches et mesures finies… » (sans raccourci).
    pub fn open_finished(&mut self) {
        if self.finished.is_none() {
            self.finished = Some(FinishedDialog::default());
        }
    }

    fn finished_act(&mut self, d: &mut FinishedDialog, action: FAction) {
        let project = self.editor.project().clone();
        let unit_of = |m: Option<MeasurementId>| {
            m.and_then(|m| project.measurement(m).ok())
                .map_or(MeasurementUnit::Mm, |m| m.unit)
        };
        let result: Result<(), String> = match action {
            FAction::AltAdd => match (
                d.alt_target,
                parse_value(unit_of(d.alt_target), &d.alt_delta),
            ) {
                (Some(target), Ok(delta)) => {
                    let r = self.measure_exec(Command::AddAlteration {
                        name: d.alt_name.clone(),
                        target,
                        delta,
                    });
                    if r.is_ok() {
                        d.alt_name.clear();
                        d.alt_delta.clear();
                    }
                    r.map(|_| ())
                }
                (None, _) => Err(tr("finished.err.no_target")),
                (_, Err(_)) => Err(tr_args("bareme.err.number", &[("text", &d.alt_delta)])),
            },
            FAction::AltRename(a, name) => self
                .measure_exec(Command::RenameAlteration {
                    alteration: a,
                    name,
                })
                .map(|_| ()),
            FAction::AltDelta(a, delta) => self
                .measure_exec(Command::SetAlterationDelta {
                    alteration: a,
                    delta,
                })
                .map(|_| ()),
            FAction::AltTarget(a, target) => self
                .measure_exec(Command::SetAlterationTarget {
                    alteration: a,
                    target,
                })
                .map(|_| ()),
            FAction::AltRemove(a) => self
                .measure_exec(Command::RemoveAlteration { alteration: a })
                .map(|_| ()),
            FAction::FinAdd => {
                let unit = unit_of(d.fin_measure);
                match (
                    d.fin_measure,
                    d.fin_source,
                    parse_value(unit, &d.fin_tm),
                    parse_value(unit, &d.fin_tp),
                ) {
                    (Some(measurement), Some(source), Ok(tm), Ok(tp)) => {
                        let r = self.measure_exec(Command::AddFinishedMeasure {
                            name: d.fin_name.clone(),
                            measurement,
                            source,
                            tolerance_minus: tm,
                            tolerance_plus: tp,
                        });
                        if r.is_ok() {
                            d.fin_name.clear();
                        }
                        r.map(|_| ())
                    }
                    (None, ..) => Err(tr("finished.err.no_measure")),
                    (_, None, ..) => Err(tr("finished.err.no_source")),
                    (.., Err(_), _) => Err(tr_args("bareme.err.number", &[("text", &d.fin_tm)])),
                    (.., Err(_)) => Err(tr_args("bareme.err.number", &[("text", &d.fin_tp)])),
                }
            }
            FAction::FinEdit(id, name, tm, tp) => match project.finished_measure(id) {
                Ok(f) => self
                    .measure_exec(Command::SetFinishedMeasure {
                        finished: id,
                        name: name.unwrap_or_else(|| f.name.clone()),
                        measurement: f.measurement,
                        source: f.source,
                        tolerance_minus: tm.unwrap_or(f.tolerance_minus),
                        tolerance_plus: tp.unwrap_or(f.tolerance_plus),
                    })
                    .map(|_| ()),
                Err(e) => Err(e.to_string()),
            },
            FAction::FinRemove(id) => self
                .measure_exec(Command::RemoveFinishedMeasure { finished: id })
                .map(|_| ()),
        };
        d.error = result.err();
    }

    pub fn finished_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.finished.take() else {
            return;
        };
        let project = self.editor.project().clone();
        let tokens = pf_ui::theme::tokens(ctx);
        let (danger, secondary) = (tokens.danger, tokens.text_secondary);
        let ranges: Vec<(SizeRangeId, String)> = project
            .sizes
            .iter()
            .map(|r| (r.id, r.name.clone()))
            .collect();
        d.range = pick_range(&ranges, d.range);
        // Choix de formulaire : une mesure ou une source disparue (annulation) est oubliée.
        if d.alt_target
            .is_some_and(|m| project.measurement(m).is_err())
        {
            d.alt_target = None;
        }
        if d.fin_measure
            .is_some_and(|m| project.measurement(m).is_err())
        {
            d.fin_measure = None;
        }
        let mut open = true;
        let mut close = false;
        let mut actions: Vec<FAction> = Vec::new();
        let slot = crate::dock::slot(ctx, &self.dock, ToolWin::Finished);
        let width = self.dock.content_width(ToolWin::Finished);
        egui::Window::new(tr("finished.title"))
            .id(ToolWin::Finished.id())
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
                            RichText::new(tr("finished.intro")).color(secondary),
                        );
                        if let Some(text) = &d.error {
                            crate::a11y::label(ui, RichText::new(text).color(danger));
                        }
                        ui.add_space(4.0);
                        alterations_section(ui, &mut d, &project, secondary, &mut actions);
                        ui.add_space(4.0);
                        definitions_section(ui, &mut d, &project, secondary, &mut actions);
                        ui.add_space(4.0);
                        table_section(ui, &mut d, &project, &ranges, secondary, danger);
                        ui.add_space(6.0);
                        close = button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                    });
            });
        for a in actions {
            self.finished_act(&mut d, a);
        }
        if open && !close {
            self.finished = Some(d);
        }
    }
}

fn alterations_section(
    ui: &mut egui::Ui,
    d: &mut FinishedDialog,
    project: &Project,
    secondary: egui::Color32,
    actions: &mut Vec<FAction>,
) {
    section_header(
        ui,
        &mut d.alt_open,
        tr_args(
            "finished.alt.section",
            &[("count", &project.alterations.len().to_string())],
        ),
    );
    if !d.alt_open {
        return;
    }
    if project.measurements.is_empty() {
        crate::a11y::label(
            ui,
            RichText::new(tr("finished.alt.no_measures")).color(secondary),
        );
        return;
    }
    if project.alterations.is_empty() {
        crate::a11y::label(ui, RichText::new(tr("finished.alt.none")).color(secondary));
    }
    for a in &project.alterations {
        let unit = project
            .measurement(a.target)
            .map_or(MeasurementUnit::Mm, |m| m.unit);
        ui.horizontal(|ui| {
            cell(ui, W_NAME + 10.0, |ui| {
                if let Some(t) = edit(ui, &mut d.buffers, (K_ALT_NAME, a.id.0, 0), &a.name, W_NAME)
                {
                    actions.push(FAction::AltRename(a.id, t));
                }
            });
            let mut target = Some(a.target);
            if measure_combo(ui, &format!("alt_target_{}", a.id.0), project, &mut target) {
                if let Some(t) = target {
                    actions.push(FAction::AltTarget(a.id, t));
                }
            }
            cell(ui, W_SMALL, |ui| {
                if let Some(t) = edit(
                    ui,
                    &mut d.buffers,
                    (K_ALT_DELTA, a.id.0, 0),
                    &signed_field(unit, a.delta),
                    W_SMALL - 6.0,
                ) {
                    match parse_value(unit, &t) {
                        Ok(v) => actions.push(FAction::AltDelta(a.id, v)),
                        Err(_) => {
                            d.error = Some(tr_args("bareme.err.number", &[("text", &t)]));
                        }
                    }
                }
            });
            if button(ui, &tr("finished.remove"), ButtonKind::Danger).clicked() {
                actions.push(FAction::AltRemove(a.id));
            }
        });
    }
    ui.horizontal(|ui| {
        crate::a11y::label(ui, tr("finished.alt.new"));
        crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(&mut d.alt_name)
                .id(egui::Id::new("finished_alt_name"))
                .desired_width(W_NAME),
        );
        measure_combo(ui, "alt_new_target", project, &mut d.alt_target);
        crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(&mut d.alt_delta)
                .id(egui::Id::new("finished_alt_delta"))
                .hint_text(tr("finished.alt.delta"))
                .desired_width(W_SMALL),
        );
        if button(ui, &tr("finished.add"), ButtonKind::Primary).clicked() {
            actions.push(FAction::AltAdd);
        }
    });
}

fn definitions_section(
    ui: &mut egui::Ui,
    d: &mut FinishedDialog,
    project: &Project,
    secondary: egui::Color32,
    actions: &mut Vec<FAction>,
) {
    section_header(
        ui,
        &mut d.def_open,
        tr_args(
            "finished.fin.section",
            &[("count", &project.finished_measures.len().to_string())],
        ),
    );
    if !d.def_open {
        return;
    }
    if project.finished_measures.is_empty() {
        crate::a11y::label(ui, RichText::new(tr("finished.fin.none")).color(secondary));
    }
    for f in &project.finished_measures {
        let unit = project
            .measurement(f.measurement)
            .map_or(MeasurementUnit::Mm, |m| m.unit);
        ui.horizontal(|ui| {
            cell(ui, W_NAME + 10.0, |ui| {
                if let Some(t) = edit(ui, &mut d.buffers, (K_FIN_NAME, f.id.0, 0), &f.name, W_NAME)
                {
                    actions.push(FAction::FinEdit(f.id, Some(t), None, None));
                }
            });
            crate::a11y::label(ui, tr("finished.fin.tol_minus"));
            cell(ui, W_SMALL - 14.0, |ui| {
                if let Some(t) = edit(
                    ui,
                    &mut d.buffers,
                    (K_FIN_TM, f.id.0, 0),
                    &field_u(unit, f.tolerance_minus),
                    W_SMALL - 20.0,
                ) {
                    match parse_value(unit, &t) {
                        Ok(v) => actions.push(FAction::FinEdit(f.id, None, Some(v), None)),
                        Err(_) => d.error = Some(tr_args("bareme.err.number", &[("text", &t)])),
                    }
                }
            });
            crate::a11y::label(ui, tr("finished.fin.tol_plus"));
            cell(ui, W_SMALL - 14.0, |ui| {
                if let Some(t) = edit(
                    ui,
                    &mut d.buffers,
                    (K_FIN_TP, f.id.0, 0),
                    &field_u(unit, f.tolerance_plus),
                    W_SMALL - 20.0,
                ) {
                    match parse_value(unit, &t) {
                        Ok(v) => actions.push(FAction::FinEdit(f.id, None, None, Some(v))),
                        Err(_) => d.error = Some(tr_args("bareme.err.number", &[("text", &t)])),
                    }
                }
            });
            if button(ui, &tr("finished.remove"), ButtonKind::Danger).clicked() {
                actions.push(FAction::FinRemove(f.id));
            }
        });
        let m = project
            .measurement(f.measurement)
            .map(|m| m.name.clone())
            .unwrap_or_default();
        crate::a11y::label(
            ui,
            RichText::new(format!(
                "{} : {} · {} : {}",
                tr("finished.fin.measure"),
                m,
                tr("finished.fin.source"),
                source_label(project, &f.source)
            ))
            .color(secondary),
        );
    }
    ui.add_space(2.0);
    crate::a11y::label(ui, tr("finished.fin.new"));
    ui.horizontal_wrapped(|ui| {
        crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(&mut d.fin_name)
                .id(egui::Id::new("finished_fin_name"))
                .hint_text(tr("finished.fin.name"))
                .desired_width(W_NAME),
        );
        if measure_combo(ui, "fin_new_measure", project, &mut d.fin_measure) {
            d.fin_source = None;
        }
        let unit = d
            .fin_measure
            .and_then(|m| project.measurement(m).ok())
            .map(|m| m.unit);
        let all = sources(project, unit);
        egui::ComboBox::from_id_salt("fin_new_source")
            .selected_text(
                d.fin_source
                    .map(|s| source_label(project, &s))
                    .unwrap_or_else(|| tr("finished.choose")),
            )
            .show_ui_named(ui, |ui| {
                if all.is_empty() {
                    crate::a11y::label(
                        ui,
                        RichText::new(tr("finished.fin.no_source")).color(secondary),
                    );
                }
                for s in all {
                    if ui
                        .selectable_label(d.fin_source == Some(s), source_label(project, &s))
                        .clicked()
                    {
                        d.fin_source = Some(s);
                    }
                }
            });
    });
    // Tolérances et bouton sur leur propre rangée : une étiquette ne se sépare plus de son champ
    // (PH-6, défaut 4).
    ui.horizontal(|ui| {
        crate::a11y::label(ui, tr("finished.fin.tol_minus"));
        crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(&mut d.fin_tm)
                .id(egui::Id::new("finished_fin_tm"))
                .desired_width(W_SMALL - 20.0),
        );
        crate::a11y::label(ui, tr("finished.fin.tol_plus"));
        crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(&mut d.fin_tp)
                .id(egui::Id::new("finished_fin_tp"))
                .desired_width(W_SMALL - 20.0),
        );
        if button(ui, &tr("finished.add"), ButtonKind::Primary).clicked() {
            actions.push(FAction::FinAdd);
        }
    });
}

fn cell_text(unit: MeasurementUnit, r: &FinishedReading) -> (String, bool) {
    match r.status {
        FinishedStatus::Conforming => (
            format!(
                "{} {}",
                tr("finished.cell.ok"),
                signed(unit, r.gap.unwrap_or(0.0))
            ),
            false,
        ),
        FinishedStatus::OutOfTolerance => (
            format!(
                "{} {}",
                tr("finished.cell.out"),
                signed(unit, r.gap.unwrap_or(0.0))
            ),
            true,
        ),
        FinishedStatus::NotComparable => (tr("finished.cell.na"), false),
    }
}

fn table_section(
    ui: &mut egui::Ui,
    d: &mut FinishedDialog,
    project: &Project,
    ranges: &[(SizeRangeId, String)],
    secondary: egui::Color32,
    danger: egui::Color32,
) {
    section_header(ui, &mut d.table_open, tr("finished.table.section"));
    if !d.table_open {
        return;
    }
    if let Some(id) = range_selector(ui, ranges, d.range) {
        d.range = Some(id);
    }
    let Some(range) = d.range else {
        crate::a11y::label(
            ui,
            RichText::new(tr("finished.table.no_sizes")).color(secondary),
        );
        return;
    };
    ui.checkbox(&mut d.with_alterations, tr("finished.table.with_alt"));
    let Ok(table) = project.finished_measure_table(range, d.with_alterations) else {
        return;
    };
    if project.finished_measures.is_empty() {
        crate::a11y::label(ui, RichText::new(tr("finished.fin.none")).color(secondary));
        return;
    }
    let labels: Vec<String> = table
        .sizes
        .iter()
        .map(|s| {
            project
                .find_size(*s)
                .map(|(r, i)| r.sizes[i].label.clone())
                .unwrap_or_default()
        })
        .collect();
    egui::ScrollArea::horizontal()
        .id_salt("finished_table")
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                cell(ui, W_NAME, |ui| {
                    crate::a11y::label(ui, RichText::new(tr("finished.fin.name")).color(secondary));
                });
                for l in &labels {
                    cell(ui, W_CELL, |ui| {
                        crate::a11y::label(ui, RichText::new(l).color(secondary).strong());
                    });
                }
            });
            for (i, f) in project.finished_measures.iter().enumerate() {
                ui.horizontal(|ui| {
                    cell(ui, W_NAME, |ui| {
                        crate::a11y::label(ui, &f.name);
                    });
                    for col in &table.columns {
                        let r = &col[i];
                        let unit = unit_of(project, f.measurement);
                        let (text, out) = cell_text(unit, r);
                        cell(ui, W_CELL, |ui| {
                            let rich = if out {
                                RichText::new(text).color(danger).strong()
                            } else {
                                RichText::new(text)
                            };
                            let hover = match r.reason {
                                Some(reason) => tr_args(
                                    "finished.table.hover_na",
                                    &[("reason", &reason_text(reason))],
                                ),
                                None => tr_args(
                                    "finished.table.hover",
                                    &[
                                        (
                                            "measured",
                                            &r.measured.map_or("?".into(), |v| show_u(unit, v)),
                                        ),
                                        (
                                            "reference",
                                            &r.reference.map_or("?".into(), |v| show_u(unit, v)),
                                        ),
                                        ("gap", &r.gap.map_or("?".into(), |v| signed(unit, v))),
                                        ("tm", &show_u(unit, r.tolerance_minus)),
                                        ("tp", &show_u(unit, r.tolerance_plus)),
                                    ],
                                ),
                            };
                            crate::a11y::label(ui, rich).on_hover_text(hover);
                        });
                    }
                });
            }
        });
    crate::a11y::label(
        ui,
        RichText::new(tr("finished.table.legend")).color(secondary),
    );
    crate::a11y::label(ui, RichText::new(tr("finished.note")).color(secondary));
}

/// Projet de démonstration de l'étape `retouches` : rectangle 400 x 600 gradué en cinq tailles,
/// barème « Bottom width » (et « Side height » sans valeur en XS), retouche « Width -2 » et deux mesures finies (une conforme, une hors
/// tolérance). Valeurs d'exemple : aucune convention n'est présentée comme validée.
pub(crate) fn demo_project() -> Result<Project, pf_core::CoreError> {
    use pf_core::{FullDimensionKind, PieceSource, PointRef, SizeKind};
    use pf_geometry::PointMm;
    let mut p = Project::new("Finished measurements");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddSizeRange {
        name: "Women".into(),
        kind: SizeKind::Alphanumeric,
        labels: ["XS", "S", "M", "L", "XL"].map(String::from).to_vec(),
        base: 2,
    })?;
    let sizes: Vec<pf_core::SizeId> = p.sizes[0].sizes.iter().map(|s| s.id).collect();
    let pt = PointMm::new;
    let mut edges = Vec::new();
    let corners = [
        pt(0.0, 0.0),
        pt(400.0, 0.0),
        pt(400.0, 600.0),
        pt(0.0, 600.0),
    ];
    for i in 0..4 {
        edges.push(
            p.apply(Command::AddSegment {
                sheet,
                from: PointRef::New(corners[i]),
                to: PointRef::New(corners[(i + 1) % 4]),
            })?
            .entities[0],
        );
    }
    p.apply(Command::ExtractPiece {
        sheet,
        source: PieceSource::Paths {
            entities: edges.clone(),
        },
        name: "Front".into(),
        detached: false,
    })?;
    let at = |p: &Project, c: PointMm| {
        let s = p.sheet(sheet).ok()?;
        s.points
            .iter()
            .find(|q| {
                s.position(q.id)
                    .is_ok_and(|v| (v.x - c.x).abs() < 1e-9 && (v.y - c.y).abs() < 1e-9)
            })
            .map(|q| q.id)
    };
    let (b, c, dd) = (at(&p, corners[1]), at(&p, corners[2]), at(&p, corners[3]));
    let deltas = [
        (0, -9.0, -20.0),
        (1, -5.0, -10.0),
        (3, 5.0, 10.0),
        (4, 10.0, 22.0),
    ];
    for (i, dx, dy) in deltas {
        for (point, x, y) in [(b, dx, 0.0), (c, dx, dy), (dd, 0.0, dy)] {
            if let Some(point) = point {
                p.apply(Command::SetPointGrading {
                    sheet,
                    point,
                    size: sizes[i],
                    dx_mm: x,
                    dy_mm: y,
                })?;
            }
        }
    }
    let dim = p
        .apply(Command::AddFullDimension {
            sheet,
            kind: FullDimensionKind::PathLength {
                entity: edges[0],
                offset_mm: 10.0,
            },
            label: None,
            text_shift_mm: 0.0,
        })?
        .full_dimensions[0];
    let dim_h = p
        .apply(Command::AddFullDimension {
            sheet,
            kind: FullDimensionKind::PathLength {
                entity: edges[1],
                offset_mm: 10.0,
            },
            label: None,
            text_shift_mm: 0.0,
        })?
        .full_dimensions[0];
    let rows: [(&str, [f64; 5]); 2] = [
        ("Bottom width", [390.0, 395.0, 402.0, 405.0, 415.0]),
        ("Side height", [580.0, 590.0, 600.0, 610.0, 620.0]),
    ];
    let mut ids = Vec::new();
    for (name, values) in rows {
        let m = p
            .apply(Command::AddMeasurement {
                name: name.into(),
                unit: MeasurementUnit::Mm,
            })?
            .measurements[0];
        ids.push(m);
        for (i, (s, v)) in sizes.iter().zip(values).enumerate() {
            // Taille XS de « Side height » laissée vide : cellule « n/c » (jamais devinée).
            if name == "Side height" && i == 0 {
                continue;
            }
            p.apply(Command::SetMeasurementValue {
                measurement: m,
                size: *s,
                value: v,
            })?;
        }
    }
    p.apply(Command::AddAlteration {
        name: "Width -2".into(),
        target: ids[0],
        delta: -2.0,
    })?;
    for (name, m, d, tm, tp) in [
        ("Finished width", ids[0], dim, 2.0, 3.0),
        ("Finished height", ids[1], dim_h, 1.0, 1.0),
    ] {
        p.apply(Command::AddFinishedMeasure {
            name: name.into(),
            measurement: m,
            source: FinishedSource::FullDimension {
                sheet,
                dimension: d,
            },
            tolerance_minus: tm,
            tolerance_plus: tp,
        })?;
    }
    Ok(p)
}

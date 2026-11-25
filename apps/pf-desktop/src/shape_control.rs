//! US-09.9 — fenêtre « Contrôle de forme » du dock (menu Affichage, sans raccourci) : la courbure
//! de chaque courbe (Bézier ou lissée) de la feuille active, taille par taille, relue par
//! `Project::shape_control` (calcul dérivé, jamais stocké, aucune `Command`). Lecture seule.
//!
//! Toujours visible : la liste des courbes avec leur verdict (icône dessinée **et** texte). Deux
//! sections repliées au départ : les signalements (un texte par signalement, précédé d'une icône
//! dessinée propre à son type) et le tableau courbe × taille (rebroussements, inflexions,
//! courbure maximale, rayon minimal). Un clic sur une courbe ou un signalement sélectionne le
//! tracé dans le plan. Aucune information n'est portée par la couleur seule.
//!
//! Aucun calcul métier ici : tout vient du cœur. Le contrôle est recalculé seulement quand le
//! projet, la feuille ou la gamme change.

use crate::app::App;
use crate::diagnostic::{object_bbox, severity_icon};
use crate::dock::ToolWin;
use crate::grading::section_header;
use crate::sizes::{cell, marker_icon};
use crate::text::entity_name;
use eframe::egui::{self, RichText};
use pf_core::input::format_number;
use pf_core::{
    CurveShape, DegenerateReason, DiagnosticObject, DiagnosticSeverity, EntityId, Project,
    ShapeControl, ShapeControlError, ShapeReading, ShapeSignal, ShapeSignalKind, SheetId,
    SizeRangeId, CURVATURE_JUMP_RATIO,
};
use pf_i18n::{tr, tr_args};
use pf_ui::components::{button, ButtonKind};

const W_SIZE: f32 = 56.0;
const W_COUNT: f32 = 48.0;
const W_VALUE: f32 = 92.0;
const W_ICON: f32 = 24.0;

/// Dernier contrôle calculé et ce dont il vient.
#[derive(Debug, Clone)]
struct Cache {
    project: Project,
    sheet: SheetId,
    range: SizeRangeId,
    control: Result<ShapeControl, ShapeControlError>,
}

/// État de la fenêtre (interface seulement, jamais enregistré).
#[derive(Debug, Clone, Default)]
pub struct ShapeControlDialog {
    /// Gamme affichée ; `None` = la première du projet.
    pub range: Option<SizeRangeId>,
    /// Sections dépliées (repliées au départ).
    pub signals_open: bool,
    pub table_open: bool,
    cache: Option<Cache>,
}

/// Nombre signé à une décimale (« +71,4 », « −100,0 »).
fn signed_percent(p: f64) -> String {
    if p > 0.0 {
        format!("+{}", format_number(p, 1))
    } else {
        format_number(p, 1)
    }
}

/// Courbure en 1/mm : trois chiffres significatifs au plus, jamais d'écriture exponentielle.
fn curvature_text(k: f64) -> String {
    if k == 0.0 {
        "0".to_string()
    } else if k >= 1.0 {
        format_number(k, 2)
    } else if k >= 0.1 {
        format_number(k, 3)
    } else {
        format_number(k, 4)
    }
}

fn radius_text(r: f64) -> String {
    if r >= 100.0 {
        format_number(r, 0)
    } else if r >= 10.0 {
        format_number(r, 1)
    } else if r >= 1.0 {
        format_number(r, 2)
    } else {
        format_number(r, 3)
    }
}

/// Texte d'un signalement ; `labels` = libellés des tailles (ordre de la gamme), `base` = index de
/// la taille de base.
fn signal_text(
    s: &ShapeSignal,
    sizes: &[pf_core::SizeId],
    labels: &[String],
    base: usize,
) -> String {
    let label_of = |id: pf_core::SizeId| {
        sizes
            .iter()
            .position(|x| *x == id)
            .and_then(|i| labels.get(i))
            .map_or("?", String::as_str)
    };
    let size = label_of(s.size);
    let base_label = labels.get(base).map_or("?", String::as_str);
    match s.kind {
        ShapeSignalKind::CuspIntroduced { cusps } => tr_args(
            "shape.sig.cusp",
            &[
                ("size", size),
                ("count", &cusps.to_string()),
                ("base", base_label),
            ],
        ),
        ShapeSignalKind::InflectionChanged { base: b, found } => tr_args(
            "shape.sig.inflection",
            &[
                ("size", size),
                ("found", &found.to_string()),
                ("was", &b.to_string()),
                ("base", base_label),
            ],
        ),
        ShapeSignalKind::CurvatureJump { from, percent } => match percent {
            Some(p) => tr_args(
                "shape.sig.jump",
                &[
                    ("size", size),
                    ("percent", &signed_percent(p)),
                    ("from", label_of(from)),
                    ("limit", &format_number(CURVATURE_JUMP_RATIO * 100.0, 0)),
                ],
            ),
            None => tr_args(
                "shape.sig.jump_flat",
                &[("size", size), ("from", label_of(from))],
            ),
        },
        ShapeSignalKind::Degenerate { reason } => tr_args(
            match reason {
                DegenerateReason::ZeroLength => "shape.sig.zero",
                DegenerateReason::Unreadable => "shape.sig.unreadable",
            },
            &[("size", size)],
        ),
    }
}

/// Icône dessinée d'un signalement : pointe (rebroussement), S (inflexion), marche (saut de
/// courbure), cercle barré (courbe illisible). Le type est aussi écrit dans le texte.
fn signal_icon(ui: &mut egui::Ui, kind: &ShapeSignalKind) {
    let tokens = pf_ui::theme::tokens(ui.ctx());
    let stroke = egui::Stroke::new(1.6_f32, tokens.warning);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 18.0), egui::Sense::hover());
    let c = rect.center();
    let p = ui.painter();
    let at = |x: f32, y: f32| c + egui::vec2(x, y);
    match kind {
        ShapeSignalKind::CuspIntroduced { .. } => {
            p.add(egui::Shape::line(
                vec![
                    at(-8.0, 6.0),
                    at(-1.0, -6.0),
                    at(0.0, 2.0),
                    at(1.0, -6.0),
                    at(8.0, 6.0),
                ],
                stroke,
            ));
        }
        ShapeSignalKind::InflectionChanged { .. } => {
            let pts: Vec<egui::Pos2> = (0..=16)
                .map(|i| {
                    let t = i as f32 / 16.0;
                    at(-8.0 + 16.0 * t, -5.0 * (t * std::f32::consts::TAU).sin())
                })
                .collect();
            p.add(egui::Shape::line(pts, stroke));
        }
        ShapeSignalKind::CurvatureJump { .. } => {
            p.add(egui::Shape::line(
                vec![at(-8.0, 5.0), at(-1.0, 5.0), at(-1.0, -5.0), at(8.0, -5.0)],
                stroke,
            ));
        }
        ShapeSignalKind::Degenerate { .. } => {
            p.circle_stroke(c, 7.0, stroke);
            p.line_segment([at(-5.0, 5.0), at(5.0, -5.0)], stroke);
        }
    }
}

/// Coche dessinée : courbe sans signalement.
fn ok_icon(ui: &mut egui::Ui) {
    let tokens = pf_ui::theme::tokens(ui.ctx());
    let stroke = egui::Stroke::new(1.6_f32, tokens.success);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 18.0), egui::Sense::hover());
    let c = rect.center();
    ui.painter().add(egui::Shape::line(
        vec![
            c + egui::vec2(-6.0, 0.0),
            c + egui::vec2(-2.0, 4.0),
            c + egui::vec2(6.0, -5.0),
        ],
        stroke,
    ));
}

impl App {
    /// Menu Affichage › « Contrôle de forme… » (sans raccourci).
    pub fn open_shape_control(&mut self) {
        if self.shape_control.is_none() {
            self.shape_control = Some(ShapeControlDialog::default());
        }
    }

    /// Clic sur une courbe : la sélectionne dans le plan et cadre la vue.
    fn select_curve(&mut self, entity: EntityId) {
        if self.plan.is_some() {
            self.leave_plan();
        }
        self.editor.selection.clear();
        self.editor.selection.entities.push(entity);
        let sheet = self.editor.sheet;
        let bbox = self
            .editor
            .project()
            .sheet(sheet)
            .ok()
            .and_then(|s| object_bbox(s, &DiagnosticObject::Entity(entity)));
        if let Some(bbox) = bbox {
            self.focus_on(bbox);
        }
    }

    pub fn shape_control_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.shape_control.take() else {
            return;
        };
        let secondary = pf_ui::theme::tokens(ctx).text_secondary;
        let sheet = self.editor.sheet;
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
        d.range = range;
        // Contrôle recalculé seulement si le projet, la feuille ou la gamme a changé.
        if let Some(range) = range {
            let stale = d.cache.as_ref().is_none_or(|c| {
                c.range != range || c.sheet != sheet || &c.project != self.editor.project()
            });
            if stale {
                d.cache = Some(Cache {
                    project: self.editor.project().clone(),
                    sheet,
                    range,
                    control: self.editor.project().shape_control(sheet, range),
                });
            }
        } else {
            d.cache = None;
        }
        let mut open = true;
        let mut close = false;
        let mut clicked: Option<EntityId> = None;
        let mut chosen: Option<SizeRangeId> = None;
        let slot = crate::dock::slot(ctx, &self.dock, ToolWin::ShapeControl);
        let width = self.dock.content_width(ToolWin::ShapeControl);
        let project = self.editor.project();
        let cache = d.cache.take();
        let (mut signals_open, mut table_open) = (d.signals_open, d.table_open);
        egui::Window::new(tr("shape.title"))
            .id(ToolWin::ShapeControl.id())
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
                        crate::a11y::label(ui, RichText::new(tr("shape.intro")).color(secondary));
                        crate::a11y::label(ui, RichText::new(tr("shape.readonly")).strong());
                        ui.add_space(4.0);
                        let control = cache.as_ref().and_then(|c| c.control.as_ref().ok());
                        match (range, control) {
                            (Some(range), Some(control)) => {
                                if ranges.len() > 1 {
                                    ui.horizontal_wrapped(|ui| {
                                        crate::a11y::label(ui, tr("shape.range"));
                                        for (id, name) in &ranges {
                                            if ui.selectable_label(*id == range, name).clicked() {
                                                chosen = Some(*id);
                                            }
                                        }
                                    });
                                }
                                draw_control(
                                    ui,
                                    project,
                                    sheet,
                                    control,
                                    secondary,
                                    (&mut signals_open, &mut table_open),
                                    &mut clicked,
                                );
                            }
                            _ => {
                                crate::a11y::label(
                                    ui,
                                    RichText::new(tr("shape.no_sizes")).color(secondary),
                                );
                            }
                        }
                        ui.add_space(6.0);
                        crate::a11y::label(ui, RichText::new(tr("shape.note")).color(secondary));
                        ui.add_space(6.0);
                        close = button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                    });
            });
        d.cache = cache;
        d.signals_open = signals_open;
        d.table_open = table_open;
        if let Some(id) = chosen {
            d.range = Some(id);
        }
        if open && !close {
            self.shape_control = Some(d);
        }
        if let Some(entity) = clicked {
            self.select_curve(entity);
        }
    }
}

/// Libellés des tailles de la gamme, dans l'ordre, et rang de la base.
fn size_labels(project: &Project, control: &ShapeControl) -> (Vec<String>, usize) {
    let labels: Vec<String> = project
        .size_range(control.range)
        .map(|r| r.sizes.iter().map(|s| s.label.clone()).collect())
        .unwrap_or_default();
    let base = control
        .sizes
        .iter()
        .position(|s| *s == control.base)
        .unwrap_or(0);
    (labels, base)
}

fn curve_name(project: &Project, sheet: SheetId, entity: EntityId) -> String {
    project
        .sheet(sheet)
        .map(|s| entity_name(s, entity))
        .unwrap_or_else(|_| tr("entity.missing"))
}

fn draw_control(
    ui: &mut egui::Ui,
    project: &Project,
    sheet: SheetId,
    control: &ShapeControl,
    secondary: egui::Color32,
    sections: (&mut bool, &mut bool),
    clicked: &mut Option<EntityId>,
) {
    let (signals_open, table_open) = sections;
    let (labels, base) = size_labels(project, control);
    if control.curves.is_empty() {
        crate::a11y::label(ui, RichText::new(tr("shape.no_curves")).color(secondary));
        return;
    }
    let flagged = control.flagged().count();
    crate::a11y::label(
        ui,
        tr_args(
            "shape.summary",
            &[
                ("curves", &control.curves.len().to_string()),
                ("flagged", &flagged.to_string()),
            ],
        ),
    );
    crate::a11y::label(ui, RichText::new(tr("shape.units")).color(secondary));
    ui.add_space(2.0);
    // Liste des courbes et verdict : icône dessinée + texte.
    for cv in &control.curves {
        ui.horizontal(|ui| {
            if cv.is_clean() {
                ok_icon(ui);
            } else {
                severity_icon(ui, DiagnosticSeverity::Warning);
            }
            let name = curve_name(project, sheet, cv.entity);
            let r = ui
                .add(egui::Button::new(name.as_str()).frame(false))
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(&name);
            if r.clicked() {
                *clicked = Some(cv.entity);
            }
            crate::a11y::label(ui, RichText::new(verdict_text(cv)).color(secondary));
        });
    }
    // Signalements.
    let total: usize = control.curves.iter().map(|c| c.signals.len()).sum();
    ui.add_space(4.0);
    section_header(
        ui,
        signals_open,
        tr_args("shape.section.signals", &[("count", &total.to_string())]),
    );
    if *signals_open {
        if total == 0 {
            crate::a11y::label(ui, RichText::new(tr("shape.none")).color(secondary));
        }
        for cv in control.curves.iter().filter(|c| !c.is_clean()) {
            let name = curve_name(project, sheet, cv.entity);
            crate::a11y::label(ui, RichText::new(name.as_str()).strong());
            for s in &cv.signals {
                ui.horizontal(|ui| {
                    signal_icon(ui, &s.kind);
                    let text = signal_text(s, &control.sizes, &labels, base);
                    let r = ui
                        .add(egui::Button::new(text.as_str()).frame(false).wrap())
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if r.clicked() {
                        *clicked = Some(cv.entity);
                    }
                });
            }
        }
    }
    // Tableau courbe × taille.
    ui.add_space(4.0);
    section_header(
        ui,
        table_open,
        tr_args(
            "shape.section.table",
            &[("count", &control.curves.len().to_string())],
        ),
    );
    if *table_open {
        ui.horizontal(|ui| {
            marker_icon(ui, false, true);
            crate::a11y::label(ui, RichText::new(tr("sizes.base")).color(secondary));
        });
        for cv in &control.curves {
            draw_curve_table(ui, project, sheet, control, cv, &labels, secondary, clicked);
        }
    }
}

fn verdict_text(cv: &CurveShape) -> String {
    if cv.is_clean() {
        tr("shape.verdict.ok")
    } else {
        tr_args(
            "shape.verdict.flagged",
            &[("count", &cv.signals.len().to_string())],
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_curve_table(
    ui: &mut egui::Ui,
    project: &Project,
    sheet: SheetId,
    control: &ShapeControl,
    cv: &CurveShape,
    labels: &[String],
    secondary: egui::Color32,
    clicked: &mut Option<EntityId>,
) {
    ui.add_space(4.0);
    let name = curve_name(project, sheet, cv.entity);
    if ui
        .add(egui::Button::new(RichText::new(name).strong()).frame(false))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
    {
        *clicked = Some(cv.entity);
    }
    let right = egui::Layout::right_to_left(egui::Align::Center);
    ui.spacing_mut().item_spacing.x = 2.0;
    ui.horizontal(|ui| {
        cell(ui, W_SIZE, |ui| {
            crate::a11y::label(ui, RichText::new(tr("shape.col.size")).strong());
        });
        for (w, key) in [
            (W_COUNT, "shape.col.cusps"),
            (W_COUNT, "shape.col.inflections"),
            (W_VALUE, "shape.col.curvature"),
            (W_VALUE, "shape.col.radius"),
        ] {
            cell(ui, w, |ui| {
                ui.with_layout(right, |ui| {
                    crate::a11y::label(ui, RichText::new(tr(key)).strong());
                });
            });
        }
    });
    for (i, sz) in cv.sizes.iter().enumerate() {
        let flagged: Vec<&ShapeSignal> = cv.signals.iter().filter(|s| s.size == sz.size).collect();
        ui.horizontal(|ui| {
            cell(ui, W_SIZE, |ui| {
                let label = labels.get(i).map_or("?", String::as_str);
                if sz.size == control.base {
                    marker_icon(ui, false, true);
                } else {
                    // Même retrait que la ligne de la base : les libellés restent alignés.
                    ui.allocate_space(egui::vec2(16.0, 18.0));
                }
                let r = crate::a11y::label(ui, label);
                if sz.size == control.base {
                    r.on_hover_text(tr_args("size.base_mark", &[("label", label)]));
                }
            });
            match &sz.reading {
                ShapeReading::Metrics(m) => {
                    let cusp = flagged
                        .iter()
                        .any(|s| matches!(s.kind, ShapeSignalKind::CuspIntroduced { .. }));
                    let infl = flagged
                        .iter()
                        .any(|s| matches!(s.kind, ShapeSignalKind::InflectionChanged { .. }));
                    let jump = flagged
                        .iter()
                        .any(|s| matches!(s.kind, ShapeSignalKind::CurvatureJump { .. }));
                    for (w, text, strong) in [
                        (W_COUNT, m.cusps.to_string(), cusp),
                        (W_COUNT, m.inflections.to_string(), infl),
                        (W_VALUE, curvature_text(m.max_curvature_per_mm), jump),
                        (
                            W_VALUE,
                            m.min_radius_mm
                                .map_or_else(|| tr("shape.straight"), radius_text),
                            jump,
                        ),
                    ] {
                        cell(ui, w, |ui| {
                            ui.with_layout(right, |ui| {
                                let t = RichText::new(text);
                                // Une valeur signalée est en gras ET la ligne porte un texte.
                                crate::a11y::label(ui, if strong { t.strong() } else { t });
                            });
                        });
                    }
                    cell(ui, W_ICON, |ui| {
                        if !flagged.is_empty() {
                            severity_icon(ui, DiagnosticSeverity::Warning);
                        }
                    });
                }
                ShapeReading::Degenerate(_) => {
                    cell(ui, 2.0 * W_COUNT + 2.0 * W_VALUE, |ui| {
                        severity_icon(ui, DiagnosticSeverity::Warning);
                        crate::a11y::label(
                            ui,
                            RichText::new(tr("shape.cell.unreadable")).color(secondary),
                        );
                    });
                }
            }
        });
    }
}

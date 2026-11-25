//! US-09.2a — fenêtre « Gradation » du dock (menu Affichage, sans raccourci) et sélecteur de
//! **taille active** de la barre d'état.
//!
//! La fenêtre édite, pour le point **libre** sélectionné, ses écarts cumulés (dx, dy) par taille,
//! dans le repère de la feuille (X vers la droite, Y vers le haut), par rapport à la taille de base.
//! Aucune section n'est dépliée au départ (une section par gamme). Chaque saisie est d'abord
//! vérifiée par `Project::check_grading_command` (refus **typé**, localisé ici) puis envoyée par
//! `Command::SetPointGrading` : rien n'est modifié tant qu'une saisie est refusée. La base est
//! marquée par une icône dessinée **et** un texte (écart nul, non modifiable).
//!
//! US-09.3a : deux sections **repliées** sous le tableau. « Reporter la règle » : le point affiché
//! est la source, les cibles sont le reste de la sélection (le point affiché reste celui du tableau
//! tant qu'il fait partie de la sélection) ; « Interpoler » : deux points gradués A et B choisis
//! dans des listes, les intermédiaires sont la sélection (hors A et B). Même chemin que le tableau :
//! vérification typée (`check_grading_command`) puis `Command::CopyPointGrading` /
//! `InterpolatePointGrading` (une étape annulable). Désactivées hors taille de base.
//!
//! La taille active est un état d'interface (jamais enregistré). Quand ce n'est pas la base, le
//! plan affiche la géométrie évaluée de cette taille en **lecture seule** : l'édition de géométrie
//! est refusée par `Editor::exec` (« Revenir à la taille de base pour modifier ») ; seules les
//! commandes de tailles et de gradation restent permises. Exports et mesures restent en taille de
//! base. Aucun calcul métier ici : tout vient du cœur.

use crate::a11y::ComboExt;
use crate::app::App;
use crate::dock::ToolWin;
use crate::sizes::{cell, fold_icon, marker_icon};
use eframe::egui::{self, RichText};
use pf_core::units::parse_length_display;
use pf_core::{
    Command, GradingCopyMode, GradingError, GradingFollower, PointDefinition, PointId, SizeId,
    SizeRangeId,
};
use pf_i18n::{tr, tr_args};
use pf_ui::components::{button, ButtonKind};
use std::collections::{HashMap, HashSet};

const W_LABEL: f32 = 70.0;
const W_FIELD: f32 = 74.0;

/// État de la fenêtre (interface seulement, jamais enregistré).
#[derive(Debug, Clone, Default)]
pub struct GradingDialog {
    /// Gammes dépliées (aucune au départ).
    pub open: HashSet<SizeRangeId>,
    /// Tampons des champs, par (taille, axe) ; resynchronisés sur le projet tant que le champ n'a
    /// pas le focus.
    buffers: HashMap<(u64, u8), String>,
    /// Dernier refus, localisé.
    error: Option<String>,
    /// Dernière confirmation (report ou interpolation faits), localisée.
    notice: Option<String>,
    /// Point affiché : le point sélectionné, conservé quand la sélection s'élargit (source du
    /// report).
    pub shown: Option<PointId>,
    /// Section « Reporter la règle » dépliée (repliée au départ).
    pub report_open: bool,
    /// Section « Interpoler » dépliée (repliée au départ).
    pub interp_open: bool,
    /// Mode du report.
    pub mode: GradingCopyMode,
    /// Extrémités A et B de l'interpolation.
    pub from: Option<PointId>,
    pub to: Option<PointId>,
}

impl GradingDialog {
    /// Fenêtre avec ces gammes dépliées (étape de capture) ; les autres restent repliées.
    pub fn with_open(open: impl IntoIterator<Item = SizeRangeId>) -> GradingDialog {
        GradingDialog {
            open: open.into_iter().collect(),
            ..GradingDialog::default()
        }
    }
}

enum Action {
    Set {
        size: SizeId,
        dx: f64,
        dy: f64,
    },
    Clear,
    Copy {
        mode: GradingCopyMode,
        targets: Vec<PointId>,
    },
    Interpolate {
        from: PointId,
        to: PointId,
        points: Vec<PointId>,
    },
}

/// Message d'un refus de gradation dans la langue de l'interface (le texte du cœur est en français).
pub(crate) fn error_text(e: &GradingError) -> String {
    match e {
        GradingError::UnknownSheet(_)
        | GradingError::UnknownPoint(_)
        | GradingError::UnknownSize(_) => tr("grading.err.unknown"),
        GradingError::BaseSize { label } => tr_args("grading.err.base", &[("label", label)]),
        GradingError::NotFinite => tr("grading.err.not_finite"),
        GradingError::OutOfBounds => tr("grading.err.bounds"),
        GradingError::NotFree { label } => tr_args("grading.err.not_free", &[("name", label)]),
        GradingError::Locked { label } => tr_args("grading.err.locked", &[("name", label)]),
        GradingError::Followed { label, by } => tr_args(
            match by {
                GradingFollower::Kept => "grading.err.followed_kept",
                GradingFollower::Length => "grading.err.followed_length",
            },
            &[("name", label)],
        ),
        GradingError::OutOfWorkspace { label } => {
            tr_args("grading.err.workspace", &[("name", label)])
        }
        GradingError::NoTargets => tr("grading.err.no_targets"),
        GradingError::DuplicateTarget { label } => {
            tr_args("grading.err.duplicate", &[("name", label)])
        }
        GradingError::TargetIsSource { label } => {
            tr_args("grading.err.is_source", &[("name", label)])
        }
        GradingError::DegenerateSegment { from, to } => {
            tr_args("grading.err.degenerate", &[("from", from), ("to", to)])
        }
        GradingError::NotAGradingCommand => {
            tr_args("grading.err.other", &[("detail", &e.to_string())])
        }
    }
}

/// Champ de texte à tampon resynchronisé sur le projet tant qu'il n'a pas le focus ; renvoie le
/// texte validé (focus perdu ou Entrée) quand il diffère du projet.
fn field(
    ui: &mut egui::Ui,
    buffers: &mut HashMap<(u64, u8), String>,
    key: (u64, u8),
    current: &str,
) -> Option<String> {
    let id = egui::Id::new(("grading_field", key.0, key.1));
    let focused = ui.ctx().memory(|m| m.has_focus(id));
    let buf = buffers.entry(key).or_default();
    if !focused {
        *buf = current.to_string();
    }
    let response = crate::a11y::add_text(
        ui,
        egui::TextEdit::singleline(buf)
            .id(id)
            .desired_width(W_FIELD - 8.0),
    );
    (response.lost_focus() && buf.as_str() != current).then(|| buf.clone())
}

impl App {
    /// Ouvre la fenêtre « Gradation » (menu Affichage, sans raccourci).
    pub fn open_grading(&mut self) {
        if self.grading.is_none() {
            self.grading = Some(GradingDialog::default());
        }
    }

    /// Sélecteur de taille active de la barre d'état : un menu qui liste les tailles de chaque
    /// gamme. Rien n'est affiché quand le projet n'a aucune gamme.
    pub fn size_selector(&mut self, ui: &mut egui::Ui) {
        let ranges = self.editor.project().sizes.clone();
        if ranges.is_empty() {
            return;
        }
        ui.separator();
        let active = self.editor.active_size;
        let name = active
            .and_then(|id| self.editor.project().find_size(id))
            .map(|(r, i)| r.sizes[i].label.clone());
        let text = match &name {
            Some(n) => tr_args("size.active", &[("name", n)]),
            None => tr("size.active.base"),
        };
        let mut chosen: Option<Option<SizeId>> = None;
        ui.menu_button(text, |ui| {
            if ui
                .selectable_label(active.is_none(), tr("size.active.base_item"))
                .clicked()
            {
                chosen = Some(None);
                ui.close();
            }
            for range in &ranges {
                ui.separator();
                crate::a11y::label(ui, RichText::new(&range.name).weak());
                for size in &range.sizes {
                    let text = if size.id == range.base {
                        tr_args("size.base_mark", &[("label", &size.label)])
                    } else {
                        size.label.clone()
                    };
                    if ui.selectable_label(active == Some(size.id), text).clicked() {
                        chosen = Some(Some(size.id));
                        ui.close();
                    }
                }
            }
        })
        .response
        .on_hover_text(tr("size.active.hover"));
        if let Some(size) = chosen {
            self.editor.set_active_size(size);
        }
    }

    /// Vérification typée d'abord (refus localisé, rien n'est modifié), commande ensuite.
    fn grading_act(
        &mut self,
        d: &mut GradingDialog,
        sheet: pf_core::SheetId,
        point: PointId,
        action: Action,
    ) {
        let name = |p: PointId| {
            self.editor
                .project()
                .sheet(sheet)
                .map(|s| s.label(p).to_string())
                .unwrap_or_default()
        };
        let (cmd, done) = match action {
            Action::Set { size, dx, dy } => (
                Command::SetPointGrading {
                    sheet,
                    point,
                    size,
                    dx_mm: dx,
                    dy_mm: dy,
                },
                None,
            ),
            Action::Clear => (Command::ClearPointGrading { sheet, point }, None),
            Action::Copy { mode, targets } => {
                let done = tr_args(
                    "grading.report.done",
                    &[
                        ("name", &name(point)),
                        ("count", &targets.len().to_string()),
                    ],
                );
                (
                    Command::CopyPointGrading {
                        sheet,
                        source: point,
                        targets,
                        mode,
                    },
                    Some(done),
                )
            }
            Action::Interpolate { from, to, points } => {
                let done = tr_args(
                    "grading.interp.done",
                    &[
                        ("count", &points.len().to_string()),
                        ("from", &name(from)),
                        ("to", &name(to)),
                    ],
                );
                (
                    Command::InterpolatePointGrading {
                        sheet,
                        from,
                        to,
                        points,
                    },
                    Some(done),
                )
            }
        };
        d.notice = None;
        if let Err(e) = self.editor.project().check_grading_command(&cmd) {
            d.error = Some(error_text(&e));
            return;
        }
        match self.editor.exec(cmd) {
            Some(_) => {
                d.error = None;
                d.notice = done;
            }
            None => d.error = Some(self.editor.status.text.clone()),
        }
    }

    pub fn grading_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.grading.take() else {
            return;
        };
        let project = self.editor.project().clone();
        d.open
            .retain(|id| project.sizes.iter().any(|r| r.id == *id));
        let sheet_id = self.editor.sheet;
        let selected: Vec<PointId> = self.editor.selection.points.clone();
        let tokens = pf_ui::theme::tokens(ctx);
        let (danger, secondary) = (tokens.danger, tokens.text_secondary);
        let mut open = true;
        let mut close = false;
        let mut actions: Vec<Action> = Vec::new();
        let slot = crate::dock::slot(ctx, &self.dock, ToolWin::Grading);
        let width = self.dock.content_width(ToolWin::Grading);
        // Le point affiché reste celui du tableau quand la sélection s'élargit (il est la source
        // du report) ; sinon c'est le premier point sélectionné.
        let target = match selected.as_slice() {
            [] => None,
            [p] => Some(*p),
            many => d.shown.filter(|s| many.contains(s)).or(Some(many[0])),
        };
        d.shown = target;
        let read_only = self.editor.size_view().is_some();
        egui::Window::new(tr("grading.title"))
            .id(ToolWin::Grading.id())
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
                        crate::a11y::label(ui, RichText::new(tr("grading.intro")).color(secondary));
                        ui.add_space(4.0);
                        let sheet = project.sheet(sheet_id).ok();
                        if let Some(sheet) = sheet {
                            crate::a11y::label(
                                ui,
                                tr_args(
                                    "grading.count",
                                    &[("count", &sheet.graded_point_count().to_string())],
                                ),
                            );
                        }
                        ui.add_space(4.0);
                        match (sheet, target) {
                            _ if project.sizes.is_empty() => {
                                crate::a11y::label(
                                    ui,
                                    RichText::new(tr("grading.no_sizes")).color(secondary),
                                );
                            }
                            (Some(sheet), Some(point)) => {
                                draw_point(
                                    ui,
                                    &mut d,
                                    &project,
                                    sheet,
                                    point,
                                    &selected,
                                    read_only,
                                    danger,
                                    secondary,
                                    &mut actions,
                                );
                            }
                            _ => {
                                crate::a11y::label(
                                    ui,
                                    RichText::new(tr("grading.none_selected")).color(secondary),
                                );
                            }
                        }
                        ui.add_space(6.0);
                        crate::a11y::label(ui, RichText::new(tr("grading.note")).color(secondary));
                        ui.add_space(6.0);
                        close = button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                    });
            });
        if let Some(point) = target {
            for a in actions {
                self.grading_act(&mut d, sheet_id, point, a);
            }
        }
        if open && !close {
            self.grading = Some(d);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_point(
    ui: &mut egui::Ui,
    d: &mut GradingDialog,
    project: &pf_core::Project,
    sheet: &pf_core::Sheet,
    point: PointId,
    selected: &[PointId],
    read_only: bool,
    danger: egui::Color32,
    secondary: egui::Color32,
    actions: &mut Vec<Action>,
) {
    let label = sheet.label(point).to_string();
    let Ok(p) = sheet.point(point) else {
        crate::a11y::label(
            ui,
            RichText::new(tr("grading.none_selected")).color(secondary),
        );
        return;
    };
    let PointDefinition::Free { x, y } = p.definition else {
        crate::a11y::label(
            ui,
            RichText::new(tr_args("grading.constructed", &[("name", &label)])).color(secondary),
        );
        return;
    };
    crate::a11y::label(
        ui,
        RichText::new(pf_i18n::tr_len(
            "grading.point",
            &[
                ("name", &label),
                ("x", &pf_core::units::len_number(x, 3)),
                ("y", &pf_core::units::len_number(y, 3)),
            ],
        ))
        .strong(),
    );
    if sheet.gradings_of(point).next().is_some() {
        if let Some(by) = sheet.grading_follower(point) {
            crate::a11y::label(
                ui,
                RichText::new(tr(match by {
                    GradingFollower::Kept => "grading.conflict_kept",
                    GradingFollower::Length => "grading.conflict_length",
                }))
                .color(danger),
            );
        }
    }
    for range in &project.sizes {
        let is_open = d.open.contains(&range.id);
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            fold_icon(ui, is_open);
            let header = tr_args(
                "grading.range.header",
                &[
                    ("name", &range.name),
                    ("count", &range.sizes.len().to_string()),
                ],
            );
            let r = ui.add(egui::Button::new(RichText::new(header).strong()).frame(false));
            if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                if is_open {
                    d.open.remove(&range.id);
                } else {
                    d.open.insert(range.id);
                }
            }
        });
        if !is_open {
            continue;
        }
        ui.horizontal(|ui| {
            cell(ui, W_LABEL + 100.0, |ui| {
                crate::a11y::label(ui, RichText::new(tr("grading.col.size")).color(secondary));
            });
            cell(ui, W_FIELD, |ui| {
                crate::a11y::label(ui, RichText::new(tr("grading.col.dx")).color(secondary));
            });
            cell(ui, W_FIELD, |ui| {
                crate::a11y::label(ui, RichText::new(tr("grading.col.dy")).color(secondary));
            });
            crate::a11y::label(ui, RichText::new(tr("grading.col.pos")).color(secondary));
        });
        for size in &range.sizes {
            let is_base = size.id == range.base;
            let current = sheet
                .grading(point, size.id)
                .map_or((0.0, 0.0), |g| (g.dx_mm, g.dy_mm));
            ui.horizontal(|ui| {
                cell(ui, W_LABEL, |ui| {
                    crate::a11y::label(ui, &size.label);
                });
                cell(ui, 100.0, |ui| {
                    if is_base {
                        marker_icon(ui, false, true);
                        crate::a11y::label(ui, RichText::new(tr("sizes.base")).strong());
                    }
                });
                if is_base {
                    // Écart nul, non modifiable : du texte, pas un champ.
                    for _ in 0..2 {
                        cell(ui, W_FIELD, |ui| {
                            crate::a11y::label(ui, "0");
                        });
                    }
                    crate::a11y::label(
                        ui,
                        RichText::new(tr_args(
                            "grading.pos",
                            &[
                                ("x", &pf_core::units::len_number(x, 3)),
                                ("y", &pf_core::units::len_number(y, 3)),
                            ],
                        ))
                        .color(secondary),
                    );
                    return;
                }
                let texts = [
                    pf_core::units::field_text(current.0),
                    pf_core::units::field_text(current.1),
                ];
                for axis in 0..2u8 {
                    let committed = cell_field(ui, d, size.id, axis, &texts[usize::from(axis)]);
                    if let Some(text) = committed {
                        match parse_length_display(&text) {
                            Ok(v) => {
                                let (dx, dy) = if axis == 0 {
                                    (v, current.1)
                                } else {
                                    (current.0, v)
                                };
                                d.error = None;
                                actions.push(Action::Set {
                                    size: size.id,
                                    dx,
                                    dy,
                                });
                            }
                            Err(_) => {
                                d.error = Some(tr_args("grading.err.number", &[("text", &text)]));
                            }
                        }
                    }
                }
                crate::a11y::label(
                    ui,
                    RichText::new(tr_args(
                        "grading.pos",
                        &[
                            ("x", &pf_core::units::len_number(x + current.0, 3)),
                            ("y", &pf_core::units::len_number(y + current.1, 3)),
                        ],
                    ))
                    .color(secondary),
                );
            });
        }
    }
    ui.add_space(6.0);
    ui.add_enabled_ui(sheet.gradings_of(point).next().is_some(), |ui| {
        if button(ui, &tr("grading.clear"), ButtonKind::Danger).clicked() {
            actions.push(Action::Clear);
        }
    });
    ui.add_space(6.0);
    ui.separator();
    report_section(
        ui, d, point, &label, selected, read_only, secondary, actions,
    );
    ui.add_space(6.0);
    interp_section(ui, d, sheet, selected, read_only, secondary, actions);
    if let Some(text) = &d.error {
        ui.add_space(4.0);
        crate::a11y::label(ui, RichText::new(text).color(danger));
    } else if let Some(text) = &d.notice {
        ui.add_space(4.0);
        crate::a11y::label(ui, RichText::new(text).color(secondary));
    }
}

/// En-tête repliable d'une section (flèche dessinée + titre).
pub(crate) fn section_header(ui: &mut egui::Ui, open: &mut bool, title: String) {
    ui.horizontal(|ui| {
        fold_icon(ui, *open);
        let r = ui.add(egui::Button::new(RichText::new(title).strong()).frame(false));
        if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            *open = !*open;
        }
    });
}

fn mode_label(mode: GradingCopyMode) -> String {
    tr(match mode {
        GradingCopyMode::Same => "grading.mode.same",
        GradingCopyMode::MirrorX => "grading.mode.mirror_x",
        GradingCopyMode::MirrorY => "grading.mode.mirror_y",
        GradingCopyMode::Invert => "grading.mode.invert",
    })
}

/// « Reporter la règle » : le point affiché est la source, les cibles sont le reste de la
/// sélection.
#[allow(clippy::too_many_arguments)]
fn report_section(
    ui: &mut egui::Ui,
    d: &mut GradingDialog,
    point: PointId,
    label: &str,
    selected: &[PointId],
    read_only: bool,
    secondary: egui::Color32,
    actions: &mut Vec<Action>,
) {
    section_header(ui, &mut d.report_open, tr("grading.report.title"));
    if !d.report_open {
        return;
    }
    crate::a11y::label(
        ui,
        RichText::new(tr_args("grading.report.intro", &[("name", label)])).color(secondary),
    );
    for mode in GradingCopyMode::ALL {
        ui.radio_value(&mut d.mode, mode, mode_label(mode));
    }
    let targets: Vec<PointId> = selected.iter().copied().filter(|p| *p != point).collect();
    if targets.is_empty() {
        crate::a11y::label(
            ui,
            RichText::new(tr_args("grading.report.no_targets", &[("name", label)]))
                .color(secondary),
        );
    } else {
        crate::a11y::label(
            ui,
            tr_args(
                "grading.report.targets",
                &[("count", &targets.len().to_string()), ("name", label)],
            ),
        );
    }
    if read_only {
        crate::a11y::label(ui, RichText::new(tr("size.readonly")).color(secondary));
    }
    ui.add_enabled_ui(!read_only && !targets.is_empty(), |ui| {
        if button(ui, &tr("grading.report.copy"), ButtonKind::Primary).clicked() {
            actions.push(Action::Copy {
                mode: d.mode,
                targets,
            });
        }
    });
}

/// « Interpoler » : deux points gradués A et B (listes) ; les intermédiaires sont la sélection.
fn interp_section(
    ui: &mut egui::Ui,
    d: &mut GradingDialog,
    sheet: &pf_core::Sheet,
    selected: &[PointId],
    read_only: bool,
    secondary: egui::Color32,
    actions: &mut Vec<Action>,
) {
    section_header(ui, &mut d.interp_open, tr("grading.interp.title"));
    if !d.interp_open {
        return;
    }
    crate::a11y::label(
        ui,
        RichText::new(tr("grading.interp.intro")).color(secondary),
    );
    let mut graded: Vec<PointId> = sheet.gradings.iter().map(|g| g.point).collect();
    graded.dedup();
    // Un point qui n'est plus gradué n'est plus proposé.
    if d.from.is_some_and(|p| !graded.contains(&p)) {
        d.from = None;
    }
    if d.to.is_some_and(|p| !graded.contains(&p)) {
        d.to = None;
    }
    if graded.is_empty() {
        crate::a11y::label(
            ui,
            RichText::new(tr("grading.interp.none_graded")).color(secondary),
        );
        return;
    }
    for (n, key) in [(0u8, "grading.interp.from"), (1u8, "grading.interp.to")] {
        let slot = if n == 0 { &mut d.from } else { &mut d.to };
        let shown = slot
            .map(|p| sheet.label(p).to_string())
            .unwrap_or_else(|| tr("grading.interp.pick"));
        ui.horizontal(|ui| {
            cell(ui, W_LABEL, |ui| {
                crate::a11y::label(ui, tr(key));
            });
            egui::ComboBox::from_id_salt(("grading-interp", n))
                .selected_text(shown)
                .show_ui_named(ui, |ui| {
                    for p in &graded {
                        ui.selectable_value(slot, Some(*p), sheet.label(*p).to_string());
                    }
                });
        });
    }
    let (from, to) = (d.from, d.to);
    let points: Vec<PointId> = selected
        .iter()
        .copied()
        .filter(|p| Some(*p) != from && Some(*p) != to)
        .collect();
    let ready = matches!((from, to), (Some(a), Some(b)) if a != b) && !points.is_empty();
    crate::a11y::label(
        ui,
        tr_args(
            "grading.interp.targets",
            &[("count", &points.len().to_string())],
        ),
    );
    if !ready {
        crate::a11y::label(
            ui,
            RichText::new(tr("grading.interp.need")).color(secondary),
        );
    }
    if read_only {
        crate::a11y::label(ui, RichText::new(tr("size.readonly")).color(secondary));
    }
    ui.add_enabled_ui(!read_only && ready, |ui| {
        if button(ui, &tr("grading.interp.run"), ButtonKind::Primary).clicked() {
            if let (Some(from), Some(to)) = (from, to) {
                actions.push(Action::Interpolate { from, to, points });
            }
        }
    });
}

/// Une cellule de saisie (largeur fixe) ; renvoie le texte validé.
fn cell_field(
    ui: &mut egui::Ui,
    d: &mut GradingDialog,
    size: SizeId,
    axis: u8,
    current: &str,
) -> Option<String> {
    let mut out = None;
    cell(ui, W_FIELD, |ui| {
        out = field(ui, &mut d.buffers, (size.0, axis), current);
    });
    out
}

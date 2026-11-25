//! US-09.1 — fenêtre « Tailles » du dock (menu Affichage, sans raccourci) : gammes de tailles du
//! modèle, tailles dans l'ordre, taille de base, tailles de rupture.
//!
//! La fenêtre ne calcule rien : chaque action est d'abord vérifiée par
//! `Project::check_size_command` (refus **typé**, localisé ici), puis envoyée au cœur par
//! `Command` (une étape d'annulation). Rien n'est modifié tant qu'une saisie est refusée ; le
//! message s'affiche sous la gamme en cause. Les gammes sont **repliées par défaut**, comme la
//! section « Nouvelle gamme ». La base et la rupture sont marquées par une icône dessinée **et**
//! un texte, jamais par la couleur seule. La « taille active » d'affichage se choisit dans la
//! barre d'état (`grading.rs`) ; les écarts par point se posent dans la fenêtre « Gradation ».

use crate::app::App;
use crate::dock::ToolWin;
use eframe::egui::{self, RichText};
use pf_core::{
    Command, SizeError, SizeId, SizeKind, SizeRange, SizeRangeId, TextProblem, MAX_SIZES_PER_RANGE,
    MAX_SIZE_RANGES,
};
use pf_i18n::{tr, tr_args};
use pf_ui::components::{button, ButtonKind};
use std::collections::{HashMap, HashSet};

const W_ARROW: f32 = 30.0;
const W_LABEL: f32 = 84.0;
const W_BASE: f32 = 120.0;
const W_BREAK: f32 = 96.0;

/// État de la fenêtre (interface seulement, jamais enregistré).
#[derive(Debug, Clone)]
pub struct SizesDialog {
    /// Gammes dépliées (aucune au départ).
    pub open: HashSet<SizeRangeId>,
    /// Section « Nouvelle gamme » dépliée (repliée au départ).
    pub new_open: bool,
    new_name: String,
    new_kind: SizeKind,
    new_sizes: String,
    /// Saisie « nouvelle taille » par gamme.
    add_label: HashMap<SizeRangeId, String>,
    /// Tampons des champs de nom / libellé, par identifiant ; resynchronisés sur le projet tant
    /// que le champ n'a pas le focus (annuler / rétablir, refus).
    buffers: HashMap<u64, String>,
    /// Dernier refus : gamme concernée (`None` = création) et texte localisé.
    error: Option<(Option<SizeRangeId>, String)>,
}

impl Default for SizesDialog {
    fn default() -> Self {
        SizesDialog {
            open: HashSet::new(),
            new_open: false,
            new_name: String::new(),
            new_kind: SizeKind::Alphanumeric,
            new_sizes: String::new(),
            add_label: HashMap::new(),
            buffers: HashMap::new(),
            error: None,
        }
    }
}

impl SizesDialog {
    /// Fenêtre avec ces gammes dépliées (étape de capture) ; les autres restent repliées.
    pub fn with_open(open: impl IntoIterator<Item = SizeRangeId>) -> SizesDialog {
        SizesDialog {
            open: open.into_iter().collect(),
            ..SizesDialog::default()
        }
    }
}

enum Action {
    AddRange,
    RenameRange { range: SizeRangeId, name: String },
    RemoveRange { range: SizeRangeId },
    AddSize { range: SizeRangeId, label: String },
    RemoveSize { size: SizeId },
    RenameSize { size: SizeId, label: String },
    MoveSize { size: SizeId, to: usize },
    SetBreak { size: SizeId, on: bool },
    SetBase { size: SizeId },
}

fn kind_label(kind: SizeKind) -> String {
    match kind {
        SizeKind::Alphanumeric => tr("sizes.kind.alphanumeric"),
        SizeKind::Numeric => tr("sizes.kind.numeric"),
    }
}

fn problem_text(p: &TextProblem) -> String {
    match p {
        TextProblem::Empty => tr("sizes.problem.empty"),
        TextProblem::TooLong { max } => {
            tr_args("sizes.problem.too_long", &[("max", &max.to_string())])
        }
        TextProblem::Control => tr("sizes.problem.control"),
        TextProblem::Untrimmed => tr("sizes.problem.untrimmed"),
    }
}

/// Message d'un refus de tailles dans la langue de l'interface (le texte du cœur est en français).
pub(crate) fn error_text(e: &SizeError) -> String {
    match e {
        SizeError::UnknownRange(_) | SizeError::UnknownSize(_) => tr("sizes.err.unknown"),
        SizeError::TooManyRanges { .. } => tr_args(
            "sizes.err.too_many_ranges",
            &[("max", &MAX_SIZE_RANGES.to_string())],
        ),
        SizeError::TooManySizes { .. } => tr_args(
            "sizes.err.too_many_sizes",
            &[("max", &MAX_SIZES_PER_RANGE.to_string())],
        ),
        SizeError::RangeName { problem } => tr_args(
            "sizes.err.range_name",
            &[("reason", &problem_text(problem))],
        ),
        SizeError::RangeNameTaken { name } => tr_args("sizes.err.range_taken", &[("name", name)]),
        SizeError::SizeLabel { label, problem } => tr_args(
            "sizes.err.label",
            &[("label", label), ("reason", &problem_text(problem))],
        ),
        SizeError::SizeLabelTaken { label } => {
            tr_args("sizes.err.label_taken", &[("label", label)])
        }
        SizeError::NoSizes => tr("sizes.err.no_sizes"),
        SizeError::BaseOutOfRange { .. } | SizeError::IndexOutOfRange { .. } => {
            tr("sizes.err.index")
        }
        SizeError::CannotRemoveBase { label } => {
            tr_args("sizes.err.remove_base", &[("label", label)])
        }
        SizeError::CannotRemoveLastSize => tr("sizes.err.last_size"),
        SizeError::BaseShiftsOtherRanges { label } => {
            tr_args("sizes.err.base_shifts_ranges", &[("label", label)])
        }
        SizeError::BaseShiftsMaintained { label, sheet } => tr_args(
            "sizes.err.base_shifts_maintained",
            &[("label", label), ("sheet", sheet)],
        ),
        SizeError::BaseShiftsVariables { label } => {
            tr_args("sizes.err.base_shifts_variables", &[("label", label)])
        }
        SizeError::NotASizeCommand | SizeError::Inconsistent(_) => {
            tr_args("sizes.err.other", &[("detail", &e.to_string())])
        }
    }
}

/// Découpe une saisie « XS S M, L; XL » en libellés (espaces, virgules, points-virgules).
fn split_labels(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

/// Icône pleine (vraie) ou creuse (fausse) : disque pour la base, losange pour la rupture.
pub(crate) fn marker_icon(ui: &mut egui::Ui, diamond: bool, filled: bool) {
    let tokens = pf_ui::theme::tokens(ui.ctx());
    let stroke = egui::Stroke::new(1.5_f32, tokens.text_secondary);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 18.0), egui::Sense::hover());
    let c = rect.center();
    let p = ui.painter();
    if diamond {
        let pts = vec![
            c + egui::vec2(0.0, -5.5),
            c + egui::vec2(5.5, 0.0),
            c + egui::vec2(0.0, 5.5),
            c + egui::vec2(-5.5, 0.0),
        ];
        let fill = if filled {
            tokens.text_secondary
        } else {
            egui::Color32::TRANSPARENT
        };
        p.add(egui::Shape::convex_polygon(pts, fill, stroke));
    } else {
        let fill = if filled {
            tokens.text_secondary
        } else {
            egui::Color32::TRANSPARENT
        };
        p.circle(c, 5.0, fill, stroke);
    }
}

pub(crate) fn fold_icon(ui: &mut egui::Ui, open: bool) {
    let tokens = pf_ui::theme::tokens(ui.ctx());
    let stroke = egui::Stroke::new(1.6_f32, tokens.text_secondary);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
    let c = rect.center();
    let pts: [egui::Pos2; 3] = if open {
        [
            c + egui::vec2(-4.0, -2.0),
            c + egui::vec2(4.0, -2.0),
            c + egui::vec2(0.0, 3.0),
        ]
    } else {
        [
            c + egui::vec2(-2.0, -4.0),
            c + egui::vec2(-2.0, 4.0),
            c + egui::vec2(3.0, 0.0),
        ]
    };
    let p = ui.painter();
    p.line_segment([pts[0], pts[1]], stroke);
    p.line_segment([pts[1], pts[2]], stroke);
    p.line_segment([pts[2], pts[0]], stroke);
}

/// Bouton flèche dessiné (la police embarquée n'a pas de flèches) ; son nom accessible est
/// `label` (lu par les lecteurs d'écran et les tests), comme son info-bulle.
fn arrow_button(ui: &mut egui::Ui, up: bool, enabled: bool, label: &str) -> egui::Response {
    let response = ui.add_enabled(
        enabled,
        egui::Button::new("").min_size(egui::vec2(26.0, 20.0)),
    );
    let color = if enabled {
        ui.visuals().widgets.inactive.fg_stroke.color
    } else {
        ui.visuals()
            .widgets
            .noninteractive
            .fg_stroke
            .color
            .gamma_multiply(0.45)
    };
    let c = response.rect.center();
    let s = if up { -1.0_f32 } else { 1.0 };
    let pts = vec![
        c + egui::vec2(-4.5, -2.0 * s),
        c + egui::vec2(4.5, -2.0 * s),
        c + egui::vec2(0.0, 3.0 * s),
    ];
    ui.painter()
        .add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    response.on_hover_text(label)
}

/// Cellule de largeur fixe : les colonnes restent alignées d'une ligne à l'autre.
pub(crate) fn cell(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    let size = egui::vec2(width, 22.0);
    ui.allocate_ui_with_layout(
        size,
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_size(size);
            add(ui);
        },
    );
}

/// Champ de texte à tampon resynchronisé sur le projet tant qu'il n'a pas le focus ; renvoie le
/// texte validé (focus perdu ou Entrée) quand il diffère du projet.
fn text_field(
    ui: &mut egui::Ui,
    buffers: &mut HashMap<u64, String>,
    key: u64,
    kind: &str,
    current: &str,
    width: f32,
) -> Option<String> {
    let id = egui::Id::new((kind, key));
    let focused = ui.ctx().memory(|m| m.has_focus(id));
    let buf = buffers.entry(key).or_default();
    if !focused {
        *buf = current.to_string();
    }
    let response = crate::a11y::add_text(
        ui,
        egui::TextEdit::singleline(buf).id(id).desired_width(width),
    );
    (response.lost_focus() && buf.as_str() != current).then(|| buf.clone())
}

impl App {
    /// Ouvre la fenêtre « Tailles » (menu Affichage, sans raccourci).
    pub fn open_sizes(&mut self) {
        if self.sizes.is_none() {
            self.sizes = Some(SizesDialog::default());
        }
    }

    /// Commande correspondant à une action de la fenêtre.
    fn sizes_command(&self, d: &SizesDialog, action: &Action) -> Command {
        match action {
            Action::AddRange => Command::AddSizeRange {
                name: d.new_name.clone(),
                kind: d.new_kind,
                labels: split_labels(&d.new_sizes),
                base: 0,
            },
            Action::RenameRange { range, name } => Command::RenameSizeRange {
                range: *range,
                name: name.clone(),
            },
            Action::RemoveRange { range } => Command::RemoveSizeRange { range: *range },
            Action::AddSize { range, label } => Command::AddSize {
                range: *range,
                label: label.clone(),
                at: None,
            },
            Action::RemoveSize { size } => Command::RemoveSize { size: *size },
            Action::RenameSize { size, label } => Command::RenameSize {
                size: *size,
                label: label.clone(),
            },
            Action::MoveSize { size, to } => Command::MoveSize {
                size: *size,
                to: *to,
            },
            Action::SetBreak { size, on } => Command::SetSizeBreak {
                size: *size,
                break_size: *on,
            },
            Action::SetBase { size } => Command::SetBaseSize { size: *size },
        }
    }

    /// Gamme concernée par une action (pour afficher le refus au bon endroit).
    fn sizes_scope(&self, action: &Action) -> Option<SizeRangeId> {
        let project = self.editor.project();
        match action {
            Action::AddRange => None,
            Action::RenameRange { range, .. }
            | Action::RemoveRange { range }
            | Action::AddSize { range, .. } => Some(*range),
            Action::RemoveSize { size }
            | Action::RenameSize { size, .. }
            | Action::MoveSize { size, .. }
            | Action::SetBreak { size, .. }
            | Action::SetBase { size } => project.find_size(*size).map(|(r, _)| r.id),
        }
    }

    /// Vérification typée d'abord (refus localisé, rien n'est modifié), commande ensuite.
    fn sizes_act(&mut self, d: &mut SizesDialog, action: Action) {
        let cmd = self.sizes_command(d, &action);
        let scope = self.sizes_scope(&action);
        if let Err(e) = self.editor.project().check_size_command(&cmd) {
            d.error = Some((scope, error_text(&e)));
            return;
        }
        let before = self
            .editor
            .project()
            .sizes
            .iter()
            .map(|r| r.id)
            .collect::<Vec<_>>();
        match self.editor.exec(cmd) {
            Some(outcome) => {
                d.error = None;
                match action {
                    Action::AddRange => {
                        d.new_name.clear();
                        d.new_sizes.clear();
                        // La gamme créée s'ouvre : l'utilisatrice voit le résultat.
                        for id in outcome.size_ranges {
                            if !before.contains(&id) {
                                d.open.insert(id);
                            }
                        }
                    }
                    Action::AddSize { range, .. } => {
                        d.add_label.remove(&range);
                    }
                    _ => {}
                }
            }
            None => d.error = Some((scope, self.editor.status.text.clone())),
        }
    }

    pub fn sizes_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.sizes.take() else {
            return;
        };
        let ranges: Vec<SizeRange> = self.editor.project().sizes.clone();
        d.open.retain(|id| ranges.iter().any(|r| r.id == *id));
        let tokens = pf_ui::theme::tokens(ctx);
        let (danger, secondary) = (tokens.danger, tokens.text_secondary);
        let mut open = true;
        let mut close = false;
        let mut actions: Vec<Action> = Vec::new();
        let slot = crate::dock::slot(ctx, &self.dock, ToolWin::Sizes);
        let width = self.dock.content_width(ToolWin::Sizes);
        egui::Window::new(tr("sizes.title"))
            .id(ToolWin::Sizes.id())
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
                        crate::a11y::label(ui, RichText::new(tr("sizes.intro")).color(secondary));
                        ui.add_space(4.0);
                        if ranges.is_empty() {
                            crate::a11y::label(
                                ui,
                                RichText::new(tr("sizes.empty")).color(secondary),
                            );
                        }
                        for range in &ranges {
                            draw_range(ui, &mut d, range, danger, secondary, &mut actions);
                        }
                        ui.separator();
                        draw_new_range(ui, &mut d, danger, secondary, &mut actions);
                        ui.add_space(6.0);
                        crate::a11y::label(ui, RichText::new(tr("sizes.note")).color(secondary));
                        ui.add_space(6.0);
                        close = button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                    });
            });
        for a in actions {
            self.sizes_act(&mut d, a);
        }
        if open && !close {
            self.sizes = Some(d);
        }
    }
}

fn draw_range(
    ui: &mut egui::Ui,
    d: &mut SizesDialog,
    range: &SizeRange,
    danger: egui::Color32,
    secondary: egui::Color32,
    actions: &mut Vec<Action>,
) {
    let is_open = d.open.contains(&range.id);
    let header = if range.sizes.len() == 1 {
        tr_args(
            "sizes.range.header.one",
            &[("name", &range.name), ("kind", &kind_label(range.kind))],
        )
    } else {
        tr_args(
            "sizes.range.header",
            &[
                ("name", &range.name),
                ("kind", &kind_label(range.kind)),
                ("count", &range.sizes.len().to_string()),
            ],
        )
    };
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        fold_icon(ui, is_open);
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
        return;
    }
    ui.horizontal(|ui| {
        crate::a11y::label(ui, RichText::new(tr("sizes.range.name")).color(secondary));
        if let Some(name) = text_field(
            ui,
            &mut d.buffers,
            range.id.0,
            "range_name",
            &range.name,
            150.0,
        ) {
            actions.push(Action::RenameRange {
                range: range.id,
                name,
            });
        }
    });
    let len = range.sizes.len();
    for (i, size) in range.sizes.iter().enumerate() {
        let is_base = size.id == range.base;
        ui.horizontal(|ui| {
            cell(ui, W_ARROW, |ui| {
                if arrow_button(ui, true, i > 0, &tr("sizes.up")).clicked() {
                    actions.push(Action::MoveSize {
                        size: size.id,
                        to: i - 1,
                    });
                }
            });
            cell(ui, W_ARROW, |ui| {
                if arrow_button(ui, false, i + 1 < len, &tr("sizes.down")).clicked() {
                    actions.push(Action::MoveSize {
                        size: size.id,
                        to: i + 1,
                    });
                }
            });
            cell(ui, W_LABEL, |ui| {
                if let Some(label) = text_field(
                    ui,
                    &mut d.buffers,
                    size.id.0,
                    "size_label",
                    &size.label,
                    W_LABEL - 8.0,
                ) {
                    actions.push(Action::RenameSize {
                        size: size.id,
                        label,
                    });
                }
            });
            cell(ui, W_BASE, |ui| {
                if is_base {
                    marker_icon(ui, false, true);
                    crate::a11y::label(ui, RichText::new(tr("sizes.base")).strong());
                } else {
                    marker_icon(ui, false, false);
                    if ui.button(tr("sizes.set_base")).clicked() {
                        actions.push(Action::SetBase { size: size.id });
                    }
                }
            });
            cell(ui, W_BREAK, |ui| {
                marker_icon(ui, true, size.break_size);
                let r = ui
                    .selectable_label(size.break_size, tr("sizes.break"))
                    .on_hover_text(tr("sizes.break.hint"));
                if r.clicked() {
                    actions.push(Action::SetBreak {
                        size: size.id,
                        on: !size.break_size,
                    });
                }
            });
            if ui
                .add(egui::Button::new(RichText::new("×").size(16.0)))
                .on_hover_text(tr("sizes.remove"))
                .clicked()
            {
                actions.push(Action::RemoveSize { size: size.id });
            }
        });
    }
    ui.horizontal(|ui| {
        let buf = d.add_label.entry(range.id).or_default();
        let r = crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(buf)
                .hint_text(tr("sizes.add.hint"))
                .desired_width(120.0),
        );
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let clicked = ui.button(tr("sizes.add")).clicked();
        if (enter || clicked) && !buf.trim().is_empty() {
            actions.push(Action::AddSize {
                range: range.id,
                label: buf.clone(),
            });
        }
    });
    if button(ui, &tr("sizes.remove_range"), ButtonKind::Danger).clicked() {
        actions.push(Action::RemoveRange { range: range.id });
    }
    if let Some((Some(scope), text)) = &d.error {
        if *scope == range.id {
            crate::a11y::label(ui, RichText::new(text).color(danger));
        }
    }
}

fn draw_new_range(
    ui: &mut egui::Ui,
    d: &mut SizesDialog,
    danger: egui::Color32,
    secondary: egui::Color32,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        fold_icon(ui, d.new_open);
        let r =
            ui.add(egui::Button::new(RichText::new(tr("sizes.new.title")).strong()).frame(false));
        if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            d.new_open = !d.new_open;
        }
    });
    if !d.new_open {
        return;
    }
    crate::a11y::add_text(
        ui,
        egui::TextEdit::singleline(&mut d.new_name)
            .hint_text(tr("sizes.new.name"))
            .desired_width(220.0),
    );
    ui.horizontal(|ui| {
        crate::a11y::label(ui, RichText::new(tr("sizes.new.kind")).color(secondary));
        for kind in [SizeKind::Alphanumeric, SizeKind::Numeric] {
            if ui
                .selectable_label(d.new_kind == kind, kind_label(kind))
                .clicked()
            {
                d.new_kind = kind;
            }
        }
    });
    crate::a11y::add_text(
        ui,
        egui::TextEdit::singleline(&mut d.new_sizes)
            .hint_text(tr("sizes.new.sizes"))
            .desired_width(260.0),
    );
    crate::a11y::label(ui, RichText::new(tr("sizes.new.hint")).color(secondary));
    if button(ui, &tr("sizes.new.create"), ButtonKind::Primary).clicked() {
        actions.push(Action::AddRange);
    }
    if let Some((None, text)) = &d.error {
        crate::a11y::label(ui, RichText::new(text).color(danger));
    }
}

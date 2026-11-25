//! US-03.1a — fenêtre « Variables » : tableau nom | unité | expression | valeur calculée.
//!
//! La fenêtre ne calcule rien : elle appelle `pf_core::evaluate_variables` (lecture pure) pour
//! **vérifier** une saisie, et n'envoie la commande (`AddVariable`, `SetVariableExpr`,
//! `RenameVariable`, `RemoveVariable`, `SetVariableUnit`) au cœur que si la saisie est valide :
//! **rien n'est modifié tant qu'une saisie est invalide**, l'erreur (catégorie + position) est
//! affichée sous la ligne en cause. Une saisie est validée à la perte du focus ou à Entrée.
//! Les variables se citent dans la formule d'un paramètre (US-03.1b, `formulas.rs`) : la fenêtre
//! dit lesquels.

use crate::a11y::ComboExt;
use crate::app::App;
use crate::dock::ToolWin;
use eframe::egui;
use pf_core::expr::{ExprError, ExprErrorKind, RESERVED_WORDS};
use pf_core::{
    evaluate_variables, Command, SizeId, Variable, VariableError, VariableUnit, MAX_VARIABLES,
    MAX_VARIABLE_NAME_CHARS,
};
use pf_i18n::{tr, tr_args};
use pf_ui::components::{button, ButtonKind};
use std::collections::HashMap;

const W_NAME: f32 = 96.0;
const W_UNIT: f32 = 62.0;
const W_EXPR: f32 = 156.0;
const W_VALUE: f32 = 92.0;
const W_REMOVE: f32 = 24.0;

/// Tampons de saisie d'une ligne : ce qui est tapé, et ce que le projet contenait à la dernière
/// synchronisation (si le projet change, par exemple par Annuler, la ligne repart du projet).
#[derive(Debug, Clone)]
struct RowBuf {
    name: String,
    expr: String,
    synced_name: String,
    synced_expr: String,
}

/// Saisie de la valeur d'une variable pour la taille choisie (US-09.7) : texte tapé et texte du
/// projet à la dernière synchronisation (vide = la taille garde l'expression de base).
#[derive(Debug, Clone, Default)]
struct SizeBuf {
    text: String,
    synced: String,
}

#[derive(Debug, Clone)]
pub struct VariablesDialog {
    rows: Vec<RowBuf>,
    /// Erreur affichée sous une ligne, par nom de variable (au moment de la synchronisation).
    errors: HashMap<String, String>,
    new_name: String,
    new_unit: VariableUnit,
    new_expr: String,
    new_error: Option<String>,
    /// Section « valeurs par taille » : ouverte à la première apparition, taille choisie, saisies.
    sizes_open: bool,
    size_sel: Option<SizeId>,
    size_rows: Vec<SizeBuf>,
    size_errors: HashMap<String, String>,
}

impl Default for VariablesDialog {
    fn default() -> Self {
        VariablesDialog {
            rows: Vec::new(),
            errors: HashMap::new(),
            new_name: String::new(),
            new_unit: VariableUnit::Mm,
            new_expr: String::new(),
            new_error: None,
            sizes_open: false,
            size_sel: None,
            size_rows: Vec::new(),
            size_errors: HashMap::new(),
        }
    }
}

enum Action {
    Add,
    /// Valeur d'une variable pour la taille choisie ; texte vide = retirer la valeur.
    SetSize {
        name: String,
        expr: String,
    },
    SetExpr {
        name: String,
        expr: String,
    },
    Rename {
        from: String,
        to: String,
    },
    SetUnit {
        name: String,
        unit: VariableUnit,
    },
    Remove {
        name: String,
    },
}

impl VariablesDialog {
    /// Aligne les tampons sur le projet : une ligne dont le projet a changé repart du projet
    /// (et perd son erreur) ; une ligne inchangée garde sa saisie en cours.
    fn sync(&mut self, vars: &[Variable]) {
        self.rows.truncate(vars.len());
        for (i, v) in vars.iter().enumerate() {
            match self.rows.get_mut(i) {
                Some(r) if r.synced_name == v.name && r.synced_expr == v.expr => {}
                Some(r) => {
                    self.errors.remove(&r.synced_name);
                    *r = RowBuf::from(v);
                }
                None => self.rows.push(RowBuf::from(v)),
            }
        }
        let live: Vec<&str> = vars.iter().map(|v| v.name.as_str()).collect();
        self.errors.retain(|k, _| live.contains(&k.as_str()));
        self.size_errors.retain(|k, _| live.contains(&k.as_str()));
    }

    /// Aligne les saisies « valeurs par taille » sur le projet pour la taille choisie (une saisie
    /// dont le projet a changé repart du projet, comme les lignes de la table).
    fn sync_sizes(&mut self, vars: &[Variable], sizes: &[(SizeId, String)]) {
        if !self
            .size_sel
            .is_some_and(|s| sizes.iter().any(|(id, _)| *id == s))
        {
            self.size_sel = sizes.first().map(|(id, _)| *id);
        }
        self.size_rows.truncate(vars.len());
        for (i, v) in vars.iter().enumerate() {
            let synced = self
                .size_sel
                .and_then(|s| v.per_size.iter().find(|e| e.size == s))
                .map(|e| e.expr.clone())
                .unwrap_or_default();
            match self.size_rows.get_mut(i) {
                Some(r) if r.synced == synced => {}
                Some(r) => {
                    self.size_errors.remove(&v.name);
                    *r = SizeBuf {
                        text: synced.clone(),
                        synced,
                    };
                }
                None => self.size_rows.push(SizeBuf {
                    text: synced.clone(),
                    synced,
                }),
            }
        }
    }
}

impl RowBuf {
    fn from(v: &Variable) -> RowBuf {
        RowBuf {
            name: v.name.clone(),
            expr: v.expr.clone(),
            synced_name: v.name.clone(),
            synced_expr: v.expr.clone(),
        }
    }
}

fn unit_label(unit: VariableUnit) -> String {
    match unit {
        VariableUnit::Mm => tr("variables.unit.mm"),
        VariableUnit::Deg => tr("variables.unit.deg"),
        VariableUnit::Unitless => tr("variables.unit.none"),
    }
}

/// Valeur calculée affichée avec le formateur de l'application (séparateur de la langue).
fn value_text(unit: VariableUnit, value: f64) -> String {
    match unit {
        // L'unité d'une variable est déclarée en mm : elle ne suit pas l'unité d'affichage.
        VariableUnit::Mm => pf_core::units::format_length(value, Default::default()),
        VariableUnit::Deg => pf_core::input::format_deg(value),
        VariableUnit::Unitless => pf_core::input::format_number(value, 3),
    }
}

fn name_reason(name: &str) -> String {
    let count = name.chars().count();
    if count == 0 {
        tr("variables.err.name_empty")
    } else if count > MAX_VARIABLE_NAME_CHARS {
        tr_args(
            "variables.err.name_long",
            &[("max", &MAX_VARIABLE_NAME_CHARS.to_string())],
        )
    } else if RESERVED_WORDS.iter().any(|w| w.eq_ignore_ascii_case(name)) {
        tr("variables.err.name_reserved")
    } else if name.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        tr("variables.err.name_digit")
    } else {
        let bad = name
            .chars()
            .find(|c| !(c.is_ascii_alphanumeric() || *c == '_'))
            .map(String::from)
            .unwrap_or_default();
        tr_args("variables.err.name_char", &[("char", &bad)])
    }
}

pub(crate) fn expr_error_text(e: &ExprError) -> String {
    let pos = (e.pos + 1).to_string();
    let name = e.name.clone().unwrap_or_default();
    match e.kind {
        ExprErrorKind::Syntax => tr_args("variables.err.syntax", &[("pos", &pos)]),
        ExprErrorKind::UnknownVariable => tr_args(
            "variables.err.unknown_var",
            &[("name", &name), ("pos", &pos)],
        ),
        ExprErrorKind::DivisionByZero => tr_args("variables.err.div0", &[("pos", &pos)]),
        ExprErrorKind::Domain => tr_args("variables.err.domain", &[("pos", &pos)]),
        ExprErrorKind::Cycle => tr_args("variables.err.cycle", &[("path", &name)]),
        ExprErrorKind::TooDeep => tr_args("variables.err.deep", &[("pos", &pos)]),
        ExprErrorKind::TooLong => tr_args(
            "variables.err.long",
            &[("max", &pf_core::expr::MAX_EXPR_CHARS.to_string())],
        ),
    }
}

/// Message d'une erreur de la table, dans la langue de l'interface (le détail de syntaxe du
/// cœur, en français, reste dans l'info-bulle de la ligne).
fn error_text(e: &VariableError) -> String {
    match e {
        VariableError::TooMany { .. } => tr_args(
            "variables.err.too_many",
            &[("max", &MAX_VARIABLES.to_string())],
        ),
        VariableError::BadName { name, .. } => tr_args(
            "variables.err.name",
            &[("name", name), ("reason", &name_reason(name))],
        ),
        VariableError::DuplicateName { name, other } => {
            if name == other {
                tr_args("variables.err.exists", &[("name", name)])
            } else {
                tr_args(
                    "variables.err.case_clash",
                    &[("name", name), ("other", other)],
                )
            }
        }
        VariableError::Unknown { name } => tr_args("variables.err.unknown", &[("name", name)]),
        VariableError::Expr { error, .. } => expr_error_text(error),
        VariableError::InUse { name, dependents } => tr_args(
            "variables.err.in_use",
            &[("name", name), ("list", &dependents.join(", "))],
        ),
    }
}

impl App {
    /// Ouvre la fenêtre « Variables » (menu Édition, sans raccourci).
    pub fn open_variables(&mut self) {
        if self.variables.is_none() {
            self.variables = Some(VariablesDialog::default());
        }
    }

    /// Une saisie d'expression est-elle acceptée pour cette table ? Lecture pure.
    fn variables_check(&self, edit: impl FnOnce(&mut Vec<Variable>)) -> Result<(), VariableError> {
        let mut cand = self.editor.project().variables.clone();
        edit(&mut cand);
        evaluate_variables(&cand).map(|_| ())
    }

    /// Exécute une action de la fenêtre : vérification pure d'abord, commande ensuite.
    fn variables_act(&mut self, d: &mut VariablesDialog, action: Action) {
        match action {
            Action::Add => {
                let (name, unit) = (d.new_name.clone(), d.new_unit);
                let expr = d.new_expr.trim().to_string();
                let check = self.variables_check(|t| {
                    t.push(Variable {
                        name: name.clone(),
                        unit,
                        expr: expr.clone(),
                        per_size: Vec::new(),
                    });
                });
                match check {
                    Err(e) => d.new_error = Some(error_text(&e)),
                    Ok(()) => {
                        let cmd = Command::AddVariable { name, unit, expr };
                        if self.editor.exec(cmd).is_some() {
                            d.new_name.clear();
                            d.new_expr.clear();
                            d.new_error = None;
                        } else {
                            d.new_error = Some(self.editor.status.text.clone());
                        }
                    }
                }
            }
            // Valeur d'une variable pour la taille choisie : le cœur valide (table, formules
            // liées, géométrie de la taille) et refuse sans trace ; sa raison reste sous la ligne.
            Action::SetSize { name, expr } => {
                let Some(size) = d.size_sel else { return };
                let cmd = if expr.trim().is_empty() {
                    Command::ClearVariableSizeExpr {
                        name: name.clone(),
                        size,
                    }
                } else {
                    Command::SetVariableSizeExpr {
                        name: name.clone(),
                        size,
                        expr,
                    }
                };
                if self.editor.exec(cmd).is_some() {
                    d.size_errors.remove(&name);
                } else {
                    d.size_errors.insert(name, self.editor.status.text.clone());
                }
            }
            Action::SetExpr { name, expr } => {
                let text = expr.trim().to_string();
                let check = self.variables_check(|t| {
                    if let Some(v) = t.iter_mut().find(|v| v.name == name) {
                        v.expr = text.clone();
                    }
                });
                self.variables_finish(
                    d,
                    &name,
                    check,
                    Command::SetVariableExpr {
                        name: name.clone(),
                        expr,
                    },
                );
            }
            Action::Rename { from, to } => {
                let check = self.variables_check(|t| {
                    if let Some(v) = t.iter_mut().find(|v| v.name == from) {
                        v.name = to.clone();
                    }
                });
                // Un renommage réécrit aussi les références : la vérification de nom suffit ici,
                // la commande refuse le reste (expression devenue trop longue).
                let check = match check {
                    Err(VariableError::Expr { .. }) => Ok(()),
                    other => other,
                };
                self.variables_finish(
                    d,
                    &from,
                    check,
                    Command::RenameVariable {
                        from: from.clone(),
                        to,
                    },
                );
            }
            Action::SetUnit { name, unit } => {
                self.editor.exec(Command::SetVariableUnit { name, unit });
            }
            Action::Remove { name } => {
                let dependents = self.editor.project().variable_dependents(&name);
                if dependents.is_empty() {
                    // Une formule liée (US-03.1b) peut encore la refuser : le message du cœur
                    // nomme alors les paramètres.
                    if self
                        .editor
                        .exec(Command::RemoveVariable { name: name.clone() })
                        .is_none()
                    {
                        d.errors.insert(name, self.editor.status.text.clone());
                    }
                } else {
                    let e = VariableError::InUse {
                        name: name.clone(),
                        dependents,
                    };
                    d.errors.insert(name, error_text(&e));
                }
            }
        }
    }

    fn variables_finish(
        &mut self,
        d: &mut VariablesDialog,
        key: &str,
        check: Result<(), VariableError>,
        cmd: Command,
    ) {
        match check {
            Err(e) => {
                d.errors.insert(key.to_string(), error_text(&e));
            }
            Ok(()) => {
                if self.editor.exec(cmd).is_some() {
                    d.errors.remove(key);
                } else {
                    d.errors
                        .insert(key.to_string(), self.editor.status.text.clone());
                }
            }
        }
    }

    pub fn variables_window(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.variables.take() else {
            return;
        };
        let vars = self.editor.project().variables.clone();
        let values = self.editor.project().variable_values().ok();
        d.sync(&vars);
        // Tailles autres que la base de leur gamme (une valeur par taille ne vise pas la base).
        let sizes: Vec<(SizeId, String)> = self
            .editor
            .project()
            .sizes
            .iter()
            .flat_map(|r| {
                r.sizes
                    .iter()
                    .filter(|s| s.id != r.base)
                    .map(|s| (s.id, format!("{} · {}", r.name, s.label)))
            })
            .collect();
        d.sync_sizes(&vars, &sizes);
        let size_values = d
            .size_sel
            .and_then(|s| self.editor.project().variable_values_for_size(Some(s)).ok());
        let danger = pf_ui::theme::tokens(ctx).danger;
        let secondary = pf_ui::theme::tokens(ctx).text_secondary;
        let mut open = true;
        let mut close = false;
        let mut actions: Vec<Action> = Vec::new();
        let slot = crate::dock::slot(ctx, &self.dock, ToolWin::Variables);
        let width = self.dock.content_width(ToolWin::Variables);
        egui::Window::new(tr("variables.title"))
            .id(ToolWin::Variables.id())
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
                            egui::RichText::new(tr("variables.intro")).color(secondary),
                        );
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            for (w, title) in [
                                (W_NAME, tr("variables.col.name")),
                                (W_UNIT, tr("variables.col.unit")),
                                (W_EXPR, tr("variables.col.expr")),
                                (W_VALUE, tr("variables.col.value")),
                            ] {
                                ui.add_sized(
                                    [w, 18.0],
                                    egui::Label::new(egui::RichText::new(title).strong()),
                                );
                            }
                        });
                        if vars.is_empty() {
                            crate::a11y::label(
                                ui,
                                egui::RichText::new(tr("variables.empty")).color(secondary),
                            );
                        }
                        for (i, var) in vars.iter().enumerate() {
                            let value = values.as_ref().and_then(|v| v.get(i)).map(|v| v.1);
                            draw_row(ui, &mut d, i, var, value, danger, &mut actions);
                        }
                        ui.separator();
                        draw_add(ui, &mut d, danger, &mut actions);
                        ui.add_space(4.0);
                        draw_sizes(
                            ui,
                            &mut d,
                            &vars,
                            &sizes,
                            size_values.as_deref(),
                            (danger, secondary),
                            &mut actions,
                        );
                        ui.add_space(6.0);
                        crate::a11y::label(
                            ui,
                            egui::RichText::new(tr("variables.syntax")).color(secondary),
                        );
                        crate::a11y::label(
                            ui,
                            egui::RichText::new(tr("variables.next_step")).color(secondary),
                        );
                        ui.add_space(6.0);
                        close = button(ui, &tr("dialog.close"), ButtonKind::Secondary).clicked();
                    });
            });
        for a in actions {
            self.variables_act(&mut d, a);
        }
        if open && !close {
            self.variables = Some(d);
        }
    }
}

/// Cellule de largeur fixe : les colonnes restent alignées d'une ligne à l'autre, quelle que
/// soit la largeur du contenu (le curseur avance de la cellule entière).
fn cell(ui: &mut egui::Ui, width: f32, layout: egui::Layout, add: impl FnOnce(&mut egui::Ui)) {
    let size = egui::vec2(width, 22.0);
    ui.allocate_ui_with_layout(size, layout, |ui| {
        ui.set_min_size(size);
        add(ui);
    });
}

fn draw_row(
    ui: &mut egui::Ui,
    d: &mut VariablesDialog,
    i: usize,
    var: &Variable,
    value: Option<f64>,
    danger: egui::Color32,
    actions: &mut Vec<Action>,
) {
    let key = var.name.clone();
    ui.horizontal(|ui| {
        let buf = &mut d.rows[i];
        let failing = d.errors.contains_key(&key);
        let name = crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(&mut buf.name)
                .id_salt(("var-name", &key))
                .text_color_opt(failing.then_some(danger))
                .desired_width(W_NAME),
        );
        let mut unit = var.unit;
        cell(
            ui,
            W_UNIT,
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                egui::ComboBox::from_id_salt(("var-unit", &key))
                    .width(W_UNIT - 22.0)
                    .selected_text(unit_label(unit))
                    .show_ui_named(ui, |ui| {
                        for u in [VariableUnit::Mm, VariableUnit::Deg, VariableUnit::Unitless] {
                            ui.selectable_value(&mut unit, u, unit_label(u));
                        }
                    });
            },
        );
        let expr = crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(&mut buf.expr)
                .id_salt(("var-expr", &key))
                .font(egui::TextStyle::Monospace)
                .text_color_opt(failing.then_some(danger))
                .desired_width(W_EXPR),
        );
        cell(
            ui,
            W_VALUE,
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                let text = value.map_or_else(|| "—".to_string(), |v| value_text(var.unit, v));
                crate::a11y::label(ui, egui::RichText::new(text).monospace());
            },
        );
        let remove = ui
            .add_sized(
                [W_REMOVE, 22.0],
                egui::Button::new(egui::RichText::new("×").size(16.0)),
            )
            .on_hover_text(tr("variables.remove"));
        if name.changed() || expr.changed() {
            d.errors.remove(&key);
        }
        if name.lost_focus() && buf.name != buf.synced_name {
            actions.push(Action::Rename {
                from: key.clone(),
                to: buf.name.clone(),
            });
        }
        if expr.lost_focus() && buf.expr.trim() != buf.synced_expr {
            actions.push(Action::SetExpr {
                name: key.clone(),
                expr: buf.expr.clone(),
            });
        }
        if unit != var.unit {
            actions.push(Action::SetUnit {
                name: key.clone(),
                unit,
            });
        }
        if remove.clicked() {
            actions.push(Action::Remove { name: key.clone() });
        }
    });
    if let Some(msg) = d.errors.get(&key) {
        crate::a11y::label(ui, egui::RichText::new(msg).color(danger))
            .on_hover_text(tr("variables.err.hover"));
    }
}

fn draw_add(
    ui: &mut egui::Ui,
    d: &mut VariablesDialog,
    danger: egui::Color32,
    actions: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        let name = crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(&mut d.new_name)
                .id_salt("var-new-name")
                .hint_text(tr("variables.new_name"))
                .desired_width(W_NAME),
        );
        let mut unit = d.new_unit;
        cell(
            ui,
            W_UNIT,
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                egui::ComboBox::from_id_salt("var-new-unit")
                    .width(W_UNIT - 22.0)
                    .selected_text(unit_label(unit))
                    .show_ui_named(ui, |ui| {
                        for u in [VariableUnit::Mm, VariableUnit::Deg, VariableUnit::Unitless] {
                            ui.selectable_value(&mut unit, u, unit_label(u));
                        }
                    });
            },
        );
        d.new_unit = unit;
        let expr = crate::a11y::add_text(
            ui,
            egui::TextEdit::singleline(&mut d.new_expr)
                .id_salt("var-new-expr")
                .font(egui::TextStyle::Monospace)
                .hint_text(tr("variables.new_expr"))
                .desired_width(W_EXPR),
        );
        let enter = expr.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let add = button(ui, &tr("variables.add"), ButtonKind::Primary).clicked();
        if name.changed() || expr.changed() {
            d.new_error = None;
        }
        if add || enter {
            actions.push(Action::Add);
        }
    });
    if let Some(msg) = &d.new_error {
        crate::a11y::label(ui, egui::RichText::new(msg).color(danger))
            .on_hover_text(tr("variables.err.hover"));
    }
}

/// Section « valeurs par taille » (US-09.7) : taille choisie, puis pour chaque variable
/// l'expression propre à la taille (vide = celle de base), sa valeur dans la taille, et un bouton
/// pour la retirer. Repliée par défaut ; la fenêtre ne calcule rien (valeurs lues du cœur).
fn draw_sizes(
    ui: &mut egui::Ui,
    d: &mut VariablesDialog,
    vars: &[Variable],
    sizes: &[(SizeId, String)],
    values: Option<&[(String, f64)]>,
    (danger, secondary): (egui::Color32, egui::Color32),
    actions: &mut Vec<Action>,
) {
    egui::CollapsingHeader::new(tr("variables.sizes.title"))
        .id_salt("var-sizes")
        .default_open(d.sizes_open)
        .show(ui, |ui| {
            let Some(selected) = d.size_sel else {
                crate::a11y::label(
                    ui,
                    egui::RichText::new(tr("variables.sizes.none")).color(secondary),
                );
                return;
            };
            ui.horizontal(|ui| {
                crate::a11y::label(ui, tr("variables.sizes.pick"));
                let current = sizes
                    .iter()
                    .find(|(id, _)| *id == selected)
                    .map(|(_, l)| l.clone())
                    .unwrap_or_default();
                egui::ComboBox::from_id_salt("var-size-pick")
                    .selected_text(current)
                    .show_ui_named(ui, |ui| {
                        for (id, label) in sizes {
                            ui.selectable_value(&mut d.size_sel, Some(*id), label);
                        }
                    });
            });
            crate::a11y::label(
                ui,
                egui::RichText::new(tr("variables.sizes.note")).color(secondary),
            );
            for (i, var) in vars.iter().enumerate() {
                let key = var.name.clone();
                let own = var.per_size.iter().any(|e| e.size == selected);
                ui.horizontal(|ui| {
                    cell(
                        ui,
                        W_NAME,
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            crate::a11y::label(ui, egui::RichText::new(&var.name).monospace());
                        },
                    );
                    let Some(buf) = d.size_rows.get_mut(i) else {
                        return;
                    };
                    let edit = crate::a11y::add_text(
                        ui,
                        egui::TextEdit::singleline(&mut buf.text)
                            .id_salt(("var-size-expr", &key, selected.0))
                            .font(egui::TextStyle::Monospace)
                            .hint_text(format!("{} {}", tr("variables.sizes.hint"), var.expr))
                            .desired_width(W_EXPR + W_UNIT - 8.0),
                    );
                    cell(
                        ui,
                        W_VALUE,
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            let value = values.and_then(|v| v.get(i)).map(|v| v.1);
                            let text =
                                value.map_or_else(|| "—".to_string(), |v| value_text(var.unit, v));
                            crate::a11y::label(ui, egui::RichText::new(text).monospace());
                        },
                    );
                    let clear = ui
                        .add_enabled(
                            own,
                            egui::Button::new(egui::RichText::new("×").size(16.0))
                                .min_size(egui::vec2(W_REMOVE, 22.0)),
                        )
                        .on_hover_text(tr("variables.sizes.clear"));
                    if edit.changed() {
                        d.size_errors.remove(&key);
                    }
                    if edit.lost_focus() && buf.text.trim() != buf.synced {
                        actions.push(Action::SetSize {
                            name: key.clone(),
                            expr: buf.text.clone(),
                        });
                    }
                    if clear.clicked() {
                        actions.push(Action::SetSize {
                            name: key.clone(),
                            expr: String::new(),
                        });
                    }
                });
                if let Some(msg) = d.size_errors.get(&key) {
                    crate::a11y::label(ui, egui::RichText::new(msg).color(danger))
                        .on_hover_text(tr("variables.err.hover"));
                }
            }
        });
}

/// Projet de capture `--etape taille-valeurs` (US-09.5c, US-09.7) : rectangle 200 × 120, gamme
/// XS S M L XL (base M), écarts sur les deux coins de droite, variables `ample` (40 ; XS 30, S 35,
/// L 46, XL 52) et `profond = ample / 2`, un point en direction dont la distance suit `ample`, un
/// cran à 60 mm déclaré proportionnel. Renvoie aussi le cran et la taille L.
pub(crate) fn demo_sizes_project(
) -> Result<(pf_core::Project, pf_core::NotchId, SizeId), pf_core::CoreError> {
    use pf_core::{NotchStyle, ParamKind, ParamTarget, PointDefinition, PointRef, SizeKind};
    use pf_geometry::{PointMm, Side};
    let mut p = pf_core::Project::new("Sizes values");
    let sheet = p.sheets[0].id;
    let rect = p.apply(Command::AddRectangle {
        sheet,
        corner: PointRef::New(PointMm::new(0.0, 0.0)),
        width_mm: 200.0,
        height_mm: 120.0,
    })?;
    p.apply(Command::AddSizeRange {
        name: "Women".into(),
        kind: SizeKind::Alphanumeric,
        labels: ["XS", "S", "M", "L", "XL"].map(String::from).to_vec(),
        base: 2,
    })?;
    let sizes: Vec<SizeId> = p.sizes[0].sizes.iter().map(|s| s.id).collect();
    let at = |p: &pf_core::Project, x: f64, y: f64| {
        let s = &p.sheets[0];
        s.points
            .iter()
            .find(|q| {
                s.position(q.id)
                    .is_ok_and(|v| (v.x - x).abs() < 1e-9 && (v.y - y).abs() < 1e-9)
            })
            .map(|q| q.id)
    };
    for (x, y) in [(200.0, 0.0), (200.0, 120.0)] {
        let point = at(&p, x, y).ok_or_else(|| pf_core::CoreError::Invalid("coin".into()))?;
        for (i, dx) in [(0, -20.0), (1, -10.0), (3, 10.0), (4, 20.0)] {
            p.apply(Command::SetPointGrading {
                sheet,
                point,
                size: sizes[i],
                dx_mm: dx,
                dy_mm: 0.0,
            })?;
        }
    }
    for (name, expr) in [("ample", "40"), ("profond", "ample / 2")] {
        p.apply(Command::AddVariable {
            name: name.into(),
            unit: VariableUnit::Mm,
            expr: expr.into(),
        })?;
    }
    let origin = at(&p, 0.0, 0.0).ok_or_else(|| pf_core::CoreError::Invalid("origine".into()))?;
    let c = p
        .apply(Command::AddPointAt {
            sheet,
            point: PointRef::Derived(PointDefinition::Polar {
                from: origin,
                distance_mm: 1.0,
                angle_deg: 45.0,
                reference: None,
            }),
        })?
        .points[0];
    p.apply(Command::SetParameterExpr {
        target: ParamTarget {
            sheet,
            point: c,
            param: ParamKind::Distance,
        },
        expr: Some("ample".into()),
    })?;
    for (i, expr) in [(0, "30"), (1, "35"), (3, "46"), (4, "52")] {
        p.apply(Command::SetVariableSizeExpr {
            name: "ample".into(),
            size: sizes[i],
            expr: expr.into(),
        })?;
    }
    let edge = *rect
        .entities
        .iter()
        .find(|e| {
            p.sheets[0]
                .path_length_mm(**e)
                .is_ok_and(|l| (l - 200.0).abs() < 1e-9)
        })
        .ok_or_else(|| pf_core::CoreError::Invalid("bord".into()))?;
    let notch = p
        .apply(Command::AddNotch {
            sheet,
            entity: edge,
            distance_mm: 60.0,
            side: Side::Left,
            length_mm: 8.0,
            style: NotchStyle::Single,
        })?
        .notches[0];
    p.apply(Command::SetNotchProportional {
        sheet,
        notch,
        proportional: true,
    })?;
    Ok((p, notch, sizes[3]))
}

/// Étape de capture `--etape taille-valeurs` : section « valeurs par taille » dépliée sur la
/// taille L ; le cran proportionnel est choisi (case cochée dans l'inspecteur).
pub(crate) fn demo_sizes_stage(app: &mut App, size: SizeId) {
    let mut d = VariablesDialog {
        sizes_open: true,
        size_sel: Some(size),
        ..VariablesDialog::default()
    };
    d.sync(&app.editor.project().variables.clone());
    app.variables = Some(d);
}

/// Étape de capture `--etape variables` : la table de la démonstration, une modification
/// invalide d'une ligne et un ajout invalide, **réellement refusés** par la même vérification que
/// la fenêtre (rien n'est modifié ; les erreurs s'affichent sous les lignes en cause).
pub(crate) fn demo_stage(app: &mut App) {
    let mut d = VariablesDialog::default();
    let vars = app.editor.project().variables.clone();
    d.sync(&vars);
    if let Some(r) = d.rows.get_mut(2) {
        r.expr = "poitrine/0".into();
    }
    app.variables_act(
        &mut d,
        Action::SetExpr {
            name: "largeur_dos".into(),
            expr: "poitrine/0".into(),
        },
    );
    d.new_name = "tour_cou".into();
    d.new_expr = "(poitrine+".into();
    app.variables_act(&mut d, Action::Add);
    app.variables = Some(d);
}

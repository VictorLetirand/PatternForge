//! US-03.1b — valeurs maintenues par une formule, dans l'inspecteur d'un point construit.
//!
//! Douze paramètres peuvent être liés (distance et angle d'un point en direction, dx et dy d'un
//! point décalé, distance sur un segment ou un tracé, bissectrice, compas, proportions). Le champ de formule accepte un **nombre** (valeur figée : `SetPointValues`, qui
//! délie un paramètre lié avec remarque) ou une **expression** (`SetParameterExpr`) qui peut citer
//! les variables. Une expression est vérifiée **avant** l'envoi (`pf_core::expr::evaluate`) :
//! invalide, rien n'est modifié et l'erreur (catégorie + position) s'affiche sous le champ.
//! L'état « maintenu » se lit par une icône **et** un texte, jamais par la couleur seule.

use crate::app::{App, TextBuf};
use crate::palette::pal;
use crate::variables::expr_error_text;
use eframe::egui::{self, RichText};
use pf_core::input::{format_number, parse_angle_deg, parse_fraction};
use pf_core::units::parse_length_display;
use pf_core::{Command, ParamKind, ParamTarget, PointId};
use pf_i18n::tr;
use std::collections::HashMap;

/// Icône de l'état « maintenu » (toujours accompagnée du texte).
const KEPT_ICON: &str = "ƒ";
/// Les clés des champs de formule d'un point : `(id << 8) | KEY_BASE + indice`.
const KEY_BASE: u64 = 0xC0;

/// Un paramètre affiché : nature, libellé, valeur courante.
pub(crate) struct ParamRow {
    pub kind: ParamKind,
    pub label: String,
    pub value: f64,
}

/// Les paramètres qui acceptent une formule, dans l'ordre de `SetPointValues`.
pub(crate) fn kinds_of(definition: &pf_core::PointDefinition) -> Vec<Option<ParamKind>> {
    use pf_core::PointDefinition::{
        AlongCurve, Bisector, Compass, Division, Offset, OnCurve, OnSegment, OnSegmentRatio, Polar,
    };
    match definition {
        Polar { .. } => vec![Some(ParamKind::Distance), Some(ParamKind::Angle)],
        Offset { .. } => vec![Some(ParamKind::Dx), Some(ParamKind::Dy)],
        AlongCurve { .. } => vec![Some(ParamKind::AlongDistance)],
        Bisector { .. } => vec![Some(ParamKind::BisectorDistance)],
        Compass { .. } => vec![Some(ParamKind::RadiusA), Some(ParamKind::RadiusB)],
        OnSegment { .. } => vec![Some(ParamKind::SegmentDistance)],
        OnSegmentRatio { .. } => vec![Some(ParamKind::SegmentRatio)],
        OnCurve { .. } => vec![Some(ParamKind::CurveRatio)],
        Division { .. } => vec![Some(ParamKind::DivisionRatio)],
        _ => Vec::new(),
    }
}

/// Une proportion se saisit et se lit en **fraction** de 0 à 1 (affichée aussi en %).
pub(crate) fn is_ratio(kind: ParamKind) -> bool {
    matches!(
        kind,
        ParamKind::SegmentRatio | ParamKind::CurveRatio | ParamKind::DivisionRatio
    )
}

/// Commande qui fige `value` pour la ligne `index` : celle que l'inspecteur utilise déjà pour ce
/// paramètre (une valeur modifiée directement délie la formule, avec remarque).
fn plain_command(
    sheet: pf_core::SheetId,
    point: PointId,
    rows: &[ParamRow],
    index: usize,
    value: f64,
) -> Command {
    match rows[index].kind {
        ParamKind::SegmentDistance => Command::SetPointDistance {
            sheet,
            point,
            distance_mm: value,
        },
        k if is_ratio(k) => Command::SetPointParameter {
            sheet,
            point,
            value,
        },
        _ => {
            let mut values: Vec<f64> = rows.iter().map(|r| r.value).collect();
            values[index] = value;
            Command::SetPointValues {
                sheet,
                point,
                values,
            }
        }
    }
}

/// Valeur calculée d'un paramètre lié, pour l'affichage en lecture seule.
pub(crate) fn value_text(kind: ParamKind, value: f64) -> String {
    if is_ratio(kind) {
        format!("{} %", format_number(value * 100.0, 2))
    } else if kind == ParamKind::Angle {
        App::kept_value_text(value, "°")
    } else {
        App::kept_value_text(value, "mm")
    }
}

/// Champ de texte à validation différée (perte du focus ou Entrée) avec un texte d'aide ; renvoie
/// `(texte validé, modifié à cette image)`.
fn edit_with_hint(
    ui: &mut egui::Ui,
    buf: &mut TextBuf,
    key: u64,
    current: &str,
    width: f32,
    hint: &str,
) -> (Option<String>, bool) {
    let mut text = match buf {
        Some((k, t)) if *k == key => t.clone(),
        _ => current.to_string(),
    };
    let resp = crate::a11y::add_text(
        ui,
        egui::TextEdit::singleline(&mut text)
            .id_salt(("formula", key))
            .font(egui::TextStyle::Monospace)
            .hint_text(hint)
            .desired_width(width),
    );
    if resp.has_focus() || resp.changed() {
        *buf = Some((key, text.clone()));
    }
    let mut submitted = None;
    if resp.lost_focus() {
        *buf = None;
        if text != current {
            submitted = Some(text);
        }
    }
    (submitted, resp.changed())
}

impl App {
    /// Lignes de formule d'un point : une par paramètre retenu. Aucune section : elles prolongent
    /// le bloc des paramètres de l'inspecteur.
    pub(crate) fn formula_rows(&mut self, ui: &mut egui::Ui, id: PointId, rows: &[ParamRow]) {
        let danger = pf_ui::theme::tokens(ui.ctx()).danger;
        let muted = pal().muted;
        let mut submit: Option<(usize, String)> = None;
        let mut unlink: Option<ParamKind> = None;
        for (i, row) in rows.iter().enumerate() {
            let key = (id.0 << 8) | (KEY_BASE + i as u64);
            let formula = self
                .editor
                .sheet()
                .formula(id, row.kind)
                .map(|f| f.expr.clone());
            ui.horizontal(|ui| match &formula {
                Some(_) => {
                    crate::a11y::label(
                        ui,
                        RichText::new(format!("{KEPT_ICON} {}", tr("inspector.formula.kept")))
                            .strong()
                            .size(15.0),
                    );
                    crate::a11y::label(ui, RichText::new(&row.label).color(muted));
                }
                None => {
                    crate::a11y::label(
                        ui,
                        RichText::new(format!("{} · {}", tr("inspector.formula.label"), row.label))
                            .color(muted),
                    );
                }
            });
            ui.horizontal(|ui| {
                let current = formula.clone().unwrap_or_default();
                let (done, edited) = edit_with_hint(
                    ui,
                    &mut self.text_buf,
                    key,
                    &current,
                    150.0,
                    &tr(if is_ratio(row.kind) {
                        "inspector.formula.hint_ratio"
                    } else {
                        "inspector.formula.hint"
                    }),
                );
                if edited {
                    self.formula_error = None;
                }
                if let Some(text) = done {
                    submit = Some((i, text));
                }
                if formula.is_some()
                    && ui
                        .button(tr("inspector.formula.unlink"))
                        .on_hover_text(tr("inspector.formula.unlink.hover"))
                        .clicked()
                {
                    unlink = Some(row.kind);
                }
            });
            if let Some((k, msg)) = &self.formula_error {
                if *k == key {
                    crate::a11y::label(ui, RichText::new(msg).color(danger))
                        .on_hover_text(tr("variables.err.hover"));
                }
            }
        }
        if let Some(kind) = unlink {
            self.formula_error = None;
            self.editor.exec(Command::SetParameterExpr {
                target: ParamTarget {
                    sheet: self.editor.sheet,
                    point: id,
                    param: kind,
                },
                expr: None,
            });
        }
        if let Some((i, text)) = submit {
            self.formula_submit(id, rows, i, &text);
        }
    }

    /// Texte validé d'un champ de formule : nombre simple (valeur figée) ou expression.
    fn formula_submit(&mut self, id: PointId, rows: &[ParamRow], index: usize, text: &str) {
        let text = text.trim();
        let key = (id.0 << 8) | (KEY_BASE + index as u64);
        self.formula_error = None;
        if text.is_empty() {
            return;
        }
        let row = &rows[index];
        let plain = if row.kind == ParamKind::Angle {
            parse_angle_deg(text)
        } else if is_ratio(row.kind) {
            parse_fraction(text)
        } else {
            parse_length_display(text)
        };
        if let Ok(value) = plain {
            let cmd = plain_command(self.editor.sheet, id, rows, index, value);
            if self.editor.exec(cmd).is_none() {
                self.formula_error = Some((key, self.editor.status.text.clone()));
            }
            return;
        }
        // Expression : vérification pure d'abord, commande ensuite.
        let values: HashMap<String, f64> = self
            .editor
            .project()
            .variable_values()
            .map(|v| v.into_iter().collect())
            .unwrap_or_default();
        // Une saisie refusée reste dans le champ : on la corrige, on ne la retape pas.
        let keep_text = |app: &mut App| app.text_buf = Some((key, text.to_string()));
        if let Err(e) = pf_core::expr::evaluate(text, &|n| values.get(n).copied()) {
            self.formula_error = Some((key, expr_error_text(&e)));
            keep_text(self);
            return;
        }
        let cmd = Command::SetParameterExpr {
            target: ParamTarget {
                sheet: self.editor.sheet,
                point: id,
                param: row.kind,
            },
            expr: Some(text.to_string()),
        };
        if self.editor.exec(cmd).is_none() {
            self.formula_error = Some((key, self.editor.status.text.clone()));
            keep_text(self);
        }
    }

    /// Texte de la formule liée à ce paramètre, s'il y en a une.
    pub(crate) fn kept_formula_text(&self, id: PointId, kind: ParamKind) -> Option<String> {
        self.editor
            .sheet()
            .formula(id, kind)
            .map(|f| f.expr.clone())
    }

    /// Valeur calculée d'un paramètre lié, en lecture seule, à côté de `= formule`.
    pub(crate) fn kept_value_line(
        &self,
        ui: &mut egui::Ui,
        kind: ParamKind,
        value: f64,
        expr: &str,
    ) {
        ui.horizontal(|ui| {
            crate::a11y::label(ui, RichText::new(value_text(kind, value)).monospace());
            crate::a11y::label(
                ui,
                RichText::new(format!("= {expr}"))
                    .monospace()
                    .color(pal().muted),
            );
        });
    }

    /// Étape de capture : saisie **réelle** de `text` dans le champ du paramètre `index` du point
    /// (la même vérification et la même commande que l'inspecteur).
    pub(crate) fn demo_formula_submit(&mut self, id: PointId, index: usize, text: &str) {
        let sheet = self.editor.sheet();
        let Ok(point) = sheet.point(id) else { return };
        let rows: Vec<ParamRow> = kinds_of(&point.definition)
            .into_iter()
            .flatten()
            .map(|kind| ParamRow {
                kind,
                label: String::new(),
                value: sheet.parameter_value(id, kind).unwrap_or(0.0),
            })
            .collect();
        if index < rows.len() {
            self.formula_submit(id, &rows, index, text);
        }
    }

    /// Texte de la valeur calculée d'un paramètre maintenu : « 70 mm » (virgule selon la langue).
    pub(crate) fn kept_value_text(value: f64, unit: &str) -> String {
        if unit == "mm" {
            pf_core::units::len_text(value, 3)
        } else {
            format!("{} {unit}", format_number(value, 3))
        }
    }
}

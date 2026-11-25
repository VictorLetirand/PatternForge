//! US-03.1b — valeurs numériques **maintenues** par une formule.
//!
//! Un paramètre numérique d'un point construit peut être lié à une expression (qui cite les
//! variables d'US-03.1a). `Sheet.formulas` ne stocke que le **texte** ; la valeur évaluée reste
//! écrite dans la définition du point (`distance_mm`, `angle_deg`, `dx_mm`, `dy_mm`) : un lecteur
//! qui ignore les formules voit la bonne valeur, et délier garde cette valeur en nombre figé.
//!
//! Paramètres retenus (12) : `Distance` et `Angle` d'un point `Polar` (direction), `Dx` et `Dy`
//! d'un point `Offset` (décalage), `AlongDistance` (`AlongCurve`), `BisectorDistance`
//! (`Bisector`), `RadiusA` et `RadiusB` (`Compass`), `SegmentDistance` (`OnSegment`),
//! `SegmentRatio` (`OnSegmentRatio`), `CurveRatio` (`OnCurve`) et `DivisionRatio` (`Division`) ;
//! les proportions sont des **fractions** de 0 à 1 (US-03.1 CA 3). Ils ont déjà une commande
//! d'édition (`SetPointValues`, `SetPointDistance`, `SetPointParameter`) et une
//! régénération (la position d'un point construit est lue de sa définition, parents avant enfants).
//!
//! Ordre d'évaluation : une formule ne cite que des **variables**, et les variables ne citent
//! jamais la géométrie : aucun cycle n'est possible entre formules et points. Toutes les valeurs
//! sont donc calculées d'abord (variables dans l'ordre de leurs dépendances), puis les positions
//! suivent par le graphe existant des points construits.
//!
//! Atomicité : après chaque commande (`Project::sync_formulas`, dans `Project::apply`), les
//! formules sont réévaluées dans la **même étape** ; une formule devenue non évaluable refuse la
//! commande tout entière (aucune trace). Une valeur modifiée directement (autre commande que celle
//! de la formule) **délie** le paramètre, avec remarque : jamais de formule qui ment.

use crate::command::Outcome;
use crate::error::CoreError;
use crate::expr;
use crate::model::{PointDefinition, PointId, Project, Sheet, SheetId};
use crate::variable::evaluate_variables;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Écart toléré entre la valeur écrite et la valeur de la formule à l'ouverture.
pub const FORMULA_TOLERANCE: f64 = 1e-9;

/// Paramètre numérique d'un point construit pouvant porter une formule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParamKind {
    /// Distance d'un point `Polar` (direction).
    Distance,
    /// Angle d'un point `Polar`, en degrés.
    Angle,
    /// Décalage en X d'un point `Offset`.
    Dx,
    /// Décalage en Y d'un point `Offset`.
    Dy,
    /// Distance le long du tracé d'un point `AlongCurve` (US-03.1 CA 3).
    AlongDistance,
    /// Distance au sommet d'un point `Bisector` (US-03.1 CA 3).
    BisectorDistance,
    /// Rayon du premier cercle d'un point `Compass` (US-03.1 CA 3).
    RadiusA,
    /// Rayon du second cercle d'un point `Compass` (US-03.1 CA 3).
    RadiusB,
    /// Distance d'un point `OnSegment` depuis l'extrémité choisie, en mm (US-03.1 CA 3, reste).
    SegmentDistance,
    /// Proportion d'un point `OnSegmentRatio`, **fraction** de 0 à 1 (0,25 = 25 %).
    SegmentRatio,
    /// Paramètre `t` d'un point `OnCurve`, **fraction** de 0 à 1.
    CurveRatio,
    /// Fraction de longueur d'arc d'un point `Division`, strictement entre 0 et 1.
    DivisionRatio,
}

impl ParamKind {
    /// Nom neutre (identique en français et en anglais) pour les messages.
    pub fn name(self) -> &'static str {
        match self {
            ParamKind::Distance => "distance",
            ParamKind::Angle => "angle",
            ParamKind::Dx => "dx",
            ParamKind::Dy => "dy",
            ParamKind::AlongDistance => "along_distance",
            ParamKind::BisectorDistance => "bisector_distance",
            ParamKind::RadiusA => "radius_a",
            ParamKind::RadiusB => "radius_b",
            ParamKind::SegmentDistance => "segment_distance",
            ParamKind::SegmentRatio => "segment_ratio",
            ParamKind::CurveRatio => "curve_ratio",
            ParamKind::DivisionRatio => "division_ratio",
        }
    }
}

/// Paramètre visé : feuille, point et paramètre.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParamTarget {
    pub sheet: SheetId,
    pub point: PointId,
    pub param: ParamKind,
}

/// Formule liée à un paramètre (`Sheet.formulas`) : seul le texte est stocké.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamFormula {
    pub point: PointId,
    pub param: ParamKind,
    pub expr: String,
}

/// Formule d'un paramètre, avant que le point existe (`Command::AddPointWithFormulas`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParamExpr {
    pub param: ParamKind,
    pub expr: String,
}

impl PointDefinition {
    /// Même définition avec la valeur du paramètre remplacée ; `None` si elle n'a pas ce paramètre.
    pub fn with_param(self, param: ParamKind, value: f64) -> Option<PointDefinition> {
        let mut out = self;
        let ok = match (&mut out, param) {
            (PointDefinition::Polar { distance_mm, .. }, ParamKind::Distance)
            | (PointDefinition::AlongCurve { distance_mm, .. }, ParamKind::AlongDistance)
            | (PointDefinition::Bisector { distance_mm, .. }, ParamKind::BisectorDistance)
            | (PointDefinition::OnSegment { distance_mm, .. }, ParamKind::SegmentDistance) => {
                *distance_mm = value;
                true
            }
            (PointDefinition::Polar { angle_deg, .. }, ParamKind::Angle) => {
                *angle_deg = value;
                true
            }
            (PointDefinition::Offset { dx_mm, .. }, ParamKind::Dx) => {
                *dx_mm = value;
                true
            }
            (PointDefinition::Offset { dy_mm, .. }, ParamKind::Dy) => {
                *dy_mm = value;
                true
            }
            (PointDefinition::Compass { radius_a_mm, .. }, ParamKind::RadiusA) => {
                *radius_a_mm = value;
                true
            }
            (PointDefinition::Compass { radius_b_mm, .. }, ParamKind::RadiusB) => {
                *radius_b_mm = value;
                true
            }
            (PointDefinition::OnSegmentRatio { ratio, .. }, ParamKind::SegmentRatio)
            | (PointDefinition::Division { ratio, .. }, ParamKind::DivisionRatio) => {
                *ratio = value;
                true
            }
            (PointDefinition::OnCurve { t, .. }, ParamKind::CurveRatio) => {
                *t = value;
                true
            }
            _ => false,
        };
        ok.then_some(out)
    }
}

impl Sheet {
    /// Valeur courante du paramètre, ou `None` si le point n'existe pas ou n'a pas ce paramètre.
    pub fn parameter_value(&self, point: PointId, param: ParamKind) -> Option<f64> {
        match (self.point(point).ok()?.definition, param) {
            (PointDefinition::Polar { distance_mm, .. }, ParamKind::Distance) => Some(distance_mm),
            (PointDefinition::Polar { angle_deg, .. }, ParamKind::Angle) => Some(angle_deg),
            (PointDefinition::Offset { dx_mm, .. }, ParamKind::Dx) => Some(dx_mm),
            (PointDefinition::Offset { dy_mm, .. }, ParamKind::Dy) => Some(dy_mm),
            (PointDefinition::AlongCurve { distance_mm, .. }, ParamKind::AlongDistance) => {
                Some(distance_mm)
            }
            (PointDefinition::Bisector { distance_mm, .. }, ParamKind::BisectorDistance) => {
                Some(distance_mm)
            }
            (PointDefinition::Compass { radius_a_mm, .. }, ParamKind::RadiusA) => Some(radius_a_mm),
            (PointDefinition::Compass { radius_b_mm, .. }, ParamKind::RadiusB) => Some(radius_b_mm),
            (PointDefinition::OnSegment { distance_mm, .. }, ParamKind::SegmentDistance) => {
                Some(distance_mm)
            }
            (PointDefinition::OnSegmentRatio { ratio, .. }, ParamKind::SegmentRatio)
            | (PointDefinition::Division { ratio, .. }, ParamKind::DivisionRatio) => Some(ratio),
            (PointDefinition::OnCurve { t, .. }, ParamKind::CurveRatio) => Some(t),
            _ => None,
        }
    }

    /// Formule liée à ce paramètre, s'il y en a une.
    pub fn formula(&self, point: PointId, param: ParamKind) -> Option<&ParamFormula> {
        self.formulas
            .iter()
            .find(|f| f.point == point && f.param == param)
    }

    pub(crate) fn write_parameter(&mut self, point: PointId, param: ParamKind, value: f64) {
        let Some(p) = self.points.iter_mut().find(|p| p.id == point) else {
            return;
        };
        match (&mut p.definition, param) {
            (PointDefinition::Polar { distance_mm, .. }, ParamKind::Distance) => {
                *distance_mm = value
            }
            (PointDefinition::Polar { angle_deg, .. }, ParamKind::Angle) => *angle_deg = value,
            (PointDefinition::Offset { dx_mm, .. }, ParamKind::Dx) => *dx_mm = value,
            (PointDefinition::Offset { dy_mm, .. }, ParamKind::Dy) => *dy_mm = value,
            (PointDefinition::AlongCurve { distance_mm, .. }, ParamKind::AlongDistance)
            | (PointDefinition::Bisector { distance_mm, .. }, ParamKind::BisectorDistance) => {
                *distance_mm = value
            }
            (PointDefinition::Compass { radius_a_mm, .. }, ParamKind::RadiusA) => {
                *radius_a_mm = value
            }
            (PointDefinition::Compass { radius_b_mm, .. }, ParamKind::RadiusB) => {
                *radius_b_mm = value
            }
            (PointDefinition::OnSegment { distance_mm, .. }, ParamKind::SegmentDistance) => {
                *distance_mm = value
            }
            (PointDefinition::OnSegmentRatio { ratio, .. }, ParamKind::SegmentRatio)
            | (PointDefinition::Division { ratio, .. }, ParamKind::DivisionRatio) => *ratio = value,
            (PointDefinition::OnCurve { t, .. }, ParamKind::CurveRatio) => *t = value,
            _ => {}
        }
    }

    /// « point C (dx) » : désignation d'un paramètre dans les messages.
    pub fn parameter_label(&self, point: PointId, param: ParamKind) -> String {
        format!("point {} ({})", self.label(point), param.name())
    }
}

pub(crate) fn eval_text(values: &HashMap<String, f64>, text: &str) -> Result<f64, expr::ExprError> {
    expr::evaluate(text, &|n| values.get(n).copied())
}

impl Project {
    /// Valeurs des variables, par nom (échoue si la table est invalide).
    pub(crate) fn variable_value_map(&self) -> Result<HashMap<String, f64>, CoreError> {
        Ok(evaluate_variables(&self.variables)?.into_iter().collect())
    }

    /// Formule liée à un paramètre (lecture seule).
    pub fn parameter_formula(&self, target: ParamTarget) -> Option<&ParamFormula> {
        self.sheet(target.sheet)
            .ok()?
            .formula(target.point, target.param)
    }

    /// Paramètres dont la formule cite la variable `name` (désignations pour les messages).
    pub fn formula_users(&self, name: &str) -> Vec<String> {
        let mut out = Vec::new();
        for s in &self.sheets {
            for f in &s.formulas {
                let uses = expr::parse(&f.expr)
                    .map(|e| e.references().iter().any(|r| r.name == name))
                    .unwrap_or(false);
                if uses {
                    out.push(s.parameter_label(f.point, f.param));
                }
            }
        }
        out
    }

    /// `SetParameterExpr` : lie (`Some`) ou délie (`None`) un paramètre. Délier garde la valeur
    /// courante ; sans formule liée, délier ne fait rien (aucune étape).
    pub(crate) fn set_parameter_expr(
        &mut self,
        target: ParamTarget,
        expr_text: Option<String>,
    ) -> Result<(), CoreError> {
        let sheet = self.sheet(target.sheet)?;
        sheet.point(target.point)?;
        let label = sheet.parameter_label(target.point, target.param);
        if sheet.parameter_value(target.point, target.param).is_none() {
            return Err(CoreError::Invalid(format!(
                "{label} : ce point n'a pas ce paramètre (peuvent être liés : distance et angle \
                 d'un point en direction, dx et dy d'un point décalé, distance d'un point sur un \
                 segment, sur un tracé ou sur une bissectrice, proportion d'un point sur un \
                 segment, sur une courbe ou de division, rayons d'un point au compas)"
            )));
        }
        let Some(text) = expr_text else {
            self.sheet_mut(target.sheet)?
                .formulas
                .retain(|f| !(f.point == target.point && f.param == target.param));
            return Ok(());
        };
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err(CoreError::Invalid(format!(
                "{label} : formule vide (saisissez un nombre ou une expression)"
            )));
        }
        let values = self.variable_value_map()?;
        let value = eval_text(&values, &text)
            .map_err(|e| CoreError::Invalid(format!("{label} : formule « {text} » : {e}")))?;
        let sheet = self.sheet_mut(target.sheet)?;
        sheet.write_parameter(target.point, target.param, value);
        match sheet
            .formulas
            .iter_mut()
            .find(|f| f.point == target.point && f.param == target.param)
        {
            Some(f) => f.expr = text,
            None => sheet.formulas.push(ParamFormula {
                point: target.point,
                param: target.param,
                expr: text,
            }),
        }
        Ok(())
    }

    /// `AddPointWithFormulas` : évalue les formules, crée le point avec ces valeurs puis pose les
    /// liens. Tout ou rien : l'appelant (`apply`) travaille sur une copie.
    pub(crate) fn add_point_with_formulas(
        &mut self,
        sheet: SheetId,
        definition: PointDefinition,
        formulas: Vec<ParamExpr>,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        self.sheet(sheet)?;
        let values = self.variable_value_map()?;
        let mut definition = definition;
        let mut seen = HashSet::new();
        let mut bound = Vec::with_capacity(formulas.len());
        for f in formulas {
            let text = f.expr.trim().to_string();
            let name = f.param.name();
            if !seen.insert(f.param) {
                return Err(CoreError::Invalid(format!(
                    "paramètre {name} : deux formules pour le même paramètre"
                )));
            }
            if text.is_empty() {
                return Err(CoreError::Invalid(format!(
                    "paramètre {name} : formule vide (saisissez un nombre ou une expression)"
                )));
            }
            let value = eval_text(&values, &text).map_err(|e| {
                CoreError::Invalid(format!("paramètre {name} : formule « {text} » : {e}"))
            })?;
            definition = definition.with_param(f.param, value).ok_or_else(|| {
                CoreError::Invalid(format!("ce point n'a pas le paramètre {name}"))
            })?;
            bound.push((f.param, text));
        }
        let before = out.points.len();
        let id = self.resolve_ref(sheet, crate::command::PointRef::Derived(definition), out)?;
        if out.points.len() == before {
            return Err(CoreError::Invalid(
                "un point existe déjà à cet endroit : la formule n'a rien à maintenir (déplacez                  la saisie ou liez le paramètre du point existant)"
                    .into(),
            ));
        }
        let s = self.sheet_mut(sheet)?;
        for (param, expr) in bound {
            s.formulas.push(ParamFormula {
                point: id,
                param,
                expr,
            });
        }
        Ok(())
    }

    /// Réévalue toutes les formules liées après une commande (dans la même étape). `old` est le
    /// projet d'avant la commande. Renvoie la désignation des paramètres dont la valeur a changé.
    ///
    /// - point supprimé ou redéfini : formule retirée, avec remarque ;
    /// - valeur écrite autrement que par la formule (commande directe) : paramètre délié, avec
    ///   remarque ;
    /// - formule non évaluable : **refus** (erreur qui nomme le paramètre).
    ///
    /// Sans effet quand la table de variables est invalide : `validate` la refuse ensuite.
    pub(crate) fn sync_formulas(
        &mut self,
        old: &Project,
        out: &mut Outcome,
    ) -> Result<Vec<String>, CoreError> {
        if self.sheets.iter().all(|s| s.formulas.is_empty()) {
            return Ok(Vec::new());
        }
        let Ok(values) = self.variable_value_map() else {
            return Ok(Vec::new());
        };
        let mut moved = Vec::new();
        for si in 0..self.sheets.len() {
            let formulas = std::mem::take(&mut self.sheets[si].formulas);
            let old_sheet = old.sheets.iter().find(|s| s.id == self.sheets[si].id);
            let mut kept = Vec::with_capacity(formulas.len());
            for f in formulas {
                let sheet = &mut self.sheets[si];
                let label = format!("point {} ({})", sheet.label(f.point), f.param.name());
                let Some(current) = sheet.parameter_value(f.point, f.param) else {
                    out.notes.push(format!(
                        "La formule « {} » du paramètre {} est retirée : le point a été \
                         supprimé ou redéfini.",
                        f.expr,
                        f.param.name()
                    ));
                    continue;
                };
                let before = old_sheet.and_then(|s| {
                    s.formula(f.point, f.param)
                        .filter(|of| of.expr == f.expr)
                        .and_then(|_| s.parameter_value(f.point, f.param))
                });
                if before.is_some_and(|b| b != current) {
                    out.notes.push(format!(
                        "{label} : la valeur a été modifiée directement, la formule « {} » est \
                         déliée (la valeur {} est conservée).",
                        f.expr,
                        crate::input::format_number(current, 3)
                    ));
                    continue;
                }
                let value = eval_text(&values, &f.expr).map_err(|e| {
                    CoreError::Invalid(format!(
                        "{label} : la formule « {} » ne peut plus être calculée : {e}",
                        f.expr
                    ))
                })?;
                if value != current {
                    sheet.write_parameter(f.point, f.param, value);
                    moved.push(label);
                }
                kept.push(f);
            }
            self.sheets[si].formulas = kept;
        }
        Ok(moved)
    }

    /// Validation à l'ouverture et après chaque commande : chaque formule vise un paramètre
    /// existant, s'évalue (variables existantes) et donne la valeur écrite à 1e-9 près.
    pub(crate) fn validate_formulas(&self) -> Result<(), CoreError> {
        match self.formula_problems()?.into_iter().next() {
            Some((_, _, e)) => Err(e),
            None => Ok(()),
        }
    }

    /// Tous les problèmes des formules (même contrôle que `validate_formulas`, qui garde le
    /// premier ; le diagnostic US-03.5 les garde tous) : (feuille, rang dans la liste des
    /// formules de la feuille, erreur localisée),
    /// au plus un par formule. `Err` : la table de variables est invalide (aucune formule ne
    /// peut être évaluée).
    pub(crate) fn formula_problems(&self) -> Result<Vec<(SheetId, usize, CoreError)>, CoreError> {
        if self.sheets.iter().all(|s| s.formulas.is_empty()) {
            return Ok(Vec::new());
        }
        let values = self.variable_value_map()?;
        let mut out = Vec::new();
        for sheet in &self.sheets {
            let mut seen = HashSet::new();
            for (index, f) in sheet.formulas.iter().enumerate() {
                let label = format!(
                    "feuille « {} », {}",
                    sheet.name,
                    sheet.parameter_label(f.point, f.param)
                );
                let problem = (|| {
                    if !seen.insert((f.point, f.param)) {
                        return Some(CoreError::Invalid(format!(
                            "{label} : deux formules pour le même paramètre"
                        )));
                    }
                    let Some(current) = sheet.parameter_value(f.point, f.param) else {
                        return Some(CoreError::Invalid(format!(
                            "{label} : formule sur un point ou un paramètre inexistant"
                        )));
                    };
                    let value = match eval_text(&values, &f.expr) {
                        Ok(v) => v,
                        Err(e) => {
                            return Some(CoreError::Invalid(format!(
                                "{label} : formule « {} » : {e}",
                                f.expr
                            )))
                        }
                    };
                    ((value - current).abs() > FORMULA_TOLERANCE).then(|| {
                        CoreError::Invalid(format!(
                            "{label} : la valeur écrite ({}) ne correspond pas à la formule « {} » \
                             ({})",
                            crate::input::format_number(current, 6),
                            f.expr,
                            crate::input::format_number(value, 6)
                        ))
                    })
                })();
                if let Some(e) = problem {
                    out.push((sheet.id, index, e));
                }
            }
        }
        Ok(out)
    }

    /// Reprend dans `sheet` (copie importée) les formules qui s'évaluent dans **ce** projet avec
    /// la valeur écrite ; les autres sont retirées avec remarque.
    pub(crate) fn retain_adoptable_formulas(&self, sheet: &mut Sheet, out: &mut Outcome) {
        if sheet.formulas.is_empty() {
            return;
        }
        let values = self.variable_value_map().unwrap_or_default();
        let formulas = std::mem::take(&mut sheet.formulas);
        let mut dropped = 0;
        for f in formulas {
            let ok = sheet
                .parameter_value(f.point, f.param)
                .zip(eval_text(&values, &f.expr).ok())
                .is_some_and(|(cur, v)| (cur - v).abs() <= FORMULA_TOLERANCE);
            if ok {
                sheet.formulas.push(f);
            } else {
                dropped += 1;
            }
        }
        if dropped > 0 {
            out.notes.push(format!(
                "{dropped} formule(s) de la feuille « {} » ne sont pas reprises : leurs variables \
                 manquent ou ont une autre valeur dans ce projet (les valeurs sont conservées en \
                 nombres figés).",
                sheet.name
            ));
        }
    }
}

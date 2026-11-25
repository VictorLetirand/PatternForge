//! US-09.5c et US-09.7 — cran proportionnel et valeurs de variables par taille (décision 335).
//!
//! Rien de ce module n'est stocké au-delà des déclarations (`Notch.proportional`,
//! `Variable.per_size`) : les valeurs par taille et les distances proportionnelles sont
//! **calculées à la lecture**, dans `Project::graded_project` (donc pour `derived_for_size`, les
//! mesures, le nid, les exports et le contrôle de crans par taille).
//!
//! Ordre d'application dans une taille `S` (testé, décrit dans le contrat d'interface) :
//! 1. la table de variables de `S` (chaque variable prend son expression `per_size` de `S`, sinon
//!    son expression de base) est évaluée, et chaque **formule liée** (US-03.1b) réécrit le
//!    paramètre de son point (distance, angle, dx, dy) avec la valeur de `S` ;
//! 2. les écarts de gradation (US-09.2a) déplacent les **points libres** ;
//! 3. les points construits sont recalculés depuis les points libres déplacés et les paramètres
//!    de `S` : les deux mécanismes **s'ajoutent**, sans se recouvrir (un écart ne vise qu'un point
//!    libre, une formule qu'un point construit) ;
//! 4. les crans proportionnels prennent `distance = fraction × longueur du tracé dans S`, où la
//!    fraction est `distance écrite / longueur du tracé dans la taille de base` ;
//! 5. pièces dérivées et surpiqûres liées sont rejouées.
//!
//! Conventions (à faire valider par une modéliste) : un cran proportionnel garde sa fraction de
//! la longueur de son tracé ; une valeur par taille n'existe pas pour la taille de base (sa valeur
//! est l'expression de la variable) ; changer la base d'une gamme qui porte des valeurs par taille
//! est refusé (les valeurs des autres tailles seraient à ré-exprimer).

use crate::command::Outcome;
use crate::error::CoreError;
use crate::expr;
use crate::model::{NotchId, Project, SheetId, SizeId};
use crate::notch_check::NotchCheckReading;
use crate::variable::{evaluate_variables, Variable, VariableSizeExpr};
use std::collections::{BTreeSet, HashMap};

/// Libellé d'une taille pour les messages (identifiant si la taille est inconnue).
fn label_of(p: &Project, size: SizeId) -> String {
    p.find_size(size)
        .map(|(r, i)| r.sizes[i].label.clone())
        .unwrap_or_else(|| format!("id {}", size.0))
}

impl Project {
    /// Table des variables **dans la taille** `size` : chaque expression est celle de la taille
    /// quand elle existe, celle de base sinon (`None` = base). Lecture pure.
    pub fn variable_table_for_size(&self, size: Option<SizeId>) -> Vec<Variable> {
        self.variables
            .iter()
            .map(|v| Variable {
                expr: size
                    .and_then(|s| v.per_size.iter().find(|e| e.size == s))
                    .map_or_else(|| v.expr.clone(), |e| e.expr.clone()),
                per_size: Vec::new(),
                ..v.clone()
            })
            .collect()
    }

    /// Valeurs des variables dans la taille `size` (`None` = base), dans l'ordre de la table.
    /// Lecture pure ; refuse une taille inconnue et une table qui ne s'évalue pas dans la taille.
    pub fn variable_values_for_size(
        &self,
        size: Option<SizeId>,
    ) -> Result<Vec<(String, f64)>, CoreError> {
        if let Some(id) = size {
            if self.find_size(id).is_none() {
                return Err(crate::size::SizeError::UnknownSize(id).into());
            }
        }
        evaluate_variables(&self.variable_table_for_size(size)).map_err(|e| match size {
            Some(id) => CoreError::Invalid(format!(
                "valeurs par taille (taille « {} ») : {e}",
                label_of(self, id)
            )),
            None => e.into(),
        })
    }

    /// Expression propre à la taille `size` pour la variable `name`, s'il y en a une.
    pub fn variable_size_expr(&self, name: &str, size: SizeId) -> Option<&str> {
        self.variables
            .iter()
            .find(|v| v.name == name)?
            .per_size
            .iter()
            .find(|e| e.size == size)
            .map(|e| e.expr.as_str())
    }

    /// Contrôle des crans de chaque couple apparié **dans la taille** (`None` = base) : le même
    /// contrôle que `notch_check_report`, sur le projet évalué (crans proportionnels à la longueur
    /// du tracé de la taille).
    pub fn notch_check_for_size(
        &self,
        size: Option<SizeId>,
    ) -> Result<Vec<NotchCheckReading>, CoreError> {
        Ok(self.graded_project(size)?.notch_check_report())
    }

    /// Contrôle d'une taille : toute la table substituée s'évalue, et chaque formule liée aussi
    /// (un paramètre ne peut pas devenir inévaluable dans une taille). Message avec le libellé.
    pub(crate) fn check_size_table(&self, size: SizeId) -> Result<(), CoreError> {
        let label = label_of(self, size);
        let values: HashMap<String, f64> =
            evaluate_variables(&self.variable_table_for_size(Some(size)))
                .map_err(|e| {
                    CoreError::Invalid(format!("valeurs par taille (taille « {label} ») : {e}"))
                })?
                .into_iter()
                .collect();
        for s in &self.sheets {
            for f in &s.formulas {
                expr::evaluate(&f.expr, &|n| values.get(n).copied()).map_err(|e| {
                    CoreError::Invalid(format!(
                        "valeurs par taille (taille « {label} ») : feuille « {} », {} : \
                         formule « {} » : {e}",
                        s.name,
                        s.parameter_label(f.point, f.param),
                        f.expr
                    ))
                })?;
            }
        }
        // Les valeurs de la taille ne doivent créer aucune erreur de géométrie (une distance
        // polaire nulle ou négative, par exemple) que la taille n'a pas sans elles.
        let (with, first) = self.size_issues(size);
        let mut bare = self.clone();
        bare.variables.iter_mut().for_each(|v| v.per_size.clear());
        if with > bare.size_issues(size).0 {
            return Err(CoreError::Invalid(format!(
                "valeurs par taille (taille « {label} ») : la géométrie de la taille devient \
                 impossible : {}",
                first.unwrap_or_default()
            )));
        }
        Ok(())
    }

    /// Nombre d'objets qui ne se calculent pas dans la taille (toutes feuilles) et le premier
    /// message. Lecture pure.
    fn size_issues(&self, size: SizeId) -> (usize, Option<String>) {
        let Ok(g) = self.graded_project(Some(size)) else {
            return (0, None);
        };
        let all: Vec<String> = g
            .sheets
            .iter()
            .flat_map(|s| s.evaluation_issues())
            .collect();
        (all.len(), all.into_iter().next())
    }

    /// `SetVariableSizeExpr` : pose ou remplace l'expression d'une variable pour une taille.
    pub(crate) fn set_variable_size_expr(
        &mut self,
        name: &str,
        size: SizeId,
        expr_text: String,
    ) -> Result<(), CoreError> {
        let i = self.variable_index(name)?;
        let (range, k) = self
            .find_size(size)
            .ok_or(crate::size::SizeError::UnknownSize(size))?;
        if range.base == size {
            return Err(CoreError::Invalid(format!(
                "la taille « {} » est la taille de base : sa valeur est l'expression de la \
                 variable « {name} » (une valeur par taille ne vise qu'une autre taille)",
                range.sizes[k].label
            )));
        }
        let text = expr_text.trim().to_string();
        if text.is_empty() {
            return Err(CoreError::Invalid(format!(
                "variable « {name} » : expression vide (pour revenir à l'expression de base, \
                 retirez la valeur de la taille)"
            )));
        }
        let entries = &mut self.variables[i].per_size;
        match entries.iter_mut().find(|e| e.size == size) {
            Some(e) => e.expr = text,
            None => {
                entries.push(VariableSizeExpr { size, expr: text });
                entries.sort_by_key(|e| e.size);
            }
        }
        self.check_size_table(size)
    }

    /// `ClearVariableSizeExpr` : la taille reprend l'expression de base ; sans effet (aucune
    /// étape) quand elle n'avait pas de valeur propre.
    pub(crate) fn clear_variable_size_expr(
        &mut self,
        name: &str,
        size: SizeId,
    ) -> Result<(), CoreError> {
        let i = self.variable_index(name)?;
        if self.find_size(size).is_none() {
            return Err(crate::size::SizeError::UnknownSize(size).into());
        }
        self.variables[i].per_size.retain(|e| e.size != size);
        Ok(())
    }

    /// Retire les valeurs par taille de ces tailles (suppression d'une taille ou d'une gamme),
    /// dans la même étape, avec une remarque.
    pub(crate) fn drop_size_variable_values(
        &mut self,
        sizes: &BTreeSet<SizeId>,
        out: &mut Outcome,
    ) {
        let mut removed = 0usize;
        for v in &mut self.variables {
            let before = v.per_size.len();
            v.per_size.retain(|e| !sizes.contains(&e.size));
            removed += before - v.per_size.len();
        }
        if removed > 0 {
            out.notes.push(format!(
                "{removed} valeur(s) de variable par taille retirée(s) avec la taille supprimée ; \
                 les valeurs des autres tailles sont conservées."
            ));
        }
    }

    /// `SetNotchProportional` : déclare ou retire le caractère proportionnel d'un cran. Refusé
    /// pour un cran ancré (il suit déjà son bord : l'ancrage est conservé, rien n'est écrit).
    pub(crate) fn set_notch_proportional(
        &mut self,
        sheet: SheetId,
        notch: NotchId,
        proportional: bool,
    ) -> Result<(), CoreError> {
        let anchored = self.sheet(sheet)?.notch(notch)?.anchor.is_some();
        if proportional && anchored {
            return Err(CoreError::Invalid(
                "cran proportionnel : un cran ancré suit déjà son bord, son ancrage est conservé \
                 (détachez-le d'abord pour le rendre proportionnel)"
                    .into(),
            ));
        }
        if let Some(n) = self
            .sheet_mut(sheet)?
            .notches
            .iter_mut()
            .find(|n| n.id == notch)
        {
            n.proportional = proportional;
        }
        Ok(())
    }

    /// Distances (mm) des crans proportionnels dans la taille `size`, telles que la lecture par
    /// taille les donne : (feuille, cran, distance). Vide sans cran proportionnel.
    pub(crate) fn proportional_distances_in(
        &self,
        size: SizeId,
    ) -> Result<Vec<(SheetId, NotchId, f64)>, crate::size::SizeError> {
        let any = |p: &Project| {
            p.sheets
                .iter()
                .any(|s| s.notches.iter().any(|n| n.proportional))
        };
        if !any(self) {
            return Ok(Vec::new());
        }
        let graded = self
            .graded_project(Some(size))
            .map_err(|e| crate::size::SizeError::Inconsistent(e.to_string()))?;
        Ok(graded
            .sheets
            .iter()
            .flat_map(|s| {
                s.notches
                    .iter()
                    .filter(|n| n.proportional && n.anchor.is_none())
                    .map(move |n| (s.id, n.id, n.distance_mm))
            })
            .collect())
    }

    /// Écrit ces distances de cran (feuille, cran, distance).
    pub(crate) fn write_notch_distances(&mut self, distances: &[(SheetId, NotchId, f64)]) {
        for &(sheet, notch, d) in distances {
            if let Some(n) = self
                .sheets
                .iter_mut()
                .find(|s| s.id == sheet)
                .and_then(|s| s.notches.iter_mut().find(|n| n.id == notch))
            {
                n.distance_mm = d;
            }
        }
    }

    /// Lecture : dans ce projet **évalué** (`self`), chaque cran proportionnel prend
    /// `fraction × longueur du tracé dans la taille`, la fraction étant `distance écrite ÷
    /// longueur du tracé dans le projet de base` (`base`). Une longueur qui ne se calcule pas
    /// laisse le cran tel quel (l'erreur est rapportée par les lectures du tracé).
    pub(crate) fn scale_proportional_notches(&mut self, base: &Project) {
        for s in &mut self.sheets {
            let Ok(b) = base.sheet(s.id) else { continue };
            let moves: Vec<(NotchId, f64)> = s
                .notches
                .iter()
                .filter(|n| n.proportional && n.anchor.is_none())
                .filter_map(|n| {
                    let before = b.path_length_mm(n.entity).ok()?;
                    let now = s.path_length_mm(n.entity).ok()?;
                    (before > 0.0 && before != now)
                        .then(|| (n.id, (n.distance_mm / before * now).clamp(0.0, now)))
                })
                .collect();
            for (id, d) in moves {
                if let Some(n) = s.notches.iter_mut().find(|n| n.id == id) {
                    n.distance_mm = d;
                }
            }
        }
    }

    /// Lecture : réécrit, dans ce projet évalué, le paramètre de chaque formule liée avec sa
    /// valeur dans la taille. Renvoie `true` si une valeur a changé. Sans valeur propre à la
    /// taille, rien n'est évalué ni réécrit (les octets de la taille de base sont intacts).
    pub(crate) fn apply_size_variable_values(&mut self, size: SizeId) -> Result<bool, CoreError> {
        if !self
            .variables
            .iter()
            .any(|v| v.per_size.iter().any(|e| e.size == size))
        {
            return Ok(false);
        }
        let label = label_of(self, size);
        let values: HashMap<String, f64> = self
            .variable_values_for_size(Some(size))?
            .into_iter()
            .collect();
        let mut changed = false;
        for s in &mut self.sheets {
            for f in s.formulas.clone() {
                let v = expr::evaluate(&f.expr, &|n| values.get(n).copied()).map_err(|e| {
                    CoreError::Invalid(format!(
                        "taille « {label} » : {} : formule « {} » : {e}",
                        s.parameter_label(f.point, f.param),
                        f.expr
                    ))
                })?;
                if s.parameter_value(f.point, f.param).is_some_and(|c| c != v) {
                    s.write_parameter(f.point, f.param, v);
                    changed = true;
                }
            }
        }
        Ok(changed)
    }
}

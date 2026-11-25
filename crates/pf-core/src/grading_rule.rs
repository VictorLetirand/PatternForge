//! Stockage v5 : bibliothèque de règles de gradation nommées (décision 334, pour US-09.3b).
//!
//! `Project.grading_rules` est une liste ordonnée de règles `{ id, name, mode?, deltas }` : une
//! règle est un tableau d'écarts **relatifs** `(taille, dx, dy)`, indépendant de tout point ; son
//! application à des points (avec le `mode` de copie : identique, symétrie X / Y, inversion) est
//! l'affaire d'US-09.3b. Types et validation seulement : aucune commande, aucune interface.
//! Les tailles sont référencées par leur identifiant (`SizeId`), jamais par leur rang.

use crate::error::CoreError;
use crate::grading::{GradingCopyMode, MAX_GRADING_MM};
use crate::model::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_GRADING_RULES: usize = 200;

/// Écart relatif d'une règle pour une taille (mm, repère de la feuille).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradingRuleDelta {
    pub size: SizeId,
    pub dx_mm: f64,
    pub dy_mm: f64,
}

/// Règle de gradation nommée.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradingRule {
    pub id: GradingRuleId,
    pub name: String,
    /// Transformation appliquée à l'application de la règle ; omis quand `same`.
    #[serde(default, skip_serializing_if = "is_same")]
    pub mode: GradingCopyMode,
    pub deltas: Vec<GradingRuleDelta>,
}

fn is_same(m: &GradingCopyMode) -> bool {
    *m == GradingCopyMode::Same
}

/// Nom d'un objet v5 : non vide, rogné, borné, sans caractère de contrôle.
pub(crate) fn check_stored_name(name: &str, what: &str) -> Result<(), CoreError> {
    check_name(name, what)?;
    if name != name.trim() {
        return Err(CoreError::Invalid(format!(
            "{what} a des espaces en début ou en fin"
        )));
    }
    Ok(())
}

impl Project {
    pub(crate) fn validate_grading_rules(&self) -> Result<(), CoreError> {
        if self.grading_rules.len() > MAX_GRADING_RULES {
            return Err(CoreError::Invalid(format!(
                "trop de règles de gradation : {MAX_GRADING_RULES} au plus"
            )));
        }
        for (i, r) in self.grading_rules.iter().enumerate() {
            let what = format!("règle de gradation {}", r.id.0);
            check_stored_name(&r.name, &format!("le nom de la {what}"))?;
            if self.grading_rules[..i]
                .iter()
                .any(|o| o.name.to_lowercase() == r.name.to_lowercase())
            {
                return Err(CoreError::Invalid(format!(
                    "deux règles de gradation « {} » (casse ignorée)",
                    r.name
                )));
            }
            if r.deltas.is_empty() {
                return Err(CoreError::Invalid(format!("la {what} n'a aucun écart")));
            }
            let mut seen = BTreeSet::new();
            for d in &r.deltas {
                if self.find_size(d.size).is_none() {
                    return Err(CoreError::DeadReference(format!(
                        "la {what} référence une taille absente (id {})",
                        d.size.0
                    )));
                }
                if !seen.insert(d.size) {
                    return Err(CoreError::Invalid(format!(
                        "la {what} a deux écarts pour la taille {}",
                        d.size.0
                    )));
                }
                if !d.dx_mm.is_finite() || !d.dy_mm.is_finite() {
                    return Err(CoreError::Invalid(format!("la {what} a un écart non fini")));
                }
                if d.dx_mm.abs() > MAX_GRADING_MM || d.dy_mm.abs() > MAX_GRADING_MM {
                    return Err(CoreError::Invalid(format!(
                        "la {what} a un écart hors de ±{} mm",
                        MAX_GRADING_MM as i64
                    )));
                }
            }
        }
        Ok(())
    }
}

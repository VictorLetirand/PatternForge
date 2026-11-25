//! US-03.1a — table de variables du projet : modèle, graphe de dépendances, commandes.
//!
//! `Project.variables` est une liste **ordonnée** (ordre d'insertion) de `{ name, unit, expr }`.
//! Seul le **texte** de l'expression est stocké ; les valeurs sont **calculées à la lecture**
//! (`Project::variable_values`), jamais écrites.
//!
//! Conventions (à faire valider par une modéliste, voir le suivi) :
//!
//! - nom : `[A-Za-z_][A-Za-z0-9_]*`, 40 caractères au plus, **sans accent** (portable dans le
//!   JSON et les outils tiers) ; unique **sans tenir compte de la casse** (`Tour` et `tour` se
//!   confondent à la lecture, donc refusés) ; jamais `min`, `max`, `abs`, `sqrt`, `pi`
//!   (même casse différente) ; les références, elles, respectent la casse ;
//! - l'**unité** (`mm`, `deg`, sans unité) est une étiquette d'affichage : aucune conversion, le
//!   calcul ne manipule que des nombres ;
//! - 500 variables au plus, expression de 500 caractères au plus ;
//! - une variable peut être citée par la formule d'un paramètre (US-03.1b, module `formula`) :
//!   elle n'est alors pas supprimable, et son renommage réécrit aussi la formule.

use crate::error::CoreError;
use crate::expr::{self, Expr, ExprError, ExprErrorKind, RESERVED_WORDS};
use crate::model::Project;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

pub const MAX_VARIABLES: usize = 500;
pub const MAX_VARIABLE_NAME_CHARS: usize = 40;

/// Unité affichée d'une variable (étiquette, jamais une conversion).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VariableUnit {
    #[serde(rename = "mm")]
    Mm,
    #[serde(rename = "deg")]
    Deg,
    #[serde(rename = "none")]
    Unitless,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Variable {
    pub name: String,
    pub unit: VariableUnit,
    /// Texte de l'expression saisie (syntaxe : voir `expr`).
    pub expr: String,
    /// Valeur par taille (schéma v5, US-09.7) : remplace `expr` dans la taille citée (formule ou
    /// nombre) ; une taille sans entrée garde `expr`. Absent du fichier si vide ; stockage posé,
    /// pas encore évalué par taille.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub per_size: Vec<VariableSizeExpr>,
}

/// Expression d'une variable pour une taille (identifiant de taille, jamais le rang).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariableSizeExpr {
    pub size: crate::model::SizeId,
    pub expr: String,
}

/// Erreur sur la table de variables.
#[derive(Debug, Clone, PartialEq)]
pub enum VariableError {
    TooMany {
        count: usize,
    },
    BadName {
        name: String,
        reason: String,
    },
    DuplicateName {
        name: String,
        other: String,
    },
    Unknown {
        name: String,
    },
    /// Une expression ne s'analyse pas ou ne s'évalue pas (syntaxe, cycle, division par zéro…).
    Expr {
        variable: String,
        error: ExprError,
    },
    /// Suppression refusée : d'autres variables s'y réfèrent.
    InUse {
        name: String,
        dependents: Vec<String>,
    },
}

impl fmt::Display for VariableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooMany { count } => {
                write!(f, "trop de variables ({count}) : {MAX_VARIABLES} au plus")
            }
            Self::BadName { name, reason } => write!(f, "nom de variable « {name} » : {reason}"),
            Self::DuplicateName { name, other } => {
                if name == other {
                    write!(f, "la variable « {name} » existe déjà")
                } else {
                    write!(
                        f,
                        "« {name} » et « {other} » ne diffèrent que par la casse : \
                         un nom de variable doit être unique sans tenir compte de la casse"
                    )
                }
            }
            Self::Unknown { name } => write!(f, "variable « {name} » introuvable"),
            Self::Expr { variable, error } => write!(f, "variable « {variable} » : {error}"),
            Self::InUse { name, dependents } => write!(
                f,
                "la variable « {name} » est utilisée par {} : modifiez-les avant de la supprimer",
                dependents
                    .iter()
                    .map(|d| format!("« {d} »"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }
    }
}

impl std::error::Error for VariableError {}

impl From<VariableError> for CoreError {
    fn from(e: VariableError) -> CoreError {
        CoreError::Invalid(e.to_string())
    }
}

/// Vérifie la forme d'un nom (pas son unicité).
pub fn check_variable_name(name: &str) -> Result<(), VariableError> {
    let bad = |reason: String| VariableError::BadName {
        name: name.to_string(),
        reason,
    };
    let count = name.chars().count();
    if count == 0 {
        return Err(bad("le nom ne peut pas être vide".into()));
    }
    if count > MAX_VARIABLE_NAME_CHARS {
        return Err(bad(format!("au plus {MAX_VARIABLE_NAME_CHARS} caractères")));
    }
    for (i, c) in name.chars().enumerate() {
        let ok = if i == 0 {
            c.is_ascii_alphabetic() || c == '_'
        } else {
            c.is_ascii_alphanumeric() || c == '_'
        };
        if !ok {
            return Err(bad(if i == 0 && c.is_ascii_digit() {
                "le nom ne peut pas commencer par un chiffre".into()
            } else {
                format!(
                    "caractère « {c} » non permis (lettres sans accent, chiffres et _ seulement)"
                )
            }));
        }
    }
    if RESERVED_WORDS.iter().any(|w| w.eq_ignore_ascii_case(name)) {
        return Err(bad("nom réservé (fonction ou constante)".into()));
    }
    Ok(())
}

struct Graph<'a> {
    vars: &'a [Variable],
    parsed: Vec<Expr>,
    index: HashMap<&'a str, usize>,
    state: Vec<u8>,
    values: Vec<f64>,
}

impl Graph<'_> {
    fn visit(&mut self, i: usize, stack: &mut Vec<usize>) -> Result<(), VariableError> {
        self.state[i] = 1;
        stack.push(i);
        let me = self.vars[i].name.clone();
        let fail = |error: ExprError| VariableError::Expr {
            variable: me.clone(),
            error,
        };
        for r in self.parsed[i].references() {
            let Some(&j) = self.index.get(r.name.as_str()) else {
                let hint = self
                    .vars
                    .iter()
                    .find(|v| v.name.eq_ignore_ascii_case(&r.name))
                    .map(|v| format!(" (la casse compte : « {} » existe)", v.name))
                    .unwrap_or_default();
                return Err(fail(
                    ExprError::new(
                        ExprErrorKind::UnknownVariable,
                        r.pos,
                        format!("variable « {} » inconnue{hint}", r.name),
                    )
                    .with_name(r.name.clone()),
                ));
            };
            match self.state[j] {
                1 => {
                    let from = stack.iter().position(|&k| k == j).unwrap_or(0);
                    let mut path: Vec<&str> = stack[from..]
                        .iter()
                        .map(|&k| self.vars[k].name.as_str())
                        .collect();
                    path.push(self.vars[j].name.as_str());
                    let path = path.join(" → ");
                    return Err(fail(
                        ExprError::new(
                            ExprErrorKind::Cycle,
                            r.pos,
                            format!("dépendance circulaire : {path}"),
                        )
                        .with_name(path),
                    ));
                }
                0 => self.visit(j, stack)?,
                _ => {}
            }
        }
        let value = {
            let values = &self.values;
            let index = &self.index;
            self.parsed[i].eval(&|n| index.get(n).map(|&k| values[k]))
        };
        self.values[i] = value.map_err(fail)?;
        self.state[i] = 2;
        stack.pop();
        Ok(())
    }
}

/// Valide une table (nombre, noms, unicité, syntaxe, références, cycles) et calcule toutes les
/// valeurs, dans l'ordre des dépendances ; le résultat suit l'ordre de la table. Fonction pure.
pub fn evaluate_variables(vars: &[Variable]) -> Result<Vec<(String, f64)>, VariableError> {
    if vars.len() > MAX_VARIABLES {
        return Err(VariableError::TooMany { count: vars.len() });
    }
    let mut seen: HashMap<String, &str> = HashMap::new();
    for v in vars {
        check_variable_name(&v.name)?;
        if let Some(other) = seen.insert(v.name.to_ascii_lowercase(), &v.name) {
            return Err(VariableError::DuplicateName {
                name: v.name.clone(),
                other: other.to_string(),
            });
        }
    }
    let mut parsed = Vec::with_capacity(vars.len());
    for v in vars {
        parsed.push(expr::parse(&v.expr).map_err(|error| VariableError::Expr {
            variable: v.name.clone(),
            error,
        })?);
    }
    let mut graph = Graph {
        vars,
        parsed,
        index: vars
            .iter()
            .enumerate()
            .map(|(i, v)| (v.name.as_str(), i))
            .collect(),
        state: vec![0; vars.len()],
        values: vec![0.0; vars.len()],
    };
    for i in 0..vars.len() {
        if graph.state[i] == 0 {
            graph.visit(i, &mut Vec::new())?;
        }
    }
    Ok(vars
        .iter()
        .zip(graph.values)
        .map(|(v, x)| (v.name.clone(), x))
        .collect())
}

impl Project {
    /// Valeurs de toutes les variables (ordre de la table). Lecture seule, pure ; échoue si la
    /// table est invalide (ce qu'une commande ne laisse jamais arriver).
    pub fn variable_values(&self) -> Result<Vec<(String, f64)>, VariableError> {
        evaluate_variables(&self.variables)
    }

    /// Variables dont l'expression (ou une expression par taille) cite `name` (dans l'ordre de
    /// la table).
    pub fn variable_dependents(&self, name: &str) -> Vec<String> {
        let cites = |text: &str| {
            expr::parse(text)
                .map(|e| e.references().iter().any(|r| r.name == name))
                .unwrap_or(false)
        };
        self.variables
            .iter()
            .filter(|v| cites(&v.expr) || v.per_size.iter().any(|e| cites(&e.expr)))
            .map(|v| v.name.clone())
            .collect()
    }

    pub(crate) fn variable_index(&self, name: &str) -> Result<usize, VariableError> {
        self.variables
            .iter()
            .position(|v| v.name == name)
            .ok_or_else(|| VariableError::Unknown {
                name: name.to_string(),
            })
    }

    /// Validation à l'ouverture et après chaque commande.
    pub(crate) fn validate_variables(&self) -> Result<(), CoreError> {
        if self.variables.is_empty() {
            return Ok(());
        }
        evaluate_variables(&self.variables)?;
        Ok(())
    }

    /// `AddVariable` : ajoute à la fin de la table.
    pub(crate) fn add_variable(
        &mut self,
        name: String,
        unit: VariableUnit,
        expr: String,
    ) -> Result<(), CoreError> {
        check_variable_name(&name)?;
        self.variables.push(Variable {
            name,
            unit,
            expr: expr.trim().to_string(),
            per_size: Vec::new(),
        });
        Ok(())
    }

    /// Valeurs par taille (schéma v5) : taille existante, une entrée par taille et variable, et,
    /// pour chaque taille citée, la table complète ainsi substituée s'évalue (syntaxe, références,
    /// cycles, divisions) : un fichier qui ne s'évaluerait pas dans une taille est refusé.
    pub(crate) fn validate_variable_sizes(&self) -> Result<(), CoreError> {
        let mut sizes = std::collections::BTreeSet::new();
        for v in &self.variables {
            let mut own = std::collections::BTreeSet::new();
            for e in &v.per_size {
                if self.find_size(e.size).is_none() {
                    return Err(CoreError::DeadReference(format!(
                        "la variable « {} » référence une taille absente (id {})",
                        v.name, e.size.0
                    )));
                }
                if !own.insert(e.size) {
                    return Err(CoreError::Invalid(format!(
                        "la variable « {} » a deux valeurs pour la taille {}",
                        v.name, e.size.0
                    )));
                }
                sizes.insert(e.size);
            }
        }
        for size in sizes {
            self.check_size_table(size)?;
        }
        Ok(())
    }

    /// `SetVariableExpr` : sans effet (aucune étape) si le texte est inchangé.
    pub(crate) fn set_variable_expr(&mut self, name: &str, expr: String) -> Result<(), CoreError> {
        let i = self.variable_index(name)?;
        self.variables[i].expr = expr.trim().to_string();
        Ok(())
    }

    pub(crate) fn set_variable_unit(
        &mut self,
        name: &str,
        unit: VariableUnit,
    ) -> Result<(), CoreError> {
        let i = self.variable_index(name)?;
        self.variables[i].unit = unit;
        Ok(())
    }

    /// `RenameVariable` : réécrit les références des autres variables dans la même étape.
    pub(crate) fn rename_variable(&mut self, from: &str, to: &str) -> Result<(), CoreError> {
        let i = self.variable_index(from)?;
        check_variable_name(to)?;
        if from == to {
            return Ok(());
        }
        if let Some(other) = self
            .variables
            .iter()
            .enumerate()
            .find(|(k, v)| *k != i && v.name.eq_ignore_ascii_case(to))
        {
            return Err(VariableError::DuplicateName {
                name: to.to_string(),
                other: other.1.name.clone(),
            }
            .into());
        }
        for v in &mut self.variables {
            v.expr =
                expr::rename_reference(&v.expr, from, to).map_err(|error| VariableError::Expr {
                    variable: v.name.clone(),
                    error,
                })?;
            // Valeurs par taille (US-09.7) : réécrites dans la même étape.
            for e in &mut v.per_size {
                e.expr = expr::rename_reference(&e.expr, from, to).map_err(|error| {
                    VariableError::Expr {
                        variable: v.name.clone(),
                        error,
                    }
                })?;
            }
        }
        // Les formules liées (US-03.1b) suivent le renommage dans la même étape.
        for s in &mut self.sheets {
            for f in &mut s.formulas {
                f.expr = expr::rename_reference(&f.expr, from, to).map_err(|error| {
                    CoreError::Invalid(format!("formule « {} » : {error}", f.expr))
                })?;
            }
            // Cibles de longueur imposée maintenues par une formule (US-03.3b).
            for c in &mut s.length_constraints {
                if let Some(text) = &c.expr {
                    c.expr = Some(expr::rename_reference(text, from, to).map_err(|error| {
                        CoreError::Invalid(format!("formule « {text} » : {error}"))
                    })?);
                }
                // Décalage d'une égalité de longueurs maintenu par une formule (US-03.4b).
                if let Some(link) = &mut c.link {
                    if let Some(text) = &link.offset_expr {
                        link.offset_expr =
                            Some(expr::rename_reference(text, from, to).map_err(|error| {
                                CoreError::Invalid(format!("formule « {text} » : {error}"))
                            })?);
                    }
                }
            }
        }
        self.variables[i].name = to.to_string();
        Ok(())
    }

    /// `RemoveVariable` : refusé tant qu'une autre variable ou une formule liée s'y réfère.
    pub(crate) fn remove_variable(&mut self, name: &str) -> Result<(), CoreError> {
        let i = self.variable_index(name)?;
        let mut dependents = self.variable_dependents(name);
        dependents.extend(self.formula_users(name));
        dependents.extend(self.length_formula_users(name));
        if !dependents.is_empty() {
            return Err(VariableError::InUse {
                name: name.to_string(),
                dependents,
            }
            .into());
        }
        self.variables.remove(i);
        Ok(())
    }
}

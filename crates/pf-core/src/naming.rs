//! US-11.5 — conventions de nommage (tranche 130, schéma 5 inchangé : clé optionnelle).
//!
//! `Project.naming_rules` = règles `{ id, name, target, prefix?, separator?, suffix?, counter? }`.
//! Une règle vise **une** cible que le modèle nomme déjà (pièce, code de pièce, feuille, variante),
//! une règle par cible. Le nom produit est `préfixe + séparateur + numéro + séparateur + suffixe`
//! (parties vides omises, séparateurs seulement entre deux parties présentes) ; le numéro est
//! rempli de zéros à gauche jusqu'à `counter.width`.
//!
//! **Rien d'appliqué par défaut.** Le prochain nom se calcule à la lecture ([`Project::next_name`]) :
//! plus grand numéro déjà présent dans les noms de la cible qui suivent le motif, plus le pas
//! (ou le début s'il n'y en a pas), puis saut des noms déjà pris (casse ignorée). Aucun état de
//! compteur n'est stocké : annuler une création rend donc exactement le même prochain nom. Les
//! noms existants ne changent que par `Command::ApplyNamingRule` (renommage en lot explicite,
//! aperçu par [`Project::naming_preview`], un seul pas d'annulation).

use crate::command::Outcome;
use crate::error::CoreError;
use crate::grading_rule::check_stored_name;
use crate::measurement_ops::{clean_name, same_name};
use crate::model::*;
use serde::{Deserialize, Serialize};

/// Nombre maximal de règles d'un projet.
pub const MAX_NAMING_RULES: usize = 20;
/// Longueur maximale (caractères) d'un préfixe ou d'un suffixe.
pub const MAX_NAMING_PART_CHARS: usize = 40;
/// Longueur maximale (caractères) d'un séparateur.
pub const MAX_NAMING_SEPARATOR_CHARS: usize = 4;
/// Plus grand début de compteur.
pub const MAX_NAMING_START: u32 = 999_999;
/// Plus grand pas de compteur.
pub const MAX_NAMING_STEP: u32 = 1000;
/// Plus grande largeur de remplissage.
pub const MAX_NAMING_WIDTH: u8 = 9;
/// Plus grand numéro produit (9 chiffres).
pub const MAX_NAMING_NUMBER: u64 = 999_999_999;

/// Ce que la règle nomme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NamingTarget {
    Piece,
    PieceCode,
    Sheet,
    Variant,
}

/// Compteur d'une règle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamingCounter {
    pub start: u32,
    pub step: u32,
    /// Largeur de remplissage par des zéros (0 = aucun).
    #[serde(default)]
    pub width: u8,
}

/// Paramètres d'une règle (sans identifiant), utilisés par les commandes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamingRuleSpec {
    pub name: String,
    pub target: NamingTarget,
    #[serde(default)]
    pub prefix: String,
    #[serde(default)]
    pub separator: String,
    #[serde(default)]
    pub suffix: String,
    #[serde(default)]
    pub counter: Option<NamingCounter>,
}

/// Règle stockée dans [`Project::naming_rules`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamingRule {
    pub id: NamingRuleId,
    pub name: String,
    pub target: NamingTarget,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prefix: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub separator: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub suffix: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub counter: Option<NamingCounter>,
}

/// Une ligne de l'aperçu d'un renommage en lot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamingChange {
    /// Identifiant de l'objet renommé (pièce, feuille ou variante).
    pub id: u64,
    pub old: String,
    pub new: String,
}

impl NamingRule {
    /// Nom produit pour ce numéro (`None` : règle sans compteur).
    pub fn format(&self, n: Option<u64>) -> String {
        let num = n.map(|n| match self.counter {
            Some(c) => format!("{n:0>w$}", w = c.width as usize),
            None => n.to_string(),
        });
        let mut parts: Vec<String> = Vec::new();
        if !self.prefix.is_empty() {
            parts.push(self.prefix.clone());
        }
        if let Some(num) = num {
            parts.push(num);
        }
        if !self.suffix.is_empty() {
            parts.push(self.suffix.clone());
        }
        parts.join(&self.separator)
    }

    /// Numéro porté par un nom qui suit exactement le motif de la règle.
    pub fn parse_number(&self, name: &str) -> Option<u64> {
        let head = if self.prefix.is_empty() {
            String::new()
        } else {
            format!("{}{}", self.prefix, self.separator)
        };
        let tail = if self.suffix.is_empty() {
            String::new()
        } else {
            format!("{}{}", self.separator, self.suffix)
        };
        let mid = name
            .strip_prefix(head.as_str())?
            .strip_suffix(tail.as_str())?;
        if mid.is_empty() || mid.len() > 9 || !mid.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        mid.parse().ok()
    }
}

fn bad(msg: String) -> CoreError {
    CoreError::Invalid(msg)
}

fn check_part(text: &str, what: &str, max: usize) -> Result<(), CoreError> {
    if text.chars().count() > max {
        return Err(bad(format!("{what} dépasse {max} caractères")));
    }
    if text.chars().any(|c| c.is_control()) {
        return Err(bad(format!("{what} contient un caractère de contrôle")));
    }
    Ok(())
}

fn check_trimmed_part(text: &str, what: &str) -> Result<(), CoreError> {
    check_part(text, what, MAX_NAMING_PART_CHARS)?;
    if text != text.trim() {
        return Err(bad(format!("{what} a des espaces en début ou en fin")));
    }
    Ok(())
}

fn check_counter(c: &NamingCounter, what: &str) -> Result<(), CoreError> {
    if c.start > MAX_NAMING_START {
        return Err(bad(format!(
            "{what} : début du compteur hors bornes (0 à {MAX_NAMING_START})"
        )));
    }
    if !(1..=MAX_NAMING_STEP).contains(&c.step) {
        return Err(bad(format!(
            "{what} : pas du compteur hors bornes (1 à {MAX_NAMING_STEP})"
        )));
    }
    if c.width > MAX_NAMING_WIDTH {
        return Err(bad(format!(
            "{what} : largeur de remplissage hors bornes (0 à {MAX_NAMING_WIDTH})"
        )));
    }
    Ok(())
}

/// Contrôle du contenu d'une règle déjà rognée (commande et validation du fichier).
fn check_rule_content(r: &NamingRule) -> Result<(), CoreError> {
    let what = format!("règle de nommage « {} »", r.name);
    check_trimmed_part(&r.prefix, &format!("le préfixe de la {what}"))?;
    check_trimmed_part(&r.suffix, &format!("le suffixe de la {what}"))?;
    check_part(
        &r.separator,
        &format!("le séparateur de la {what}"),
        MAX_NAMING_SEPARATOR_CHARS,
    )?;
    match &r.counter {
        Some(c) => check_counter(c, &what)?,
        None if r.prefix.is_empty() => {
            return Err(bad(format!(
                "{what} : un préfixe vide exige un compteur (sinon aucun nom ne serait produit)"
            )))
        }
        None => {}
    }
    Ok(())
}

impl Project {
    pub fn naming_rule(&self, id: NamingRuleId) -> Result<&NamingRule, CoreError> {
        self.naming_rules
            .iter()
            .find(|r| r.id == id)
            .ok_or_else(|| bad(format!("règle de nommage introuvable (id {})", id.0)))
    }

    /// Règle de la cible, si elle existe (au plus une par cible).
    pub fn naming_rule_for(&self, target: NamingTarget) -> Option<&NamingRule> {
        self.naming_rules.iter().find(|r| r.target == target)
    }

    /// Noms (ou codes) déjà portés par les objets de la cible, dans l'ordre du projet.
    pub fn naming_existing(&self, target: NamingTarget) -> Vec<(u64, String)> {
        match target {
            NamingTarget::Piece => self
                .pieces
                .iter()
                .map(|p| (p.id.0, p.name.clone()))
                .collect(),
            NamingTarget::PieceCode => self
                .pieces
                .iter()
                .filter_map(|p| p.code.clone().map(|c| (p.id.0, c)))
                .collect(),
            NamingTarget::Sheet => self
                .sheets
                .iter()
                .map(|s| (s.id.0, s.name.clone()))
                .collect(),
            NamingTarget::Variant => self
                .variants
                .iter()
                .map(|v| (v.id.0, v.name.clone()))
                .collect(),
        }
    }

    /// Prochain nom libre produit par la règle (lecture seule, rien n'est consommé).
    pub fn next_name(&self, rule: NamingRuleId) -> Result<String, CoreError> {
        let r = self.naming_rule(rule)?;
        let existing = self.naming_existing(r.target);
        let taken = |s: &str| existing.iter().any(|(_, n)| same_name(n, s));
        let Some(c) = r.counter else {
            let name = r.format(None);
            if taken(&name) {
                return Err(bad(format!(
                    "règle « {} » : le nom « {name} » est déjà pris et la règle n'a pas de compteur",
                    r.name
                )));
            }
            return Ok(name);
        };
        let max = existing.iter().filter_map(|(_, n)| r.parse_number(n)).max();
        let start = u64::from(c.start);
        let mut n = match max {
            Some(m) => (m + u64::from(c.step)).max(start),
            None => start,
        };
        for _ in 0..=existing.len() {
            if n > MAX_NAMING_NUMBER {
                break;
            }
            let name = r.format(Some(n));
            if !taken(&name) {
                return Ok(name);
            }
            n += u64::from(c.step);
        }
        Err(bad(format!(
            "règle « {} » : le compteur dépasse {MAX_NAMING_NUMBER}",
            r.name
        )))
    }

    /// Aperçu du renommage en lot : tous les objets de la cible, dans l'ordre du projet,
    /// reçoivent `début`, `début + pas`, … (les pièces sans code sont ignorées pour `PieceCode`).
    pub fn naming_preview(&self, rule: NamingRuleId) -> Result<Vec<NamingChange>, CoreError> {
        let r = self.naming_rule(rule)?;
        let Some(c) = r.counter else {
            return Err(bad(format!(
                "règle « {} » : un renommage en lot exige un compteur",
                r.name
            )));
        };
        let mut out = Vec::new();
        let mut n = u64::from(c.start);
        for (id, old) in self.naming_existing(r.target) {
            if n > MAX_NAMING_NUMBER {
                return Err(bad(format!(
                    "règle « {} » : le compteur dépasse {MAX_NAMING_NUMBER}",
                    r.name
                )));
            }
            let new = r.format(Some(n));
            check_name(&new, "le nom produit")?;
            out.push(NamingChange { id, old, new });
            n += u64::from(c.step);
        }
        Ok(out)
    }

    fn check_naming_spec(
        &self,
        spec: &NamingRuleSpec,
        except: Option<NamingRuleId>,
        id: NamingRuleId,
    ) -> Result<NamingRule, CoreError> {
        let name = clean_name(&spec.name, "le nom de la règle de nommage")?;
        let rule = NamingRule {
            id,
            name,
            target: spec.target,
            prefix: spec.prefix.clone(),
            separator: spec.separator.clone(),
            suffix: spec.suffix.clone(),
            counter: spec.counter,
        };
        check_rule_content(&rule)?;
        for o in &self.naming_rules {
            if Some(o.id) == except {
                continue;
            }
            if same_name(&o.name, &rule.name) {
                return Err(bad(format!(
                    "une règle de nommage « {} » existe déjà (casse ignorée)",
                    rule.name
                )));
            }
            if o.target == rule.target {
                return Err(bad(format!(
                    "la règle « {} » vise déjà la même cible : une seule règle par cible",
                    o.name
                )));
            }
        }
        Ok(rule)
    }

    /// `AddNamingRule`.
    pub(crate) fn add_naming_rule(
        &mut self,
        spec: &NamingRuleSpec,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        if self.naming_rules.len() >= MAX_NAMING_RULES {
            return Err(bad(format!(
                "trop de règles de nommage : {MAX_NAMING_RULES} au plus"
            )));
        }
        let id = NamingRuleId(self.alloc_id());
        let rule = self.check_naming_spec(spec, None, id)?;
        self.naming_rules.push(rule);
        out.naming_rules.push(id);
        Ok(())
    }

    /// `UpdateNamingRule` (mêmes valeurs = aucune étape).
    pub(crate) fn update_naming_rule(
        &mut self,
        id: NamingRuleId,
        spec: &NamingRuleSpec,
    ) -> Result<(), CoreError> {
        self.naming_rule(id)?;
        let rule = self.check_naming_spec(spec, Some(id), id)?;
        if let Some(slot) = self.naming_rules.iter_mut().find(|r| r.id == id) {
            *slot = rule;
        }
        Ok(())
    }

    /// `RemoveNamingRule` : les noms déjà donnés ne changent pas.
    pub(crate) fn remove_naming_rule(&mut self, id: NamingRuleId) -> Result<(), CoreError> {
        self.naming_rule(id)?;
        self.naming_rules.retain(|r| r.id != id);
        Ok(())
    }

    /// `ApplyNamingRule` : renommage en lot, une seule étape.
    pub(crate) fn apply_naming_rule(&mut self, id: NamingRuleId) -> Result<(), CoreError> {
        let target = self.naming_rule(id)?.target;
        let changes = self.naming_preview(id)?;
        for ch in changes {
            match target {
                NamingTarget::Piece => self.piece_mut(PieceId(ch.id))?.name = ch.new,
                NamingTarget::PieceCode => self.piece_mut(PieceId(ch.id))?.code = Some(ch.new),
                NamingTarget::Sheet => self.sheet_mut(SheetId(ch.id))?.name = ch.new,
                NamingTarget::Variant => {
                    if let Some(v) = self.variants.iter_mut().find(|v| v.id.0 == ch.id) {
                        v.name = ch.new;
                    }
                }
            }
        }
        Ok(())
    }

    /// Validation des règles (appelée par `Project::validate`).
    pub(crate) fn validate_naming_rules(&self) -> Result<(), CoreError> {
        if self.naming_rules.len() > MAX_NAMING_RULES {
            return Err(bad(format!(
                "trop de règles de nommage : {MAX_NAMING_RULES} au plus"
            )));
        }
        for (i, r) in self.naming_rules.iter().enumerate() {
            check_stored_name(
                &r.name,
                &format!("le nom de la règle de nommage {}", r.id.0),
            )?;
            check_rule_content(r)?;
            for o in &self.naming_rules[..i] {
                if same_name(&o.name, &r.name) {
                    return Err(bad(format!(
                        "deux règles de nommage « {} » (casse ignorée)",
                        r.name
                    )));
                }
                if o.target == r.target {
                    return Err(bad(format!(
                        "les règles « {} » et « {} » visent la même cible",
                        o.name, r.name
                    )));
                }
            }
        }
        Ok(())
    }
}

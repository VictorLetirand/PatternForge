//! US-09.1 — tailles du modèle (schéma v4, décision 324).
//!
//! `Project.sizes` est une liste **ordonnée** de gammes ([`SizeRange`]) : une gamme nommée
//! (alphanumérique XS S M L, ou numérique 34 36 38…) est une suite **ordonnée** de tailles
//! ([`Size`]) dont une et une seule est la **taille de base** (`base`). Plusieurs gammes sont des
//! gammes **indépendantes** (stature × tour) : aucun produit cartésien n'est stocké.
//!
//! Les écarts par point (US-09.2a, module `grading`) s'attachent à ces tailles ; aucune géométrie
//! graduée n'est stockée (ADR-C). `Project::evaluate_sheet(feuille, taille)` calcule la feuille
//! dans une taille (les points libres déplacés de leur écart, les points construits recalculés
//! depuis leurs parents) ; les exports et les mesures ne prennent pas encore la taille
//! (US-09.5 à 09.8).
//!
//! Conventions (à faire valider par une modéliste) :
//! - l'**ordre** des tailles est celui de la liste (jamais un tri alphabétique) ;
//! - le **type** (`alphanumeric` / `numeric`) est une étiquette de la gamme : seule la cohérence
//!   du libellé est vérifiée (non vide, rogné, 20 caractères au plus, sans caractère de contrôle,
//!   unique dans la gamme **sans tenir compte de la casse**) ;
//! - le nom d'une gamme est unique dans le projet (casse ignorée) ;
//! - une gamme a au moins une taille : retirer la dernière est refusé (supprimer la gamme) ;
//! - la taille de base ne peut pas être retirée : désigner d'abord une autre base ;
//! - retirer une taille (ou une gamme) retire ses écarts dans la même étape, jamais ceux des
//!   autres tailles ; changer la base re-exprime les écarts (positions évaluées identiques) ;
//! - une taille de rupture est un simple marqueur (la base peut l'être) ;
//! - 20 gammes au plus, 60 tailles par gamme ;
//! - la « taille active » d'affichage n'est **pas** enregistrée (état d'interface).

use crate::command::{Command, Outcome};
use crate::error::CoreError;
use crate::model::*;
use pf_geometry::PointMm;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

pub const MAX_SIZE_RANGES: usize = 20;
pub const MAX_SIZES_PER_RANGE: usize = 60;
pub const MAX_SIZE_LABEL_CHARS: usize = 20;

/// Famille de libellés d'une gamme (étiquette ; aucun ordre n'en est déduit).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SizeKind {
    #[serde(rename = "alphanumeric")]
    Alphanumeric,
    #[serde(rename = "numeric")]
    Numeric,
}

/// Une taille d'une gamme.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Size {
    pub id: SizeId,
    pub label: String,
    /// Taille de rupture (marqueur) ; absent du fichier si faux.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub break_size: bool,
    /// Fiche client dont la taille a été générée (schéma v6, `GenerateClientSize`) ; absent pour
    /// une taille ordinaire. Supprimer la fiche tant qu'une taille la cite est refusé.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<ClientId>,
}

/// Une gamme nommée : tailles ordonnées et taille de base.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SizeRange {
    pub id: SizeRangeId,
    pub name: String,
    pub kind: SizeKind,
    pub sizes: Vec<Size>,
    /// Taille de base : l'une des tailles de `sizes`.
    pub base: SizeId,
}

impl SizeRange {
    pub fn size(&self, id: SizeId) -> Option<&Size> {
        self.sizes.iter().find(|s| s.id == id)
    }

    pub fn index_of(&self, id: SizeId) -> Option<usize> {
        self.sizes.iter().position(|s| s.id == id)
    }

    /// Taille de base (la validation garantit qu'elle existe).
    pub fn base_size(&self) -> Option<&Size> {
        self.size(self.base)
    }
}

/// Élément qui dépend d'une taille : aujourd'hui, les écarts de gradation qui lui sont attachés
/// (US-09.2a). Retirer la taille les retire, dans la même étape.
#[derive(Debug, Clone, PartialEq)]
pub enum SizeDependent {
    /// Écart d'un point libre pour cette taille.
    Grading {
        sheet: SheetId,
        point: PointId,
        dx_mm: f64,
        dy_mm: f64,
    },
}

/// Pourquoi un libellé ou un nom est refusé.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextProblem {
    Empty,
    TooLong {
        max: usize,
    },
    Control,
    /// Espaces en tête ou en queue (fichier édité à la main ; les commandes rognent).
    Untrimmed,
}

impl fmt::Display for TextProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TextProblem::Empty => write!(f, "vide"),
            TextProblem::TooLong { max } => write!(f, "plus de {max} caractères"),
            TextProblem::Control => write!(f, "caractère de contrôle"),
            TextProblem::Untrimmed => write!(f, "espaces en début ou en fin"),
        }
    }
}

/// Refus d'une opération sur les tailles (affichable tel quel, localisable par l'interface).
#[derive(Debug, Clone, PartialEq)]
pub enum SizeError {
    UnknownRange(SizeRangeId),
    UnknownSize(SizeId),
    TooManyRanges {
        max: usize,
    },
    TooManySizes {
        max: usize,
    },
    RangeName {
        problem: TextProblem,
    },
    RangeNameTaken {
        name: String,
    },
    SizeLabel {
        label: String,
        problem: TextProblem,
    },
    SizeLabelTaken {
        label: String,
    },
    NoSizes,
    BaseOutOfRange {
        index: usize,
        len: usize,
    },
    IndexOutOfRange {
        index: usize,
        len: usize,
    },
    CannotRemoveBase {
        label: String,
    },
    CannotRemoveLastSize,
    /// Changer la base déplacerait la position de base commune à toutes les gammes (la nouvelle
    /// base porte des écarts) alors que le projet a plusieurs gammes.
    BaseShiftsOtherRanges {
        label: String,
    },
    /// Changer la base déplacerait des points d'une feuille qui a des liens maintenus ou des
    /// longueurs imposées (non réévalués par taille).
    BaseShiftsMaintained {
        label: String,
        sheet: String,
    },
    /// Changer la base laisserait des valeurs de variables par taille à ré-exprimer (US-09.7).
    BaseShiftsVariables {
        label: String,
    },
    NotASizeCommand,
    /// État incohérent trouvé par la validation (fichier altéré).
    Inconsistent(String),
}

impl fmt::Display for SizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SizeError::UnknownRange(id) => write!(f, "gamme de tailles inconnue (id {})", id.0),
            SizeError::UnknownSize(id) => write!(f, "taille inconnue (id {})", id.0),
            SizeError::TooManyRanges { max } => {
                write!(f, "au plus {max} gammes de tailles par projet")
            }
            SizeError::TooManySizes { max } => write!(f, "au plus {max} tailles par gamme"),
            SizeError::RangeName { problem } => {
                write!(f, "nom de gamme invalide ({problem})")
            }
            SizeError::RangeNameTaken { name } => {
                write!(f, "une gamme « {name} » existe déjà (casse ignorée)")
            }
            SizeError::SizeLabel { label, problem } => {
                write!(f, "libellé de taille « {label} » invalide ({problem})")
            }
            SizeError::SizeLabelTaken { label } => write!(
                f,
                "la taille « {label} » existe déjà dans cette gamme (casse ignorée)"
            ),
            SizeError::NoSizes => write!(f, "une gamme doit avoir au moins une taille"),
            SizeError::BaseOutOfRange { index, len } => write!(
                f,
                "la taille de base (rang {index}) n'existe pas : la gamme a {len} taille(s)"
            ),
            SizeError::IndexOutOfRange { index, len } => {
                write!(f, "rang {index} hors de la gamme ({len} taille(s))")
            }
            SizeError::CannotRemoveBase { label } => write!(
                f,
                "la taille « {label} » est la taille de base : désignez d'abord une autre base"
            ),
            SizeError::CannotRemoveLastSize => write!(
                f,
                "une gamme garde au moins une taille : supprimez la gamme à la place"
            ),
            SizeError::BaseShiftsOtherRanges { label } => write!(
                f,
                "« {label} » porte des écarts : la changer en base déplacerait la position de base \
                 commune à toutes les gammes (le projet en a plusieurs), refusé"
            ),
            SizeError::BaseShiftsMaintained { label, sheet } => write!(
                f,
                "« {label} » porte des écarts : la changer en base déplacerait des points de la \
                 feuille « {sheet} », qui a des liens maintenus ou des longueurs imposées (non \
                 réévalués par taille), refusé"
            ),
            SizeError::BaseShiftsVariables { label } => write!(
                f,
                "la gamme porte des valeurs de variables par taille : « {label} » ne peut pas \
                 devenir la base tant qu'elles existent (retirez-les d'abord)"
            ),
            SizeError::NotASizeCommand => write!(f, "commande étrangère aux tailles"),
            SizeError::Inconsistent(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for SizeError {}

impl From<SizeError> for CoreError {
    fn from(e: SizeError) -> CoreError {
        CoreError::Invalid(e.to_string())
    }
}

/// Positions de tous les points d'une feuille pour une taille (API posée en US-09.1).
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluatedSheet {
    pub sheet: SheetId,
    /// Taille demandée (`None` = taille de base).
    pub size: Option<SizeId>,
    /// Positions en mm, dans l'ordre des points de la feuille (un point qui ne se calcule pas
    /// dans cette taille est absent et décrit dans `issues`).
    pub points: Vec<(PointId, PointMm)>,
    /// Points et tracés qui ne se calculent pas dans cette taille (message du cœur) ; vide pour
    /// une géométrie saine.
    pub issues: Vec<String>,
}

fn check_text(s: &str, max: usize) -> Result<(), TextProblem> {
    if s.trim().is_empty() {
        return Err(TextProblem::Empty);
    }
    if s.chars().any(|c| c.is_control()) {
        return Err(TextProblem::Control);
    }
    if s.chars().count() > max {
        return Err(TextProblem::TooLong { max });
    }
    if s != s.trim() {
        return Err(TextProblem::Untrimmed);
    }
    Ok(())
}

fn check_range_name(name: &str) -> Result<(), SizeError> {
    check_text(name, MAX_NAME_LEN).map_err(|problem| SizeError::RangeName { problem })
}

fn check_size_label(label: &str) -> Result<(), SizeError> {
    check_text(label, MAX_SIZE_LABEL_CHARS).map_err(|problem| SizeError::SizeLabel {
        label: label.to_string(),
        problem,
    })
}

fn same_text(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

impl Project {
    pub fn size_range(&self, id: SizeRangeId) -> Result<&SizeRange, SizeError> {
        self.sizes
            .iter()
            .find(|r| r.id == id)
            .ok_or(SizeError::UnknownRange(id))
    }

    /// Gamme qui porte la taille, et rang de la taille dans cette gamme.
    pub fn find_size(&self, id: SizeId) -> Option<(&SizeRange, usize)> {
        self.sizes
            .iter()
            .find_map(|r| r.index_of(id).map(|i| (r, i)))
    }

    fn size_label_of(&self, id: SizeId) -> String {
        self.find_size(id)
            .map(|(r, i)| r.sizes[i].label.clone())
            .unwrap_or_default()
    }

    fn range_pos_of_size(&self, id: SizeId) -> Result<(usize, usize), SizeError> {
        self.sizes
            .iter()
            .enumerate()
            .find_map(|(ri, r)| r.index_of(id).map(|si| (ri, si)))
            .ok_or(SizeError::UnknownSize(id))
    }

    /// Ce qui dépend d'une taille : les écarts de gradation qui lui sont attachés (retirer la
    /// taille les retire).
    pub fn size_dependents(&self, size: SizeId) -> Vec<SizeDependent> {
        self.gradings_of_size(size)
            .into_iter()
            .map(|(sheet, g)| SizeDependent::Grading {
                sheet,
                point: g.point,
                dx_mm: g.dx_mm,
                dy_mm: g.dy_mm,
            })
            .collect()
    }

    /// Positions de tous les points d'une feuille pour une taille. `None` = taille de base =
    /// géométrie écrite ; sinon les points libres sont déplacés de leur écart pour cette taille
    /// (position de base + écart cumulé) et les points construits sont recalculés depuis leurs
    /// parents gradués. Une taille inconnue est refusée (« taille inconnue »). Les liens
    /// maintenus, formules et longueurs imposées ne sont pas réévalués par taille.
    ///
    /// Nommée `evaluate_sheet` et portée par le projet : `Sheet::evaluate` existe déjà (position
    /// d'une définition de point) et une feuille seule ne connaît pas les gammes.
    pub fn evaluate_sheet(
        &self,
        sheet: SheetId,
        size: Option<SizeId>,
    ) -> Result<EvaluatedSheet, CoreError> {
        if let Some(id) = size {
            if self.find_size(id).is_none() {
                return Err(SizeError::UnknownSize(id).into());
            }
        }
        // Projet évalué (US-09.5a) : les pièces dérivées et surpiqûres liées d'autres feuilles
        // suivent aussi ; `None` = géométrie écrite.
        let graded;
        let s = match size {
            None => self.sheet(sheet)?,
            Some(_) => {
                graded = self.graded_project(size)?;
                graded.sheet(sheet)?
            }
        };
        let points = s
            .points
            .iter()
            .filter_map(|p| s.position(p.id).ok().map(|pos| (p.id, pos)))
            .collect();
        Ok(EvaluatedSheet {
            sheet,
            size,
            points,
            issues: s.evaluation_issues(),
        })
    }

    /// Valide `Project.sizes` (les identifiants sont contrôlés par `validate`).
    pub(crate) fn validate_sizes(&self) -> Result<(), CoreError> {
        let bad = |m: String| CoreError::from(SizeError::Inconsistent(m));
        if self.sizes.len() > MAX_SIZE_RANGES {
            return Err(SizeError::TooManyRanges {
                max: MAX_SIZE_RANGES,
            }
            .into());
        }
        for (i, r) in self.sizes.iter().enumerate() {
            check_range_name(&r.name)?;
            if self.sizes[..i].iter().any(|o| same_text(&o.name, &r.name)) {
                return Err(SizeError::RangeNameTaken {
                    name: r.name.clone(),
                }
                .into());
            }
            if r.sizes.is_empty() {
                return Err(bad(format!("la gamme « {} » n'a aucune taille", r.name)));
            }
            if r.sizes.len() > MAX_SIZES_PER_RANGE {
                return Err(SizeError::TooManySizes {
                    max: MAX_SIZES_PER_RANGE,
                }
                .into());
            }
            for (k, s) in r.sizes.iter().enumerate() {
                check_size_label(&s.label)?;
                if r.sizes[..k].iter().any(|o| same_text(&o.label, &s.label)) {
                    return Err(SizeError::SizeLabelTaken {
                        label: s.label.clone(),
                    }
                    .into());
                }
                if let Some(c) = s.client {
                    if self.client(c).is_err() {
                        return Err(CoreError::DeadReference(format!(
                            "la taille « {} » de la gamme « {} » cite la fiche client {} qui n'existe plus",
                            s.label, r.name, c.0
                        )));
                    }
                }
            }
            if r.size(r.base).is_none() {
                return Err(bad(format!(
                    "la taille de base (id {}) n'appartient pas à la gamme « {} »",
                    r.base.0, r.name
                )));
            }
        }
        Ok(())
    }

    /// Exécute une commande de tailles. Refus : l'état n'est pas à relire (le projet appelant
    /// travaille sur une copie, voir `Project::apply`).
    pub(crate) fn apply_size_command(
        &mut self,
        command: Command,
        out: &mut Outcome,
    ) -> Result<(), SizeError> {
        match command {
            Command::AddSizeRange {
                name,
                kind,
                labels,
                base,
            } => self.add_size_range(name, kind, labels, base, out),
            Command::RenameSizeRange { range, name } => self.rename_size_range(range, name),
            Command::RemoveSizeRange { range } => self.remove_size_range(range, out),
            Command::AddSize { range, label, at } => self.add_size(range, label, at, out),
            Command::RemoveSize { size } => self.remove_size(size, out),
            Command::RenameSize { size, label } => self.rename_size(size, label),
            Command::MoveSize { size, to } => self.move_size(size, to),
            Command::SetSizeBreak { size, break_size } => {
                let (ri, si) = self.range_pos_of_size(size)?;
                self.sizes[ri].sizes[si].break_size = break_size;
                Ok(())
            }
            Command::SetBaseSize { size } => {
                let (ri, _) = self.range_pos_of_size(size)?;
                if self.sizes[ri].base != size {
                    // Valeurs de variables par taille (US-09.7) : leur ré-expression n'est pas
                    // prise en charge, la base ne change pas tant qu'il en existe dans la gamme.
                    if self.sizes[ri].sizes.iter().any(|s| {
                        self.variables
                            .iter()
                            .any(|v| v.per_size.iter().any(|e| e.size == s.id))
                    }) {
                        return Err(SizeError::BaseShiftsVariables {
                            label: self.size_label_of(size),
                        });
                    }
                    // Crans proportionnels (US-09.5c) : leur distance écrite devient celle de la
                    // nouvelle base, pour que leur position dans chaque taille ne change pas.
                    let proportional = self.proportional_distances_in(size)?;
                    self.rebase_gradings(size)?;
                    self.sizes[ri].base = size;
                    self.write_notch_distances(&proportional);
                }
                Ok(())
            }
            _ => Err(SizeError::NotASizeCommand),
        }
    }

    /// Refus éventuel d'une commande de tailles sur ce projet, **typé** (l'interface le localise) ;
    /// ne modifie rien.
    pub fn check_size_command(&self, command: &Command) -> Result<(), SizeError> {
        let mut draft = self.clone();
        draft.apply_size_command(command.clone(), &mut Outcome::default())
    }

    fn add_size_range(
        &mut self,
        name: String,
        kind: SizeKind,
        labels: Vec<String>,
        base: usize,
        out: &mut Outcome,
    ) -> Result<(), SizeError> {
        if self.sizes.len() >= MAX_SIZE_RANGES {
            return Err(SizeError::TooManyRanges {
                max: MAX_SIZE_RANGES,
            });
        }
        let name = name.trim().to_string();
        check_range_name(&name)?;
        if self.sizes.iter().any(|r| same_text(&r.name, &name)) {
            return Err(SizeError::RangeNameTaken { name });
        }
        if labels.is_empty() {
            return Err(SizeError::NoSizes);
        }
        if labels.len() > MAX_SIZES_PER_RANGE {
            return Err(SizeError::TooManySizes {
                max: MAX_SIZES_PER_RANGE,
            });
        }
        let labels: Vec<String> = labels.iter().map(|l| l.trim().to_string()).collect();
        for (i, l) in labels.iter().enumerate() {
            check_size_label(l)?;
            if labels[..i].iter().any(|o| same_text(o, l)) {
                return Err(SizeError::SizeLabelTaken { label: l.clone() });
            }
        }
        if base >= labels.len() {
            return Err(SizeError::BaseOutOfRange {
                index: base,
                len: labels.len(),
            });
        }
        let id = SizeRangeId(self.alloc_id());
        let sizes: Vec<Size> = labels
            .into_iter()
            .map(|label| Size {
                id: SizeId(self.alloc_id()),
                label,
                break_size: false,
                client: None,
            })
            .collect();
        out.sizes.extend(sizes.iter().map(|s| s.id));
        out.size_ranges.push(id);
        let base = sizes[base].id;
        self.sizes.push(SizeRange {
            id,
            name,
            kind,
            sizes,
            base,
        });
        Ok(())
    }

    fn rename_size_range(&mut self, range: SizeRangeId, name: String) -> Result<(), SizeError> {
        self.size_range(range)?;
        let name = name.trim().to_string();
        check_range_name(&name)?;
        if self
            .sizes
            .iter()
            .any(|r| r.id != range && same_text(&r.name, &name))
        {
            return Err(SizeError::RangeNameTaken { name });
        }
        if let Some(r) = self.sizes.iter_mut().find(|r| r.id == range) {
            r.name = name;
        }
        Ok(())
    }

    fn remove_size_range(
        &mut self,
        range: SizeRangeId,
        out: &mut Outcome,
    ) -> Result<(), SizeError> {
        let ids: BTreeSet<SizeId> = self.size_range(range)?.sizes.iter().map(|s| s.id).collect();
        self.drop_size_gradings(&ids, out);
        self.drop_size_variable_values(&ids, out);
        self.drop_size_measurement_values(&ids, out);
        self.sizes.retain(|r| r.id != range);
        Ok(())
    }

    fn add_size(
        &mut self,
        range: SizeRangeId,
        label: String,
        at: Option<usize>,
        out: &mut Outcome,
    ) -> Result<(), SizeError> {
        let r = self.size_range(range)?;
        if r.sizes.len() >= MAX_SIZES_PER_RANGE {
            return Err(SizeError::TooManySizes {
                max: MAX_SIZES_PER_RANGE,
            });
        }
        let label = label.trim().to_string();
        check_size_label(&label)?;
        if r.sizes.iter().any(|s| same_text(&s.label, &label)) {
            return Err(SizeError::SizeLabelTaken { label });
        }
        let len = r.sizes.len();
        let at = at.unwrap_or(len);
        if at > len {
            return Err(SizeError::IndexOutOfRange { index: at, len });
        }
        let id = SizeId(self.alloc_id());
        out.sizes.push(id);
        if let Some(r) = self.sizes.iter_mut().find(|r| r.id == range) {
            r.sizes.insert(
                at,
                Size {
                    id,
                    label,
                    break_size: false,
                    client: None,
                },
            );
        }
        Ok(())
    }

    fn remove_size(&mut self, size: SizeId, out: &mut Outcome) -> Result<(), SizeError> {
        let (ri, si) = self.range_pos_of_size(size)?;
        let r = &self.sizes[ri];
        let s = &r.sizes[si];
        if r.sizes.len() == 1 {
            return Err(SizeError::CannotRemoveLastSize);
        }
        if r.base == size {
            return Err(SizeError::CannotRemoveBase {
                label: s.label.clone(),
            });
        }
        self.drop_size_gradings(&BTreeSet::from([size]), out);
        self.drop_size_variable_values(&BTreeSet::from([size]), out);
        self.drop_size_measurement_values(&BTreeSet::from([size]), out);
        self.sizes[ri].sizes.remove(si);
        Ok(())
    }

    fn rename_size(&mut self, size: SizeId, label: String) -> Result<(), SizeError> {
        let (ri, si) = self.range_pos_of_size(size)?;
        let label = label.trim().to_string();
        check_size_label(&label)?;
        let r = &self.sizes[ri];
        if r.sizes
            .iter()
            .any(|s| s.id != size && same_text(&s.label, &label))
        {
            return Err(SizeError::SizeLabelTaken { label });
        }
        self.sizes[ri].sizes[si].label = label;
        Ok(())
    }

    fn move_size(&mut self, size: SizeId, to: usize) -> Result<(), SizeError> {
        let (ri, si) = self.range_pos_of_size(size)?;
        let len = self.sizes[ri].sizes.len();
        if to >= len {
            return Err(SizeError::IndexOutOfRange { index: to, len });
        }
        let moved = self.sizes[ri].sizes.remove(si);
        self.sizes[ri].sizes.insert(to, moved);
        Ok(())
    }
}

//! US-10.3 — retouches nommées : commandes et lecture (schéma 5 inchangé).
//!
//! Une retouche (`Project.alterations`) = `{ id, name, target, delta }` : `target` est une **mesure
//! du barème** (`MeasurementId`), `delta` s'ajoute à la valeur de cette mesure, dans son unité
//! (mm ou degrés). Rien n'est dérivé ni stocké : la valeur retouchée est **calculée à la lecture**
//! ([`Project::altered_measurement_value`], [`Project::alteration_report`]) et appliquée, dans le
//! projet lu par taille ([`Project::graded_project`]), **après** les valeurs de variables par
//! taille, les écarts de gradation, les crans proportionnels et la resynchronisation des
//! dérivées : la retouche ne touche donc ni la géométrie de base, ni la géométrie graduée.
//!
//! Limite déclarée : aucun lien ne rattache une mesure du barème à la géométrie (pas de solveur
//! de mesure), la retouche change **la valeur de référence** de la mesure, pas le tracé.
//!
//! Plusieurs retouches sur la même mesure s'additionnent (l'ordre est sans effet) ; la somme est
//! vérifiée **une fois**, à la fin : une valeur retouchée négative ou au-delà de 1 000 000 n'est
//! jamais fabriquée (`OutOfRange`, la valeur retouchée est absente). Une taille sans valeur de
//! barème n'est jamais devinée (`NoBaseValue`).

use crate::error::CoreError;
use crate::measurement::*;
use crate::measurement_ops::{clean_name, same_name};
use crate::model::*;
use crate::size::SizeError;

/// Pourquoi une valeur retouchée est absente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlterationIssue {
    /// La mesure n'a pas de valeur pour cette taille : rien n'est deviné.
    NoBaseValue,
    /// La somme valeur + deltas est négative, non finie ou au-delà de 1 000 000.
    OutOfRange,
}

/// Valeur d'une mesure pour une taille, avant et après retouches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AlteredValue {
    /// Valeur du barème (`None` = non renseignée).
    pub base: Option<f64>,
    /// Somme des deltas des retouches qui ciblent la mesure.
    pub delta: f64,
    /// Nombre de retouches qui ciblent la mesure.
    pub count: usize,
    /// Valeur retouchée ; sans retouche = `base`.
    pub altered: Option<f64>,
    /// `Some` seulement si une retouche existe et ne peut pas être appliquée.
    pub issue: Option<AlterationIssue>,
}

/// État d'une retouche dans une taille.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlterationStatus {
    Applied,
    NoBaseValue,
    OutOfRange,
    /// La mesure ciblée n'existe plus (impossible après `Project::apply`, signalé pour un projet
    /// construit à la main).
    DeadTarget,
}

/// Lecture d'une retouche pour une taille.
#[derive(Debug, Clone, PartialEq)]
pub struct AlterationReading {
    pub id: AlterationId,
    pub name: String,
    pub target: MeasurementId,
    pub delta: f64,
    pub base: Option<f64>,
    /// Valeur de la cible avec **toutes** les retouches qui la visent.
    pub altered: Option<f64>,
    pub status: AlterationStatus,
}

fn bad(m: String) -> CoreError {
    CoreError::Invalid(m)
}

fn check_delta(delta: f64, what: &str) -> Result<(), CoreError> {
    if !delta.is_finite() {
        return Err(bad(format!("{what} : delta non fini")));
    }
    if delta.abs() > MAX_MEASUREMENT_VALUE {
        return Err(bad(format!(
            "{what} : delta hors bornes ({MAX_MEASUREMENT_VALUE} au plus en valeur absolue)"
        )));
    }
    Ok(())
}

impl Project {
    pub fn alteration(&self, id: AlterationId) -> Result<&Alteration, CoreError> {
        self.alterations
            .iter()
            .find(|a| a.id == id)
            .ok_or_else(|| bad(format!("retouche introuvable (id {})", id.0)))
    }

    fn alteration_mut(&mut self, id: AlterationId) -> Result<&mut Alteration, CoreError> {
        self.alterations
            .iter_mut()
            .find(|a| a.id == id)
            .ok_or_else(|| bad(format!("retouche introuvable (id {})", id.0)))
    }

    fn check_new_alteration_name(
        &self,
        name: &str,
        except: Option<AlterationId>,
    ) -> Result<(), CoreError> {
        if self
            .alterations
            .iter()
            .any(|a| Some(a.id) != except && same_name(&a.name, name))
        {
            return Err(bad(format!(
                "une retouche « {name} » existe déjà (casse ignorée)"
            )));
        }
        Ok(())
    }

    /// `AddAlteration`.
    pub(crate) fn add_alteration(
        &mut self,
        name: &str,
        target: MeasurementId,
        delta: f64,
        out: &mut crate::command::Outcome,
    ) -> Result<(), CoreError> {
        let name = clean_name(name, "le nom de la retouche")?;
        self.check_new_alteration_name(&name, None)?;
        self.measurement(target)?;
        check_delta(delta, &format!("retouche « {name} »"))?;
        if self.alterations.len() >= MAX_ALTERATIONS {
            return Err(bad(format!(
                "trop de retouches : {MAX_ALTERATIONS} au plus"
            )));
        }
        let id = AlterationId(self.alloc_id());
        self.alterations.push(Alteration {
            id,
            name,
            target,
            delta,
        });
        out.alterations.push(id);
        Ok(())
    }

    /// `RenameAlteration` : même nom = aucun effet.
    pub(crate) fn rename_alteration(
        &mut self,
        id: AlterationId,
        name: &str,
    ) -> Result<(), CoreError> {
        self.alteration(id)?;
        let name = clean_name(name, "le nom de la retouche")?;
        self.check_new_alteration_name(&name, Some(id))?;
        self.alteration_mut(id)?.name = name;
        Ok(())
    }

    /// `RemoveAlteration`.
    pub(crate) fn remove_alteration(&mut self, id: AlterationId) -> Result<(), CoreError> {
        self.alteration(id)?;
        self.alterations.retain(|a| a.id != id);
        Ok(())
    }

    /// `SetAlterationTarget`.
    pub(crate) fn set_alteration_target(
        &mut self,
        id: AlterationId,
        target: MeasurementId,
    ) -> Result<(), CoreError> {
        self.alteration(id)?;
        self.measurement(target)?;
        self.alteration_mut(id)?.target = target;
        Ok(())
    }

    /// `SetAlterationDelta`.
    pub(crate) fn set_alteration_delta(
        &mut self,
        id: AlterationId,
        delta: f64,
    ) -> Result<(), CoreError> {
        let name = self.alteration(id)?.name.clone();
        check_delta(delta, &format!("retouche « {name} »"))?;
        self.alteration_mut(id)?.delta = delta;
        Ok(())
    }

    /// Valeur de `measurement` pour `size` avant et après retouches (lecture pure : rien n'est
    /// écrit, la géométrie n'est pas lue). Refus : mesure ou taille inconnue.
    pub fn altered_measurement_value(
        &self,
        measurement: MeasurementId,
        size: SizeId,
    ) -> Result<AlteredValue, CoreError> {
        let m = self.measurement(measurement)?;
        if self.find_size(size).is_none() {
            return Err(SizeError::UnknownSize(size).into());
        }
        let base = m.values.iter().find(|v| v.size == size).map(|v| v.value);
        let (mut delta, mut count) = (0.0, 0usize);
        for a in self.alterations.iter().filter(|a| a.target == measurement) {
            delta += a.delta;
            count += 1;
        }
        let (altered, issue) = match (base, count) {
            (b, 0) => (b, None),
            (None, _) => (None, Some(AlterationIssue::NoBaseValue)),
            (Some(b), _) => {
                let v = b + delta;
                if v.is_finite() && (0.0..=MAX_MEASUREMENT_VALUE).contains(&v) {
                    (Some(v), None)
                } else {
                    (None, Some(AlterationIssue::OutOfRange))
                }
            }
        };
        Ok(AlteredValue {
            base,
            delta,
            count,
            altered,
            issue,
        })
    }

    /// Une lecture par retouche, dans l'ordre du projet, pour la taille `size`.
    pub fn alteration_report(&self, size: SizeId) -> Result<Vec<AlterationReading>, CoreError> {
        if self.find_size(size).is_none() {
            return Err(SizeError::UnknownSize(size).into());
        }
        Ok(self
            .alterations
            .iter()
            .map(|a| {
                let mut r = AlterationReading {
                    id: a.id,
                    name: a.name.clone(),
                    target: a.target,
                    delta: a.delta,
                    base: None,
                    altered: None,
                    status: AlterationStatus::DeadTarget,
                };
                if let Ok(v) = self.altered_measurement_value(a.target, size) {
                    r.base = v.base;
                    r.altered = v.altered;
                    r.status = match v.issue {
                        None => AlterationStatus::Applied,
                        Some(AlterationIssue::NoBaseValue) => AlterationStatus::NoBaseValue,
                        Some(AlterationIssue::OutOfRange) => AlterationStatus::OutOfRange,
                    };
                }
                r
            })
            .collect())
    }

    /// Écrit, dans **cette copie de lecture** (jamais dans le projet de l'utilisateur), la valeur
    /// retouchée de `size` pour chaque mesure ciblée ; une valeur qui ne peut pas être retouchée
    /// est retirée de la copie (jamais une valeur non retouchée présentée comme retouchée).
    pub(crate) fn apply_alterations_in_read_copy(&mut self, size: SizeId) {
        if self.alterations.is_empty() {
            return;
        }
        let targets: Vec<MeasurementId> = self.measurements.iter().map(|m| m.id).collect();
        for id in targets {
            let Ok(v) = self.altered_measurement_value(id, size) else {
                continue;
            };
            if v.count == 0 || v.base.is_none() {
                continue;
            }
            let Some(m) = self.measurements.iter_mut().find(|m| m.id == id) else {
                continue;
            };
            match v.altered {
                Some(x) => {
                    if let Some(e) = m.values.iter_mut().find(|e| e.size == size) {
                        e.value = x;
                    }
                }
                None => m.values.retain(|e| e.size != size),
            }
        }
    }
}

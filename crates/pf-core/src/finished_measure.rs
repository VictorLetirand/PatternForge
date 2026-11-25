//! US-10.4 — mesures finies avec tolérances : commandes et lecture (schéma 5 inchangé).
//!
//! Une mesure finie (`Project.finished_measures`) = `{ id, name, measurement, source, tolerance_minus,
//! tolerance_plus }` : la **valeur mesurée** est lue sur la géométrie de la taille (couture appariée
//! ou cote complète, par [`Project::graded_project`]), la **référence** est la valeur de la mesure du
//! barème pour cette taille (avec ou sans retouches, au choix de l'appelant). Rien n'est stocké.
//!
//! Écart signé `gap = mesuré − référence` ; **conforme** si `−tolerance_minus ≤ gap ≤ tolerance_plus`
//! (bornes incluses, à 1e-9 près) ; **hors tolérance** sinon ; **non comparable** si la source est
//! dégénérée, si la valeur du barème est absente (jamais devinée, jamais interpolée), si l'unité de
//! la source et celle de la mesure diffèrent, ou si la retouche rend la référence impossible.
//!
//! Conventions d'atelier (à faire valider par une modéliste) : la longueur d'une couture est celle du
//! **côté A** (côté à embuer) ; une cote horizontale ou verticale est prise en **valeur absolue** ;
//! « sur la base » = taille de base de la gamme (aucune gradation, géométrie écrite) ; le rapport de
//! contrôle lit la base de la **première** gamme.
//!
//! Une couture, une cote ou une feuille supprimée retire la mesure finie dans la même étape, avec
//! remarque ([`Project::prune_dead_finished_measures`]) ; une mesure du barème supprimée la retire
//! aussi (`measurement_ops`).

use crate::command::Outcome;
use crate::error::CoreError;
use crate::full_dimension::FullDimensionKind;
use crate::measurement::*;
use crate::measurement_ops::{clean_name, same_name};
use crate::model::*;

/// Tolérance numérique des bornes (mm ou degrés).
pub const FINISHED_EPSILON: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishedStatus {
    Conforming,
    OutOfTolerance,
    NotComparable,
}

/// Pourquoi une mesure finie n'est pas comparable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FinishedReason {
    /// Couture ou cote introuvable, tracé dégénéré, contour ouvert : valeur mesurée illisible.
    SourceDegenerate,
    /// Le barème n'a pas de valeur pour cette taille.
    NoReferenceValue,
    /// Unité de la source (mm / degrés) différente de celle de la mesure du barème.
    UnitMismatch,
    /// Les retouches rendent la valeur de référence négative ou hors bornes.
    AlterationOutOfRange,
    /// La mesure du barème n'existe plus (impossible après `Project::apply`).
    DeadMeasurement,
}

impl FinishedReason {
    /// Identifiant stable (colonne du CSV, clés de langue).
    pub fn key(self) -> &'static str {
        match self {
            FinishedReason::SourceDegenerate => "source_degenerate",
            FinishedReason::NoReferenceValue => "no_reference",
            FinishedReason::UnitMismatch => "unit_mismatch",
            FinishedReason::AlterationOutOfRange => "alteration_out_of_range",
            FinishedReason::DeadMeasurement => "dead_measurement",
        }
    }
}

/// Lecture d'une mesure finie pour une taille (valeurs **observées**, jamais stockées).
#[derive(Debug, Clone, PartialEq)]
pub struct FinishedReading {
    pub id: FinishedMeasureId,
    pub name: String,
    pub measurement: MeasurementId,
    /// Nom de la mesure du barème (vide si elle a disparu).
    pub measurement_name: String,
    pub unit: MeasurementUnit,
    pub source: FinishedSource,
    /// Taille lue ; `None` = géométrie écrite sans taille (aucune référence possible).
    pub size: Option<SizeId>,
    pub size_label: Option<String>,
    pub measured: Option<f64>,
    pub reference: Option<f64>,
    /// Somme des deltas de retouche comprise dans `reference` (0 sans retouche).
    pub alteration_delta: f64,
    /// `mesuré − référence` (signé).
    pub gap: Option<f64>,
    pub tolerance_minus: f64,
    pub tolerance_plus: f64,
    pub status: FinishedStatus,
    pub reason: Option<FinishedReason>,
}

/// Tableau des mesures finies d'une gamme : une colonne par taille, dans l'ordre de la gamme.
#[derive(Debug, Clone, PartialEq)]
pub struct FinishedTable {
    pub range: SizeRangeId,
    pub sizes: Vec<SizeId>,
    /// `columns[i]` = lectures de `sizes[i]`, une par mesure finie (ordre du projet).
    pub columns: Vec<Vec<FinishedReading>>,
}

fn bad(m: String) -> CoreError {
    CoreError::Invalid(m)
}

fn check_tolerance(v: f64, what: &str) -> Result<(), CoreError> {
    if !v.is_finite() || v < 0.0 || v > MAX_MEASUREMENT_VALUE {
        return Err(bad(format!(
            "{what} : tolérance non finie, négative ou hors bornes ({MAX_MEASUREMENT_VALUE} au plus)"
        )));
    }
    Ok(())
}

impl Project {
    pub fn finished_measure(&self, id: FinishedMeasureId) -> Result<&FinishedMeasure, CoreError> {
        self.finished_measures
            .iter()
            .find(|f| f.id == id)
            .ok_or_else(|| bad(format!("mesure finie introuvable (id {})", id.0)))
    }

    fn check_new_finished_name(
        &self,
        name: &str,
        except: Option<FinishedMeasureId>,
    ) -> Result<(), CoreError> {
        if self
            .finished_measures
            .iter()
            .any(|f| Some(f.id) != except && same_name(&f.name, name))
        {
            return Err(bad(format!(
                "une mesure finie « {name} » existe déjà (casse ignorée)"
            )));
        }
        Ok(())
    }

    /// Unité attendue de la source (seule une cote d'angle est en degrés).
    fn finished_source_unit(&self, source: &FinishedSource) -> Result<MeasurementUnit, CoreError> {
        match *source {
            FinishedSource::Seam { seam } => {
                if self.seams.iter().any(|s| s.id == seam) {
                    Ok(MeasurementUnit::Mm)
                } else {
                    Err(bad(format!("couture appariée introuvable (id {})", seam.0)))
                }
            }
            FinishedSource::FullDimension { sheet, dimension } => {
                let d = self.sheet(sheet)?.full_dimension(dimension)?;
                Ok(match d.kind {
                    FullDimensionKind::Angle { .. } => MeasurementUnit::Deg,
                    _ => MeasurementUnit::Mm,
                })
            }
        }
    }

    fn check_finished(
        &self,
        name: &str,
        measurement: MeasurementId,
        source: &FinishedSource,
        tolerance_minus: f64,
        tolerance_plus: f64,
    ) -> Result<(), CoreError> {
        let what = format!("mesure finie « {name} »");
        let m = self.measurement(measurement)?;
        let unit = self.finished_source_unit(source)?;
        if unit != m.unit {
            return Err(bad(format!(
                "{what} : la source est en {} et la mesure « {} » en {}",
                unit_text(unit),
                m.name,
                unit_text(m.unit)
            )));
        }
        check_tolerance(tolerance_minus, &what)?;
        check_tolerance(tolerance_plus, &what)
    }

    /// `AddFinishedMeasure`.
    pub(crate) fn add_finished_measure(
        &mut self,
        name: &str,
        measurement: MeasurementId,
        source: FinishedSource,
        tolerance_minus: f64,
        tolerance_plus: f64,
        out: &mut Outcome,
    ) -> Result<(), CoreError> {
        let name = clean_name(name, "le nom de la mesure finie")?;
        self.check_new_finished_name(&name, None)?;
        self.check_finished(&name, measurement, &source, tolerance_minus, tolerance_plus)?;
        if self.finished_measures.len() >= MAX_FINISHED_MEASURES {
            return Err(bad(format!(
                "trop de mesures finies : {MAX_FINISHED_MEASURES} au plus"
            )));
        }
        let id = FinishedMeasureId(self.alloc_id());
        self.finished_measures.push(FinishedMeasure {
            id,
            name,
            measurement,
            source,
            tolerance_minus,
            tolerance_plus,
        });
        out.finished_measures.push(id);
        Ok(())
    }

    /// `SetFinishedMeasure` : remplace toute la définition (mêmes valeurs = aucune étape).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn set_finished_measure(
        &mut self,
        id: FinishedMeasureId,
        name: &str,
        measurement: MeasurementId,
        source: FinishedSource,
        tolerance_minus: f64,
        tolerance_plus: f64,
    ) -> Result<(), CoreError> {
        self.finished_measure(id)?;
        let name = clean_name(name, "le nom de la mesure finie")?;
        self.check_new_finished_name(&name, Some(id))?;
        self.check_finished(&name, measurement, &source, tolerance_minus, tolerance_plus)?;
        if let Some(f) = self.finished_measures.iter_mut().find(|f| f.id == id) {
            *f = FinishedMeasure {
                id,
                name,
                measurement,
                source,
                tolerance_minus,
                tolerance_plus,
            };
        }
        Ok(())
    }

    /// `RemoveFinishedMeasure`.
    pub(crate) fn remove_finished_measure(
        &mut self,
        id: FinishedMeasureId,
    ) -> Result<(), CoreError> {
        self.finished_measure(id)?;
        self.finished_measures.retain(|f| f.id != id);
        Ok(())
    }

    /// Retire, avec remarque, les mesures finies dont la couture, la feuille ou la cote a disparu
    /// (la validation refuse les références mortes) ; appelée par `Project::apply`.
    pub(crate) fn prune_dead_finished_measures(&mut self, out: &mut Outcome) {
        if self.finished_measures.is_empty() {
            return;
        }
        let dead: Vec<(FinishedMeasureId, String)> = self
            .finished_measures
            .iter()
            .filter(|f| self.finished_source_unit(&f.source).is_err())
            .map(|f| (f.id, f.name.clone()))
            .collect();
        for (id, name) in dead {
            self.finished_measures.retain(|f| f.id != id);
            out.notes.push(format!(
                "La mesure finie « {name} » mesurait une couture ou une cote supprimée : elle est \
                 retirée."
            ));
        }
    }

    /// Valeur mesurée par la source dans `geometry` (projet lu dans la taille) ; `None` = illisible.
    fn finished_measured(geometry: &Project, source: &FinishedSource) -> Option<f64> {
        let v = match *source {
            FinishedSource::Seam { seam } => geometry
                .seam_pair_report()
                .into_iter()
                .find(|r| r.id == seam)
                .and_then(|r| r.length_a_mm),
            FinishedSource::FullDimension { sheet, dimension } => {
                let g = geometry
                    .sheet(sheet)
                    .ok()?
                    .full_dimension_geometry(dimension)
                    .ok()?;
                Some(match g.kind {
                    FullDimensionKind::Horizontal { .. } | FullDimensionKind::Vertical { .. } => {
                        g.value.abs()
                    }
                    _ => g.value,
                })
            }
        };
        v.filter(|v| v.is_finite())
    }

    /// Lecture de toutes les mesures finies dans la taille `size` (`None` = géométrie écrite, sans
    /// référence possible). Lecture pure. `with_alterations` : la référence est la valeur de
    /// barème **retouchée** (`alteration_delta` dit de combien). Refus : taille inconnue.
    pub fn finished_measure_readings(
        &self,
        size: Option<SizeId>,
        with_alterations: bool,
    ) -> Result<Vec<FinishedReading>, CoreError> {
        let geometry = match size {
            Some(s) => self.graded_project(Some(s))?,
            None => self.clone(),
        };
        let size_label =
            size.and_then(|s| self.find_size(s).map(|(r, i)| r.sizes[i].label.clone()));
        let mut out = Vec::with_capacity(self.finished_measures.len());
        for f in &self.finished_measures {
            let m = self.measurement(f.measurement).ok();
            let mut r = FinishedReading {
                id: f.id,
                name: f.name.clone(),
                measurement: f.measurement,
                measurement_name: m.map(|m| m.name.clone()).unwrap_or_default(),
                unit: m.map_or(MeasurementUnit::Mm, |m| m.unit),
                source: f.source,
                size,
                size_label: size_label.clone(),
                measured: None,
                reference: None,
                alteration_delta: 0.0,
                gap: None,
                tolerance_minus: f.tolerance_minus,
                tolerance_plus: f.tolerance_plus,
                status: FinishedStatus::NotComparable,
                reason: None,
            };
            let Some(m) = m else {
                r.reason = Some(FinishedReason::DeadMeasurement);
                out.push(r);
                continue;
            };
            match self.finished_source_unit(&f.source) {
                Ok(u) if u != m.unit => {
                    r.reason = Some(FinishedReason::UnitMismatch);
                    out.push(r);
                    continue;
                }
                Err(_) => {
                    r.reason = Some(FinishedReason::SourceDegenerate);
                    out.push(r);
                    continue;
                }
                Ok(_) => {}
            }
            r.measured = Self::finished_measured(&geometry, &f.source);
            if let Some(s) = size {
                if with_alterations {
                    let v = self.altered_measurement_value(f.measurement, s)?;
                    r.reference = v.altered;
                    if v.altered.is_some() && v.count > 0 {
                        r.alteration_delta = v.delta;
                    }
                    if v.issue == Some(crate::alteration::AlterationIssue::OutOfRange) {
                        r.reason = Some(FinishedReason::AlterationOutOfRange);
                    }
                } else {
                    r.reference = m.values.iter().find(|v| v.size == s).map(|v| v.value);
                }
            }
            if r.measured.is_none() {
                r.reason = Some(FinishedReason::SourceDegenerate);
            } else if r.reference.is_none() && r.reason.is_none() {
                r.reason = Some(FinishedReason::NoReferenceValue);
            }
            if r.reason.is_none() {
                if let (Some(a), Some(b)) = (r.measured, r.reference) {
                    let gap = a - b;
                    r.gap = Some(gap);
                    r.status = if gap >= -f.tolerance_minus - FINISHED_EPSILON
                        && gap <= f.tolerance_plus + FINISHED_EPSILON
                    {
                        FinishedStatus::Conforming
                    } else {
                        FinishedStatus::OutOfTolerance
                    };
                }
            }
            out.push(r);
        }
        Ok(out)
    }

    /// Tableau des mesures finies de la gamme : une colonne par taille (la base comprise).
    pub fn finished_measure_table(
        &self,
        range: SizeRangeId,
        with_alterations: bool,
    ) -> Result<FinishedTable, CoreError> {
        let r = self.size_range(range)?;
        let sizes: Vec<SizeId> = r.sizes.iter().map(|s| s.id).collect();
        let mut columns = Vec::with_capacity(sizes.len());
        for s in &sizes {
            columns.push(self.finished_measure_readings(Some(*s), with_alterations)?);
        }
        Ok(FinishedTable {
            range,
            sizes,
            columns,
        })
    }

    /// Lecture de la taille de base de la **première** gamme, retouches comprises (rapport de
    /// contrôle) ; sans gamme : géométrie écrite, aucune référence (non comparable).
    pub fn finished_measure_base_readings(&self) -> Vec<FinishedReading> {
        let base = self.sizes.first().map(|r| r.base);
        self.finished_measure_readings(base, true)
            .unwrap_or_default()
    }
}

fn unit_text(u: MeasurementUnit) -> &'static str {
    match u {
        MeasurementUnit::Mm => "mm",
        MeasurementUnit::Deg => "deg",
    }
}

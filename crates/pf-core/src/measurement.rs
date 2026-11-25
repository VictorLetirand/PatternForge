//! Stockage v5 : mesures, fiches client, retouches, mesures finies (décision 334, pour E10).
//!
//! Types et validation seulement : aucune commande, aucune interface avant US-10.1 à 10.4.
//! - `Project.measurements` : barème, une mesure = `{ id, name, unit, values }` où `values` est la
//!   valeur par **taille** (`SizeId`) ; une taille sans valeur = mesure non renseignée.
//! - `Project.clients` : fiche = valeurs par mesure ; une mesure absente est **manquante**, jamais
//!   devinée ni remplie par défaut.
//! - `Project.alterations` : retouche nommée = delta sur une mesure.
//! - `Project.finished_measures` : mesure du vêtement fini = source calculée + tolérances,
//!   comparée à une ligne du barème.
//!
//! Les valeurs sont en mm (ou degrés pour `deg`) : l'unité est une étiquette, aucune conversion.

use crate::error::CoreError;
use crate::grading_rule::check_stored_name;
use crate::model::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MAX_MEASUREMENTS: usize = 500;
pub const MAX_CLIENTS: usize = 1000;
pub const MAX_ALTERATIONS: usize = 500;
pub const MAX_FINISHED_MEASURES: usize = 500;
/// Borne (valeur absolue) d'une valeur, d'un delta ou d'une tolérance.
pub const MAX_MEASUREMENT_VALUE: f64 = 1_000_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MeasurementUnit {
    #[serde(rename = "mm")]
    Mm,
    #[serde(rename = "deg")]
    Deg,
}

/// Valeur d'une mesure pour une taille.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeasurementValue {
    pub size: SizeId,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measurement {
    pub id: MeasurementId,
    pub name: String,
    pub unit: MeasurementUnit,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<MeasurementValue>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientValue {
    pub measurement: MeasurementId,
    pub value: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Client {
    pub id: ClientId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<ClientValue>,
}

/// Retouche nommée : `delta` (dans l'unité de la mesure ciblée) ajouté à `target`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Alteration {
    pub id: AlterationId,
    pub name: String,
    pub target: MeasurementId,
    pub delta: f64,
}

/// D'où vient la valeur calculée d'une mesure finie.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum FinishedSource {
    /// Longueur d'une couture appariée (E08).
    Seam { seam: SeamPairId },
    /// Cote complète d'une feuille (US-06.7).
    FullDimension {
        sheet: SheetId,
        dimension: FullDimensionId,
    },
}

/// Mesure du vêtement fini, comparée à la ligne `measurement` du barème avec ses tolérances
/// (valeurs ≥ 0, dans l'unité de la mesure ; écart admis : `−tolerance_minus` à `+tolerance_plus`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FinishedMeasure {
    pub id: FinishedMeasureId,
    pub name: String,
    pub measurement: MeasurementId,
    pub source: FinishedSource,
    pub tolerance_minus: f64,
    pub tolerance_plus: f64,
}

fn bad(m: String) -> CoreError {
    CoreError::Invalid(m)
}

fn dead(m: String) -> CoreError {
    CoreError::DeadReference(m)
}

fn in_bounds(v: f64, signed: bool) -> bool {
    v.is_finite() && v.abs() <= MAX_MEASUREMENT_VALUE && (signed || v >= 0.0)
}

fn unique_names<'a>(names: impl Iterator<Item = &'a str>, what: &str) -> Result<(), CoreError> {
    let mut seen = BTreeSet::new();
    for n in names {
        if !seen.insert(n.to_lowercase()) {
            return Err(bad(format!("deux {what} « {n} » (casse ignorée)")));
        }
    }
    Ok(())
}

fn limit(n: usize, max: usize, what: &str) -> Result<(), CoreError> {
    if n > max {
        Err(bad(format!("trop de {what} : {max} au plus")))
    } else {
        Ok(())
    }
}

impl Project {
    pub(crate) fn validate_measurements(&self) -> Result<(), CoreError> {
        limit(self.measurements.len(), MAX_MEASUREMENTS, "mesures")?;
        limit(self.clients.len(), MAX_CLIENTS, "fiches client")?;
        limit(self.alterations.len(), MAX_ALTERATIONS, "retouches")?;
        limit(
            self.finished_measures.len(),
            MAX_FINISHED_MEASURES,
            "mesures finies",
        )?;
        for m in &self.measurements {
            check_stored_name(&m.name, &format!("le nom de la mesure {}", m.id.0))?;
            let mut sizes = BTreeSet::new();
            for v in &m.values {
                if self.find_size(v.size).is_none() {
                    return Err(dead(format!(
                        "la mesure « {} » référence une taille absente (id {})",
                        m.name, v.size.0
                    )));
                }
                if !sizes.insert(v.size) {
                    return Err(bad(format!(
                        "la mesure « {} » a deux valeurs pour la taille {}",
                        m.name, v.size.0
                    )));
                }
                if !in_bounds(v.value, false) {
                    return Err(bad(format!(
                        "la mesure « {} » a une valeur non finie, négative ou hors bornes",
                        m.name
                    )));
                }
            }
        }
        unique_names(self.measurements.iter().map(|m| m.name.as_str()), "mesures")?;
        let measure = |id: MeasurementId| self.measurements.iter().find(|m| m.id == id);
        for c in &self.clients {
            check_stored_name(&c.name, &format!("le nom de la fiche client {}", c.id.0))?;
            let mut seen = BTreeSet::new();
            for v in &c.values {
                if measure(v.measurement).is_none() {
                    return Err(dead(format!(
                        "la fiche « {} » référence une mesure absente (id {})",
                        c.name, v.measurement.0
                    )));
                }
                if !seen.insert(v.measurement) {
                    return Err(bad(format!(
                        "la fiche « {} » a deux valeurs pour la mesure {}",
                        c.name, v.measurement.0
                    )));
                }
                if !in_bounds(v.value, false) {
                    return Err(bad(format!(
                        "la fiche « {} » a une valeur non finie, négative ou hors bornes",
                        c.name
                    )));
                }
            }
        }
        unique_names(
            self.clients.iter().map(|c| c.name.as_str()),
            "fiches client",
        )?;
        for a in &self.alterations {
            check_stored_name(&a.name, &format!("le nom de la retouche {}", a.id.0))?;
            if measure(a.target).is_none() {
                return Err(dead(format!(
                    "la retouche « {} » cible une mesure absente (id {})",
                    a.name, a.target.0
                )));
            }
            if !in_bounds(a.delta, true) {
                return Err(bad(format!(
                    "la retouche « {} » a un delta non fini ou hors bornes",
                    a.name
                )));
            }
        }
        unique_names(
            self.alterations.iter().map(|a| a.name.as_str()),
            "retouches",
        )?;
        for f in &self.finished_measures {
            check_stored_name(&f.name, &format!("le nom de la mesure finie {}", f.id.0))?;
            if measure(f.measurement).is_none() {
                return Err(dead(format!(
                    "la mesure finie « {} » référence une mesure absente (id {})",
                    f.name, f.measurement.0
                )));
            }
            match f.source {
                FinishedSource::Seam { seam } => {
                    if !self.seams.iter().any(|s| s.id == seam) {
                        return Err(dead(format!(
                            "la mesure finie « {} » référence une couture absente (id {})",
                            f.name, seam.0
                        )));
                    }
                }
                FinishedSource::FullDimension { sheet, dimension } => {
                    let found = self
                        .sheet(sheet)
                        .is_ok_and(|s| s.full_dimensions.iter().any(|d| d.id == dimension));
                    if !found {
                        return Err(dead(format!(
                            "la mesure finie « {} » référence une cote absente (feuille {}, id {})",
                            f.name, sheet.0, dimension.0
                        )));
                    }
                }
            }
            if !in_bounds(f.tolerance_minus, false) || !in_bounds(f.tolerance_plus, false) {
                return Err(bad(format!(
                    "la mesure finie « {} » a une tolérance non finie, négative ou hors bornes",
                    f.name
                )));
            }
        }
        unique_names(
            self.finished_measures.iter().map(|f| f.name.as_str()),
            "mesures finies",
        )
    }
}

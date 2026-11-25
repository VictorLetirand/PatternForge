//! Cotations : mesures affichées entre deux points (et non contraintes). La
//! valeur est recalculée à chaque lecture depuis les positions actuelles.

use crate::error::CoreError;
use crate::model::{Dimension, DimensionId, Sheet};
use pf_geometry::PointMm;

/// Décalage par défaut d'une cotation créée à la souris (mm).
pub const DEFAULT_DIMENSION_OFFSET_MM: f64 = 10.0;
/// Décalage maximal accepté, en valeur absolue (mm).
pub const MAX_DIMENSION_OFFSET_MM: f64 = 10_000.0;

pub(crate) fn check_offset(offset_mm: f64) -> Result<(), CoreError> {
    if !offset_mm.is_finite() || offset_mm.abs() > MAX_DIMENSION_OFFSET_MM {
        return Err(CoreError::Invalid(format!(
            "cotation : le décalage doit être un nombre d'au plus {MAX_DIMENSION_OFFSET_MM} mm en valeur absolue"
        )));
    }
    Ok(())
}

/// Géométrie calculée d'une cotation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DimensionGeometry {
    pub from: PointMm,
    pub to: PointMm,
    /// Extrémités de la ligne de cote (points décalés perpendiculairement).
    pub from_offset: PointMm,
    pub to_offset: PointMm,
    /// Distance actuelle entre les deux points (mm).
    pub value_mm: f64,
}

impl Sheet {
    pub fn dimension(&self, id: DimensionId) -> Result<&Dimension, CoreError> {
        self.dimensions
            .iter()
            .find(|d| d.id == id)
            .ok_or(CoreError::UnknownDimension(id))
    }

    /// Géométrie d'une cotation ; le décalage est signé, positif à gauche de from → to.
    pub fn dimension_geometry(&self, id: DimensionId) -> Result<DimensionGeometry, CoreError> {
        let d = self.dimension(id)?;
        let a = self.position(d.from)?;
        let b = self.position(d.to)?;
        let value_mm = pf_geometry::segment_length_mm(a, b)?;
        let n = (b - a).unit()?.rotated_left() * d.offset_mm;
        Ok(DimensionGeometry {
            from: a,
            to: b,
            from_offset: a + n,
            to_offset: b + n,
            value_mm,
        })
    }

    /// Géométries de toutes les cotations (celles dont le calcul échoue sont ignorées).
    pub fn all_dimension_geometries(&self) -> Vec<DimensionGeometry> {
        self.dimensions
            .iter()
            .filter_map(|d| self.dimension_geometry(d.id).ok())
            .collect()
    }
}

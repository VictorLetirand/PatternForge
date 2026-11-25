//! Mesures **observées** : lectures calculées à la demande à partir du modèle.
//! Elles ne sont ni stockées ni contraignantes, et ne dépendent jamais du zoom.

use crate::error::CoreError;
use crate::model::{EntityId, EntityKind, PointId, Sheet};
use pf_geometry::DEFAULT_LENGTH_TOLERANCE_MM;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeasurementKind {
    /// Longueur d'un segment (mm).
    SegmentLength,
    /// Longueur développée d'une courbe (mm).
    CurveLength,
    /// Longueur exacte d'un arc de cercle : rayon × balayage (mm).
    ArcLength,
    /// Circonférence exacte d'un cercle : 2πr (mm).
    CircleCircumference,
    /// Corde d'une courbe : distance droite entre ses extrémités (mm).
    CurveChord,
    /// Distance entre deux points (mm).
    Distance,
    /// Angle intérieur entre deux segments (degrés, 0–180).
    Angle,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measurement {
    pub kind: MeasurementKind,
    pub value: f64,
}

/// Longueur d'un tracé : segment → longueur, courbe → longueur développée.
pub fn entity_length(sheet: &Sheet, id: EntityId) -> Result<Measurement, CoreError> {
    match sheet.entity(id)?.kind {
        EntityKind::Segment { .. } => {
            let (a, b) = sheet.segment_ends(id)?;
            Ok(Measurement {
                kind: MeasurementKind::SegmentLength,
                value: pf_geometry::segment_length_mm(a, b)?,
            })
        }
        EntityKind::Curve { .. } => Ok(Measurement {
            kind: MeasurementKind::CurveLength,
            value: sheet
                .curve_geometry(id)?
                .length_mm(DEFAULT_LENGTH_TOLERANCE_MM)?,
        }),
        EntityKind::Arc { .. } => Ok(Measurement {
            kind: MeasurementKind::ArcLength,
            value: sheet.arc_geometry(id)?.length_mm(),
        }),
        EntityKind::Circle { .. } => Ok(Measurement {
            kind: MeasurementKind::CircleCircumference,
            value: sheet.circle_geometry(id)?.length_mm(),
        }),
        EntityKind::Spline { .. } => Ok(Measurement {
            kind: MeasurementKind::CurveLength,
            value: sheet.spline_geometry(id)?.length_mm()?,
        }),
    }
}

/// Longueur développée de chaque tronçon d'une courbe par points de passage
/// (mm), dans l'ordre des points de passage ; leur somme est la longueur totale.
pub fn spline_segment_lengths(sheet: &Sheet, id: EntityId) -> Result<Vec<f64>, CoreError> {
    Ok(sheet.spline_geometry(id)?.segment_lengths_mm()?)
}

pub fn curve_chord(sheet: &Sheet, id: EntityId) -> Result<Measurement, CoreError> {
    Ok(Measurement {
        kind: MeasurementKind::CurveChord,
        value: sheet.curve_geometry(id)?.chord_mm(),
    })
}

/// Angle de direction d'un segment (0–360°, sens anti-horaire depuis +X).
pub fn segment_direction_deg(sheet: &Sheet, id: EntityId) -> Result<f64, CoreError> {
    let (a, b) = sheet.segment_ends(id)?;
    Ok(pf_geometry::direction_angle_deg(a, b)?)
}

pub fn point_distance(sheet: &Sheet, a: PointId, b: PointId) -> Result<Measurement, CoreError> {
    Ok(Measurement {
        kind: MeasurementKind::Distance,
        value: pf_geometry::distance_mm(sheet.position(a)?, sheet.position(b)?),
    })
}

/// Angle intérieur (0–180°) entre deux segments.
///
/// S'ils partagent une extrémité, c'est l'angle au sommet commun. Sinon, c'est
/// l'angle entre leurs directions (de `from` vers `to`).
pub fn angle_between_segments(
    sheet: &Sheet,
    first: EntityId,
    second: EntityId,
) -> Result<Measurement, CoreError> {
    let (a1, b1) = sheet.segment_point_ids(first)?;
    let (a2, b2) = sheet.segment_point_ids(second)?;
    let pos = |id| sheet.position(id);
    let shared = [
        (a1, b1, a2, b2),
        (a1, b1, b2, a2),
        (b1, a1, a2, b2),
        (b1, a1, b2, a2),
    ]
    .into_iter()
    .find(|(v1, _, v2, _)| v1 == v2);
    let value = match shared {
        Some((vertex, o1, _, o2)) => {
            pf_geometry::angle_at_vertex_deg(pos(o1)?, pos(vertex)?, pos(o2)?)?
        }
        None => pf_geometry::interior_angle_deg(pos(b1)? - pos(a1)?, pos(b2)? - pos(a2)?)?,
    };
    Ok(Measurement {
        kind: MeasurementKind::Angle,
        value,
    })
}

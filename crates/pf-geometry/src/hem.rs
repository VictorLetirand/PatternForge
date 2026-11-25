//! Crans de rempli (US-05.3) : positions **calculées** le long d'un bord courbe.
//!
//! Un rempli (marge à rabattre, maroquinerie) sur un bord courbe porte des crans régulièrement
//! répartis. `fold_notches(edges, plan)` reçoit les arêtes élémentaires **consécutives** du bord,
//! dans l'ordre du parcours, et un [`FoldNotchPlan`] :
//!
//! - `Count(n)` : `n` crans, à `k · L / (n + 1)` du début (`k = 1..=n`) ;
//! - `Step(s)` : le bord est divisé en `m = ⌈L / s⌉` intervalles égaux (au moins 1, donc un pas
//!   réel `L / m ≤ s`), les crans sont aux `k · L / m` (`k = 1..m`) : aucun cran aux extrémités,
//!   aucun cran si le bord est plus court que le pas.
//!
//! Les positions sont des longueurs développées depuis le début du bord. Rien n'est stocké.

use crate::region::EdgeGeom;
use crate::{GeometryError, PointMm, Result, VectorMm, DEFAULT_LENGTH_TOLERANCE_MM, MAX_COORD_MM};

/// Plus grand nombre de crans de rempli sur un bord.
pub const MAX_FOLD_NOTCHES: usize = 1000;
/// Pas par défaut des crans de rempli d'un bord courbe (mm).
pub const DEFAULT_FOLD_NOTCH_STEP_MM: f64 = 25.0;
/// Bornes du pas des crans de rempli (mm).
pub const MIN_FOLD_NOTCH_STEP_MM: f64 = 1.0;
pub const MAX_FOLD_NOTCH_STEP_MM: f64 = 1000.0;

/// Répartition des crans de rempli.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FoldNotchPlan {
    /// Pas maximal entre deux crans (mm).
    Step(f64),
    /// Nombre de crans.
    Count(usize),
}

/// Un cran de rempli : longueur développée depuis le début, point du bord, normale extérieure
/// (à droite du sens de parcours d'un contour anti-horaire).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoldNotchPoint {
    pub distance_mm: f64,
    pub point: PointMm,
    pub normal: VectorMm,
}

fn err(msg: impl Into<String>) -> GeometryError {
    GeometryError::InvalidConstruction(msg.into())
}

/// Positions (mm depuis le début) des crans d'un bord de longueur `length_mm`.
pub fn fold_notch_distances(length_mm: f64, plan: FoldNotchPlan) -> Result<Vec<f64>> {
    if !length_mm.is_finite() || length_mm <= 0.0 {
        return Err(err("crans de rempli : le bord est de longueur nulle"));
    }
    let intervals = match plan {
        FoldNotchPlan::Count(n) => {
            if n == 0 || n > MAX_FOLD_NOTCHES {
                return Err(err(format!(
                    "crans de rempli : le nombre de crans doit être compris entre 1 et \
                     {MAX_FOLD_NOTCHES}"
                )));
            }
            n + 1
        }
        FoldNotchPlan::Step(s) => {
            if !s.is_finite() || !(MIN_FOLD_NOTCH_STEP_MM..=MAX_FOLD_NOTCH_STEP_MM).contains(&s) {
                return Err(err(format!(
                    "crans de rempli : le pas doit être compris entre {MIN_FOLD_NOTCH_STEP_MM} et \
                     {MAX_FOLD_NOTCH_STEP_MM} mm"
                )));
            }
            let m = ((length_mm / s) - 1e-9).ceil().max(1.0);
            if m - 1.0 > MAX_FOLD_NOTCHES as f64 {
                return Err(err(format!(
                    "crans de rempli : un pas de {s} mm sur {length_mm:.1} mm donnerait plus de \
                     {MAX_FOLD_NOTCHES} crans (augmentez le pas)"
                )));
            }
            m as usize
        }
    };
    Ok((1..intervals)
        .map(|k| length_mm * k as f64 / intervals as f64)
        .collect())
}

/// Point et direction de parcours à la distance `d` (mm, bornée à l'arête) du début d'une arête.
fn locate(edge: &EdgeGeom, d: f64) -> Result<(PointMm, VectorMm)> {
    match edge {
        EdgeGeom::Line(a, b) => {
            let u = (*b - *a).unit()?;
            Ok((*a + u * d, u))
        }
        EdgeGeom::Arc(a) => a.point_at_length(d),
        EdgeGeom::Cubic(c) => {
            let t = c.t_at_length_mm(d, DEFAULT_LENGTH_TOLERANCE_MM)?;
            Ok((c.point_at(t), c.unit_tangent_at(t)?))
        }
    }
}

/// Crans d'un bord fait d'arêtes consécutives (voir la documentation du module).
pub fn fold_notches(edges: &[EdgeGeom], plan: FoldNotchPlan) -> Result<Vec<FoldNotchPoint>> {
    let lengths: Vec<f64> = edges.iter().map(EdgeGeom::length_mm).collect();
    let total: f64 = lengths.iter().sum();
    let mut out = Vec::new();
    for d in fold_notch_distances(total, plan)? {
        let (mut start, mut found) = (0.0, None);
        for (i, (e, len)) in edges.iter().zip(&lengths).enumerate() {
            if d <= start + len || i + 1 == edges.len() {
                found = Some((e, (d - start).clamp(0.0, *len)));
                break;
            }
            start += len;
        }
        let Some((edge, local)) = found else {
            return Err(err("crans de rempli : bord vide"));
        };
        let (point, tangent) = locate(edge, local)?;
        if !point.x.is_finite()
            || !point.y.is_finite()
            || point.x.abs().max(point.y.abs()) > MAX_COORD_MM
        {
            return Err(err(
                "crans de rempli : position hors de l'espace de travail",
            ));
        }
        out.push(FoldNotchPoint {
            distance_mm: d,
            point,
            normal: VectorMm::new(tangent.y, -tangent.x),
        });
    }
    Ok(out)
}

#[cfg(test)]
#[path = "hem_tests.rs"]
mod tests;

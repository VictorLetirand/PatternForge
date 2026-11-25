//! E15 / US-15.5 — cuir : **diagnostics de la peau** sur un plan de coupe, calculés à la lecture,
//! jamais stockés. Module pur (aucune E/S).
//!
//! Conventions métier (décision 465) :
//! - une pose **touche un défaut** si l'aire d'intersection exacte (pièce ∩ défaut) dépasse
//!   [`OVERLAP_MIN_MM2`] ; un simple contact de bord n'est pas une collision ;
//! - une pose **déborde de la peau** si l'aire exacte de la pièce hors du contour dépasse
//!   [`OVERLAP_MIN_MM2`] (le débordement rapporté est cette aire) ;
//! - la **qualité d'une pose** est le **pire grade** (le plus grand nombre) des zones qu'elle
//!   recoupe (aire > [`OVERLAP_MIN_MM2`]) ; l'aire est rapportée par grade (somme des
//!   intersections avec les zones du grade : deux zones de même grade qui se recouvrent
//!   comptent deux fois) ; une aire hors de toute zone n'est pas notée ;
//! - la **surface utile** de la peau = contour − défauts (défauts qui se recouvrent : somme
//!   des aires, signalé par [`HideYield::defects_overlap`]).
//!
//! Avec une peau, la laize rectangulaire, les lisières, le pli et le début de l'étoffe ne
//! contraignent plus la pose : le contour de la peau les remplace (voir
//! `Project::analyze_cutting_plan`).

use crate::cutting_layout::{
    intersection_area_mm2, polygon_signed_area, PlanAnalysis, TriSet, OVERLAP_MIN_MM2,
};
use crate::hide::{DefectKind, Hide};
use crate::model::*;
use pf_geometry::PointMm;

/// Une pose qui touche un défaut.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DefectHit {
    pub placement: PlacementId,
    pub defect: HideDefectId,
    pub kind: DefectKind,
    /// Aire exacte de pièce ∩ défaut (mm²).
    pub area_mm2: f64,
}

/// Une pose qui sort du contour de la peau.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HideOverflow {
    pub placement: PlacementId,
    /// Aire exacte de la pièce hors du contour (mm²).
    pub area_mm2: f64,
}

/// Qualité d'une pose : pire grade et aires par grade (1, 2, 3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacementQuality {
    pub placement: PlacementId,
    /// Pire grade recoupé ; `None` si la pose ne recoupe aucune zone.
    pub worst_grade: Option<u8>,
    pub areas_mm2: [f64; 3],
}

/// Métrage cuir : surface posée / surface utile de la peau.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HideYield {
    pub contour_area_mm2: f64,
    pub defects_area_mm2: f64,
    /// Contour − défauts.
    pub useful_area_mm2: f64,
    pub used_area_mm2: f64,
    /// Surface posée / surface utile, en pourcent.
    pub percent: f64,
    /// Des défauts se recouvrent : la surface utile est alors sous-estimée.
    pub defects_overlap: bool,
}

/// Polygone anti-horaire en mm.
pub fn ccw(points: &[crate::PlanePoint]) -> Vec<PointMm> {
    let mut v: Vec<PointMm> = points.iter().map(|p| PointMm::new(p.x, p.y)).collect();
    if polygon_signed_area(&v) < 0.0 {
        v.reverse();
    }
    v
}

/// Aire d'un polygone simple (mm², positive).
pub fn polygon_area_abs(points: &[crate::PlanePoint]) -> f64 {
    polygon_signed_area(&ccw(points))
}

/// Diagnostics de la peau pour des contours posés (complète `an.defect_hits`,
/// `an.hide_overflows` et `an.qualities`).
pub(crate) fn analyze_hide(hide: &Hide, an: &mut PlanAnalysis) {
    let contour = TriSet::new(&ccw(&hide.contour));
    let defects: Vec<_> = hide
        .defects
        .iter()
        .map(|d| TriSet::new(&ccw(&d.polygon)))
        .collect();
    let zones: Vec<_> = hide
        .quality_zones
        .iter()
        .map(|z| TriSet::new(&ccw(&z.polygon)))
        .collect();
    let outlines = std::mem::take(&mut an.outlines);
    for o in &outlines {
        let me = TriSet::new(&o.polygon);
        let own = polygon_signed_area(&o.polygon);
        let inside = me.intersection_area(&contour);
        let out = (own - inside).max(0.0);
        if out > OVERLAP_MIN_MM2 {
            an.hide_overflows.push(HideOverflow {
                placement: o.placement,
                area_mm2: out,
            });
        }
        for (d, poly) in hide.defects.iter().zip(&defects) {
            let a = me.intersection_area(poly);
            if a > OVERLAP_MIN_MM2 {
                an.defect_hits.push(DefectHit {
                    placement: o.placement,
                    defect: d.id,
                    kind: d.kind,
                    area_mm2: a,
                });
            }
        }
        let mut areas = [0.0_f64; 3];
        let mut worst = None;
        for (z, poly) in hide.quality_zones.iter().zip(&zones) {
            let a = me.intersection_area(poly);
            if a > OVERLAP_MIN_MM2 && (1..=3).contains(&z.grade) {
                areas[usize::from(z.grade) - 1] += a;
                worst = Some(worst.map_or(z.grade, |w: u8| w.max(z.grade)));
            }
        }
        an.qualities.push(PlacementQuality {
            placement: o.placement,
            worst_grade: worst,
            areas_mm2: areas,
        });
    }
    an.outlines = outlines;
}

/// Poses dont la qualité est pire que l'exigence `required` (grade maximal accepté, 1 à 3).
pub fn quality_violations(an: &PlanAnalysis, required: u8) -> Vec<PlacementId> {
    an.qualities
        .iter()
        .filter(|q| q.worst_grade.is_some_and(|g| g > required))
        .map(|q| q.placement)
        .collect()
}

/// Métrage cuir ; `None` si le plan n'a pas de peau ou si ses diagnostics sont invalides
/// (aucun chiffre sur un plan invalide, comme le métrage de la laize).
pub fn hide_yield(hide: Option<&Hide>, an: &PlanAnalysis) -> Option<HideYield> {
    let h = hide?;
    if !an.is_valid() {
        return None;
    }
    let contour_area_mm2 = polygon_area_abs(&h.contour);
    let areas: Vec<f64> = h
        .defects
        .iter()
        .map(|d| polygon_area_abs(&d.polygon))
        .collect();
    // `+ 0.0` : la somme d'une liste vide vaut -0.0 (jamais affichée « -0 »).
    let defects_area_mm2: f64 = areas.iter().sum::<f64>() + 0.0;
    let polys: Vec<Vec<PointMm>> = h.defects.iter().map(|d| ccw(&d.polygon)).collect();
    let defects_overlap = (0..polys.len()).any(|i| {
        ((i + 1)..polys.len())
            .any(|j| intersection_area_mm2(&polys[i], &polys[j]) > OVERLAP_MIN_MM2)
    });
    let useful_area_mm2 = (contour_area_mm2 - defects_area_mm2).max(0.0);
    let used_area_mm2: f64 = an.outlines.iter().map(|o| o.area_mm2).sum::<f64>() + 0.0;
    let percent = if useful_area_mm2 > 0.0 {
        100.0 * used_area_mm2 / useful_area_mm2
    } else {
        0.0
    };
    Some(HideYield {
        contour_area_mm2,
        defects_area_mm2,
        useful_area_mm2,
        used_area_mm2,
        percent,
        defects_overlap,
    })
}

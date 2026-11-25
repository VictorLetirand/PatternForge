//! E15 — cuir : peau, défauts et zones de qualité d'un plan de coupe, **stockage seulement**
//! (schéma 6, décision 455).
//!
//! `CuttingPlan.hide` = contour de la peau (polygone fermé en mm, ≥ 3 sommets, simple), défauts
//! (`id` stable, genre, polygone, note facultative) et zones de qualité (`id`, `grade` de 1 à 3,
//! polygone). Aucune commande, aucune interface, aucun placement qui en tienne compte : la
//! validation décide seulement de ce qu'un fichier peut porter. Rien de dérivé n'est stocké.
//!
//! Le polygone est **fermé implicitement** (le dernier sommet rejoint le premier : ne pas répéter
//! le premier). Un défaut doit être contenu dans le contour (sommets dedans ou sur le bord, aucun
//! côté qui le traverse) ; une zone de qualité n'a pas cette contrainte.

use crate::error::CoreError;
use crate::model::*;
use crate::sheet_background::{PlanePoint, MAX_BACKGROUND_VALUE};
use pf_geometry::{is_simple_polygon, point_in_polygon, PointMm};
use serde::{Deserialize, Serialize};

/// Nombre maximal de défauts d'une peau.
pub const MAX_HIDE_DEFECTS: usize = 500;
/// Nombre maximal de zones de qualité d'une peau.
pub const MAX_QUALITY_ZONES: usize = 100;
/// Nombre maximal de sommets du contour.
pub const MAX_HIDE_CONTOUR_VERTICES: usize = 1000;
/// Nombre maximal de sommets d'un défaut ou d'une zone.
pub const MAX_HIDE_SHAPE_VERTICES: usize = 200;
/// Nombre maximal de sommets de tous les polygones d'une peau.
pub const MAX_HIDE_TOTAL_VERTICES: usize = 20_000;
/// Longueur maximale de la note d'un défaut, en caractères.
pub const MAX_DEFECT_NOTE_CHARS: usize = 500;

/// Genre d'un défaut (étiquette ; aucun calcul n'en dépend).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DefectKind {
    #[serde(rename = "hole")]
    Hole,
    #[serde(rename = "scar")]
    Scar,
    #[serde(rename = "stain")]
    Stain,
    #[serde(rename = "thin")]
    Thin,
    #[serde(rename = "other")]
    Other,
}

/// Défaut de la peau.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HideDefect {
    pub id: HideDefectId,
    pub kind: DefectKind,
    pub polygon: Vec<PlanePoint>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Zone de qualité : 1 (meilleure) à 3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityZone {
    pub id: QualityZoneId,
    pub grade: u8,
    pub polygon: Vec<PlanePoint>,
}

/// Peau posée sur un plan de coupe.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hide {
    pub contour: Vec<PlanePoint>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub defects: Vec<HideDefect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub quality_zones: Vec<QualityZone>,
}

fn bad(msg: String) -> CoreError {
    CoreError::Invalid(msg)
}

fn to_mm(polygon: &[PlanePoint]) -> Vec<PointMm> {
    polygon.iter().map(|p| PointMm { x: p.x, y: p.y }).collect()
}

fn orient(a: PointMm, b: PointMm, c: PointMm) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

/// Les deux segments se traversent strictement (en un point intérieur à chacun).
fn proper_crossing(a: PointMm, b: PointMm, c: PointMm, d: PointMm) -> bool {
    let (o1, o2) = (orient(a, b, c), orient(a, b, d));
    let (o3, o4) = (orient(c, d, a), orient(c, d, b));
    ((o1 > 0.0 && o2 < 0.0) || (o1 < 0.0 && o2 > 0.0))
        && ((o3 > 0.0 && o4 < 0.0) || (o3 < 0.0 && o4 > 0.0))
}

/// Polygone `inner` contenu dans `outer` (bord compris) : sommets dedans, aucun côté qui traverse.
pub fn polygon_inside(inner: &[PointMm], outer: &[PointMm]) -> bool {
    if inner.iter().any(|p| !point_in_polygon(*p, outer)) {
        return false;
    }
    let (n, m) = (inner.len(), outer.len());
    (0..n).all(|i| {
        let (a, b) = (inner[i], inner[(i + 1) % n]);
        (0..m).all(|j| !proper_crossing(a, b, outer[j], outer[(j + 1) % m]))
    })
}

fn check_polygon(
    polygon: &[PlanePoint],
    max: usize,
    what: &str,
) -> Result<Vec<PointMm>, CoreError> {
    if !(3..=max).contains(&polygon.len()) {
        return Err(bad(format!("{what} : un polygone a de 3 à {max} sommets")));
    }
    if polygon.iter().any(|p| !p.x.is_finite() || !p.y.is_finite())
        || polygon
            .iter()
            .any(|p| p.x.abs() > MAX_BACKGROUND_VALUE || p.y.abs() > MAX_BACKGROUND_VALUE)
    {
        return Err(bad(format!(
            "{what} : coordonnée non finie ou hors de ±{MAX_BACKGROUND_VALUE} mm"
        )));
    }
    let mm = to_mm(polygon);
    if !is_simple_polygon(&mm) {
        return Err(bad(format!(
            "{what} : le polygone n'est pas simple (côtés qui se croisent ou se touchent, sommets \
             confondus, sommet de fermeture répété)"
        )));
    }
    Ok(mm)
}

impl Hide {
    /// Contrôle complet de la peau ; `what` nomme le plan dans les messages.
    pub fn validate(&self, what: &str) -> Result<(), CoreError> {
        let what = format!("{what} : peau");
        let contour = check_polygon(
            &self.contour,
            MAX_HIDE_CONTOUR_VERTICES,
            &format!("{what}, contour"),
        )?;
        if self.defects.len() > MAX_HIDE_DEFECTS {
            return Err(bad(format!(
                "{what} : trop de défauts ({MAX_HIDE_DEFECTS} au plus)"
            )));
        }
        if self.quality_zones.len() > MAX_QUALITY_ZONES {
            return Err(bad(format!(
                "{what} : trop de zones de qualité ({MAX_QUALITY_ZONES} au plus)"
            )));
        }
        let total = self.contour.len()
            + self.defects.iter().map(|d| d.polygon.len()).sum::<usize>()
            + self
                .quality_zones
                .iter()
                .map(|z| z.polygon.len())
                .sum::<usize>();
        if total > MAX_HIDE_TOTAL_VERTICES {
            return Err(bad(format!(
                "{what} : trop de sommets en tout ({MAX_HIDE_TOTAL_VERTICES} au plus)"
            )));
        }
        for d in &self.defects {
            let w = format!("{what}, défaut {}", d.id.0);
            let poly = check_polygon(&d.polygon, MAX_HIDE_SHAPE_VERTICES, &w)?;
            if let Some(n) = &d.note {
                if n.trim().is_empty()
                    || n.trim() != n
                    || n.chars().count() > MAX_DEFECT_NOTE_CHARS
                    || n.chars().any(char::is_control)
                {
                    return Err(bad(format!(
                        "{w} : la note doit être non vide, sans espaces en bordure ni caractère \
                         de contrôle, de {MAX_DEFECT_NOTE_CHARS} caractères au plus"
                    )));
                }
            }
            if !polygon_inside(&poly, &contour) {
                return Err(bad(format!("{w} : le défaut sort du contour de la peau")));
            }
        }
        for z in &self.quality_zones {
            let w = format!("{what}, zone de qualité {}", z.id.0);
            if !(1..=3).contains(&z.grade) {
                return Err(bad(format!(
                    "{w} : la note de qualité doit être 1, 2 ou 3 (reçu {})",
                    z.grade
                )));
            }
            check_polygon(&z.polygon, MAX_HIDE_SHAPE_VERTICES, &w)?;
        }
        Ok(())
    }
}

impl Project {
    /// Validation des peaux (appelée par `Project::validate`).
    pub(crate) fn validate_hides(&self) -> Result<(), CoreError> {
        for c in &self.cutting_plans {
            if let Some(h) = &c.hide {
                h.validate(&format!("plan de coupe « {} »", c.name))?;
            }
        }
        Ok(())
    }
}

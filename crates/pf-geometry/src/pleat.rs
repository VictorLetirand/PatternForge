//! US-07.3 : plis. Écarter un contour le long d'une ligne de pli, et le refermer.
//!
//! `spread_chain` coupe une chaîne fermée par la droite infinie `a → b` (exactement deux
//! points de coupe, voir `split::cut_chain_by_line`) et **écarte** la moitié du côté `side`
//! d'une `width` perpendiculaire à la droite : les deux arêtes de raccord sont des segments
//! perpendiculaires à la ligne, donc l'aire gagne exactement `width × (longueur de la corde)`.
//! `fold_chain` fait l'inverse : il supprime la bande comprise entre la droite et sa parallèle
//! à `width` et ramène la moitié éloignée contre l'autre. Tout est en mm, sans état ; les
//! arêtes sont déplacées par translation (aucune approximation).

use crate::region::EdgeGeom;
use crate::split::{cut_chain_by_line, CutEdge, CutError};
use crate::{Arc, CubicBezier, PointMm, Side, VectorMm, EPSILON_MM};
use std::fmt;

/// Plus petite largeur de bande acceptée (mm).
pub const PLEAT_MIN_WIDTH_MM: f64 = 0.1;
/// Plus grande largeur de bande acceptée (mm).
pub const PLEAT_MAX_WIDTH_MM: f64 = 2000.0;
/// Écart maximal (mm) de fermeture au repliage : au-delà, le contour ne correspond plus au pli.
pub const PLEAT_CLOSE_TOLERANCE_MM: f64 = 1e-6;

/// Pourquoi un pli ne peut pas être posé ou refermé.
#[derive(Debug, Clone, PartialEq)]
pub enum PleatError {
    /// Les deux points de la ligne sont confondus ou non finis.
    BadLine,
    /// Largeur de bande non finie ou hors bornes.
    BadWidth(f64),
    /// La coupe du contour par la ligne est impossible (raison).
    Cut(CutError),
    /// Le contour ne correspond pas au pli (raison).
    Mismatch(String),
}

impl fmt::Display for PleatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PleatError::BadLine => write!(f, "la ligne de pli est dégénérée : points confondus"),
            PleatError::BadWidth(w) => write!(
                f,
                "la largeur du pli ({w}) doit être un nombre fini compris entre \
                 {PLEAT_MIN_WIDTH_MM} et {PLEAT_MAX_WIDTH_MM} mm"
            ),
            PleatError::Cut(e) => write!(f, "{e}"),
            PleatError::Mismatch(why) => write!(f, "le contour ne correspond pas au pli : {why}"),
        }
    }
}

impl std::error::Error for PleatError {}

impl From<CutError> for PleatError {
    fn from(e: CutError) -> Self {
        PleatError::Cut(e)
    }
}

/// Normale unitaire de la droite `a → b` du côté `side` (gauche = +90°).
pub fn pleat_normal(a: PointMm, b: PointMm, side: Side) -> Result<VectorMm, PleatError> {
    let u = (b - a).unit().map_err(|_| PleatError::BadLine)?;
    let left = u.rotated_left();
    Ok(match side {
        Side::Left => left,
        Side::Right => left * -1.0,
    })
}

/// Distance signée de `p` à la droite `a → b`, positive du côté `side`.
pub fn side_offset_mm(p: PointMm, a: PointMm, b: PointMm, side: Side) -> Result<f64, PleatError> {
    Ok(pleat_normal(a, b, side)?.dot(p - a))
}

fn check_width(width: f64) -> Result<(), PleatError> {
    if !width.is_finite() || !(PLEAT_MIN_WIDTH_MM..=PLEAT_MAX_WIDTH_MM).contains(&width) {
        return Err(PleatError::BadWidth(width));
    }
    Ok(())
}

/// Arête déplacée de `v`.
pub fn translate_edge(e: &EdgeGeom, v: VectorMm) -> EdgeGeom {
    match e {
        EdgeGeom::Line(a, b) => EdgeGeom::Line(*a + v, *b + v),
        EdgeGeom::Cubic(c) => {
            EdgeGeom::Cubic(CubicBezier::new(c.p0 + v, c.c1 + v, c.c2 + v, c.p3 + v))
        }
        EdgeGeom::Arc(a) => EdgeGeom::Arc(Arc {
            center: a.center + v,
            radius_mm: a.radius_mm,
            start_rad: a.start_rad,
            sweep_rad: a.sweep_rad,
        }),
    }
}

pub(crate) fn run_without_cut(half: Vec<CutEdge>) -> Vec<CutEdge> {
    let mut h = half;
    h.pop();
    h
}

/// Résultat de `spread_chain`.
#[derive(Debug, Clone, PartialEq)]
pub struct Spread {
    /// Nouveau contour (même sens que l'entrée) : `source` = arête d'origine, `None` pour les
    /// deux arêtes de raccord.
    pub edges: Vec<CutEdge>,
    /// Moitié déplacée, **avant** déplacement, fermée par son arête de coupe.
    pub moving: Vec<EdgeGeom>,
    /// Longueur de la corde de la ligne dans le contour d'origine (mm).
    pub chord_mm: f64,
}

/// Écarte la chaîne fermée `chain` de `width` le long de la droite `a → b` : la moitié du côté
/// `side` est translatée de `width` perpendiculairement à la droite, l'autre reste en place.
pub fn spread_chain(
    chain: &[EdgeGeom],
    a: PointMm,
    b: PointMm,
    side: Side,
    width: f64,
) -> Result<Spread, PleatError> {
    check_width(width)?;
    let shift = pleat_normal(a, b, side)? * width;
    let cut = cut_chain_by_line(chain, a, b)?;
    let chord_mm = crate::distance_mm(cut.points[0], cut.points[1]);
    let (moving_half, stay_half) = match side {
        Side::Left => (cut.left, cut.right),
        Side::Right => (cut.right, cut.left),
    };
    let moving: Vec<EdgeGeom> = moving_half.iter().map(|e| e.geom).collect();
    let (stay, moved) = (run_without_cut(stay_half), run_without_cut(moving_half));
    let (Some(stay_last), Some(moved_last)) = (stay.last(), moved.last()) else {
        return Err(PleatError::Mismatch("moitié vide".into()));
    };
    let (e1, e2) = (stay_last.geom.end(), moved_last.geom.end());
    let mut edges = stay;
    edges.push(CutEdge {
        geom: EdgeGeom::Line(e1, e1 + shift),
        source: None,
    });
    edges.extend(moved.iter().map(|e| CutEdge {
        geom: translate_edge(&e.geom, shift),
        source: e.source,
    }));
    edges.push(CutEdge {
        geom: EdgeGeom::Line(e2 + shift, e2),
        source: None,
    });
    Ok(Spread {
        edges,
        moving,
        chord_mm,
    })
}

/// Supprime la bande de largeur `width` qui commence à la droite `a → b` et s'étend du côté
/// `side`, et ramène la partie éloignée contre l'autre. Refus si la chaîne n'est pas coupée
/// proprement par la droite et sa parallèle, ou si les deux morceaux ne se rejoignent pas à
/// `PLEAT_CLOSE_TOLERANCE_MM` près.
pub fn fold_chain(
    chain: &[EdgeGeom],
    a: PointMm,
    b: PointMm,
    side: Side,
    width: f64,
) -> Result<Vec<EdgeGeom>, PleatError> {
    check_width(width)?;
    let shift = pleat_normal(a, b, side)? * width;
    let first = cut_chain_by_line(chain, a, b)?;
    let (far, stay) = match side {
        Side::Left => (first.left, first.right),
        Side::Right => (first.right, first.left),
    };
    let far_chain: Vec<EdgeGeom> = far.iter().map(|e| e.geom).collect();
    let second = cut_chain_by_line(&far_chain, a + shift, b + shift)?;
    let beyond = match side {
        Side::Left => second.left,
        Side::Right => second.right,
    };
    let back = shift * -1.0;
    let stay: Vec<EdgeGeom> = run_without_cut(stay).into_iter().map(|e| e.geom).collect();
    let moved: Vec<EdgeGeom> = run_without_cut(beyond)
        .into_iter()
        .map(|e| translate_edge(&e.geom, back))
        .collect();
    let (Some(s0), Some(s1), Some(m0), Some(m1)) =
        (stay.first(), stay.last(), moved.first(), moved.last())
    else {
        return Err(PleatError::Mismatch("moitié vide".into()));
    };
    for (p, q, name) in [
        (s1.end(), m0.start(), "premier raccord"),
        (m1.end(), s0.start(), "second raccord"),
    ] {
        let gap = crate::distance_mm(p, q);
        if gap > PLEAT_CLOSE_TOLERANCE_MM {
            return Err(PleatError::Mismatch(format!(
                "le {name} est à {gap:.6} mm (tolérance {PLEAT_CLOSE_TOLERANCE_MM} mm)"
            )));
        }
    }
    let mut out = stay;
    out.extend(moved);
    Ok(out)
}

/// Fusionne les segments consécutifs alignés (même sens) qui portent la même étiquette ; la
/// chaîne est cyclique. Garde au moins trois arêtes.
pub fn merge_collinear<T: PartialEq>(mut edges: Vec<(EdgeGeom, T)>) -> Vec<(EdgeGeom, T)> {
    loop {
        let n = edges.len();
        if n <= 3 {
            return edges;
        }
        let mut merged = false;
        for i in 0..n {
            let j = (i + 1) % n;
            if let (EdgeGeom::Line(p0, p1), EdgeGeom::Line(q0, q1)) = (edges[i].0, edges[j].0) {
                if edges[i].1 == edges[j].1 && aligned(p0, p1, q0, q1) {
                    edges[i].0 = EdgeGeom::Line(p0, q1);
                    edges.remove(j);
                    merged = true;
                    break;
                }
            }
        }
        if !merged {
            return edges;
        }
    }
}

fn aligned(p0: PointMm, p1: PointMm, q0: PointMm, q1: PointMm) -> bool {
    let (u, v) = (p1 - p0, q1 - q0);
    let (nu, nv) = (u.norm(), v.norm());
    if nu <= EPSILON_MM || nv <= EPSILON_MM {
        return false;
    }
    u.dot(v) > 0.0
        && u.cross(v).abs() <= 1e-9 * nu * nv
        && u.cross(q1 - p0).abs() / nu <= 1e-7
        && crate::distance_mm(p1, q0) <= 1e-6
}

#[cfg(test)]
#[path = "pleat_tests.rs"]
mod tests;

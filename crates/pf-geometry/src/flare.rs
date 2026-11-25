//! US-07.4 : ampleur par ouverture en éventail.
//!
//! `fan_chain` coupe une chaîne fermée par la droite perpendiculaire au bord fixe qui passe par un
//! point du bord (le **pivot**), puis **tourne** autour du pivot la moitié située au-delà de la
//! droite (côté `+dir` du bord fixe) pour écarter le haut de la coupe de `opening_mm` (corde entre
//! les deux copies de l'extrémité haute de la ligne). Le pivot reste commun aux deux moitiés : le
//! bord fixe garde sa longueur ; chaque moitié garde sa forme (corps rigide), donc la longueur de la
//! ligne de coupe est conservée. Le vide entre les deux lèvres est **rempli** : le contour est
//! refermé par la corde de l'ouverture (la pièce élargie contient le coin de tissu ajouté), de sorte
//! que le périmètre gagne exactement `opening_mm` et l'aire `½ L² sin θ`.
//!
//! Tout est en mm, sans état ; les arêtes sont tournées exactement (segment, Bézier, arc).

use crate::dart::{rotate_edge, rotate_point};
use crate::pleat::run_without_cut;
use crate::region::EdgeGeom;
use crate::split::{cut_chain_by_line, edge_sample, CutEdge, CutError};
use crate::{distance_mm, PointMm, VectorMm, EPSILON_MM};
use std::fmt;

/// Écart maximal (mm) entre le pivot demandé et le point où la droite rencontre le contour.
pub const FLARE_HINGE_TOLERANCE_MM: f64 = 1e-6;

/// Pourquoi une ouverture ne peut pas être faite.
#[derive(Debug, Clone, PartialEq)]
pub enum FlareError {
    /// Direction du bord fixe dégénérée.
    BadLine,
    /// Ouverture non finie, nulle ou plus grande que `√2 × longueur de la ligne` (rotation > 90°).
    BadOpening(f64),
    /// La coupe du contour par la droite est impossible (raison).
    Cut(CutError),
    /// Le pivot n'est pas sur le contour, ou le contour est incohérent (raison).
    Mismatch(String),
}

impl fmt::Display for FlareError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FlareError::BadLine => write!(f, "le bord fixe est dégénéré : points confondus"),
            FlareError::BadOpening(o) => write!(
                f,
                "l'ouverture ({o}) doit être un nombre fini positif et ne pas dépasser 1,414 fois \
                 la longueur de la ligne de coupe (rotation de 90° au plus)"
            ),
            FlareError::Cut(e) => write!(f, "{e}"),
            FlareError::Mismatch(why) => write!(f, "ouverture impossible : {why}"),
        }
    }
}

impl std::error::Error for FlareError {}

impl From<CutError> for FlareError {
    fn from(e: CutError) -> Self {
        FlareError::Cut(e)
    }
}

/// Résultat de `fan_chain`.
#[derive(Debug, Clone, PartialEq)]
pub struct Fan {
    /// Nouveau contour (même sens que l'entrée) : `source` = arête d'origine, `None` pour la corde
    /// de remplissage.
    pub edges: Vec<CutEdge>,
    /// Moitié tournée, **avant** rotation, fermée par son arête de coupe.
    pub moving: Vec<EdgeGeom>,
    /// Pivot (point du contour où passe la droite).
    pub hinge: PointMm,
    /// Extrémité haute de la ligne côté fixe.
    pub stay_end: PointMm,
    /// Extrémité haute de la ligne côté tourné (après rotation).
    pub moved_end: PointMm,
    /// Longueur de la ligne de coupe (mm), conservée.
    pub length_mm: f64,
    /// Angle d'ouverture (rad, positif).
    pub angle_rad: f64,
    /// Rotation appliquée à la moitié tournée : cosinus et sinus (anti-horaire positif).
    pub cos: f64,
    pub sin: f64,
}

/// Ouvre la chaîne fermée `chain` en éventail à `hinge`, sur la droite perpendiculaire à `dir`
/// (vecteur **unitaire** du bord fixe). La moitié du côté `+dir` tourne autour du pivot d'un angle
/// tel que la corde entre les deux extrémités hautes de la ligne vaille `opening_mm`.
pub fn fan_chain(
    chain: &[EdgeGeom],
    hinge: PointMm,
    dir: VectorMm,
    opening_mm: f64,
) -> Result<Fan, FlareError> {
    let dir = dir.unit().map_err(|_| FlareError::BadLine)?;
    if !opening_mm.is_finite() || opening_mm <= 0.0 {
        return Err(FlareError::BadOpening(opening_mm));
    }
    let p = dir.rotated_left();
    let cut = cut_chain_by_line(chain, hinge, hinge + p)?;
    let [q0, q1] = cut.points;
    let (h, t) = if distance_mm(q0, hinge) <= distance_mm(q1, hinge) {
        (q0, q1)
    } else {
        (q1, q0)
    };
    if distance_mm(h, hinge) > FLARE_HINGE_TOLERANCE_MM {
        return Err(FlareError::Mismatch(
            "le pivot n'est pas sur le contour de la pièce".into(),
        ));
    }
    let length = distance_mm(h, t);
    if length <= EPSILON_MM {
        return Err(FlareError::Mismatch("la ligne de coupe est nulle".into()));
    }
    let half_sin = opening_mm / (2.0 * length);
    if half_sin > std::f64::consts::FRAC_1_SQRT_2 + 1e-12 {
        return Err(FlareError::BadOpening(opening_mm));
    }
    let theta = 2.0 * half_sin.min(1.0).asin();
    // L'intérieur est à gauche du bord (`p` vers le haut de la ligne) : la moitié `+dir` tourne
    // dans le sens horaire pour que le haut s'écarte ; sinon dans le sens anti-horaire.
    let up = p.dot(t - h) > 0.0;
    let phi = if up { -theta } else { theta };
    let (sin, cos) = phi.sin_cos();
    // Droite orientée `hinge → hinge + p` : sa gauche est le côté `-dir` (reste en place), sa
    // droite le côté `+dir` (tourne).
    let (moving_half, stay_half) = (cut.right, cut.left);
    let moving: Vec<EdgeGeom> = moving_half.iter().map(|e| e.geom).collect();
    let (stay, moved) = (run_without_cut(stay_half), run_without_cut(moving_half));
    let (Some(stay_last), Some(moved_last)) = (stay.last(), moved.last()) else {
        return Err(FlareError::Mismatch("moitié vide".into()));
    };
    let rot_p = |q: PointMm| rotate_point(q, h, cos, sin);
    let (e1, e2) = (stay_last.geom.end(), moved_last.geom.end());
    let mut edges = stay;
    if distance_mm(e1, rot_p(e1)) > EPSILON_MM {
        edges.push(CutEdge {
            geom: EdgeGeom::Line(e1, rot_p(e1)),
            source: None,
        });
    }
    edges.extend(moved.iter().map(|e| CutEdge {
        geom: rotate_edge(&e.geom, h, cos, sin),
        source: e.source,
    }));
    if distance_mm(rot_p(e2), e2) > EPSILON_MM {
        edges.push(CutEdge {
            geom: EdgeGeom::Line(rot_p(e2), e2),
            source: None,
        });
    }
    Ok(Fan {
        edges,
        moving,
        hinge: h,
        stay_end: t,
        moved_end: rot_p(t),
        length_mm: length,
        angle_rad: theta,
        cos,
        sin,
    })
}

/// Vrai si le contour fermé se recoupe ou se touche lui-même (arêtes non voisines), en échantillonnant
/// les courbes. Sert à refuser une ouverture dont les morceaux se chevauchent.
pub fn chain_self_touches(chain: &[EdgeGeom]) -> bool {
    let mut pts: Vec<PointMm> = Vec::new();
    for e in chain {
        let s = edge_sample(e, 24);
        pts.extend(s.iter().take(s.len().saturating_sub(1)).copied());
    }
    let n = pts.len();
    if n < 3 {
        return true;
    }
    let seg = |i: usize| (pts[i], pts[(i + 1) % n]);
    for i in 0..n {
        let (a, b) = seg(i);
        let (lo_x, hi_x) = (a.x.min(b.x) - 1e-7, a.x.max(b.x) + 1e-7);
        let (lo_y, hi_y) = (a.y.min(b.y) - 1e-7, a.y.max(b.y) + 1e-7);
        for j in (i + 2)..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            let (c, d) = seg(j);
            if c.x.min(d.x) > hi_x
                || c.x.max(d.x) < lo_x
                || c.y.min(d.y) > hi_y
                || c.y.max(d.y) < lo_y
            {
                continue;
            }
            if segments_touch(a, b, c, d) {
                return true;
            }
        }
    }
    false
}

fn segments_touch(a: PointMm, b: PointMm, c: PointMm, d: PointMm) -> bool {
    let o1 = (b - a).cross(c - a);
    let o2 = (b - a).cross(d - a);
    let o3 = (d - c).cross(a - c);
    let o4 = (d - c).cross(b - c);
    if o1 * o2 < 0.0 && o3 * o4 < 0.0 {
        return true;
    }
    [(c, a, b), (d, a, b), (a, c, d), (b, c, d)]
        .into_iter()
        .any(|(q, s0, s1)| point_segment_distance(q, s0, s1) < 1e-7)
}

fn point_segment_distance(q: PointMm, a: PointMm, b: PointMm) -> f64 {
    let ab = b - a;
    let len2 = ab.dot(ab);
    if len2 <= 0.0 {
        return distance_mm(q, a);
    }
    let t = ((q - a).dot(ab) / len2).clamp(0.0, 1.0);
    distance_mm(q, a + ab * t)
}

#[cfg(test)]
#[path = "flare_tests.rs"]
mod tests;

//! US-04.7 — dépliage d'une moitié construite sur une pliure.
//!
//! Une chaîne fermée **anti-horaire** dont une arête rectiligne est la pliure donne une
//! chaîne fermée entière : la moitié (la chaîne sans la pliure) suivie de son image en
//! miroir par rapport à la pliure, parcourue en sens inverse. Tout est exact : les
//! sommets sont réfléchis par la formule de la symétrie axiale, les poignées de Bézier
//! sont réfléchies point par point, un arc garde centre et rayon réfléchis et un balayage
//! de signe opposé. Les extrémités de la pliure sont communes aux deux moitiés.
//!
//! La pliure est la droite **infinie** qui porte l'arête désignée (prolongée aux arêtes
//! consécutives alignées à 1e-6 mm près). Toute la moitié doit être du même côté de
//! cette droite (à 1e-6 mm près) et ne la toucher qu'aux deux extrémités de la pliure.

use crate::region::EdgeGeom;
use crate::split::chain_signed_area_mm2;
use crate::{distance_mm, Arc, CubicBezier, PointMm, VectorMm};
use std::f64::consts::{FRAC_PI_2, TAU};
use std::fmt;

/// Tolérance (mm) de position par rapport à la pliure : alignement, côté, contact.
pub const FOLD_TOLERANCE_MM: f64 = 1e-6;
/// Plus courte pliure acceptée (mm).
pub const MIN_FOLD_LENGTH_MM: f64 = 1e-3;
/// Aire minimale (mm²) de la moitié : en dessous, il n'y a pas de moitié.
const MIN_HALF_AREA_MM2: f64 = 1e-9;

/// Pourquoi une chaîne ne se déplie pas.
#[derive(Debug, Clone, PartialEq)]
pub enum FoldError {
    /// L'arête désignée n'existe pas dans la chaîne.
    NoSuchEdge,
    /// L'arête désignée n'est pas un segment.
    NotStraight,
    /// La pliure est plus courte que [`MIN_FOLD_LENGTH_MM`].
    TooShort { length_mm: f64 },
    /// Toutes les arêtes sont alignées sur la pliure, ou l'aire est nulle ou négative
    /// (chaîne horaire) : il n'y a pas de moitié.
    NoHalf,
    /// L'arête (rang dans la chaîne d'origine) passe de l'autre côté de la pliure.
    Crosses { edge: usize },
    /// L'arête (rang dans la chaîne d'origine) touche la pliure ailleurs qu'à ses
    /// extrémités, ou la longe en plusieurs morceaux.
    Touches { edge: usize },
}

impl fmt::Display for FoldError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FoldError::NoSuchEdge => write!(f, "la pliure désignée n'est pas dans le contour"),
            FoldError::NotStraight => write!(f, "la pliure doit être un segment rectiligne"),
            FoldError::TooShort { length_mm } => write!(
                f,
                "la pliure est trop courte ({length_mm:.6} mm, au moins {MIN_FOLD_LENGTH_MM} mm)"
            ),
            FoldError::NoHalf => write!(
                f,
                "le contour est aligné sur la pliure ou d'aire nulle : il n'y a pas de moitié à déplier"
            ),
            FoldError::Crosses { edge } => write!(
                f,
                "le contour traverse la droite de pliure (tracé n° {} du contour) : la moitié \
                 doit être entièrement d'un seul côté",
                edge + 1
            ),
            FoldError::Touches { edge } => write!(
                f,
                "le contour touche la droite de pliure ailleurs qu'à ses extrémités (tracé n° {} \
                 du contour) : la moitié ne doit la rejoindre qu'en un seul morceau",
                edge + 1
            ),
        }
    }
}

impl std::error::Error for FoldError {}

/// Droite de pliure : un point et une direction unitaire.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoldAxis {
    pub origin: PointMm,
    pub dir: VectorMm,
}

impl FoldAxis {
    /// Droite orientée `a → b` (refusée si les deux points sont confondus).
    pub fn through(a: PointMm, b: PointMm) -> Result<FoldAxis, FoldError> {
        let dir = (b - a).unit().map_err(|_| FoldError::TooShort {
            length_mm: distance_mm(a, b),
        })?;
        Ok(FoldAxis { origin: a, dir })
    }

    /// Distance signée d'un point à la droite : positive à gauche de `a → b`.
    pub fn signed_distance(&self, p: PointMm) -> f64 {
        self.dir.cross(p - self.origin)
    }

    /// Image d'un point par la symétrie axiale.
    pub fn reflect_point(&self, p: PointMm) -> PointMm {
        let v = p - self.origin;
        let k = 2.0 * self.dir.dot(v);
        PointMm::new(
            self.origin.x + self.dir.x * k - v.x,
            self.origin.y + self.dir.y * k - v.y,
        )
    }

    fn reflect_vector(&self, v: VectorMm) -> VectorMm {
        let k = 2.0 * self.dir.dot(v);
        VectorMm::new(self.dir.x * k - v.x, self.dir.y * k - v.y)
    }

    /// Image d'une arête : même sens de parcours (donc orientation du contour inversée).
    pub fn reflect_edge(&self, e: &EdgeGeom) -> EdgeGeom {
        match e {
            EdgeGeom::Line(a, b) => EdgeGeom::Line(self.reflect_point(*a), self.reflect_point(*b)),
            EdgeGeom::Cubic(c) => EdgeGeom::Cubic(CubicBezier::new(
                self.reflect_point(c.p0),
                self.reflect_point(c.c1),
                self.reflect_point(c.c2),
                self.reflect_point(c.p3),
            )),
            EdgeGeom::Arc(a) => {
                let r = self.reflect_vector(VectorMm::new(a.start_rad.cos(), a.start_rad.sin()));
                EdgeGeom::Arc(Arc {
                    center: self.reflect_point(a.center),
                    radius_mm: a.radius_mm,
                    start_rad: r.y.atan2(r.x),
                    sweep_rad: -a.sweep_rad,
                })
            }
        }
    }
}

/// Distances signées d'une arête à la droite : extrémités et plus petit extremum
/// strictement intérieur (`None` s'il n'y en a pas).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SideRange {
    pub start: f64,
    pub end: f64,
    pub interior_extremum_min: Option<f64>,
}

impl SideRange {
    /// Plus petite distance signée atteinte par l'arête.
    pub fn min(&self) -> f64 {
        self.start
            .min(self.end)
            .min(self.interior_extremum_min.unwrap_or(f64::INFINITY))
    }
}

/// Étendue de la distance signée d'une arête à la droite (exacte : extrema de la
/// cubique de Bernstein, ou du sinus d'un arc).
pub fn edge_side_range(e: &EdgeGeom, axis: &FoldAxis) -> SideRange {
    match e {
        EdgeGeom::Line(a, b) => SideRange {
            start: axis.signed_distance(*a),
            end: axis.signed_distance(*b),
            interior_extremum_min: None,
        },
        EdgeGeom::Cubic(c) => {
            let d = [c.p0, c.c1, c.c2, c.p3].map(|p| axis.signed_distance(p));
            let at = |t: f64| {
                let u = 1.0 - t;
                u * u * u * d[0]
                    + 3.0 * u * u * t * d[1]
                    + 3.0 * u * t * t * d[2]
                    + t * t * t * d[3]
            };
            // d'(t) / 3 = q2 t² + q1 t + q0.
            let (a, b, cc) = (d[1] - d[0], d[2] - d[1], d[3] - d[2]);
            let (q2, q1, q0) = (a - 2.0 * b + cc, 2.0 * (b - a), a);
            let scale = a.abs().max(b.abs()).max(cc.abs()).max(1.0);
            let mut roots: Vec<f64> = Vec::new();
            if q2.abs() <= 1e-12 * scale {
                if q1.abs() > 1e-12 * scale {
                    roots.push(-q0 / q1);
                }
            } else {
                let disc = q1 * q1 - 4.0 * q2 * q0;
                if disc >= 0.0 {
                    let s = disc.sqrt();
                    roots.push((-q1 - s) / (2.0 * q2));
                    roots.push((-q1 + s) / (2.0 * q2));
                }
            }
            let interior = roots
                .into_iter()
                .filter(|t| *t > 1e-9 && *t < 1.0 - 1e-9)
                .map(at)
                .fold(None, |m: Option<f64>, v| Some(m.map_or(v, |x| x.min(v))));
            SideRange {
                start: d[0],
                end: d[3],
                interior_extremum_min: interior,
            }
        }
        EdgeGeom::Arc(a) => {
            // d(θ) = d(centre) + r · sin(θ − φ) : minimum en θ = φ − π/2.
            let phi = axis.dir.y.atan2(axis.dir.x);
            let theta_min = phi - FRAC_PI_2;
            let rel = if a.sweep_rad > 0.0 {
                (theta_min - a.start_rad).rem_euclid(TAU)
            } else {
                (a.start_rad - theta_min).rem_euclid(TAU)
            };
            let inside = rel > 1e-9 && rel < a.sweep_rad.abs() - 1e-9;
            SideRange {
                start: axis.signed_distance(a.start_point()),
                end: axis.signed_distance(a.end_point()),
                interior_extremum_min: inside.then(|| axis.signed_distance(a.center) - a.radius_mm),
            }
        }
    }
}

/// Une chaîne dépliée.
#[derive(Debug, Clone, PartialEq)]
pub struct Unfolded {
    pub axis: FoldAxis,
    /// Début de la pliure dans le sens du parcours de la chaîne d'origine.
    pub fold_start: PointMm,
    /// Fin de la pliure.
    pub fold_end: PointMm,
    pub fold_length_mm: f64,
    /// Rang, dans la chaîne d'origine, de la première arête de la pliure.
    pub first_fold_edge: usize,
    /// Nombre d'arêtes élémentaires de la chaîne d'origine qui forment la pliure.
    pub fold_edges: usize,
    /// La moitié : de `fold_end` à `fold_start`, sans la pliure.
    pub half: Vec<EdgeGeom>,
    /// Chaîne entière fermée, anti-horaire : la moitié puis son image en miroir.
    pub chain: Vec<EdgeGeom>,
}

fn with_start(e: EdgeGeom, p: PointMm) -> EdgeGeom {
    match e {
        EdgeGeom::Line(_, b) => EdgeGeom::Line(p, b),
        EdgeGeom::Cubic(c) => EdgeGeom::Cubic(CubicBezier { p0: p, ..c }),
        arc @ EdgeGeom::Arc(_) => arc,
    }
}

fn with_end(e: EdgeGeom, p: PointMm) -> EdgeGeom {
    match e {
        EdgeGeom::Line(a, _) => EdgeGeom::Line(a, p),
        EdgeGeom::Cubic(c) => EdgeGeom::Cubic(CubicBezier { p3: p, ..c }),
        arc @ EdgeGeom::Arc(_) => arc,
    }
}

/// Déplie la chaîne fermée anti-horaire `chain` le long de son arête rectiligne
/// `fold_index`. Refus : voir [`FoldError`].
pub fn unfold_chain(chain: &[EdgeGeom], fold_index: usize) -> Result<Unfolded, FoldError> {
    let n = chain.len();
    let EdgeGeom::Line(fa, fb) = *chain.get(fold_index).ok_or(FoldError::NoSuchEdge)? else {
        return Err(FoldError::NotStraight);
    };
    let length = distance_mm(fa, fb);
    if length < MIN_FOLD_LENGTH_MM {
        return Err(FoldError::TooShort { length_mm: length });
    }
    let axis0 = FoldAxis::through(fa, fb)?;
    // Arêtes consécutives alignées dans le même sens : elles forment une seule pliure.
    let aligned = |e: &EdgeGeom| match e {
        EdgeGeom::Line(a, b) => {
            axis0.signed_distance(*a).abs() <= FOLD_TOLERANCE_MM
                && axis0.signed_distance(*b).abs() <= FOLD_TOLERANCE_MM
                && axis0.dir.dot(*b - *a) > 0.0
        }
        _ => false,
    };
    let (mut first, mut last, mut count) = (fold_index, fold_index, 1usize);
    while count < n && aligned(&chain[(first + n - 1) % n]) {
        first = (first + n - 1) % n;
        count += 1;
    }
    while count < n && aligned(&chain[(last + 1) % n]) {
        last = (last + 1) % n;
        count += 1;
    }
    if count >= n {
        return Err(FoldError::NoHalf);
    }
    let (a, b) = (chain[first].start(), chain[last].end());
    let axis = FoldAxis::through(a, b)?;
    let m = n - count;
    let order: Vec<usize> = (0..m).map(|j| (last + 1 + j) % n).collect();
    let ranges: Vec<SideRange> = order
        .iter()
        .map(|&i| edge_side_range(&chain[i], &axis))
        .collect();
    if let Some(j) = ranges.iter().position(|r| r.min() < -FOLD_TOLERANCE_MM) {
        return Err(FoldError::Crosses { edge: order[j] });
    }
    for (j, r) in ranges.iter().enumerate() {
        let touches = (j > 0 && r.start <= FOLD_TOLERANCE_MM)
            || (j + 1 < m && r.end <= FOLD_TOLERANCE_MM)
            || r.interior_extremum_min
                .is_some_and(|v| v <= FOLD_TOLERANCE_MM);
        if touches {
            return Err(FoldError::Touches { edge: order[j] });
        }
    }
    if chain_signed_area_mm2(chain) <= MIN_HALF_AREA_MM2 {
        return Err(FoldError::NoHalf);
    }
    let half: Vec<EdgeGeom> = order.iter().map(|&i| chain[i]).collect();
    let mut out = half.clone();
    for e in half.iter().rev() {
        out.push(axis.reflect_edge(e).reversed());
    }
    // Les extrémités de la pliure sont communes aux deux moitiés : exactes.
    let first_mirror = m;
    out[first_mirror] = with_start(out[first_mirror], a);
    let last_mirror = out.len() - 1;
    out[last_mirror] = with_end(out[last_mirror], b);
    Ok(Unfolded {
        axis,
        fold_start: a,
        fold_end: b,
        fold_length_mm: distance_mm(a, b),
        first_fold_edge: first,
        fold_edges: count,
        half,
        chain: out,
    })
}

#[cfg(test)]
#[path = "fold_tests.rs"]
mod tests;

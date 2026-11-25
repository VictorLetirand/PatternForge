//! US-07.1 — Pince : forme, fermeture par pliage et capuchon (calcul pur, mm).
//!
//! Une pince est un triangle isocèle : le **sommet** `A`, un **axe** (direction `A → toward`),
//! une **longueur** `L` (distance de `A` au milieu de la base, mesurée sur l'axe) et une
//! **ouverture** `w` (largeur de la base, perpendiculaire à l'axe). Les deux pieds sont
//! `M ± n · w/2` (`M = A + L · axe`, `n` = normale gauche) : les deux jambes sont donc égales
//! (`√(L² + (w/2)²)`) par construction, pas par réglage.
//!
//! **Fermer** la pince = plier : les pieds se rejoignent. Seule la ligne qui porte la base est
//! contrôlée (une suite d'arêtes du contour, dans le sens du contour) : la portion entre les
//! deux pieds disparaît, la portion du côté **mobile** tourne autour du sommet d'un angle
//! `2·atan(w/2L)` jusqu'à rejoindre le côté **fixe**. Rien n'est modifié : c'est une mesure.
//!
//! Le **capuchon** est la forme corrigée de la base : après pliage, la ligne a un angle au
//! pied ; une droite lisse relie les points situés à `min(longueur du côté, 2 w)` de part et
//! d'autre du pied ; dépliée, cette droite se brise sur les deux jambes en `Ka` (jambe
//! gauche ou droite selon le contour) et `Kb`, à égale distance du sommet. La base corrigée est
//! la ligne `P1 → Ka → Kb → P2`.

use crate::region::{trace_nearest_param, EdgeGeom};
use crate::split::{chain_length_mm, edge_portion, edge_trace};
use crate::{intersect, Arc, CubicBezier, PointMm, Side, VectorMm, EPSILON_MM};
use std::fmt;

/// Plus petite longueur de pince acceptée (mm).
pub const DART_MIN_LENGTH_MM: f64 = 1.0;
/// Plus grande longueur de pince acceptée (mm).
pub const DART_MAX_LENGTH_MM: f64 = 2000.0;
/// Plus petite ouverture de pince acceptée (mm).
pub const DART_MIN_OPENING_MM: f64 = 0.1;
/// Plus grande ouverture de pince acceptée (mm) ; elle ne dépasse jamais la longueur.
pub const DART_MAX_OPENING_MM: f64 = 1000.0;
/// Distance maximale (mm) d'un pied de pince à la ligne qui porte la base.
pub const DART_FOOT_TOLERANCE_MM: f64 = 1e-3;
/// Étendue du capuchon de chaque côté du pied, en nombre d'ouvertures.
pub const DART_CAP_SPAN_FACTOR: f64 = 2.0;
/// Longueur (mm) en dessous de laquelle un morceau de ligne n'existe pas.
const MIN_PART_MM: f64 = 1e-9;

/// Pourquoi une pince n'est pas calculable.
#[derive(Debug, Clone, PartialEq)]
pub enum DartError {
    /// Valeur non finie.
    NonFinite,
    /// Le sommet et le point d'axe sont confondus.
    NoAxis,
    /// Longueur hors de [`DART_MIN_LENGTH_MM`] ; [`DART_MAX_LENGTH_MM`].
    Length(f64),
    /// Ouverture hors de [`DART_MIN_OPENING_MM`] ; [`DART_MAX_OPENING_MM`] ou plus grande que la longueur.
    Opening(f64),
    /// Ligne vide ou sans longueur.
    EmptyLine,
    /// Un pied est trop loin de la ligne (mm).
    FootOffLine { side: Side, distance_mm: f64 },
    /// Les deux pieds sont au même endroit de la ligne.
    FeetMerged,
    /// Part transférée hors de ]0 ; 1] ou non finie (US-07.2).
    Fraction(f64),
    /// La cible du transfert n'est pas sur le contour (écart en mm).
    TargetOffContour { distance_mm: f64 },
    /// La cible du transfert est sur la base de la pince (ou sur l'un de ses pieds).
    TargetOnBase,
    /// La cible est confondue avec le sommet de la pince.
    TargetAtApex,
}

impl fmt::Display for DartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DartError::NonFinite => write!(f, "une valeur de la pince n'est pas un nombre fini"),
            DartError::NoAxis => write!(
                f,
                "le sommet de la pince et son point d'axe sont confondus : l'axe n'est pas défini"
            ),
            DartError::Length(v) => write!(
                f,
                "la longueur de la pince ({v} mm) doit être comprise entre {DART_MIN_LENGTH_MM} et \
                 {DART_MAX_LENGTH_MM} mm"
            ),
            DartError::Opening(v) => write!(
                f,
                "l'ouverture de la pince ({v} mm) doit être comprise entre {DART_MIN_OPENING_MM} et \
                 {DART_MAX_OPENING_MM} mm et ne pas dépasser sa longueur"
            ),
            DartError::EmptyLine => write!(f, "la ligne qui porte la base de la pince est vide"),
            DartError::FootOffLine { side, distance_mm } => write!(
                f,
                "le pied {} de la pince est à {distance_mm:.3} mm de la ligne : la base doit être \
                 sur un bord du contour (tolérance {DART_FOOT_TOLERANCE_MM} mm)",
                match side {
                    Side::Left => "gauche",
                    Side::Right => "droit",
                }
            ),
            DartError::FeetMerged => write!(f, "les deux pieds de la pince sont confondus"),
            DartError::Fraction(v) => write!(
                f,
                "la part transférée ({v}) doit être un nombre fini supérieur à 0 et au plus 1"
            ),
            DartError::TargetOffContour { distance_mm } => write!(
                f,
                "la cible du transfert est à {distance_mm:.6} mm du contour : elle doit être sur \
                 un bord"
            ),
            DartError::TargetOnBase => write!(
                f,
                "la cible du transfert est sur la base de la pince ou sur un de ses pieds"
            ),
            DartError::TargetAtApex => write!(
                f,
                "la cible du transfert est confondue avec le sommet (pivot) de la pince"
            ),
        }
    }
}

impl std::error::Error for DartError {}

/// Forme d'une pince : sommet, pieds et jambes (calculés, jamais stockés).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DartShape {
    pub apex: PointMm,
    /// Axe unitaire, du sommet vers la base.
    pub axis: VectorMm,
    /// Milieu de la base.
    pub base_mid: PointMm,
    /// Pied à gauche de l'axe (en regardant du sommet vers la base).
    pub left_foot: PointMm,
    pub right_foot: PointMm,
    /// Longueur de la jambe gauche, mesurée sur les points calculés.
    pub leg_left_mm: f64,
    pub leg_right_mm: f64,
    /// Angle d'ouverture au sommet, en degrés.
    pub angle_deg: f64,
    pub opening_mm: f64,
    pub length_mm: f64,
}

impl DartShape {
    pub fn foot(&self, side: Side) -> PointMm {
        match side {
            Side::Left => self.left_foot,
            Side::Right => self.right_foot,
        }
    }
}

fn d(a: PointMm, b: PointMm) -> f64 {
    (b - a).norm()
}

/// Forme d'une pince d'après son sommet, son point d'axe, sa longueur et son ouverture.
pub fn dart_shape(
    apex: PointMm,
    toward: PointMm,
    length_mm: f64,
    opening_mm: f64,
) -> Result<DartShape, DartError> {
    let all = [apex.x, apex.y, toward.x, toward.y, length_mm, opening_mm];
    if all.iter().any(|v| !v.is_finite()) {
        return Err(DartError::NonFinite);
    }
    if !(DART_MIN_LENGTH_MM..=DART_MAX_LENGTH_MM).contains(&length_mm) {
        return Err(DartError::Length(length_mm));
    }
    if !(DART_MIN_OPENING_MM..=DART_MAX_OPENING_MM).contains(&opening_mm) || opening_mm > length_mm
    {
        return Err(DartError::Opening(opening_mm));
    }
    let axis = (toward - apex).unit().map_err(|_| DartError::NoAxis)?;
    let normal = axis.rotated_left();
    let base_mid = apex + axis * length_mm;
    let half = opening_mm / 2.0;
    let left_foot = base_mid + normal * half;
    let right_foot = base_mid + normal * -half;
    Ok(DartShape {
        apex,
        axis,
        base_mid,
        left_foot,
        right_foot,
        leg_left_mm: d(apex, left_foot),
        leg_right_mm: d(apex, right_foot),
        angle_deg: 2.0 * half.atan2(length_mm).to_degrees(),
        opening_mm,
        length_mm,
    })
}

/// Résultat de la fermeture d'une pince sur la ligne qui porte sa base.
#[derive(Debug, Clone, PartialEq)]
pub struct ClosedLine {
    /// Longueur de la ligne avant pliage (mm).
    pub before_mm: f64,
    /// Longueur de la ligne après pliage (mm) : avant moins la base retirée.
    pub after_mm: f64,
    /// Longueur de la portion de ligne entre les deux pieds (mm), retirée par le pliage.
    pub removed_mm: f64,
    /// Ligne pliée : côté fixe, puis côté mobile tourné autour du sommet.
    pub chain: Vec<EdgeGeom>,
    /// Angle de rotation appliqué au côté mobile (degrés, signé, anti-horaire positif).
    pub rotation_deg: f64,
    /// Angle de la ligne pliée au pied (degrés, signé ; 0 = ligne lisse) ; `None` si un côté est vide.
    pub corner_deg: Option<f64>,
    /// Pied du côté fixe.
    pub fixed: Side,
    /// Base corrigée `P1, Ka, Kb, P2` (dans le sens de la ligne) ; `None` si elle n'existe pas.
    pub capuchon: Option<[PointMm; 4]>,
    /// Distance du sommet aux deux points `Ka` / `Kb` du capuchon (mm).
    pub capuchon_leg_mm: Option<f64>,
}

fn locate(line: &[EdgeGeom], p: PointMm) -> (usize, f64, f64) {
    let mut best = (0usize, 0.0f64, f64::INFINITY);
    for (i, e) in line.iter().enumerate() {
        let (t, dist) = trace_nearest_param(&edge_trace(e), p);
        if dist < best.2 - 1e-12 {
            best = (i, t, dist);
        }
    }
    best
}

fn keep(parts: Vec<EdgeGeom>) -> Vec<EdgeGeom> {
    parts
        .into_iter()
        .filter(|e| e.length_mm() > MIN_PART_MM)
        .collect()
}

/// Portion de la ligne entre deux positions `(arête, paramètre)`, `a ≤ b`.
fn slice(line: &[EdgeGeom], a: (usize, f64), b: (usize, f64)) -> Vec<EdgeGeom> {
    if a.0 == b.0 {
        return keep(vec![edge_portion(&line[a.0], a.1, b.1)]);
    }
    let mut v = vec![edge_portion(&line[a.0], a.1, 1.0)];
    v.extend_from_slice(&line[a.0 + 1..b.0]);
    v.push(edge_portion(&line[b.0], 0.0, b.1));
    keep(v)
}

fn rot(p: PointMm, c: PointMm, cos: f64, sin: f64) -> PointMm {
    let (dx, dy) = (p.x - c.x, p.y - c.y);
    PointMm::new(c.x + cos * dx - sin * dy, c.y + sin * dx + cos * dy)
}

pub(crate) fn rot_edge(e: &EdgeGeom, c: PointMm, cos: f64, sin: f64) -> EdgeGeom {
    match e {
        EdgeGeom::Line(a, b) => EdgeGeom::Line(rot(*a, c, cos, sin), rot(*b, c, cos, sin)),
        EdgeGeom::Cubic(q) => EdgeGeom::Cubic(CubicBezier::new(
            rot(q.p0, c, cos, sin),
            rot(q.c1, c, cos, sin),
            rot(q.c2, c, cos, sin),
            rot(q.p3, c, cos, sin),
        )),
        EdgeGeom::Arc(a) => EdgeGeom::Arc(Arc {
            center: rot(a.center, c, cos, sin),
            radius_mm: a.radius_mm,
            start_rad: a.start_rad + sin.atan2(cos),
            sweep_rad: a.sweep_rad,
        }),
    }
}

fn tangent_start(e: &EdgeGeom) -> Option<VectorMm> {
    match e {
        EdgeGeom::Line(a, b) => (*b - *a).unit().ok(),
        EdgeGeom::Cubic(c) => [c.c1 - c.p0, c.c2 - c.p0, c.p3 - c.p0]
            .into_iter()
            .find_map(|v| v.unit().ok()),
        EdgeGeom::Arc(a) => {
            let s = if a.sweep_rad >= 0.0 { 1.0 } else { -1.0 };
            Some(VectorMm::new(-a.start_rad.sin() * s, a.start_rad.cos() * s))
        }
    }
}

fn tangent_end(e: &EdgeGeom) -> Option<VectorMm> {
    match e {
        EdgeGeom::Line(a, b) => (*b - *a).unit().ok(),
        EdgeGeom::Cubic(c) => [c.p3 - c.c2, c.p3 - c.c1, c.p3 - c.p0]
            .into_iter()
            .find_map(|v| v.unit().ok()),
        EdgeGeom::Arc(a) => {
            let s = if a.sweep_rad >= 0.0 { 1.0 } else { -1.0 };
            let end = a.start_rad + a.sweep_rad;
            Some(VectorMm::new(-end.sin() * s, end.cos() * s))
        }
    }
}

/// Point situé à `dist` mm de longueur développée du début de la suite d'arêtes.
fn point_at_distance(edges: &[EdgeGeom], dist: f64) -> Option<PointMm> {
    let mut left = dist;
    for e in edges {
        let len = e.length_mm();
        if left <= len + 1e-9 {
            let f = (left / len).clamp(0.0, 1.0);
            return Some(match e {
                EdgeGeom::Line(a, b) => a.lerp(*b, f),
                EdgeGeom::Cubic(c) => c.point_at(c.t_at_length_mm(left.min(len), 1e-6).ok()?),
                EdgeGeom::Arc(a) => {
                    let ang = a.start_rad + a.sweep_rad * f;
                    PointMm::new(
                        a.center.x + a.radius_mm * ang.cos(),
                        a.center.y + a.radius_mm * ang.sin(),
                    )
                }
            });
        }
        left -= len;
    }
    None
}

fn reversed_chain(edges: &[EdgeGeom]) -> Vec<EdgeGeom> {
    edges.iter().rev().map(EdgeGeom::reversed).collect()
}

/// Ferme la pince sur `line` (suite d'arêtes contiguës, dans le sens du contour) : mesure
/// pure, rien n'est modifié. `fixed` = jambe qui reste en place (gauche ou droite de l'axe).
pub fn close_dart(
    line: &[EdgeGeom],
    shape: &DartShape,
    fixed: Side,
) -> Result<ClosedLine, DartError> {
    if line.is_empty() || chain_length_mm(line) <= MIN_PART_MM {
        return Err(DartError::EmptyLine);
    }
    let locs = [
        (Side::Left, locate(line, shape.left_foot)),
        (Side::Right, locate(line, shape.right_foot)),
    ];
    for (side, (_, _, dist)) in locs {
        if dist > DART_FOOT_TOLERANCE_MM {
            return Err(DartError::FootOffLine {
                side,
                distance_mm: dist,
            });
        }
    }
    let pos = |i: usize| (locs[i].1 .0, locs[i].1 .1);
    let left_first = pos(0) < pos(1);
    let (first, second) = if left_first {
        (pos(0), pos(1))
    } else {
        (pos(1), pos(0))
    };
    if first == second {
        return Err(DartError::FeetMerged);
    }
    let first_side = if left_first { Side::Left } else { Side::Right };
    let last = line.len() - 1;
    let e1 = slice(line, (0, 0.0), first);
    let base = slice(line, first, second);
    let e2 = slice(line, second, (last, 1.0));
    // Côté mobile : celui du pied qui ne reste pas en place.
    let moving_is_second = first_side == fixed;
    let (fixed_foot, moving_foot) = (shape.foot(fixed), shape.foot(fixed.flipped()));
    let (va, vb) = (moving_foot - shape.apex, fixed_foot - shape.apex);
    let norm = va.norm() * vb.norm();
    let (cos, sin) = (va.dot(vb) / norm, va.cross(vb) / norm);
    let (cos_back, sin_back) = (cos, -sin);
    let moved = |part: &[EdgeGeom]| -> Vec<EdgeGeom> {
        part.iter()
            .map(|e| rot_edge(e, shape.apex, cos, sin))
            .collect()
    };
    let chain: Vec<EdgeGeom> = if moving_is_second {
        e1.iter().copied().chain(moved(&e2)).collect()
    } else {
        moved(&e1).into_iter().chain(e2.iter().copied()).collect()
    };
    let (len1, len2) = (chain_length_mm(&e1), chain_length_mm(&e2));
    let corner_deg = if moving_is_second {
        e1.last()
            .and_then(tangent_end)
            .zip(chain.get(e1.len()).and_then(tangent_start))
    } else {
        chain
            .get(e1.len().wrapping_sub(1))
            .and_then(tangent_end)
            .zip(e2.first().and_then(tangent_start))
    }
    .map(|(t1, t2)| t1.cross(t2).atan2(t1.dot(t2)).to_degrees());

    // Capuchon.
    let span = DART_CAP_SPAN_FACTOR * shape.opening_mm;
    let (d1, d2) = (len1.min(span), len2.min(span));
    let p1 = (!e1.is_empty())
        .then(|| point_at_distance(&reversed_chain(&e1), d1))
        .flatten();
    let p2 = (!e2.is_empty())
        .then(|| point_at_distance(&e2, d2))
        .flatten();
    let (capuchon, capuchon_leg_mm) = match p1.zip(p2) {
        Some((p1, p2)) => {
            let (q1, q2) = if moving_is_second {
                (p1, rot(p2, shape.apex, cos, sin))
            } else {
                (rot(p1, shape.apex, cos, sin), p2)
            };
            let leg = fixed_foot - shape.apex;
            let chord = q2 - q1;
            let den = leg.cross(chord);
            if den.abs() <= EPSILON_MM {
                (None, None)
            } else {
                let w = q1 - shape.apex;
                let s = w.cross(chord) / den;
                let u = w.cross(leg) / den;
                if s > 0.0 && (-1e-9..=1.0 + 1e-9).contains(&u) {
                    let k_fixed = shape.apex + leg * s;
                    let k_moving = rot(k_fixed, shape.apex, cos_back, sin_back);
                    let pts = if moving_is_second {
                        [p1, k_fixed, k_moving, p2]
                    } else {
                        [p1, k_moving, k_fixed, p2]
                    };
                    (Some(pts), Some(s * leg.norm()))
                } else {
                    (None, None)
                }
            }
        }
        None => (None, None),
    };
    let before_mm = chain_length_mm(line);
    let removed_mm = chain_length_mm(&base);
    Ok(ClosedLine {
        before_mm,
        after_mm: len1 + len2,
        removed_mm,
        chain,
        rotation_deg: sin.atan2(cos).to_degrees(),
        corner_deg,
        fixed,
        capuchon,
        capuchon_leg_mm,
    })
}

/// Distance (mm) du point `p` à la suite d'arêtes (infinie si vide).
pub fn distance_to_chain_mm(chain: &[EdgeGeom], p: PointMm) -> f64 {
    chain
        .iter()
        .map(|e| trace_nearest_param(&edge_trace(e), p).1)
        .fold(f64::INFINITY, f64::min)
}

/// Une jambe de la pince traverse-t-elle le contour ailleurs qu'à son pied ? Rend le côté
/// de la première jambe qui le fait. Un tracé illisible compte comme une traversée.
pub fn leg_crossing_contour(chain: &[EdgeGeom], shape: &DartShape) -> Option<Side> {
    for side in [Side::Left, Side::Right] {
        let foot = shape.foot(side);
        let leg = crate::Trace::Line(shape.apex, foot);
        for e in chain {
            match intersect(&leg, &edge_trace(e)) {
                Ok(hits) => {
                    if hits
                        .iter()
                        .any(|h| d(h.point, foot) > DART_FOOT_TOLERANCE_MM)
                    {
                        return Some(side);
                    }
                }
                Err(_) => return Some(side),
            }
        }
    }
    None
}

/// Distance (mm) en deçà de laquelle la cible d'un transfert est « sur » le contour, la base ou
/// un pied.
pub const DART_TARGET_TOLERANCE_MM: f64 = 1e-6;

/// Rôle d'une arête du contour transféré.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferPart {
    /// Arête (ou portion d'arête) du contour d'origine, éventuellement tournée.
    Contour,
    /// Base de la pince créée à la cible (corde entre ses deux pieds).
    NewBase,
    /// Base de la pince d'origine qui reste ouverte (transfert partiel seulement).
    RemainingBase,
}

/// Arête du contour après transfert.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransferEdge {
    pub geom: EdgeGeom,
    /// Indice, dans le contour d'origine, de l'arête dont elle provient (`None` pour une base).
    pub source: Option<usize>,
    pub part: TransferPart,
}

/// Résultat du transfert d'une pince (calcul pur : rien n'est modifié).
#[derive(Debug, Clone, PartialEq)]
pub struct DartTransfer {
    /// Contour fermé après transfert, dans le sens du contour d'origine.
    pub edges: Vec<TransferEdge>,
    /// Rotation appliquée à la partie mobile (degrés, signé, anti-horaire positif).
    pub rotation_deg: f64,
    pub cos: f64,
    pub sin: f64,
    /// Pivot (sommet de la pince).
    pub apex: PointMm,
    /// Pied de la nouvelle pince resté en place, puis pied de la partie tournée.
    pub new_fixed_foot: PointMm,
    pub new_moving_foot: PointMm,
    /// Pieds de la pince d'origine qui reste (fixe, puis tourné) ; `None` si transfert total.
    pub remaining: Option<(PointMm, PointMm)>,
    /// Partie du contour qui tourne, dans sa position **d'origine** (avec le pivot, elle borde la
    /// région à tourner).
    pub moving_arc: Vec<EdgeGeom>,
}

/// Position `(arête, paramètre)` le long d'une suite d'arêtes.
type Pos = (usize, f64);

/// Portion de la suite entre deux positions (`a ≤ b`), avec l'indice d'arête d'origine.
fn slice_src(line: &[EdgeGeom], a: Pos, b: Pos) -> Vec<(EdgeGeom, usize)> {
    let mut v = Vec::new();
    if a.0 == b.0 {
        v.push((edge_portion(&line[a.0], a.1, b.1), a.0));
    } else {
        v.push((edge_portion(&line[a.0], a.1, 1.0), a.0));
        for (i, e) in line.iter().enumerate().take(b.0).skip(a.0 + 1) {
            v.push((*e, i));
        }
        v.push((edge_portion(&line[b.0], 0.0, b.1), b.0));
    }
    v.retain(|(e, _)| e.length_mm() > MIN_PART_MM);
    v
}

fn total(parts: &[(EdgeGeom, usize)]) -> f64 {
    parts.iter().map(|(e, _)| e.length_mm()).sum()
}

/// Pivote la pince `shape` : la portion du contour entre la base d'origine et `target` tourne
/// autour du sommet de `part · angle` (`part` dans ]0 ; 1], 1 = transfert total) ; la base
/// d'origine se ferme (en partie si `part < 1`), une base s'ouvre à la cible.
///
/// `chain` est le contour fermé (arêtes élémentaires contiguës, sens du contour) dont les
/// `line_len` premières arêtes sont la ligne qui porte la base de la pince. Calcul pur.
pub fn transfer_dart(
    chain: &[EdgeGeom],
    line_len: usize,
    shape: &DartShape,
    fixed: Side,
    target: PointMm,
    part: f64,
) -> Result<DartTransfer, DartError> {
    if !part.is_finite() || part <= 0.0 || part > 1.0 {
        return Err(DartError::Fraction(part));
    }
    if !target.x.is_finite() || !target.y.is_finite() {
        return Err(DartError::NonFinite);
    }
    if line_len == 0 || line_len > chain.len() || chain_length_mm(chain) <= MIN_PART_MM {
        return Err(DartError::EmptyLine);
    }
    let line = &chain[..line_len];
    let locs = [
        (Side::Left, locate(line, shape.left_foot)),
        (Side::Right, locate(line, shape.right_foot)),
    ];
    for (side, (_, _, dist)) in locs {
        if dist > DART_FOOT_TOLERANCE_MM {
            return Err(DartError::FootOffLine {
                side,
                distance_mm: dist,
            });
        }
    }
    let pos = |i: usize| (locs[i].1 .0, locs[i].1 .1);
    let left_first = pos(0) < pos(1);
    let (first, second) = if left_first {
        (pos(0), pos(1))
    } else {
        (pos(1), pos(0))
    };
    if first == second {
        return Err(DartError::FeetMerged);
    }
    let first_side = if left_first { Side::Left } else { Side::Right };
    let (_, _, target_dist) = locate(chain, target);
    if target_dist > DART_TARGET_TOLERANCE_MM {
        return Err(DartError::TargetOffContour {
            distance_mm: target_dist,
        });
    }
    if d(target, shape.apex) <= DART_TARGET_TOLERANCE_MM {
        return Err(DartError::TargetAtApex);
    }
    let base: Vec<EdgeGeom> = slice_src(chain, first, second)
        .into_iter()
        .map(|x| x.0)
        .collect();
    if distance_to_chain_mm(&base, target) <= DART_TARGET_TOLERANCE_MM {
        return Err(DartError::TargetOnBase);
    }
    // Contour vu depuis le pied `second`, base exclue : de `second` à la fin, puis du début à
    // `first`.
    let last = chain.len() - 1;
    let tail = slice_src(chain, second, (last, 1.0));
    let head = slice_src(chain, (0, 0.0), first);
    let cycle: Vec<(EdgeGeom, usize)> = tail.into_iter().chain(head).collect();
    let cyc: Vec<EdgeGeom> = cycle.iter().map(|x| x.0).collect();
    if cyc.is_empty() {
        return Err(DartError::EmptyLine);
    }
    let (ti, tt, _) = locate(&cyc, target);
    let src = |parts: Vec<(EdgeGeom, usize)>| -> Vec<(EdgeGeom, usize)> {
        parts.into_iter().map(|(g, i)| (g, cycle[i].1)).collect()
    };
    // X : du pied `second` à la cible ; Y : de la cible au pied `first`.
    let x = src(slice_src(&cyc, (0, 0.0), (ti, tt)));
    let y = src(slice_src(&cyc, (ti, tt), (cyc.len() - 1, 1.0)));
    if total(&x) <= DART_TARGET_TOLERANCE_MM || total(&y) <= DART_TARGET_TOLERANCE_MM {
        return Err(DartError::TargetOnBase);
    }

    let moving_is_second = first_side == fixed;
    let (fixed_foot, moving_foot) = (shape.foot(fixed), shape.foot(fixed.flipped()));
    let (va, vb) = (moving_foot - shape.apex, fixed_foot - shape.apex);
    let norm = va.norm() * vb.norm();
    let (cos1, sin1) = (va.dot(vb) / norm, va.cross(vb) / norm);
    let (cos, sin) = if part == 1.0 {
        (cos1, sin1)
    } else {
        let angle = sin1.atan2(cos1) * part;
        (angle.cos(), angle.sin())
    };
    let apex = shape.apex;
    let keep_edge = |(g, s): &(EdgeGeom, usize)| TransferEdge {
        geom: *g,
        source: Some(*s),
        part: TransferPart::Contour,
    };
    let turn_edge = |(g, s): &(EdgeGeom, usize)| TransferEdge {
        geom: rot_edge(g, apex, cos, sin),
        source: Some(*s),
        part: TransferPart::Contour,
    };
    let base_edge = |a: PointMm, b: PointMm, part: TransferPart| {
        (d(a, b) > MIN_PART_MM).then_some(TransferEdge {
            geom: EdgeGeom::Line(a, b),
            source: None,
            part,
        })
    };
    let tau = target;
    let mut edges: Vec<TransferEdge>;
    let remaining;
    let moving_arc: Vec<EdgeGeom>;
    if moving_is_second {
        // Y (fixe), base restante, X (tourné), nouvelle base.
        edges = y.iter().map(keep_edge).collect();
        let turned: Vec<TransferEdge> = x.iter().map(turn_edge).collect();
        let f_fixed = y[y.len() - 1].0.end();
        let f_moved = turned[0].geom.start();
        let rem = base_edge(f_fixed, f_moved, TransferPart::RemainingBase);
        edges.extend(rem);
        edges.extend(turned.iter().copied());
        edges.extend(base_edge(
            turned[turned.len() - 1].geom.end(),
            tau,
            TransferPart::NewBase,
        ));
        remaining = rem.map(|_| (f_fixed, f_moved));
        moving_arc = x.iter().map(|e| e.0).collect();
    } else {
        // X (fixe), nouvelle base, Y (tourné), base restante.
        edges = x.iter().map(keep_edge).collect();
        let turned: Vec<TransferEdge> = y.iter().map(turn_edge).collect();
        edges.extend(base_edge(
            tau,
            turned[0].geom.start(),
            TransferPart::NewBase,
        ));
        edges.extend(turned.iter().copied());
        let f_moved = turned[turned.len() - 1].geom.end();
        let f_fixed = x[0].0.start();
        let rem = base_edge(f_moved, f_fixed, TransferPart::RemainingBase);
        edges.extend(rem);
        remaining = rem.map(|_| (f_fixed, f_moved));
        moving_arc = y.iter().map(|e| e.0).collect();
    }
    Ok(DartTransfer {
        edges,
        rotation_deg: sin.atan2(cos).to_degrees(),
        cos,
        sin,
        apex,
        new_fixed_foot: tau,
        new_moving_foot: rot(tau, apex, cos, sin),
        remaining,
        moving_arc,
    })
}

/// Tourne un point autour de `center` (`cos`, `sin` de l'angle, anti-horaire positif).
pub fn rotate_point(p: PointMm, center: PointMm, cos: f64, sin: f64) -> PointMm {
    rot(p, center, cos, sin)
}

/// Tourne une arête autour de `center`.
pub fn rotate_edge(e: &EdgeGeom, center: PointMm, cos: f64, sin: f64) -> EdgeGeom {
    rot_edge(e, center, cos, sin)
}

#[cfg(test)]
#[path = "dart_tests.rs"]
mod tests;

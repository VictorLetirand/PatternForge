//! Région fermée la plus petite contenant un point (US-04.1).
//!
//! À partir de tracés bornés (segment, Bézier, courbe par points, arc, cercle) et
//! d'un point visé, [`find_region`] :
//! 1. calcule toutes les intersections deux à deux (`intersect`), y compris
//!    quand les tracés se croisent **sans point commun**, et découpe chaque
//!    tracé en arêtes entre sommets successifs ;
//! 2. fusionne les sommets distants de moins de [`JOIN_TOLERANCE_MM`] (1e-6 mm) :
//!    c'est la tolérance de raccord, la même que la fusion des points du modèle ;
//! 3. retire les arêtes qui ne bordent aucune région (impasses, ponts entre
//!    deux formes : toute arête dont la suppression sépare le graphe) ;
//! 4. parcourt les faces du graphe planaire (tri angulaire des demi-arêtes,
//!    égalité de tangente départagée par la courbure) ;
//! 5. renvoie la plus petite face (aire) dont le bord, bord compris, contient le point.
//!
//! Le contour renvoyé est **orienté anti-horaire** (face à gauche) ; l'aire et le
//! périmètre sont exacts (formule de Green sur segments, arcs et Béziers).
//! Une forme complètement à l'intérieur d'une région sans la toucher (îlot)
//! n'est pas un trou : le contour de la région est son bord extérieur.
//!
//! Limites assumées (voir `docs/DECISIONS.md`, décisions 96 à 98) : deux tracés
//! superposés sur une longueur non nulle sont gérés pour les segments colinéaires
//! (le doublon est ignoré) et refusés ([`RegionError::Overlap`]) pour les autres
//! types ; un contour qui touche son propre bord en un point est refusé
//! ([`RegionError::SelfTouching`]) ; la position du clic est comparée à un
//! polygone qui approche les courbes à 1e-3 mm près.

use crate::{
    check_radius, distance_mm, intersect, segment_length_mm, Arc, Bbox, Circle, CubicBezier,
    GeometryError, PointMm, Trace, VectorMm, DEFAULT_LENGTH_TOLERANCE_MM, EPSILON_MM,
};
use std::f64::consts::{PI, TAU};
use std::fmt;

/// Tolérance de raccord (mm) : deux extrémités ou intersections plus proches
/// que cela sont un seul sommet ; un point plus proche d'un tracé que cela est
/// considéré comme posé dessus.
pub const JOIN_TOLERANCE_MM: f64 = 1e-6;
/// Une région d'aire inférieure (mm²) n'en est pas une.
const MIN_REGION_AREA_MM2: f64 = 1e-9;
/// Écart maximal (mm) entre une courbe et le polygone qui sert au test d'appartenance.
const FLATTEN_TOLERANCE_MM: f64 = 1e-3;
/// Nombre maximal de fermetures d'ouverture essayées avant d'abandonner.
const MAX_GAP_TRIES: usize = 8;
/// Nombre maximal d'extrémités libres examinées pour trouver une ouverture.
const MAX_FREE_ENDS: usize = 64;
/// Dépassement (mm) de la ligne de fermeture virtuelle au-delà du tracé visé.
const CLOSING_OVERSHOOT_MM: f64 = 1e-5;
/// Arête rebouclée sur son sommet et plus courte que cela (mm) : ignorée.
const MIN_LOOP_LENGTH_MM: f64 = 1e-5;

/// Géométrie élémentaire d'une portion de tracé.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeGeom {
    Line(PointMm, PointMm),
    Cubic(CubicBezier),
    Arc(Arc),
}

impl EdgeGeom {
    pub fn start(&self) -> PointMm {
        match self {
            EdgeGeom::Line(a, _) => *a,
            EdgeGeom::Cubic(c) => c.p0,
            EdgeGeom::Arc(a) => a.start_point(),
        }
    }

    pub fn end(&self) -> PointMm {
        match self {
            EdgeGeom::Line(_, b) => *b,
            EdgeGeom::Cubic(c) => c.p3,
            EdgeGeom::Arc(a) => a.end_point(),
        }
    }

    pub fn length_mm(&self) -> f64 {
        match self {
            EdgeGeom::Line(a, b) => distance_mm(*a, *b),
            EdgeGeom::Cubic(c) => c
                .length_mm(DEFAULT_LENGTH_TOLERANCE_MM)
                .unwrap_or_else(|_| c.chord_mm()),
            EdgeGeom::Arc(a) => a.length_mm(),
        }
    }

    /// Contribution à l'aire signée (Green), par rapport à `origin`, dans le sens du parcours.
    pub fn signed_area_mm2(&self, origin: PointMm) -> f64 {
        match self {
            EdgeGeom::Line(a, b) => 0.5 * (*a - origin).cross(*b - origin),
            EdgeGeom::Cubic(c) => c.signed_area_contribution_mm2(origin),
            EdgeGeom::Arc(a) => a.signed_area_contribution_mm2(origin),
        }
    }

    pub fn reversed(&self) -> EdgeGeom {
        match self {
            EdgeGeom::Line(a, b) => EdgeGeom::Line(*b, *a),
            EdgeGeom::Cubic(c) => EdgeGeom::Cubic(c.reversed()),
            EdgeGeom::Arc(a) => EdgeGeom::Arc(a.reversed()),
        }
    }

    /// Point au milieu du paramètre (sert à reconnaître deux arêtes identiques).
    fn mid_point(&self) -> PointMm {
        match self {
            EdgeGeom::Line(a, b) => a.lerp(*b, 0.5),
            EdgeGeom::Cubic(c) => c.point_at(0.5),
            EdgeGeom::Arc(a) => {
                let ang = a.start_rad + 0.5 * a.sweep_rad;
                PointMm::new(
                    a.center.x + a.radius_mm * ang.cos(),
                    a.center.y + a.radius_mm * ang.sin(),
                )
            }
        }
    }

    /// Tangente unitaire au départ (`None` si la géométrie est nulle).
    pub fn start_tangent(&self) -> Option<VectorMm> {
        match self {
            EdgeGeom::Line(a, b) => unit_snapped(*b - *a),
            EdgeGeom::Cubic(c) => unit_snapped(c.derivative_at(0.0))
                .or_else(|| unit_snapped(c.c2 - c.p0))
                .or_else(|| unit_snapped(c.p3 - c.p0)),
            EdgeGeom::Arc(a) => {
                let ang = a.start_rad;
                let t = VectorMm::new(-ang.sin(), ang.cos()) * a.sweep_rad.signum();
                unit_snapped(t)
            }
        }
    }

    /// Tangente unitaire à l'arrivée, dans le sens du parcours (`None` si la géométrie est nulle).
    pub fn end_tangent(&self) -> Option<VectorMm> {
        self.reversed().start_tangent().map(|t| t * -1.0)
    }

    /// Point situé à environ `d` mm du départ (au plus à mi-parcours).
    fn probe(&self, d: f64) -> PointMm {
        match self {
            EdgeGeom::Line(a, b) => {
                let len = distance_mm(*a, *b);
                if len <= EPSILON_MM {
                    *a
                } else {
                    a.lerp(*b, (d / len).min(0.5))
                }
            }
            EdgeGeom::Cubic(c) => {
                let poly =
                    distance_mm(c.p0, c.c1) + distance_mm(c.c1, c.c2) + distance_mm(c.c2, c.p3);
                let t = if poly <= EPSILON_MM {
                    0.0
                } else {
                    (d / poly).clamp(1e-6, 0.5)
                };
                c.point_at(t)
            }
            EdgeGeom::Arc(a) => {
                let frac = (d / a.length_mm().max(EPSILON_MM)).min(0.5);
                let ang = a.start_rad + a.sweep_rad * frac;
                PointMm::new(
                    a.center.x + a.radius_mm * ang.cos(),
                    a.center.y + a.radius_mm * ang.sin(),
                )
            }
        }
    }

    /// Ajoute à `out` les points d'une polyligne qui approche la géométrie à
    /// [`FLATTEN_TOLERANCE_MM`] près (le départ n'est pas ajouté).
    fn flatten(&self, out: &mut Vec<PointMm>) {
        match self {
            EdgeGeom::Line(_, b) => out.push(*b),
            EdgeGeom::Cubic(c) => flatten_cubic(c, 16, out),
            EdgeGeom::Arc(a) => {
                let r = a.radius_mm;
                let step = if r <= FLATTEN_TOLERANCE_MM {
                    PI
                } else {
                    2.0 * (1.0 - FLATTEN_TOLERANCE_MM / r).acos()
                };
                let n = ((a.sweep_rad.abs() / step).ceil() as usize).clamp(2, 20_000);
                for i in 1..=n {
                    let ang = a.start_rad + a.sweep_rad * i as f64 / n as f64;
                    out.push(PointMm::new(
                        a.center.x + r * ang.cos(),
                        a.center.y + r * ang.sin(),
                    ));
                }
            }
        }
    }
}

fn unit_snapped(v: VectorMm) -> Option<VectorMm> {
    let n = v.norm();
    if !n.is_finite() || n <= 1e-12 {
        return None;
    }
    let mut u = VectorMm::new(v.x / n, v.y / n);
    if u.x.abs() < 1e-9 {
        u.x = 0.0;
    }
    if u.y.abs() < 1e-9 {
        u.y = 0.0;
    }
    Some(u)
}

fn split_cubic(c: &CubicBezier, t: f64) -> (CubicBezier, CubicBezier) {
    let q0 = c.p0.lerp(c.c1, t);
    let q1 = c.c1.lerp(c.c2, t);
    let q2 = c.c2.lerp(c.p3, t);
    let r0 = q0.lerp(q1, t);
    let r1 = q1.lerp(q2, t);
    let x = r0.lerp(r1, t);
    (
        CubicBezier::new(c.p0, q0, r0, x),
        CubicBezier::new(x, r1, q2, c.p3),
    )
}

/// Portion `[t0 ; t1]` d'une Bézier (de Casteljau, exacte).
pub(crate) fn sub_cubic(c: &CubicBezier, t0: f64, t1: f64) -> CubicBezier {
    let left = if t1 >= 1.0 { *c } else { split_cubic(c, t1).0 };
    if t0 <= 0.0 || t1 <= 0.0 {
        left
    } else {
        split_cubic(&left, t0 / t1).1
    }
}

fn flatness(c: &CubicBezier) -> f64 {
    let chord = c.p3 - c.p0;
    let len = chord.norm();
    let dist = |p: PointMm| {
        if len <= 1e-12 {
            distance_mm(p, c.p0)
        } else {
            chord.cross(p - c.p0).abs() / len
        }
    };
    0.75 * dist(c.c1).max(dist(c.c2))
}

fn flatten_cubic(c: &CubicBezier, depth: u32, out: &mut Vec<PointMm>) {
    if depth == 0 || flatness(c) <= FLATTEN_TOLERANCE_MM {
        out.push(c.p3);
        return;
    }
    let (l, r) = split_cubic(c, 0.5);
    flatten_cubic(&l, depth - 1, out);
    flatten_cubic(&r, depth - 1, out);
}

/// Point d'un tracé au paramètre `t` (convention de [`crate::Hit`]).
pub fn trace_point_at(trace: &Trace, t: f64) -> PointMm {
    match trace {
        Trace::Line(a, b) => a.lerp(*b, t),
        Trace::Cubic(c) => c.point_at(t),
        Trace::Arc(a) => {
            let ang = a.start_rad + a.sweep_rad * t;
            PointMm::new(
                a.center.x + a.radius_mm * ang.cos(),
                a.center.y + a.radius_mm * ang.sin(),
            )
        }
        Trace::Circle(c) => {
            let ang = TAU * t;
            PointMm::new(
                c.center.x + c.radius_mm * ang.cos(),
                c.center.y + c.radius_mm * ang.sin(),
            )
        }
        Trace::Spline(s) => {
            let segs = s.segments();
            let n = segs.len();
            let u = (t * n as f64).clamp(0.0, n as f64);
            let i = (u.floor() as usize).min(n - 1);
            segs[i].point_at(u - i as f64)
        }
    }
}

fn trace_start(trace: &Trace) -> PointMm {
    match trace {
        Trace::Line(a, _) => *a,
        Trace::Cubic(c) => c.p0,
        Trace::Spline(s) => s.start_point(),
        Trace::Arc(a) => a.start_point(),
        Trace::Circle(c) => trace_point_at(&Trace::Circle(*c), 0.0),
    }
}

fn trace_end(trace: &Trace) -> PointMm {
    match trace {
        Trace::Line(_, b) => *b,
        Trace::Cubic(c) => c.p3,
        Trace::Spline(s) => s.end_point(),
        Trace::Arc(a) => a.end_point(),
        Trace::Circle(c) => trace_point_at(&Trace::Circle(*c), 0.0),
    }
}

fn trace_bbox(trace: &Trace) -> Bbox {
    match trace {
        Trace::Line(a, b) => {
            let mut bb = Bbox::from_point(*a);
            bb.include(*b);
            bb
        }
        Trace::Cubic(c) => c.bbox(),
        Trace::Spline(s) => s.bbox(),
        Trace::Arc(a) => a.bbox(),
        Trace::Circle(c) => c.bbox(),
    }
}

fn boxes_touch(a: &Bbox, b: &Bbox) -> bool {
    let tol = JOIN_TOLERANCE_MM;
    !(a.max.x + tol < b.min.x
        || b.max.x + tol < a.min.x
        || a.max.y + tol < b.min.y
        || b.max.y + tol < a.min.y)
}

fn check_trace(trace: &Trace) -> Result<(), GeometryError> {
    match trace {
        Trace::Line(a, b) => {
            segment_length_mm(*a, *b)?;
        }
        Trace::Cubic(c) => c.validate()?,
        Trace::Arc(a) => {
            a.center.validate()?;
            check_radius(a.radius_mm)?;
            if !a.start_rad.is_finite()
                || !a.sweep_rad.is_finite()
                || a.sweep_rad == 0.0
                || a.sweep_rad.abs() > TAU
            {
                return Err(GeometryError::InvalidArc("arc invalide".into()));
            }
        }
        Trace::Circle(c) => {
            Circle::new(c.center, c.radius_mm)?;
        }
        Trace::Spline(s) => {
            if s.segments().is_empty() {
                return Err(GeometryError::InvalidSpline("courbe vide".into()));
            }
            for c in s.segments() {
                c.validate()?;
            }
        }
    }
    Ok(())
}

/// Portion `[t0 ; t1]` d'un tracé (`t1 > t0` ; un cercle accepte `t1 ≤ t0 + 1`).
fn sub_geometry(trace: &Trace, t0: f64, t1: f64) -> Vec<EdgeGeom> {
    match trace {
        Trace::Line(a, b) => vec![EdgeGeom::Line(a.lerp(*b, t0), a.lerp(*b, t1))],
        Trace::Cubic(c) => vec![EdgeGeom::Cubic(sub_cubic(c, t0, t1))],
        Trace::Arc(a) => vec![EdgeGeom::Arc(Arc {
            center: a.center,
            radius_mm: a.radius_mm,
            start_rad: a.start_rad + a.sweep_rad * t0,
            sweep_rad: a.sweep_rad * (t1 - t0),
        })],
        Trace::Circle(c) => {
            let sweep = TAU * (t1 - t0);
            let start = TAU * t0;
            if sweep >= TAU - 1e-12 {
                [0.0, PI]
                    .iter()
                    .map(|k| {
                        EdgeGeom::Arc(Arc {
                            center: c.center,
                            radius_mm: c.radius_mm,
                            start_rad: start + k,
                            sweep_rad: PI,
                        })
                    })
                    .collect()
            } else {
                vec![EdgeGeom::Arc(Arc {
                    center: c.center,
                    radius_mm: c.radius_mm,
                    start_rad: start,
                    sweep_rad: sweep,
                })]
            }
        }
        Trace::Spline(s) => {
            let segs = s.segments();
            let n = segs.len() as f64;
            let (u0, u1) = (t0 * n, t1 * n);
            let mut out = Vec::new();
            let first = (u0.floor().max(0.0)) as usize;
            let last = ((u1.ceil() as isize) - 1).max(0) as usize;
            for (i, seg) in segs
                .iter()
                .enumerate()
                .take(last.min(segs.len() - 1) + 1)
                .skip(first.min(segs.len() - 1))
            {
                let lo = (u0 - i as f64).max(0.0);
                let hi = (u1 - i as f64).min(1.0);
                if hi - lo > 1e-12 {
                    out.push(EdgeGeom::Cubic(sub_cubic(seg, lo, hi)));
                }
            }
            out
        }
    }
}

/// Géométrie complète d'un tracé (cercle : deux demi-arcs).
pub fn trace_geometry(trace: &Trace) -> Vec<EdgeGeom> {
    sub_geometry(trace, 0.0, 1.0)
}

/// Paramètre du point du tracé le plus proche de `p`, et sa distance.
pub fn trace_nearest_param(trace: &Trace, p: PointMm) -> (f64, f64) {
    match trace {
        Trace::Line(a, b) => {
            let v = *b - *a;
            let len2 = v.dot(v);
            let s = if len2 <= 0.0 {
                0.0
            } else {
                ((p - *a).dot(v) / len2).clamp(0.0, 1.0)
            };
            (s, distance_mm(a.lerp(*b, s), p))
        }
        Trace::Arc(a) => match a.fraction_of(p) {
            Some(f) => (f, (distance_mm(a.center, p) - a.radius_mm).abs()),
            None => {
                let (ds, de) = (
                    distance_mm(p, a.start_point()),
                    distance_mm(p, a.end_point()),
                );
                if ds <= de {
                    (0.0, ds)
                } else {
                    (1.0, de)
                }
            }
        },
        Trace::Circle(c) => {
            let v = p - c.center;
            if v.norm() <= EPSILON_MM {
                (0.0, c.radius_mm)
            } else {
                (
                    v.y.atan2(v.x).rem_euclid(TAU) / TAU,
                    (v.norm() - c.radius_mm).abs(),
                )
            }
        }
        Trace::Cubic(_) | Trace::Spline(_) => {
            let n = match trace {
                Trace::Spline(s) => 64 * s.segments().len(),
                _ => 128,
            };
            let dist = |t: f64| distance_mm(trace_point_at(trace, t), p);
            let best = (0..=n)
                .map(|i| i as f64 / n as f64)
                .min_by(|a, b| dist(*a).total_cmp(&dist(*b)))
                .unwrap_or(0.0);
            let (mut lo, mut hi) = (
                (best - 1.0 / n as f64).max(0.0),
                (best + 1.0 / n as f64).min(1.0),
            );
            let phi = 0.618_033_988_749_894_9;
            for _ in 0..60 {
                let (m1, m2) = (hi - (hi - lo) * phi, lo + (hi - lo) * phi);
                if dist(m1) < dist(m2) {
                    hi = m2;
                } else {
                    lo = m1;
                }
            }
            let t = 0.5 * (lo + hi);
            (t, dist(t))
        }
    }
}

/// Ouverture : où la région ne se ferme pas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpenGap {
    /// Extrémité libre d'un tracé.
    pub from: PointMm,
    /// Extrémité libre la plus proche, ou point le plus proche d'un autre tracé.
    pub to: PointMm,
    pub gap_mm: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RegionError {
    /// Position de clic non finie ou hors de l'espace de travail.
    InvalidClick,
    /// Un tracé est dégénéré, non fini ou hors de l'espace de travail.
    InvalidTrace { index: usize, error: GeometryError },
    /// Aucune région fermée à cet endroit (hors de toute face, aucune ouverture proche).
    NoRegion,
    /// La région se fermerait si cette ouverture était comblée.
    Open(OpenGap),
    /// Deux tracés se superposent sur une longueur non nulle (cas non géré).
    Overlap { first: usize, second: usize },
    /// Le contour de la région touche son propre bord en un point.
    SelfTouching,
}

impl fmt::Display for RegionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidClick => write!(f, "position invalide (non finie ou hors de l'espace de travail)"),
            Self::InvalidTrace { index, error } => write!(f, "le tracé n° {} est invalide : {error}", index + 1),
            Self::NoRegion => write!(f, "aucune région fermée à cet endroit"),
            Self::Open(g) => write!(
                f,
                "la région n'est pas fermée : ouverture de {:.3} mm entre ({:.3} ; {:.3}) et ({:.3} ; {:.3})",
                g.gap_mm, g.from.x, g.from.y, g.to.x, g.to.y
            ),
            Self::Overlap { first, second } => write!(
                f,
                "les tracés n° {} et n° {} se superposent sur une longueur non nulle : supprimez le doublon",
                first + 1,
                second + 1
            ),
            Self::SelfTouching => write!(f, "le contour de la région touche son propre bord en un point"),
        }
    }
}

impl std::error::Error for RegionError {}

/// Une arête du contour d'une région, dans le sens du parcours.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionEdge {
    /// Indice du tracé source dans la liste fournie.
    pub source: usize,
    /// Intervalle de paramètre parcouru, dans le sens croissant du tracé
    /// (convention de [`crate::Hit`]). Un cercle peut avoir `t_hi > 1` : l'arête
    /// franchit l'angle 0.
    pub t_lo: f64,
    pub t_hi: f64,
    /// Parcourue de l'arrivée vers le départ du tracé source.
    pub reversed: bool,
    /// L'arête est le tracé source entier.
    pub whole: bool,
    pub start: PointMm,
    pub end: PointMm,
    /// Géométrie dans le sens du parcours (plusieurs éléments pour une courbe
    /// par points ou une arête fusionnée).
    pub geometry: Vec<EdgeGeom>,
}

/// Plus petite région fermée contenant le point visé.
#[derive(Debug, Clone, PartialEq)]
pub struct Region {
    /// Contour anti-horaire, arêtes bout à bout (la dernière finit où la première commence).
    pub edges: Vec<RegionEdge>,
    pub area_mm2: f64,
    pub perimeter_mm: f64,
    /// Polygone qui approche le contour à 1e-3 mm près (aperçu).
    pub outline: Vec<PointMm>,
}

#[derive(Clone, Copy)]
struct Cut {
    t: f64,
    p: PointMm,
}

struct GEdge {
    src: usize,
    t0: f64,
    t1: f64,
    a: usize,
    b: usize,
    geom: Vec<EdgeGeom>,
    whole: bool,
}

struct Graph {
    verts: Vec<PointMm>,
    edges: Vec<GEdge>,
}

struct Cycle {
    halves: Vec<usize>,
    area: f64,
    perimeter: f64,
    poly: Vec<PointMm>,
    bbox: Bbox,
}

/// Chevauchement de deux segments colinéaires : extrémités de la zone commune.
fn line_overlap(a: PointMm, b: PointMm, c: PointMm, d: PointMm) -> Option<(PointMm, PointMm)> {
    let u = b - a;
    let lu = u.norm();
    let uh = u.unit().ok()?;
    if uh.cross(c - a).abs() > EPSILON_MM || uh.cross(d - a).abs() > EPSILON_MM {
        return None;
    }
    let (sc, sd) = ((c - a).dot(uh), (d - a).dot(uh));
    let lo = sc.min(sd).max(0.0);
    let hi = sc.max(sd).min(lu);
    (hi - lo > EPSILON_MM).then(|| (a + uh * lo, a + uh * hi))
}

fn line_param(a: PointMm, b: PointMm, p: PointMm) -> f64 {
    let v = b - a;
    ((p - a).dot(v) / v.dot(v)).clamp(0.0, 1.0)
}

fn finalize_cuts(trace: &Trace, mut cuts: Vec<Cut>) -> Vec<Cut> {
    let tol = JOIN_TOLERANCE_MM;
    if let Trace::Circle(_) = trace {
        for c in &mut cuts {
            c.t = c.t.rem_euclid(1.0);
        }
        cuts.sort_by(|x, y| x.t.total_cmp(&y.t));
        let mut kept: Vec<Cut> = Vec::new();
        for c in cuts {
            if kept.last().is_some_and(|l| distance_mm(l.p, c.p) <= tol) {
                continue;
            }
            kept.push(c);
        }
        if kept.len() > 1 && distance_mm(kept[0].p, kept[kept.len() - 1].p) <= tol {
            kept.pop();
        }
        if kept.is_empty() {
            kept.push(Cut {
                t: 0.0,
                p: trace_start(trace),
            });
        }
        return kept;
    }
    cuts.sort_by(|x, y| x.t.total_cmp(&y.t));
    let mut kept: Vec<Cut> = Vec::new();
    for c in cuts {
        if let Some(last) = kept.last().copied() {
            let near = distance_mm(last.p, c.p) <= tol;
            let closing = kept.len() == 1 && last.t == 0.0 && c.t == 1.0;
            if near && !closing {
                if c.t == 1.0 && last.t != 0.0 {
                    let n = kept.len();
                    kept[n - 1] = c;
                }
                continue;
            }
        }
        kept.push(c);
    }
    kept
}

struct RawEdge {
    src: usize,
    t0: f64,
    t1: f64,
    pa: usize,
    pb: usize,
    whole: bool,
}

fn build_graph(traces: &[Trace]) -> Result<Graph, RegionError> {
    let n = traces.len();
    for (index, t) in traces.iter().enumerate() {
        check_trace(t).map_err(|error| RegionError::InvalidTrace { index, error })?;
    }
    let boxes: Vec<Bbox> = traces.iter().map(trace_bbox).collect();
    let mut cuts: Vec<Vec<Cut>> = vec![Vec::new(); n];
    for (i, t) in traces.iter().enumerate() {
        if !matches!(t, Trace::Circle(_)) {
            cuts[i].push(Cut {
                t: 0.0,
                p: trace_start(t),
            });
            cuts[i].push(Cut {
                t: 1.0,
                p: trace_end(t),
            });
        }
    }
    for i in 0..n {
        for j in (i + 1)..n {
            if !boxes_touch(&boxes[i], &boxes[j]) {
                continue;
            }
            if let (Trace::Line(a, b), Trace::Line(c, d)) = (&traces[i], &traces[j]) {
                if let Some((p, q)) = line_overlap(*a, *b, *c, *d) {
                    for pt in [p, q] {
                        cuts[i].push(Cut {
                            t: line_param(*a, *b, pt),
                            p: pt,
                        });
                        cuts[j].push(Cut {
                            t: line_param(*c, *d, pt),
                            p: pt,
                        });
                    }
                    continue;
                }
            }
            let hits = intersect(&traces[i], &traces[j]).map_err(|error| match error {
                GeometryError::OverlappingTraces => RegionError::Overlap {
                    first: i,
                    second: j,
                },
                error => RegionError::InvalidTrace { index: i, error },
            })?;
            for h in hits {
                cuts[i].push(Cut {
                    t: h.t1,
                    p: h.point,
                });
                cuts[j].push(Cut {
                    t: h.t2,
                    p: h.point,
                });
            }
        }
    }
    // Extrémité posée sur un autre tracé à 1e-6 mm près, sans intersection exacte.
    for i in 0..n {
        if matches!(traces[i], Trace::Circle(_)) {
            continue;
        }
        for end in [trace_start(&traces[i]), trace_end(&traces[i])] {
            for j in 0..n {
                if j == i {
                    continue;
                }
                let bb = &boxes[j];
                let tol = JOIN_TOLERANCE_MM;
                if end.x < bb.min.x - tol
                    || end.x > bb.max.x + tol
                    || end.y < bb.min.y - tol
                    || end.y > bb.max.y + tol
                {
                    continue;
                }
                let (t, d) = trace_nearest_param(&traces[j], end);
                if d <= tol {
                    cuts[j].push(Cut { t, p: end });
                }
            }
        }
    }

    // Arêtes brutes (entre coupes successives), points numérotés globalement.
    let mut pts: Vec<PointMm> = Vec::new();
    let mut raw: Vec<RawEdge> = Vec::new();
    for (i, trace) in traces.iter().enumerate() {
        let kept = finalize_cuts(trace, std::mem::take(&mut cuts[i]));
        let base = pts.len();
        pts.extend(kept.iter().map(|c| c.p));
        let m = kept.len();
        if let Trace::Circle(_) = trace {
            if m == 1 {
                raw.push(RawEdge {
                    src: i,
                    t0: kept[0].t,
                    t1: kept[0].t + 1.0,
                    pa: base,
                    pb: base,
                    whole: true,
                });
            } else {
                for k in 0..m {
                    let (t0, t1) = if k + 1 < m {
                        (kept[k].t, kept[k + 1].t)
                    } else {
                        (kept[k].t, kept[0].t + 1.0)
                    };
                    raw.push(RawEdge {
                        src: i,
                        t0,
                        t1,
                        pa: base + k,
                        pb: base + (k + 1) % m,
                        whole: false,
                    });
                }
            }
        } else {
            for k in 0..m.saturating_sub(1) {
                raw.push(RawEdge {
                    src: i,
                    t0: kept[k].t,
                    t1: kept[k + 1].t,
                    pa: base + k,
                    pb: base + k + 1,
                    whole: m == 2 && kept[0].t == 0.0 && kept[1].t == 1.0,
                });
            }
        }
    }
    let (ids, verts) = cluster(&pts);

    let mut edges: Vec<GEdge> = Vec::new();
    for r in raw {
        if r.t1 <= r.t0 {
            continue;
        }
        let (a, b) = (ids[r.pa], ids[r.pb]);
        let mut geom = sub_geometry(&traces[r.src], r.t0, r.t1);
        if geom.is_empty() {
            continue;
        }
        // Les extrémités des arêtes tombent exactement sur les sommets.
        let last = geom.len() - 1;
        if let EdgeGeom::Line(s, _) = &mut geom[0] {
            *s = verts[a];
        }
        if let EdgeGeom::Cubic(c) = &mut geom[0] {
            c.p0 = verts[a];
        }
        if let EdgeGeom::Line(_, e) = &mut geom[last] {
            *e = verts[b];
        }
        if let EdgeGeom::Cubic(c) = &mut geom[last] {
            c.p3 = verts[b];
        }
        let len: f64 = geom.iter().map(EdgeGeom::length_mm).sum();
        if a == b && len < MIN_LOOP_LENGTH_MM {
            continue;
        }
        edges.push(GEdge {
            src: r.src,
            t0: r.t0,
            t1: r.t1,
            a,
            b,
            geom,
            whole: r.whole,
        });
    }
    // Arêtes identiques (tracés superposés) : on n'en garde qu'une.
    let mut keep = vec![true; edges.len()];
    for i in 0..edges.len() {
        if !keep[i] {
            continue;
        }
        for j in (i + 1)..edges.len() {
            if !keep[j] {
                continue;
            }
            let (ei, ej) = (&edges[i], &edges[j]);
            let same_ends = (ei.a == ej.a && ei.b == ej.b) || (ei.a == ej.b && ei.b == ej.a);
            if same_ends
                && ei.geom.len() == ej.geom.len()
                && distance_mm(mid_of(&ei.geom), mid_of(&ej.geom)) <= JOIN_TOLERANCE_MM
            {
                keep[j] = false;
            }
        }
    }
    let mut it = keep.iter();
    edges.retain(|_| *it.next().unwrap_or(&true));
    Ok(Graph { verts, edges })
}

fn mid_of(geom: &[EdgeGeom]) -> PointMm {
    geom[geom.len() / 2].mid_point()
}

/// Regroupe les points distants de moins de [`JOIN_TOLERANCE_MM`] (chaînage inclus).
fn cluster(points: &[PointMm]) -> (Vec<usize>, Vec<PointMm>) {
    let n = points.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&i, &j| points[i].x.total_cmp(&points[j].x));
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for (oi, &i) in order.iter().enumerate() {
        for &j in &order[oi + 1..] {
            if points[j].x - points[i].x > JOIN_TOLERANCE_MM {
                break;
            }
            if distance_mm(points[i], points[j]) <= JOIN_TOLERANCE_MM {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                if ri != rj {
                    parent[rj.max(ri)] = rj.min(ri);
                }
            }
        }
    }
    let mut ids = vec![0usize; n];
    let mut sums: Vec<(f64, f64, usize)> = Vec::new();
    let mut root_id: Vec<Option<usize>> = vec![None; n];
    for i in 0..n {
        let r = find(&mut parent, i);
        let id = *root_id[r].get_or_insert_with(|| {
            sums.push((0.0, 0.0, 0));
            sums.len() - 1
        });
        ids[i] = id;
        sums[id].0 += points[i].x;
        sums[id].1 += points[i].y;
        sums[id].2 += 1;
    }
    let verts = sums
        .iter()
        .map(|(x, y, k)| PointMm::new(x / *k as f64, y / *k as f64))
        .collect();
    (ids, verts)
}

/// Arêtes dont la suppression sépare le graphe : elles ne bordent aucune région.
fn bridges(g: &Graph) -> Vec<bool> {
    let (nv, ne) = (g.verts.len(), g.edges.len());
    let mut adj: Vec<Vec<(usize, usize)>> = vec![Vec::new(); nv];
    for (e, ed) in g.edges.iter().enumerate() {
        if ed.a != ed.b {
            adj[ed.a].push((e, ed.b));
            adj[ed.b].push((e, ed.a));
        }
    }
    let mut disc = vec![0usize; nv];
    let mut low = vec![0usize; nv];
    let mut bridge = vec![false; ne];
    let mut timer = 1usize;
    for root in 0..nv {
        if disc[root] != 0 {
            continue;
        }
        disc[root] = timer;
        low[root] = timer;
        timer += 1;
        let mut stack: Vec<(usize, usize, usize)> = vec![(root, usize::MAX, 0)];
        while let Some(&(v, pe, idx)) = stack.last() {
            if idx < adj[v].len() {
                let top = stack.len() - 1;
                stack[top].2 += 1;
                let (e, w) = adj[v][idx];
                if e == pe {
                    continue;
                }
                if disc[w] == 0 {
                    disc[w] = timer;
                    low[w] = timer;
                    timer += 1;
                    stack.push((w, e, 0));
                } else {
                    low[v] = low[v].min(disc[w]);
                }
            } else {
                stack.pop();
                if let Some(&(u, _, _)) = stack.last() {
                    low[u] = low[u].min(low[v]);
                    if low[v] > disc[u] {
                        bridge[pe] = true;
                    }
                }
            }
        }
    }
    bridge
}

fn half_geom(g: &Graph, h: usize) -> Vec<EdgeGeom> {
    let e = &g.edges[h / 2];
    if h % 2 == 0 {
        e.geom.clone()
    } else {
        e.geom.iter().rev().map(EdgeGeom::reversed).collect()
    }
}

fn half_origin(g: &Graph, h: usize) -> usize {
    let e = &g.edges[h / 2];
    if h % 2 == 0 {
        e.a
    } else {
        e.b
    }
}

fn half_dest(g: &Graph, h: usize) -> usize {
    half_origin(g, h ^ 1)
}

#[derive(Clone, Copy)]
struct HalfInfo {
    dir: VectorMm,
    /// Courbure signée approchée (côté gauche > 0), pour départager deux tangentes égales.
    side: f64,
}

fn half_info(g: &Graph, h: usize) -> HalfInfo {
    let geom = half_geom(g, h);
    let origin = g.verts[half_origin(g, h)];
    let dest = g.verts[half_dest(g, h)];
    let dir = geom[0]
        .start_tangent()
        .or_else(|| unit_snapped(dest - origin))
        .unwrap_or(VectorMm::new(1.0, 0.0));
    let probe = geom[0].probe(1.0);
    let w = probe - origin;
    let d2 = w.dot(w);
    let side = if d2 <= 1e-24 { 0.0 } else { dir.cross(w) / d2 };
    HalfInfo { dir, side }
}

fn half_plane(u: VectorMm) -> u8 {
    if u.y > 0.0 || (u.y == 0.0 && u.x > 0.0) {
        0
    } else {
        1
    }
}

fn angular_cmp(a: &HalfInfo, b: &HalfInfo) -> std::cmp::Ordering {
    let (ha, hb) = (half_plane(a.dir), half_plane(b.dir));
    if ha != hb {
        return ha.cmp(&hb);
    }
    let cross = a.dir.cross(b.dir);
    if cross.abs() > 1e-9 {
        return 0.0_f64.total_cmp(&cross);
    }
    a.side.total_cmp(&b.side)
}

fn faces_of(g: &Graph) -> Vec<Cycle> {
    let ne = g.edges.len();
    let bridge = bridges(g);
    let nv = g.verts.len();
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); nv];
    let mut info: Vec<Option<HalfInfo>> = vec![None; 2 * ne];
    for (e, ed) in g.edges.iter().enumerate() {
        if bridge[e] {
            continue;
        }
        out[ed.a].push(2 * e);
        out[ed.b].push(2 * e + 1);
        info[2 * e] = Some(half_info(g, 2 * e));
        info[2 * e + 1] = Some(half_info(g, 2 * e + 1));
    }
    let mut pos = vec![usize::MAX; 2 * ne];
    for list in &mut out {
        list.sort_by(|&x, &y| match (info[x], info[y]) {
            (Some(ix), Some(iy)) => angular_cmp(&ix, &iy).then(x.cmp(&y)),
            _ => x.cmp(&y),
        });
        for (k, &h) in list.iter().enumerate() {
            pos[h] = k;
        }
    }
    let mut visited = vec![false; 2 * ne];
    let mut cycles = Vec::new();
    for start in 0..2 * ne {
        if visited[start] || pos[start] == usize::MAX {
            continue;
        }
        let mut halves = Vec::new();
        let mut h = start;
        while !visited[h] {
            visited[h] = true;
            halves.push(h);
            let v = half_dest(g, h);
            let list = &out[v];
            let k = pos[h ^ 1];
            h = list[(k + list.len() - 1) % list.len()];
        }
        if h != start {
            continue;
        }
        let origin = g.verts[half_origin(g, halves[0])];
        let (mut area, mut perimeter) = (0.0, 0.0);
        let mut poly = vec![origin];
        for &hh in &halves {
            for eg in half_geom(g, hh) {
                area += eg.signed_area_mm2(origin);
                perimeter += eg.length_mm();
                eg.flatten(&mut poly);
            }
        }
        poly.pop();
        if area.is_nan() || area <= MIN_REGION_AREA_MM2 || poly.len() < 3 {
            continue;
        }
        let mut bbox = Bbox::from_point(poly[0]);
        for p in &poly {
            bbox.include(*p);
        }
        cycles.push(Cycle {
            halves,
            area,
            perimeter,
            poly,
            bbox,
        });
    }
    cycles
}

fn dist_point_segment(p: PointMm, a: PointMm, b: PointMm) -> f64 {
    let v = b - a;
    let len2 = v.dot(v);
    let s = if len2 <= 0.0 {
        0.0
    } else {
        ((p - a).dot(v) / len2).clamp(0.0, 1.0)
    };
    distance_mm(a.lerp(b, s), p)
}

/// Le point est-il dans le polygone, bord compris ?
fn contains_inclusive(poly: &[PointMm], p: PointMm) -> bool {
    let n = poly.len();
    let mut winding = 0i32;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if dist_point_segment(p, a, b) <= EPSILON_MM {
            return true;
        }
        if a.y <= p.y {
            if b.y > p.y && (b - a).cross(p - a) > 0.0 {
                winding += 1;
            }
        } else if b.y <= p.y && (b - a).cross(p - a) < 0.0 {
            winding -= 1;
        }
    }
    winding != 0
}

fn smallest_containing(cycles: &[Cycle], click: PointMm) -> Option<usize> {
    let mut best: Option<usize> = None;
    for (i, c) in cycles.iter().enumerate() {
        let bb = &c.bbox;
        if click.x < bb.min.x - EPSILON_MM
            || click.x > bb.max.x + EPSILON_MM
            || click.y < bb.min.y - EPSILON_MM
            || click.y > bb.max.y + EPSILON_MM
        {
            continue;
        }
        if !contains_inclusive(&c.poly, click) {
            continue;
        }
        let better = match best {
            None => true,
            Some(b) => {
                let o = &cycles[b];
                c.area
                    .total_cmp(&o.area)
                    .then(c.perimeter.total_cmp(&o.perimeter))
                    .is_lt()
            }
        };
        if better {
            best = Some(i);
        }
    }
    best
}

fn circle_normalize(lo: f64, span: f64) -> (f64, f64) {
    let lo = lo.rem_euclid(1.0);
    (lo, lo + span)
}

fn region_from(g: &Graph, traces: &[Trace], cycle: &Cycle) -> Result<Region, RegionError> {
    let mut seen = std::collections::HashSet::new();
    for &h in &cycle.halves {
        if !seen.insert(half_origin(g, h)) {
            return Err(RegionError::SelfTouching);
        }
    }
    let mut edges: Vec<RegionEdge> = cycle
        .halves
        .iter()
        .map(|&h| {
            let e = &g.edges[h / 2];
            let geometry = half_geom(g, h);
            RegionEdge {
                source: e.src,
                t_lo: e.t0,
                t_hi: e.t1,
                reversed: h % 2 == 1,
                whole: e.whole,
                start: g.verts[half_origin(g, h)],
                end: g.verts[half_dest(g, h)],
                geometry,
            }
        })
        .collect();
    let is_circle = |src: usize| matches!(traces[src], Trace::Circle(_));
    let joins = |x: &RegionEdge, y: &RegionEdge| -> bool {
        if x.source != y.source || x.reversed != y.reversed || x.whole || y.whole {
            return false;
        }
        let close = |a: f64, b: f64| {
            if is_circle(x.source) {
                (a - b).rem_euclid(1.0).min((b - a).rem_euclid(1.0)) <= 1e-12
            } else {
                a == b
            }
        };
        if x.reversed {
            close(y.t_hi, x.t_lo)
        } else {
            close(x.t_hi, y.t_lo)
        }
    };
    let merge = |x: &mut RegionEdge, y: RegionEdge| {
        let span = (x.t_hi - x.t_lo) + (y.t_hi - y.t_lo);
        if x.reversed {
            x.t_lo = y.t_lo;
        } else {
            x.t_hi = y.t_hi;
        }
        if matches!(traces[x.source], Trace::Circle(_)) {
            let lo = if x.reversed { y.t_lo } else { x.t_lo };
            let (l, h) = circle_normalize(lo, span.min(1.0));
            x.t_lo = l;
            x.t_hi = h;
        }
        x.end = y.end;
        x.geometry.extend(y.geometry);
    };
    let mut merged: Vec<RegionEdge> = Vec::new();
    for e in edges.drain(..) {
        match merged.last_mut() {
            Some(last) if joins(last, &e) => merge(last, e),
            _ => merged.push(e),
        }
    }
    if merged.len() > 1 {
        let first_joins = joins(&merged[merged.len() - 1], &merged[0]);
        if first_joins {
            let first = merged.remove(0);
            let n = merged.len();
            merge(&mut merged[n - 1], first);
        }
    }
    for e in &mut merged {
        let circle = matches!(traces[e.source], Trace::Circle(_));
        e.whole = if circle {
            e.t_hi - e.t_lo >= 1.0 - 1e-12
        } else {
            e.t_lo <= 0.0 && e.t_hi >= 1.0
        };
    }
    Ok(Region {
        edges: merged,
        area_mm2: cycle.area,
        perimeter_mm: cycle.perimeter,
        outline: cycle.poly.clone(),
    })
}

struct GapCand {
    from: PointMm,
    to: PointMm,
    gap: f64,
    /// La ligne de fermeture dépasse `to` (cible : l'intérieur d'un tracé).
    overshoot: bool,
}

fn gap_candidates(traces: &[Trace], g: &Graph, click: PointMm) -> Vec<GapCand> {
    let mut degree = vec![0usize; g.verts.len()];
    let mut owner = vec![usize::MAX; g.verts.len()];
    for e in &g.edges {
        degree[e.a] += 1;
        degree[e.b] += 1;
        owner[e.a] = e.src;
        owner[e.b] = e.src;
    }
    let mut free: Vec<usize> = (0..g.verts.len()).filter(|&v| degree[v] == 1).collect();
    free.sort_by(|&a, &b| {
        distance_mm(g.verts[a], click).total_cmp(&distance_mm(g.verts[b], click))
    });
    free.truncate(MAX_FREE_ENDS);
    let mut out: Vec<GapCand> = Vec::new();
    for (k, &u) in free.iter().enumerate() {
        let pu = g.verts[u];
        for &w in &free[k + 1..] {
            let gap = distance_mm(pu, g.verts[w]);
            if gap > JOIN_TOLERANCE_MM {
                out.push(GapCand {
                    from: pu,
                    to: g.verts[w],
                    gap,
                    overshoot: false,
                });
            }
        }
        let mut best: Option<(f64, PointMm)> = None;
        for (j, tr) in traces.iter().enumerate() {
            if j == owner[u] {
                continue;
            }
            let (t, d) = trace_nearest_param(tr, pu);
            if d > JOIN_TOLERANCE_MM && best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, trace_point_at(tr, t)));
            }
        }
        if let Some((gap, to)) = best {
            out.push(GapCand {
                from: pu,
                to,
                gap,
                overshoot: true,
            });
        }
    }
    out.sort_by(|a, b| {
        let da = distance_mm(a.from.lerp(a.to, 0.5), click);
        let db = distance_mm(b.from.lerp(b.to, 0.5), click);
        a.gap.total_cmp(&b.gap).then(da.total_cmp(&db))
    });
    out
}

/// Plus petite région fermée contenant `click`.
///
/// Erreurs : [`RegionError::Open`] si combler une ouverture ferait de `click`
/// un point intérieur (les deux extrémités sont données), [`RegionError::NoRegion`]
/// sinon (hors de tout contour, ou rien de fermé).
pub fn find_region(traces: &[Trace], click: PointMm) -> Result<Region, RegionError> {
    click.validate().map_err(|_| RegionError::InvalidClick)?;
    let g = build_graph(traces)?;
    let cycles = faces_of(&g);
    if let Some(i) = smallest_containing(&cycles, click) {
        return region_from(&g, traces, &cycles[i]);
    }
    for cand in gap_candidates(traces, &g, click)
        .into_iter()
        .take(MAX_GAP_TRIES)
    {
        let end = if cand.overshoot {
            match (cand.to - cand.from).unit() {
                Ok(u) => cand.to + u * CLOSING_OVERSHOOT_MM,
                Err(_) => continue,
            }
        } else {
            cand.to
        };
        let mut extended = traces.to_vec();
        extended.push(Trace::Line(cand.from, end));
        let Ok(g2) = build_graph(&extended) else {
            continue;
        };
        if smallest_containing(&faces_of(&g2), click).is_some() {
            return Err(RegionError::Open(OpenGap {
                from: cand.from,
                to: cand.to,
                gap_mm: cand.gap,
            }));
        }
    }
    Err(RegionError::NoRegion)
}

impl Region {
    /// Sommets du contour dans l'ordre du parcours (début de chaque arête).
    pub fn vertices(&self) -> Vec<PointMm> {
        self.edges.iter().map(|e| e.start).collect()
    }
}

#[cfg(test)]
#[path = "region_tests.rs"]
mod tests;

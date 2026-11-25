//! Intersections générales de deux tracés (US-02.4) : segment, Bézier cubique,
//! courbe par points de passage, arc, cercle.
//!
//! Les tracés sont **bornés** (un segment n'est pas prolongé en droite, un arc
//! n'est pas prolongé en cercle). Chaque tracé est découpé en pièces simples
//! (segment, arc/cercle, Bézier cubique) puis :
//! - segment/segment, segment/cercle, cercle/cercle : formules fermées ;
//! - Bézier/segment et Bézier/cercle : polynôme en `t` (degré 3 ou 6) dont les
//!   racines dans `[0, 1]` sont isolées par ses points critiques (racines de la
//!   dérivée, récursivement) puis bissectées : aucune racine n'est manquée, la
//!   tangence (racine double) est reconnue par `|g| <= 1e-9 mm` au point critique ;
//! - Bézier/Bézier : subdivision (boîtes englobantes des polygones de contrôle)
//!   puis raffinement de Newton sur `A(t1) − B(t2) = 0`.
//!
//! Les solutions sont triées par `(t1, t2)` et sans doublon (deux solutions à
//! moins de 1e-6 mm l'une de l'autre sont fusionnées). Deux tracés qui se
//! superposent sur une longueur non nulle ont une infinité de solutions :
//! c'est une erreur ([`GeometryError::OverlappingTraces`]), jamais un résultat
//! tronqué.

use crate::{
    circle_circle_intersections, circle_line_intersections, distance_mm, segment_length_mm, Arc,
    Bbox, Circle, CubicBezier, GeometryError, PointMm, Result, Spline,
};
use std::f64::consts::TAU;

/// Précision visée des solutions et tolérance de tangence (mm).
pub const INTERSECTION_TOLERANCE_MM: f64 = 1e-9;
/// Deux solutions plus proches que ceci (mm) n'en font qu'une.
const DEDUP_MM: f64 = 1e-6;
/// Taille (mm) sous laquelle la subdivision Bézier/Bézier s'arrête.
const LEAF_MM: f64 = 1e-2;
/// Nombre maximal de nœuds de subdivision par paire de Béziers.
const MAX_NODES: usize = 200_000;

const TOL: f64 = INTERSECTION_TOLERANCE_MM;

/// Tracé borné.
#[derive(Debug, Clone, PartialEq)]
pub enum Trace {
    Line(PointMm, PointMm),
    Cubic(CubicBezier),
    Spline(Spline),
    Arc(Arc),
    /// Cercle entier (paramètre : angle / 2π depuis l'axe +X).
    Circle(Circle),
}

/// Une solution : le point et le paramètre de chaque tracé dans `[0, 1]`
/// (segment : fraction de la longueur ; Bézier : `t` ; courbe par points :
/// `(tronçon + t) / nombre de tronçons` ; arc : fraction du balayage ;
/// cercle : angle / 2π).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub point: PointMm,
    pub t1: f64,
    pub t2: f64,
}

enum Prim {
    Line(PointMm, PointMm),
    Cubic(CubicBezier),
    Round(Circle, Option<Arc>),
}

struct Piece {
    prim: Prim,
    /// Paramètre du tracé = `off + scale * paramètre local`.
    off: f64,
    scale: f64,
    bbox: Bbox,
}

fn pieces(tr: &Trace) -> Result<Vec<Piece>> {
    let one = |prim: Prim, bbox: Bbox| {
        vec![Piece {
            prim,
            off: 0.0,
            scale: 1.0,
            bbox,
        }]
    };
    Ok(match tr {
        Trace::Line(a, b) => {
            segment_length_mm(*a, *b)?;
            let mut bb = Bbox::from_point(*a);
            bb.include(*b);
            one(Prim::Line(*a, *b), bb)
        }
        Trace::Cubic(c) => {
            c.validate()?;
            one(Prim::Cubic(*c), c.bbox())
        }
        Trace::Arc(a) => one(
            Prim::Round(Circle::new(a.center, a.radius_mm)?, Some(*a)),
            a.bbox(),
        ),
        Trace::Circle(c) => one(Prim::Round(*c, None), c.bbox()),
        Trace::Spline(s) => {
            let n = s.segments().len() as f64;
            s.segments()
                .iter()
                .enumerate()
                .map(|(i, c)| Piece {
                    prim: Prim::Cubic(*c),
                    off: i as f64 / n,
                    scale: 1.0 / n,
                    bbox: c.bbox(),
                })
                .collect()
        }
    })
}

/// Intersections de deux tracés bornés, triées par `(t1, t2)`, sans doublon.
/// Vide si les tracés ne se coupent pas ; erreur s'ils se superposent.
pub fn intersect(a: &Trace, b: &Trace) -> Result<Vec<Hit>> {
    let (pa, pb) = (pieces(a)?, pieces(b)?);
    let mut hits: Vec<Hit> = Vec::new();
    for p in &pa {
        for q in &pb {
            if p.bbox.max.x + TOL < q.bbox.min.x
                || q.bbox.max.x + TOL < p.bbox.min.x
                || p.bbox.max.y + TOL < q.bbox.min.y
                || q.bbox.max.y + TOL < p.bbox.min.y
            {
                continue;
            }
            for (tp, tq, point) in prim_hits(&p.prim, &q.prim)? {
                hits.push(Hit {
                    point,
                    t1: (p.off + p.scale * tp).clamp(0.0, 1.0),
                    t2: (q.off + q.scale * tq).clamp(0.0, 1.0),
                });
            }
        }
    }
    hits.sort_by(|x, y| x.t1.total_cmp(&y.t1).then(x.t2.total_cmp(&y.t2)));
    let mut out: Vec<Hit> = Vec::with_capacity(hits.len());
    for h in hits {
        if out.iter().all(|k| distance_mm(k.point, h.point) > DEDUP_MM) {
            out.push(h);
        }
    }
    Ok(out)
}

type Raw = Vec<(f64, f64, PointMm)>;

fn swapped(r: Raw) -> Raw {
    r.into_iter().map(|(a, b, p)| (b, a, p)).collect()
}

fn prim_hits(p: &Prim, q: &Prim) -> Result<Raw> {
    match (p, q) {
        (Prim::Line(a, b), Prim::Line(c, d)) => line_line(*a, *b, *c, *d),
        (Prim::Line(a, b), Prim::Round(c, arc)) => line_round(*a, *b, c, arc.as_ref()),
        (Prim::Round(..), Prim::Line(..)) => Ok(swapped(prim_hits(q, p)?)),
        (Prim::Round(c1, a1), Prim::Round(c2, a2)) => round_round(c1, a1.as_ref(), c2, a2.as_ref()),
        (Prim::Cubic(c), Prim::Line(a, b)) => cubic_line(c, *a, *b),
        (Prim::Line(..), Prim::Cubic(..)) => Ok(swapped(prim_hits(q, p)?)),
        (Prim::Cubic(c), Prim::Round(k, arc)) => cubic_round(c, k, arc.as_ref()),
        (Prim::Round(..), Prim::Cubic(..)) => Ok(swapped(prim_hits(q, p)?)),
        (Prim::Cubic(c), Prim::Cubic(d)) => cubic_cubic(c, d),
    }
}

fn overlap_err() -> GeometryError {
    GeometryError::OverlappingTraces
}

// ---------------------------------------------------------------- segment / segment

fn line_line(a: PointMm, b: PointMm, c: PointMm, d: PointMm) -> Result<Raw> {
    let (u, v) = (b - a, d - c);
    let (lu, lv) = (segment_length_mm(a, b)?, segment_length_mm(c, d)?);
    let den = u.cross(v);
    let r = c - a;
    if den.abs() <= 1e-12 * lu * lv {
        // Parallèles : distincts, ou colinéaires.
        let uh = u.unit()?;
        if uh.cross(r).abs() > TOL || uh.cross(d - a).abs() > TOL {
            return Ok(Vec::new());
        }
        let (sc, sd) = (r.dot(uh), (d - a).dot(uh));
        let (lo, hi) = (sc.min(sd).max(0.0), sc.max(sd).min(lu));
        if hi - lo > TOL {
            return Err(overlap_err());
        }
        if hi - lo < -TOL {
            return Ok(Vec::new());
        }
        // Contact par une extrémité commune.
        let p = a + uh * (0.5 * (lo + hi));
        let p = [a, b, c, d]
            .into_iter()
            .find(|e| distance_mm(*e, p) <= TOL)
            .unwrap_or(p);
        let ta = ((p - a).dot(uh) / lu).clamp(0.0, 1.0);
        let tc = ((p - c).dot(v) / (lv * lv)).clamp(0.0, 1.0);
        return Ok(vec![(ta, tc, p)]);
    }
    let t = r.cross(v) / den;
    let s = r.cross(u) / den;
    let (et, es) = (TOL / lu, TOL / lv);
    if t < -et || t > 1.0 + et || s < -es || s > 1.0 + es {
        return Ok(Vec::new());
    }
    let t = t.clamp(0.0, 1.0);
    Ok(vec![(t, s.clamp(0.0, 1.0), a + u * t)])
}

// ---------------------------------------------------------------- arcs et cercles

/// Paramètre local d'un point supposé sur le cercle : fraction du balayage
/// (arc) ou angle / 2π (cercle) ; `None` si le point est hors de l'arc.
fn round_param(c: &Circle, arc: Option<&Arc>, p: PointMm) -> Option<f64> {
    match arc {
        Some(a) => a.fraction_of(p),
        None => {
            let v = p - c.center;
            (v.norm() > crate::EPSILON_MM).then(|| v.y.atan2(v.x).rem_euclid(TAU) / TAU)
        }
    }
}

fn line_round(a: PointMm, b: PointMm, c: &Circle, arc: Option<&Arc>) -> Result<Raw> {
    let len = segment_length_mm(a, b)?;
    let u = (b - a).unit()?;
    let mut out = Vec::new();
    for p in circle_line_intersections(c, a, b)? {
        let along = (p - a).dot(u);
        if !(-TOL..=len + TOL).contains(&along) {
            continue;
        }
        if let Some(tr) = round_param(c, arc, p) {
            out.push(((along / len).clamp(0.0, 1.0), tr, p));
        }
    }
    Ok(out)
}

/// Intervalle angulaire anti-horaire `(départ, longueur)` d'un arc ou d'un cercle.
fn interval(arc: Option<&Arc>) -> (f64, f64) {
    match arc {
        None => (0.0, TAU),
        Some(a) if a.sweep_rad > 0.0 => (a.start_rad.rem_euclid(TAU), a.sweep_rad),
        Some(a) => ((a.start_rad + a.sweep_rad).rem_euclid(TAU), -a.sweep_rad),
    }
}

fn round_round(c1: &Circle, a1: Option<&Arc>, c2: &Circle, a2: Option<&Arc>) -> Result<Raw> {
    let coincident =
        distance_mm(c1.center, c2.center) <= TOL && (c1.radius_mm - c2.radius_mm).abs() <= TOL;
    let candidates: Vec<PointMm> = if coincident {
        let tol = TOL / c1.radius_mm;
        let ((s1, l1), (s2, l2)) = (interval(a1), interval(a2));
        let d = (s2 - s1).rem_euclid(TAU);
        let d2 = (s1 - s2).rem_euclid(TAU);
        if l1 >= TAU - tol || l2 >= TAU - tol || d < l1 - tol || d2 < l2 - tol {
            return Err(overlap_err());
        }
        // Arcs disjoints : ils ne peuvent se toucher que par leurs extrémités.
        [a1, a2]
            .into_iter()
            .flatten()
            .flat_map(|a| [a.start_point(), a.end_point()])
            .collect()
    } else {
        circle_circle_intersections(c1, c2)?
    };
    Ok(candidates
        .into_iter()
        .filter_map(|p| Some((round_param(c1, a1, p)?, round_param(c2, a2, p)?, p)))
        .collect())
}

// ---------------------------------------------------------------- polynômes sur [0, 1]

fn horner(c: &[f64], t: f64) -> f64 {
    c.iter().rev().fold(0.0, |acc, k| acc * t + k)
}

/// Racines de `c[0] + c[1] t + …` dans `[0, 1]`. Entre deux points critiques le
/// polynôme est monotone : au plus une racine, trouvée par bissection. Un point
/// critique (ou une extrémité) où `|g| <= tol` est une racine (tangence).
fn unit_roots(c: &[f64], tol: f64) -> Vec<f64> {
    let m = c.iter().fold(0.0_f64, |a, k| a.max(k.abs()));
    let mut c = c.to_vec();
    while c.len() > 1 && c.last().is_some_and(|k| k.abs() <= 1e-14 * m) {
        c.pop();
    }
    let mut pts = vec![0.0];
    if c.len() >= 3 {
        let d: Vec<f64> = c
            .iter()
            .enumerate()
            .skip(1)
            .map(|(i, k)| k * i as f64)
            .collect();
        pts.extend(
            unit_roots(&d, 0.0)
                .into_iter()
                .filter(|t| *t > 0.0 && *t < 1.0),
        );
    }
    pts.push(1.0);
    let mut roots = Vec::new();
    for w in pts.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        let (flo, fhi) = (horner(&c, lo), horner(&c, hi));
        if flo.abs() <= tol {
            roots.push(lo);
        } else if fhi.abs() > tol && (flo < 0.0) != (fhi < 0.0) {
            let (mut a, mut b) = (lo, hi);
            for _ in 0..200 {
                let mid = 0.5 * (a + b);
                if mid <= a || mid >= b {
                    break;
                }
                let fm = horner(&c, mid);
                if fm == 0.0 {
                    a = mid;
                    b = mid;
                    break;
                }
                if (fm < 0.0) == (flo < 0.0) {
                    a = mid;
                } else {
                    b = mid;
                }
            }
            roots.push(0.5 * (a + b));
        }
    }
    if horner(&c, 1.0).abs() <= tol {
        roots.push(1.0);
    }
    roots.sort_by(f64::total_cmp);
    roots.dedup_by(|a, b| (*a - *b).abs() <= 1e-12);
    roots
}

/// Coefficients de `B(t) − origine` en base de puissances (k0 + k1 t + k2 t² + k3 t³).
fn power_basis(c: &CubicBezier, o: PointMm) -> [(f64, f64); 4] {
    let (q0, q1, q2, q3) = (c.p0 - o, c.c1 - o, c.c2 - o, c.p3 - o);
    let f = |a: f64, b: f64, c: f64, d: f64| {
        [
            a,
            3.0 * (b - a),
            3.0 * (a - 2.0 * b + c),
            d - 3.0 * c + 3.0 * b - a,
        ]
    };
    let (x, y) = (f(q0.x, q1.x, q2.x, q3.x), f(q0.y, q1.y, q2.y, q3.y));
    [(x[0], y[0]), (x[1], y[1]), (x[2], y[2]), (x[3], y[3])]
}

fn cubic_line(cb: &CubicBezier, a: PointMm, b: PointMm) -> Result<Raw> {
    let len = segment_length_mm(a, b)?;
    let u = (b - a).unit()?;
    let ctrl = [cb.p0, cb.c1, cb.c2, cb.p3];
    if ctrl.iter().all(|p| u.cross(*p - a).abs() <= TOL) {
        // Bézier aplatie sur la droite : superposée si elle recouvre le segment.
        let al: Vec<f64> = ctrl.iter().map(|p| (*p - a).dot(u)).collect();
        let lo = al.iter().copied().fold(f64::INFINITY, f64::min).max(0.0);
        let hi = al
            .iter()
            .copied()
            .fold(f64::NEG_INFINITY, f64::max)
            .min(len);
        if hi - lo > TOL {
            return Err(overlap_err());
        }
        return Ok(Vec::new());
    }
    let g: Vec<f64> = power_basis(cb, a)
        .iter()
        .map(|k| u.x * k.1 - u.y * k.0)
        .collect();
    let mut out = Vec::new();
    for t in unit_roots(&g, TOL) {
        let p = cb.point_at(t);
        let along = (p - a).dot(u);
        if (-TOL..=len + TOL).contains(&along) {
            out.push((t, (along / len).clamp(0.0, 1.0), p));
        }
    }
    Ok(out)
}

fn cubic_round(cb: &CubicBezier, c: &Circle, arc: Option<&Arc>) -> Result<Raw> {
    let r = c.radius_mm;
    let k = power_basis(cb, c.center);
    let mut g = [0.0_f64; 7];
    for (i, ki) in k.iter().enumerate() {
        for (j, kj) in k.iter().enumerate() {
            g[i + j] += ki.0 * kj.0 + ki.1 * kj.1;
        }
    }
    g[0] -= r * r;
    for x in &mut g {
        *x /= 2.0 * r;
    }
    let mut out = Vec::new();
    for t in unit_roots(&g, TOL) {
        let p = cb.point_at(t);
        if let Some(tr) = round_param(c, arc, p) {
            out.push((t, tr, p));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- Bézier / Bézier

fn split(c: &CubicBezier) -> (CubicBezier, CubicBezier) {
    let m = |a: PointMm, b: PointMm| a.lerp(b, 0.5);
    let (p01, p12, p23) = (m(c.p0, c.c1), m(c.c1, c.c2), m(c.c2, c.p3));
    let (p012, p123) = (m(p01, p12), m(p12, p23));
    let mid = m(p012, p123);
    (
        CubicBezier::new(c.p0, p01, p012, mid),
        CubicBezier::new(mid, p123, p23, c.p3),
    )
}

fn ctrl_box(c: &CubicBezier) -> Bbox {
    let mut bb = Bbox::from_point(c.p0);
    for p in [c.c1, c.c2, c.p3] {
        bb.include(p);
    }
    bb
}

fn size(b: &Bbox) -> f64 {
    b.width().max(b.height())
}

#[derive(Clone, Copy)]
struct Sub {
    curve: CubicBezier,
    t0: f64,
    t1: f64,
}

fn candidates(a: Sub, b: Sub, out: &mut Vec<(f64, f64)>, budget: &mut usize) -> Result<()> {
    if *budget == 0 {
        return Err(overlap_err());
    }
    *budget -= 1;
    let (ba, bb) = (ctrl_box(&a.curve), ctrl_box(&b.curve));
    if ba.max.x + TOL < bb.min.x
        || bb.max.x + TOL < ba.min.x
        || ba.max.y + TOL < bb.min.y
        || bb.max.y + TOL < ba.min.y
    {
        return Ok(());
    }
    let (sa, sb) = (size(&ba), size(&bb));
    if sa <= LEAF_MM && sb <= LEAF_MM {
        out.push((0.5 * (a.t0 + a.t1), 0.5 * (b.t0 + b.t1)));
        return Ok(());
    }
    let halves = |s: Sub| {
        let (l, r) = split(&s.curve);
        let m = 0.5 * (s.t0 + s.t1);
        (
            Sub {
                curve: l,
                t0: s.t0,
                t1: m,
            },
            Sub {
                curve: r,
                t0: m,
                t1: s.t1,
            },
        )
    };
    if sa > LEAF_MM && sb > LEAF_MM {
        let ((a1, a2), (b1, b2)) = (halves(a), halves(b));
        for (x, y) in [(a1, b1), (a1, b2), (a2, b1), (a2, b2)] {
            candidates(x, y, out, budget)?;
        }
    } else if sa > LEAF_MM {
        let (a1, a2) = halves(a);
        candidates(a1, b, out, budget)?;
        candidates(a2, b, out, budget)?;
    } else {
        let (b1, b2) = halves(b);
        candidates(a, b1, out, budget)?;
        candidates(a, b2, out, budget)?;
    }
    Ok(())
}

/// Dérivée seconde d'une Bézier cubique.
fn second_derivative(c: &CubicBezier, t: f64) -> (f64, f64) {
    let f = |a: f64, b: f64, c: f64, d: f64| {
        6.0 * ((1.0 - t) * (a - 2.0 * b + c) + t * (b - 2.0 * c + d))
    };
    (
        f(c.p0.x, c.c1.x, c.c2.x, c.p3.x),
        f(c.p0.y, c.c1.y, c.c2.y, c.p3.y),
    )
}

/// Contact tangent : la jacobienne de `A − B` est singulière à la solution
/// (racine double, Newton ne gagne qu'un bit par pas). On résout alors
/// `A − B = 0` avec `A' ∥ B'` (trois équations, rang 2 : Gauss–Newton
/// quadratique). Résultat refusé s'il n'est pas un vrai contact à 1e-9 mm.
fn tangent_polish(
    a: &CubicBezier,
    b: &CubicBezier,
    mut t1: f64,
    mut t2: f64,
) -> Option<(f64, f64)> {
    let scale = a.chord_mm().max(b.chord_mm()).max(1e-3);
    for _ in 0..40 {
        let (da, db) = (a.derivative_at(t1), b.derivative_at(t2));
        let norm = da.norm() * db.norm();
        if norm <= 1e-300 {
            return None;
        }
        let f = a.point_at(t1) - b.point_at(t2);
        let (a2, b2) = (second_derivative(a, t1), second_derivative(b, t2));
        let e = scale * da.cross(db) / norm;
        let rows = [
            ([da.x, -db.x], f.x),
            ([da.y, -db.y], f.y),
            (
                [
                    scale * (a2.0 * db.y - a2.1 * db.x) / norm,
                    scale * (da.x * b2.1 - da.y * b2.0) / norm,
                ],
                e,
            ),
        ];
        let (mut m11, mut m12, mut m22, mut r1, mut r2) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for (j, r) in rows {
            m11 += j[0] * j[0];
            m12 += j[0] * j[1];
            m22 += j[1] * j[1];
            r1 -= j[0] * r;
            r2 -= j[1] * r;
        }
        let det = m11 * m22 - m12 * m12;
        if det.abs() <= 1e-300 {
            return None;
        }
        let (d1, d2) = ((r1 * m22 - m12 * r2) / det, (m11 * r2 - m12 * r1) / det);
        if !d1.is_finite() || !d2.is_finite() {
            return None;
        }
        let (n1, n2) = ((t1 + d1).clamp(0.0, 1.0), (t2 + d2).clamp(0.0, 1.0));
        let moved = (n1 - t1).abs().max((n2 - t2).abs());
        t1 = n1;
        t2 = n2;
        if moved <= 1e-14 {
            break;
        }
    }
    (distance_mm(a.point_at(t1), b.point_at(t2)) <= TOL).then_some((t1, t2))
}

/// Newton sur `A(t1) − B(t2) = 0` ; Levenberg–Marquardt si la jacobienne est
/// presque singulière, puis affinage de contact tangent.
fn refine(a: &CubicBezier, b: &CubicBezier, mut t1: f64, mut t2: f64) -> Option<(f64, f64)> {
    for _ in 0..200 {
        let f = a.point_at(t1) - b.point_at(t2);
        let (da, db) = (a.derivative_at(t1), b.derivative_at(t2));
        let det = -da.cross(db);
        let (d1, d2) = if det.abs() > 1e-12 * da.norm() * db.norm() {
            (
                (f.x * db.y - db.x * f.y) / det,
                -(da.x * f.y - da.y * f.x) / det,
            )
        } else {
            let (aa, ab, bb) = (da.dot(da), da.dot(db), db.dot(db));
            let lam = 1e-6 * (aa + bb) + 1e-300;
            let (m11, m12, m22) = (aa + lam, -ab, bb + lam);
            let (r1, r2) = (-da.dot(f), db.dot(f));
            let det2 = m11 * m22 - m12 * m12;
            ((r1 * m22 - m12 * r2) / det2, (m11 * r2 - m12 * r1) / det2)
        };
        if !d1.is_finite() || !d2.is_finite() {
            return None;
        }
        let n1 = (t1 + d1.clamp(-0.25, 0.25)).clamp(0.0, 1.0);
        let n2 = (t2 + d2.clamp(-0.25, 0.25)).clamp(0.0, 1.0);
        let moved = (n1 - t1).abs().max((n2 - t2).abs());
        t1 = n1;
        t2 = n2;
        // Arrêt sur le pas, pas sur le résidu : à une tangence le résidu est
        // quadratique et passerait sous 1e-13 à 1e-7 mm de la solution.
        if moved <= 1e-14 {
            break;
        }
    }
    let r = distance_mm(a.point_at(t1), b.point_at(t2));
    if r > TOL {
        return None;
    }
    let (da, db) = (a.derivative_at(t1), b.derivative_at(t2));
    if da.cross(db).abs() <= 1e-4 * da.norm() * db.norm() {
        if let Some((s1, s2)) = tangent_polish(a, b, t1, t2) {
            if distance_mm(a.point_at(s1), a.point_at(t1)) <= 1e-3 {
                return Some((s1, s2));
            }
        }
    }
    Some((t1, t2))
}

fn cubic_cubic(a: &CubicBezier, b: &CubicBezier) -> Result<Raw> {
    let mut cands = Vec::new();
    let mut budget = MAX_NODES;
    candidates(
        Sub {
            curve: *a,
            t0: 0.0,
            t1: 1.0,
        },
        Sub {
            curve: *b,
            t0: 0.0,
            t1: 1.0,
        },
        &mut cands,
        &mut budget,
    )?;
    let mut out: Raw = Vec::new();
    for (s, t) in cands {
        if let Some((t1, t2)) = refine(a, b, s, t) {
            let p = a.point_at(t1);
            if out.iter().all(|(_, _, q)| distance_mm(*q, p) > DEDUP_MM) {
                out.push((t1, t2, p));
            }
        }
    }
    // Deux cubiques se coupent en 9 points au plus (Bézout) : davantage de
    // solutions distinctes signalent des courbes superposées.
    if out.len() > 9 {
        return Err(overlap_err());
    }
    Ok(out)
}

#[cfg(test)]
#[path = "intersect_tests.rs"]
mod tests;

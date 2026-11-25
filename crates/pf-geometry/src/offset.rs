//! Décalage (offset) d'une chaîne de tracés bout à bout (US-02.10).
//!
//! `offset_chain(chain, d, options)` renvoie la courbe parallèle à `chain` à la
//! distance signée `d` (`d > 0` = à **gauche** du sens de parcours de la chaîne,
//! `d < 0` = à droite). La chaîne est ouverte ou fermée ; ses pièces sont des
//! segments, des arcs, des Béziers cubiques ou des courbes lisses (un cercle
//! entier se décale seul).
//!
//! # Méthode
//!
//! 1. **Courbe brute.** Chaque pièce est décalée : un segment donne un segment,
//!    un arc de rayon `r` un arc concentrique de rayon `r ∓ d` (exact), une
//!    Bézier une suite de Béziers (voir plus bas). Aux sommets de la chaîne où la
//!    tangente change de direction, un coin **extérieur** (le côté décalé est à
//!    l'extérieur du virage) reçoit un raccord : arc de rayon `|d|` centré sur le
//!    sommet (`Round`), ou prolongement des deux tangentes jusqu'à leur
//!    intersection (`Miter`), repli en biseau (segment) quand la pointe dépasse
//!    `miter_limit × |d|` ou que le virage est un demi-tour. Un coin **intérieur**
//!    ne reçoit rien : les deux pièces décalées se croisent.
//! 2. **Auto-intersections.** Toutes les intersections entre pièces brutes sont
//!    calculées avec [`crate::intersect`] (les contacts aux extrémités partagées
//!    sont ignorés) et les pièces sont coupées en ces points.
//! 3. **Filtre par la distance.** Un morceau est gardé seulement si ses points
//!    d'échantillonnage sont à la distance voulue du tracé d'origine (`≥ |d|`,
//!    à 1e-6 mm près ; `1e-4` mm pour les Béziers approchées). Les boucles
//!    parasites, les parties retournées (rayon de courbure < `d`) et les coins
//!    intérieurs sont ainsi coupés exactement à leur intersection. Les morceaux
//!    « retournés » (arc dont le rayon décalé est négatif, partie de Bézier où
//!    `1 − d·κ < 0`) sont éliminés d'office.
//! 4. Les segments consécutifs colinéaires issus d'un raccord sont refondus.
//!
//! # Courbes de Bézier et lisses
//!
//! Le décalage exact d'une cubique n'est pas une cubique. Chaque cubique est
//! coupée aux paramètres où `1 − d·κ(t)` s'annule (points de rebroussement du
//! décalage), puis chaque intervalle est approché par une Bézier d'Hermite
//! `o(t) = C(t) + d·N(t)`, `o'(t) = C'(t)·(1 − d·κ(t))`. L'intervalle est
//! bissecté jusqu'à ce que l'écart entre la Bézier et le décalage exact, en
//! cinq paramètres intermédiaires, soit ≤ [`OFFSET_FIT_TOLERANCE_MM`] (2e-5 mm,
//! très en dessous des 0,005 mm exigés). La distance au tracé d'origine reste
//! donc `|d|` à 2e-5 mm près sur les courbes, et exacte sur les segments et arcs.
//!
//! # Limites connues
//!
//! - Un coin extérieur en pointe (`Miter`) est plus loin que `|d|` de l'original
//!   (jusqu'à `miter_limit × |d|`) : c'est le prolongement voulu.
//! - Une boucle parasite dont tous les points sont à moins de 1e-6 mm (segments,
//!   arcs) ou 1e-4 mm (Béziers) de la distance voulue ne se distingue pas d'une
//!   partie valide : elle peut subsister (angles de moins de ~2° pour les
//!   Béziers).
//! - Une chaîne qui se recoupe elle-même est traitée comme le reste (la distance
//!   est mesurée à toute la chaîne), sans notion d'« intérieur » du contour.

use crate::{
    distance_mm, project_on_segment, segment_length_mm, Arc, Bbox, Circle, CubicBezier,
    GeometryError, PointMm, Result, Trace, VectorMm, MAX_COORD_MM,
};
use serde::{Deserialize, Serialize};
use std::f64::consts::PI;

/// Écart maximal (mm) entre une Bézier de décalage et le décalage exact.
pub const OFFSET_FIT_TOLERANCE_MM: f64 = 2e-5;
/// Plus grande distance de décalage acceptée (mm).
pub const MAX_OFFSET_MM: f64 = 100_000.0;
/// Limite de pointe par défaut : longueur de la pointe ÷ `|d|` (valeur de SVG).
pub const DEFAULT_MITER_LIMIT: f64 = 4.0;
/// Nombre maximal de pièces dans la chaîne d'entrée.
pub const MAX_OFFSET_PIECES: usize = 5_000;
/// Nombre maximal de pièces brutes (après approximation des courbes).
const MAX_RAW_PIECES: usize = 40_000;
/// Deux extrémités plus proches que cela (mm) sont reliées.
const CHAIN_TOL: f64 = 1e-6;
/// Rayon (mm) autour d'une extrémité partagée où une intersection est ignorée.
const JOIN_TOL: f64 = 1e-5;
const EXACT_EPS: f64 = 1e-8;
const FIT_EPS: f64 = 5.0 * OFFSET_FIT_TOLERANCE_MM;

/// Traitement des coins extérieurs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CornerStyle {
    /// Prolongement jusqu'à l'intersection, repli en biseau (défaut).
    #[default]
    Miter,
    /// Arc de raccord de rayon `|d|` centré sur le sommet.
    Round,
}

/// Options du décalage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OffsetOptions {
    pub corner: CornerStyle,
    /// Longueur de pointe maximale ÷ `|d|` avant repli en biseau (≥ 1).
    pub miter_limit: f64,
}

impl Default for OffsetOptions {
    fn default() -> Self {
        OffsetOptions {
            corner: CornerStyle::Miter,
            miter_limit: DEFAULT_MITER_LIMIT,
        }
    }
}

fn err(msg: &str) -> GeometryError {
    GeometryError::InvalidConstruction(msg.to_string())
}

// ------------------------------------------------------------------ primitives

#[derive(Clone, Copy, Debug)]
enum Prim {
    Line(PointMm, PointMm),
    Arc(Arc),
    Cubic(CubicBezier),
}

impl Prim {
    fn start(&self) -> PointMm {
        match self {
            Prim::Line(a, _) => *a,
            Prim::Arc(a) => a.start_point(),
            Prim::Cubic(c) => c.p0,
        }
    }

    fn end(&self) -> PointMm {
        match self {
            Prim::Line(_, b) => *b,
            Prim::Arc(a) => a.end_point(),
            Prim::Cubic(c) => c.p3,
        }
    }

    fn start_tangent(&self) -> Result<VectorMm> {
        match self {
            Prim::Line(a, b) => (*b - *a).unit(),
            Prim::Arc(a) => Ok(a.point_at_length(0.0)?.1),
            Prim::Cubic(c) => c.unit_tangent_at(0.0),
        }
    }

    fn end_tangent(&self) -> Result<VectorMm> {
        match self {
            Prim::Line(a, b) => (*b - *a).unit(),
            Prim::Arc(a) => Ok(a.point_at_length(a.length_mm())?.1),
            Prim::Cubic(c) => c.unit_tangent_at(1.0),
        }
    }

    fn bbox(&self) -> Bbox {
        match self {
            Prim::Line(a, b) => {
                let mut bb = Bbox::from_point(*a);
                bb.include(*b);
                bb
            }
            Prim::Arc(a) => a.bbox(),
            Prim::Cubic(c) => c.bbox(),
        }
    }

    /// Point à la fraction `u` du paramètre.
    fn point_at(&self, u: f64) -> PointMm {
        match self {
            Prim::Line(a, b) => a.lerp(*b, u),
            Prim::Arc(a) => {
                let ang = a.start_rad + a.sweep_rad * u;
                PointMm::new(
                    a.center.x + a.radius_mm * ang.cos(),
                    a.center.y + a.radius_mm * ang.sin(),
                )
            }
            Prim::Cubic(c) => c.point_at(u),
        }
    }

    fn distance(&self, p: PointMm) -> f64 {
        match self {
            Prim::Line(a, b) => project_on_segment(p, *a, *b)
                .map(|r| r.1)
                .unwrap_or(f64::INFINITY),
            Prim::Arc(a) => a.distance_to_point(p),
            Prim::Cubic(c) => cubic_distance(c, p),
        }
    }

    fn trace(self) -> Trace {
        match self {
            Prim::Line(a, b) => Trace::Line(a, b),
            Prim::Arc(a) => Trace::Arc(a),
            Prim::Cubic(c) => Trace::Cubic(c),
        }
    }

    /// Longueur minimale pour qu'un morceau soit conservé.
    fn is_negligible(&self) -> bool {
        match self {
            Prim::Line(a, b) => distance_mm(*a, *b) < 1e-9,
            Prim::Arc(a) => a.length_mm() < 1e-8,
            Prim::Cubic(c) => {
                distance_mm(c.p0, c.p3) < 1e-9
                    && distance_mm(c.p0, c.c1) + distance_mm(c.c1, c.c2) + distance_mm(c.c2, c.p3)
                        < 1e-9
            }
        }
    }
}

/// Distance d'un point à une Bézier : chaque minimum local d'un échantillonnage
/// à 200 pas est affiné (section dorée), pour ne pas rater un second bassin
/// (`CubicBezier::nearest_t` n'affine que le meilleur échantillon).
fn cubic_distance(c: &CubicBezier, p: PointMm) -> f64 {
    const N: usize = 200;
    let dist = |t: f64| distance_mm(c.point_at(t), p);
    let samples: Vec<f64> = (0..=N).map(|i| dist(i as f64 / N as f64)).collect();
    let phi = 0.618_033_988_749_894_9;
    let mut best = f64::INFINITY;
    for i in 0..=N {
        let left = if i == 0 {
            f64::INFINITY
        } else {
            samples[i - 1]
        };
        let right = if i == N {
            f64::INFINITY
        } else {
            samples[i + 1]
        };
        if samples[i] > left || samples[i] > right {
            continue;
        }
        best = best.min(samples[i]);
        let mut lo = (i as f64 - 1.0).max(0.0) / N as f64;
        let mut hi = (i as f64 + 1.0).min(N as f64) / N as f64;
        for _ in 0..50 {
            let (m1, m2) = (hi - (hi - lo) * phi, lo + (hi - lo) * phi);
            if dist(m1) < dist(m2) {
                hi = m2;
            } else {
                lo = m1;
            }
        }
        best = best.min(dist(0.5 * (lo + hi)));
    }
    best
}

/// Chaîne d'entrée aplatie en primitives orientées ; `closed` si elle se referme.
fn flatten(chain: &[Trace]) -> Result<(Vec<Prim>, bool)> {
    if chain.is_empty() {
        return Err(err("décalage : la chaîne est vide"));
    }
    let mut prims = Vec::new();
    for t in chain {
        match t {
            Trace::Line(a, b) => {
                segment_length_mm(*a, *b)?;
                prims.push(Prim::Line(*a, *b));
            }
            Trace::Arc(a) => {
                a.center.validate()?;
                crate::check_radius(a.radius_mm)?;
                if !a.sweep_rad.is_finite()
                    || !a.start_rad.is_finite()
                    || a.sweep_rad == 0.0
                    || a.sweep_rad.abs() >= 2.0 * PI
                {
                    return Err(err("décalage : arc invalide"));
                }
                prims.push(Prim::Arc(*a));
            }
            Trace::Cubic(c) => {
                c.validate()?;
                prims.push(Prim::Cubic(*c));
            }
            Trace::Spline(s) => {
                for c in s.segments() {
                    c.validate()?;
                    prims.push(Prim::Cubic(*c));
                }
            }
            Trace::Circle(_) => {
                return Err(err(
                    "décalage : un cercle forme un contour à lui seul, il ne se chaîne pas",
                ))
            }
        }
        if prims.len() > MAX_OFFSET_PIECES {
            return Err(err("décalage : chaîne trop longue"));
        }
    }
    for w in prims.windows(2) {
        if distance_mm(w[0].end(), w[1].start()) > CHAIN_TOL {
            return Err(err(
                "décalage : les tracés ne forment pas une chaîne continue (bout à bout)",
            ));
        }
    }
    let closed = distance_mm(prims[prims.len() - 1].end(), prims[0].start()) <= CHAIN_TOL;
    Ok((prims, closed))
}

// ------------------------------------------------------------- courbe brute

struct Raw {
    prim: Prim,
    /// Partie « retournée » (`1 − d·κ < 0`) : toujours écartée, mais utile aux intersections.
    inverted: bool,
    /// Raccord de coin (réunissable avec un segment colinéaire voisin).
    join: bool,
    /// Distance minimale attendue au tracé d'origine.
    thr: f64,
    /// Tolérance de la comparaison.
    eps: f64,
}

fn second_derivative(c: &CubicBezier, t: f64) -> VectorMm {
    let (a, b) = (
        VectorMm::new(
            c.c2.x - 2.0 * c.c1.x + c.p0.x,
            c.c2.y - 2.0 * c.c1.y + c.p0.y,
        ),
        VectorMm::new(
            c.p3.x - 2.0 * c.c2.x + c.c1.x,
            c.p3.y - 2.0 * c.c2.y + c.c1.y,
        ),
    );
    VectorMm::new(
        6.0 * ((1.0 - t) * a.x + t * b.x),
        6.0 * ((1.0 - t) * a.y + t * b.y),
    )
}

/// `1 − d·κ(t)` : facteur de vitesse du décalage (négatif = partie retournée).
fn speed_factor(c: &CubicBezier, t: f64, d: f64) -> f64 {
    let t = t.clamp(1e-6, 1.0 - 1e-6);
    let v = c.derivative_at(t);
    let n = v.norm();
    if n < 1e-12 {
        return 1.0;
    }
    1.0 - d * v.cross(second_derivative(c, t)) / (n * n * n)
}

fn offset_point(c: &CubicBezier, t: f64, d: f64) -> Result<PointMm> {
    let tan = c.unit_tangent_at(t)?;
    Ok(c.point_at(t) + tan.rotated_left() * d)
}

/// Dérivée du décalage par rapport à `t`.
fn offset_derivative(c: &CubicBezier, t: f64, d: f64) -> Result<VectorMm> {
    let v = c.derivative_at(t);
    if v.norm() > 1e-6 {
        return Ok(v * speed_factor(c, t, d));
    }
    // Vitesse nulle (poignée sur l'extrémité) : différence finie.
    let h = 1e-5;
    let (ta, tb) = ((t - h).max(0.0), (t + h).min(1.0));
    let (pa, pb) = (offset_point(c, ta, d)?, offset_point(c, tb, d)?);
    Ok((pb - pa) * (1.0 / (tb - ta)))
}

fn fit_interval(
    c: &CubicBezier,
    d: f64,
    (t0, t1): (f64, f64),
    depth: u32,
    inverted: bool,
    out: &mut Vec<Raw>,
) -> Result<()> {
    let h = t1 - t0;
    let (p0, p3) = (offset_point(c, t0, d)?, offset_point(c, t1, d)?);
    let (d0, d1) = (offset_derivative(c, t0, d)?, offset_derivative(c, t1, d)?);
    let b = CubicBezier::new(p0, p0 + d0 * (h / 3.0), p3 + d1 * (-h / 3.0), p3);
    let mut worst: f64 = 0.0;
    for u in [0.2, 0.4, 0.5, 0.6, 0.8] {
        let exact = offset_point(c, t0 + u * h, d)?;
        worst = worst.max(distance_mm(b.point_at(u), exact));
    }
    if worst <= OFFSET_FIT_TOLERANCE_MM || depth == 0 {
        out.push(Raw {
            prim: Prim::Cubic(b),
            inverted,
            join: false,
            thr: d.abs(),
            eps: FIT_EPS,
        });
        return Ok(());
    }
    let mid = 0.5 * (t0 + t1);
    fit_interval(c, d, (t0, mid), depth - 1, inverted, out)?;
    fit_interval(c, d, (mid, t1), depth - 1, inverted, out)
}

fn offset_cubic(c: &CubicBezier, d: f64, out: &mut Vec<Raw>) -> Result<()> {
    // Paramètres où 1 − d·κ change de signe.
    const N: usize = 128;
    let mut cuts = vec![0.0];
    let mut prev = speed_factor(c, 0.0, d);
    for i in 1..=N {
        let t = i as f64 / N as f64;
        let s = speed_factor(c, t, d);
        if (prev < 0.0) != (s < 0.0) {
            let (mut lo, mut hi) = ((i - 1) as f64 / N as f64, t);
            let neg_lo = prev < 0.0;
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                if (speed_factor(c, mid, d) < 0.0) == neg_lo {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            cuts.push(0.5 * (lo + hi));
        }
        prev = s;
    }
    cuts.push(1.0);
    for w in cuts.windows(2) {
        if w[1] - w[0] < 1e-9 {
            continue;
        }
        let inverted = speed_factor(c, 0.5 * (w[0] + w[1]), d) < 0.0;
        fit_interval(c, d, (w[0], w[1]), 14, inverted, out)?;
        if out.len() > MAX_RAW_PIECES {
            return Err(err("décalage : chaîne trop complexe"));
        }
    }
    Ok(())
}

fn offset_prim(p: &Prim, d: f64, out: &mut Vec<Raw>) -> Result<()> {
    match p {
        Prim::Line(a, b) => {
            let n = (*b - *a).unit()?.rotated_left() * d;
            out.push(Raw {
                prim: Prim::Line(*a + n, *b + n),
                inverted: false,
                join: false,
                thr: d.abs(),
                eps: EXACT_EPS,
            });
        }
        Prim::Arc(a) => {
            let rho = a.radius_mm - d * a.sweep_rad.signum();
            let (radius, start, inverted) = if rho > 1e-9 {
                (rho, a.start_rad, false)
            } else if rho < -1e-9 {
                (-rho, a.start_rad + PI, true)
            } else {
                return Ok(());
            };
            out.push(Raw {
                prim: Prim::Arc(Arc {
                    center: a.center,
                    radius_mm: radius,
                    start_rad: start,
                    sweep_rad: a.sweep_rad,
                }),
                inverted,
                join: false,
                thr: d.abs(),
                eps: EXACT_EPS,
            });
        }
        Prim::Cubic(c) => offset_cubic(c, d, out)?,
    }
    Ok(())
}

/// Raccord du coin entre la fin de `a` et le début de `b`.
fn push_join(a: &Prim, b: &Prim, d: f64, opts: &OffsetOptions, out: &mut Vec<Raw>) -> Result<()> {
    let v = a.end();
    let (t_in, t_out) = (a.end_tangent()?, b.start_tangent()?);
    let theta = t_in.cross(t_out).atan2(t_in.dot(t_out));
    if theta.abs() * d.abs() <= 1e-6 {
        return Ok(()); // tangence : les deux pièces décalées se rejoignent seules
    }
    let reversal = theta.abs() > PI - 1e-9;
    if !(d * theta < 0.0 || reversal) {
        return Ok(()); // coin intérieur : les pièces se croisent, on coupe
    }
    let (n_in, n_out) = (t_in.rotated_left(), t_out.rotated_left());
    let (e, s) = (v + n_in * d, v + n_out * d);
    let line = |p: PointMm, q: PointMm, thr: f64, out: &mut Vec<Raw>| {
        if distance_mm(p, q) > 1e-9 {
            out.push(Raw {
                prim: Prim::Line(p, q),
                inverted: false,
                join: true,
                thr,
                eps: EXACT_EPS,
            });
        }
    };
    match opts.corner {
        CornerStyle::Round => {
            let radial = e - v;
            out.push(Raw {
                prim: Prim::Arc(Arc {
                    center: v,
                    radius_mm: d.abs(),
                    start_rad: radial.y.atan2(radial.x),
                    sweep_rad: -d.signum() * theta.abs(),
                }),
                inverted: false,
                join: true,
                thr: d.abs(),
                eps: EXACT_EPS,
            });
        }
        CornerStyle::Miter => {
            let ratio = 1.0 / (theta.abs() / 2.0).cos();
            if !reversal && theta.abs() < PI - 1e-6 && ratio <= opts.miter_limit {
                let sum = VectorMm::new(n_in.x + n_out.x, n_in.y + n_out.y);
                let m = v + sum * (d / (1.0 + n_in.dot(n_out)));
                line(e, m, d.abs(), out);
                line(m, s, d.abs(), out);
            } else {
                line(e, s, d.abs() * (theta.abs() / 2.0).cos(), out);
            }
        }
    }
    Ok(())
}

// ------------------------------------------------------------ intersections

/// Extrémités que deux primitives ont en commun (à `JOIN_TOL` près).
fn shared_endpoints(a: &Prim, b: &Prim) -> Vec<PointMm> {
    let mut out = Vec::new();
    for p in [a.start(), a.end()] {
        for q in [b.start(), b.end()] {
            if distance_mm(p, q) <= JOIN_TOL {
                out.push(p);
            }
        }
    }
    out
}

fn bboxes_overlap(a: &Bbox, b: &Bbox) -> bool {
    const T: f64 = 1e-6;
    !(a.max.x + T < b.min.x
        || b.max.x + T < a.min.x
        || a.max.y + T < b.min.y
        || b.max.y + T < a.min.y)
}

fn sub_cubic(c: &CubicBezier, a: f64, b: f64) -> CubicBezier {
    fn split(c: &CubicBezier, t: f64) -> (CubicBezier, CubicBezier) {
        let l = |p: PointMm, q: PointMm| p.lerp(q, t);
        let (p01, p12, p23) = (l(c.p0, c.c1), l(c.c1, c.c2), l(c.c2, c.p3));
        let (p012, p123) = (l(p01, p12), l(p12, p23));
        let m = l(p012, p123);
        (
            CubicBezier::new(c.p0, p01, p012, m),
            CubicBezier::new(m, p123, p23, c.p3),
        )
    }
    let right = if a > 0.0 { split(c, a).1 } else { *c };
    if b >= 1.0 {
        right
    } else {
        split(&right, (b - a) / (1.0 - a)).0
    }
}

/// Coupe une pièce brute aux paramètres donnés (avec les points exacts).
fn split_raw(r: &Raw, mut cuts: Vec<(f64, PointMm)>) -> Vec<Raw> {
    let (s, e) = (r.prim.start(), r.prim.end());
    cuts.retain(|(_, p)| distance_mm(*p, s) > 1e-6 && distance_mm(*p, e) > 1e-6);
    cuts.sort_by(|a, b| a.0.total_cmp(&b.0));
    cuts.dedup_by(|a, b| distance_mm(a.1, b.1) <= 1e-6);
    let mut params = vec![(0.0, s)];
    params.extend(cuts);
    params.push((1.0, e));
    let mut out = Vec::new();
    for w in params.windows(2) {
        let ((ta, pa), (tb, pb)) = (w[0], w[1]);
        let prim = match r.prim {
            Prim::Line(..) => Prim::Line(pa, pb),
            Prim::Arc(a) => Prim::Arc(Arc {
                center: a.center,
                radius_mm: a.radius_mm,
                start_rad: a.start_rad + a.sweep_rad * ta,
                sweep_rad: a.sweep_rad * (tb - ta),
            }),
            Prim::Cubic(c) => {
                let mut sub = sub_cubic(&c, ta, tb);
                sub.p0 = pa;
                sub.p3 = pb;
                Prim::Cubic(sub)
            }
        };
        if prim.is_negligible() {
            continue;
        }
        out.push(Raw {
            prim,
            inverted: r.inverted,
            join: r.join,
            thr: r.thr,
            eps: r.eps,
        });
    }
    out
}

struct Original<'a> {
    prims: &'a [Prim],
    boxes: Vec<Bbox>,
}

impl Original<'_> {
    /// Un point de la chaîne d'origine est-il à moins de `limit` de `p` ?
    fn has_closer(&self, p: PointMm, limit: f64) -> bool {
        if limit <= 0.0 {
            return false;
        }
        for (prim, bb) in self.prims.iter().zip(&self.boxes) {
            let dx = (bb.min.x - p.x).max(0.0).max(p.x - bb.max.x);
            let dy = (bb.min.y - p.y).max(0.0).max(p.y - bb.max.y);
            if dx.hypot(dy) >= limit {
                continue;
            }
            if prim.distance(p) < limit {
                return true;
            }
        }
        false
    }
}

/// Ordre de grandeur de la longueur d'une pièce (mm).
fn rough_length(p: &Prim) -> f64 {
    match p {
        Prim::Line(a, b) => distance_mm(*a, *b),
        Prim::Arc(a) => a.length_mm(),
        Prim::Cubic(c) => {
            distance_mm(c.p0, c.c1) + distance_mm(c.c1, c.c2) + distance_mm(c.c2, c.p3)
        }
    }
}

/// Garde les parties de `r` qui sont à la distance voulue du tracé d'origine.
///
/// Une pièce peut entrer dans le voisinage d'une autre partie du tracé sans
/// croiser aucune autre pièce du décalage (elle croise alors le décalage de
/// l'autre côté, qui n'est pas calculé) : on échantillonne donc la pièce, puis
/// on localise par bissection chaque passage valide/invalide et on coupe là.
fn filter_piece(r: Raw, original: &Original, d_abs: f64) -> Vec<Raw> {
    let limit = r.thr - r.eps;
    let n = ((rough_length(&r.prim) / (d_abs.max(1.0) / 6.0)).ceil() as usize).clamp(6, 150);
    let at = |u: f64| r.prim.point_at(u);
    // Échantillons de 0 à 1, extrémités comprises.
    let u_of = |i: usize| i as f64 / n as f64;
    let valid: Vec<bool> = (0..=n)
        .map(|i| !original.has_closer(at(u_of(i)), limit))
        .collect();
    if valid.iter().all(|&v| v) {
        return vec![r];
    }
    if valid.iter().all(|&v| !v) {
        return Vec::new();
    }
    let mut cuts = Vec::new();
    for i in 0..n {
        if valid[i] != valid[i + 1] {
            let (mut lo, mut hi) = (u_of(i), u_of(i + 1));
            for _ in 0..60 {
                let mid = 0.5 * (lo + hi);
                if !original.has_closer(at(mid), limit) == valid[i] {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let u = 0.5 * (lo + hi);
            // Un passage à moins de ~eps/sin(angle) d'une extrémité est un coin déjà
            // coupé à son intersection : pas de sliver.
            let margin = (20.0 * r.eps).max(1e-6);
            let q = at(u);
            if distance_mm(q, r.prim.start()) > margin && distance_mm(q, r.prim.end()) > margin {
                cuts.push((u, q));
            }
        }
    }
    split_raw(&r, cuts)
        .into_iter()
        .filter(|f| !original.has_closer(f.prim.point_at(0.5), limit))
        .collect()
}

fn mergeable(a: &Raw, b: &Raw) -> Option<Prim> {
    if let (Prim::Line(a0, a1), Prim::Line(b0, b1)) = (a.prim, b.prim) {
        if (a.join || b.join) && distance_mm(a1, b0) <= 1e-7 {
            let (u, v) = (a1 - a0, b1 - b0);
            let (nu, nv) = (u.norm(), v.norm());
            if u.cross(v).abs() <= 1e-9 * nu * nv && u.dot(v) > 0.0 {
                return Some(Prim::Line(a0, b1));
            }
        }
    }
    None
}

fn check_distance(d: f64) -> Result<()> {
    if !d.is_finite() {
        return Err(GeometryError::NonFinite("la distance de décalage"));
    }
    if d.abs() < 1e-6 {
        return Err(err(
            "décalage : la distance doit être non nulle (au moins 0,000001 mm)",
        ));
    }
    if d.abs() > MAX_OFFSET_MM {
        return Err(err(&format!(
            "décalage : distance trop grande (au plus {MAX_OFFSET_MM} mm)"
        )));
    }
    Ok(())
}

/// Côté (`+1` gauche, `-1` droite) de la chaîne où se trouve `p`, d'après le
/// point de la chaîne le plus proche. Sert à choisir le signe de `d` à la souris.
pub fn chain_side(chain: &[Trace], p: PointMm) -> Result<f64> {
    Ok(chain_signed_distance(chain, p)?.signum())
}

/// Distance de `p` à la chaîne, signée : `> 0` à gauche du sens de parcours.
/// C'est la valeur de `d` qui fait passer le décalage par `p` (hors coins).
pub fn chain_signed_distance(chain: &[Trace], p: PointMm) -> Result<f64> {
    p.validate()?;
    if let [Trace::Circle(c)] = chain {
        let gap = c.radius_mm - distance_mm(c.center, p);
        return if gap == 0.0 {
            Err(err("le point visé est sur le tracé : côté indéterminé"))
        } else {
            Ok(gap)
        };
    }
    let (prims, _) = flatten(chain)?;
    let mut best: Option<(f64, f64)> = None; // (distance, produit vectoriel)
    for prim in &prims {
        let (foot, tan) = match prim {
            Prim::Line(a, b) => {
                let (along, _) = project_on_segment(p, *a, *b)?;
                let u = (*b - *a).unit()?;
                (*a + u * along, u)
            }
            Prim::Arc(a) => a.point_at_length(a.nearest_length_mm(p))?,
            Prim::Cubic(c) => {
                let (t, _) = c.nearest_t(p);
                (c.point_at(t), c.unit_tangent_at(t)?)
            }
        };
        let dist = distance_mm(foot, p);
        if best.is_none_or(|(b, _)| dist < b) {
            best = Some((dist, tan.cross(p - foot)));
        }
    }
    match best {
        Some((dist, cross)) if cross > 0.0 => Ok(dist),
        Some((dist, cross)) if cross < 0.0 => Ok(-dist),
        _ => Err(err("le point visé est sur le tracé : côté indéterminé")),
    }
}

/// Courbe parallèle à `chain` à la distance signée `d` (`> 0` : à gauche du
/// sens de parcours). Voir la documentation du module pour la méthode.
///
/// Erreurs (rien n'est renvoyé de partiel) : `d` nul, non fini ou trop grand ;
/// chaîne vide, non connexe ou contenant un tracé dégénéré ; décalage qui
/// annule entièrement la chaîne (petit contour fermé décalé vers l'intérieur) ;
/// résultat hors de l'espace de travail.
pub fn offset_chain(chain: &[Trace], d: f64, options: &OffsetOptions) -> Result<Vec<Trace>> {
    check_distance(d)?;
    if !options.miter_limit.is_finite() || options.miter_limit < 1.0 {
        return Err(err("décalage : la limite de pointe doit être au moins 1"));
    }
    // Un cercle seul : cercle concentrique (à gauche du sens anti-horaire = vers le centre).
    if let [Trace::Circle(c)] = chain {
        Circle::new(c.center, c.radius_mm)?;
        let radius = c.radius_mm - d;
        if radius <= 1e-6 {
            return Err(err(
                "décalage : le décalage annule entièrement le cercle (rayon nul ou négatif)",
            ));
        }
        crate::check_radius(radius)?;
        return Ok(vec![Trace::Circle(Circle::new(c.center, radius)?)]);
    }
    let (prims, closed) = flatten(chain)?;

    // 1. Courbe brute.
    let mut raws: Vec<Raw> = Vec::new();
    for (i, p) in prims.iter().enumerate() {
        offset_prim(p, d, &mut raws)?;
        if i + 1 < prims.len() {
            push_join(p, &prims[i + 1], d, options, &mut raws)?;
        } else if closed {
            push_join(p, &prims[0], d, options, &mut raws)?;
        }
        if raws.len() > MAX_RAW_PIECES {
            return Err(err("décalage : chaîne trop complexe"));
        }
    }
    raws.retain(|r| !r.prim.is_negligible());

    // 2. Intersections entre pièces brutes.
    let boxes: Vec<Bbox> = raws.iter().map(|r| r.prim.bbox()).collect();
    let traces: Vec<Trace> = raws.iter().map(|r| r.prim.trace()).collect();
    let mut cuts: Vec<Vec<(f64, PointMm)>> = vec![Vec::new(); raws.len()];
    let mut order: Vec<usize> = (0..raws.len()).collect();
    order.sort_by(|&a, &b| boxes[a].min.x.total_cmp(&boxes[b].min.x));
    for (oi, &i) in order.iter().enumerate() {
        for &j in &order[oi + 1..] {
            if boxes[j].min.x > boxes[i].max.x + 1e-6 {
                break;
            }
            if !bboxes_overlap(&boxes[i], &boxes[j]) {
                continue;
            }
            let shared = shared_endpoints(&raws[i].prim, &raws[j].prim);
            let hits = match crate::intersect(&traces[i], &traces[j]) {
                Ok(h) => h,
                Err(GeometryError::OverlappingTraces) => {
                    return Err(err(
                        "décalage impossible : deux parties du décalage se superposent \
                         (distance égale à la demi-largeur d'une forme étroite)",
                    ))
                }
                Err(e) => return Err(e),
            };
            for h in hits {
                if shared.iter().any(|s| distance_mm(*s, h.point) <= JOIN_TOL) {
                    continue;
                }
                cuts[i].push((h.t1, h.point));
                cuts[j].push((h.t2, h.point));
            }
        }
    }
    let mut pieces: Vec<Raw> = Vec::new();
    for (r, c) in raws.iter().zip(cuts) {
        pieces.extend(split_raw(r, c));
    }

    // 3. Filtre par la distance au tracé d'origine.
    let original = Original {
        boxes: prims.iter().map(Prim::bbox).collect(),
        prims: &prims,
    };
    let mut kept: Vec<Raw> = Vec::new();
    for r in pieces {
        if !r.inverted {
            kept.extend(filter_piece(r, &original, d.abs()));
        }
    }
    if kept.is_empty() {
        return Err(err(
            "décalage : il annule entièrement le tracé (distance trop grande pour cette forme)",
        ));
    }

    // 4. Refonte des segments colinéaires issus d'un raccord.
    let mut merged: Vec<Raw> = Vec::new();
    for r in kept {
        if let Some(last) = merged.last_mut() {
            if let Some(prim) = mergeable(last, &r) {
                last.prim = prim;
                last.join = last.join && r.join;
                continue;
            }
        }
        merged.push(r);
    }
    if closed && merged.len() > 1 {
        let (first, last) = (&merged[0], &merged[merged.len() - 1]);
        if let Some(prim) = mergeable(last, first) {
            let join = last.join && first.join;
            merged[0].prim = prim;
            merged[0].join = join;
            merged.pop();
        }
    }
    let out: Vec<Trace> = merged.into_iter().map(|r| r.prim.trace()).collect();
    for t in &out {
        let pts: Vec<PointMm> = match t {
            Trace::Line(a, b) => vec![*a, *b],
            Trace::Arc(a) => vec![a.center, a.start_point(), a.end_point()],
            Trace::Cubic(c) => vec![c.p0, c.c1, c.c2, c.p3],
            _ => Vec::new(),
        };
        if pts.iter().any(|p| {
            !p.x.is_finite() || !p.y.is_finite() || p.x.abs().max(p.y.abs()) > MAX_COORD_MM
        }) {
            return Err(err(
                "décalage : le résultat sort de l'espace de travail (distance trop grande)",
            ));
        }
        if let Trace::Arc(a) = t {
            if a.radius_mm > MAX_COORD_MM {
                return Err(err(
                    "décalage : le résultat sort de l'espace de travail (distance trop grande)",
                ));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
#[path = "offset_tests.rs"]
mod tests;

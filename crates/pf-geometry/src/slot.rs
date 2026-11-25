//! Fente (rectangle à bouts arrondis) et distance d'un segment à une arête (US-06.4).
//!
//! Une boutonnière est dessinée comme une **fente** : deux côtés droits parallèles et
//! deux bouts en demi-cercle. `length_mm` est la longueur **hors tout** (les bouts
//! arrondis compris), `width_mm` la largeur ; l'axe de la longueur fait l'angle
//! `angle_deg` avec +X (anti-horaire). Tout est en mm.
//!
//! [`segment_edge_distance_mm`] sert au contrôle du recul du bord : distance exacte
//! d'un segment (ou d'un point) à une arête élémentaire. Segments et arcs : formules
//! fermées (extrémités, intersection, point critique du cercle). Bézier : minimum
//! affiné par recherche à section dorée autour de chaque minimum local d'un
//! échantillonnage fin, à mieux que 1e-6 mm près (pas de formule fermée).

use crate::region::EdgeGeom;
use crate::split::edge_trace;
use crate::{
    distance_mm, intersect, sin_cos_deg, Arc, CubicBezier, GeometryError, PointMm, VectorMm,
    EPSILON_MM,
};
use std::f64::consts::PI;

/// Fente : rectangle de `length_mm × width_mm` à bouts en demi-cercle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Slot {
    pub center: PointMm,
    /// Longueur hors tout (mm), `>= width_mm`.
    pub length_mm: f64,
    pub width_mm: f64,
    /// Direction de la longueur, en degrés depuis +X (anti-horaire).
    pub angle_deg: f64,
}

impl Slot {
    /// Vecteur unitaire de la longueur (exact pour les multiples de 90°).
    pub fn direction(&self) -> VectorMm {
        let (s, c) = sin_cos_deg(self.angle_deg);
        VectorMm::new(c, s)
    }

    /// Centres des deux bouts arrondis (l'âme de la fente), du côté « −longueur » au
    /// côté « +longueur ». Confondus quand `length_mm == width_mm` (fente ronde).
    pub fn spine(&self) -> (PointMm, PointMm) {
        let h = (self.length_mm - self.width_mm) / 2.0;
        let u = self.direction() * h;
        (
            PointMm::new(self.center.x - u.x, self.center.y - u.y),
            PointMm::new(self.center.x + u.x, self.center.y + u.y),
        )
    }

    /// Contour fermé, anti-horaire : côté droit, bout « + », côté gauche, bout « − ».
    pub fn outline(&self) -> [EdgeGeom; 4] {
        let (a, b) = self.spine();
        let r = self.width_mm / 2.0;
        let n = self.direction().rotated_left() * r;
        let plus = |p: PointMm, k: f64| PointMm::new(p.x + n.x * k, p.y + n.y * k);
        let theta = self.angle_deg.to_radians();
        let arc = |center: PointMm, start_rad: f64| {
            EdgeGeom::Arc(Arc {
                center,
                radius_mm: r,
                start_rad,
                sweep_rad: PI,
            })
        };
        [
            EdgeGeom::Line(plus(a, -1.0), plus(b, -1.0)),
            arc(b, theta - PI / 2.0),
            EdgeGeom::Line(plus(b, 1.0), plus(a, 1.0)),
            arc(a, theta + PI / 2.0),
        ]
    }

    /// Polygone fermé (sans répéter le premier sommet) qui approche le contour, avec
    /// `steps` pas par demi-cercle : pour l'affichage seulement.
    pub fn polyline(&self, steps: usize) -> Vec<PointMm> {
        let mut out = Vec::new();
        for e in self.outline() {
            let s = crate::split::edge_sample(&e, steps.max(1));
            out.extend(s.iter().take(s.len().saturating_sub(1)).copied());
        }
        out
    }

    /// Demi-emprise (mm) de la fente selon X puis selon Y (boîte alignée sur les axes).
    pub fn half_extent(&self) -> (f64, f64) {
        let (u, r) = (self.direction(), self.width_mm / 2.0);
        let h = (self.length_mm - self.width_mm) / 2.0;
        (u.x.abs() * h + r, u.y.abs() * h + r)
    }
}

/// Distance d'un point au segment `[a, b]` (un point si `a == b`).
pub fn point_segment_distance_mm(p: PointMm, a: PointMm, b: PointMm) -> f64 {
    let v = b - a;
    let len2 = v.dot(v);
    if len2 <= 0.0 {
        return distance_mm(p, a);
    }
    let t = ((p - a).dot(v) / len2).clamp(0.0, 1.0);
    distance_mm(p, PointMm::new(a.x + v.x * t, a.y + v.y * t))
}

/// Nombre d'échantillons de la recherche du minimum sur une Bézier.
const CUBIC_SAMPLES: usize = 256;
/// Nombre de minima locaux affinés au plus.
const CUBIC_REFINED: usize = 4;
/// Itérations de la recherche à section dorée (intervalle réduit de 0,618^60).
const GOLDEN_ITERATIONS: usize = 60;

/// Minimum de `f` sur `[lo, hi]` par recherche à section dorée.
fn golden_min(f: &dyn Fn(f64) -> f64, mut lo: f64, mut hi: f64) -> f64 {
    let g = (5f64.sqrt() - 1.0) / 2.0;
    let (mut x1, mut x2) = (hi - g * (hi - lo), lo + g * (hi - lo));
    let (mut f1, mut f2) = (f(x1), f(x2));
    for _ in 0..GOLDEN_ITERATIONS {
        if f1 <= f2 {
            hi = x2;
            (x2, f2) = (x1, f1);
            x1 = hi - g * (hi - lo);
            f1 = f(x1);
        } else {
            lo = x1;
            (x1, f1) = (x2, f2);
            x2 = lo + g * (hi - lo);
            f2 = f(x2);
        }
    }
    f1.min(f2)
}

fn cubic_distance(c: &CubicBezier, a: PointMm, b: PointMm) -> f64 {
    let f = |t: f64| point_segment_distance_mm(c.point_at(t), a, b);
    let n = CUBIC_SAMPLES;
    let vals: Vec<f64> = (0..=n).map(|i| f(i as f64 / n as f64)).collect();
    let mut minima: Vec<usize> = (0..=n)
        .filter(|&i| (i == 0 || vals[i] <= vals[i - 1]) && (i == n || vals[i] <= vals[i + 1]))
        .collect();
    minima.sort_by(|&i, &j| vals[i].total_cmp(&vals[j]));
    let mut best = vals.iter().copied().fold(f64::INFINITY, f64::min);
    for &i in minima.iter().take(CUBIC_REFINED) {
        let lo = i.saturating_sub(1) as f64 / n as f64;
        let hi = (i + 1).min(n) as f64 / n as f64;
        best = best.min(golden_min(&f, lo, hi));
    }
    best
}

fn arc_distance(arc: &Arc, a: PointMm, b: PointMm) -> f64 {
    let mut best = arc
        .distance_to_point(a)
        .min(arc.distance_to_point(b))
        .min(point_segment_distance_mm(arc.start_point(), a, b))
        .min(point_segment_distance_mm(arc.end_point(), a, b));
    // Points de l'arc les plus proches de la droite (AB) : sur la normale passant par
    // le centre, de part et d'autre.
    if let Ok(n) = (b - a).unit().map(VectorMm::rotated_left) {
        for k in [-1.0, 1.0] {
            let p = PointMm::new(
                arc.center.x + n.x * arc.radius_mm * k,
                arc.center.y + n.y * arc.radius_mm * k,
            );
            if arc.fraction_of(p).is_some() {
                best = best.min(point_segment_distance_mm(p, a, b));
            }
        }
    }
    best
}

/// Distance (mm) du segment `[a, b]` (réduit à un point si `a == b`) à une arête
/// élémentaire ; 0 si ils se coupent ou se superposent.
pub fn segment_edge_distance_mm(a: PointMm, b: PointMm, edge: &EdgeGeom) -> f64 {
    let is_point = distance_mm(a, b) <= EPSILON_MM;
    if !is_point {
        match intersect(&crate::Trace::Line(a, b), &edge_trace(edge)) {
            Ok(hits) if !hits.is_empty() => return 0.0,
            Err(GeometryError::OverlappingTraces) => return 0.0,
            _ => {}
        }
    }
    match edge {
        EdgeGeom::Line(c, d) => point_segment_distance_mm(a, *c, *d)
            .min(point_segment_distance_mm(b, *c, *d))
            .min(point_segment_distance_mm(*c, a, b))
            .min(point_segment_distance_mm(*d, a, b)),
        EdgeGeom::Arc(arc) => arc_distance(arc, a, b),
        EdgeGeom::Cubic(c) => cubic_distance(c, a, b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> PointMm {
        PointMm::new(x, y)
    }

    fn near(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn segment_parallele_distance_cinq() {
        let e = EdgeGeom::Line(p(0.0, 5.0), p(10.0, 5.0));
        assert!(near(
            segment_edge_distance_mm(p(0.0, 0.0), p(10.0, 0.0), &e),
            5.0,
            1e-12
        ));
    }

    #[test]
    fn segments_qui_se_croisent_ou_se_superposent_zero() {
        let croix = EdgeGeom::Line(p(5.0, -3.0), p(5.0, 3.0));
        assert_eq!(
            segment_edge_distance_mm(p(0.0, 0.0), p(10.0, 0.0), &croix),
            0.0
        );
        let meme = EdgeGeom::Line(p(2.0, 0.0), p(8.0, 0.0));
        assert_eq!(
            segment_edge_distance_mm(p(0.0, 0.0), p(10.0, 0.0), &meme),
            0.0
        );
    }

    #[test]
    fn extremite_la_plus_proche_triangle_3_4_5() {
        let e = EdgeGeom::Line(p(13.0, 4.0), p(13.0, 10.0));
        assert!(near(
            segment_edge_distance_mm(p(0.0, 0.0), p(10.0, 0.0), &e),
            5.0,
            1e-12
        ));
    }

    #[test]
    fn point_seul_distance_au_segment() {
        let e = EdgeGeom::Line(p(0.0, 5.0), p(10.0, 5.0));
        assert!(near(
            segment_edge_distance_mm(p(3.0, 1.0), p(3.0, 1.0), &e),
            4.0,
            1e-12
        ));
    }

    fn demi_cercle_haut() -> EdgeGeom {
        EdgeGeom::Arc(Arc {
            center: p(0.0, 0.0),
            radius_mm: 10.0,
            start_rad: 0.0,
            sweep_rad: PI,
        })
    }

    #[test]
    fn arc_segment_interieur_extremite_la_plus_proche() {
        // Segment de (-3 ; 0) à (3 ; 0) : extrémité à 3 mm du centre, donc 7 mm de l'arc.
        let d = segment_edge_distance_mm(p(-3.0, 0.0), p(3.0, 0.0), &demi_cercle_haut());
        assert!(near(d, 7.0, 1e-9), "{d}");
    }

    #[test]
    fn arc_segment_exterieur_point_critique_au_sommet() {
        // Segment horizontal à y = 15 : le sommet (0 ; 10) est à 5 mm ; les extrémités
        // (±20 ; 15) sont à 15 mm de l'arc.
        let d = segment_edge_distance_mm(p(-20.0, 15.0), p(20.0, 15.0), &demi_cercle_haut());
        assert!(near(d, 5.0, 1e-9), "{d}");
    }

    #[test]
    fn arc_segment_qui_traverse_zero() {
        let d = segment_edge_distance_mm(p(0.0, 0.0), p(0.0, 20.0), &demi_cercle_haut());
        assert_eq!(d, 0.0);
    }

    #[test]
    fn arc_point_critique_hors_de_l_arc_est_ignore() {
        // Arc de 0° à 90° (quart haut-droit) ; segment horizontal à y = -4 : l'arc le plus
        // proche est son départ (10 ; 0), à 4 mm.
        let quart = EdgeGeom::Arc(Arc {
            center: p(0.0, 0.0),
            radius_mm: 10.0,
            start_rad: 0.0,
            sweep_rad: PI / 2.0,
        });
        let d = segment_edge_distance_mm(p(-20.0, -4.0), p(20.0, -4.0), &quart);
        assert!(near(d, 4.0, 1e-9), "{d}");
    }

    #[test]
    fn bezier_sommet_a_six_millimetres() {
        // P0 (0;0), C1 (0;12), C2 (10;12), P3 (10;0) : sommet (5 ; 9) à t = 0,5.
        let c = EdgeGeom::Cubic(CubicBezier::new(
            p(0.0, 0.0),
            p(0.0, 12.0),
            p(10.0, 12.0),
            p(10.0, 0.0),
        ));
        let d = segment_edge_distance_mm(p(-5.0, 15.0), p(15.0, 15.0), &c);
        assert!(near(d, 6.0, 1e-9), "{d}");
    }

    #[test]
    fn bezier_qui_traverse_zero() {
        let c = EdgeGeom::Cubic(CubicBezier::new(
            p(0.0, 0.0),
            p(0.0, 12.0),
            p(10.0, 12.0),
            p(10.0, 0.0),
        ));
        assert_eq!(
            segment_edge_distance_mm(p(-5.0, 5.0), p(15.0, 5.0), &c),
            0.0
        );
    }

    #[test]
    fn bezier_droite_distance_constante() {
        let c = EdgeGeom::Cubic(CubicBezier::new(
            p(0.0, 5.0),
            p(3.0, 5.0),
            p(6.0, 5.0),
            p(10.0, 5.0),
        ));
        let d = segment_edge_distance_mm(p(0.0, 0.0), p(10.0, 0.0), &c);
        assert!(near(d, 5.0, 1e-9), "{d}");
    }

    #[test]
    fn fente_horizontale_20_par_6() {
        let s = Slot {
            center: p(100.0, 50.0),
            length_mm: 20.0,
            width_mm: 6.0,
            angle_deg: 0.0,
        };
        let (a, b) = s.spine();
        assert_eq!((a, b), (p(93.0, 50.0), p(107.0, 50.0)));
        assert_eq!(s.half_extent(), (10.0, 3.0));
        let [e0, _, e2, _] = s.outline();
        assert_eq!(e0, EdgeGeom::Line(p(93.0, 47.0), p(107.0, 47.0)));
        assert_eq!(e2, EdgeGeom::Line(p(107.0, 53.0), p(93.0, 53.0)));
    }

    #[test]
    fn fente_verticale_et_contour_ferme() {
        let s = Slot {
            center: p(0.0, 0.0),
            length_mm: 20.0,
            width_mm: 6.0,
            angle_deg: 90.0,
        };
        assert_eq!(s.half_extent(), (3.0, 10.0));
        let edges = s.outline();
        for i in 0..4 {
            let (end, next) = (edges[i].end(), edges[(i + 1) % 4].start());
            assert!(
                distance_mm(end, next) < 1e-9,
                "arête {i} : {end:?} {next:?}"
            );
        }
        // Périmètre : 2 côtés de 14 + 2 demi-cercles de rayon 3.
        let per: f64 = edges.iter().map(EdgeGeom::length_mm).sum();
        assert!(near(per, 28.0 + 2.0 * PI * 3.0, 1e-9));
    }

    #[test]
    fn fente_ronde_ame_reduite_a_un_point() {
        let s = Slot {
            center: p(1.0, 2.0),
            length_mm: 6.0,
            width_mm: 6.0,
            angle_deg: 33.0,
        };
        let (a, b) = s.spine();
        assert!(distance_mm(a, b) < 1e-12);
    }
}

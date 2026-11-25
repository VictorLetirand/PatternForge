//! Courbe par points de passage (US-02.2) : spline cubique d'Hermite par
//! morceaux, représentée par des Béziers cubiques (un par tronçon).
//!
//! **Continuité.** En un point *lisse*, le tronçon qui arrive et celui qui part
//! partagent la même tangente d'Hermite (dérivée par rapport au paramètre
//! `u ∈ [0, 1]` de chaque tronçon) : la courbe est **C1** (continue en position
//! et en vitesse). En un point *anguleux*, les deux tangentes sont libres : la
//! courbe est seulement C0. La continuité de courbure (G2) n'est **pas**
//! recherchée : la tangente par défaut est celle de Catmull-Rom, qui ne
//! l'assure pas.
//!
//! Un tronçon d'Hermite `(P0, m0) → (P1, m1)` est la Bézier
//! `(P0, P0 + m0/3, P1 − m1/3, P1)` : l'export réutilise donc tel quel le chemin
//! Bézier existant.

use crate::{
    distance_mm, Bbox, CubicBezier, GeometryError, PointMm, Result, VectorMm,
    DEFAULT_LENGTH_TOLERANCE_MM,
};

/// Deux points de passage consécutifs plus proches que cela (mm) sont confondus.
pub const MIN_NODE_SPACING_MM: f64 = 1e-6;

/// Un point de passage avec ses deux tangentes d'Hermite résolues.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HermiteNode {
    pub point: PointMm,
    /// Tangente du tronçon qui arrive (inutilisée au premier point).
    pub tangent_in: VectorMm,
    /// Tangente du tronçon qui part (inutilisée au dernier point).
    pub tangent_out: VectorMm,
}

/// Tangente par défaut en un point **lisse** : Catmull-Rom uniforme
/// `(suivant − précédent) / 2` ; à une extrémité, la corde vers le voisin.
pub fn default_smooth_tangent(
    prev: Option<PointMm>,
    p: PointMm,
    next: Option<PointMm>,
) -> VectorMm {
    match (prev, next) {
        (Some(a), Some(b)) => (b - a) * 0.5,
        (None, Some(b)) => b - p,
        (Some(a), None) => p - a,
        (None, None) => VectorMm::new(0.0, 0.0),
    }
}

/// Tangentes par défaut en un point **anguleux** : la corde vers chaque voisin
/// (`(entrante, sortante)`), ce qui donne un angle net.
pub fn default_corner_tangents(
    prev: Option<PointMm>,
    p: PointMm,
    next: Option<PointMm>,
) -> (VectorMm, VectorMm) {
    let zero = VectorMm::new(0.0, 0.0);
    (prev.map_or(zero, |a| p - a), next.map_or(zero, |b| b - p))
}

/// Vecteur de tangente de module `length_mm` (≥ 0, fini) et de direction
/// `angle_deg` (anti-horaire depuis +X).
pub fn tangent_from_polar(length_mm: f64, angle_deg: f64) -> Result<VectorMm> {
    if !length_mm.is_finite() || length_mm < 0.0 {
        return Err(GeometryError::InvalidLength(length_mm));
    }
    if !angle_deg.is_finite() {
        return Err(GeometryError::NonFinite("l'angle"));
    }
    let (s, c) = crate::sin_cos_deg(angle_deg);
    Ok(VectorMm::new(c * length_mm, s * length_mm))
}

/// Module et direction (0 à 360°) d'un vecteur de tangente.
pub fn tangent_to_polar(v: VectorMm) -> (f64, f64) {
    let angle = v.y.atan2(v.x).to_degrees().rem_euclid(360.0);
    (v.norm(), if v.norm() == 0.0 { 0.0 } else { angle })
}

/// Suite continue de Béziers cubiques issue de points de passage.
#[derive(Debug, Clone, PartialEq)]
pub struct Spline {
    segments: Vec<CubicBezier>,
}

fn spline_err(msg: String) -> GeometryError {
    GeometryError::InvalidSpline(msg)
}

impl Spline {
    /// Construit la spline ; refuse moins de 2 points, une valeur non finie ou
    /// hors de l'espace de travail, et deux points consécutifs confondus.
    pub fn from_nodes(nodes: &[HermiteNode]) -> Result<Spline> {
        if nodes.len() < 2 {
            return Err(spline_err(
                "une courbe par points de passage en exige au moins deux".into(),
            ));
        }
        for n in nodes {
            n.point.validate()?;
            for t in [n.tangent_in, n.tangent_out] {
                if !t.x.is_finite() || !t.y.is_finite() {
                    return Err(GeometryError::NonFinite("une tangente"));
                }
            }
        }
        let mut segments = Vec::with_capacity(nodes.len() - 1);
        for w in nodes.windows(2) {
            if distance_mm(w[0].point, w[1].point) <= MIN_NODE_SPACING_MM {
                return Err(spline_err(
                    "deux points de passage consécutifs sont confondus".into(),
                ));
            }
            let seg = CubicBezier::new(
                w[0].point,
                w[0].point + w[0].tangent_out * (1.0 / 3.0),
                w[1].point + w[1].tangent_in * (-1.0 / 3.0),
                w[1].point,
            );
            seg.validate()?;
            segments.push(seg);
        }
        Ok(Spline { segments })
    }

    /// Courbe lisse (C1) passant par les points, tangentes par défaut
    /// (Catmull-Rom) : celle que crée l'outil avant toute retouche.
    pub fn through_points(points: &[PointMm]) -> Result<Spline> {
        let nodes: Vec<HermiteNode> = (0..points.len())
            .map(|i| {
                let t = default_smooth_tangent(
                    i.checked_sub(1).map(|j| points[j]),
                    points[i],
                    points.get(i + 1).copied(),
                );
                HermiteNode {
                    point: points[i],
                    tangent_in: t,
                    tangent_out: t,
                }
            })
            .collect();
        Spline::from_nodes(&nodes)
    }

    /// Les tronçons, dans l'ordre (un par intervalle entre points de passage).
    pub fn segments(&self) -> &[CubicBezier] {
        &self.segments
    }

    pub fn start_point(&self) -> PointMm {
        self.segments[0].p0
    }

    pub fn end_point(&self) -> PointMm {
        self.segments[self.segments.len() - 1].p3
    }

    /// Longueurs développées de chaque tronçon (mm).
    pub fn segment_lengths_mm(&self) -> Result<Vec<f64>> {
        self.segments
            .iter()
            .map(|s| s.length_mm(DEFAULT_LENGTH_TOLERANCE_MM))
            .collect()
    }

    /// Longueur développée totale (mm).
    pub fn length_mm(&self) -> Result<f64> {
        Ok(self.segment_lengths_mm()?.iter().sum())
    }

    pub fn bbox(&self) -> Bbox {
        let mut bb = self.segments[0].bbox();
        for s in &self.segments[1..] {
            bb.union(s.bbox());
        }
        bb
    }

    /// Polyligne d'approximation : `per_segment` pas par tronçon.
    pub fn sample(&self, per_segment: usize) -> Vec<PointMm> {
        let mut out = self.segments[0].sample(per_segment);
        for s in &self.segments[1..] {
            out.extend(s.sample(per_segment).into_iter().skip(1));
        }
        out
    }

    pub fn distance_to_point(&self, p: PointMm) -> f64 {
        self.segments
            .iter()
            .map(|s| s.distance_to_point(p))
            .fold(f64::INFINITY, f64::min)
    }

    /// Spline parcourue dans l'autre sens.
    pub fn reversed(&self) -> Spline {
        Spline {
            segments: self.segments.iter().rev().map(|s| s.reversed()).collect(),
        }
    }

    /// Contribution à l'aire signée d'un contour fermé (somme des tronçons).
    pub fn signed_area_contribution_mm2(&self, origin: PointMm) -> f64 {
        self.segments
            .iter()
            .map(|s| s.signed_area_contribution_mm2(origin))
            .sum()
    }

    /// Point situé à `distance_mm` de longueur développée du départ, avec la
    /// tangente unitaire.
    pub fn point_at_length(&self, distance_mm: f64) -> Result<(PointMm, VectorMm)> {
        let lengths = self.segment_lengths_mm()?;
        let total: f64 = lengths.iter().sum();
        if !distance_mm.is_finite() || distance_mm < 0.0 || distance_mm > total + 1e-9 {
            return Err(GeometryError::DistanceOutOfSegment {
                distance_mm,
                length_mm: total,
            });
        }
        let mut rest = distance_mm.min(total);
        let last = self.segments.len() - 1;
        for (i, (seg, len)) in self.segments.iter().zip(&lengths).enumerate() {
            if rest <= *len || i == last {
                let t = seg.t_at_length_mm(rest.min(*len), DEFAULT_LENGTH_TOLERANCE_MM)?;
                return Ok((seg.point_at(t), seg.unit_tangent_at(t)?));
            }
            rest -= len;
        }
        unreachable!("la boucle retourne au dernier tronçon")
    }

    /// Distance, le long de la courbe depuis son départ, du point de la
    /// courbe le plus proche de `p`.
    pub fn nearest_length_mm(&self, p: PointMm) -> Result<f64> {
        let lengths = self.segment_lengths_mm()?;
        let (mut best, mut best_d, mut before) = (0.0, f64::INFINITY, 0.0);
        for (seg, len) in self.segments.iter().zip(&lengths) {
            let (t, d) = seg.nearest_t(p);
            if d < best_d {
                best_d = d;
                best = before + seg.length_to_t_mm(t, DEFAULT_LENGTH_TOLERANCE_MM)?;
            }
            before += len;
        }
        Ok(best)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> PointMm {
        PointMm::new(x, y)
    }

    fn smooth(points: &[PointMm]) -> Vec<HermiteNode> {
        (0..points.len())
            .map(|i| {
                let t = default_smooth_tangent(
                    i.checked_sub(1).map(|j| points[j]),
                    points[i],
                    points.get(i + 1).copied(),
                );
                HermiteNode {
                    point: points[i],
                    tangent_in: t,
                    tangent_out: t,
                }
            })
            .collect()
    }

    #[test]
    fn passe_par_tous_les_points_et_est_c1() {
        let pts = [p(0., 0.), p(30., 20.), p(60., -5.), p(100., 10.)];
        let s = Spline::from_nodes(&smooth(&pts)).unwrap();
        assert_eq!(s.segments().len(), 3);
        for (i, seg) in s.segments().iter().enumerate() {
            assert!(distance_mm(seg.p0, pts[i]) < 1e-12);
            assert!(distance_mm(seg.p3, pts[i + 1]) < 1e-12);
        }
        for w in s.segments().windows(2) {
            let (a, b) = (w[0].derivative_at(1.0), w[1].derivative_at(0.0));
            assert!((a.x - b.x).hypot(a.y - b.y) < 1e-9, "dérivée discontinue");
        }
    }

    #[test]
    fn point_anguleux_garde_deux_tangentes() {
        let pts = [p(0., 0.), p(50., 0.), p(50., 50.)];
        let (tin, tout) = default_corner_tangents(Some(pts[0]), pts[1], Some(pts[2]));
        let mut nodes = smooth(&pts);
        nodes[1].tangent_in = tin;
        nodes[1].tangent_out = tout;
        let s = Spline::from_nodes(&nodes).unwrap();
        let (a, b) = (
            s.segments()[0].derivative_at(1.0),
            s.segments()[1].derivative_at(0.0),
        );
        assert!((a.x - b.x).hypot(a.y - b.y) > 1.0);
    }

    #[test]
    fn deux_points_droits_donne_un_segment_de_la_bonne_longueur() {
        let s = Spline::from_nodes(&smooth(&[p(0., 0.), p(30., 40.)])).unwrap();
        assert!((s.length_mm().unwrap() - 50.0).abs() < 1e-9);
    }

    #[test]
    fn longueur_totale_est_la_somme_des_troncons() {
        let s = Spline::from_nodes(&smooth(&[p(0., 0.), p(30., 20.), p(60., -5.)])).unwrap();
        let parts = s.segment_lengths_mm().unwrap();
        assert!((parts.iter().sum::<f64>() - s.length_mm().unwrap()).abs() < 1e-12);
        let (pt, _) = s.point_at_length(parts[0]).unwrap();
        assert!(distance_mm(pt, p(30., 20.)) < 1e-4);
        assert!(s.nearest_length_mm(p(30., 20.)).unwrap() - parts[0] < 1e-4);
    }

    #[test]
    fn refus_moins_de_deux_points_confondus_et_non_finis() {
        assert!(Spline::from_nodes(&smooth(&[p(0., 0.)])).is_err());
        assert!(Spline::from_nodes(&[]).is_err());
        assert!(Spline::from_nodes(&smooth(&[p(0., 0.), p(0., 0.)])).is_err());
        assert!(Spline::from_nodes(&smooth(&[p(0., 0.), p(f64::NAN, 1.)])).is_err());
        let mut n = smooth(&[p(0., 0.), p(10., 0.)]);
        n[0].tangent_out = VectorMm::new(f64::INFINITY, 0.0);
        assert!(Spline::from_nodes(&n).is_err());
    }
}

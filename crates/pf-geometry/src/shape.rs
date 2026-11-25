//! Forme d'une courbe : rebroussements, inflexions, courbure maximale (US-09.9).
//!
//! Lecture pure d'une suite de Béziers cubiques (une Bézier, ou une courbe lisse = suite de
//! tronçons). Tout est **échantillonné** : [`CURVE_SAMPLES_PER_SEGMENT`] pas par tronçon, `t`
//! de 0 à 1 inclus. En chaque échantillon, `v = B′(t)`, `a = B″(t)`, vitesse `s = |v|` et
//! courbure signée `κ = (v.x·a.y − v.y·a.x) / s³` (en 1/mm, positive à gauche du sens de parcours).
//!
//! Conventions (nommées ci-dessous, à faire valider par une modéliste) :
//! - **rebroussement** : un échantillon *intérieur* (ni `t = 0` ni `t = 1` d'un tronçon : une
//!   poignée nulle à l'extrémité n'est pas un rebroussement) dont la vitesse est inférieure à
//!   [`CUSP_SPEED_RATIO`] × la vitesse moyenne, ou deux échantillons consécutifs dont la tangente
//!   tourne de plus de [`CUSP_TURN_DEG`] (retour en arrière entre deux pas). Les échantillons
//!   signalés qui se suivent forment **un** rebroussement ;
//! - **inflexion** : changement de signe de `κ` entre deux échantillons *significatifs*
//!   (`|κ| ≥` [`CURVATURE_ZERO_PER_MM`], vitesse hors zone de rebroussement) ;
//! - **courbure maximale** : plus grand `|κ|` des échantillons hors zone de rebroussement, affiné
//!   par une recherche à section dorée entre les deux échantillons voisins du meilleur.
//!
//! Aucune division par une vitesse proche de zéro : ces échantillons sont écartés de `κ`.

use crate::{CubicBezier, GeometryError, Result, VectorMm};

/// Nombre de pas par tronçon.
pub const CURVE_SAMPLES_PER_SEGMENT: usize = 256;
/// Vitesse (rapport à la vitesse moyenne) en dessous de laquelle un échantillon intérieur est un
/// rebroussement.
pub const CUSP_SPEED_RATIO: f64 = 1e-3;
/// Rotation de la tangente entre deux échantillons consécutifs (degrés) au-delà de laquelle il y a
/// retour en arrière, donc rebroussement.
pub const CUSP_TURN_DEG: f64 = 150.0;
/// `|κ|` (1/mm) en dessous duquel la courbe est tenue pour droite (rayon > 100 m).
pub const CURVATURE_ZERO_PER_MM: f64 = 1e-5;
/// Vitesse moyenne (mm par unité de `t`) en dessous de laquelle la courbe est dégénérée.
const MIN_MEAN_SPEED: f64 = 1e-9;
const REFINE_ITERATIONS: usize = 48;

/// Résultat de [`analyze_curvature`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurvatureReport {
    /// Points de rebroussement (zones d'échantillons consécutifs comptées une fois).
    pub cusps: usize,
    /// Changements de signe de la courbure.
    pub inflections: usize,
    /// Plus grand `|κ|` (1/mm) hors zone de rebroussement ; 0 pour une droite.
    pub max_curvature_per_mm: f64,
}

fn second_derivative(c: &CubicBezier, t: f64) -> VectorMm {
    let a = VectorMm::new(
        c.c2.x - 2.0 * c.c1.x + c.p0.x,
        c.c2.y - 2.0 * c.c1.y + c.p0.y,
    );
    let b = VectorMm::new(
        c.p3.x - 2.0 * c.c2.x + c.c1.x,
        c.p3.y - 2.0 * c.c2.y + c.c1.y,
    );
    VectorMm::new(
        6.0 * ((1.0 - t) * a.x + t * b.x),
        6.0 * ((1.0 - t) * a.y + t * b.y),
    )
}

/// Courbure signée (1/mm) d'une Bézier cubique en `t`, ou `None` si la vitesse est nulle.
pub fn signed_curvature(c: &CubicBezier, t: f64) -> Option<f64> {
    let v = c.derivative_at(t);
    let s = v.norm();
    (s > 0.0 && s.is_finite())
        .then(|| v.cross(second_derivative(c, t)) / (s * s * s))
        .filter(|k| k.is_finite())
}

struct Sample {
    interior: bool,
    speed: f64,
    v: VectorMm,
    /// `κ` si la vitesse est hors zone de rebroussement.
    kappa: Option<f64>,
}

/// Analyse de la forme d'une suite de tronçons cubiques. Refuse (`DegenerateCurve`) une suite
/// vide, un tronçon dont tous les points sont confondus ou une vitesse moyenne nulle ; une valeur
/// non finie est refusée (`NonFinite`).
pub fn analyze_curvature(segments: &[CubicBezier]) -> Result<CurvatureReport> {
    if segments.is_empty() {
        return Err(GeometryError::DegenerateCurve);
    }
    for c in segments {
        c.validate()?;
    }
    let n = CURVE_SAMPLES_PER_SEGMENT;
    let mut raw = Vec::with_capacity(segments.len() * (n + 1));
    for c in segments {
        for j in 0..=n {
            let t = j as f64 / n as f64;
            let v = c.derivative_at(t);
            raw.push((j > 0 && j < n, t, v, second_derivative(c, t)));
        }
    }
    let mean = raw.iter().map(|r| r.2.norm()).sum::<f64>() / raw.len() as f64;
    if !mean.is_finite() {
        return Err(GeometryError::NonFinite("la courbe"));
    }
    if mean <= MIN_MEAN_SPEED {
        return Err(GeometryError::DegenerateCurve);
    }
    let slow = mean * CUSP_SPEED_RATIO;
    let samples: Vec<Sample> = raw
        .iter()
        .map(|&(interior, _, v, a)| {
            let speed = v.norm();
            Sample {
                interior,
                speed,
                v,
                kappa: (speed > slow).then(|| v.cross(a) / (speed * speed * speed)),
            }
        })
        .collect();

    // Rebroussements : vitesse presque nulle à l'intérieur, ou retour en arrière entre deux pas.
    let mut flagged: Vec<bool> = samples
        .iter()
        .map(|s| s.interior && s.speed < slow)
        .collect();
    let cos_turn = CUSP_TURN_DEG.to_radians().cos();
    let tiny = mean * 1e-12;
    for (i, w) in samples.windows(2).enumerate() {
        if w[0].speed > tiny
            && w[1].speed > tiny
            && w[0].v.dot(w[1].v) < cos_turn * w[0].speed * w[1].speed
        {
            flagged[i] = true;
            flagged[i + 1] = true;
        }
    }
    let cusps = flagged
        .iter()
        .enumerate()
        .filter(|&(i, &f)| f && (i == 0 || !flagged[i - 1]))
        .count();

    // Inflexions : changements de signe entre échantillons significatifs.
    let mut inflections = 0;
    let mut last = 0.0_f64;
    for k in samples.iter().filter_map(|s| s.kappa) {
        if k.abs() < CURVATURE_ZERO_PER_MM {
            continue;
        }
        if last != 0.0 && k.signum() != last {
            inflections += 1;
        }
        last = k.signum();
    }

    // Courbure maximale : meilleur échantillon, puis affinage entre ses deux voisins.
    let mut best: Option<(usize, f64)> = None;
    for (i, s) in samples.iter().enumerate() {
        if let Some(k) = s.kappa {
            if best.is_none_or(|(_, b)| k.abs() > b) {
                best = Some((i, k.abs()));
            }
        }
    }
    let max_curvature_per_mm = match best {
        None => 0.0,
        Some((i, k)) => {
            let seg = i / (n + 1);
            let j = i % (n + 1);
            let at = |jj: usize| jj as f64 / n as f64;
            let lo = at(j.saturating_sub(1));
            let hi = at((j + 1).min(n));
            let c = &segments[seg];
            let f = |t: f64| -> f64 {
                let v = c.derivative_at(t);
                let s = v.norm();
                if s > slow {
                    (v.cross(second_derivative(c, t)) / (s * s * s)).abs()
                } else {
                    0.0
                }
            };
            k.max(golden_max(f, lo, hi))
        }
    };
    Ok(CurvatureReport {
        cusps,
        inflections,
        max_curvature_per_mm,
    })
}

/// Maximum approché de `f` sur `[lo, hi]` (section dorée, `f` supposée unimodale sur l'intervalle).
fn golden_max(f: impl Fn(f64) -> f64, mut lo: f64, mut hi: f64) -> f64 {
    let g = (5.0_f64.sqrt() - 1.0) / 2.0;
    let mut best = f(lo).max(f(hi));
    let (mut x1, mut x2) = (hi - g * (hi - lo), lo + g * (hi - lo));
    let (mut f1, mut f2) = (f(x1), f(x2));
    for _ in 0..REFINE_ITERATIONS {
        best = best.max(f1).max(f2);
        if f1 < f2 {
            lo = x1;
            x1 = x2;
            f1 = f2;
            x2 = lo + g * (hi - lo);
            f2 = f(x2);
        } else {
            hi = x2;
            x2 = x1;
            f2 = f1;
            x1 = hi - g * (hi - lo);
            f1 = f(x1);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PointMm;

    fn bez(p: [(f64, f64); 4]) -> CubicBezier {
        let q = |i: usize| PointMm::new(p[i].0, p[i].1);
        CubicBezier::new(q(0), q(1), q(2), q(3))
    }

    /// Parabole `y = x² / 200` de 0 à 60 mm : sommet en `t = 0`, `κ = 1/R = 0,01` (R = 100 mm),
    /// décroissante ensuite ; calculée à la main (`x′ = 60`, `y″ = 36`, `κ = 60·36 / 60³`).
    #[test]
    fn parabola_vertex_curvature_is_one_over_radius() {
        let r = analyze_curvature(&[bez([(0.0, 0.0), (20.0, 0.0), (40.0, 6.0), (60.0, 18.0)])])
            .unwrap();
        assert!((r.max_curvature_per_mm - 0.01).abs() < 1e-12, "{r:?}");
        assert_eq!((r.cusps, r.inflections), (0, 0));
    }

    /// Courbe en S symétrique par rapport à (50 ; 0) : une inflexion en `t = 1/2`.
    #[test]
    fn symmetric_s_curve_has_one_inflection() {
        let r = analyze_curvature(&[bez([(0.0, 0.0), (40.0, 40.0), (60.0, -40.0), (100.0, 0.0)])])
            .unwrap();
        assert_eq!((r.cusps, r.inflections), (0, 1), "{r:?}");
    }

    /// `c1 = (60 ; 40)`, `c2 = (0 ; 40)`, `p3 = (60 ; 0)` : `B′(1/2) = 0` exactement.
    #[test]
    fn symmetric_cusp_curve_has_one_cusp() {
        let r = analyze_curvature(&[bez([(0.0, 0.0), (60.0, 40.0), (0.0, 40.0), (60.0, 0.0)])])
            .unwrap();
        assert_eq!((r.cusps, r.inflections), (1, 0), "{r:?}");
        // Plus large (a < D) : simple arche, plus de rebroussement.
        let r = analyze_curvature(&[bez([(0.0, 0.0), (60.0, 40.0), (40.0, 40.0), (100.0, 0.0)])])
            .unwrap();
        assert_eq!(r.cusps, 0, "{r:?}");
    }

    #[test]
    fn straight_curve_has_no_curvature_and_zero_handles_are_no_cusp() {
        let r =
            analyze_curvature(&[bez([(0.0, 0.0), (0.0, 0.0), (80.0, 0.0), (80.0, 0.0)])]).unwrap();
        assert_eq!((r.cusps, r.inflections), (0, 0));
        assert!(r.max_curvature_per_mm < 1e-12);
    }

    #[test]
    fn degenerate_inputs_are_refused_without_panic() {
        assert!(analyze_curvature(&[]).is_err());
        let p = (5.0, 5.0);
        assert_eq!(
            analyze_curvature(&[bez([p, p, p, p])]),
            Err(GeometryError::DegenerateCurve)
        );
    }

    /// Cercle de 100 mm de rayon : quatre Béziers de quart de cercle (k = 0,5522847…) :
    /// courbure maximale 1/100 à 1 % près (l'écart de l'approximation cubique de l'arc, qui
    /// n'est pas un cercle exact).
    #[test]
    fn circle_quarter_curvature_is_close_to_one_over_radius() {
        let k = 0.552_284_749_830_793_4 * 100.0;
        let r = analyze_curvature(&[bez([(100.0, 0.0), (100.0, k), (k, 100.0), (0.0, 100.0)])])
            .unwrap();
        assert!((r.max_curvature_per_mm - 0.01).abs() < 1e-4, "{r:?}");
        assert_eq!((r.cusps, r.inflections), (0, 0));
    }
}

//! Ajustement d'une polyligne par des Béziers cubiques (US-14.2, lissage du décalque).
//!
//! Méthode : moindres carrés de Schneider (Graphics Gems, 1990). Pour un tronçon de points
//! `d[0..n]`, paramétrage par longueur de corde ; les extrémités sont imposées, les directions
//! des poignées sont fixées par des tangentes estimées aux extrémités, leurs **longueurs** `a1`, `a2`
//! sont la solution des moindres carrés (système 2 × 2 sur la somme des carrés des écarts
//! `|B(u_i) - d_i|`). Jusqu'à 4 reparamétrages de Newton-Raphson, puis, si l'écart maximal dépasse
//! la tolérance, coupure au point le plus écarté (tangente centrée) et récursion.
//!
//! Garantie : chaque point d'entrée est à au plus `tolerance_mm` de la courbe résultat (écart
//! mesuré au paramètre retenu, donc majorant la distance réelle). Entre deux points d'entrée,
//! aucune garantie. Les coins (virage > `corner_deg`) coupent la polyligne : chaque côté est ajusté
//! séparément, la courbe résultat passe exactement par le coin. Rien n'est « imposé » : le
//! résultat est une suite de Béziers ordinaires.

use crate::{distance_mm, CubicBezier, GeometryError, PointMm};

/// Nombre maximal de points acceptés (borne le coût de la récursion).
pub const MAX_FIT_POINTS: usize = 20_000;

/// Résultat d'un ajustement.
#[derive(Debug, Clone, PartialEq)]
pub struct Fit {
    /// Béziers consécutifs (la fin de l'un est le départ du suivant).
    pub curves: Vec<CubicBezier>,
    /// Écart maximal observé entre un point d'entrée et la courbe (mm).
    pub max_error_mm: f64,
}

fn err(msg: &str) -> GeometryError {
    GeometryError::InvalidConstruction(msg.to_string())
}

type V = (f64, f64);

fn sub(a: PointMm, b: PointMm) -> V {
    (a.x - b.x, a.y - b.y)
}
fn dot(a: V, b: V) -> f64 {
    a.0 * b.0 + a.1 * b.1
}
fn unit(v: V) -> V {
    let n = dot(v, v).sqrt();
    if n < 1e-300 {
        (0.0, 0.0)
    } else {
        (v.0 / n, v.1 / n)
    }
}

fn eval(b: &CubicBezier, t: f64) -> PointMm {
    b.point_at(t)
}

fn eval_d1(b: &CubicBezier, t: f64) -> V {
    let d = b.derivative_at(t);
    (d.x, d.y)
}

/// Dérivée seconde d'une Bézier cubique.
fn eval_d2(b: &CubicBezier, t: f64) -> V {
    let u = 1.0 - t;
    let a = (
        b.p0.x - 2.0 * b.c1.x + b.c2.x,
        b.p0.y - 2.0 * b.c1.y + b.c2.y,
    );
    let c = (
        b.c1.x - 2.0 * b.c2.x + b.p3.x,
        b.c1.y - 2.0 * b.c2.y + b.p3.y,
    );
    (6.0 * (u * a.0 + t * c.0), 6.0 * (u * a.1 + t * c.1))
}

fn chord_params(d: &[PointMm]) -> Vec<f64> {
    let mut u = vec![0.0];
    for i in 1..d.len() {
        u.push(u[i - 1] + distance_mm(d[i], d[i - 1]));
    }
    let total = *u.last().unwrap();
    for v in u.iter_mut() {
        *v /= total;
    }
    u
}

/// Béziers aux extrémités imposées, longueurs de poignées par moindres carrés.
fn generate(d: &[PointMm], u: &[f64], t1: V, t2: V) -> CubicBezier {
    let (first, last) = (d[0], d[d.len() - 1]);
    let (mut c00, mut c01, mut c11) = (0.0, 0.0, 0.0);
    let (mut x0, mut x1) = (0.0, 0.0);
    for (i, &t) in u.iter().enumerate() {
        let s = 1.0 - t;
        let (b0, b1, b2, b3) = (s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t);
        let a1 = (t1.0 * b1, t1.1 * b1);
        let a2 = (t2.0 * b2, t2.1 * b2);
        c00 += dot(a1, a1);
        c01 += dot(a1, a2);
        c11 += dot(a2, a2);
        let tmp = (
            d[i].x - (first.x * (b0 + b1) + last.x * (b2 + b3)),
            d[i].y - (first.y * (b0 + b1) + last.y * (b2 + b3)),
        );
        x0 += dot(a1, tmp);
        x1 += dot(a2, tmp);
    }
    let det = c00 * c11 - c01 * c01;
    let seg = distance_mm(first, last);
    let eps = 1e-6 * seg;
    let (mut al, mut ar) = if det.abs() > 1e-12 {
        ((x0 * c11 - x1 * c01) / det, (c00 * x1 - c01 * x0) / det)
    } else {
        (seg / 3.0, seg / 3.0)
    };
    // Poignées dégénérées ou à l'envers : repli sur le tiers de la corde (règle de Schneider).
    if al < eps || ar < eps || !al.is_finite() || !ar.is_finite() {
        al = seg / 3.0;
        ar = seg / 3.0;
    }
    CubicBezier::new(
        first,
        PointMm::new(first.x + t1.0 * al, first.y + t1.1 * al),
        PointMm::new(last.x + t2.0 * ar, last.y + t2.1 * ar),
        last,
    )
}

fn newton(b: &CubicBezier, p: PointMm, u: f64) -> f64 {
    let q = eval(b, u);
    let d1 = eval_d1(b, u);
    let d2 = eval_d2(b, u);
    let diff = (q.x - p.x, q.y - p.y);
    let num = dot(diff, d1);
    let den = dot(d1, d1) + dot(diff, d2);
    if den.abs() < 1e-12 {
        u
    } else {
        (u - num / den).clamp(0.0, 1.0)
    }
}

/// Écart maximal et indice du point le plus écarté (hors extrémités si possible).
fn max_error(d: &[PointMm], b: &CubicBezier, u: &[f64]) -> (f64, usize) {
    let mut worst = (0.0, d.len() / 2);
    for i in 0..d.len() {
        let e = distance_mm(eval(b, u[i]), d[i]);
        if e > worst.0 {
            worst = (e, i);
        }
    }
    // Un point de coupe doit être intérieur.
    worst.1 = worst.1.clamp(1, d.len() - 2);
    worst
}

fn center_tangent(d: &[PointMm], i: usize) -> V {
    let a = sub(d[i - 1], d[i]);
    let b = sub(d[i], d[i + 1]);
    unit(((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0))
}

fn fit_cubic(d: &[PointMm], t1: V, t2: V, tol: f64, out: &mut Vec<(CubicBezier, f64)>) {
    if d.len() == 2 {
        let dist = distance_mm(d[0], d[1]) / 3.0;
        out.push((
            CubicBezier::new(
                d[0],
                PointMm::new(d[0].x + t1.0 * dist, d[0].y + t1.1 * dist),
                PointMm::new(d[1].x + t2.0 * dist, d[1].y + t2.1 * dist),
                d[1],
            ),
            0.0,
        ));
        return;
    }
    let mut u = chord_params(d);
    let mut b = generate(d, &u, t1, t2);
    let (mut err, mut split) = max_error(d, &b, &u);
    if err <= tol {
        out.push((b, err));
        return;
    }
    for _ in 0..4 {
        for (i, ui) in u.iter_mut().enumerate() {
            *ui = newton(&b, d[i], *ui);
        }
        b = generate(d, &u, t1, t2);
        (err, split) = max_error(d, &b, &u);
        if err <= tol {
            out.push((b, err));
            return;
        }
    }
    let tc = center_tangent(d, split);
    fit_cubic(&d[..=split], t1, tc, tol, out);
    fit_cubic(&d[split..], (-tc.0, -tc.1), t2, tol, out);
}

/// Ajuste un tronçon sans coin.
fn fit_run(d: &[PointMm], tol: f64) -> Vec<(CubicBezier, f64)> {
    let n = d.len();
    let t1 = unit(sub(d[1], d[0]));
    let t2 = unit(sub(d[n - 2], d[n - 1]));
    let mut out = Vec::new();
    fit_cubic(d, t1, t2, tol, &mut out);
    out
}

/// Angle de virage en `d[i]` en degrés (0 = tout droit).
fn turn_deg(a: PointMm, b: PointMm, c: PointMm) -> f64 {
    let u = unit(sub(b, a));
    let v = unit(sub(c, b));
    let cos = dot(u, v).clamp(-1.0, 1.0);
    cos.acos().to_degrees()
}

/// Écart maximal réel entre chaque point et la courbe (échantillonnage fin) : contrôle indépendant,
/// quadratique, réservé aux tests et aux petites entrées.
pub fn max_deviation_mm(points: &[PointMm], curves: &[CubicBezier]) -> f64 {
    const N: usize = 64;
    let dist_to = |c: &CubicBezier, p: PointMm| -> f64 {
        let mut best = (f64::INFINITY, 0usize);
        for i in 0..=N {
            let d = distance_mm(p, c.point_at(i as f64 / N as f64));
            if d < best.0 {
                best = (d, i);
            }
        }
        // Raffinement par recherche ternaire autour de l'échantillon le plus proche.
        let mut lo = best.1.saturating_sub(1) as f64 / N as f64;
        let mut hi = (best.1 + 1).min(N) as f64 / N as f64;
        for _ in 0..60 {
            let m1 = lo + (hi - lo) / 3.0;
            let m2 = hi - (hi - lo) / 3.0;
            if distance_mm(p, c.point_at(m1)) < distance_mm(p, c.point_at(m2)) {
                hi = m2;
            } else {
                lo = m1;
            }
        }
        best.0.min(distance_mm(p, c.point_at((lo + hi) / 2.0)))
    };
    points
        .iter()
        .map(|p| {
            curves
                .iter()
                .map(|c| dist_to(c, *p))
                .fold(f64::INFINITY, f64::min)
        })
        .fold(0.0, f64::max)
}

/// Ajuste la polyligne `points` par des Béziers à `tolerance_mm` près (voir le module).
/// `corner_deg` : virage au-delà duquel un sommet est un coin (`None` = aucun coin, courbe lisse).
/// Refus : moins de 2 points distincts, point non fini, tolérance ≤ 0 ou non finie, trop de points.
pub fn fit_polyline(
    points: &[PointMm],
    tolerance_mm: f64,
    corner_deg: Option<f64>,
) -> Result<Fit, GeometryError> {
    if !tolerance_mm.is_finite() || tolerance_mm <= 0.0 {
        return Err(GeometryError::InvalidTolerance(tolerance_mm));
    }
    if points.len() > MAX_FIT_POINTS {
        return Err(err("trop de points à ajuster"));
    }
    if points.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
        return Err(err("point non fini"));
    }
    if let Some(c) = corner_deg {
        if !c.is_finite() || c <= 0.0 || c > 180.0 {
            return Err(err("angle de coin hors de ]0 ; 180]"));
        }
    }
    // Points consécutifs confondus retirés (ils annuleraient les tangentes).
    let mut d: Vec<PointMm> = Vec::with_capacity(points.len());
    for p in points {
        if d.last().is_none_or(|q| distance_mm(*p, *q) > 1e-9) {
            d.push(*p);
        }
    }
    if d.len() < 2 {
        return Err(err("il faut au moins deux points distincts"));
    }
    // Découpe aux coins.
    let mut cuts = vec![0usize];
    if let Some(c) = corner_deg {
        for i in 1..d.len() - 1 {
            if turn_deg(d[i - 1], d[i], d[i + 1]) > c {
                cuts.push(i);
            }
        }
    }
    cuts.push(d.len() - 1);
    let mut curves = Vec::new();
    let mut max_error_mm = 0.0_f64;
    for w in cuts.windows(2) {
        for (c, e) in fit_run(&d[w[0]..=w[1]], tolerance_mm) {
            curves.push(c);
            max_error_mm = max_error_mm.max(e);
        }
    }
    Ok(Fit {
        curves,
        max_error_mm,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f64, y: f64) -> PointMm {
        PointMm::new(x, y)
    }

    #[test]
    fn points_alignes_donnent_une_droite_dans_la_tolerance() {
        let pts: Vec<PointMm> = (0..=10)
            .map(|i| p(i as f64 * 10.0, i as f64 * 5.0))
            .collect();
        let f = fit_polyline(&pts, 0.1, None).unwrap();
        assert_eq!(f.curves.len(), 1);
        assert!(f.max_error_mm < 1e-6, "{}", f.max_error_mm);
        assert!(max_deviation_mm(&pts, &f.curves) < 1e-3);
    }

    #[test]
    fn quart_de_cercle_echantillonne_reste_dans_la_tolerance() {
        let r = 100.0;
        let pts: Vec<PointMm> = (0..=30)
            .map(|i| {
                let a = (i as f64) * 90.0 / 30.0;
                p(r * a.to_radians().cos(), r * a.to_radians().sin())
            })
            .collect();
        let f = fit_polyline(&pts, 0.2, None).unwrap();
        assert!(f.max_error_mm <= 0.2, "{}", f.max_error_mm);
        assert!(max_deviation_mm(&pts, &f.curves) <= 0.2);
        assert!(f.curves.len() <= 3, "{}", f.curves.len());
        // Continuité de position.
        for w in f.curves.windows(2) {
            assert_eq!(w[0].p3, w[1].p0);
        }
    }

    #[test]
    fn un_coin_a_angle_droit_est_conserve() {
        let pts = [
            p(0.0, 0.0),
            p(50.0, 0.0),
            p(100.0, 0.0),
            p(100.0, 50.0),
            p(100.0, 100.0),
        ];
        let f = fit_polyline(&pts, 0.1, Some(45.0)).unwrap();
        assert_eq!(f.curves.len(), 2);
        assert_eq!(f.curves[0].p3, p(100.0, 0.0));
        assert!(f.max_error_mm < 1e-6);
    }

    #[test]
    fn tolerance_plus_serree_donne_au_moins_autant_de_courbes() {
        let pts: Vec<PointMm> = (0..=40)
            .map(|i| {
                let x = i as f64 * 5.0;
                p(x, 20.0 * (x / 30.0).sin())
            })
            .collect();
        let large = fit_polyline(&pts, 2.0, None).unwrap();
        let fine = fit_polyline(&pts, 0.05, None).unwrap();
        assert!(fine.curves.len() >= large.curves.len());
        assert!(fine.max_error_mm <= 0.05 && large.max_error_mm <= 2.0);
    }

    #[test]
    fn refus() {
        assert!(fit_polyline(&[p(0.0, 0.0)], 1.0, None).is_err());
        assert!(fit_polyline(&[p(1.0, 1.0), p(1.0, 1.0)], 1.0, None).is_err());
        assert!(fit_polyline(&[p(0.0, 0.0), p(1.0, 0.0)], 0.0, None).is_err());
        assert!(fit_polyline(&[p(0.0, 0.0), p(f64::NAN, 0.0)], 1.0, None).is_err());
        assert!(fit_polyline(&[p(0.0, 0.0), p(1.0, 0.0)], 1.0, Some(0.0)).is_err());
    }
}

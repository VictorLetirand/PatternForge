//! US-03.3 — recherche numérique **monotone à une inconnue** (ADR-B : bissection / Brent, pas de
//! solveur global).
//!
//! [`solve_monotone`] cherche `x` tel que `f(x) = cible` (à [`SOLVE_TOLERANCE_MM`]) sur un
//! intervalle borné où `f` est continue et monotone ; il refuse proprement une cible hors de la plage
//! atteignable (avec les bornes), un intervalle vide, une cible non positive ou une fonction qui
//! n'est pas monotone (contrôlée par échantillonnage). [`solve_convex`] sert les fonctions
//! **convexes** (la longueur d'une Bézier quand une poignée ou une extrémité glisse l'est : sa
//! dérivée est affine en l'inconnue, donc sa norme convexe) : il localise le minimum, choisit la
//! branche monotone qui contient la position de départ, puis appelle [`solve_monotone`].
//!
//! Module pur : aucune dépendance au modèle, sert US-03.3 et la suite (US-03.4, égalité de longueurs).

use std::fmt;

/// Précision garantie sur la valeur de la fonction (mm) : critère d'acceptation d'une solution.
pub const SOLVE_TOLERANCE_MM: f64 = 1e-6;
/// Écart en dessous duquel la position de départ est déjà une solution (aucun déplacement).
pub const SOLVE_CLOSE_MM: f64 = 1e-9;
/// Nombre de pas de l'échantillonnage de monotonie.
const MONOTONE_SAMPLES: usize = 32;
/// Itérations maximales de Brent.
const MAX_ITERATIONS: usize = 200;
/// Itérations de la section dorée (le minimum est localisé à 1e-13 de l'intervalle).
const GOLDEN_ITERATIONS: usize = 64;

/// Pourquoi une recherche échoue.
#[derive(Debug, Clone, PartialEq)]
pub enum SolveError {
    /// La cible n'est pas un nombre fini strictement positif.
    InvalidTarget(f64),
    /// L'intervalle de recherche est vide ou n'est pas fini.
    EmptyInterval { lo: f64, hi: f64 },
    /// La cible est hors de la plage atteignable `[min ; max]` de la fonction sur `[lo ; hi]`.
    OutOfRange {
        target: f64,
        min: f64,
        max: f64,
        lo: f64,
        hi: f64,
    },
    /// La fonction n'est pas monotone sur l'intervalle (plusieurs solutions possibles).
    NotMonotone { at: f64 },
    /// L'évaluation de la fonction a échoué.
    Evaluation(String),
    /// La recherche n'a pas atteint la tolérance demandée.
    NoConvergence { residual: f64 },
}

impl fmt::Display for SolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SolveError::InvalidTarget(t) => {
                write!(
                    f,
                    "la longueur cible {t} n'est pas un nombre strictement positif"
                )
            }
            SolveError::EmptyInterval { lo, hi } => {
                write!(f, "plage de recherche vide ou invalide [{lo} ; {hi}]")
            }
            SolveError::OutOfRange {
                target, min, max, ..
            } => write!(
                f,
                "la cible {target} est hors de la plage atteignable [{min} ; {max}]"
            ),
            SolveError::NotMonotone { at } => {
                write!(f, "la fonction n'est pas monotone (autour de {at})")
            }
            SolveError::Evaluation(m) => write!(f, "évaluation impossible : {m}"),
            SolveError::NoConvergence { residual } => {
                write!(f, "pas de convergence (écart restant {residual})")
            }
        }
    }
}

impl std::error::Error for SolveError {}

/// Fonction scalaire évaluée par le solveur ; l'erreur est un message déjà lisible.
pub type ScalarFn<'a> = &'a mut dyn FnMut(f64) -> Result<f64, String>;

fn eval(f: &mut dyn FnMut(f64) -> Result<f64, String>, x: f64) -> Result<f64, SolveError> {
    let v = f(x).map_err(SolveError::Evaluation)?;
    if v.is_finite() {
        Ok(v)
    } else {
        Err(SolveError::Evaluation(format!(
            "valeur non finie pour x = {x}"
        )))
    }
}

/// Brent sur `g(x) = f(x) − cible`, `a` et `b` encadrant une racine (signes opposés ou nuls).
#[allow(clippy::too_many_arguments)]
fn brent(
    f: &mut dyn FnMut(f64) -> Result<f64, String>,
    target: f64,
    mut a: f64,
    mut b: f64,
    mut fa: f64,
    mut fb: f64,
    ftol: f64,
    xtol: f64,
) -> Result<f64, SolveError> {
    let (mut c, mut fc) = (b, fb);
    let (mut d, mut e) = (0.0_f64, 0.0_f64);
    for _ in 0..MAX_ITERATIONS {
        if (fb > 0.0 && fc > 0.0) || (fb < 0.0 && fc < 0.0) {
            c = a;
            fc = fa;
            d = b - a;
            e = d;
        }
        if fc.abs() < fb.abs() {
            a = b;
            b = c;
            c = a;
            fa = fb;
            fb = fc;
            fc = fa;
        }
        let tol1 = 2.0 * f64::EPSILON * b.abs() + 0.5 * xtol;
        let xm = 0.5 * (c - b);
        if xm.abs() <= tol1 || fb == 0.0 || fb.abs() <= ftol {
            return Ok(b);
        }
        if e.abs() >= tol1 && fa.abs() > fb.abs() {
            let s = fb / fa;
            let (mut p, mut q);
            if a == c {
                p = 2.0 * xm * s;
                q = 1.0 - s;
            } else {
                let qq = fa / fc;
                let r = fb / fc;
                p = s * (2.0 * xm * qq * (qq - r) - (b - a) * (r - 1.0));
                q = (qq - 1.0) * (r - 1.0) * (s - 1.0);
            }
            if p > 0.0 {
                q = -q;
            }
            p = p.abs();
            let min1 = 3.0 * xm * q - (tol1 * q).abs();
            let min2 = (e * q).abs();
            if 2.0 * p < min1.min(min2) {
                e = d;
                d = p / q;
            } else {
                d = xm;
                e = d;
            }
        } else {
            d = xm;
            e = d;
        }
        a = b;
        fa = fb;
        if d.abs() > tol1 {
            b += d;
        } else {
            b += tol1.copysign(xm);
        }
        fb = eval(f, b)? - target;
    }
    Err(SolveError::NoConvergence { residual: fb.abs() })
}

/// Cherche `x` dans `[lo ; hi]` tel que `f(x) = target` (à `tolerance`), `f` continue et monotone
/// sur l'intervalle. Les deux extrémités sont évaluées, la cible doit être dans la plage qu'elles
/// bornent (à `tolerance` près), la monotonie est contrôlée sur 32 pas, puis Brent.
pub fn solve_monotone(
    f: ScalarFn<'_>,
    lo: f64,
    hi: f64,
    target: f64,
    tolerance: f64,
) -> Result<f64, SolveError> {
    if !target.is_finite() || target <= 0.0 {
        return Err(SolveError::InvalidTarget(target));
    }
    if !(lo.is_finite() && hi.is_finite() && lo < hi) {
        return Err(SolveError::EmptyInterval { lo, hi });
    }
    let flo = eval(f, lo)?;
    let fhi = eval(f, hi)?;
    let (min, max) = (flo.min(fhi), flo.max(fhi));
    // Monotonie d'abord : sur une fonction non monotone, les bornes des extrémités ne disent rien
    // de la plage atteignable.
    let increasing = fhi >= flo;
    let slack = 1e-9 * (1.0 + max.abs().max(min.abs()));
    let mut prev = flo;
    for i in 1..=MONOTONE_SAMPLES {
        let x = lo + (hi - lo) * (i as f64) / (MONOTONE_SAMPLES as f64);
        let v = if i == MONOTONE_SAMPLES {
            fhi
        } else {
            eval(f, x)?
        };
        if (increasing && v < prev - slack) || (!increasing && v > prev + slack) {
            return Err(SolveError::NotMonotone { at: x });
        }
        prev = v;
    }
    if target < min - tolerance || target > max + tolerance {
        return Err(SolveError::OutOfRange {
            target,
            min,
            max,
            lo,
            hi,
        });
    }
    let (ga, gb) = (flo - target, fhi - target);
    let x = if ga.abs() <= SOLVE_CLOSE_MM {
        lo
    } else if gb.abs() <= SOLVE_CLOSE_MM {
        hi
    } else if (ga > 0.0) == (gb > 0.0) {
        // Cible dans la tolérance mais au-delà d'une borne : la borne est la meilleure solution.
        if ga.abs() < gb.abs() {
            lo
        } else {
            hi
        }
    } else {
        brent(f, target, lo, hi, ga, gb, SOLVE_CLOSE_MM * 0.1, 1e-12)?
    };
    let residual = (eval(f, x)? - target).abs();
    if residual > tolerance {
        return Err(SolveError::NoConvergence { residual });
    }
    Ok(x)
}

/// Abscisse du minimum d'une fonction convexe sur `[lo ; hi]` (section dorée).
fn golden_min(
    f: &mut dyn FnMut(f64) -> Result<f64, String>,
    mut lo: f64,
    mut hi: f64,
) -> Result<f64, SolveError> {
    let inv_phi = 0.5 * (5.0_f64.sqrt() - 1.0);
    let mut x1 = hi - inv_phi * (hi - lo);
    let mut x2 = lo + inv_phi * (hi - lo);
    let (mut f1, mut f2) = (eval(f, x1)?, eval(f, x2)?);
    for _ in 0..GOLDEN_ITERATIONS {
        if f1 <= f2 {
            hi = x2;
            x2 = x1;
            f2 = f1;
            x1 = hi - inv_phi * (hi - lo);
            f1 = eval(f, x1)?;
        } else {
            lo = x1;
            x1 = x2;
            f1 = f2;
            x2 = lo + inv_phi * (hi - lo);
            f2 = eval(f, x2)?;
        }
    }
    Ok(0.5 * (lo + hi))
}

/// Cherche `x` dans `[lo ; hi]` tel que `f(x) = target`, `f` **convexe**. Le minimum `x*` partage
/// l'intervalle en deux branches monotones ; on prend celle qui contient `start` (la position
/// courante : la solution la plus proche, jamais un saut de l'autre côté du minimum), puis on
/// résout comme [`solve_monotone`]. `start` déjà solution (à [`SOLVE_CLOSE_MM`]) : renvoyé tel quel.
pub fn solve_convex(
    f: ScalarFn<'_>,
    start: f64,
    lo: f64,
    hi: f64,
    target: f64,
    tolerance: f64,
) -> Result<f64, SolveError> {
    if !target.is_finite() || target <= 0.0 {
        return Err(SolveError::InvalidTarget(target));
    }
    if !(lo.is_finite() && hi.is_finite() && lo < hi && start >= lo && start <= hi) {
        return Err(SolveError::EmptyInterval { lo, hi });
    }
    if (eval(f, start)? - target).abs() <= SOLVE_CLOSE_MM {
        return Ok(start);
    }
    let x_min = golden_min(f, lo, hi)?;
    if start >= x_min {
        solve_monotone(f, x_min.max(lo), hi, target, tolerance)
    } else {
        solve_monotone(f, lo, x_min.min(hi), target, tolerance)
    }
}

/// Pas d'échantillonnage de [`solve_nearest`] de chaque côté de la position de départ.
const NEAREST_STEPS: usize = 128;

/// Cherche `x` dans `[lo ; hi]` tel que `f(x) = target` pour une fonction **continue quelconque**
/// (courbe lisse, non convexe : US-03.3b). On marche depuis `start` vers `hi` puis vers `lo` sur
/// [`NEAREST_STEPS`] pas ; le premier changement de signe de `f − target` de chaque côté encadre une
/// racine, la plus proche de `start` est affinée (Brent, via [`solve_monotone`]). Une évaluation
/// impossible loin de `start` arrête la marche de ce côté (jamais une erreur) ; en `start`, c'est une
/// erreur. **Limite assumée** : deux racines dans le même pas d'échantillonnage passent inaperçues.
/// Aucune racine sur la plage : `OutOfRange` avec le minimum et le maximum **échantillonnés**.
pub fn solve_nearest(
    f: ScalarFn<'_>,
    start: f64,
    lo: f64,
    hi: f64,
    target: f64,
    tolerance: f64,
) -> Result<f64, SolveError> {
    if !target.is_finite() || target <= 0.0 {
        return Err(SolveError::InvalidTarget(target));
    }
    if !(lo.is_finite() && hi.is_finite() && lo < hi && start >= lo && start <= hi) {
        return Err(SolveError::EmptyInterval { lo, hi });
    }
    let f0 = eval(f, start)?;
    if (f0 - target).abs() <= SOLVE_CLOSE_MM {
        return Ok(start);
    }
    let (mut min, mut max) = (f0, f0);
    let mut brackets: Vec<(f64, f64)> = Vec::new();
    for end in [hi, lo] {
        let (mut prev_x, mut prev_g) = (start, f0 - target);
        for i in 1..=NEAREST_STEPS {
            let x = start + (end - start) * (i as f64) / (NEAREST_STEPS as f64);
            let Ok(v) = eval(f, x) else {
                break;
            };
            min = min.min(v);
            max = max.max(v);
            let g = v - target;
            if g == 0.0 || (g > 0.0) != (prev_g > 0.0) {
                brackets.push((prev_x.min(x), prev_x.max(x)));
                break;
            }
            prev_x = x;
            prev_g = g;
        }
    }
    let nearest = brackets.into_iter().min_by(|a, b| {
        let da = (a.0 - start).abs().min((a.1 - start).abs());
        let db = (b.0 - start).abs().min((b.1 - start).abs());
        da.total_cmp(&db)
    });
    match nearest {
        Some((a, b)) => {
            // Le changement de signe garantit une racine (fonction continue) : Brent direct, sans
            // exiger la monotonie sur le pas (un extremum dans le pas ne doit rien refuser).
            let (ga, gb) = (eval(f, a)? - target, eval(f, b)? - target);
            let x = if ga.abs() <= SOLVE_CLOSE_MM {
                a
            } else if gb.abs() <= SOLVE_CLOSE_MM {
                b
            } else {
                brent(f, target, a, b, ga, gb, SOLVE_CLOSE_MM * 0.1, 1e-12)?
            };
            let residual = (eval(f, x)? - target).abs();
            if residual > tolerance {
                return Err(SolveError::NoConvergence { residual });
            }
            Ok(x)
        }
        None => Err(SolveError::OutOfRange {
            target,
            min,
            max,
            lo,
            hi,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_prend_la_racine_la_plus_proche_d_une_fonction_non_convexe() {
        // f(x) = 10 + 4 sin(x) sur [0 ; 12], départ x = 2 (f = 13,637), cible 12 :
        // sin(x) = 0,5 → x = π/6 + 2kπ ou 5π/6 + 2kπ ; la plus proche de 2 est 5π/6 = 2,61799388.
        let mut f = |x: f64| Ok(10.0 + 4.0 * x.sin());
        let x = solve_nearest(&mut f, 2.0, 0.0, 12.0, 12.0, SOLVE_TOLERANCE_MM).unwrap();
        assert!((x - 5.0 * std::f64::consts::PI / 6.0).abs() < 1e-6, "{x}");
    }

    #[test]
    fn nearest_hors_plage_donne_les_bornes_echantillonnees() {
        let mut f = |x: f64| Ok(10.0 + x * x);
        match solve_nearest(&mut f, 1.0, -3.0, 3.0, 100.0, SOLVE_TOLERANCE_MM) {
            Err(SolveError::OutOfRange { min, max, .. }) => {
                assert!(
                    (min - 10.0).abs() < 0.5 && (max - 19.0).abs() < 1e-9,
                    "{min} {max}"
                );
            }
            other => panic!("{other:?}"),
        }
    }

    fn ok(f: impl Fn(f64) -> f64) -> impl FnMut(f64) -> Result<f64, String> {
        move |x| Ok(f(x))
    }

    #[test]
    fn racine_carree_par_brent() {
        // x² = 2 sur [0 ; 10] : racine √2, calculée à la main (1,41421356…).
        let mut f = ok(|x| x * x);
        let x = solve_monotone(&mut f, 0.0, 10.0, 2.0, SOLVE_TOLERANCE_MM).unwrap();
        assert!((x - 2.0_f64.sqrt()).abs() < 1e-9, "{x}");
    }

    #[test]
    fn fonction_decroissante() {
        // 100 / (1 + x) = 25 sur [0 ; 10] : x = 3.
        let mut f = ok(|x| 100.0 / (1.0 + x));
        let x = solve_monotone(&mut f, 0.0, 10.0, 25.0, SOLVE_TOLERANCE_MM).unwrap();
        assert!((x - 3.0).abs() < 1e-9, "{x}");
    }

    #[test]
    fn cible_hors_plage_donne_les_bornes() {
        let mut f = ok(|x| 2.0 * x + 1.0);
        match solve_monotone(&mut f, 0.0, 10.0, 50.0, SOLVE_TOLERANCE_MM) {
            Err(SolveError::OutOfRange {
                min, max, lo, hi, ..
            }) => {
                assert_eq!((min, max, lo, hi), (1.0, 21.0, 0.0, 10.0));
            }
            autre => panic!("{autre:?}"),
        }
        assert!(matches!(
            solve_monotone(&mut f, 0.0, 10.0, 0.5, SOLVE_TOLERANCE_MM),
            Err(SolveError::OutOfRange { .. })
        ));
    }

    #[test]
    fn cas_degeneres_refuses() {
        let mut f = ok(|x| x + 1.0);
        for t in [0.0, -3.0, f64::NAN, f64::INFINITY] {
            assert!(matches!(
                solve_monotone(&mut f, 0.0, 10.0, t, SOLVE_TOLERANCE_MM),
                Err(SolveError::InvalidTarget(_))
            ));
        }
        for (lo, hi) in [
            (5.0, 5.0),
            (6.0, 5.0),
            (0.0, f64::INFINITY),
            (f64::NAN, 1.0),
        ] {
            assert!(matches!(
                solve_monotone(&mut f, lo, hi, 3.0, SOLVE_TOLERANCE_MM),
                Err(SolveError::EmptyInterval { .. })
            ));
        }
    }

    #[test]
    fn non_monotone_detecte() {
        // (x − 5)² + 1 sur [0 ; 10] : cible 5 atteinte deux fois (x = 3 et x = 7).
        let mut f = ok(|x| (x - 5.0) * (x - 5.0) + 1.0);
        assert!(matches!(
            solve_monotone(&mut f, 0.0, 10.0, 5.0, SOLVE_TOLERANCE_MM),
            Err(SolveError::NotMonotone { .. })
        ));
    }

    #[test]
    fn erreur_d_evaluation_propagee() {
        let mut f = |_x: f64| -> Result<f64, String> { Err("dégénéré".into()) };
        assert_eq!(
            solve_monotone(&mut f, 0.0, 1.0, 1.0, SOLVE_TOLERANCE_MM),
            Err(SolveError::Evaluation("dégénéré".into()))
        );
    }

    #[test]
    fn convexe_choisit_la_branche_du_depart() {
        // (x − 5)² + 1 = 5 : x = 3 ou x = 7 ; départ à droite → 7, à gauche → 3.
        let mut f = ok(|x| (x - 5.0) * (x - 5.0) + 1.0);
        let right = solve_convex(&mut f, 8.0, -20.0, 20.0, 5.0, SOLVE_TOLERANCE_MM).unwrap();
        assert!((right - 7.0).abs() < 1e-7, "{right}");
        let left = solve_convex(&mut f, 1.0, -20.0, 20.0, 5.0, SOLVE_TOLERANCE_MM).unwrap();
        assert!((left - 3.0).abs() < 1e-7, "{left}");
    }

    #[test]
    fn convexe_cible_sous_le_minimum_refusee_avec_bornes() {
        let mut f = ok(|x| (x - 5.0) * (x - 5.0) + 1.0);
        match solve_convex(&mut f, 8.0, -20.0, 20.0, 0.5, SOLVE_TOLERANCE_MM) {
            Err(SolveError::OutOfRange { min, max, .. }) => {
                assert!((min - 1.0).abs() < 1e-9, "{min}");
                assert!((max - 226.0).abs() < 1e-9, "{max}");
            }
            autre => panic!("{autre:?}"),
        }
    }

    #[test]
    fn convexe_depart_deja_solution_ne_bouge_pas() {
        let mut f = ok(|x| x * x + 1.0);
        let x = solve_convex(&mut f, 2.0, -20.0, 20.0, 5.0, SOLVE_TOLERANCE_MM).unwrap();
        assert_eq!(x, 2.0);
    }
}

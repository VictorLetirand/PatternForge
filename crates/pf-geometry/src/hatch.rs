//! Motifs de surface d'une zone (US-06.10) : hachures parallèles et grille de points, découpées
//! par un polygone (règle pair-impair), et point intérieur pour poser une étiquette.
//!
//! Les lignes de balayage sont à `(k + 0,5) × pas` dans le repère tourné : un balayage ne passe
//! donc jamais exactement par un sommet d'un polygone aligné sur la grille. Tout est en mm et ne
//! panique jamais (polygone ou pas invalide : résultat vide).

use crate::{point_in_polygon, PointMm};

/// Nombre maximal de hachures ou de points d'un motif : le pas est augmenté au-delà.
pub const PATTERN_MAX_ITEMS: usize = 2000;

fn valid(polygon: &[PointMm], step: f64) -> bool {
    polygon.len() >= 3
        && step.is_finite()
        && step > 0.0
        && polygon.iter().all(|p| p.x.is_finite() && p.y.is_finite())
}

/// Intervalles `[x0, x1]` où l'horizontale `y` est dans le polygone (pair-impair).
fn spans(polygon: &[PointMm], y: f64) -> Vec<(f64, f64)> {
    let n = polygon.len();
    let mut xs: Vec<f64> = Vec::new();
    for i in 0..n {
        let (a, b) = (polygon[i], polygon[(i + 1) % n]);
        if (a.y > y) != (b.y > y) {
            xs.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
        }
    }
    xs.sort_by(f64::total_cmp);
    xs.chunks_exact(2).map(|c| (c[0], c[1])).collect()
}

fn rotate(p: PointMm, cos: f64, sin: f64) -> PointMm {
    PointMm::new(p.x * cos - p.y * sin, p.x * sin + p.y * cos)
}

/// Hachures parallèles d'angle `angle_deg` (anti-horaire depuis +X) espacées de `step_mm`,
/// découpées par le polygone : un segment par intervalle intérieur.
pub fn hatch_segments(
    polygon: &[PointMm],
    angle_deg: f64,
    step_mm: f64,
) -> Vec<(PointMm, PointMm)> {
    if !valid(polygon, step_mm) || !angle_deg.is_finite() {
        return Vec::new();
    }
    let (sin, cos) = angle_deg.to_radians().sin_cos();
    // Repère tourné de -angle : les hachures y sont horizontales.
    let local: Vec<PointMm> = polygon.iter().map(|p| rotate(*p, cos, -sin)).collect();
    let (lo, hi) = local
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p.y), hi.max(p.y))
        });
    let step = step_mm.max((hi - lo) / PATTERN_MAX_ITEMS as f64);
    let first = (lo / step - 0.5).floor() as i64;
    let last = (hi / step - 0.5).ceil() as i64;
    let mut out = Vec::new();
    for k in first..=last {
        let y = (k as f64 + 0.5) * step;
        if y < lo || y > hi {
            continue;
        }
        for (x0, x1) in spans(&local, y) {
            if x1 - x0 > 1e-9 {
                out.push((
                    rotate(PointMm::new(x0, y), cos, sin),
                    rotate(PointMm::new(x1, y), cos, sin),
                ));
            }
        }
    }
    out
}

/// Points d'une grille carrée de pas `step_mm` (nœuds à `(i + 0,5) × pas`) qui sont dans le
/// polygone (bord compris).
pub fn dot_grid(polygon: &[PointMm], step_mm: f64) -> Vec<PointMm> {
    if !valid(polygon, step_mm) {
        return Vec::new();
    }
    let (mut x0, mut x1, mut y0, mut y1) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for p in polygon {
        x0 = x0.min(p.x);
        x1 = x1.max(p.x);
        y0 = y0.min(p.y);
        y1 = y1.max(p.y);
    }
    let cells = ((x1 - x0) / step_mm + 1.0) * ((y1 - y0) / step_mm + 1.0);
    let step = if cells > PATTERN_MAX_ITEMS as f64 {
        step_mm * (cells / PATTERN_MAX_ITEMS as f64).sqrt()
    } else {
        step_mm
    };
    let (i0, i1) = (
        (x0 / step - 0.5).floor() as i64,
        (x1 / step - 0.5).ceil() as i64,
    );
    let (j0, j1) = (
        (y0 / step - 0.5).floor() as i64,
        (y1 / step - 0.5).ceil() as i64,
    );
    let mut out = Vec::new();
    for j in j0..=j1 {
        for i in i0..=i1 {
            let p = PointMm::new((i as f64 + 0.5) * step, (j as f64 + 0.5) * step);
            if point_in_polygon(p, polygon) {
                out.push(p);
            }
        }
    }
    out
}

/// Un point **à l'intérieur** du polygone, au milieu du plus long intervalle intérieur trouvé
/// sur neuf horizontales (étiquette de zone). `None` si le polygone est dégénéré.
pub fn interior_point(polygon: &[PointMm]) -> Option<PointMm> {
    if !valid(polygon, 1.0) {
        return None;
    }
    let (lo, hi) = polygon
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p.y), hi.max(p.y))
        });
    let mut best: Option<(f64, PointMm)> = None;
    for k in 1..=9 {
        let y = lo + (hi - lo) * f64::from(k) / 10.0;
        for (x0, x1) in spans(polygon, y) {
            let len = x1 - x0;
            if best.is_none_or(|(l, _)| len > l) {
                best = Some((len, PointMm::new((x0 + x1) / 2.0, y)));
            }
        }
    }
    best.map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square(side: f64) -> Vec<PointMm> {
        vec![
            PointMm::new(0.0, 0.0),
            PointMm::new(side, 0.0),
            PointMm::new(side, side),
            PointMm::new(0.0, side),
        ]
    }

    #[test]
    fn hachures_horizontales_d_un_carre_de_10() {
        // Pas 2 : lignes à y = 1, 3, 5, 7, 9, chacune de 10 mm.
        let h = hatch_segments(&square(10.0), 0.0, 2.0);
        assert_eq!(h.len(), 5);
        for (k, (a, b)) in h.iter().enumerate() {
            assert!((a.y - (2.0 * k as f64 + 1.0)).abs() < 1e-9);
            assert!((b.x - a.x - 10.0).abs() < 1e-9);
        }
    }

    #[test]
    fn hachures_a_45_degres_longueur_totale() {
        // Carré 10 x 10, lignes à 45° espacées de 0,5 : somme des longueurs ≈ aire / pas =
        // 100 / 0,5 = 200 (à la discrétisation près).
        let total: f64 = hatch_segments(&square(10.0), 45.0, 0.5)
            .iter()
            .map(|(a, b)| (b.x - a.x).hypot(b.y - a.y))
            .sum();
        assert!((total - 200.0).abs() < 1.0, "{total}");
    }

    #[test]
    fn hachures_d_un_u_ne_traversent_pas_l_encoche() {
        // U : [0 ; 30] × [0 ; 10] moins l'encoche [10 ; 20] × [5 ; 10]. Pas 2, lignes à y = 1, 3
        // (30 mm chacune, une seule portée) puis 5, 7, 9 (deux portées de 10 mm : 20 mm), soit
        // 8 segments et 30 + 30 + 20 + 20 + 20 = 120 mm au total.
        let u: Vec<PointMm> = [
            (0.0, 0.0),
            (30.0, 0.0),
            (30.0, 10.0),
            (20.0, 10.0),
            (20.0, 5.0),
            (10.0, 5.0),
            (10.0, 10.0),
            (0.0, 10.0),
        ]
        .iter()
        .map(|&(x, y)| PointMm::new(x, y))
        .collect();
        let h = hatch_segments(&u, 0.0, 2.0);
        assert_eq!(h.len(), 8);
        let total: f64 = h.iter().map(|(a, b)| (b.x - a.x).abs()).sum();
        assert!((total - 120.0).abs() < 1e-9, "{total}");
        for (a, b) in &h {
            let mid = PointMm::new((a.x + b.x) / 2.0, a.y);
            assert!(point_in_polygon(mid, &u));
        }
    }

    #[test]
    fn grille_de_points_d_un_carre_de_10() {
        assert_eq!(dot_grid(&square(10.0), 2.0).len(), 25);
    }

    #[test]
    fn point_interieur_d_un_l() {
        let l = vec![
            PointMm::new(0.0, 0.0),
            PointMm::new(10.0, 0.0),
            PointMm::new(10.0, 2.0),
            PointMm::new(2.0, 2.0),
            PointMm::new(2.0, 10.0),
            PointMm::new(0.0, 10.0),
        ];
        let p = interior_point(&l).unwrap();
        assert!(point_in_polygon(p, &l));
    }

    #[test]
    fn entrees_invalides_donnent_vide() {
        assert!(hatch_segments(&[], 0.0, 2.0).is_empty());
        assert!(hatch_segments(&square(10.0), 0.0, 0.0).is_empty());
        assert!(hatch_segments(&square(10.0), f64::NAN, 2.0).is_empty());
        assert!(dot_grid(&square(10.0), -1.0).is_empty());
        assert!(interior_point(&square(10.0)[..2]).is_none());
    }
}

//! Tests de la détection de région (US-04.1).

use super::*;
use crate::{polygon_signed_area_mm2, Circle, CubicBezier, Spline};

fn p(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn seg(a: PointMm, b: PointMm) -> Trace {
    Trace::Line(a, b)
}

/// Rectangle dessiné par quatre segments qui ne partagent que leurs coins.
fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<Trace> {
    vec![
        seg(p(x0, y0), p(x1, y0)),
        seg(p(x1, y0), p(x1, y1)),
        seg(p(x1, y1), p(x0, y1)),
        seg(p(x0, y1), p(x0, y0)),
    ]
}

fn circle(cx: f64, cy: f64, r: f64) -> Trace {
    Trace::Circle(Circle::new(p(cx, cy), r).unwrap())
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

/// Les arêtes se suivent bout à bout, le contour est fermé et anti-horaire.
fn check_region(r: &Region) {
    assert!(r.area_mm2 > 0.0 && r.perimeter_mm > 0.0);
    let n = r.edges.len();
    assert!(n >= 1);
    for i in 0..n {
        let (a, b) = (&r.edges[i], &r.edges[(i + 1) % n]);
        assert!(
            distance_mm(a.end, b.start) <= 1e-6,
            "arête {i} non raccordée"
        );
        let (g0, g1) = (
            a.geometry[0].start(),
            a.geometry[a.geometry.len() - 1].end(),
        );
        assert!(distance_mm(g0, a.start) <= 1e-6);
        assert!(distance_mm(g1, a.end) <= 1e-6);
    }
    assert!(
        polygon_signed_area_mm2(&r.outline) > 0.0,
        "contour non anti-horaire"
    );
}

#[test]
fn region_carre_simple() {
    let r = find_region(&rect(0.0, 0.0, 10.0, 10.0), p(5.0, 5.0)).unwrap();
    check_region(&r);
    assert!(near(r.area_mm2, 100.0, 1e-9));
    assert!(near(r.perimeter_mm, 40.0, 1e-9));
    assert_eq!(r.edges.len(), 4);
    assert!(r.edges.iter().all(|e| e.whole && !e.reversed));
}

#[test]
fn region_carre_trace_a_l_envers_reste_antihoraire() {
    let cw = vec![
        seg(p(0.0, 0.0), p(0.0, 10.0)),
        seg(p(0.0, 10.0), p(10.0, 10.0)),
        seg(p(10.0, 10.0), p(10.0, 0.0)),
        seg(p(10.0, 0.0), p(0.0, 0.0)),
    ];
    let r = find_region(&cw, p(3.0, 3.0)).unwrap();
    check_region(&r);
    assert!(near(r.area_mm2, 100.0, 1e-9));
    // Parcours anti-horaire : chaque segment (tracé horaire) est parcouru à l'envers.
    assert!(r.edges.iter().all(|e| e.reversed));
}

#[test]
fn region_deux_rectangles_qui_se_croisent_sans_point_commun() {
    let mut t = rect(0.0, 0.0, 10.0, 10.0);
    t.extend(rect(5.0, 5.0, 15.0, 15.0));
    // Zone commune : petit carré de 5 x 5.
    let common = find_region(&t, p(7.0, 7.0)).unwrap();
    check_region(&common);
    assert!(near(common.area_mm2, 25.0, 1e-9));
    assert!(near(common.perimeter_mm, 20.0, 1e-9));
    // Zones non communes : plus petite région contenant le clic (en L).
    for click in [p(2.0, 2.0), p(12.0, 12.0), p(2.0, 8.0)] {
        let l = find_region(&t, click).unwrap();
        check_region(&l);
        assert!(near(l.area_mm2, 75.0, 1e-9), "{click:?}");
        assert!(near(l.perimeter_mm, 40.0, 1e-9));
    }
    // Hors des deux rectangles : aucune région.
    assert_eq!(
        find_region(&t, p(14.0, 2.0)).unwrap_err(),
        RegionError::NoRegion
    );
}

#[test]
fn region_cercle_coupe_par_une_corde() {
    let r = 10.0_f64;
    let t = vec![circle(0.0, 0.0, r), seg(p(-8.0, 6.0), p(8.0, 6.0))];
    let alpha = (0.6_f64).acos();
    let small = r * r * (alpha - alpha.sin() * alpha.cos());
    let big = std::f64::consts::PI * r * r - small;
    let arc_small = 2.0 * alpha * r;
    let up = find_region(&t, p(0.0, 8.0)).unwrap();
    check_region(&up);
    assert!(near(up.area_mm2, small, 1e-9));
    assert!(near(up.perimeter_mm, 16.0 + arc_small, 1e-9));
    let down = find_region(&t, p(0.0, 0.0)).unwrap();
    check_region(&down);
    assert!(near(down.area_mm2, big, 1e-9));
    assert!(near(
        down.perimeter_mm,
        16.0 + 2.0 * std::f64::consts::PI * r - arc_small,
        1e-9
    ));
    // Le cercle est découpé : une arête du cercle (pas le cercle entier).
    assert!(down.edges.iter().any(|e| e.source == 0 && !e.whole));
}

#[test]
fn region_cercle_entier_decoupe_et_impasse_ignoree() {
    // Le diamètre dépasse le cercle des deux côtés : deux impasses à ignorer.
    let t = vec![circle(0.0, 0.0, 10.0), seg(p(-20.0, 0.0), p(20.0, 0.0))];
    let pi = std::f64::consts::PI;
    let up = find_region(&t, p(0.0, 5.0)).unwrap();
    check_region(&up);
    assert!(near(up.area_mm2, pi * 50.0, 1e-9));
    assert!(near(up.perimeter_mm, 20.0 + 10.0 * pi, 1e-9));
    let arc = up.edges.iter().find(|e| e.source == 0).unwrap();
    assert!(near(arc.t_lo, 0.0, 1e-12) && near(arc.t_hi, 0.5, 1e-12) && !arc.reversed);
    let chord = up.edges.iter().find(|e| e.source == 1).unwrap();
    assert!(!chord.whole, "seule la corde du cercle borde la région");
    let down = find_region(&t, p(0.0, -5.0)).unwrap();
    let arc = down.edges.iter().find(|e| e.source == 0).unwrap();
    assert!(near(arc.t_lo, 0.5, 1e-12) && near(arc.t_hi, 1.0, 1e-12));
    // En dehors du cercle, le diamètre qui dépasse ne délimite rien.
    assert_eq!(
        find_region(&t, p(15.0, 3.0)).unwrap_err(),
        RegionError::NoRegion
    );
}

#[test]
fn region_cercle_seul_est_une_arete_entiere() {
    let r = find_region(&[circle(3.0, -2.0, 7.0)], p(3.0, -2.0)).unwrap();
    check_region(&r);
    assert!(near(r.area_mm2, std::f64::consts::PI * 49.0, 1e-9));
    assert!(near(r.perimeter_mm, 2.0 * std::f64::consts::PI * 7.0, 1e-9));
    assert_eq!(r.edges.len(), 1);
    assert!(r.edges[0].whole);
}

#[test]
fn region_impasse_dans_la_region_ignoree() {
    let mut t = rect(0.0, 0.0, 10.0, 10.0);
    // Impasse qui part du milieu du bas vers l'intérieur, et une autre à l'extérieur.
    t.push(seg(p(5.0, 0.0), p(5.0, 6.0)));
    t.push(seg(p(10.0, 10.0), p(14.0, 14.0)));
    let r = find_region(&t, p(2.0, 2.0)).unwrap();
    check_region(&r);
    assert!(near(r.area_mm2, 100.0, 1e-9));
    assert_eq!(
        r.edges.len(),
        4,
        "les arêtes contiguës d'un même tracé sont fusionnées"
    );
    assert!(r.edges.iter().all(|e| e.whole));
}

#[test]
fn region_bezier_ferme_une_forme() {
    // Arche : aire sous la courbe exactement 60 (voir intégrale de 30 t (1-t) dx).
    let arch = CubicBezier::new(p(0.0, 0.0), p(0.0, 10.0), p(10.0, 10.0), p(10.0, 0.0));
    let t = vec![Trace::Cubic(arch), seg(p(10.0, 0.0), p(0.0, 0.0))];
    let r = find_region(&t, p(5.0, 3.0)).unwrap();
    check_region(&r);
    assert!(near(r.area_mm2, 60.0, 1e-9), "{}", r.area_mm2);
    let len = arch.length_mm(1e-9).unwrap();
    assert!(near(r.perimeter_mm, 10.0 + len, 1e-6));
    // Coupée par une verticale au milieu : par symétrie, 30 de chaque côté.
    let mut cut = t.clone();
    cut.push(seg(p(5.0, -1.0), p(5.0, 12.0)));
    let left = find_region(&cut, p(2.0, 2.0)).unwrap();
    check_region(&left);
    assert!(near(left.area_mm2, 30.0, 1e-8), "{}", left.area_mm2);
    let right = find_region(&cut, p(8.0, 2.0)).unwrap();
    assert!(near(right.area_mm2, 30.0, 1e-8));
}

#[test]
fn region_arcs_secteur_circulaire() {
    let arc = crate::Arc::from_center_angles(p(0.0, 0.0), 10.0, 0.0, 90.0, true).unwrap();
    let t = vec![
        Trace::Arc(arc),
        seg(p(0.0, 0.0), p(10.0, 0.0)),
        seg(p(0.0, 10.0), p(0.0, 0.0)),
    ];
    let r = find_region(&t, p(3.0, 3.0)).unwrap();
    check_region(&r);
    let pi = std::f64::consts::PI;
    assert!(near(r.area_mm2, pi * 25.0, 1e-9));
    assert!(near(r.perimeter_mm, 20.0 + 5.0 * pi, 1e-9));
    // Deux arcs de sens opposés forment une lentille.
    let a1 = crate::Arc::from_center_angles(
        p(0.0, -6.0),
        10.0,
        36.869_897_645_844_02,
        143.130_102_354_155_98,
        true,
    )
    .unwrap();
    let a2 = crate::Arc::from_center_angles(
        p(0.0, 6.0),
        10.0,
        216.869_897_645_844_02,
        323.130_102_354_155_98,
        true,
    )
    .unwrap();
    let lens = find_region(&[Trace::Arc(a1), Trace::Arc(a2)], p(0.0, 0.0)).unwrap();
    check_region(&lens);
    // Lentille de deux arcs de rayon 10, corde 16 : 2 (r² α − d √(r² − d²))
    let alpha = (0.6_f64).acos();
    let expected = 2.0 * (100.0 * alpha - 6.0 * 8.0);
    assert!(
        near(lens.area_mm2, expected, 1e-6),
        "{} {expected}",
        lens.area_mm2
    );
}

#[test]
fn region_courbe_lisse_fermee_par_un_segment() {
    let sp = Spline::through_points(&[p(0.0, 0.0), p(5.0, 6.0), p(10.0, 0.0)]).unwrap();
    let t = vec![Trace::Spline(sp.clone()), seg(p(10.0, 0.0), p(0.0, 0.0))];
    let r = find_region(&t, p(5.0, 2.0)).unwrap();
    check_region(&r);
    let poly = sp.sample(4000);
    let dense = polygon_signed_area_mm2(&poly).abs();
    assert!(near(r.area_mm2, dense, 1e-3), "{} {dense}", r.area_mm2);
    // Coupée par une verticale hors des points de passage : l'arête est une portion.
    let mut cut = t.clone();
    cut.push(seg(p(2.0, -1.0), p(2.0, 9.0)));
    let left = find_region(&cut, p(1.0, 0.5)).unwrap();
    check_region(&left);
    let spline_edge = left.edges.iter().find(|e| e.source == 0).unwrap();
    assert!(!spline_edge.whole && spline_edge.t_hi < 0.5);
    assert!(left.area_mm2 < r.area_mm2);
}

#[test]
fn region_clic_exactement_sur_un_trace_bord_compris() {
    let t = rect(0.0, 0.0, 10.0, 10.0);
    // Sur un côté, sur un coin : le bord appartient à la région.
    for click in [p(0.0, 5.0), p(10.0, 10.0), p(5.0, 0.0)] {
        let r = find_region(&t, click).unwrap();
        assert!(near(r.area_mm2, 100.0, 1e-9));
    }
    // Sur le côté commun de deux carrés accolés : le plus petit des deux, à égalité le premier.
    let mut two = rect(0.0, 0.0, 10.0, 10.0);
    two.extend(rect(10.0, 0.0, 20.0, 10.0));
    let r = find_region(&two, p(10.0, 5.0)).unwrap();
    assert!(near(r.area_mm2, 100.0, 1e-9));
}

#[test]
fn region_traces_superposes() {
    // Carré dessiné deux fois : le doublon est ignoré.
    let mut t = rect(0.0, 0.0, 10.0, 10.0);
    t.extend(rect(0.0, 0.0, 10.0, 10.0));
    let r = find_region(&t, p(5.0, 5.0)).unwrap();
    check_region(&r);
    assert!(near(r.area_mm2, 100.0, 1e-9));
    // Segments colinéaires qui se chevauchent sur une partie d'un côté.
    let mut over = rect(0.0, 0.0, 10.0, 10.0);
    over.push(seg(p(4.0, 0.0), p(14.0, 0.0)));
    let r = find_region(&over, p(5.0, 5.0)).unwrap();
    assert!(near(r.area_mm2, 100.0, 1e-9));
    // Deux cercles identiques : refus explicite, pas de résultat trompeur.
    let e = find_region(&[circle(0.0, 0.0, 5.0), circle(0.0, 0.0, 5.0)], p(0.0, 0.0)).unwrap_err();
    assert!(
        matches!(
            e,
            RegionError::Overlap {
                first: 0,
                second: 1
            }
        ),
        "{e:?}"
    );
    assert!(e.to_string().contains("se superposent"));
}

#[test]
fn region_ouverture_de_demi_millimetre() {
    // Côté gauche interrompu de 0,5 mm.
    let t = vec![
        seg(p(0.0, 0.0), p(10.0, 0.0)),
        seg(p(10.0, 0.0), p(10.0, 10.0)),
        seg(p(10.0, 10.0), p(0.0, 10.0)),
        seg(p(0.0, 10.0), p(0.0, 5.25)),
        seg(p(0.0, 4.75), p(0.0, 0.0)),
    ];
    let e = find_region(&t, p(5.0, 5.0)).unwrap_err();
    let RegionError::Open(gap) = e.clone() else {
        panic!("{e:?}");
    };
    assert!(near(gap.gap_mm, 0.5, 1e-9));
    let mut ys = [gap.from.y, gap.to.y];
    ys.sort_by(f64::total_cmp);
    assert!(near(ys[0], 4.75, 1e-9) && near(ys[1], 5.25, 1e-9));
    assert!(e.to_string().contains("0.500"), "{e}");
    // Extrémité qui n'atteint pas un autre tracé (sous-dépassement), 0,5 mm avant le côté droit.
    let t2 = vec![
        seg(p(0.0, 0.0), p(10.0, 0.0)),
        seg(p(10.0, -2.0), p(10.0, 12.0)),
        seg(p(0.0, 10.0), p(9.5, 10.0)),
        seg(p(0.0, 10.0), p(0.0, 0.0)),
    ];
    let RegionError::Open(gap) = find_region(&t2, p(5.0, 5.0)).unwrap_err() else {
        panic!();
    };
    assert!(near(gap.gap_mm, 0.5, 1e-9));
    assert!(distance_mm(gap.from, p(9.5, 10.0)) < 1e-9);
    assert!(distance_mm(gap.to, p(10.0, 10.0)) < 1e-6);
    // Un clic loin de l'ouverture, hors de ce que la fermerait : aucune région.
    assert_eq!(
        find_region(&t, p(50.0, 50.0)).unwrap_err(),
        RegionError::NoRegion
    );
}

#[test]
fn region_tolerance_de_raccord() {
    let build = |dy: f64| {
        vec![
            seg(p(0.0, 0.0), p(10.0, 0.0)),
            seg(p(10.0, 0.0), p(10.0, 10.0)),
            seg(p(10.0, 10.0), p(0.0, 10.0)),
            seg(p(0.0, 10.0), p(0.0, dy)),
        ]
    };
    // 5e-7 mm < 1e-6 mm : raccordé.
    let r = find_region(&build(5e-7), p(5.0, 5.0)).unwrap();
    assert!(near(r.area_mm2, 100.0, 1e-4));
    // 5e-6 mm > 1e-6 mm : ouverture signalée avec son écart.
    let RegionError::Open(g) = find_region(&build(5e-6), p(5.0, 5.0)).unwrap_err() else {
        panic!();
    };
    assert!(near(g.gap_mm, 5e-6, 1e-9));
}

#[test]
fn region_tangences_et_ilots() {
    let pi = std::f64::consts::PI;
    // Droite tangente à un cercle : le cercle reste une région entière.
    let t = vec![circle(0.0, 0.0, 5.0), seg(p(-10.0, 5.0), p(10.0, 5.0))];
    let r = find_region(&t, p(0.0, 0.0)).unwrap();
    assert!(near(r.area_mm2, pi * 25.0, 1e-9));
    assert!(r.edges.len() == 1 && r.edges[0].whole);
    // Cercles tangents intérieurement : le petit est une région ; le grand moins le petit
    // touche son bord en un point (refusé).
    let t = vec![circle(0.0, 0.0, 10.0), circle(5.0, 0.0, 5.0)];
    let small = find_region(&t, p(5.0, 0.0)).unwrap();
    assert!(near(small.area_mm2, pi * 25.0, 1e-9));
    assert_eq!(
        find_region(&t, p(-5.0, 0.0)).unwrap_err(),
        RegionError::SelfTouching
    );
    // Îlot disjoint : pas un trou, la région garde son bord extérieur.
    let mut t = rect(0.0, 0.0, 20.0, 20.0);
    t.push(circle(10.0, 10.0, 3.0));
    let outer = find_region(&t, p(2.0, 2.0)).unwrap();
    assert!(near(outer.area_mm2, 400.0, 1e-9));
    let island = find_region(&t, p(10.0, 10.0)).unwrap();
    assert!(near(island.area_mm2, pi * 9.0, 1e-9));
}

fn rotate(pt: PointMm, c: f64, s: f64, dx: f64, dy: f64) -> PointMm {
    p(pt.x * c - pt.y * s + dx, pt.x * s + pt.y * c + dy)
}

fn transformed(t: &[Trace], c: f64, s: f64, dx: f64, dy: f64) -> Vec<Trace> {
    t.iter()
        .map(|x| match x {
            Trace::Line(a, b) => seg(rotate(*a, c, s, dx, dy), rotate(*b, c, s, dx, dy)),
            Trace::Circle(k) => {
                Trace::Circle(Circle::new(rotate(k.center, c, s, dx, dy), k.radius_mm).unwrap())
            }
            _ => unreachable!(),
        })
        .collect()
}

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
}

#[test]
fn region_invariante_par_rotation_et_translation() {
    let mut base = rect(0.0, 0.0, 30.0, 20.0);
    base.extend(rect(15.0, 10.0, 45.0, 35.0));
    base.push(circle(22.0, 14.0, 6.0));
    let click = p(20.0, 12.0);
    let reference = find_region(&base, click).unwrap();
    let mut rng = Lcg(42);
    for _ in 0..40 {
        let a = rng.next() * std::f64::consts::TAU;
        let (s, c) = a.sin_cos();
        let (dx, dy) = ((rng.next() - 0.5) * 2000.0, (rng.next() - 0.5) * 2000.0);
        let moved = transformed(&base, c, s, dx, dy);
        let r = find_region(&moved, rotate(click, c, s, dx, dy)).unwrap();
        check_region(&r);
        assert!(
            near(r.area_mm2, reference.area_mm2, 1e-7),
            "{} {}",
            r.area_mm2,
            reference.area_mm2
        );
        assert!(near(r.perimeter_mm, reference.perimeter_mm, 1e-7));
        assert_eq!(r.edges.len(), reference.edges.len());
    }
}

#[test]
fn region_entrees_degenerees_sans_panique() {
    let sq = rect(0.0, 0.0, 10.0, 10.0);
    assert_eq!(
        find_region(&[], p(0.0, 0.0)).unwrap_err(),
        RegionError::NoRegion
    );
    for bad in [
        p(f64::NAN, 1.0),
        p(1.0, f64::INFINITY),
        p(1e300, 0.0),
        p(0.0, -1e300),
    ] {
        assert_eq!(
            find_region(&sq, bad).unwrap_err(),
            RegionError::InvalidClick
        );
    }
    let degenerate: Vec<Vec<Trace>> = vec![
        vec![seg(p(1.0, 1.0), p(1.0, 1.0))],
        vec![seg(p(f64::NAN, 0.0), p(1.0, 1.0))],
        vec![seg(p(0.0, 0.0), p(1e300, 1e300))],
        vec![Trace::Cubic(CubicBezier::new(
            p(1.0, 1.0),
            p(1.0, 1.0),
            p(1.0, 1.0),
            p(1.0, 1.0),
        ))],
        vec![Trace::Circle(Circle {
            center: p(0.0, 0.0),
            radius_mm: 0.0,
        })],
        vec![Trace::Circle(Circle {
            center: p(0.0, 0.0),
            radius_mm: f64::NAN,
        })],
        vec![Trace::Arc(crate::Arc {
            center: p(0.0, 0.0),
            radius_mm: 5.0,
            start_rad: 0.0,
            sweep_rad: 0.0,
        })],
    ];
    for t in degenerate {
        let e = find_region(&t, p(0.5, 0.5)).unwrap_err();
        assert!(
            matches!(e, RegionError::InvalidTrace { index: 0, .. }),
            "{e:?}"
        );
    }
    // Un tracé dégénéré parmi d'autres tracés valides est signalé, pas ignoré.
    let mut mixed = sq.clone();
    mixed.push(seg(p(2.0, 2.0), p(2.0, 2.0)));
    assert!(matches!(
        find_region(&mixed, p(5.0, 5.0)).unwrap_err(),
        RegionError::InvalidTrace { index: 4, .. }
    ));
}

#[test]
fn region_campagne_aleatoire_invariants() {
    let mut rng = Lcg(2026);
    let (mut found, mut open, mut none) = (0, 0, 0);
    for _ in 0..300 {
        let mut t: Vec<Trace> = Vec::new();
        for _ in 0..(3 + (rng.next() * 6.0) as usize) {
            let (a, b) = (
                p(rng.next() * 100.0, rng.next() * 100.0),
                p(rng.next() * 100.0, rng.next() * 100.0),
            );
            if distance_mm(a, b) > 1.0 {
                t.push(seg(a, b));
            }
        }
        if rng.next() < 0.5 {
            t.push(circle(
                rng.next() * 100.0,
                rng.next() * 100.0,
                5.0 + rng.next() * 30.0,
            ));
        }
        let click = p(rng.next() * 100.0, rng.next() * 100.0);
        match find_region(&t, click) {
            Ok(r) => {
                found += 1;
                check_region(&r);
                assert!(contains_inclusive(&r.outline, click));
                let dense = polygon_signed_area_mm2(&r.outline);
                assert!(
                    near(dense, r.area_mm2, 1e-2 * r.area_mm2.max(1.0)),
                    "{dense} {}",
                    r.area_mm2
                );
            }
            Err(RegionError::Open(g)) => {
                open += 1;
                assert!(g.gap_mm > 0.0);
            }
            Err(_) => none += 1,
        }
    }
    assert!(
        found > 10,
        "trouvées {found}, ouvertures {open}, aucune {none}"
    );
}

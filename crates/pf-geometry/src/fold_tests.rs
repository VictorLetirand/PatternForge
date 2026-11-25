use super::*;
use crate::split::{chain_length_mm, chain_signed_area_mm2};
use std::f64::consts::PI;

fn p(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn line(a: (f64, f64), b: (f64, f64)) -> EdgeGeom {
    EdgeGeom::Line(p(a.0, a.1), p(b.0, b.1))
}

/// Rectangle anti-horaire 10 × 5 ; l'arête 3 (de (0, 5) à (0, 0)) est la pliure.
fn half_rectangle() -> Vec<EdgeGeom> {
    vec![
        line((0.0, 0.0), (10.0, 0.0)),
        line((10.0, 0.0), (10.0, 5.0)),
        line((10.0, 5.0), (0.0, 5.0)),
        line((0.0, 5.0), (0.0, 0.0)),
    ]
}

fn assert_closed(chain: &[EdgeGeom]) {
    for j in 0..chain.len() {
        let gap = distance_mm(chain[j].end(), chain[(j + 1) % chain.len()].start());
        assert!(gap < 1e-9, "arête {j} : écart {gap}");
    }
}

#[test]
fn rectangle_calcule_a_la_main() {
    let u = unfold_chain(&half_rectangle(), 3).unwrap();
    // Moitié : (0,0) (10,0) (10,5) (0,5) ; image : (−10,5) (−10,0) ; retour en (0,0).
    let expected = [
        ((0.0, 0.0), (10.0, 0.0)),
        ((10.0, 0.0), (10.0, 5.0)),
        ((10.0, 5.0), (0.0, 5.0)),
        ((0.0, 5.0), (-10.0, 5.0)),
        ((-10.0, 5.0), (-10.0, 0.0)),
        ((-10.0, 0.0), (0.0, 0.0)),
    ];
    assert_eq!(u.chain.len(), expected.len());
    for (e, (a, b)) in u.chain.iter().zip(expected) {
        assert_eq!(e.start(), p(a.0, a.1));
        assert_eq!(e.end(), p(b.0, b.1));
    }
    assert_eq!(u.fold_start, p(0.0, 5.0));
    assert_eq!(u.fold_end, p(0.0, 0.0));
    assert!((u.fold_length_mm - 5.0).abs() < 1e-12);
    assert_eq!(u.fold_edges, 1);
    // Aire 2 × 50, périmètre 2 × (30 − 5), sens anti-horaire conservé.
    assert!((chain_signed_area_mm2(&u.chain) - 100.0).abs() < 1e-12);
    assert!((chain_length_mm(&u.chain) - 50.0).abs() < 1e-12);
    assert_closed(&u.chain);
}

#[test]
fn pliure_oblique_reflexion_exacte() {
    // Axe à 45° : la moitié est le triangle (0,0) (10,0) (10,10) sous la diagonale y = x ;
    // l'image de (10,0) est (0,10).
    let tri = vec![
        line((0.0, 0.0), (10.0, 0.0)),
        line((10.0, 0.0), (10.0, 10.0)),
        line((10.0, 10.0), (0.0, 0.0)),
    ];
    let u = unfold_chain(&tri, 2).unwrap();
    assert_eq!(u.chain.len(), 4);
    let pts: Vec<PointMm> = u.chain.iter().map(EdgeGeom::start).collect();
    for (got, want) in pts
        .iter()
        .zip([p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0), p(0.0, 10.0)])
    {
        assert!(distance_mm(*got, want) < 1e-12, "{got:?} ≠ {want:?}");
    }
    assert!((chain_signed_area_mm2(&u.chain) - 100.0).abs() < 1e-9);
}

#[test]
fn bezier_reflechie_point_par_point() {
    let c = CubicBezier::new(p(20.0, 0.0), p(30.0, 2.0), p(30.0, 8.0), p(20.0, 10.0));
    let half = vec![
        line((0.0, 0.0), (20.0, 0.0)),
        EdgeGeom::Cubic(c),
        line((20.0, 10.0), (0.0, 10.0)),
        line((0.0, 10.0), (0.0, 0.0)),
    ];
    let u = unfold_chain(&half, 3).unwrap();
    assert_eq!(u.chain.len(), 6);
    // Image de la Bézier : x → −x, parcourue en sens inverse.
    let EdgeGeom::Cubic(m) = u.chain[4] else {
        panic!("la Bézier réfléchie doit être la cinquième arête");
    };
    assert_eq!(m.p0, p(-20.0, 10.0));
    assert_eq!(m.c1, p(-30.0, 8.0));
    assert_eq!(m.c2, p(-30.0, 2.0));
    assert_eq!(m.p3, p(-20.0, 0.0));
    let half_area = chain_signed_area_mm2(&half);
    let area = chain_signed_area_mm2(&u.chain);
    assert!((area - 2.0 * half_area).abs() <= 1e-9 * 2.0 * half_area);
    assert_closed(&u.chain);
}

#[test]
fn demi_disque_donne_un_disque() {
    let arc = Arc {
        center: p(0.0, 0.0),
        radius_mm: 10.0,
        start_rad: -PI / 2.0,
        sweep_rad: PI,
    };
    let half = vec![EdgeGeom::Arc(arc), line((0.0, 10.0), (0.0, -10.0))];
    let u = unfold_chain(&half, 1).unwrap();
    assert_eq!(u.chain.len(), 2);
    let EdgeGeom::Arc(m) = u.chain[1] else {
        panic!("arc attendu");
    };
    assert!(distance_mm(m.center, p(0.0, 0.0)) < 1e-12);
    assert!((m.sweep_rad - PI).abs() < 1e-12);
    assert!(distance_mm(m.start_point(), p(0.0, 10.0)) < 1e-9);
    assert!(distance_mm(m.end_point(), p(0.0, -10.0)) < 1e-9);
    assert!((chain_signed_area_mm2(&u.chain) - PI * 100.0).abs() < 1e-9);
    assert!((chain_length_mm(&u.chain) - 2.0 * PI * 10.0).abs() < 1e-9);
}

#[test]
fn pliure_en_plusieurs_segments_alignes() {
    let chain = vec![
        line((0.0, 0.0), (10.0, 0.0)),
        line((10.0, 0.0), (10.0, 5.0)),
        line((10.0, 5.0), (0.0, 5.0)),
        line((0.0, 5.0), (0.0, 2.0)),
        line((0.0, 2.0), (0.0, 0.0)),
    ];
    // Quelle que soit l'arête désignée, la pliure est le segment entier (5 mm).
    for fold in [3, 4] {
        let u = unfold_chain(&chain, fold).unwrap();
        assert_eq!(u.fold_edges, 2);
        assert!((u.fold_length_mm - 5.0).abs() < 1e-12);
        assert_eq!(u.chain.len(), 6);
        assert!((chain_signed_area_mm2(&u.chain) - 100.0).abs() < 1e-12);
    }
}

#[test]
fn refus_pliure_invalide() {
    let rect = half_rectangle();
    assert_eq!(unfold_chain(&rect, 9), Err(FoldError::NoSuchEdge));
    let curved = vec![
        EdgeGeom::Cubic(CubicBezier::new(
            p(0.0, 0.0),
            p(3.0, 0.0),
            p(7.0, 0.0),
            p(10.0, 0.0),
        )),
        line((10.0, 0.0), (0.0, 5.0)),
        line((0.0, 5.0), (0.0, 0.0)),
    ];
    assert_eq!(unfold_chain(&curved, 0), Err(FoldError::NotStraight));
    let short = vec![
        line((0.0, 0.0), (10.0, 0.0)),
        line((10.0, 0.0), (0.0, 0.0005)),
        line((0.0, 0.0005), (0.0, 0.0)),
    ];
    assert!(matches!(
        unfold_chain(&short, 2),
        Err(FoldError::TooShort { .. })
    ));
    let flat = vec![line((0.0, 0.0), (10.0, 0.0)), line((10.0, 0.0), (0.0, 0.0))];
    // Les deux arêtes sont sur la droite mais en sens contraires : la seconde n'est
    // pas un prolongement de la pliure, elle touche la droite sans pouvoir être la moitié.
    assert!(unfold_chain(&flat, 0).is_err());
}

/// Rectangle 10 × 8 dont le bord droit s'enfonce jusqu'à `x = dent` au niveau y = 4 ;
/// la pliure est l'arête 6, de (0, 8) à (0, 0).
fn dented(dent: f64) -> Vec<EdgeGeom> {
    vec![
        line((0.0, 0.0), (10.0, 0.0)),
        line((10.0, 0.0), (10.0, 3.0)),
        line((10.0, 3.0), (dent, 4.0)),
        line((dent, 4.0), (10.0, 5.0)),
        line((10.0, 5.0), (10.0, 8.0)),
        line((10.0, 8.0), (0.0, 8.0)),
        line((0.0, 8.0), (0.0, 0.0)),
    ]
}

#[test]
fn refus_contour_qui_traverse() {
    assert!(unfold_chain(&dented(1.0), 6).is_ok());
    assert!(unfold_chain(&dented(2e-6), 6).is_ok());
    // De l'autre côté de 1e-5 mm : refusé, et le tracé fautif est désigné.
    assert_eq!(
        unfold_chain(&dented(-1e-5), 6),
        Err(FoldError::Crosses { edge: 2 })
    );
    // Dans la tolérance (±5e-7 mm) : le sommet touche la droite, ce qui est refusé aussi.
    for dent in [5e-7, -5e-7, 0.0] {
        assert!(
            matches!(
                unfold_chain(&dented(dent), 6),
                Err(FoldError::Touches { edge: 2 })
            ),
            "dent {dent}"
        );
    }
}

#[test]
fn refus_bezier_qui_traverse_sans_que_les_extremites_le_montrent() {
    // Extrémités du bon côté, mais le milieu de la courbe passe à gauche de x = 0.
    let c = CubicBezier::new(p(10.0, 2.0), p(-6.0, 3.0), p(-6.0, 4.0), p(10.0, 5.0));
    let chain = vec![
        line((0.0, 0.0), (10.0, 0.0)),
        line((10.0, 0.0), (10.0, 2.0)),
        EdgeGeom::Cubic(c),
        line((10.0, 5.0), (10.0, 8.0)),
        line((10.0, 8.0), (0.0, 8.0)),
        line((0.0, 8.0), (0.0, 0.0)),
    ];
    assert_eq!(unfold_chain(&chain, 5), Err(FoldError::Crosses { edge: 2 }));
    // Minimum à x = 0,25 : la courbe reste du bon côté.
    let ok = CubicBezier::new(p(10.0, 2.0), p(-3.0, 3.0), p(-3.0, 4.0), p(10.0, 5.0));
    let mut chain = chain;
    chain[2] = EdgeGeom::Cubic(ok);
    assert!(unfold_chain(&chain, 5).is_ok());
}

#[test]
fn refus_contact_ailleurs_que_les_extremites() {
    // Sommet posé sur la droite : le résultat se pincerait.
    let vertex = vec![
        line((0.0, 0.0), (10.0, 0.0)),
        line((10.0, 0.0), (10.0, 3.0)),
        line((10.0, 3.0), (0.0, 3.0)),
        line((0.0, 3.0), (10.0, 5.0)),
        line((10.0, 5.0), (0.0, 8.0)),
        line((0.0, 8.0), (0.0, 0.0)),
    ];
    // Le sommet (0, 3) est sur la droite : la pliure est (0,8)→(0,0), plus longue que la moitié.
    assert!(matches!(
        unfold_chain(&vertex, 5),
        Err(FoldError::Touches { edge: 2 }) | Err(FoldError::Touches { edge: 3 })
    ));
    // Segment aligné séparé de la pliure : deux morceaux.
    let two = vec![
        line((0.0, 0.0), (10.0, 0.0)),
        line((10.0, 0.0), (10.0, 10.0)),
        line((10.0, 10.0), (0.0, 10.0)),
        line((0.0, 10.0), (0.0, 8.0)),
        line((0.0, 8.0), (5.0, 6.0)),
        line((5.0, 6.0), (0.0, 4.0)),
        line((0.0, 4.0), (0.0, 0.0)),
    ];
    assert!(matches!(
        unfold_chain(&two, 6),
        Err(FoldError::Touches { .. })
    ));
}

#[test]
fn etendue_d_une_arete_calculee_a_la_main() {
    // Axe x = 0 orienté vers le bas : la gauche de (0,10)→(0,0) est x > 0.
    let axis = FoldAxis::through(p(0.0, 10.0), p(0.0, 0.0)).unwrap();
    assert!((axis.signed_distance(p(7.0, 3.0)) - 7.0).abs() < 1e-12);
    // Bézier d'extrémités à x = 4 et x = 6, poignées à x = 1 : elle plonge sous 4 avant de
    // remonter, son minimum est strictement intérieur (comparé à un échantillonnage dense).
    let c = CubicBezier::new(p(4.0, 0.0), p(1.0, 3.0), p(1.0, 6.0), p(6.0, 9.0));
    let r = edge_side_range(&EdgeGeom::Cubic(c), &axis);
    assert_eq!(r.start, 4.0);
    assert_eq!(r.end, 6.0);
    let min = r.interior_extremum_min.unwrap();
    // Valeur au point critique : vérifiée par échantillonnage dense.
    let sampled = (1..1000)
        .map(|k| c.point_at(k as f64 / 1000.0).x)
        .fold(f64::INFINITY, f64::min);
    assert!(
        min <= sampled + 1e-9 && min > sampled - 1e-4,
        "{min} vs {sampled}"
    );
    // Demi-cercle de centre (5,5), rayon 2, du côté x ≥ 5 : son minimum (x = 3) est hors de l'arc.
    let arc = Arc {
        center: p(5.0, 5.0),
        radius_mm: 2.0,
        start_rad: -PI / 2.0,
        sweep_rad: PI,
    };
    let ra = edge_side_range(&EdgeGeom::Arc(arc), &axis);
    assert!(
        ra.interior_extremum_min.is_none(),
        "le minimum est hors de l'arc"
    );
    let full = Arc {
        center: p(5.0, 5.0),
        radius_mm: 2.0,
        start_rad: 0.0,
        sweep_rad: 1.5 * PI,
    };
    let rf = edge_side_range(&EdgeGeom::Arc(full), &axis);
    assert!((rf.interior_extremum_min.unwrap() - 3.0).abs() < 1e-12);
}

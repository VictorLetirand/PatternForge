//! Tests de `pleat.rs` : toutes les valeurs attendues sont calculées à la main.
//!
//! Rectangle R = (0;0) (200;0) (200;100) (0;100), anti-horaire, aire 20 000, périmètre 600.
//! Ligne x = 100 orientée vers le haut ; à droite de la ligne = côté +x.

use super::*;
use crate::split::chain_signed_area_mm2;

fn p(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn line_chain(pts: &[(f64, f64)]) -> Vec<EdgeGeom> {
    (0..pts.len())
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            EdgeGeom::Line(p(a.0, a.1), p(b.0, b.1))
        })
        .collect()
}

fn rect() -> Vec<EdgeGeom> {
    line_chain(&[(0.0, 0.0), (200.0, 0.0), (200.0, 100.0), (0.0, 100.0)])
}

fn vertices(edges: &[EdgeGeom]) -> Vec<(f64, f64)> {
    edges.iter().map(|e| (e.start().x, e.start().y)).collect()
}

fn has_vertex(edges: &[EdgeGeom], x: f64, y: f64) -> bool {
    edges
        .iter()
        .any(|e| (e.start().x - x).abs() < 1e-9 && (e.start().y - y).abs() < 1e-9)
}

fn geoms(s: &Spread) -> Vec<EdgeGeom> {
    s.edges.iter().map(|e| e.geom).collect()
}

fn closed(edges: &[EdgeGeom]) -> bool {
    (0..edges.len())
        .all(|i| crate::distance_mm(edges[i].end(), edges[(i + 1) % edges.len()].start()) < 1e-9)
}

#[test]
fn ecarter_un_rectangle_de_20_donne_220_de_large() {
    let s = spread_chain(&rect(), p(100.0, 0.0), p(100.0, 100.0), Side::Right, 20.0).unwrap();
    let g = geoms(&s);
    assert!(closed(&g));
    // Huit sommets calculés à la main.
    for (x, y) in [
        (0.0, 0.0),
        (100.0, 0.0),
        (120.0, 0.0),
        (220.0, 0.0),
        (220.0, 100.0),
        (120.0, 100.0),
        (100.0, 100.0),
        (0.0, 100.0),
    ] {
        assert!(
            has_vertex(&g, x, y),
            "sommet ({x} ; {y}) absent : {:?}",
            vertices(&g)
        );
    }
    assert_eq!(g.len(), 8);
    assert!((chain_signed_area_mm2(&g) - 22_000.0).abs() < 1e-9);
    assert!((chain_length_sum(&g) - 640.0).abs() < 1e-9);
    assert!((s.chord_mm - 100.0).abs() < 1e-9);
}

fn chain_length_sum(g: &[EdgeGeom]) -> f64 {
    g.iter().map(EdgeGeom::length_mm).sum()
}

#[test]
fn ecarter_du_cote_gauche_d_une_ligne_horizontale() {
    // Ligne y = 50 orientée vers +x : la gauche est le côté +y ; la moitié haute monte de 10.
    let s = spread_chain(&rect(), p(0.0, 50.0), p(200.0, 50.0), Side::Left, 10.0).unwrap();
    let g = geoms(&s);
    assert!(closed(&g));
    for (x, y) in [
        (200.0, 50.0),
        (200.0, 60.0),
        (200.0, 110.0),
        (0.0, 110.0),
        (0.0, 60.0),
        (0.0, 50.0),
    ] {
        assert!(has_vertex(&g, x, y), "sommet ({x} ; {y}) absent");
    }
    assert!((chain_signed_area_mm2(&g) - 22_000.0).abs() < 1e-9);
}

#[test]
fn l_aire_gagnee_est_largeur_fois_corde_sur_un_trapeze() {
    // Trapèze (0;0) (200;0) (150;100) (50;100) : aire = (200 + 100) / 2 × 100 = 15 000.
    // Corde de x = 100 : 100 mm. Largeur 20 : aire finale 15 000 + 20 × 100 = 17 000.
    let trap = line_chain(&[(0.0, 0.0), (200.0, 0.0), (150.0, 100.0), (50.0, 100.0)]);
    let s = spread_chain(&trap, p(100.0, 0.0), p(100.0, 100.0), Side::Right, 20.0).unwrap();
    let g = geoms(&s);
    assert!((chain_signed_area_mm2(&g) - 17_000.0).abs() < 1e-9);
    assert!((s.chord_mm - 100.0).abs() < 1e-9);
    assert!(has_vertex(&g, 220.0, 0.0) && has_vertex(&g, 170.0, 100.0));
}

#[test]
fn replier_rend_le_rectangle_d_origine() {
    let s = spread_chain(&rect(), p(100.0, 0.0), p(100.0, 100.0), Side::Right, 20.0).unwrap();
    let back = fold_chain(
        &geoms(&s),
        p(100.0, 0.0),
        p(100.0, 100.0),
        Side::Right,
        20.0,
    )
    .unwrap();
    assert!(closed(&back));
    assert!((chain_signed_area_mm2(&back) - 20_000.0).abs() < 1e-6);
    let merged = merge_collinear(back.into_iter().map(|e| (e, 0u8)).collect());
    let m: Vec<EdgeGeom> = merged.into_iter().map(|(e, _)| e).collect();
    assert_eq!(m.len(), 4);
    for (x, y) in [(0.0, 0.0), (200.0, 0.0), (200.0, 100.0), (0.0, 100.0)] {
        assert!(
            has_vertex(&m, x, y),
            "sommet ({x} ; {y}) absent : {:?}",
            vertices(&m)
        );
    }
}

#[test]
fn replier_rend_le_trapeze_d_origine() {
    let trap = line_chain(&[(0.0, 0.0), (200.0, 0.0), (150.0, 100.0), (50.0, 100.0)]);
    let s = spread_chain(&trap, p(100.0, 0.0), p(100.0, 100.0), Side::Right, 20.0).unwrap();
    let back = fold_chain(
        &geoms(&s),
        p(100.0, 0.0),
        p(100.0, 100.0),
        Side::Right,
        20.0,
    )
    .unwrap();
    let merged: Vec<EdgeGeom> = merge_collinear(back.into_iter().map(|e| (e, 0u8)).collect())
        .into_iter()
        .map(|(e, _)| e)
        .collect();
    assert_eq!(merged.len(), 4);
    for (x, y) in [(0.0, 0.0), (200.0, 0.0), (150.0, 100.0), (50.0, 100.0)] {
        assert!(has_vertex(&merged, x, y), "sommet ({x} ; {y}) absent");
    }
}

#[test]
fn replier_une_ligne_oblique_rend_la_forme_d_origine() {
    // Ligne à 45° : a = (50 ; 0), b = (150 ; 100) ; le côté droit est en bas à droite.
    let s = spread_chain(&rect(), p(50.0, 0.0), p(150.0, 100.0), Side::Right, 10.0).unwrap();
    let g = geoms(&s);
    // Corde de 50 à 150 en x, de 0 à 100 en y : 100·√2 ; aire = 20 000 + 10 × 100√2.
    let chord = 100.0 * 2.0_f64.sqrt();
    assert!((s.chord_mm - chord).abs() < 1e-9);
    assert!((chain_signed_area_mm2(&g) - (20_000.0 + 10.0 * chord)).abs() < 1e-9);
    let back = fold_chain(&g, p(50.0, 0.0), p(150.0, 100.0), Side::Right, 10.0).unwrap();
    assert!((chain_signed_area_mm2(&back) - 20_000.0).abs() < 1e-6);
    let merged: Vec<EdgeGeom> = merge_collinear(back.into_iter().map(|e| (e, 0u8)).collect())
        .into_iter()
        .map(|(e, _)| e)
        .collect();
    for (x, y) in [(0.0, 0.0), (200.0, 0.0), (200.0, 100.0), (0.0, 100.0)] {
        assert!(has_vertex(&merged, x, y), "sommet ({x} ; {y}) absent");
    }
}

#[test]
fn refus_ligne_hors_du_contour_largeur_invalide_points_confondus() {
    let r = rect();
    assert!(matches!(
        spread_chain(&r, p(300.0, 0.0), p(300.0, 100.0), Side::Right, 20.0),
        Err(PleatError::Cut(CutError::NoCrossing))
    ));
    for w in [0.0, -5.0, f64::NAN, f64::INFINITY, 0.05, 3000.0] {
        assert!(matches!(
            spread_chain(&r, p(100.0, 0.0), p(100.0, 100.0), Side::Right, w),
            Err(PleatError::BadWidth(_))
        ));
    }
    assert_eq!(
        spread_chain(&r, p(100.0, 0.0), p(100.0, 0.0), Side::Right, 20.0),
        Err(PleatError::BadLine)
    );
    // Ligne le long d'un bord.
    assert!(spread_chain(&r, p(0.0, 0.0), p(200.0, 0.0), Side::Left, 20.0).is_err());
}

#[test]
fn replier_refuse_un_contour_qui_ne_correspond_pas() {
    // Bord du bas oblique (0;0) → (200;20) : la corde en x = 100 est à y = 10, celle de x = 120
    // à y = 12 ; une fois la bande retirée, les deux morceaux sont à 2 mm l'un de l'autre.
    let oblique = line_chain(&[(0.0, 0.0), (200.0, 20.0), (200.0, 100.0), (0.0, 100.0)]);
    let err = fold_chain(&oblique, p(100.0, 0.0), p(100.0, 100.0), Side::Right, 20.0).unwrap_err();
    assert!(matches!(err, PleatError::Mismatch(_)), "{err}");
}

#[test]
fn fusion_des_segments_alignes_garde_les_etiquettes_differentes() {
    let edges = vec![
        (EdgeGeom::Line(p(0.0, 0.0), p(1.0, 0.0)), 1u8),
        (EdgeGeom::Line(p(1.0, 0.0), p(2.0, 0.0)), 1),
        (EdgeGeom::Line(p(2.0, 0.0), p(2.0, 1.0)), 1),
        (EdgeGeom::Line(p(2.0, 1.0), p(0.0, 1.0)), 2),
        (EdgeGeom::Line(p(0.0, 1.0), p(0.0, 0.0)), 1),
    ];
    let m = merge_collinear(edges);
    assert_eq!(m.len(), 4);
    assert_eq!(m[0].0, EdgeGeom::Line(p(0.0, 0.0), p(2.0, 0.0)));
}

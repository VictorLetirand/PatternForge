//! Tests de `flare.rs` : toutes les valeurs attendues sont calculées à la main.
//!
//! Rectangle R = (0;0) (200;0) (200;100) (0;100), anti-horaire, aire 20 000, périmètre 600. Bord
//! fixe : le bas, de (0;0) vers (200;0), `dir` = (1;0). Ouverture de 30 mm sur une ligne de
//! longueur L = 100 : sin(θ/2) = 30 / 200 = 0,15 ; cos θ = 1 − 2 × 0,15² = 0,955 ;
//! sin θ = 2 × 0,15 × √(1 − 0,0225) = 0,3 × √0,9775.

use super::*;
use crate::split::{chain_length_mm, chain_signed_area_mm2};
use crate::CubicBezier;

fn p(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn d(x: f64, y: f64) -> VectorMm {
    VectorMm::new(x, y)
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

fn geoms(f: &Fan) -> Vec<EdgeGeom> {
    f.edges.iter().map(|e| e.geom).collect()
}

fn has_vertex(edges: &[EdgeGeom], x: f64, y: f64) -> bool {
    edges
        .iter()
        .any(|e| (e.start().x - x).abs() < 1e-9 && (e.start().y - y).abs() < 1e-9)
}

fn closed(edges: &[EdgeGeom]) -> bool {
    (0..edges.len())
        .all(|i| distance_mm(edges[i].end(), edges[(i + 1) % edges.len()].start()) < 1e-9)
}

const C: f64 = 0.955;

fn s() -> f64 {
    0.3 * 0.9775_f64.sqrt()
}

/// Rotation horaire de `a` radians (indépendante du code testé : formule écrite ici).
fn cw(q: (f64, f64), c: (f64, f64), cos: f64, sin: f64) -> (f64, f64) {
    let (dx, dy) = (q.0 - c.0, q.1 - c.1);
    (c.0 + dx * cos + dy * sin, c.1 - dx * sin + dy * cos)
}

#[test]
fn ouvrir_un_rectangle_de_30_a_x_100_donne_sept_sommets_calcules() {
    let f = fan_chain(&rect(), p(100.0, 0.0), d(1.0, 0.0), 30.0).unwrap();
    let g = geoms(&f);
    assert!(closed(&g));
    assert_eq!(g.len(), 7);
    let r_200_0 = cw((200.0, 0.0), (100.0, 0.0), C, s());
    let r_200_100 = cw((200.0, 100.0), (100.0, 0.0), C, s());
    let r_100_100 = cw((100.0, 100.0), (100.0, 0.0), C, s());
    // (195,5 ; -29,6604), (225,1604 ; 65,8396), (129,6604 ; 95,5), calculés à la main plus haut.
    assert!((r_200_0.0 - 195.5).abs() < 1e-12 && (r_200_0.1 + 100.0 * s()).abs() < 1e-12);
    assert!((r_200_100.0 - (100.0 + 95.5 + 100.0 * s())).abs() < 1e-12);
    assert!((r_100_100.1 - 95.5).abs() < 1e-12);
    for (x, y) in [
        (100.0, 100.0),
        (0.0, 100.0),
        (0.0, 0.0),
        (100.0, 0.0),
        r_200_0,
        r_200_100,
        r_100_100,
    ] {
        assert!(has_vertex(&g, x, y), "sommet ({x} ; {y}) absent");
    }
    // Longueur de ligne, pivot, extrémités.
    assert!((f.length_mm - 100.0).abs() < 1e-9);
    assert!(distance_mm(f.hinge, p(100.0, 0.0)) < 1e-9);
    assert!(distance_mm(f.stay_end, p(100.0, 100.0)) < 1e-9);
    assert!(distance_mm(f.moved_end, p(r_100_100.0, r_100_100.1)) < 1e-9);
    assert!((distance_mm(f.stay_end, f.moved_end) - 30.0).abs() < 1e-9);
    assert!((f.angle_rad.sin() - s()).abs() < 1e-12);
}

#[test]
fn le_perimetre_gagne_l_ouverture_et_l_aire_un_demi_l2_sin_theta() {
    let f = fan_chain(&rect(), p(100.0, 0.0), d(1.0, 0.0), 30.0).unwrap();
    let g = geoms(&f);
    assert!((chain_length_mm(&g) - 630.0).abs() < 1e-9);
    assert!((chain_signed_area_mm2(&g) - (20000.0 + 5000.0 * s())).abs() < 1e-6);
    assert!(!chain_self_touches(&g));
}

#[test]
fn la_longueur_du_bord_fixe_est_conservee_les_deux_morceaux_du_bas_font_100_et_100() {
    let f = fan_chain(&rect(), p(100.0, 0.0), d(1.0, 0.0), 30.0).unwrap();
    let g = geoms(&f);
    // Bas : (0;0)-(100;0) puis (100;0)-(195,5 ; -29,66) : 100 + 100.
    let bottom: f64 = g
        .iter()
        .filter(|e| e.start().y <= 1e-9 && e.end().y <= 1e-9 && e.start().x < 196.0)
        .map(EdgeGeom::length_mm)
        .sum();
    assert!((bottom - 200.0).abs() < 1e-9, "bord fixe : {bottom}");
}

#[test]
fn bord_fixe_en_haut_donne_le_resultat_symetrique_par_demi_tour() {
    // Bord fixe = le haut, de (200;100) vers (0;100) : dir = (-1;0), pivot (100;100).
    let f = fan_chain(&rect(), p(100.0, 100.0), d(-1.0, 0.0), 30.0).unwrap();
    let g = geoms(&f);
    assert!(closed(&g));
    let flip = |q: (f64, f64)| (200.0 - q.0, 100.0 - q.1);
    for q in [
        (100.0, 100.0),
        (0.0, 100.0),
        (0.0, 0.0),
        (100.0, 0.0),
        cw((200.0, 0.0), (100.0, 0.0), C, s()),
        cw((200.0, 100.0), (100.0, 0.0), C, s()),
        cw((100.0, 100.0), (100.0, 0.0), C, s()),
    ] {
        let (x, y) = flip(q);
        assert!(has_vertex(&g, x, y), "sommet ({x} ; {y}) absent");
    }
    assert!((chain_length_mm(&g) - 630.0).abs() < 1e-9);
}

#[test]
fn trois_lignes_de_30_cumulent_les_rotations_et_les_ouvertures() {
    // Lignes en x = 50, 100, 150, traitées de la plus éloignée à la plus proche.
    let mut chain = rect();
    let mut chords = 0.0;
    let mut stay_ends = Vec::new();
    for x in [150.0, 100.0, 50.0] {
        let f = fan_chain(&chain, p(x, 0.0), d(1.0, 0.0), 30.0).unwrap();
        chords += distance_mm(f.stay_end, f.moved_end);
        stay_ends.push(f.stay_end);
        chain = geoms(&f);
        assert!(closed(&chain));
    }
    assert!((chords - 90.0).abs() < 1e-9);
    assert!((chain_length_mm(&chain) - 690.0).abs() < 1e-9);
    assert!((chain_signed_area_mm2(&chain) - (20000.0 + 3.0 * 5000.0 * s())).abs() < 1e-6);
    // Bas : quatre morceaux de 50, chacun tourné d'un angle de plus (calculé à la main).
    let (c1, s1) = (C, s());
    let (c2, s2) = (2.0 * C * C - 1.0, 2.0 * s() * C);
    let (c3, s3) = (c2 * C - s2 * s(), s2 * C + c2 * s());
    let b1 = (50.0, 0.0);
    let b2 = (b1.0 + 50.0 * c1, b1.1 - 50.0 * s1);
    let b3 = (b2.0 + 50.0 * c2, b2.1 - 50.0 * s2);
    let b4 = (b3.0 + 50.0 * c3, b3.1 - 50.0 * s3);
    for (x, y) in [(0.0, 0.0), b1, b2, b3, b4] {
        assert!(has_vertex(&chain, x, y), "sommet du bas ({x} ; {y}) absent");
    }
    // Le coin bas-droit est le dernier sommet du bord fixe : (50 + 50 c1 + ...).
    assert!(has_vertex(&chain, b4.0, b4.1));
    // Les trois cordes du haut mesurent 30 chacune.
    let n_chords = chain
        .iter()
        .filter(|e| matches!(e, EdgeGeom::Line(..)) && (e.length_mm() - 30.0).abs() < 1e-9)
        .count();
    assert_eq!(n_chords, 3);
    assert!(!chain_self_touches(&chain));
}

#[test]
fn courbe_au_sommet_longueurs_et_aire_conservees() {
    // Haut en Bézier symétrique de (200;100) à (0;100), commandes (150;140) et (50;140) :
    // B(0,5) = (100 ; 130), donc L = 130. Ouverture 26 : sin(θ/2) = 26 / 260 = 0,1.
    let top = CubicBezier::new(
        p(200.0, 100.0),
        p(150.0, 140.0),
        p(50.0, 140.0),
        p(0.0, 100.0),
    );
    let chain = vec![
        EdgeGeom::Line(p(0.0, 0.0), p(200.0, 0.0)),
        EdgeGeom::Line(p(200.0, 0.0), p(200.0, 100.0)),
        EdgeGeom::Cubic(top),
        EdgeGeom::Line(p(0.0, 100.0), p(0.0, 0.0)),
    ];
    let f = fan_chain(&chain, p(100.0, 0.0), d(1.0, 0.0), 26.0).unwrap();
    assert!((f.length_mm - 130.0).abs() < 1e-6);
    let sin_theta = 2.0 * 0.1 * 0.99_f64.sqrt();
    assert!((f.angle_rad.sin() - sin_theta).abs() < 1e-12);
    let g = geoms(&f);
    assert!(closed(&g));
    assert!((chain_length_mm(&g) - (chain_length_mm(&chain) + 26.0)).abs() < 1e-6);
    let gain = 0.5 * 130.0 * 130.0 * sin_theta;
    assert!(
        (chain_signed_area_mm2(&g) - chain_signed_area_mm2(&chain) - gain).abs()
            < 1e-5 * chain_signed_area_mm2(&g)
    );
}

#[test]
fn refus_ouverture_nulle_negative_non_finie_ou_trop_grande() {
    for o in [0.0, -5.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            fan_chain(&rect(), p(100.0, 0.0), d(1.0, 0.0), o),
            Err(FlareError::BadOpening(_))
        ));
    }
    // L = 100 : au plus 141,42 (rotation de 90°).
    assert!(matches!(
        fan_chain(&rect(), p(100.0, 0.0), d(1.0, 0.0), 142.0),
        Err(FlareError::BadOpening(_))
    ));
    assert!(fan_chain(&rect(), p(100.0, 0.0), d(1.0, 0.0), 141.0).is_ok());
}

#[test]
fn refus_pivot_hors_du_contour_direction_nulle_et_coupe_impossible() {
    assert!(matches!(
        fan_chain(&rect(), p(100.0, 40.0), d(1.0, 0.0), 10.0),
        Err(FlareError::Mismatch(_))
    ));
    assert_eq!(
        fan_chain(&rect(), p(100.0, 0.0), d(0.0, 0.0), 10.0),
        Err(FlareError::BadLine)
    );
    assert!(matches!(
        fan_chain(&rect(), p(300.0, 0.0), d(1.0, 0.0), 10.0),
        Err(FlareError::Cut(CutError::NoCrossing))
    ));
    // Forme en C : la droite x = 100 traverse quatre fois (y = 0, 30, 70, 100).
    let u = line_chain(&[
        (0.0, 0.0),
        (200.0, 0.0),
        (200.0, 30.0),
        (50.0, 30.0),
        (50.0, 70.0),
        (200.0, 70.0),
        (200.0, 100.0),
        (0.0, 100.0),
    ]);
    assert!(matches!(
        fan_chain(&u, p(100.0, 0.0), d(1.0, 0.0), 10.0),
        Err(FlareError::Cut(CutError::TooManyCrossings { .. }))
    ));
}

#[test]
fn contour_qui_se_recoupe_ou_se_touche_est_detecte() {
    assert!(!chain_self_touches(&rect()));
    let bowtie = line_chain(&[(0.0, 0.0), (100.0, 100.0), (100.0, 0.0), (0.0, 100.0)]);
    assert!(chain_self_touches(&bowtie));
    // Deux sommets non voisins qui se touchent.
    let pinch = line_chain(&[
        (0.0, 0.0),
        (100.0, 0.0),
        (100.0, 50.0),
        (50.0, 50.0),
        (100.0, 50.0 + 1e-9),
        (100.0, 100.0),
        (0.0, 100.0),
    ]);
    assert!(chain_self_touches(&pinch));
}

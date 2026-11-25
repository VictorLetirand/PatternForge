use super::*;
use std::f64::consts::PI;

fn p(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Rectangle `w × h` anti-horaire, coin bas gauche en (0, 0).
fn rect(w: f64, h: f64) -> Vec<EdgeGeom> {
    vec![
        EdgeGeom::Line(p(0.0, 0.0), p(w, 0.0)),
        EdgeGeom::Line(p(w, 0.0), p(w, h)),
        EdgeGeom::Line(p(w, h), p(0.0, h)),
        EdgeGeom::Line(p(0.0, h), p(0.0, 0.0)),
    ]
}

fn geoms(h: &[CutEdge]) -> Vec<EdgeGeom> {
    h.iter().map(|e| e.geom).collect()
}

fn area(h: &[CutEdge]) -> f64 {
    chain_signed_area_mm2(&geoms(h))
}

#[test]
fn rectangle_cut_vertical_gives_exact_areas_and_points() {
    let c = cut_chain_by_line(&rect(100.0, 60.0), p(30.0, -5.0), p(30.0, 5.0)).unwrap();
    // Droite orientée vers le haut : la gauche est x < 30.
    assert!((area(&c.left) - 1800.0).abs() < 1e-9);
    assert!((area(&c.right) - 4200.0).abs() < 1e-9);
    assert!(distance_mm(c.points[0], p(30.0, 0.0)) < 1e-9);
    assert!(distance_mm(c.points[1], p(30.0, 60.0)) < 1e-9);
    // Les deux moitiés sont fermées et dans le sens de la chaîne (anti-horaire).
    for h in [&c.left, &c.right] {
        let g = geoms(h);
        for j in 0..g.len() {
            assert!(distance_mm(g[j].end(), g[(j + 1) % g.len()].start()) < 1e-12);
        }
        assert!(chain_signed_area_mm2(&g) > 0.0);
    }
}

#[test]
fn reversed_line_swaps_sides() {
    let c = cut_chain_by_line(&rect(100.0, 60.0), p(30.0, 5.0), p(30.0, -5.0)).unwrap();
    assert!((area(&c.left) - 4200.0).abs() < 1e-9);
    assert!((area(&c.right) - 1800.0).abs() < 1e-9);
}

#[test]
fn diagonal_through_two_vertices_gives_two_triangles() {
    let c = cut_chain_by_line(&rect(100.0, 60.0), p(0.0, 0.0), p(100.0, 60.0)).unwrap();
    assert!((area(&c.left) - 3000.0).abs() < 1e-9);
    assert!((area(&c.right) - 3000.0).abs() < 1e-9);
    assert_eq!((c.left.len(), c.right.len()), (3, 3));
}

#[test]
fn refusals() {
    let r = rect(100.0, 60.0);
    assert_eq!(
        cut_chain_by_line(&r, p(1.0, 1.0), p(1.0, 1.0)),
        Err(CutError::DegenerateLine)
    );
    assert_eq!(
        cut_chain_by_line(&r, p(0.0, 100.0), p(1.0, 100.0)),
        Err(CutError::NoCrossing)
    );
    // Contact en un seul coin.
    assert_eq!(
        cut_chain_by_line(&r, p(0.0, 0.0), p(-1.0, 1.0)),
        Err(CutError::Touches)
    );
    // Droite le long d'un bord.
    assert_eq!(
        cut_chain_by_line(&r, p(0.0, 60.0), p(10.0, 60.0)),
        Err(CutError::AlongEdge)
    );
}

#[test]
fn concave_shape_cut_in_four_points_is_refused() {
    // U renversé : une droite horizontale basse le traverse en 4 points.
    let u = vec![
        EdgeGeom::Line(p(0.0, 0.0), p(30.0, 0.0)),
        EdgeGeom::Line(p(30.0, 0.0), p(30.0, 40.0)),
        EdgeGeom::Line(p(30.0, 40.0), p(60.0, 40.0)),
        EdgeGeom::Line(p(60.0, 40.0), p(60.0, 0.0)),
        EdgeGeom::Line(p(60.0, 0.0), p(90.0, 0.0)),
        EdgeGeom::Line(p(90.0, 0.0), p(90.0, 100.0)),
        EdgeGeom::Line(p(90.0, 100.0), p(0.0, 100.0)),
        EdgeGeom::Line(p(0.0, 100.0), p(0.0, 0.0)),
    ];
    assert_eq!(
        cut_chain_by_line(&u, p(-10.0, 20.0), p(100.0, 20.0)),
        Err(CutError::TooManyCrossings { count: 4 })
    );
    // Une droite haute ne coupe qu'en deux points : acceptée.
    assert!(cut_chain_by_line(&u, p(-10.0, 70.0), p(100.0, 70.0)).is_ok());
}

#[test]
fn bezier_dome_is_cut_exactly() {
    // Dôme : y = 0 de 0 à 100, puis Bézier (100,0)-(100,100)-(0,100)-(0,0), symétrique en x = 50.
    // Aire = 180000 · ∫ t²(1-t)² dt = 6000 ; coupe en x = 50 : 3000 + 3000, haut à y = 75.
    let dome = vec![
        EdgeGeom::Line(p(0.0, 0.0), p(100.0, 0.0)),
        EdgeGeom::Cubic(CubicBezier::new(
            p(100.0, 0.0),
            p(100.0, 100.0),
            p(0.0, 100.0),
            p(0.0, 0.0),
        )),
    ];
    assert!((chain_signed_area_mm2(&dome) - 6000.0).abs() < 1e-6);
    let c = cut_chain_by_line(&dome, p(50.0, 0.0), p(50.0, 1.0)).unwrap();
    assert!((area(&c.left) - 3000.0).abs() < 1e-6);
    assert!((area(&c.right) - 3000.0).abs() < 1e-6);
    assert!(distance_mm(c.points[1], p(50.0, 75.0)) < 1e-6);
    // Un des deux côtés contient la moitié de Bézier (de Casteljau en t = 0,5).
    let cubic_halves = c
        .left
        .iter()
        .chain(c.right.iter())
        .filter(|e| matches!(e.geom, EdgeGeom::Cubic(_)))
        .count();
    assert_eq!(cubic_halves, 2);
}

#[test]
fn half_disc_arc_is_cut_through_the_center() {
    let half = vec![
        EdgeGeom::Line(p(-50.0, 0.0), p(50.0, 0.0)),
        EdgeGeom::Arc(Arc {
            center: p(0.0, 0.0),
            radius_mm: 50.0,
            start_rad: 0.0,
            sweep_rad: PI,
        }),
    ];
    let c = cut_chain_by_line(&half, p(0.0, 0.0), p(0.0, 1.0)).unwrap();
    let quarter = PI * 50.0 * 50.0 / 4.0;
    assert!((area(&c.left) - quarter).abs() < 1e-9);
    assert!((area(&c.right) - quarter).abs() < 1e-9);
}

#[test]
fn rigid_between_equal_segments_maps_endpoints() {
    let r = Rigid::between_segments(p(0.0, 0.0), p(10.0, 0.0), p(5.0, 5.0), p(5.0, -5.0)).unwrap();
    assert!(distance_mm(r.apply(p(0.0, 0.0)), p(5.0, 5.0)) < 1e-12);
    assert!(distance_mm(r.apply(p(10.0, 0.0)), p(5.0, -5.0)) < 1e-12);
    assert!(Rigid::between_segments(p(0.0, 0.0), p(0.0, 0.0), p(1.0, 1.0), p(2.0, 2.0)).is_err());
}

#[test]
fn deviation_is_zero_for_identical_edges_and_positive_otherwise() {
    let line = [EdgeGeom::Line(p(0.0, 0.0), p(100.0, 0.0))];
    let same = [EdgeGeom::Line(p(0.0, 0.0), p(100.0, 0.0))];
    let bow = [EdgeGeom::Cubic(CubicBezier::new(
        p(0.0, 0.0),
        p(30.0, 20.0),
        p(70.0, 20.0),
        p(100.0, 0.0),
    ))];
    assert!(max_deviation_mm(&line, &same) < 1e-9);
    assert!(max_deviation_mm(&line, &bow) > 10.0);
}

// ---- US-07.5 : coupe par une ligne (une arête) ----

#[test]
fn edge_cut_oblique_segment_exact_areas_and_orientation() {
    // Rectangle 200 x 100 ; ligne (50,-20) -> (150,120) : pente 1,4, coupe y = 0 en
    // x = 50 + 20/1,4 et y = 100 en x = 50 + 120/1,4.
    let line = EdgeGeom::Line(p(50.0, -20.0), p(150.0, 120.0));
    let c = cut_chain_by_edge(&rect(200.0, 100.0), &line).unwrap();
    let (x0, x1) = (50.0 + 20.0 / 1.4, 50.0 + 120.0 / 1.4);
    assert!(distance_mm(c.points[0], p(x0, 0.0)) < 1e-9);
    assert!(distance_mm(c.points[1], p(x1, 100.0)) < 1e-9);
    // Gauche de P -> Q : trapèze de côtés x0 (bas) et x1 (haut) sur la hauteur 100.
    let left = 100.0 * (x0 + x1) / 2.0;
    assert!((area(&c.left) - left).abs() < 1e-9);
    assert!((area(&c.right) - (20000.0 - left)).abs() < 1e-9);
    // L'arête de coupe : P -> Q à gauche, Q -> P à droite.
    let (cl, cr) = (c.left.last().unwrap(), c.right.last().unwrap());
    assert!(cl.source.is_none() && cr.source.is_none());
    assert!(distance_mm(cl.geom.start(), c.points[0]) < 1e-12);
    assert!(distance_mm(cl.geom.end(), c.points[1]) < 1e-12);
    assert!(distance_mm(cr.geom.start(), c.points[1]) < 1e-12);
    assert!(distance_mm(cr.geom.end(), c.points[0]) < 1e-12);
}

#[test]
fn edge_cut_curve_and_arc_lines_are_exact() {
    // Bézier dont les quatre points sont sur x = 100 : une droite paramétrée, demi-rectangles.
    let cubic = EdgeGeom::Cubic(CubicBezier {
        p0: p(100.0, -10.0),
        c1: p(100.0, 30.0),
        c2: p(100.0, 70.0),
        p3: p(100.0, 110.0),
    });
    let c = cut_chain_by_edge(&rect(200.0, 100.0), &cubic).unwrap();
    assert!((area(&c.left) - 10000.0).abs() < 1e-6);
    assert!((area(&c.right) - 10000.0).abs() < 1e-6);
    // Arc de centre (100, 50), rayon 80, de -90 à 90 degrés : coupe y = 0 et y = 100 en
    // x = 100 + sqrt(80^2 - 50^2).
    let arc = EdgeGeom::Arc(Arc {
        center: p(100.0, 50.0),
        radius_mm: 80.0,
        start_rad: -PI / 2.0,
        sweep_rad: PI,
    });
    let c = cut_chain_by_edge(&rect(200.0, 100.0), &arc).unwrap();
    let dx = (80.0f64 * 80.0 - 50.0 * 50.0).sqrt();
    assert!(distance_mm(c.points[0], p(100.0 + dx, 0.0)) < 1e-9);
    assert!(distance_mm(c.points[1], p(100.0 + dx, 100.0)) < 1e-9);
    // Segment circulaire de corde 100 (demi-angle a = asin(50/80)) : r^2/2 (2a - sin 2a), qui
    // bombe vers x grand : il appartient à la moitié de gauche ; la droite = le reste.
    let a = (50.0f64 / 80.0).asin();
    let seg = 80.0 * 80.0 / 2.0 * (2.0 * a - (2.0 * a).sin());
    let right = 100.0 * (200.0 - (100.0 + dx)) - seg;
    assert!((area(&c.right) - right).abs() < 1e-6);
    assert!((area(&c.left) + area(&c.right) - 20000.0).abs() < 1e-9);
}

#[test]
fn edge_cut_refusals() {
    let r = rect(200.0, 100.0);
    assert_eq!(
        cut_chain_by_edge(&r, &EdgeGeom::Line(p(300.0, 0.0), p(300.0, 100.0))),
        Err(CutError::NoCrossing)
    );
    // S'arrête à l'intérieur : un seul contact.
    assert_eq!(
        cut_chain_by_edge(&r, &EdgeGeom::Line(p(50.0, -10.0), p(50.0, 60.0))),
        Err(CutError::Touches)
    );
    // Entièrement à l'intérieur.
    assert_eq!(
        cut_chain_by_edge(&r, &EdgeGeom::Line(p(50.0, 10.0), p(50.0, 60.0))),
        Err(CutError::NoCrossing)
    );
    // Confondue avec un bord.
    assert_eq!(
        cut_chain_by_edge(&r, &EdgeGeom::Line(p(-10.0, 0.0), p(210.0, 0.0))),
        Err(CutError::AlongEdge)
    );
    // Dégénérée.
    assert!(cut_chain_by_edge(&r, &EdgeGeom::Line(p(50.0, 0.0), p(50.0, 0.0))).is_err());
    // Arc de 270 degrés autour de (100, 50), rayon 70 : plus de deux traversées.
    let arc = EdgeGeom::Arc(Arc {
        center: p(100.0, 50.0),
        radius_mm: 70.0,
        start_rad: 0.0,
        sweep_rad: 1.5 * PI,
    });
    assert!(matches!(
        cut_chain_by_edge(&r, &arc),
        Err(CutError::TooManyCrossings { .. })
    ));
}

#[test]
fn edge_cut_concave_piece() {
    // Contour en U : une verticale dans le montant gauche coupe en deux points ; une ligne qui
    // rejoint les deux pointes passe hors de la pièce.
    let u = vec![
        EdgeGeom::Line(p(0.0, 0.0), p(100.0, 0.0)),
        EdgeGeom::Line(p(100.0, 0.0), p(100.0, 100.0)),
        EdgeGeom::Line(p(100.0, 100.0), p(70.0, 100.0)),
        EdgeGeom::Line(p(70.0, 100.0), p(70.0, 30.0)),
        EdgeGeom::Line(p(70.0, 30.0), p(30.0, 30.0)),
        EdgeGeom::Line(p(30.0, 30.0), p(30.0, 100.0)),
        EdgeGeom::Line(p(30.0, 100.0), p(0.0, 100.0)),
        EdgeGeom::Line(p(0.0, 100.0), p(0.0, 0.0)),
    ];
    let ok = cut_chain_by_edge(&u, &EdgeGeom::Line(p(15.0, -5.0), p(15.0, 105.0))).unwrap();
    assert!((area(&ok.left) - 1500.0).abs() < 1e-9);
    // U : 100 x 100 - 40 x 70 = 7200.
    assert!((area(&ok.right) - 5700.0).abs() < 1e-9);
    // Ligne entre les pointes des montants (30,100) -> (70,100) prolongée : hors pièce.
    let out = cut_chain_by_edge(&u, &EdgeGeom::Line(p(30.0, 110.0), p(70.0, 90.0)));
    assert!(out.is_err(), "{out:?}");
    let out = cut_chain_by_edge(&u, &EdgeGeom::Line(p(-10.0, 50.0), p(110.0, 50.0)));
    assert!(matches!(out, Err(CutError::TooManyCrossings { .. })));
}

#[test]
fn edge_cut_self_crossing_bezier_is_refused() {
    let l = EdgeGeom::Cubic(CubicBezier {
        p0: p(40.0, -10.0),
        c1: p(160.0, 140.0),
        c2: p(-60.0, 140.0),
        p3: p(160.0, -10.0),
    });
    let r = cut_chain_by_edge(&rect(200.0, 100.0), &l);
    assert!(r.is_err(), "{r:?}");
}

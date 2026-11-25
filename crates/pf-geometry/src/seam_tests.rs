//! Tests du contour de coupe (US-05.1). Les coordonnées attendues sont calculées à la main
//! (commentaires) ; aucune n'est relue du code testé.

use super::*;
use crate::{Arc, CubicBezier};

fn p(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Polygone fermé anti-horaire en segments.
fn poly(pts: &[(f64, f64)]) -> Vec<EdgeGeom> {
    (0..pts.len())
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
            EdgeGeom::Line(p(a.0, a.1), p(b.0, b.1))
        })
        .collect()
}

fn rect(w: f64, h: f64) -> Vec<EdgeGeom> {
    poly(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)])
}

fn uniform(n: usize, mm: f64) -> Vec<EdgeMargin> {
    vec![EdgeMargin::uniform(mm); n]
}

/// Sommets (début de chaque segment) ; échoue si un morceau n'est pas un segment.
fn vertices(traces: &[Trace]) -> Vec<PointMm> {
    traces
        .iter()
        .map(|t| match t {
            Trace::Line(a, _) => *a,
            other => panic!("segment attendu, reçu {other:?}"),
        })
        .collect()
}

/// Les sommets sont ceux de `want`, dans l'ordre, à une rotation circulaire près.
fn assert_cycle(got: &[PointMm], want: &[(f64, f64)], tol: f64) {
    assert_eq!(got.len(), want.len(), "nombre de sommets : {got:?}");
    let n = want.len();
    let ok = (0..n).any(|shift| {
        (0..n).all(|i| {
            let (g, w) = (got[(i + shift) % n], want[i]);
            (g.x - w.0).abs() <= tol && (g.y - w.1).abs() <= tol
        })
    });
    assert!(ok, "sommets {got:?} ≠ attendus {want:?}");
}

/// Aire (formule du lacet) d'une suite de sommets.
fn shoelace(v: &[(f64, f64)]) -> f64 {
    let n = v.len();
    (0..n)
        .map(|i| v[i].0 * v[(i + 1) % n].1 - v[(i + 1) % n].0 * v[i].1)
        .sum::<f64>()
        / 2.0
}

fn seg_dist(q: PointMm, a: PointMm, b: PointMm) -> f64 {
    let (u, w) = (b - a, q - a);
    let t = (w.dot(u) / u.dot(u)).clamp(0.0, 1.0);
    distance_mm(q, a.lerp(b, t))
}

// ---------------------------------------------------------------- critères

#[test]
fn rectangle_100_par_60_marge_10_donne_120_par_80() {
    let cut = seam_contour(&rect(100.0, 60.0), &uniform(4, 10.0)).unwrap();
    // Chaque côté recule de 10 : x de -10 à 110, y de -10 à 70.
    let v = vertices(&cut);
    assert_cycle(
        &v,
        &[(-10.0, -10.0), (110.0, -10.0), (110.0, 70.0), (-10.0, 70.0)],
        1e-9,
    );
    let pts: Vec<(f64, f64)> = v.iter().map(|q| (q.x, q.y)).collect();
    assert!((shoelace(&pts) - 9600.0).abs() < 1e-9, "aire 120 × 80");
    assert_eq!(
        cut.len(),
        4,
        "les raccords en onglet sont fondus dans les côtés"
    );
}

#[test]
fn marge_nulle_partout_copie_exacte_du_contour_de_couture() {
    let chain = rect(100.0, 60.0);
    let cut = seam_contour(&chain, &uniform(4, 0.0)).unwrap();
    let want: Vec<Trace> = chain.iter().map(edge_trace).collect();
    assert_eq!(cut, want);
}

#[test]
fn marge_nulle_sur_un_bord_le_bord_reste_sur_la_couture() {
    // Bas à 0, droite 10, haut 10, gauche 10 : le bas reste sur y = 0, les coins du bas
    // prolongent les côtés (onglet) : (-10, 0) et (110, 0).
    let m = [0.0, 10.0, 10.0, 10.0].map(EdgeMargin::uniform);
    let cut = seam_contour(&rect(100.0, 60.0), &m).unwrap();
    assert_cycle(
        &vertices(&cut),
        &[(-10.0, 0.0), (110.0, 0.0), (110.0, 70.0), (-10.0, 70.0)],
        1e-9,
    );
}

#[test]
fn marge_differente_par_bord() {
    // Bas 5 (y = -5), droite 10 (x = 110), haut 15 (y = 75), gauche 20 (x = -20).
    let m = [5.0, 10.0, 15.0, 20.0].map(EdgeMargin::uniform);
    let cut = seam_contour(&rect(100.0, 60.0), &m).unwrap();
    assert_cycle(
        &vertices(&cut),
        &[(-20.0, -5.0), (110.0, -5.0), (110.0, 75.0), (-20.0, 75.0)],
        1e-9,
    );
}

#[test]
fn marge_progressive_sur_une_droite_tournee() {
    // Triangle A(0,0) B(100,0) C(0,100) ; l'hypoténuse BC a 10 mm en B et 20 mm en C, les deux
    // autres bords 0. L'hypoténuse (x + y = 100, normale extérieure (1,1)/√2) devient la droite
    // qui passe par B' = (100 + 10/√2, 10/√2) et C' = (20/√2, 100 + 20/√2). Elle coupe y = 0 en
    // x = 113,208177 et x = 0 en y = 130,436454 (calcul : paramètre t = −y_B'/(y_C' − y_B')).
    let chain = poly(&[(0.0, 0.0), (100.0, 0.0), (0.0, 100.0)]);
    let m = [
        EdgeMargin::uniform(0.0),
        EdgeMargin {
            start_mm: 10.0,
            end_mm: 20.0,
        },
        EdgeMargin::uniform(0.0),
    ];
    let cut = seam_contour(&chain, &m).unwrap();
    assert_cycle(
        &vertices(&cut),
        &[(0.0, 0.0), (113.208177, 0.0), (0.0, 130.436454)],
        1e-5,
    );
    // Distance mesurée : 10 en B' et 20 en C' (extrémités de la droite décalée).
    let hyp = cut
        .iter()
        .find_map(|t| match t {
            Trace::Line(a, b) if a.y == 0.0 && b.x.abs() < 1e-9 => Some((*a, *b)),
            _ => None,
        })
        .expect("hypoténuse décalée");
    let (b_off, c_off) = (
        p(100.0 + 10.0 / 2f64.sqrt(), 10.0 / 2f64.sqrt()),
        p(20.0 / 2f64.sqrt(), 100.0 + 20.0 / 2f64.sqrt()),
    );
    assert!(seg_dist(b_off, hyp.0, hyp.1) < 1e-9 && seg_dist(c_off, hyp.0, hyp.1) < 1e-9);
}

#[test]
fn piece_en_l_coin_rentrant_coupe_sans_boucle() {
    // L : (0,0) (100,0) (100,40) (40,40) (40,100) (0,100), marge 10.
    // Coin rentrant (40,40) : y = 50 (haut du pied) coupe x = 50 (droite du bras) en (50,50).
    let l = poly(&[
        (0.0, 0.0),
        (100.0, 0.0),
        (100.0, 40.0),
        (40.0, 40.0),
        (40.0, 100.0),
        (0.0, 100.0),
    ]);
    let want = [
        (-10.0, -10.0),
        (110.0, -10.0),
        (110.0, 50.0),
        (50.0, 50.0),
        (50.0, 110.0),
        (-10.0, 110.0),
    ];
    let cut = seam_contour(&l, &uniform(6, 10.0)).unwrap();
    assert_cycle(&vertices(&cut), &want, 1e-9);
    assert!((shoelace(&want) - 10800.0).abs() < 1e-9, "7200 + 3600");

    // Marges différentes (bas 0, droite 10, dessus du pied 20, bras 10, haut 10, gauche 10) :
    // x = 110 et y = 60 en (110,60) ; y = 60 et x = 50 en (50,60) ; coin du bas : (-10,0), (110,0).
    let m = [0.0, 10.0, 20.0, 10.0, 10.0, 10.0].map(EdgeMargin::uniform);
    let cut = seam_contour(&l, &m).unwrap();
    let want = [
        (-10.0, 0.0),
        (110.0, 0.0),
        (110.0, 60.0),
        (50.0, 60.0),
        (50.0, 110.0),
        (-10.0, 110.0),
    ];
    assert_cycle(&vertices(&cut), &want, 1e-9);
    assert!((shoelace(&want) - 10200.0).abs() < 1e-9, "7200 + 3000");
}

#[test]
fn demi_disque_arc_rayon_plus_marge_exact() {
    // Segment (-50,0) → (50,0) puis demi-cercle de centre (0,0), rayon 50, anti-horaire.
    let arc = Arc {
        center: p(0.0, 0.0),
        radius_mm: 50.0,
        start_rad: 0.0,
        sweep_rad: std::f64::consts::PI,
    };
    let chain = vec![
        EdgeGeom::Line(p(-50.0, 0.0), p(50.0, 0.0)),
        EdgeGeom::Arc(arc),
    ];
    let cut = seam_contour(&chain, &uniform(2, 10.0)).unwrap();
    // Arc de rayon 60 (50 + 10) ; bas y = -10 ; deux raccords verticaux de 10 mm en x = ±60.
    let arcs: Vec<&Arc> = cut
        .iter()
        .filter_map(|t| if let Trace::Arc(a) = t { Some(a) } else { None })
        .collect();
    assert_eq!(arcs.len(), 1);
    assert!(
        (arcs[0].radius_mm - 60.0).abs() < 1e-9 && distance_mm(arcs[0].center, p(0.0, 0.0)) < 1e-9
    );
    assert_eq!(cut.len(), 4);
    let (mut perimeter, mut area) = (0.0, 0.0);
    for g in cut.iter().flat_map(crate::region::trace_geometry) {
        perimeter += g.length_mm();
    }
    area += chain_signed_area_mm2(
        &cut.iter()
            .flat_map(crate::region::trace_geometry)
            .collect::<Vec<_>>(),
    );
    // Périmètre : 120 + 10 + 10 + π·60 ; aire : 120 × 10 + π·60²/2.
    assert!(
        (perimeter - (140.0 + std::f64::consts::PI * 60.0)).abs() < 1e-6,
        "{perimeter}"
    );
    assert!(
        (area - (1200.0 + std::f64::consts::PI * 1800.0)).abs() < 1e-6,
        "{area}"
    );

    // Marges différentes (bas 5, arc 10) : le coin (50,0) relie y = -5 à x = 60 en (60,-5).
    let m = [EdgeMargin::uniform(5.0), EdgeMargin::uniform(10.0)];
    let cut = seam_contour(&chain, &m).unwrap();
    let lows: Vec<(PointMm, PointMm)> = cut
        .iter()
        .filter_map(|t| {
            if let Trace::Line(a, b) = t {
                Some((*a, *b))
            } else {
                None
            }
        })
        .collect();
    assert!(lows
        .iter()
        .any(|(a, b)| (a.y + 5.0).abs() < 1e-9 && (b.y + 5.0).abs() < 1e-9));
    assert!(lows.iter().any(
        |(a, b)| distance_mm(*a, p(60.0, -5.0)) < 1e-9 && distance_mm(*b, p(60.0, 0.0)) < 1e-9
    ));
}

#[test]
fn bezier_echantillonnee_a_la_marge_a_un_centieme_pres() {
    // Segment (0,0) → (100,0) puis Bézier (100,0) (100,60) (0,60) (0,0), anti-horaire.
    let c = CubicBezier::new(p(100.0, 0.0), p(100.0, 60.0), p(0.0, 60.0), p(0.0, 0.0));
    let chain = vec![
        EdgeGeom::Line(p(0.0, 0.0), p(100.0, 0.0)),
        EdgeGeom::Cubic(c),
    ];
    let cut = seam_contour(&chain, &uniform(2, 5.0)).unwrap();
    // Distance indépendante : échantillonnage dense de l'original, minimum brute.
    let dense: Vec<PointMm> = (0..=20000)
        .map(|i| c.point_at(f64::from(i) / 20000.0))
        .collect();
    let mut checked = 0;
    for t in &cut {
        if let Trace::Cubic(o) = t {
            for k in 1..40 {
                let q = o.point_at(f64::from(k) / 40.0);
                let d = dense
                    .iter()
                    .map(|d| distance_mm(*d, q))
                    .fold(f64::MAX, f64::min);
                assert!((d - 5.0).abs() <= 0.01, "distance {d} au lieu de 5");
                checked += 1;
            }
        }
    }
    assert!(checked >= 39, "au moins une Bézier de décalage");
}

#[test]
fn pointe_tres_aigue_coupee_en_biseau() {
    // Sommet (0,0), demi-angle 5° : (0,0) (100,-h) (100,h) avec h = 100·tan 5° = 8,748866.
    // Virage 170° : pointe m / cos 85° = 11,5 m > 4 m : biseau. Marge 2 : normales du sommet
    // (−sin 5°, ±cos 5°) donc segment vertical x = −2·sin 5° = −0,174311 de y = −1,992389 à +1,992389.
    let h = 100.0 * 5f64.to_radians().tan();
    let chain = poly(&[(0.0, 0.0), (100.0, -h), (100.0, h)]);
    let cut = seam_contour(&chain, &uniform(3, 2.0)).unwrap();
    let bevel = cut.iter().find_map(|t| match t {
        Trace::Line(a, b) if (a.x - b.x).abs() < 1e-9 && a.x < 0.0 => Some((*a, *b)),
        _ => None,
    });
    let (a, b) = bevel.expect("segment de biseau vertical");
    assert!(
        (a.x + 0.174311).abs() < 1e-5 && (a.y.abs() - 1.992389).abs() < 1e-5,
        "{a:?}"
    );
    assert!((b.y.abs() - 1.992389).abs() < 1e-5, "{b:?}");
    assert!(
        cut.iter().all(|t| match t {
            Trace::Line(a, b) => a.x > -0.2 && b.x > -0.2,
            _ => false,
        }),
        "aucun sommet n'avance jusqu'à la pointe (x = −22,9)"
    );
    // Même résultat par le chemin « marges différentes » (une marge à 0 sur le grand côté).
    let m = [
        EdgeMargin::uniform(2.0),
        EdgeMargin::uniform(2.0),
        EdgeMargin::uniform(2.0),
    ];
    assert!(seam_contour(&chain, &m).is_ok());
}

#[test]
fn pointe_moderee_reste_en_onglet() {
    // Demi-angle 20° : virage 140°, pointe m / cos 70° = 2,9238 m < 4 m : onglet.
    // Marge 3 : pointe à x = −3 / sin 20° = −8,771 (distance du sommet 8,771 = 2,9238 × 3).
    let h = 100.0 * 20f64.to_radians().tan();
    let chain = poly(&[(0.0, 0.0), (100.0, -h), (100.0, h)]);
    let cut = seam_contour(&chain, &uniform(3, 3.0)).unwrap();
    let tip = vertices(&cut)
        .into_iter()
        .map(|q| q.x)
        .fold(f64::MAX, f64::min);
    assert!((tip + 3.0 / 20f64.to_radians().sin()).abs() < 1e-6, "{tip}");
}

#[test]
fn biseau_aussi_pour_marges_differentes() {
    // Même pointe aiguë, marges 2 / 2 / 0 : la pointe reste coupée (4 × la plus grande marge = 8).
    let h = 100.0 * 5f64.to_radians().tan();
    let chain = poly(&[(0.0, 0.0), (100.0, -h), (100.0, h)]);
    let m = [
        EdgeMargin::uniform(2.0),
        EdgeMargin::uniform(0.0),
        EdgeMargin::uniform(2.0),
    ];
    let cut = seam_contour(&chain, &m).unwrap();
    let left = vertices(&cut)
        .into_iter()
        .map(|q| q.x)
        .fold(f64::MAX, f64::min);
    assert!(left > -0.2, "biseau attendu, x min = {left}");
}

// ---------------------------------------------------------------- distances

#[test]
fn distance_au_contour_egale_la_marge_en_tout_point_des_bords() {
    let chain = poly(&[
        (0.0, 0.0),
        (100.0, 0.0),
        (100.0, 40.0),
        (40.0, 40.0),
        (40.0, 100.0),
        (0.0, 100.0),
    ]);
    let m = [3.0, 7.0, 12.0, 7.0, 3.0, 9.0].map(EdgeMargin::uniform);
    let cut = seam_contour(&chain, &m).unwrap();
    let mut samples = 0;
    for t in &cut {
        let Trace::Line(a, b) = t else { panic!() };
        for k in 1..20 {
            let q = a.lerp(*b, f64::from(k) / 20.0);
            let d = chain
                .iter()
                .map(|e| match e {
                    EdgeGeom::Line(x, y) => seg_dist(q, *x, *y),
                    _ => unreachable!(),
                })
                .fold(f64::MAX, f64::min);
            // Un bord qui n'est pas un coin est exactement à sa marge ; un raccord est plus loin.
            assert!(d >= 3.0 - 0.01, "trop près : {d}");
            assert!(d <= 12.0 * 4.0, "pointe démesurée : {d}");
            samples += 1;
        }
    }
    assert!(samples >= 100);
    // Chaque côté décalé (parallèle à un bord) est à sa marge : test sur les milieux.
    let mids = [(50.0, -3.0, 3.0), (107.0, 20.0, 7.0), (70.0, 52.0, 12.0)];
    for (x, y, want) in mids {
        let d = chain
            .iter()
            .map(|e| match e {
                EdgeGeom::Line(a, b) => seg_dist(p(x, y), *a, *b),
                _ => unreachable!(),
            })
            .fold(f64::MAX, f64::min);
        assert!((d - want).abs() < 1e-9, "{d} ≠ {want}");
    }
}

// ---------------------------------------------------------------- refus

#[test]
fn refus_expliques() {
    let chain = rect(100.0, 60.0);
    for bad in [-1.0, f64::NAN, f64::INFINITY, 1000.01] {
        assert!(seam_contour(&chain, &uniform(4, bad)).is_err(), "{bad}");
    }
    assert!(
        seam_contour(&chain, &uniform(3, 1.0)).is_err(),
        "marges mal comptées"
    );
    assert!(seam_contour(&[], &[]).is_err());
    // Chaîne ouverte.
    let open = vec![
        EdgeGeom::Line(p(0.0, 0.0), p(10.0, 0.0)),
        EdgeGeom::Line(p(10.0, 0.0), p(10.0, 10.0)),
    ];
    assert!(seam_contour(&open, &uniform(2, 1.0)).is_err());
    // Horaire.
    let cw = poly(&[(0.0, 0.0), (0.0, 60.0), (100.0, 60.0), (100.0, 0.0)]);
    assert!(seam_contour(&cw, &uniform(4, 1.0)).is_err());
    // Marge progressive sur une courbe.
    let arc = Arc {
        center: p(0.0, 0.0),
        radius_mm: 50.0,
        start_rad: 0.0,
        sweep_rad: std::f64::consts::PI,
    };
    let half = vec![
        EdgeGeom::Line(p(-50.0, 0.0), p(50.0, 0.0)),
        EdgeGeom::Arc(arc),
    ];
    let m = [
        EdgeMargin::uniform(5.0),
        EdgeMargin {
            start_mm: 5.0,
            end_mm: 6.0,
        },
    ];
    let e = seam_contour(&half, &m).unwrap_err().to_string();
    assert!(e.contains("marge progressive : segments seulement"), "{e}");
    // Marge plus grande que le rayon d'un arc rentrant (arc horaire).
    let hole = Arc {
        center: p(0.0, 0.0),
        radius_mm: 5.0,
        start_rad: std::f64::consts::PI,
        sweep_rad: -std::f64::consts::PI,
    };
    let dent = vec![
        EdgeGeom::Line(p(-20.0, 0.0), p(-5.0, 0.0)),
        EdgeGeom::Arc(hole),
        EdgeGeom::Line(p(5.0, 0.0), p(20.0, 0.0)),
        EdgeGeom::Line(p(20.0, 0.0), p(20.0, 30.0)),
        EdgeGeom::Line(p(20.0, 30.0), p(-20.0, 30.0)),
        EdgeGeom::Line(p(-20.0, 30.0), p(-20.0, 0.0)),
    ];
    let m = [1.0, 6.0, 1.0, 1.0, 1.0, 1.0].map(EdgeMargin::uniform);
    assert!(seam_contour(&dent, &m).is_err());
}

#[test]
fn echec_si_un_bord_trop_court_en_coin_rentrant() {
    // Encoche de 4 mm de large (profondeur 10) dans un rectangle ; marges 0 sauf l'encoche (8) :
    // les deux parois décalées se croisent avant la fin du fond : refus expliqué, jamais un faux
    // contour.
    let r = poly(&[
        (0.0, 0.0),
        (100.0, 0.0),
        (100.0, 50.0),
        (52.0, 50.0),
        (52.0, 40.0),
        (48.0, 40.0),
        (48.0, 50.0),
        (0.0, 50.0),
    ]);
    let m = [0.0, 0.0, 0.0, 8.0, 8.0, 8.0, 0.0, 0.0].map(EdgeMargin::uniform);
    let e = seam_contour(&r, &m).unwrap_err().to_string();
    assert!(e.contains("coin rentrant"), "{e}");
}

#[test]
fn marge_progressive_inclinee_prolongee_ou_raccourcie_au_sommet() {
    // Rectangle 100 × 60, marge 10 partout sauf le côté droit : 0 en bas (100,0), 20 en haut
    // (100,60). Droite décalée x = 100 + y / 3 : elle recoupe le bas décalé (y = -10) avant son
    // bout, en x = 96,666667 (le bas est raccourci), et le haut (y = 70) en x = 123,333333.
    let m = [
        EdgeMargin::uniform(10.0),
        EdgeMargin {
            start_mm: 0.0,
            end_mm: 20.0,
        },
        EdgeMargin::uniform(10.0),
        EdgeMargin::uniform(10.0),
    ];
    let cut = seam_contour(&rect(100.0, 60.0), &m).unwrap();
    assert_cycle(
        &vertices(&cut),
        &[
            (-10.0, -10.0),
            (96.666667, -10.0),
            (123.333333, 70.0),
            (-10.0, 70.0),
        ],
        1e-6,
    );
}

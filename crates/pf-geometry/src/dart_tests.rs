//! Tests de `dart.rs` : toutes les valeurs attendues sont calculées à la main.
//!
//! Pince de référence : sommet A = (0 ; 100), point d'axe (0 ; 0), longueur 100, ouverture 20.
//! Axe = (0 ; −1), normale gauche = (1 ; 0) : pied gauche (10 ; 0), pied droit (−10 ; 0) ;
//! jambes = √(100² + 10²) = √10100. Angle d'ouverture θ = 2·atan(0,1) = 11,421186275°,
//! cos θ = 0,99 / 1,01 et sin θ = 0,2 / 1,01 (formules en tan(θ/2) = 0,1).

use super::*;

const C: f64 = 0.99 / 1.01;
const S: f64 = 0.2 / 1.01;
const THETA_DEG: f64 = 11.421186274999286;

fn p(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn reference() -> DartShape {
    dart_shape(p(0.0, 100.0), p(0.0, 0.0), 100.0, 20.0).unwrap()
}

fn base_line(x0: f64, x1: f64) -> Vec<EdgeGeom> {
    vec![EdgeGeom::Line(p(x0, 0.0), p(x1, 0.0))]
}

fn close_to(a: PointMm, x: f64, y: f64) {
    assert!(
        (a.x - x).abs() < 1e-9 && (a.y - y).abs() < 1e-9,
        "({}, {}) attendu ({x}, {y})",
        a.x,
        a.y
    );
}

#[test]
fn les_deux_jambes_sont_egales_a_un_millionieme() {
    let s = reference();
    let expected = 10100.0_f64.sqrt();
    assert!((s.leg_left_mm - expected).abs() < 1e-9);
    assert!((s.leg_right_mm - expected).abs() < 1e-9);
    assert!((s.leg_left_mm - s.leg_right_mm).abs() < 1e-6);
    close_to(s.left_foot, 10.0, 0.0);
    close_to(s.right_foot, -10.0, 0.0);
    close_to(s.base_mid, 0.0, 0.0);
    assert!((s.angle_deg - THETA_DEG).abs() < 1e-9);
}

#[test]
fn les_jambes_restent_egales_sur_un_axe_oblique() {
    // Axe (3 ; 4) / 5 : milieu de base = A + 50 · (0,6 ; 0,8) ; pieds = M ± 5 · (−0,8 ; 0,6).
    let s = dart_shape(p(10.0, 20.0), p(13.0, 24.0), 50.0, 10.0).unwrap();
    close_to(s.base_mid, 40.0, 60.0);
    close_to(s.left_foot, 36.0, 63.0);
    close_to(s.right_foot, 44.0, 57.0);
    assert!((s.leg_left_mm - s.leg_right_mm).abs() < 1e-6);
    assert!((s.leg_left_mm - 2525.0_f64.sqrt()).abs() < 1e-9);
}

#[test]
fn une_pince_degeneree_est_refusee() {
    let a = p(0.0, 0.0);
    let b = p(0.0, 5.0);
    assert_eq!(dart_shape(a, a, 10.0, 2.0), Err(DartError::NoAxis));
    assert_eq!(dart_shape(a, b, 0.5, 0.2), Err(DartError::Length(0.5)));
    assert_eq!(
        dart_shape(a, b, 2001.0, 2.0),
        Err(DartError::Length(2001.0))
    );
    assert_eq!(dart_shape(a, b, 10.0, 0.0), Err(DartError::Opening(0.0)));
    assert_eq!(dart_shape(a, b, 10.0, 0.05), Err(DartError::Opening(0.05)));
    assert_eq!(dart_shape(a, b, 10.0, 11.0), Err(DartError::Opening(11.0)));
    assert!(dart_shape(a, b, 10.0, 10.0).is_ok());
    assert_eq!(dart_shape(a, b, f64::NAN, 2.0), Err(DartError::NonFinite));
    assert_eq!(
        dart_shape(p(f64::INFINITY, 0.0), b, 10.0, 2.0),
        Err(DartError::NonFinite)
    );
}

#[test]
fn fermer_une_pince_retire_la_largeur_de_la_base() {
    let s = reference();
    let r = close_dart(&base_line(-100.0, 100.0), &s, Side::Left).unwrap();
    assert!((r.before_mm - 200.0).abs() < 1e-9);
    assert!((r.removed_mm - 20.0).abs() < 1e-9);
    assert!((r.after_mm - 180.0).abs() < 1e-9);
    assert!((chain_length_mm(&r.chain) - 180.0).abs() < 1e-9);
}

#[test]
fn pince_couchee_jambe_gauche_fixe_le_cote_droit_tourne() {
    // Jambe gauche fixe : le pied droit (−10 ; 0), donc la partie [−100 ; −10], tourne de +θ
    // autour de A. Départ (−100 ; 0) : vecteur (−100 ; −100) → (−100 C + 100 S ; −100 S − 100 C).
    let s = reference();
    let r = close_dart(&base_line(-100.0, 100.0), &s, Side::Left).unwrap();
    assert_eq!(r.chain.len(), 2);
    close_to(
        r.chain[0].start(),
        -100.0 * C + 100.0 * S,
        100.0 - 100.0 * S - 100.0 * C,
    );
    close_to(r.chain[0].end(), 10.0, 0.0);
    close_to(r.chain[1].start(), 10.0, 0.0);
    close_to(r.chain[1].end(), 100.0, 0.0);
    assert!((r.rotation_deg - THETA_DEG).abs() < 1e-9);
    // Tourné de +θ puis ligne droite : virage de −θ au pied.
    assert!((r.corner_deg.unwrap() + THETA_DEG).abs() < 1e-9);
}

#[test]
fn pince_couchee_de_l_autre_cote() {
    // Jambe droite fixe : la partie [10 ; 100] tourne de −θ. Fin (100 ; 0) : vecteur (100 ; −100)
    // → (100 C − 100 S ; −100 S − 100 C).
    let s = reference();
    let r = close_dart(&base_line(-100.0, 100.0), &s, Side::Right).unwrap();
    assert_eq!(r.chain.len(), 2);
    close_to(r.chain[0].end(), -10.0, 0.0);
    close_to(r.chain[1].start(), -10.0, 0.0);
    close_to(
        r.chain[1].end(),
        100.0 * C - 100.0 * S,
        100.0 - 100.0 * S - 100.0 * C,
    );
    assert!((r.rotation_deg + THETA_DEG).abs() < 1e-9);
    assert!((r.corner_deg.unwrap() + THETA_DEG).abs() < 1e-9);
    assert!((r.after_mm - 180.0).abs() < 1e-9);
}

/// Intersection de la droite `a → b` avec le segment `c → d` (formule de Cramer, écrite à part).
fn cramer(a: PointMm, b: PointMm, c: PointMm, d: PointMm) -> (f64, f64) {
    let (ux, uy) = (b.x - a.x, b.y - a.y);
    let (vx, vy) = (d.x - c.x, d.y - c.y);
    let (wx, wy) = (c.x - a.x, c.y - a.y);
    let det = ux * (-vy) - uy * (-vx);
    let s = (wx * (-vy) - wy * (-vx)) / det;
    let u = (ux * wy - uy * wx) / det;
    (s, u)
}

#[test]
fn capuchon_des_deux_sens_de_pliage() {
    // Ligne (−25 ; 0) → (100 ; 0) : côté avant 15 mm, côté après 90 mm ; étendue 2 w = 40 mm :
    // P1 = (−25 ; 0) (15 mm), P2 = (50 ; 0) (40 mm).
    let s = reference();
    let line = base_line(-25.0, 100.0);
    let a = s.apex;

    // Jambe gauche fixe : P1 tourne de +θ ; la droite P1' → P2 coupe la jambe A → (10 ; 0).
    let r = close_dart(&line, &s, Side::Left).unwrap();
    let cap = r.capuchon.unwrap();
    let p1r = p(
        a.x + C * (-25.0 - a.x) - S * (0.0 - a.y),
        a.y + S * (-25.0 - a.x) + C * (0.0 - a.y),
    );
    let (sk, uk) = cramer(a, p(10.0, 0.0), p1r, p(50.0, 0.0));
    assert!(sk > 0.0 && (0.0..=1.0).contains(&uk));
    let kfix = p(a.x + sk * (10.0 - a.x), a.y + sk * (0.0 - a.y));
    close_to(cap[0], -25.0, 0.0);
    close_to(cap[3], 50.0, 0.0);
    close_to(cap[2], kfix.x, kfix.y);
    // Ka = K tourné de −θ : sur la jambe droite, même distance au sommet.
    let kmov = p(
        a.x + C * (kfix.x - a.x) + S * (kfix.y - a.y),
        a.y - S * (kfix.x - a.x) + C * (kfix.y - a.y),
    );
    close_to(cap[1], kmov.x, kmov.y);
    let dist = |q: PointMm| ((q.x - a.x).powi(2) + (q.y - a.y).powi(2)).sqrt();
    assert!((dist(cap[1]) - dist(cap[2])).abs() < 1e-9);
    assert!((r.capuchon_leg_mm.unwrap() - dist(kfix)).abs() < 1e-9);
    // Les deux points du capuchon sont sur les prolongements des jambes.
    let on_line =
        |q: PointMm, f: PointMm| ((q.x - a.x) * (f.y - a.y) - (q.y - a.y) * (f.x - a.x)).abs();
    assert!(on_line(cap[2], p(10.0, 0.0)) < 1e-9);
    assert!(on_line(cap[1], p(-10.0, 0.0)) < 1e-9);

    // Jambe droite fixe : P2 tourne de −θ ; la droite P1 → P2' coupe la jambe A → (−10 ; 0).
    let r2 = close_dart(&line, &s, Side::Right).unwrap();
    let cap2 = r2.capuchon.unwrap();
    let p2r = p(
        a.x + C * (50.0 - a.x) + S * (0.0 - a.y),
        a.y - S * (50.0 - a.x) + C * (0.0 - a.y),
    );
    let (sk2, uk2) = cramer(a, p(-10.0, 0.0), p(-25.0, 0.0), p2r);
    assert!(sk2 > 0.0 && (0.0..=1.0).contains(&uk2));
    let kfix2 = p(a.x + sk2 * (-10.0 - a.x), a.y + sk2 * (0.0 - a.y));
    close_to(cap2[1], kfix2.x, kfix2.y);
    assert!((dist(cap2[1]) - dist(cap2[2])).abs() < 1e-9);
    assert!(on_line(cap2[2], p(10.0, 0.0)) < 1e-9);
    // Plier d'un côté ou de l'autre revient à tourner tout le dessin de θ autour du sommet : la
    // base corrigée dépliée est la même (|AK| = 102,6697…), alors que les lignes pliées diffèrent.
    assert!((r.capuchon_leg_mm.unwrap() - r2.capuchon_leg_mm.unwrap()).abs() < 1e-9);
    for k in 0..4 {
        close_to(cap2[k], cap[k].x, cap[k].y);
    }
    assert!((r.capuchon_leg_mm.unwrap() - 102.66974644439436).abs() < 1e-9);
    assert!(
        (r.chain[0].start().x - r2.chain[0].start().x).abs() > 1.0,
        "les deux lignes pliées doivent différer"
    );
}

#[test]
fn pied_hors_de_la_ligne_est_refuse() {
    let s = reference();
    let off = vec![EdgeGeom::Line(p(-100.0, 5.0), p(100.0, 5.0))];
    match close_dart(&off, &s, Side::Left) {
        Err(DartError::FootOffLine { distance_mm, .. }) => {
            assert!((distance_mm - 5.0).abs() < 1e-9)
        }
        other => panic!("{other:?}"),
    }
    // Ligne trop courte : le pied droit (−10 ; 0) est à 10 − 5 = 5 mm de la fin de la ligne.
    let short = base_line(-5.0, 100.0);
    assert!(matches!(
        close_dart(&short, &s, Side::Left),
        Err(DartError::FootOffLine {
            side: Side::Right,
            ..
        })
    ));
    assert_eq!(close_dart(&[], &s, Side::Left), Err(DartError::EmptyLine));
}

#[test]
fn ligne_courbe_longueur_conservee() {
    // Arc de cercle de rayon 1000 centré (0 ; −1000), de −100 à +100 en x environ : les pieds
    // sont sur l'arc ; la longueur après = longueur avant − arc entre les pieds.
    let center = p(0.0, -1000.0);
    let half = (100.0_f64 / 1000.0).asin();
    let arc = Arc {
        center,
        radius_mm: 1000.0,
        start_rad: std::f64::consts::FRAC_PI_2 + half,
        sweep_rad: -2.0 * half,
    };
    // Pince dont les pieds sont sur l'arc : pieds (±10 ; y) avec y = −1000 + √(1000² − 10²).
    let y = -1000.0 + (1000.0_f64 * 1000.0 - 100.0).sqrt();
    let apex = p(0.0, y + 100.0);
    let s = dart_shape(apex, p(0.0, y), 100.0, 20.0).unwrap();
    let r = close_dart(&[EdgeGeom::Arc(arc)], &s, Side::Left).unwrap();
    let removed = 1000.0 * 2.0 * (10.0_f64 / 1000.0).asin();
    assert!((r.removed_mm - removed).abs() < 1e-6, "{}", r.removed_mm);
    assert!((r.before_mm - 1000.0 * 2.0 * half).abs() < 1e-6);
    assert!((r.after_mm - (r.before_mm - removed)).abs() < 1e-6);
    assert!(r.corner_deg.is_some());
}

fn rect() -> Vec<EdgeGeom> {
    let v = [
        p(-100.0, 0.0),
        p(100.0, 0.0),
        p(100.0, 200.0),
        p(-100.0, 200.0),
    ];
    (0..4)
        .map(|i| EdgeGeom::Line(v[i], v[(i + 1) % 4]))
        .collect()
}

#[test]
fn jambe_qui_traverse_le_contour() {
    let s = reference();
    assert_eq!(leg_crossing_contour(&rect(), &s), None);
    // Fente de 3 à 4 mm de x, de y = 50 à 200 : la jambe gauche (0 ; 100) → (10 ; 0) la coupe.
    let slit = [
        p(-100.0, 0.0),
        p(100.0, 0.0),
        p(100.0, 200.0),
        p(4.0, 200.0),
        p(4.0, 50.0),
        p(3.0, 50.0),
        p(3.0, 200.0),
        p(-100.0, 200.0),
    ];
    let chain: Vec<EdgeGeom> = (0..slit.len())
        .map(|i| EdgeGeom::Line(slit[i], slit[(i + 1) % slit.len()]))
        .collect();
    assert_eq!(leg_crossing_contour(&chain, &s), Some(Side::Left));
    assert!((distance_to_chain_mm(&rect(), p(0.0, 100.0)) - 100.0).abs() < 1e-9);
    assert!(distance_to_chain_mm(&[], p(0.0, 0.0)).is_infinite());
}

// ----- US-07.2 : transfert ---------------------------------------------------------------------
// Rectangle `rect()` (le bas y = 0 porte la base), jambe gauche fixe : le pied mobile est
// (−10 ; 0), la rotation de fermeture est θ anti-horaire (cos = 99/101, sin = 20/101) autour de A.

fn contour_starting_at_base() -> Vec<EdgeGeom> {
    // `rect()` commence par le bord bas : la ligne qui porte la base est la première arête.
    rect()
}

#[test]
fn transfert_total_sommets_calcules_a_la_main() {
    let s = reference();
    let t = transfer_dart(
        &contour_starting_at_base(),
        1,
        &s,
        Side::Left,
        p(0.0, 200.0),
        1.0,
    )
    .unwrap();
    assert!((t.rotation_deg - THETA_DEG).abs() < 1e-9);
    assert!(t.remaining.is_none());
    let starts: Vec<PointMm> = t.edges.iter().map(|e| e.geom.start()).collect();
    let expected = [
        (10.0, 0.0),
        (100.0, 0.0),
        (100.0, 200.0),
        (0.0, 200.0),
        (-2000.0 / 101.0, 100.0 + 9900.0 / 101.0),
        (-11900.0 / 101.0, 100.0 + 7900.0 / 101.0),
        (-7900.0 / 101.0, 100.0 - 11900.0 / 101.0),
    ];
    assert_eq!(starts.len(), expected.len());
    for (a, (x, y)) in starts.iter().zip(expected) {
        close_to(*a, x, y);
    }
    assert_eq!(t.edges[3].part, TransferPart::NewBase);
    // Fermeture exacte : l'arête tournée finit sur le pied fixe.
    close_to(t.edges[6].geom.end(), 10.0, 0.0);
    // Corde de la nouvelle base = 2 · 100 · sin(θ/2) = 20 / √1,01.
    assert!((t.edges[3].geom.length_mm() - 20.0 / 1.01f64.sqrt()).abs() < 1e-9);
    close_to(t.new_fixed_foot, 0.0, 200.0);
    close_to(t.new_moving_foot, -2000.0 / 101.0, 100.0 + 9900.0 / 101.0);
    let _ = (C, S);
}

#[test]
fn transfert_partiel_tourne_de_la_part_de_l_angle_et_garde_une_base() {
    let s = reference();
    let t = transfer_dart(
        &contour_starting_at_base(),
        1,
        &s,
        Side::Left,
        p(0.0, 200.0),
        0.5,
    )
    .unwrap();
    assert!((t.rotation_deg - THETA_DEG / 2.0).abs() < 1e-9);
    let (fixed, moved) = t.remaining.expect("base restante");
    // Pied tourné de φ = atan(0,1) : (−10 ; −100) → (0 ; −101/√1,01) autour de A.
    close_to(fixed, 10.0, 0.0);
    close_to(moved, 0.0, 100.0 - 101.0 / 1.01f64.sqrt());
    assert_eq!(t.edges.len(), 8);
    assert_eq!(t.edges[7].part, TransferPart::RemainingBase);
}

#[test]
fn transfert_refus_cible_part_et_degenere() {
    let s = reference();
    let chain = contour_starting_at_base();
    let go = |target, part| transfer_dart(&chain, 1, &s, Side::Left, target, part);
    assert_eq!(go(p(0.0, 200.0), 0.0), Err(DartError::Fraction(0.0)));
    assert_eq!(go(p(0.0, 200.0), 1.5), Err(DartError::Fraction(1.5)));
    assert!(matches!(
        go(p(0.0, 200.0), f64::NAN),
        Err(DartError::Fraction(_))
    ));
    assert_eq!(go(p(0.0, 0.0), 1.0), Err(DartError::TargetOnBase));
    assert_eq!(go(p(10.0, 0.0), 1.0), Err(DartError::TargetOnBase));
    assert!(matches!(
        go(p(0.0, 100.0), 1.0),
        Err(DartError::TargetOffContour { .. })
    ));
    assert_eq!(go(p(f64::NAN, 0.0), 1.0), Err(DartError::NonFinite));
    assert_eq!(
        transfer_dart(&chain, 0, &s, Side::Left, p(0.0, 200.0), 1.0),
        Err(DartError::EmptyLine)
    );
}

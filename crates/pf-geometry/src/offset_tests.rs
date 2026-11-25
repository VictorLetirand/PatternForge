//! Tests du décalage de chaîne (US-02.10).

use super::*;
use crate::Spline;
use std::f64::consts::{PI, TAU};

fn p(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn lines(points: &[PointMm], closed: bool) -> Vec<Trace> {
    let n = points.len();
    let count = if closed { n } else { n - 1 };
    (0..count)
        .map(|i| Trace::Line(points[i], points[(i + 1) % n]))
        .collect()
}

fn round() -> OffsetOptions {
    OffsetOptions {
        corner: CornerStyle::Round,
        ..OffsetOptions::default()
    }
}

fn miter() -> OffsetOptions {
    OffsetOptions::default()
}

/// Longueur d'une pièce (Bézier : polyligne très fine).
fn len(t: &Trace) -> f64 {
    match t {
        Trace::Line(a, b) => distance_mm(*a, *b),
        Trace::Arc(a) => a.length_mm(),
        Trace::Circle(c) => c.circumference_mm(),
        Trace::Cubic(c) => poly_len(&c.sample(4000)),
        Trace::Spline(s) => poly_len(&s.sample(2000)),
    }
}

fn poly_len(pts: &[PointMm]) -> f64 {
    pts.windows(2).map(|w| distance_mm(w[0], w[1])).sum()
}

fn total(ts: &[Trace]) -> f64 {
    ts.iter().map(len).sum()
}

/// Polyligne dense d'une pièce (référence indépendante des méthodes testées).
fn dense(t: &Trace, per_mm: f64) -> Vec<PointMm> {
    let n = ((len(t) * per_mm).ceil() as usize).clamp(8, 200_000);
    match t {
        Trace::Line(a, b) => (0..=n).map(|i| a.lerp(*b, i as f64 / n as f64)).collect(),
        Trace::Arc(a) => (0..=n)
            .map(|i| {
                let ang = a.start_rad + a.sweep_rad * i as f64 / n as f64;
                p(
                    a.center.x + a.radius_mm * ang.cos(),
                    a.center.y + a.radius_mm * ang.sin(),
                )
            })
            .collect(),
        Trace::Cubic(c) => (0..=n).map(|i| c.point_at(i as f64 / n as f64)).collect(),
        Trace::Spline(s) => s.sample(n / s.segments().len() + 1),
        Trace::Circle(c) => (0..=n)
            .map(|i| {
                let a = TAU * i as f64 / n as f64;
                p(
                    c.center.x + c.radius_mm * a.cos(),
                    c.center.y + c.radius_mm * a.sin(),
                )
            })
            .collect(),
    }
}

/// Distance d'un point à une polyligne, calculée ici (indépendante de la bibliothèque).
fn dist_poly(q: PointMm, poly: &[PointMm]) -> f64 {
    let mut best = f64::INFINITY;
    for w in poly.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (ab, aq) = (b - a, q - a);
        let l2 = ab.x * ab.x + ab.y * ab.y;
        let t = if l2 > 0.0 {
            ((aq.x * ab.x + aq.y * ab.y) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let foot = p(a.x + ab.x * t, a.y + ab.y * t);
        best = best.min(((q.x - foot.x).powi(2) + (q.y - foot.y).powi(2)).sqrt());
    }
    best
}

/// Écart maximal `| dist(q, original) − |d| |` sur tous les points du résultat.
fn worst_distance_error(original: &[Trace], result: &[Trace], d: f64) -> f64 {
    let reference: Vec<Vec<PointMm>> = original.iter().map(|t| dense(t, 40.0)).collect();
    let mut worst: f64 = 0.0;
    for t in result {
        for q in dense(t, 4.0) {
            let dist = reference
                .iter()
                .map(|poly| dist_poly(q, poly))
                .fold(f64::INFINITY, f64::min);
            worst = worst.max((dist - d.abs()).abs());
        }
    }
    worst
}

fn start(t: &Trace) -> PointMm {
    match t {
        Trace::Line(a, _) => *a,
        Trace::Arc(a) => a.start_point(),
        Trace::Cubic(c) => c.p0,
        _ => unreachable!(),
    }
}

fn end(t: &Trace) -> PointMm {
    match t {
        Trace::Line(_, b) => *b,
        Trace::Arc(a) => a.end_point(),
        Trace::Cubic(c) => c.p3,
        _ => unreachable!(),
    }
}

fn rect(w: f64, h: f64) -> Vec<Trace> {
    lines(&[p(0.0, 0.0), p(w, 0.0), p(w, h), p(0.0, h)], true)
}

// ---------------------------------------------------------------- exactitude

#[test]
fn rectangle_offset_exterieur_est_un_rectangle_exact() {
    // Sens anti-horaire : la gauche est l'intérieur, d < 0 = extérieur.
    let out = offset_chain(&rect(100.0, 50.0), -5.0, &miter()).unwrap();
    assert_eq!(out.len(), 4, "quatre segments, coins prolongés");
    assert!(out.iter().all(|t| matches!(t, Trace::Line(..))));
    let starts: Vec<PointMm> = out.iter().map(start).collect();
    for e in [p(-5.0, -5.0), p(105.0, -5.0), p(105.0, 55.0), p(-5.0, 55.0)] {
        assert!(
            starts.iter().any(|s| distance_mm(*s, e) < 1e-9),
            "sommet {e:?} absent de {starts:?}"
        );
    }
    assert!((total(&out) - 340.0).abs() < 1e-9);
}

#[test]
fn rectangle_offset_interieur_est_un_rectangle_exact() {
    let out = offset_chain(&rect(100.0, 50.0), 5.0, &miter()).unwrap();
    assert_eq!(out.len(), 4);
    let starts: Vec<PointMm> = out.iter().map(start).collect();
    for e in [p(5.0, 5.0), p(95.0, 5.0), p(95.0, 45.0), p(5.0, 45.0)] {
        assert!(starts.iter().any(|s| distance_mm(*s, e) < 1e-9));
    }
    assert!((total(&out) - 260.0).abs() < 1e-9);
}

#[test]
fn rectangle_offset_exterieur_arrondi_a_des_arcs_de_raccord() {
    let out = offset_chain(&rect(100.0, 50.0), -5.0, &round()).unwrap();
    let arcs: Vec<&Arc> = out
        .iter()
        .filter_map(|t| if let Trace::Arc(a) = t { Some(a) } else { None })
        .collect();
    assert_eq!(arcs.len(), 4);
    for a in arcs {
        assert!((a.radius_mm - 5.0).abs() < 1e-12);
        assert!((a.sweep_deg().abs() - 90.0).abs() < 1e-9);
    }
    // périmètre = P + 2π d
    assert!((total(&out) - (300.0 + TAU * 5.0)).abs() < 1e-9);
}

#[test]
fn cercle_seul_perimetre_exact() {
    let c = Trace::Circle(Circle::new(p(3.0, 4.0), 30.0).unwrap());
    for (d, r) in [(5.0, 25.0), (-5.0, 35.0), (29.0, 1.0), (-1000.0, 1030.0)] {
        let out = offset_chain(std::slice::from_ref(&c), d, &round()).unwrap();
        assert_eq!(out.len(), 1);
        let Trace::Circle(o) = &out[0] else {
            panic!("un cercle attendu")
        };
        assert!((o.circumference_mm() - TAU * r).abs() < 1e-9);
        assert!(distance_mm(o.center, p(3.0, 4.0)) < 1e-12);
    }
    assert!(offset_chain(std::slice::from_ref(&c), 30.0, &round()).is_err());
    assert!(offset_chain(std::slice::from_ref(&c), 31.0, &round()).is_err());
}

#[test]
fn cercle_en_deux_arcs_perimetre_exact() {
    let c = Circle::new(p(0.0, 0.0), 20.0).unwrap();
    let halves = [
        Trace::Arc(Arc::from_center_angles(c.center, 20.0, 0.0, 180.0, true).unwrap()),
        Trace::Arc(Arc::from_center_angles(c.center, 20.0, 180.0, 0.0, true).unwrap()),
    ];
    for (d, r) in [(6.0, 14.0), (-6.0, 26.0)] {
        let out = offset_chain(&halves, d, &miter()).unwrap();
        assert!((total(&out) - TAU * r).abs() < 1e-9, "d = {d}");
        // Référence = polyligne (flèche de corde ≈ 6e-6 mm à 40 points/mm).
        assert!(worst_distance_error(&halves, &out, d) < 1e-4);
    }
    assert!(offset_chain(&halves, 20.0, &miter()).is_err());
}

#[test]
fn arc_decale_est_concentrique() {
    let a = Arc::from_center_angles(p(0.0, 0.0), 20.0, 0.0, 90.0, true).unwrap();
    let out = offset_chain(&[Trace::Arc(a)], 5.0, &miter()).unwrap();
    let Trace::Arc(o) = &out[0] else { panic!() };
    assert!((o.radius_mm - 15.0).abs() < 1e-12);
    assert!((o.length_mm() - 15.0 * PI / 2.0).abs() < 1e-9);
    let out = offset_chain(&[Trace::Arc(a)], -5.0, &miter()).unwrap();
    let Trace::Arc(o) = &out[0] else { panic!() };
    assert!((o.radius_mm - 25.0).abs() < 1e-12);
}

// -------------------------------------------------------------------- coins

#[test]
fn coin_exterieur_round_et_interieur_coupe() {
    // Chaîne ouverte (0,0) → (10,0) → (10,10) : virage à gauche.
    let chain = lines(&[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)], false);
    // Droite (d < 0) = extérieur du virage : arc de rayon 3 centré sur le sommet.
    let out = offset_chain(&chain, -3.0, &round()).unwrap();
    assert_eq!(out.len(), 3);
    let Trace::Arc(a) = &out[1] else {
        panic!("arc de raccord attendu, obtenu {:?}", out[1])
    };
    assert!(distance_mm(a.center, p(10.0, 0.0)) < 1e-12);
    assert!((a.radius_mm - 3.0).abs() < 1e-12);
    assert!((a.sweep_deg() - 90.0).abs() < 1e-9);
    assert!((total(&out) - (20.0 + 3.0 * PI / 2.0)).abs() < 1e-9);
    // Miter : prolongement jusqu'à (13, -3).
    let out = offset_chain(&chain, -3.0, &miter()).unwrap();
    assert_eq!(out.len(), 2);
    assert!(distance_mm(end(&out[0]), p(13.0, -3.0)) < 1e-9);
    assert!((total(&out) - 26.0).abs() < 1e-9);
    // Gauche (d > 0) = intérieur : coupe à l'intersection, aucune boucle.
    let out = offset_chain(&chain, 3.0, &round()).unwrap();
    assert_eq!(out.len(), 2);
    assert!(distance_mm(end(&out[0]), p(7.0, 3.0)) < 1e-9);
    assert!(distance_mm(start(&out[1]), p(7.0, 3.0)) < 1e-9);
    assert!((total(&out) - 14.0).abs() < 1e-9);
}

#[test]
fn pointe_trop_aigue_replie_en_biseau() {
    // Virage de `turn_deg` degrés à gauche ; 1/cos(75°) ≈ 3,86 < 4 : pointe ; 85° : biseau.
    let sharp = |turn_deg: f64| {
        let (s, c) = turn_deg.to_radians().sin_cos();
        lines(
            &[p(0.0, 0.0), p(10.0, 0.0), p(10.0 + 10.0 * c, 10.0 * s)],
            false,
        )
    };
    let out = offset_chain(&sharp(150.0), -2.0, &miter()).unwrap();
    assert_eq!(out.len(), 2, "pointe gardée : deux segments");
    let out = offset_chain(&sharp(170.0), -2.0, &miter()).unwrap();
    assert_eq!(out.len(), 3, "biseau : un segment de plus");
    // Le biseau (pièce du milieu) reste à moins de 4 × d du sommet, ici à 2 × d exactement.
    for q in [end(&out[0]), start(&out[2])] {
        assert!((distance_mm(q, p(10.0, 0.0)) - 2.0).abs() < 1e-9);
    }
    // Limite de pointe réglable : 1,0 = toujours en biseau.
    let tight = OffsetOptions {
        miter_limit: 1.0,
        ..miter()
    };
    assert_eq!(offset_chain(&sharp(150.0), -2.0, &tight).unwrap().len(), 3);
}

#[test]
fn demi_tour_donne_un_demi_cercle_en_round() {
    let chain = lines(&[p(0.0, 0.0), p(10.0, 0.0), p(0.0, 0.0001)], false);
    let out = offset_chain(&chain, -2.0, &round()).unwrap();
    assert!(out
        .iter()
        .any(|t| matches!(t, Trace::Arc(a) if a.sweep_deg().abs() > 170.0)));
}

#[test]
fn polygone_orthogonal_concave_perimetre_exact() {
    // En L, anti-horaire : 5 coins convexes, 1 concave → P ± 8 d.
    let l = lines(
        &[
            p(0.0, 0.0),
            p(60.0, 0.0),
            p(60.0, 20.0),
            p(20.0, 20.0),
            p(20.0, 60.0),
            p(0.0, 60.0),
        ],
        true,
    );
    let perimeter = 240.0;
    let out = offset_chain(&l, -4.0, &miter()).unwrap();
    assert!((total(&out) - (perimeter + 8.0 * 4.0)).abs() < 1e-9);
    let out = offset_chain(&l, 4.0, &miter()).unwrap();
    assert!((total(&out) - (perimeter - 8.0 * 4.0)).abs() < 1e-9);
    // Round vers l'extérieur : 5 quarts de cercle, et le coin concave raccourcit de 2 d.
    let out = offset_chain(&l, -4.0, &round()).unwrap();
    assert!((total(&out) - (perimeter - 2.0 * 4.0 + 5.0 * PI / 2.0 * 4.0)).abs() < 1e-9);
    assert!(worst_distance_error(&l, &out, -4.0) < 1e-6);
    let out = offset_chain(&l, 4.0, &round()).unwrap();
    assert!(worst_distance_error(&l, &out, 4.0) < 1e-6);
}

#[test]
fn contour_arrondi_aux_angles_ne_se_retourne_pas() {
    // Rectangle 100 × 60 aux coins de rayon 2, décalé de 5 vers l'intérieur : les
    // arcs (rayon décalé négatif) disparaissent, les côtés se coupent : 90 × 50.
    let r = 2.0;
    let (w, h) = (100.0, 60.0);
    let arc = |cx: f64, cy: f64, a0: f64| {
        Trace::Arc(Arc::from_center_angles(p(cx, cy), r, a0, a0 + 90.0, true).unwrap())
    };
    let chain = vec![
        Trace::Line(p(r, 0.0), p(w - r, 0.0)),
        arc(w - r, r, 270.0),
        Trace::Line(p(w, r), p(w, h - r)),
        arc(w - r, h - r, 0.0),
        Trace::Line(p(w - r, h), p(r, h)),
        arc(r, h - r, 90.0),
        Trace::Line(p(0.0, h - r), p(0.0, r)),
        arc(r, r, 180.0),
    ];
    let out = offset_chain(&chain, 5.0, &miter()).unwrap();
    assert!((total(&out) - 280.0).abs() < 1e-9, "obtenu {}", total(&out));
    assert!(out.iter().all(|t| matches!(t, Trace::Line(..))));
    assert!(worst_distance_error(&chain, &out, 5.0) < 1e-6);
    // Vers l'extérieur : arcs de rayon 7 ; périmètre exact.
    let out = offset_chain(&chain, -5.0, &miter()).unwrap();
    let base = 2.0 * (w - 2.0 * r) + 2.0 * (h - 2.0 * r) + TAU * r;
    assert!((total(&out) - (base + TAU * 5.0)).abs() < 1e-9);
}

// ------------------------------------------------------------------ courbes

fn s_curve() -> CubicBezier {
    CubicBezier::new(p(0.0, 0.0), p(40.0, 50.0), p(-20.0, -30.0), p(60.0, 10.0))
}

#[test]
fn bezier_distance_echantillonnee_a_dix_microns() {
    let chain = vec![Trace::Cubic(s_curve())];
    for d in [3.0, -3.0, 0.5, -8.0] {
        let out = offset_chain(&chain, d, &round()).unwrap();
        let worst = worst_distance_error(&chain, &out, d);
        assert!(worst < 0.01, "d = {d} : écart {worst}");
    }
}

#[test]
fn bezier_et_courbe_lisse_distance_et_continuite() {
    let spline = Spline::through_points(&[
        p(0.0, 0.0),
        p(20.0, 15.0),
        p(45.0, 5.0),
        p(70.0, 30.0),
        p(90.0, 10.0),
    ])
    .unwrap();
    let chain = vec![Trace::Spline(spline)];
    for d in [4.0, -4.0] {
        let out = offset_chain(&chain, d, &round()).unwrap();
        let worst = worst_distance_error(&chain, &out, d);
        assert!(worst < 0.01, "d = {d} : écart {worst}");
        for w in out.windows(2) {
            assert!(
                distance_mm(end(&w[0]), start(&w[1])) < 1e-6,
                "chaîne rompue"
            );
        }
    }
}

#[test]
fn chaine_mixte_segment_arc_bezier_spline() {
    let a = p(0.0, 0.0);
    let arc = Arc::from_center_angles(p(30.0, 0.0), 30.0, 180.0, 120.0, false).unwrap();
    let arc_end = arc.end_point();
    let cubic = CubicBezier::new(
        arc_end,
        arc_end + VectorMm::new(10.0, 15.0),
        p(70.0, 45.0),
        p(80.0, 30.0),
    );
    let spline = Spline::through_points(&[p(80.0, 30.0), p(95.0, 10.0), p(110.0, 25.0)]).unwrap();
    let chain = vec![
        Trace::Line(p(-20.0, 0.0), a),
        Trace::Arc(arc),
        Trace::Cubic(cubic),
        Trace::Spline(spline),
    ];
    for d in [3.0, -3.0] {
        let out = offset_chain(&chain, d, &round()).unwrap();
        assert!(worst_distance_error(&chain, &out, d) < 0.01, "d = {d}");
    }
}

#[test]
fn rayon_de_courbure_inferieur_a_d_boucles_supprimees() {
    // Courbe à forte courbure (rayon ~ 8 mm au milieu) décalée de 9 à 25 mm.
    let c = CubicBezier::new(p(0.0, 0.0), p(0.0, 30.0), p(20.0, 30.0), p(20.0, 0.0));
    let chain = vec![Trace::Cubic(c)];
    let mut found = false;
    for d in [-15.0, 15.0, -9.0, 9.0, 25.0, -25.0] {
        match offset_chain(&chain, d, &round()) {
            Ok(out) => {
                found = true;
                assert!(
                    worst_distance_error(&chain, &out, d) < 0.01,
                    "d = {d} : un morceau est à la mauvaise distance"
                );
                for w in out.windows(2) {
                    assert!(
                        distance_mm(end(&w[0]), start(&w[1])) < 1e-6,
                        "d = {d} : chaîne rompue"
                    );
                }
            }
            Err(e) => assert!(e.to_string().contains("annule"), "d = {d} : {e}"),
        }
    }
    assert!(found);
}

// ---------------------------------------------------------------- refus

#[test]
fn cas_degeneres_refuses() {
    let chain = rect(10.0, 10.0);
    for d in [
        0.0,
        f64::NAN,
        f64::INFINITY,
        -f64::INFINITY,
        1e-9,
        1e12,
        -1e300,
    ] {
        assert!(offset_chain(&chain, d, &miter()).is_err(), "d = {d}");
    }
    assert!(offset_chain(&[], 1.0, &miter()).is_err());
    // Non connexe.
    let broken = vec![
        Trace::Line(p(0.0, 0.0), p(10.0, 0.0)),
        Trace::Line(p(10.5, 0.0), p(20.0, 0.0)),
    ];
    let e = offset_chain(&broken, 1.0, &miter()).unwrap_err();
    assert!(e.to_string().contains("continue"));
    // Segment nul.
    assert!(offset_chain(&[Trace::Line(p(1.0, 1.0), p(1.0, 1.0))], 1.0, &miter()).is_err());
    // Cercle dans une chaîne.
    let c = Trace::Circle(Circle::new(p(0.0, 0.0), 5.0).unwrap());
    assert!(offset_chain(&[c.clone(), c], 1.0, &miter()).is_err());
    // Limite de pointe invalide.
    let bad = OffsetOptions {
        miter_limit: f64::NAN,
        ..miter()
    };
    assert!(offset_chain(&chain, 1.0, &bad).is_err());
}

#[test]
fn petit_contour_annule_vers_l_interieur() {
    let sq = rect(10.0, 10.0);
    let e = offset_chain(&sq, 6.0, &miter()).unwrap_err();
    assert!(e.to_string().contains("annule"), "{e}");
    assert!(offset_chain(&sq, 5.0, &miter()).is_err(), "pile au centre");
    let out = offset_chain(&sq, 4.9, &miter()).unwrap();
    assert!((total(&out) - 4.0 * 0.2).abs() < 1e-9);
    // Vers l'extérieur : toujours possible.
    assert!(offset_chain(&sq, -6.0, &miter()).is_ok());
}

#[test]
fn cote_de_la_chaine() {
    let chain = rect(10.0, 10.0);
    assert_eq!(chain_side(&chain, p(5.0, 5.0)).unwrap(), 1.0);
    assert_eq!(chain_side(&chain, p(15.0, 5.0)).unwrap(), -1.0);
    let open = lines(&[p(0.0, 0.0), p(10.0, 0.0)], false);
    assert_eq!(chain_side(&open, p(5.0, 2.0)).unwrap(), 1.0);
    assert_eq!(chain_side(&open, p(5.0, -2.0)).unwrap(), -1.0);
    assert!(chain_side(&open, p(5.0, 0.0)).is_err());
    let c = Trace::Circle(Circle::new(p(0.0, 0.0), 5.0).unwrap());
    assert_eq!(
        chain_side(std::slice::from_ref(&c), p(1.0, 0.0)).unwrap(),
        1.0
    );
    assert_eq!(chain_side(&[c], p(9.0, 0.0)).unwrap(), -1.0);
}

// ---------------------------------------------------------- campagne aléatoire

#[allow(dead_code)]
fn debug_report(chain: &[Trace], out: &[Trace], d: f64) {
    eprintln!("chaîne : {chain:?}");
    let reference: Vec<Vec<PointMm>> = chain.iter().map(|t| dense(t, 40.0)).collect();
    for (i, t) in out.iter().enumerate() {
        let mut worst: f64 = 0.0;
        for q in dense(t, 4.0) {
            let dist = reference
                .iter()
                .map(|poly| dist_poly(q, poly))
                .fold(f64::INFINITY, f64::min);
            worst = worst.max((dist - d.abs()).abs());
        }
        eprintln!("  pièce {i} : écart {worst:.6} {t:?}");
    }
}

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 11) as f64) / ((1u64 << 53) as f64)
    }
    fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.next()
    }
}

#[test]
fn campagne_polylignes_aleatoires_toujours_a_la_bonne_distance() {
    // Coins arrondis : tout point valide du résultat est exactement à |d| du tracé.
    let mut rng = Lcg(0x5EED);
    let (mut ok, mut refused) = (0, 0);
    for case in 0..90 {
        let n = 3 + (rng.next() * 6.0) as usize;
        let mut pts = Vec::new();
        let mut cur = p(0.0, 0.0);
        for _ in 0..n {
            pts.push(cur);
            let ang = rng.range(0.0, TAU);
            let l = rng.range(8.0, 40.0);
            cur = p(cur.x + l * ang.cos(), cur.y + l * ang.sin());
        }
        let closed = case % 2 == 0;
        let chain = lines(&pts, closed);
        let d = rng.range(-6.0, 6.0);
        if d.abs() < 0.5 {
            continue;
        }
        match offset_chain(&chain, d, &round()) {
            Ok(out) => {
                ok += 1;
                let worst = worst_distance_error(&chain, &out, d);
                if worst >= 0.01 {
                    debug_report(&chain, &out, d);
                }
                assert!(worst < 0.01, "cas {case} (d = {d}) : écart {worst}");
            }
            Err(_) => refused += 1,
        }
    }
    assert!(ok > 45, "trop de refus : {ok} acceptés, {refused} refusés");
}

#[test]
fn campagne_courbes_aleatoires_a_la_bonne_distance() {
    let mut rng = Lcg(0xC0FFEE);
    for case in 0..40 {
        let pts: Vec<PointMm> = (0..4 + case % 3)
            .map(|i| p(i as f64 * rng.range(10.0, 30.0), rng.range(-30.0, 30.0)))
            .collect();
        let Ok(spline) = Spline::through_points(&pts) else {
            continue;
        };
        let chain = vec![Trace::Spline(spline)];
        let d = rng.range(-12.0, 12.0);
        if d.abs() < 0.5 {
            continue;
        }
        if let Ok(out) = offset_chain(&chain, d, &round()) {
            let worst = worst_distance_error(&chain, &out, d);
            if worst >= 0.01 {
                debug_report(&chain, &out, d);
            }
            assert!(worst < 0.01, "cas {case} (d = {d}) : écart {worst}");
        }
    }
}

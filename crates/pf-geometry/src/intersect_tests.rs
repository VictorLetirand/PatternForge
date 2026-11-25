//! Tests des intersections générales (US-02.4).

use super::*;

fn p(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Arche symétrique : sommet (5 ; 7,5) en t = 0,5 ; y(t) = 30 t (1 − t).
fn arch() -> CubicBezier {
    CubicBezier::new(p(0.0, 0.0), p(0.0, 10.0), p(10.0, 10.0), p(10.0, 0.0))
}

fn arch_flipped() -> CubicBezier {
    CubicBezier::new(p(0.0, 15.0), p(0.0, 5.0), p(10.0, 5.0), p(10.0, 15.0))
}

/// Courbe en S : coupe y = 5 en trois points.
fn s_curve() -> CubicBezier {
    CubicBezier::new(p(0.0, 0.0), p(30.0, 40.0), p(-20.0, -30.0), p(10.0, 10.0))
}

fn near(a: PointMm, b: PointMm) -> bool {
    distance_mm(a, b) < 1e-9
}

// --- Référence par échantillonnage dense -----------------------------

fn polyline(t: &Trace) -> Vec<PointMm> {
    match t {
        Trace::Line(a, b) => vec![*a, *b],
        Trace::Cubic(c) => c.sample(1500),
        Trace::Spline(s) => s.sample(300),
        Trace::Arc(a) => a.sample(1500),
        Trace::Circle(c) => (0..=1500)
            .map(|i| {
                let a = TAU * i as f64 / 1500.0;
                p(
                    c.center.x + c.radius_mm * a.cos(),
                    c.center.y + c.radius_mm * a.sin(),
                )
            })
            .collect(),
    }
}

fn reference(a: &Trace, b: &Trace) -> Vec<PointMm> {
    let (pa, pb) = (polyline(a), polyline(b));
    let mut out: Vec<PointMm> = Vec::new();
    for w in pa.windows(2) {
        for z in pb.windows(2) {
            if distance_mm(w[0], w[1]) < 1e-12
                || distance_mm(z[0], z[1]) < 1e-12
                || w[0].x.max(w[1].x) < z[0].x.min(z[1].x) - 1e-6
                || z[0].x.max(z[1].x) < w[0].x.min(w[1].x) - 1e-6
                || w[0].y.max(w[1].y) < z[0].y.min(z[1].y) - 1e-6
                || z[0].y.max(z[1].y) < w[0].y.min(w[1].y) - 1e-6
            {
                continue;
            }
            if let Ok(r) = line_line(w[0], w[1], z[0], z[1]) {
                for (_, _, q) in r {
                    if out.iter().all(|k| distance_mm(*k, q) > 1e-3) {
                        out.push(q);
                    }
                }
            }
        }
    }
    out
}

/// Les solutions et la référence se correspondent dans les deux sens.
fn assert_matches_reference(a: &Trace, b: &Trace) -> Vec<Hit> {
    let hits = intersect(a, b).unwrap();
    let refs = reference(a, b);
    for r in &refs {
        assert!(
            hits.iter().any(|h| distance_mm(h.point, *r) < 2e-2),
            "référence {r:?} sans solution ; solutions {hits:?}"
        );
    }
    for h in &hits {
        assert!(
            refs.iter().any(|r| distance_mm(h.point, *r) < 2e-2),
            "solution {h:?} sans référence ; références {refs:?}"
        );
    }
    assert_eq!(hits.len(), refs.len(), "{hits:?} / {refs:?}");
    hits
}

fn on_both(a: &Trace, b: &Trace, h: &Hit) {
    let at = |t: &Trace, u: f64| -> PointMm {
        match t {
            Trace::Line(x, y) => x.lerp(*y, u),
            Trace::Cubic(c) => c.point_at(u),
            Trace::Spline(s) => {
                let n = s.segments().len() as f64;
                let i = ((u * n).floor()).min(n - 1.0);
                s.segments()[i as usize].point_at(u * n - i)
            }
            Trace::Arc(x) => x.point_at_length(u * x.length_mm()).unwrap().0,
            Trace::Circle(c) => p(
                c.center.x + c.radius_mm * (u * TAU).cos(),
                c.center.y + c.radius_mm * (u * TAU).sin(),
            ),
        }
    };
    assert!(distance_mm(at(a, h.t1), h.point) < 1e-8, "t1 {h:?}");
    assert!(distance_mm(at(b, h.t2), h.point) < 1e-8, "t2 {h:?}");
}

// --- Segments --------------------------------------------------------

#[test]
fn segments_croisement_extremite_commune_parallele_et_superposition() {
    let l = |a, b, c, d| intersect(&Trace::Line(a, b), &Trace::Line(c, d));
    let h = l(p(0.0, 0.0), p(10.0, 10.0), p(0.0, 10.0), p(10.0, 0.0)).unwrap();
    assert_eq!(h.len(), 1);
    assert!(near(h[0].point, p(5.0, 5.0)) && (h[0].t1 - 0.5).abs() < 1e-12);
    // Extrémité commune, et T (extrémité sur le milieu de l'autre).
    let h = l(p(0.0, 0.0), p(10.0, 0.0), p(10.0, 0.0), p(10.0, 5.0)).unwrap();
    assert_eq!(h.len(), 1);
    assert!(near(h[0].point, p(10.0, 0.0)));
    let h = l(p(0.0, 0.0), p(10.0, 0.0), p(4.0, 0.0), p(4.0, 5.0)).unwrap();
    assert!(h.len() == 1 && near(h[0].point, p(4.0, 0.0)));
    // Ne se touchent pas (les droites se couperaient).
    assert!(l(p(0.0, 0.0), p(1.0, 0.0), p(5.0, -1.0), p(5.0, 1.0))
        .unwrap()
        .is_empty());
    // Parallèles distincts.
    assert!(l(p(0.0, 0.0), p(10.0, 0.0), p(0.0, 1.0), p(10.0, 1.0))
        .unwrap()
        .is_empty());
    // Colinéaires : disjoints, contact par un bout, superposés.
    assert!(l(p(0.0, 0.0), p(1.0, 0.0), p(2.0, 0.0), p(3.0, 0.0))
        .unwrap()
        .is_empty());
    let h = l(p(0.0, 0.0), p(1.0, 0.0), p(1.0, 0.0), p(3.0, 0.0)).unwrap();
    assert!(h.len() == 1 && near(h[0].point, p(1.0, 0.0)));
    assert_eq!(
        l(p(0.0, 0.0), p(2.0, 0.0), p(1.0, 0.0), p(3.0, 0.0)),
        Err(GeometryError::OverlappingTraces)
    );
    // Segment nul : refusé.
    assert!(l(p(0.0, 0.0), p(0.0, 0.0), p(0.0, 1.0), p(1.0, 1.0)).is_err());
}

// --- Cercles et arcs -------------------------------------------------

#[test]
fn cercle_arc_segments_bornes_et_tangence() {
    let c = Circle::new(p(0.0, 0.0), 5.0).unwrap();
    // Le segment [0, 10] x {3} coupe le cercle en (4 ; 3) seulement : (-4 ; 3) est hors segment.
    let h = intersect(&Trace::Line(p(0.0, 3.0), p(10.0, 3.0)), &Trace::Circle(c)).unwrap();
    assert_eq!(h.len(), 1);
    assert!(near(h[0].point, p(4.0, 3.0)));
    // Tangente (0 ; 5) : une solution.
    let h = intersect(&Trace::Line(p(-3.0, 5.0), p(3.0, 5.0)), &Trace::Circle(c)).unwrap();
    assert!(h.len() == 1 && near(h[0].point, p(0.0, 5.0)));
    // Arc 0..90° : la ligne y = 3 ne le coupe qu'en (4 ; 3).
    let arc = Arc::from_center_angles(p(0.0, 0.0), 5.0, 0.0, 90.0, true).unwrap();
    let line = Trace::Line(p(-10.0, 3.0), p(10.0, 3.0));
    let h = intersect(&line, &Trace::Arc(arc)).unwrap();
    assert!(h.len() == 1 && near(h[0].point, p(4.0, 3.0)));
    // Deux cercles sécants, tangents, disjoints, confondus.
    let c2 = Circle::new(p(6.0, 0.0), 5.0).unwrap();
    let h = intersect(&Trace::Circle(c), &Trace::Circle(c2)).unwrap();
    assert_eq!(h.len(), 2);
    let tang = Circle::new(p(8.0, 0.0), 3.0).unwrap();
    let h = intersect(&Trace::Circle(c), &Trace::Circle(tang)).unwrap();
    assert!(h.len() == 1 && near(h[0].point, p(5.0, 0.0)));
    let far = Circle::new(p(50.0, 0.0), 3.0).unwrap();
    assert!(intersect(&Trace::Circle(c), &Trace::Circle(far))
        .unwrap()
        .is_empty());
    assert_eq!(
        intersect(&Trace::Circle(c), &Trace::Circle(c)),
        Err(GeometryError::OverlappingTraces)
    );
    // Arcs du même cercle : disjoints = rien ; contact = extrémité ; recouvrement = erreur.
    let a1 = Arc::from_center_angles(p(0.0, 0.0), 5.0, 0.0, 90.0, true).unwrap();
    let a2 = Arc::from_center_angles(p(0.0, 0.0), 5.0, 180.0, 270.0, true).unwrap();
    assert!(intersect(&Trace::Arc(a1), &Trace::Arc(a2))
        .unwrap()
        .is_empty());
    let a3 = Arc::from_center_angles(p(0.0, 0.0), 5.0, 90.0, 200.0, true).unwrap();
    let h = intersect(&Trace::Arc(a1), &Trace::Arc(a3)).unwrap();
    assert!(h.len() == 1 && near(h[0].point, p(0.0, 5.0)), "{h:?}");
    let a4 = Arc::from_center_angles(p(0.0, 0.0), 5.0, 45.0, 135.0, true).unwrap();
    assert_eq!(
        intersect(&Trace::Arc(a1), &Trace::Arc(a4)),
        Err(GeometryError::OverlappingTraces)
    );
    // Un arc et le cercle entier dont il est tiré : superposés.
    assert_eq!(
        intersect(&Trace::Arc(a1), &Trace::Circle(c)),
        Err(GeometryError::OverlappingTraces)
    );
}

// --- Bézier / segment ------------------------------------------------

#[test]
fn bezier_segment_solutions_exactes_multiples_tangence_et_aucune() {
    // y = c coupe l'arche en t = (1 ± sqrt(1 − 4c/30)) / 2, valeurs analytiques.
    let line = |y: f64| Trace::Line(p(-5.0, y), p(15.0, y));
    let c = 3.0;
    let disc = (1.0_f64 - 4.0 * c / 30.0).sqrt();
    let h = intersect(&Trace::Cubic(arch()), &line(c)).unwrap();
    assert_eq!(h.len(), 2);
    for (hit, t) in h.iter().zip([(1.0 - disc) / 2.0, (1.0 + disc) / 2.0]) {
        assert!((hit.t1 - t).abs() < 1e-12, "{hit:?}");
        assert!(near(hit.point, arch().point_at(t)));
        assert!((hit.point.y - c).abs() < 1e-9);
    }
    // Tangence au sommet : une solution, exacte.
    let h = intersect(&Trace::Cubic(arch()), &line(7.5)).unwrap();
    assert_eq!(h.len(), 1);
    assert!(near(h[0].point, p(5.0, 7.5)) && (h[0].t1 - 0.5).abs() < 1e-12);
    // Au-dessus : aucune.
    assert!(intersect(&Trace::Cubic(arch()), &line(7.6))
        .unwrap()
        .is_empty());
    // Segment trop court : borné, pas de prolongement.
    let short = Trace::Line(p(-5.0, 3.0), p(0.5, 3.0));
    assert_eq!(intersect(&Trace::Cubic(arch()), &short).unwrap().len(), 1);
    // Courbe en S : trois croisements, comparés à la référence dense.
    let sl = Trace::Line(p(-50.0, 5.0), p(50.0, 5.0));
    let hits = assert_matches_reference(&Trace::Cubic(s_curve()), &sl);
    assert_eq!(hits.len(), 3, "{hits:?}");
    for h in &hits {
        assert!((h.point.y - 5.0).abs() < 1e-9);
        on_both(&Trace::Cubic(s_curve()), &sl, h);
    }
    // Extrémités de la courbe sur le segment : (0, 0) et (10, 0).
    let base = Trace::Line(p(0.0, 0.0), p(10.0, 0.0));
    let h = intersect(&Trace::Cubic(arch()), &base).unwrap();
    assert_eq!(h.len(), 2, "{h:?}");
    assert!(near(h[0].point, p(0.0, 0.0)) && near(h[1].point, p(10.0, 0.0)));
    // Bézier aplatie superposée au segment : erreur.
    let flat = CubicBezier::new(p(0.0, 0.0), p(3.0, 0.0), p(6.0, 0.0), p(9.0, 0.0));
    assert_eq!(
        intersect(&Trace::Cubic(flat), &base),
        Err(GeometryError::OverlappingTraces)
    );
}

// --- Bézier / cercle -------------------------------------------------

#[test]
fn bezier_cercle_tangence_et_solutions_multiples() {
    // Cercle au-dessus du sommet : tangent extérieurement en (5 ; 7,5).
    let top = Circle::new(p(5.0, 10.0), 2.5).unwrap();
    let h = intersect(&Trace::Cubic(arch()), &Trace::Circle(top)).unwrap();
    assert_eq!(h.len(), 1);
    assert!(near(h[0].point, p(5.0, 7.5)), "{h:?}");
    // Cercle centré sur l'arche : plusieurs croisements (référence dense).
    let big = Circle::new(p(5.0, 0.0), 6.0).unwrap();
    let hits = assert_matches_reference(&Trace::Cubic(arch()), &Trace::Circle(big));
    assert_eq!(hits.len(), 2);
    for h in &hits {
        assert!((distance_mm(h.point, big.center) - 6.0).abs() < 1e-9);
        on_both(&Trace::Cubic(arch()), &Trace::Circle(big), h);
    }
    // Courbe en S et cercle : plusieurs solutions.
    let c = Circle::new(p(5.0, 5.0), 8.0).unwrap();
    let hits = assert_matches_reference(&Trace::Cubic(s_curve()), &Trace::Circle(c));
    assert!(hits.len() >= 2, "{hits:?}");
    // Arc : seules les solutions du secteur.
    let arc = Arc::from_center_angles(p(5.0, 0.0), 6.0, 0.0, 90.0, true).unwrap();
    let hits = assert_matches_reference(&Trace::Cubic(arch()), &Trace::Arc(arc));
    assert_eq!(hits.len(), 1);
    // Aucune.
    let away = Circle::new(p(100.0, 100.0), 2.0).unwrap();
    assert!(intersect(&Trace::Cubic(arch()), &Trace::Circle(away))
        .unwrap()
        .is_empty());
}

// --- Bézier / Bézier -------------------------------------------------

#[test]
fn bezier_bezier_croisements_tangence_extremite_commune_et_superposition() {
    // Arche et son double abaissé : se croisent 2 fois.
    let lowered = CubicBezier::new(p(0.0, 12.0), p(0.0, 2.0), p(10.0, 2.0), p(10.0, 12.0));
    let (a, b) = (Trace::Cubic(arch()), Trace::Cubic(lowered));
    let hits = assert_matches_reference(&a, &b);
    assert_eq!(hits.len(), 2);
    for h in &hits {
        on_both(&a, &b, h);
    }
    // Tangence : arche et symétrique se touchent en (5 ; 7,5).
    let flipped = Trace::Cubic(arch_flipped());
    let h = intersect(&a, &flipped).unwrap();
    assert_eq!(h.len(), 1, "{h:?}");
    assert!(near(h[0].point, p(5.0, 7.5)), "{:?}", h[0].point);
    // Aucune.
    let far = CubicBezier::new(p(0.0, 20.0), p(0.0, 30.0), p(10.0, 30.0), p(10.0, 20.0));
    assert!(intersect(&a, &Trace::Cubic(far)).unwrap().is_empty());
    // Extrémité commune : deux courbes qui partent du même point.
    let up = CubicBezier::new(p(0.0, 0.0), p(5.0, 5.0), p(10.0, 5.0), p(20.0, 0.0));
    let h = intersect(&a, &Trace::Cubic(up)).unwrap();
    assert!(h.iter().any(|x| near(x.point, p(0.0, 0.0))), "{h:?}");
    // Superposées : erreur, sans calcul interminable.
    assert_eq!(intersect(&a, &a), Err(GeometryError::OverlappingTraces));
}

// --- Courbe par points de passage -----------------------------------

#[test]
fn courbe_lisse_avec_boucle_et_recoupements_multiples() {
    // Boucle : la courbe se recoupe elle-même ; une droite la traverse en plusieurs points.
    let pts = [
        p(0.0, 0.0),
        p(20.0, 20.0),
        p(0.0, 20.0),
        p(20.0, 0.0),
        p(40.0, 10.0),
    ];
    let spline = Spline::through_points(&pts).unwrap();
    let line = Trace::Line(p(-10.0, 10.0), p(50.0, 10.0));
    let s = Trace::Spline(spline.clone());
    let hits = assert_matches_reference(&s, &line);
    assert!(hits.len() >= 3, "{hits:?}");
    for h in &hits {
        on_both(&s, &line, h);
    }
    // Auto-recoupement : les deux moitiés de la courbe se coupent au moins une fois.
    let first = Spline::through_points(&pts[..3]).unwrap();
    let second = Spline::through_points(&pts[2..]).unwrap();
    let _ = assert_matches_reference(&Trace::Spline(first), &Trace::Spline(second));
    // Deux ondulations : plusieurs recoupements, comparés à la référence.
    let wave = |phase: usize| {
        let v: Vec<PointMm> = (0..9)
            .map(|i| {
                p(
                    i as f64 * 10.0,
                    if (i + phase) % 2 == 0 { 0.0 } else { 10.0 },
                )
            })
            .collect();
        Trace::Spline(Spline::through_points(&v).unwrap())
    };
    let hits = assert_matches_reference(&wave(0), &wave(1));
    assert!(hits.len() >= 8, "{}", hits.len());
    // Spline contre Bézier et cercle.
    let _ = assert_matches_reference(&s, &Trace::Cubic(arch()));
    let c = Circle::new(p(20.0, 10.0), 12.0).unwrap();
    let _ = assert_matches_reference(&s, &Trace::Circle(c));
    // La courbe contre elle-même : superposition signalée.
    assert_eq!(intersect(&s, &s), Err(GeometryError::OverlappingTraces));
    // Jonction entre deux tronçons : un seul point (pas de doublon).
    let joint = spline.segments()[0].p3;
    let through = Trace::Line(p(joint.x, joint.y - 100.0), p(joint.x, joint.y + 100.0));
    let h = intersect(&s, &through).unwrap();
    let near_joint = h
        .iter()
        .filter(|x| distance_mm(x.point, joint) < 1e-6)
        .count();
    assert_eq!(near_joint, 1, "{h:?}");
}

// --- Campagnes aléatoires --------------------------------------------

struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn pt(&mut self, span: f64) -> PointMm {
        p(self.next() * span, self.next() * span)
    }
}

#[test]
fn campagne_aleatoire_contre_reference_dense() {
    let mut r = Lcg(42);
    let mut compared = 0;
    for _ in 0..40 {
        let cubic = |r: &mut Lcg| {
            Trace::Cubic(CubicBezier::new(
                r.pt(100.0),
                r.pt(100.0),
                r.pt(100.0),
                r.pt(100.0),
            ))
        };
        let (a, b) = (cubic(&mut r), cubic(&mut r));
        let line = Trace::Line(r.pt(100.0), r.pt(100.0));
        let circle = Trace::Circle(Circle::new(r.pt(100.0), 5.0 + 40.0 * r.next()).unwrap());
        let pts: Vec<PointMm> = (0..5).map(|_| r.pt(100.0)).collect();
        let spline = Trace::Spline(Spline::through_points(&pts).unwrap());
        for (x, y) in [
            (&a, &b),
            (&a, &line),
            (&a, &circle),
            (&spline, &b),
            (&spline, &line),
            (&spline, &circle),
        ] {
            let hits = assert_matches_reference(x, y);
            for h in &hits {
                on_both(x, y, h);
            }
            compared += hits.len();
        }
    }
    assert!(compared > 100, "campagne trop pauvre : {compared}");
}

#[test]
fn coordonnees_extremes_et_valeurs_invalides() {
    // Très loin de l'origine (9e5 mm) : précision sous 1e-6 mm.
    let off = 9.0e5;
    let sh = |c: CubicBezier| {
        CubicBezier::new(
            p(c.p0.x + off, c.p0.y + off),
            p(c.c1.x + off, c.c1.y + off),
            p(c.c2.x + off, c.c2.y + off),
            p(c.p3.x + off, c.p3.y + off),
        )
    };
    let h = intersect(
        &Trace::Cubic(sh(arch())),
        &Trace::Line(p(off - 5.0, off + 3.0), p(off + 15.0, off + 3.0)),
    )
    .unwrap();
    assert_eq!(h.len(), 2);
    let c = Circle::new(p(off + 5.0, off + 10.0), 2.5).unwrap();
    let h = intersect(&Trace::Cubic(sh(arch())), &Trace::Circle(c)).unwrap();
    assert_eq!(h.len(), 1);
    assert!(distance_mm(h[0].point, p(off + 5.0, off + 7.5)) < 1e-6);
    // Très petit.
    let tiny = CubicBezier::new(p(0.0, 0.0), p(0.0, 1e-3), p(1e-3, 1e-3), p(1e-3, 0.0));
    let l = Trace::Line(p(-1.0, 3e-4), p(1.0, 3e-4));
    assert_eq!(intersect(&Trace::Cubic(tiny), &l).unwrap().len(), 2);
    // Valeurs non finies : erreur, pas de panique.
    let bad = CubicBezier::new(p(f64::NAN, 0.0), p(0.0, 1.0), p(1.0, 1.0), p(1.0, 0.0));
    assert!(intersect(&Trace::Cubic(bad), &l).is_err());
    // Aucun doublon, résultat trié.
    let hits = intersect(&Trace::Cubic(s_curve()), &Trace::Cubic(arch_flipped())).unwrap();
    for w in hits.windows(2) {
        assert!(w[0].t1 <= w[1].t1);
        assert!(distance_mm(w[0].point, w[1].point) > 1e-7);
    }
}

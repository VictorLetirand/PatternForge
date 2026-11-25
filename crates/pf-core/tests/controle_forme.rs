//! US-09.9 — contrôle de forme entre tailles : rebroussements, inflexions, courbure maximale et
//! signalements typés. Valeurs calculées **à la main**.
//!
//! Gamme `XS S M L XL`, base `M`, écarts cumulés `S (−5 ; −10)`, `L (+5 ; +10)`, `XL (+10 ; +22)`,
//! `XS (−9 ; −20)` posés sur le point d'arrivée `p3` de la courbe ; la poignée d'arrivée est un
//! décalage relatif à `p3` (elle le suit).
//!
//! - Parabole `y = x² / 200` de 0 à 60 mm (`p0 = (0 ; 0)`, `c1 = (20 ; 0)`, `c2 = (40 ; 6)`,
//!   `p3 = (60 ; 18)`) : sommet en `t = 0`, `x′ = 60`, `y″ = 36`, `κ = 60·36 / 60³ = 0,01`
//!   (rayon 100 mm), décroissante ensuite.
//! - Cubique en S `p0 = (0 ; 0)`, `c1 = (40 ; 40)`, `c2 = (60 ; −40)`, `p3 = (100 ; 0)` : symétrique
//!   par rapport à `(50 ; 0)`, une inflexion en `t = 1/2`.
//! - Rebroussement : `p0 = (0 ; 0)`, `c1 = (120 ; 80)`, `c2 = (0 ; 80)`, `p3 = (120 ; 0)` donne
//!   `B′(1/2) = 3·[(120 ; 80) + 2·(−120 ; 0) + (120 ; −80)] / 4 = (0 ; 0)` : rebroussement exact.
//!   La poignée d'arrivée est `c2 − p3 = (−120 ; 80)` : avec `XL = (+10 ; +22)` la taille de base a
//!   `p3 = (110 ; −22)` et `c2 = (−10 ; 58)`.

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;

const SIZES: [&str; 5] = ["XS", "S", "M", "L", "XL"];
/// (taille, dx, dy) cumulés depuis la base M.
const DELTAS: [(&str, f64, f64); 5] = [
    ("XS", -9.0, -20.0),
    ("S", -5.0, -10.0),
    ("M", 0.0, 0.0),
    ("L", 5.0, 10.0),
    ("XL", 10.0, 22.0),
];

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).expect("projet valide")
}

fn size(p: &Project, label: &str) -> SizeId {
    p.sizes
        .iter()
        .flat_map(|r| r.sizes.iter())
        .find(|s| s.label == label)
        .unwrap_or_else(|| panic!("taille {label}"))
        .id
}

fn point_at(p: &Project, sheet: SheetId, at: PointMm) -> PointId {
    let s = p.sheet(sheet).unwrap();
    s.points
        .iter()
        .find(|q| pf_geometry::distance_mm(s.position(q.id).unwrap(), at) < 1e-9)
        .unwrap_or_else(|| panic!("aucun point en {at:?}"))
        .id
}

struct Fx {
    p: Project,
    sheet: SheetId,
    range: SizeRangeId,
    curve: EntityId,
    /// Point d'arrivée (libre, gradué).
    end: PointId,
}

/// Une Bézier `p0 c1 c2 p3` dans la gamme `labels` (base d'indice `base`), `p3` gradué des
/// écarts `deltas` (taille, dx, dy) cumulés depuis la base.
fn bezier(labels: &[&str], base: usize, pts: [(f64, f64); 4], deltas: &[(&str, f64, f64)]) -> Fx {
    let mut p = Project::new("Contrôle de forme");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: labels.iter().map(|s| s.to_string()).collect(),
        base,
    })
    .unwrap();
    let range = p.sizes[0].id;
    let q = |i: usize| pt(pts[i].0, pts[i].1);
    let curve = p
        .apply(Command::AddCurve {
            sheet,
            start: PointRef::New(q(0)),
            c1: q(1),
            c2: q(2),
            end: PointRef::New(q(3)),
        })
        .unwrap()
        .entities[0];
    let end = point_at(&p, sheet, q(3));
    for (label, dx, dy) in deltas {
        let s = size(&p, label);
        p.apply(Command::SetPointGrading {
            sheet,
            point: end,
            size: s,
            dx_mm: *dx,
            dy_mm: *dy,
        })
        .unwrap();
    }
    Fx {
        p,
        sheet,
        range,
        curve,
        end,
    }
}

fn deltas_without_base() -> Vec<(&'static str, f64, f64)> {
    DELTAS.into_iter().filter(|t| t.0 != "M").collect()
}

fn control(fx: &Fx) -> ShapeControl {
    fx.p.shape_control(fx.sheet, fx.range).expect("contrôle")
}

fn curve_of(c: &ShapeControl, e: EntityId) -> &CurveShape {
    c.curves
        .iter()
        .find(|x| x.entity == e)
        .expect("courbe contrôlée")
}

fn metrics(c: &CurveShape, p: &Project, label: &str) -> ShapeMetrics {
    let s = size(p, label);
    *c.sizes
        .iter()
        .find(|x| x.size == s)
        .unwrap()
        .metrics()
        .unwrap_or_else(|| panic!("{label} illisible"))
}

fn signals_of(c: &CurveShape, p: &Project, label: &str) -> Vec<ShapeSignalKind> {
    let s = size(p, label);
    c.signals
        .iter()
        .filter(|x| x.size == s)
        .map(|x| x.kind)
        .collect()
}

// (i) arc sans inflexion : courbure et rayon calculés à la main ------------------------------

#[test]
fn parabola_arc_has_hand_computed_curvature_and_radius() {
    let fx = bezier(
        &SIZES,
        2,
        [(0.0, 0.0), (20.0, 0.0), (40.0, 6.0), (60.0, 18.0)],
        &[],
    );
    let c = control(&fx);
    assert_eq!(c.curves.len(), 1);
    let cv = curve_of(&c, fx.curve);
    assert_eq!(cv.kind, CurveKind::Bezier);
    assert_eq!(cv.sizes.len(), 5);
    for label in SIZES {
        let m = metrics(cv, &fx.p, label);
        assert!((m.max_curvature_per_mm - 0.01).abs() < 1e-12, "{label}");
        assert!((m.min_radius_mm.unwrap() - 100.0).abs() < 1e-9, "{label}");
        assert_eq!((m.cusps, m.inflections), (0, 0), "{label}");
    }
    // Aucun écart : pas nuls, aucun signalement.
    assert_eq!(cv.steps.len(), 4);
    for st in cv.steps.iter().flatten() {
        assert!(st.delta.abs() < 1e-12);
        assert!(st.percent.unwrap().abs() < 1e-9);
    }
    assert!(cv.is_clean());
    assert_eq!(c.flagged().count(), 0);
}

#[test]
fn straight_bezier_has_no_radius() {
    let fx = bezier(
        &SIZES,
        2,
        [(0.0, 0.0), (0.0, 0.0), (80.0, 0.0), (80.0, 0.0)],
        &[],
    );
    let c = control(&fx);
    let m = metrics(curve_of(&c, fx.curve), &fx.p, "M");
    assert!(m.max_curvature_per_mm < 1e-9);
    assert_eq!(m.min_radius_mm, None);
    assert_eq!((m.cusps, m.inflections), (0, 0));
    assert!(curve_of(&c, fx.curve).is_clean());
}

// (ii) courbe en S : une inflexion partout, aucun signalement --------------------------------

#[test]
fn s_curve_keeps_one_inflection_in_every_size() {
    // `c2 = p3 + (−40 ; −40)` : poignée d'arrivée relative, elle suit `p3` gradué.
    let fx = bezier(
        &SIZES,
        2,
        [(0.0, 0.0), (40.0, 40.0), (60.0, -40.0), (100.0, 0.0)],
        &deltas_without_base(),
    );
    let c = control(&fx);
    let cv = curve_of(&c, fx.curve);
    for label in SIZES {
        let m = metrics(cv, &fx.p, label);
        assert_eq!((m.cusps, m.inflections), (0, 1), "{label}");
        assert!(m.max_curvature_per_mm > 0.0);
    }
    assert!(cv.signals.is_empty(), "{:?}", cv.signals);
    // Écarts de courbure max entre tailles : cohérents avec les valeurs lues.
    for (i, st) in cv.steps.iter().enumerate() {
        let (a, b) = (
            cv.sizes[i].metrics().unwrap().max_curvature_per_mm,
            cv.sizes[i + 1].metrics().unwrap().max_curvature_per_mm,
        );
        let st = st.expect("pas lisible");
        assert!((st.delta - (b - a)).abs() < 1e-12);
        assert!((st.percent.unwrap() - (b - a) / a * 100.0).abs() < 1e-9);
        assert!(st.percent.unwrap().abs() < 50.0, "pas {i}");
    }
}

// (iii) rebroussement introduit par la gradation ---------------------------------------------

fn cusp_fixture() -> Fx {
    bezier(
        &SIZES,
        2,
        [(0.0, 0.0), (120.0, 80.0), (-10.0, 58.0), (110.0, -22.0)],
        &deltas_without_base(),
    )
}

#[test]
fn cusp_appears_in_xl_only_and_is_signalled_once() {
    let fx = cusp_fixture();
    // Vérification à la main : en XL, p3 = (120 ; 0) et c2 = (0 ; 80).
    let xl =
        fx.p.graded_sheet(fx.sheet, Some(size(&fx.p, "XL")))
            .unwrap();
    assert!(pf_geometry::distance_mm(xl.position(fx.end).unwrap(), pt(120.0, 0.0)) < 1e-9);
    let c = control(&fx);
    let cv = curve_of(&c, fx.curve);
    for label in ["XS", "S", "M", "L"] {
        let m = metrics(cv, &fx.p, label);
        assert_eq!((m.cusps, m.inflections), (0, 0), "{label}");
        assert!(
            !signals_of(cv, &fx.p, label)
                .iter()
                .any(|k| matches!(k, ShapeSignalKind::CuspIntroduced { .. })),
            "{label}"
        );
    }
    assert_eq!(metrics(cv, &fx.p, "XL").cusps, 1);
    assert_eq!(
        signals_of(cv, &fx.p, "XL")
            .into_iter()
            .filter(|k| matches!(k, ShapeSignalKind::CuspIntroduced { cusps: 1 }))
            .count(),
        1
    );
    let introduced: Vec<SizeId> = cv
        .signals
        .iter()
        .filter(|s| matches!(s.kind, ShapeSignalKind::CuspIntroduced { .. }))
        .map(|s| s.size)
        .collect();
    assert_eq!(introduced, vec![size(&fx.p, "XL")]);
    assert!(!cv
        .signals
        .iter()
        .any(|s| matches!(s.kind, ShapeSignalKind::InflectionChanged { .. })));
    assert_eq!(c.flagged().count(), 1);
}

#[test]
fn cusp_already_in_the_base_is_not_introduced() {
    // Base = taille du rebroussement : rien n'est « introduit » (la base est la référence).
    let mut fx = cusp_fixture();
    let xl = size(&fx.p, "XL");
    fx.p.apply(Command::SetBaseSize { size: xl }).unwrap();
    let c = control(&fx);
    let cv = curve_of(&c, fx.curve);
    assert_eq!(metrics(cv, &fx.p, "XL").cusps, 1);
    assert!(!cv
        .signals
        .iter()
        .any(|s| matches!(s.kind, ShapeSignalKind::CuspIntroduced { .. })));
}

// (iv) courbes dégénérées ---------------------------------------------------------------------

#[test]
fn zero_length_curve_in_one_size_is_degenerate_there_only() {
    // Poignées nulles (c1 = p0, c2 = p3) ; en XL, p3 rejoint p0 : tous les points sont confondus.
    let fx = bezier(
        &SIZES,
        2,
        [(0.0, 0.0), (0.0, 0.0), (50.0, 0.0), (50.0, 0.0)],
        &[("XL", -50.0, 0.0)],
    );
    let c = control(&fx);
    let cv = curve_of(&c, fx.curve);
    let xl = size(&fx.p, "XL");
    assert_eq!(
        cv.sizes.iter().find(|s| s.size == xl).unwrap().reading,
        ShapeReading::Degenerate(DegenerateReason::ZeroLength)
    );
    assert_eq!(
        signals_of(cv, &fx.p, "XL"),
        vec![ShapeSignalKind::Degenerate {
            reason: DegenerateReason::ZeroLength
        }]
    );
    for label in ["XS", "S", "M", "L"] {
        assert!(signals_of(cv, &fx.p, label).is_empty(), "{label}");
        assert_eq!(metrics(cv, &fx.p, label).cusps, 0);
    }
    // Le pas qui touche la taille illisible n'existe pas.
    assert!(cv.steps[3].is_none());
    assert!(cv.steps[..3].iter().all(Option::is_some));
}

#[test]
fn a_degenerate_base_cannot_be_written_so_the_reference_always_reads() {
    // Le modèle refuse une courbe dégénérée à l'écriture, et refuse de prendre pour base une
    // taille où la courbe l'est : la base d'une courbe contrôlée est toujours lisible.
    let mut fx = bezier(
        &SIZES,
        2,
        [(0.0, 0.0), (0.0, 0.0), (50.0, 0.0), (50.0, 0.0)],
        &[("XL", -50.0, 0.0)],
    );
    let xl = size(&fx.p, "XL");
    let before = bytes(&fx.p);
    assert!(fx.p.apply(Command::SetBaseSize { size: xl }).is_err());
    assert_eq!(bytes(&fx.p), before);
    let mut bare = Project::new("Confondue");
    let sheet = bare.sheets[0].id;
    let p = PointMm::new(5.0, 5.0);
    assert!(bare
        .apply(Command::AddCurve {
            sheet,
            start: PointRef::New(p),
            c1: p,
            c2: p,
            end: PointRef::New(p),
        })
        .is_err());
}

#[test]
fn smooth_curve_with_merged_nodes_is_unreadable_not_a_panic() {
    let mut p = Project::new("Lisse");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddSizeRange {
        name: "G".into(),
        kind: SizeKind::Alphanumeric,
        labels: vec!["S".into(), "M".into(), "L".into()],
        base: 1,
    })
    .unwrap();
    let range = p.sizes[0].id;
    let curve = p
        .apply(Command::AddSpline {
            sheet,
            points: vec![
                PointRef::New(pt(0.0, 0.0)),
                PointRef::New(pt(30.0, 20.0)),
                PointRef::New(pt(60.0, 0.0)),
            ],
        })
        .unwrap()
        .entities[0];
    let mid = point_at(&p, sheet, pt(30.0, 20.0));
    // En L, le point de passage du milieu rejoint le premier : deux nœuds consécutifs confondus.
    let l = size(&p, "L");
    p.apply(Command::SetPointGrading {
        sheet,
        point: mid,
        size: l,
        dx_mm: -30.0,
        dy_mm: -20.0,
    })
    .unwrap();
    let c = p.shape_control(sheet, range).unwrap();
    let cv = curve_of(&c, curve);
    assert_eq!(cv.kind, CurveKind::Smooth);
    assert_eq!(metrics(cv, &p, "M").cusps, 0);
    assert_eq!(
        signals_of(cv, &p, "L"),
        vec![ShapeSignalKind::Degenerate {
            reason: DegenerateReason::Unreadable
        }]
    );
    assert!(signals_of(cv, &p, "S").is_empty());
}

#[test]
fn only_curves_are_controlled_in_sheet_order() {
    let mut p = Project::new("Ordre");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddSizeRange {
        name: "G".into(),
        kind: SizeKind::Alphanumeric,
        labels: vec!["S".into(), "M".into()],
        base: 0,
    })
    .unwrap();
    let range = p.sizes[0].id;
    let seg = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(pt(0.0, 0.0)),
            to: PointRef::New(pt(10.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let spline = p
        .apply(Command::AddSpline {
            sheet,
            points: vec![
                PointRef::New(pt(0.0, 10.0)),
                PointRef::New(pt(30.0, 30.0)),
                PointRef::New(pt(60.0, 10.0)),
            ],
        })
        .unwrap()
        .entities[0];
    let bez = p
        .apply(Command::AddCurve {
            sheet,
            start: PointRef::New(pt(0.0, 50.0)),
            c1: pt(20.0, 70.0),
            c2: pt(40.0, 70.0),
            end: PointRef::New(pt(60.0, 50.0)),
        })
        .unwrap()
        .entities[0];
    let c = p.shape_control(sheet, range).unwrap();
    let ids: Vec<EntityId> = c.curves.iter().map(|x| x.entity).collect();
    assert_eq!(ids, vec![spline, bez]);
    assert!(!ids.contains(&seg));
    assert_eq!(c.sizes.len(), 2);
    assert_eq!(c.base, c.sizes[0]);
}

// (v) saut de courbure ------------------------------------------------------------------------

/// Gamme M L XL (base M), `p3` déplacé de `(0 ; dy)` : pour la parabole ci-dessus, en `t = 0`,
/// `κ = (2/3)·cross(c1 − p0, c2 − c1) / |c1 − p0|³ = (2/3)·20·(6 + dy) / 8000 = (6 + dy) / 600`
/// (c'est le maximum pour `0 ≤ dy ≤ 6`, vérifié par le test lui-même sur la valeur lue).
fn jump_fixture() -> Fx {
    bezier(
        &["M", "L", "XL"],
        0,
        [(0.0, 0.0), (20.0, 0.0), (40.0, 6.0), (60.0, 18.0)],
        &[("L", 0.0, 1.0), ("XL", 0.0, 6.0)],
    )
}

#[test]
fn curvature_jump_above_fifty_percent_is_signalled_below_is_not() {
    let fx = jump_fixture();
    let c = control(&fx);
    let cv = curve_of(&c, fx.curve);
    for (label, want) in [("M", 6.0 / 600.0), ("L", 7.0 / 600.0), ("XL", 12.0 / 600.0)] {
        let m = metrics(cv, &fx.p, label);
        assert!((m.max_curvature_per_mm - want).abs() < 1e-12, "{label}");
        assert_eq!((m.cusps, m.inflections), (0, 0), "{label}");
    }
    // M → L : +16,7 % (sous le seuil) ; L → XL : (12 − 7) / 7 = +71,43 % (au-dessus).
    assert!(signals_of(cv, &fx.p, "M").is_empty());
    assert!(signals_of(cv, &fx.p, "L").is_empty());
    let l = size(&fx.p, "L");
    let kinds = signals_of(cv, &fx.p, "XL");
    assert_eq!(kinds.len(), 1, "{kinds:?}");
    match kinds[0] {
        ShapeSignalKind::CurvatureJump { from, percent } => {
            assert_eq!(from, l);
            assert!((percent.unwrap() - 500.0 / 7.0).abs() < 1e-9);
        }
        other => panic!("{other:?}"),
    }
    let steps: Vec<f64> = cv
        .steps
        .iter()
        .map(|s| s.unwrap().percent.unwrap())
        .collect();
    assert!((steps[0] - 100.0 / 6.0).abs() < 1e-9);
    assert!((steps[1] - 500.0 / 7.0).abs() < 1e-9);
}

#[test]
fn jump_threshold_is_strictly_above_fifty_percent() {
    let m = |k: f64| ShapeMetrics {
        cusps: 0,
        inflections: 0,
        max_curvature_per_mm: k,
        min_radius_mm: Some(1.0 / k),
    };
    // 2 → 3 : exactement +50 % (valeurs exactes en binaire) : pas un saut.
    let (st, jump) = curvature_step(&m(2.0), &m(3.0));
    assert!(!jump);
    assert_eq!(st.percent, Some(50.0));
    assert_eq!(st.delta, 1.0);
    // Juste au-dessus ; et −50 % exactement / −51 %.
    assert!(curvature_step(&m(2.0), &m(3.001)).1);
    assert!(!curvature_step(&m(2.0), &m(1.0)).1);
    assert!(curvature_step(&m(2.0), &m(0.98)).1);
    assert_eq!(CURVATURE_JUMP_RATIO, 0.5);
    // Droite → droite : rien ; droite → courbe : saut sans pourcentage ; courbe → droite : −100 %.
    let flat = ShapeMetrics {
        max_curvature_per_mm: 0.0,
        min_radius_mm: None,
        ..m(1.0)
    };
    assert!(!curvature_step(&flat, &flat).1);
    let (st, jump) = curvature_step(&flat, &m(0.01));
    assert!(jump && st.percent.is_none());
    let (st, jump) = curvature_step(&m(0.01), &flat);
    assert!(jump && (st.percent.unwrap() + 100.0).abs() < 1e-9);
}

// (vi) lecture sans effet ---------------------------------------------------------------------

#[test]
fn reading_changes_nothing() {
    let fx = cusp_fixture();
    let before = bytes(&fx.p);
    let history = fx.p.clone();
    let a = control(&fx);
    let b = control(&fx);
    assert_eq!(a, b, "lecture déterministe");
    assert_eq!(bytes(&fx.p), before, "octets identiques");
    assert_eq!(bytes(&history), before);
    // Aucune donnée de forme n'est écrite dans le fichier.
    assert!(!before.contains("shape") && !before.contains("cusp"));
}

// (vii) gamme et feuille refusées -------------------------------------------------------------

#[test]
fn unknown_or_empty_range_and_unknown_sheet_are_refused() {
    let mut fx = cusp_fixture();
    assert_eq!(
        fx.p.shape_control(fx.sheet, SizeRangeId(987_654)),
        Err(ShapeControlError::UnknownRange(SizeRangeId(987_654)))
    );
    assert_eq!(
        fx.p.shape_control(SheetId(987_654), fx.range),
        Err(ShapeControlError::UnknownSheet(SheetId(987_654)))
    );
    // Un projet sans aucune gamme.
    let bare = Project::new("Sans gamme");
    assert!(matches!(
        bare.shape_control(bare.sheets[0].id, fx.range),
        Err(ShapeControlError::UnknownRange(_))
    ));
    // Gamme vidée (le modèle l'interdit par commande : état construit à la main).
    fx.p.sizes[0].sizes.clear();
    assert_eq!(
        fx.p.shape_control(fx.sheet, fx.range),
        Err(ShapeControlError::EmptyRange(fx.range))
    );
    assert!(CoreError::from(ShapeControlError::EmptyRange(fx.range))
        .to_string()
        .contains("gamme"));
}

// (viii) invariance sous changement de base --------------------------------------------------

#[test]
fn base_change_keeps_per_size_readings() {
    let fx = cusp_fixture();
    let before = control(&fx);
    for new_base in ["L", "S", "XL", "XS"] {
        let mut q = fx.clone_project();
        let s = size(&q, new_base);
        q.apply(Command::SetBaseSize { size: s }).unwrap();
        let after = q.shape_control(fx.sheet, fx.range).unwrap();
        assert_eq!(after.base, s);
        let (a, b) = (curve_of(&before, fx.curve), curve_of(&after, fx.curve));
        assert_eq!(a.sizes.len(), b.sizes.len());
        for (x, y) in a.sizes.iter().zip(&b.sizes) {
            assert_eq!(x.size, y.size);
            let (mx, my) = (x.metrics().unwrap(), y.metrics().unwrap());
            assert_eq!((mx.cusps, mx.inflections), (my.cusps, my.inflections));
            let tol = 1e-9 * mx.max_curvature_per_mm.max(1.0);
            assert!(
                (mx.max_curvature_per_mm - my.max_curvature_per_mm).abs() <= tol,
                "{new_base}"
            );
        }
        // Les sauts de courbure suivent l'ordre de la gamme : indépendants de la base.
        let jumps = |c: &CurveShape| -> Vec<(SizeId, SizeId)> {
            c.signals
                .iter()
                .filter_map(|s| match s.kind {
                    ShapeSignalKind::CurvatureJump { from, .. } => Some((s.size, from)),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(jumps(a), jumps(b), "{new_base}");
        // Le rebroussement est « introduit » seulement par rapport à une base qui n'en a pas.
        let introduced = b
            .signals
            .iter()
            .any(|s| matches!(s.kind, ShapeSignalKind::CuspIntroduced { .. }));
        assert_eq!(introduced, new_base != "XL", "{new_base}");
    }
}

impl Fx {
    fn clone_project(&self) -> Project {
        self.p.clone()
    }
}

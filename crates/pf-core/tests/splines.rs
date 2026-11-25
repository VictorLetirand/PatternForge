//! US-02.2 — courbe par points de passage : continuité, édition annulable,
//! points lisses/anguleux et tangentes, longueurs, conversions Bézier, contours,
//! crans, transformations, calques/groupes, presse-papiers, feuilles, fichier.

use pf_core::clipboard::Fragment;
use pf_core::contour::{closed_contours, measure_contour};
use pf_core::*;
use pf_geometry::{
    distance_mm, polygon_signed_area_mm2, tangent_from_polar, CubicBezier, PointMm, Side, Transform,
};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn setup() -> (Project, SheetId) {
    let p = Project::new("Courbes");
    let s = p.sheets[0].id;
    (p, s)
}

fn snapshot(p: &Project) -> String {
    serde_json::to_string(p).unwrap()
}

fn news(points: &[(f64, f64)]) -> Vec<PointRef> {
    points
        .iter()
        .map(|&(x, y)| PointRef::New(pt(x, y)))
        .collect()
}

fn spline(p: &mut Project, s: SheetId, points: &[(f64, f64)]) -> Outcome {
    p.apply(Command::AddSpline {
        sheet: s,
        points: news(points),
    })
    .unwrap()
}

const FOUR: [(f64, f64); 4] = [(0.0, 0.0), (30.0, 20.0), (60.0, -5.0), (100.0, 10.0)];

/// Écart maximal entre deux courbes échantillonnées au même paramètre.
fn max_gap(a: &CubicBezier, b: &CubicBezier) -> f64 {
    (0..=200)
        .map(|i| {
            let t = i as f64 / 200.0;
            distance_mm(a.point_at(t), b.point_at(t))
        })
        .fold(0.0, f64::max)
}

// --- CA 1 : spline cubique par morceaux, C1 ----------------------------------

#[test]
fn ca1_passe_par_les_points_et_est_c1() {
    let (mut p, s) = setup();
    let e = spline(&mut p, s, &FOUR).entities[0];
    let sh = p.sheet(s).unwrap();
    let g = sh.spline_geometry(e).unwrap();
    assert_eq!(g.segments().len(), 3);
    for (i, seg) in g.segments().iter().enumerate() {
        assert!(distance_mm(seg.p0, pt(FOUR[i].0, FOUR[i].1)) < 1e-9);
        assert!(distance_mm(seg.p3, pt(FOUR[i + 1].0, FOUR[i + 1].1)) < 1e-9);
    }
    // C1 : même vecteur vitesse de part et d'autre de chaque point interne.
    for w in g.segments().windows(2) {
        let (a, b) = (w[0].derivative_at(1.0), w[1].derivative_at(0.0));
        assert!((a.x - b.x).hypot(a.y - b.y) < 1e-9);
    }
    assert_eq!(sh.entity_name(e), "Courbe lisse A…D");
}

// --- CA 2 : ajouter / supprimer / déplacer, annulable, minimum 2 points -------

#[test]
fn ca2_ajout_suppression_deplacement_annulables() {
    let (p, s) = setup();
    let mut d = Document::new(p);
    let e = d
        .execute(Command::AddSpline {
            sheet: s,
            points: news(&FOUR[..3]),
        })
        .unwrap()
        .entities[0];
    let nodes = |d: &Document| d.project().sheet(s).unwrap().spline_nodes(e).unwrap();
    let s0 = snapshot(d.project());

    // Ajout au milieu.
    let o = d
        .execute(Command::AddSplinePoint {
            sheet: s,
            spline: e,
            index: 1,
            point: PointRef::New(pt(15.0, 15.0)),
        })
        .unwrap();
    assert_eq!(nodes(&d).len(), 4);
    assert_eq!(nodes(&d)[1].point, o.points[0]);
    let s1 = snapshot(d.project());

    // Déplacement d'un point de passage (point libre).
    let moved = nodes(&d)[2].point;
    let before = d.project().sheet(s).unwrap().spline_geometry(e).unwrap();
    d.execute(Command::MovePoint {
        sheet: s,
        point: moved,
        to: pt(35.0, 40.0),
    })
    .unwrap();
    let after = d.project().sheet(s).unwrap().spline_geometry(e).unwrap();
    assert_ne!(before, after);
    assert!(distance_mm(nodes(&d)[2].position, pt(35.0, 40.0)) < 1e-9);
    let s2 = snapshot(d.project());

    // Suppression.
    d.execute(Command::RemoveSplinePoint {
        sheet: s,
        spline: e,
        index: 1,
    })
    .unwrap();
    assert_eq!(nodes(&d).len(), 3);

    // Annulations pas à pas, puis rétablissement.
    assert!(d.undo());
    assert_eq!(snapshot(d.project()), s2);
    assert!(d.undo());
    assert_eq!(snapshot(d.project()), s1);
    assert!(d.undo());
    assert_eq!(snapshot(d.project()), s0);
    assert!(d.redo() && d.redo() && d.redo());
    assert_eq!(nodes(&d).len(), 3);

    // Insertion aux deux extrémités.
    for index in [0, 3] {
        d.execute(Command::AddSplinePoint {
            sheet: s,
            spline: e,
            index,
            point: PointRef::New(pt(-50.0 + index as f64 * 60.0, 60.0)),
        })
        .unwrap();
    }
    assert_eq!(nodes(&d).len(), 5);
}

#[test]
fn ca2_minimum_deux_points_refus_sans_trace() {
    let (mut p, s) = setup();
    let e = spline(&mut p, s, &FOUR[..2]).entities[0];
    let before = snapshot(&p);
    for index in [0, 1, 7] {
        assert!(p
            .apply(Command::RemoveSplinePoint {
                sheet: s,
                spline: e,
                index
            })
            .is_err());
    }
    assert_eq!(snapshot(&p), before);
    // Position d'insertion hors de la courbe.
    assert!(p
        .apply(Command::AddSplinePoint {
            sheet: s,
            spline: e,
            index: 3,
            point: PointRef::New(pt(5.0, 5.0)),
        })
        .is_err());
    assert_eq!(snapshot(&p), before);
}

#[test]
fn ca2_la_suppression_d_un_point_corrige_les_extremites() {
    let (mut p, s) = setup();
    let e = spline(&mut p, s, &FOUR).entities[0];
    // Le deuxième point devient anguleux, puis le premier est retiré : il devient
    // extrémité, donc lisse (sinon le modèle serait invalide).
    p.apply(Command::SetSplineNode {
        sheet: s,
        spline: e,
        index: 1,
        corner: true,
        tangent: None,
        tangent_in: None,
    })
    .unwrap();
    p.apply(Command::RemoveSplinePoint {
        sheet: s,
        spline: e,
        index: 0,
    })
    .unwrap();
    let n = p.sheet(s).unwrap().spline_nodes(e).unwrap();
    assert_eq!(n.len(), 3);
    assert!(!n[0].corner);
    p.validate().unwrap();
}

// --- CA 3 : point anguleux vs lisse, tangentes modifiables -------------------

#[test]
fn ca3_point_lisse_c1_point_anguleux_c0() {
    let (mut p, s) = setup();
    let e = spline(&mut p, s, &FOUR).entities[0];
    let gap = |p: &Project| {
        let g = p.sheet(s).unwrap().spline_geometry(e).unwrap();
        let (a, b) = (
            g.segments()[0].derivative_at(1.0),
            g.segments()[1].derivative_at(0.0),
        );
        (a.x - b.x).hypot(a.y - b.y)
    };
    assert!(gap(&p) < 1e-9);
    p.apply(Command::SetSplineNode {
        sheet: s,
        spline: e,
        index: 1,
        corner: true,
        tangent: None,
        tangent_in: None,
    })
    .unwrap();
    assert!(gap(&p) > 1.0, "le point anguleux doit casser la tangente");
    // La courbe passe toujours par le point.
    let g = p.sheet(s).unwrap().spline_geometry(e).unwrap();
    assert!(distance_mm(g.segments()[0].p3, g.segments()[1].p0) < 1e-12);
    // Retour en point lisse : C1 de nouveau.
    p.apply(Command::SetSplineNode {
        sheet: s,
        spline: e,
        index: 1,
        corner: false,
        tangent: None,
        tangent_in: None,
    })
    .unwrap();
    assert!(gap(&p) < 1e-9);
}

#[test]
fn ca3_tangentes_modifiables_module_et_direction() {
    let (mut p, s) = setup();
    let e = spline(&mut p, s, &FOUR).entities[0];
    let default = p.sheet(s).unwrap().spline_nodes(e).unwrap()[1];
    assert!(!default.tangent_out_explicit);
    // Module 60 mm, direction 90° : départ vertical, poignée de Bézier = 20 mm.
    let t = tangent_from_polar(60.0, 90.0).unwrap();
    p.apply(Command::SetSplineNode {
        sheet: s,
        spline: e,
        index: 1,
        corner: false,
        tangent: Some(OffsetMm::from_vector(t)),
        tangent_in: None,
    })
    .unwrap();
    let sh = p.sheet(s).unwrap();
    let n = sh.spline_nodes(e).unwrap()[1];
    assert!(n.tangent_out_explicit && n.tangent_in_explicit);
    assert!((n.tangent_out.norm() - 60.0).abs() < 1e-9);
    let g = sh.spline_geometry(e).unwrap();
    let seg0 = g.segments()[0];
    let seg1 = g.segments()[1];
    assert!(distance_mm(seg1.c1, pt(30.0, 40.0)) < 1e-9);
    assert!(distance_mm(seg0.c2, pt(30.0, 0.0)) < 1e-9);
    // Point anguleux : tangentes entrante et sortante indépendantes.
    p.apply(Command::SetSplineNode {
        sheet: s,
        spline: e,
        index: 1,
        corner: true,
        tangent: Some(OffsetMm { dx: 30.0, dy: 0.0 }),
        tangent_in: Some(OffsetMm { dx: 0.0, dy: 30.0 }),
    })
    .unwrap();
    let g = p.sheet(s).unwrap().spline_geometry(e).unwrap();
    assert!(distance_mm(g.segments()[1].c1, pt(40.0, 20.0)) < 1e-9);
    assert!(distance_mm(g.segments()[0].c2, pt(30.0, 10.0)) < 1e-9);
}

#[test]
fn ca3_refus_extremite_anguleuse_tangente_non_finie_sans_trace() {
    let (mut p, s) = setup();
    let e = spline(&mut p, s, &FOUR).entities[0];
    let before = snapshot(&p);
    let set = |index, corner, tangent, tangent_in| Command::SetSplineNode {
        sheet: s,
        spline: e,
        index,
        corner,
        tangent,
        tangent_in,
    };
    let nan = Some(OffsetMm {
        dx: f64::NAN,
        dy: 0.0,
    });
    let huge = Some(OffsetMm { dx: 1e300, dy: 0.0 });
    for cmd in [
        set(0, true, None, None),
        set(3, true, None, None),
        set(1, false, None, Some(OffsetMm { dx: 1.0, dy: 1.0 })),
        set(1, false, nan, None),
        set(1, true, None, nan),
        set(1, false, huge, None),
        set(9, false, None, None),
    ] {
        assert!(p.apply(cmd.clone()).is_err(), "{cmd:?}");
        assert_eq!(snapshot(&p), before);
    }
}

// --- CA 4 : longueur développée totale et par tronçon -------------------------

#[test]
fn ca4_longueur_totale_et_par_troncon() {
    let (mut p, s) = setup();
    let e = spline(&mut p, s, &FOUR).entities[0];
    let sh = p.sheet(s).unwrap();
    let parts = measure::spline_segment_lengths(sh, e).unwrap();
    let total = measure::entity_length(sh, e).unwrap();
    assert_eq!(parts.len(), 3);
    assert_eq!(total.kind, measure::MeasurementKind::CurveLength);
    assert!((parts.iter().sum::<f64>() - total.value).abs() < 1e-9);
    // Mesure indépendante : polyligne très fine de chaque tronçon.
    let g = sh.spline_geometry(e).unwrap();
    for (seg, &len) in g.segments().iter().zip(&parts) {
        let poly = seg.sample(20_000);
        let l: f64 = poly.windows(2).map(|w| distance_mm(w[0], w[1])).sum();
        assert!((l - len).abs() < 1e-3, "{l} contre {len}");
        assert!(len > distance_mm(seg.p0, seg.p3) - 1e-9, "longueur ≥ corde");
    }
}

// --- CA 5 : conversions Bézier ↔ courbe par points de passage ----------------

#[test]
fn ca5_bezier_vers_spline_et_retour_sans_changement_de_forme() {
    let (mut p, s) = setup();
    let c = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(pt(0.0, 0.0)),
            c1: pt(10.0, 40.0),
            c2: pt(70.0, -30.0),
            end: PointRef::New(pt(80.0, 5.0)),
        })
        .unwrap()
        .entities[0];
    let sh = p.sheet(s).unwrap();
    let orig = sh.curve_geometry(c).unwrap();
    let orig_len = measure::entity_length(sh, c).unwrap().value;
    p.apply(Command::AddNotch {
        sheet: s,
        entity: c,
        distance_mm: 30.0,
        side: Side::Left,
        length_mm: 4.0,
        style: NotchStyle::Single,
    })
    .unwrap();

    p.apply(Command::CurveToSpline { sheet: s, curve: c })
        .unwrap();
    let sh = p.sheet(s).unwrap();
    assert!(matches!(
        sh.entity(c).unwrap().kind,
        EntityKind::Spline { .. }
    ));
    let sp = sh.spline_geometry(c).unwrap();
    assert_eq!(sp.segments().len(), 1);
    assert!(max_gap(&orig, &sp.segments()[0]) < 1e-9);
    assert!((measure::entity_length(sh, c).unwrap().value - orig_len).abs() < 1e-9);
    // L'identifiant et le cran sont conservés.
    assert_eq!(sh.notches.len(), 1);
    p.validate().unwrap();

    p.apply(Command::SplineToCurve {
        sheet: s,
        spline: c,
    })
    .unwrap();
    let sh = p.sheet(s).unwrap();
    let back = sh.curve_geometry(c).unwrap();
    assert!(max_gap(&orig, &back) < 1e-6);
    assert_eq!(sh.notches.len(), 1);
}

#[test]
fn ca5_spline_a_deux_points_par_defaut_est_une_bezier_exacte() {
    let (mut p, s) = setup();
    let e = spline(&mut p, s, &[(0.0, 0.0), (30.0, 40.0)]).entities[0];
    p.apply(Command::SplineToCurve {
        sheet: s,
        spline: e,
    })
    .unwrap();
    let c = p.sheet(s).unwrap().curve_geometry(e).unwrap();
    // Tangentes par défaut = corde : Bézier alignée sur le segment.
    assert!(distance_mm(c.c1, pt(10.0, 40.0 / 3.0)) < 1e-9, "{:?}", c.c1);
    assert!((c.length_mm(1e-9).unwrap() - 50.0).abs() < 1e-9);
}

#[test]
fn ca5_refus_conversions_sans_trace() {
    let (mut p, s) = setup();
    let e = spline(&mut p, s, &FOUR).entities[0];
    let seg = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(200.0, 0.0)),
            to: PointRef::New(pt(220.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let before = snapshot(&p);
    for cmd in [
        Command::SplineToCurve {
            sheet: s,
            spline: e,
        },
        Command::SplineToCurve {
            sheet: s,
            spline: seg,
        },
        Command::CurveToSpline { sheet: s, curve: e },
        Command::CurveToSpline {
            sheet: s,
            curve: seg,
        },
    ] {
        assert!(p.apply(cmd.clone()).is_err(), "{cmd:?}");
        assert_eq!(snapshot(&p), before);
    }
}

// --- Création : refus sans trace --------------------------------------------

#[test]
fn creation_refus_sans_trace() {
    let (mut p, s) = setup();
    spline(&mut p, s, &[(500.0, 0.0), (510.0, 0.0)]);
    let before = snapshot(&p);
    let many: Vec<(f64, f64)> = (0..1001).map(|i| (i as f64 * 2.0, 0.0)).collect();
    let a = PointRef::New(pt(0.0, 0.0));
    let cases: Vec<Vec<PointRef>> = vec![
        vec![],
        news(&[(0.0, 0.0)]),
        // Points confondus (fusionnés en un seul point par la création).
        vec![a, PointRef::New(pt(0.0, 0.0))],
        vec![a, PointRef::New(pt(5e-7, 0.0))],
        news(&[(0.0, 0.0), (f64::NAN, 1.0)]),
        news(&[(0.0, 0.0), (f64::INFINITY, 1.0)]),
        news(&[(0.0, 0.0), (1e12, 1.0)]),
        // Même point deux fois, non consécutif.
        news(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 0.0)]),
        news(&many),
        vec![PointRef::Existing(PointId(424_242)), a],
    ];
    for points in cases {
        let n = points.len();
        assert!(
            p.apply(Command::AddSpline { sheet: s, points }).is_err(),
            "{n} points"
        );
        assert_eq!(snapshot(&p), before, "{n} points");
    }
}

// --- Contours, mesure, sélection ---------------------------------------------

#[test]
fn contour_ferme_par_une_courbe_et_un_segment() {
    let (mut p, s) = setup();
    let e = spline(&mut p, s, &[(0.0, 0.0), (50.0, 30.0), (100.0, 0.0)]).entities[0];
    let a = p.sheet(s).unwrap().entity_points(e).unwrap();
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::Existing(a.1),
        to: PointRef::Existing(a.0),
    })
    .unwrap();
    let sh = p.sheet(s).unwrap();
    let contours = closed_contours(sh);
    assert_eq!(contours.len(), 1);
    let m = measure_contour(sh, &contours[0]).unwrap();
    let g = sh.spline_geometry(e).unwrap();
    assert!((m.perimeter_mm - (g.length_mm().unwrap() + 100.0)).abs() < 1e-9);
    // Aire : polygone fin (courbe échantillonnée + corde).
    let poly = g.sample(4000);
    let area = polygon_signed_area_mm2(&poly).abs();
    assert!(
        (m.area_mm2 - area).abs() < 1e-2,
        "{} contre {area}",
        m.area_mm2
    );
    assert!(!m.self_intersecting);
}

// --- Crans -------------------------------------------------------------------

#[test]
fn cran_a_une_distance_le_long_de_la_courbe() {
    let (mut p, s) = setup();
    let e = spline(&mut p, s, &FOUR).entities[0];
    let sh = p.sheet(s).unwrap();
    let total = sh.path_length_mm(e).unwrap();
    let parts = measure::spline_segment_lengths(sh, e).unwrap();
    // Un cran exactement au deuxième point de passage.
    p.apply(Command::AddNotch {
        sheet: s,
        entity: e,
        distance_mm: parts[0],
        side: Side::Left,
        length_mm: 5.0,
        style: NotchStyle::Single,
    })
    .unwrap();
    let sh = p.sheet(s).unwrap();
    let marks = sh.notch_marks(&sh.notches[0]).unwrap();
    assert!(distance_mm(marks[0].0, pt(30.0, 20.0)) < 1e-4);
    // Le pied du cran est à 5 mm de long, perpendiculaire à la tangente.
    let g = sh.spline_geometry(e).unwrap();
    let t = g.segments()[0].derivative_at(1.0);
    let v = marks[0].1 - marks[0].0;
    assert!((v.norm() - 5.0).abs() < 1e-9);
    assert!(v.dot(t).abs() / (v.norm() * t.norm()) < 1e-6);
    // Distance de retour (sélection d'un point proche).
    let d = sh.path_distance_of(e, pt(30.0, 20.0)).unwrap();
    assert!((d - parts[0]).abs() < 1e-4);
    // Hors de la courbe : refus sans trace.
    let before = snapshot(&p);
    assert!(p
        .apply(Command::AddNotch {
            sheet: s,
            entity: e,
            distance_mm: total + 0.5,
            side: Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .is_err());
    assert_eq!(snapshot(&p), before);
    // Un cran suit la courbe quand un point de passage bouge, tant qu'il reste dessus.
    let moved = p.sheet(s).unwrap().spline_nodes(e).unwrap()[3].point;
    p.apply(Command::MovePoint {
        sheet: s,
        point: moved,
        to: pt(120.0, 10.0),
    })
    .unwrap();
}

// --- Transformations ---------------------------------------------------------

#[test]
fn rotation_et_symetrie_gardent_la_forme_et_font_suivre_les_tangentes() {
    for transform in [
        Transform::Rotate {
            center: pt(10.0, -5.0),
            angle_deg: 37.0,
        },
        Transform::Mirror {
            axis_point: pt(0.0, 0.0),
            axis_angle_deg: 20.0,
        },
    ] {
        let (mut p, s) = setup();
        let e = spline(&mut p, s, &FOUR).entities[0];
        let t = tangent_from_polar(50.0, 10.0).unwrap();
        p.apply(Command::SetSplineNode {
            sheet: s,
            spline: e,
            index: 1,
            corner: false,
            tangent: Some(OffsetMm::from_vector(t)),
            tangent_in: None,
        })
        .unwrap();
        p.apply(Command::AddNotch {
            sheet: s,
            entity: e,
            distance_mm: 20.0,
            side: Side::Left,
            length_mm: 4.0,
            style: NotchStyle::Single,
        })
        .unwrap();
        let sh = p.sheet(s).unwrap();
        let before = sh.spline_geometry(e).unwrap();
        let points = sh.entity_all_points(e).unwrap();
        p.apply(Command::Transform {
            sheet: s,
            points,
            transform,
        })
        .unwrap();
        let sh = p.sheet(s).unwrap();
        let after = sh.spline_geometry(e).unwrap();
        for (a, b) in before.segments().iter().zip(after.segments()) {
            for i in 0..=20 {
                let u = i as f64 / 20.0;
                let expected = transform.point(a.point_at(u)).unwrap();
                assert!(distance_mm(expected, b.point_at(u)) < 1e-9, "{transform:?}");
            }
        }
        assert!((before.length_mm().unwrap() - after.length_mm().unwrap()).abs() < 1e-9);
        // Le cran reste du même côté de la matière : la symétrie le retourne.
        let flipped = matches!(transform, Transform::Mirror { .. });
        assert_eq!(
            sh.notches[0].side,
            if flipped { Side::Right } else { Side::Left }
        );
        let marks = sh.notch_marks(&sh.notches[0]).unwrap();
        let (base, _) = sh.path_point_at(e, 20.0).unwrap();
        assert!(distance_mm(marks[0].0, base) < 1e-9);
    }
}

#[test]
fn duplication_avec_symetrie_copie_courbe_tangentes_et_crans() {
    let (mut p, s) = setup();
    let e = spline(&mut p, s, &FOUR).entities[0];
    p.apply(Command::SetSplineNode {
        sheet: s,
        spline: e,
        index: 2,
        corner: true,
        tangent: Some(OffsetMm { dx: 20.0, dy: 5.0 }),
        tangent_in: Some(OffsetMm { dx: 10.0, dy: -8.0 }),
    })
    .unwrap();
    p.apply(Command::AddNotch {
        sheet: s,
        entity: e,
        distance_mm: 10.0,
        side: Side::Left,
        length_mm: 4.0,
        style: NotchStyle::Single,
    })
    .unwrap();
    let transform = Transform::Mirror {
        axis_point: pt(0.0, -50.0),
        axis_angle_deg: 0.0,
    };
    let d = p
        .apply(Command::Duplicate {
            sheet: s,
            entities: vec![e],
            transform,
        })
        .unwrap();
    let sh = p.sheet(s).unwrap();
    let (a, b) = (
        sh.spline_geometry(e).unwrap(),
        sh.spline_geometry(d.entities[0]).unwrap(),
    );
    assert_eq!(sh.spline_nodes(d.entities[0]).unwrap().len(), 4);
    assert!(sh.spline_nodes(d.entities[0]).unwrap()[2].corner);
    for (x, y) in a.segments().iter().zip(b.segments()) {
        for i in 0..=10 {
            let u = i as f64 / 10.0;
            let m = transform.point(x.point_at(u)).unwrap();
            assert!(distance_mm(m, y.point_at(u)) < 1e-9);
        }
    }
    assert_eq!(sh.notches.len(), 2);
    assert_eq!(sh.notches[1].side, Side::Right);
}

// --- Calques, groupes, suppression, presse-papiers, feuilles ------------------

#[test]
fn calques_groupes_suppression_et_points_utilises() {
    let (mut p, s) = setup();
    let e = spline(&mut p, s, &FOUR).entities[0];
    let seg = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(0.0, 100.0)),
            to: PointRef::New(pt(50.0, 100.0)),
        })
        .unwrap()
        .entities[0];
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "L".into(),
        })
        .unwrap()
        .layers[0];
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![],
        entities: vec![e],
        layer: Some(layer),
    })
    .unwrap();
    p.apply(Command::CreateGroup {
        sheet: s,
        entities: vec![e, seg],
    })
    .unwrap();
    let node = p.sheet(s).unwrap().spline_nodes(e).unwrap()[1].point;
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: true,
    })
    .unwrap();
    let before = snapshot(&p);
    for cmd in [
        Command::MovePoint {
            sheet: s,
            point: node,
            to: pt(1.0, 1.0),
        },
        Command::SetSplineNode {
            sheet: s,
            spline: e,
            index: 1,
            corner: true,
            tangent: None,
            tangent_in: None,
        },
        Command::RemoveSplinePoint {
            sheet: s,
            spline: e,
            index: 1,
        },
        Command::Delete {
            sheet: s,
            points: vec![],
            entities: vec![e],
        },
    ] {
        let err = p.apply(cmd.clone()).unwrap_err();
        assert!(matches!(err, CoreError::LayerLocked(_)), "{cmd:?} : {err}");
        assert_eq!(snapshot(&p), before);
    }
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: false,
    })
    .unwrap();
    // Un point utilisé par la courbe ne se supprime pas seul.
    let err = p
        .apply(Command::Delete {
            sheet: s,
            points: vec![node],
            entities: vec![],
        })
        .unwrap_err();
    assert!(err.to_string().contains("utilisé"), "{err}");
    // Supprimer la courbe supprime ses points libres devenus inutiles.
    let n_points = p.sheet(s).unwrap().points.len();
    p.apply(Command::Delete {
        sheet: s,
        points: vec![],
        entities: vec![e],
    })
    .unwrap();
    let sh = p.sheet(s).unwrap();
    assert_eq!(sh.points.len(), n_points - 4);
    assert_eq!(sh.groups[0].members, vec![seg]);
    p.validate().unwrap();
}

#[test]
fn presse_papiers_et_duplication_de_feuille() {
    let (mut p, s) = setup();
    let e = spline(&mut p, s, &FOUR).entities[0];
    p.apply(Command::SetSplineNode {
        sheet: s,
        spline: e,
        index: 1,
        corner: true,
        tangent: Some(OffsetMm { dx: 12.0, dy: 3.0 }),
        tangent_in: None,
    })
    .unwrap();
    p.apply(Command::AddNotch {
        sheet: s,
        entity: e,
        distance_mm: 12.0,
        side: Side::Right,
        length_mm: 3.0,
        style: NotchStyle::Double,
    })
    .unwrap();
    let frag = p.copy_fragment(s, &[], &[e], &[], &[]).unwrap();
    let frag = Fragment::from_json(&frag.to_json()).unwrap();
    let pasted = p
        .apply(Command::PasteFragment {
            sheet: s,
            fragment: Box::new(frag),
            layer: None,
        })
        .unwrap();
    let sh = p.sheet(s).unwrap();
    let new = pasted.entities[0];
    assert_ne!(new, e);
    assert!(
        (sh.spline_geometry(new).unwrap().length_mm().unwrap()
            - sh.spline_geometry(e).unwrap().length_mm().unwrap())
        .abs()
            < 1e-9
    );
    assert_eq!(sh.notches.len(), 2);
    let old_nodes = sh.spline_nodes(e).unwrap();
    let new_nodes = sh.spline_nodes(new).unwrap();
    assert!(old_nodes
        .iter()
        .zip(&new_nodes)
        .all(|(a, b)| a.point != b.point));

    let old_ids: Vec<EntityId> = sh.entities.iter().map(|x| x.id).collect();
    let d = p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    let copy = p.sheet(d.sheet.unwrap()).unwrap();
    assert_eq!(copy.entities.len(), 2);
    for en in &copy.entities {
        copy.spline_geometry(en.id).unwrap();
        for pid in en.kind.all_points() {
            assert!(copy.point(pid).is_ok());
        }
        assert!(!old_ids.contains(&en.id), "identifiants régénérés");
    }
    p.validate().unwrap();
}

// --- Fichier -----------------------------------------------------------------

#[test]
fn fichier_aller_retour_et_sans_courbe_inchange() {
    let (mut p, s) = setup();
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::New(pt(0.0, 0.0)),
        to: PointRef::New(pt(10.0, 0.0)),
    })
    .unwrap();
    let plain = persist::to_json(&p).unwrap();
    assert!(!plain.contains("spline") && !plain.contains("nodes"));
    assert_eq!(p.schema_version, 6, "schéma 4 (US-09.1)");

    let e = spline(&mut p, s, &FOUR).entities[0];
    p.apply(Command::SetSplineNode {
        sheet: s,
        spline: e,
        index: 1,
        corner: true,
        tangent: Some(OffsetMm { dx: 7.0, dy: 1.0 }),
        tangent_in: Some(OffsetMm { dx: 2.0, dy: 9.0 }),
    })
    .unwrap();
    let json = persist::to_json(&p).unwrap();
    assert!(json.contains("\"type\": \"spline\""), "{json}");
    let back = persist::from_json(&json).unwrap();
    assert_eq!(back, p);
    assert_eq!(persist::to_json(&back).unwrap(), json);
}

#[test]
fn fichier_courbe_invalide_refuse_avec_raison() {
    let (mut p, s) = setup();
    spline(&mut p, s, &FOUR);
    let good = persist::to_json(&p).unwrap();
    let id_of = |text: &str| text.contains("\"nodes\"");
    assert!(id_of(&good));
    let mut v: serde_json::Value = serde_json::from_str(&good).unwrap();
    let nodes = |v: &mut serde_json::Value| {
        v["sheets"][0]["entities"][0]["kind"]["nodes"]
            .as_array_mut()
            .unwrap()
            .clone()
    };
    let all = nodes(&mut v);
    let mutate = |f: &dyn Fn(&mut Vec<serde_json::Value>)| {
        let mut v = v.clone();
        let mut n = all.clone();
        f(&mut n);
        v["sheets"][0]["entities"][0]["kind"]["nodes"] = serde_json::Value::Array(n);
        persist::from_json(&serde_json::to_string(&v).unwrap())
    };
    assert!(mutate(&|n| n.truncate(1)).is_err());
    assert!(mutate(&|n| n.clear()).is_err());
    assert!(mutate(&|n| n[1] = n[0].clone()).is_err(), "point répété");
    assert!(mutate(&|n| n[0]["corner"] = true.into()).is_err());
    assert!(mutate(&|n| n[1]["point"] = 987_654.into()).is_err());
    assert!(mutate(&|n| n[1]["tangent_in"] = serde_json::json!({"dx": 1.0, "dy": 1.0})).is_err());
    assert!(mutate(&|n| n[1]["inconnu"] = 1.into()).is_err());
    assert!(mutate(&|_| {}).is_ok());
}

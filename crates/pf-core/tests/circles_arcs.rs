//! US-02.1 — cercles et arcs : création, mesures, intersections, contours,
//! déplacements, transformations, calques, presse-papiers, fichier, annulation.

use pf_core::contour::{closed_contours, measure_contour};
use pf_core::*;
use pf_geometry::{distance_mm, PointMm, Side, Transform};
use std::f64::consts::PI;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn setup() -> (Project, SheetId) {
    let p = Project::new("Arcs");
    let s = p.sheets[0].id;
    (p, s)
}

fn snapshot(p: &Project) -> String {
    serde_json::to_string(p).unwrap()
}

fn circle(p: &mut Project, s: SheetId, c: PointMm, r: f64) -> EntityId {
    p.apply(Command::AddCircle {
        sheet: s,
        center: PointRef::New(c),
        radius_mm: r,
    })
    .unwrap()
    .entities[0]
}

fn arc(p: &mut Project, s: SheetId, c: PointMm, r: f64, a0: f64, a1: f64, ccw: bool) -> Outcome {
    p.apply(Command::AddArc {
        sheet: s,
        center: PointRef::New(c),
        radius_mm: r,
        start_deg: a0,
        end_deg: a1,
        ccw,
    })
    .unwrap()
}

// --- CA : créations ---------------------------------------------------------

#[test]
fn cercle_centre_et_rayon() {
    let (mut p, s) = setup();
    let c = circle(&mut p, s, pt(10.0, 20.0), 15.0);
    let sh = p.sheet(s).unwrap();
    let m = measure::entity_length(sh, c).unwrap();
    assert_eq!(m.kind, measure::MeasurementKind::CircleCircumference);
    assert!((m.value - 30.0 * PI).abs() < 1e-9);
    assert_eq!(sh.entity_name(c), "Cercle A");
}

#[test]
fn cercle_par_trois_points_et_deux_points_plus_rayon() {
    let (mut p, s) = setup();
    let c = p
        .apply(Command::AddCircleThreePoints {
            sheet: s,
            a: PointRef::New(pt(10.0, 0.0)),
            b: PointRef::New(pt(0.0, 10.0)),
            c: PointRef::New(pt(-10.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let g = p.sheet(s).unwrap().circle_geometry(c).unwrap();
    assert!(distance_mm(g.circle.center, pt(0.0, 0.0)) < 1e-9);
    assert!((g.circle.radius_mm - 10.0).abs() < 1e-9);
    // Le départ du cercle est le premier point cliqué.
    let (start, _) = g.point_at_length(0.0).unwrap();
    assert!(distance_mm(start, pt(10.0, 0.0)) < 1e-9);

    // 2 points + rayon : centre à gauche ou à droite de a → b.
    for (side, expected) in [(Side::Left, pt(0.0, 4.0)), (Side::Right, pt(0.0, -4.0))] {
        let c = p
            .apply(Command::AddCircleTwoPointsRadius {
                sheet: s,
                a: PointRef::New(pt(-3.0, 0.0)),
                b: PointRef::New(pt(3.0, 0.0)),
                radius_mm: 5.0,
                side,
            })
            .unwrap()
            .entities[0];
        let g = p.sheet(s).unwrap().circle_geometry(c).unwrap();
        assert!(distance_mm(g.circle.center, expected) < 1e-9, "{side:?}");
    }
}

#[test]
fn arc_centre_et_angles_longueur_exacte_et_sens() {
    let (mut p, s) = setup();
    let o = arc(&mut p, s, pt(0.0, 0.0), 10.0, 0.0, 90.0, true);
    let sh = p.sheet(s).unwrap();
    let len = measure::entity_length(sh, o.entities[0]).unwrap();
    assert_eq!(len.kind, measure::MeasurementKind::ArcLength);
    assert!((len.value - 5.0 * PI).abs() < 1e-9);
    // Sens horaire de 0° à 90° : le grand arc de 270°.
    let o2 = arc(&mut p, s, pt(50.0, 0.0), 10.0, 0.0, 90.0, false);
    let len2 = measure::entity_length(p.sheet(s).unwrap(), o2.entities[0]).unwrap();
    assert!((len2.value - 15.0 * PI).abs() < 1e-9);
}

#[test]
fn arc_par_trois_points_et_deux_points_plus_rayon() {
    let (mut p, s) = setup();
    let a = p
        .apply(Command::AddArcThreePoints {
            sheet: s,
            start: PointRef::New(pt(10.0, 0.0)),
            through: PointRef::New(pt(0.0, 10.0)),
            end: PointRef::New(pt(-10.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let sh = p.sheet(s).unwrap();
    assert!((sh.arc_geometry(a).unwrap().length_mm() - 10.0 * PI).abs() < 1e-9);
    assert!(sh.arc_geometry(a).unwrap().is_ccw());

    let b = p
        .apply(Command::AddArcTwoPointsRadius {
            sheet: s,
            start: PointRef::New(pt(-3.0, 100.0)),
            end: PointRef::New(pt(3.0, 100.0)),
            radius_mm: 5.0,
            side: Side::Left,
            ccw: true,
        })
        .unwrap()
        .entities[0];
    // Centre (0, 104) à gauche de (-3,100) → (3,100) ; ccw de (-3,100) à (3,100)
    // passe par le haut du cercle : le grand arc.
    let g = p.sheet(s).unwrap().arc_geometry(b).unwrap();
    assert!(distance_mm(g.center, pt(0.0, 104.0)) < 1e-9);
    // Les extrémités sont sous le centre : l'arc anti-horaire est le petit arc.
    let chord_angle = 2.0 * (3.0_f64 / 5.0).asin();
    assert!((g.sweep_rad - chord_angle).abs() < 1e-9);
}

// --- Validation, refus sans trace -------------------------------------------

#[test]
fn commandes_refusees_sans_trace_et_avec_message() {
    let (mut p, s) = setup();
    circle(&mut p, s, pt(0.0, 0.0), 5.0);
    let before = snapshot(&p);
    let bad = vec![
        Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: 0.0,
        },
        Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: -3.0,
        },
        Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: f64::NAN,
        },
        Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: f64::INFINITY,
        },
        Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(f64::NAN, 0.0)),
            radius_mm: 3.0,
        },
        Command::AddCircleThreePoints {
            sheet: s,
            a: PointRef::New(pt(0.0, 0.0)),
            b: PointRef::New(pt(1.0, 1.0)),
            c: PointRef::New(pt(2.0, 2.0)),
        },
        Command::AddCircleTwoPointsRadius {
            sheet: s,
            a: PointRef::New(pt(0.0, 0.0)),
            b: PointRef::New(pt(10.0, 0.0)),
            radius_mm: 4.0,
            side: Side::Left,
        },
        Command::AddArc {
            sheet: s,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: 0.0,
            start_deg: 0.0,
            end_deg: 90.0,
            ccw: true,
        },
        Command::AddArc {
            sheet: s,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: 5.0,
            start_deg: 30.0,
            end_deg: 30.0,
            ccw: true,
        },
        Command::AddArc {
            sheet: s,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: 5.0,
            start_deg: f64::NAN,
            end_deg: 30.0,
            ccw: true,
        },
        Command::AddArcThreePoints {
            sheet: s,
            start: PointRef::New(pt(0.0, 0.0)),
            through: PointRef::New(pt(5.0, 0.0)),
            end: PointRef::New(pt(9.0, 0.0)),
        },
        Command::SetCircleRadius {
            sheet: s,
            entity: EntityId(9999),
            radius_mm: 3.0,
        },
        Command::SetCircleRadius {
            sheet: s,
            entity: p.sheet(s).unwrap().entities[0].id,
            radius_mm: 0.0,
        },
    ];
    for c in bad {
        let err = p.apply(c.clone()).expect_err(&format!("{c:?}"));
        assert!(!err.to_string().is_empty());
        assert_eq!(snapshot(&p), before, "trace laissée par {c:?}");
    }
}

// --- Annulation, fichier -----------------------------------------------------

#[test]
fn annuler_retablir_et_aller_retour_fichier() {
    let (p, s) = setup();
    let mut doc = Document::new(p);
    let before = snapshot(doc.project());
    doc.execute(Command::AddCircle {
        sheet: s,
        center: PointRef::New(pt(1.0, 2.0)),
        radius_mm: 12.5,
    })
    .unwrap();
    doc.execute(Command::AddArc {
        sheet: s,
        center: PointRef::New(pt(40.0, 2.0)),
        radius_mm: 7.0,
        start_deg: 10.0,
        end_deg: 200.0,
        ccw: false,
    })
    .unwrap();
    let after = snapshot(doc.project());
    // Aller-retour fichier : identique.
    let json = persist::to_json(doc.project()).unwrap();
    let back = persist::from_json(&json).unwrap();
    assert_eq!(&back, doc.project());
    assert_eq!(persist::to_json(&back).unwrap(), json);
    assert_eq!(back.schema_version, 6);
    assert!(json.contains("\"type\": \"arc\"") && json.contains("\"type\": \"circle\""));
    // Annuler deux fois : état initial exact ; rétablir : état final exact.
    assert!(doc.undo() && doc.undo());
    assert_eq!(snapshot(doc.project()), before);
    assert!(doc.redo() && doc.redo());
    assert_eq!(snapshot(doc.project()), after);
}

#[test]
fn fichier_sans_arc_inchange_et_champ_sens_par_defaut() {
    // Un projet sans arc ne contient aucune trace des nouvelles variantes.
    let p = demo_piece_project().unwrap();
    let json = persist::to_json(&p).unwrap();
    assert!(!json.contains("circle") && !json.contains("\"arc\""));
    assert_eq!(p.schema_version, 6);
    // Un cercle écrit sans `start_deg` ni `ccw` est relu avec les valeurs par défaut.
    let (mut q, s) = setup();
    let c = circle(&mut q, s, pt(0.0, 0.0), 5.0);
    let mut v: serde_json::Value = serde_json::from_str(&persist::to_json(&q).unwrap()).unwrap();
    let kind = &mut v["sheets"][0]["entities"][0]["kind"];
    kind.as_object_mut().unwrap().remove("start_deg");
    kind.as_object_mut().unwrap().remove("ccw");
    let json = serde_json::to_string(&v).unwrap();
    assert!(!json.contains("start_deg") && !json.contains("ccw"));
    let ok = persist::from_json(&json).unwrap();
    let g = ok.sheet(s).unwrap().circle_geometry(c).unwrap();
    assert_eq!((g.start_deg, g.ccw), (0.0, true));
}

// --- Intersections -----------------------------------------------------------

#[test]
fn intersections_cercle_droite_cercle_cercle_et_tangence() {
    let (mut p, s) = setup();
    let c1 = circle(&mut p, s, pt(0.0, 0.0), 5.0);
    let line_two = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(-10.0, 3.0)),
            to: PointRef::New(pt(10.0, 3.0)),
        })
        .unwrap()
        .entities[0];
    let line_tangent = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(-10.0, 5.0)),
            to: PointRef::New(pt(10.0, 5.0)),
        })
        .unwrap()
        .entities[0];
    let line_none = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(-10.0, 6.0)),
            to: PointRef::New(pt(10.0, 6.0)),
        })
        .unwrap()
        .entities[0];
    let sh = p.sheet(s).unwrap();
    assert_eq!(sh.circle_intersections(c1, line_two).unwrap().len(), 2);
    let tangent = sh.circle_intersections(line_tangent, c1).unwrap();
    assert_eq!(tangent.len(), 1);
    assert!(distance_mm(tangent[0], pt(0.0, 5.0)) < 1e-9);
    assert!(sh.circle_intersections(c1, line_none).unwrap().is_empty());
    // Deux segments : pas une intersection de cercle.
    assert!(sh.circle_intersections(line_two, line_none).is_err());

    // Cercle / cercle : 2 solutions, puis tangents.
    let c2 = circle(&mut p, s, pt(6.0, 0.0), 5.0);
    let c3 = circle(&mut p, s, pt(8.0, 0.0), 3.0);
    let sh = p.sheet(s).unwrap();
    assert_eq!(sh.circle_intersections(c1, c2).unwrap().len(), 2);
    let t = sh.circle_intersections(c1, c3).unwrap();
    assert_eq!(t.len(), 1);
    assert!(distance_mm(t[0], pt(5.0, 0.0)) < 1e-9);
}

#[test]
fn point_d_intersection_suit_les_tracés_et_choisit_par_proximite() {
    let (mut p, s) = setup();
    let c1 = circle(&mut p, s, pt(0.0, 0.0), 5.0);
    let c2 = circle(&mut p, s, pt(6.0, 0.0), 5.0);
    let o = p
        .apply(Command::AddPointAt {
            sheet: s,
            point: PointRef::Derived(PointDefinition::CircleIntersection {
                first: c1,
                second: c2,
                near: pt(3.0, -30.0).into(),
            }),
        })
        .unwrap();
    let id = o.points[0];
    let pos = p.sheet(s).unwrap().position(id).unwrap();
    assert!(distance_mm(pos, pt(3.0, -4.0)) < 1e-9);
    // Le point suit : on déplace le centre du second cercle (translation du cercle).
    let center2 = match p.sheet(s).unwrap().entity(c2).unwrap().kind {
        EntityKind::Circle { center, .. } => center,
        _ => unreachable!(),
    };
    p.apply(Command::MovePoint {
        sheet: s,
        point: center2,
        to: pt(8.0, 0.0),
    })
    .unwrap();
    let pos = p.sheet(s).unwrap().position(id).unwrap();
    assert!(distance_mm(pos, pt(4.0, -3.0)) < 1e-9, "{pos:?}");
    // Plus d'intersection : déplacement refusé, rien ne change.
    let before = snapshot(&p);
    assert!(p
        .apply(Command::MovePoint {
            sheet: s,
            point: center2,
            to: pt(30.0, 0.0),
        })
        .is_err());
    assert_eq!(snapshot(&p), before);
    // Intersection cercle / droite.
    let l = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(-10.0, 3.0)),
            to: PointRef::New(pt(10.0, 3.0)),
        })
        .unwrap()
        .entities[0];
    let q = p
        .apply(Command::AddPointAt {
            sheet: s,
            point: PointRef::Derived(PointDefinition::CircleIntersection {
                first: l,
                second: c1,
                near: pt(10.0, 3.0).into(),
            }),
        })
        .unwrap()
        .points[0];
    assert!(distance_mm(p.sheet(s).unwrap().position(q).unwrap(), pt(4.0, 3.0)) < 1e-9);
}

// --- Contours : périmètre et aire --------------------------------------------

#[test]
fn contours_cercle_et_arcs_perimetre_et_aire() {
    let (mut p, s) = setup();
    let r = 10.0;
    circle(&mut p, s, pt(0.0, 0.0), r);
    let sh = p.sheet(s).unwrap();
    let cs = closed_contours(sh);
    assert_eq!(cs.len(), 1);
    let m = measure_contour(sh, &cs[0]).unwrap();
    assert!((m.perimeter_mm - 2.0 * PI * r).abs() < 1e-9);
    assert!((m.area_mm2 - PI * r * r).abs() < 1e-9);
    assert!(!m.clockwise && !m.self_intersecting);

    // Deux demi-cercles partageant leurs extrémités : même contour fermé.
    let (mut p, s) = setup();
    let top = arc(&mut p, s, pt(0.0, 0.0), r, 0.0, 180.0, true);
    let (p0, p1) = (top.points[1], top.points[2]);
    p.apply(Command::AddArcThreePoints {
        sheet: s,
        start: PointRef::Existing(p1),
        through: PointRef::New(pt(0.0, -r)),
        end: PointRef::Existing(p0),
    })
    .unwrap();
    let sh = p.sheet(s).unwrap();
    let cs = closed_contours(sh);
    assert_eq!(cs.len(), 1, "{:?}", sh.entities);
    let m = measure_contour(sh, &cs[0]).unwrap();
    assert!((m.perimeter_mm - 2.0 * PI * r).abs() < 1e-9);
    assert!((m.area_mm2 - PI * r * r).abs() < 1e-8);

    // Forme en D : demi-cercle + corde. Aire = pi r^2 / 2, recoupée par un polygone fin.
    let (mut p, s) = setup();
    let o = arc(&mut p, s, pt(3.0, -4.0), r, 0.0, 180.0, true);
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::Existing(o.points[2]),
        to: PointRef::Existing(o.points[1]),
    })
    .unwrap();
    let sh = p.sheet(s).unwrap();
    let cs = closed_contours(sh);
    assert_eq!(cs.len(), 1);
    let m = measure_contour(sh, &cs[0]).unwrap();
    assert!((m.perimeter_mm - (PI * r + 2.0 * r)).abs() < 1e-9);
    assert!((m.area_mm2 - PI * r * r / 2.0).abs() < 1e-8);
    let mut poly = sh.arc_geometry(o.entities[0]).unwrap().sample(20_000);
    poly.push(poly[0]);
    let fine = pf_geometry::polygon_signed_area_mm2(&poly[..poly.len() - 1]).abs();
    assert!((fine - m.area_mm2).abs() < 1e-3);
}

// --- Déplacements ------------------------------------------------------------

#[test]
fn deplacement_du_centre_et_des_extremites() {
    let (mut p, s) = setup();
    let o = arc(&mut p, s, pt(0.0, 0.0), 10.0, 0.0, 90.0, true);
    let (center, start, end) = (o.points[0], o.points[1], o.points[2]);
    let e = o.entities[0];
    // Centre : l'arc se translate entièrement.
    p.apply(Command::MovePoint {
        sheet: s,
        point: center,
        to: pt(5.0, 5.0),
    })
    .unwrap();
    let sh = p.sheet(s).unwrap();
    assert!(distance_mm(sh.position(start).unwrap(), pt(15.0, 5.0)) < 1e-9);
    assert!(distance_mm(sh.position(end).unwrap(), pt(5.0, 15.0)) < 1e-9);
    assert!((sh.arc_geometry(e).unwrap().length_mm() - 5.0 * PI).abs() < 1e-9);
    // Extrémité de départ : nouveau rayon, l'autre extrémité suit sur son rayon.
    p.apply(Command::MovePoint {
        sheet: s,
        point: start,
        to: pt(25.0, 5.0),
    })
    .unwrap();
    let sh = p.sheet(s).unwrap();
    let g = sh.arc_geometry(e).unwrap();
    assert!((g.radius_mm - 20.0).abs() < 1e-9);
    assert!(distance_mm(sh.position(end).unwrap(), pt(5.0, 25.0)) < 1e-9);
    assert!((g.length_mm() - 10.0 * PI).abs() < 1e-9);
    // Annulation d'un déplacement : état exact (via Document).
    let mut d = Document::new(p);
    let before = snapshot(d.project());
    d.execute(Command::MovePoint {
        sheet: s,
        point: end,
        to: pt(5.0, 30.0),
    })
    .unwrap();
    assert!(d.undo());
    assert_eq!(snapshot(d.project()), before);
}

#[test]
fn rayon_d_un_cercle_modifiable_et_annulable() {
    let (p, s) = setup();
    let mut d = Document::new(p);
    let c = d
        .execute(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: 5.0,
        })
        .unwrap()
        .entities[0];
    d.execute(Command::SetCircleRadius {
        sheet: s,
        entity: c,
        radius_mm: 8.0,
    })
    .unwrap();
    assert_eq!(
        d.project()
            .sheet(s)
            .unwrap()
            .circle_geometry(c)
            .unwrap()
            .circle
            .radius_mm,
        8.0
    );
    assert!(d.undo());
    assert_eq!(
        d.project()
            .sheet(s)
            .unwrap()
            .circle_geometry(c)
            .unwrap()
            .circle
            .radius_mm,
        5.0
    );
}

// --- Crans -------------------------------------------------------------------

#[test]
fn cran_a_distance_le_long_d_un_arc_et_d_un_cercle() {
    let (mut p, s) = setup();
    let o = arc(&mut p, s, pt(0.0, 0.0), 10.0, 0.0, 180.0, true);
    let e = o.entities[0];
    p.apply(Command::AddNotch {
        sheet: s,
        entity: e,
        distance_mm: 5.0 * PI, // quart de tour : sommet de l'arc
        side: Side::Left,
        length_mm: 4.0,
        style: NotchStyle::Single,
    })
    .unwrap();
    let sh = p.sheet(s).unwrap();
    let marks = sh.notch_marks(&sh.notches[0]).unwrap();
    assert!(distance_mm(marks[0].0, pt(0.0, 10.0)) < 1e-9);
    // Côté gauche d'un arc anti-horaire = intérieur : la pointe est à 4 mm vers le centre.
    assert!(distance_mm(marks[0].1, pt(0.0, 6.0)) < 1e-9);
    // Position hors de l'arc refusée.
    let before = snapshot(&p);
    assert!(p
        .apply(Command::AddNotch {
            sheet: s,
            entity: e,
            distance_mm: 10.0 * PI + 1.0,
            side: Side::Left,
            length_mm: 4.0,
            style: NotchStyle::Single,
        })
        .is_err());
    assert_eq!(snapshot(&p), before);
    // Cercle : cran à un quart de la circonférence.
    let c = circle(&mut p, s, pt(100.0, 0.0), 10.0);
    p.apply(Command::AddNotch {
        sheet: s,
        entity: c,
        distance_mm: 5.0 * PI,
        side: Side::Right,
        length_mm: 3.0,
        style: NotchStyle::Double,
    })
    .unwrap();
    let sh = p.sheet(s).unwrap();
    assert_eq!(sh.notch_marks(&sh.notches[1]).unwrap().len(), 2);
    assert!((sh.path_distance_of(c, pt(100.0, 50.0)).unwrap() - 5.0 * PI).abs() < 1e-9);
}

// --- Transformations ---------------------------------------------------------

#[test]
fn symetrie_inverse_le_sens_et_conserve_la_longueur() {
    let (mut p, s) = setup();
    let o = arc(&mut p, s, pt(0.0, 0.0), 10.0, 0.0, 90.0, true);
    let e = o.entities[0];
    let c = circle(&mut p, s, pt(50.0, 0.0), 4.0);
    p.apply(Command::AddNotch {
        sheet: s,
        entity: e,
        distance_mm: 5.0,
        side: Side::Left,
        length_mm: 3.0,
        style: NotchStyle::Single,
    })
    .unwrap();
    let len = measure::entity_length(p.sheet(s).unwrap(), e)
        .unwrap()
        .value;
    let pts: Vec<PointId> = p.sheet(s).unwrap().points.iter().map(|q| q.id).collect();
    p.apply(Command::Transform {
        sheet: s,
        points: pts,
        transform: Transform::Mirror {
            axis_point: pt(0.0, 0.0),
            axis_angle_deg: 0.0,
        },
    })
    .unwrap();
    let sh = p.sheet(s).unwrap();
    let g = sh.arc_geometry(e).unwrap();
    assert!(!g.is_ccw(), "la symétrie inverse le sens");
    assert!((g.length_mm() - len).abs() < 1e-9);
    // Quart de cercle de (10,0) à (0,-10) : départ miroir de (10,0).
    assert!(distance_mm(g.start_point(), pt(10.0, 0.0)) < 1e-9);
    assert!(distance_mm(g.end_point(), pt(0.0, -10.0)) < 1e-9);
    assert_eq!(sh.notches[0].side, Side::Right);
    assert!(!sh.circle_geometry(c).unwrap().ccw);
    // Rotation de 90° : le départ du cercle tourne avec lui.
    let pts: Vec<PointId> = sh.points.iter().map(|q| q.id).collect();
    let start_before = sh
        .circle_geometry(c)
        .unwrap()
        .point_at_length(0.0)
        .unwrap()
        .0;
    p.apply(Command::Transform {
        sheet: s,
        points: pts,
        transform: Transform::Rotate {
            center: pt(0.0, 0.0),
            angle_deg: 90.0,
        },
    })
    .unwrap();
    let after = p
        .sheet(s)
        .unwrap()
        .circle_geometry(c)
        .unwrap()
        .point_at_length(0.0)
        .unwrap()
        .0;
    assert!(distance_mm(after, pt(-start_before.y, start_before.x)) < 1e-9);
    // Transformer seulement une partie de l'arc le déformerait : refusé.
    let before = snapshot(&p);
    let only_start = vec![match p.sheet(s).unwrap().entity(e).unwrap().kind {
        EntityKind::Arc { start, .. } => start,
        _ => unreachable!(),
    }];
    assert!(p
        .apply(Command::Transform {
            sheet: s,
            points: only_start,
            transform: Transform::Translate {
                dx_mm: 3.0,
                dy_mm: 0.0
            },
        })
        .is_err());
    assert_eq!(snapshot(&p), before);
}

#[test]
fn duplication_avec_symetrie() {
    let (mut p, s) = setup();
    let o = arc(&mut p, s, pt(0.0, 0.0), 10.0, 0.0, 90.0, true);
    let c = circle(&mut p, s, pt(30.0, 0.0), 4.0);
    let d = p
        .apply(Command::Duplicate {
            sheet: s,
            entities: vec![o.entities[0], c],
            transform: Transform::Mirror {
                axis_point: pt(0.0, 0.0),
                axis_angle_deg: 90.0,
            },
        })
        .unwrap();
    assert_eq!(d.entities.len(), 2);
    let sh = p.sheet(s).unwrap();
    let g = sh.arc_geometry(d.entities[0]).unwrap();
    assert!(!g.is_ccw());
    assert!(distance_mm(g.start_point(), pt(-10.0, 0.0)) < 1e-9);
    assert!((g.length_mm() - 5.0 * PI).abs() < 1e-9);
    let gc = sh.circle_geometry(d.entities[1]).unwrap();
    assert!(distance_mm(gc.circle.center, pt(-30.0, 0.0)) < 1e-9);
    // Les originaux sont intacts.
    assert!(sh.arc_geometry(o.entities[0]).unwrap().is_ccw());
}

// --- Calques, groupes, presse-papiers, feuilles ------------------------------

#[test]
fn calques_groupes_et_suppression() {
    let (mut p, s) = setup();
    let o = arc(&mut p, s, pt(0.0, 0.0), 10.0, 0.0, 90.0, true);
    let c = circle(&mut p, s, pt(40.0, 0.0), 4.0);
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
        entities: vec![o.entities[0], c],
        layer: Some(layer),
    })
    .unwrap();
    p.apply(Command::CreateGroup {
        sheet: s,
        entities: vec![o.entities[0], c],
    })
    .unwrap();
    // Verrouillé : déplacer le centre est refusé.
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: true,
    })
    .unwrap();
    let before = snapshot(&p);
    let err = p
        .apply(Command::MovePoint {
            sheet: s,
            point: o.points[0],
            to: pt(1.0, 1.0),
        })
        .unwrap_err();
    assert!(matches!(err, CoreError::LayerLocked(_)), "{err}");
    assert_eq!(snapshot(&p), before);
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: false,
    })
    .unwrap();
    // Suppression de l'arc : son centre et ses extrémités libres disparaissent.
    let n_points = p.sheet(s).unwrap().points.len();
    p.apply(Command::Delete {
        sheet: s,
        points: vec![],
        entities: vec![o.entities[0]],
    })
    .unwrap();
    let sh = p.sheet(s).unwrap();
    assert_eq!(sh.points.len(), n_points - 3);
    assert_eq!(sh.groups[0].members, vec![c]);
}

#[test]
fn presse_papiers_et_duplication_de_feuille_reecrivent_les_ids() {
    let (mut p, s) = setup();
    let o = arc(&mut p, s, pt(0.0, 0.0), 10.0, 0.0, 90.0, true);
    let c = circle(&mut p, s, pt(40.0, 0.0), 4.0);
    p.apply(Command::AddNotch {
        sheet: s,
        entity: o.entities[0],
        distance_mm: 4.0,
        side: Side::Left,
        length_mm: 3.0,
        style: NotchStyle::Single,
    })
    .unwrap();
    let frag = p
        .copy_fragment(s, &[], &[o.entities[0], c], &[], &[])
        .unwrap();
    let frag = Fragment::from_json(&frag.to_json()).unwrap();
    let pasted = p
        .apply(Command::PasteFragment {
            sheet: s,
            fragment: Box::new(frag),
            layer: None,
        })
        .unwrap();
    assert_eq!(pasted.entities.len(), 2);
    let sh = p.sheet(s).unwrap();
    let new_arc = sh.arc_geometry(pasted.entities[0]).unwrap();
    let old_arc = sh.arc_geometry(o.entities[0]).unwrap();
    assert!((new_arc.length_mm() - old_arc.length_mm()).abs() < 1e-9);
    assert_ne!(pasted.entities[0], o.entities[0]);
    assert_eq!(sh.notches.len(), 2);
    let old_ids: Vec<EntityId> = sh.entities.iter().map(|e| e.id).collect();
    // Duplication de feuille : arcs et cercles copiés avec de nouveaux ids.
    let d = p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    let copy = p.sheet(d.sheet.unwrap()).unwrap();
    assert_eq!(copy.entities.len(), 4);
    assert!(copy.entities.iter().all(|e| !old_ids.contains(&e.id)));
    for e in &copy.entities {
        copy.shape(e.id).unwrap();
        for pid in e.kind.all_points() {
            assert!(copy.point(pid).is_ok());
        }
    }
    p.validate().unwrap();
}

#[test]
fn bbox_de_feuille_inclut_les_arcs() {
    let (mut p, s) = setup();
    circle(&mut p, s, pt(0.0, 0.0), 10.0);
    let bb = p.sheet(s).unwrap().bbox().unwrap();
    assert!((bb.min.x + 10.0).abs() < 1e-9 && (bb.max.y - 10.0).abs() < 1e-9);
}

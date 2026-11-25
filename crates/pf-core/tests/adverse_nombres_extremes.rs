//! US-17.6 - valeurs extrêmes, géométries dégénérées et export aux limites.

mod adverse_support;
use adverse_support::*;

use pf_core::command::{Command, CurveHandle, DivisionSpec, PointRef, TangentAt};
use pf_core::model::*;
use pf_core::{contour, persist, Project};
use pf_geometry::{PointMm, Side, Transform};
use std::panic::{catch_unwind, AssertUnwindSafe};

#[test]
fn nombres_extremes_dans_toutes_les_commandes_numeriques() {
    let (base, sheet, pts, ents) = base_square();
    for &x in &extreme_numbers() {
        let mut cmds: Vec<(String, Command)> = vec![
            (
                "polaire longueur".into(),
                Command::AddSegmentPolar {
                    sheet,
                    from: PointRef::Existing(pts[0]),
                    length_mm: x,
                    angle_deg: 30.0,
                },
            ),
            (
                "polaire angle".into(),
                Command::AddSegmentPolar {
                    sheet,
                    from: PointRef::Existing(pts[0]),
                    length_mm: 10.0,
                    angle_deg: x,
                },
            ),
            (
                "perpendiculaire".into(),
                Command::AddPerpendicular {
                    sheet,
                    reference: ents[0],
                    from: PointRef::Existing(pts[1]),
                    length_mm: x,
                    side: Side::Left,
                },
            ),
            (
                "rectangle largeur".into(),
                Command::AddRectangle {
                    sheet,
                    corner: PointRef::New(PointMm::new(300.0, 300.0)),
                    width_mm: x,
                    height_mm: 10.0,
                },
            ),
            (
                "rectangle coin".into(),
                Command::AddRectangle {
                    sheet,
                    corner: PointRef::New(PointMm::new(x, 0.0)),
                    width_mm: 10.0,
                    height_mm: 10.0,
                },
            ),
            (
                "parallèle".into(),
                Command::AddParallel {
                    sheet,
                    reference: ents[0],
                    distance_mm: x,
                    side: Side::Right,
                },
            ),
            (
                "point sur segment".into(),
                Command::AddPointOnSegment {
                    sheet,
                    segment: ents[0],
                    distance_mm: x,
                    origin: SegmentEnd::End,
                },
            ),
            (
                "cran".into(),
                Command::AddNotch {
                    sheet,
                    entity: ents[0],
                    distance_mm: x,
                    side: Side::Left,
                    length_mm: 5.0,
                    style: NotchStyle::Single,
                },
            ),
            (
                "cran longueur".into(),
                Command::AddNotch {
                    sheet,
                    entity: ents[0],
                    distance_mm: 10.0,
                    side: Side::Left,
                    length_mm: x,
                    style: NotchStyle::Double,
                },
            ),
            (
                "cotation décalage".into(),
                Command::AddDimension {
                    sheet,
                    from: pts[0],
                    to: pts[2],
                    offset_mm: x,
                },
            ),
            (
                "déplace point".into(),
                Command::MovePoint {
                    sheet,
                    point: pts[0],
                    to: PointMm::new(x, x),
                },
            ),
            (
                "distance point".into(),
                Command::SetPointDistance {
                    sheet,
                    point: pts[0],
                    distance_mm: x,
                },
            ),
            (
                "paramètre point".into(),
                Command::SetPointParameter {
                    sheet,
                    point: pts[0],
                    value: x,
                },
            ),
            (
                "courbe".into(),
                Command::AddCurve {
                    sheet,
                    start: PointRef::Existing(pts[0]),
                    c1: PointMm::new(x, 10.0),
                    c2: PointMm::new(10.0, x),
                    end: PointRef::Existing(pts[2]),
                },
            ),
            (
                "ajoute point".into(),
                Command::AddPoint {
                    sheet,
                    at: PointMm::new(x, -x),
                },
            ),
        ];
        for t in [
            Transform::Translate {
                dx_mm: x,
                dy_mm: 1.0,
            },
            Transform::Rotate {
                center: PointMm::new(0.0, 0.0),
                angle_deg: x,
            },
            Transform::Rotate {
                center: PointMm::new(x, 0.0),
                angle_deg: 45.0,
            },
            Transform::Mirror {
                axis_point: PointMm::new(0.0, 0.0),
                axis_angle_deg: x,
            },
        ] {
            cmds.push((
                format!("transform {t:?}"),
                Command::Transform {
                    sheet,
                    points: pts.clone(),
                    transform: t,
                },
            ));
            cmds.push((
                format!("duplique {t:?}"),
                Command::Duplicate {
                    sheet,
                    entities: ents.clone(),
                    transform: t,
                },
            ));
        }
        for (what, cmd) in cmds {
            let mut p = base.clone();
            try_cmd(&mut p, cmd, &format!("{what} avec {x:e}"));
        }
    }
}

/// US-02.2 : nombres extrêmes dans les commandes de courbes par points de
/// passage (création, points, tangentes, transformations, copies, crans).
#[test]
fn nombres_extremes_dans_les_courbes_par_points_de_passage() {
    let mut base = rich_project_with_splines();
    let sheet = base.sheets[0].id;
    let sp = base.sheets[0]
        .entities
        .iter()
        .find(|e| matches!(e.kind, EntityKind::Spline { .. }))
        .unwrap()
        .id;
    let sp_points = base.sheets[0].entity_all_points(sp).unwrap();
    base.apply(Command::AddNotch {
        sheet,
        entity: sp,
        distance_mm: 10.0,
        side: Side::Right,
        length_mm: 3.0,
        style: NotchStyle::Double,
    })
    .unwrap();
    let mut accepted = 0;
    for &x in &extreme_numbers() {
        let off = |a: f64, b: f64| Some(OffsetMm { dx: a, dy: b });
        let mut cmds: Vec<(String, Command)> = vec![
            (
                "création".into(),
                Command::AddSpline {
                    sheet,
                    points: vec![
                        PointRef::New(PointMm::new(0.0, 0.0)),
                        PointRef::New(PointMm::new(x, 10.0)),
                        PointRef::New(PointMm::new(50.0, x)),
                    ],
                },
            ),
            (
                "insertion".into(),
                Command::AddSplinePoint {
                    sheet,
                    spline: sp,
                    index: 1,
                    point: PointRef::New(PointMm::new(x, -x)),
                },
            ),
            (
                "tangente sortante".into(),
                Command::SetSplineNode {
                    sheet,
                    spline: sp,
                    index: 1,
                    corner: false,
                    tangent: off(x, 3.0),
                    tangent_in: None,
                },
            ),
            (
                "tangente entrante".into(),
                Command::SetSplineNode {
                    sheet,
                    spline: sp,
                    index: 2,
                    corner: true,
                    tangent: off(1.0, 1.0),
                    tangent_in: off(3.0, x),
                },
            ),
            (
                "déplacement".into(),
                Command::MovePoint {
                    sheet,
                    point: sp_points[1],
                    to: PointMm::new(x, x),
                },
            ),
        ];
        for t in [
            Transform::Translate {
                dx_mm: x,
                dy_mm: 1.0,
            },
            Transform::Rotate {
                center: PointMm::new(x, 0.0),
                angle_deg: 45.0,
            },
            Transform::Rotate {
                center: PointMm::new(0.0, 0.0),
                angle_deg: x,
            },
            Transform::Mirror {
                axis_point: PointMm::new(0.0, 0.0),
                axis_angle_deg: x,
            },
        ] {
            cmds.push((
                format!("transform {t:?}"),
                Command::Transform {
                    sheet,
                    points: sp_points.clone(),
                    transform: t,
                },
            ));
            cmds.push((
                format!("duplique {t:?}"),
                Command::Duplicate {
                    sheet,
                    entities: vec![sp],
                    transform: t,
                },
            ));
        }
        for (what, cmd) in cmds {
            let mut p = base.clone();
            accepted += try_cmd(&mut p, cmd, &format!("{what} avec {x:e}")) as usize;
            check_export(&p, &format!("{what} avec {x:e}"));
        }
    }
    assert!(
        accepted > 10,
        "trop peu de commandes acceptées : {accepted}"
    );
}

/// Mille points de passage : accepté, mesuré, exporté ; mille et un : refusé.
/// US-02.3 : nombres extrêmes dans les congés, chanfreins et tangentes (jamais
/// de panic, projet intact si refusé, valide si accepté).
#[test]
fn nombres_extremes_dans_les_conges_chanfreins_et_tangentes() {
    let (mut base, sheet, _pts, ents) = base_square();
    let c = base
        .apply(Command::AddCircle {
            sheet,
            center: PointRef::New(PointMm::new(60.0, 25.0)),
            radius_mm: 20.0,
        })
        .unwrap()
        .entities[0];
    let a = base
        .apply(Command::AddArc {
            sheet,
            center: PointRef::New(PointMm::new(-60.0, 25.0)),
            radius_mm: 30.0,
            start_deg: 0.0,
            end_deg: 200.0,
            ccw: true,
        })
        .unwrap()
        .entities[0];
    let aim = PointMm::new(50.0, 0.0);
    let corner = (PointMm::new(50.0, 0.0), PointMm::new(100.0, 25.0));
    for &x in &extreme_numbers() {
        let fillet = |first, second, radius_mm, pick_first, pick_second| Command::FilletPaths {
            sheet,
            first,
            second,
            radius_mm,
            pick_first,
            pick_second,
        };
        let chamfer = |d1, d2| Command::ChamferPaths {
            sheet,
            first: ents[0],
            second: ents[1],
            distance_first_mm: d1,
            distance_second_mm: d2,
            pick_first: corner.0,
            pick_second: corner.1,
        };
        let tangent_line = |from: PointMm, pick, length_mm| Command::AddTangentLine {
            sheet,
            from: PointRef::New(from),
            target: c,
            pick,
            length_mm,
        };
        let tangent_at = |entity, at, length_mm, backwards| Command::AddTangentAt {
            sheet,
            entity,
            at,
            length_mm,
            backwards,
        };
        let cmds: Vec<(&str, Command)> = vec![
            (
                "congé rayon",
                fillet(ents[0], ents[1], x, corner.0, corner.1),
            ),
            (
                "congé point visé",
                fillet(
                    ents[0],
                    ents[1],
                    5.0,
                    PointMm::new(x, 0.0),
                    PointMm::new(100.0, x),
                ),
            ),
            (
                "congé cercle",
                fillet(ents[0], c, x, aim, PointMm::new(50.0, 15.0)),
            ),
            (
                "congé arc",
                fillet(
                    ents[2],
                    a,
                    x,
                    PointMm::new(50.0, 50.0),
                    PointMm::new(-60.0, 55.0),
                ),
            ),
            ("chanfrein distance 1", chamfer(x, 5.0)),
            ("chanfrein distance 2", chamfer(5.0, x)),
            (
                "tangente depuis un point",
                tangent_line(PointMm::new(x, 0.0), aim, Some(10.0)),
            ),
            (
                "tangente longueur",
                tangent_line(PointMm::new(80.0, 25.0), aim, Some(x)),
            ),
            (
                "tangente point visé",
                tangent_line(PointMm::new(150.0, 25.0), PointMm::new(x, x), None),
            ),
            (
                "tangente à un arc fraction",
                tangent_at(a, TangentAt::Fraction(x), 10.0, false),
            ),
            (
                "tangente à un cercle longueur",
                tangent_at(c, TangentAt::Fraction(0.3), x, true),
            ),
            (
                "tangente point proche",
                tangent_at(c, TangentAt::Near(PointMm::new(x, -x)), 10.0, false),
            ),
        ];
        for (what, cmd) in cmds {
            let mut p = base.clone();
            try_cmd(&mut p, cmd, &format!("{what} avec {x:e}"));
        }
    }
}

/// US-02.3 : configurations presque dégénérées (droites quasi parallèles,
/// cercle quasi tangent, coordonnées énormes, rayons minuscules ou immenses).
#[test]
fn conges_sur_geometries_presque_degenerees() {
    for eps in [0.0, 1e-15, 1e-12, 1e-9, 1e-6, 1e-3] {
        for big in [1.0, 1e3, 9e3] {
            for r in [1e-6, 1e-3, 1.0, 50.0, 1e4, 1e6] {
                let mut p = Project::new("Dégénéré");
                let s = p.sheets[0].id;
                let seg = |p: &mut Project, b: (f64, f64)| {
                    p.apply(Command::AddSegment {
                        sheet: s,
                        from: PointRef::New(PointMm::new(0.0, 0.0)),
                        to: PointRef::New(PointMm::new(b.0, b.1)),
                    })
                    .map(|o| o.entities[0])
                };
                let a = seg(&mut p, (100.0 * big, 0.0)).unwrap();
                // eps minuscule : les extrémités fusionnent, le second segment existe déjà.
                let Ok(b) = seg(&mut p, (100.0 * big, 100.0 * big * eps)) else {
                    continue;
                };
                let (pa, pb) = (
                    PointMm::new(50.0 * big, 0.0),
                    PointMm::new(50.0 * big, 50.0 * big * eps),
                );
                let label = format!("eps={eps:e} big={big} r={r}");
                try_cmd(
                    &mut p.clone(),
                    Command::FilletPaths {
                        sheet: s,
                        first: a,
                        second: b,
                        radius_mm: r,
                        pick_first: pa,
                        pick_second: pb,
                    },
                    &format!("quasi parallèles {label}"),
                );
                try_cmd(
                    &mut p.clone(),
                    Command::ChamferPaths {
                        sheet: s,
                        first: a,
                        second: b,
                        distance_first_mm: r,
                        distance_second_mm: r,
                        pick_first: pa,
                        pick_second: pb,
                    },
                    &format!("chanfrein quasi parallèles {label}"),
                );
                // Cercle quasi tangent à la droite.
                let c = p
                    .apply(Command::AddCircle {
                        sheet: s,
                        center: PointRef::New(PointMm::new(50.0 * big, 10.0 + eps)),
                        radius_mm: 10.0,
                    })
                    .unwrap()
                    .entities[0];
                try_cmd(
                    &mut p.clone(),
                    Command::FilletPaths {
                        sheet: s,
                        first: a,
                        second: c,
                        radius_mm: r,
                        pick_first: pa,
                        pick_second: PointMm::new(50.0 * big, 20.0),
                    },
                    &format!("cercle quasi tangent {label}"),
                );
                try_cmd(
                    &mut p.clone(),
                    Command::AddTangentLine {
                        sheet: s,
                        from: PointRef::New(PointMm::new(50.0 * big, 20.0 + eps)),
                        target: c,
                        pick: pa,
                        length_mm: Some(r),
                    },
                    &format!("tangente depuis un point quasi sur le cercle {label}"),
                );
            }
        }
    }
}

#[test]
fn courbe_de_mille_points_de_passage() {
    let mut p = Project::new("Mille");
    let sheet = p.sheets[0].id;
    let pts = |n: usize| -> Vec<PointRef> {
        (0..n)
            .map(|i| {
                let t = i as f64;
                PointRef::New(PointMm::new(t * 3.0, 40.0 * (t * 0.1).sin()))
            })
            .collect()
    };
    let e = p
        .apply(Command::AddSpline {
            sheet,
            points: pts(1000),
        })
        .unwrap()
        .entities[0];
    let len = p.sheet(sheet).unwrap().path_length_mm(e).unwrap();
    assert!(len > 2997.0);
    check_export(&p, "mille points");
    let before = snap(&p);
    assert!(p
        .apply(Command::AddSplinePoint {
            sheet,
            spline: e,
            index: 500,
            point: PointRef::New(PointMm::new(1500.0, 77.0)),
        })
        .is_err());
    assert_eq!(snap(&p), before);
    assert!(p
        .apply(Command::AddSpline {
            sheet,
            points: pts(1001),
        })
        .is_err());
    assert_eq!(snap(&p), before);
}

#[test]
fn rotations_minuscules_et_facteurs_enormes_gardent_un_modele_valide() {
    let (mut p, sheet, pts, ents) = base_square();
    for angle in [
        1e-300,
        -1e-300,
        1e-15,
        1e-9,
        360.0,
        720.0 + 1e-12,
        1e12,
        -1e12,
    ] {
        assert!(
            try_cmd(
                &mut p,
                Command::Transform {
                    sheet,
                    points: pts.clone(),
                    transform: Transform::Rotate {
                        center: PointMm::new(0.0, 0.0),
                        angle_deg: angle,
                    },
                },
                &format!("rotation {angle:e}"),
            ),
            "rotation {angle:e} refusée"
        );
    }
    // translations cumulées jusqu'aux limites : refus propre, jamais de dépassement silencieux
    let mut accepted = 0;
    for _ in 0..40 {
        if try_cmd(
            &mut p,
            Command::Transform {
                sheet,
                points: pts.clone(),
                transform: Transform::Translate {
                    dx_mm: 4e4,
                    dy_mm: -4e4,
                },
            },
            "translation 4e4",
        ) {
            accepted += 1;
        }
    }
    assert!(
        accepted > 10 && accepted < 40,
        "{accepted} translations acceptées"
    );
    // duplications successives
    for _ in 0..10 {
        try_cmd(
            &mut p,
            Command::Duplicate {
                sheet,
                entities: ents.clone(),
                transform: Transform::Translate {
                    dx_mm: 1.0,
                    dy_mm: 1.0,
                },
            },
            "duplication",
        );
    }
}

#[test]
fn segments_et_courbes_degeneres() {
    let (base, sheet, pts, ents) = base_square();
    let here = PointMm::new(0.0, 0.0);
    let cases: Vec<(&str, Command)> = vec![
        (
            "segment point à point identique",
            Command::AddSegment {
                sheet,
                from: PointRef::Existing(pts[0]),
                to: PointRef::Existing(pts[0]),
            },
        ),
        (
            "segment de longueur 0 par coordonnées",
            Command::AddSegment {
                sheet,
                from: PointRef::New(here),
                to: PointRef::New(here),
            },
        ),
        (
            "segment sous la tolérance de fusion",
            Command::AddSegment {
                sheet,
                from: PointRef::New(here),
                to: PointRef::New(PointMm::new(1e-9, 0.0)),
            },
        ),
        (
            "segment polaire de longueur 0",
            Command::AddSegmentPolar {
                sheet,
                from: PointRef::Existing(pts[0]),
                length_mm: 0.0,
                angle_deg: 0.0,
            },
        ),
        (
            "courbe début = fin",
            Command::AddCurve {
                sheet,
                start: PointRef::Existing(pts[0]),
                c1: PointMm::new(10.0, 10.0),
                c2: PointMm::new(-10.0, 10.0),
                end: PointRef::Existing(pts[0]),
            },
        ),
        (
            "courbe aplatie sur un point",
            Command::AddCurve {
                sheet,
                start: PointRef::Existing(pts[0]),
                c1: here,
                c2: here,
                end: PointRef::New(here),
            },
        ),
        (
            "courbe poignées alignées sur la corde",
            Command::AddCurve {
                sheet,
                start: PointRef::Existing(pts[0]),
                c1: PointMm::new(10.0, 0.0),
                c2: PointMm::new(20.0, 0.0),
                end: PointRef::Existing(pts[1]),
            },
        ),
        (
            "courbe poignées géantes",
            Command::AddCurve {
                sheet,
                start: PointRef::Existing(pts[0]),
                c1: PointMm::new(1e9, 1e9),
                c2: PointMm::new(-1e9, 1e9),
                end: PointRef::Existing(pts[1]),
            },
        ),
        (
            "intersection de segments parallèles",
            Command::AddPointAt {
                sheet,
                point: PointRef::Derived(PointDefinition::Intersection {
                    first: ents[0],
                    second: ents[2],
                }),
            },
        ),
        (
            "intersection d'un segment avec lui-même",
            Command::AddPointAt {
                sheet,
                point: PointRef::Derived(PointDefinition::Intersection {
                    first: ents[0],
                    second: ents[0],
                }),
            },
        ),
        (
            "rectangle de largeur nulle",
            Command::AddRectangle {
                sheet,
                corner: PointRef::New(PointMm::new(500.0, 0.0)),
                width_mm: 0.0,
                height_mm: 10.0,
            },
        ),
        (
            "rectangle minuscule",
            Command::AddRectangle {
                sheet,
                corner: PointRef::New(PointMm::new(500.0, 0.0)),
                width_mm: 1e-7,
                height_mm: 1e-7,
            },
        ),
        (
            "cotation d'un point à lui-même",
            Command::AddDimension {
                sheet,
                from: pts[0],
                to: pts[0],
                offset_mm: 5.0,
            },
        ),
        (
            "parallèle à distance 0",
            Command::AddParallel {
                sheet,
                reference: ents[0],
                distance_mm: 0.0,
                side: Side::Left,
            },
        ),
        (
            "point sur courbe avec un segment",
            Command::AddPointAt {
                sheet,
                point: PointRef::Derived(PointDefinition::OnCurve {
                    curve: ents[0],
                    t: 0.5,
                }),
            },
        ),
    ];
    for (what, cmd) in cases {
        let mut p = base.clone();
        try_cmd(&mut p, cmd, what);
    }
    // déplacer un point pour que deux points coïncident, puis un segment devienne nul
    let mut p = base.clone();
    let before = snap(&p);
    let ok = try_cmd(
        &mut p,
        Command::MovePoint {
            sheet,
            point: pts[1],
            to: PointMm::new(0.0, 0.0),
        },
        "déplace B sur A",
    );
    assert!(!ok, "un segment de longueur nulle ne doit pas être produit");
    assert_eq!(snap(&p), before);
}

#[test]
fn courbe_et_point_sur_courbe_apres_deplacements_extremes() {
    let (mut p, sheet, pts, _) = base_square();
    let c = p
        .apply(Command::AddCurve {
            sheet,
            start: PointRef::Existing(pts[0]),
            c1: PointMm::new(20.0, 80.0),
            c2: PointMm::new(80.0, 80.0),
            end: PointRef::Existing(pts[2]),
        })
        .unwrap();
    let on = p
        .apply(Command::AddPointAt {
            sheet,
            point: PointRef::Derived(PointDefinition::OnCurve {
                curve: c.entities[0],
                t: 0.5,
            }),
        })
        .unwrap();
    let n = p
        .apply(Command::AddNotch {
            sheet,
            entity: c.entities[0],
            distance_mm: 10.0,
            side: Side::Left,
            length_mm: 4.0,
            style: NotchStyle::Single,
        })
        .unwrap();
    assert_eq!(n.notches.len(), 1);
    for to in [
        PointMm::new(1e5, 1e5),
        PointMm::new(0.0, 0.0),
        PointMm::new(-1e5, 3.0),
        PointMm::new(100.0, 50.0),
    ] {
        try_cmd(
            &mut p,
            Command::MovePoint {
                sheet,
                point: pts[2],
                to,
            },
            "déplace extrémité de courbe",
        );
        for h in [CurveHandle::Start, CurveHandle::End] {
            try_cmd(
                &mut p,
                Command::MoveCurveHandle {
                    sheet,
                    curve: c.entities[0],
                    handle: h,
                    to,
                },
                "déplace poignée",
            );
        }
    }
    for v in extreme_numbers() {
        try_cmd(
            &mut p,
            Command::SetPointParameter {
                sheet,
                point: on.points[0],
                value: v,
            },
            "paramètre sur courbe",
        );
    }
}

#[test]
fn noms_longs_unicode_et_de_controle() {
    let (base, sheet, pts, ents) = base_square();
    for n in names() {
        let short: String = n.chars().take(12).collect();
        let tag = format!("nom {short:?}");
        let mut with_piece = base.clone();
        let piece = with_piece
            .apply(Command::AddPiece {
                sheet,
                name: "p".into(),
                quantity: 1,
                material: None,
                mirrored_pair: false,
            })
            .unwrap()
            .pieces[0];
        let layer = with_piece
            .apply(Command::AddLayer {
                sheet,
                name: "c".into(),
            })
            .unwrap()
            .layers[0];
        let cmds = vec![
            Command::RenameProject { name: n.clone() },
            Command::CreateSheet { name: n.clone() },
            Command::RenameSheet {
                sheet,
                name: n.clone(),
            },
            Command::AddPiece {
                sheet,
                name: n.clone(),
                quantity: 1,
                material: Some(n.clone()),
                mirrored_pair: true,
            },
            Command::SetPieceInfo {
                piece,
                name: "ok".into(),
                quantity: 1,
                material: Some(n.clone()),
                mirrored_pair: false,
            },
            Command::AddLayer {
                sheet,
                name: n.clone(),
            },
            Command::RenameLayer {
                sheet,
                layer,
                name: n.clone(),
            },
            Command::SetPointLabel {
                sheet,
                point: pts[0],
                label: n.clone(),
            },
            Command::SetEntityName {
                sheet,
                entity: ents[0],
                name: Some(n.clone()),
            },
        ];
        for cmd in cmds {
            let mut p = with_piece.clone();
            let accepted = try_cmd(&mut p, cmd, &tag);
            if accepted {
                // un nom accepté doit survivre à un aller-retour fichier
                let text = persist::to_json(&p).unwrap_or_else(|e| panic!("{tag} : {e}"));
                let q = persist::from_json(&text).unwrap_or_else(|e| panic!("{tag} : {e}"));
                assert_eq!(p, q, "{tag} : perte à l'aller-retour");
            }
        }
    }
}

#[test]
fn dix_mille_objets_validation_fichier_et_export() {
    let mut p = Project::new("Gros");
    let sheet = p.sheets[0].id;
    let n = 10_000usize;
    let mut points = Vec::with_capacity(n + 1);
    let mut entities = Vec::with_capacity(n);
    let mut next = p.next_id;
    for i in 0..=n {
        points.push(Point {
            id: PointId(next),
            label: format!("P{i}"),
            definition: PointDefinition::Free {
                x: (i % 100) as f64 * 3.0,
                y: (i / 100) as f64 * 3.0 + (i % 7) as f64 * 0.1,
            },
            role: PointRole::Construction,
            layer: None,
        });
        next += 1;
    }
    for i in 0..n {
        entities.push(Entity {
            id: EntityId(next),
            role: PathRole::Construction,
            kind: EntityKind::Segment {
                from: points[i].id,
                to: points[i + 1].id,
                construction: None,
            },
            layer: None,
            name: None,
        });
        next += 1;
    }
    p.next_id = next;
    p.sheets[0].points = points;
    p.sheets[0].entities = entities;
    let t = std::time::Instant::now();
    p.validate().expect("10 000 objets valides");
    let text = persist::to_json(&p).unwrap();
    let q = persist::from_json(&text).unwrap();
    assert_eq!(p, q);
    // une commande sur un gros projet reste acceptée
    let mut p2 = p.clone();
    p2.apply(Command::AddSegmentPolar {
        sheet,
        from: PointRef::New(PointMm::new(-50.0, -50.0)),
        length_mm: 10.0,
        angle_deg: 0.0,
    })
    .unwrap();
    // contours fermés sur un gros projet : ne doit pas exploser
    let _ = contour::closed_contours(p.sheet(sheet).unwrap());
    eprintln!("10 000 objets : {:?}", t.elapsed());
}

/// Valeurs extrêmes de US-02.10 : tracés minuscules ou énormes, distances
/// non finies, nulles, énormes ou infimes, chaînes absurdes. Une erreur propre
/// ou un projet valide, jamais de panique ; état inchangé après un refus.
#[test]
fn decalages_valeurs_extremes_sans_panique() {
    // 2e3 : les objets les plus éloignés restent dans l'espace de travail (±1e6 mm).
    for scale in [1e-6, 1e-3, 1.0, 1e3, 2e3] {
        let mut p = Project::new("Extrêmes");
        let s = p.sheets[0].id;
        let k = |x: f64, y: f64| PointMm::new(x * scale, y * scale);
        let rect = p
            .apply(Command::AddRectangle {
                sheet: s,
                corner: PointRef::New(k(0.0, 0.0)),
                width_mm: 40.0 * scale,
                height_mm: 20.0 * scale,
            })
            .unwrap()
            .entities;
        let curve = p
            .apply(Command::AddCurve {
                sheet: s,
                start: PointRef::New(k(100.0, 0.0)),
                c1: k(100.0, 60.0),
                c2: k(120.0, 60.0),
                end: PointRef::New(k(120.0, 0.0)),
            })
            .unwrap()
            .entities[0];
        let spline = p
            .apply(Command::AddSpline {
                sheet: s,
                points: vec![
                    PointRef::New(k(200.0, 0.0)),
                    PointRef::New(k(210.0, 15.0)),
                    PointRef::New(k(230.0, 5.0)),
                    PointRef::New(k(250.0, 20.0)),
                ],
            })
            .unwrap()
            .entities[0];
        let circle = p
            .apply(Command::AddCircle {
                sheet: s,
                center: PointRef::New(k(300.0, 0.0)),
                radius_mm: 10.0 * scale,
            })
            .unwrap()
            .entities[0];
        let arc = p
            .apply(Command::AddArc {
                sheet: s,
                center: PointRef::New(k(400.0, 0.0)),
                radius_mm: 10.0 * scale,
                start_deg: 10.0,
                end_deg: 200.0,
                ccw: true,
            })
            .unwrap()
            .entities[0];
        let groups: Vec<Vec<EntityId>> = vec![
            rect.clone(),
            vec![rect[0]],
            vec![curve],
            vec![spline],
            vec![circle],
            vec![arc],
            vec![curve, spline],
            vec![],
            vec![EntityId(u64::MAX)],
        ];
        let mut numbers = vec![
            scale,
            -scale,
            0.5 * scale,
            5.0 * scale,
            -5.0 * scale,
            25.0 * scale,
            1e-12,
            1e-9,
            1e-6,
            1e12,
            -1e300,
        ];
        numbers.extend(extreme_numbers());
        for paths in &groups {
            for &d in &numbers {
                for corner in [
                    pf_geometry::CornerStyle::Miter,
                    pf_geometry::CornerStyle::Round,
                ] {
                    let cmd = Command::OffsetPaths {
                        sheet: s,
                        paths: paths.clone(),
                        distance_mm: d,
                        corner,
                        role: PathRole::Construction,
                    };
                    let before = snap(&p);
                    let mut trial = p.clone();
                    let r = catch_unwind(AssertUnwindSafe(|| trial.apply(cmd.clone())))
                        .unwrap_or_else(|_| panic!("panique (échelle {scale}) : {cmd:?}"));
                    match r {
                        Ok(_) => {
                            trial.validate().unwrap_or_else(|e| {
                                panic!("projet invalide ({scale}) {cmd:?} : {e}")
                            });
                            let text = persist::to_json(&trial).unwrap();
                            assert_eq!(
                                persist::to_json(&persist::from_json(&text).unwrap()).unwrap(),
                                text
                            );
                        }
                        Err(e) => {
                            assert!(!e.to_string().is_empty());
                            assert_eq!(snap(&p), before, "refus avec trace : {cmd:?}");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn nombres_extremes_dans_les_lignes_d_aide() {
    let mut p = rich_project_with_guides();
    let sheet = p.sheets[0].id;
    let guide = p.sheets[0].guides[0].id;
    for &x in &extreme_numbers() {
        for &y in &[0.0, 1e9, f64::NAN] {
            let what = format!("AddGuide x={x:e} y={y:e}");
            try_cmd(
                &mut p,
                Command::AddGuide {
                    sheet,
                    x_mm: x,
                    y_mm: y,
                    angle_deg: 30.0,
                    axis: false,
                    layer: None,
                },
                &what,
            );
            let what = format!("AddGuide angle={x:e} y={y:e}");
            try_cmd(
                &mut p,
                Command::AddGuide {
                    sheet,
                    x_mm: 10.0,
                    y_mm: 10.0,
                    angle_deg: x,
                    axis: false,
                    layer: None,
                },
                &what,
            );
            let what = format!("MoveGuide x={x:e} y={y:e}");
            try_cmd(
                &mut p,
                Command::MoveGuide {
                    sheet,
                    guide,
                    x_mm: x,
                    y_mm: y,
                    angle_deg: x,
                },
                &what,
            );
        }
    }
    // Un guide accepté a toujours un angle normalisé dans [0 ; 180[.
    for g in &p.sheets[0].guides {
        assert!((0.0..180.0).contains(&g.angle_deg), "{g:?}");
    }
    // Accrochage / export avec des guides extrêmes acceptés : pas de panique.
    for g in p.sheets[0].guides.clone() {
        for e in &p.sheets[0].entities {
            let _ = p.sheets[0].guide_trace_hits(&g, e.id);
        }
    }
}

/// Valeurs extrêmes de US-02.7 : tracés minuscules ou énormes, valeurs non
/// finies ou gigantesques, points visés extrêmes. Une erreur propre ou un
/// projet valide, jamais de panique ; état inchangé après un refus.
#[test]
fn modifier_un_trace_valeurs_extremes_sans_panique() {
    for scale in [1e-6, 1e-3, 1.0, 1e3, 5e5] {
        let mut p = Project::new("Extrêmes");
        let s = p.sheets[0].id;
        let k = |x: f64, y: f64| PointMm::new(x * scale, y * scale);
        let mut traces = Vec::new();
        for o in (vec![
            p.apply(Command::AddSegment {
                sheet: s,
                from: PointRef::New(k(0.0, 0.0)),
                to: PointRef::New(k(10.0, 3.0)),
            }),
            p.apply(Command::AddSegment {
                sheet: s,
                from: PointRef::New(k(10.0, 3.0)),
                to: PointRef::New(k(20.0, 6.0)),
            }),
            p.apply(Command::AddCurve {
                sheet: s,
                start: PointRef::New(k(0.0, 30.0)),
                c1: k(0.0, 60.0),
                c2: k(10.0, 0.0),
                end: PointRef::New(k(10.0, 30.0)),
            }),
            p.apply(Command::AddSpline {
                sheet: s,
                points: vec![
                    PointRef::New(k(0.0, 50.0)),
                    PointRef::New(k(5.0, 60.0)),
                    PointRef::New(k(9.0, 45.0)),
                ],
            }),
            p.apply(Command::AddCircle {
                sheet: s,
                center: PointRef::New(k(30.0, 0.0)),
                radius_mm: 5.0 * scale,
            }),
            p.apply(Command::AddArc {
                sheet: s,
                center: PointRef::New(k(30.0, 30.0)),
                radius_mm: 5.0 * scale,
                start_deg: 10.0,
                end_deg: 200.0,
                ccw: true,
            }),
        ])
        .into_iter()
        .flatten()
        {
            traces.push(o.entities[0]);
        }
        let numbers = [
            0.0,
            scale,
            -scale,
            1e-12,
            1e-9,
            1e-300,
            f64::MIN_POSITIVE,
            1e300,
            -1e300,
            f64::MAX,
            f64::MIN,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ];
        let mut cmds: Vec<Command> = Vec::new();
        for &e in &traces {
            for &x in &numbers {
                for end in [SegmentEnd::Start, SegmentEnd::End] {
                    cmds.push(Command::ExtendPath {
                        sheet: s,
                        entity: e,
                        end,
                        delta_mm: x,
                    });
                }
                cmds.push(Command::SplitPath {
                    sheet: s,
                    entity: e,
                    at: PointMm::new(x, x),
                });
                cmds.push(Command::TrimPathTo {
                    sheet: s,
                    entity: e,
                    end: SegmentEnd::End,
                    target: traces[(e.0 as usize) % traces.len()],
                    pick: PointMm::new(x, 0.0),
                });
            }
            cmds.push(Command::ReversePath {
                sheet: s,
                entity: e,
            });
            for &f in &traces {
                cmds.push(Command::MergePaths {
                    sheet: s,
                    first: e,
                    second: f,
                });
                cmds.push(Command::TrimPathTo {
                    sheet: s,
                    entity: e,
                    end: SegmentEnd::Start,
                    target: f,
                    pick: k(5.0, 3.0),
                });
            }
        }
        for cmd in cmds {
            let before = snap(&p);
            let mut trial = p.clone();
            let r = catch_unwind(AssertUnwindSafe(|| trial.apply(cmd.clone())))
                .unwrap_or_else(|_| panic!("panique (échelle {scale}) : {cmd:?}"));
            match r {
                Ok(_) => {
                    trial
                        .validate()
                        .unwrap_or_else(|e| panic!("projet invalide ({scale}) {cmd:?} : {e}"));
                    // l'état accepté se relit et se réécrit à l'identique
                    let text = persist::to_json(&trial).unwrap();
                    assert_eq!(
                        persist::to_json(&persist::from_json(&text).unwrap()).unwrap(),
                        text
                    );
                    p = trial;
                }
                Err(_) => assert_eq!(snap(&p), before, "refus avec trace : {cmd:?}"),
            }
        }
    }
}

/// Valeurs extrêmes des divisions : tracés minuscules ou énormes, N et
/// proportions extrêmes, point de départ au centre du cercle. Une erreur propre
/// ou des points valides, jamais de panique ; état inchangé après un refus.
#[test]
fn divisions_valeurs_extremes_sans_panique() {
    let specs = || {
        vec![
            DivisionSpec::Equal { parts: 0 },
            DivisionSpec::Equal { parts: 1 },
            DivisionSpec::Equal { parts: 2 },
            DivisionSpec::Equal { parts: 7 },
            DivisionSpec::Equal { parts: 1000 },
            DivisionSpec::Equal { parts: 1001 },
            DivisionSpec::Equal { parts: usize::MAX },
            DivisionSpec::Proportions { values: vec![] },
            DivisionSpec::Proportions {
                values: vec![1.0, 1.0],
            },
            DivisionSpec::Proportions {
                values: vec![1e-300, 1.0],
            },
            DivisionSpec::Proportions {
                values: vec![1e300, 1e300],
            },
            DivisionSpec::Proportions {
                values: vec![f64::MAX, f64::MAX],
            },
            DivisionSpec::Proportions {
                values: vec![f64::MIN_POSITIVE, f64::MAX],
            },
            DivisionSpec::Proportions {
                values: vec![1.0, f64::NAN, 1.0],
            },
            DivisionSpec::Proportions {
                values: vec![1.0, f64::INFINITY],
            },
            DivisionSpec::Proportions {
                values: vec![-1.0, 3.0],
            },
            DivisionSpec::Proportions {
                values: vec![0.0, 0.0],
            },
            DivisionSpec::Proportions {
                values: vec![1.0; 999],
            },
        ]
    };
    for scale in [1e-6, 1e-3, 1.0, 1e3, 5e5] {
        let mut p = Project::new("Extrêmes");
        let s = p.sheets[0].id;
        let k = |x: f64, y: f64| PointMm::new(x * scale, y * scale);
        let mut traces = Vec::new();
        for o in (vec![
            p.apply(Command::AddSegment {
                sheet: s,
                from: PointRef::New(k(0.0, 0.0)),
                to: PointRef::New(k(10.0, 3.0)),
            }),
            p.apply(Command::AddCurve {
                sheet: s,
                start: PointRef::New(k(0.0, 30.0)),
                c1: k(0.0, 60.0),
                c2: k(10.0, 0.0),
                end: PointRef::New(k(10.0, 30.0)),
            }),
            p.apply(Command::AddSpline {
                sheet: s,
                points: vec![
                    PointRef::New(k(0.0, 50.0)),
                    PointRef::New(k(5.0, 60.0)),
                    PointRef::New(k(9.0, 45.0)),
                ],
            }),
            p.apply(Command::AddCircle {
                sheet: s,
                center: PointRef::New(k(30.0, 0.0)),
                radius_mm: 5.0 * scale,
            }),
            p.apply(Command::AddArc {
                sheet: s,
                center: PointRef::New(k(30.0, 30.0)),
                radius_mm: 5.0 * scale,
                start_deg: 10.0,
                end_deg: 200.0,
                ccw: true,
            }),
        ])
        .into_iter()
        .flatten()
        {
            traces.push(o.entities[0]);
        }
        let starts: Vec<Option<PointId>> = std::iter::once(None)
            .chain(p.sheet(s).unwrap().points.iter().map(|q| Some(q.id)))
            .collect();
        for &trace in &traces {
            for spec in specs() {
                for &start in &starts {
                    let before = snap(&p);
                    let cmd = Command::DivideTrace {
                        sheet: s,
                        trace,
                        spec: spec.clone(),
                        start,
                    };
                    // ne garder qu'un sous-ensemble de départs pour borner la durée
                    if matches!(spec, DivisionSpec::Equal { parts } if parts >= 1000)
                        && start.is_some()
                    {
                        continue;
                    }
                    let mut trial = p.clone();
                    let r = catch_unwind(AssertUnwindSafe(|| trial.apply(cmd.clone())))
                        .unwrap_or_else(|_| {
                            panic!("PANIC échelle {scale} : {spec:?} départ {start:?}")
                        });
                    match r {
                        Err(e) => {
                            assert!(!e.to_string().is_empty());
                            assert_eq!(snap(&trial), before, "refus mais modifié : {spec:?}");
                        }
                        Ok(o) => {
                            assert!(trial.validate().is_ok());
                            assert!(!o.points.is_empty() && o.points.len() < 1000);
                        }
                    }
                }
            }
        }
    }
}

/// Valeurs extrêmes des points relatifs : coordonnées énormes ou minuscules,
/// rayons et distances extrêmes, tracés dégénérés. Une erreur propre ou un
/// point, jamais de panique ; état inchangé après un refus.
#[test]
fn points_relatifs_valeurs_extremes_sans_panique() {
    for scale in [1e-6, 1e-3, 1.0, 1e3, 5e5] {
        let mut p = Project::new("Extrêmes");
        let s = p.sheets[0].id;
        let k = |x: f64, y: f64| PointMm::new(x * scale, y * scale);
        let free = |p: &mut Project, x: f64, y: f64| {
            p.apply(Command::AddPoint {
                sheet: s,
                at: k(x, y),
            })
            .ok()
            .and_then(|o| o.points.first().copied())
        };
        let (a, b, c) = (
            free(&mut p, 0.0, 0.0),
            free(&mut p, 10.0, 0.0),
            free(&mut p, 0.0, 10.0),
        );
        let curve = p
            .apply(Command::AddCurve {
                sheet: s,
                start: PointRef::New(k(0.0, 30.0)),
                c1: k(0.0, 40.0),
                c2: k(10.0, 40.0),
                end: PointRef::New(k(10.0, 30.0)),
            })
            .ok()
            .map(|o| o.entities[0]);
        let seg = p
            .apply(Command::AddSegment {
                sheet: s,
                from: PointRef::New(k(20.0, 0.0)),
                to: PointRef::New(k(30.0, 0.0)),
            })
            .ok()
            .map(|o| o.entities[0]);
        let (Some(a), Some(b), Some(c), Some(curve), Some(seg)) = (a, b, c, curve, seg) else {
            continue;
        };
        let big = [
            0.0,
            -1.0,
            1e-12,
            1e-9,
            1.0,
            1e3 * scale,
            1e9,
            1e300,
            f64::MAX,
            f64::NAN,
            f64::INFINITY,
        ];
        for v in big {
            let defs = [
                PointDefinition::AlongCurve {
                    curve,
                    distance_mm: v,
                    origin: SegmentEnd::Start,
                },
                PointDefinition::AlongCurve {
                    curve,
                    distance_mm: v,
                    origin: SegmentEnd::End,
                },
                PointDefinition::Offset {
                    from: a,
                    dx_mm: v,
                    dy_mm: -v,
                },
                PointDefinition::Bisector {
                    vertex: a,
                    first: b,
                    second: c,
                    distance_mm: v,
                },
                PointDefinition::Polar {
                    from: a,
                    distance_mm: v,
                    angle_deg: v,
                    reference: Some(seg),
                },
                PointDefinition::Polar {
                    from: a,
                    distance_mm: 5.0,
                    angle_deg: v,
                    reference: None,
                },
                PointDefinition::Compass {
                    center_a: a,
                    center_b: b,
                    radius_a_mm: v,
                    radius_b_mm: v,
                    near: k(5.0, 5.0).into(),
                },
                PointDefinition::Compass {
                    center_a: a,
                    center_b: b,
                    radius_a_mm: 6.0 * scale,
                    radius_b_mm: v,
                    near: PointMm::new(v, v).into(),
                },
                PointDefinition::Projection {
                    point: c,
                    trace: curve,
                },
                PointDefinition::Projection {
                    point: c,
                    trace: seg,
                },
            ];
            for d in defs {
                let before = snap(&p);
                let r = catch_unwind(AssertUnwindSafe(|| {
                    p.apply(Command::AddPointAt {
                        sheet: s,
                        point: PointRef::Derived(d),
                    })
                }))
                .unwrap_or_else(|_| panic!("PANIC échelle {scale} valeur {v} : {d:?}"));
                match r {
                    Err(e) => {
                        assert!(!e.to_string().is_empty());
                        assert_eq!(snap(&p), before, "refus mais modifié : {d:?}");
                    }
                    Ok(_) => assert!(p.validate().is_ok()),
                }
            }
        }
    }
}

/// Valeurs extrêmes : très grandes coordonnées, très petits tracés, courbes
/// quasi superposées ou quasi tangentes : une erreur propre ou un point, jamais de panique,
/// et l'état est inchangé après un refus.
#[test]
fn intersections_valeurs_extremes_sans_panique() {
    for scale in [1e-6, 1e-3, 1.0, 1e3, 5e5] {
        for dy in [0.0, 1e-12, 1e-9, 1e-6, 0.5] {
            let mut p = Project::new("Extrêmes");
            let s = p.sheets[0].id;
            let k = |x: f64, y: f64| PointMm::new(x * scale, y * scale);
            let a = p.apply(Command::AddCurve {
                sheet: s,
                start: PointRef::New(k(0.0, 0.0)),
                c1: k(0.0, 10.0),
                c2: k(10.0, 10.0),
                end: PointRef::New(k(10.0, 0.0)),
            });
            let b = p.apply(Command::AddCurve {
                sheet: s,
                start: PointRef::New(k(0.0, 15.0 - dy)),
                c1: k(0.0, 5.0 - dy),
                c2: k(10.0, 5.0 - dy),
                end: PointRef::New(k(10.0, 15.0 - dy)),
            });
            let l = p.apply(Command::AddSegment {
                sheet: s,
                from: PointRef::New(k(-5.0, 7.5)),
                to: PointRef::New(k(15.0, 7.5)),
            });
            let ids: Vec<EntityId> = [a, b, l]
                .into_iter()
                .filter_map(|o| o.ok().map(|o| o.entities[0]))
                .collect();
            for &x in &ids {
                for &y in &ids {
                    let before = snap(&p);
                    let r = catch_unwind(AssertUnwindSafe(|| {
                        p.apply(Command::AddPointAt {
                            sheet: s,
                            point: PointRef::Derived(PointDefinition::TraceIntersection {
                                first: x,
                                second: y,
                                near: k(5.0, 7.5).into(),
                            }),
                        })
                    }))
                    .unwrap_or_else(|_| panic!("PANIC échelle {scale} dy {dy}"));
                    if r.is_err() {
                        assert_eq!(snap(&p), before, "refus mais modifié");
                    } else {
                        assert!(p.validate().is_ok());
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 4. Export sur projets extrêmes
// ---------------------------------------------------------------------------

#[test]
fn export_du_projet_riche_et_des_exemples_sans_nan() {
    check_export(&rich_project(), "riche");
    check_export(&rich_project_with_arcs(), "riche avec arcs");
    check_export(&rich_project_with_splines(), "riche avec courbes lisses");
    check_export(&pf_core::demo_project().unwrap(), "démo");
    check_export(&pf_core::demo_piece_project().unwrap(), "pièce");
}

#[test]
fn export_de_geometries_aux_limites() {
    let (mut p, sheet, pts, _) = base_square();
    // grande géométrie acceptée par le modèle
    for to in [
        PointMm::new(50_000.0, 0.0),
        PointMm::new(-40_000.0, 40_000.0),
    ] {
        let _ = p.apply(Command::MovePoint {
            sheet,
            point: pts[1],
            to,
        });
    }
    let _ = p.apply(Command::AddCurve {
        sheet,
        start: PointRef::Existing(pts[0]),
        c1: PointMm::new(0.0, 0.0),
        c2: PointMm::new(0.0, 0.0),
        end: PointRef::New(PointMm::new(1e-3, 1e-3)),
    });
    let _ = p.apply(Command::AddNotch {
        sheet,
        entity: p.sheets[0].entities[0].id,
        distance_mm: 1e-3,
        side: Side::Left,
        length_mm: 1e-6,
        style: NotchStyle::Double,
    });
    check_export(&p, "limites");
}

#[test]
fn symetrie_liee_valeurs_extremes_sans_panique() {
    for &x in &extreme_numbers() {
        for &angle in &[0.0, 45.0, 90.0, x] {
            let (mut p, sheet, _pts, ents) = base_square();
            let g = p
                .apply(Command::AddGuide {
                    sheet,
                    x_mm: x,
                    y_mm: 10.0,
                    angle_deg: angle,
                    axis: true,
                    layer: None,
                })
                .map(|o| o.guides[0]);
            let Ok(g) = g else { continue };
            let what = format!("LinkMirror x={x:e} angle={angle:e}");
            let ok = try_cmd(
                &mut p,
                Command::LinkMirror {
                    sheet,
                    points: Vec::new(),
                    entities: ents.clone(),
                    axis: MirrorAxis::Guide { guide: g },
                },
                &what,
            );
            if ok {
                // Déplacer l'axe vers des valeurs extrêmes : accepté et valide, ou refusé sans trace.
                try_cmd(
                    &mut p,
                    Command::MoveGuide {
                        sheet,
                        guide: g,
                        x_mm: x,
                        y_mm: x,
                        angle_deg: x,
                    },
                    &format!("{what} puis MoveGuide"),
                );
                try_cmd(
                    &mut p,
                    Command::UnfoldMirror {
                        sheet,
                        axis: MirrorAxis::Guide { guide: g },
                    },
                    &format!("{what} puis UnfoldMirror"),
                );
            }
        }
    }
    // Coordonnées proches de la limite de l'espace de travail : l'image sort de l'espace.
    let (mut p, sheet, _pts, ents) = base_square();
    let g = p
        .apply(Command::AddGuide {
            sheet,
            x_mm: -1e6,
            y_mm: 0.0,
            angle_deg: 90.0,
            axis: true,
            layer: None,
        })
        .unwrap()
        .guides[0];
    assert!(!try_cmd(
        &mut p,
        Command::LinkMirror {
            sheet,
            points: Vec::new(),
            entities: ents,
            axis: MirrorAxis::Guide { guide: g },
        },
        "image hors de l'espace de travail",
    ));
}

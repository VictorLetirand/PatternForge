//! Rectangle, parallèle, transformation en place et duplication.

use pf_core::contour::{closed_contours, measure_contour};
use pf_core::measure::entity_length;
use pf_core::*;
use pf_geometry::{distance_mm, PointMm, Side, Transform};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn base() -> (Project, SheetId) {
    let p = Project::new("Édition");
    let sheet = p.sheets[0].id;
    (p, sheet)
}

fn rect(p: &mut Project, sheet: SheetId, x: f64, y: f64, w: f64, h: f64) -> Outcome {
    p.apply(Command::AddRectangle {
        sheet,
        corner: PointRef::New(pt(x, y)),
        width_mm: w,
        height_mm: h,
    })
    .unwrap()
}

#[test]
fn rectangle_quatre_segments_relies() {
    let (mut p, sheet) = base();
    let o = rect(&mut p, sheet, 10., 20., 600., 450.);
    assert_eq!((o.points.len(), o.entities.len()), (4, 4));
    let s = p.sheet(sheet).unwrap();
    let c = closed_contours(s);
    assert_eq!(c.len(), 1, "les quatre côtés forment un contour fermé");
    let m = measure_contour(s, &c[0]).unwrap();
    assert_eq!((m.perimeter_mm, m.area_mm2), (2100.0, 270_000.0));
    // Signes négatifs : vers la gauche et le bas, même rectangle.
    let o = rect(&mut p, sheet, 10., 20., -100., -50.);
    let s = p.sheet(sheet).unwrap();
    assert_eq!(o.entities.len(), 4);
    assert!(s
        .points
        .iter()
        .any(|q| s.position(q.id).unwrap() == pt(-90., -30.)));
    // Rectangle qui partage un coin avec un point existant : pas de doublon.
    let n = p.sheet(sheet).unwrap().points.len();
    let o = rect(&mut p, sheet, 610., 470., 40., 30.);
    assert_eq!(o.points.len(), 3);
    assert_eq!(p.sheet(sheet).unwrap().points.len(), n + 3);
}

#[test]
fn rectangle_refuse_les_valeurs_invalides_sans_effet() {
    let (mut p, sheet) = base();
    let before = p.clone();
    for (w, h) in [
        (0.0, 10.0),
        (10.0, 0.0),
        (f64::NAN, 10.0),
        (10.0, f64::INFINITY),
        (2.0e6, 10.0),
    ] {
        let r = p.apply(Command::AddRectangle {
            sheet,
            corner: PointRef::New(pt(0., 0.)),
            width_mm: w,
            height_mm: h,
        });
        assert!(r.is_err(), "{w} × {h} aurait dû être refusé");
        assert_eq!(p, before);
    }
    // Un rectangle superposé à un côté existant est refusé (segment en double).
    rect(&mut p, sheet, 0., 0., 100., 50.);
    let before = p.clone();
    assert!(p
        .apply(Command::AddRectangle {
            sheet,
            corner: PointRef::New(pt(0., 0.)),
            width_mm: 100.0,
            height_mm: 80.0,
        })
        .is_err());
    assert_eq!(p, before);
}

#[test]
fn parallele_de_meme_longueur() {
    let (mut p, sheet) = base();
    let o = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(pt(0., 0.)),
            to: PointRef::New(pt(120., 50.)),
        })
        .unwrap();
    let reference = o.entities[0];
    for side in [Side::Left, Side::Right] {
        let o = p
            .apply(Command::AddParallel {
                sheet,
                reference,
                distance_mm: 20.0,
                side,
            })
            .unwrap();
        let s = p.sheet(sheet).unwrap();
        let id = o.entities[0];
        let (a, b) = s.segment_ends(id).unwrap();
        let (ra, rb) = s.segment_ends(reference).unwrap();
        assert!((entity_length(s, id).unwrap().value - 130.0).abs() < 1e-9);
        // Distance au support : 20 mm aux deux extrémités.
        assert!((pf_geometry::project_on_segment(a, ra, rb).unwrap().1 - 20.0).abs() < 1e-9);
        assert!((pf_geometry::project_on_segment(b, ra, rb).unwrap().1 - 20.0).abs() < 1e-9);
        assert_eq!(pf_geometry::side_of(ra, rb, a), Some(side));
        // Direction identique.
        assert!(((b - a).cross(rb - ra)).abs() < 1e-9);
    }
    let before = p.clone();
    for d in [0.0, -5.0, f64::NAN] {
        assert!(p
            .apply(Command::AddParallel {
                sheet,
                reference,
                distance_mm: d,
                side: Side::Left
            })
            .is_err());
        assert_eq!(p, before);
    }
}

#[test]
fn rotation_d_un_rectangle_conserve_longueurs_et_aire() {
    let (mut p, sheet) = base();
    let o = rect(&mut p, sheet, 0., 0., 120., 80.);
    let t = Transform::Rotate {
        center: pt(0., 0.),
        angle_deg: 90.0,
    };
    p.apply(Command::Transform {
        sheet,
        points: o.points.clone(),
        transform: t,
    })
    .unwrap();
    let s = p.sheet(sheet).unwrap();
    let positions: Vec<PointMm> = o.points.iter().map(|id| s.position(*id).unwrap()).collect();
    assert_eq!(
        positions,
        vec![pt(0., 0.), pt(0., 120.), pt(-80., 120.), pt(-80., 0.)]
    );
    let m = measure_contour(s, &closed_contours(s)[0]).unwrap();
    assert_eq!((m.perimeter_mm, m.area_mm2), (400.0, 9600.0));
}

#[test]
fn symetrie_de_courbe_transforme_les_poignees() {
    let (mut p, sheet) = base();
    let o = p
        .apply(Command::AddCurve {
            sheet,
            start: PointRef::New(pt(0., 0.)),
            c1: pt(30., 40.),
            c2: pt(70., 40.),
            end: PointRef::New(pt(100., 0.)),
        })
        .unwrap();
    let curve = o.entities[0];
    let before = p.sheet(sheet).unwrap().curve_geometry(curve).unwrap();
    let len = entity_length(p.sheet(sheet).unwrap(), curve).unwrap().value;
    p.apply(Command::Transform {
        sheet,
        points: o.points.clone(),
        transform: Transform::Mirror {
            axis_point: pt(0., 0.),
            axis_angle_deg: 0.0,
        },
    })
    .unwrap();
    let s = p.sheet(sheet).unwrap();
    let g = s.curve_geometry(curve).unwrap();
    assert_eq!((g.c1, g.c2), (pt(30., -40.), pt(70., -40.)));
    assert!(distance_mm(g.p3, before.p3) < 1e-12);
    assert!((entity_length(s, curve).unwrap().value - len).abs() < 1e-9);

    // Un seul point déplacé : la forme se déforme (étirement), sans erreur.
    p.apply(Command::Transform {
        sheet,
        points: vec![o.points[1]],
        transform: Transform::Translate {
            dx_mm: 50.0,
            dy_mm: 0.0,
        },
    })
    .unwrap();
}

#[test]
fn transformation_sans_point_libre_refusee_et_derives_suivent() {
    let (mut p, sheet) = base();
    let o = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(pt(0., 0.)),
            to: PointRef::New(pt(100., 0.)),
        })
        .unwrap();
    let mid = p
        .apply(Command::AddPointAt {
            sheet,
            point: PointRef::Derived(PointDefinition::OnSegmentRatio {
                segment: o.entities[0],
                ratio: 0.5,
            }),
        })
        .unwrap()
        .points[0];
    let before = p.clone();
    let err = p
        .apply(Command::Transform {
            sheet,
            points: vec![mid],
            transform: Transform::Translate {
                dx_mm: 5.0,
                dy_mm: 0.0,
            },
        })
        .unwrap_err();
    assert!(err.to_string().contains("point libre"), "{err}");
    assert_eq!(p, before);
    // Déplacer les extrémités : le milieu suit.
    p.apply(Command::Transform {
        sheet,
        points: o.points.clone(),
        transform: Transform::Translate {
            dx_mm: 10.0,
            dy_mm: 20.0,
        },
    })
    .unwrap();
    assert_eq!(p.sheet(sheet).unwrap().position(mid).unwrap(), pt(60., 20.));
    // Sortie de l'espace de travail : refusée, rien ne change.
    let before = p.clone();
    assert!(p
        .apply(Command::Transform {
            sheet,
            points: o.points,
            transform: Transform::Translate {
                dx_mm: 5.0e6,
                dy_mm: 0.0
            },
        })
        .is_err());
    assert_eq!(p, before);
}

#[test]
fn duplication_avec_symetrie_garde_role_et_forme() {
    let (mut p, sheet) = base();
    let o = rect(&mut p, sheet, 0., 0., 100., 60.);
    for e in &o.entities {
        p.apply(Command::SetPathRole {
            sheet,
            entity: *e,
            role: PathRole::CuttingContour,
        })
        .unwrap();
    }
    // Symétrie par rapport à l'axe x = 100 : les copies partagent les points de l'axe.
    let c = p
        .apply(Command::Duplicate {
            sheet,
            entities: o.entities.clone(),
            transform: Transform::Mirror {
                axis_point: pt(100., 0.),
                axis_angle_deg: 90.0,
            },
        })
        .unwrap();
    // Le côté x = 100 est sur l'axe : sa copie est ce même segment, réutilisé.
    assert_eq!(c.entities.len(), 3);
    assert_eq!(
        c.points.len(),
        2,
        "seuls deux nouveaux points : les deux autres sont sur l'axe"
    );
    let s = p.sheet(sheet).unwrap();
    assert!(c
        .entities
        .iter()
        .all(|id| s.entity(*id).unwrap().role == PathRole::CuttingContour));
    assert_eq!(s.entities.len(), 7);

    // Duplication par déplacement : copie indépendante.
    let (mut p, sheet) = base();
    let o = rect(&mut p, sheet, 0., 0., 100., 60.);
    let c = p
        .apply(Command::Duplicate {
            sheet,
            entities: o.entities.clone(),
            transform: Transform::Translate {
                dx_mm: 150.0,
                dy_mm: 0.0,
            },
        })
        .unwrap();
    assert_eq!((c.points.len(), c.entities.len()), (4, 4));
    assert_eq!(closed_contours(p.sheet(sheet).unwrap()).len(), 2);
    // Copier sur place : refusé (segments en double), rien ne change.
    let before = p.clone();
    assert!(p
        .apply(Command::Duplicate {
            sheet,
            entities: o.entities,
            transform: Transform::Translate {
                dx_mm: 0.0,
                dy_mm: 0.0
            },
        })
        .is_err());
    assert_eq!(p, before);
    assert!(p
        .apply(Command::Duplicate {
            sheet,
            entities: vec![],
            transform: Transform::Translate {
                dx_mm: 1.0,
                dy_mm: 1.0
            },
        })
        .is_err());
}

#[test]
fn nouvelles_commandes_serialisables_et_enregistrables() {
    let (mut p, sheet) = base();
    let o = rect(&mut p, sheet, 0., 0., 100., 60.);
    p.apply(Command::AddParallel {
        sheet,
        reference: o.entities[0],
        distance_mm: 15.0,
        side: Side::Right,
    })
    .unwrap();
    let back = persist::from_json(&persist::to_json(&p).unwrap()).unwrap();
    assert_eq!(back, p);
    for c in [
        Command::AddRectangle {
            sheet,
            corner: PointRef::New(pt(1., 2.)),
            width_mm: -3.0,
            height_mm: 4.0,
        },
        Command::Transform {
            sheet,
            points: o.points.clone(),
            transform: Transform::Rotate {
                center: pt(0., 0.),
                angle_deg: 33.0,
            },
        },
        Command::Duplicate {
            sheet,
            entities: o.entities.clone(),
            transform: Transform::Mirror {
                axis_point: pt(1., 1.),
                axis_angle_deg: 45.0,
            },
        },
    ] {
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(serde_json::from_str::<Command>(&json).unwrap(), c, "{json}");
    }
}

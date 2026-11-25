//! Points dérivés (proportion, intersection, point sur courbe), dépendances,
//! compatibilité des fichiers v1 et sérialisation des commandes.

use pf_core::*;
use pf_geometry::PointMm;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

struct Fx {
    p: Project,
    sheet: SheetId,
    a: PointId,
    b: PointId,
    ab: EntityId,
    cd: EntityId,
    d: PointId,
}

/// AB horizontal (0,0)→(120,0) et CD qui le croise en (40,0) : (40,-30)→(40,50).
fn fx() -> Fx {
    let mut p = Project::new("Dérivés");
    let sheet = p.sheets[0].id;
    let o = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(pt(0., 0.)),
            to: PointRef::New(pt(120., 0.)),
        })
        .unwrap();
    let (a, b, ab) = (o.points[0], o.points[1], o.entities[0]);
    let o = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(pt(40., -30.)),
            to: PointRef::New(pt(40., 50.)),
        })
        .unwrap();
    Fx {
        p,
        sheet,
        a,
        b,
        ab,
        cd: o.entities[0],
        d: o.points[1],
    }
}

fn add(p: &mut Project, sheet: SheetId, definition: PointDefinition) -> Result<Outcome, CoreError> {
    p.apply(Command::AddPointAt {
        sheet,
        point: PointRef::Derived(definition),
    })
}

fn pos(f: &Fx, id: PointId) -> PointMm {
    f.p.sheet(f.sheet).unwrap().position(id).unwrap()
}

#[test]
fn milieu_suit_son_segment_et_sans_doublon() {
    let mut f = fx();
    let mid = PointDefinition::OnSegmentRatio {
        segment: f.ab,
        ratio: 0.5,
    };
    let m = add(&mut f.p, f.sheet, mid).unwrap().points[0];
    assert_eq!(pos(&f, m), pt(60., 0.));
    // Redemander le même point n'en crée pas un second.
    assert!(add(&mut f.p, f.sheet, mid).unwrap().points.is_empty());

    f.p.apply(Command::MovePoint {
        sheet: f.sheet,
        point: f.b,
        to: pt(100., 80.),
    })
    .unwrap();
    assert_eq!(pos(&f, m), pt(50., 40.));

    // Divisions : tiers du segment.
    f.p.apply(Command::SetPointParameter {
        sheet: f.sheet,
        point: m,
        value: 0.25,
    })
    .unwrap();
    assert_eq!(pos(&f, m), pt(25., 20.));
    for bad in [-0.1, 1.5, f64::NAN, f64::INFINITY] {
        let before = f.p.clone();
        assert!(f
            .p
            .apply(Command::SetPointParameter {
                sheet: f.sheet,
                point: m,
                value: bad
            })
            .is_err());
        assert_eq!(f.p, before);
    }
}

#[test]
fn intersection_suit_ses_parents_et_refuse_le_parallelisme() {
    let mut f = fx();
    let x = add(
        &mut f.p,
        f.sheet,
        PointDefinition::Intersection {
            first: f.ab,
            second: f.cd,
        },
    )
    .unwrap()
    .points[0];
    assert_eq!(pos(&f, x), pt(40., 0.));

    // CD pivote autour de son pied : l'intersection glisse sur AB.
    f.p.apply(Command::MovePoint {
        sheet: f.sheet,
        point: f.d,
        to: pt(80., 50.),
    })
    .unwrap();
    let q = pos(&f, x);
    assert!((q.y).abs() < 1e-12 && (q.x - 55.0).abs() < 1e-9, "{q:?}");

    // Rendre CD parallèle à AB : refusé, rien ne change.
    let before = f.p.clone();
    let err =
        f.p.apply(Command::MovePoint {
            sheet: f.sheet,
            point: f.d,
            to: pt(140., -30.),
        })
        .unwrap_err();
    assert!(err.to_string().contains("parallèles"), "{err}");
    assert_eq!(f.p, before);

    // Un point dérivé ne se déplace pas librement.
    assert!(f
        .p
        .apply(Command::MovePoint {
            sheet: f.sheet,
            point: x,
            to: pt(0., 0.)
        })
        .is_err());

    // Un segment peut partir d'un point dérivé.
    let o =
        f.p.apply(Command::AddSegment {
            sheet: f.sheet,
            from: PointRef::Existing(x),
            to: PointRef::New(pt(0., 60.)),
        })
        .unwrap();
    assert_eq!(o.points.len(), 1);
}

#[test]
fn point_sur_courbe_suit_la_courbe() {
    let mut f = fx();
    let o =
        f.p.apply(Command::AddCurve {
            sheet: f.sheet,
            start: PointRef::Existing(f.a),
            c1: pt(30., 40.),
            c2: pt(70., 40.),
            end: PointRef::New(pt(100., 0.)),
        })
        .unwrap();
    let curve = o.entities[0];
    let c = add(
        &mut f.p,
        f.sheet,
        PointDefinition::OnCurve { curve, t: 0.5 },
    )
    .unwrap()
    .points[0];
    assert_eq!(pos(&f, c), pt(50., 30.));
    f.p.apply(Command::MoveCurveHandle {
        sheet: f.sheet,
        curve,
        handle: CurveHandle::Start,
        to: pt(30., 80.),
    })
    .unwrap();
    assert_eq!(pos(&f, c), pt(50., 45.));
    assert!(add(
        &mut f.p,
        f.sheet,
        PointDefinition::OnCurve { curve, t: 1.2 }
    )
    .is_err());
    // Une courbe n'est pas un segment, et inversement.
    assert!(add(
        &mut f.p,
        f.sheet,
        PointDefinition::OnSegmentRatio {
            segment: curve,
            ratio: 0.5
        }
    )
    .is_err());
    assert!(add(
        &mut f.p,
        f.sheet,
        PointDefinition::OnCurve {
            curve: f.ab,
            t: 0.5
        }
    )
    .is_err());
}

#[test]
fn references_mortes_et_suppression() {
    let mut f = fx();
    let before = f.p.clone();
    assert!(add(
        &mut f.p,
        f.sheet,
        PointDefinition::Intersection {
            first: f.ab,
            second: EntityId(9999)
        }
    )
    .is_err());
    assert_eq!(f.p, before);

    let x = add(
        &mut f.p,
        f.sheet,
        PointDefinition::Intersection {
            first: f.ab,
            second: f.cd,
        },
    )
    .unwrap()
    .points[0];
    let m = add(
        &mut f.p,
        f.sheet,
        PointDefinition::OnSegmentRatio {
            segment: f.cd,
            ratio: 0.5,
        },
    )
    .unwrap()
    .points[0];
    // Supprimer CD : les deux points qui en dépendaient sont figés, signalés, conservés.
    let o =
        f.p.apply(Command::Delete {
            sheet: f.sheet,
            points: vec![],
            entities: vec![f.cd],
        })
        .unwrap();
    assert_eq!(o.notes.len(), 2, "{:?}", o.notes);
    let s = f.p.sheet(f.sheet).unwrap();
    assert!(s.point(x).unwrap().definition.is_free());
    assert_eq!(s.position(x).unwrap(), pt(40., 0.));
    assert_eq!(s.position(m).unwrap(), pt(40., 10.));
    f.p.validate().unwrap();
}

#[test]
fn dependance_circulaire_detectee_sans_debordement() {
    // Fichier fabriqué : le segment part d'un point défini… sur ce même segment.
    let mut f = fx();
    let m = add(
        &mut f.p,
        f.sheet,
        PointDefinition::OnSegmentRatio {
            segment: f.ab,
            ratio: 0.5,
        },
    )
    .unwrap()
    .points[0];
    let json = persist::to_json(&f.p).unwrap();
    let cyclic = json.replacen(
        &format!("\"from\": {}", f.a.0),
        &format!("\"from\": {}", m.0),
        1,
    );
    assert_ne!(cyclic, json);
    let err = persist::from_json(&cyclic).unwrap_err();
    assert!(matches!(err, CoreError::CyclicReference(_)), "{err:?}");

    // Même chose avec une courbe dont le départ est un point de cette courbe.
    let o =
        f.p.apply(Command::AddCurve {
            sheet: f.sheet,
            start: PointRef::New(pt(0., 100.)),
            c1: pt(30., 140.),
            c2: pt(70., 140.),
            end: PointRef::New(pt(100., 100.)),
        })
        .unwrap();
    let (curve, start) = (o.entities[0], o.points[0]);
    let on = add(
        &mut f.p,
        f.sheet,
        PointDefinition::OnCurve { curve, t: 0.5 },
    )
    .unwrap()
    .points[0];
    let json = persist::to_json(&f.p).unwrap();
    let cyclic = json.replacen(
        &format!("\"start\": {}", start.0),
        &format!("\"start\": {}", on.0),
        1,
    );
    assert_ne!(cyclic, json);
    assert!(matches!(
        persist::from_json(&cyclic),
        Err(CoreError::CyclicReference(_))
    ));
}

#[test]
fn enregistrement_stable_de_toutes_les_definitions() {
    let mut f = fx();
    add(
        &mut f.p,
        f.sheet,
        PointDefinition::OnSegmentRatio {
            segment: f.ab,
            ratio: 1.0 / 7.0,
        },
    )
    .unwrap();
    add(
        &mut f.p,
        f.sheet,
        PointDefinition::Intersection {
            first: f.ab,
            second: f.cd,
        },
    )
    .unwrap();
    add(
        &mut f.p,
        f.sheet,
        PointDefinition::OnSegment {
            segment: f.ab,
            distance_mm: 100.0,
            origin: SegmentEnd::End,
        },
    )
    .unwrap();
    let json = persist::to_json(&f.p).unwrap();
    let back = persist::from_json(&json).unwrap();
    assert_eq!(back, f.p);
    assert_eq!(persist::to_json(&back).unwrap(), json);
    assert!(json.contains("\"schema_version\": 6"));
    assert!(json.contains("\"type\": \"intersection\"") && json.contains("on_segment_ratio"));
}

#[test]
fn fichiers_v1_toujours_lisibles() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let text = std::fs::read_to_string(dir.join("v1-exercice.pforge.json")).unwrap();
    assert!(text.contains("\"schema_version\": 1"));
    let old = persist::from_json(&text).expect("un projet v1 doit s'ouvrir");
    assert_eq!(old.schema_version, SCHEMA_VERSION);
    assert_eq!(
        old,
        demo_project().unwrap(),
        "contenu v1 conservé à l'identique"
    );
    // Réenregistré en v2, il se rouvre à l'identique.
    let saved = persist::to_json(&old).unwrap();
    assert_eq!(persist::from_json(&saved).unwrap(), old);

    let big = std::fs::read_to_string(dir.join("v1-grande-piece.pforge.json")).unwrap();
    let big = persist::from_json(&big).unwrap();
    assert_eq!(big.sheets[0].entities.len(), 4);
    assert_eq!(big.pieces[0].quantity, 2);
    assert!(big.sheets[0]
        .entities
        .iter()
        .all(|e| e.role == PathRole::CuttingContour));

    // Version 0, 5 ou 99 : refus explicite.
    for v in ["0", "7", "99"] {
        let other = text.replace("\"schema_version\": 1", &format!("\"schema_version\": {v}"));
        assert!(matches!(
            persist::from_json(&other),
            Err(CoreError::UnsupportedVersion { .. })
        ));
    }
}

#[test]
fn commandes_serialisables_aller_retour() {
    let f = fx();
    let commands = vec![
        Command::CreateSheet {
            name: "Poche".into(),
        },
        Command::AddSegmentPolar {
            sheet: f.sheet,
            from: PointRef::Existing(f.a),
            length_mm: 120.5,
            angle_deg: 90.0,
        },
        Command::AddPointAt {
            sheet: f.sheet,
            point: PointRef::Derived(PointDefinition::Intersection {
                first: f.ab,
                second: f.cd,
            }),
        },
        Command::AddCurve {
            sheet: f.sheet,
            start: PointRef::New(pt(0.1, 0.2)),
            c1: pt(1., 2.),
            c2: pt(3., 4.),
            end: PointRef::Existing(f.b),
        },
        Command::Delete {
            sheet: f.sheet,
            points: vec![f.a],
            entities: vec![f.ab],
        },
    ];
    for c in commands {
        let json = serde_json::to_string(&c).unwrap();
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c, "{json}");
    }
    let json = r#"{"type":"add_segment_polar","sheet":1,"from":{"new":{"x":0.0,"y":0.0}},"length_mm":50.0,"angle_deg":0.0}"#;
    assert!(serde_json::from_str::<Command>(json).is_ok());
    assert!(serde_json::from_str::<Command>(r#"{"type":"format_disk"}"#).is_err());
}

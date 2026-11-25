//! Tests d'acceptation du cœur documentaire (références AT-xx du cahier v0.3).

use pf_core::measure::{angle_between_segments, curve_chord, entity_length, point_distance};
use pf_core::*;
use pf_geometry::{PointMm, Side};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

/// Projet avec AB = 120 mm à 0° et BC perpendiculaire de 80 mm.
struct Fixture {
    p: Project,
    sheet: SheetId,
    a: PointId,
    b: PointId,
    c: PointId,
    ab: EntityId,
    bc: EntityId,
}

fn fixture() -> Fixture {
    let mut p = Project::new("Test");
    let sheet = p.sheets[0].id;
    let o = p
        .apply(Command::AddSegmentPolar {
            sheet,
            from: PointRef::New(pt(0., 0.)),
            length_mm: 120.0,
            angle_deg: 0.0,
        })
        .unwrap();
    let (a, b, ab) = (o.points[0], o.points[1], o.entities[0]);
    let o = p
        .apply(Command::AddPerpendicular {
            sheet,
            reference: ab,
            from: PointRef::Existing(b),
            length_mm: 80.0,
            side: Side::Left,
        })
        .unwrap();
    Fixture {
        p,
        sheet,
        a,
        b,
        c: o.points[0],
        ab,
        bc: o.entities[0],
    }
}

#[test]
fn at01_at06_segment_perpendiculaire_et_angle() {
    let f = fixture();
    let s = f.p.sheet(f.sheet).unwrap();
    assert_eq!(entity_length(s, f.ab).unwrap().value, 120.0);
    assert_eq!(entity_length(s, f.bc).unwrap().value, 80.0);
    assert_eq!(angle_between_segments(s, f.ab, f.bc).unwrap().value, 90.0);
    assert_eq!(angle_between_segments(s, f.bc, f.ab).unwrap().value, 90.0);
    assert_eq!(s.position(f.c).unwrap(), pt(120., 80.));
    assert_eq!(s.entity_name(f.ab), "Segment AB");
    assert_eq!(s.entity_name(f.bc), "Segment BC");
    // Les paramètres de création sont mémorisés comme provenance.
    match &s.entity(f.ab).unwrap().kind {
        EntityKind::Segment {
            construction:
                Some(Construction::Polar {
                    length_mm,
                    angle_deg,
                    ..
                }),
            ..
        } => {
            assert_eq!((*length_mm, *angle_deg), (120.0, 0.0));
        }
        other => panic!("construction inattendue : {other:?}"),
    }
}

#[test]
fn at02_point_a_42_mm_et_suivi_du_segment() {
    let mut f = fixture();
    let o =
        f.p.apply(Command::AddPointOnSegment {
            sheet: f.sheet,
            segment: f.ab,
            distance_mm: 42.0,
            origin: SegmentEnd::Start,
        })
        .unwrap();
    let pid = o.points[0];
    let s = f.p.sheet(f.sheet).unwrap();
    assert_eq!(s.position(pid).unwrap(), pt(42., 0.));
    assert_eq!(point_distance(s, f.a, pid).unwrap().value, 42.0);
    assert_eq!(s.label(pid), "D");

    // Mode « développé » : P reste à 42 mm de A quand B est déplacé.
    f.p.apply(Command::MovePoint {
        sheet: f.sheet,
        point: f.b,
        to: pt(0., 150.),
    })
    .unwrap();
    let s = f.p.sheet(f.sheet).unwrap();
    let pos = s.position(pid).unwrap();
    assert!((pos.x).abs() < 1e-12 && (pos.y - 42.0).abs() < 1e-12);
    assert!((point_distance(s, f.a, pid).unwrap().value - 42.0).abs() < 1e-12);

    // Depuis l'autre extrémité.
    let o =
        f.p.apply(Command::AddPointOnSegment {
            sheet: f.sheet,
            segment: f.ab,
            distance_mm: 10.0,
            origin: SegmentEnd::End,
        })
        .unwrap();
    let s = f.p.sheet(f.sheet).unwrap();
    assert!((point_distance(s, f.b, o.points[0]).unwrap().value - 10.0).abs() < 1e-12);
}

#[test]
fn at03_distance_hors_segment_refusee_sans_effet() {
    let mut f = fixture();
    let before = f.p.clone();
    let err =
        f.p.apply(Command::AddPointOnSegment {
            sheet: f.sheet,
            segment: f.ab,
            distance_mm: 130.0,
            origin: SegmentEnd::Start,
        })
        .unwrap_err();
    assert!(err.to_string().contains("130"), "{err}");
    assert_eq!(f.p, before, "une commande refusée ne doit rien modifier");

    // Raccourcir AB sous 42 mm alors que P en dépend : refusé explicitement.
    let o =
        f.p.apply(Command::AddPointOnSegment {
            sheet: f.sheet,
            segment: f.ab,
            distance_mm: 42.0,
            origin: SegmentEnd::Start,
        })
        .unwrap();
    let before = f.p.clone();
    assert!(f
        .p
        .apply(Command::MovePoint {
            sheet: f.sheet,
            point: f.b,
            to: pt(30., 0.)
        })
        .is_err());
    assert_eq!(f.p, before);
    // Modifier la distance (paramètre de construction) est possible et borné.
    f.p.apply(Command::SetPointDistance {
        sheet: f.sheet,
        point: o.points[0],
        distance_mm: 60.0,
    })
    .unwrap();
    assert!(f
        .p
        .apply(Command::SetPointDistance {
            sheet: f.sheet,
            point: o.points[0],
            distance_mm: 121.0
        })
        .is_err());
}

#[test]
fn at04_at05_at14_courbe_longueur_developpee_et_poignees() {
    let mut p = Project::new("Courbe");
    let sheet = p.sheets[0].id;
    let o = p
        .apply(Command::AddCurve {
            sheet,
            start: PointRef::New(pt(0., 0.)),
            c1: pt(30., 0.),
            c2: pt(70., 0.),
            end: PointRef::New(pt(100., 0.)),
        })
        .unwrap();
    let curve = o.entities[0];
    let s = p.sheet(sheet).unwrap();
    assert!((entity_length(s, curve).unwrap().value - 100.0).abs() < 1e-6);
    assert_eq!(curve_chord(s, curve).unwrap().value, 100.0);

    p.apply(Command::MoveCurveHandle {
        sheet,
        curve,
        handle: CurveHandle::Start,
        to: pt(30., 40.),
    })
    .unwrap();
    let s = p.sheet(sheet).unwrap();
    let bowed = entity_length(s, curve).unwrap().value;
    assert!(bowed > 100.5, "la longueur doit se recalculer : {bowed}");
    assert_eq!(
        curve_chord(s, curve).unwrap().value,
        100.0,
        "la corde ne change pas"
    );

    // La poignée suit son extrémité quand on déplace celle-ci.
    let start = o.points[0];
    p.apply(Command::MovePoint {
        sheet,
        point: start,
        to: pt(0., 10.),
    })
    .unwrap();
    let g = p.sheet(sheet).unwrap().curve_geometry(curve).unwrap();
    assert_eq!(g.c1, pt(30., 50.));
}

#[test]
fn at10_geometries_degenerees_refusees() {
    let mut f = fixture();
    let before = f.p.clone();
    let sheet = f.sheet;
    let cases = vec![
        Command::AddSegment {
            sheet,
            from: PointRef::Existing(f.a),
            to: PointRef::Existing(f.a),
        },
        Command::AddSegment {
            sheet,
            from: PointRef::New(pt(5., 5.)),
            to: PointRef::New(pt(5., 5.)),
        },
        Command::AddSegment {
            sheet,
            from: PointRef::Existing(f.a),
            to: PointRef::Existing(f.b),
        },
        Command::AddSegmentPolar {
            sheet,
            from: PointRef::Existing(f.a),
            length_mm: 0.0,
            angle_deg: 0.0,
        },
        Command::AddSegmentPolar {
            sheet,
            from: PointRef::Existing(f.a),
            length_mm: -10.0,
            angle_deg: 0.0,
        },
        Command::AddSegmentPolar {
            sheet,
            from: PointRef::Existing(f.a),
            length_mm: f64::NAN,
            angle_deg: 0.0,
        },
        Command::AddSegmentPolar {
            sheet,
            from: PointRef::Existing(f.a),
            length_mm: 10.0,
            angle_deg: f64::INFINITY,
        },
        Command::AddPoint {
            sheet,
            at: pt(f64::NAN, 0.),
        },
        Command::AddPoint {
            sheet,
            at: pt(1.0e9, 0.),
        },
        Command::AddCurve {
            sheet,
            start: PointRef::Existing(f.a),
            c1: pt(0., 0.),
            c2: pt(0., 0.),
            end: PointRef::Existing(f.a),
        },
        Command::MovePoint {
            sheet,
            point: f.b,
            to: pt(0., 0.),
        },
        Command::MovePoint {
            sheet,
            point: f.b,
            to: pt(f64::NAN, 0.),
        },
        Command::RenameSheet {
            sheet,
            name: "   ".into(),
        },
        Command::CreateSheet {
            name: String::new(),
        },
        Command::DeleteSheet { sheet },
    ];
    for cmd in cases {
        let label = format!("{cmd:?}");
        let err = f.p.apply(cmd).expect_err(&label);
        assert!(!err.to_string().is_empty());
        assert_eq!(f.p, before, "état modifié malgré l'erreur : {label}");
    }
}

#[test]
fn at07_huit_feuilles_sauvegarde_rechargement() {
    let mut p = demo_project().unwrap();
    let names = ["Dos", "Poche", "Rabat", "Doublure", "Soufflet"];
    for n in names {
        p.apply(Command::CreateSheet { name: n.into() }).unwrap();
    }
    assert_eq!(p.sheets.len(), 8);
    let third = p.sheets[2].id;
    p.apply(Command::RenameSheet {
        sheet: third,
        name: "Anse gauche".into(),
    })
    .unwrap();
    let last = p.sheets[7].id;
    p.apply(Command::MoveSheet {
        sheet: last,
        new_index: 1,
    })
    .unwrap();
    p.apply(Command::AddPiece {
        sheet: third,
        name: "Anse".into(),
        quantity: 2,
        material: Some("Cuir".into()),
        mirrored_pair: false,
    })
    .unwrap();

    let dir = tempdir("at07");
    let path = dir.join("huit-feuilles.pforge.json");
    persist::save(&path, &p).unwrap();
    let reloaded = persist::load(&path).unwrap();
    assert_eq!(reloaded, p, "aucun nom, ordre ni entité ne doit être perdu");
    let order: Vec<&str> = reloaded.sheets.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        order,
        [
            "Devant",
            "Soufflet",
            "Fond",
            "Anse gauche",
            "Dos",
            "Poche",
            "Rabat",
            "Doublure"
        ]
    );
    // Deuxième cycle : la sérialisation est stable octet pour octet.
    assert_eq!(
        persist::to_json(&reloaded).unwrap(),
        persist::to_json(&p).unwrap()
    );
    // Les nouveaux identifiants ne réutilisent jamais les anciens.
    let mut r = reloaded;
    let before_max = r.next_id;
    let o = r
        .apply(Command::CreateSheet {
            name: "Neuvième".into(),
        })
        .unwrap();
    assert_eq!(o.sheet.unwrap().0, before_max);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn at15_modification_puis_rechargement_coordonnees_exactes() {
    let mut f = fixture();
    // Valeur sans représentation décimale courte : doit survivre bit à bit.
    let target = pt(120.0 + 1.0 / 3.0, -0.1 - 0.2);
    f.p.apply(Command::MovePoint {
        sheet: f.sheet,
        point: f.c,
        to: target,
    })
    .unwrap();
    let json = persist::to_json(&f.p).unwrap();
    let back = persist::from_json(&json).unwrap();
    assert_eq!(back, f.p);
    assert_eq!(back.sheet(f.sheet).unwrap().position(f.c).unwrap(), target);
}

#[test]
fn at16_feuille_sans_contour_ferme_enregistrable() {
    let mut p = Project::new("Incomplet");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddPoint {
        sheet,
        at: pt(10., 10.),
    })
    .unwrap();
    p.apply(Command::AddSegment {
        sheet,
        from: PointRef::New(pt(0., 0.)),
        to: PointRef::New(pt(50., 0.)),
    })
    .unwrap();
    p.apply(Command::CreateSheet {
        name: "Vide".into(),
    })
    .unwrap();
    let back = persist::from_json(&persist::to_json(&p).unwrap()).unwrap();
    assert_eq!(back, p);
}

#[test]
fn at17_fichiers_malformes_et_versions() {
    let good = persist::to_json(&demo_project().unwrap()).unwrap();

    assert!(matches!(
        persist::from_json("{ pas du json"),
        Err(CoreError::Malformed(_))
    ));
    assert!(matches!(
        persist::from_json("[1,2,3]"),
        Err(CoreError::NotAPatternForgeFile)
    ));
    assert!(matches!(
        persist::from_json("{\"a\":1}"),
        Err(CoreError::NotAPatternForgeFile)
    ));
    assert!(matches!(
        persist::from_json(&good.replace("\"schema_version\": 6", "\"schema_version\": 7")),
        Err(CoreError::UnsupportedVersion {
            found: 7,
            supported: 6
        })
    ));
    assert!(matches!(
        persist::from_json(&good.replace("\"schema_version\": 6", "\"schema_version\": \"un\"")),
        Err(CoreError::Malformed(_))
    ));
    // Champ obligatoire manquant, type faux, champ inconnu dans un point.
    assert!(persist::from_json(&good.replace("\"sheets\"", "\"feuilles\"")).is_err());
    assert!(persist::from_json(&good.replacen("\"x\": 0.0", "\"x\": \"zéro\"", 1)).is_err());
    assert!(persist::from_json(&good.replacen(
        "\"label\": \"A\"",
        "\"label\": \"A\", \"zz\": 1",
        1
    ))
    .is_err());
    // Fichier tronqué.
    assert!(persist::from_json(&good[..good.len() / 2]).is_err());

    // Le fichier d'origine n'est jamais réécrit par une ouverture en échec.
    let dir = tempdir("at17");
    let path = dir.join("corrompu.pforge.json");
    let garbage = "{ \"format\": \"patternforge\", \"schema_version\": 3, \"sheets\": [ ";
    std::fs::write(&path, garbage).unwrap();
    assert!(Document::open(&path).is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), garbage);
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn champs_inconnus_de_haut_niveau_conserves() {
    let good = persist::to_json(&demo_project().unwrap()).unwrap();
    let with_extra = good.replacen(
        "\"format\": \"patternforge\"",
        "\"format\": \"patternforge\", \"tailles\": [\"38\", \"40\"]",
        1,
    );
    let p = persist::from_json(&with_extra).unwrap();
    assert!(persist::to_json(&p).unwrap().contains("\"tailles\""));
}

#[test]
fn at18_references_mortes_detectees() {
    let mut f = fixture();
    // Créer un point sur un segment supprimé.
    f.p.apply(Command::Delete {
        sheet: f.sheet,
        points: vec![],
        entities: vec![f.bc],
    })
    .unwrap();
    let err =
        f.p.apply(Command::AddPointOnSegment {
            sheet: f.sheet,
            segment: f.bc,
            distance_mm: 10.0,
            origin: SegmentEnd::Start,
        })
        .unwrap_err();
    assert!(matches!(err, CoreError::UnknownEntity(_)));
    assert!(f
        .p
        .apply(Command::AddSegment {
            sheet: f.sheet,
            from: PointRef::Existing(f.c),
            to: PointRef::Existing(f.a),
        })
        .is_err());
    assert!(f
        .p
        .apply(Command::AddPoint {
            sheet: SheetId(9999),
            at: pt(0., 0.)
        })
        .is_err());

    // Fichier dont un segment pointe vers un point absent.
    let json = persist::to_json(&f.p).unwrap();
    let broken = json.replacen(&format!("\"to\": {}", f.b.0), "\"to\": 777", 1);
    assert_ne!(broken, json);
    let err = persist::from_json(&broken).unwrap_err();
    assert!(matches!(err, CoreError::DeadReference(_)), "{err:?}");
    // Identifiant en double.
    let dup = json.replacen(
        &format!("\"id\": {}", f.b.0),
        &format!("\"id\": {}", f.a.0),
        1,
    );
    assert!(persist::from_json(&dup).is_err());
}

#[test]
fn at12_suppression_controlee() {
    let mut f = fixture();
    let o =
        f.p.apply(Command::AddPointOnSegment {
            sheet: f.sheet,
            segment: f.bc,
            distance_mm: 20.0,
            origin: SegmentEnd::Start,
        })
        .unwrap();
    let on_bc = o.points[0];

    // Supprimer un point encore utilisé : refus explicite.
    let before = f.p.clone();
    let err =
        f.p.apply(Command::Delete {
            sheet: f.sheet,
            points: vec![f.b],
            entities: vec![],
        })
        .unwrap_err();
    assert!(err.to_string().contains("utilisé"), "{err}");
    assert_eq!(f.p, before);

    // Supprimer BC : AB et ses points restent, C (orphelin) disparaît,
    // le point construit sur BC est figé à sa position et signalé.
    let o =
        f.p.apply(Command::Delete {
            sheet: f.sheet,
            points: vec![],
            entities: vec![f.bc],
        })
        .unwrap();
    assert_eq!(o.notes.len(), 1);
    let s = f.p.sheet(f.sheet).unwrap();
    assert!(s.entity(f.ab).is_ok());
    assert!(s.entity(f.bc).is_err());
    assert!(s.point(f.a).is_ok() && s.point(f.b).is_ok());
    assert!(s.point(f.c).is_err());
    assert_eq!(s.position(on_bc).unwrap(), pt(120., 20.));
    assert!(matches!(
        s.point(on_bc).unwrap().definition,
        PointDefinition::Free { .. }
    ));
    f.p.validate().unwrap();
}

#[test]
fn historique_annuler_retablir_et_gestes_groupes() {
    let f = fixture();
    let mut doc = Document::new(f.p.clone());
    assert!(!doc.is_dirty());
    doc.execute(Command::CreateSheet {
        name: "Fond".into(),
    })
    .unwrap();
    assert!(doc.is_dirty());
    assert!(doc.undo());
    assert_eq!(doc.project(), &f.p);
    assert!(!doc.is_dirty());
    assert!(doc.redo());
    assert_eq!(doc.project().sheets.len(), 2);

    // Un glisser de 50 étapes = une seule étape d'historique.
    doc.begin_group();
    for i in 1..=50 {
        doc.execute(Command::MovePoint {
            sheet: f.sheet,
            point: f.c,
            to: pt(120., 80. + i as f64),
        })
        .unwrap();
    }
    doc.end_group();
    assert!(doc.undo());
    assert_eq!(
        doc.project().sheet(f.sheet).unwrap().position(f.c).unwrap(),
        pt(120., 80.)
    );

    // Une commande refusée ne crée pas d'étape.
    assert!(doc
        .execute(Command::CreateSheet { name: "".into() })
        .is_err());
    assert!(doc.undo());
    assert!(!doc.undo());
}

#[test]
fn etiquettes_automatiques_et_fusion_des_points_confondus() {
    let mut f = fixture();
    // Un nouveau segment partant des coordonnées exactes de A réutilise A.
    let o =
        f.p.apply(Command::AddSegment {
            sheet: f.sheet,
            from: PointRef::New(pt(0., 0.)),
            to: PointRef::New(pt(0., 50.)),
        })
        .unwrap();
    assert_eq!(o.points.len(), 1, "pas de doublon de A");
    let s = f.p.sheet(f.sheet).unwrap();
    assert_eq!(s.label(o.points[0]), "D");
    assert!(f
        .p
        .apply(Command::SetPointLabel {
            sheet: f.sheet,
            point: f.c,
            label: "A".into()
        })
        .is_err());
    f.p.apply(Command::SetPointLabel {
        sheet: f.sheet,
        point: f.c,
        label: "Sommet".into(),
    })
    .unwrap();
}

#[test]
fn projet_de_demonstration_valide() {
    let p = demo_project().unwrap();
    p.validate().unwrap();
    assert_eq!(
        p.sheets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["Devant", "Fond", "Anse"]
    );
}

fn tempdir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("pf-core-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn document_enregistrer_rouvrir_et_etat_modifie() {
    let dir = tempdir("doc");
    let path = dir.join("doc.pforge.json");
    let mut doc = Document::new(demo_project().unwrap());
    doc.execute(Command::CreateSheet {
        name: "Poche".into(),
    })
    .unwrap();
    assert!(doc.is_dirty());
    doc.save_as(&path).unwrap();
    assert!(!doc.is_dirty());
    assert_eq!(doc.path.as_deref(), Some(path.as_path()));
    // Aucun fichier temporaire ne subsiste a cote du projet.
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);

    let reopened = Document::open(&path).unwrap();
    assert_eq!(reopened.project(), doc.project());
    assert!(!reopened.is_dirty() && !reopened.can_undo());

    // Reenregistrer par-dessus un fichier existant le remplace proprement.
    doc.execute(Command::CreateSheet {
        name: "Rabat".into(),
    })
    .unwrap();
    doc.save_as(&path).unwrap();
    assert_eq!(Document::open(&path).unwrap().project().sheets.len(), 5);
    std::fs::remove_dir_all(dir).ok();
}

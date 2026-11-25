//! US-01.2 — calques : création, visibilité, verrouillage, annulation, fichier.

use pf_core::persist;
use pf_core::*;
use pf_geometry::{PointMm, Transform};
use serde_json::Value;
use std::path::PathBuf;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn setup() -> (Project, SheetId) {
    let p = Project::new("calques");
    let s = p.sheets[0].id;
    (p, s)
}

fn add_layer(p: &mut Project, sheet: SheetId, name: &str) -> LayerId {
    p.apply(Command::AddLayer {
        sheet,
        name: name.into(),
    })
    .unwrap()
    .layers[0]
}

/// Crée un segment (60 mm sur l'axe x) dans le calque `layer`, points compris.
fn segment_in(p: &mut Project, sheet: SheetId, layer: LayerId, x: f64) -> Outcome {
    p.apply(Command::InLayer {
        sheet,
        layer,
        command: Box::new(Command::AddSegment {
            sheet,
            from: PointRef::New(pt(x, 0.0)),
            to: PointRef::New(pt(x + 60.0, 0.0)),
        }),
    })
    .unwrap()
}

fn locked_err(p: &mut Project, command: Command) {
    let before = p.clone();
    let err = p.apply(command).expect_err("doit être refusée");
    assert!(
        matches!(err, CoreError::LayerLocked(_)),
        "erreur inattendue : {err}"
    );
    assert!(err.to_string().contains("verrouillé"), "{err}");
    assert_eq!(*p, before, "une commande refusée ne laisse aucune trace");
}

// ---------------------------------------------------------------- CA 1

#[test]
fn ca1_creer_renommer_reordonner_supprimer() {
    let (mut p, s) = setup();
    let a = add_layer(&mut p, s, "Base");
    let b = add_layer(&mut p, s, "Finitions");
    let c = add_layer(&mut p, s, "Notes");
    let names = |p: &Project| -> Vec<String> {
        p.sheets[0].layers.iter().map(|l| l.name.clone()).collect()
    };
    assert_eq!(names(&p), ["Base", "Finitions", "Notes"]);
    assert!(p.sheets[0].layers.iter().all(|l| l.visible && !l.locked));

    p.apply(Command::RenameLayer {
        sheet: s,
        layer: b,
        name: "  Détails ".into(),
    })
    .unwrap();
    assert_eq!(names(&p), ["Base", "Détails", "Notes"]);

    p.apply(Command::MoveLayer {
        sheet: s,
        layer: c,
        new_index: 0,
    })
    .unwrap();
    assert_eq!(names(&p), ["Notes", "Base", "Détails"]);

    // Noms uniques et non vides.
    for bad in ["Base", "", "   "] {
        let before = p.clone();
        assert!(p
            .apply(Command::RenameLayer {
                sheet: s,
                layer: b,
                name: bad.into(),
            })
            .is_err());
        assert!(p
            .apply(Command::AddLayer {
                sheet: s,
                name: bad.into()
            })
            .is_err());
        assert_eq!(p, before);
    }
    assert!(matches!(
        p.apply(Command::RenameLayer {
            sheet: s,
            layer: LayerId(9999),
            name: "x".into()
        }),
        Err(CoreError::UnknownLayer(_))
    ));

    // Suppression d'un calque vide.
    p.apply(Command::DeleteLayer {
        sheet: s,
        layer: b,
        move_to: None,
    })
    .unwrap();
    assert_eq!(names(&p), ["Notes", "Base"]);

    // Calque non vide : refusée sans effet…
    let seg = segment_in(&mut p, s, a, 0.0);
    let before = p.clone();
    let err = p
        .apply(Command::DeleteLayer {
            sheet: s,
            layer: a,
            move_to: None,
        })
        .expect_err("calque non vide");
    assert!(err.to_string().contains("contient"), "{err}");
    assert_eq!(p, before);
    // …ou objets déplacés si `move_to` est fourni.
    let out = p
        .apply(Command::DeleteLayer {
            sheet: s,
            layer: a,
            move_to: Some(c),
        })
        .unwrap();
    assert!(!out.notes.is_empty());
    let sheet = &p.sheets[0];
    assert!(sheet.layer(a).is_err());
    assert_eq!(sheet.entity(seg.entities[0]).unwrap().layer, Some(c));
    for pid in &seg.points {
        assert_eq!(sheet.point(*pid).unwrap().layer, Some(c));
    }
    // Destination inconnue ou identique : refusée.
    assert!(p
        .apply(Command::DeleteLayer {
            sheet: s,
            layer: c,
            move_to: Some(c)
        })
        .is_err());
    assert!(p
        .apply(Command::DeleteLayer {
            sheet: s,
            layer: c,
            move_to: Some(LayerId(777))
        })
        .is_err());
}

#[test]
fn ca1_deplacer_des_objets_vers_un_calque_ou_le_calque_par_defaut() {
    let (mut p, s) = setup();
    let a = add_layer(&mut p, s, "A");
    let out = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(0.0, 0.0)),
            to: PointRef::New(pt(10.0, 0.0)),
        })
        .unwrap();
    assert_eq!(p.sheets[0].entity(out.entities[0]).unwrap().layer, None);
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: out.points.clone(),
        entities: out.entities.clone(),
        layer: Some(a),
    })
    .unwrap();
    assert_eq!(p.sheets[0].entity(out.entities[0]).unwrap().layer, Some(a));
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![],
        entities: out.entities.clone(),
        layer: None,
    })
    .unwrap();
    assert_eq!(p.sheets[0].entity(out.entities[0]).unwrap().layer, None);
    // Calque ou objet inconnu : refusé.
    assert!(p
        .apply(Command::SetObjectsLayer {
            sheet: s,
            points: vec![],
            entities: out.entities,
            layer: Some(LayerId(4242)),
        })
        .is_err());
}

#[test]
fn les_objets_crees_via_in_layer_et_leurs_derives_vont_dans_le_calque() {
    let (mut p, s) = setup();
    let a = add_layer(&mut p, s, "Actif");
    let seg = segment_in(&mut p, s, a, 0.0);
    // Point dérivé (milieu) créé dans le calque actif.
    let mid = p
        .apply(Command::InLayer {
            sheet: s,
            layer: a,
            command: Box::new(Command::AddPointAt {
                sheet: s,
                point: PointRef::Derived(PointDefinition::OnSegmentRatio {
                    segment: seg.entities[0],
                    ratio: 0.5,
                }),
            }),
        })
        .unwrap();
    assert_eq!(p.sheets[0].point(mid.points[0]).unwrap().layer, Some(a));
    // Sans InLayer : calque par défaut.
    let out = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(pt(0.0, 50.0)),
            width_mm: 10.0,
            height_mm: 10.0,
        })
        .unwrap();
    assert!(out
        .entities
        .iter()
        .all(|e| p.sheets[0].entity(*e).unwrap().layer.is_none()));
}

// ---------------------------------------------------------------- CA 2 (modèle)

#[test]
fn ca2_calque_masque_objets_non_visibles_calque_par_defaut_toujours_visible() {
    let (mut p, s) = setup();
    let a = add_layer(&mut p, s, "A");
    let in_a = segment_in(&mut p, s, a, 0.0);
    let outside = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(0.0, 20.0)),
            to: PointRef::New(pt(10.0, 20.0)),
        })
        .unwrap();
    let notch = p
        .apply(Command::AddNotch {
            sheet: s,
            entity: in_a.entities[0],
            distance_mm: 10.0,
            side: pf_geometry::Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap()
        .notches[0];
    p.apply(Command::SetLayerVisible {
        sheet: s,
        layer: a,
        visible: false,
    })
    .unwrap();
    let sheet = &p.sheets[0];
    assert!(!sheet.entity_visible(in_a.entities[0]));
    assert!(!sheet.point_visible(in_a.points[0]));
    assert!(!sheet.notch_visible(sheet.notch(notch).unwrap()));
    assert!(sheet.entity_visible(outside.entities[0]));
    assert!(sheet.layer_visible(None));
    // Cotation : masquée dès qu'un de ses points l'est.
    p.apply(Command::AddDimension {
        sheet: s,
        from: in_a.points[0],
        to: outside.points[0],
        offset_mm: 5.0,
    })
    .unwrap();
    let sheet = &p.sheets[0];
    assert!(!sheet.dimension_visible(&sheet.dimensions[0]));
}

// ---------------------------------------------------------------- CA 3

#[test]
fn ca3_calque_verrouille_reste_visible_et_accrochable_mais_refuse_les_modifications() {
    let (mut p, s) = setup();
    p.apply(Command::CreateSheet {
        name: "Feuille 2".into(),
    })
    .unwrap();
    let l = add_layer(&mut p, s, "Fixe");
    let seg = segment_in(&mut p, s, l, 0.0);
    let (e, a, b) = (seg.entities[0], seg.points[0], seg.points[1]);
    // Un cran et une cotation sur l'objet, avant verrouillage.
    let notch = p
        .apply(Command::AddNotch {
            sheet: s,
            entity: e,
            distance_mm: 10.0,
            side: pf_geometry::Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap()
        .notches[0];
    let dim = p
        .apply(Command::AddDimension {
            sheet: s,
            from: a,
            to: b,
            offset_mm: 5.0,
        })
        .unwrap()
        .dimensions[0];
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer: l,
        locked: true,
    })
    .unwrap();
    assert!(
        p.sheets[0].layer_visible(Some(l)),
        "verrouillé reste visible"
    );

    locked_err(
        &mut p,
        Command::MovePoint {
            sheet: s,
            point: a,
            to: pt(5.0, 5.0),
        },
    );
    locked_err(
        &mut p,
        Command::Transform {
            sheet: s,
            points: vec![a, b],
            transform: Transform::Translate {
                dx_mm: 10.0,
                dy_mm: 0.0,
            },
        },
    );
    locked_err(
        &mut p,
        Command::Delete {
            sheet: s,
            points: vec![],
            entities: vec![e],
        },
    );
    locked_err(
        &mut p,
        Command::SetPathRole {
            sheet: s,
            entity: e,
            role: PathRole::CuttingContour,
        },
    );
    locked_err(
        &mut p,
        Command::SetPointLabel {
            sheet: s,
            point: a,
            label: "Z".into(),
        },
    );
    locked_err(
        &mut p,
        Command::SetNotch {
            sheet: s,
            notch,
            distance_mm: 20.0,
            side: pf_geometry::Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        },
    );
    locked_err(&mut p, Command::DeleteNotch { sheet: s, notch });
    locked_err(
        &mut p,
        Command::SetDimensionOffset {
            sheet: s,
            dimension: dim,
            offset_mm: 9.0,
        },
    );
    locked_err(
        &mut p,
        Command::DeleteDimension {
            sheet: s,
            dimension: dim,
        },
    );
    locked_err(
        &mut p,
        Command::AddNotch {
            sheet: s,
            entity: e,
            distance_mm: 30.0,
            side: pf_geometry::Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        },
    );
    // Sortir un objet du calque verrouillé, ou en supprimer le calque
    // (même avec destination), est aussi une modification.
    locked_err(
        &mut p,
        Command::SetObjectsLayer {
            sheet: s,
            points: vec![],
            entities: vec![e],
            layer: None,
        },
    );
    let other = add_layer(&mut p, s, "Autre");
    locked_err(
        &mut p,
        Command::DeleteLayer {
            sheet: s,
            layer: l,
            move_to: Some(other),
        },
    );
    locked_err(&mut p, Command::DeleteSheet { sheet: s });
    // Atomicité : un lot dont un élément est refusé n'a aucun effet.
    locked_err(
        &mut p,
        Command::Batch {
            commands: vec![
                Command::AddPoint {
                    sheet: s,
                    at: pt(500.0, 500.0),
                },
                Command::MovePoint {
                    sheet: s,
                    point: b,
                    to: pt(1.0, 1.0),
                },
            ],
        },
    );
    // Ajouter dans un calque verrouillé est refusé ; ailleurs, tout reste possible.
    locked_err(
        &mut p,
        Command::InLayer {
            sheet: s,
            layer: l,
            command: Box::new(Command::AddPoint {
                sheet: s,
                at: pt(300.0, 300.0),
            }),
        },
    );
    p.apply(Command::AddPoint {
        sheet: s,
        at: pt(300.0, 300.0),
    })
    .unwrap();
    // Une cotation entre points verrouillés peut être ajoutée (lecture seule).
    // Déverrouiller rend tout modifiable.
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer: l,
        locked: false,
    })
    .unwrap();
    p.apply(Command::MovePoint {
        sheet: s,
        point: a,
        to: pt(5.0, 5.0),
    })
    .unwrap();
}

#[test]
fn ca3_un_trace_verrouille_protege_ses_deux_points() {
    let (mut p, s) = setup();
    let l = add_layer(&mut p, s, "Tracé fixe");
    // Points dans le calque par défaut, tracé dans le calque verrouillé.
    let a = p
        .apply(Command::AddPoint {
            sheet: s,
            at: pt(0.0, 0.0),
        })
        .unwrap()
        .points[0];
    let b = p
        .apply(Command::AddPoint {
            sheet: s,
            at: pt(50.0, 0.0),
        })
        .unwrap()
        .points[0];
    let e = p
        .apply(Command::InLayer {
            sheet: s,
            layer: l,
            command: Box::new(Command::AddSegment {
                sheet: s,
                from: PointRef::Existing(a),
                to: PointRef::Existing(b),
            }),
        })
        .unwrap()
        .entities[0];
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer: l,
        locked: true,
    })
    .unwrap();
    assert_eq!(p.sheets[0].point(a).unwrap().layer, None);
    locked_err(
        &mut p,
        Command::MovePoint {
            sheet: s,
            point: a,
            to: pt(3.0, 3.0),
        },
    );
    locked_err(
        &mut p,
        Command::Transform {
            sheet: s,
            points: vec![b],
            transform: Transform::Translate {
                dx_mm: 1.0,
                dy_mm: 0.0,
            },
        },
    );
    // Un point construit sur le tracé verrouillé (calque par défaut) reste modifiable…
    let m = p
        .apply(Command::AddPointOnSegment {
            sheet: s,
            segment: e,
            distance_mm: 10.0,
            origin: SegmentEnd::Start,
        })
        .unwrap()
        .points[0];
    p.apply(Command::SetPointDistance {
        sheet: s,
        point: m,
        distance_mm: 20.0,
    })
    .unwrap();
    // … mais le supprimer avec le tracé ne l'est pas.
    locked_err(
        &mut p,
        Command::Delete {
            sheet: s,
            points: vec![m],
            entities: vec![e],
        },
    );
}

#[test]
fn ca3_un_point_verrouille_ne_peut_pas_etre_deplace_par_son_tracé_parent() {
    let (mut p, s) = setup();
    let l = add_layer(&mut p, s, "Repères");
    // Segment CD libre ; point P construit dessus, rangé dans le calque verrouillé.
    let cd = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(0.0, 0.0)),
            to: PointRef::New(pt(100.0, 0.0)),
        })
        .unwrap();
    let pp = p
        .apply(Command::InLayer {
            sheet: s,
            layer: l,
            command: Box::new(Command::AddPointOnSegment {
                sheet: s,
                segment: cd.entities[0],
                distance_mm: 40.0,
                origin: SegmentEnd::Start,
            }),
        })
        .unwrap()
        .points[0];
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer: l,
        locked: true,
    })
    .unwrap();
    // Déplacer C déplacerait P : refusé. Régler la distance de P : refusé.
    locked_err(
        &mut p,
        Command::MovePoint {
            sheet: s,
            point: cd.points[1],
            to: pt(0.0, 100.0),
        },
    );
    locked_err(
        &mut p,
        Command::SetPointDistance {
            sheet: s,
            point: pp,
            distance_mm: 10.0,
        },
    );
    // Ce qui ne bouge pas P reste possible (l'autre extrémité n'a pas d'effet sur P
    // car la distance est mesurée depuis le départ : déplacer D change le segment mais
    // pas la position de P sur l'axe x).
    p.apply(Command::MovePoint {
        sheet: s,
        point: cd.points[1],
        to: pt(150.0, 0.0),
    })
    .unwrap();
}

// ---------------------------------------------------------------- CA 4

#[test]
fn ca4_toutes_les_commandes_de_calque_sont_annulables() {
    let (p, s) = setup();
    let mut doc = Document::new(p);
    let initial = doc.project().clone();
    let a = doc
        .execute(Command::AddLayer {
            sheet: s,
            name: "A".into(),
        })
        .unwrap()
        .layers[0];
    let out = doc
        .execute(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(0.0, 0.0)),
            to: PointRef::New(pt(10.0, 0.0)),
        })
        .unwrap();
    let steps = vec![
        Command::RenameLayer {
            sheet: s,
            layer: a,
            name: "B".into(),
        },
        Command::SetLayerVisible {
            sheet: s,
            layer: a,
            visible: false,
        },
        Command::SetLayerLocked {
            sheet: s,
            layer: a,
            locked: true,
        },
        Command::SetLayerLocked {
            sheet: s,
            layer: a,
            locked: false,
        },
        Command::SetObjectsLayer {
            sheet: s,
            points: out.points.clone(),
            entities: out.entities.clone(),
            layer: Some(a),
        },
        Command::AddLayer {
            sheet: s,
            name: "C".into(),
        },
        Command::MoveLayer {
            sheet: s,
            layer: a,
            new_index: 1,
        },
    ];
    for c in steps {
        let before = doc.project().clone();
        doc.execute(c).unwrap();
        assert_ne!(*doc.project(), before);
        assert!(doc.undo());
        assert_eq!(*doc.project(), before, "annuler restitue l'état exact");
        assert!(doc.redo());
    }
    // Suppression avec déplacement : annulée en une étape.
    let c = doc.project().sheets[0].layers[0].id;
    let before = doc.project().clone();
    doc.execute(Command::DeleteLayer {
        sheet: s,
        layer: a,
        move_to: Some(if c == a {
            doc.project().sheets[0].layers[1].id
        } else {
            c
        }),
    })
    .unwrap();
    assert!(doc.undo());
    assert_eq!(*doc.project(), before);
    while doc.undo() {}
    assert_eq!(*doc.project(), initial);
}

// ---------------------------------------------------------------- CA 5

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

#[test]
fn ca5_fichiers_sans_calque_octets_inchanges_apres_ouverture_et_enregistrement() {
    let mut seen = 0;
    let dirs = [
        examples_dir(),
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures"),
    ];
    for dir in dirs {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if !path.to_string_lossy().ends_with(".pforge.json") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let project = persist::from_json(&text).unwrap();
            let raw: Value = serde_json::from_str(&text).unwrap();
            if raw["schema_version"] != 6 {
                continue; // v1/v2 : migrées vers v3 (autre test)
            }
            assert!(project.sheets.iter().all(|s| s.layers.is_empty()));
            assert_eq!(persist::to_json(&project).unwrap(), text, "{path:?}");
            seen += 1;
        }
    }
    assert!(seen >= 2, "au moins deux fichiers v3 de référence");
}

#[test]
fn ca5_un_projet_sans_calque_n_ecrit_aucun_champ_de_calque() {
    let (mut p, s) = setup();
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::New(pt(0.0, 0.0)),
        to: PointRef::New(pt(10.0, 0.0)),
    })
    .unwrap();
    let json = persist::to_json(&p).unwrap();
    assert!(!json.contains("layer"), "{json}");
    assert_eq!(p.schema_version, 6, "schéma 4 (US-09.1)");
}

#[test]
fn ca5_aller_retour_avec_calques_et_references_mortes_refusees() {
    let (mut p, s) = setup();
    let a = add_layer(&mut p, s, "A");
    add_layer(&mut p, s, "B");
    segment_in(&mut p, s, a, 0.0);
    p.apply(Command::SetLayerVisible {
        sheet: s,
        layer: a,
        visible: false,
    })
    .unwrap();
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer: a,
        locked: true,
    })
    .unwrap();
    let json = persist::to_json(&p).unwrap();
    let back = persist::from_json(&json).unwrap();
    assert_eq!(back, p);
    assert_eq!(persist::to_json(&back).unwrap(), json);
    assert_eq!(p.schema_version, 6);

    // Calque absent alors qu'un objet y renvoie : refusé.
    let mut raw: Value = serde_json::from_str(&json).unwrap();
    raw["sheets"][0].as_object_mut().unwrap().remove("layers");
    let err = persist::from_json(&raw.to_string()).expect_err("référence morte");
    assert!(matches!(err, CoreError::DeadReference(_)), "{err}");

    // Référence vers un calque inexistant.
    let mut raw: Value = serde_json::from_str(&json).unwrap();
    raw["sheets"][0]["entities"][0]["layer"] = Value::from(99_999u64);
    assert!(persist::from_json(&raw.to_string()).is_err());

    // Noms de calques dupliqués ou vides.
    let mut raw: Value = serde_json::from_str(&json).unwrap();
    raw["sheets"][0]["layers"][1]["name"] = raw["sheets"][0]["layers"][0]["name"].clone();
    assert!(persist::from_json(&raw.to_string()).is_err());
    let mut raw: Value = serde_json::from_str(&json).unwrap();
    raw["sheets"][0]["layers"][0]["name"] = Value::from("  ");
    assert!(persist::from_json(&raw.to_string()).is_err());

    // `visible` et `locked` absents : visible, déverrouillé.
    let mut raw: Value = serde_json::from_str(&json).unwrap();
    let layer = raw["sheets"][0]["layers"][0].as_object_mut().unwrap();
    layer.remove("visible");
    layer.remove("locked");
    let loaded = persist::from_json(&raw.to_string()).unwrap();
    assert!(loaded.sheets[0].layers[0].visible && !loaded.sheets[0].layers[0].locked);
}

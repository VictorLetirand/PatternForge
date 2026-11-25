//! US-01.1 — schéma v3 : pièces-entités et migration depuis v1/v2.

use pf_core::persist;
use pf_core::*;
use serde_json::Value;
use std::path::PathBuf;

const FIXTURES: [&str; 4] = [
    "v1-exercice.pforge.json",
    "v1-grande-piece.pforge.json",
    "v2-piece-contour-droit-fil.pforge.json",
    "v2-piece-sans-contour.pforge.json",
];

fn fixture_text(name: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    std::fs::read_to_string(dir.join(name)).unwrap()
}

fn open(name: &str) -> Project {
    persist::from_json(&fixture_text(name)).unwrap_or_else(|e| panic!("{name} : {e}"))
}

/// Positions résolues de tous les points, en bits.
fn geometry_bits(p: &Project) -> Vec<(u64, u64)> {
    p.sheets
        .iter()
        .flat_map(|s| {
            s.points.iter().map(|pt| {
                let pos = s.position(pt.id).unwrap();
                (pos.x.to_bits(), pos.y.to_bits())
            })
        })
        .collect()
}

#[test]
fn ca_a_v1_et_v2_ouverts_reenregistres_en_v4_geometrie_identique_bit_a_bit() {
    for name in FIXTURES {
        let opened = open(name);
        assert_eq!(opened.schema_version, 6, "{name}");
        // Les points libres du fichier d'origine sont relus à l'identique.
        let raw: Value = serde_json::from_str(&fixture_text(name)).unwrap();
        for (si, sheet) in raw["sheets"].as_array().unwrap().iter().enumerate() {
            for (pi, pt) in sheet["points"].as_array().unwrap().iter().enumerate() {
                if pt["definition"]["type"] == "free" {
                    let got = opened.sheets[si].position(opened.sheets[si].points[pi].id);
                    let got = got.unwrap();
                    assert_eq!(
                        got.x.to_bits(),
                        pt["definition"]["x"].as_f64().unwrap().to_bits()
                    );
                    assert_eq!(
                        got.y.to_bits(),
                        pt["definition"]["y"].as_f64().unwrap().to_bits()
                    );
                }
            }
        }
        let saved = persist::to_json(&opened).unwrap();
        assert!(saved.contains("\"schema_version\": 6"), "{name}");
        assert!(
            !saved.contains("\"piece\":"),
            "{name} : ancien champ réécrit"
        );
        let reopened = persist::from_json(&saved).unwrap();
        assert_eq!(geometry_bits(&opened), geometry_bits(&reopened), "{name}");
        assert_eq!(reopened, opened, "{name}");
    }
}

#[test]
fn ca_b_champs_inconnus_preserves_projet_feuille_piece() {
    let base = persist::to_json(&open("v2-piece-contour-droit-fil.pforge.json")).unwrap();
    let mut v: Value = serde_json::from_str(&base).unwrap();
    v["zz_projet"] = serde_json::json!({"a": [1, 2]});
    v["sheets"][0]["zz_feuille"] = serde_json::json!("x");
    v["pieces"][0]["zz_piece"] = serde_json::json!(42);
    let project = persist::from_json(&v.to_string()).unwrap();
    let out: Value = serde_json::from_str(&persist::to_json(&project).unwrap()).unwrap();
    assert_eq!(out["zz_projet"], v["zz_projet"]);
    assert_eq!(out["sheets"][0]["zz_feuille"], "x");
    assert_eq!(out["pieces"][0]["zz_piece"], 42);
}

#[test]
fn ca_c_fichier_v5_refuse_avec_message_clair() {
    let text = persist::to_json(&open("v1-exercice.pforge.json")).unwrap();
    let v5 = text.replace("\"schema_version\": 6", "\"schema_version\": 7");
    let err = persist::from_json(&v5).unwrap_err();
    assert_eq!(
        err,
        CoreError::UnsupportedVersion {
            found: 7,
            supported: 6
        }
    );
    let msg = err.to_string();
    assert!(
        msg.contains("version 7") && msg.contains("plus récent"),
        "{msg}"
    );
}

#[test]
fn ca_d_piece_etiquette_v2_devient_piece_v3_avec_contour_unique() {
    let p = open("v2-piece-contour-droit-fil.pforge.json");
    let sheet = &p.sheets[0];
    assert!(p.migration_notes.is_empty());
    assert_eq!(p.pieces.len(), 1);
    let piece = &p.pieces[0];
    assert_eq!(piece.name, "Rabat");
    assert_eq!(piece.quantity, 2);
    assert_eq!(piece.material.as_deref(), Some("Cuir"));
    assert!(!piece.mirrored_pair);
    assert_eq!(piece.link, PieceLink::Linked { sheet: sheet.id });
    assert_eq!(piece.outline.len(), 4);
    let cutting: Vec<EntityId> = sheet
        .entities
        .iter()
        .filter(|e| e.role == PathRole::CuttingContour)
        .map(|e| e.id)
        .collect();
    let mut in_outline: Vec<EntityId> = piece.outline.iter().map(|e| e.entity).collect();
    in_outline.sort();
    assert_eq!(in_outline, cutting);
    let grain = sheet
        .entities
        .iter()
        .find(|e| e.role == PathRole::Grainline)
        .unwrap()
        .id;
    assert_eq!(piece.grainline, Some(grain));
    // Le contour migré est exactement le contour détecté (sens compris).
    let detected = contour::closed_contours(sheet).remove(0);
    let expected: Vec<(EntityId, bool)> = detected
        .edges
        .iter()
        .map(|e| (e.entity, e.reversed))
        .collect();
    let got: Vec<(EntityId, bool)> = piece
        .outline
        .iter()
        .map(|e| (e.entity, e.reversed))
        .collect();
    assert_eq!(got, expected);
}

#[test]
fn ca_d_piece_sans_contour_unique_migre_avec_contour_vide_et_remarque() {
    let p = open("v2-piece-sans-contour.pforge.json");
    assert_eq!(p.pieces.len(), 1);
    assert!(p.pieces[0].outline.is_empty());
    assert_eq!(p.pieces[0].grainline, None);
    assert_eq!(p.migration_notes.len(), 1);
    assert!(
        p.migration_notes[0].contains("Rabat"),
        "{:?}",
        p.migration_notes
    );
    // La remarque est conservée à l'enregistrement.
    let back = persist::from_json(&persist::to_json(&p).unwrap()).unwrap();
    assert_eq!(back.migration_notes, p.migration_notes);

    // Deux contours de coupe fermés : ambigu, donc vide aussi.
    let mut two = Project::new("Deux contours");
    let sheet = two.sheets[0].id;
    for x in [0.0, 500.0] {
        let r = two
            .apply(Command::AddRectangle {
                sheet,
                corner: PointRef::New(pf_geometry::PointMm::new(x, 0.0)),
                width_mm: 100.0,
                height_mm: 50.0,
            })
            .unwrap();
        for entity in r.entities {
            two.apply(Command::SetPathRole {
                sheet,
                entity,
                role: PathRole::CuttingContour,
            })
            .unwrap();
        }
    }
    let text = persist::to_json(&two)
        .unwrap()
        .replace("\"schema_version\": 6", "\"schema_version\": 2");
    let mut v: Value = serde_json::from_str(&text).unwrap();
    v["sheets"][0]["piece"] = serde_json::json!({"name": "Deux", "quantity": 1});
    let migrated = persist::from_json(&v.to_string()).unwrap();
    assert!(migrated.pieces[0].outline.is_empty());
    assert_eq!(migrated.pieces[0].material, None);
    assert!(migrated.migration_notes[0].contains("2 contours"));
}

#[test]
fn ancienne_fixture_grande_piece_obtient_son_contour() {
    let p = open("v1-grande-piece.pforge.json");
    assert_eq!(p.pieces[0].quantity, 2);
    assert_eq!(p.pieces[0].outline.len(), 4);
    assert!(p.migration_notes.is_empty());
}

fn with_piece() -> (Project, SheetId, PieceId) {
    let mut p = Project::new("Test");
    let sheet = p.sheets[0].id;
    let out = p
        .apply(Command::AddPiece {
            sheet,
            name: "Dos".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap();
    (p, sheet, out.pieces[0])
}

#[test]
fn ca_e_validation_refuse_sans_effet() {
    let (mut p, sheet, piece) = with_piece();
    let seg = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(pf_geometry::PointMm::new(0.0, 0.0)),
            to: PointRef::New(pf_geometry::PointMm::new(10.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let before = p.clone();
    let bad = [
        Command::AddPiece {
            sheet,
            name: "X".into(),
            quantity: 0,
            material: None,
            mirrored_pair: false,
        },
        Command::AddPiece {
            sheet,
            name: "  ".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        },
        Command::AddPiece {
            sheet: SheetId(9999),
            name: "X".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        },
        Command::SetPieceInfo {
            piece,
            name: "Dos".into(),
            quantity: 0,
            material: None,
            mirrored_pair: false,
        },
        Command::SetPieceInfo {
            piece: PieceId(9999),
            name: "Dos".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        },
        Command::SetPieceOutline {
            piece,
            outline: vec![PieceEdge {
                entity: EntityId(9999),
                reversed: false,
            }],
            grainline: None,
        },
        Command::SetPieceOutline {
            piece,
            outline: Vec::new(),
            grainline: Some(EntityId(9999)),
        },
        Command::SetPieceOutline {
            piece,
            outline: vec![
                PieceEdge {
                    entity: seg,
                    reversed: false,
                },
                PieceEdge {
                    entity: seg,
                    reversed: true,
                },
            ],
            grainline: None,
        },
        Command::DeletePiece {
            piece: PieceId(9999),
        },
    ];
    for c in bad {
        assert!(p.apply(c.clone()).is_err(), "{c:?} aurait dû être refusée");
        assert_eq!(p, before, "{c:?} a laissé une trace");
    }

    // Fichier : références mortes, quantité 0, identifiants dupliqués.
    let good = persist::to_json(&{
        let mut q = p.clone();
        q.apply(Command::SetPieceOutline {
            piece,
            outline: vec![PieceEdge {
                entity: seg,
                reversed: false,
            }],
            grainline: Some(seg),
        })
        .unwrap();
        q
    })
    .unwrap();
    let mutate = |f: &dyn Fn(&mut Value)| {
        let mut v: Value = serde_json::from_str(&good).unwrap();
        f(&mut v);
        persist::from_json(&v.to_string())
    };
    assert!(persist::from_json(&good).is_ok());
    assert!(matches!(
        mutate(&|v| v["pieces"][0]["outline"][0]["entity"] = 9999.into()),
        Err(CoreError::DeadReference(_))
    ));
    assert!(matches!(
        mutate(&|v| v["pieces"][0]["grainline"] = 9999.into()),
        Err(CoreError::DeadReference(_))
    ));
    assert!(matches!(
        mutate(&|v| v["pieces"][0]["link"]["sheet"] = 9999.into()),
        Err(CoreError::DeadReference(_))
    ));
    assert!(mutate(&|v| v["pieces"][0]["quantity"] = 0.into()).is_err());
    assert!(mutate(&|v| v["pieces"][0]["name"] = "".into()).is_err());
    let sheet_id = p.sheets[0].id.0;
    assert!(matches!(
        mutate(&|v| v["pieces"][0]["id"] = sheet_id.into()),
        Err(CoreError::DuplicateId(_))
    ));
}

#[test]
fn ca_f_annuler_retablir_ajout_et_modification_de_piece() {
    let mut doc = Document::new(Project::new("Undo"));
    let sheet = doc.project().sheets[0].id;
    let start = doc.project().clone();
    let id = doc
        .execute(Command::AddPiece {
            sheet,
            name: "Poche".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    let added = doc.project().clone();
    doc.execute(Command::SetPieceInfo {
        piece: id,
        name: "Poche plaquée".into(),
        quantity: 3,
        material: Some("Lin".into()),
        mirrored_pair: true,
    })
    .unwrap();
    let edited = doc.project().clone();
    let p = edited.piece(id).unwrap();
    assert_eq!((p.quantity, p.mirrored_pair), (3, true));
    assert_eq!(p.material.as_deref(), Some("Lin"));

    assert!(doc.undo());
    assert_eq!(*doc.project(), added);
    assert!(doc.undo());
    assert_eq!(*doc.project(), start);
    assert!(doc.project().pieces.is_empty());
    assert!(doc.redo());
    assert_eq!(*doc.project(), added);
    assert!(doc.redo());
    assert_eq!(*doc.project(), edited);
}

#[test]
fn ca_g_aller_retour_json_stable_octet_par_octet() {
    for name in FIXTURES {
        let first = persist::to_json(&open(name)).unwrap();
        let second = persist::to_json(&persist::from_json(&first).unwrap()).unwrap();
        assert_eq!(first, second, "{name}");
    }
    let (p, _, _) = with_piece();
    let first = persist::to_json(&p).unwrap();
    assert_eq!(
        persist::to_json(&persist::from_json(&first).unwrap()).unwrap(),
        first
    );
    // Sans pièce ni remarque, ni `pieces` ni `migration_notes` dans le fichier.
    let bare = persist::to_json(&Project::new("Vide")).unwrap();
    assert!(!bare.contains("pieces") && !bare.contains("migration_notes"));
}

#[test]
fn decision_36_supprimer_un_trace_reference_vide_le_contour_et_previent() {
    let mut p = open("v2-piece-contour-droit-fil.pforge.json");
    let sheet = p.sheets[0].id;
    let piece = p.pieces[0].id;
    let outline_edge = p.pieces[0].outline[0].entity;
    let out = p
        .apply(Command::Delete {
            sheet,
            points: Vec::new(),
            entities: vec![outline_edge],
        })
        .unwrap();
    assert!(p.piece(piece).unwrap().outline.is_empty());
    assert!(p.piece(piece).unwrap().grainline.is_some());
    assert!(out.notes.iter().any(|n| n.contains("contour de la pièce")));

    let grain = p.pieces[0].grainline.unwrap();
    let out = p
        .apply(Command::Delete {
            sheet,
            points: Vec::new(),
            entities: vec![grain],
        })
        .unwrap();
    assert_eq!(p.piece(piece).unwrap().grainline, None);
    assert!(out.notes.iter().any(|n| n.contains("droit-fil")));
}

#[test]
fn supprimer_une_feuille_supprime_ses_pieces_liees() {
    let (mut p, _, _) = with_piece();
    let first = p.sheets[0].id;
    p.apply(Command::CreateSheet {
        name: "Autre".into(),
    })
    .unwrap();
    let out = p.apply(Command::DeleteSheet { sheet: first }).unwrap();
    assert!(p.pieces.is_empty());
    assert!(!out.notes.is_empty());
}

#[test]
fn une_feuille_peut_porter_plusieurs_pieces() {
    let (mut p, sheet, _) = with_piece();
    p.apply(Command::AddPiece {
        sheet,
        name: "Doublure".into(),
        quantity: 1,
        material: Some("Soie".into()),
        mirrored_pair: true,
    })
    .unwrap();
    assert_eq!(p.pieces_of(sheet).count(), 2);
}

#[test]
fn demo_piece_project_produit_une_piece_v3_complete() {
    let p = demo_piece_project().unwrap();
    assert_eq!(p.schema_version, 6);
    assert_eq!(p.pieces.len(), 1);
    assert_eq!(p.pieces[0].outline.len(), 4);
    assert!(p.pieces[0].grainline.is_some());
}

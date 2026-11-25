//! US-04.5 — plan de pièces : placement (translation + rotation de mise en page) des
//! pièces, sans jamais toucher à la construction. Un groupe de tests par critère
//! d'acceptation (CA1 modèle et fichier, CA2 commande, CA3 lecture). Les valeurs
//! attendues sont calculées à la main ou par des formules indépendantes (formule du
//! lacet, distances entre sommets).

use pf_core::persist;
use pf_core::piece_plan::{normalize_rotation_deg, PLAN_GAP_MM, PLAN_WIDTH_MM};
use pf_core::*;
use pf_geometry::PointMm;
use serde_json::{json, Value};
use std::path::PathBuf;

const FIXTURES: [&str; 4] = [
    "v1-exercice.pforge.json",
    "v1-grande-piece.pforge.json",
    "v2-piece-contour-droit-fil.pforge.json",
    "v2-piece-sans-contour.pforge.json",
];

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn pl(x: f64, y: f64, r: f64) -> Placement {
    Placement {
        x_mm: x,
        y_mm: y,
        rotation_deg: r,
    }
}

fn fixture(name: &str) -> Project {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let text = std::fs::read_to_string(dir.join(name)).unwrap();
    persist::from_json(&text).unwrap_or_else(|e| panic!("{name} : {e}"))
}

fn to_value(p: &Project) -> Value {
    serde_json::from_str(&persist::to_json(p).unwrap()).unwrap()
}

/// Projet sans le champ `placement` des pièces : tout le reste, à l'octet près.
fn without_placements(p: &Project) -> String {
    let mut v = serde_json::to_value(p).unwrap();
    if let Some(pieces) = v["pieces"].as_array_mut() {
        for piece in pieces {
            piece.as_object_mut().unwrap().remove("placement");
        }
    }
    serde_json::to_string(&v).unwrap()
}

fn snap(p: &Project) -> String {
    serde_json::to_string(p).unwrap()
}

/// Rectangle `w × h` de coin `(x, y)` sur la feuille, extrait en pièce liée.
fn rect_piece(p: &mut Project, sheet: SheetId, x: f64, y: f64, w: f64, h: f64) -> PieceId {
    p.apply(Command::AddRectangle {
        sheet,
        corner: PointRef::New(pt(x, y)),
        width_mm: w,
        height_mm: h,
    })
    .unwrap();
    p.apply(Command::ExtractPiece {
        sheet,
        source: PieceSource::Click {
            at: pt(x + w / 2.0, y + h / 2.0),
        },
        name: format!("P{}", p.pieces.len() + 1),
        detached: false,
    })
    .unwrap()
    .pieces[0]
}

/// Projet à `n` pièces rectangulaires de tailles variées (feuille unique).
fn many(n: usize) -> (Project, Vec<PieceId>) {
    let mut p = Project::new("Plan");
    let sheet = p.sheets[0].id;
    let mut ids = Vec::new();
    for i in 0..n {
        let (w, h) = (40.0 + (i * 37 % 300) as f64, 30.0 + (i * 53 % 250) as f64);
        // Rectangles posés loin les uns des autres sur la feuille de construction.
        ids.push(rect_piece(&mut p, sheet, (i as f64) * 1000.0, 0.0, w, h));
    }
    (p, ids)
}

fn item(p: &Project, id: PieceId) -> PlanItem {
    p.plan_layout()
        .into_iter()
        .find(|i| i.piece == id)
        .expect("pièce dans le plan")
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

fn shoelace(points: &[PointMm]) -> f64 {
    let n = points.len();
    (0..n)
        .map(|i| {
            let (a, b) = (points[i], points[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f64>()
        .abs()
        / 2.0
}

fn perimeter(points: &[PointMm]) -> f64 {
    let n = points.len();
    (0..n)
        .map(|i| pf_geometry::distance_mm(points[i], points[(i + 1) % n]))
        .sum()
}

// ----- CA1 : modèle et fichier ----------------------------------------------------

#[test]
fn ca1_champ_absent_octets_inchanges_fixtures_et_exemples() {
    for name in FIXTURES {
        let first = persist::to_json(&fixture(name)).unwrap();
        assert!(!first.contains("placement"), "{name} : placement écrit");
        let second = persist::to_json(&persist::from_json(&first).unwrap()).unwrap();
        assert_eq!(first, second, "{name}");
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut seen = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(".pforge.json") {
            continue;
        }
        let shipped = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n");
        let again = persist::to_json(&persist::from_json(&shipped).unwrap()).unwrap();
        assert_eq!(shipped, again, "{}", path.display());
        assert!(!shipped.contains("placement"));
        seen += 1;
    }
    assert!(seen >= 3, "exemples livrés : {seen}");
    // Un projet avec pièces mais sans placement n'écrit rien non plus (même après lecture
    // du plan, qui n'écrit jamais).
    let (p, _) = many(5);
    let before = persist::to_json(&p).unwrap();
    let _ = p.plan_layout();
    assert_eq!(before, persist::to_json(&p).unwrap());
    assert!(!before.contains("placement"));
}

#[test]
fn ca1_champ_present_relu_a_l_identique_schema_inchange() {
    let (mut p, ids) = many(2);
    p.apply(Command::SetPiecePlacement {
        piece: ids[1],
        placement: Some(pl(250.5, -80.25, 37.5)),
    })
    .unwrap();
    let text = persist::to_json(&p).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["schema_version"], 6, "schéma 4 (US-09.1)");
    assert!(v["pieces"][0].get("placement").is_none());
    assert_eq!(
        v["pieces"][1]["placement"],
        json!({"x_mm": 250.5, "y_mm": -80.25, "rotation_deg": 37.5})
    );
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, p);
    assert_eq!(persist::to_json(&back).unwrap(), text);
    // Vrai champ typé, pas une clé inconnue conservée.
    assert!(back.pieces[1].extra.is_empty());
    assert_eq!(back.pieces[1].placement, Some(pl(250.5, -80.25, 37.5)));
}

#[test]
fn ca1_refus_a_l_ouverture_pour_chaque_cas() {
    let (mut p, ids) = many(1);
    p.apply(Command::SetPiecePlacement {
        piece: ids[0],
        placement: Some(pl(10.0, 20.0, 30.0)),
    })
    .unwrap();
    let base = to_value(&p);
    let bad: Vec<(&str, Value)> = vec![
        (
            "clé inconnue",
            json!({"x_mm": 1, "y_mm": 2, "rotation_deg": 3, "zz": 1}),
        ),
        ("rotation absente", json!({"x_mm": 1, "y_mm": 2})),
        ("x absent", json!({"y_mm": 2, "rotation_deg": 3})),
        (
            "x texte",
            json!({"x_mm": "1", "y_mm": 2, "rotation_deg": 3}),
        ),
        ("x nul", json!({"x_mm": null, "y_mm": 2, "rotation_deg": 3})),
        (
            "x trop grand",
            json!({"x_mm": 2.0e6, "y_mm": 2, "rotation_deg": 3}),
        ),
        (
            "y trop petit",
            json!({"x_mm": 1, "y_mm": -1.0e7, "rotation_deg": 3}),
        ),
        (
            "x énorme",
            json!({"x_mm": 1.0e300, "y_mm": 2, "rotation_deg": 3}),
        ),
        (
            "rotation énorme",
            json!({"x_mm": 1, "y_mm": 2, "rotation_deg": 1.0e300}),
        ),
        (
            "rotation hors borne",
            json!({"x_mm": 1, "y_mm": 2, "rotation_deg": -2.0e6}),
        ),
        ("tableau de 4", json!([1, 2, 3, 4])),
        ("tableau de 2", json!([1, 2])),
        ("nombre", json!(5)),
        ("texte", json!("0;0;0")),
    ];
    for (what, value) in bad {
        let mut v = base.clone();
        v["pieces"][0]["placement"] = value;
        assert!(persist::from_json(&v.to_string()).is_err(), "{what}");
    }
    // Valeurs non finies (impossibles en JSON strict, possibles à la main) : refusées
    // par la validation et à l'écriture.
    for bad in [
        pl(f64::NAN, 0.0, 0.0),
        pl(0.0, f64::INFINITY, 0.0),
        pl(0.0, 0.0, f64::NAN),
        pl(0.0, 0.0, f64::NEG_INFINITY),
        pl(0.0, 0.0, 360.0),
        pl(0.0, 0.0, -1.0),
        pl(2.0e6, 0.0, 0.0),
    ] {
        let mut q = p.clone();
        q.pieces[0].placement = Some(bad);
        assert!(q.validate().is_err(), "{bad:?}");
        assert!(persist::to_json(&q).is_err(), "{bad:?}");
    }
    // Homonyme dans `extra` : jamais deux clés identiques écrites.
    let mut q = p.clone();
    q.pieces[0].placement = None;
    q.pieces[0].extra.insert("placement".into(), json!(1));
    assert!(q.validate().unwrap_err().to_string().contains("placement"));
    assert!(persist::to_json(&q).is_err());
    // Le fichier d'origine reste lisible.
    assert!(persist::from_json(&base.to_string()).is_ok());
    // `null` = absent : relu sans placement.
    let mut v = base.clone();
    v["pieces"][0]["placement"] = Value::Null;
    assert_eq!(
        persist::from_json(&v.to_string()).unwrap().pieces[0].placement,
        None
    );
}

#[test]
fn ca1_normalisation_de_la_rotation() {
    // −1e-300 : `rem_euclid` rend exactement 360 ; la valeur stockée est 0.
    assert_eq!(normalize_rotation_deg(-1e-300), 0.0);
    assert!(normalize_rotation_deg(-1e-300).is_sign_positive());
    assert_eq!(normalize_rotation_deg(360.0), 0.0);
    assert_eq!(normalize_rotation_deg(-720.0), 0.0);
    assert_eq!(normalize_rotation_deg(450.0), 90.0);
    assert_eq!(normalize_rotation_deg(-90.0), 270.0);
    assert!(normalize_rotation_deg(-0.0).is_sign_positive());
    for a in [-1e-300, -1e-17, 359.999_999_999_999_9, 1e5 + 0.3, -1e6, 1e6] {
        let n = normalize_rotation_deg(a);
        assert!((0.0..360.0).contains(&n), "{a} -> {n}");
    }
    let (mut p, ids) = many(1);
    for (input, expected) in [
        (-1e-300, 0.0),
        (360.0, 0.0),
        (-720.0, 0.0),
        (725.0, 5.0),
        (-0.0, 0.0),
        (-90.0, 270.0),
    ] {
        p.apply(Command::SetPiecePlacement {
            piece: ids[0],
            placement: Some(pl(1.0, 2.0, input)),
        })
        .unwrap();
        let stored = p.pieces[0].placement.unwrap();
        assert_eq!(stored.rotation_deg, expected, "{input}");
        assert!(stored.rotation_deg.is_sign_positive(), "{input}");
        p.validate().unwrap();
    }
    // Zéro négatif de position écrasé.
    p.apply(Command::SetPiecePlacement {
        piece: ids[0],
        placement: Some(pl(-0.0, -0.0, 0.0)),
    })
    .unwrap();
    let s = p.pieces[0].placement.unwrap();
    assert!(s.x_mm.is_sign_positive() && s.y_mm.is_sign_positive());
    // NaN refusé.
    assert!(p
        .apply(Command::SetPiecePlacement {
            piece: ids[0],
            placement: Some(pl(0.0, 0.0, f64::NAN)),
        })
        .is_err());
    // À l'ouverture : 360, -90, 720 sont ramenés dans [0 ; 360[.
    let mut base = to_value(&p);
    for (raw, expected) in [(360.0, 0.0), (-90.0, 270.0), (720.0, 0.0), (-1e-300, 0.0)] {
        base["pieces"][0]["placement"] = json!({"x_mm": 1, "y_mm": 2, "rotation_deg": raw});
        let q = persist::from_json(&base.to_string()).unwrap();
        assert_eq!(
            q.pieces[0].placement.unwrap().rotation_deg,
            expected,
            "{raw}"
        );
        let text = persist::to_json(&q).unwrap();
        assert_eq!(
            persist::to_json(&persist::from_json(&text).unwrap()).unwrap(),
            text
        );
    }
}

// ----- CA2 : commande SetPiecePlacement -------------------------------------------

#[test]
fn ca2_une_etape_annulable_annuler_retablir_exacts() {
    let (p, ids) = many(3);
    let mut doc = Document::new(p);
    let steps = [
        Some(pl(10.0, 20.0, 30.0)),
        Some(pl(-400.0, 55.5, 90.0)),
        None,
        Some(pl(0.0, 0.0, 0.0)),
    ];
    for placement in steps {
        let before = doc.project().clone();
        let had = doc.can_undo();
        doc.execute(Command::SetPiecePlacement {
            piece: ids[1],
            placement,
        })
        .unwrap();
        let after = doc.project().clone();
        assert_ne!(after, before, "{placement:?}");
        assert!(doc.undo());
        assert_eq!(doc.project(), &before, "undo exact");
        assert_eq!(snap(doc.project()), snap(&before));
        assert_eq!(doc.can_undo(), had, "une seule étape");
        assert!(doc.redo());
        assert_eq!(doc.project(), &after, "redo exact");
        // Les autres pièces n'ont pas bougé.
        assert_eq!(after.pieces[0], before.pieces[0]);
        assert_eq!(after.pieces[2], before.pieces[2]);
    }
}

#[test]
fn ca2_refus_sans_trace() {
    let (p, ids) = many(2);
    let mut doc = Document::new(p);
    doc.execute(Command::SetPiecePlacement {
        piece: ids[0],
        placement: Some(pl(5.0, 6.0, 7.0)),
    })
    .unwrap();
    let bytes = persist::to_json(doc.project()).unwrap();
    let raw = snap(doc.project());
    let undo_before = doc.can_undo();
    let bad = [
        (PieceId(9_999_999), Some(pl(0.0, 0.0, 0.0))),
        (ids[0], Some(pl(f64::NAN, 0.0, 0.0))),
        (ids[0], Some(pl(0.0, f64::INFINITY, 0.0))),
        (ids[0], Some(pl(0.0, 0.0, f64::NAN))),
        (ids[0], Some(pl(0.0, 0.0, f64::INFINITY))),
        (ids[0], Some(pl(2.0e6, 0.0, 0.0))),
        (ids[1], Some(pl(0.0, -1.0e9, 0.0))),
        (ids[1], Some(pl(0.0, 0.0, 1.0e300))),
        (ids[1], Some(pl(0.0, 0.0, -1.0e300))),
        (ids[1], Some(pl(0.0, 0.0, 2.0e6))),
    ];
    for (piece, placement) in bad {
        let e = doc
            .execute(Command::SetPiecePlacement { piece, placement })
            .expect_err(&format!("{placement:?}"));
        assert!(!e.to_string().is_empty());
        assert_eq!(snap(doc.project()), raw, "{placement:?} : trace laissée");
        assert_eq!(persist::to_json(doc.project()).unwrap(), bytes);
        assert_eq!(doc.can_undo(), undo_before, "pas d'étape d'annulation");
    }
    // Sur Project::apply directement aussi.
    let mut p = doc.project().clone();
    assert!(p
        .apply(Command::SetPiecePlacement {
            piece: ids[0],
            placement: Some(pl(f64::NAN, 0.0, 0.0)),
        })
        .is_err());
    assert_eq!(snap(&p), raw);
    // Bornes incluses acceptées.
    assert!(p
        .apply(Command::SetPiecePlacement {
            piece: ids[0],
            placement: Some(pl(1.0e6, -1.0e6, 1.0e6)),
        })
        .is_ok());
}

#[test]
fn ca2_no_op_sans_etape_d_annulation() {
    let (p, ids) = many(2);
    let mut doc = Document::new(p);
    // Déjà automatique : None ne change rien.
    doc.execute(Command::SetPiecePlacement {
        piece: ids[0],
        placement: None,
    })
    .unwrap();
    assert!(!doc.can_undo());
    let q = Some(pl(10.0, 20.0, 30.0));
    doc.execute(Command::SetPiecePlacement {
        piece: ids[0],
        placement: q,
    })
    .unwrap();
    assert!(doc.can_undo());
    assert!(doc.undo());
    assert!(!doc.can_undo());
    assert!(doc.redo());
    // Même valeur (même après normalisation) : aucune nouvelle étape.
    for same in [q, Some(pl(10.0, 20.0, 390.0)), Some(pl(10.0, 20.0, -330.0))] {
        doc.execute(Command::SetPiecePlacement {
            piece: ids[0],
            placement: same,
        })
        .unwrap();
    }
    assert!(doc.undo());
    assert!(!doc.can_undo(), "les répétitions n'ont créé aucune étape");
}

/// Projet avec crans, cotations, droit-fil, fiche, plus des pièces extraites.
fn rich() -> (Project, Vec<PieceId>) {
    let mut p = pf_core::demo_piece_project().unwrap();
    let sheet = p.sheets[0].id;
    let mut ids: Vec<PieceId> = p.pieces.iter().map(|x| x.id).collect();
    ids.push(rect_piece(&mut p, sheet, 500.0, 500.0, 80.0, 60.0));
    // Pièce détachée (copie du contour dans une feuille dédiée).
    p.apply(Command::AddRectangle {
        sheet,
        corner: PointRef::New(pt(900.0, 0.0)),
        width_mm: 50.0,
        height_mm: 50.0,
    })
    .unwrap();
    ids.push(
        p.apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Click {
                at: pt(925.0, 25.0),
            },
            name: "Détachée".into(),
            detached: true,
        })
        .unwrap()
        .pieces[0],
    );
    (p, ids)
}

#[test]
fn ca2_construction_inchangee_a_l_octet_pres_hors_placement() {
    let (mut p, ids) = rich();
    assert!(ids.len() >= 3);
    let baseline = without_placements(&p);
    // Seul le contrôle de rotation du droit-fil (US-06.5) dépend du placement : un
    // avertissement, pas une mesure ; il est écarté de la comparaison.
    let card = |p: &Project, id: PieceId| {
        let mut c = p.piece_card(id).unwrap();
        c.issues
            .retain(|i| *i != pf_core::piece::PieceIssue::GrainRotationExceeded);
        c
    };
    let measures: Vec<_> = p.pieces.iter().map(|x| card(&p, x.id)).collect();
    let notches: usize = p.sheets.iter().map(|s| s.notches.len()).sum();
    let dims: usize = p.sheets.iter().map(|s| s.dimensions.len()).sum();
    assert!(
        notches > 0 && dims > 0,
        "le projet a des crans et cotations"
    );
    let placements = [
        Some(pl(10.0, 20.0, 30.0)),
        Some(pl(-1.0e6, 1.0e6, 359.999)),
        Some(pl(0.0, 0.0, 90.0)),
        None,
        Some(pl(123.456, -78.9, 180.0)),
    ];
    for (i, placement) in placements.into_iter().enumerate() {
        for &id in &ids {
            p.apply(Command::SetPiecePlacement {
                piece: id,
                placement: placement.map(|q| Placement {
                    x_mm: q.x_mm / 3.0 * (i + 1) as f64,
                    ..q
                }),
            })
            .unwrap();
            assert_eq!(without_placements(&p), baseline, "{id:?} {placement:?}");
        }
        let now: Vec<_> = p.pieces.iter().map(|x| card(&p, x.id)).collect();
        assert_eq!(now, measures, "fiches, aires et périmètres inchangés");
        // Tracés, points, crans, cotations feuille par feuille.
        p.validate().unwrap();
    }
}

#[test]
fn ca2_commandes_hors_plan_ne_touchent_pas_aux_placements() {
    let (mut p, ids) = rich();
    let sheet = p.sheets[0].id;
    for (i, &id) in ids.iter().enumerate() {
        p.apply(Command::SetPiecePlacement {
            piece: id,
            placement: Some(pl(i as f64 * 10.0, 5.0, 45.0)),
        })
        .unwrap();
    }
    let placed = |p: &Project| -> Vec<Option<Placement>> {
        ids.iter()
            .map(|id| p.piece(*id).ok().and_then(|x| x.placement))
            .collect()
    };
    let expected = placed(&p);
    p.apply(Command::AddSegment {
        sheet,
        from: PointRef::New(pt(1.0, 1.0)),
        to: PointRef::New(pt(40.0, 3.0)),
    })
    .unwrap();
    p.apply(Command::RenameSheet {
        sheet,
        name: "Autre".into(),
    })
    .unwrap();
    p.apply(Command::CreateSheet { name: "Z".into() }).unwrap();
    p.apply(Command::SetPieceCard {
        piece: ids[0],
        name: "Nouveau nom".into(),
        code: Some("C1".into()),
        quantity: 2,
        mirrored_pair: true,
        material: None,
        category: None,
        comment: None,
    })
    .unwrap();
    p.apply(Command::RenameProject {
        name: "Renommé".into(),
    })
    .unwrap();
    assert_eq!(placed(&p), expected);
}

#[test]
fn ca2_suppression_de_piece_et_de_feuille_supprime_le_placement() {
    let (mut p, ids) = many(3);
    for &id in &ids {
        p.apply(Command::SetPiecePlacement {
            piece: id,
            placement: Some(pl(1.0, 2.0, 3.0)),
        })
        .unwrap();
    }
    let mut doc = Document::new(p);
    let before = doc.project().clone();
    doc.execute(Command::DeletePiece { piece: ids[1] }).unwrap();
    assert!(doc.project().piece(ids[1]).is_err());
    assert_eq!(
        doc.project()
            .pieces
            .iter()
            .filter(|x| x.placement.is_some())
            .count(),
        2
    );
    let text = persist::to_json(doc.project()).unwrap();
    assert_eq!(text.matches("\"placement\"").count(), 2);
    persist::from_json(&text).unwrap();
    // Annuler restaure aussi le placement.
    assert!(doc.undo());
    assert_eq!(doc.project(), &before);
    assert_eq!(
        doc.project().piece(ids[1]).unwrap().placement,
        Some(pl(1.0, 2.0, 3.0))
    );
    // Une commande sur une pièce supprimée est refusée proprement.
    doc.execute(Command::DeletePiece { piece: ids[1] }).unwrap();
    assert!(doc
        .execute(Command::SetPiecePlacement {
            piece: ids[1],
            placement: None
        })
        .is_err());
    // Suppression de la feuille d'une pièce liée : pièce et placement disparaissent.
    let sheet = match doc.project().piece(ids[0]).unwrap().link {
        PieceLink::Linked { sheet } => sheet,
        PieceLink::Detached => unreachable!(),
    };
    let mut q = doc.project().clone();
    q.apply(Command::CreateSheet {
        name: "Reste".into(),
    })
    .unwrap();
    q.apply(Command::DeleteSheet { sheet }).unwrap();
    assert!(q.pieces.is_empty());
    q.validate().unwrap();
}

#[test]
fn ca2_copie_de_feuille_revient_au_placement_automatique() {
    let (mut p, ids) = many(1);
    let sheet = p.sheets[0].id;
    p.apply(Command::SetPiecePlacement {
        piece: ids[0],
        placement: Some(pl(300.0, 300.0, 45.0)),
    })
    .unwrap();
    let out = p.apply(Command::DuplicateSheet { sheet }).unwrap();
    assert_eq!(out.pieces.len(), 1);
    let copy = p.piece(out.pieces[0]).unwrap();
    assert_eq!(
        copy.placement, None,
        "la copie est en placement automatique"
    );
    assert_eq!(
        p.piece(ids[0]).unwrap().placement,
        Some(pl(300.0, 300.0, 45.0)),
        "l'original garde son placement"
    );
    // Jamais superposée exactement : les deux boîtes sont distinctes dans le plan.
    let a = item(&p, ids[0]).bbox.unwrap();
    let b = item(&p, out.pieces[0]).bbox.unwrap();
    assert_ne!(a, b);
    // Import d'une feuille depuis un autre projet : même règle.
    let cmd = p.import_command(sheet).unwrap();
    let out = p.apply(cmd).unwrap();
    assert_eq!(p.piece(out.pieces[0]).unwrap().placement, None);
    p.validate().unwrap();
}

// ----- CA3 : lecture du plan --------------------------------------------------------

#[test]
fn ca3_placement_automatique_deterministe_et_sans_chevauchement() {
    for n in [1usize, 2, 7, 30, 60] {
        let (p, ids) = many(n);
        let a = p.plan_layout();
        let b = p.clone().plan_layout();
        assert_eq!(a, b, "déterministe ({n})");
        assert_eq!(a.len(), n);
        assert!(a.iter().all(|i| !i.manual && i.skipped.is_none()));
        assert!(p.plan_overlaps().is_empty(), "{n} pièces : chevauchement");
        // Les boîtes respectent l'écart et la largeur d'étagère.
        for (i, x) in a.iter().enumerate() {
            let bx = x.bbox.unwrap();
            assert!(bx.min.x >= -1e-9, "étagère à gauche de l'origine");
            if bx.width() <= PLAN_WIDTH_MM {
                assert!(
                    bx.max.x <= PLAN_WIDTH_MM + 1e-9,
                    "{n} : largeur {}",
                    bx.max.x
                );
            }
            for y in &a[i + 1..] {
                let by = y.bbox.unwrap();
                let gap_x = (bx.min.x - by.max.x).max(by.min.x - bx.max.x);
                let gap_y = (bx.min.y - by.max.y).max(by.min.y - bx.max.y);
                assert!(
                    gap_x.max(gap_y) >= PLAN_GAP_MM - 1e-6,
                    "{n} : écart {} entre pièces",
                    gap_x.max(gap_y)
                );
            }
        }
        // Rien n'est écrit dans le projet.
        assert!(p.pieces.iter().all(|x| x.placement.is_none()));
        // L'ordre des pièces dans le fichier ne change pas le plan (tri par identifiant).
        let mut shuffled = p.clone();
        shuffled.pieces.reverse();
        for id in &ids {
            let (x, y) = (item(&p, *id), item(&shuffled, *id));
            assert_eq!(x.bbox, y.bbox);
        }
    }
    // Plusieurs rangées au-delà de la largeur : 60 pièces occupent plus d'une rangée.
    let (p, _) = many(60);
    let rows: std::collections::BTreeSet<i64> = p
        .plan_layout()
        .iter()
        .map(|i| (i.bbox.unwrap().max.y * 1000.0).round() as i64)
        .collect();
    assert!(rows.len() > 1);
}

#[test]
fn ca3_placement_manuel_prioritaire_et_autres_pieces_immobiles() {
    let (mut p, ids) = many(6);
    let before = p.plan_layout();
    p.apply(Command::SetPiecePlacement {
        piece: ids[2],
        placement: Some(pl(5000.0, 5000.0, 0.0)),
    })
    .unwrap();
    let after = p.plan_layout();
    assert!(after[2].manual);
    let c = after[2].placement.unwrap();
    assert_eq!((c.x_mm, c.y_mm, c.rotation_deg), (5000.0, 5000.0, 0.0));
    let bb = after[2].bbox.unwrap();
    assert!(near((bb.min.x + bb.max.x) / 2.0, 5000.0, 1e-9));
    assert!(near((bb.min.y + bb.max.y) / 2.0, 5000.0, 1e-9));
    for i in [0usize, 1, 3, 4, 5] {
        assert!(!after[i].manual);
        assert_eq!(after[i].bbox, before[i].bbox, "pièce {i} n'a pas bougé");
    }
    // Retour à l'automatique : exactement le plan d'origine.
    p.apply(Command::SetPiecePlacement {
        piece: ids[2],
        placement: None,
    })
    .unwrap();
    assert_eq!(p.plan_layout(), before);
}

#[test]
fn ca3_contour_transforme_exact_rectangle_tourne_de_90_degres() {
    let mut p = Project::new("Rect");
    let sheet = p.sheets[0].id;
    // Coin (10, 20) : centre de la boîte (60, 45).
    let id = rect_piece(&mut p, sheet, 10.0, 20.0, 100.0, 50.0);
    p.apply(Command::SetPiecePlacement {
        piece: id,
        placement: Some(pl(200.0, 100.0, 90.0)),
    })
    .unwrap();
    let it = item(&p, id);
    assert!(it.manual);
    assert_eq!(it.origin, Some(pt(60.0, 45.0)));
    // Sommets relatifs au centre (±50, ±25), tournés de 90° : (x, y) → (−y, x), puis
    // + (200, 100) : (225, 50), (225, 150), (175, 150), (175, 50).
    let expected = [
        pt(225.0, 50.0),
        pt(225.0, 150.0),
        pt(175.0, 150.0),
        pt(175.0, 50.0),
    ];
    assert_eq!(it.outline.len(), 4, "{:?}", it.outline);
    for e in expected {
        assert!(
            it.outline
                .iter()
                .any(|v| near(v.x, e.x, 1e-9) && near(v.y, e.y, 1e-9)),
            "sommet {e:?} absent de {:?}",
            it.outline
        );
    }
    let bb = it.bbox.unwrap();
    assert_eq!((bb.min, bb.max), (pt(175.0, 50.0), pt(225.0, 150.0)));
    assert!(near(it.area_mm2.unwrap(), 5000.0, 1e-9));
    assert!(near(it.perimeter_mm.unwrap(), 300.0, 1e-9));
    // 180° et 270° autour du centre (60, 45), sans translation : le coin (10, 20), de
    // position relative (−50, −25), devient (50, 25) → (110, 70) à 180° et
    // (y, −x) = (−25, 50) → (35, 95) à 270°.
    let at = |rotation_deg| Placement {
        x_mm: 60.0,
        y_mm: 45.0,
        rotation_deg,
    };
    let (c, corner) = (pt(60.0, 45.0), pt(10.0, 20.0));
    assert_eq!(at(180.0).apply(c, corner), pt(110.0, 70.0));
    assert_eq!(at(270.0).apply(c, corner), pt(35.0, 95.0));
    assert_eq!(at(0.0).apply(c, corner), corner);
    // La construction n'a pas bougé : le rectangle est toujours en (10, 20).
    let s = p.sheet(sheet).unwrap();
    let xs: Vec<f64> = s
        .points
        .iter()
        .map(|q| s.position(q.id).unwrap().x)
        .collect();
    assert!(xs.iter().cloned().fold(f64::INFINITY, f64::min) == 10.0);
}

#[test]
fn ca3_aire_et_perimetre_invariants_et_distances_conservees_200_placements() {
    let mut p = Project::new("Propriété");
    let sheet = p.sheets[0].id;
    let id = rect_piece(&mut p, sheet, 0.0, 0.0, 120.0, 70.0);
    let base = item(&p, id);
    let (area, perim) = (base.area_mm2.unwrap(), base.perimeter_mm.unwrap());
    assert!(near(area, 8400.0, 1e-9) && near(perim, 380.0, 1e-9));
    let ref_outline = base.outline.clone();
    let dist = |o: &[PointMm], i: usize, j: usize| pf_geometry::distance_mm(o[i], o[j]);
    let mut state = 0x04_0500_u64;
    let mut next = move || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        (z ^ (z >> 31)) as f64 / u64::MAX as f64
    };
    for k in 0..200 {
        let q = pl(
            next() * 10_000.0 - 5_000.0,
            next() * 10_000.0 - 5_000.0,
            next() * 720.0 - 360.0,
        );
        p.apply(Command::SetPiecePlacement {
            piece: id,
            placement: Some(q),
        })
        .unwrap();
        let it = item(&p, id);
        assert!(near(it.area_mm2.unwrap(), area, 1e-9), "{k}");
        assert!(near(it.perimeter_mm.unwrap(), perim, 1e-9), "{k}");
        // Mesures recalculées sur le contour transformé : mêmes valeurs (relatif 1e-9).
        assert!(near(shoelace(&it.outline), area, 1e-9 * area), "{k} aire");
        assert!(
            near(perimeter(&it.outline), perim, 1e-9 * perim),
            "{k} périmètre"
        );
        let n = ref_outline.len();
        assert_eq!(it.outline.len(), n);
        for i in 0..n {
            for j in (i + 1)..n {
                assert!(
                    near(dist(&it.outline, i, j), dist(&ref_outline, i, j), 1e-9),
                    "{k} : distance {i}-{j}"
                );
            }
        }
        // Le centre de la boîte d'origine est envoyé sur (x, y).
        let c = it.placement.unwrap();
        let origin = it.origin.unwrap();
        let image = c.apply(origin, origin);
        assert!(near(image.x, c.x_mm, 1e-9) && near(image.y, c.y_mm, 1e-9));
    }
}

#[test]
fn ca3_piece_courbe_aire_et_perimetre_exacts_sous_rotation() {
    let mut p = Project::new("Cercle");
    let sheet = p.sheets[0].id;
    let e = p
        .apply(Command::AddCircle {
            sheet,
            center: PointRef::New(pt(50.0, 50.0)),
            radius_mm: 40.0,
        })
        .unwrap()
        .entities;
    let id = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths { entities: e },
            name: "Rond".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let r = 40.0_f64;
    for rot in [0.0, 17.0, 90.0, 213.3] {
        p.apply(Command::SetPiecePlacement {
            piece: id,
            placement: Some(pl(-300.0, 120.0, rot)),
        })
        .unwrap();
        let it = item(&p, id);
        assert!(near(
            it.area_mm2.unwrap(),
            std::f64::consts::PI * r * r,
            1e-6
        ));
        assert!(near(
            it.perimeter_mm.unwrap(),
            2.0 * std::f64::consts::PI * r,
            1e-6
        ));
        let bb = it.bbox.unwrap();
        assert!(
            near(bb.width(), 2.0 * r, 0.2),
            "boîte du cercle : {}",
            bb.width()
        );
    }
}

#[test]
fn ca3_piece_sans_contour_listee_avec_raison_et_non_dessinee() {
    let mut p = Project::new("Étiquette");
    let sheet = p.sheets[0].id;
    let rect = rect_piece(&mut p, sheet, 0.0, 0.0, 50.0, 50.0);
    let label = p
        .apply(Command::AddPiece {
            sheet,
            name: "Étiquette".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    let plan = p.plan_layout();
    assert_eq!(plan.len(), 2);
    let l = plan.iter().find(|i| i.piece == label).unwrap();
    assert_eq!(l.skipped, Some(PlanSkip::EmptyOutline));
    assert!(l.outline.is_empty() && l.bbox.is_none() && l.area_mm2.is_none());
    assert!(l.placement.is_none() && !l.manual);
    assert!(plan
        .iter()
        .find(|i| i.piece == rect)
        .unwrap()
        .skipped
        .is_none());
    assert!(p.plan_overlaps().is_empty());
    // Un placement manuel reste enregistré, la pièce n'est toujours pas dessinée.
    p.apply(Command::SetPiecePlacement {
        piece: label,
        placement: Some(pl(1.0, 1.0, 0.0)),
    })
    .unwrap();
    let l = item(&p, label);
    assert!(l.manual && l.outline.is_empty() && l.bbox.is_none());
    // Clés stables des raisons (le contour illisible n'est pas fabricable par les
    // commandes : la validation refuse les références mortes).
    assert_eq!(PlanSkip::EmptyOutline.key(), "empty_outline");
    assert_eq!(PlanSkip::Unreadable.key(), "outline_unreadable");
}

#[test]
fn ca3_chevauchements_de_boites_listes_en_lecture_seule() {
    let (mut p, ids) = many(4);
    assert!(p.plan_overlaps().is_empty());
    let raw = snap(&p);
    // Deux pièces au même endroit : une paire.
    for id in [ids[0], ids[1]] {
        p.apply(Command::SetPiecePlacement {
            piece: id,
            placement: Some(pl(8000.0, 8000.0, 0.0)),
        })
        .unwrap();
    }
    let pairs = p.plan_overlaps();
    assert_eq!(pairs, vec![(ids[0], ids[1])]);
    // Lecture seule : rien n'a été modifié ni refusé.
    let after = snap(&p);
    let _ = p.plan_overlaps();
    assert_eq!(after, snap(&p));
    assert_ne!(raw, after);
    // Bords qui se touchent seulement : pas de chevauchement. P1 : 40 × 30 ; P2 : 77 × 83.
    let (w0, w1) = (
        item(&p, ids[0]).bbox.unwrap().width(),
        item(&p, ids[1]).bbox.unwrap().width(),
    );
    p.apply(Command::SetPiecePlacement {
        piece: ids[1],
        placement: Some(pl(8000.0 + (w0 + w1) / 2.0, 8000.0, 0.0)),
    })
    .unwrap();
    assert!(
        p.plan_overlaps().is_empty(),
        "bords en contact = pas de recouvrement"
    );
    // Légèrement plus près : recouvrement.
    p.apply(Command::SetPiecePlacement {
        piece: ids[1],
        placement: Some(pl(8000.0 + (w0 + w1) / 2.0 - 1.0, 8000.0, 0.0)),
    })
    .unwrap();
    assert_eq!(p.plan_overlaps(), vec![(ids[0], ids[1])]);
    // La rotation compte : une pièce tournée de 90° échange sa largeur et sa hauteur.
    p.apply(Command::SetPiecePlacement {
        piece: ids[1],
        placement: Some(pl(8000.0, 8000.0, 90.0)),
    })
    .unwrap();
    let b = item(&p, ids[1]).bbox.unwrap();
    assert!(
        near(b.width(), 83.0, 1e-9) && near(b.height(), 77.0, 1e-9),
        "{b:?}"
    );
}

//! US-04.3 — éléments internes d'une pièce : lignes (pli, surpiqûre, position de poche)
//! et repères (points, crans) liés à la construction. Un groupe de tests par critère
//! d'acceptation (CA1 modèle, CA2 commandes, CA3 import, CA4 liens, CA5 lecture,
//! CA7 copie de feuille).

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::piece_internal::{InternalGeometry, SkipReason};
use pf_core::*;
use pf_geometry::{PointMm, Side};
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

fn fixture(name: &str) -> Project {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let text = std::fs::read_to_string(dir.join(name)).unwrap();
    persist::from_json(&text).unwrap_or_else(|e| panic!("{name} : {e}"))
}

fn to_value(p: &Project) -> Value {
    serde_json::from_str(&persist::to_json(p).unwrap()).unwrap()
}

fn segment(p: &mut Project, s: SheetId, a: PointMm, b: PointMm, role: PathRole) -> EntityId {
    let e = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(a),
            to: PointRef::New(b),
        })
        .unwrap()
        .entities[0];
    if role != PathRole::Construction {
        p.apply(Command::SetPathRole {
            sheet: s,
            entity: e,
            role,
        })
        .unwrap();
    }
    e
}

/// Rectangle de pièce 100 × 60 mm (contour liant quatre segments) et, à l'intérieur :
/// un pli (rôle pliure), une surpiqûre, un point libre, un cran sur la surpiqûre ;
/// autour : un tracé qui sort du contour, un droit-fil, une ligne de couture.
struct Scene {
    p: Project,
    s: SheetId,
    piece: PieceId,
    fold: EntityId,
    stitch: EntityId,
    free: PointId,
    notch: NotchId,
    exit: EntityId,
    grain: EntityId,
    sewing: EntityId,
}

fn scene() -> Scene {
    let mut p = Project::new("Internes");
    let s = p.sheets[0].id;
    p.apply(Command::AddRectangle {
        sheet: s,
        corner: PointRef::New(pt(0.0, 0.0)),
        width_mm: 100.0,
        height_mm: 60.0,
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(50.0, 30.0) },
            name: "Devant".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let fold = segment(&mut p, s, pt(20.0, 10.0), pt(20.0, 50.0), PathRole::Fold);
    let stitch = segment(
        &mut p,
        s,
        pt(10.0, 5.0),
        pt(90.0, 5.0),
        PathRole::Construction,
    );
    let free = p
        .apply(Command::AddPoint {
            sheet: s,
            at: pt(50.0, 30.0),
        })
        .unwrap()
        .points[0];
    let notch = p
        .apply(Command::AddNotch {
            sheet: s,
            entity: stitch,
            distance_mm: 40.0,
            side: Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap()
        .notches[0];
    let exit = segment(
        &mut p,
        s,
        pt(80.0, 20.0),
        pt(150.0, 20.0),
        PathRole::Construction,
    );
    let grain = segment(
        &mut p,
        s,
        pt(70.0, 10.0),
        pt(70.0, 50.0),
        PathRole::Grainline,
    );
    let sewing = segment(
        &mut p,
        s,
        pt(30.0, 20.0),
        pt(60.0, 20.0),
        PathRole::SewingContour,
    );
    Scene {
        p,
        s,
        piece,
        fold,
        stitch,
        free,
        notch,
        exit,
        grain,
        sewing,
    }
}

fn path(e: EntityId) -> InternalTarget {
    InternalTarget::Path { entity: e }
}
fn point(p: PointId) -> InternalTarget {
    InternalTarget::Point { point: p }
}
fn notch(n: NotchId) -> InternalTarget {
    InternalTarget::Notch { notch: n }
}

fn add(piece: PieceId, target: InternalTarget, kind: InternalKind) -> Command {
    Command::AddPieceInternal {
        piece,
        target,
        kind,
    }
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

/// Scène avec les cinq éléments importés.
fn imported() -> Scene {
    let mut sc = scene();
    sc.p.apply(Command::ImportPieceInternals { piece: sc.piece })
        .unwrap();
    sc
}

// ----- CA1 : modèle ------------------------------------------------------------

#[test]
fn ca1_champ_absent_octets_inchanges() {
    for name in FIXTURES {
        let first = bytes(&fixture(name));
        let second = bytes(&persist::from_json(&first).unwrap());
        assert_eq!(first, second, "{name}");
        assert!(!first.contains("\"internal\""), "{name}");
        assert!(fixture(name).pieces.iter().all(|p| p.internal.is_empty()));
    }
    let sc = scene();
    let text = bytes(&sc.p);
    assert!(!text.contains("\"internal\""));
    assert_eq!(text, bytes(&persist::from_json(&text).unwrap()));
    // Fichier livré (schéma 3) : relu puis réécrit, identique à l'octet.
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/piece-crans-droit-fil.pforge.json");
    let shipped = std::fs::read_to_string(file).unwrap().replace("\r\n", "\n");
    assert_eq!(
        shipped,
        persist::to_json(&persist::from_json(&shipped).unwrap()).unwrap()
    );
    // Une pièce dont on retire le dernier élément interne retrouve ses octets d'origine.
    let mut sc = scene();
    let before = bytes(&sc.p);
    sc.p.apply(add(sc.piece, path(sc.fold), InternalKind::Fold))
        .unwrap();
    assert_ne!(before, bytes(&sc.p));
    sc.p.apply(Command::RemovePieceInternal {
        piece: sc.piece,
        target: path(sc.fold),
    })
    .unwrap();
    assert_eq!(before, bytes(&sc.p));
}

#[test]
fn ca1_champ_present_relu_a_l_identique_schema_3() {
    let sc = imported();
    let text = bytes(&sc.p);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["schema_version"], 6, "schéma 4 (US-09.1)");
    let list = v["pieces"][0]["internal"].as_array().unwrap();
    assert_eq!(list.len(), 4);
    assert_eq!(
        list[0],
        json!({"target": {"type": "path", "entity": sc.fold.0}, "kind": "fold"})
    );
    assert!(text.contains("\"type\": \"notch\"") || text.contains("\"type\":\"notch\""));
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, sc.p);
    assert_eq!(bytes(&back), text);
    assert!(
        back.pieces[0].extra.is_empty(),
        "champ typé, pas une clé inconnue"
    );
    // Clés stables des types.
    let keys: Vec<&str> = InternalKind::LINES
        .iter()
        .chain(&InternalKind::MARKS)
        .map(|k| k.key())
        .collect();
    assert_eq!(
        keys,
        [
            "fold",
            "topstitch",
            "pocket_position",
            "other",
            "mark",
            "buttonhole",
            "other"
        ]
    );
}

#[test]
fn ca1_references_mortes_autre_feuille_et_donnees_invalides_refusees_a_l_ouverture() {
    let mut sc = imported();
    let other =
        sc.p.apply(Command::CreateSheet {
            name: "Autre".into(),
        })
        .unwrap()
        .sheet
        .unwrap();
    let alien = segment(
        &mut sc.p,
        other,
        pt(0.0, 0.0),
        pt(5.0, 5.0),
        PathRole::Construction,
    );
    let alien_pt =
        sc.p.apply(Command::AddPoint {
            sheet: other,
            at: pt(9.0, 9.0),
        })
        .unwrap()
        .points[0];
    let base = to_value(&sc.p);
    let target = |t: Value, kind: &str| json!({ "target": t, "kind": kind });
    let bad: Vec<(&str, Value)> = vec![
        (
            "tracé inexistant",
            target(json!({"type":"path","entity":99999}), "fold"),
        ),
        (
            "point inexistant",
            target(json!({"type":"point","point":99999}), "mark"),
        ),
        (
            "cran inexistant",
            target(json!({"type":"notch","notch":99999}), "mark"),
        ),
        (
            "tracé d'une autre feuille",
            target(json!({"type":"path","entity":alien.0}), "fold"),
        ),
        (
            "point d'une autre feuille",
            target(json!({"type":"point","point":alien_pt.0}), "mark"),
        ),
        (
            "repère typé en ligne",
            target(json!({"type":"point","point":sc.free.0}), "fold"),
        ),
        (
            "ligne typée en repère",
            target(json!({"type":"path","entity":sc.stitch.0}), "buttonhole"),
        ),
        (
            "type inconnu",
            target(json!({"type":"path","entity":sc.stitch.0}), "dentelle"),
        ),
        (
            "doublon",
            target(json!({"type":"path","entity":sc.fold.0}), "fold"),
        ),
        (
            "clé inconnue",
            json!({"target":{"type":"path","entity":sc.stitch.0},"kind":"other","x":1}),
        ),
        (
            "cible inconnue",
            target(json!({"type":"dimension","id":3}), "other"),
        ),
    ];
    for (what, el) in bad {
        let mut v = base.clone();
        v["pieces"][0]["internal"].as_array_mut().unwrap().push(el);
        assert!(
            persist::from_json(&v.to_string()).is_err(),
            "{what} aurait dû être refusé à l'ouverture"
        );
    }
    // Un tracé du contour (ou le droit-fil) ne peut pas être élément interne.
    let outline_edge = sc.p.pieces[0].outline[0].entity;
    let mut v = base.clone();
    v["pieces"][0]["internal"]
        .as_array_mut()
        .unwrap()
        .push(target(
            json!({"type":"path","entity":outline_edge.0}),
            "fold",
        ));
    let e = persist::from_json(&v.to_string()).unwrap_err().to_string();
    assert!(e.contains("contour"), "{e}");
    // Le message d'une référence morte dit laquelle.
    let mut v = base.clone();
    v["pieces"][0]["internal"][0]["target"]["entity"] = json!(99999);
    let e = persist::from_json(&v.to_string()).unwrap_err().to_string();
    assert!(e.contains("99999") && e.contains("Devant"), "{e}");
    // Le fichier d'origine reste lisible ; un champ homonyme placé à la main dans
    // `extra` est refusé (jamais deux clés identiques écrites).
    assert!(persist::from_json(&base.to_string()).is_ok());
    let mut q = sc.p.clone();
    q.pieces[0].extra.insert("internal".into(), json!([]));
    assert!(q.validate().is_err());
    assert!(persist::to_json(&q).is_err());
}

// ----- CA2 : commandes ---------------------------------------------------------

#[test]
fn ca2_chaque_commande_une_etape_annulable_exacte() {
    let sc = scene();
    let mut doc = Document::new(sc.p.clone());
    let piece = sc.piece;
    let steps: Vec<Command> = vec![
        add(piece, path(sc.fold), InternalKind::Fold),
        add(piece, path(sc.stitch), InternalKind::Topstitch),
        add(piece, point(sc.free), InternalKind::Buttonhole),
        add(piece, notch(sc.notch), InternalKind::Mark),
        Command::SetPieceInternalKind {
            piece,
            target: path(sc.fold),
            kind: InternalKind::PocketPosition,
        },
        Command::RemovePieceInternal {
            piece,
            target: point(sc.free),
        },
        Command::ImportPieceInternals { piece },
    ];
    let mut states = vec![bytes(doc.project())];
    for (i, c) in steps.into_iter().enumerate() {
        doc.execute(c).unwrap_or_else(|e| panic!("étape {i} : {e}"));
        states.push(bytes(doc.project()));
        assert_ne!(states[i], states[i + 1], "étape {i} sans effet");
        doc.project().validate().unwrap();
    }
    let internal = &doc.project().pieces[0].internal;
    assert_eq!(internal.len(), 4, "fold, stitch, notch + point réimporté");
    assert_eq!(internal[0].kind, InternalKind::PocketPosition);
    // Annuler : un état antérieur exact à chaque pas, puis rétablir.
    for i in (0..states.len() - 1).rev() {
        assert!(doc.undo());
        assert_eq!(bytes(doc.project()), states[i], "annulation vers {i}");
    }
    assert!(!doc.can_undo());
    for (i, state) in states.iter().enumerate().skip(1) {
        assert!(doc.redo());
        assert_eq!(&bytes(doc.project()), state, "rétablissement vers {i}");
    }
}

#[test]
fn ca2_tous_les_refus_sont_sans_trace() {
    let mut sc = imported();
    let other =
        sc.p.apply(Command::CreateSheet {
            name: "Autre".into(),
        })
        .unwrap()
        .sheet
        .unwrap();
    let alien = segment(
        &mut sc.p,
        other,
        pt(0.0, 0.0),
        pt(5.0, 5.0),
        PathRole::Construction,
    );
    let alien_pt =
        sc.p.apply(Command::AddPoint {
            sheet: other,
            at: pt(9.0, 9.0),
        })
        .unwrap()
        .points[0];
    // Une seconde pièce sans contour, et une pièce détachée sans contour.
    let empty =
        sc.p.apply(Command::AddPiece {
            sheet: sc.s,
            name: "Étiquette".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    let outline_edge = sc.p.pieces[0].outline[0].entity;
    let piece = sc.piece;
    let free2 =
        sc.p.apply(Command::AddPoint {
            sheet: sc.s,
            at: pt(60.0, 40.0),
        })
        .unwrap()
        .points[0];
    let cases: Vec<(&str, Command, &str)> = vec![
        (
            "pièce inconnue",
            add(PieceId(77777), path(sc.exit), InternalKind::Other),
            "pièce",
        ),
        (
            "tracé inexistant",
            add(piece, path(EntityId(88888)), InternalKind::Other),
            "introuvable",
        ),
        (
            "point inexistant",
            add(piece, point(PointId(88888)), InternalKind::Mark),
            "introuvable",
        ),
        (
            "cran inexistant",
            add(piece, notch(NotchId(88888)), InternalKind::Mark),
            "introuvable",
        ),
        (
            "tracé d'une autre feuille",
            add(piece, path(alien), InternalKind::Other),
            "pas sur la feuille",
        ),
        (
            "point d'une autre feuille",
            add(piece, point(alien_pt), InternalKind::Mark),
            "pas sur la feuille",
        ),
        (
            "tracé du contour",
            add(piece, path(outline_edge), InternalKind::Fold),
            "contour",
        ),
        (
            "doublon",
            add(piece, path(sc.fold), InternalKind::Fold),
            "déjà",
        ),
        (
            "repère typé en ligne",
            add(piece, point(free2), InternalKind::Fold),
            "convient pas",
        ),
        (
            "ligne typée en repère",
            add(piece, path(sc.exit), InternalKind::Mark),
            "convient pas",
        ),
        (
            "retrait d'un non-élément",
            Command::RemovePieceInternal {
                piece,
                target: path(sc.exit),
            },
            "pas un élément interne",
        ),
        (
            "type d'un non-élément",
            Command::SetPieceInternalKind {
                piece,
                target: path(sc.exit),
                kind: InternalKind::Fold,
            },
            "pas un élément interne",
        ),
        (
            "type incohérent",
            Command::SetPieceInternalKind {
                piece,
                target: point(sc.free),
                kind: InternalKind::Topstitch,
            },
            "convient pas",
        ),
        (
            "import sans contour",
            Command::ImportPieceInternals { piece: empty },
            "pas de contour",
        ),
        (
            "import d'une pièce inconnue",
            Command::ImportPieceInternals {
                piece: PieceId(77777),
            },
            "pièce",
        ),
    ];
    for (what, cmd, expect) in cases {
        let before = sc.p.clone();
        let before_bytes = bytes(&sc.p);
        let err = sc.p.apply(cmd).expect_err(what).to_string();
        assert!(err.contains(expect), "{what} : « {err} »");
        assert_eq!(sc.p, before, "{what} : projet modifié");
        assert_eq!(bytes(&sc.p), before_bytes, "{what} : octets modifiés");
    }
    // Le droit-fil ne peut pas être élément interne ; un tracé interne ne peut pas
    // devenir le contour ni le droit-fil (SetPieceOutline refusé, sans trace).
    sc.p.apply(Command::SetPieceOutline {
        piece,
        outline: sc.p.pieces[0].outline.clone(),
        grainline: Some(sc.grain),
    })
    .unwrap();
    let before = bytes(&sc.p);
    assert!(sc
        .p
        .apply(add(piece, path(sc.grain), InternalKind::Other))
        .is_err());
    assert!(sc
        .p
        .apply(Command::SetPieceOutline {
            piece,
            outline: sc.p.pieces[0].outline.clone(),
            grainline: Some(sc.fold),
        })
        .is_err());
    assert_eq!(before, bytes(&sc.p));
}

#[test]
fn ca2_sans_changement_aucune_etape() {
    let sc = imported();
    let mut doc = Document::new(sc.p.clone());
    let piece = sc.piece;
    // Même type, import rejoué : projet identique, pas d'étape d'annulation.
    doc.execute(Command::SetPieceInternalKind {
        piece,
        target: path(sc.fold),
        kind: InternalKind::Fold,
    })
    .unwrap();
    doc.execute(Command::ImportPieceInternals { piece })
        .unwrap();
    assert!(!doc.can_undo(), "aucune étape pour un no-op");
    assert_eq!(bytes(doc.project()), bytes(&sc.p));
}

// ----- CA3 : import depuis la construction ---------------------------------------

#[test]
fn ca3_import_rapport_et_types_par_defaut() {
    let sc = scene();
    let report = sc.p.piece_internal_import(sc.piece).unwrap();
    // Ajoutés : pli (rôle pliure → Fold), surpiqûre (→ Other), point libre, cran.
    let added: Vec<(InternalTarget, InternalKind)> =
        report.added.iter().map(|e| (e.target, e.kind)).collect();
    assert_eq!(
        added,
        vec![
            (path(sc.fold), InternalKind::Fold),
            (path(sc.stitch), InternalKind::Other),
            (point(sc.free), InternalKind::Mark),
            (notch(sc.notch), InternalKind::Mark),
        ]
    );
    // Ignorés, avec la raison : tracé qui sort, droit-fil, ligne de couture, 4 côtés.
    let reason = |t: InternalTarget| report.skipped.iter().find(|s| s.0 == t).map(|s| s.1);
    assert_eq!(reason(path(sc.exit)), Some(SkipReason::OutsideOutline));
    assert_eq!(reason(path(sc.grain)), Some(SkipReason::Grainline));
    assert_eq!(reason(path(sc.sewing)), Some(SkipReason::ContourRole));
    for e in &sc.p.pieces[0].outline {
        assert_eq!(reason(path(e.entity)), Some(SkipReason::OutlineEdge));
    }
    // Les extrémités de tracés ne sont pas des repères : aucun point listé sauf le libre.
    assert!(report
        .skipped
        .iter()
        .all(|s| !matches!(s.0, InternalTarget::Point { .. })));
    assert_eq!(report.skipped.len(), 3 + 4);
    assert_eq!(report.already, 0);
    // Lecture pure : le rapport ne modifie rien.
    assert!(sc.p.pieces[0].internal.is_empty());
}

#[test]
fn ca3_import_une_etape_idempotent_et_annulable() {
    let sc = scene();
    let mut doc = Document::new(sc.p.clone());
    doc.execute(Command::ImportPieceInternals { piece: sc.piece })
        .unwrap();
    assert_eq!(doc.project().pieces[0].internal.len(), 4);
    let after = bytes(doc.project());
    doc.project().validate().unwrap();
    // Relancé : aucun doublon, rien d'ajouté, aucune étape de plus.
    let report = doc.project().piece_internal_import(sc.piece).unwrap();
    assert!(report.added.is_empty());
    assert_eq!(report.already, 4);
    doc.execute(Command::ImportPieceInternals { piece: sc.piece })
        .unwrap();
    assert_eq!(bytes(doc.project()), after);
    // Une seule étape pour tout l'import.
    assert!(doc.undo());
    assert!(!doc.can_undo());
    assert_eq!(bytes(doc.project()), bytes(&sc.p));
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), after);
    // Un élément retiré à la main est reproposé (puis jamais en double).
    let mut p = doc.project().clone();
    p.apply(Command::RemovePieceInternal {
        piece: sc.piece,
        target: point(sc.free),
    })
    .unwrap();
    p.apply(Command::ImportPieceInternals { piece: sc.piece })
        .unwrap();
    assert_eq!(p.pieces[0].internal.len(), 4);
}

#[test]
fn ca3_import_pieces_sans_contour_exploitable() {
    let mut sc = scene();
    let empty =
        sc.p.apply(Command::AddPiece {
            sheet: sc.s,
            name: "Étiquette".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    // Sans contour : refus expliqué, projet intact.
    let before = bytes(&sc.p);
    let e = sc.p.piece_internal_import(empty).unwrap_err().to_string();
    assert!(e.contains("pas de contour"), "{e}");
    assert!(sc
        .p
        .apply(Command::ImportPieceInternals { piece: empty })
        .is_err());
    assert_eq!(before, bytes(&sc.p));
    // Contour ouvert (un côté retiré) : refus expliqué.
    let mut open = sc.p.pieces[0].outline.clone();
    open.pop();
    sc.p.apply(Command::SetPieceOutline {
        piece: empty,
        outline: open,
        grainline: None,
    })
    .unwrap();
    let e = sc.p.piece_internal_import(empty).unwrap_err().to_string();
    assert!(e.contains("pas fermé"), "{e}");
    // Pièce détachée : sa feuille dédiée ne contient que le contour ; rien à importer.
    let det =
        sc.p.apply(Command::ExtractPiece {
            sheet: sc.s,
            source: PieceSource::Click { at: pt(50.0, 30.0) },
            name: "Copie".into(),
            detached: true,
        })
        .unwrap()
        .pieces[0];
    let report = sc.p.piece_internal_import(det).unwrap();
    assert!(report.added.is_empty());
    sc.p.apply(Command::ImportPieceInternals { piece: det })
        .unwrap();
    sc.p.validate().unwrap();
}

#[test]
fn ca3_contour_d_une_autre_piece_jamais_importe() {
    let mut sc = scene();
    // Second rectangle, entièrement dans la première pièce, extrait en pièce liée.
    sc.p.apply(Command::AddRectangle {
        sheet: sc.s,
        corner: PointRef::New(pt(40.0, 30.0)),
        width_mm: 20.0,
        height_mm: 15.0,
    })
    .unwrap();
    let second =
        sc.p.apply(Command::ExtractPiece {
            sheet: sc.s,
            source: PieceSource::Click { at: pt(50.0, 38.0) },
            name: "Poche".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let report = sc.p.piece_internal_import(sc.piece).unwrap();
    let inner: Vec<EntityId> =
        sc.p.piece(second)
            .unwrap()
            .outline
            .iter()
            .map(|e| e.entity)
            .collect();
    assert!(!inner.is_empty());
    for e in inner {
        assert!(
            report
                .skipped
                .contains(&(path(e), SkipReason::OtherPieceOutline)),
            "contour de l'autre pièce"
        );
    }
}

// ----- CA4 : liés, pas copiés ----------------------------------------------------

#[test]
fn ca4_deplacer_la_construction_met_a_jour_la_geometrie_resolue() {
    let mut sc = imported();
    let find = |p: &Project, t: InternalTarget| {
        p.piece_internals(sc.piece)
            .into_iter()
            .find(|r| r.element.target == t)
            .unwrap()
    };
    assert!(matches!(find(&sc.p, point(sc.free)).geometry,
        InternalGeometry::Position { at } if at == pt(50.0, 30.0)));
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: sc.free,
        to: pt(55.0, 35.0),
    })
    .unwrap();
    assert!(matches!(find(&sc.p, point(sc.free)).geometry,
        InternalGeometry::Position { at } if at == pt(55.0, 35.0)));
    // Pli : déplacer une extrémité change la longueur résolue (40 → 30).
    let end = match sc.p.sheet(sc.s).unwrap().entity(sc.fold).unwrap().kind {
        EntityKind::Segment { to, .. } => to,
        _ => unreachable!(),
    };
    let InternalGeometry::Path { length_mm } = find(&sc.p, path(sc.fold)).geometry else {
        panic!("tracé");
    };
    assert!(near(length_mm, 40.0));
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: end,
        to: pt(20.0, 40.0),
    })
    .unwrap();
    let InternalGeometry::Path { length_mm } = find(&sc.p, path(sc.fold)).geometry else {
        panic!("tracé");
    };
    assert!(near(length_mm, 30.0));
    // Le cran suit son tracé : position (50, 5) → déplacer la surpiqûre le décale.
    let before = find(&sc.p, notch(sc.notch)).geometry;
    assert!(
        matches!(before, InternalGeometry::Position { at } if near(at.x, 50.0) && near(at.y, 5.0))
    );
}

#[test]
fn ca4_supprimer_une_cible_retire_l_element_dans_la_meme_etape() {
    // Règle (DECISIONS 108) : comme le droit-fil et le contour d'une pièce, la
    // suppression n'est pas refusée : l'élément interne est retiré, avec remarque.
    let sc = imported();
    let piece = sc.piece;
    let check = |p: &Project| {
        p.validate().unwrap();
        for r in p.piece_internals(piece) {
            assert_ne!(r.geometry, InternalGeometry::Unreadable, "{r:?}");
        }
    };
    // Point libre.
    let mut doc = Document::new(sc.p.clone());
    let out = doc
        .execute(Command::Delete {
            sheet: sc.s,
            points: vec![sc.free],
            entities: vec![],
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("élément interne")),
        "{:?}",
        out.notes
    );
    assert_eq!(doc.project().pieces[0].internal.len(), 3);
    assert!(!doc.project().pieces[0]
        .internal
        .iter()
        .any(|e| e.target == point(sc.free)));
    check(doc.project());
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), bytes(&sc.p), "une seule étape");
    // Cran.
    let mut p = sc.p.clone();
    p.apply(Command::DeleteNotch {
        sheet: sc.s,
        notch: sc.notch,
    })
    .unwrap();
    assert_eq!(p.pieces[0].internal.len(), 3);
    check(&p);
    // Pli.
    let mut p = sc.p.clone();
    p.apply(Command::Delete {
        sheet: sc.s,
        points: vec![],
        entities: vec![sc.fold],
    })
    .unwrap();
    assert_eq!(p.pieces[0].internal.len(), 3);
    check(&p);
    // Surpiqûre : son cran disparaît avec elle, donc deux éléments internes de moins.
    let mut p = sc.p.clone();
    p.apply(Command::Delete {
        sheet: sc.s,
        points: vec![],
        entities: vec![sc.stitch],
    })
    .unwrap();
    assert_eq!(p.pieces[0].internal.len(), 2);
    assert!(p.sheet(sc.s).unwrap().notches.is_empty());
    check(&p);
    // Une feuille supprimée emporte ses pièces : aucune référence ne survit.
    let mut p = sc.p.clone();
    let other = p
        .apply(Command::CreateSheet { name: "B".into() })
        .unwrap()
        .sheet
        .unwrap();
    p.apply(Command::DeleteSheet { sheet: sc.s }).unwrap();
    assert!(p.pieces.is_empty());
    p.validate().unwrap();
    let _ = other;
}

#[test]
fn ca4_couper_et_fusionner_une_ligne_interne() {
    let mut sc = imported();
    let piece = sc.piece;
    let out =
        sc.p.apply(Command::SplitPath {
            sheet: sc.s,
            entity: sc.fold,
            at: pt(20.0, 30.0),
        })
        .unwrap();
    let second = out.entities[0];
    let internal = &sc.p.pieces[0].internal;
    assert!(internal
        .iter()
        .any(|e| e.target == path(second) && e.kind == InternalKind::Fold));
    sc.p.validate().unwrap();
    // Fusion : l'élément du tronçon absorbé disparaît, l'autre reste ; pas de doublon.
    sc.p.apply(Command::MergePaths {
        sheet: sc.s,
        first: sc.fold,
        second,
    })
    .unwrap();
    let folds = sc.p.pieces[0]
        .internal
        .iter()
        .filter(|e| e.kind == InternalKind::Fold)
        .count();
    assert_eq!(folds, 1);
    sc.p.validate().unwrap();
    let _ = piece;
}

// ----- CA5 : lecture, fiche et points d'attention --------------------------------

#[test]
fn ca5_piece_internals_geometrie_resolue_en_mm() {
    let sc = imported();
    let list = sc.p.piece_internals(sc.piece);
    assert_eq!(list.len(), 4);
    let by = |t: InternalTarget| list.iter().find(|r| r.element.target == t).unwrap();
    assert!(matches!(by(path(sc.fold)).geometry,
        InternalGeometry::Path { length_mm } if near(length_mm, 40.0)));
    assert!(matches!(by(path(sc.stitch)).geometry,
        InternalGeometry::Path { length_mm } if near(length_mm, 80.0)));
    assert!(matches!(by(point(sc.free)).geometry,
        InternalGeometry::Position { at } if at == pt(50.0, 30.0)));
    assert!(matches!(by(notch(sc.notch)).geometry,
        InternalGeometry::Position { at } if near(at.x, 50.0) && near(at.y, 5.0)));
    assert_eq!(
        by(path(sc.fold)).name,
        sc.p.sheet(sc.s).unwrap().entity_name(sc.fold)
    );
    assert!(list.iter().all(|r| r.inside_outline == Some(true)));
    // Pièce inconnue ou sans élément : liste vide.
    assert!(sc.p.piece_internals(PieceId(424242)).is_empty());
    assert!(scene().p.piece_internals(scene().piece).is_empty());
}

#[test]
fn ca5_piece_card_compte_les_elements_par_type() {
    let mut sc = scene();
    let piece = sc.piece;
    assert_eq!(sc.p.piece_card(piece).unwrap().internal.total(), 0);
    for c in [
        add(piece, path(sc.fold), InternalKind::Fold),
        add(piece, path(sc.stitch), InternalKind::Topstitch),
        add(piece, path(sc.exit), InternalKind::PocketPosition),
        add(piece, path(sc.sewing), InternalKind::Other),
        add(piece, point(sc.free), InternalKind::Buttonhole),
        add(piece, notch(sc.notch), InternalKind::Other),
    ] {
        sc.p.apply(c).unwrap();
    }
    let n = sc.p.piece_card(piece).unwrap().internal;
    assert_eq!(
        (n.fold, n.topstitch, n.pocket_position, n.other_lines),
        (1, 1, 1, 1)
    );
    assert_eq!((n.mark, n.buttonhole, n.other_marks), (0, 1, 1));
    assert_eq!((n.lines(), n.marks(), n.total()), (4, 2, 6));
    // Les autres champs de la fiche sont inchangés.
    let card = sc.p.piece_card(piece).unwrap();
    assert_eq!(card.name, "Devant");
    assert!(near(card.area_mm2.unwrap(), 6000.0));
}

#[test]
fn ca5_element_sorti_du_contour_signale_jamais_refuse() {
    let mut sc = imported();
    let piece = sc.piece;
    assert!(!sc
        .p
        .piece_issues(piece)
        .iter()
        .any(|i| matches!(i, PieceIssue::InternalOutsideOutline { .. })));
    // On sort le point libre du contour en déplaçant la construction : accepté.
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: sc.free,
        to: pt(300.0, 300.0),
    })
    .unwrap();
    let issues = sc.p.piece_issues(piece);
    assert!(
        issues.contains(&PieceIssue::InternalOutsideOutline { count: 1 }),
        "{issues:?}"
    );
    assert_eq!(
        PieceIssue::InternalOutsideOutline { count: 1 }.key(),
        "internal_outside_outline"
    );
    let card = sc.p.piece_card(piece).unwrap();
    assert!(card
        .issues
        .contains(&PieceIssue::InternalOutsideOutline { count: 1 }));
    let r =
        sc.p.piece_internals(piece)
            .into_iter()
            .find(|r| r.element.target == point(sc.free))
            .unwrap();
    assert_eq!(r.inside_outline, Some(false));
    // Pièce toujours valide et sauvegardable ; relecture à l'identique.
    sc.p.validate().unwrap();
    let text = bytes(&sc.p);
    assert_eq!(persist::from_json(&text).unwrap(), sc.p);
    // Un tracé qui dépasse partiellement est lui aussi signalé (2 éléments).
    let end = match sc.p.sheet(sc.s).unwrap().entity(sc.fold).unwrap().kind {
        EntityKind::Segment { to, .. } => to,
        _ => unreachable!(),
    };
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: end,
        to: pt(20.0, 200.0),
    })
    .unwrap();
    assert!(sc
        .p
        .piece_issues(piece)
        .contains(&PieceIssue::InternalOutsideOutline { count: 2 }));
    // Le contour absent n'ajoute aucun faux avertissement d'élément sorti.
    sc.p.apply(Command::SetPieceOutline {
        piece,
        outline: vec![],
        grainline: None,
    })
    .unwrap();
    assert!(!sc
        .p
        .piece_issues(piece)
        .iter()
        .any(|i| matches!(i, PieceIssue::InternalOutsideOutline { .. })));
}

// ----- CA7 : copie de feuille ----------------------------------------------------

#[test]
fn ca7_dupliquer_la_feuille_remappe_les_elements_internes() {
    let mut sc = imported();
    // Un type modifié doit survivre à la copie.
    sc.p.apply(Command::SetPieceInternalKind {
        piece: sc.piece,
        target: path(sc.stitch),
        kind: InternalKind::Topstitch,
    })
    .unwrap();
    let out = sc.p.apply(Command::DuplicateSheet { sheet: sc.s }).unwrap();
    let new_sheet = out.sheet.unwrap();
    assert_eq!(sc.p.pieces.len(), 2);
    let copy = &sc.p.pieces[1];
    assert_eq!(copy.link, PieceLink::Linked { sheet: new_sheet });
    assert_eq!(copy.internal.len(), 4);
    let orig = &sc.p.pieces[0];
    assert_eq!(orig.internal.len(), 4, "l'original ne change pas");
    let ns = sc.p.sheet(new_sheet).unwrap();
    for (o, c) in orig.internal.iter().zip(&copy.internal) {
        assert_eq!(o.kind, c.kind);
        assert_ne!(o.target, c.target, "identifiants régénérés");
        match c.target {
            InternalTarget::Path { entity } => assert!(ns.entity(entity).is_ok()),
            InternalTarget::Point { point } => assert!(ns.point(point).is_ok()),
            InternalTarget::Notch { notch } => assert!(ns.notch(notch).is_ok()),
        }
    }
    sc.p.validate().unwrap();
    // Même géométrie résolue.
    let a = sc.p.piece_internals(orig.id);
    let b = sc.p.piece_internals(copy.id);
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(&b) {
        assert_eq!(x.geometry, y.geometry);
    }
    // Les éléments de la copie sont indépendants : retirer l'un ne touche pas l'autre.
    let copy_id = copy.id;
    let target = copy.internal[0].target;
    sc.p.apply(Command::RemovePieceInternal {
        piece: copy_id,
        target,
    })
    .unwrap();
    assert_eq!(sc.p.pieces[0].internal.len(), 4);
    assert_eq!(sc.p.pieces[1].internal.len(), 3);
    // Export puis import de feuille entre projets : les éléments suivent, remappés.
    let cmd = sc.p.import_command(sc.s).unwrap();
    let mut other = Project::new("Autre");
    other.apply(cmd).unwrap();
    assert_eq!(other.pieces[0].internal.len(), 4);
    other.validate().unwrap();
}

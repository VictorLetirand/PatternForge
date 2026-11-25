//! US-05.6 — pièce « coupe nette » (cuir bord franc) : contour de coupe = contour de couture,
//! **déclaré** (`Piece.net_cut`, `Command::SetNetCut`). Un groupe de tests par critère :
//! CA1 coupe = couture (rectangle 100 × 60 : périmètre 320 mm, aire 6000 mm², valeurs calculées à
//! la main), CA2 exclusion avec une marge, CA3 refus sans trace, CA4 une étape annulable et
//! no-op, CA5 contrôles, CA6 la déclaration voyage (copie, séparer, assembler, déplier),
//! CA7 fichier (schéma 3, octets inchangés, chargeur strict, `pf-validate`).

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::{distance_mm, PointMm, Trace};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn segment(p: &mut Project, sheet: SheetId, a: PointMm, b: PointMm) -> EntityId {
    p.apply(Command::AddSegment {
        sheet,
        from: PointRef::New(a),
        to: PointRef::New(b),
    })
    .unwrap()
    .entities[0]
}

fn poly_piece(p: &mut Project, sheet: SheetId, pts: &[PointMm], name: &str) -> PieceId {
    let n = pts.len();
    let ids: Vec<EntityId> = (0..n)
        .map(|i| segment(p, sheet, pts[i], pts[(i + 1) % n]))
        .collect();
    p.apply(Command::ExtractPiece {
        sheet,
        source: PieceSource::Paths { entities: ids },
        name: name.into(),
        detached: false,
    })
    .unwrap()
    .pieces[0]
}

/// Rectangle 100 × 60 en (0,0), pièce « Devant ».
fn rect() -> (Project, SheetId, PieceId) {
    let mut p = Project::new("Coupe nette");
    let s = p.sheets[0].id;
    let piece = poly_piece(
        &mut p,
        s,
        &[pt(0.0, 0.0), pt(100.0, 0.0), pt(100.0, 60.0), pt(0.0, 60.0)],
        "Devant",
    );
    (p, s, piece)
}

fn net(p: &mut Project, piece: PieceId, on: bool) -> Result<Outcome, CoreError> {
    p.apply(Command::SetNetCut { piece, net_cut: on })
}

fn seam10() -> Option<SeamAllowance> {
    Some(SeamAllowance {
        default_mm: 10.0,
        ..Default::default()
    })
}

/// Refus : message d'erreur ; le projet est identique (valeur et octets).
fn refused(p: &mut Project, c: Command) -> String {
    let (before, text) = (p.clone(), bytes(p));
    let err = p.apply(c).expect_err("la commande aurait dû être refusée");
    assert_eq!(*p, before, "refus sans trace (valeur)");
    assert_eq!(text, bytes(p), "refus sans trace (octets)");
    err.to_string()
}

// ---------------------------------------------------------------- CA1

#[test]
fn ca1_coupe_nette_contour_de_coupe_egal_contour_de_couture() {
    let (mut p, _, piece) = rect();
    net(&mut p, piece, true).unwrap();
    assert!(p.piece(piece).unwrap().net_cut);
    let cut = p.piece_cut_contour(piece).unwrap();
    // Valeurs calculées à la main : 2 × (100 + 60) = 320 mm ; 100 × 60 = 6000 mm².
    assert!(
        (cut.perimeter_mm - 320.0).abs() < 1e-9,
        "{}",
        cut.perimeter_mm
    );
    assert!((cut.area_mm2 - 6000.0).abs() < 1e-9, "{}", cut.area_mm2);
    // Coupe = couture : mêmes mesures observées du contour de la pièce.
    let sewing = p.piece_contour(piece).unwrap();
    assert!((cut.perimeter_mm - sewing.perimeter_mm).abs() < 1e-9);
    assert!((cut.area_mm2 - sewing.area_mm2).abs() < 1e-9);
    // Quatre segments : les sommets du rectangle, dans le sens anti-horaire.
    let starts: Vec<PointMm> = cut
        .traces
        .iter()
        .map(|t| match t {
            Trace::Line(a, _) => *a,
            other => panic!("segment attendu, reçu {other:?}"),
        })
        .collect();
    let want = [(0.0, 0.0), (100.0, 0.0), (100.0, 60.0), (0.0, 60.0)];
    assert_eq!(starts.len(), 4);
    let n = 4;
    let cycle = (0..n).any(|shift| {
        (0..n).all(|i| {
            let (g, w) = (starts[(i + shift) % n], want[i]);
            distance_mm(g, pt(w.0, w.1)) < 1e-9
        })
    });
    assert!(cycle, "sommets du contour de coupe : {starts:?}");
}

#[test]
fn ca1_sans_marge_ni_declaration_le_contour_de_coupe_reste_celui_de_couture() {
    // Une pièce sans marge a déjà une coupe égale à la couture : la déclaration n'y change rien
    // de mesurable, elle rend l'intention explicite.
    let (mut p, _, piece) = rect();
    let before = p.piece_cut_contour(piece).unwrap();
    net(&mut p, piece, true).unwrap();
    assert_eq!(p.piece_cut_contour(piece).unwrap(), before);
    assert!(!before.traces.is_empty());
    assert!(p.piece(piece).unwrap().seam.is_none());
}

#[test]
fn ca1_la_coupe_suit_la_construction() {
    // Déplacer un sommet : la coupe nette suit (jamais stockée).
    let (mut p, s, piece) = rect();
    net(&mut p, piece, true).unwrap();
    let top_right = p
        .sheet(s)
        .unwrap()
        .points
        .iter()
        .find(|q| distance_mm(p.sheet(s).unwrap().position(q.id).unwrap(), pt(100.0, 60.0)) < 1e-9)
        .unwrap()
        .id;
    p.apply(Command::MovePoint {
        sheet: s,
        point: top_right,
        to: pt(100.0, 80.0),
    })
    .unwrap();
    // Trapèze (0,0) (100,0) (100,80) (0,60) : aire (60 + 80) / 2 × 100 = 7000 mm².
    let cut = p.piece_cut_contour(piece).unwrap();
    assert!((cut.area_mm2 - 7000.0).abs() < 1e-6, "{}", cut.area_mm2);
    // Périmètre : 100 + 80 + √(100² + 20²) + 60.
    let want = 100.0 + 80.0 + (100.0f64 * 100.0 + 20.0 * 20.0).sqrt() + 60.0;
    assert!(
        (cut.perimeter_mm - want).abs() < 1e-6,
        "{}",
        cut.perimeter_mm
    );
}

// ---------------------------------------------------------------- CA2

#[test]
fn ca2_exclusive_avec_une_marge_refus_expliques() {
    let (mut p, _, piece) = rect();
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: seam10(),
    })
    .unwrap();
    // Coupe nette sur une pièce à marge : refus (la marge n'est jamais effacée en silence).
    let msg = refused(
        &mut p,
        Command::SetNetCut {
            piece,
            net_cut: true,
        },
    );
    assert!(msg.contains("marge") && msg.contains("retirez"), "{msg}");
    // La marge reste intacte : coupe 120 × 80.
    assert!((p.piece_cut_contour(piece).unwrap().area_mm2 - 120.0 * 80.0).abs() < 1e-6);
    // Retirer la marge, puis déclarer : accepté.
    p.apply(Command::SetSeamAllowance { piece, seam: None })
        .unwrap();
    net(&mut p, piece, true).unwrap();
    // Marge sur une pièce coupe nette : refus.
    let msg = refused(
        &mut p,
        Command::SetSeamAllowance {
            piece,
            seam: seam10(),
        },
    );
    assert!(msg.contains("coupe nette"), "{msg}");
    // Retirer une marge (None) reste permis, sans effet ; retirer la déclaration rouvre la marge.
    p.apply(Command::SetSeamAllowance { piece, seam: None })
        .unwrap();
    net(&mut p, piece, false).unwrap();
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: seam10(),
    })
    .unwrap();
    assert!(p.piece(piece).unwrap().seam.is_some() && !p.piece(piece).unwrap().net_cut);
}

#[test]
fn ca2_le_chargeur_refuse_coupe_nette_et_marge() {
    let (mut p, _, piece) = rect();
    net(&mut p, piece, true).unwrap();
    let mut v: serde_json::Value = serde_json::from_str(&bytes(&p)).unwrap();
    v["pieces"][0]["seam"] = serde_json::json!({ "default_mm": 5.0 });
    let err = persist::from_json(&v.to_string()).expect_err("refus attendu");
    assert!(err.to_string().contains("s'excluent"), "{err}");
}

// ---------------------------------------------------------------- CA3

#[test]
fn ca3_refus_sans_trace() {
    let (mut p, s, piece) = rect();
    // Pièce inconnue.
    refused(
        &mut p,
        Command::SetNetCut {
            piece: PieceId(9999),
            net_cut: true,
        },
    );
    // Pièce dérivée liée.
    let d = p
        .apply(Command::CreateDerivedPiece {
            mother: piece,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset { distance_mm: -5.0 },
            name: None,
        })
        .unwrap()
        .pieces[0];
    let msg = refused(
        &mut p,
        Command::SetNetCut {
            piece: d,
            net_cut: true,
        },
    );
    assert!(msg.contains("dérivée"), "{msg}");
    // Contour ouvert : un tracé seul n'est pas fermé.
    let open = segment(&mut p, s, pt(300.0, 0.0), pt(400.0, 0.0));
    let q = p
        .apply(Command::AddPiece {
            sheet: s,
            name: "Ouverte".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    p.apply(Command::SetPieceOutline {
        piece: q,
        outline: vec![PieceEdge {
            entity: open,
            reversed: false,
        }],
        grainline: None,
    })
    .unwrap();
    let msg = refused(
        &mut p,
        Command::SetNetCut {
            piece: q,
            net_cut: true,
        },
    );
    assert!(msg.contains("fermé"), "{msg}");
    // Sans contour du tout.
    let r = p
        .apply(Command::AddPiece {
            sheet: s,
            name: "Etiquette".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    let msg = refused(
        &mut p,
        Command::SetNetCut {
            piece: r,
            net_cut: true,
        },
    );
    assert!(msg.contains("contour"), "{msg}");
}

// ---------------------------------------------------------------- CA4

#[test]
fn ca4_une_etape_annulable_et_sans_effet_sans_etape() {
    let (p, _, piece) = rect();
    let mut doc = Document::new(p);
    let before = bytes(doc.project());
    doc.execute(Command::SetNetCut {
        piece,
        net_cut: true,
    })
    .unwrap();
    let after = bytes(doc.project());
    assert_ne!(before, after);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
    assert!(!doc.can_undo(), "une seule étape");
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), after);
    // Déclarer de nouveau : aucune étape.
    doc.execute(Command::SetNetCut {
        piece,
        net_cut: true,
    })
    .unwrap();
    assert!(doc.undo());
    assert!(!doc.can_undo());
    // Retirer ce qui n'est pas déclaré : aucune étape non plus.
    doc.execute(Command::SetNetCut {
        piece,
        net_cut: false,
    })
    .unwrap();
    assert!(!doc.can_undo());
}

// ---------------------------------------------------------------- CA5

#[test]
fn ca5_controles() {
    let (mut p, _, piece) = rect();
    net(&mut p, piece, true).unwrap();
    assert!(
        !p.piece_issues(piece).iter().any(|i| matches!(
            i,
            PieceIssue::NetCutWithSeam { .. } | PieceIssue::CutContourFailed { .. }
        )),
        "{:?}",
        p.piece_issues(piece)
    );
    // Projet construit à la main : coupe nette + ourlet + rempli = contrôle, jamais une erreur.
    let edge = p.piece(piece).unwrap().outline[0].entity;
    let other = p.piece(piece).unwrap().outline[1].entity;
    p.pieces[0].seam = Some(SeamAllowance {
        hems: vec![HemAllowance {
            entity: edge,
            height_mm: 20.0,
        }],
        folds: vec![FoldAllowance {
            entity: other,
            value_mm: 6.0,
            notch_step_mm: None,
            notch_count: None,
        }],
        ..Default::default()
    });
    let issues = p.piece_issues(piece);
    let issue = issues
        .iter()
        .find(|i| matches!(i, PieceIssue::NetCutWithSeam { .. }))
        .expect("contrôle NetCutWithSeam");
    assert_eq!(issue, &PieceIssue::NetCutWithSeam { hems: 1, folds: 1 });
    assert_eq!(issue.key(), "net_cut_with_seam");
    // La coupe nette ignore la marge : la coupe reste la couture.
    assert!((p.piece_cut_contour(piece).unwrap().area_mm2 - 6000.0).abs() < 1e-9);
    // Et le fichier d'un tel projet est refusé à la lecture.
    assert!(p.validate().is_err());
}

#[test]
fn ca5_contour_devenu_ouvert_est_un_controle() {
    let (mut p, s, piece) = rect();
    net(&mut p, piece, true).unwrap();
    // Supprimer un tracé du contour : le contour n'est plus fermé, la commande passe, la pièce
    // signale que sa coupe est impossible (jamais un refus de la commande).
    let edge = p.piece(piece).unwrap().outline[0].entity;
    p.apply(Command::Delete {
        sheet: s,
        points: vec![],
        entities: vec![edge],
    })
    .unwrap();
    assert!(p
        .piece_issues(piece)
        .iter()
        .any(|i| matches!(i, PieceIssue::CutContourFailed { .. })));
}

// ---------------------------------------------------------------- CA6

#[test]
fn ca6_copie_de_feuille_reporte_la_declaration() {
    let (mut p, s, piece) = rect();
    net(&mut p, piece, true).unwrap();
    let out = p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    let copy = out.pieces[0];
    assert!(p.piece(copy).unwrap().net_cut);
    assert_eq!(
        p.piece_cut_contour(copy).unwrap().traces.len(),
        p.piece_cut_contour(piece).unwrap().traces.len()
    );
    assert!(p.validate().is_ok());
}

#[test]
fn ca6_separer_reporte_la_declaration_sur_les_deux_moities() {
    let (mut p, _, piece) = rect();
    net(&mut p, piece, true).unwrap();
    let out = p
        .apply(Command::SplitPiece {
            piece,
            a: pt(50.0, -5.0),
            b: pt(50.0, 65.0),
        })
        .unwrap();
    assert_eq!(out.pieces.len(), 2);
    for id in &out.pieces {
        assert!(p.piece(*id).unwrap().net_cut);
        // Moitié 50 × 60 : périmètre 220, aire 3000 (calculé à la main).
        let c = p.piece_cut_contour(*id).unwrap();
        assert!((c.area_mm2 - 3000.0).abs() < 1e-6);
        assert!((c.perimeter_mm - 220.0).abs() < 1e-6);
    }
    assert!(p.validate().is_ok());
}

fn two_rects() -> (Project, PieceId, PieceId, EntityId, EntityId) {
    let mut p = Project::new("Assembler");
    let s = p.sheets[0].id;
    let a = poly_piece(
        &mut p,
        s,
        &[pt(0.0, 0.0), pt(100.0, 0.0), pt(100.0, 60.0), pt(0.0, 60.0)],
        "A",
    );
    let b = poly_piece(
        &mut p,
        s,
        &[
            pt(200.0, 0.0),
            pt(300.0, 0.0),
            pt(300.0, 60.0),
            pt(200.0, 60.0),
        ],
        "B",
    );
    let find = |p: &Project, piece: PieceId, x: f64| {
        let sh = p.piece_sheet(piece).unwrap();
        p.piece(piece)
            .unwrap()
            .outline
            .iter()
            .find(|e| {
                let (u, v) = sh.segment_ends(e.entity).unwrap();
                (u.x - x).abs() < 1e-9 && (v.x - x).abs() < 1e-9
            })
            .unwrap()
            .entity
    };
    let (ea, eb) = (find(&p, a, 100.0), find(&p, b, 200.0));
    (p, a, b, ea, eb)
}

fn join(p: &mut Project, a: PieceId, b: PieceId, ea: EntityId, eb: EntityId) -> Outcome {
    p.apply(Command::JoinPieces {
        first: a,
        first_edge: ea,
        second: b,
        second_edge: eb,
        tolerance_mm: 0.5,
    })
    .unwrap()
}

#[test]
fn ca6_assembler_deux_coupes_nettes_donne_une_coupe_nette() {
    let (mut p, a, b, ea, eb) = two_rects();
    net(&mut p, a, true).unwrap();
    net(&mut p, b, true).unwrap();
    let out = join(&mut p, a, b, ea, eb);
    let joined = p.piece(out.pieces[0]).unwrap();
    assert!(joined.net_cut && joined.seam.is_none());
    // 200 × 60 : aire 12 000 mm², périmètre 520 mm (calculés à la main).
    let c = p.piece_cut_contour(joined.id).unwrap();
    assert!((c.area_mm2 - 12_000.0).abs() < 1e-6);
    assert!((c.perimeter_mm - 520.0).abs() < 1e-6);
    assert!(p.validate().is_ok());
}

#[test]
fn ca6_assembler_melange_la_premiere_piece_l_emporte_avec_remarque() {
    // Première pièce coupe nette, seconde à marge : coupe nette, marge non reprise (remarque).
    let (mut p, a, b, ea, eb) = two_rects();
    net(&mut p, a, true).unwrap();
    p.apply(Command::SetSeamAllowance {
        piece: b,
        seam: seam10(),
    })
    .unwrap();
    let out = join(&mut p, a, b, ea, eb);
    let joined = p.piece(out.pieces[0]).unwrap();
    assert!(joined.net_cut && joined.seam.is_none());
    assert!(
        out.notes
            .iter()
            .any(|n| n.contains("coupe nette") && n.contains("pas reprise")),
        "{:?}",
        out.notes
    );
    assert!(p.validate().is_ok());
    // Première pièce à marge, seconde coupe nette : la marge l'emporte, remarque.
    let (mut p, a, b, ea, eb) = two_rects();
    p.apply(Command::SetSeamAllowance {
        piece: a,
        seam: seam10(),
    })
    .unwrap();
    net(&mut p, b, true).unwrap();
    let out = join(&mut p, a, b, ea, eb);
    let joined = p.piece(out.pieces[0]).unwrap();
    assert!(!joined.net_cut && joined.seam.is_some());
    assert!(
        out.notes
            .iter()
            .any(|n| n.contains("coupe nette") && n.contains("pas reprise")),
        "{:?}",
        out.notes
    );
    assert!(p.validate().is_ok());
    // Seconde seule coupe nette : non reprise, remarque.
    let (mut p, a, b, ea, eb) = two_rects();
    net(&mut p, b, true).unwrap();
    let out = join(&mut p, a, b, ea, eb);
    assert!(!p.piece(out.pieces[0]).unwrap().net_cut);
    assert!(
        out.notes
            .iter()
            .any(|n| n.contains("coupe nette") && n.contains("pas reprise")),
        "{:?}",
        out.notes
    );
}

#[test]
fn ca6_deplier_ne_reporte_pas_et_le_dit() {
    let (mut p, _, piece) = rect();
    net(&mut p, piece, true).unwrap();
    let fold = p.piece(piece).unwrap().outline[3].entity; // bord x = 0
    let out = p
        .apply(Command::UnfoldPiece {
            piece,
            fold,
            name: None,
        })
        .unwrap();
    assert!(
        out.notes
            .iter()
            .any(|n| n.contains("coupe nette") && n.contains("pas reprise")),
        "{:?}",
        out.notes
    );
    let unfolded = p.piece(out.pieces[0]).unwrap();
    assert!(!unfolded.net_cut);
}

// ---------------------------------------------------------------- CA7

#[test]
fn ca7_fichier_schema_3_octets_inchanges_et_relecture_exacte() {
    let (mut p, _, piece) = rect();
    let without = bytes(&p);
    assert!(!without.contains("net_cut"), "aucune clé sans déclaration");
    net(&mut p, piece, true).unwrap();
    let text = bytes(&p);
    assert!(text.contains("\"net_cut\": true"));
    assert!(text.contains("\"schema_version\": 6"));
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, p);
    assert_eq!(bytes(&back), text);
    // Retirer la déclaration ramène les octets d'origine.
    net(&mut p, piece, false).unwrap();
    assert_eq!(bytes(&p), without);
    // pf-validate : 0 = valide.
    let dir = std::env::temp_dir().join(format!("pf-coupe-nette-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("nette.pforge.json");
    let run = |content: &str| {
        std::fs::write(&file, content).unwrap();
        std::process::Command::new(env!("CARGO_BIN_EXE_pf-validate"))
            .arg(&file)
            .output()
            .unwrap()
            .status
            .code()
    };
    assert_eq!(run(&text), Some(0));
    assert_eq!(
        run(&text.replace("\"net_cut\": true", "\"net_cut\": false")),
        Some(1)
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn ca7_anciens_fichiers_lus_et_reecrits_a_l_identique() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");
    let mut seen = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(".pforge.json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let p = persist::from_json(&text).unwrap();
        // Le porte-cartes (PH-3b) déclare volontairement la coupe nette ; les autres sont anciens.
        if !path.to_string_lossy().contains("porte-cartes") {
            assert!(p.pieces.iter().all(|q| !q.net_cut), "{path:?}");
            assert!(!text.contains("net_cut"));
        }
        assert_eq!(bytes(&p), text, "{path:?}");
        seen += 1;
    }
    assert!(seen >= 3, "fichiers d'exemple lus : {seen}");
}

#[test]
fn ca7_chargeur_strict() {
    let (mut p, _, piece) = rect();
    net(&mut p, piece, true).unwrap();
    let base: serde_json::Value = serde_json::from_str(&bytes(&p)).unwrap();
    let load = |v: &serde_json::Value| persist::from_json(&v.to_string());
    assert!(load(&base).is_ok());
    let mutated = |value: serde_json::Value| {
        let mut v = base.clone();
        v["pieces"][0]["net_cut"] = value;
        v
    };
    for (what, bad) in [
        ("false explicite (non canonique)", mutated(false.into())),
        ("texte", mutated("oui".into())),
        ("nombre", mutated(1.into())),
        ("null", mutated(serde_json::Value::Null)),
        ("tableau", mutated(serde_json::json!([true]))),
    ] {
        assert!(load(&bad).is_err(), "{what} aurait dû être refusé");
    }
    // Une pièce détachée / sans contour fermé ne change pas la règle de lecture : le champ est
    // typé, jamais conservé dans `extra`.
    assert!(p.piece(piece).unwrap().extra.is_empty());
}

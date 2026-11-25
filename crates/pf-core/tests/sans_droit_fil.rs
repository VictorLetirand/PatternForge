//! PH-5d — pièce « sans droit-fil » (cuir) : déclaration (CA1), cohérence avec le droit-fil (CA2),
//! avertissements (CA3), interactions : copie de feuille, séparer / assembler, dépliage, dérivée
//! (CA4). Exports : `pf-export/tests/export_sans_droit_fil.rs` ; interface : `ui_tests::ph5d_*` ;
//! campagne : `adverse_sans_droit_fil.rs`.
//!
//! **Valeurs et chaînes écrites à la main**, jamais relues du code testé.
//!
//! Fixture : deux rectangles. « A » 100 × 50 en (0 ; 0), « B » 100 × 20 en (200 ; 0), une feuille.
//! Un droit-fil = un segment de rôle droit-fil de la feuille, posé par `SetPieceOutline`.

use pf_core::derived::{DerivedKind, DerivedOp};
use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::{distance_mm, PointMm};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

struct Sc {
    p: Project,
    s: SheetId,
    a: PieceId,
    b: PieceId,
}

fn rect_piece(p: &mut Project, s: SheetId, x: f64, y: f64, w: f64, h: f64, name: &str) -> PieceId {
    let r = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(pt(x, y)),
            width_mm: w,
            height_mm: h,
        })
        .unwrap();
    p.apply(Command::ExtractPiece {
        sheet: s,
        source: PieceSource::Paths {
            entities: r.entities,
        },
        name: name.into(),
        detached: false,
    })
    .unwrap()
    .pieces[0]
}

fn two_pieces() -> Sc {
    let mut p = Project::new("Sans droit-fil");
    let s = p.sheets[0].id;
    let a = rect_piece(&mut p, s, 0.0, 0.0, 100.0, 50.0, "A");
    let b = rect_piece(&mut p, s, 200.0, 0.0, 100.0, 20.0, "B");
    Sc { p, s, a, b }
}

/// Pose un segment de rôle droit-fil sur la pièce (contour inchangé) ; renvoie le tracé.
fn give_grain(p: &mut Project, s: SheetId, piece: PieceId, at: f64) -> Result<EntityId, CoreError> {
    let g = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(at, 5.0)),
            to: PointRef::New(pt(at, 15.0)),
        })?
        .entities[0];
    p.apply(Command::SetPathRole {
        sheet: s,
        entity: g,
        role: PathRole::Grainline,
    })?;
    let outline = p.piece(piece)?.outline.clone();
    p.apply(Command::SetPieceOutline {
        piece,
        outline,
        grainline: Some(g),
    })?;
    Ok(g)
}

fn declare(p: &mut Project, piece: PieceId, no_grain: bool) -> Result<Outcome, CoreError> {
    p.apply(Command::SetPieceNoGrain { piece, no_grain })
}

/// Refus : message ; le projet est identique (valeur et octets).
fn refused(p: &mut Project, c: Command) -> String {
    let (before, text) = (p.clone(), bytes(p));
    let e = p.apply(c).expect_err("refus attendu");
    assert_eq!(*p, before, "refus sans trace (valeur)");
    assert_eq!(bytes(p), text, "refus sans trace (octets)");
    e.to_string()
}

fn edge_between(p: &Project, piece: PieceId, a: PointMm, b: PointMm) -> EntityId {
    let sheet = p.piece_sheet(piece).unwrap();
    p.piece(piece)
        .unwrap()
        .outline
        .iter()
        .find(|e| match sheet.shape(e.entity) {
            Ok(Shape::Line(s, t)) => {
                (distance_mm(s, a) < 1e-6 && distance_mm(t, b) < 1e-6)
                    || (distance_mm(s, b) < 1e-6 && distance_mm(t, a) < 1e-6)
            }
            _ => false,
        })
        .expect("bord du contour")
        .entity
}

const MISSING: &str = "MissingGrainline";

fn missing(p: &Project, piece: PieceId) -> bool {
    p.piece_issues(piece)
        .contains(&PieceIssue::MissingGrainline)
}

// ---------------------------------------------------------------------------------------------
// CA1 — déclaration
// ---------------------------------------------------------------------------------------------

#[test]
fn ca1_declarer_est_une_etape_annulable_exacte_a_l_octet() {
    let sc = two_pieces();
    let mut doc = Document::new(sc.p);
    let before = bytes(doc.project());
    assert_eq!(before.matches("no_grain").count(), 0, "omis quand faux");
    doc.execute(Command::SetPieceNoGrain {
        piece: sc.a,
        no_grain: true,
    })
    .unwrap();
    let after = bytes(doc.project());
    assert!(doc.project().piece(sc.a).unwrap().no_grain);
    assert!(!doc.project().piece(sc.b).unwrap().no_grain);
    assert_eq!(after.matches("\"no_grain\": true").count(), 1, "{after}");
    assert_eq!(after.matches("no_grain").count(), 1);
    // Même déclaration : aucune étape ; retirer ce qui n'est pas déclaré (B) : aucune étape.
    doc.execute(Command::SetPieceNoGrain {
        piece: sc.a,
        no_grain: true,
    })
    .unwrap();
    doc.execute(Command::SetPieceNoGrain {
        piece: sc.b,
        no_grain: false,
    })
    .unwrap();
    assert_eq!(bytes(doc.project()), after);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before, "annuler = octets d'avant");
    assert!(!doc.can_undo(), "un seul pas d'historique");
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), after, "rétablir = octets d'après");
    // Retirer la déclaration : une étape, retour aux octets d'avant.
    doc.execute(Command::SetPieceNoGrain {
        piece: sc.a,
        no_grain: false,
    })
    .unwrap();
    assert_eq!(bytes(doc.project()), before);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), after);
}

#[test]
fn ca1_refus_sans_trace_piece_inconnue_et_derivee_liee() {
    let mut sc = two_pieces();
    let m = refused(
        &mut sc.p,
        Command::SetPieceNoGrain {
            piece: PieceId(999_999),
            no_grain: true,
        },
    );
    assert!(!m.is_empty());
    // Pièce dérivée liée à sa mère : refus, message qui nomme le lien.
    let d =
        sc.p.apply(Command::CreateDerivedPiece {
            mother: sc.a,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset { distance_mm: -2.0 },
            name: None,
        })
        .unwrap()
        .pieces[0];
    let m = refused(
        &mut sc.p,
        Command::SetPieceNoGrain {
            piece: d,
            no_grain: true,
        },
    );
    assert!(m.contains("dérivée") && m.contains("lien"), "{m}");
    // Lien cassé : la pièce redevient ordinaire et peut être déclarée.
    sc.p.apply(Command::BreakDerivedLink { piece: d }).unwrap();
    declare(&mut sc.p, d, true).unwrap();
    assert!(sc.p.piece(d).unwrap().no_grain);
}

#[test]
fn ca1_json_avec_et_sans_champ_et_valeurs_refusees() {
    let mut sc = two_pieces();
    // Sans le champ : ouvre, faux.
    let plain = bytes(&sc.p);
    assert!(!persist::from_json(&plain).unwrap().pieces[0].no_grain);
    declare(&mut sc.p, sc.a, true).unwrap();
    let text = bytes(&sc.p);
    // Avec le champ : relu à l'identique, réécrit à l'octet près.
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, sc.p);
    assert!(back.pieces[0].no_grain && !back.pieces[1].no_grain);
    assert_eq!(bytes(&back), text);
    // `false` explicite refusé (forme unique : champ omis), valeur non booléenne refusée.
    let with = |v: serde_json::Value| {
        let mut j: serde_json::Value = serde_json::from_str(&text).unwrap();
        j["pieces"][0]["no_grain"] = v;
        serde_json::to_string_pretty(&j).unwrap()
    };
    for bad in [
        serde_json::json!(false),
        serde_json::json!("true"),
        serde_json::json!(1),
        serde_json::json!(null),
        serde_json::json!([true]),
    ] {
        assert!(persist::from_json(&with(bad.clone())).is_err(), "{bad}");
    }
    assert!(persist::from_json(&with(serde_json::json!(true))).is_ok());
}

// ---------------------------------------------------------------------------------------------
// CA2 — cohérence avec le droit-fil
// ---------------------------------------------------------------------------------------------

#[test]
fn ca2_declarer_sur_une_piece_qui_a_un_droit_fil_est_refuse() {
    let mut sc = two_pieces();
    let g = give_grain(&mut sc.p, sc.s, sc.a, 50.0).unwrap();
    let m = refused(
        &mut sc.p,
        Command::SetPieceNoGrain {
            piece: sc.a,
            no_grain: true,
        },
    );
    assert!(
        m.contains("a un droit-fil") && m.contains("retirez-le"),
        "{m}"
    );
    // Retirer le droit-fil (SetPieceOutline sans droit-fil) puis déclarer : accepté.
    let outline = sc.p.piece(sc.a).unwrap().outline.clone();
    sc.p.apply(Command::SetPieceOutline {
        piece: sc.a,
        outline,
        grainline: None,
    })
    .unwrap();
    declare(&mut sc.p, sc.a, true).unwrap();
    assert!(sc.p.piece(sc.a).unwrap().no_grain);
    // Le tracé du droit-fil existe toujours (la construction n'est pas touchée).
    assert!(sc.p.sheets[0].entity(g).is_ok());
}

#[test]
fn ca2_poser_un_droit_fil_sur_une_piece_sans_droit_fil_est_refuse() {
    let mut sc = two_pieces();
    declare(&mut sc.p, sc.a, true).unwrap();
    let g =
        sc.p.apply(Command::AddSegment {
            sheet: sc.s,
            from: PointRef::New(pt(50.0, 5.0)),
            to: PointRef::New(pt(50.0, 15.0)),
        })
        .unwrap()
        .entities[0];
    sc.p.apply(Command::SetPathRole {
        sheet: sc.s,
        entity: g,
        role: PathRole::Grainline,
    })
    .unwrap();
    let outline = sc.p.piece(sc.a).unwrap().outline.clone();
    let m = refused(
        &mut sc.p,
        Command::SetPieceOutline {
            piece: sc.a,
            outline: outline.clone(),
            grainline: Some(g),
        },
    );
    assert!(
        m.contains("déclarée sans droit-fil") && m.contains("retirez la déclaration"),
        "{m}"
    );
    // Après retrait de la déclaration, le même ordre est accepté.
    declare(&mut sc.p, sc.a, false).unwrap();
    sc.p.apply(Command::SetPieceOutline {
        piece: sc.a,
        outline,
        grainline: Some(g),
    })
    .unwrap();
    assert_eq!(sc.p.piece(sc.a).unwrap().grainline, Some(g));
    assert!(!sc.p.piece(sc.a).unwrap().no_grain);
}

#[test]
fn ca2_supprimer_le_trace_du_droit_fil_puis_declarer() {
    let mut sc = two_pieces();
    let g = give_grain(&mut sc.p, sc.s, sc.b, 250.0).unwrap();
    sc.p.apply(Command::Delete {
        sheet: sc.s,
        points: Vec::new(),
        entities: vec![g],
    })
    .unwrap();
    assert!(sc.p.piece(sc.b).unwrap().grainline.is_none());
    assert!(missing(&sc.p, sc.b));
    declare(&mut sc.p, sc.b, true).unwrap();
    assert!(!missing(&sc.p, sc.b));
}

#[test]
fn ca2_fichier_qui_cumule_droit_fil_et_declaration_est_refuse_a_l_ouverture() {
    let mut sc = two_pieces();
    give_grain(&mut sc.p, sc.s, sc.a, 50.0).unwrap();
    let text = bytes(&sc.p);
    let mut j: serde_json::Value = serde_json::from_str(&text).unwrap();
    j["pieces"][0]["no_grain"] = serde_json::json!(true);
    let e = persist::from_json(&serde_json::to_string_pretty(&j).unwrap())
        .expect_err("droit-fil et déclaration cumulés");
    assert!(e.to_string().contains("sans droit-fil"), "{e}");
}

// ---------------------------------------------------------------------------------------------
// CA3 — avertissements
// ---------------------------------------------------------------------------------------------

#[test]
fn ca3_plus_d_avertissement_de_droit_fil_pour_la_piece_declaree() {
    let mut sc = two_pieces();
    // Sans déclaration : l'avertissement existe, seul (octets d'avant la tranche).
    assert_eq!(sc.p.piece_issues(sc.a), vec![PieceIssue::MissingGrainline]);
    assert_eq!(format!("{:?}", PieceIssue::MissingGrainline), MISSING);
    assert!(!sc.p.piece_card(sc.a).unwrap().no_grain);
    declare(&mut sc.p, sc.a, true).unwrap();
    assert_eq!(sc.p.piece_issues(sc.a), vec![]);
    // Fiche : état lu ; l'autre pièce garde son avertissement.
    let card = sc.p.piece_card(sc.a).unwrap();
    assert!(card.no_grain && !card.has_grainline && card.issues.is_empty());
    assert_eq!(sc.p.piece_issues(sc.b), vec![PieceIssue::MissingGrainline]);
    // Rapport de contrôle : les anomalies de pièce ne comptent que B.
    let report = sc.p.control_report();
    let a = report.pieces.iter().find(|p| p.id == sc.a).unwrap();
    let b = report.pieces.iter().find(|p| p.id == sc.b).unwrap();
    assert!(a.issues.is_empty());
    assert_eq!(b.issues, vec![PieceIssue::MissingGrainline]);
    assert_eq!(
        report
            .anomalies
            .iter()
            .filter(|x| x.kind == control_report::AnomalyKind::PieceIssue)
            .count(),
        1
    );
    // Retirer la déclaration ramène l'avertissement.
    declare(&mut sc.p, sc.a, false).unwrap();
    assert_eq!(sc.p.piece_issues(sc.a), vec![PieceIssue::MissingGrainline]);
}

#[test]
fn ca3_la_categorie_cuir_seule_n_implique_rien() {
    let mut sc = two_pieces();
    sc.p.apply(Command::SetPieceCard {
        piece: sc.a,
        name: "A".into(),
        code: None,
        quantity: 1,
        mirrored_pair: false,
        material: Some("Cuir".into()),
        category: Some(PieceCategory::Leather),
        comment: None,
    })
    .unwrap();
    assert_eq!(sc.p.piece_issues(sc.a), vec![PieceIssue::MissingGrainline]);
}

// ---------------------------------------------------------------------------------------------
// CA4 — interactions
// ---------------------------------------------------------------------------------------------

#[test]
fn ca4_copie_de_feuille_reprend_la_declaration() {
    let mut sc = two_pieces();
    declare(&mut sc.p, sc.a, true).unwrap();
    let out = sc.p.apply(Command::DuplicateSheet { sheet: sc.s }).unwrap();
    let new_sheet = out.sheet.expect("feuille copiée");
    let copies: Vec<&Piece> =
        sc.p.pieces
            .iter()
            .filter(|p| matches!(p.link, PieceLink::Linked { sheet } if sheet == new_sheet))
            .collect();
    assert_eq!(copies.len(), 2);
    let a2 = copies.iter().find(|p| p.name == "A").unwrap();
    let b2 = copies.iter().find(|p| p.name == "B").unwrap();
    assert!(a2.no_grain && !b2.no_grain);
    assert_ne!(a2.id, sc.a);
    assert!(!missing(&sc.p, a2.id) && missing(&sc.p, b2.id));
}

#[test]
fn ca4_separer_reprend_la_declaration_sur_les_deux_moities() {
    let mut sc = two_pieces();
    declare(&mut sc.p, sc.a, true).unwrap();
    let mut doc = Document::new(sc.p);
    let before = bytes(doc.project());
    let out = doc
        .execute(Command::SplitPiece {
            piece: sc.a,
            a: pt(50.0, -10.0),
            b: pt(50.0, 60.0),
        })
        .unwrap();
    assert_eq!(out.pieces.len(), 2);
    for id in &out.pieces {
        let h = doc.project().piece(*id).unwrap();
        assert!(h.no_grain, "{}", h.name);
        assert!(!missing(doc.project(), *id));
    }
    assert!(!out.notes.iter().any(|n| n.contains("sans droit-fil")));
    let after = bytes(doc.project());
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), after);
}

fn join_ab(sc: &mut Sc) -> Result<Outcome, CoreError> {
    let ea = edge_between(&sc.p, sc.a, pt(100.0, 50.0), pt(0.0, 50.0));
    let eb = edge_between(&sc.p, sc.b, pt(200.0, 0.0), pt(300.0, 0.0));
    sc.p.apply(Command::JoinPieces {
        first: sc.a,
        first_edge: ea,
        second: sc.b,
        second_edge: eb,
        tolerance_mm: 0.5,
    })
}

#[test]
fn ca4_assembler_reprend_si_les_deux_sources_l_ont() {
    let mut sc = two_pieces();
    declare(&mut sc.p, sc.a, true).unwrap();
    declare(&mut sc.p, sc.b, true).unwrap();
    let out = join_ab(&mut sc).unwrap();
    let merged = out.pieces[0];
    assert!(sc.p.piece(merged).unwrap().no_grain);
    assert!(!missing(&sc.p, merged));
    assert!(!out.notes.iter().any(|n| n.contains("sans droit-fil")));
}

#[test]
fn ca4_assembler_ne_reprend_pas_si_une_seule_source_l_a_et_le_dit() {
    // A déclarée, B non.
    let mut sc = two_pieces();
    declare(&mut sc.p, sc.a, true).unwrap();
    let out = join_ab(&mut sc).unwrap();
    let merged = out.pieces[0];
    assert!(!sc.p.piece(merged).unwrap().no_grain);
    assert!(missing(&sc.p, merged));
    let want = "La pièce « A » est déclarée sans droit-fil mais pas « B » : la déclaration n'est \
                pas reprise sur la pièce assemblée.";
    assert!(out.notes.iter().any(|n| n == want), "{:?}", out.notes);
    // B déclarée, A non : la remarque nomme B.
    let mut sc = two_pieces();
    declare(&mut sc.p, sc.b, true).unwrap();
    let out = join_ab(&mut sc).unwrap();
    assert!(!sc.p.piece(out.pieces[0]).unwrap().no_grain);
    let want = "La pièce « B » est déclarée sans droit-fil mais pas « A » : la déclaration n'est \
                pas reprise sur la pièce assemblée.";
    assert!(out.notes.iter().any(|n| n == want), "{:?}", out.notes);
    // Aucune déclaration : aucune remarque de ce genre.
    let mut sc = two_pieces();
    let out = join_ab(&mut sc).unwrap();
    assert!(!out.notes.iter().any(|n| n.contains("sans droit-fil")));
}

#[test]
fn ca4_assembler_une_source_declaree_et_une_avec_droit_fil_garde_le_droit_fil() {
    let mut sc = two_pieces();
    declare(&mut sc.p, sc.a, true).unwrap();
    give_grain(&mut sc.p, sc.s, sc.b, 250.0).unwrap();
    let out = join_ab(&mut sc).unwrap();
    let merged = sc.p.piece(out.pieces[0]).unwrap();
    assert!(!merged.no_grain && merged.grainline.is_some());
    assert!(out
        .notes
        .iter()
        .any(|n| n.contains("déclarée sans droit-fil")));
}

#[test]
fn ca4_depliee_et_decalee_reprennent_la_declaration_de_leur_source() {
    let mut sc = two_pieces();
    declare(&mut sc.p, sc.a, true).unwrap();
    // Décalée (doublure de A) : repris, et la dérivée liée ne se modifie pas elle-même.
    let d =
        sc.p.apply(Command::CreateDerivedPiece {
            mother: sc.a,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset { distance_mm: -2.0 },
            name: None,
        })
        .unwrap()
        .pieces[0];
    assert!(sc.p.piece(d).unwrap().no_grain);
    assert!(!missing(&sc.p, d));
    let m = refused(
        &mut sc.p,
        Command::SetPieceNoGrain {
            piece: d,
            no_grain: false,
        },
    );
    assert!(m.contains("dérivée"), "{m}");
    // Dépliée selon le bord gauche de A : 200 × 50, déclaration reprise.
    let left = edge_between(&sc.p, sc.a, pt(0.0, 50.0), pt(0.0, 0.0));
    let u =
        sc.p.apply(Command::UnfoldPiece {
            piece: sc.a,
            fold: left,
            name: None,
        })
        .unwrap()
        .pieces[0];
    assert!(sc.p.piece(u).unwrap().no_grain);
    assert!(!missing(&sc.p, u));
    // La source B, non déclarée : sa dépliée ne l'est pas.
    let left_b = edge_between(&sc.p, sc.b, pt(200.0, 20.0), pt(200.0, 0.0));
    let ub =
        sc.p.apply(Command::UnfoldPiece {
            piece: sc.b,
            fold: left_b,
            name: None,
        })
        .unwrap()
        .pieces[0];
    assert!(!sc.p.piece(ub).unwrap().no_grain);
    assert!(missing(&sc.p, ub));
}

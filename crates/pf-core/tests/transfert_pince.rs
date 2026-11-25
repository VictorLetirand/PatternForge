//! US-07.2 — transférer une pince vers un autre bord, autour de son sommet. Un groupe de tests par
//! critère : CA1 somme des angles conservée, CA2 longueurs hors pince conservées, valeurs
//! calculées à la main (total, partiel), CA3 refus sans trace, CA4 une étape annulable, CA5 fichier
//! (schéma 3, relecture, chargeur strict, `pf-validate`), CA6 dérivées et reports.
//!
//! Pièce de référence : rectangle 200 × 150 mm. Pince : sommet A (100 ; 100), point d'axe (100 ; 0),
//! longueur 100, ouverture 20, jambe gauche fixe : pieds (110 ; 0) (fixe) et (90 ; 0) (mobile),
//! jambes √10100. Angle d'ouverture θ = 2·atan(0,1) ; rotation de fermeture : cos θ = 99 / 101,
//! sin θ = 20 / 101 (anti-horaire). Cible : (100 ; 150), à 50 mm au-dessus de A.
//!
//! Total : τ = (100 ; 150), τ' = A + R(θ)·(0 ; 50) = (100 − 1000/101 ; 100 + 4950/101).
//! Le contour devient 7 sommets : (110 ; 0), (200 ; 0), (200 ; 150), τ, τ', R(0 ; 150) =
//! (100 − 10900/101 ; 100 + 2950/101), R(0 ; 0) = (100 − 7900/101 ; 100 − 11900/101).
//! Nouvelle ouverture = |ττ'| = 2·50·sin(θ/2) = 10/√1,01.
//! Partiel 50 % : rotation φ = atan(0,1) (cos = 1/√1,01, sin = 0,1/√1,01) ; τ' = (100 − 5/√1,01 ;
//! 100 + 50/√1,01), R(0 ; 150) = (100 − 105/√1,01 ; 100 + 40/√1,01), R(0 ; 0) = (100 − 90/√1,01 ;
//! 100 − 110/√1,01), pied tourné de l'ancienne pince (100 ; 100 − 101/√1,01).

use pf_core::persist;
use pf_core::*;
use pf_geometry::{distance_mm, PointMm, Side};
use serde_json::{json, Value};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

struct Scene {
    p: Project,
    s: SheetId,
    piece: PieceId,
    dart: DartId,
}

fn free_point(p: &mut Project, s: SheetId, x: f64, y: f64) -> PointId {
    p.apply(Command::AddPoint {
        sheet: s,
        at: pt(x, y),
    })
    .unwrap()
    .points[0]
}

fn scene() -> Scene {
    let mut p = Project::new("Transfert");
    let s = p.sheets[0].id;
    p.apply(Command::AddRectangle {
        sheet: s,
        corner: PointRef::New(pt(0.0, 0.0)),
        width_mm: 200.0,
        height_mm: 150.0,
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click {
                at: pt(100.0, 75.0),
            },
            name: "Devant".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let apex = free_point(&mut p, s, 100.0, 100.0);
    let axis = free_point(&mut p, s, 100.0, 0.0);
    let dart = p
        .apply(Command::AddPieceDart {
            piece,
            apex: PointRef::Existing(apex),
            toward: PointRef::Existing(axis),
            length_mm: 100.0,
            opening_mm: 20.0,
            fixed: Side::Left,
        })
        .unwrap()
        .darts[0];
    Scene { p, s, piece, dart }
}

/// (tracé, distance depuis son début) du point `at` d'un bord rectiligne du contour de la pièce.
fn target(p: &Project, piece: PieceId, at: PointMm) -> (EntityId, f64) {
    let sheet = p.piece_sheet(piece).unwrap();
    for e in &p.piece(piece).unwrap().outline {
        if let Shape::Line(a, b) = sheet.shape(e.entity).unwrap() {
            let (ab, aa) = (distance_mm(a, b), distance_mm(a, at));
            if (aa + distance_mm(at, b) - ab).abs() < 1e-9 {
                return (e.entity, aa);
            }
        }
    }
    panic!("aucun bord ne porte {at:?}");
}

fn transfer(sc: &Scene, at: PointMm, part: f64) -> Command {
    let (edge, distance_mm) = target(&sc.p, sc.piece, at);
    Command::TransferPieceDart {
        piece: sc.piece,
        dart: sc.dart,
        edge,
        distance_mm,
        part,
    }
}

fn refused(p: &mut Project, c: Command) -> String {
    let (before, text) = (p.clone(), bytes(p));
    let err = p.apply(c).expect_err("la commande aurait dû être refusée");
    assert_eq!(*p, before, "refus sans trace (valeur)");
    assert_eq!(text, bytes(p), "refus sans trace (octets)");
    assert!(!err.to_string().is_empty());
    err.to_string()
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

/// Arêtes (segments) du contour de la pièce : (départ, arrivée, longueur), dans l'ordre du contour.
fn edges(p: &Project, piece: PieceId) -> Vec<(PointMm, PointMm, f64)> {
    let sheet = p.piece_sheet(piece).unwrap();
    p.piece(piece)
        .unwrap()
        .outline
        .iter()
        .map(|e| match sheet.shape(e.entity).unwrap() {
            Shape::Line(a, b) => {
                let (a, b) = if e.reversed { (b, a) } else { (a, b) };
                (a, b, distance_mm(a, b))
            }
            other => panic!("segment attendu, reçu {other:?}"),
        })
        .collect()
}

fn has_vertex(es: &[(PointMm, PointMm, f64)], x: f64, y: f64) -> bool {
    es.iter().any(|e| distance_mm(e.0, pt(x, y)) < 1e-9)
}

fn darts_calc(p: &Project, piece: PieceId) -> Vec<(PieceDart, DartCalc)> {
    p.piece_darts(piece)
        .into_iter()
        .map(|r| (r.dart, r.calc.expect("pince calculable")))
        .collect()
}

/// Angle d'ouverture d'origine, calculé à la main : 2·atan(10 / 100) en degrés.
fn theta_deg() -> f64 {
    2.0 * (0.1f64).atan().to_degrees()
}

fn is_base(e: &(PointMm, PointMm, f64), calcs: &[(PieceDart, DartCalc)]) -> bool {
    calcs.iter().any(|(_, c)| {
        let (l, r) = (c.shape.left_foot, c.shape.right_foot);
        (distance_mm(e.0, l) < 1e-6 && distance_mm(e.1, r) < 1e-6)
            || (distance_mm(e.0, r) < 1e-6 && distance_mm(e.1, l) < 1e-6)
    })
}

fn r101(x: f64) -> f64 {
    x / 101.0
}

// ----- valeurs calculées à la main : transfert total -------------------------------------

#[test]
fn total_vers_le_bord_haut_sommets_et_ouverture_calcules_a_la_main() {
    let mut sc = scene();
    let c = transfer(&sc, pt(100.0, 150.0), 1.0);
    let out = sc.p.apply(c).unwrap();
    assert_eq!(out.pieces, vec![sc.piece]);
    assert_eq!(out.darts, vec![sc.dart]);
    assert_eq!(sc.p.pieces.len(), 1);
    assert_eq!(sc.p.pieces[0].id, sc.piece);
    let es = edges(&sc.p, sc.piece);
    assert_eq!(es.len(), 7);
    for (x, y) in [
        (110.0, 0.0),
        (200.0, 0.0),
        (200.0, 150.0),
        (100.0, 150.0),
        (100.0 - r101(1000.0), 100.0 + r101(4950.0)),
        (100.0 - r101(10900.0), 100.0 + r101(2950.0)),
        (100.0 - r101(7900.0), 100.0 - r101(11900.0)),
    ] {
        assert!(has_vertex(&es, x, y), "sommet ({x} ; {y}) absent de {es:?}");
    }
    // Une seule pince, à la cible : ouverture 10/√1,01, longueur 50/√1,01, mêmes angle et sommet.
    let calcs = darts_calc(&sc.p, sc.piece);
    assert_eq!(calcs.len(), 1);
    let (d, c) = &calcs[0];
    assert_eq!(d.id, sc.dart);
    let s101 = 1.01f64.sqrt();
    assert!(near(d.opening_mm, 10.0 / s101, 1e-9), "{}", d.opening_mm);
    assert!(near(d.length_mm, 50.0 / s101, 1e-9), "{}", d.length_mm);
    assert!(distance_mm(c.shape.apex, pt(100.0, 100.0)) < 1e-9);
    // Pieds : τ (fixe) et τ' (mobile) ; la pince ne suit plus l'ancien bord.
    let feet = [c.shape.left_foot, c.shape.right_foot];
    assert!(feet
        .iter()
        .any(|f| distance_mm(*f, pt(100.0, 150.0)) < 1e-9));
    assert!(feet
        .iter()
        .any(|f| distance_mm(*f, pt(100.0 - r101(1000.0), 100.0 + r101(4950.0))) < 1e-9));
    // Fermer la nouvelle pince : la ligne qui porte la base perd exactement la base.
    assert!(near(c.closed.removed_mm, 10.0 / s101, 1e-9));
}

#[test]
fn ca1_la_somme_des_angles_de_pince_est_conservee_total_et_partiel() {
    for (part, expected_darts) in [(1.0, 1usize), (0.5, 2), (0.25, 2), (0.9, 2)] {
        let mut sc = scene();
        let before = darts_calc(&sc.p, sc.piece);
        assert!(near(before[0].1.shape.angle_deg, theta_deg(), 1e-9));
        let c = transfer(&sc, pt(100.0, 150.0), part);
        sc.p.apply(c).unwrap();
        let after = darts_calc(&sc.p, sc.piece);
        assert_eq!(after.len(), expected_darts, "part {part}");
        let sum: f64 = after.iter().map(|(_, c)| c.shape.angle_deg).sum();
        assert!(
            near(sum, theta_deg(), 1e-6),
            "part {part} : somme {sum}° ≠ {}°",
            theta_deg()
        );
        // La pince transférée porte la part demandée de l'angle (calculée à la main).
        let transferred = after.iter().find(|(d, _)| d.id == sc.dart).unwrap();
        assert!(
            near(transferred.1.shape.angle_deg, part * theta_deg(), 1e-6),
            "part {part}"
        );
    }
}

#[test]
fn ca2_les_longueurs_hors_pince_sont_conservees_total_et_partiel() {
    for part in [1.0, 0.5, 0.2] {
        let mut sc = scene();
        let c = transfer(&sc, pt(100.0, 150.0), part);
        sc.p.apply(c).unwrap();
        let calcs = darts_calc(&sc.p, sc.piece);
        let es = edges(&sc.p, sc.piece);
        // Tracés non touchés par une base : 90, 150, 100 (à droite de la cible) puis, tournés,
        // 100, 150, 90 (à gauche) ; calculés à la main, jamais relus du code testé.
        let mut kept: Vec<f64> = es
            .iter()
            .filter(|e| !is_base(e, &calcs))
            .map(|e| e.2)
            .collect();
        kept.sort_by(f64::total_cmp);
        let expected = [90.0, 90.0, 100.0, 100.0, 150.0, 150.0];
        assert_eq!(kept.len(), expected.len(), "part {part} : {kept:?}");
        for (k, e) in kept.iter().zip(expected) {
            assert!(near(*k, e, 1e-6), "part {part} : {kept:?}");
        }
        // Périmètre = 680 + bases créées (jamais 700 − 20 sans elles).
        let bases: f64 = es.iter().filter(|e| is_base(e, &calcs)).map(|e| e.2).sum();
        let perimeter = sc.p.piece_contour(sc.piece).unwrap().perimeter_mm;
        assert!(near(perimeter, 680.0 + bases, 1e-6), "part {part}");
    }
}

#[test]
fn partiel_50_pour_cent_deux_pinces_sommets_et_ouvertures_calcules_a_la_main() {
    let mut sc = scene();
    let c = transfer(&sc, pt(100.0, 150.0), 0.5);
    let out = sc.p.apply(c).unwrap();
    assert_eq!(out.darts.len(), 2);
    assert_eq!(out.darts[0], sc.dart);
    let es = edges(&sc.p, sc.piece);
    assert_eq!(es.len(), 8);
    let s = 1.01f64.sqrt();
    for (x, y) in [
        (110.0, 0.0),
        (200.0, 0.0),
        (200.0, 150.0),
        (100.0, 150.0),
        (100.0 - 5.0 / s, 100.0 + 50.0 / s),
        (100.0 - 105.0 / s, 100.0 + 40.0 / s),
        (100.0 - 90.0 / s, 100.0 - 110.0 / s),
        (100.0, 100.0 - 101.0 / s),
    ] {
        assert!(has_vertex(&es, x, y), "sommet ({x} ; {y}) absent de {es:?}");
    }
    // Ouvertures : cordes 2·r·sin(φ/2), φ = atan(0,1), r = 50 (cible) et √10100 (origine).
    let half = 0.1f64.atan() / 2.0;
    let calcs = darts_calc(&sc.p, sc.piece);
    let new = calcs.iter().find(|(d, _)| d.id == sc.dart).unwrap();
    let rest = calcs.iter().find(|(d, _)| d.id == out.darts[1]).unwrap();
    assert!(near(new.0.opening_mm, 100.0 * half.sin(), 1e-9));
    assert!(near(new.0.length_mm, 50.0 * half.cos(), 1e-9));
    assert!(near(
        rest.0.opening_mm,
        2.0 * 10100.0f64.sqrt() * half.sin(),
        1e-9
    ));
    assert!(near(rest.0.length_mm, 10100.0f64.sqrt() * half.cos(), 1e-9));
    // Chacune a la moitié de l'angle d'origine.
    assert!(near(new.1.shape.angle_deg, theta_deg() / 2.0, 1e-6));
    assert!(near(rest.1.shape.angle_deg, theta_deg() / 2.0, 1e-6));
    // Le même sommet sert les deux pinces.
    assert_eq!(new.0.apex, rest.0.apex);
}

// ----- CA3 : refus sans trace ---------------------------------------------------------------

#[test]
fn ca3_refus_sans_trace_parts_distances_cibles_et_references() {
    let mut sc = scene();
    let good = transfer(&sc, pt(100.0, 150.0), 1.0);
    let Command::TransferPieceDart { edge, .. } = good.clone() else {
        unreachable!()
    };
    let with = |f: &dyn Fn(&mut Command)| {
        let mut c = good.clone();
        f(&mut c);
        c
    };
    macro_rules! set {
        ($field:ident = $value:expr) => {
            with(&|c| {
                if let Command::TransferPieceDart { $field, .. } = c {
                    *$field = $value;
                }
            })
        };
    }
    for bad in [0.0, -0.5, 1.0000001, 2.0, f64::NAN, f64::INFINITY] {
        refused(&mut sc.p, set!(part = bad));
    }
    for bad in [-1.0, 1e9, f64::NAN, f64::INFINITY] {
        refused(&mut sc.p, set!(distance_mm = bad));
    }
    refused(&mut sc.p, set!(piece = PieceId(987_654)));
    refused(&mut sc.p, set!(dart = DartId(987_654)));
    refused(&mut sc.p, set!(edge = EntityId(987_654)));
    // Cible sur la base (bord bas, entre les pieds) et sur un pied.
    for at in [pt(100.0, 0.0), pt(110.0, 0.0), pt(90.0, 0.0)] {
        let (edge, distance_mm) = target(&sc.p, sc.piece, at);
        let msg = refused(
            &mut sc.p,
            Command::TransferPieceDart {
                piece: sc.piece,
                dart: sc.dart,
                edge,
                distance_mm,
                part: 1.0,
            },
        );
        assert!(msg.contains("base"), "{msg}");
    }
    // Tracé qui n'est pas un bord du contour : un segment de construction posé à côté.
    let a = free_point(&mut sc.p, sc.s, 300.0, 0.0);
    let b = free_point(&mut sc.p, sc.s, 350.0, 0.0);
    let stray =
        sc.p.apply(Command::AddSegment {
            sheet: sc.s,
            from: PointRef::Existing(a),
            to: PointRef::Existing(b),
        })
        .unwrap()
        .entities[0];
    let _ = edge;
    refused(
        &mut sc.p,
        with(&|c| {
            if let Command::TransferPieceDart {
                edge, distance_mm, ..
            } = c
            {
                *edge = stray;
                *distance_mm = 10.0;
            }
        }),
    );
    // Autre pince sur la pièce : refus expliqué.
    let apex2 = free_point(&mut sc.p, sc.s, 40.0, 60.0);
    let axis2 = free_point(&mut sc.p, sc.s, 40.0, 0.0);
    sc.p.apply(Command::AddPieceDart {
        piece: sc.piece,
        apex: PointRef::Existing(apex2),
        toward: PointRef::Existing(axis2),
        length_mm: 60.0,
        opening_mm: 8.0,
        fixed: Side::Left,
    })
    .unwrap();
    let msg = refused(&mut sc.p, good.clone());
    assert!(msg.contains("2 pinces"), "{msg}");
}

#[test]
fn ca3_pince_non_calculable_et_pince_obtenue_invalide_sont_refusees_sans_trace() {
    // Pince valable puis sommet déplacé hors de la pièce : pince non calculable.
    let mut sc = scene();
    let d = sc.p.piece(sc.piece).unwrap().darts[0];
    let good = transfer(&sc, pt(100.0, 150.0), 1.0);
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: d.apex,
        to: pt(100.0, 500.0),
    })
    .unwrap();
    let msg = refused(&mut sc.p, good);
    assert!(msg.contains("transférée"), "{msg}");
    // Cible tout près de l'ancienne base : la pince obtenue ne tient pas dans la pièce (la
    // jambe traverse le contour) ou la partie tournée recoupe le contour : refus, pas de panic.
    let mut sc = scene();
    for x in [111.0, 150.0, 199.0] {
        let cmd = transfer(&sc, pt(x, 0.0), 1.0);
        let _ = sc.p.clone().apply(cmd.clone());
        let before = bytes(&sc.p);
        if sc.p.apply(cmd).is_err() {
            assert_eq!(before, bytes(&sc.p));
        } else {
            // Si elle est acceptée, la pièce reste lisible et la somme des angles conservée.
            let calcs = darts_calc(&sc.p, sc.piece);
            let sum: f64 = calcs.iter().map(|(_, c)| c.shape.angle_deg).sum();
            assert!(near(sum, theta_deg(), 1e-6));
            sc = scene();
        }
    }
}

#[test]
fn ca3_piece_derivee_liee_est_refusee() {
    let mut sc = scene();
    let derived =
        sc.p.apply(Command::CreateDerivedPiece {
            mother: sc.piece,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset { distance_mm: 5.0 },
            name: None,
        })
        .unwrap()
        .pieces[0];
    let cmd = Command::TransferPieceDart {
        piece: derived,
        dart: sc.dart,
        edge: EntityId(1),
        distance_mm: 1.0,
        part: 1.0,
    };
    refused(&mut sc.p, cmd);
}

// ----- CA4 : une étape annulable --------------------------------------------------------------

#[test]
fn ca4_une_seule_etape_annulable_et_un_refus_ne_cree_aucune_etape() {
    let sc = scene();
    let mut doc = Document::new(sc.p.clone());
    let base = bytes(doc.project());
    // Refus : aucune étape.
    let bad = Command::TransferPieceDart {
        piece: sc.piece,
        dart: sc.dart,
        edge: EntityId(987_654),
        distance_mm: 1.0,
        part: 1.0,
    };
    assert!(doc.execute(bad).is_err());
    assert_eq!(bytes(doc.project()), base);
    assert!(!doc.can_undo());
    doc.execute(transfer(&sc, pt(100.0, 150.0), 0.5)).unwrap();
    let after = bytes(doc.project());
    assert_ne!(after, base);
    assert!(doc.undo(), "une annulation");
    assert_eq!(bytes(doc.project()), base);
    assert!(!doc.can_undo(), "une seule étape");
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), after);
}

// ----- CA5 : fichier ------------------------------------------------------------------------

#[test]
fn ca5_fichier_schema_3_relu_a_l_identique_chargeur_strict_et_pf_validate() {
    let mut sc = scene();
    sc.p.apply(transfer(&sc, pt(100.0, 150.0), 0.5)).unwrap();
    let text = bytes(&sc.p);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["schema_version"], 6);
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, sc.p);
    assert_eq!(text, bytes(&back));
    assert_eq!(v["pieces"][0]["darts"].as_array().unwrap().len(), 2);
    // Chargeur strict : clé inconnue, côté illégal, ouverture hors bornes.
    let mutate = |f: &dyn Fn(&mut Value)| {
        let mut w = v.clone();
        f(&mut w["pieces"][0]["darts"][1]);
        persist::from_json(&serde_json::to_string(&w).unwrap())
    };
    assert!(mutate(&|_| {}).is_ok());
    assert!(mutate(&|d| d["z"] = json!(1)).is_err());
    assert!(mutate(&|d| d["fixed"] = json!("up")).is_err());
    assert!(mutate(&|d| d["opening_mm"] = json!(-1.0)).is_err());
    // Transfert total : une seule pince, aucune clé nouvelle.
    let mut sc = scene();
    sc.p.apply(transfer(&sc, pt(100.0, 150.0), 1.0)).unwrap();
    let text = bytes(&sc.p);
    assert_eq!(text, bytes(&persist::from_json(&text).unwrap()));
    // pf-validate : code 0.
    let dir = std::env::temp_dir().join(format!("pf-transfert-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ok = dir.join("ok.pforge.json");
    std::fs::write(&ok, &text).unwrap();
    let code = std::process::Command::new(env!("CARGO_BIN_EXE_pf-validate"))
        .arg(&ok)
        .output()
        .unwrap()
        .status
        .code();
    assert_eq!(code, Some(0));
    std::fs::remove_dir_all(&dir).ok();
}

// ----- CA6 : dérivées, surpiqûres, reports ------------------------------------------------------

#[test]
fn ca6_une_derivee_liee_est_regeneree_dans_la_meme_etape() {
    let mut sc = scene();
    let derived =
        sc.p.apply(Command::CreateDerivedPiece {
            mother: sc.piece,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset { distance_mm: -5.0 },
            name: None,
        })
        .unwrap()
        .pieces[0];
    let before = sc.p.piece_contour(derived).unwrap().perimeter_mm;
    let out = sc.p.apply(transfer(&sc, pt(100.0, 150.0), 1.0)).unwrap();
    assert!(out.regenerated.contains(&derived), "{out:?}");
    let after = sc.p.piece_contour(derived).unwrap();
    assert!(after.closed && !after.self_intersecting);
    assert!(
        (after.perimeter_mm - before).abs() > 1e-3,
        "le contour de la dérivée suit celui de la mère"
    );
    assert!(sc
        .p
        .piece_issues(derived)
        .iter()
        .all(|i| !matches!(i, pf_core::piece::PieceIssue::DerivedStale { .. })));
}

#[test]
fn ca6_surpiqure_liee_jamais_un_refus_de_la_commande() {
    let mut sc = scene();
    sc.p.apply(Command::AddTopstitch {
        sheet: sc.s,
        source: TopstitchSource::Piece { piece: sc.piece },
        distance_mm: 4.0,
        side: TopstitchSide::Inside,
    })
    .unwrap();
    let out = sc.p.apply(transfer(&sc, pt(100.0, 150.0), 1.0)).unwrap();
    assert_eq!(out.pieces, vec![sc.piece]);
    // La pièce a quitté la feuille : le lien est rompu avec remarque, les lignes restent libres.
    assert!(sc.p.sheet(sc.s).unwrap().topstitches.is_empty());
    assert!(out
        .notes
        .iter()
        .any(|n| n.contains("n'est plus liée à son bord")));
}

#[test]
fn ca6_un_droit_fil_dans_la_partie_mobile_tourne_avec_elle() {
    let mut sc = scene();
    let a = free_point(&mut sc.p, sc.s, 30.0, 50.0);
    let b = free_point(&mut sc.p, sc.s, 30.0, 100.0);
    let grain =
        sc.p.apply(Command::AddSegment {
            sheet: sc.s,
            from: PointRef::Existing(a),
            to: PointRef::Existing(b),
        })
        .unwrap()
        .entities[0];
    let outline = sc.p.piece(sc.piece).unwrap().outline.clone();
    sc.p.apply(Command::SetPieceOutline {
        piece: sc.piece,
        outline,
        grainline: Some(grain),
    })
    .unwrap();
    sc.p.apply(transfer(&sc, pt(100.0, 150.0), 1.0)).unwrap();
    let g = sc.p.piece(sc.piece).unwrap().grainline.unwrap();
    // R(θ) autour de (100 ; 100), cos = 99/101, sin = 20/101 : (30 ; 50) → (100 − 5930/101 ;
    // 100 − 6350/101) ; (30 ; 100) → (100 − 6930/101 ; 100 − 1400/101).
    match sc.p.piece_sheet(sc.piece).unwrap().shape(g).unwrap() {
        Shape::Line(p, q) => {
            assert!(distance_mm(p, pt(100.0 - r101(5930.0), 100.0 - r101(6350.0))) < 1e-9);
            assert!(distance_mm(q, pt(100.0 - r101(6930.0), 100.0 - r101(1400.0))) < 1e-9);
            // Longueur du droit-fil conservée.
            assert!(near(distance_mm(p, q), 50.0, 1e-9));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn ca6_droit_fil_et_marquage_suivent_leur_partie_ceux_qui_traversent_sont_signales() {
    let mut sc = scene();
    // Droit-fil vertical dans la partie droite (x = 150, reste en place) ; marquage dans la
    // partie gauche (30 ; 100), qui tourne.
    let a = free_point(&mut sc.p, sc.s, 150.0, 30.0);
    let b = free_point(&mut sc.p, sc.s, 150.0, 120.0);
    let grain =
        sc.p.apply(Command::AddSegment {
            sheet: sc.s,
            from: PointRef::Existing(a),
            to: PointRef::Existing(b),
        })
        .unwrap()
        .entities[0];
    let outline = sc.p.piece(sc.piece).unwrap().outline.clone();
    sc.p.apply(Command::SetPieceOutline {
        piece: sc.piece,
        outline,
        grainline: Some(grain),
    })
    .unwrap();
    let out = sc.p.apply(transfer(&sc, pt(100.0, 150.0), 1.0)).unwrap();
    let sheet = sc.p.piece_sheet(sc.piece).unwrap();
    let g =
        sc.p.piece(sc.piece)
            .unwrap()
            .grainline
            .expect("droit-fil repris");
    match sheet.shape(g).unwrap() {
        Shape::Line(p, q) => {
            assert!(distance_mm(p, pt(150.0, 30.0)) < 1e-9, "{p:?}");
            assert!(distance_mm(q, pt(150.0, 120.0)) < 1e-9, "{q:?}");
        }
        other => panic!("{other:?}"),
    }
    assert!(out.notes.iter().any(|n| n.contains("transférée")));
}

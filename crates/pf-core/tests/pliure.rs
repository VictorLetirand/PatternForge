//! PH-5b — bord « sur le pli » : déclaration (CA1), coupe sans marge ni bord de coupe (CA2),
//! lecture pour la fiche et le rapport (CA4), régénération, historique, copie, dépliage (CA5).
//! Exports : `pf-export/tests/export_pliure.rs` ; interface : `ui_tests::ph5b_*` ; campagne :
//! `adverse_pliure.rs`.
//!
//! **Valeurs calculées à la main**, jamais relues du code testé.
//!
//! # Cas de référence
//!
//! Rectangle 100 × 100 de sommets (0 ; 0), (100 ; 0), (100 ; 100), (0 ; 100), marge 10 mm.
//!
//! - pliure sur le bord **gauche** (x = 0, parcouru de (0 ; 100) à (0 ; 0) en anti-horaire) : les trois
//!   autres bords sont décalés de 10 mm, le bord gauche reste en x = 0 ; coupe = [0 ; 110] × [−10 ; 110]
//!   soit 110 × 120, aire 13 200 mm², périmètre 2 × (110 + 120) = 460 mm ; la pliure suit la coupe de
//!   (0 ; 110) à (0 ; −10), longueur 120 mm ;
//! - pliure sur le bord **bas** (y = 0) : coupe = [−10 ; 110] × [0 ; 110] soit 120 × 110, mêmes aire et
//!   périmètre ; la pliure va de (−10 ; 0) à (110 ; 0), longueur 120 mm ;
//! - sans pliure : [−10 ; 110]² soit 120 × 120, aire 14 400 mm², périmètre 480 mm.

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::region::trace_geometry;
use pf_geometry::{distance_mm, PointMm, Trace};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

struct Sc {
    p: Project,
    s: SheetId,
    piece: PieceId,
}

fn polygon_scene(vertices: &[(f64, f64)], margin: f64) -> Sc {
    let mut p = Project::new("Pliure");
    let s = p.sheets[0].id;
    let n = vertices.len();
    for i in 0..n {
        let (a, b) = (vertices[i], vertices[(i + 1) % n]);
        p.apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(a.0, a.1)),
            to: PointRef::New(pt(b.0, b.1)),
        })
        .unwrap();
    }
    let cx = vertices.iter().map(|v| v.0).sum::<f64>() / n as f64;
    let cy = vertices.iter().map(|v| v.1).sum::<f64>() / n as f64;
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(cx, cy) },
            name: "Demi-devant".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    if margin > 0.0 {
        p.apply(Command::SetSeamAllowance {
            piece,
            seam: Some(SeamAllowance {
                default_mm: margin,
                ..Default::default()
            }),
        })
        .unwrap();
    }
    Sc { p, s, piece }
}

fn square(margin: f64) -> Sc {
    polygon_scene(
        &[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)],
        margin,
    )
}

/// Tracé du contour dont les extrémités sont `a` et `b` (dans un sens ou dans l'autre).
fn edge(sc: &Sc, a: (f64, f64), b: (f64, f64)) -> EntityId {
    let sheet = sc.p.piece_sheet(sc.piece).unwrap();
    let (a, b) = (pt(a.0, a.1), pt(b.0, b.1));
    sc.p.piece(sc.piece)
        .unwrap()
        .outline
        .iter()
        .find(|e| match sheet.shape(e.entity) {
            Ok(Shape::Line(x, y)) => {
                (distance_mm(x, a) < 1e-9 && distance_mm(y, b) < 1e-9)
                    || (distance_mm(x, b) < 1e-9 && distance_mm(y, a) < 1e-9)
            }
            _ => false,
        })
        .expect("bord du contour")
        .entity
}

fn set_fold(sc: &mut Sc, edge: EntityId, on: bool) -> Result<Outcome, CoreError> {
    sc.p.apply(Command::SetPieceFold {
        piece: sc.piece,
        edge,
        on,
    })
}

fn starts(p: &Project, piece: PieceId) -> Vec<PointMm> {
    p.piece_cut_contour(piece)
        .unwrap()
        .traces
        .iter()
        .flat_map(trace_geometry)
        .map(|g| g.start())
        .collect()
}

/// Boîte englobante (min x, min y, max x, max y) des sommets de la coupe.
fn cut_box(p: &Project, piece: PieceId) -> (f64, f64, f64, f64) {
    let v = starts(p, piece);
    let f = |g: fn(&PointMm) -> f64, pick: fn(f64, f64) -> f64, init: f64| {
        v.iter().map(g).fold(init, pick)
    };
    // Arrondi au millionième de mm : les sommets viennent d'intersections en flottants.
    let r = |x: f64| (x * 1e6).round() / 1e6;
    (
        r(f(|q| q.x, f64::min, f64::MAX)),
        r(f(|q| q.y, f64::min, f64::MAX)),
        r(f(|q| q.x, f64::max, f64::MIN)),
        r(f(|q| q.y, f64::max, f64::MIN)),
    )
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

// ----- CA1 : déclaration ----------------------------------------------------------------

#[test]
fn ca1_declaration_trie_les_bords_et_se_retire() {
    let mut sc = square(0.0);
    let left = edge(&sc, (0.0, 100.0), (0.0, 0.0));
    let bottom = edge(&sc, (0.0, 0.0), (100.0, 0.0));
    set_fold(&mut sc, left.max(bottom), true).unwrap();
    set_fold(&mut sc, left.min(bottom), true).unwrap();
    let declared = &sc.p.piece(sc.piece).unwrap().on_fold;
    assert_eq!(declared, &vec![left.min(bottom), left.max(bottom)], "trié");
    set_fold(&mut sc, left, false).unwrap();
    assert_eq!(sc.p.piece(sc.piece).unwrap().on_fold, vec![bottom]);
    // Aucun champ d'autre pièce touché ; relecture JSON identique ; schéma toujours 3.
    let text = bytes(&sc.p);
    assert!(text.contains("\"on_fold\""));
    let back = persist::from_json(&text).unwrap();
    assert_eq!(bytes(&back), text);
    assert_eq!(back.piece(sc.piece).unwrap().on_fold, vec![bottom]);
}

#[test]
fn ca1_sans_declaration_le_champ_est_omis() {
    let sc = square(10.0);
    assert!(!bytes(&sc.p).contains("on_fold"));
}

#[test]
fn ca1_une_etape_annulable_octets_exacts_et_un_no_op_sans_etape() {
    let sc = square(10.0);
    let left = edge(&sc, (0.0, 100.0), (0.0, 0.0));
    let piece = sc.piece;
    let mut doc = Document::new(sc.p);
    let before = bytes(doc.project());
    doc.execute(Command::SetPieceFold {
        piece,
        edge: left,
        on: true,
    })
    .unwrap();
    let after = bytes(doc.project());
    assert_ne!(before, after);
    // Même déclaration : aucune étape ; retirer ce qui n'est pas déclaré : aucune étape.
    doc.execute(Command::SetPieceFold {
        piece,
        edge: left,
        on: true,
    })
    .unwrap();
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before, "annuler = octets d'avant");
    assert!(!doc.can_undo(), "un seul pas d'historique");
    doc.execute(Command::SetPieceFold {
        piece,
        edge: left,
        on: false,
    })
    .unwrap();
    assert!(!doc.can_undo(), "retirer l'absent n'est pas une étape");
    assert_eq!(bytes(doc.project()), before);
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), after, "rétablir = octets d'après");
}

#[test]
fn ca1_refus_sans_trace() {
    let mut sc = square(10.0);
    let left = edge(&sc, (0.0, 100.0), (0.0, 0.0));
    let (before, text) = (sc.p.clone(), bytes(&sc.p));
    let refuse = |sc: &mut Sc, c: Command| {
        let e = sc.p.apply(c).expect_err("refus attendu");
        assert!(!e.to_string().is_empty());
        assert_eq!(sc.p, before, "aucune trace");
        assert_eq!(bytes(&sc.p), text, "octets identiques");
    };
    // Pièce inconnue, bord inconnu.
    refuse(
        &mut sc,
        Command::SetPieceFold {
            piece: PieceId(999_999),
            edge: left,
            on: true,
        },
    );
    let piece = sc.piece;
    refuse(
        &mut sc,
        Command::SetPieceFold {
            piece,
            edge: EntityId(999_999),
            on: true,
        },
    );
    refuse(
        &mut sc,
        Command::SetPieceFold {
            piece,
            edge: EntityId(999_999),
            on: false,
        },
    );
    // Tracé de la feuille qui n'est pas dans le contour : refusé à la pose.
    let s = sc.s;
    sc.p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::New(pt(500.0, 0.0)),
        to: PointRef::New(pt(600.0, 0.0)),
    })
    .unwrap();
    let outside = sc.p.sheets[0].entities.last().unwrap().id;
    let (before2, text2) = (sc.p.clone(), bytes(&sc.p));
    let e =
        sc.p.apply(Command::SetPieceFold {
            piece: sc.piece,
            edge: outside,
            on: true,
        })
        .expect_err("hors contour");
    assert!(e.to_string().contains("contour"), "{e}");
    assert_eq!(sc.p, before2);
    assert_eq!(bytes(&sc.p), text2);
}

#[test]
fn ca1_bord_courbe_refuse() {
    // Demi-disque approché : trois segments et une courbe de Bézier qui ferme le contour à gauche.
    let mut p = Project::new("Courbe");
    let s = p.sheets[0].id;
    for (a, b) in [
        ((0.0, 0.0), (100.0, 0.0)),
        ((100.0, 0.0), (100.0, 100.0)),
        ((100.0, 100.0), (0.0, 100.0)),
    ] {
        p.apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(a.0, a.1)),
            to: PointRef::New(pt(b.0, b.1)),
        })
        .unwrap();
    }
    p.apply(Command::AddCurve {
        sheet: s,
        start: PointRef::New(pt(0.0, 100.0)),
        c1: pt(-40.0, 70.0),
        c2: pt(-40.0, 30.0),
        end: PointRef::New(pt(0.0, 0.0)),
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(50.0, 50.0) },
            name: "Courbe".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let curve = p
        .piece(piece)
        .unwrap()
        .outline
        .iter()
        .map(|e| e.entity)
        .find(|id| {
            !matches!(
                p.sheets[0].entity(*id).unwrap().kind,
                EntityKind::Segment { .. }
            )
        })
        .expect("la courbe est dans le contour");
    let (before, text) = (p.clone(), bytes(&p));
    let e = p
        .apply(Command::SetPieceFold {
            piece,
            edge: curve,
            on: true,
        })
        .expect_err("bord non rectiligne");
    assert!(e.to_string().contains("rectiligne"), "{e}");
    assert_eq!(p, before);
    assert_eq!(bytes(&p), text);
}

#[test]
fn ca1_piece_derivee_liee_refusee() {
    let mut sc = square(10.0);
    let out =
        sc.p.apply(Command::CreateDerivedPiece {
            mother: sc.piece,
            kind: derived::DerivedKind::Lining,
            operation: derived::DerivedOp::Offset { distance_mm: -2.0 },
            name: None,
        })
        .unwrap();
    let dp = out.pieces[0];
    let dedge = sc.p.piece(dp).unwrap().outline[0].entity;
    let (before, text) = (sc.p.clone(), bytes(&sc.p));
    let e =
        sc.p.apply(Command::SetPieceFold {
            piece: dp,
            edge: dedge,
            on: true,
        })
        .expect_err("dérivée liée");
    assert!(e.to_string().contains("dérivée"), "{e}");
    assert_eq!(sc.p, before);
    assert_eq!(bytes(&sc.p), text);
}

#[test]
fn ca1_fichier_a_reference_invalide_refuse_a_l_ouverture() {
    let mut sc = square(0.0);
    let left = edge(&sc, (0.0, 100.0), (0.0, 0.0));
    set_fold(&mut sc, left, true).unwrap();
    let text = bytes(&sc.p);
    assert!(persist::from_json(&text).is_ok());
    let with = |edit: &dyn Fn(&mut serde_json::Value)| {
        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        edit(&mut v["pieces"][0]["on_fold"]);
        serde_json::to_string_pretty(&v).unwrap()
    };
    // Tracé inexistant, doublon, tracé non rectiligne n'existe pas ici : deux refus suffisent.
    let dead = with(&|f| *f = serde_json::json!([987_654]));
    assert!(persist::from_json(&dead).is_err());
    let dup = with(&|f| *f = serde_json::json!([left.0, left.0]));
    assert!(persist::from_json(&dup).is_err());
    // Texte à la place d'un identifiant : refusé.
    let typed = with(&|f| *f = serde_json::json!(["a"]));
    assert!(persist::from_json(&typed).is_err());
    // Forme valide : acceptée.
    let ok = with(&|f| *f = serde_json::json!([left.0]));
    assert!(persist::from_json(&ok).is_ok());
}

// ----- CA2 : coupe -----------------------------------------------------------------------

#[test]
fn ca2_sans_declaration_la_coupe_est_120_sur_120() {
    let sc = square(10.0);
    let c = sc.p.piece_cut_contour(sc.piece).unwrap();
    assert!(c.fold_cuts.is_empty());
    assert!(near(c.area_mm2, 14_400.0) && near(c.perimeter_mm, 480.0));
    let b = cut_box(&sc.p, sc.piece);
    assert_eq!(b, (-10.0, -10.0, 110.0, 110.0));
}

#[test]
fn ca2_pliure_a_gauche_coupe_110_sur_120() {
    let mut sc = square(10.0);
    let left = edge(&sc, (0.0, 100.0), (0.0, 0.0));
    set_fold(&mut sc, left, true).unwrap();
    let c = sc.p.piece_cut_contour(sc.piece).unwrap();
    assert_eq!(cut_box(&sc.p, sc.piece), (0.0, -10.0, 110.0, 110.0));
    assert!(near(c.area_mm2, 13_200.0), "{}", c.area_mm2);
    assert!(near(c.perimeter_mm, 460.0), "{}", c.perimeter_mm);
    // Une seule portion de coupe est la pliure : x = 0 de (0 ; 110) à (0 ; −10).
    assert_eq!(c.fold_cuts.len(), 1);
    assert_eq!(c.fold_cuts[0].entity, left);
    match c.traces[c.fold_cuts[0].trace] {
        Trace::Line(a, b) => {
            assert!(distance_mm(a, pt(0.0, 110.0)) < 1e-9, "{a:?}");
            assert!(distance_mm(b, pt(0.0, -10.0)) < 1e-9, "{b:?}");
        }
        ref other => panic!("la pliure est un segment : {other:?}"),
    }
    // Les trois autres bords de coupe restent à 10 mm : quatre traces, la coupe reste fermée.
    assert_eq!(c.traces.len(), 4);
}

#[test]
fn ca2_pliure_en_bas_coupe_120_sur_110() {
    let mut sc = square(10.0);
    let bottom = edge(&sc, (0.0, 0.0), (100.0, 0.0));
    set_fold(&mut sc, bottom, true).unwrap();
    let c = sc.p.piece_cut_contour(sc.piece).unwrap();
    assert_eq!(cut_box(&sc.p, sc.piece), (-10.0, 0.0, 110.0, 110.0));
    assert!(near(c.area_mm2, 13_200.0) && near(c.perimeter_mm, 460.0));
    match c.traces[c.fold_cuts[0].trace] {
        Trace::Line(a, b) => {
            assert!(distance_mm(a, pt(-10.0, 0.0)) < 1e-9, "{a:?}");
            assert!(distance_mm(b, pt(110.0, 0.0)) < 1e-9, "{b:?}");
        }
        ref other => panic!("{other:?}"),
    }
}

#[test]
fn ca2_deux_pliures_opposees() {
    // Bords gauche et droit sur le pli : coupe = [0 ; 100] × [−10 ; 110] soit 100 × 120.
    let mut sc = square(10.0);
    let left = edge(&sc, (0.0, 100.0), (0.0, 0.0));
    let right = edge(&sc, (100.0, 0.0), (100.0, 100.0));
    set_fold(&mut sc, left, true).unwrap();
    set_fold(&mut sc, right, true).unwrap();
    let c = sc.p.piece_cut_contour(sc.piece).unwrap();
    assert_eq!(cut_box(&sc.p, sc.piece), (0.0, -10.0, 100.0, 110.0));
    assert!(near(c.area_mm2, 12_000.0) && near(c.perimeter_mm, 440.0));
    assert_eq!(c.fold_cuts.len(), 2);
}

#[test]
fn ca2_marge_reglee_sur_le_bord_ignoree_puis_rendue() {
    let mut sc = square(0.0);
    let bottom = edge(&sc, (0.0, 0.0), (100.0, 0.0));
    // Marge 10 par défaut, 20 sur le bord bas (surcharge posée par l'utilisateur).
    sc.p.apply(Command::SetSeamAllowance {
        piece: sc.piece,
        seam: Some(SeamAllowance {
            default_mm: 10.0,
            edges: vec![EdgeAllowance {
                entity: bottom,
                start_mm: 20.0,
                end_mm: 20.0,
            }],
            ..Default::default()
        }),
    })
    .unwrap();
    assert_eq!(cut_box(&sc.p, sc.piece).1, -20.0);
    set_fold(&mut sc, bottom, true).unwrap();
    // Ignorée tant que le bord est sur le pli : y_min = 0, hauteur 110 ; contrôle posé.
    assert_eq!(cut_box(&sc.p, sc.piece), (-10.0, 0.0, 110.0, 110.0));
    let issues = sc.p.piece_issues(sc.piece);
    assert!(
        issues.contains(&PieceIssue::FoldMarginIgnored { count: 1 }),
        "{issues:?}"
    );
    // Jamais détruite : la surcharge est toujours dans la fiche, et revient avec la déclaration.
    assert_eq!(
        sc.p.piece(sc.piece)
            .unwrap()
            .seam
            .as_ref()
            .unwrap()
            .edges
            .len(),
        1
    );
    set_fold(&mut sc, bottom, false).unwrap();
    assert_eq!(cut_box(&sc.p, sc.piece).1, -20.0);
    assert!(!sc
        .p
        .piece_issues(sc.piece)
        .iter()
        .any(|i| matches!(i, PieceIssue::FoldMarginIgnored { .. })));
}

#[test]
fn ca2_voisin_aligne_de_marge_differente_marche_et_pliure_courte() {
    // Bord gauche en deux segments alignés : (0 ; 100)→(0 ; 50) et (0 ; 50)→(0 ; 0). Le bas du bord
    // gauche est sur le pli ; le haut garde la marge 10 : marche de 10 mm en (·; 50).
    // Coupe : x = −10 de y = 110 à 50, marche vers (0 ; 50), pliure de (0 ; 50) à (0 ; −10).
    // Aire = 110 × 120 (rectangle [0;110]×[−10;110]) + 10 × 60 (bande [−10;0]×[50;110]) = 13 800.
    let mut sc = polygon_scene(
        &[
            (0.0, 0.0),
            (100.0, 0.0),
            (100.0, 100.0),
            (0.0, 100.0),
            (0.0, 50.0),
        ],
        10.0,
    );
    let lower = edge(&sc, (0.0, 50.0), (0.0, 0.0));
    set_fold(&mut sc, lower, true).unwrap();
    let c = sc.p.piece_cut_contour(sc.piece).unwrap();
    assert!(near(c.area_mm2, 13_800.0), "{}", c.area_mm2);
    assert_eq!(c.fold_cuts.len(), 1);
    match c.traces[c.fold_cuts[0].trace] {
        Trace::Line(a, b) => {
            assert!(distance_mm(a, pt(0.0, 50.0)) < 1e-9, "{a:?}");
            assert!(distance_mm(b, pt(0.0, -10.0)) < 1e-9, "{b:?}");
        }
        ref other => panic!("{other:?}"),
    }
}

#[test]
fn ca2_voisin_aligne_de_marge_nulle_fusionne_puis_decoupe() {
    // Même contour, mais le haut du bord gauche a aussi une marge nulle : la coupe fusionne les deux
    // bords alignés en un seul trait x = 0 de (0 ; 110) à (0 ; −10). La pliure ne couvre que la
    // part du bord déclaré (de (0 ; 50) à (0 ; −10)) ; (0 ; 110)→(0 ; 50) reste un bord de coupe.
    // Aire = rectangle [0;110]×[−10;110] = 13 200.
    let mut sc = polygon_scene(
        &[
            (0.0, 0.0),
            (100.0, 0.0),
            (100.0, 100.0),
            (0.0, 100.0),
            (0.0, 50.0),
        ],
        0.0,
    );
    let (lower, upper) = (
        edge(&sc, (0.0, 50.0), (0.0, 0.0)),
        edge(&sc, (0.0, 100.0), (0.0, 50.0)),
    );
    sc.p.apply(Command::SetSeamAllowance {
        piece: sc.piece,
        seam: Some(SeamAllowance {
            default_mm: 10.0,
            edges: vec![EdgeAllowance {
                entity: upper,
                start_mm: 0.0,
                end_mm: 0.0,
            }],
            ..Default::default()
        }),
    })
    .unwrap();
    set_fold(&mut sc, lower, true).unwrap();
    let c = sc.p.piece_cut_contour(sc.piece).unwrap();
    assert!(near(c.area_mm2, 13_200.0), "{}", c.area_mm2);
    assert_eq!(c.fold_cuts.len(), 1);
    match c.traces[c.fold_cuts[0].trace] {
        Trace::Line(a, b) => {
            assert!(distance_mm(a, pt(0.0, 50.0)) < 1e-9, "{a:?}");
            assert!(distance_mm(b, pt(0.0, -10.0)) < 1e-9, "{b:?}");
        }
        ref other => panic!("{other:?}"),
    }
    // La part restante (0 ; 110)→(0 ; 50) est une trace de coupe, pas une pliure.
    let cut_part = c
        .traces
        .iter()
        .enumerate()
        .filter(|(i, _)| !c.fold_cuts.iter().any(|f| f.trace == *i))
        .any(|(_, t)| {
            matches!(t, Trace::Line(a, b)
            if distance_mm(*a, pt(0.0, 110.0)) < 1e-9 && distance_mm(*b, pt(0.0, 50.0)) < 1e-9)
        });
    assert!(cut_part);
}

#[test]
fn ca2_sans_marge_la_pliure_est_le_bord_lui_meme() {
    let mut sc = square(0.0);
    let left = edge(&sc, (0.0, 100.0), (0.0, 0.0));
    set_fold(&mut sc, left, true).unwrap();
    let lines = sc.p.piece_fold_lines(sc.piece, true);
    assert_eq!(lines.len(), 1);
    assert!(distance_mm(lines[0].from, pt(0.0, 100.0)) < 1e-9);
    assert!(distance_mm(lines[0].to, pt(0.0, 0.0)) < 1e-9);
    // Coupe = contour dessiné (aucune marge) : aire 10 000.
    let c = sc.p.piece_cut_contour(sc.piece).unwrap();
    assert!(near(c.area_mm2, 10_000.0));
}

#[test]
fn ca2_lignes_de_pliure_avec_marge_suivent_la_coupe_et_sans_coupe_le_bord() {
    let mut sc = square(10.0);
    let left = edge(&sc, (0.0, 100.0), (0.0, 0.0));
    set_fold(&mut sc, left, true).unwrap();
    let with_cut = sc.p.piece_fold_lines(sc.piece, true);
    assert!(distance_mm(with_cut[0].from, pt(0.0, 110.0)) < 1e-9);
    assert!(distance_mm(with_cut[0].to, pt(0.0, -10.0)) < 1e-9);
    let sewing = sc.p.piece_fold_lines(sc.piece, false);
    assert!(distance_mm(sewing[0].from, pt(0.0, 100.0)) < 1e-9);
    assert!(distance_mm(sewing[0].to, pt(0.0, 0.0)) < 1e-9);
}

// ----- CA4 : lecture seule ---------------------------------------------------------------

#[test]
fn ca4_resume_nombre_et_longueur() {
    let mut sc = square(10.0);
    assert_eq!(sc.p.piece_fold_summary(sc.piece), FoldSummary::default());
    let left = edge(&sc, (0.0, 100.0), (0.0, 0.0));
    let bottom = edge(&sc, (0.0, 0.0), (100.0, 0.0));
    set_fold(&mut sc, left, true).unwrap();
    set_fold(&mut sc, bottom, true).unwrap();
    let s = sc.p.piece_fold_summary(sc.piece);
    assert_eq!(s.count, 2);
    assert!(near(s.length_mm, 200.0), "{}", s.length_mm);
}

// ----- CA5 : interactions ---------------------------------------------------------------

#[test]
fn ca5_le_bord_reste_sur_le_pli_quand_on_deplace_un_point() {
    let mut sc = square(10.0);
    let left = edge(&sc, (0.0, 100.0), (0.0, 0.0));
    set_fold(&mut sc, left, true).unwrap();
    // Le coin haut-gauche (0 ; 100) monte en (0 ; 120) : la pliure s'allonge, la coupe suit.
    let top_left = sc.p.sheets[0]
        .points
        .iter()
        .find(|q| {
            sc.p.sheets[0]
                .position(q.id)
                .is_ok_and(|v| distance_mm(v, pt(0.0, 100.0)) < 1e-9)
        })
        .unwrap()
        .id;
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: top_left,
        to: pt(0.0, 120.0),
    })
    .unwrap();
    assert_eq!(sc.p.piece(sc.piece).unwrap().on_fold, vec![left]);
    let s = sc.p.piece_fold_summary(sc.piece);
    assert!(near(s.length_mm, 120.0), "{}", s.length_mm);
    // Coupe : x ∈ [0 ; 110], y ∈ [−10 ; 130] (le haut est maintenant penché mais son point le plus
    // haut est (0 ; 120) + marge 10 en bord supérieur incliné) : on vérifie seulement la pliure.
    let c = sc.p.piece_cut_contour(sc.piece).unwrap();
    assert_eq!(c.fold_cuts.len(), 1);
    let min_x = starts(&sc.p, sc.piece)
        .iter()
        .map(|q| q.x)
        .fold(f64::MAX, f64::min);
    assert!(
        near(min_x, 0.0),
        "aucune coupe en dehors de la pliure à gauche : {min_x}"
    );
}

#[test]
fn ca5_historique_deplacement_octets_exacts() {
    let sc = square(10.0);
    let left = edge(&sc, (0.0, 100.0), (0.0, 0.0));
    let piece = sc.piece;
    let mut doc = Document::new(sc.p);
    let base = bytes(doc.project());
    doc.execute(Command::SetPieceFold {
        piece,
        edge: left,
        on: true,
    })
    .unwrap();
    let folded = bytes(doc.project());
    let moved_point = doc.project().sheets[0]
        .points
        .iter()
        .find(|q| {
            doc.project().sheets[0]
                .position(q.id)
                .is_ok_and(|v| distance_mm(v, pt(100.0, 100.0)) < 1e-9)
        })
        .unwrap()
        .id;
    let s = doc.project().sheets[0].id;
    doc.execute(Command::MovePoint {
        sheet: s,
        point: moved_point,
        to: pt(130.0, 100.0),
    })
    .unwrap();
    let moved = bytes(doc.project());
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), folded);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), base);
    assert!(doc.redo() && doc.redo());
    assert_eq!(bytes(doc.project()), moved);
}

#[test]
fn ca5_copie_de_feuille_remappe_le_bord() {
    let mut sc = square(10.0);
    let left = edge(&sc, (0.0, 100.0), (0.0, 0.0));
    set_fold(&mut sc, left, true).unwrap();
    let out = sc.p.apply(Command::DuplicateSheet { sheet: sc.s }).unwrap();
    let new_sheet = out.sheet.expect("feuille copiée");
    let copy = sc
        .p
        .pieces
        .iter()
        .find(|p| {
            p.id != sc.piece && matches!(p.link, PieceLink::Linked { sheet } if sheet == new_sheet)
        })
        .unwrap();
    assert_eq!(copy.on_fold.len(), 1);
    assert_ne!(copy.on_fold[0], left, "identifiant de la copie");
    let copy_id = copy.id;
    let c = sc.p.piece_cut_contour(copy_id).unwrap();
    assert!(near(c.area_mm2, 13_200.0));
    assert_eq!(c.fold_cuts.len(), 1);
    assert_eq!(cut_box(&sc.p, copy_id), (0.0, -10.0, 110.0, 110.0));
}

#[test]
fn ca5_depliage_propose_le_bord_declare_en_premier() {
    let mut sc = square(0.0);
    let left = edge(&sc, (0.0, 100.0), (0.0, 0.0));
    // Sans déclaration : ordre du contour.
    let before = sc.p.unfold_candidates(sc.piece);
    assert!(before.iter().all(|c| !c.on_fold));
    let other = before.iter().find(|c| c.entity != left).unwrap().entity;
    assert_ne!(
        before[0].entity, left,
        "le bord gauche n'est pas déjà premier"
    );
    let _ = other;
    set_fold(&mut sc, left, true).unwrap();
    let after = sc.p.unfold_candidates(sc.piece);
    assert_eq!(after[0].entity, left);
    assert!(after[0].on_fold);
    assert!(after[1..].iter().all(|c| !c.on_fold));
    // Déplier selon ce bord : pièce entière 200 × 100 ; la déclaration n'est pas reprise (aucune
    // remarque : le bord déplié disparaît du contour).
    let out =
        sc.p.apply(Command::UnfoldPiece {
            piece: sc.piece,
            fold: left,
            name: None,
        })
        .unwrap();
    let whole = out.pieces[0];
    assert!(sc.p.piece(whole).unwrap().on_fold.is_empty());
    assert!(!out.notes.iter().any(|n| n.contains("sur le pli")));
    assert!(near(sc.p.piece_contour(whole).unwrap().area_mm2, 20_000.0));
}

#[test]
fn ca5_depliage_note_les_autres_bords_declares() {
    let mut sc = square(0.0);
    let left = edge(&sc, (0.0, 100.0), (0.0, 0.0));
    let bottom = edge(&sc, (0.0, 0.0), (100.0, 0.0));
    set_fold(&mut sc, left, true).unwrap();
    set_fold(&mut sc, bottom, true).unwrap();
    let out =
        sc.p.apply(Command::UnfoldPiece {
            piece: sc.piece,
            fold: left,
            name: None,
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("sur le pli")),
        "{:?}",
        out.notes
    );
}

#[test]
fn ca5_separer_ne_reprend_pas_la_declaration_et_le_dit() {
    let mut sc = square(0.0);
    let left = edge(&sc, (0.0, 100.0), (0.0, 0.0));
    set_fold(&mut sc, left, true).unwrap();
    let out =
        sc.p.apply(Command::SplitPiece {
            piece: sc.piece,
            a: pt(50.0, -10.0),
            b: pt(50.0, 110.0),
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("sur le pli")),
        "{:?}",
        out.notes
    );
    for id in &out.pieces {
        assert!(sc.p.piece(*id).unwrap().on_fold.is_empty());
    }
}

#[test]
fn ca5_bord_qui_quitte_le_contour_avertit_sans_refuser() {
    let mut sc = square(10.0);
    let left = edge(&sc, (0.0, 100.0), (0.0, 0.0));
    set_fold(&mut sc, left, true).unwrap();
    // Le contour perd le bord gauche : la commande n'est pas refusée, la déclaration reste, inerte,
    // et un contrôle le dit.
    let outline: Vec<PieceEdge> =
        sc.p.piece(sc.piece)
            .unwrap()
            .outline
            .iter()
            .copied()
            .filter(|e| e.entity != left)
            .collect();
    sc.p.apply(Command::SetPieceOutline {
        piece: sc.piece,
        outline,
        grainline: None,
    })
    .unwrap();
    assert_eq!(sc.p.piece(sc.piece).unwrap().on_fold, vec![left]);
    assert!(sc.p.piece_fold_edges(sc.piece).is_empty());
    assert!(sc
        .p
        .piece_issues(sc.piece)
        .contains(&PieceIssue::FoldOutsideContour { count: 1 }));
}

#[test]
fn ca5_tracé_supprime_retire_la_declaration_avec_remarque() {
    let mut sc = square(0.0);
    let left = edge(&sc, (0.0, 100.0), (0.0, 0.0));
    set_fold(&mut sc, left, true).unwrap();
    // On retire d'abord le bord du contour, puis le tracé : la déclaration morte est retirée.
    let outline: Vec<PieceEdge> =
        sc.p.piece(sc.piece)
            .unwrap()
            .outline
            .iter()
            .copied()
            .filter(|e| e.entity != left)
            .collect();
    sc.p.apply(Command::SetPieceOutline {
        piece: sc.piece,
        outline,
        grainline: None,
    })
    .unwrap();
    let out =
        sc.p.apply(Command::Delete {
            sheet: sc.s,
            points: vec![],
            entities: vec![left],
        })
        .unwrap();
    assert!(sc.p.piece(sc.piece).unwrap().on_fold.is_empty());
    assert!(
        out.notes.iter().any(|n| n.contains("sur le pli")),
        "{:?}",
        out.notes
    );
}

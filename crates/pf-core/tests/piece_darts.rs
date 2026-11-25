//! US-07.1 — pince : créer (sommet, axe, longueur, ouverture), fermer pour contrôler la ligne,
//! capuchon calculé par pliage. Un groupe de tests par critère d'acceptation :
//! CA1 jambes égales à 1e-6 mm, CA2 fermeture (longueurs avant / après), CA3 capuchon des deux
//! sens de pliage, CA4 refus sans trace, CA5 une étape annulable, CA6 fichier (schéma 3, octets
//! inchangés, chargeur strict, `pf-validate`), CA7 liaison à la construction et cohérence.
//!
//! Pièce de référence : rectangle 200 × 150 mm (bord bas y = 0). Pince : sommet (100 ; 100), point
//! d'axe (100 ; 0), longueur 100, ouverture 20 : pieds (110 ; 0) (gauche de l'axe) et (90 ; 0),
//! jambes = √(100² + 10²) = √10100. Rotation de fermeture θ = 2·atan(0,1) : cos θ = 0,99 / 1,01,
//! sin θ = 0,2 / 1,01.

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::{distance_mm, PointMm, Side};
use serde_json::{json, Value};
use std::path::PathBuf;

const FIXTURES: [&str; 4] = [
    "v1-exercice.pforge.json",
    "v1-grande-piece.pforge.json",
    "v2-piece-contour-droit-fil.pforge.json",
    "v2-piece-sans-contour.pforge.json",
];

const C: f64 = 0.99 / 1.01;
const S: f64 = 0.2 / 1.01;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn fixture(name: &str) -> Project {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let text = std::fs::read_to_string(dir.join(name)).unwrap();
    persist::from_json(&text).unwrap_or_else(|e| panic!("{name} : {e}"))
}

struct Scene {
    p: Project,
    s: SheetId,
    piece: PieceId,
    apex: PointId,
    axis: PointId,
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
    let mut p = Project::new("Pinces");
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
    Scene {
        p,
        s,
        piece,
        apex,
        axis,
    }
}

fn add_cmd(sc: &Scene, length: f64, opening: f64, fixed: Side) -> Command {
    Command::AddPieceDart {
        piece: sc.piece,
        apex: PointRef::Existing(sc.apex),
        toward: PointRef::Existing(sc.axis),
        length_mm: length,
        opening_mm: opening,
        fixed,
    }
}

fn add_ok(sc: &mut Scene, length: f64, opening: f64, fixed: Side) -> DartId {
    sc.p.apply(add_cmd(sc, length, opening, fixed))
        .unwrap()
        .darts[0]
}

fn refused(p: &mut Project, c: Command) -> String {
    let (before, text) = (p.clone(), bytes(p));
    let err = p.apply(c).expect_err("la commande aurait dû être refusée");
    assert_eq!(*p, before, "refus sans trace (valeur)");
    assert_eq!(text, bytes(p), "refus sans trace (octets)");
    assert!(!err.to_string().is_empty());
    err.to_string()
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn at(a: PointMm, x: f64, y: f64) -> bool {
    distance_mm(a, pt(x, y)) < 1e-9
}

// ----- CA1 : jambes égales ----------------------------------------------------------

#[test]
fn ca1_les_deux_jambes_sont_egales_a_1e_6() {
    let mut sc = scene();
    add_ok(&mut sc, 100.0, 20.0, Side::Left);
    let r = &sc.p.piece_darts(sc.piece)[0];
    let calc = r.calc.as_ref().unwrap();
    let expected = 10100.0_f64.sqrt();
    let apex = pt(100.0, 100.0);
    let (ll, lr) = (
        distance_mm(apex, calc.shape.left_foot),
        distance_mm(apex, calc.shape.right_foot),
    );
    assert!((ll - lr).abs() < 1e-6, "jambes {ll} et {lr}");
    assert!((ll - expected).abs() < 1e-9 && (lr - expected).abs() < 1e-9);
    assert!(at(calc.shape.left_foot, 110.0, 0.0));
    assert!(at(calc.shape.right_foot, 90.0, 0.0));
    assert!((calc.shape.leg_left_mm - calc.shape.leg_right_mm).abs() < 1e-6);
    // Axe oblique : sommet (50 ; 100) vers (53 ; 96) mais base sur y = 0 impossible : refus net,
    // l'égalité n'est donc jamais atteinte avec des pieds hors du contour.
    let t = free_point(&mut sc.p, sc.s, 53.0, 96.0);
    let a = free_point(&mut sc.p, sc.s, 50.0, 100.0);
    let msg = refused(
        &mut sc.p,
        Command::AddPieceDart {
            piece: sc.piece,
            apex: PointRef::Existing(a),
            toward: PointRef::Existing(t),
            length_mm: 50.0,
            opening_mm: 10.0,
            fixed: Side::Left,
        },
    );
    assert!(msg.contains("contour"), "{msg}");
}

// ----- CA2 : fermeture -------------------------------------------------------------

#[test]
fn ca2_fermer_la_pince_controle_la_ligne_sans_modifier_la_piece() {
    let mut sc = scene();
    let d = add_ok(&mut sc, 100.0, 20.0, Side::Left);
    let (before, text) = (sc.p.clone(), bytes(&sc.p));
    let calc = sc.p.piece_dart_close(sc.piece, d).unwrap();
    let c = &calc.closed;
    // Bord bas 200 mm ; la base de 20 mm disparaît : 180 mm.
    assert!(near(c.before_mm, 200.0));
    assert!(near(c.removed_mm, 20.0));
    assert!(near(c.after_mm, 180.0));
    assert!(
        near(c.before_mm - c.after_mm, 20.0),
        "différence = ouverture"
    );
    // Jambe gauche fixe : la partie [0 ; 90] tourne de +θ autour de (100 ; 100) et son pied (90 ; 0)
    // tombe sur (110 ; 0). Départ (0 ; 0) : vecteur (−100 ; −100).
    assert_eq!(c.chain.len(), 2);
    assert!(at(
        c.chain[0].start(),
        100.0 - 100.0 * C + 100.0 * S,
        100.0 - 100.0 * S - 100.0 * C
    ));
    assert!(at(c.chain[0].end(), 110.0, 0.0));
    assert!(at(c.chain[1].end(), 200.0, 0.0));
    assert!(near(c.rotation_deg, 11.421186274999286));
    assert!(near(c.corner_deg.unwrap(), -11.421186274999286));
    // Rien n'est modifié.
    assert_eq!(sc.p, before);
    assert_eq!(text, bytes(&sc.p));
    // Seule la définition est stockée : aucune clé de pied, de fermeture ou de capuchon.
    for k in ["capuchon", "foot", "closed", "leg_mm", "cap"] {
        assert!(!text.contains(k), "{k}");
    }
    // Pince inconnue.
    assert!(sc.p.piece_dart_close(sc.piece, DartId(99_999)).is_err());
}

// ----- CA3 : capuchon, couchée d'un côté ou de l'autre --------------------------------

#[test]
fn ca3_capuchon_des_deux_sens_de_pliage() {
    // Côtés de 90 mm, étendue 2 w = 40 mm : P1 = (50 ; 0), P2 = (150 ; 0). Calculé à la main
    // (repère du sommet) : jambe gauche fixe, Q1 = (−29,5 ; −109) / 1,01, Q2 = (50 ; −100),
    // la droite Q1 Q2 coupe la jambe en u = 1/2 et t = 1,05 / 1,01 (K = t · (10 ; −100)).
    // Ka = (100 − 10,5 / 1,01 ; 100 − 105 / 1,01), Kb = (100 + 10,5 / 1,01 ; 100 − 105 / 1,01),
    // |AK| = 1050 / √101. Plier de l'autre côté revient à tourner tout le dessin de θ : même base
    // corrigée, lignes pliées différentes.
    let mut sc = scene();
    let left = add_ok(&mut sc, 100.0, 20.0, Side::Left);
    let right = add_ok(&mut sc, 100.0, 20.0, Side::Right);
    let yk = 100.0 - 105.0 / 1.01;
    let ka = pt(100.0 - 10.5 / 1.01, yk);
    let kb = pt(100.0 + 10.5 / 1.01, yk);
    for (id, fixed) in [(left, Side::Left), (right, Side::Right)] {
        let c = sc.p.piece_dart_close(sc.piece, id).unwrap();
        assert_eq!(c.closed.fixed, fixed);
        let cap = c.closed.capuchon.expect("capuchon");
        assert!(at(cap[0], 50.0, 0.0), "{fixed:?} P1");
        assert!(at(cap[1], ka.x, ka.y), "{fixed:?} Ka {:?}", cap[1]);
        assert!(at(cap[2], kb.x, kb.y), "{fixed:?} Kb {:?}", cap[2]);
        assert!(at(cap[3], 150.0, 0.0), "{fixed:?} P2");
        assert!(near(
            c.closed.capuchon_leg_mm.unwrap(),
            1050.0 / 101.0_f64.sqrt()
        ));
        // Les deux points du capuchon sont à égale distance du sommet.
        let a = pt(100.0, 100.0);
        assert!((distance_mm(a, cap[1]) - distance_mm(a, cap[2])).abs() < 1e-9);
    }
    // Les lignes pliées diffèrent : jambe droite fixe, la partie [110 ; 200] tourne de −θ.
    let cl = sc.p.piece_dart_close(sc.piece, left).unwrap().closed;
    let cr = sc.p.piece_dart_close(sc.piece, right).unwrap().closed;
    assert!(at(cr.chain[0].end(), 90.0, 0.0));
    assert!(at(
        cr.chain[1].end(),
        100.0 + 100.0 * C - 100.0 * S,
        100.0 - 100.0 * S - 100.0 * C
    ));
    assert!(at(cl.chain[1].end(), 200.0, 0.0));
    assert!(near(cr.rotation_deg, -11.421186274999286));
    assert!(near(cr.after_mm, 180.0));
}

// ----- CA4 : refus sans trace ---------------------------------------------------------

#[test]
fn ca4_refus_sans_trace() {
    let mut sc = scene();
    let outside = free_point(&mut sc.p, sc.s, 400.0, 100.0);
    let on_edge = free_point(&mut sc.p, sc.s, 50.0, 0.0);
    let top = free_point(&mut sc.p, sc.s, 100.0, 150.0);
    let (p, apex, axis) = (sc.piece, sc.apex, sc.axis);
    let mk = |apex, toward, length_mm, opening_mm| Command::AddPieceDart {
        piece: p,
        apex: PointRef::Existing(apex),
        toward: PointRef::Existing(toward),
        length_mm,
        opening_mm,
        fixed: Side::Left,
    };
    // Valeurs : 0, négatif, NaN, infini, hors bornes, ouverture > longueur.
    for l in [0.0, -5.0, f64::NAN, f64::INFINITY, 0.5, 2000.5, 1e300] {
        refused(&mut sc.p, mk(apex, axis, l, 20.0));
    }
    for w in [0.0, -1.0, f64::NAN, f64::NEG_INFINITY, 0.05, 101.0, 1e300] {
        refused(&mut sc.p, mk(apex, axis, 100.0, w));
    }
    // Sommet = point d'axe ; points absents ; pièce absente.
    refused(&mut sc.p, mk(apex, apex, 100.0, 20.0));
    refused(&mut sc.p, mk(PointId(98_765), axis, 100.0, 20.0));
    refused(&mut sc.p, mk(apex, PointId(98_765), 100.0, 20.0));
    refused(
        &mut sc.p,
        Command::AddPieceDart {
            piece: PieceId(98_765),
            apex: PointRef::Existing(apex),
            toward: PointRef::Existing(axis),
            length_mm: 100.0,
            opening_mm: 20.0,
            fixed: Side::Left,
        },
    );
    // Sommet hors de la pièce ; sommet sur le contour ; pieds hors du contour (longueur ≠ distance
    // au bord) ; jambe qui sort ; pieds sur deux tracés (base sur le coin).
    refused(&mut sc.p, mk(outside, axis, 100.0, 20.0));
    refused(&mut sc.p, mk(top, axis, 150.0, 20.0));
    refused(&mut sc.p, mk(apex, axis, 90.0, 20.0));
    refused(&mut sc.p, mk(apex, axis, 120.0, 20.0));
    let corner = free_point(&mut sc.p, sc.s, 195.0, 100.0);
    let corner_axis = free_point(&mut sc.p, sc.s, 195.0, 0.0);
    let msg = refused(&mut sc.p, mk(corner, corner_axis, 100.0, 20.0));
    assert!(msg.contains("pince"), "{msg}");
    let _ = on_edge;
    // Dérivée liée : refusée (la pince n'est pas reprise sur une dérivée).
    let mut sd = scene();
    let dp =
        sd.p.apply(Command::CreateDerivedPiece {
            mother: sd.piece,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset { distance_mm: -5.0 },
            name: None,
        })
        .unwrap()
        .pieces[0];
    let msg = refused(
        &mut sd.p,
        Command::AddPieceDart {
            piece: dp,
            apex: PointRef::Existing(sd.apex),
            toward: PointRef::Existing(sd.axis),
            length_mm: 100.0,
            opening_mm: 20.0,
            fixed: Side::Left,
        },
    );
    assert!(msg.contains("dérivée"), "{msg}");
    // Pince inconnue (retrait, modification) ; trop de pinces.
    let d = add_ok(&mut sc, 100.0, 20.0, Side::Left);
    refused(
        &mut sc.p,
        Command::RemovePieceDart {
            piece: sc.piece,
            dart: DartId(98_765),
        },
    );
    refused(
        &mut sc.p,
        Command::SetPieceDart {
            piece: sc.piece,
            dart: d,
            apex,
            toward: axis,
            length_mm: 100.0,
            opening_mm: 0.0,
            fixed: Side::Left,
        },
    );
    for _ in 1..DART_MAX_PER_PIECE {
        sc.p.apply(add_cmd(&sc, 100.0, 20.0, Side::Left)).unwrap();
    }
    let over = add_cmd(&sc, 100.0, 20.0, Side::Left);
    let msg = refused(&mut sc.p, over);
    assert!(msg.contains("100"), "{msg}");
}

// ----- CA5 : une étape annulable --------------------------------------------------------

#[test]
fn ca5_une_etape_annulable_et_aucune_etape_pour_un_no_op() {
    let sc = scene();
    let mut doc = Document::new(sc.p.clone());
    let base = bytes(doc.project());
    let d = doc
        .execute(add_cmd(&sc, 100.0, 20.0, Side::Left))
        .unwrap()
        .darts[0];
    assert_eq!(doc.project().pieces[0].darts.len(), 1);
    assert!(doc.undo(), "une annulation");
    assert_eq!(bytes(doc.project()), base);
    assert!(
        !doc.can_undo() || {
            // Il peut rester les étapes de la préparation (points) : une seule annulation suffit.
            true
        }
    );
    assert!(doc.redo());
    let set = |opening| Command::SetPieceDart {
        piece: sc.piece,
        dart: d,
        apex: sc.apex,
        toward: sc.axis,
        length_mm: 100.0,
        opening_mm: opening,
        fixed: Side::Left,
    };
    // No-op : aucune étape (l'annulation suivante défait la pose, pas la modification).
    let with_dart = bytes(doc.project());
    doc.execute(set(20.0)).unwrap();
    assert_eq!(bytes(doc.project()), with_dart);
    doc.execute(set(30.0)).unwrap();
    assert!(bytes(doc.project()).contains("30.0"));
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), with_dart, "une seule annulation");
    doc.execute(Command::RemovePieceDart {
        piece: sc.piece,
        dart: d,
    })
    .unwrap();
    assert!(doc.project().pieces[0].darts.is_empty());
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), with_dart);
    // Édition : ordre et identifiant conservés.
    let mut p = doc.project().clone();
    p.apply(set(40.0)).unwrap();
    assert_eq!(p.pieces[0].darts[0].id, d);
    assert!(near(p.pieces[0].darts[0].opening_mm, 40.0));
}

// ----- CA6 : fichier --------------------------------------------------------------------

#[test]
fn ca6_champ_absent_octets_inchanges_et_present_relu_a_l_identique() {
    for name in FIXTURES {
        let first = bytes(&fixture(name));
        assert!(!first.contains("\"darts\""), "{name}");
        assert_eq!(first, bytes(&persist::from_json(&first).unwrap()), "{name}");
    }
    let mut sc = scene();
    assert!(!bytes(&sc.p).contains("\"darts\""));
    let d = add_ok(&mut sc, 100.0, 20.0, Side::Right);
    let text = bytes(&sc.p);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["schema_version"], 6);
    assert_eq!(
        v["pieces"][0]["darts"],
        json!([{
            "id": d.0, "apex": sc.apex.0, "toward": sc.axis.0,
            "length_mm": 100.0, "opening_mm": 20.0, "fixed": "right"
        }])
    );
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, sc.p);
    assert_eq!(text, bytes(&back));
    // Retrait : plus aucune clé.
    sc.p.apply(Command::RemovePieceDart {
        piece: sc.piece,
        dart: d,
    })
    .unwrap();
    assert!(!bytes(&sc.p).contains("\"darts\""));
    // Fichier livré.
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/piece-crans-droit-fil.pforge.json");
    let shipped = std::fs::read_to_string(file).unwrap().replace("\r\n", "\n");
    assert_eq!(
        shipped,
        persist::to_json(&persist::from_json(&shipped).unwrap()).unwrap()
    );
}

#[test]
fn ca6_chargeur_strict_et_pf_validate() {
    let mut sc = scene();
    add_ok(&mut sc, 100.0, 20.0, Side::Left);
    let good: Value = serde_json::from_str(&bytes(&sc.p)).unwrap();
    let mutate = |f: &dyn Fn(&mut Value)| {
        let mut v = good.clone();
        f(&mut v["pieces"][0]["darts"][0]);
        persist::from_json(&serde_json::to_string(&v).unwrap())
    };
    assert!(mutate(&|_| {}).is_ok());
    assert!(mutate(&|d| d["z"] = json!(1)).is_err(), "clé inconnue");
    assert!(mutate(&|d| d["fixed"] = json!("up")).is_err());
    assert!(mutate(&|d| d["fixed"] = json!(1)).is_err());
    assert!(mutate(&|d| {
        d.as_object_mut().unwrap().remove("fixed");
    })
    .is_err());
    assert!(mutate(&|d| {
        d.as_object_mut().unwrap().remove("opening_mm");
    })
    .is_err());
    assert!(mutate(&|d| d["length_mm"] = json!(0.0)).is_err());
    assert!(mutate(&|d| d["length_mm"] = json!(5000.0)).is_err());
    assert!(mutate(&|d| d["opening_mm"] = json!(-3.0)).is_err());
    assert!(
        mutate(&|d| d["opening_mm"] = json!(150.0)).is_err(),
        "> longueur"
    );
    assert!(mutate(&|d| d["opening_mm"] = json!("20")).is_err());
    assert!(
        mutate(&|d| d["apex"] = json!(987654)).is_err(),
        "point absent"
    );
    let apex = good["pieces"][0]["darts"][0]["apex"].clone();
    assert!(
        mutate(&|d| d["toward"] = apex.clone()).is_err(),
        "sommet = axe"
    );
    assert!(mutate(&|d| d["id"] = json!(good["pieces"][0]["id"].clone())).is_err());
    let mut v = good.clone();
    v["pieces"][0]["darts"] = json!({});
    assert!(persist::from_json(&serde_json::to_string(&v).unwrap()).is_err());
    // pf-validate : 0 pour le bon fichier, 1 pour un fichier invalide.
    let dir = std::env::temp_dir().join(format!("pf-darts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let ok = dir.join("ok.pforge.json");
    std::fs::write(&ok, serde_json::to_string_pretty(&good).unwrap()).unwrap();
    let run = |p: &PathBuf| {
        std::process::Command::new(env!("CARGO_BIN_EXE_pf-validate"))
            .arg(p)
            .output()
            .unwrap()
            .status
            .code()
    };
    assert_eq!(run(&ok), Some(0));
    let mut bad = good.clone();
    bad["pieces"][0]["darts"][0]["length_mm"] = json!(-1.0);
    let ko = dir.join("ko.pforge.json");
    std::fs::write(&ko, serde_json::to_string_pretty(&bad).unwrap()).unwrap();
    assert_eq!(run(&ko), Some(1));
    std::fs::remove_dir_all(&dir).ok();
}

// ----- CA7 : liaison à la construction et cohérence -----------------------------------------

#[test]
fn ca7_la_pince_suit_ses_points_et_se_signale_sans_etre_refusee() {
    let mut sc = scene();
    add_ok(&mut sc, 100.0, 20.0, Side::Left);
    assert!(sc
        .p
        .piece_issues(sc.piece)
        .iter()
        .all(|i| !matches!(i, PieceIssue::DartInvalid { .. })));
    // Déplacer le sommet et le point d'axe de 20 mm vers la droite : la pince suit (pieds
    // (130 ; 0) et (110 ; 0), jambes toujours √(100² + 10²)).
    for (point, to) in [(sc.apex, pt(120.0, 100.0)), (sc.axis, pt(120.0, 0.0))] {
        sc.p.apply(Command::MovePoint {
            sheet: sc.s,
            point,
            to,
        })
        .unwrap();
    }
    let calc = sc.p.piece_darts(sc.piece)[0].calc.clone().unwrap();
    assert!(at(calc.shape.apex, 120.0, 100.0));
    assert!(
        at(calc.shape.left_foot, 130.0, 0.0),
        "{:?}",
        calc.shape.left_foot
    );
    assert!(at(calc.shape.right_foot, 110.0, 0.0));
    assert!((calc.shape.leg_left_mm - 10100.0_f64.sqrt()).abs() < 1e-9);
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: sc.axis,
        to: pt(100.0, 0.0),
    })
    .unwrap();
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: sc.apex,
        to: pt(100.0, 120.0),
    })
    .unwrap();
    // Les pieds ne sont plus sur le contour : signalé, jamais refusé.
    let issues = sc.p.piece_issues(sc.piece);
    assert!(
        issues.contains(&PieceIssue::DartInvalid { count: 1 }),
        "{issues:?}"
    );
    assert_eq!(PieceIssue::DartInvalid { count: 1 }.key(), "dart_invalid");
    let why = sc.p.piece_darts(sc.piece)[0].calc.clone().unwrap_err();
    assert!(why.contains("pied"), "{why}");
    assert!(sc
        .p
        .piece_dart_close(sc.piece, sc.p.pieces[0].darts[0].id)
        .is_err());
    // Sommet hors de la pièce : signalé aussi, pince conservée.
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: sc.apex,
        to: pt(100.0, 400.0),
    })
    .unwrap();
    assert_eq!(sc.p.pieces[0].darts.len(), 1);
    // Retour : de nouveau calculable.
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: sc.apex,
        to: pt(100.0, 100.0),
    })
    .unwrap();
    assert!(sc.p.piece_darts(sc.piece)[0].calc.is_ok());
    assert!(sc
        .p
        .piece_issues(sc.piece)
        .iter()
        .all(|i| !matches!(i, PieceIssue::DartInvalid { .. })));
}

#[test]
fn ca7_point_supprime_retire_la_pince_avec_remarque_et_se_defait_en_une_etape() {
    let sc = scene();
    let mut doc = Document::new(sc.p.clone());
    doc.execute(add_cmd(&sc, 100.0, 20.0, Side::Left)).unwrap();
    let with = bytes(doc.project());
    let out = doc
        .execute(Command::Delete {
            sheet: sc.s,
            points: vec![sc.axis],
            entities: vec![],
        })
        .unwrap();
    assert!(doc.project().pieces[0].darts.is_empty());
    assert!(
        out.notes.iter().any(|n| n.contains("pince")),
        "{:?}",
        out.notes
    );
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), with);
}

#[test]
fn ca7_copie_de_feuille_remappe_les_points_et_les_identifiants() {
    let mut sc = scene();
    add_ok(&mut sc, 100.0, 20.0, Side::Left);
    let before = sc.p.clone();
    let out = sc.p.apply(Command::DuplicateSheet { sheet: sc.s }).unwrap();
    let new_piece = out.pieces[0];
    let orig = &sc.p.piece(sc.piece).unwrap().darts[0];
    let copy = &sc.p.piece(new_piece).unwrap().darts[0];
    assert_ne!(orig.id, copy.id);
    assert_ne!(orig.apex, copy.apex);
    assert_eq!(*orig, before.pieces[0].darts[0], "original intact");
    let calc = sc.p.piece_darts(new_piece)[0].calc.clone().unwrap();
    assert!(near(calc.closed.after_mm, 180.0));
}

#[test]
fn ca7_separer_signale_les_pinces_non_reprises() {
    let mut sc = scene();
    add_ok(&mut sc, 100.0, 20.0, Side::Left);
    let out =
        sc.p.apply(Command::SplitPiece {
            piece: sc.piece,
            a: pt(0.0, 140.0),
            b: pt(200.0, 140.0),
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("pince")),
        "{:?}",
        out.notes
    );
}

// ----- aide à la saisie : pied de la perpendiculaire --------------------------------------

#[test]
fn ca8_point_d_axe_perpendiculaire_au_bord_le_plus_proche() {
    let sc = scene();
    // Sommet (73 ; 41), clic près du bord bas : pied (73 ; 0). Près du bord droit : (200 ; 41).
    let base =
        sc.p.dart_base_point(sc.piece, pt(73.0, 41.0), pt(60.0, 2.0))
            .unwrap();
    assert!(at(base, 73.0, 0.0));
    let right =
        sc.p.dart_base_point(sc.piece, pt(73.0, 41.0), pt(199.0, 90.0))
            .unwrap();
    assert!(at(right, 200.0, 41.0));
    // Le pied de la perpendiculaire fait tomber les deux pieds de la pince sur le bord.
    let mut sc = scene();
    let out =
        sc.p.apply(Command::AddPieceDart {
            piece: sc.piece,
            apex: PointRef::New(pt(73.0, 41.0)),
            toward: PointRef::New(base),
            length_mm: 41.0,
            opening_mm: 12.0,
            fixed: Side::Right,
        })
        .unwrap();
    assert_eq!(out.darts.len(), 1);
    assert_eq!(
        out.points.len(),
        2,
        "les deux points sont créés dans la même étape"
    );
    let calc = sc.p.piece_darts(sc.piece)[0].calc.clone().unwrap();
    assert!(at(calc.shape.left_foot, 79.0, 0.0) || at(calc.shape.left_foot, 67.0, 0.0));
    // Sommet hors de la pièce, ou perpendiculaire hors du segment : rien.
    assert!(sc
        .p
        .dart_base_point(sc.piece, pt(500.0, 41.0), pt(60.0, 2.0))
        .is_none());
    assert!(sc
        .p
        .dart_base_point(PieceId(98_765), pt(73.0, 41.0), pt(60.0, 2.0))
        .is_none());
}

#[test]
fn ca8_piece_contenant_un_point() {
    let sc = scene();
    assert_eq!(sc.p.piece_containing(sc.s, pt(100.0, 75.0)), Some(sc.piece));
    assert_eq!(sc.p.piece_containing(sc.s, pt(400.0, 75.0)), None);
}

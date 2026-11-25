//! US-06.3 — perçages et pointages : poinçon, cercle de diamètre donné, croix, et bout
//! de pince reculé d'une valeur. Un groupe de tests par critère d'acceptation :
//! CA1 modèle et fichier (schéma 3, octets inchangés), CA2 création exacte des trois
//! formes, CA3 bout de pince reculé calculé à la main, CA4 liaison à la construction,
//! CA5 refus sans trace, CA6 une étape annulable, CA7 avertissement hors contour,
//! CA8 cohérence (copie de feuille, séparer, assembler, déplier, plan, dérivées,
//! pièce détachée). L'export (SVG, PDF) est mesuré dans `crates/pf-export/tests/marquages.rs`.

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::piece_marking::{
    MARKING_MAX_DIAMETER_MM, MARKING_MAX_PER_PIECE, MARKING_MAX_SETBACK_MM, MARKING_MIN_CROSS_MM,
    MARKING_MIN_DIAMETER_MM, MARKING_MIN_SETBACK_MM,
};
use pf_core::*;
use pf_geometry::{distance_mm, PointMm};
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

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn at_near(a: PointMm, b: PointMm) -> bool {
    distance_mm(a, b) < 1e-9
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn fixture(name: &str) -> Project {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let text = std::fs::read_to_string(dir.join(name)).unwrap();
    persist::from_json(&text).unwrap_or_else(|e| panic!("{name} : {e}"))
}

/// Rectangle de pièce 100 × 60 mm (pièce liée « Devant ») et quatre points libres :
/// `inside` (50 ; 30) dans le contour, `outside` (150 ; 30) hors du contour, `tip`
/// (10 ; 10) et `toward` (40 ; 50), distants de 50 mm (vecteur 30 ; 40 : triangle 3-4-5).
struct Scene {
    p: Project,
    s: SheetId,
    piece: PieceId,
    inside: PointId,
    outside: PointId,
    tip: PointId,
    toward: PointId,
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
    let mut p = Project::new("Marquages");
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
    let inside = free_point(&mut p, s, 50.0, 30.0);
    let outside = free_point(&mut p, s, 150.0, 30.0);
    let tip = free_point(&mut p, s, 10.0, 10.0);
    let toward = free_point(&mut p, s, 40.0, 50.0);
    Scene {
        p,
        s,
        piece,
        inside,
        outside,
        tip,
        toward,
    }
}

fn on(point: PointId) -> MarkingAnchor {
    MarkingAnchor::Point { point }
}

fn dart(tip: PointId, toward: PointId, setback_mm: f64) -> MarkingAnchor {
    MarkingAnchor::DartTip {
        tip,
        toward,
        setback_mm,
    }
}

fn drill(d: f64) -> MarkingShape {
    MarkingShape::Drill { diameter_mm: d }
}

fn cross(s: f64) -> MarkingShape {
    MarkingShape::Cross { size_mm: s }
}

fn add(piece: PieceId, at: MarkingAnchor, shape: MarkingShape) -> Command {
    Command::AddPieceMarking { piece, at, shape }
}

fn add_ok(p: &mut Project, piece: PieceId, at: MarkingAnchor, shape: MarkingShape) -> MarkingId {
    p.apply(add(piece, at, shape)).unwrap().markings[0]
}

/// Refus : message d'erreur ; le projet est identique (valeur et octets).
fn refused(p: &mut Project, c: Command) -> String {
    let (before, text) = (p.clone(), bytes(p));
    let err = p.apply(c).expect_err("la commande aurait dû être refusée");
    assert_eq!(*p, before, "refus sans trace (valeur)");
    assert_eq!(text, bytes(p), "refus sans trace (octets)");
    assert!(!err.to_string().is_empty());
    err.to_string()
}

fn positions(p: &Project, piece: PieceId) -> Vec<PointMm> {
    p.piece_markings(piece).iter().map(|r| r.at).collect()
}

// ----- CA1 : modèle et fichier ----------------------------------------------------

#[test]
fn ca1_champ_absent_octets_inchanges() {
    for name in FIXTURES {
        let first = bytes(&fixture(name));
        let second = bytes(&persist::from_json(&first).unwrap());
        assert_eq!(first, second, "{name}");
        assert!(!first.contains("\"markings\""), "{name}");
        assert!(fixture(name).pieces.iter().all(|p| p.markings.is_empty()));
    }
    let sc = scene();
    let text = bytes(&sc.p);
    assert!(!text.contains("\"markings\""));
    assert_eq!(text, bytes(&persist::from_json(&text).unwrap()));
    // Fichier livré : relu puis réécrit, identique à l'octet.
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples/piece-crans-droit-fil.pforge.json");
    let shipped = std::fs::read_to_string(file).unwrap().replace("\r\n", "\n");
    assert_eq!(
        shipped,
        persist::to_json(&persist::from_json(&shipped).unwrap()).unwrap()
    );
    // Retirer le dernier marquage : plus aucune clé « markings » dans la pièce.
    let mut sc = scene();
    let m = add_ok(&mut sc.p, sc.piece, on(sc.inside), MarkingShape::Punch);
    assert!(bytes(&sc.p).contains("\"markings\""));
    sc.p.apply(Command::RemovePieceMarking {
        piece: sc.piece,
        marking: m,
    })
    .unwrap();
    assert!(!bytes(&sc.p).contains("\"markings\""));
}

#[test]
fn ca1_champ_present_relu_a_l_identique_schema_3() {
    let mut sc = scene();
    let a = add_ok(&mut sc.p, sc.piece, on(sc.inside), drill(6.0));
    let b = add_ok(
        &mut sc.p,
        sc.piece,
        dart(sc.tip, sc.toward, 12.5),
        cross(8.0),
    );
    let c = add_ok(&mut sc.p, sc.piece, on(sc.inside), MarkingShape::Punch);
    let text = bytes(&sc.p);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["schema_version"], 6, "schéma 4 (US-09.1)");
    let list = v["pieces"][0]["markings"].as_array().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(
        list[0],
        json!({"id": a.0, "at": {"type": "point", "point": sc.inside.0},
               "shape": {"type": "drill", "diameter_mm": 6.0}})
    );
    assert_eq!(
        list[1],
        json!({"id": b.0,
               "at": {"type": "dart_tip", "tip": sc.tip.0, "toward": sc.toward.0, "setback_mm": 12.5},
               "shape": {"type": "cross", "size_mm": 8.0}})
    );
    assert_eq!(
        list[2],
        json!({"id": c.0, "at": {"type": "point", "point": sc.inside.0},
               "shape": {"type": "punch"}})
    );
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, sc.p);
    assert_eq!(bytes(&back), text);
    assert!(
        back.pieces[0].extra.is_empty(),
        "champ typé, pas une clé inconnue"
    );
    // Formes stables.
    assert_eq!(
        [MarkingShape::Punch, drill(1.0), cross(1.0)].map(|s| s.key()),
        ["punch", "drill", "cross"]
    );
}

#[test]
fn ca1_fichier_invalide_refuse_a_l_ouverture_sans_perte_silencieuse() {
    let mut sc = scene();
    add_ok(&mut sc.p, sc.piece, on(sc.inside), drill(6.0));
    add_ok(
        &mut sc.p,
        sc.piece,
        dart(sc.tip, sc.toward, 5.0),
        cross(8.0),
    );
    let good: Value = serde_json::from_str(&bytes(&sc.p)).unwrap();
    let mutate = |f: &dyn Fn(&mut Value)| -> Result<Project, CoreError> {
        let mut v = good.clone();
        f(&mut v);
        persist::from_json(&v.to_string())
    };
    // Témoin : le fichier tel quel s'ouvre.
    assert!(mutate(&|_| {}).is_ok());
    fn m0(v: &mut Value) -> &mut Value {
        &mut v["pieces"][0]["markings"][0]
    }
    fn m1(v: &mut Value) -> &mut Value {
        &mut v["pieces"][0]["markings"][1]
    }
    type Mutation = Box<dyn Fn(&mut Value)>;
    let cases: Vec<(&str, Mutation)> = vec![
        (
            "diamètre nul",
            Box::new(move |v| m0(v)["shape"]["diameter_mm"] = json!(0.0)),
        ),
        (
            "diamètre négatif",
            Box::new(move |v| m0(v)["shape"]["diameter_mm"] = json!(-3.0)),
        ),
        (
            "diamètre trop grand",
            Box::new(move |v| m0(v)["shape"]["diameter_mm"] = json!(1e9)),
        ),
        (
            "diamètre texte",
            Box::new(move |v| m0(v)["shape"]["diameter_mm"] = json!("6")),
        ),
        (
            "point absent",
            Box::new(move |v| m0(v)["at"]["point"] = json!(987_654)),
        ),
        (
            "forme inconnue",
            Box::new(move |v| m0(v)["shape"]["type"] = json!("triangle")),
        ),
        (
            "ancrage inconnu",
            Box::new(move |v| m0(v)["at"]["type"] = json!("milieu")),
        ),
        (
            "clé inconnue",
            Box::new(move |v| m0(v)["couleur"] = json!("rouge")),
        ),
        (
            "identifiant en double",
            Box::new(move |v| {
                let id = m0(v)["id"].clone();
                m1(v)["id"] = id;
            }),
        ),
        (
            "identifiant au-delà du compteur",
            Box::new(move |v| m1(v)["id"] = json!(99_999_999)),
        ),
        (
            "recul nul",
            Box::new(move |v| m1(v)["at"]["setback_mm"] = json!(0.0)),
        ),
        (
            "recul trop grand",
            Box::new(move |v| m1(v)["at"]["setback_mm"] = json!(1e6)),
        ),
        (
            "pointe = direction",
            Box::new(move |v| {
                let tip = m1(v)["at"]["tip"].clone();
                m1(v)["at"]["toward"] = tip;
            }),
        ),
        (
            "côté de croix nul",
            Box::new(move |v| m1(v)["shape"]["size_mm"] = json!(0.0)),
        ),
        (
            "marquages pas une liste",
            Box::new(|v| v["pieces"][0]["markings"] = json!(3)),
        ),
    ];
    for (what, f) in cases {
        let err = mutate(&*f);
        assert!(err.is_err(), "{what} : le fichier aurait dû être refusé");
        assert!(!err.unwrap_err().to_string().is_empty(), "{what}");
    }
}

// ----- CA2 : création exacte des trois formes -------------------------------------

#[test]
fn ca2_pointage_percage_croix_crees_a_la_position_exacte() {
    let mut sc = scene();
    let punch =
        sc.p.apply(add(sc.piece, on(sc.inside), MarkingShape::Punch))
            .unwrap();
    let hole =
        sc.p.apply(add(sc.piece, on(sc.inside), drill(6.0)))
            .unwrap();
    let plus = sc.p.apply(add(sc.piece, on(sc.tip), cross(12.0))).unwrap();
    // Un identifiant par marquage, distincts, tirés du compteur du projet.
    let ids = [punch.markings[0], hole.markings[0], plus.markings[0]];
    assert_eq!(
        ids.iter().collect::<std::collections::HashSet<_>>().len(),
        3
    );
    assert!(ids.iter().all(|m| m.0 < sc.p.next_id));
    sc.p.validate().unwrap();
    let list = sc.p.piece_markings(sc.piece);
    assert_eq!(list.len(), 3);
    assert_eq!(list[0].marking.shape, MarkingShape::Punch);
    assert_eq!(list[1].marking.shape, drill(6.0));
    assert_eq!(list[2].marking.shape, cross(12.0));
    assert!(at_near(list[0].at, pt(50.0, 30.0)));
    assert!(at_near(list[1].at, pt(50.0, 30.0)));
    assert!(at_near(list[2].at, pt(10.0, 10.0)));
    // Diamètre et côté exacts, tels que saisis (jamais arrondis).
    assert_eq!(list[1].marking.shape.size_mm(), Some(6.0));
    assert_eq!(list[2].marking.shape.size_mm(), Some(12.0));
    assert_eq!(MarkingShape::Punch.size_mm(), None);
    assert!(list.iter().all(|r| r.inside_outline == Some(true)));
    // La construction n'est pas touchée : aucun point, tracé ni cran en plus.
    let sheet = sc.p.sheet(sc.s).unwrap();
    assert_eq!(sheet.points.len(), 8, "4 coins + 4 points libres");
    assert_eq!(sheet.entities.len(), 4);
    // Édition : changer le diamètre garde l'identifiant et le rang.
    sc.p.apply(Command::SetPieceMarking {
        piece: sc.piece,
        marking: ids[1],
        at: on(sc.inside),
        shape: drill(2.5),
    })
    .unwrap();
    let list = sc.p.piece_markings(sc.piece);
    assert_eq!(list[1].marking.id, ids[1]);
    assert_eq!(list[1].marking.shape, drill(2.5));
    // Bornes incluses.
    for shape in [
        drill(MARKING_MIN_DIAMETER_MM),
        drill(MARKING_MAX_DIAMETER_MM),
        cross(MARKING_MIN_CROSS_MM),
    ] {
        sc.p.apply(add(sc.piece, on(sc.inside), shape)).unwrap();
    }
}

// ----- CA3 : bout de pince reculé -------------------------------------------------

#[test]
fn ca3_bout_de_pince_recule_calcule_a_la_main() {
    let mut sc = scene();
    // Pointe (10 ; 10), direction vers (40 ; 50) : vecteur (30 ; 40), longueur 50.
    // Recul 10 mm : (10 + 30·10/50 ; 10 + 40·10/50) = (16 ; 18).
    // Recul 25 mm : (10 + 15 ; 10 + 20) = (25 ; 30).
    // Recul 70 mm (au-delà du point de direction) : (10 + 42 ; 10 + 56) = (52 ; 66).
    for (setback, expect) in [
        (10.0, pt(16.0, 18.0)),
        (25.0, pt(25.0, 30.0)),
        (70.0, pt(52.0, 66.0)),
        (50.0, pt(40.0, 50.0)),
    ] {
        let m = add_ok(
            &mut sc.p,
            sc.piece,
            dart(sc.tip, sc.toward, setback),
            MarkingShape::Punch,
        );
        let got =
            sc.p.piece_markings(sc.piece)
                .into_iter()
                .find(|r| r.marking.id == m)
                .unwrap();
        assert!(
            at_near(got.at, expect),
            "recul {setback} : {:?} au lieu de {expect:?}",
            got.at
        );
        // La distance à la pointe est exactement le recul.
        assert!(near(distance_mm(got.at, pt(10.0, 10.0)), setback));
    }
    // Pointe et direction verticales : (20 ; 10) → (20 ; 50), recul 15 → (20 ; 25).
    let t2 = free_point(&mut sc.p, sc.s, 20.0, 10.0);
    let u2 = free_point(&mut sc.p, sc.s, 20.0, 50.0);
    let m = add_ok(&mut sc.p, sc.piece, dart(t2, u2, 15.0), cross(6.0));
    let got =
        sc.p.piece_markings(sc.piece)
            .into_iter()
            .find(|r| r.marking.id == m)
            .unwrap();
    assert_eq!(got.at, pt(20.0, 25.0), "valeur exacte, sans arrondi");
    // Le recul est borné ; les bornes sont incluses.
    for setback in [MARKING_MIN_SETBACK_MM, MARKING_MAX_SETBACK_MM] {
        sc.p.apply(add(
            sc.piece,
            dart(sc.tip, sc.toward, setback),
            MarkingShape::Punch,
        ))
        .unwrap();
    }
    // Aucune pince complète : la construction n'a gagné que les deux points libres.
    assert_eq!(sc.p.sheet(sc.s).unwrap().points.len(), 10);
    assert_eq!(sc.p.sheet(sc.s).unwrap().entities.len(), 4);
    // Direction dans l'autre sens : la pince va de (40 ; 50) vers (10 ; 10).
    let m = add_ok(
        &mut sc.p,
        sc.piece,
        dart(sc.toward, sc.tip, 10.0),
        MarkingShape::Punch,
    );
    let got =
        sc.p.piece_markings(sc.piece)
            .into_iter()
            .find(|r| r.marking.id == m)
            .unwrap();
    assert!(at_near(got.at, pt(40.0 - 6.0, 50.0 - 8.0)));
}

// ----- CA4 : lié à la construction ------------------------------------------------

#[test]
fn ca4_le_marquage_suit_son_point_et_la_pince_suit_ses_deux_points() {
    let mut sc = scene();
    let hole = add_ok(&mut sc.p, sc.piece, on(sc.inside), drill(6.0));
    let tipm = add_ok(
        &mut sc.p,
        sc.piece,
        dart(sc.tip, sc.toward, 10.0),
        MarkingShape::Punch,
    );
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: sc.inside,
        to: pt(60.0, 20.0),
    })
    .unwrap();
    let find = |p: &Project, id: MarkingId| {
        p.piece_markings(sc.piece)
            .into_iter()
            .find(|r| r.marking.id == id)
            .unwrap()
            .at
    };
    assert!(at_near(find(&sc.p, hole), pt(60.0, 20.0)));
    // La pointe se déplace : (20 ; 10) → direction (40 ; 50) : v = (20 ; 40), longueur √2000.
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: sc.tip,
        to: pt(20.0, 10.0),
    })
    .unwrap();
    let d = (20.0f64 * 20.0 + 40.0 * 40.0).sqrt();
    let want = pt(20.0 + 20.0 * 10.0 / d, 10.0 + 40.0 * 10.0 / d);
    assert!(at_near(find(&sc.p, tipm), want), "{:?}", find(&sc.p, tipm));
    // Le point de direction se déplace aussi.
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: sc.toward,
        to: pt(20.0, 50.0),
    })
    .unwrap();
    assert!(at_near(find(&sc.p, tipm), pt(20.0, 20.0)));
    // Confondre la pointe et le point de direction est refusé, sans trace.
    let msg = refused(
        &mut sc.p,
        Command::MovePoint {
            sheet: sc.s,
            point: sc.toward,
            to: pt(20.0, 10.0),
        },
    );
    assert!(msg.contains("confondus"), "{msg}");
    // Déplacer le point libre hors du contour : toujours valide, avertissement seulement.
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: sc.inside,
        to: pt(300.0, 300.0),
    })
    .unwrap();
    sc.p.validate().unwrap();
}

#[test]
fn ca4_supprimer_un_point_retire_le_marquage_dans_la_meme_etape() {
    let mut sc = scene();
    let keep = add_ok(&mut sc.p, sc.piece, on(sc.outside), drill(4.0));
    add_ok(&mut sc.p, sc.piece, on(sc.inside), drill(6.0));
    add_ok(
        &mut sc.p,
        sc.piece,
        dart(sc.tip, sc.toward, 10.0),
        cross(5.0),
    );
    let mut doc = Document::new(sc.p.clone());
    let o = doc
        .execute(Command::Delete {
            sheet: sc.s,
            points: vec![sc.inside, sc.toward],
            entities: vec![],
        })
        .unwrap();
    // Le perçage sur « inside » et la pince qui visait « toward » sont retirés avec remarque.
    let kept: Vec<MarkingId> = doc
        .project()
        .piece(sc.piece)
        .unwrap()
        .markings
        .iter()
        .map(|m| m.id)
        .collect();
    assert_eq!(kept, vec![keep]);
    assert_eq!(
        o.notes
            .iter()
            .filter(|n| n.contains("marquage") && n.contains("supprimé"))
            .count(),
        2,
        "{:?}",
        o.notes
    );
    doc.project().validate().unwrap();
    // Une seule annulation rend tout.
    assert!(doc.undo());
    assert_eq!(doc.project().piece(sc.piece).unwrap().markings.len(), 3);
    assert_eq!(bytes(doc.project()), bytes(&sc.p));
    // Supprimer la feuille d'une pièce liée supprime ses pièces, donc ses marquages.
    let other = {
        let mut p = sc.p.clone();
        let extra = p
            .apply(Command::CreateSheet {
                name: "Autre".into(),
            })
            .unwrap()
            .sheet
            .unwrap();
        p.apply(Command::DeleteSheet { sheet: sc.s }).unwrap();
        (p, extra)
    };
    other.0.validate().unwrap();
    assert!(other.0.pieces.is_empty());
}

// ----- CA5 : refus sans trace -----------------------------------------------------

#[test]
fn ca5_entrees_invalides_refusees_sans_trace() {
    let mut sc = scene();
    let hole = add_ok(&mut sc.p, sc.piece, on(sc.inside), drill(6.0));
    let (p, s, piece) = (&mut sc.p, sc.s, sc.piece);
    let bogus = PointId(9_999_999);
    // Dimensions : zéro, négatif, non fini, hors bornes, texte trop petit.
    for d in [
        0.0,
        -1.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        MARKING_MIN_DIAMETER_MM / 2.0,
        MARKING_MAX_DIAMETER_MM + 0.001,
        1e300,
    ] {
        refused(p, add(piece, on(sc.inside), drill(d)));
        refused(
            p,
            Command::SetPieceMarking {
                piece,
                marking: hole,
                at: on(sc.inside),
                shape: drill(d),
            },
        );
    }
    for c in [
        0.0,
        -2.0,
        f64::NAN,
        f64::INFINITY,
        MARKING_MIN_CROSS_MM / 2.0,
        1000.0,
        f64::MAX,
    ] {
        refused(p, add(piece, on(sc.inside), cross(c)));
    }
    // Recul : zéro, négatif, non fini, hors bornes.
    for r in [
        0.0,
        -5.0,
        f64::NAN,
        f64::INFINITY,
        MARKING_MIN_SETBACK_MM / 2.0,
        MARKING_MAX_SETBACK_MM + 0.001,
    ] {
        refused(
            p,
            add(piece, dart(sc.tip, sc.toward, r), MarkingShape::Punch),
        );
    }
    // Pointe = direction ; point absent ; pièce absente ; marquage absent.
    refused(
        p,
        add(piece, dart(sc.tip, sc.tip, 5.0), MarkingShape::Punch),
    );
    refused(p, add(piece, on(bogus), MarkingShape::Punch));
    refused(
        p,
        add(piece, dart(bogus, sc.toward, 5.0), MarkingShape::Punch),
    );
    refused(p, add(piece, dart(sc.tip, bogus, 5.0), MarkingShape::Punch));
    refused(
        p,
        add(PieceId(9_999_999), on(sc.inside), MarkingShape::Punch),
    );
    refused(
        p,
        Command::RemovePieceMarking {
            piece,
            marking: MarkingId(9_999_999),
        },
    );
    refused(
        p,
        Command::RemovePieceMarking {
            piece: PieceId(9_999_999),
            marking: hole,
        },
    );
    refused(
        p,
        Command::SetPieceMarking {
            piece,
            marking: MarkingId(9_999_999),
            at: on(sc.inside),
            shape: MarkingShape::Punch,
        },
    );
    // Un identifiant d'une autre sorte d'objet n'est pas un marquage.
    refused(
        p,
        Command::RemovePieceMarking {
            piece,
            marking: MarkingId(piece.0),
        },
    );
    // Édition : ancrage absent ou pince dégénérée.
    refused(
        p,
        Command::SetPieceMarking {
            piece,
            marking: hole,
            at: dart(sc.tip, sc.tip, 5.0),
            shape: MarkingShape::Punch,
        },
    );
    refused(
        p,
        Command::SetPieceMarking {
            piece,
            marking: hole,
            at: on(bogus),
            shape: MarkingShape::Punch,
        },
    );
    // Un point d'une autre feuille n'est pas permis pour une pièce liée.
    let other = p
        .apply(Command::CreateSheet {
            name: "Autre".into(),
        })
        .unwrap()
        .sheet
        .unwrap();
    let foreign = free_point(p, other, 5.0, 5.0);
    let msg = refused(p, add(piece, on(foreign), MarkingShape::Punch));
    assert!(!msg.is_empty());
    refused(
        p,
        add(piece, dart(sc.tip, foreign, 5.0), MarkingShape::Punch),
    );
    // Le refus d'un lot est total : aucune des commandes valides du lot ne reste.
    refused(
        p,
        Command::Batch {
            commands: vec![
                add(piece, on(sc.inside), MarkingShape::Punch),
                add(piece, on(sc.inside), drill(-1.0)),
            ],
        },
    );
    // Pièce détachée sans contour : pas de feuille où chercher les points.
    let det = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(50.0, 30.0) },
            name: "Détachée".into(),
            detached: true,
        })
        .unwrap()
        .pieces[0];
    p.apply(Command::SetPieceOutline {
        piece: det,
        outline: vec![],
        grainline: None,
    })
    .unwrap();
    let msg = refused(p, add(det, on(sc.inside), MarkingShape::Punch));
    assert!(msg.contains("contour"), "{msg}");
    // Trop de marquages sur une pièce.
    let mut q = scene();
    let batch: Vec<Command> = (0..MARKING_MAX_PER_PIECE)
        .map(|_| add(q.piece, on(q.inside), MarkingShape::Punch))
        .collect();
    q.p.apply(Command::Batch { commands: batch }).unwrap();
    assert_eq!(
        q.p.piece(q.piece).unwrap().markings.len(),
        MARKING_MAX_PER_PIECE
    );
    let msg = refused(&mut q.p, add(q.piece, on(q.inside), MarkingShape::Punch));
    assert!(msg.contains("1000"), "{msg}");
    q.p.validate().unwrap();
}

// ----- CA6 : une étape annulable --------------------------------------------------

#[test]
fn ca6_chaque_commande_est_une_etape_annulable_et_un_no_op_n_en_est_pas_une() {
    let sc = scene();
    let mut doc = Document::new(sc.p.clone());
    let original = bytes(doc.project());
    let m = doc
        .execute(add(sc.piece, dart(sc.tip, sc.toward, 10.0), drill(5.0)))
        .unwrap()
        .markings[0];
    let after_add = bytes(doc.project());
    assert_ne!(after_add, original);
    // Édition identique : aucune étape.
    doc.execute(Command::SetPieceMarking {
        piece: sc.piece,
        marking: m,
        at: dart(sc.tip, sc.toward, 10.0),
        shape: drill(5.0),
    })
    .unwrap();
    assert_eq!(bytes(doc.project()), after_add);
    // Édition réelle : une étape.
    doc.execute(Command::SetPieceMarking {
        piece: sc.piece,
        marking: m,
        at: on(sc.inside),
        shape: cross(9.0),
    })
    .unwrap();
    assert_ne!(bytes(doc.project()), after_add);
    doc.execute(Command::RemovePieceMarking {
        piece: sc.piece,
        marking: m,
    })
    .unwrap();
    // Trois étapes exactement (ajout, édition, retrait) : le no-op n'en a pas créé.
    assert!(doc.undo());
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), after_add);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), original);
    assert!(!doc.can_undo(), "le no-op n'a créé aucune étape");
    assert!(doc.redo() && doc.redo() && doc.redo());
    assert!(doc.project().piece(sc.piece).unwrap().markings.is_empty());
    // Un lot de plusieurs marquages = une seule étape.
    let mut doc = Document::new(sc.p.clone());
    doc.execute(Command::Batch {
        commands: vec![
            add(sc.piece, on(sc.inside), MarkingShape::Punch),
            add(sc.piece, on(sc.tip), drill(4.0)),
            add(sc.piece, on(sc.toward), cross(4.0)),
        ],
    })
    .unwrap();
    assert_eq!(doc.project().piece(sc.piece).unwrap().markings.len(), 3);
    assert!(doc.undo());
    assert!(!doc.can_undo());
    assert_eq!(bytes(doc.project()), original);
}

// ----- CA7 : avertissement hors contour -------------------------------------------

#[test]
fn ca7_marquage_hors_contour_signale_jamais_refuse() {
    let mut sc = scene();
    assert!(!sc
        .p
        .piece_issues(sc.piece)
        .iter()
        .any(|i| matches!(i, PieceIssue::MarkingOutsideOutline { .. })));
    add_ok(&mut sc.p, sc.piece, on(sc.inside), drill(6.0));
    assert!(!sc
        .p
        .piece_issues(sc.piece)
        .iter()
        .any(|i| matches!(i, PieceIssue::MarkingOutsideOutline { .. })));
    add_ok(&mut sc.p, sc.piece, on(sc.outside), drill(6.0));
    // Bout de pince reculé au-delà du contour : (52 ; 66), hors de la pièce de 60 mm de haut.
    add_ok(
        &mut sc.p,
        sc.piece,
        dart(sc.tip, sc.toward, 70.0),
        MarkingShape::Punch,
    );
    sc.p.validate().unwrap();
    let issues = sc.p.piece_issues(sc.piece);
    assert!(
        issues.contains(&PieceIssue::MarkingOutsideOutline { count: 2 }),
        "{issues:?}"
    );
    assert_eq!(
        PieceIssue::MarkingOutsideOutline { count: 2 }.key(),
        "marking_outside_outline"
    );
    let list = sc.p.piece_markings(sc.piece);
    assert_eq!(
        list.iter().map(|r| r.inside_outline).collect::<Vec<_>>(),
        vec![Some(true), Some(false), Some(false)]
    );
    let card = sc.p.piece_card(sc.piece).unwrap();
    assert_eq!(card.markings, 3);
    assert_eq!(card.issues, issues);
    // L'avertissement disparaît quand le point rentre dans le contour.
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: sc.outside,
        to: pt(70.0, 30.0),
    })
    .unwrap();
    assert!(sc
        .p
        .piece_issues(sc.piece)
        .contains(&PieceIssue::MarkingOutsideOutline { count: 1 }));
}

// ----- CA8 : cohérence avec les autres opérations --------------------------------

#[test]
fn ca8_copie_de_feuille_remappe_points_et_identifiants() {
    let mut sc = scene();
    let a = add_ok(&mut sc.p, sc.piece, on(sc.inside), drill(6.0));
    let b = add_ok(
        &mut sc.p,
        sc.piece,
        dart(sc.tip, sc.toward, 10.0),
        cross(7.0),
    );
    let before = sc.p.clone();
    let o = sc.p.apply(Command::DuplicateSheet { sheet: sc.s }).unwrap();
    sc.p.validate().unwrap();
    let copy = o.pieces[0];
    // La pièce d'origine n'a pas changé.
    assert_eq!(
        sc.p.piece(sc.piece).unwrap(),
        before.piece(sc.piece).unwrap()
    );
    let orig = sc.p.piece(sc.piece).unwrap();
    let dup = sc.p.piece(copy).unwrap();
    assert_eq!(dup.markings.len(), 2);
    // Identifiants neufs, formes identiques, points propres à la copie.
    assert!(dup.markings.iter().all(|m| m.id != a && m.id != b));
    assert_eq!(dup.markings[0].shape, drill(6.0));
    assert_eq!(dup.markings[1].shape, cross(7.0));
    let PieceLink::Linked { sheet: new_sheet } = dup.link else {
        panic!("copie liée");
    };
    let sh = sc.p.sheet(new_sheet).unwrap();
    for m in &dup.markings {
        for pnt in m.at.points() {
            assert!(sh.point(pnt).is_ok(), "point de la copie");
            assert!(
                !orig.markings.iter().any(|o| o.at.points().contains(&pnt)),
                "la copie ne partage aucun point avec l'original"
            );
        }
    }
    // Mêmes positions.
    for (x, y) in positions(&sc.p, copy)
        .iter()
        .zip(positions(&sc.p, sc.piece))
    {
        assert!(at_near(*x, y));
    }
    // Déplacer un point de la copie ne bouge pas l'original.
    let MarkingAnchor::Point { point } = dup.markings[0].at else {
        panic!("ancrage point");
    };
    let moved = positions(&sc.p, sc.piece);
    sc.p.apply(Command::MovePoint {
        sheet: new_sheet,
        point,
        to: pt(80.0, 50.0),
    })
    .unwrap();
    assert_eq!(positions(&sc.p, sc.piece), moved);
    assert!(at_near(positions(&sc.p, copy)[0], pt(80.0, 50.0)));
    // Import dans un autre projet : mêmes remappages, identifiants neufs.
    let mut other = Project::new("Autre projet");
    let imported = other.apply(before.import_command(sc.s).unwrap()).unwrap();
    other.validate().unwrap();
    let ip = other.piece(imported.pieces[0]).unwrap();
    assert_eq!(ip.markings.len(), 2);
    assert!(at_near(positions(&other, ip.id)[1], pt(16.0, 18.0)));
}

/// Pièce 100 × 60 à trois marquages : deux franchement de chaque côté d'une coupe
/// verticale à x = 50, un exactement sur la ligne de coupe.
fn split_scene() -> (Scene, [PointId; 3]) {
    let mut sc = scene();
    let left = free_point(&mut sc.p, sc.s, 20.0, 30.0);
    let right = free_point(&mut sc.p, sc.s, 80.0, 40.0);
    add_ok(&mut sc.p, sc.piece, on(left), drill(6.0));
    add_ok(&mut sc.p, sc.piece, on(right), cross(5.0));
    let on_cut = sc.inside;
    add_ok(&mut sc.p, sc.piece, on(on_cut), MarkingShape::Punch);
    (sc, [left, right, on_cut])
}

#[test]
fn ca8_separer_reprend_chaque_marquage_dans_sa_moitie_et_signale_le_reste() {
    let (mut sc, _) = split_scene();
    let o =
        sc.p.apply(Command::SplitPiece {
            piece: sc.piece,
            a: pt(50.0, -10.0),
            b: pt(50.0, 70.0),
        })
        .unwrap();
    sc.p.validate().unwrap();
    assert_eq!(o.pieces.len(), 2);
    let (l, r) = (o.pieces[0], o.pieces[1]);
    // « 1/2 » est à gauche de a → b (x < 50), « 2/2 » à droite.
    let lp = sc.p.piece_markings(l);
    let rp = sc.p.piece_markings(r);
    assert!(lp
        .iter()
        .any(|m| m.marking.shape == drill(6.0) && at_near(m.at, pt(20.0, 30.0))));
    assert!(rp
        .iter()
        .any(|m| m.marking.shape == cross(5.0) && at_near(m.at, pt(80.0, 40.0))));
    // Chaque marquage est repris une seule fois ou signalé : jamais perdu en silence.
    let lost = o
        .notes
        .iter()
        .filter(|n| n.contains("marquage(s)") && n.contains("ligne de coupe"))
        .count();
    assert_eq!(lp.len() + rp.len() + lost, 3, "{:?}", o.notes);
    // Un marquage exactement sur la ligne de coupe (bord compris) va à la première moitié.
    assert_eq!((lp.len(), rp.len(), lost), (2, 1, 0), "{:?}", o.notes);
    assert!(lp
        .iter()
        .any(|m| m.marking.shape == MarkingShape::Punch && at_near(m.at, pt(50.0, 30.0))));
    // Les marquages repris sont ancrés à des points libres des feuilles des moitiés.
    for piece in [l, r] {
        let sheet = sc.p.piece_sheet(piece).unwrap();
        for m in &sc.p.piece(piece).unwrap().markings {
            let MarkingAnchor::Point { point } = m.at else {
                panic!("ancrage point");
            };
            assert!(sheet.point(point).is_ok());
        }
    }
    // Une pièce séparée ne partage aucun identifiant de marquage avec l'autre.
    let ids: Vec<MarkingId> = [l, r]
        .iter()
        .flat_map(|&x| sc.p.piece(x).unwrap().markings.iter().map(|m| m.id))
        .collect();
    let set: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(set.len(), ids.len());
}

#[test]
fn ca8_separer_une_pince_reprend_la_position_calculee() {
    let mut sc = scene();
    add_ok(
        &mut sc.p,
        sc.piece,
        dart(sc.tip, sc.toward, 10.0),
        drill(3.0),
    );
    let o =
        sc.p.apply(Command::SplitPiece {
            piece: sc.piece,
            a: pt(50.0, -10.0),
            b: pt(50.0, 70.0),
        })
        .unwrap();
    // (16 ; 18) est dans la moitié gauche, ancré désormais à un point libre.
    let left = sc.p.piece_markings(o.pieces[0]);
    assert_eq!(left.len(), 1);
    assert!(at_near(left[0].at, pt(16.0, 18.0)));
    assert!(matches!(left[0].marking.at, MarkingAnchor::Point { .. }));
    assert!(sc.p.piece_markings(o.pieces[1]).is_empty());
}

#[test]
fn ca8_assembler_rend_les_marquages_des_deux_pieces_a_leur_place() {
    let mut sc = scene();
    let left = free_point(&mut sc.p, sc.s, 20.0, 30.0);
    let right = free_point(&mut sc.p, sc.s, 80.0, 40.0);
    add_ok(&mut sc.p, sc.piece, on(left), drill(6.0));
    add_ok(&mut sc.p, sc.piece, on(right), cross(5.0));
    let o =
        sc.p.apply(Command::SplitPiece {
            piece: sc.piece,
            a: pt(50.0, -10.0),
            b: pt(50.0, 70.0),
        })
        .unwrap();
    let (l, r) = (o.pieces[0], o.pieces[1]);
    let edge = |p: &Project, half: PieceId| p.piece(half).unwrap().outline.last().unwrap().entity;
    let j =
        sc.p.apply(Command::JoinPieces {
            first: l,
            first_edge: edge(&sc.p, l),
            second: r,
            second_edge: edge(&sc.p, r),
            tolerance_mm: 0.5,
        })
        .unwrap();
    sc.p.validate().unwrap();
    let merged = j.pieces[0];
    let list = sc.p.piece_markings(merged);
    assert_eq!(list.len(), 2, "{:?}", j.notes);
    let find = |shape: MarkingShape| list.iter().find(|m| m.marking.shape == shape).unwrap().at;
    assert!(distance_mm(find(drill(6.0)), pt(20.0, 30.0)) < 1e-6);
    assert!(distance_mm(find(cross(5.0)), pt(80.0, 40.0)) < 1e-6);
    assert!(sc.p.piece(l).is_err() && sc.p.piece(r).is_err());
}

#[test]
fn ca8_deplier_ne_reprend_pas_les_marquages_et_le_dit() {
    let mut p = Project::new("déplier");
    let s = p.sheets[0].id;
    let o = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(pt(0.0, 0.0)),
            width_mm: 60.0,
            height_mm: 40.0,
        })
        .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: o.entities,
            },
            name: "Demi-dos".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let fold = {
        let sheet = p.piece_sheet(piece).unwrap();
        p.piece(piece)
            .unwrap()
            .outline
            .iter()
            .find(|e| match sheet.shape(e.entity).unwrap() {
                Shape::Line(a, b) => a.x.abs() < 1e-9 && b.x.abs() < 1e-9,
                _ => false,
            })
            .unwrap()
            .entity
    };
    let q = free_point(&mut p, s, 20.0, 20.0);
    add_ok(&mut p, piece, on(q), drill(6.0));
    let before = p.piece(piece).unwrap().clone();
    let u = p
        .apply(Command::UnfoldPiece {
            piece,
            fold,
            name: None,
        })
        .unwrap();
    p.validate().unwrap();
    assert!(p.piece(u.pieces[0]).unwrap().markings.is_empty());
    assert_eq!(p.piece(piece).unwrap().markings, before.markings);
    assert!(
        u.notes
            .iter()
            .any(|n| n.contains("marquage") && n.contains("pas repris")),
        "{:?}",
        u.notes
    );
}

#[test]
fn ca8_le_plan_de_pieces_ne_touche_ni_la_construction_ni_les_marquages() {
    let mut sc = scene();
    add_ok(&mut sc.p, sc.piece, on(sc.inside), drill(6.0));
    add_ok(
        &mut sc.p,
        sc.piece,
        dart(sc.tip, sc.toward, 10.0),
        cross(5.0),
    );
    let marks = sc.p.piece(sc.piece).unwrap().markings.clone();
    let at = positions(&sc.p, sc.piece);
    sc.p.apply(Command::SetPiecePlacement {
        piece: sc.piece,
        placement: Some(Placement {
            x_mm: 300.0,
            y_mm: -80.0,
            rotation_deg: 37.0,
        }),
    })
    .unwrap();
    assert_eq!(sc.p.piece(sc.piece).unwrap().markings, marks);
    assert_eq!(positions(&sc.p, sc.piece), at, "placement = vue seulement");
    assert_eq!(sc.p.plan_layout().len(), 1);
    sc.p.validate().unwrap();
}

#[test]
fn ca8_piece_detachee_marquages_sur_les_points_de_sa_feuille() {
    let mut sc = scene();
    let det =
        sc.p.apply(Command::ExtractPiece {
            sheet: sc.s,
            source: PieceSource::Click { at: pt(50.0, 30.0) },
            name: "Copie".into(),
            detached: true,
        })
        .unwrap()
        .pieces[0];
    let dsheet = sc.p.piece_sheet(det).unwrap().id;
    assert_ne!(dsheet, sc.s);
    let q = free_point(&mut sc.p, dsheet, 30.0, 30.0);
    let t = free_point(&mut sc.p, dsheet, 10.0, 10.0);
    let u = free_point(&mut sc.p, dsheet, 10.0, 40.0);
    add_ok(&mut sc.p, det, on(q), drill(6.0));
    add_ok(&mut sc.p, det, dart(t, u, 5.0), MarkingShape::Punch);
    sc.p.validate().unwrap();
    let list = sc.p.piece_markings(det);
    assert!(at_near(list[0].at, pt(30.0, 30.0)));
    assert!(at_near(list[1].at, pt(10.0, 15.0)));
    assert!(list.iter().all(|r| r.inside_outline == Some(true)));
    // Le marquage vit avec la feuille de la pièce : exporté sur cette feuille seulement.
    assert_eq!(sc.p.sheet_markings(dsheet).len(), 2);
    assert!(sc.p.sheet_markings(sc.s).is_empty());
    // La suppression d'un de ses points retire le marquage, comme pour une pièce liée.
    let o =
        sc.p.apply(Command::Delete {
            sheet: dsheet,
            points: vec![q],
            entities: vec![],
        })
        .unwrap();
    assert_eq!(sc.p.piece(det).unwrap().markings.len(), 1);
    assert!(o.notes.iter().any(|n| n.contains("marquage")));
}

#[test]
fn ca8_piece_derivee_les_marquages_ne_sont_jamais_perdus_en_silence() {
    let mut sc = scene();
    let d =
        sc.p.apply(Command::CreateDerivedPiece {
            mother: sc.piece,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset { distance_mm: -5.0 },
            name: None,
        })
        .unwrap()
        .pieces[0];
    // Un marquage sur un point libre de la feuille de la dérivée, un sur un de ses sommets.
    let dsheet = sc.p.piece_sheet(d).unwrap().id;
    let free = free_point(&mut sc.p, dsheet, 50.0, 30.0);
    let vertex =
        sc.p.sheet(dsheet)
            .unwrap()
            .points
            .iter()
            .find(|p| p.id != free)
            .unwrap()
            .id;
    add_ok(&mut sc.p, d, on(free), drill(4.0));
    add_ok(&mut sc.p, d, on(vertex), MarkingShape::Punch);
    sc.p.validate().unwrap();
    // La mère change : la dérivée est régénérée dans la même étape.
    let mother_corner =
        sc.p.sheet(sc.s)
            .unwrap()
            .points
            .iter()
            .find(|p| {
                sc.p.sheet(sc.s)
                    .unwrap()
                    .position(p.id)
                    .is_ok_and(|a| at_near(a, pt(100.0, 60.0)))
            })
            .unwrap()
            .id;
    let o =
        sc.p.apply(Command::MovePoint {
            sheet: sc.s,
            point: mother_corner,
            to: pt(110.0, 60.0),
        })
        .unwrap();
    assert!(o.regenerated.contains(&d), "{:?}", o.notes);
    sc.p.validate().unwrap();
    // Chaque marquage encore présent a un point vivant ; chaque retrait est dit.
    let kept = sc.p.piece(d).unwrap().markings.len();
    let retired = o
        .notes
        .iter()
        .filter(|n| n.contains("marquage") && n.contains("retiré"))
        .count();
    assert_eq!(kept + retired, 2, "{:?}", o.notes);
    // Même topologie (régénération en place, mêmes identifiants de points) : tous suivent.
    assert_eq!((kept, retired), (2, 0), "{:?}", o.notes);
    for r in sc.p.piece_markings(d) {
        assert!(sc
            .p
            .sheet(dsheet)
            .unwrap()
            .point(r.marking.at.points()[0])
            .is_ok());
    }
}

#[test]
fn ca8_les_marquages_ne_sont_pas_des_elements_internes() {
    // Un point peut être à la fois une marque (US-04.3) et porter un perçage (US-06.3) :
    // deux concepts indépendants, deux listes, deux commandes.
    let mut sc = scene();
    sc.p.apply(Command::AddPieceInternal {
        piece: sc.piece,
        target: InternalTarget::Point { point: sc.inside },
        kind: InternalKind::Mark,
    })
    .unwrap();
    add_ok(&mut sc.p, sc.piece, on(sc.inside), drill(6.0));
    let p = sc.p.piece(sc.piece).unwrap();
    assert_eq!(p.internal.len(), 1);
    assert_eq!(p.markings.len(), 1);
    // Retirer l'un ne touche pas l'autre.
    sc.p.apply(Command::RemovePieceInternal {
        piece: sc.piece,
        target: InternalTarget::Point { point: sc.inside },
    })
    .unwrap();
    assert_eq!(sc.p.piece(sc.piece).unwrap().markings.len(), 1);
    // Les décomptes d'éléments internes ignorent les marquages.
    assert_eq!(sc.p.piece_internal_counts(sc.piece).total(), 0);
}

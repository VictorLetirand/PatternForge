//! US-06.5 — droit-fil avancé : direction par rapport à un tracé (parallèle, perpendiculaire),
//! biais ±45°, sens du tissu (aucun, poil, motif), tolérance de rotation au placement, lignes
//! supplémentaires. Un groupe de tests par critère d'acceptation :
//! CA1 modèle et fichier (schéma 3, octets inchangés), CA2 directions calculées à la main,
//! CA3 sens, CA4 tolérance de rotation (cas limites exacts), CA5 plusieurs lignes, CA6 refus
//! sans trace, CA7 une étape annulable, CA8 suppression du tracé de référence, CA9 cohérence
//! (copie de feuille, séparer, assembler, déplier, dérivées, plan, pièce détachée).
//! L'export (SVG, PDF) est mesuré dans `crates/pf-export/tests/droit_fil.rs`.

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::piece_grain::grain_rotation_deviation_deg;
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

fn at_near(a: PointMm, b: PointMm) -> bool {
    distance_mm(a, b) < 1e-9
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn fixture(name: &str) -> Project {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let text = std::fs::read_to_string(dir.join(name)).unwrap();
    persist::from_json(&text).unwrap_or_else(|e| panic!("{name} : {e}"))
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

fn seg(p: &mut Project, s: SheetId, a: (f64, f64), b: (f64, f64)) -> (EntityId, Vec<PointId>) {
    let o = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(a.0, a.1)),
            to: PointRef::New(pt(b.0, b.1)),
        })
        .unwrap();
    (o.entities[0], o.points)
}

fn grain_seg(p: &mut Project, s: SheetId, a: (f64, f64), b: (f64, f64)) -> EntityId {
    let (e, _) = seg(p, s, a, b);
    p.apply(Command::SetPathRole {
        sheet: s,
        entity: e,
        role: PathRole::Grainline,
    })
    .unwrap();
    e
}

/// Rectangle de pièce 100 × 60 mm (pièce liée « Devant ») avec :
/// - `main` : droit-fil vertical (50 ; 10) → (50 ; 50), longueur 40, milieu (50 ; 30) ;
/// - `extra_a` : (10 ; 45) → (90 ; 45) et `extra_b` : (10 ; 15) → (90 ; 15) (rôle droit-fil) ;
/// - `href` : tracé de référence horizontal (0 ; 100) → (40 ; 100), fin `hend` ;
/// - `ob` : tracé de référence oblique (110 ; 0) → (140 ; 40) (vecteur 30 ; 40, unitaire
///   0,6 ; 0,8), fin `ob_end`.
struct Sc {
    p: Project,
    s: SheetId,
    piece: PieceId,
    main: EntityId,
    extra_a: EntityId,
    extra_b: EntityId,
    href: EntityId,
    hend: PointId,
    ob: EntityId,
    ob_end: PointId,
}

fn scene() -> Sc {
    let mut p = Project::new("Droit-fil");
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
    let main = grain_seg(&mut p, s, (50.0, 10.0), (50.0, 50.0));
    let outline = p.piece(piece).unwrap().outline.clone();
    p.apply(Command::SetPieceOutline {
        piece,
        outline,
        grainline: Some(main),
    })
    .unwrap();
    let extra_a = grain_seg(&mut p, s, (10.0, 45.0), (90.0, 45.0));
    let extra_b = grain_seg(&mut p, s, (10.0, 15.0), (90.0, 15.0));
    let (href, hp) = seg(&mut p, s, (0.0, 100.0), (40.0, 100.0));
    let (ob, op) = seg(&mut p, s, (110.0, 0.0), (140.0, 40.0));
    Sc {
        p,
        s,
        piece,
        main,
        extra_a,
        extra_b,
        href,
        hend: hp[1],
        ob,
        ob_end: op[1],
    }
}

fn dir(g: GrainDirection) -> PieceGrain {
    PieceGrain {
        direction: g,
        ..PieceGrain::default()
    }
}

fn par(e: EntityId) -> GrainDirection {
    GrainDirection::Parallel { reference: e }
}

fn perp(e: EntityId) -> GrainDirection {
    GrainDirection::Perpendicular { reference: e }
}

fn bias(a: f64) -> GrainDirection {
    GrainDirection::Bias { angle_deg: a }
}

fn set(piece: PieceId, grain: PieceGrain) -> Command {
    Command::SetPieceGrain { piece, grain }
}

fn extra(entity: EntityId, direction: GrainDirection, sense: GrainSense) -> ExtraGrainLine {
    ExtraGrainLine {
        entity,
        direction,
        sense,
        reversed: false,
    }
}

fn main_line(p: &Project, piece: PieceId) -> ResolvedGrainLine {
    p.piece_grain_lines(piece)[0]
}

fn assert_line(l: &ResolvedGrainLine, from: PointMm, to: PointMm, angle: f64) {
    assert!(
        distance_mm(l.from, from) < 1e-9 && distance_mm(l.to, to) < 1e-9,
        "attendu {from:?} → {to:?}, obtenu {:?} → {:?}",
        l.from,
        l.to
    );
    assert!(
        (l.direction_deg - angle).abs() < 1e-9,
        "angle attendu {angle}, obtenu {}",
        l.direction_deg
    );
    assert!(near(l.length_mm, distance_mm(from, to)));
}

fn rotate(p: &mut Project, piece: PieceId, deg: f64) {
    p.apply(Command::SetPiecePlacement {
        piece,
        placement: Some(Placement {
            x_mm: 300.0,
            y_mm: -80.0,
            rotation_deg: deg,
        }),
    })
    .unwrap();
}

fn rotation_issue(p: &Project, piece: PieceId) -> bool {
    p.piece_issues(piece)
        .contains(&PieceIssue::GrainRotationExceeded)
}

// ----- CA1 : modèle et fichier ----------------------------------------------------

#[test]
fn ca1_champ_absent_octets_inchanges() {
    for name in FIXTURES {
        let first = bytes(&fixture(name));
        let second = bytes(&persist::from_json(&first).unwrap());
        assert_eq!(first, second, "{name}");
        assert!(!first.contains("\"grain\""), "{name}");
        assert!(fixture(name).pieces.iter().all(|p| p.grain.is_none()));
    }
    let sc = scene();
    let text = bytes(&sc.p);
    assert!(text.contains("\"grainline\""));
    assert!(!text.contains("\"grain\""));
    assert_eq!(text, bytes(&persist::from_json(&text).unwrap()));
    // Les fichiers livrés sont relus puis réécrits à l'identique, sans réglage de droit-fil.
    let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut seen = 0;
    for entry in std::fs::read_dir(examples).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(".pforge.json") {
            continue;
        }
        let shipped = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n");
        assert_eq!(
            shipped,
            persist::to_json(&persist::from_json(&shipped).unwrap()).unwrap(),
            "{path:?}"
        );
        assert!(!shipped.contains("\"grain\""), "{path:?}");
        seen += 1;
    }
    assert!(seen >= 1);
    // Retirer le dernier réglage : plus aucune clé « grain ».
    let mut sc = scene();
    sc.p.apply(set(sc.piece, dir(bias(45.0)))).unwrap();
    assert!(bytes(&sc.p).contains("\"grain\""));
    sc.p.apply(set(sc.piece, PieceGrain::default())).unwrap();
    assert!(!bytes(&sc.p).contains("\"grain\""));
    assert!(sc.p.piece(sc.piece).unwrap().grain.is_none());
}

#[test]
fn ca1_champ_present_relu_a_l_identique_schema_3() {
    let mut sc = scene();
    let g = PieceGrain {
        direction: par(sc.href),
        sense: GrainSense::Nap,
        reversed: true,
        rotation_tolerance_deg: Some(12.5),
        extra: vec![
            ExtraGrainLine {
                entity: sc.extra_a,
                direction: perp(sc.ob),
                sense: GrainSense::Pattern,
                reversed: false,
            },
            extra(sc.extra_b, bias(-45.0), GrainSense::Unmarked),
        ],
    };
    sc.p.apply(set(sc.piece, g.clone())).unwrap();
    let text = bytes(&sc.p);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["schema_version"], 6, "schéma 4 (US-09.1)");
    assert_eq!(
        v["pieces"][0]["grain"],
        json!({
            "direction": {"mode": "parallel", "reference": sc.href.0},
            "sense": "nap",
            "reversed": true,
            "rotation_tolerance_deg": 12.5,
            "extra": [
                {"entity": sc.extra_a.0, "direction": {"mode": "perpendicular", "reference": sc.ob.0},
                 "sense": "pattern"},
                {"entity": sc.extra_b.0, "direction": {"mode": "bias", "angle_deg": -45.0}}
            ]
        })
    );
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, sc.p);
    assert_eq!(bytes(&back), text);
    assert!(
        back.pieces[0].extra.is_empty(),
        "champ typé, pas une clé inconnue"
    );
    assert_eq!(back.piece(sc.piece).unwrap().grain.as_ref(), Some(&g));
    // Valeurs par défaut omises : une direction « dessinée » seule n'écrit rien.
    assert_eq!(
        serde_json::to_value(dir(GrainDirection::Drawn {})).unwrap(),
        json!({})
    );
    assert_eq!(
        [GrainSense::Unmarked, GrainSense::Nap, GrainSense::Pattern].map(|s| s.key()),
        ["none", "nap", "pattern"]
    );
    assert_eq!(
        [
            GrainDirection::Drawn {},
            par(sc.href),
            perp(sc.href),
            bias(45.0)
        ]
        .map(|d| d.key()),
        ["drawn", "parallel", "perpendicular", "bias"]
    );
}

#[test]
fn ca1_fichier_invalide_refuse_a_l_ouverture_sans_perte_silencieuse() {
    let mut sc = scene();
    let g = PieceGrain {
        direction: par(sc.href),
        sense: GrainSense::Nap,
        rotation_tolerance_deg: Some(10.0),
        extra: vec![extra(sc.extra_a, perp(sc.ob), GrainSense::Pattern)],
        ..PieceGrain::default()
    };
    sc.p.apply(set(sc.piece, g)).unwrap();
    let good: Value = serde_json::from_str(&bytes(&sc.p)).unwrap();
    let mutate = |f: &dyn Fn(&mut Value)| -> Result<Project, CoreError> {
        let mut v = good.clone();
        f(&mut v);
        persist::from_json(&v.to_string())
    };
    assert!(mutate(&|_| {}).is_ok(), "témoin");
    fn gr(v: &mut Value) -> &mut Value {
        &mut v["pieces"][0]["grain"]
    }
    let missing = 987_654;
    type Mutation = Box<dyn Fn(&mut Value)>;
    let cases: Vec<(&str, Mutation)> = vec![
        (
            "clé inconnue",
            Box::new(|v| gr(v)["couleur"] = json!("rouge")),
        ),
        (
            "clé inconnue dans la direction",
            Box::new(|v| gr(v)["direction"]["x"] = json!(1)),
        ),
        (
            "mode inconnu",
            Box::new(|v| gr(v)["direction"]["mode"] = json!("diagonale")),
        ),
        (
            "mode « drawn » avec une clé en trop (défaut D-04 évité)",
            Box::new(|v| gr(v)["direction"] = json!({"mode": "drawn", "reference": 1})),
        ),
        ("sens inconnu", Box::new(|v| gr(v)["sense"] = json!("bois"))),
        ("sens non texte", Box::new(|v| gr(v)["sense"] = json!(3))),
        (
            "référence absente",
            Box::new(move |v| gr(v)["direction"]["reference"] = json!(missing)),
        ),
        (
            "biais à 30°",
            Box::new(|v| gr(v)["direction"] = json!({"mode": "bias", "angle_deg": 30.0})),
        ),
        (
            "biais à 45,0000001°",
            Box::new(|v| gr(v)["direction"] = json!({"mode": "bias", "angle_deg": 45.0000001})),
        ),
        (
            "tolérance négative",
            Box::new(|v| gr(v)["rotation_tolerance_deg"] = json!(-1.0)),
        ),
        (
            "tolérance au-delà de 180",
            Box::new(|v| gr(v)["rotation_tolerance_deg"] = json!(180.5)),
        ),
        (
            "tolérance nulle écrite (forme non canonique)",
            Box::new(|v| gr(v)["rotation_tolerance_deg"] = json!(0.0)),
        ),
        (
            "tolérance texte",
            Box::new(|v| gr(v)["rotation_tolerance_deg"] = json!("10")),
        ),
        (
            "réglages tous par défaut écrits",
            Box::new(|v| *gr(v) = json!({})),
        ),
        (
            "ligne supplémentaire absente",
            Box::new(move |v| gr(v)["extra"][0]["entity"] = json!(missing)),
        ),
        (
            "ligne supplémentaire = ligne principale",
            Box::new(|v| {
                let main = v["pieces"][0]["grainline"].clone();
                gr(v)["extra"][0]["entity"] = main;
            }),
        ),
        (
            "ligne supplémentaire sans rôle droit-fil",
            Box::new(|v| {
                let any = v["pieces"][0]["outline"][0]["entity"].clone();
                gr(v)["extra"][0]["entity"] = any;
            }),
        ),
        (
            "neuf lignes supplémentaires",
            Box::new(|v| {
                let one = gr(v)["extra"][0].clone();
                gr(v)["extra"] = json!([one, one, one, one, one, one, one, one, one]);
            }),
        ),
        (
            "réglages sans droit-fil principal",
            Box::new(|v| {
                v["pieces"][0].as_object_mut().unwrap().remove("grainline");
            }),
        ),
        ("réglages pas un objet", Box::new(|v| *gr(v) = json!(3))),
    ];
    for (what, f) in cases {
        let err = mutate(&*f);
        assert!(err.is_err(), "{what} : le fichier aurait dû être refusé");
        assert!(!err.unwrap_err().to_string().is_empty(), "{what}");
    }
}

// ----- CA2 : directions calculées à la main ---------------------------------------

#[test]
fn ca2_sans_reglage_le_droit_fil_est_celui_du_trace() {
    let sc = scene();
    let l = main_line(&sc.p, sc.piece);
    assert_line(&l, pt(50.0, 10.0), pt(50.0, 50.0), 90.0);
    assert!(l.main && l.spec.direction.is_drawn());
    // Un droit-fil « dessiné » explicite est identique.
    let mut sc = scene();
    sc.p.apply(set(sc.piece, dir(GrainDirection::Drawn {})))
        .unwrap();
    assert!(sc.p.piece(sc.piece).unwrap().grain.is_none());
}

#[test]
fn ca2_parallele_et_perpendiculaire_a_un_trace_horizontal() {
    let mut sc = scene();
    // Parallèle à l'horizontale : flèche horizontale de même longueur (40) et même milieu.
    sc.p.apply(set(sc.piece, dir(par(sc.href)))).unwrap();
    assert_line(
        &main_line(&sc.p, sc.piece),
        pt(30.0, 30.0),
        pt(70.0, 30.0),
        0.0,
    );
    // Perpendiculaire : +90° (anti-horaire), donc vers le haut.
    sc.p.apply(set(sc.piece, dir(perp(sc.href)))).unwrap();
    assert_line(
        &main_line(&sc.p, sc.piece),
        pt(50.0, 10.0),
        pt(50.0, 50.0),
        90.0,
    );
    // Inversée : de haut en bas.
    let mut g = dir(perp(sc.href));
    g.reversed = true;
    sc.p.apply(set(sc.piece, g)).unwrap();
    assert_line(
        &main_line(&sc.p, sc.piece),
        pt(50.0, 50.0),
        pt(50.0, 10.0),
        270.0,
    );
}

#[test]
fn ca2_parallele_et_perpendiculaire_a_un_trace_oblique() {
    let mut sc = scene();
    // Tracé oblique (30 ; 40) : unitaire (0,6 ; 0,8), angle atan2(0,8 ; 0,6) = 53,130102…°.
    let a = (0.8_f64).atan2(0.6).to_degrees();
    sc.p.apply(set(sc.piece, dir(par(sc.ob)))).unwrap();
    assert_line(
        &main_line(&sc.p, sc.piece),
        pt(50.0 - 12.0, 30.0 - 16.0),
        pt(50.0 + 12.0, 30.0 + 16.0),
        a,
    );
    // Perpendiculaire : (−0,8 ; 0,6), moitié de longueur 20 → (−16 ; 12).
    sc.p.apply(set(sc.piece, dir(perp(sc.ob)))).unwrap();
    assert_line(
        &main_line(&sc.p, sc.piece),
        pt(50.0 + 16.0, 30.0 - 12.0),
        pt(50.0 - 16.0, 30.0 + 12.0),
        a + 90.0,
    );
    // Le produit scalaire avec le tracé de référence est nul.
    let l = main_line(&sc.p, sc.piece);
    let (dx, dy) = (l.to.x - l.from.x, l.to.y - l.from.y);
    assert!((dx * 30.0 + dy * 40.0).abs() < 1e-9);
}

#[test]
fn ca2_biais_plus_et_moins_45_degres() {
    let mut sc = scene();
    let h = 20.0 * std::f64::consts::FRAC_1_SQRT_2;
    sc.p.apply(set(sc.piece, dir(bias(45.0)))).unwrap();
    assert_line(
        &main_line(&sc.p, sc.piece),
        pt(50.0 - h, 30.0 - h),
        pt(50.0 + h, 30.0 + h),
        45.0,
    );
    sc.p.apply(set(sc.piece, dir(bias(-45.0)))).unwrap();
    assert_line(
        &main_line(&sc.p, sc.piece),
        pt(50.0 - h, 30.0 + h),
        pt(50.0 + h, 30.0 - h),
        315.0,
    );
    // Le biais est mesuré depuis l'axe X de la feuille, quel que soit le tracé dessiné ou
    // un autre tracé : il ne dépend d'aucun tracé.
    let l = main_line(&sc.p, sc.piece);
    assert!((l.length_mm - 40.0).abs() < 1e-9);
    // Inversé : de la fin vers le début.
    let mut g = dir(bias(45.0));
    g.reversed = true;
    sc.p.apply(set(sc.piece, g)).unwrap();
    assert_line(
        &main_line(&sc.p, sc.piece),
        pt(50.0 + h, 30.0 + h),
        pt(50.0 - h, 30.0 - h),
        225.0,
    );
}

#[test]
fn ca2_courbe_de_reference_direction_de_la_corde() {
    let mut sc = scene();
    // Bézier de (110 ; 0) à (140 ; 40) : corde (30 ; 40), quelle que soit la courbure.
    let c =
        sc.p.apply(Command::AddCurve {
            sheet: sc.s,
            start: PointRef::New(pt(110.0, 0.0)),
            c1: pt(140.0, -50.0),
            c2: pt(100.0, 80.0),
            end: PointRef::New(pt(140.0, 40.0)),
        })
        .unwrap()
        .entities[0];
    sc.p.apply(set(sc.piece, dir(par(c)))).unwrap();
    let a = (0.8_f64).atan2(0.6).to_degrees();
    assert_line(
        &main_line(&sc.p, sc.piece),
        pt(38.0, 14.0),
        pt(62.0, 46.0),
        a,
    );
    // Courbe lisse par points de passage : corde du premier au dernier point.
    let sp =
        sc.p.apply(Command::AddSpline {
            sheet: sc.s,
            points: vec![
                PointRef::New(pt(0.0, 200.0)),
                PointRef::New(pt(20.0, 260.0)),
                PointRef::New(pt(40.0, 200.0)),
            ],
        })
        .unwrap()
        .entities[0];
    sc.p.apply(set(sc.piece, dir(par(sp)))).unwrap();
    assert_line(
        &main_line(&sc.p, sc.piece),
        pt(30.0, 30.0),
        pt(70.0, 30.0),
        0.0,
    );
    // Un cercle n'a pas de direction : refusé.
    let circle =
        sc.p.apply(Command::AddCircle {
            sheet: sc.s,
            center: PointRef::New(pt(200.0, 200.0)),
            radius_mm: 10.0,
        })
        .unwrap()
        .entities[0];
    let msg = refused(&mut sc.p, set(sc.piece, dir(par(circle))));
    assert!(msg.contains("direction"), "{msg}");
}

#[test]
fn ca2_la_direction_suit_le_trace_de_reference_quand_il_bouge() {
    let mut sc = scene();
    sc.p.apply(set(sc.piece, dir(par(sc.ob)))).unwrap();
    let stored = sc.p.piece(sc.piece).unwrap().grain.clone();
    // Déplacer la fin du tracé oblique en (110 ; 40) : il devient vertical (0 ; 40).
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: sc.ob_end,
        to: pt(110.0, 40.0),
    })
    .unwrap();
    assert_line(
        &main_line(&sc.p, sc.piece),
        pt(50.0, 10.0),
        pt(50.0, 50.0),
        90.0,
    );
    // Puis (150 ; 0) : horizontal. Rien n'est stocké : le réglage est resté le même.
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: sc.ob_end,
        to: pt(150.0, 0.0),
    })
    .unwrap();
    assert_line(
        &main_line(&sc.p, sc.piece),
        pt(30.0, 30.0),
        pt(70.0, 30.0),
        0.0,
    );
    assert_eq!(sc.p.piece(sc.piece).unwrap().grain, stored);
    // Perpendiculaire : suit aussi.
    sc.p.apply(set(sc.piece, dir(perp(sc.ob)))).unwrap();
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: sc.ob_end,
        to: pt(110.0, 40.0),
    })
    .unwrap();
    assert_line(
        &main_line(&sc.p, sc.piece),
        pt(70.0, 30.0),
        pt(30.0, 30.0),
        180.0,
    );
    // Le tracé dessiné lui-même, déplacé, change aussi la position (milieu) et la longueur.
    sc.p.apply(set(sc.piece, dir(bias(45.0)))).unwrap();
    let before = main_line(&sc.p, sc.piece);
    let (_, pts) =
        sc.p.sheet(sc.s)
            .unwrap()
            .entity_points(sc.main)
            .map(|(a, b)| ((), [a, b]))
            .unwrap();
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: pts[1],
        to: pt(50.0, 70.0),
    })
    .unwrap();
    let after = main_line(&sc.p, sc.piece);
    assert!(near(before.length_mm, 40.0) && near(after.length_mm, 60.0));
    assert!(at_near(
        pt(
            (after.from.x + after.to.x) / 2.0,
            (after.from.y + after.to.y) / 2.0
        ),
        pt(50.0, 40.0)
    ));
    assert!(near(after.direction_deg, 45.0));
}

#[test]
fn ca2_pas_de_reference_vers_soi_meme() {
    let mut sc = scene();
    let msg = refused(&mut sc.p, set(sc.piece, dir(par(sc.main))));
    assert!(msg.contains("ligne de droit-fil"), "{msg}");
    let mut g = dir(par(sc.href));
    g.extra = vec![
        extra(sc.extra_a, perp(sc.extra_b), GrainSense::Unmarked),
        extra(sc.extra_b, GrainDirection::Drawn {}, GrainSense::Unmarked),
    ];
    refused(&mut sc.p, set(sc.piece, g));
}

// ----- CA3 : sens -------------------------------------------------------------------

#[test]
fn ca3_sens_aucun_poil_motif() {
    let mut sc = scene();
    assert_eq!(main_line(&sc.p, sc.piece).spec.sense, GrainSense::Unmarked);
    for sense in [GrainSense::Nap, GrainSense::Pattern, GrainSense::Unmarked] {
        let mut g = dir(par(sc.href));
        g.sense = sense;
        sc.p.apply(set(sc.piece, g)).unwrap();
        assert_eq!(main_line(&sc.p, sc.piece).spec.sense, sense);
        let text = bytes(&sc.p);
        let back = persist::from_json(&text).unwrap();
        assert_eq!(back.piece_grain_lines(sc.piece)[0].spec.sense, sense);
    }
    // Un sens seul (direction du tracé dessiné) est un réglage valide.
    sc.p.apply(set(
        sc.piece,
        PieceGrain {
            sense: GrainSense::Nap,
            ..PieceGrain::default()
        },
    ))
    .unwrap();
    let l = main_line(&sc.p, sc.piece);
    assert_line(&l, pt(50.0, 10.0), pt(50.0, 50.0), 90.0);
    assert_eq!(l.spec.sense, GrainSense::Nap);
    // Un sens seul n'écrit que la clé « sense ».
    let v: Value = serde_json::from_str(&bytes(&sc.p)).unwrap();
    assert_eq!(v["pieces"][0]["grain"], json!({"sense": "nap"}));
    // L'inversion échange les bouts du tracé dessiné sans rien arrondir.
    sc.p.apply(set(
        sc.piece,
        PieceGrain {
            sense: GrainSense::Nap,
            reversed: true,
            ..PieceGrain::default()
        },
    ))
    .unwrap();
    assert_line(
        &main_line(&sc.p, sc.piece),
        pt(50.0, 50.0),
        pt(50.0, 10.0),
        270.0,
    );
}

// ----- CA4 : tolérance de rotation ---------------------------------------------------

#[test]
fn ca4_deviation_calculee_a_la_main() {
    // Sens orienté (poil, motif) : [0 ; 180].
    for (rot, dev) in [
        (0.0, 0.0),
        (30.0, 30.0),
        (90.0, 90.0),
        (180.0, 180.0),
        (181.0, 179.0),
        (330.0, 30.0),
        (359.5, 0.5),
        (-30.0, 30.0),
        (390.0, 30.0),
    ] {
        assert!(
            near(grain_rotation_deviation_deg(rot, true), dev),
            "orienté {rot}"
        );
    }
    // Aucun sens (axe) : [0 ; 90], tourner de 180° laisse le droit-fil parallèle.
    for (rot, dev) in [
        (0.0, 0.0),
        (30.0, 30.0),
        (90.0, 90.0),
        (150.0, 30.0),
        (180.0, 0.0),
        (210.0, 30.0),
        (270.0, 90.0),
        (330.0, 30.0),
    ] {
        assert!(
            near(grain_rotation_deviation_deg(rot, false), dev),
            "axe {rot}"
        );
    }
}

#[test]
fn ca4_deviation_egale_a_la_tolerance_acceptee_juste_au_dessus_signalee() {
    let mut sc = scene();
    let mut g = PieceGrain {
        sense: GrainSense::Nap,
        rotation_tolerance_deg: Some(30.0),
        ..PieceGrain::default()
    };
    sc.p.apply(set(sc.piece, g.clone())).unwrap();
    // Pas de placement manuel : rien à contrôler.
    assert!(sc.p.piece_grain_rotation(sc.piece).is_none());
    assert!(!rotation_issue(&sc.p, sc.piece));
    for (rot, exceeded) in [
        (0.0, false),
        (29.999, false),
        (30.0, false),
        (30.000_001, true),
        (30.001, true),
        (90.0, true),
        (330.0, false),
        (329.999, true),
        (180.0, true),
    ] {
        rotate(&mut sc.p, sc.piece, rot);
        let r = sc.p.piece_grain_rotation(sc.piece).unwrap();
        assert_eq!(r.exceeded(), exceeded, "rotation {rot}");
        assert_eq!(rotation_issue(&sc.p, sc.piece), exceeded, "rotation {rot}");
        assert!(near(r.tolerance_deg, 30.0));
    }
    // Aucun refus : le placement est toujours enregistré, la pièce reste valide.
    sc.p.validate().unwrap();
    // Tolérance 180 : toute rotation est acceptée.
    g.rotation_tolerance_deg = Some(180.0);
    sc.p.apply(set(sc.piece, g.clone())).unwrap();
    for rot in [0.0, 45.0, 180.0, 270.5, 359.9] {
        rotate(&mut sc.p, sc.piece, rot);
        assert!(!rotation_issue(&sc.p, sc.piece), "rotation {rot}");
    }
}

#[test]
fn ca4_sans_tolerance_aucune_rotation_libre_et_sans_sens_le_droit_fil_est_un_axe() {
    let mut sc = scene();
    // Tolérance absente = 0 : une rotation d'un millième de degré est déjà signalée.
    rotate(&mut sc.p, sc.piece, 0.0);
    assert!(!rotation_issue(&sc.p, sc.piece));
    rotate(&mut sc.p, sc.piece, 0.001);
    assert!(rotation_issue(&sc.p, sc.piece));
    // Sans sens : 180° laisse le droit-fil parallèle, pas d'avertissement.
    rotate(&mut sc.p, sc.piece, 180.0);
    assert!(!rotation_issue(&sc.p, sc.piece));
    rotate(&mut sc.p, sc.piece, 90.0);
    assert!(rotation_issue(&sc.p, sc.piece));
    // Avec un sens (poil), 180° inverse le poil : signalé.
    sc.p.apply(set(
        sc.piece,
        PieceGrain {
            sense: GrainSense::Nap,
            ..PieceGrain::default()
        },
    ))
    .unwrap();
    rotate(&mut sc.p, sc.piece, 180.0);
    assert!(rotation_issue(&sc.p, sc.piece));
    // Pire déviation parmi les lignes : une ligne « motif » supplémentaire suffit.
    sc.p.apply(set(
        sc.piece,
        PieceGrain {
            extra: vec![extra(
                sc.extra_a,
                GrainDirection::Drawn {},
                GrainSense::Pattern,
            )],
            ..PieceGrain::default()
        },
    ))
    .unwrap();
    assert!(rotation_issue(&sc.p, sc.piece));
    // Remise en placement automatique : plus de contrôle.
    sc.p.apply(Command::SetPiecePlacement {
        piece: sc.piece,
        placement: None,
    })
    .unwrap();
    assert!(!rotation_issue(&sc.p, sc.piece));
    // Le contrôle ne touche ni la construction ni le droit-fil calculé.
    let before = main_line(&sc.p, sc.piece);
    rotate(&mut sc.p, sc.piece, 77.0);
    assert_eq!(main_line(&sc.p, sc.piece), before);
}

#[test]
fn ca4_bornes_de_la_tolerance() {
    let mut sc = scene();
    for ok in [0.0, 0.000_1, 90.0, 180.0] {
        sc.p.apply(set(
            sc.piece,
            PieceGrain {
                rotation_tolerance_deg: Some(ok),
                ..PieceGrain::default()
            },
        ))
        .unwrap_or_else(|e| panic!("{ok} : {e}"));
    }
    // Zéro = absente : rien n'est stocké.
    sc.p.apply(set(
        sc.piece,
        PieceGrain {
            rotation_tolerance_deg: Some(0.0),
            ..PieceGrain::default()
        },
    ))
    .unwrap();
    assert!(sc.p.piece(sc.piece).unwrap().grain.is_none());
    sc.p.apply(set(
        sc.piece,
        PieceGrain {
            rotation_tolerance_deg: Some(-0.0),
            ..PieceGrain::default()
        },
    ))
    .unwrap();
    assert!(sc.p.piece(sc.piece).unwrap().grain.is_none());
    for bad in [
        -0.000_1,
        180.000_001,
        1e9,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        refused(
            &mut sc.p,
            set(
                sc.piece,
                PieceGrain {
                    rotation_tolerance_deg: Some(bad),
                    ..PieceGrain::default()
                },
            ),
        );
    }
}

// ----- CA5 : plusieurs lignes --------------------------------------------------------

#[test]
fn ca5_une_principale_et_des_lignes_supplementaires_chacune_avec_sa_direction() {
    let mut sc = scene();
    let g = PieceGrain {
        direction: par(sc.href),
        sense: GrainSense::Nap,
        extra: vec![
            extra(sc.extra_a, bias(45.0), GrainSense::Pattern),
            extra(sc.extra_b, perp(sc.ob), GrainSense::Unmarked),
        ],
        ..PieceGrain::default()
    };
    sc.p.apply(set(sc.piece, g)).unwrap();
    let lines = sc.p.piece_grain_lines(sc.piece);
    assert_eq!(lines.len(), 3);
    assert!(lines[0].main && !lines[1].main && !lines[2].main);
    assert_eq!(
        lines.iter().map(|l| l.entity).collect::<Vec<_>>(),
        [sc.main, sc.extra_a, sc.extra_b]
    );
    // Principale : parallèle à l'horizontale ; chacune garde sa longueur et son milieu.
    assert_line(&lines[0], pt(30.0, 30.0), pt(70.0, 30.0), 0.0);
    // extra_a : (10 ; 45) → (90 ; 45), longueur 80, milieu (50 ; 45), biais +45°.
    let h = 40.0 * std::f64::consts::FRAC_1_SQRT_2;
    assert_line(
        &lines[1],
        pt(50.0 - h, 45.0 - h),
        pt(50.0 + h, 45.0 + h),
        45.0,
    );
    assert_eq!(lines[1].spec.sense, GrainSense::Pattern);
    // extra_b : longueur 80, milieu (50 ; 15), perpendiculaire à l'oblique (−0,8 ; 0,6).
    assert_line(
        &lines[2],
        pt(50.0 + 32.0, 15.0 - 24.0),
        pt(50.0 - 32.0, 15.0 + 24.0),
        (0.6_f64).atan2(-0.8).to_degrees(),
    );
    assert_eq!(lines[2].spec.sense, GrainSense::Unmarked);
    // Fichier relu à l'identique.
    let text = bytes(&sc.p);
    assert_eq!(bytes(&persist::from_json(&text).unwrap()), text);
    // Le droit-fil principal reste dans Piece.grainline (inchangé).
    assert_eq!(sc.p.piece(sc.piece).unwrap().grainline, Some(sc.main));
    // Les lignes supplémentaires ne comptent pas comme « droit-fil manquant » à elles seules.
    assert!(!sc
        .p
        .piece_issues(sc.piece)
        .contains(&PieceIssue::MissingGrainline));
}

#[test]
fn ca5_borne_de_huit_lignes_supplementaires() {
    let mut sc = scene();
    let mut lines = vec![];
    for k in 0..GRAIN_MAX_EXTRA_LINES {
        let e = grain_seg(
            &mut sc.p,
            sc.s,
            (5.0, 5.0 + k as f64),
            (95.0, 5.0 + k as f64),
        );
        lines.push(extra(e, GrainDirection::Drawn {}, GrainSense::Unmarked));
    }
    let mut g = PieceGrain {
        extra: lines.clone(),
        ..PieceGrain::default()
    };
    sc.p.apply(set(sc.piece, g.clone())).unwrap();
    assert_eq!(
        sc.p.piece_grain_lines(sc.piece).len(),
        1 + GRAIN_MAX_EXTRA_LINES
    );
    // La neuvième est refusée, sans trace.
    let nine = grain_seg(&mut sc.p, sc.s, (5.0, 90.0), (95.0, 90.0));
    g.extra
        .push(extra(nine, GrainDirection::Drawn {}, GrainSense::Unmarked));
    let msg = refused(&mut sc.p, set(sc.piece, g));
    assert!(msg.contains(&GRAIN_MAX_EXTRA_LINES.to_string()), "{msg}");
}

// ----- CA6 : refus sans trace ---------------------------------------------------------

#[test]
fn ca6_entrees_invalides_refusees_sans_trace() {
    let mut sc = scene();
    // Un témoin valide : l'état de départ n'est pas vide, un refus ne doit pas l'effacer.
    sc.p.apply(set(sc.piece, dir(par(sc.href)))).unwrap();
    let other_sheet =
        sc.p.apply(Command::CreateSheet {
            name: "Autre".into(),
        })
        .unwrap()
        .sheet
        .unwrap();
    let (foreign, _) = seg(&mut sc.p, other_sheet, (0.0, 0.0), (10.0, 10.0));
    let foreign_grain = grain_seg(&mut sc.p, other_sheet, (0.0, 50.0), (10.0, 50.0));
    let curve =
        sc.p.apply(Command::AddCurve {
            sheet: sc.s,
            start: PointRef::New(pt(110.0, 100.0)),
            c1: pt(120.0, 120.0),
            c2: pt(130.0, 80.0),
            end: PointRef::New(pt(140.0, 100.0)),
        })
        .unwrap()
        .entities[0];
    sc.p.apply(Command::SetPathRole {
        sheet: sc.s,
        entity: curve,
        role: PathRole::Grainline,
    })
    .unwrap();
    let circle =
        sc.p.apply(Command::AddCircle {
            sheet: sc.s,
            center: PointRef::New(pt(200.0, 200.0)),
            radius_mm: 10.0,
        })
        .unwrap()
        .entities[0];
    let bare =
        sc.p.apply(Command::AddPiece {
            sheet: sc.s,
            name: "Sans droit-fil".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    let ghost = EntityId(987_654);
    let piece = sc.piece;
    let ok_extra = extra(sc.extra_a, GrainDirection::Drawn {}, GrainSense::Unmarked);
    let with = |f: &dyn Fn(&mut PieceGrain)| -> Command {
        let mut g = PieceGrain {
            sense: GrainSense::Nap,
            ..PieceGrain::default()
        };
        f(&mut g);
        set(piece, g)
    };
    let cases: Vec<(&str, Command)> = vec![
        ("pièce inconnue", set(PieceId(987_654), dir(bias(45.0)))),
        ("pièce sans droit-fil", set(bare, dir(bias(45.0)))),
        (
            "tolérance NaN",
            with(&|g| g.rotation_tolerance_deg = Some(f64::NAN)),
        ),
        (
            "tolérance infinie",
            with(&|g| g.rotation_tolerance_deg = Some(f64::INFINITY)),
        ),
        (
            "tolérance négative",
            with(&|g| g.rotation_tolerance_deg = Some(-1.0)),
        ),
        (
            "tolérance > 180",
            with(&|g| g.rotation_tolerance_deg = Some(181.0)),
        ),
        ("biais à 30°", with(&|g| g.direction = bias(30.0))),
        ("biais à 0°", with(&|g| g.direction = bias(0.0))),
        ("biais à 90°", with(&|g| g.direction = bias(90.0))),
        ("biais NaN", with(&|g| g.direction = bias(f64::NAN))),
        ("biais infini", with(&|g| g.direction = bias(f64::INFINITY))),
        ("biais −45,001°", with(&|g| g.direction = bias(-45.001))),
        ("référence absente", with(&|g| g.direction = par(ghost))),
        (
            "référence d'une autre feuille",
            with(&|g| g.direction = perp(foreign)),
        ),
        (
            "référence = cercle (pas de direction)",
            with(&|g| g.direction = par(circle)),
        ),
        (
            "référence = ligne principale",
            with(&|g| g.direction = par(sc.main)),
        ),
        (
            "référence = ligne supplémentaire",
            with(&|g| {
                g.direction = par(sc.extra_a);
                g.extra = vec![ok_extra];
            }),
        ),
        (
            "neuf lignes supplémentaires",
            with(&|g| g.extra = vec![ok_extra; 9]),
        ),
        (
            "ligne supplémentaire en double",
            with(&|g| g.extra = vec![ok_extra, ok_extra]),
        ),
        (
            "ligne supplémentaire = principale",
            with(&|g| {
                g.extra = vec![extra(
                    sc.main,
                    GrainDirection::Drawn {},
                    GrainSense::Unmarked,
                )]
            }),
        ),
        (
            "ligne supplémentaire absente",
            with(&|g| g.extra = vec![extra(ghost, GrainDirection::Drawn {}, GrainSense::Unmarked)]),
        ),
        (
            "ligne supplémentaire d'une autre feuille",
            with(&|g| {
                g.extra = vec![extra(
                    foreign_grain,
                    GrainDirection::Drawn {},
                    GrainSense::Unmarked,
                )]
            }),
        ),
        (
            "ligne supplémentaire sans rôle droit-fil",
            with(&|g| {
                g.extra = vec![extra(
                    sc.href,
                    GrainDirection::Drawn {},
                    GrainSense::Unmarked,
                )]
            }),
        ),
        (
            "ligne supplémentaire courbe",
            with(&|g| g.extra = vec![extra(curve, GrainDirection::Drawn {}, GrainSense::Unmarked)]),
        ),
        (
            "référence de la ligne supplémentaire absente",
            with(&|g| g.extra = vec![extra(sc.extra_a, par(ghost), GrainSense::Unmarked)]),
        ),
        (
            "biais invalide sur une ligne supplémentaire",
            with(&|g| g.extra = vec![extra(sc.extra_a, bias(10.0), GrainSense::Unmarked)]),
        ),
    ];
    let before = sc.p.piece(sc.piece).unwrap().grain.clone();
    for (what, c) in cases {
        let (b, t) = (sc.p.clone(), bytes(&sc.p));
        assert!(sc.p.apply(c).is_err(), "{what} : aurait dû être refusé");
        assert_eq!(sc.p, b, "{what} : refus sans trace (valeur)");
        assert_eq!(bytes(&sc.p), t, "{what} : refus sans trace (octets)");
    }
    assert_eq!(sc.p.piece(sc.piece).unwrap().grain, before);
}

#[test]
fn ca6_les_invariants_tiennent_apres_coup() {
    let mut sc = scene();
    let mut g = dir(par(sc.href));
    g.extra = vec![extra(sc.extra_a, GrainDirection::Drawn {}, GrainSense::Nap)];
    sc.p.apply(set(sc.piece, g)).unwrap();
    // Une ligne de droit-fil ne perd pas son rôle tant qu'elle porte des réglages.
    for e in [sc.main, sc.extra_a] {
        refused(
            &mut sc.p,
            Command::SetPathRole {
                sheet: sc.s,
                entity: e,
                role: PathRole::Construction,
            },
        );
    }
    // Le tracé de référence peut changer de rôle (il reste une référence valide).
    sc.p.apply(Command::SetPathRole {
        sheet: sc.s,
        entity: sc.href,
        role: PathRole::Fold,
    })
    .unwrap();
    // Une référence qui deviendrait de longueur nulle est refusée (déplacer le point).
    let hstart = sc.p.sheet(sc.s).unwrap().entity_points(sc.href).unwrap().0;
    let start_at = sc.p.sheet(sc.s).unwrap().position(hstart).unwrap();
    refused(
        &mut sc.p,
        Command::MovePoint {
            sheet: sc.s,
            point: sc.hend,
            to: start_at,
        },
    );
    // Un droit-fil principal qui n'est plus un segment est refusé quand des réglages existent.
    let curve =
        sc.p.apply(Command::AddCurve {
            sheet: sc.s,
            start: PointRef::New(pt(110.0, 100.0)),
            c1: pt(120.0, 120.0),
            c2: pt(130.0, 80.0),
            end: PointRef::New(pt(140.0, 100.0)),
        })
        .unwrap()
        .entities[0];
    sc.p.apply(Command::SetPathRole {
        sheet: sc.s,
        entity: curve,
        role: PathRole::Grainline,
    })
    .unwrap();
    let outline = sc.p.piece(sc.piece).unwrap().outline.clone();
    refused(
        &mut sc.p,
        Command::SetPieceOutline {
            piece: sc.piece,
            outline: outline.clone(),
            grainline: Some(curve),
        },
    );
    // Changer le droit-fil principal pour un autre segment valide garde les réglages.
    sc.p.apply(Command::SetPieceOutline {
        piece: sc.piece,
        outline,
        grainline: Some(sc.extra_b),
    })
    .unwrap();
    let kept = sc.p.piece(sc.piece).unwrap().grain.clone().unwrap();
    assert_eq!(kept.direction, par(sc.href));
    sc.p.validate().unwrap();
}

#[test]
fn ca6_une_ligne_reglee_n_appartient_qu_a_une_piece() {
    let mut sc = scene();
    let second =
        sc.p.apply(Command::AddPiece {
            sheet: sc.s,
            name: "Second".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    // La seconde pièce a déjà pour droit-fil la ligne principale de la première : refusé.
    sc.p.apply(Command::SetPieceOutline {
        piece: second,
        outline: vec![],
        grainline: Some(sc.main),
    })
    .unwrap();
    let msg = refused(&mut sc.p, set(sc.piece, dir(bias(45.0))));
    assert!(msg.contains("Second"), "{msg}");
    // Une ligne supplémentaire ne peut pas non plus être le droit-fil de l'autre pièce.
    sc.p.apply(Command::SetPieceOutline {
        piece: second,
        outline: vec![],
        grainline: Some(sc.extra_a),
    })
    .unwrap();
    let mut g = dir(bias(45.0));
    g.extra = vec![extra(
        sc.extra_a,
        GrainDirection::Drawn {},
        GrainSense::Unmarked,
    )];
    refused(&mut sc.p, set(sc.piece, g));
    // Réglages posés d'abord : donner ensuite cette ligne à l'autre pièce est refusé.
    sc.p.apply(Command::SetPieceOutline {
        piece: second,
        outline: vec![],
        grainline: Some(sc.extra_b),
    })
    .unwrap();
    sc.p.apply(set(sc.piece, dir(bias(45.0)))).unwrap();
    refused(
        &mut sc.p,
        Command::SetPieceOutline {
            piece: second,
            outline: vec![],
            grainline: Some(sc.main),
        },
    );
}

// ----- CA7 : une étape annulable -------------------------------------------------------

#[test]
fn ca7_chaque_commande_est_une_etape_annulable_et_un_no_op_n_en_est_pas_une() {
    let sc = scene();
    let (piece, href, extra_a) = (sc.piece, sc.href, sc.extra_a);
    let mut doc = Document::new(sc.p);
    let initial = bytes(doc.project());
    // Pas de réglage : mettre « tout par défaut » ne crée aucune étape.
    doc.execute(set(piece, PieceGrain::default())).unwrap();
    assert!(!doc.can_undo());
    // Une commande = une étape, annulée d'un coup.
    let g = PieceGrain {
        direction: par(href),
        sense: GrainSense::Pattern,
        reversed: true,
        rotation_tolerance_deg: Some(15.0),
        extra: vec![extra(extra_a, bias(-45.0), GrainSense::Nap)],
    };
    doc.execute(set(piece, g.clone())).unwrap();
    assert!(doc.can_undo());
    let applied = bytes(doc.project());
    assert_ne!(applied, initial);
    // Même valeur : aucune étape de plus.
    doc.execute(set(piece, g.clone())).unwrap();
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), initial);
    assert!(!doc.can_undo());
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), applied);
    // Un refus ne laisse aucune étape.
    let n_before = bytes(doc.project());
    assert!(doc.execute(set(piece, dir(bias(7.0)))).is_err());
    assert_eq!(bytes(doc.project()), n_before);
    doc.undo();
    assert!(!doc.can_undo(), "un refus n'a pas créé d'étape");
}

// ----- CA8 : suppression du tracé de référence --------------------------------------------

fn full_grain(sc: &Sc) -> PieceGrain {
    PieceGrain {
        direction: par(sc.href),
        sense: GrainSense::Nap,
        rotation_tolerance_deg: Some(20.0),
        extra: vec![
            extra(sc.extra_a, perp(sc.href), GrainSense::Pattern),
            extra(sc.extra_b, perp(sc.ob), GrainSense::Unmarked),
        ],
        ..PieceGrain::default()
    }
}

#[test]
fn ca8_tracé_de_reference_supprime_retour_a_la_direction_dessinee_avec_remarque() {
    let mut sc = scene();
    sc.p.apply(set(sc.piece, full_grain(&sc))).unwrap();
    let o =
        sc.p.apply(Command::Delete {
            sheet: sc.s,
            points: vec![],
            entities: vec![sc.href],
        })
        .unwrap();
    sc.p.validate().unwrap();
    let g = sc.p.piece(sc.piece).unwrap().grain.clone().unwrap();
    // Principale et première ligne supplémentaire reviennent à la direction dessinée.
    assert!(g.direction.is_drawn());
    assert!(g.extra[0].direction.is_drawn());
    // Sens, tolérance, et ligne qui suit un autre tracé : inchangés.
    assert_eq!(g.sense, GrainSense::Nap);
    assert_eq!(g.rotation_tolerance_deg, Some(20.0));
    assert_eq!(g.extra[0].sense, GrainSense::Pattern);
    assert_eq!(g.extra[1].direction, perp(sc.ob));
    let said = o
        .notes
        .iter()
        .filter(|n| n.contains("tracé supprimé") && n.contains("direction"))
        .count();
    assert_eq!(said, 2, "{:?}", o.notes);
    // Aucune référence morte : tout se lit.
    assert_eq!(sc.p.piece_grain_lines(sc.piece).len(), 3);
    assert_line(
        &main_line(&sc.p, sc.piece),
        pt(50.0, 10.0),
        pt(50.0, 50.0),
        90.0,
    );
}

#[test]
fn ca8_supprimer_un_point_du_trace_de_reference_agit_de_meme() {
    let mut sc = scene();
    sc.p.apply(set(sc.piece, dir(par(sc.href)))).unwrap();
    let o =
        sc.p.apply(Command::Delete {
            sheet: sc.s,
            points: vec![sc.hend],
            entities: vec![sc.href],
        })
        .unwrap();
    sc.p.validate().unwrap();
    assert!(sc.p.sheet(sc.s).unwrap().entity(sc.href).is_err());
    assert!(
        sc.p.piece(sc.piece).unwrap().grain.is_none(),
        "{:?}",
        o.notes
    );
    assert!(
        o.notes.iter().any(|n| n.contains("tracé supprimé")),
        "{:?}",
        o.notes
    );
}

#[test]
fn ca8_ligne_supplementaire_supprimee_est_retiree_avec_remarque() {
    let mut sc = scene();
    sc.p.apply(set(sc.piece, full_grain(&sc))).unwrap();
    let o =
        sc.p.apply(Command::Delete {
            sheet: sc.s,
            points: vec![],
            entities: vec![sc.extra_a],
        })
        .unwrap();
    sc.p.validate().unwrap();
    let g = sc.p.piece(sc.piece).unwrap().grain.clone().unwrap();
    assert_eq!(g.extra.len(), 1);
    assert_eq!(g.extra[0].entity, sc.extra_b);
    assert!(
        o.notes
            .iter()
            .any(|n| n.contains("ligne de droit-fil supplémentaire") && n.contains("retirée")),
        "{:?}",
        o.notes
    );
}

#[test]
fn ca8_droit_fil_principal_supprime_les_reglages_sont_retires_avec_remarque() {
    let mut sc = scene();
    sc.p.apply(set(sc.piece, full_grain(&sc))).unwrap();
    let o =
        sc.p.apply(Command::Delete {
            sheet: sc.s,
            points: vec![],
            entities: vec![sc.main],
        })
        .unwrap();
    sc.p.validate().unwrap();
    let p = sc.p.piece(sc.piece).unwrap();
    assert!(p.grainline.is_none() && p.grain.is_none());
    assert!(
        o.notes
            .iter()
            .any(|n| n.contains("réglages du droit-fil") && n.contains("retirés")),
        "{:?}",
        o.notes
    );
    assert!(sc
        .p
        .piece_issues(sc.piece)
        .contains(&PieceIssue::MissingGrainline));
    // Même chose quand on retire le droit-fil par SetPieceOutline.
    let mut sc = scene();
    sc.p.apply(set(sc.piece, full_grain(&sc))).unwrap();
    let outline = sc.p.piece(sc.piece).unwrap().outline.clone();
    let o =
        sc.p.apply(Command::SetPieceOutline {
            piece: sc.piece,
            outline,
            grainline: None,
        })
        .unwrap();
    assert!(sc.p.piece(sc.piece).unwrap().grain.is_none());
    assert!(o.notes.iter().any(|n| n.contains("réglages du droit-fil")));
}

#[test]
fn ca8_la_suppression_est_une_etape_annulable_qui_rend_la_reference() {
    let sc = scene();
    let (s, piece, href) = (sc.s, sc.piece, sc.href);
    let g = full_grain(&sc);
    let mut doc = Document::new(sc.p);
    doc.execute(set(piece, g.clone())).unwrap();
    let before = bytes(doc.project());
    doc.execute(Command::Delete {
        sheet: s,
        points: vec![],
        entities: vec![href],
    })
    .unwrap();
    assert_ne!(bytes(doc.project()), before);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
    assert_eq!(doc.project().piece(piece).unwrap().grain.as_ref(), Some(&g));
}

#[test]
fn ca8_couper_le_trace_de_reference_garde_la_reference_sur_le_premier_troncon() {
    let mut sc = scene();
    sc.p.apply(set(sc.piece, dir(perp(sc.href)))).unwrap();
    sc.p.apply(Command::SplitPath {
        sheet: sc.s,
        entity: sc.href,
        at: pt(10.0, 100.0),
    })
    .unwrap();
    sc.p.validate().unwrap();
    // Un segment coupé garde sa direction : la flèche est inchangée.
    assert_line(
        &main_line(&sc.p, sc.piece),
        pt(50.0, 10.0),
        pt(50.0, 50.0),
        90.0,
    );
    assert_eq!(
        sc.p.piece(sc.piece)
            .unwrap()
            .grain
            .as_ref()
            .unwrap()
            .direction,
        perp(sc.href)
    );
}

// ----- CA9 : cohérence avec les autres fonctions ------------------------------------------

#[test]
fn ca9_copie_de_feuille_remappe_lignes_et_references() {
    let mut sc = scene();
    sc.p.apply(set(sc.piece, full_grain(&sc))).unwrap();
    let before = sc.p.clone();
    let o = sc.p.apply(Command::DuplicateSheet { sheet: sc.s }).unwrap();
    sc.p.validate().unwrap();
    let copy = o.pieces[0];
    assert_eq!(
        sc.p.piece(sc.piece).unwrap(),
        before.piece(sc.piece).unwrap(),
        "l'original n'a pas changé"
    );
    let dup = sc.p.piece(copy).unwrap();
    let PieceLink::Linked { sheet: new_sheet } = dup.link else {
        panic!("copie liée");
    };
    let sh = sc.p.sheet(new_sheet).unwrap();
    let g = dup.grain.clone().unwrap();
    assert_eq!(g.sense, GrainSense::Nap);
    assert_eq!(g.rotation_tolerance_deg, Some(20.0));
    assert_eq!(g.extra.len(), 2);
    for e in [dup.grainline.unwrap(), g.extra[0].entity, g.extra[1].entity] {
        assert!(sh.entity(e).is_ok(), "ligne de la copie");
        assert!(![sc.main, sc.extra_a, sc.extra_b].contains(&e));
    }
    for r in [
        g.direction.reference().unwrap(),
        g.extra[0].direction.reference().unwrap(),
        g.extra[1].direction.reference().unwrap(),
    ] {
        assert!(sh.entity(r).is_ok(), "référence de la copie");
        assert!(![sc.href, sc.ob].contains(&r));
    }
    // Mêmes flèches ; déplacer une référence de la copie ne bouge pas l'original.
    let (a, b) = (
        sc.p.piece_grain_lines(sc.piece),
        sc.p.piece_grain_lines(copy),
    );
    assert_eq!(a.len(), b.len());
    for (x, y) in a.iter().zip(&b) {
        assert!(at_near(x.from, y.from) && at_near(x.to, y.to));
    }
    let r = g.direction.reference().unwrap();
    let end = sh.entity_points(r).unwrap().1;
    sc.p.apply(Command::MovePoint {
        sheet: new_sheet,
        point: end,
        to: pt(0.0, 140.0),
    })
    .unwrap();
    assert_eq!(sc.p.piece_grain_lines(sc.piece), a);
    assert!(near(sc.p.piece_grain_lines(copy)[0].direction_deg, 90.0));
    // Import dans un autre projet : mêmes remappages.
    let mut other = Project::new("Autre projet");
    let imported = other.apply(before.import_command(sc.s).unwrap()).unwrap();
    other.validate().unwrap();
    let ip = other.piece(imported.pieces[0]).unwrap();
    assert_eq!(ip.grain.as_ref().unwrap().extra.len(), 2);
    assert_eq!(other.piece_grain_lines(ip.id).len(), 3);
}

/// Pièce 100 × 60 dont le droit-fil (poil, tolérance 20°, parallèle à `href`) est dans la
/// moitié droite d'une coupe à x = 30 ; une ligne supplémentaire est dans la moitié gauche
/// (perdue), une autre dans la moitié droite (reprise).
fn split_scene() -> (Sc, EntityId, EntityId, EntityId) {
    let mut sc = scene();
    let p_main = grain_seg(&mut sc.p, sc.s, (70.0, 10.0), (70.0, 50.0));
    let left = grain_seg(&mut sc.p, sc.s, (5.0, 45.0), (25.0, 45.0));
    let right = grain_seg(&mut sc.p, sc.s, (60.0, 20.0), (90.0, 20.0));
    let outline = sc.p.piece(sc.piece).unwrap().outline.clone();
    sc.p.apply(Command::SetPieceOutline {
        piece: sc.piece,
        outline,
        grainline: Some(p_main),
    })
    .unwrap();
    sc.p.apply(set(
        sc.piece,
        PieceGrain {
            direction: par(sc.href),
            sense: GrainSense::Nap,
            rotation_tolerance_deg: Some(20.0),
            extra: vec![
                extra(left, GrainDirection::Drawn {}, GrainSense::Pattern),
                extra(right, perp(sc.href), GrainSense::Unmarked),
            ],
            ..PieceGrain::default()
        },
    ))
    .unwrap();
    (sc, p_main, left, right)
}

#[test]
fn ca9_separer_reporte_la_fleche_calculee_dans_la_moitie_qui_la_contient() {
    let (mut sc, _, _, _) = split_scene();
    let before = sc.p.piece_grain_lines(sc.piece);
    let o =
        sc.p.apply(Command::SplitPiece {
            piece: sc.piece,
            a: pt(30.0, -10.0),
            b: pt(30.0, 70.0),
        })
        .unwrap();
    sc.p.validate().unwrap();
    let (l, r) = (o.pieces[0], o.pieces[1]);
    // Gauche : sans droit-fil (la coupe est à x = 30, le droit-fil est à x = 70).
    assert!(sc.p.piece(l).unwrap().grainline.is_none());
    assert!(sc.p.piece(l).unwrap().grain.is_none());
    let rp = sc.p.piece(r).unwrap();
    let g = rp.grain.clone().expect("réglages repris");
    assert_eq!(g.sense, GrainSense::Nap);
    assert_eq!(g.rotation_tolerance_deg, Some(20.0));
    assert!(g.direction.is_drawn() && !g.reversed, "direction figée");
    assert_eq!(
        g.extra.len(),
        1,
        "une ligne perdue, une reprise : {:?}",
        o.notes
    );
    assert_eq!(g.extra[0].sense, GrainSense::Unmarked);
    let lines = sc.p.piece_grain_lines(r);
    assert_eq!(lines.len(), 2);
    // La flèche calculée (parallèle à l'horizontale, milieu (70 ; 30), longueur 40) est reprise.
    assert!(at_near(lines[0].from, before[0].from) && at_near(lines[0].to, before[0].to));
    assert!(at_near(lines[1].from, before[2].from) && at_near(lines[1].to, before[2].to));
    // Chaque perte et la figeance sont dites.
    assert!(
        o.notes
            .iter()
            .any(|n| n.contains("figée") && n.contains("droit-fil")),
        "{:?}",
        o.notes
    );
    assert!(
        o.notes
            .iter()
            .any(|n| n.contains("ligne(s) de droit-fil supplémentaire(s)")),
        "{:?}",
        o.notes
    );
    // Les lignes reprises sont de vrais tracés de rôle droit-fil de la feuille de la pièce.
    let sheet = sc.p.piece_sheet(r).unwrap();
    for l in &lines {
        assert_eq!(sheet.entity(l.entity).unwrap().role, PathRole::Grainline);
    }
    // La pièce d'origine n'existe plus ; la construction non plus n'a pas changé.
    assert!(sc.p.piece(sc.piece).is_err());
    assert!(sc.p.sheet(sc.s).unwrap().entity(sc.href).is_ok());
}

#[test]
fn ca9_assembler_rend_la_fleche_a_sa_place() {
    let (mut sc, _, _, _) = split_scene();
    let before = sc.p.piece_grain_lines(sc.piece);
    let o =
        sc.p.apply(Command::SplitPiece {
            piece: sc.piece,
            a: pt(30.0, -10.0),
            b: pt(30.0, 70.0),
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
    let g =
        sc.p.piece(merged)
            .unwrap()
            .grain
            .clone()
            .expect("réglages repris");
    assert_eq!(g.sense, GrainSense::Nap);
    assert_eq!(g.rotation_tolerance_deg, Some(20.0));
    let lines = sc.p.piece_grain_lines(merged);
    assert_eq!(lines.len(), 2, "{:?}", j.notes);
    assert!(distance_mm(lines[0].from, before[0].from) < 1e-6);
    assert!(distance_mm(lines[0].to, before[0].to) < 1e-6);
    assert!(distance_mm(lines[1].from, before[2].from) < 1e-6);
}

#[test]
fn ca9_deplier_ne_reprend_pas_le_droit_fil_et_le_dit() {
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
    let main = grain_seg(&mut p, s, (30.0, 10.0), (30.0, 30.0));
    let outline = p.piece(piece).unwrap().outline.clone();
    p.apply(Command::SetPieceOutline {
        piece,
        outline,
        grainline: Some(main),
    })
    .unwrap();
    p.apply(set(piece, dir(bias(-45.0)))).unwrap();
    let before = p.piece(piece).unwrap().clone();
    let u = p
        .apply(Command::UnfoldPiece {
            piece,
            fold,
            name: None,
        })
        .unwrap();
    p.validate().unwrap();
    let unfolded = p.piece(u.pieces[0]).unwrap();
    assert!(unfolded.grain.is_none() && unfolded.grainline.is_none());
    assert_eq!(
        *p.piece(piece).unwrap(),
        before,
        "la source garde ses réglages"
    );
    assert!(
        u.notes.iter().any(|n| n.contains("droit-fil")),
        "{:?}",
        u.notes
    );
    // Une dépliée est obsolète-ou-à-jour sans jamais porter de réglage orphelin.
    assert!(p
        .piece_issues(u.pieces[0])
        .contains(&PieceIssue::MissingGrainline));
}

#[test]
fn ca9_piece_derivee_ne_porte_pas_de_reglage_et_la_mere_les_garde() {
    let mut sc = scene();
    sc.p.apply(set(sc.piece, full_grain(&sc))).unwrap();
    let before = sc.p.piece(sc.piece).unwrap().grain.clone();
    let d =
        sc.p.apply(Command::CreateDerivedPiece {
            mother: sc.piece,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset { distance_mm: -5.0 },
            name: None,
        })
        .unwrap()
        .pieces[0];
    sc.p.validate().unwrap();
    assert!(sc.p.piece(d).unwrap().grain.is_none());
    assert!(sc.p.piece_grain_lines(d).is_empty());
    assert_eq!(sc.p.piece(sc.piece).unwrap().grain, before);
    // La mère change : la dérivée est régénérée, les réglages de la mère n'en souffrent pas.
    let corner = {
        let sh = sc.p.sheet(sc.s).unwrap();
        sh.points
            .iter()
            .find(|p| sh.position(p.id).is_ok_and(|a| at_near(a, pt(100.0, 60.0))))
            .unwrap()
            .id
    };
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: corner,
        to: pt(110.0, 60.0),
    })
    .unwrap();
    sc.p.validate().unwrap();
    assert_eq!(sc.p.piece(sc.piece).unwrap().grain, before);
    // Une dérivée dont le lien est cassé peut recevoir un droit-fil avec ses réglages.
    sc.p.apply(Command::BreakDerivedLink { piece: d }).unwrap();
    let dsheet = sc.p.piece_sheet(d).unwrap().id;
    let line = grain_seg(&mut sc.p, dsheet, (40.0, 20.0), (40.0, 40.0));
    let outline = sc.p.piece(d).unwrap().outline.clone();
    sc.p.apply(Command::SetPieceOutline {
        piece: d,
        outline,
        grainline: Some(line),
    })
    .unwrap();
    sc.p.apply(set(d, dir(bias(45.0)))).unwrap();
    assert_eq!(sc.p.piece_grain_lines(d).len(), 1);
}

#[test]
fn ca9_le_plan_de_pieces_ne_touche_ni_la_construction_ni_les_reglages() {
    let mut sc = scene();
    sc.p.apply(set(sc.piece, full_grain(&sc))).unwrap();
    let stored = sc.p.piece(sc.piece).unwrap().grain.clone();
    let lines = sc.p.piece_grain_lines(sc.piece);
    rotate(&mut sc.p, sc.piece, 37.0);
    assert_eq!(sc.p.piece(sc.piece).unwrap().grain, stored);
    assert_eq!(
        sc.p.piece_grain_lines(sc.piece),
        lines,
        "placement = vue seulement"
    );
    assert_eq!(sc.p.plan_layout().len(), 1);
    // Une rotation qui dépasse la tolérance (20°) est signalée, jamais refusée.
    assert!(rotation_issue(&sc.p, sc.piece));
    sc.p.validate().unwrap();
    let card = sc.p.piece_card(sc.piece).unwrap();
    assert!(card.issues.contains(&PieceIssue::GrainRotationExceeded));
    assert_eq!(
        PieceIssue::GrainRotationExceeded.key(),
        "grain_rotation_exceeded"
    );
    let r = sc.p.piece_grain_rotation(sc.piece).unwrap();
    assert!(near(r.deviation_deg, 37.0) && near(r.tolerance_deg, 20.0));
}

#[test]
fn ca9_piece_detachee_lignes_et_references_sur_sa_feuille() {
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
    let main = grain_seg(&mut sc.p, dsheet, (50.0, 10.0), (50.0, 50.0));
    let (reference, _) = seg(&mut sc.p, dsheet, (0.0, 100.0), (40.0, 100.0));
    let outline = sc.p.piece(det).unwrap().outline.clone();
    sc.p.apply(Command::SetPieceOutline {
        piece: det,
        outline,
        grainline: Some(main),
    })
    .unwrap();
    sc.p.apply(set(det, dir(par(reference)))).unwrap();
    assert_line(&main_line(&sc.p, det), pt(30.0, 30.0), pt(70.0, 30.0), 0.0);
    // Une référence de la feuille de la pièce d'origine est refusée.
    refused(&mut sc.p, set(det, dir(par(sc.href))));
    // Les lignes ne sont exportées que sur la feuille de la pièce.
    assert_eq!(sc.p.sheet_grain_lines(dsheet).len(), 1);
    assert!(sc.p.sheet_grain_lines(sc.s).is_empty());
    sc.p.validate().unwrap();
}

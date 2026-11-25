//! US-06.4 — boutons et boutonnières : N éléments répartis régulièrement entre deux points,
//! taille, orientation, recul du bord (contrôle affiché, jamais une contrainte). Un groupe de
//! tests par critère d'acceptation : CA1 modèle et fichier (schéma 3, octets inchangés),
//! CA2 répartition exacte calculée à la main, CA3 orientations, CA4 recul du bord (avertissement
//! calculé à la main, contours à segments, arc et Bézier), CA5 refus sans trace, CA6 une étape
//! annulable, CA7 liaison à la construction, CA8 cohérence (copie de feuille, séparer, assembler,
//! déplier, plan, dérivées, pièce détachée). L'export (SVG, PDF) est mesuré dans
//! `crates/pf-export/tests/boutons.rs`.

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::piece_buttons::{
    distribute, normalize_degrees, BUTTONHOLE_MAX_LENGTH_MM, BUTTONHOLE_MAX_WIDTH_MM,
    BUTTONHOLE_MIN_LENGTH_MM, BUTTONHOLE_MIN_WIDTH_MM, BUTTON_MAX_COUNT, BUTTON_MAX_DIAMETER_MM,
    BUTTON_MAX_EDGE_MARGIN_MM, BUTTON_MAX_GROUPS_PER_PIECE, BUTTON_MIN_DIAMETER_MM,
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

/// Pièce « Devant » : rectangle 100 × 60 mm (0 ; 0) → (100 ; 60), liée à la feuille.
struct Scene {
    p: Project,
    s: SheetId,
    piece: PieceId,
}

fn rectangle_piece(name: &str) -> Scene {
    let mut p = Project::new("Boutons");
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
            name: name.into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    Scene { p, s, piece }
}

fn scene() -> Scene {
    rectangle_piece("Devant")
}

fn free_point(p: &mut Project, s: SheetId, x: f64, y: f64) -> PointId {
    p.apply(Command::AddPoint {
        sheet: s,
        at: pt(x, y),
    })
    .unwrap()
    .points[0]
}

fn button(d: f64) -> ButtonElement {
    ButtonElement::Button { diameter_mm: d }
}

fn hole(length_mm: f64, width_mm: f64, orientation: ButtonholeOrientation) -> ButtonElement {
    ButtonElement::Buttonhole {
        length_mm,
        width_mm,
        orientation,
    }
}

const H: ButtonholeOrientation = ButtonholeOrientation::Horizontal {};
const V: ButtonholeOrientation = ButtonholeOrientation::Vertical {};
const PERP: ButtonholeOrientation = ButtonholeOrientation::Perpendicular {};

fn angle(degrees: f64) -> ButtonholeOrientation {
    ButtonholeOrientation::Angle { degrees }
}

fn add(
    piece: PieceId,
    from: PointId,
    to: PointId,
    count: u32,
    element: ButtonElement,
    edge_margin_mm: f64,
) -> Command {
    Command::AddButtonGroup {
        piece,
        from,
        to,
        count,
        element,
        edge_margin_mm,
    }
}

fn add_ok(
    p: &mut Project,
    piece: PieceId,
    (from, to): (PointId, PointId),
    count: u32,
    element: ButtonElement,
    margin: f64,
) -> ButtonGroupId {
    p.apply(add(piece, from, to, count, element, margin))
        .unwrap()
        .button_groups[0]
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

fn centers(p: &Project, piece: PieceId, group: usize) -> Vec<PointMm> {
    p.piece_buttons(piece)[group]
        .elements
        .iter()
        .map(|e| e.center)
        .collect()
}

fn assert_points(got: &[PointMm], want: &[(f64, f64)]) {
    assert_eq!(got.len(), want.len(), "{got:?} / {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert!(at_near(*g, pt(w.0, w.1)), "{g:?} ≠ {w:?}");
    }
}

fn angle_of(p: &Project, piece: PieceId, group: usize) -> f64 {
    match p.piece_buttons(piece)[group].elements[0].shape {
        ButtonShape::Buttonhole { angle_deg, .. } => angle_deg,
        ButtonShape::Button { .. } => panic!("un bouton n'a pas d'angle"),
    }
}

// ----- CA1 : modèle et fichier ------------------------------------------------------

#[test]
fn ca1_champ_absent_octets_inchanges() {
    for name in FIXTURES {
        let first = bytes(&fixture(name));
        let second = bytes(&persist::from_json(&first).unwrap());
        assert_eq!(first, second, "{name}");
        assert!(!first.contains("\"buttons\""), "{name}");
        assert!(fixture(name).pieces.iter().all(|p| p.buttons.is_empty()));
    }
    // Les fichiers livrés : relus puis réécrits, identiques à l'octet, sans la clé.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut seen = 0;
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if !path.to_string_lossy().ends_with(".pforge.json") {
            continue;
        }
        let shipped = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n");
        assert_eq!(
            shipped,
            persist::to_json(&persist::from_json(&shipped).unwrap()).unwrap(),
            "{}",
            path.display()
        );
        assert!(!shipped.contains("\"buttons\""), "{}", path.display());
        seen += 1;
    }
    assert!(seen >= 3, "exemples introuvables");
    // Une pièce sans groupe n'écrit rien ; après retrait du dernier groupe non plus.
    let mut sc = scene();
    let (a, b) = (
        free_point(&mut sc.p, sc.s, 10.0, 30.0),
        free_point(&mut sc.p, sc.s, 90.0, 30.0),
    );
    let text = bytes(&sc.p);
    assert!(!text.contains("\"buttons\""));
    assert_eq!(text, bytes(&persist::from_json(&text).unwrap()));
    let g = add_ok(&mut sc.p, sc.piece, (a, b), 3, button(12.0), 5.0);
    assert!(bytes(&sc.p).contains("\"buttons\""));
    sc.p.apply(Command::RemoveButtonGroup {
        piece: sc.piece,
        group: g,
    })
    .unwrap();
    assert!(!bytes(&sc.p).contains("\"buttons\""));
}

#[test]
fn ca1_champ_present_relu_a_l_identique_schema_3() {
    let mut sc = scene();
    let a = free_point(&mut sc.p, sc.s, 10.0, 30.0);
    let b = free_point(&mut sc.p, sc.s, 90.0, 30.0);
    let g1 = add_ok(&mut sc.p, sc.piece, (a, b), 5, button(12.5), 8.0);
    let g2 = add_ok(
        &mut sc.p,
        sc.piece,
        (b, a),
        2,
        hole(20.0, 3.5, angle(-30.0)),
        0.0,
    );
    let g3 = add_ok(&mut sc.p, sc.piece, (a, b), 1, hole(18.0, 3.0, PERP), 2.5);
    let text = bytes(&sc.p);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["schema_version"], 6, "schéma 4 (US-09.1)");
    let list = v["pieces"][0]["buttons"].as_array().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(
        list[0],
        json!({"id": g1.0, "from": a.0, "to": b.0, "count": 5,
               "element": {"type": "button", "diameter_mm": 12.5},
               "edge_margin_mm": 8.0})
    );
    // L'angle libre est enregistré normalisé : −30° devient 330°.
    assert_eq!(
        list[1],
        json!({"id": g2.0, "from": b.0, "to": a.0, "count": 2,
               "element": {"type": "buttonhole", "length_mm": 20.0, "width_mm": 3.5,
                           "orientation": {"type": "angle", "degrees": 330.0}},
               "edge_margin_mm": 0.0})
    );
    // Orientation sans paramètre : une variante à champs vides, seule la clé « type ».
    assert_eq!(
        list[2]["element"]["orientation"],
        json!({"type": "perpendicular"})
    );
    let _ = g3;
    // Relecture : identique en valeur et à l'octet.
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, sc.p);
    assert_eq!(bytes(&back), text);
    back.validate().unwrap();
}

#[test]
fn ca1_fichier_invalide_refuse_a_l_ouverture_sans_perte_silencieuse() {
    let mut sc = scene();
    let a = free_point(&mut sc.p, sc.s, 10.0, 30.0);
    let b = free_point(&mut sc.p, sc.s, 90.0, 30.0);
    add_ok(&mut sc.p, sc.piece, (a, b), 3, button(10.0), 5.0);
    add_ok(
        &mut sc.p,
        sc.piece,
        (a, b),
        2,
        hole(20.0, 4.0, angle(45.0)),
        5.0,
    );
    let good: Value = serde_json::from_str(&bytes(&sc.p)).unwrap();
    let mutate = |f: &dyn Fn(&mut Value)| -> Result<Project, CoreError> {
        let mut v = good.clone();
        f(&mut v);
        persist::from_json(&v.to_string())
    };
    assert!(mutate(&|_| {}).is_ok(), "témoin");
    fn g0(v: &mut Value) -> &mut Value {
        &mut v["pieces"][0]["buttons"][0]
    }
    fn g1(v: &mut Value) -> &mut Value {
        &mut v["pieces"][0]["buttons"][1]
    }
    type Mutation = Box<dyn Fn(&mut Value)>;
    let cases: Vec<(&str, Mutation)> = vec![
        ("nombre nul", Box::new(|v| g0(v)["count"] = json!(0))),
        (
            "nombre trop grand",
            Box::new(|v| g0(v)["count"] = json!(101)),
        ),
        ("nombre négatif", Box::new(|v| g0(v)["count"] = json!(-2))),
        ("nombre décimal", Box::new(|v| g0(v)["count"] = json!(2.5))),
        ("nombre texte", Box::new(|v| g0(v)["count"] = json!("3"))),
        (
            "diamètre nul",
            Box::new(|v| g0(v)["element"]["diameter_mm"] = json!(0.0)),
        ),
        (
            "diamètre trop grand",
            Box::new(|v| g0(v)["element"]["diameter_mm"] = json!(1e9)),
        ),
        (
            "diamètre texte",
            Box::new(|v| g0(v)["element"]["diameter_mm"] = json!("12")),
        ),
        (
            "longueur nulle",
            Box::new(|v| g1(v)["element"]["length_mm"] = json!(0.0)),
        ),
        (
            "largeur nulle",
            Box::new(|v| g1(v)["element"]["width_mm"] = json!(0.0)),
        ),
        (
            "largeur plus grande que la longueur",
            Box::new(|v| g1(v)["element"]["width_mm"] = json!(30.0)),
        ),
        (
            "angle hors de [0 ; 360[",
            Box::new(|v| g1(v)["element"]["orientation"]["degrees"] = json!(400.0)),
        ),
        (
            "angle négatif non normalisé",
            Box::new(|v| g1(v)["element"]["orientation"]["degrees"] = json!(-10.0)),
        ),
        (
            "angle 360 exactement",
            Box::new(|v| g1(v)["element"]["orientation"]["degrees"] = json!(360.0)),
        ),
        (
            "recul négatif",
            Box::new(|v| g0(v)["edge_margin_mm"] = json!(-1.0)),
        ),
        (
            "recul trop grand",
            Box::new(|v| g0(v)["edge_margin_mm"] = json!(1e6)),
        ),
        ("point absent", Box::new(|v| g0(v)["from"] = json!(987_654))),
        (
            "deux points identiques",
            Box::new(|v| {
                let from = g0(v)["from"].clone();
                g0(v)["to"] = from;
            }),
        ),
        (
            "élément inconnu",
            Box::new(|v| g0(v)["element"]["type"] = json!("rivet")),
        ),
        (
            "orientation inconnue",
            Box::new(|v| g1(v)["element"]["orientation"] = json!({"type": "oblique"})),
        ),
        (
            "clé en trop sur une orientation sans paramètre",
            Box::new(|v| {
                g1(v)["element"]["orientation"] = json!({"type": "horizontal", "degrees": 3.0})
            }),
        ),
        (
            "clé en trop sur le groupe",
            Box::new(|v| g0(v)["couleur"] = json!("rouge")),
        ),
        (
            "clé en trop sur l'élément",
            Box::new(|v| g0(v)["element"]["couleur"] = json!("rouge")),
        ),
        (
            "identifiant en double",
            Box::new(|v| {
                let id = g0(v)["id"].clone();
                g1(v)["id"] = id;
            }),
        ),
        (
            "identifiant au-delà du compteur",
            Box::new(|v| g1(v)["id"] = json!(99_999_999)),
        ),
        (
            "groupes pas une liste",
            Box::new(|v| v["pieces"][0]["buttons"] = json!(3)),
        ),
        (
            "recul absent",
            Box::new(|v| {
                g0(v).as_object_mut().unwrap().remove("edge_margin_mm");
            }),
        ),
    ];
    for (what, f) in cases {
        let err = mutate(&*f);
        assert!(err.is_err(), "{what} : le fichier aurait dû être refusé");
        assert!(!err.unwrap_err().to_string().is_empty(), "{what}");
    }
}

// ----- CA2 : répartition exacte ---------------------------------------------------------

#[test]
fn ca2_repartition_sur_un_segment_horizontal() {
    let mut sc = scene();
    let a = free_point(&mut sc.p, sc.s, 10.0, 30.0);
    let b = free_point(&mut sc.p, sc.s, 90.0, 30.0);
    // Espacement = 80 / (N − 1), le premier élément en A, le dernier exactement en B.
    let table: [(u32, &[(f64, f64)]); 4] = [
        (1, &[(10.0, 30.0)]),
        (2, &[(10.0, 30.0), (90.0, 30.0)]),
        (3, &[(10.0, 30.0), (50.0, 30.0), (90.0, 30.0)]),
        (
            5,
            &[
                (10.0, 30.0),
                (30.0, 30.0),
                (50.0, 30.0),
                (70.0, 30.0),
                (90.0, 30.0),
            ],
        ),
    ];
    for (n, want) in table {
        let g = add_ok(&mut sc.p, sc.piece, (a, b), n, button(10.0), 0.0);
        let idx = sc.p.piece(sc.piece).unwrap().buttons.len() - 1;
        assert_points(&centers(&sc.p, sc.piece, idx), want);
        let r = &sc.p.piece_buttons(sc.piece)[idx];
        assert_eq!(r.group.id, g);
        assert_eq!(r.group.count, n);
        let spacing = if n > 1 { 80.0 / f64::from(n - 1) } else { 0.0 };
        assert!((r.spacing_mm - spacing).abs() < 1e-9, "N={n}");
        assert_eq!(r.elements.len(), n as usize);
        assert!(r.elements.iter().enumerate().all(|(i, e)| e.index == i));
    }
    // Le dernier élément est exactement au second point (aucune erreur d'arrondi).
    assert_eq!(centers(&sc.p, sc.piece, 3)[4], pt(90.0, 30.0));
}

#[test]
fn ca2_repartition_sur_un_segment_vertical_et_dans_le_sens_inverse() {
    let mut sc = scene();
    let a = free_point(&mut sc.p, sc.s, 50.0, 10.0);
    let b = free_point(&mut sc.p, sc.s, 50.0, 50.0);
    add_ok(&mut sc.p, sc.piece, (a, b), 5, button(8.0), 0.0);
    assert_points(
        &centers(&sc.p, sc.piece, 0),
        &[
            (50.0, 10.0),
            (50.0, 20.0),
            (50.0, 30.0),
            (50.0, 40.0),
            (50.0, 50.0),
        ],
    );
    // Sens inverse : le premier élément est au premier point choisi (B).
    add_ok(&mut sc.p, sc.piece, (b, a), 3, button(8.0), 0.0);
    assert_points(
        &centers(&sc.p, sc.piece, 1),
        &[(50.0, 50.0), (50.0, 30.0), (50.0, 10.0)],
    );
    // Un seul élément : au premier point seulement.
    add_ok(&mut sc.p, sc.piece, (b, a), 1, button(8.0), 0.0);
    assert_points(&centers(&sc.p, sc.piece, 2), &[(50.0, 50.0)]);
}

#[test]
fn ca2_repartition_sur_un_segment_oblique() {
    let mut sc = scene();
    // (10 ; 10) → (50 ; 40) : vecteur (40 ; 30), distance 50 (triangle 3-4-5).
    let a = free_point(&mut sc.p, sc.s, 10.0, 10.0);
    let b = free_point(&mut sc.p, sc.s, 50.0, 40.0);
    add_ok(&mut sc.p, sc.piece, (a, b), 5, button(6.0), 0.0);
    // Pas de 1/4 : (10 ; 7,5) → espacement 12,5 mm.
    assert_points(
        &centers(&sc.p, sc.piece, 0),
        &[
            (10.0, 10.0),
            (20.0, 17.5),
            (30.0, 25.0),
            (40.0, 32.5),
            (50.0, 40.0),
        ],
    );
    let r = &sc.p.piece_buttons(sc.piece)[0];
    assert!((r.spacing_mm - 12.5).abs() < 1e-12);
    for w in r.elements.windows(2) {
        assert!((distance_mm(w[0].center, w[1].center) - 12.5).abs() < 1e-9);
    }
    add_ok(&mut sc.p, sc.piece, (a, b), 3, button(6.0), 0.0);
    assert_points(
        &centers(&sc.p, sc.piece, 1),
        &[(10.0, 10.0), (30.0, 25.0), (50.0, 40.0)],
    );
    add_ok(&mut sc.p, sc.piece, (a, b), 2, button(6.0), 0.0);
    assert_points(&centers(&sc.p, sc.piece, 2), &[(10.0, 10.0), (50.0, 40.0)]);
    add_ok(&mut sc.p, sc.piece, (a, b), 1, button(6.0), 0.0);
    assert_points(&centers(&sc.p, sc.piece, 3), &[(10.0, 10.0)]);
}

#[test]
fn ca2_la_fonction_de_repartition_pure() {
    assert_eq!(
        distribute(pt(0.0, 0.0), pt(10.0, 0.0), 1),
        vec![pt(0.0, 0.0)]
    );
    assert_eq!(
        distribute(pt(0.0, 0.0), pt(10.0, 0.0), 2),
        vec![pt(0.0, 0.0), pt(10.0, 0.0)]
    );
    assert_eq!(
        distribute(pt(-5.0, 2.0), pt(5.0, 2.0), 3),
        vec![pt(-5.0, 2.0), pt(0.0, 2.0), pt(5.0, 2.0)]
    );
    let hundred = distribute(pt(0.0, 0.0), pt(99.0, 0.0), BUTTON_MAX_COUNT);
    assert_eq!(hundred.len(), 100);
    assert_eq!(hundred[99], pt(99.0, 0.0));
    assert!((hundred[50].x - 50.0).abs() < 1e-9);
}

#[test]
fn ca2_les_boutons_et_les_boutonnieres_ont_les_dimensions_demandees() {
    let mut sc = scene();
    let a = free_point(&mut sc.p, sc.s, 20.0, 30.0);
    let b = free_point(&mut sc.p, sc.s, 80.0, 30.0);
    add_ok(&mut sc.p, sc.piece, (a, b), 2, button(11.5), 0.0);
    add_ok(&mut sc.p, sc.piece, (a, b), 3, hole(22.0, 3.5, H), 0.0);
    let groups = sc.p.piece_buttons(sc.piece);
    for e in &groups[0].elements {
        assert_eq!(e.shape, ButtonShape::Button { diameter_mm: 11.5 });
    }
    for e in &groups[1].elements {
        assert_eq!(
            e.shape,
            ButtonShape::Buttonhole {
                length_mm: 22.0,
                width_mm: 3.5,
                angle_deg: 0.0
            }
        );
    }
    // Une boutonnière aussi large que longue est un cercle : acceptée.
    add_ok(&mut sc.p, sc.piece, (a, b), 1, hole(6.0, 6.0, H), 0.0);
    sc.p.validate().unwrap();
}

// ----- CA3 : orientations ---------------------------------------------------------------

#[test]
fn ca3_orientations_horizontale_verticale_perpendiculaire_et_angle_libre() {
    let mut sc = scene();
    let (left, right) = (
        free_point(&mut sc.p, sc.s, 10.0, 30.0),
        free_point(&mut sc.p, sc.s, 90.0, 30.0),
    );
    let (low, high) = (
        free_point(&mut sc.p, sc.s, 50.0, 10.0),
        free_point(&mut sc.p, sc.s, 50.0, 50.0),
    );
    let (o, q) = (
        free_point(&mut sc.p, sc.s, 10.0, 10.0),
        free_point(&mut sc.p, sc.s, 50.0, 40.0),
    );
    let ends = [(left, right), (low, high), (o, q)];
    // Horizontale : 0° ; verticale : 90°, quel que soit l'axe du groupe.
    for (i, pair) in ends.iter().enumerate() {
        add_ok(&mut sc.p, sc.piece, *pair, 2, hole(20.0, 4.0, H), 0.0);
        add_ok(&mut sc.p, sc.piece, *pair, 2, hole(20.0, 4.0, V), 0.0);
        assert_eq!(angle_of(&sc.p, sc.piece, 2 * i), 0.0);
        assert_eq!(angle_of(&sc.p, sc.piece, 2 * i + 1), 90.0);
    }
    // Perpendiculaire à l'axe = axe + 90° : axe horizontal (0°) → 90° ; axe vertical (90°) → 180° ;
    // axe (40 ; 30) → atan2(30 ; 40) + 90° = 126,8699°.
    let perp: Vec<usize> = ends
        .iter()
        .map(|pair| {
            add_ok(&mut sc.p, sc.piece, *pair, 2, hole(20.0, 4.0, PERP), 0.0);
            sc.p.piece(sc.piece).unwrap().buttons.len() - 1
        })
        .collect();
    assert!((angle_of(&sc.p, sc.piece, perp[0]) - 90.0).abs() < 1e-9);
    assert!((angle_of(&sc.p, sc.piece, perp[1]) - 180.0).abs() < 1e-9);
    let want = (30.0f64).atan2(40.0).to_degrees() + 90.0;
    assert!((angle_of(&sc.p, sc.piece, perp[2]) - want).abs() < 1e-9);
    assert!((want - 126.8699).abs() < 1e-4);
    // La même orientation s'applique à tous les éléments du groupe.
    for e in &sc.p.piece_buttons(sc.piece)[perp[2]].elements {
        let ButtonShape::Buttonhole { angle_deg, .. } = e.shape else {
            panic!("boutonnière");
        };
        assert!((angle_deg - want).abs() < 1e-9);
    }
    // L'axe suit la construction : déplacer un point change l'angle « perpendiculaire ».
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: q,
        to: pt(10.0, 40.0),
    })
    .unwrap();
    assert!((angle_of(&sc.p, sc.piece, perp[2]) - 180.0).abs() < 1e-9);
}

#[test]
fn ca3_angle_libre_normalise_dans_0_360() {
    let mut sc = scene();
    let a = free_point(&mut sc.p, sc.s, 10.0, 30.0);
    let b = free_point(&mut sc.p, sc.s, 90.0, 30.0);
    let table = [
        (30.0, 30.0),
        (0.0, 0.0),
        (-90.0, 270.0),
        (-0.0, 0.0),
        (360.0, 0.0),
        (450.0, 90.0),
        (-720.5, 359.5),
        (359.999, 359.999),
        (-1e-20, 0.0),
    ];
    for (given, stored) in table {
        let g = add_ok(
            &mut sc.p,
            sc.piece,
            (a, b),
            2,
            hole(20.0, 4.0, angle(given)),
            0.0,
        );
        let found =
            sc.p.piece(sc.piece)
                .unwrap()
                .buttons
                .iter()
                .find(|x| x.id == g)
                .unwrap()
                .element;
        let ButtonElement::Buttonhole { orientation, .. } = found else {
            panic!("boutonnière");
        };
        let ButtonholeOrientation::Angle { degrees } = orientation else {
            panic!("angle libre");
        };
        assert!(
            (degrees - stored).abs() < 1e-9,
            "{given} → {degrees}, attendu {stored}"
        );
        assert!((0.0..360.0).contains(&degrees));
        assert!(!degrees.is_sign_negative(), "jamais −0 : {given}");
    }
    sc.p.validate().unwrap();
    // L'écriture est identique après relecture (angle déjà normalisé).
    let text = bytes(&sc.p);
    assert_eq!(text, bytes(&persist::from_json(&text).unwrap()));
    assert_eq!(normalize_degrees(-90.0), 270.0);
    assert_eq!(normalize_degrees(720.0), 0.0);
}

// ----- CA4 : recul du bord -----------------------------------------------------------

fn too_close(p: &Project, piece: PieceId) -> usize {
    p.piece_buttons(piece)
        .iter()
        .flat_map(|g| &g.elements)
        .filter(|e| e.too_close)
        .count()
}

fn issue_count(p: &Project, piece: PieceId) -> Option<usize> {
    p.piece_issues(piece).iter().find_map(|i| match i {
        PieceIssue::ButtonsTooCloseToEdge { count } => Some(*count),
        _ => None,
    })
}

#[test]
fn ca4_bouton_distance_au_bord_calculee_a_la_main() {
    let mut sc = scene();
    // Bouton Ø 10 (rayon 5) centré en (10 ; 30) : le bord gauche (x = 0) est à 10 mm du
    // centre, donc à 5 mm du bord du bouton ; les autres côtés sont plus loin.
    let c = free_point(&mut sc.p, sc.s, 10.0, 30.0);
    let d = free_point(&mut sc.p, sc.s, 40.0, 30.0);
    add_ok(&mut sc.p, sc.piece, (c, d), 1, button(10.0), 5.0);
    let e = sc.p.piece_buttons(sc.piece)[0].elements[0];
    assert!((e.clearance_mm.unwrap() - 5.0).abs() < 1e-9, "{e:?}");
    assert!(!e.too_close, "recul exactement respecté");
    assert_eq!(issue_count(&sc.p, sc.piece), None);
    // Recul demandé plus grand : avertissement, la pièce reste valide et sauvegardable.
    let g = sc.p.piece(sc.piece).unwrap().buttons[0].id;
    let set = |margin: f64| Command::SetButtonGroup {
        piece: sc.piece,
        group: g,
        from: c,
        to: d,
        count: 1,
        element: button(10.0),
        edge_margin_mm: margin,
    };
    sc.p.apply(set(5.001)).unwrap();
    assert_eq!(too_close(&sc.p, sc.piece), 1);
    assert_eq!(issue_count(&sc.p, sc.piece), Some(1));
    sc.p.validate().unwrap();
    persist::from_json(&bytes(&sc.p)).unwrap();
    sc.p.apply(set(4.999)).unwrap();
    assert_eq!(too_close(&sc.p, sc.piece), 0);
    assert_eq!(issue_count(&sc.p, sc.piece), None);
    // Un recul nul n'avertit que si le bouton touche ou traverse le bord : (3 ; 30) Ø 10 →
    // le bord du bouton dépasse de 2 mm.
    let near = free_point(&mut sc.p, sc.s, 3.0, 30.0);
    let g2 = add_ok(&mut sc.p, sc.piece, (near, d), 1, button(10.0), 0.0);
    let e = sc.p.piece_buttons(sc.piece)[1].elements[0];
    assert!((e.clearance_mm.unwrap() + 2.0).abs() < 1e-9, "{e:?}");
    assert!(e.too_close);
    assert_eq!(issue_count(&sc.p, sc.piece), Some(1));
    // Exactement tangent au bord : clearance 0, recul 0 → pas d'avertissement.
    let tangent = free_point(&mut sc.p, sc.s, 5.0, 30.0);
    sc.p.apply(Command::SetButtonGroup {
        piece: sc.piece,
        group: g2,
        from: tangent,
        to: d,
        count: 1,
        element: button(10.0),
        edge_margin_mm: 0.0,
    })
    .unwrap();
    let e = sc.p.piece_buttons(sc.piece)[1].elements[0];
    assert!(e.clearance_mm.unwrap().abs() < 1e-9);
    assert!(!e.too_close);
}

#[test]
fn ca4_boutonniere_tient_compte_de_sa_longueur_et_de_son_orientation() {
    let mut sc = scene();
    // Boutonnières 20 × 4 réparties de (50 ; 10) à (50 ; 50) : centres (50 ; 10), (50 ; 30),
    // (50 ; 50). Horizontales : âme de (42 ; y) à (58 ; y), demi-largeur 2.
    //  – centre y = 10 : bord du bas à 10 mm de l'âme → recul réel 8 ;
    //  – centre y = 30 : plus proche côté à 30 mm → 28 ; centre y = 50 : 8.
    let low = free_point(&mut sc.p, sc.s, 50.0, 10.0);
    let high = free_point(&mut sc.p, sc.s, 50.0, 50.0);
    let horizontal = add_ok(&mut sc.p, sc.piece, (low, high), 3, hole(20.0, 4.0, H), 8.0);
    let clearances: Vec<f64> = sc.p.piece_buttons(sc.piece)[0]
        .elements
        .iter()
        .map(|e| e.clearance_mm.unwrap())
        .collect();
    for (got, want) in clearances.iter().zip([8.0, 28.0, 8.0]) {
        assert!((got - want).abs() < 1e-9, "{clearances:?}");
    }
    assert_eq!(
        too_close(&sc.p, sc.piece),
        0,
        "recul 8 respecté à 1e-9 près"
    );
    let set = |margin: f64, o: ButtonholeOrientation| Command::SetButtonGroup {
        piece: sc.piece,
        group: horizontal,
        from: low,
        to: high,
        count: 3,
        element: hole(20.0, 4.0, o),
        edge_margin_mm: margin,
    };
    sc.p.apply(set(8.5, H)).unwrap();
    assert_eq!(
        too_close(&sc.p, sc.piece),
        2,
        "les deux extrémités du groupe"
    );
    assert_eq!(issue_count(&sc.p, sc.piece), Some(2));
    // Verticales : âme de (50 ; y − 8) à (50 ; y + 8) : le bord du bas touche à y = 10 − 8 − 2 = 0
    // (clearance 0) et le bord du haut aussi à y = 50 + 8 + 2 = 60.
    sc.p.apply(set(1.0, V)).unwrap();
    let clear: Vec<f64> = sc.p.piece_buttons(sc.piece)[0]
        .elements
        .iter()
        .map(|e| e.clearance_mm.unwrap())
        .collect();
    for (got, want) in clear.iter().zip([0.0, 20.0, 0.0]) {
        assert!((got - want).abs() < 1e-9, "{clear:?}");
    }
    assert_eq!(too_close(&sc.p, sc.piece), 2);
    // Perpendiculaire à l'axe vertical = horizontale : mêmes valeurs que la première série.
    sc.p.apply(set(8.5, PERP)).unwrap();
    assert_eq!(too_close(&sc.p, sc.piece), 2);
    sc.p.apply(set(8.0, PERP)).unwrap();
    assert_eq!(too_close(&sc.p, sc.piece), 0);
    // Une orientation à 90° plus loin change le résultat : le contrôle dépend de l'orientation.
    sc.p.apply(set(8.0, angle(90.0))).unwrap();
    assert_eq!(too_close(&sc.p, sc.piece), 2);
}

#[test]
fn ca4_element_hors_de_la_piece_ou_qui_traverse_le_contour_est_signale() {
    let mut sc = scene();
    let out_a = free_point(&mut sc.p, sc.s, 150.0, 30.0);
    let out_b = free_point(&mut sc.p, sc.s, 170.0, 30.0);
    add_ok(&mut sc.p, sc.piece, (out_a, out_b), 2, button(6.0), 0.0);
    // Centres dehors : clearance négative, avertissement même avec un recul nul.
    let e = sc.p.piece_buttons(sc.piece)[0].elements[0];
    assert!(
        (e.clearance_mm.unwrap() + (50.0 + 3.0)).abs() < 1e-9,
        "{e:?}"
    );
    assert_eq!(issue_count(&sc.p, sc.piece), Some(2));
    // Une boutonnière dont l'âme traverse le bord droit (x = 100) : clearance −demi-largeur.
    let cross_a = free_point(&mut sc.p, sc.s, 95.0, 20.0);
    let cross_b = free_point(&mut sc.p, sc.s, 96.0, 20.0);
    add_ok(
        &mut sc.p,
        sc.piece,
        (cross_a, cross_b),
        1,
        hole(30.0, 4.0, H),
        0.0,
    );
    let e = sc.p.piece_buttons(sc.piece)[1].elements[0];
    assert!((e.clearance_mm.unwrap() + 2.0).abs() < 1e-9, "{e:?}");
    assert!(e.too_close);
    assert_eq!(issue_count(&sc.p, sc.piece), Some(3));
    // Jamais un refus ni un déplacement : le projet reste valide, les points n'ont pas bougé.
    sc.p.validate().unwrap();
    assert!(at_near(centers(&sc.p, sc.piece, 0)[0], pt(150.0, 30.0)));
    assert_eq!(
        PieceIssue::ButtonsTooCloseToEdge { count: 3 }.key(),
        "buttons_too_close_to_edge"
    );
    assert_eq!(sc.p.piece_card(sc.piece).unwrap().issues.len(), {
        sc.p.piece_issues(sc.piece).len()
    });
}

#[test]
fn ca4_pas_de_jugement_sans_contour_ferme() {
    let mut sc = scene();
    let a = free_point(&mut sc.p, sc.s, 10.0, 30.0);
    let b = free_point(&mut sc.p, sc.s, 40.0, 30.0);
    add_ok(&mut sc.p, sc.piece, (a, b), 2, button(10.0), 1000.0);
    assert_eq!(too_close(&sc.p, sc.piece), 2);
    // Pièce-étiquette (aucun contour) : positions calculées, aucun jugement sur le recul.
    let label =
        sc.p.apply(Command::AddPiece {
            sheet: sc.s,
            name: "Étiquette".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    add_ok(&mut sc.p, label, (a, b), 2, button(10.0), 1000.0);
    let r = &sc.p.piece_buttons(label)[0];
    assert_eq!(r.elements.len(), 2);
    assert!(r
        .elements
        .iter()
        .all(|e| e.clearance_mm.is_none() && !e.too_close));
    assert_eq!(issue_count(&sc.p, label), None);
}

#[test]
fn ca4_contour_en_arc_cercle_complet() {
    let mut p = Project::new("Rond");
    let s = p.sheets[0].id;
    p.apply(Command::AddCircle {
        sheet: s,
        center: PointRef::New(pt(0.0, 0.0)),
        radius_mm: 50.0,
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(0.0, 0.0) },
            name: "Rond".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let a = free_point(&mut p, s, 30.0, 0.0);
    let b = free_point(&mut p, s, 30.0, 10.0);
    // Bouton Ø 10 en (30 ; 0) : 20 mm du bord du cercle (rayon 50), 15 mm du bord du bouton.
    add_ok(&mut p, piece, (a, b), 1, button(10.0), 15.0);
    let e = p.piece_buttons(piece)[0].elements[0];
    assert!((e.clearance_mm.unwrap() - 15.0).abs() < 1e-9, "{e:?}");
    assert!(!e.too_close);
    // Boutonnière horizontale 20 × 4 en (30 ; 0) : âme de (22 ; 0) à (38 ; 0), bout à 12 mm du bord,
    // soit 10 mm de recul réel.
    add_ok(&mut p, piece, (a, b), 1, hole(20.0, 4.0, H), 10.0);
    let e = p.piece_buttons(piece)[1].elements[0];
    assert!((e.clearance_mm.unwrap() - 10.0).abs() < 1e-9, "{e:?}");
    assert!(!e.too_close);
    // Même boutonnière en (30 ; 10), verticale : âme de (30 ; 2) à (30 ; 18), le point le plus
    // proche du cercle est (30 ; 18) à 50 − √(900 + 324) mm = 50 − 34,9857… ; recul réel =
    // cette distance − 2.
    let d = free_point(&mut p, s, 30.0, 20.0);
    add_ok(&mut p, piece, (b, d), 1, hole(20.0, 4.0, V), 0.0);
    let want = 50.0 - (30.0f64 * 30.0 + 18.0 * 18.0).sqrt() - 2.0;
    let e = p.piece_buttons(piece)[2].elements[0];
    assert!(
        (e.clearance_mm.unwrap() - want).abs() < 1e-9,
        "{e:?} / {want}"
    );
}

#[test]
fn ca4_contour_en_courbe_de_bezier_comparee_a_un_echantillonnage_independant() {
    let mut p = Project::new("Bézier");
    let s = p.sheets[0].id;
    // Courbe (0 ; 0) → (100 ; 0) bombée vers le haut, fermée par la base.
    let (c1, c2) = (pt(0.0, 60.0), pt(100.0, 60.0));
    let o = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(pt(0.0, 0.0)),
            c1,
            c2,
            end: PointRef::New(pt(100.0, 0.0)),
        })
        .unwrap();
    let (pa, pb) = (o.points[0], o.points[1]);
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::Existing(pb),
        to: PointRef::Existing(pa),
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(50.0, 10.0) },
            name: "Dôme".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let curve = |t: f64| {
        let u = 1.0 - t;
        let (x0, y0, x1, y1, x2, y2, x3, y3) = (0.0, 0.0, c1.x, c1.y, c2.x, c2.y, 100.0, 0.0);
        pt(
            u * u * u * x0 + 3.0 * u * u * t * x1 + 3.0 * u * t * t * x2 + t * t * t * x3,
            u * u * u * y0 + 3.0 * u * u * t * y1 + 3.0 * u * t * t * y2 + t * t * t * y3,
        )
    };
    let oracle = |centre: PointMm| -> f64 {
        // Distance de `centre` à la courbe par échantillonnage très fin (indépendant du cœur).
        (0..=200_000)
            .map(|i| distance_mm(centre, curve(f64::from(i) / 200_000.0)))
            .fold(f64::INFINITY, f64::min)
    };
    // Bouton Ø 8 sous la courbe, au-dessus de la base : le plus proche est la courbe.
    for (x, y) in [(50.0, 38.0), (20.0, 25.0), (85.0, 20.0)] {
        let a = free_point(&mut p, s, x, y);
        let b = free_point(&mut p, s, x + 1.0, y);
        add_ok(&mut p, piece, (a, b), 1, button(8.0), 0.0);
        let idx = p.piece(piece).unwrap().buttons.len() - 1;
        let e = p.piece_buttons(piece)[idx].elements[0];
        let want = oracle(pt(x, y)).min(y) - 4.0;
        assert!(
            (e.clearance_mm.unwrap() - want).abs() < 1e-4,
            "({x} ; {y}) : {e:?} / {want}"
        );
    }
    // Et près de la base : c'est le segment (distance exacte y − rayon).
    let a = free_point(&mut p, s, 50.0, 6.0);
    let b = free_point(&mut p, s, 51.0, 6.0);
    add_ok(&mut p, piece, (a, b), 1, button(8.0), 0.0);
    let e = p.piece_buttons(piece)[3].elements[0];
    assert!((e.clearance_mm.unwrap() - 2.0).abs() < 1e-9, "{e:?}");
}

// ----- CA5 : refus sans trace -------------------------------------------------------------

#[test]
fn ca5_entrees_invalides_refusees_sans_trace() {
    let mut sc = scene();
    let a = free_point(&mut sc.p, sc.s, 10.0, 30.0);
    let b = free_point(&mut sc.p, sc.s, 90.0, 30.0);
    let g = add_ok(&mut sc.p, sc.piece, (a, b), 3, button(10.0), 2.0);
    let h = add_ok(&mut sc.p, sc.piece, (a, b), 3, hole(20.0, 4.0, H), 2.0);
    let (p, piece) = (&mut sc.p, sc.piece);
    let set = |group: ButtonGroupId, count: u32, element: ButtonElement, margin: f64| {
        Command::SetButtonGroup {
            piece,
            group,
            from: a,
            to: b,
            count,
            element,
            edge_margin_mm: margin,
        }
    };
    // Nombre d'éléments : 0, au-delà de la borne documentée, énorme.
    for n in [0, BUTTON_MAX_COUNT + 1, 1_000, u32::MAX] {
        refused(p, add(piece, a, b, n, button(10.0), 0.0));
        refused(p, set(g, n, button(10.0), 0.0));
    }
    // Diamètre : nul, négatif, non fini, sous le minimum, au-dessus du maximum.
    for d in [
        0.0,
        -1.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        BUTTON_MIN_DIAMETER_MM / 2.0,
        BUTTON_MAX_DIAMETER_MM + 0.001,
        1e300,
    ] {
        refused(p, add(piece, a, b, 2, button(d), 0.0));
        refused(p, set(g, 2, button(d), 0.0));
    }
    // Longueur et largeur de boutonnière.
    for v in [0.0, -3.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e300] {
        refused(p, add(piece, a, b, 2, hole(v, 3.0, H), 0.0));
        refused(p, add(piece, a, b, 2, hole(20.0, v, H), 0.0));
        refused(p, set(h, 2, hole(v, 3.0, H), 0.0));
        refused(p, set(h, 2, hole(20.0, v, H), 0.0));
    }
    for (l, w) in [
        (BUTTONHOLE_MIN_LENGTH_MM / 2.0, BUTTONHOLE_MIN_WIDTH_MM),
        (BUTTONHOLE_MAX_LENGTH_MM + 0.001, 3.0),
        (20.0, BUTTONHOLE_MIN_WIDTH_MM / 2.0),
        (60.0, BUTTONHOLE_MAX_WIDTH_MM + 0.001),
        (10.0, 12.0),
        (10.0, 10.001),
    ] {
        refused(p, add(piece, a, b, 2, hole(l, w, H), 0.0));
    }
    // Angle non fini.
    for d in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        refused(p, add(piece, a, b, 2, hole(20.0, 4.0, angle(d)), 0.0));
        refused(p, set(h, 2, hole(20.0, 4.0, angle(d)), 0.0));
    }
    // Recul : négatif, non fini, au-delà de la borne.
    for m in [
        -0.000_001,
        -5.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        BUTTON_MAX_EDGE_MARGIN_MM + 0.001,
        1e300,
    ] {
        refused(p, add(piece, a, b, 2, button(10.0), m));
        refused(p, set(g, 2, button(10.0), m));
    }
    // Points : identiques, absents, pièce absente, groupe absent.
    let bogus = PointId(9_999_999);
    refused(p, add(piece, a, a, 2, button(10.0), 0.0));
    refused(p, add(piece, a, bogus, 2, button(10.0), 0.0));
    refused(p, add(piece, bogus, b, 2, button(10.0), 0.0));
    refused(p, add(PieceId(9_999_999), a, b, 2, button(10.0), 0.0));
    refused(
        p,
        Command::SetButtonGroup {
            piece,
            group: g,
            from: a,
            to: a,
            count: 2,
            element: button(10.0),
            edge_margin_mm: 0.0,
        },
    );
    refused(
        p,
        Command::SetButtonGroup {
            piece,
            group: g,
            from: bogus,
            to: b,
            count: 2,
            element: button(10.0),
            edge_margin_mm: 0.0,
        },
    );
    refused(
        p,
        Command::SetButtonGroup {
            piece,
            group: ButtonGroupId(9_999_999),
            from: a,
            to: b,
            count: 2,
            element: button(10.0),
            edge_margin_mm: 0.0,
        },
    );
    refused(
        p,
        Command::SetButtonGroup {
            piece: PieceId(9_999_999),
            group: g,
            from: a,
            to: b,
            count: 2,
            element: button(10.0),
            edge_margin_mm: 0.0,
        },
    );
    refused(
        p,
        Command::RemoveButtonGroup {
            piece,
            group: ButtonGroupId(9_999_999),
        },
    );
    refused(
        p,
        Command::RemoveButtonGroup {
            piece: PieceId(9_999_999),
            group: g,
        },
    );
    // Un identifiant d'une autre sorte d'objet n'est pas un groupe.
    refused(
        p,
        Command::RemoveButtonGroup {
            piece,
            group: ButtonGroupId(piece.0),
        },
    );
    // Le groupe d'une pièce n'est pas modifiable par le biais d'une autre pièce.
    let other_piece = p
        .apply(Command::AddPiece {
            sheet: sc.s,
            name: "Autre".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    refused(
        p,
        Command::RemoveButtonGroup {
            piece: other_piece,
            group: g,
        },
    );
    refused(
        p,
        Command::SetButtonGroup {
            piece: other_piece,
            group: g,
            from: a,
            to: b,
            count: 2,
            element: button(10.0),
            edge_margin_mm: 0.0,
        },
    );
    // Un point d'une autre feuille n'est pas permis pour une pièce liée.
    let other = p
        .apply(Command::CreateSheet {
            name: "Autre feuille".into(),
        })
        .unwrap()
        .sheet
        .unwrap();
    let foreign = free_point(p, other, 5.0, 5.0);
    refused(p, add(piece, a, foreign, 2, button(10.0), 0.0));
    refused(p, add(piece, foreign, b, 2, button(10.0), 0.0));
    refused(
        p,
        Command::SetButtonGroup {
            piece,
            group: g,
            from: a,
            to: foreign,
            count: 2,
            element: button(10.0),
            edge_margin_mm: 0.0,
        },
    );
    // Le refus d'un lot est total.
    refused(
        p,
        Command::Batch {
            commands: vec![
                add(piece, a, b, 2, button(10.0), 0.0),
                add(piece, a, b, 0, button(10.0), 0.0),
            ],
        },
    );
    // Pièce détachée sans contour : pas de feuille où chercher les points.
    let det = p
        .apply(Command::ExtractPiece {
            sheet: sc.s,
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
    let msg = refused(p, add(det, a, b, 2, button(10.0), 0.0));
    assert!(msg.contains("contour"), "{msg}");
    // Les valeurs aux bornes exactes sont acceptées (et le projet reste valide).
    p.apply(add(
        piece,
        a,
        b,
        BUTTON_MAX_COUNT,
        button(BUTTON_MIN_DIAMETER_MM),
        0.0,
    ))
    .unwrap();
    p.apply(add(
        piece,
        a,
        b,
        1,
        button(BUTTON_MAX_DIAMETER_MM),
        BUTTON_MAX_EDGE_MARGIN_MM,
    ))
    .unwrap();
    p.apply(add(
        piece,
        a,
        b,
        1,
        hole(BUTTONHOLE_MAX_LENGTH_MM, BUTTONHOLE_MAX_WIDTH_MM, H),
        0.0,
    ))
    .unwrap();
    p.apply(add(
        piece,
        a,
        b,
        1,
        hole(BUTTONHOLE_MIN_LENGTH_MM, BUTTONHOLE_MIN_WIDTH_MM, H),
        0.0,
    ))
    .unwrap();
    p.validate().unwrap();
}

#[test]
fn ca5_trop_de_groupes_sur_une_piece() {
    let mut sc = scene();
    let a = free_point(&mut sc.p, sc.s, 10.0, 30.0);
    let b = free_point(&mut sc.p, sc.s, 90.0, 30.0);
    let batch: Vec<Command> = (0..BUTTON_MAX_GROUPS_PER_PIECE)
        .map(|_| add(sc.piece, a, b, 2, button(5.0), 0.0))
        .collect();
    sc.p.apply(Command::Batch { commands: batch }).unwrap();
    assert_eq!(
        sc.p.piece(sc.piece).unwrap().buttons.len(),
        BUTTON_MAX_GROUPS_PER_PIECE
    );
    let msg = refused(&mut sc.p, add(sc.piece, a, b, 2, button(5.0), 0.0));
    assert!(msg.contains("100"), "{msg}");
    sc.p.validate().unwrap();
}

#[test]
fn ca5_deux_points_confondus_sont_refuses_et_deplacer_un_point_pour_les_confondre_aussi() {
    let mut sc = scene();
    let a = free_point(&mut sc.p, sc.s, 10.0, 30.0);
    let b = free_point(&mut sc.p, sc.s, 90.0, 30.0);
    // Deux points distincts au même endroit (construits à la main : les commandes
    // fusionnent les points confondus).
    let twin = PointId(sc.p.next_id);
    sc.p.next_id += 1;
    sc.p.sheets[0].points.push(Point {
        id: twin,
        label: "Z9".into(),
        definition: PointDefinition::Free { x: 10.0, y: 30.0 },
        role: PointRole::Construction,
        layer: None,
    });
    sc.p.validate().unwrap();
    let msg = refused(&mut sc.p, add(sc.piece, a, twin, 3, button(8.0), 0.0));
    assert!(msg.contains("confondus"), "{msg}");
    // Déplacer le point d'un groupe existant sur l'autre : refusé, sans trace.
    add_ok(&mut sc.p, sc.piece, (a, b), 3, button(8.0), 0.0);
    refused(
        &mut sc.p,
        Command::MovePoint {
            sheet: sc.s,
            point: b,
            to: pt(10.0, 30.0),
        },
    );
    // À 1 mm l'un de l'autre, c'est permis (seule la confusion est un refus).
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: b,
        to: pt(11.0, 30.0),
    })
    .unwrap();
    assert_points(
        &centers(&sc.p, sc.piece, 0),
        &[(10.0, 30.0), (10.5, 30.0), (11.0, 30.0)],
    );
}

// ----- CA6 : une étape annulable --------------------------------------------------------

#[test]
fn ca6_chaque_commande_est_une_etape_annulable_et_un_no_op_n_en_est_pas_une() {
    let mut sc = scene();
    let a = free_point(&mut sc.p, sc.s, 10.0, 30.0);
    let b = free_point(&mut sc.p, sc.s, 90.0, 30.0);
    let c = free_point(&mut sc.p, sc.s, 50.0, 50.0);
    let mut doc = Document::new(sc.p.clone());
    let original = bytes(doc.project());
    let g = doc
        .execute(add(sc.piece, a, b, 4, hole(20.0, 4.0, angle(-90.0)), 3.0))
        .unwrap()
        .button_groups[0];
    let after_add = bytes(doc.project());
    assert_ne!(after_add, original);
    let set = |from: PointId, count: u32, element: ButtonElement| Command::SetButtonGroup {
        piece: sc.piece,
        group: g,
        from,
        to: b,
        count,
        element,
        edge_margin_mm: 3.0,
    };
    // Édition identique : aucune étape ; angle équivalent (−90° = 270°) : aucune étape non plus.
    doc.execute(set(a, 4, hole(20.0, 4.0, angle(-90.0))))
        .unwrap();
    doc.execute(set(a, 4, hole(20.0, 4.0, angle(270.0))))
        .unwrap();
    doc.execute(set(a, 4, hole(20.0, 4.0, angle(630.0))))
        .unwrap();
    assert_eq!(bytes(doc.project()), after_add);
    // Édition réelle : une étape (nombre, taille, orientation, points).
    doc.execute(set(a, 6, hole(20.0, 4.0, angle(270.0))))
        .unwrap();
    doc.execute(set(a, 6, hole(24.0, 5.0, V))).unwrap();
    doc.execute(set(c, 6, hole(24.0, 5.0, PERP))).unwrap();
    doc.execute(Command::RemoveButtonGroup {
        piece: sc.piece,
        group: g,
    })
    .unwrap();
    // Cinq étapes exactement (ajout, trois éditions, retrait) : les no-op n'en ont pas créé.
    let mut steps = 0;
    while doc.undo() {
        steps += 1;
    }
    assert_eq!(steps, 5);
    assert_eq!(bytes(doc.project()), original);
    assert!(!doc.can_undo(), "les no-op n'ont créé aucune étape");
    while doc.redo() {}
    assert!(doc.project().piece(sc.piece).unwrap().buttons.is_empty());
    // Un lot de plusieurs groupes = une seule étape.
    let mut doc = Document::new(sc.p.clone());
    doc.execute(Command::Batch {
        commands: vec![
            add(sc.piece, a, b, 2, button(6.0), 0.0),
            add(sc.piece, a, c, 3, hole(10.0, 3.0, H), 0.0),
        ],
    })
    .unwrap();
    assert_eq!(doc.project().piece(sc.piece).unwrap().buttons.len(), 2);
    assert!(doc.undo());
    assert!(!doc.can_undo());
    assert_eq!(bytes(doc.project()), original);
}

// ----- CA7 : liaison à la construction ----------------------------------------------------

#[test]
fn ca7_le_groupe_suit_ses_deux_points() {
    let mut sc = scene();
    let a = free_point(&mut sc.p, sc.s, 10.0, 30.0);
    let b = free_point(&mut sc.p, sc.s, 90.0, 30.0);
    add_ok(&mut sc.p, sc.piece, (a, b), 3, button(8.0), 0.0);
    assert_points(
        &centers(&sc.p, sc.piece, 0),
        &[(10.0, 30.0), (50.0, 30.0), (90.0, 30.0)],
    );
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: b,
        to: pt(90.0, 50.0),
    })
    .unwrap();
    assert_points(
        &centers(&sc.p, sc.piece, 0),
        &[(10.0, 30.0), (50.0, 40.0), (90.0, 50.0)],
    );
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: a,
        to: pt(20.0, 10.0),
    })
    .unwrap();
    assert_points(
        &centers(&sc.p, sc.piece, 0),
        &[(20.0, 10.0), (55.0, 30.0), (90.0, 50.0)],
    );
    // Aucune position n'est écrite dans le fichier : seulement les deux points.
    let v: Value = serde_json::from_str(&bytes(&sc.p)).unwrap();
    let g = &v["pieces"][0]["buttons"][0];
    assert!(g.get("positions").is_none() && g.get("x").is_none());
}

#[test]
fn ca7_supprimer_un_point_retire_le_groupe_dans_la_meme_etape() {
    let mut sc = scene();
    let a = free_point(&mut sc.p, sc.s, 10.0, 30.0);
    let b = free_point(&mut sc.p, sc.s, 90.0, 30.0);
    let c = free_point(&mut sc.p, sc.s, 50.0, 50.0);
    add_ok(&mut sc.p, sc.piece, (a, b), 3, button(8.0), 0.0);
    add_ok(&mut sc.p, sc.piece, (c, b), 2, hole(10.0, 3.0, H), 0.0);
    add_ok(&mut sc.p, sc.piece, (a, c), 2, button(5.0), 0.0);
    let mut doc = Document::new(sc.p.clone());
    let before = bytes(doc.project());
    let o = doc
        .execute(Command::Delete {
            sheet: sc.s,
            points: vec![a],
            entities: vec![],
        })
        .unwrap();
    // Les deux groupes qui utilisaient A sont retirés, l'autre reste, avec une remarque par groupe.
    let left = &doc.project().piece(sc.piece).unwrap().buttons;
    assert_eq!(left.len(), 1);
    assert_eq!((left[0].from, left[0].to), (c, b));
    assert_eq!(
        o.notes
            .iter()
            .filter(|n| n.contains("groupe de boutons") && n.contains("retiré"))
            .count(),
        2,
        "{:?}",
        o.notes
    );
    doc.project().validate().unwrap();
    // Une seule étape : annuler rend les trois groupes.
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
    assert!(!doc.can_undo());
}

#[test]
fn ca7_un_groupe_n_est_ni_un_marquage_ni_un_element_interne() {
    let mut sc = scene();
    let a = free_point(&mut sc.p, sc.s, 10.0, 30.0);
    let b = free_point(&mut sc.p, sc.s, 90.0, 30.0);
    sc.p.apply(Command::AddPieceInternal {
        piece: sc.piece,
        target: InternalTarget::Point { point: a },
        kind: InternalKind::Buttonhole,
    })
    .unwrap();
    sc.p.apply(Command::AddPieceMarking {
        piece: sc.piece,
        at: MarkingAnchor::Point { point: a },
        shape: MarkingShape::Drill { diameter_mm: 4.0 },
    })
    .unwrap();
    add_ok(&mut sc.p, sc.piece, (a, b), 3, button(8.0), 0.0);
    let p = sc.p.piece(sc.piece).unwrap();
    assert_eq!(
        (p.internal.len(), p.markings.len(), p.buttons.len()),
        (1, 1, 1)
    );
    // Retirer l'un ne touche pas les autres ; les exports de marquages ignorent les boutons.
    sc.p.apply(Command::RemovePieceInternal {
        piece: sc.piece,
        target: InternalTarget::Point { point: a },
    })
    .unwrap();
    let p = sc.p.piece(sc.piece).unwrap();
    assert_eq!((p.markings.len(), p.buttons.len()), (1, 1));
    assert_eq!(sc.p.sheet_markings(sc.s).len(), 1);
    assert_eq!(sc.p.sheet_buttons(sc.s).len(), 3);
    assert_eq!(sc.p.piece_internal_counts(sc.piece).total(), 0);
    assert_eq!(sc.p.piece_card(sc.piece).unwrap().markings, 1);
}

// ----- CA8 : cohérence avec les autres opérations ----------------------------------------------

#[test]
fn ca8_copie_de_feuille_remappe_points_et_identifiants() {
    let mut sc = scene();
    let a = free_point(&mut sc.p, sc.s, 10.0, 30.0);
    let b = free_point(&mut sc.p, sc.s, 90.0, 30.0);
    let g1 = add_ok(&mut sc.p, sc.piece, (a, b), 3, button(8.0), 4.0);
    let g2 = add_ok(
        &mut sc.p,
        sc.piece,
        (b, a),
        2,
        hole(16.0, 3.0, angle(33.0)),
        1.5,
    );
    let before = sc.p.clone();
    let o = sc.p.apply(Command::DuplicateSheet { sheet: sc.s }).unwrap();
    sc.p.validate().unwrap();
    let copy = o.pieces[0];
    assert_eq!(
        sc.p.piece(sc.piece).unwrap(),
        before.piece(sc.piece).unwrap(),
        "la pièce d'origine n'a pas changé"
    );
    let orig = sc.p.piece(sc.piece).unwrap();
    let dup = sc.p.piece(copy).unwrap();
    assert_eq!(dup.buttons.len(), 2);
    assert!(dup.buttons.iter().all(|g| g.id != g1 && g.id != g2));
    // Mêmes paramètres, points propres à la copie.
    assert_eq!(dup.buttons[0].count, 3);
    assert_eq!(dup.buttons[0].element, button(8.0));
    assert_eq!(dup.buttons[0].edge_margin_mm, 4.0);
    assert_eq!(dup.buttons[1].element, hole(16.0, 3.0, angle(33.0)));
    let PieceLink::Linked { sheet: new_sheet } = dup.link else {
        panic!("copie liée");
    };
    let sh = sc.p.sheet(new_sheet).unwrap();
    for g in &dup.buttons {
        for pnt in [g.from, g.to] {
            assert!(sh.point(pnt).is_ok(), "point de la copie");
            assert!(
                !orig.buttons.iter().any(|o| o.from == pnt || o.to == pnt),
                "la copie ne partage aucun point avec l'original"
            );
        }
    }
    for (x, y) in centers(&sc.p, copy, 0)
        .iter()
        .zip(centers(&sc.p, sc.piece, 0))
    {
        assert!(at_near(*x, y));
    }
    // Déplacer un point de la copie ne bouge pas l'original.
    let moved = centers(&sc.p, sc.piece, 0);
    sc.p.apply(Command::MovePoint {
        sheet: new_sheet,
        point: dup.buttons[0].to,
        to: pt(80.0, 50.0),
    })
    .unwrap();
    assert_eq!(centers(&sc.p, sc.piece, 0), moved);
    assert!(at_near(centers(&sc.p, copy, 0)[2], pt(80.0, 50.0)));
    // Import dans un autre projet : mêmes remappages, identifiants neufs.
    let mut other = Project::new("Autre projet");
    let imported = other.apply(before.import_command(sc.s).unwrap()).unwrap();
    other.validate().unwrap();
    let ip = other.piece(imported.pieces[0]).unwrap();
    assert_eq!(ip.buttons.len(), 2);
    assert!(at_near(centers(&other, ip.id, 0)[1], pt(50.0, 30.0)));
}

/// Pièce 100 × 60 coupée par la verticale x = 50 : un groupe dans chaque moitié, un groupe
/// qui traverse la ligne de coupe.
fn split_scene() -> Scene {
    let mut sc = scene();
    let pts = [
        (20.0, 20.0),
        (40.0, 20.0),
        (60.0, 40.0),
        (90.0, 40.0),
        (10.0, 50.0),
        (90.0, 50.0),
    ]
    .map(|(x, y)| free_point(&mut sc.p, sc.s, x, y));
    add_ok(&mut sc.p, sc.piece, (pts[0], pts[1]), 3, button(6.0), 0.0);
    add_ok(
        &mut sc.p,
        sc.piece,
        (pts[2], pts[3]),
        2,
        hole(12.0, 3.0, V),
        0.0,
    );
    add_ok(&mut sc.p, sc.piece, (pts[4], pts[5]), 4, button(6.0), 0.0);
    sc
}

#[test]
fn ca8_separer_reprend_chaque_groupe_dans_sa_moitie_et_signale_le_reste() {
    let mut sc = split_scene();
    let o =
        sc.p.apply(Command::SplitPiece {
            piece: sc.piece,
            a: pt(50.0, -10.0),
            b: pt(50.0, 70.0),
        })
        .unwrap();
    sc.p.validate().unwrap();
    let (l, r) = (o.pieces[0], o.pieces[1]);
    // « 1/2 » est à gauche de a → b (x < 50).
    let lg = sc.p.piece_buttons(l);
    let rg = sc.p.piece_buttons(r);
    assert_eq!((lg.len(), rg.len()), (1, 1), "{:?}", o.notes);
    assert_points(
        &lg[0].elements.iter().map(|e| e.center).collect::<Vec<_>>(),
        &[(20.0, 20.0), (30.0, 20.0), (40.0, 20.0)],
    );
    assert_points(
        &rg[0].elements.iter().map(|e| e.center).collect::<Vec<_>>(),
        &[(60.0, 40.0), (90.0, 40.0)],
    );
    assert_eq!(lg[0].group.element, button(6.0));
    assert_eq!(rg[0].group.element, hole(12.0, 3.0, V));
    // Le groupe qui traverse la ligne de coupe est signalé, jamais perdu en silence.
    let lost: Vec<_> = o
        .notes
        .iter()
        .filter(|n| n.contains("groupe(s) de boutons") && n.contains("pas repris"))
        .collect();
    assert_eq!(lost.len(), 1, "{:?}", o.notes);
    // Points libres des feuilles des moitiés, identifiants distincts.
    for piece in [l, r] {
        let sheet = sc.p.piece_sheet(piece).unwrap();
        for g in &sc.p.piece(piece).unwrap().buttons {
            assert!(sheet.point(g.from).is_ok() && sheet.point(g.to).is_ok());
        }
    }
    assert_ne!(
        sc.p.piece(l).unwrap().buttons[0].id,
        sc.p.piece(r).unwrap().buttons[0].id
    );
    // La pièce d'origine est remplacée ; la construction de la feuille d'origine est intacte.
    assert!(sc.p.piece(sc.piece).is_err());
}

#[test]
fn ca8_assembler_rend_les_groupes_des_deux_pieces_a_leur_place() {
    let mut sc = scene();
    let (a, b) = (
        free_point(&mut sc.p, sc.s, 10.0, 30.0),
        free_point(&mut sc.p, sc.s, 40.0, 30.0),
    );
    let (c, d) = (
        free_point(&mut sc.p, sc.s, 60.0, 40.0),
        free_point(&mut sc.p, sc.s, 90.0, 40.0),
    );
    add_ok(&mut sc.p, sc.piece, (a, b), 4, button(6.0), 0.0);
    add_ok(&mut sc.p, sc.piece, (c, d), 3, hole(12.0, 3.0, H), 0.0);
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
    let groups = sc.p.piece_buttons(merged);
    assert_eq!(groups.len(), 2, "{:?}", j.notes);
    let find = |count: u32| groups.iter().find(|g| g.group.count == count).unwrap();
    for (e, want) in
        find(4)
            .elements
            .iter()
            .zip([(10.0, 30.0), (20.0, 30.0), (30.0, 30.0), (40.0, 30.0)])
    {
        assert!(distance_mm(e.center, pt(want.0, want.1)) < 1e-6, "{e:?}");
    }
    for (e, want) in find(3)
        .elements
        .iter()
        .zip([(60.0, 40.0), (75.0, 40.0), (90.0, 40.0)])
    {
        assert!(distance_mm(e.center, pt(want.0, want.1)) < 1e-6, "{e:?}");
    }
    // La boutonnière « horizontale » le reste (aucune rotation à l'assemblage ici).
    assert!(find(3).elements.iter().all(
        |e| matches!(e.shape, ButtonShape::Buttonhole { angle_deg, .. } if angle_deg.abs() < 1e-6)
    ));
    assert!(sc.p.piece(l).is_err() && sc.p.piece(r).is_err());
}

#[test]
fn ca8_assembler_avec_rotation_l_orientation_suit_la_piece() {
    // A : rectangle 100 × 60 en (0 ; 0). B : rectangle 60 × 40 en (200 ; 0) ; son bord du bas
    // (60 mm) est assemblé au bord droit de A (60 mm) : B tourne de ±90°.
    let mut sc = scene();
    let o =
        sc.p.apply(Command::AddRectangle {
            sheet: sc.s,
            corner: PointRef::New(pt(200.0, 0.0)),
            width_mm: 60.0,
            height_mm: 40.0,
        })
        .unwrap();
    let b_piece =
        sc.p.apply(Command::ExtractPiece {
            sheet: sc.s,
            source: PieceSource::Paths {
                entities: o.entities,
            },
            name: "B".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let (p1, p2) = (
        free_point(&mut sc.p, sc.s, 210.0, 20.0),
        free_point(&mut sc.p, sc.s, 250.0, 20.0),
    );
    add_ok(&mut sc.p, b_piece, (p1, p2), 2, hole(12.0, 3.0, H), 0.0);
    add_ok(&mut sc.p, b_piece, (p1, p2), 2, hole(12.0, 3.0, V), 0.0);
    add_ok(&mut sc.p, b_piece, (p1, p2), 2, hole(12.0, 3.0, PERP), 0.0);
    add_ok(
        &mut sc.p,
        b_piece,
        (p1, p2),
        2,
        hole(12.0, 3.0, angle(30.0)),
        0.0,
    );
    let find_edge = |p: &Project, piece: PieceId, f: &dyn Fn(PointMm, PointMm) -> bool| {
        let sheet = p.piece_sheet(piece).unwrap();
        p.piece(piece)
            .unwrap()
            .outline
            .iter()
            .find(|e| matches!(sheet.shape(e.entity), Ok(Shape::Line(a, b)) if f(a, b)))
            .unwrap()
            .entity
    };
    let right_of_a = find_edge(&sc.p, sc.piece, &|a, b| {
        (a.x - 100.0).abs() < 1e-9 && (b.x - 100.0).abs() < 1e-9
    });
    let bottom_of_b = find_edge(&sc.p, b_piece, &|a, b| {
        a.y.abs() < 1e-9 && b.y.abs() < 1e-9 && a.x > 150.0
    });
    let j =
        sc.p.apply(Command::JoinPieces {
            first: sc.piece,
            first_edge: right_of_a,
            second: b_piece,
            second_edge: bottom_of_b,
            tolerance_mm: 0.5,
        })
        .unwrap();
    sc.p.validate().unwrap();
    let merged = j.pieces[0];
    let groups = sc.p.piece_buttons(merged);
    assert_eq!(groups.len(), 4, "{:?}", j.notes);
    // L'axe du groupe a tourné de θ (±90°, la pièce B a été tournée) ; chaque boutonnière garde
    // sa place par rapport à la pièce : horizontale → axe + 0°, verticale → axe + 90°,
    // perpendiculaire → axe + 90°, angle 30° → axe + 30° (l'axe de B était horizontal).
    let axis = groups[0].axis_deg;
    let rotation = pf_geometry::direction_angle_deg(groups[0].from, groups[0].to).unwrap();
    assert!((axis - rotation).abs() < 1e-9);
    assert!(
        (axis - 90.0).abs() < 1e-6 || (axis - 270.0).abs() < 1e-6,
        "la pièce B doit avoir tourné : {axis}"
    );
    let norm = |d: f64| d.rem_euclid(360.0);
    let got: Vec<f64> = (0..4).map(|i| angle_of(&sc.p, merged, i)).collect();
    for (i, want) in [axis, axis + 90.0, axis + 90.0, axis + 30.0]
        .iter()
        .enumerate()
    {
        let diff = (norm(got[i]) - norm(*want)).abs();
        assert!(
            diff < 1e-6 || (360.0 - diff) < 1e-6,
            "{i} : {got:?} / axe {axis}"
        );
    }
    // Même distance entre les deux éléments (assemblage rigide).
    assert!((groups[0].spacing_mm - 40.0).abs() < 1e-6);
}

#[test]
fn ca8_deplier_ne_reprend_pas_les_groupes_et_le_dit() {
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
    let (a, b) = (
        free_point(&mut p, s, 20.0, 10.0),
        free_point(&mut p, s, 20.0, 30.0),
    );
    add_ok(&mut p, piece, (a, b), 3, button(6.0), 0.0);
    let before = p.piece(piece).unwrap().clone();
    let u = p
        .apply(Command::UnfoldPiece {
            piece,
            fold,
            name: None,
        })
        .unwrap();
    p.validate().unwrap();
    assert!(p.piece(u.pieces[0]).unwrap().buttons.is_empty());
    assert_eq!(p.piece(piece).unwrap().buttons, before.buttons);
    assert!(
        u.notes
            .iter()
            .any(|n| n.contains("groupe(s) de boutons") && n.contains("pas repris")),
        "{:?}",
        u.notes
    );
}

#[test]
fn ca8_le_plan_de_pieces_ne_touche_ni_la_construction_ni_les_groupes() {
    let mut sc = scene();
    let a = free_point(&mut sc.p, sc.s, 10.0, 30.0);
    let b = free_point(&mut sc.p, sc.s, 90.0, 30.0);
    add_ok(&mut sc.p, sc.piece, (a, b), 4, hole(16.0, 3.0, PERP), 5.0);
    let groups = sc.p.piece(sc.piece).unwrap().buttons.clone();
    let at = centers(&sc.p, sc.piece, 0);
    sc.p.apply(Command::SetPiecePlacement {
        piece: sc.piece,
        placement: Some(Placement {
            x_mm: 300.0,
            y_mm: -80.0,
            rotation_deg: 37.0,
        }),
    })
    .unwrap();
    assert_eq!(sc.p.piece(sc.piece).unwrap().buttons, groups);
    assert_eq!(centers(&sc.p, sc.piece, 0), at, "placement = vue seulement");
    assert_eq!(sc.p.plan_layout().len(), 1);
    sc.p.validate().unwrap();
}

#[test]
fn ca8_piece_detachee_groupes_sur_les_points_de_sa_feuille() {
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
    let a = free_point(&mut sc.p, dsheet, 10.0, 30.0);
    let b = free_point(&mut sc.p, dsheet, 90.0, 30.0);
    add_ok(&mut sc.p, det, (a, b), 3, button(8.0), 0.0);
    sc.p.validate().unwrap();
    assert_points(
        &centers(&sc.p, det, 0),
        &[(10.0, 30.0), (50.0, 30.0), (90.0, 30.0)],
    );
    // Exporté sur la feuille de la pièce détachée seulement.
    assert_eq!(sc.p.sheet_buttons(dsheet).len(), 3);
    assert!(sc.p.sheet_buttons(sc.s).is_empty());
    // Un point de la feuille d'origine n'est pas permis pour cette pièce.
    let foreign = free_point(&mut sc.p, sc.s, 30.0, 30.0);
    refused(&mut sc.p, add(det, a, foreign, 2, button(8.0), 0.0));
    // La suppression d'un de ses points retire le groupe, comme pour une pièce liée.
    let o =
        sc.p.apply(Command::Delete {
            sheet: dsheet,
            points: vec![a],
            entities: vec![],
        })
        .unwrap();
    assert!(sc.p.piece(det).unwrap().buttons.is_empty());
    assert!(o.notes.iter().any(|n| n.contains("groupe de boutons")));
}

#[test]
fn ca8_piece_derivee_les_groupes_ne_sont_jamais_perdus_en_silence() {
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
    let dsheet = sc.p.piece_sheet(d).unwrap().id;
    let a = free_point(&mut sc.p, dsheet, 20.0, 30.0);
    let b = free_point(&mut sc.p, dsheet, 80.0, 30.0);
    add_ok(&mut sc.p, d, (a, b), 3, button(6.0), 0.0);
    sc.p.validate().unwrap();
    // La mère change : la dérivée est régénérée dans la même étape.
    let corner =
        sc.p.sheet(sc.s)
            .unwrap()
            .points
            .iter()
            .find(|p| {
                sc.p.sheet(sc.s)
                    .unwrap()
                    .position(p.id)
                    .is_ok_and(|q| at_near(q, pt(100.0, 60.0)))
            })
            .unwrap()
            .id;
    let o =
        sc.p.apply(Command::MovePoint {
            sheet: sc.s,
            point: corner,
            to: pt(110.0, 60.0),
        })
        .unwrap();
    assert!(o.regenerated.contains(&d), "{:?}", o.notes);
    sc.p.validate().unwrap();
    let kept = sc.p.piece(d).unwrap().buttons.len();
    let retired = o
        .notes
        .iter()
        .filter(|n| n.contains("groupe de boutons") && n.contains("retiré"))
        .count();
    assert_eq!(kept + retired, 1, "{:?}", o.notes);
    // Les deux points libres de la dérivée ne sont pas des tracés régénérés : le groupe suit.
    assert_eq!((kept, retired), (1, 0), "{:?}", o.notes);
    assert_points(
        &centers(&sc.p, d, 0),
        &[(20.0, 30.0), (50.0, 30.0), (80.0, 30.0)],
    );
}

#[test]
fn ca8_l_inspection_de_la_fiche_et_des_avertissements_est_stable() {
    // Aucun groupe : aucun avertissement de recul, fiche inchangée.
    let sc = scene();
    assert_eq!(issue_count(&sc.p, sc.piece), None);
    assert!(sc.p.piece_buttons(sc.piece).is_empty());
    assert!(sc.p.piece_buttons(PieceId(9_999_999)).is_empty());
    assert!(sc.p.sheet_buttons(sc.s).is_empty());
}

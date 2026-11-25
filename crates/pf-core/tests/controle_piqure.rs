//! US-08.6 — contrôle des trous de piqûre entre pièces : `PairStitchLines`, `UnpairStitchLines`,
//! `CarryStitchLine` et `Project::stitch_check_report`. Valeurs attendues **calculées à la main** ;
//! la campagne adverse est dans `adverse_controle_piqure.rs`.
//!
//! Scène : pièce A = rectangle 100 × 70 en (0 ; 0), pièce B = rectangle 100 × 70 en (200 ; 0), pièce
//! C = rectangle 90 × 70 en (400 ; 0), pièce D = rectangle 100 × 70 tourné d'un quart de tour
//! (sommets (600 ; 0), (600 ; 100), (530 ; 100), (530 ; 0)). Tous les contours sont anti-horaires :
//! les deux côtés d'un couple sont donc cousus en sens opposés (A de gauche à droite, B de droite
//! à gauche).

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn refused(p: &mut Project, c: Command) -> String {
    let (before, text) = (p.clone(), bytes(p));
    let err = p.apply(c).expect_err("la commande aurait dû être refusée");
    assert_eq!(*p, before, "refus sans trace (valeur)");
    assert_eq!(text, bytes(p), "refus sans trace (octets)");
    err.to_string()
}

struct Scene {
    p: Project,
    s: SheetId,
    pieces: [PieceId; 4],
}

fn polygon(
    p: &mut Project,
    s: SheetId,
    v: [(f64, f64); 4],
    name: &str,
    click: (f64, f64),
) -> PieceId {
    p.apply(Command::AddPolygon {
        sheet: s,
        vertices: v.map(|(x, y)| PointRef::New(pt(x, y))).to_vec(),
        closed: true,
        role: PathRole::default(),
        layer: None,
    })
    .unwrap();
    p.apply(Command::ExtractPiece {
        sheet: s,
        source: PieceSource::Click {
            at: pt(click.0, click.1),
        },
        name: name.into(),
        detached: false,
    })
    .unwrap()
    .pieces[0]
}

fn scene() -> Scene {
    let mut p = Project::new("Controle piqure");
    let s = p.sheets[0].id;
    let a = polygon(
        &mut p,
        s,
        [(0.0, 0.0), (100.0, 0.0), (100.0, 70.0), (0.0, 70.0)],
        "A",
        (50.0, 35.0),
    );
    let b = polygon(
        &mut p,
        s,
        [(200.0, 0.0), (300.0, 0.0), (300.0, 70.0), (200.0, 70.0)],
        "B",
        (250.0, 35.0),
    );
    let c = polygon(
        &mut p,
        s,
        [(400.0, 0.0), (490.0, 0.0), (490.0, 70.0), (400.0, 70.0)],
        "C",
        (445.0, 35.0),
    );
    let d = polygon(
        &mut p,
        s,
        [(600.0, 0.0), (600.0, 100.0), (530.0, 100.0), (530.0, 0.0)],
        "D",
        (565.0, 50.0),
    );
    Scene {
        p,
        s,
        pieces: [a, b, c, d],
    }
}

/// Tracé de la feuille entre deux positions, dans un sens ou dans l'autre.
fn edge(p: &Project, s: SheetId, a: PointMm, b: PointMm) -> EntityId {
    let sheet = p.sheet(s).unwrap();
    let near = |x: PointMm, y: PointMm| (x.x - y.x).abs() < 1e-9 && (x.y - y.y).abs() < 1e-9;
    sheet
        .entities
        .iter()
        .find(|e| {
            sheet
                .segment_ends(e.id)
                .is_ok_and(|(x, y)| (near(x, a) && near(y, b)) || (near(x, b) && near(y, a)))
        })
        .expect("segment")
        .id
}

fn line(edges: Vec<EntityId>, pitch: f64, count: Option<u32>) -> StitchLineSpec {
    StitchLineSpec {
        edges,
        hole: StitchHole::Round { diameter_mm: 1.2 },
        pitch_mm: pitch,
        setback_mm: 3.0,
        count,
        continues: None,
    }
}

fn add_line(p: &mut Project, piece: PieceId, spec: StitchLineSpec) -> StitchLineId {
    p.apply(Command::AddStitchLine { piece, line: spec })
        .unwrap()
        .stitch_lines[0]
}

fn bottom_line(
    sc: &mut Scene,
    piece: PieceId,
    x0: f64,
    w: f64,
    pitch: f64,
    count: Option<u32>,
) -> StitchLineId {
    let e = bottom_of(sc, x0, w);
    add_line(&mut sc.p, piece, line(vec![e], pitch, count))
}

fn sref(piece: PieceId, line: StitchLineId) -> StitchRef {
    StitchRef { piece, line }
}

fn bottom_of(sc: &Scene, x0: f64, w: f64) -> EntityId {
    edge(&sc.p, sc.s, pt(x0, 0.0), pt(x0 + w, 0.0))
}

// ----- CA1 : report conserve l'abscisse curviligne -------------------------------------------

#[test]
fn ca1_report_meme_longueur_conserve_l_abscisse_a_0_01_mm_et_numerote_en_couple() {
    let mut sc = scene();
    let [a, b, _, _] = sc.pieces;
    let la = bottom_line(&mut sc, a, 0.0, 100.0, 4.0, None);
    let o =
        sc.p.apply(Command::CarryStitchLine {
            from: sref(a, la),
            to_piece: b,
            to_edges: vec![bottom_of(&sc, 200.0, 100.0)],
        })
        .unwrap();
    let lb = o.stitch_lines[0];
    assert_eq!(o.stitch_pairs.len(), 1);
    let r = &sc.p.stitch_check_report()[0];
    // 26 trous des deux côtés, pas 4,0 / 4,0, écart de pas nul, 26 vis-à-vis à écart nul.
    assert!(r.not_computable.is_none() && r.counts_equal() && r.count_a == 26);
    assert!((r.mean_step_a - 4.0).abs() < 1e-9 && (r.mean_step_b - 4.0).abs() < 1e-9);
    assert!(r.step_gap_mm.abs() < 1e-9);
    assert_eq!(r.count_status(StitchMatch::Matched), 26);
    assert!(r.items.iter().all(|i| i.gap_mm.unwrap() <= 0.01));
    // Cousus en sens opposés : le trou k de A (x = 4 (k − 1)) face au trou de B à x = 300 − 4 (k − 1).
    for it in &r.items {
        let (ha, hb) = (it.a.unwrap(), it.b.unwrap());
        assert_eq!(it.number, ha.number);
        assert!((ha.position.x - 4.0 * (ha.number - 1) as f64).abs() < 1e-9);
        assert!((hb.position.x - (300.0 - 4.0 * (ha.number - 1) as f64)).abs() < 1e-9);
        assert!((ha.abscissa_mm - (100.0 - hb.abscissa_mm)).abs() < 0.01);
    }
    // Numéros appariés lus sur les deux pièces : B est numérotée en sens inverse de ses abscisses.
    let na = sc.p.stitch_paired_numbers(a, la).unwrap();
    let nb = sc.p.stitch_paired_numbers(b, lb).unwrap();
    assert_eq!(na, (1..=26).collect::<Vec<u32>>());
    assert_eq!(nb, (1..=26).rev().collect::<Vec<u32>>());
}

#[test]
fn ca1_report_sur_un_bord_plus_court_conserve_la_fraction_d_abscisse() {
    let mut sc = scene();
    let [a, _, c, _] = sc.pieces;
    let la = bottom_line(&mut sc, a, 0.0, 100.0, 4.0, None);
    let o =
        sc.p.apply(Command::CarryStitchLine {
            from: sref(a, la),
            to_piece: c,
            to_edges: vec![bottom_of(&sc, 400.0, 90.0)],
        })
        .unwrap();
    let lc = sc.p.stitch_line_layout(c, o.stitch_lines[0]).unwrap();
    // Même nombre (26) ; bord de 90 mm : 25 intervalles de 3,6 mm.
    assert_eq!(lc.holes.len(), 26);
    for (k, h) in lc.holes.iter().enumerate() {
        assert!((h.abscissa_mm - 3.6 * k as f64).abs() < 0.01, "{k}");
    }
    let r = &sc.p.stitch_check_report()[0];
    // Pas moyen A 4,0, C 3,6 : écart 0,4 ; tous les vis-à-vis proportionnels exacts.
    assert!((r.step_gap_mm - 0.4).abs() < 1e-9);
    assert_eq!(r.count_status(StitchMatch::Matched), 26);
    assert!(r.items.iter().all(|i| i.gap_mm.unwrap() <= 0.01));
}

#[test]
fn ca1_report_sur_une_piece_tournee_d_un_quart_de_tour() {
    let mut sc = scene();
    let [a, _, _, d] = sc.pieces;
    let la = bottom_line(&mut sc, a, 0.0, 100.0, 4.0, None);
    // Bord (600 ; 0) → (600 ; 100) de la pièce D : le « bas » tourné de 90 degrés.
    let e = edge(&sc.p, sc.s, pt(600.0, 0.0), pt(600.0, 100.0));
    let o =
        sc.p.apply(Command::CarryStitchLine {
            from: sref(a, la),
            to_piece: d,
            to_edges: vec![e],
        })
        .unwrap();
    let ld = sc.p.stitch_line_layout(d, o.stitch_lines[0]).unwrap();
    // Intérieur de D à gauche du bord montant : x = 600 − 3 ; trous à y = 4 k.
    assert_eq!(ld.holes.len(), 26);
    for (k, h) in ld.holes.iter().enumerate() {
        assert!(
            (h.position.x - 597.0).abs() < 1e-9 && (h.position.y - 4.0 * k as f64).abs() < 1e-9
        );
    }
    let r = &sc.p.stitch_check_report()[0];
    assert_eq!(r.count_status(StitchMatch::Matched), 26);
}

// ----- CA2 : trou en trop ou manquant, listé avec sa position -------------------------------

/// A : 5 trous (x = 0, 25, 50, 75, 100) ; B : 4 trous (x − 200 = 0 ; 33,33 ; 66,67 ; 100).
fn five_four() -> (Scene, StitchRef, StitchRef) {
    let mut sc = scene();
    let [a, b, _, _] = sc.pieces;
    let la = bottom_line(&mut sc, a, 0.0, 100.0, 4.0, Some(5));
    let lb = bottom_line(&mut sc, b, 200.0, 100.0, 4.0, Some(4));
    (sc, sref(a, la), sref(b, lb))
}

#[test]
fn ca2_trou_manquant_sur_b_liste_avec_sa_position() {
    let (mut sc, ra, rb) = five_four();
    sc.p.apply(Command::PairStitchLines { a: ra, b: rb })
        .unwrap();
    let r = &sc.p.stitch_check_report()[0];
    assert_eq!((r.count_a, r.count_b), (5, 4));
    assert!(!r.counts_equal());
    // Pas moyen : 25 et 100 / 3 ; écart −8,333…
    assert!((r.mean_step_a - 25.0).abs() < 1e-9);
    assert!((r.mean_step_b - 100.0 / 3.0).abs() < 1e-9);
    assert!((r.step_gap_mm - (25.0 - 100.0 / 3.0)).abs() < 1e-9);
    // Vis-à-vis attendus (sens opposés) : A0 ↔ B3 et A4 ↔ B0 exacts, A1 ↔ B2 et A3 ↔ B1 décalés de
    // 25/3 = 8,33 mm (≤ la moitié du plus petit pas, 12,5), A2 (x = 50) sans vis-à-vis.
    assert_eq!(r.count_status(StitchMatch::Matched), 2);
    assert_eq!(r.count_status(StitchMatch::Shifted), 2);
    let missing: Vec<&StitchCheckItem> = r.unmatched().collect();
    assert_eq!(missing.len(), 1);
    assert_eq!(
        missing[0].status,
        StitchMatch::Missing(pf_core::seam_pair::SeamWhich::B)
    );
    assert_eq!(missing[0].number, 3);
    let h = missing[0].a.unwrap();
    assert!((h.position.x - 50.0).abs() < 1e-9 && (h.position.y - 3.0).abs() < 1e-9);
    for it in r.items.iter().filter(|i| i.status == StitchMatch::Shifted) {
        assert!((it.gap_mm.unwrap() - 25.0 / 3.0).abs() < 1e-9);
    }
    // La tolérance n'est pas stockée : à 10 mm, les deux décalés sont des vis-à-vis.
    let loose = &sc.p.stitch_check_report_with(10.0)[0];
    assert_eq!(loose.count_status(StitchMatch::Matched), 4);
    assert_eq!(loose.count_status(StitchMatch::Shifted), 0);
    assert_eq!(loose.tolerance_mm, 10.0);
    // Une tolérance invalide retombe sur la valeur par défaut.
    assert_eq!(
        sc.p.stitch_check_report_with(f64::NAN)[0].tolerance_mm,
        STITCH_CHECK_TOLERANCE_MM
    );
}

#[test]
fn ca2_trou_en_trop_sur_b_liste_avec_sa_position() {
    let (mut sc, ra, rb) = five_four();
    // Couple inversé : A = la ligne à 4 trous, B = celle à 5 trous.
    sc.p.apply(Command::PairStitchLines { a: rb, b: ra })
        .unwrap();
    let r = &sc.p.stitch_check_report()[0];
    assert_eq!((r.count_a, r.count_b), (4, 5));
    let extra: Vec<&StitchCheckItem> = r.unmatched().collect();
    assert_eq!(extra.len(), 1);
    assert_eq!(
        extra[0].status,
        StitchMatch::Missing(pf_core::seam_pair::SeamWhich::A)
    );
    // Numéro après les 4 trous de A ; trou central de la pièce A (x = 50, y = 3).
    assert_eq!(extra[0].number, 5);
    let h = extra[0].b.unwrap();
    assert!((h.position.x - 50.0).abs() < 1e-9 && (h.position.y - 3.0).abs() < 1e-9);
    assert!(extra[0].a.is_none());
}

#[test]
fn ca2_la_lecture_ne_depend_pas_de_la_place_de_la_piece_dans_le_plan() {
    let (mut sc, ra, rb) = five_four();
    sc.p.apply(Command::PairStitchLines { a: ra, b: rb })
        .unwrap();
    let before = sc.p.stitch_check_report();
    let placed = Placement {
        x_mm: 1234.0,
        y_mm: -321.0,
        rotation_deg: 37.0,
    };
    sc.p.apply(Command::SetPiecePlacement {
        piece: rb.piece,
        placement: Some(placed),
    })
    .unwrap();
    assert_eq!(sc.p.stitch_check_report(), before);
}

// ----- CA3 : refus sans trace -------------------------------------------------------------

#[test]
fn ca3_refus_sans_trace() {
    let (mut sc, ra, rb) = five_four();
    let [a, b, c, _] = sc.pieces;
    refused(&mut sc.p, Command::PairStitchLines { a: ra, b: ra });
    refused(
        &mut sc.p,
        Command::PairStitchLines {
            a: ra,
            b: sref(b, StitchLineId(999_999)),
        },
    );
    sc.p.apply(Command::PairStitchLines { a: ra, b: rb })
        .unwrap();
    // Une ligne n'appartient qu'à un couple.
    let lc = bottom_line(&mut sc, c, 400.0, 90.0, 4.0, None);
    refused(
        &mut sc.p,
        Command::PairStitchLines {
            a: sref(c, lc),
            b: ra,
        },
    );
    refused(
        &mut sc.p,
        Command::UnpairStitchLines {
            pair: StitchPairId(999_999),
        },
    );
    // Report : source déjà appariée, même pièce, bord invalide, trou hors de la pièce.
    let c_bottom = bottom_of(&sc, 400.0, 90.0);
    refused(
        &mut sc.p,
        Command::CarryStitchLine {
            from: ra,
            to_piece: c,
            to_edges: vec![c_bottom],
        },
    );
    let top_a = edge(&sc.p, sc.s, pt(100.0, 70.0), pt(0.0, 70.0));
    let la2 = add_line(&mut sc.p, a, line(vec![top_a], 4.0, None));
    refused(
        &mut sc.p,
        Command::CarryStitchLine {
            from: sref(a, la2),
            to_piece: a,
            to_edges: vec![top_a],
        },
    );
    refused(
        &mut sc.p,
        Command::CarryStitchLine {
            from: sref(a, la2),
            to_piece: c,
            to_edges: vec![top_a],
        },
    );
}

// ----- CA4 : une étape annulable, nettoyage, fichier -------------------------------------

#[test]
fn ca4_report_en_une_etape_annulable_et_couple_retire_avec_la_ligne() {
    let mut sc = scene();
    let [a, b, _, _] = sc.pieces;
    let la = bottom_line(&mut sc, a, 0.0, 100.0, 4.0, None);
    let mut doc = Document::new(sc.p.clone());
    let original = bytes(doc.project());
    let o = doc
        .execute(Command::CarryStitchLine {
            from: sref(a, la),
            to_piece: b,
            to_edges: vec![bottom_of(&sc, 200.0, 100.0)],
        })
        .unwrap();
    assert_eq!(doc.project().stitch_pairs.len(), 1);
    let done = bytes(doc.project());
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), original);
    assert!(!doc.can_undo() || bytes(doc.project()) == original);
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), done);
    // Supprimer une ligne retire le couple, avec remarque.
    let out = doc
        .execute(Command::RemoveStitchLine {
            piece: b,
            line: o.stitch_lines[0],
        })
        .unwrap();
    assert!(doc.project().stitch_pairs.is_empty());
    assert!(!out.notes.is_empty());
    // Relecture JSON identique, clé omise quand vide, réservée à la v5.
    assert!(!bytes(doc.project()).contains("stitch_pairs"));
    assert!(doc.undo());
    let json = bytes(doc.project());
    assert!(json.contains("\"stitch_pairs\""));
    let back = persist::from_json(&json).unwrap();
    assert_eq!(&back, doc.project());
    let v4 = json.replacen("\"schema_version\": 6", "\"schema_version\": 4", 1);
    let err = persist::from_json(&v4).expect_err("réservée à la v5");
    assert!(err.to_string().contains("stitch_pairs") || err.to_string().contains("stitch_lines"));
}

#[path = "format_ouvert/validateur.rs"]
mod validateur;

#[test]
fn ca4_le_schema_json_valide_les_couples_et_refuse_une_cle_inconnue() {
    let (mut sc, ra, rb) = five_four();
    sc.p.apply(Command::PairStitchLines { a: ra, b: rb })
        .unwrap();
    let schema: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs/pforge-v6.schema.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let mut doc: serde_json::Value = serde_json::from_str(&bytes(&sc.p)).unwrap();
    assert!(validateur::validate(&schema, &doc).is_empty());
    doc["stitch_pairs"][0]["tolerance_mm"] = serde_json::json!(0.5);
    assert!(
        !validateur::validate(&schema, &doc).is_empty(),
        "tolérance non stockée"
    );
    // Le chargeur refuse la même ligne des deux côtés et une ligne inconnue.
    let mut bad: serde_json::Value = serde_json::from_str(&bytes(&sc.p)).unwrap();
    let a = bad["stitch_pairs"][0]["a"].clone();
    bad["stitch_pairs"][0]["b"] = a;
    assert!(persist::from_json(&bad.to_string()).is_err());
}

#[test]
fn ca4_couple_dont_la_piece_disparait_est_retire() {
    let (mut sc, ra, rb) = five_four();
    sc.p.apply(Command::PairStitchLines { a: ra, b: rb })
        .unwrap();
    let o =
        sc.p.apply(Command::DeletePiece { piece: rb.piece })
            .unwrap();
    assert!(sc.p.stitch_pairs.is_empty(), "{:?}", o.notes);
    // Rien n'est lu pour un projet sans couple.
    assert!(sc.p.stitch_check_report().is_empty());
}

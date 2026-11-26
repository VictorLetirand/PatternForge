//! US-08.1 — coutures appariées. Valeurs attendues calculées à la main dans les tests (jamais
//! relues du code testé). CA1 longueurs / écart / embu / statut, CA2 longueur de couture et non de
//! coupe, CA3 mise à jour en direct, CA4 refus sans trace, CA5 une étape annulable, CA6 retrait
//! avec remarque, CA7 contour ouvert = non calculable, CA8 fichiers (aller-retour, omis si vide,
//! chargeur strict, `pf-validate`, anciens fichiers), CA9 copie / séparer / découper / assembler.

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;
use serde_json::{json, Value};
use std::path::PathBuf;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn setup() -> (Project, SheetId) {
    let p = Project::new("coutures");
    let s = p.sheets[0].id;
    (p, s)
}

fn rect_piece(p: &mut Project, s: SheetId, x: f64, y: f64, w: f64, h: f64, name: &str) -> PieceId {
    let o = p
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
            entities: o.entities,
        },
        name: name.into(),
        detached: false,
    })
    .unwrap()
    .pieces[0]
}

/// Tracé segment de la feuille dont les extrémités sont exactement a et b (dans un sens ou l'autre).
fn edge(p: &Project, s: SheetId, a: (f64, f64), b: (f64, f64)) -> EntityId {
    let sheet = p.sheet(s).unwrap();
    let near = |u: PointMm, v: (f64, f64)| (u.x - v.0).abs() < 1e-9 && (u.y - v.1).abs() < 1e-9;
    sheet
        .entities
        .iter()
        .map(|e| e.id)
        .find(|id| {
            sheet
                .segment_ends(*id)
                .is_ok_and(|(u, v)| (near(u, a) && near(v, b)) || (near(u, b) && near(v, a)))
        })
        .expect("tracé introuvable")
}

/// Côté formé de ces tracés (consécutifs dans le contour, quel que soit le sens d'enregistrement).
fn side(p: &Project, piece: PieceId, edges: &[EntityId]) -> SeamSide {
    for start in edges {
        let s = p.seam_side_range(piece, *start, edges.len()).unwrap();
        let mut a = s.edges.clone();
        let mut b = edges.to_vec();
        a.sort();
        b.sort();
        if a == b {
            return s;
        }
    }
    panic!("tracés non consécutifs");
}

fn add(
    p: &mut Project,
    a: SeamSide,
    b: SeamSide,
    embu: Option<f64>,
    tol: Option<f64>,
) -> SeamPairId {
    p.apply(Command::AddSeamPair {
        a,
        b,
        embu_mm: embu,
        tolerance_mm: tol,
    })
    .unwrap()
    .seam_pairs[0]
}

fn set(
    p: &mut Project,
    id: SeamPairId,
    a: &SeamSide,
    b: &SeamSide,
    embu: Option<f64>,
    tol: Option<f64>,
) {
    p.apply(Command::SetSeamPair {
        id,
        a: a.clone(),
        b: b.clone(),
        embu_mm: embu,
        tolerance_mm: tol,
    })
    .unwrap();
}

struct Two {
    p: Project,
    s: SheetId,
    r1: PieceId,
    r2: PieceId,
    /// R1 205 × 50 en (0, 0) : bord du bas (205 mm).
    a: SeamSide,
    /// R2 120 × 80 en (500, 0) : bas + droite (120 + 80 = 200 mm).
    b: SeamSide,
    /// Autres tracés : R1 haut, R2 haut.
    r1_top: EntityId,
    r2_top: EntityId,
}

fn two() -> Two {
    let (mut p, s) = setup();
    let r1 = rect_piece(&mut p, s, 0.0, 0.0, 205.0, 50.0, "Dessus");
    let r2 = rect_piece(&mut p, s, 500.0, 0.0, 120.0, 80.0, "Dessous");
    let a = side(&p, r1, &[edge(&p, s, (0.0, 0.0), (205.0, 0.0))]);
    let b = side(
        &p,
        r2,
        &[
            edge(&p, s, (500.0, 0.0), (620.0, 0.0)),
            edge(&p, s, (620.0, 0.0), (620.0, 80.0)),
        ],
    );
    let r1_top = edge(&p, s, (0.0, 50.0), (205.0, 50.0));
    let r2_top = edge(&p, s, (500.0, 80.0), (620.0, 80.0));
    Two {
        p,
        s,
        r1,
        r2,
        a,
        b,
        r1_top,
        r2_top,
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

// ======================================================================== CA1

#[test]
fn ca1_longueurs_ecart_embu_et_statut_a_la_main() {
    let mut t = two();
    let id = add(&mut t.p, t.a.clone(), t.b.clone(), None, None);
    let r = t.p.seam_pair_report();
    assert_eq!(r.len(), 1);
    let q = &r[0];
    assert_eq!(q.number, 1);
    // A : 205 ; B : 120 + 80 = 200 ; écart 5 ; embu mesuré 205 − 200 = 5 ; attendu 0 ; défaut 1 mm.
    assert!(close(q.length_a_mm.unwrap(), 205.0));
    assert!(close(q.length_b_mm.unwrap(), 200.0));
    assert!(close(q.gap_mm.unwrap(), 5.0));
    assert!(close(q.embu_measured_mm.unwrap(), 5.0));
    assert!(close(q.embu_expected_mm, 0.0));
    assert!(close(q.tolerance_mm, 1.0));
    assert!(close(q.deviation_mm.unwrap(), 5.0));
    assert_eq!(q.status, SeamStatus::OutOfTolerance);
    // Embu attendu 5 : conforme (écart à l'attendu 0).
    set(&mut t.p, id, &t.a, &t.b, Some(5.0), None);
    let q = &t.p.seam_pair_report()[0];
    assert_eq!(q.status, SeamStatus::Within);
    assert!(close(q.deviation_mm.unwrap(), 0.0));
    // Embu attendu 4, tolérance 1 : écart à l'attendu exactement 1 → limite **incluse**.
    set(&mut t.p, id, &t.a, &t.b, Some(4.0), Some(1.0));
    assert_eq!(t.p.seam_pair_report()[0].status, SeamStatus::Within);
    // Embu attendu 3, tolérance 1 : écart 2 → hors tolérance.
    set(&mut t.p, id, &t.a, &t.b, Some(3.0), Some(1.0));
    assert_eq!(t.p.seam_pair_report()[0].status, SeamStatus::OutOfTolerance);
    // Tolérance 0 et embu exact : conforme ; embu 5.5 : hors tolérance.
    set(&mut t.p, id, &t.a, &t.b, Some(5.0), Some(0.0));
    assert_eq!(t.p.seam_pair_report()[0].status, SeamStatus::Within);
    set(&mut t.p, id, &t.a, &t.b, Some(5.5), Some(0.0));
    assert_eq!(t.p.seam_pair_report()[0].status, SeamStatus::OutOfTolerance);
    // Tolérance 5 sans embu : écart 5 ≤ 5 → conforme.
    set(&mut t.p, id, &t.a, &t.b, None, Some(5.0));
    assert_eq!(t.p.seam_pair_report()[0].status, SeamStatus::Within);
}

#[test]
fn ca1_cote_a_plus_court_embu_negatif_et_couples_numerotes() {
    let mut t = two();
    // Côtés échangés : A = 200, B = 205 → embu mesuré −5, écart 5.
    let id = add(&mut t.p, t.b.clone(), t.a.clone(), None, None);
    let q = &t.p.seam_pair_report()[0];
    assert!(close(q.embu_measured_mm.unwrap(), -5.0));
    assert!(close(q.gap_mm.unwrap(), 5.0));
    assert_eq!(q.status, SeamStatus::OutOfTolerance);
    // Un embu attendu de 5 ne rend pas conforme un côté A plus court : écart à l'attendu 10.
    set(&mut t.p, id, &t.b, &t.a, Some(5.0), None);
    let q = &t.p.seam_pair_report()[0];
    assert!(close(q.deviation_mm.unwrap(), 10.0));
    assert_eq!(q.status, SeamStatus::OutOfTolerance);
    // Deuxième couple (hauts) : numéro 2, longueurs 205 et 120.
    let a2 = side(&t.p, t.r1, &[t.r1_top]);
    let b2 = side(&t.p, t.r2, &[t.r2_top]);
    let id2 = add(&mut t.p, a2, b2, Some(85.0), Some(0.5));
    let r = t.p.seam_pair_report();
    assert_eq!(r.len(), 2);
    assert_eq!((r[1].id, r[1].number), (id2, 2));
    assert!(close(r[1].length_a_mm.unwrap(), 205.0));
    assert!(close(r[1].length_b_mm.unwrap(), 120.0));
    assert_eq!(r[1].status, SeamStatus::Within);
}

#[test]
fn ca1_courbe_arc_longueur_developpee() {
    let (mut p, s) = setup();
    // Quart de disque de rayon 100 : arc (longueur π/2 × 100) et deux rayons.
    let arc = p
        .apply(Command::AddArc {
            sheet: s,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: 100.0,
            start_deg: 0.0,
            end_deg: 90.0,
            ccw: true,
        })
        .unwrap()
        .entities[0];
    let r1 = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(0.0, 0.0)),
            to: PointRef::New(pt(100.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let r2 = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(0.0, 0.0)),
            to: PointRef::New(pt(0.0, 100.0)),
        })
        .unwrap()
        .entities[0];
    let q = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths {
                entities: vec![arc, r1, r2],
            },
            name: "Quart".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let t = rect_piece(&mut p, s, 300.0, 0.0, 157.0, 40.0, "Bande");
    let a = side(&p, q, &[arc]);
    let b = side(&p, t, &[edge(&p, s, (300.0, 0.0), (457.0, 0.0))]);
    add(&mut p, a, b, None, Some(0.1));
    let l = std::f64::consts::FRAC_PI_2 * 100.0; // 157,0796326…
    let r = &p.seam_pair_report()[0];
    assert!((r.length_a_mm.unwrap() - l).abs() < 1e-6, "{:?}", r);
    assert!(close(r.length_b_mm.unwrap(), 157.0));
    assert!((r.gap_mm.unwrap() - (l - 157.0)).abs() < 1e-6);
    assert_eq!(r.status, SeamStatus::Within); // 0,0796 ≤ 0,1 hors embu attendu
}

// ======================================================================== CA2

#[test]
fn ca2_longueur_de_couture_pas_de_coupe() {
    let mut t = two();
    add(&mut t.p, t.a.clone(), t.b.clone(), Some(5.0), None);
    let before = t.p.seam_pair_report();
    // Marge de 10 mm sur les deux pièces : le contour de coupe grandit, la couture non.
    for piece in [t.r1, t.r2] {
        t.p.apply(Command::SetSeamAllowance {
            piece,
            seam: Some(SeamAllowance {
                default_mm: 10.0,
                edges: vec![],
                hems: vec![],
                folds: vec![],
                corners: vec![],
                max_corner_mm: None,
            }),
        })
        .unwrap();
    }
    assert!(t.p.piece(t.r1).unwrap().seam.is_some());
    // La coupe de R1 est bien plus longue (205 + 20 sur le bord du bas), mais le rapport est identique.
    let cut = t.p.piece_cut_contour(t.r1).unwrap();
    // Coupe de R1 : (205 + 2 × 10) × 2 + (50 + 2 × 10) × 2 = 590 mm, à la main.
    assert!(close(cut.perimeter_mm, 590.0), "{cut:?}");
    assert_eq!(t.p.seam_pair_report(), before);
    assert!(close(before[0].length_a_mm.unwrap(), 205.0));
    assert_eq!(before[0].status, SeamStatus::Within);
}

// ======================================================================== CA3

#[test]
fn ca3_mise_a_jour_en_direct_sans_toucher_aux_couples() {
    let mut t = two();
    add(&mut t.p, t.a.clone(), t.b.clone(), Some(5.0), None);
    assert_eq!(t.p.seam_pair_report()[0].status, SeamStatus::Within);
    let seams = t.p.seams.clone();
    // Déplace le coin (620, 0) de R2 en (620, −10) : bas = √(120² + 10²) ... on veut un calcul
    // exact : déplace plutôt le coin haut-droit (620, 80) en (620, 90) : côté droit 90 → B = 210.
    let sheet = t.p.sheet(t.s).unwrap();
    let corner = sheet
        .points
        .iter()
        .find(|q| {
            sheet
                .position(q.id)
                .is_ok_and(|v| close(v.x, 620.0) && close(v.y, 80.0))
        })
        .unwrap()
        .id;
    t.p.apply(Command::MovePoint {
        sheet: t.s,
        point: corner,
        to: pt(620.0, 90.0),
    })
    .unwrap();
    assert_eq!(t.p.seams, seams, "les couples ne sont pas modifiés");
    let q = &t.p.seam_pair_report()[0];
    assert!(close(q.length_b_mm.unwrap(), 210.0));
    assert!(close(q.embu_measured_mm.unwrap(), -5.0)); // 205 − 210
    assert!(close(q.gap_mm.unwrap(), 5.0));
    assert_eq!(q.status, SeamStatus::OutOfTolerance); // attendu 5, mesuré −5 : écart 10
}

// ======================================================================== CA4

fn refused(p: &mut Project, c: Command) -> String {
    let before = bytes(p);
    let e = p.apply(c).unwrap_err().to_string();
    assert_eq!(before, bytes(p), "un refus ne laisse aucune trace");
    e
}

#[test]
fn ca4_refus_sans_trace_octets_identiques() {
    let mut t = two();
    let id = add(&mut t.p, t.a.clone(), t.b.clone(), None, None);
    let c = |a: SeamSide, b: SeamSide, e: Option<f64>, tol: Option<f64>| Command::AddSeamPair {
        a,
        b,
        embu_mm: e,
        tolerance_mm: tol,
    };
    let top1 = side(&t.p, t.r1, &[t.r1_top]);
    let top2 = side(&t.p, t.r2, &[t.r2_top]);
    // Pièce inconnue.
    let ghost = SeamSide {
        piece: PieceId(999_999),
        edges: top1.edges.clone(),
    };
    assert!(refused(&mut t.p, c(ghost, top2.clone(), None, None)).contains("pièce inconnue"));
    // Aucun tracé.
    let empty = SeamSide {
        piece: t.r1,
        edges: vec![],
    };
    assert!(refused(&mut t.p, c(empty, top2.clone(), None, None)).contains("aucun tracé"));
    // Tracé d'une autre pièce.
    let alien = SeamSide {
        piece: t.r1,
        edges: top2.edges.clone(),
    };
    assert!(refused(&mut t.p, c(alien, top2.clone(), None, None)).contains("contour"));
    // Tracés qui ne se suivent pas : bas et haut de R1 (séparés par un côté).
    let bottom = t.a.edges[0];
    let apart = SeamSide {
        piece: t.r1,
        edges: vec![bottom, t.r1_top],
    };
    assert!(refused(&mut t.p, c(apart, top2.clone(), None, None)).contains("ne se suivent pas"));
    // Doublon dans un côté.
    let dup = SeamSide {
        piece: t.r1,
        edges: vec![t.r1_top, t.r1_top],
    };
    assert!(refused(&mut t.p, c(dup, top2.clone(), None, None)).contains("ne se suivent pas"));
    // Même tracé des deux côtés.
    assert!(refused(&mut t.p, c(top1.clone(), top1.clone(), None, None)).contains("déjà cousu"));
    // Tracé déjà cousu dans un autre couple.
    assert!(refused(&mut t.p, c(t.a.clone(), top2.clone(), None, None)).contains("déjà cousu"));
    // Valeurs hors bornes ou non finies.
    for bad in [-0.1, 1000.1, f64::NAN, f64::INFINITY, -1.0] {
        assert!(refused(&mut t.p, c(top1.clone(), top2.clone(), Some(bad), None)).contains("embu"));
        assert!(
            refused(&mut t.p, c(top1.clone(), top2.clone(), None, Some(bad))).contains("tolérance")
        );
    }
    // Couple inconnu : modification et retrait.
    let ghost_id = SeamPairId(999_999);
    assert!(refused(
        &mut t.p,
        Command::SetSeamPair {
            id: ghost_id,
            a: top1.clone(),
            b: top2.clone(),
            embu_mm: None,
            tolerance_mm: None
        }
    )
    .contains("inconnue"));
    assert!(refused(&mut t.p, Command::RemoveSeamPair { id: ghost_id }).contains("inconnue"));
    // Modification vers un tracé déjà cousu par un autre couple.
    let id2 = add(&mut t.p, top1.clone(), top2.clone(), None, None);
    assert!(refused(
        &mut t.p,
        Command::SetSeamPair {
            id: id2,
            a: t.a.clone(),
            b: top2.clone(),
            embu_mm: None,
            tolerance_mm: None
        }
    )
    .contains("déjà cousu"));
    // Bornes incluses acceptées.
    set(&mut t.p, id, &t.a, &t.b, Some(1000.0), Some(1000.0));
    set(&mut t.p, id, &t.a, &t.b, Some(0.0), Some(0.0));
}

#[test]
fn ca4_meme_piece_des_deux_cotes_autorise_sans_arete_commune() {
    let (mut p, s) = setup();
    let r = rect_piece(&mut p, s, 0.0, 0.0, 100.0, 50.0, "Tube");
    let bottom = edge(&p, s, (0.0, 0.0), (100.0, 0.0));
    let top = edge(&p, s, (0.0, 50.0), (100.0, 50.0));
    let a = side(&p, r, &[bottom]);
    let b = side(&p, r, &[top]);
    add(&mut p, a.clone(), b, None, None);
    let q = &p.seam_pair_report()[0];
    assert!(close(q.length_a_mm.unwrap(), 100.0) && close(q.length_b_mm.unwrap(), 100.0));
    assert_eq!(q.status, SeamStatus::Within);
    // Une arête commune aux deux côtés est refusée.
    let both = side(&p, r, &[bottom, edge(&p, s, (100.0, 0.0), (100.0, 50.0))]);
    let e = refused(
        &mut p,
        Command::AddSeamPair {
            a: both,
            b: a,
            embu_mm: None,
            tolerance_mm: None,
        },
    );
    assert!(e.contains("déjà cousu"), "{e}");
}

#[test]
fn ca4_cote_qui_fait_le_tour_de_la_fermeture() {
    let (mut p, s) = setup();
    let r = rect_piece(&mut p, s, 0.0, 0.0, 100.0, 50.0, "Tour");
    let order: Vec<EntityId> = p
        .piece(r)
        .unwrap()
        .outline
        .iter()
        .map(|e| e.entity)
        .collect();
    // Dernier tracé puis premier : suite continue qui passe par la fermeture.
    let wrap = p.seam_side_range(r, order[3], 2).unwrap();
    assert_eq!(wrap.edges, vec![order[3], order[0]]);
    let other = rect_piece(&mut p, s, 300.0, 0.0, 10.0, 10.0, "Petit");
    let b = side(&p, other, &[edge(&p, s, (300.0, 0.0), (310.0, 0.0))]);
    add(&mut p, wrap, b, None, None);
    assert!(p.seam_pair_report()[0].length_a_mm.is_some());
    // Trop de tracés : refusé par l'aide de construction.
    assert!(p.seam_side_range(r, order[0], 5).is_err());
    assert!(p.seam_side_range(r, order[0], 0).is_err());
}

// ======================================================================== CA5

#[test]
fn ca5_une_etape_annulable_et_noop_sans_etape() {
    let t = two();
    let mut d = Document::new(t.p.clone());
    let start = bytes(d.project());
    let o = d
        .execute(Command::AddSeamPair {
            a: t.a.clone(),
            b: t.b.clone(),
            embu_mm: Some(5.0),
            tolerance_mm: None,
        })
        .unwrap();
    let id = o.seam_pairs[0];
    assert_eq!(d.project().seams.len(), 1);
    assert!(d.undo());
    assert_eq!(bytes(d.project()), start);
    assert!(!d.can_undo(), "une seule étape pour déclarer un couple");
    assert!(d.redo());
    // Modification identique : aucune étape ; modification réelle : une étape.
    d.execute(Command::SetSeamPair {
        id,
        a: t.a.clone(),
        b: t.b.clone(),
        embu_mm: Some(5.0),
        tolerance_mm: None,
    })
    .unwrap();
    assert!(d.undo()); // annule l'ajout, pas un no-op
    assert!(d.project().seams.is_empty());
    assert!(d.redo());
    d.execute(Command::SetSeamPair {
        id,
        a: t.a.clone(),
        b: t.b.clone(),
        embu_mm: Some(2.0),
        tolerance_mm: None,
    })
    .unwrap();
    assert!(d.undo());
    assert_eq!(d.project().seams[0].embu_mm, Some(5.0));
    // Retrait : une étape.
    assert!(d.redo());
    d.execute(Command::RemoveSeamPair { id }).unwrap();
    assert!(d.project().seams.is_empty());
    assert!(d.undo());
    assert_eq!(d.project().seams.len(), 1);
    // Un refus ne crée pas d'étape.
    let mut d2 = Document::new(t.p);
    assert!(d2.execute(Command::RemoveSeamPair { id }).is_err());
    assert!(!d2.can_undo());
}

// ======================================================================== CA6

#[test]
fn ca6_suppression_d_un_trace_retire_le_couple_avec_remarque() {
    let mut t = two();
    let a = t.a.clone();
    add(&mut t.p, a, t.b.clone(), None, None);
    // Troisième pièce et second couple (haut de R2 avec le bas de R3), étranger à R1.
    let r3 = rect_piece(&mut t.p, t.s, 900.0, 0.0, 120.0, 30.0, "Doublure");
    let top2 = side(&t.p, t.r2, &[t.r2_top]);
    let bottom3 = side(&t.p, r3, &[edge(&t.p, t.s, (900.0, 0.0), (1020.0, 0.0))]);
    let keep = add(&mut t.p, top2, bottom3, None, None);
    // Supprimer le tracé du bas de R1 (cité par le premier couple seulement).
    let o =
        t.p.apply(Command::Delete {
            sheet: t.s,
            points: vec![],
            entities: vec![t.a.edges[0]],
        })
        .unwrap();
    assert_eq!(t.p.seams.len(), 1);
    assert_eq!(t.p.seams[0].id, keep, "le couple étranger à R1 reste");
    let notes: Vec<&String> = o
        .notes
        .iter()
        .filter(|n| n.contains("couture appariée"))
        .collect();
    assert_eq!(notes.len(), 1, "{:?}", o.notes);
    assert!(
        notes[0].contains("Dessus") && notes[0].contains("Dessous") && notes[0].contains("retirée")
    );
    assert!(t.p.seam_pair_report()[0].length_a_mm.is_some());
}

#[test]
fn ca6_suppression_d_une_piece_et_sortie_du_contour() {
    let mut t = two();
    add(&mut t.p, t.a.clone(), t.b.clone(), None, None);
    let before = t.p.clone();
    let o = t.p.apply(Command::DeletePiece { piece: t.r2 }).unwrap();
    assert!(t.p.seams.is_empty());
    assert_eq!(
        o.notes
            .iter()
            .filter(|n| n.contains("couture appariée"))
            .count(),
        1
    );
    // Sortie d'un tracé du contour par SetPieceOutline : retrait avec remarque.
    let mut p = before;
    let outline: Vec<PieceEdge> = p
        .piece(t.r1)
        .unwrap()
        .outline
        .iter()
        .copied()
        .filter(|e| e.entity != t.a.edges[0])
        .collect();
    let o = p
        .apply(Command::SetPieceOutline {
            piece: t.r1,
            outline,
            grainline: None,
        })
        .unwrap();
    assert!(p.seams.is_empty(), "{:?}", o.notes);
    assert!(o.notes.iter().any(|n| n.contains("couture appariée")));
}

// ======================================================================== CA7

#[test]
fn ca7_contour_ouvert_non_calculable_jamais_un_refus() {
    let mut t = two();
    let b2 = side(&t.p, t.r2, &[t.r2_top]);
    let a2 = side(&t.p, t.r1, &[t.r1_top]);
    add(&mut t.p, a2, b2, None, None);
    // On retire du contour de R1 un tracé qui n'est pas cité (le côté gauche) : contour ouvert.
    let left = edge(&t.p, t.s, (0.0, 0.0), (0.0, 50.0));
    let outline: Vec<PieceEdge> =
        t.p.piece(t.r1)
            .unwrap()
            .outline
            .iter()
            .copied()
            .filter(|e| e.entity != left)
            .collect();
    t.p.apply(Command::SetPieceOutline {
        piece: t.r1,
        outline,
        grainline: None,
    })
    .expect("la commande de la mère n'est pas refusée");
    assert_eq!(t.p.seams.len(), 1, "le couple reste déclaré");
    let q = &t.p.seam_pair_report()[0];
    assert_eq!(
        q.status,
        SeamStatus::NotComputable {
            side: SeamWhich::A,
            reason: SeamIssue::OutlineOpen
        }
    );
    assert!(q.length_a_mm.is_none() && q.gap_mm.is_none() && q.embu_measured_mm.is_none());
    assert!(
        close(q.length_b_mm.unwrap(), 120.0),
        "l'autre côté reste lu"
    );
}

// ======================================================================== CA8

fn pair_project() -> Two {
    let mut t = two();
    add(&mut t.p, t.a.clone(), t.b.clone(), Some(5.0), Some(0.5));
    t
}

#[test]
fn ca8_aller_retour_json_omis_si_vide_et_etiquettes() {
    let t = pair_project();
    let text = bytes(&t.p);
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, t.p);
    assert_eq!(bytes(&back), text, "relu puis réécrit à l'identique");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["schema_version"], 6);
    assert_eq!(v["seams"][0]["embu_mm"], 5.0);
    assert_eq!(v["seams"][0]["tolerance_mm"], 0.5);
    assert_eq!(v["seams"][0]["a"]["edges"].as_array().unwrap().len(), 1);
    assert_eq!(v["seams"][0]["b"]["edges"].as_array().unwrap().len(), 2);
    // Aucune mesure n'est écrite.
    for k in ["length", "gap", "status", "measured"] {
        assert!(!text.contains(k), "{k} ne doit pas être stocké");
    }
    // Retrait : la clé disparaît, le fichier redevient celui d'avant.
    let mut q = t.p.clone();
    q.apply(Command::RemoveSeamPair { id: q.seams[0].id })
        .unwrap();
    assert!(!bytes(&q).contains("\"seams\""));
    // Embu / tolérance absents : omis.
    let mut q = t.p.clone();
    let id = q.seams[0].id;
    set(&mut q, id, &t.a, &t.b, None, None);
    let w = bytes(&q);
    assert!(w.contains("\"seams\"") && !w.contains("embu_mm") && !w.contains("tolerance_mm"));
    assert_eq!(bytes(&persist::from_json(&w).unwrap()), w);
}

fn broken(t: &Two, edit: impl FnOnce(&mut Value)) -> Result<Project, CoreError> {
    let mut v: Value = serde_json::from_str(&bytes(&t.p)).unwrap();
    edit(&mut v);
    persist::from_json(&v.to_string())
}

#[test]
fn ca8_chargeur_strict() {
    let t = pair_project();
    let ok = broken(&t, |_| {});
    assert!(ok.is_ok());
    // Clé en trop : couple, côté.
    assert!(broken(&t, |v| v["seams"][0]["inconnue"] = json!(1)).is_err());
    assert!(broken(&t, |v| v["seams"][0]["a"]["inconnue"] = json!(1)).is_err());
    // Référence cassée : pièce, tracé hors contour, tracés qui ne se suivent pas, côté vide.
    assert!(broken(&t, |v| v["seams"][0]["a"]["piece"] = json!(987_654)).is_err());
    assert!(broken(&t, |v| v["seams"][0]["a"]["edges"] = json!([987_654])).is_err());
    assert!(broken(&t, |v| v["seams"][0]["a"]["edges"] = json!([])).is_err());
    let alien = t.b.edges[0].0;
    assert!(broken(&t, |v| v["seams"][0]["a"]["edges"] = json!([alien])).is_err());
    let (b0, b1) = (t.b.edges[0].0, t.b.edges[1].0);
    assert!(broken(&t, |v| v["seams"][0]["b"]["edges"] = json!([b1, b0])).is_err());
    // Tracé cousu deux fois : même arête des deux côtés.
    assert!(broken(&t, |v| v["seams"][0]["b"] = v["seams"][0]["a"].clone()).is_err());
    // Valeurs hors bornes ou de mauvais type.
    assert!(broken(&t, |v| v["seams"][0]["embu_mm"] = json!(1000.5)).is_err());
    assert!(broken(&t, |v| v["seams"][0]["embu_mm"] = json!(-1)).is_err());
    assert!(broken(&t, |v| v["seams"][0]["tolerance_mm"] = json!(1001)).is_err());
    assert!(broken(&t, |v| v["seams"][0]["tolerance_mm"] = json!("1")).is_err());
    // Identifiant dupliqué ou incohérent avec le compteur.
    let piece_id = t.r1.0;
    assert!(broken(&t, |v| v["seams"][0]["id"] = json!(piece_id)).is_err());
    assert!(broken(&t, |v| v["seams"][0]["id"] = json!(0)).is_err());
    // Clé dupliquée dans le texte (D-02).
    let text = bytes(&t.p).replacen("\"embu_mm\"", "\"embu_mm\": 1, \"embu_mm\"", 1);
    assert!(persist::from_json(&text).is_err());
}

#[test]
fn ca8_pf_validate_code_0_puis_1() {
    let t = pair_project();
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "pf-coutures-validate-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("coutures.pforge.json");
    persist::save(&path, &t.p).unwrap();
    let run = |path: &PathBuf| {
        std::process::Command::new(env!("CARGO_BIN_EXE_pf-validate"))
            .arg(path)
            .output()
            .unwrap()
    };
    let out = run(&path);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        text.replacen("\"embu_mm\": 5.0", "\"embu_mm\": 5000.0", 1),
    )
    .unwrap();
    assert_eq!(run(&path).status.code(), Some(1));
    std::fs::write(
        &path,
        text.replacen("\"tolerance_mm\"", "\"inconnue\": 1, \"tolerance_mm\"", 1),
    )
    .unwrap();
    assert_eq!(run(&path).status.code(), Some(1));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ca8_anciens_fichiers_et_exemples_relus_a_l_identique() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let examples = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut n = 0;
    for dir in [fixtures, examples] {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if !(path.to_string_lossy().ends_with(".pforge")
                || path.to_string_lossy().ends_with(".pforge.json"))
            {
                continue;
            }
            let t = std::fs::read_to_string(&path)
                .unwrap()
                .replace("\r\n", "\n");
            let q = persist::from_json(&t).unwrap();
            assert!(q.seams.is_empty(), "{}", path.display());
            if t.contains("\"schema_version\": 6") {
                assert_eq!(t, bytes(&q), "{}", path.display());
            }
            n += 1;
        }
    }
    assert!(n >= 6);
}

// ======================================================================== CA9

#[test]
fn ca9_copie_de_feuille_ne_reprend_pas_les_couples_avec_remarque() {
    let mut t = pair_project();
    let before = t.p.seams.clone();
    let o = t.p.apply(Command::DuplicateSheet { sheet: t.s }).unwrap();
    assert_eq!(t.p.seams, before, "le couple d'origine est intact");
    assert_eq!(t.p.pieces.len(), 4);
    assert_eq!(
        o.notes
            .iter()
            .filter(|n| n.contains("coutures appariées"))
            .count(),
        2,
        "une remarque par pièce copiée citée : {:?}",
        o.notes
    );
    assert!(t.p.seam_pair_report()[0].length_a_mm.is_some());
}

#[test]
fn ca9_separer_decouper_assembler_retirent_le_couple_avec_remarque() {
    // Séparer R1 : la pièce d'origine est remplacée, le couple est retiré.
    let mut t = pair_project();
    let o =
        t.p.apply(Command::SplitPiece {
            piece: t.r1,
            a: pt(100.0, -10.0),
            b: pt(100.0, 60.0),
        })
        .unwrap();
    assert!(t.p.seams.is_empty());
    assert!(
        o.notes.iter().any(|n| n.contains("couture appariée")),
        "{:?}",
        o.notes
    );
    // Découper : idem.
    let mut t = pair_project();
    let o =
        t.p.apply(Command::CutPiece {
            piece: t.r2,
            line: CutLine::Points {
                a: pt(560.0, -10.0),
                b: pt(560.0, 90.0),
            },
            notches: CutNotches::Count { count: 0 },
        })
        .unwrap();
    assert!(t.p.seams.is_empty());
    assert!(o.notes.iter().any(|n| n.contains("couture appariée")));
    // Assembler deux pièces de 100 × 50 par un côté de 50 mm : les deux sont remplacées.
    let (mut p, sh) = setup();
    let q1 = rect_piece(&mut p, sh, 0.0, 0.0, 100.0, 50.0, "Gauche");
    let q2 = rect_piece(&mut p, sh, 300.0, 0.0, 100.0, 50.0, "Droite");
    let e1 = edge(&p, sh, (0.0, 0.0), (100.0, 0.0));
    let e2 = edge(&p, sh, (300.0, 0.0), (400.0, 0.0));
    let (sa, sb) = (side(&p, q1, &[e1]), side(&p, q2, &[e2]));
    add(&mut p, sa, sb, None, None);
    let o = p
        .apply(Command::JoinPieces {
            first: q1,
            first_edge: edge(&p, sh, (100.0, 0.0), (100.0, 50.0)),
            second: q2,
            second_edge: edge(&p, sh, (300.0, 0.0), (300.0, 50.0)),
            tolerance_mm: 10.0,
        })
        .unwrap();
    assert!(p.seams.is_empty());
    assert!(
        o.notes.iter().any(|n| n.contains("couture appariée")),
        "{:?}",
        o.notes
    );
}

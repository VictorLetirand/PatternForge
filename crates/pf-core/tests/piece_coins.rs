//! US-05.2 — types de coins. Un groupe de tests par critère d'acceptation :
//! CA1 un test géométrique par type (coordonnées calculées à la main), CA2 coin choisi par
//! sommet, CA3 pointe très aiguë bornée (longueur maximale paramétrable), CA4 refus sans trace,
//! CA5 une étape annulable, CA6 échec de calcul = contrôle, CA7 cohérence (suppression, copie,
//! séparer), CA8 fichier (schéma 3, anciens fichiers à l'identique, chargeur strict).
//!
//! Parallélogramme (0,0) (100,0) (160,80) (60,80), marge 10 : directions de couture (1,0),
//! (0,6 ; 0,8), (-1,0), (-0,6 ; -0,8). Onglets : X0 = (-20,-10), X1 = (105,-10), X2 = (180,90),
//! X3 = (55,90). Sommet 1 = (100,0) : E = (100,-10), S = (108,-6) (calculs détaillés dans
//! `pf-geometry/src/seam_corner_tests.rs`, repris ici à la main pour le chemin complet de la commande).

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::{distance_mm, PointMm, Trace};
use serde_json::Value;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn near(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
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
    let ids = (0..n)
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

fn ends_of(shape: &Shape) -> (PointMm, PointMm) {
    match shape {
        Shape::Line(a, b) => (*a, *b),
        other => panic!("forme inattendue {other:?}"),
    }
}

fn edge_between(p: &Project, piece: PieceId, a: PointMm, b: PointMm) -> EntityId {
    let sheet = p.piece_sheet(piece).unwrap();
    p.piece(piece)
        .unwrap()
        .outline
        .iter()
        .find(|e| {
            let (s, t) = ends_of(&sheet.shape(e.entity).unwrap());
            (distance_mm(s, a) < 1e-6 && distance_mm(t, b) < 1e-6)
                || (distance_mm(s, b) < 1e-6 && distance_mm(t, a) < 1e-6)
        })
        .unwrap_or_else(|| panic!("aucun bord entre {a:?} et {b:?}"))
        .entity
}

fn para() -> (Project, SheetId, PieceId) {
    let mut p = Project::new("Coins");
    let s = p.sheets[0].id;
    let piece = poly_piece(
        &mut p,
        s,
        &[
            pt(0.0, 0.0),
            pt(100.0, 0.0),
            pt(160.0, 80.0),
            pt(60.0, 80.0),
        ],
        "Pièce",
    );
    (p, s, piece)
}

/// Tracé qui **commence** au sommet 1 = (100,0) (contour anti-horaire).
fn v1(p: &Project, piece: PieceId) -> EntityId {
    edge_between(p, piece, pt(100.0, 0.0), pt(160.0, 80.0))
}

/// Tracé qui commence au sommet 2 = (160,80).
fn v2(p: &Project, piece: PieceId) -> EntityId {
    edge_between(p, piece, pt(160.0, 80.0), pt(60.0, 80.0))
}

fn seam_with(
    default_mm: f64,
    corners: Vec<CornerAllowance>,
    max_corner_mm: Option<f64>,
) -> Option<SeamAllowance> {
    Some(SeamAllowance {
        default_mm,
        corners,
        max_corner_mm,
        ..Default::default()
    })
}

fn corner(entity: EntityId, corner: CornerType) -> CornerAllowance {
    CornerAllowance { entity, corner }
}

fn set(p: &mut Project, piece: PieceId, s: Option<SeamAllowance>) -> Result<Outcome, CoreError> {
    p.apply(Command::SetSeamAllowance { piece, seam: s })
}

fn refused(p: &mut Project, c: Command) -> String {
    let (before, text) = (p.clone(), bytes(p));
    let err = p.apply(c).expect_err("la commande aurait dû être refusée");
    assert_eq!(*p, before, "refus sans trace (valeur)");
    assert_eq!(text, bytes(p), "refus sans trace (octets)");
    err.to_string()
}

fn vertices(p: &Project, piece: PieceId) -> Vec<PointMm> {
    p.piece_cut_contour(piece)
        .expect("contour de coupe")
        .traces
        .iter()
        .map(|t| match t {
            Trace::Line(a, _) => *a,
            other => panic!("segment attendu, reçu {other:?}"),
        })
        .collect()
}

fn assert_cycle(got: &[PointMm], want: &[(f64, f64)]) {
    assert_eq!(got.len(), want.len(), "nombre de sommets : {got:?}");
    let n = want.len();
    let ok = (0..n).any(|shift| {
        (0..n).all(|i| {
            let (g, w) = (got[(i + shift) % n], want[i]);
            near(g.x, w.0, 1e-9) && near(g.y, w.1, 1e-9)
        })
    });
    assert!(ok, "sommets {got:?} ≠ attendus {want:?}");
}

fn has(v: &[PointMm], w: (f64, f64)) -> bool {
    v.iter()
        .any(|q| near(q.x, w.0, 1e-9) && near(q.y, w.1, 1e-9))
}

fn with_corner_at_v1(c: CornerType) -> Vec<PointMm> {
    let (mut p, _, piece) = para();
    let e = v1(&p, piece);
    set(&mut p, piece, seam_with(10.0, vec![corner(e, c)], None)).unwrap();
    vertices(&p, piece)
}

fn smooth_piece() -> (Project, PieceId) {
    let mut q = Project::new("Lisse");
    let sq = q.sheets[0].id;
    let piece = poly_piece(
        &mut q,
        sq,
        &[
            pt(0.0, 0.0),
            pt(50.0, 0.0),
            pt(100.0, 0.0),
            pt(100.0, 60.0),
            pt(0.0, 60.0),
        ],
        "R",
    );
    (q, piece)
}

fn l_piece() -> (Project, PieceId) {
    let mut l = Project::new("L");
    let sl = l.sheets[0].id;
    let piece = poly_piece(
        &mut l,
        sl,
        &[
            pt(0.0, 0.0),
            pt(100.0, 0.0),
            pt(100.0, 50.0),
            pt(50.0, 50.0),
            pt(50.0, 100.0),
            pt(0.0, 100.0),
        ],
        "L",
    );
    (l, piece)
}

// ---------------------------------------------------------------- CA1

#[test]
fn ca1_onglet_par_defaut() {
    let (mut p, _, piece) = para();
    set(&mut p, piece, seam_with(10.0, vec![], None)).unwrap();
    assert_cycle(
        &vertices(&p, piece),
        &[(-20.0, -10.0), (105.0, -10.0), (180.0, 90.0), (55.0, 90.0)],
    );
}

#[test]
fn ca1_perpendiculaire_a_la_couture_precedente() {
    // L1 (y = -10) prolongée jusqu'à x = 108 (pied de la perpendiculaire passant par S).
    assert_cycle(
        &with_corner_at_v1(CornerType::PerpendicularPrevious {}),
        &[
            (-20.0, -10.0),
            (108.0, -10.0),
            (108.0, -6.0),
            (180.0, 90.0),
            (55.0, 90.0),
        ],
    );
}

#[test]
fn ca1_perpendiculaire_a_la_couture_suivante() {
    // Pied de la perpendiculaire à (0,6 ; 0,8) menée de E : S - 8 (0,6 ; 0,8) = (103,2 ; -12,4).
    assert_cycle(
        &with_corner_at_v1(CornerType::PerpendicularNext {}),
        &[
            (-20.0, -10.0),
            (100.0, -10.0),
            (103.2, -12.4),
            (180.0, 90.0),
            (55.0, 90.0),
        ],
    );
}

#[test]
fn ca1_symetrique_retour_de_couture() {
    // E et S à égale hauteur sur la bissectrice : coupe E → S.
    assert_cycle(
        &with_corner_at_v1(CornerType::Symmetric {}),
        &[
            (-20.0, -10.0),
            (100.0, -10.0),
            (108.0, -6.0),
            (180.0, 90.0),
            (55.0, 90.0),
        ],
    );
}

#[test]
fn ca1_tronque() {
    // Profondeur √5 / 2 sous la pointe : droite (p - V)·(1,-2) = 22,5 ; (102,5 ; -10) et (106,5 ; -8).
    assert_cycle(
        &with_corner_at_v1(CornerType::Truncated {
            depth_mm: 0.5 * 5f64.sqrt(),
        }),
        &[
            (-20.0, -10.0),
            (102.5, -10.0),
            (106.5, -8.0),
            (180.0, 90.0),
            (55.0, 90.0),
        ],
    );
}

#[test]
fn ca1_en_marche() {
    // M = V + 10 n1 + 10 n2 = (100,0) + (0,-10) + (8,-6) = (108,-16).
    assert_cycle(
        &with_corner_at_v1(CornerType::Stepped {}),
        &[
            (-20.0, -10.0),
            (100.0, -10.0),
            (108.0, -16.0),
            (108.0, -6.0),
            (180.0, 90.0),
            (55.0, 90.0),
        ],
    );
}

#[test]
fn ca1_ourlet_rabattu_pli_sur_la_couture_suivante() {
    // F = L1 ∩ couture suivante = (92,5 ; -10) ; miroir à 10 mm du pli : (96 ; -22) ; puis L2.
    assert_cycle(
        &with_corner_at_v1(CornerType::HemFoldNext {}),
        &[
            (-20.0, -10.0),
            (92.5, -10.0),
            (96.0, -22.0),
            (180.0, 90.0),
            (55.0, 90.0),
        ],
    );
}

#[test]
fn ca1_ourlet_rabattu_pli_sur_la_couture_precedente() {
    // L2 ∩ couture précédente (y = 0) = (112,5 ; 0) ; miroir à 10 mm du pli : (120 ; -10) sur L1.
    assert_cycle(
        &with_corner_at_v1(CornerType::HemFoldPrevious {}),
        &[
            (-20.0, -10.0),
            (120.0, -10.0),
            (112.5, 0.0),
            (180.0, 90.0),
            (55.0, 90.0),
        ],
    );
}

#[test]
fn ca1_les_mesures_observees_suivent_le_coin_choisi() {
    // Contour « en marche » : sommets (-20,-10) (100,-10) (108,-16) (108,-6) (180,90) (55,90).
    // Aire (lacet) = 12 420 mm² et périmètre = 120 + 10 + 10 + 120 + 125 + 125 + √(20² + 100²)…
    // calculés ici sommet par sommet, jamais relus du code testé.
    let (mut p, _, piece) = para();
    let e = v1(&p, piece);
    set(
        &mut p,
        piece,
        seam_with(10.0, vec![corner(e, CornerType::Stepped {})], None),
    )
    .unwrap();
    let c = p.piece_cut_contour(piece).unwrap();
    let pts = [
        (-20.0, -10.0),
        (100.0, -10.0),
        (108.0, -16.0),
        (108.0, -6.0),
        (180.0, 90.0),
        (55.0, 90.0),
    ];
    let n = pts.len();
    let (mut area, mut perimeter) = (0.0_f64, 0.0_f64);
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        area += a.0 * b.1 - b.0 * a.1;
        perimeter += ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
    }
    assert!(near(c.area_mm2, (area / 2.0).abs(), 1e-6), "{}", c.area_mm2);
    assert!(near(c.perimeter_mm, perimeter, 1e-6), "{}", c.perimeter_mm);
}

// ---------------------------------------------------------------- CA2

#[test]
fn ca2_le_coin_est_choisi_par_sommet() {
    let (mut p, _, piece) = para();
    let (a, b) = (v1(&p, piece), v2(&p, piece));
    set(
        &mut p,
        piece,
        seam_with(
            10.0,
            vec![
                corner(b, CornerType::Stepped {}),
                corner(a, CornerType::PerpendicularPrevious {}),
            ],
            None,
        ),
    )
    .unwrap();
    // Forme canonique : triés par tracé, quel que soit l'ordre fourni.
    let stored = p.piece(piece).unwrap().seam.clone().unwrap();
    assert!(stored.corners.windows(2).all(|w| w[0].entity < w[1].entity));
    let v = vertices(&p, piece);
    // Sommet 1 : (108,-10) ; sommet 2 en marche : M = (160,80) + (8,-6) + (0,10) = (168,84).
    assert!(has(&v, (108.0, -10.0)));
    assert!(has(&v, (168.0, 84.0)));
    // Les sommets 0 et 3 gardent l'onglet.
    assert!(has(&v, (-20.0, -10.0)));
    assert!(has(&v, (55.0, 90.0)));
}

#[test]
fn ca2_sommets_proposes_au_choix() {
    let (p, _, piece) = para();
    let vs = p.seam_vertices(piece).unwrap();
    assert_eq!(vs.len(), 4);
    assert!(vs.iter().all(|v| v.convex));
    let at1 = vs.iter().find(|v| v.entity == v1(&p, piece)).unwrap();
    assert!(distance_mm(at1.point, pt(100.0, 0.0)) < 1e-9);
    // Pièce en L : 6 sommets dont un rentrant, en (50,50).
    let (q, l) = l_piece();
    let vs = q.seam_vertices(l).unwrap();
    assert_eq!(vs.len(), 6);
    let reentrant: Vec<_> = vs.iter().filter(|v| !v.convex).collect();
    assert_eq!(reentrant.len(), 1);
    assert!(distance_mm(reentrant[0].point, pt(50.0, 50.0)) < 1e-9);
    // Côté coupé en deux segments alignés : le sommet lisse n'est pas proposé.
    let (r, piece) = smooth_piece();
    assert_eq!(r.seam_vertices(piece).unwrap().len(), 4);
}

// ---------------------------------------------------------------- CA3

#[test]
fn ca3_pointe_tres_aigue_bornee_par_la_longueur_maximale() {
    // Sommets 0 et 2 : pointe à √500 = 22,36 mm. Défaut (40 mm) : onglet ; limite 20 : biseaux
    // (-8,6) → (0,-10) et (168,74) → (160,90) ; limite 30 : onglet.
    let (mut p, _, piece) = para();
    set(&mut p, piece, seam_with(10.0, vec![], None)).unwrap();
    assert_eq!(vertices(&p, piece).len(), 4);
    set(&mut p, piece, seam_with(10.0, vec![], Some(20.0))).unwrap();
    assert_cycle(
        &vertices(&p, piece),
        &[
            (55.0, 90.0),
            (-8.0, 6.0),
            (0.0, -10.0),
            (105.0, -10.0),
            (168.0, 74.0),
            (160.0, 90.0),
        ],
    );
    set(&mut p, piece, seam_with(10.0, vec![], Some(30.0))).unwrap();
    assert_cycle(
        &vertices(&p, piece),
        &[(-20.0, -10.0), (105.0, -10.0), (180.0, 90.0), (55.0, 90.0)],
    );
}

#[test]
fn ca3_la_longueur_maximale_est_validee() {
    let (mut p, _, piece) = para();
    for bad in [-1.0, f64::NAN, f64::INFINITY, MAX_CORNER_MM + 0.001] {
        let msg = refused(
            &mut p,
            Command::SetSeamAllowance {
                piece,
                seam: seam_with(10.0, vec![], Some(bad)),
            },
        );
        assert!(msg.contains("pointe"), "{msg}");
    }
    // Les bornes sont acceptées ; une longueur de 0 coupe tous les coins en biseau.
    set(&mut p, piece, seam_with(10.0, vec![], Some(0.0))).unwrap();
    assert_eq!(vertices(&p, piece).len(), 8);
    set(&mut p, piece, seam_with(10.0, vec![], Some(MAX_CORNER_MM))).unwrap();
}

#[test]
fn ca3_une_longueur_maximale_sans_marge_est_une_donnee_valide() {
    let (mut p, _, piece) = para();
    set(&mut p, piece, seam_with(0.0, vec![], Some(25.0))).unwrap();
    assert!(p.piece(piece).unwrap().seam.is_some());
    // Marge nulle : le contour de coupe est le contour de couture.
    assert_cycle(
        &vertices(&p, piece),
        &[(0.0, 0.0), (100.0, 0.0), (160.0, 80.0), (60.0, 80.0)],
    );
}

// ---------------------------------------------------------------- CA4

#[test]
fn ca4_refus_sans_trace() {
    let (mut p, s, piece) = para();
    let a = v1(&p, piece);
    set(&mut p, piece, seam_with(10.0, vec![], None)).unwrap();
    let cmd = |seam| Command::SetSeamAllowance { piece, seam };
    let ghost = EntityId(9_999_999);
    let cases = [
        seam_with(10.0, vec![corner(ghost, CornerType::Stepped {})], None),
        seam_with(
            10.0,
            vec![
                corner(a, CornerType::Stepped {}),
                corner(a, CornerType::Symmetric {}),
            ],
            None,
        ),
        seam_with(
            10.0,
            vec![corner(a, CornerType::Truncated { depth_mm: -1.0 })],
            None,
        ),
        seam_with(
            10.0,
            vec![corner(a, CornerType::Truncated { depth_mm: f64::NAN })],
            None,
        ),
        seam_with(
            10.0,
            vec![corner(
                a,
                CornerType::Truncated {
                    depth_mm: MAX_CORNER_MM + 1.0,
                },
            )],
            None,
        ),
    ];
    for c in cases {
        refused(&mut p, cmd(c));
    }
    // Sommet lisse : refus expliqué, projet identique.
    let (mut q, piece_q) = smooth_piece();
    let smooth = edge_between(&q, piece_q, pt(50.0, 0.0), pt(100.0, 0.0));
    let msg = refused(
        &mut q,
        Command::SetSeamAllowance {
            piece: piece_q,
            seam: seam_with(10.0, vec![corner(smooth, CornerType::Stepped {})], None),
        },
    );
    assert!(msg.contains("lisse"), "{msg}");
    // Coin rentrant : refus expliqué.
    let (mut l, piece_l) = l_piece();
    let inner = edge_between(&l, piece_l, pt(50.0, 50.0), pt(50.0, 100.0));
    let msg = refused(
        &mut l,
        Command::SetSeamAllowance {
            piece: piece_l,
            seam: seam_with(5.0, vec![corner(inner, CornerType::Symmetric {})], None),
        },
    );
    assert!(msg.contains("rentrant"), "{msg}");
    // Calque verrouillé : refus.
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Verrou".into(),
        })
        .unwrap()
        .layers[0];
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![],
        entities: vec![a],
        layer: Some(layer),
    })
    .unwrap();
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: true,
    })
    .unwrap();
    refused(
        &mut p,
        cmd(seam_with(
            10.0,
            vec![corner(a, CornerType::Stepped {})],
            None,
        )),
    );
}

#[test]
fn ca4_pas_de_coin_sur_une_derivee_liee() {
    let (mut p, _, piece) = para();
    let out = p
        .apply(Command::CreateDerivedPiece {
            mother: piece,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset { distance_mm: -5.0 },
            name: None,
        })
        .unwrap();
    let derived = out.pieces[0];
    let e = p.piece(derived).unwrap().outline[0].entity;
    refused(
        &mut p,
        Command::SetSeamAllowance {
            piece: derived,
            seam: seam_with(5.0, vec![corner(e, CornerType::Stepped {})], None),
        },
    );
}

// ---------------------------------------------------------------- CA5

#[test]
fn ca5_une_seule_annulation_et_aucune_etape_pour_un_no_op() {
    let (mut p, _, piece) = para();
    set(&mut p, piece, seam_with(10.0, vec![], None)).unwrap();
    let a = v1(&p, piece);
    let mut doc = Document::new(p);
    let before = bytes(doc.project());
    let wanted = seam_with(10.0, vec![corner(a, CornerType::Symmetric {})], Some(30.0));
    doc.execute(Command::SetSeamAllowance {
        piece,
        seam: wanted.clone(),
    })
    .unwrap();
    let after = bytes(doc.project());
    assert_ne!(before, after);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
    assert!(!doc.can_undo(), "une seule étape");
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), after);
    // Même valeur : aucune étape.
    doc.execute(Command::SetSeamAllowance {
        piece,
        seam: wanted,
    })
    .unwrap();
    assert!(doc.undo());
    assert!(!doc.can_undo());
}

// ---------------------------------------------------------------- CA6

#[test]
fn ca6_geometrie_changee_apres_coup_est_un_controle_pas_un_refus() {
    let (mut p, s, piece) = para();
    let a = v1(&p, piece);
    set(
        &mut p,
        piece,
        seam_with(10.0, vec![corner(a, CornerType::Stepped {})], None),
    )
    .unwrap();
    assert!(p.piece_cut_contour(piece).is_ok());
    // Le sommet 2 passe sur la droite (0,0)-(100,0) : le sommet 1 devient lisse. Le
    // déplacement n'est pas refusé ; le contour de coupe devient un contrôle expliqué.
    let sheet = p.sheet(s).unwrap();
    let moved = sheet
        .points
        .iter()
        .find(|q| distance_mm(sheet.position(q.id).unwrap(), pt(160.0, 80.0)) < 1e-9)
        .unwrap()
        .id;
    p.apply(Command::MovePoint {
        sheet: s,
        point: moved,
        to: pt(200.0, 0.0),
    })
    .expect("le déplacement n'est pas refusé");
    let err = p.piece_cut_contour(piece).unwrap_err().to_string();
    assert!(err.contains("lisse"), "{err}");
    assert!(p
        .piece_issues(piece)
        .iter()
        .any(|i| matches!(i, PieceIssue::CutContourFailed { reason } if reason.contains("lisse"))));
    assert!(p.validate().is_ok());
}

#[test]
fn ca6_aucun_panic_sur_des_formes_degenerees() {
    // Triangle très plat : coins presque plats, tous les types. Résultat ou erreur expliquée,
    // jamais un panic.
    let mut p = Project::new("Plat");
    let s = p.sheets[0].id;
    let piece = poly_piece(
        &mut p,
        s,
        &[pt(0.0, 0.0), pt(100.0, 0.0), pt(50.0, 0.01)],
        "Plat",
    );
    for vs in p.seam_vertices(piece).unwrap() {
        for c in [
            CornerType::PerpendicularPrevious {},
            CornerType::PerpendicularNext {},
            CornerType::Symmetric {},
            CornerType::Truncated { depth_mm: 1.0 },
            CornerType::Stepped {},
            CornerType::HemFoldNext {},
            CornerType::HemFoldPrevious {},
        ] {
            let mut q = p.clone();
            let r = q.apply(Command::SetSeamAllowance {
                piece,
                seam: seam_with(5.0, vec![corner(vs.entity, c)], None),
            });
            if r.is_ok() {
                let _ = q.piece_cut_contour(piece);
                let _ = q.piece_issues(piece);
            }
        }
    }
}

// ---------------------------------------------------------------- CA7

#[test]
fn ca7_type_de_coin_retire_avec_remarque_quand_le_trace_quitte_le_contour() {
    let (mut p, s, piece) = para();
    let a = v1(&p, piece);
    set(
        &mut p,
        piece,
        seam_with(10.0, vec![corner(a, CornerType::Stepped {})], Some(30.0)),
    )
    .unwrap();
    let out = p
        .apply(Command::Delete {
            sheet: s,
            points: vec![],
            entities: vec![a],
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("type(s) de coin")),
        "{:?}",
        out.notes
    );
    let stored = p.piece(piece).unwrap().seam.clone().unwrap();
    assert!(stored.corners.is_empty());
    assert_eq!(stored.max_corner_mm, Some(30.0), "le reste est conservé");
    assert!(p.validate().is_ok());
}

#[test]
fn ca7_copie_de_feuille_reecrit_les_identifiants_des_coins() {
    let (mut p, s, piece) = para();
    let a = v1(&p, piece);
    set(
        &mut p,
        piece,
        seam_with(10.0, vec![corner(a, CornerType::Stepped {})], None),
    )
    .unwrap();
    let out = p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    let copy = out.pieces[0];
    let cs = p.piece(copy).unwrap().seam.clone().unwrap();
    assert_eq!(cs.corners.len(), 1);
    assert_ne!(cs.corners[0].entity, a, "identifiant réécrit");
    assert!(p
        .piece(copy)
        .unwrap()
        .outline
        .iter()
        .any(|e| e.entity == cs.corners[0].entity));
    assert_eq!(
        p.piece_cut_contour(copy).unwrap().traces,
        p.piece_cut_contour(piece).unwrap().traces
    );
    assert!(p.validate().is_ok());
}

#[test]
fn ca7_separer_ne_reprend_pas_les_coins_avec_remarque_mais_garde_la_limite() {
    let mut p = Project::new("Rect");
    let s = p.sheets[0].id;
    let piece = poly_piece(
        &mut p,
        s,
        &[pt(0.0, 0.0), pt(100.0, 0.0), pt(100.0, 60.0), pt(0.0, 60.0)],
        "R",
    );
    let right = edge_between(&p, piece, pt(100.0, 0.0), pt(100.0, 60.0));
    set(
        &mut p,
        piece,
        seam_with(
            10.0,
            vec![corner(right, CornerType::Stepped {})],
            Some(25.0),
        ),
    )
    .unwrap();
    let out = p
        .apply(Command::SplitPiece {
            piece,
            a: pt(50.0, -5.0),
            b: pt(50.0, 65.0),
        })
        .unwrap();
    for id in &out.pieces {
        let sm = p.piece(*id).unwrap().seam.clone().unwrap();
        assert!(sm.corners.is_empty());
        assert_eq!(sm.max_corner_mm, Some(25.0));
    }
    assert!(
        out.notes.iter().any(|n| n.contains("type(s) de coin")),
        "{:?}",
        out.notes
    );
}

// ---------------------------------------------------------------- CA8

fn with_everything() -> (Project, PieceId) {
    let (mut p, _, piece) = para();
    let (a, b) = (v1(&p, piece), v2(&p, piece));
    set(
        &mut p,
        piece,
        seam_with(
            10.0,
            vec![
                corner(a, CornerType::Truncated { depth_mm: 1.5 }),
                corner(b, CornerType::HemFoldNext {}),
            ],
            Some(35.0),
        ),
    )
    .unwrap();
    (p, piece)
}

#[test]
fn ca8_aller_retour_json_et_validation_en_ligne_de_commande() {
    let (p, _) = with_everything();
    let text = bytes(&p);
    assert!(text.contains("\"corners\""));
    assert!(text.contains("\"truncated\"") && text.contains("\"hem_fold_next\""));
    assert!(text.contains("\"max_corner_mm\""));
    assert!(text.contains("\"schema_version\": 6") || text.contains("\"schema_version\": 6"));
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, p);
    assert_eq!(bytes(&back), text);
    let path = std::env::temp_dir().join(format!("pf-us052-{}.pforge.json", std::process::id()));
    std::fs::write(&path, &text).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_pf-validate"))
        .arg(&path)
        .output()
        .expect("lancement de pf-validate");
    let _ = std::fs::remove_file(&path);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn ca8_ancien_fichier_sans_le_champ_relu_et_reecrit_a_l_identique() {
    let (mut p, _, piece) = para();
    set(&mut p, piece, seam_with(10.0, vec![], None)).unwrap();
    let text = bytes(&p);
    assert!(!text.contains("corners") && !text.contains("max_corner_mm"));
    let back = persist::from_json(&text).unwrap();
    assert_eq!(bytes(&back), text);
    let stored = back.piece(piece).unwrap().seam.clone().unwrap();
    assert!(stored.corners.is_empty() && stored.max_corner_mm.is_none());
    // Retirer la marge ramène les octets d'avant la story.
    let (mut q, _, pq) = para();
    let none = bytes(&q);
    set(&mut q, pq, seam_with(10.0, vec![], Some(30.0))).unwrap();
    set(&mut q, pq, None).unwrap();
    assert_eq!(bytes(&q), none);
}

fn edit(text: &str, f: impl FnOnce(&mut Value)) -> String {
    let mut v: Value = serde_json::from_str(text).unwrap();
    f(&mut v);
    serde_json::to_string_pretty(&v).unwrap()
}

#[test]
fn ca8_le_chargeur_refuse_les_coins_invalides() {
    let (p, _) = with_everything();
    let text = bytes(&p);
    assert!(persist::from_json(&text).is_ok());
    let seam = |v: &mut Value| v["pieces"][0]["seam"].take();
    let _ = seam;
    // Clé en trop sur une variante sans champ (D-04), sur une variante à champ, sur le coin.
    for (what, bad) in [
        (
            "hem_fold_next + clé",
            edit(&text, |v| {
                v["pieces"][0]["seam"]["corners"][1]["corner"]["extra"] = Value::from(1);
            }),
        ),
        (
            "truncated + clé",
            edit(&text, |v| {
                v["pieces"][0]["seam"]["corners"][0]["corner"]["extra"] = Value::from(1);
            }),
        ),
        (
            "coin + clé",
            edit(&text, |v| {
                v["pieces"][0]["seam"]["corners"][0]["extra"] = Value::from(1);
            }),
        ),
        (
            "type inconnu",
            edit(&text, |v| {
                v["pieces"][0]["seam"]["corners"][0]["corner"]["type"] = Value::from("round");
            }),
        ),
        (
            "profondeur absente",
            edit(&text, |v| {
                v["pieces"][0]["seam"]["corners"][0]["corner"]
                    .as_object_mut()
                    .unwrap()
                    .remove("depth_mm");
            }),
        ),
        (
            "profondeur négative",
            edit(&text, |v| {
                v["pieces"][0]["seam"]["corners"][0]["corner"]["depth_mm"] = Value::from(-1.0);
            }),
        ),
        (
            "longueur maximale hors bornes",
            edit(&text, |v| {
                v["pieces"][0]["seam"]["max_corner_mm"] = Value::from(1e9);
            }),
        ),
        (
            "tracé hors du contour",
            edit(&text, |v| {
                v["pieces"][0]["seam"]["corners"][0]["entity"] = Value::from(9_999_999u64);
            }),
        ),
        (
            "doublon de sommet",
            edit(&text, |v| {
                let e = v["pieces"][0]["seam"]["corners"][0]["entity"].clone();
                v["pieces"][0]["seam"]["corners"][1]["entity"] = e;
            }),
        ),
    ] {
        assert!(
            persist::from_json(&bad).is_err(),
            "{what} aurait dû être refusé"
        );
    }
}

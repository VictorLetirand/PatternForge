//! US-07.3 : plis. Un groupe de tests par critère : CA1 largeur ajoutée = somme des largeurs
//! (somme des profondeurs au sens de la convention), CA2 pièce repliée = pièce d'origine à 1e-6 mm,
//! CA3 plis multiples répartis, CA4 repères de pli et de contre-pli, sens, CA5 refus sans trace,
//! CA6 une étape annulable, CA7 fichier (schéma 3, relecture, chargeur strict, `pf-validate`),
//! CA8 marge de couture calculée sur le contour déplié, reports.
//!
//! Convention (non validée métier) : un pli plat de profondeur `p` consomme `2p`, un pli creux `4p`.
//! La ligne saisie est le bord de départ de la bande, qui s'étend du côté du sens.
//!
//! Pièce de référence : rectangle 200 × 100, anti-horaire (0;0) (200;0) (200;100) (0;100).

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

fn segment(p: &mut Project, s: SheetId, a: PointMm, b: PointMm) -> EntityId {
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::New(a),
        to: PointRef::New(b),
    })
    .unwrap()
    .entities[0]
}

fn poly_piece(pts: &[(f64, f64)]) -> (Project, SheetId, PieceId) {
    let mut p = Project::new("Plis");
    let s = p.sheets[0].id;
    let n = pts.len();
    let ids = (0..n)
        .map(|i| {
            segment(
                &mut p,
                s,
                pt(pts[i].0, pts[i].1),
                pt(pts[(i + 1) % n].0, pts[(i + 1) % n].1),
            )
        })
        .collect();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Paths { entities: ids },
            name: "Jupe".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    (p, s, piece)
}

fn rect() -> (Project, SheetId, PieceId) {
    poly_piece(&[(0.0, 0.0), (200.0, 0.0), (200.0, 100.0), (0.0, 100.0)])
}

/// Sommets du contour de la pièce (début de chaque tracé, dans le sens du contour).
fn vertices(p: &Project, piece: PieceId) -> Vec<PointMm> {
    let sheet = p.piece_sheet(piece).unwrap();
    p.piece(piece)
        .unwrap()
        .outline
        .iter()
        .map(|e| match sheet.shape(e.entity).unwrap() {
            Shape::Line(a, b) => {
                if e.reversed {
                    b
                } else {
                    a
                }
            }
            other => panic!("segment attendu, reçu {other:?}"),
        })
        .collect()
}

fn same_vertices(got: &[PointMm], want: &[(f64, f64)], tol: f64) -> bool {
    got.len() == want.len()
        && want
            .iter()
            .all(|&(x, y)| got.iter().any(|g| distance_mm(*g, pt(x, y)) <= tol))
}

#[allow(clippy::too_many_arguments)]
fn pleat_cmd(
    piece: PieceId,
    from: PointMm,
    to: PointMm,
    depth: f64,
    kind: PleatKind,
    sense: Side,
    count: u32,
    span: f64,
) -> Command {
    Command::AddPiecePleats {
        piece,
        from,
        to,
        depth_mm: depth,
        kind,
        sense,
        count,
        span_mm: span,
    }
}

/// Pli plat vertical de profondeur 10 à x = 100, bande vers la droite.
fn mid_pleat(piece: PieceId) -> Command {
    pleat_cmd(
        piece,
        pt(100.0, 0.0),
        pt(100.0, 100.0),
        10.0,
        PleatKind::Flat,
        Side::Right,
        1,
        0.0,
    )
}

fn refused(p: &mut Project, c: Command) -> String {
    let (before, text) = (p.clone(), bytes(p));
    let err = p.apply(c).expect_err("la commande aurait dû être refusée");
    assert_eq!(*p, before, "refus sans trace (valeur)");
    assert_eq!(text, bytes(p), "refus sans trace (octets)");
    assert!(!err.to_string().is_empty());
    err.to_string()
}

// ----- CA1 : largeur ajoutée ------------------------------------------------------------------

#[test]
fn ca1_pli_plat_de_10_au_milieu_donne_220_de_large() {
    let (mut p, _s, piece) = rect();
    let out = p.apply(mid_pleat(piece)).unwrap();
    assert_eq!(out.pleats.len(), 1);
    let v = vertices(&p, piece);
    // Sommets calculés à la main : la moitié droite passe de x à x + 20.
    assert!(
        same_vertices(
            &v,
            &[(0.0, 0.0), (220.0, 0.0), (220.0, 100.0), (0.0, 100.0)],
            1e-9
        ),
        "{v:?}"
    );
    let c = p.piece_contour(piece).unwrap();
    assert!((c.area_mm2 - 22_000.0).abs() < 1e-9);
    assert!((c.perimeter_mm - 640.0).abs() < 1e-9);
    let pl = p.piece(piece).unwrap().pleats[0];
    assert!((pl.width_mm() - 20.0).abs() < 1e-12);
    // La feuille d'origine est intacte : la construction garde ses quatre segments à 200 de large.
    assert_eq!(p.sheets[0].entities.len(), 4);
}

#[test]
fn ca1_pli_creux_de_10_ajoute_40() {
    let (mut p, _s, piece) = rect();
    p.apply(pleat_cmd(
        piece,
        pt(100.0, 0.0),
        pt(100.0, 100.0),
        10.0,
        PleatKind::Hollow,
        Side::Right,
        1,
        0.0,
    ))
    .unwrap();
    let c = p.piece_contour(piece).unwrap();
    assert!((c.area_mm2 - 24_000.0).abs() < 1e-9, "240 × 100");
}

#[test]
fn ca1_largeur_ajoutee_egale_la_somme_des_largeurs_plis_horizontaux_du_cote_gauche() {
    // Ligne horizontale orientée vers +x : la gauche est le côté +y. Deux plis plats de 5
    // (largeur 10 chacun) à y = 20 et y = 60 : hauteur 100 + 20 = 120.
    let (mut p, _s, piece) = rect();
    let mut total = 0.0;
    for y in [60.0, 20.0] {
        p.apply(pleat_cmd(
            piece,
            pt(0.0, y),
            pt(200.0, y),
            5.0,
            PleatKind::Flat,
            Side::Left,
            1,
            0.0,
        ))
        .unwrap();
        total += 10.0;
    }
    let v = vertices(&p, piece);
    assert!(
        same_vertices(
            &v,
            &[(0.0, 0.0), (200.0, 0.0), (200.0, 120.0), (0.0, 120.0)],
            1e-9
        ),
        "{v:?}"
    );
    assert!((p.piece_contour(piece).unwrap().area_mm2 - 200.0 * (100.0 + total)).abs() < 1e-9);
}

#[test]
fn ca1_trapeze_oblique_aire_gagnee_largeur_fois_corde() {
    // Trapèze (0;0) (200;0) (150;100) (50;100), aire 15 000, corde de x = 100 : 100 mm.
    let (mut p, _s, piece) = poly_piece(&[(0.0, 0.0), (200.0, 0.0), (150.0, 100.0), (50.0, 100.0)]);
    p.apply(mid_pleat(piece)).unwrap();
    assert!((p.piece_contour(piece).unwrap().area_mm2 - (15_000.0 + 20.0 * 100.0)).abs() < 1e-9);
    let v = vertices(&p, piece);
    assert!(
        same_vertices(
            &v,
            &[(0.0, 0.0), (220.0, 0.0), (170.0, 100.0), (50.0, 100.0)],
            1e-9
        ),
        "{v:?}"
    );
}

// ----- CA2 : pièce repliée = pièce d'origine ----------------------------------------------------

fn assert_folded_is(p: &Project, piece: PieceId, want: &[(f64, f64)], area: f64, perimeter: f64) {
    let f = p.piece_folded(piece).unwrap();
    let got: Vec<PointMm> = f.edges.iter().map(|e| e.start()).collect();
    assert!(same_vertices(&got, want, 1e-6), "{got:?}");
    assert!((f.area_mm2 - area).abs() < 1e-6);
    assert!((f.perimeter_mm - perimeter).abs() < 1e-6);
}

#[test]
fn ca2_la_piece_repliee_est_la_piece_d_origine_rectangle() {
    let (mut p, _s, piece) = rect();
    let before = p.piece_contour(piece).unwrap();
    p.apply(mid_pleat(piece)).unwrap();
    let after = bytes(&p);
    assert_folded_is(
        &p,
        piece,
        &[(0.0, 0.0), (200.0, 0.0), (200.0, 100.0), (0.0, 100.0)],
        20_000.0,
        600.0,
    );
    assert!((p.piece_folded(piece).unwrap().area_mm2 - before.area_mm2).abs() < 1e-6);
    assert!((p.piece_folded(piece).unwrap().removed_mm - 20.0).abs() < 1e-12);
    // Lecture pure : rien n'a changé.
    assert_eq!(bytes(&p), after);
}

#[test]
fn ca2_la_piece_repliee_est_la_piece_d_origine_trapeze_et_plis_multiples() {
    let (mut p, _s, piece) = poly_piece(&[(0.0, 0.0), (200.0, 0.0), (150.0, 100.0), (50.0, 100.0)]);
    p.apply(pleat_cmd(
        piece,
        pt(40.0, 0.0),
        pt(40.0, 100.0),
        5.0,
        PleatKind::Flat,
        Side::Right,
        3,
        100.0,
    ))
    .unwrap();
    // Aire d'origine 15 000, périmètre 200 + 100 + 2·√(50² + 100²).
    let slant = (50.0_f64 * 50.0 + 100.0 * 100.0).sqrt();
    assert_folded_is(
        &p,
        piece,
        &[(0.0, 0.0), (200.0, 0.0), (150.0, 100.0), (50.0, 100.0)],
        15_000.0,
        300.0 + 2.0 * slant,
    );
    assert!((p.piece_folded(piece).unwrap().removed_mm - 30.0).abs() < 1e-12);
}

#[test]
fn ca2_la_piece_repliee_est_la_piece_d_origine_ligne_oblique_et_pli_creux() {
    let (mut p, _s, piece) = rect();
    p.apply(pleat_cmd(
        piece,
        pt(50.0, 0.0),
        pt(150.0, 100.0),
        7.5,
        PleatKind::Hollow,
        Side::Right,
        1,
        0.0,
    ))
    .unwrap();
    assert_folded_is(
        &p,
        piece,
        &[(0.0, 0.0), (200.0, 0.0), (200.0, 100.0), (0.0, 100.0)],
        20_000.0,
        600.0,
    );
}

#[test]
fn ca2_plis_de_sens_opposes_et_pose_a_la_suite_se_replient_dans_l_ordre_inverse() {
    let (mut p, _s, piece) = rect();
    p.apply(pleat_cmd(
        piece,
        pt(150.0, 0.0),
        pt(150.0, 100.0),
        10.0,
        PleatKind::Flat,
        Side::Left,
        1,
        0.0,
    ))
    .unwrap();
    p.apply(mid_pleat(piece)).unwrap();
    assert_folded_is(
        &p,
        piece,
        &[(0.0, 0.0), (200.0, 0.0), (200.0, 100.0), (0.0, 100.0)],
        20_000.0,
        600.0,
    );
    assert!((p.piece_contour(piece).unwrap().area_mm2 - 100.0 * 240.0).abs() < 1e-9);
}

#[test]
fn ca2_une_piece_sans_pli_ne_se_replie_pas() {
    let (p, _s, piece) = rect();
    assert!(p.piece_folded(piece).is_err());
    assert!(p.piece_pleat_marks(piece).is_empty());
}

// ----- CA3 : plis multiples répartis ------------------------------------------------------------

#[test]
fn ca3_trois_plis_repartis_a_pas_egal() {
    // Lignes d'origine x = 40, 90, 140 (étendue 100, pas 50), bande 10 chacune : largeur 230.
    // Lignes finales (calculées à la main) : 40, 100, 160 ; bandes [40;50], [100;110], [160;170].
    let (mut p, _s, piece) = rect();
    let out = p
        .apply(pleat_cmd(
            piece,
            pt(40.0, 0.0),
            pt(40.0, 100.0),
            5.0,
            PleatKind::Flat,
            Side::Right,
            3,
            100.0,
        ))
        .unwrap();
    assert_eq!(out.pleats.len(), 3);
    let v = vertices(&p, piece);
    assert!(
        same_vertices(
            &v,
            &[(0.0, 0.0), (230.0, 0.0), (230.0, 100.0), (0.0, 100.0)],
            1e-9
        ),
        "{v:?}"
    );
    let mut xs: Vec<f64> = p
        .piece(piece)
        .unwrap()
        .pleats
        .iter()
        .map(|l| l.from_x_mm)
        .collect();
    xs.sort_by(f64::total_cmp);
    assert_eq!(xs.len(), 3);
    for (got, want) in xs.iter().zip([40.0, 100.0, 160.0]) {
        assert!((got - want).abs() < 1e-9, "{xs:?}");
    }
    // Largeur ajoutée = somme des largeurs.
    let sum: f64 = p
        .piece(piece)
        .unwrap()
        .pleats
        .iter()
        .map(PiecePleat::width_mm)
        .sum();
    assert!((sum - 30.0).abs() < 1e-12);
    assert!((p.piece_contour(piece).unwrap().area_mm2 - 23_000.0).abs() < 1e-9);
}

#[test]
fn ca3_un_pli_pose_entre_deux_plis_existants_les_decale() {
    let (mut p, _s, piece) = rect();
    for x in [150.0, 50.0] {
        p.apply(pleat_cmd(
            piece,
            pt(x, 0.0),
            pt(x, 100.0),
            5.0,
            PleatKind::Flat,
            Side::Right,
            1,
            0.0,
        ))
        .unwrap();
    }
    // Plis d'origine 150 puis 50 : le premier est devenu x = 160 (décalé de 10 par le second).
    let xs: Vec<f64> = p
        .piece(piece)
        .unwrap()
        .pleats
        .iter()
        .map(|l| l.from_x_mm)
        .collect();
    assert!(
        (xs[0] - 160.0).abs() < 1e-9 && (xs[1] - 50.0).abs() < 1e-9,
        "{xs:?}"
    );
    p.apply(pleat_cmd(
        piece,
        pt(110.0, 0.0),
        pt(110.0, 100.0),
        5.0,
        PleatKind::Flat,
        Side::Right,
        1,
        0.0,
    ))
    .unwrap();
    let xs: Vec<f64> = p
        .piece(piece)
        .unwrap()
        .pleats
        .iter()
        .map(|l| l.from_x_mm)
        .collect();
    assert!((xs[0] - 170.0).abs() < 1e-9, "{xs:?}");
    assert_folded_is(
        &p,
        piece,
        &[(0.0, 0.0), (200.0, 0.0), (200.0, 100.0), (0.0, 100.0)],
        20_000.0,
        600.0,
    );
}

// ----- CA4 : repères et sens --------------------------------------------------------------------

fn find_line(m: &PleatMarks, kind: PleatLineKind, x: f64) -> bool {
    m.lines.iter().any(|l| {
        l.kind == kind
            && distance_mm(l.from, pt(x, 0.0)) < 1e-9
            && distance_mm(l.to, pt(x, 100.0)) < 1e-9
    })
}

#[test]
fn ca4_pli_plat_une_ligne_de_pli_une_de_contre_pli_et_une_fleche() {
    let (mut p, _s, piece) = rect();
    p.apply(mid_pleat(piece)).unwrap();
    let marks = p.piece_pleat_marks(piece);
    assert_eq!(marks.len(), 1);
    let m = marks[0].marks.as_ref().unwrap();
    assert_eq!(m.lines.len(), 2);
    // Pli à x = 100, contre-pli à x = 110 (profondeur 10), de bord à bord (y de 0 à 100).
    assert!(find_line(m, PleatLineKind::Fold, 100.0), "{m:?}");
    assert!(find_line(m, PleatLineKind::Counter, 110.0), "{m:?}");
    // Flèche de sens du pli vers le contre-pli : de (100 ; 50) à (110 ; 50).
    assert_eq!(m.arrows.len(), 1);
    assert!(distance_mm(m.arrows[0].from, pt(100.0, 50.0)) < 1e-9);
    assert!(distance_mm(m.arrows[0].to, pt(110.0, 50.0)) < 1e-9);
}

#[test]
fn ca4_pli_creux_deux_plis_deux_contre_plis_deux_fleches_opposees() {
    let (mut p, _s, piece) = rect();
    p.apply(pleat_cmd(
        piece,
        pt(100.0, 0.0),
        pt(100.0, 100.0),
        10.0,
        PleatKind::Hollow,
        Side::Right,
        1,
        0.0,
    ))
    .unwrap();
    let marks = p.piece_pleat_marks(piece);
    let m = marks[0].marks.as_ref().unwrap();
    assert!(find_line(m, PleatLineKind::Fold, 100.0));
    assert!(find_line(m, PleatLineKind::Counter, 110.0));
    assert!(find_line(m, PleatLineKind::Counter, 130.0));
    assert!(find_line(m, PleatLineKind::Fold, 140.0));
    assert_eq!(m.arrows.len(), 2);
    assert!(distance_mm(m.arrows[0].to, pt(110.0, 50.0)) < 1e-9);
    assert!(distance_mm(m.arrows[1].from, pt(140.0, 50.0)) < 1e-9);
    assert!(distance_mm(m.arrows[1].to, pt(130.0, 50.0)) < 1e-9);
}

#[test]
fn ca4_le_sens_gauche_met_le_contre_pli_du_cote_gauche_de_la_ligne() {
    // Ligne verticale orientée vers le haut : la gauche est le côté −x. La moitié gauche
    // (x ≤ 100) est écartée vers −x : elle se déplace de −20 ; la ligne de pli reste à x = 100.
    let (mut p, _s, piece) = rect();
    p.apply(pleat_cmd(
        piece,
        pt(100.0, 0.0),
        pt(100.0, 100.0),
        10.0,
        PleatKind::Flat,
        Side::Left,
        1,
        0.0,
    ))
    .unwrap();
    let v = vertices(&p, piece);
    assert!(
        same_vertices(
            &v,
            &[(-20.0, 0.0), (200.0, 0.0), (200.0, 100.0), (-20.0, 100.0)],
            1e-9
        ),
        "{v:?}"
    );
    let marks = p.piece_pleat_marks(piece);
    let m = marks[0].marks.as_ref().unwrap();
    assert!(find_line(m, PleatLineKind::Fold, 100.0));
    assert!(find_line(m, PleatLineKind::Counter, 90.0));
}

// ----- CA5 : refus sans trace -------------------------------------------------------------------

#[test]
fn ca5_refus_sans_trace() {
    let (mut p, _s, piece) = rect();
    let ok = |depth: f64, count: u32, span: f64| {
        pleat_cmd(
            piece,
            pt(100.0, 0.0),
            pt(100.0, 100.0),
            depth,
            PleatKind::Flat,
            Side::Right,
            count,
            span,
        )
    };
    for d in [0.0, -3.0, f64::NAN, f64::INFINITY, 0.05, 501.0] {
        refused(&mut p, ok(d, 1, 0.0));
    }
    refused(&mut p, ok(10.0, 0, 0.0));
    refused(&mut p, ok(10.0, 51, 1000.0));
    for span in [0.0, -5.0, f64::NAN, f64::INFINITY] {
        refused(&mut p, ok(10.0, 2, span));
    }
    // Pas (10) inférieur à la largeur (20) : chevauchement.
    let msg = refused(&mut p, ok(10.0, 2, 10.0));
    assert!(msg.contains("chevauchent"), "{msg}");
    // Ligne hors du contour, confondue, le long d'un bord, avec coordonnée non finie.
    let line =
        |a: PointMm, b: PointMm| pleat_cmd(piece, a, b, 10.0, PleatKind::Flat, Side::Right, 1, 0.0);
    refused(&mut p, line(pt(300.0, 0.0), pt(300.0, 100.0)));
    refused(&mut p, line(pt(100.0, 0.0), pt(100.0, 0.0)));
    refused(&mut p, line(pt(0.0, 0.0), pt(200.0, 0.0)));
    refused(&mut p, line(pt(f64::NAN, 0.0), pt(100.0, 100.0)));
    refused(&mut p, line(pt(0.0, 0.0), pt(f64::INFINITY, 100.0)));
    // Pièce inconnue.
    refused(
        &mut p,
        pleat_cmd(
            PieceId(999_999),
            pt(100.0, 0.0),
            pt(100.0, 100.0),
            10.0,
            PleatKind::Flat,
            Side::Right,
            1,
            0.0,
        ),
    );
}

#[test]
fn ca5_refus_deux_plis_qui_se_chevauchent_ou_se_croisent() {
    let (mut p, _s, piece) = rect();
    p.apply(mid_pleat(piece)).unwrap();
    // Bande existante [100 ; 120] : un pli à x = 110 (bande [110 ; 130]) la chevauche.
    let msg = refused(
        &mut p,
        pleat_cmd(
            piece,
            pt(110.0, 0.0),
            pt(110.0, 100.0),
            10.0,
            PleatKind::Flat,
            Side::Right,
            1,
            0.0,
        ),
    );
    assert!(msg.contains("chevauchent"), "{msg}");
    // Un pli oblique croise la ligne du premier.
    let msg = refused(
        &mut p,
        pleat_cmd(
            piece,
            pt(20.0, 0.0),
            pt(40.0, 100.0),
            5.0,
            PleatKind::Flat,
            Side::Right,
            1,
            0.0,
        ),
    );
    assert!(
        msg.contains("croisent") || msg.contains("parallèles"),
        "{msg}"
    );
}

#[test]
fn ca5_refus_piece_derivee_pince_ou_contour_concave() {
    let (mut p, s, piece) = rect();
    let derived = p
        .apply(Command::CreateDerivedPiece {
            mother: piece,
            kind: DerivedKind::Lining,
            operation: DerivedOp::Offset { distance_mm: -5.0 },
            name: None,
        })
        .unwrap()
        .pieces[0];
    let msg = refused(&mut p, mid_pleat(derived));
    assert!(msg.contains("dérivée"), "{msg}");
    // Pièce avec pince.
    let (mut q, qs, qpiece) = rect();
    let apex = q
        .apply(Command::AddPoint {
            sheet: qs,
            at: pt(50.0, 80.0),
        })
        .unwrap()
        .points[0];
    let axis = q
        .apply(Command::AddPoint {
            sheet: qs,
            at: pt(50.0, 0.0),
        })
        .unwrap()
        .points[0];
    q.apply(Command::AddPieceDart {
        piece: qpiece,
        apex: PointRef::Existing(apex),
        toward: PointRef::Existing(axis),
        length_mm: 80.0,
        opening_mm: 10.0,
        fixed: Side::Left,
    })
    .unwrap();
    let msg = refused(&mut q, mid_pleat(qpiece));
    assert!(msg.contains("pince"), "{msg}");
    let _ = s;
    // Forme en U : une droite horizontale à y = 70 la coupe en quatre points.
    let (mut u, _us, upiece) = poly_piece(&[
        (0.0, 0.0),
        (300.0, 0.0),
        (300.0, 100.0),
        (200.0, 100.0),
        (200.0, 40.0),
        (100.0, 40.0),
        (100.0, 100.0),
        (0.0, 100.0),
    ]);
    let msg = refused(
        &mut u,
        pleat_cmd(
            upiece,
            pt(-10.0, 70.0),
            pt(310.0, 70.0),
            10.0,
            PleatKind::Flat,
            Side::Right,
            1,
            0.0,
        ),
    );
    assert!(!msg.is_empty());
    // Ce même U coupé par une droite qui le traverse en deux points : accepté et repliable.
    u.apply(pleat_cmd(
        upiece,
        pt(50.0, -10.0),
        pt(50.0, 120.0),
        10.0,
        PleatKind::Flat,
        Side::Right,
        1,
        0.0,
    ))
    .unwrap();
    assert!((u.piece_folded(upiece).unwrap().area_mm2 - (24_000.0)).abs() < 1e-6);
}

// ----- CA6 : une étape annulable ----------------------------------------------------------------

#[test]
fn ca6_une_seule_etape_annulable_et_un_refus_ne_cree_aucune_etape() {
    let (p, _s, piece) = rect();
    let mut doc = Document::new(p);
    let base = bytes(doc.project());
    assert!(doc
        .execute(pleat_cmd(
            piece,
            pt(300.0, 0.0),
            pt(300.0, 100.0),
            10.0,
            PleatKind::Flat,
            Side::Right,
            1,
            0.0
        ))
        .is_err());
    assert_eq!(bytes(doc.project()), base);
    assert!(!doc.can_undo());
    doc.execute(pleat_cmd(
        piece,
        pt(40.0, 0.0),
        pt(40.0, 100.0),
        5.0,
        PleatKind::Flat,
        Side::Right,
        3,
        100.0,
    ))
    .unwrap();
    let after = bytes(doc.project());
    assert_ne!(after, base);
    assert!(doc.undo(), "une annulation");
    assert_eq!(bytes(doc.project()), base);
    assert!(!doc.can_undo(), "une seule étape pour trois plis");
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), after);
}

// ----- CA7 : fichier ----------------------------------------------------------------------------

#[test]
fn ca7_fichier_schema_3_relu_a_l_identique_chargeur_strict_et_pf_validate() {
    let (mut p, _s, piece) = rect();
    p.apply(mid_pleat(piece)).unwrap();
    let text = bytes(&p);
    let v: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["schema_version"], 6);
    let back = persist::from_json(&text).unwrap();
    assert_eq!(back, p);
    assert_eq!(text, bytes(&back));
    assert_eq!(v["pieces"][0]["pleats"].as_array().unwrap().len(), 1);
    let mutate = |f: &dyn Fn(&mut Value)| {
        let mut w = v.clone();
        f(&mut w["pieces"][0]["pleats"][0]);
        persist::from_json(&serde_json::to_string(&w).unwrap())
    };
    assert!(mutate(&|_| {}).is_ok());
    assert!(mutate(&|d| d["z"] = json!(1)).is_err());
    assert!(mutate(&|d| d["kind"] = json!("round")).is_err());
    assert!(mutate(&|d| d["sense"] = json!("up")).is_err());
    assert!(mutate(&|d| d["depth_mm"] = json!(0.0)).is_err());
    assert!(mutate(&|d| d["depth_mm"] = json!(-4.0)).is_err());
    assert!(mutate(&|d| d["depth_mm"] = json!(1000.0)).is_err());
    assert!(mutate(&|d| {
        d["to_x_mm"] = d["from_x_mm"].clone();
        d["to_y_mm"] = d["from_y_mm"].clone();
    })
    .is_err());
    assert!(
        mutate(&|d| d["id"] = json!(1)).is_err(),
        "identifiant dupliqué"
    );
    // Un fichier sans pli n'a pas la clé.
    let (q, _qs, _qp) = rect();
    assert!(!bytes(&q).contains("pleats"));
    // pf-validate : code 0.
    let dir = std::env::temp_dir().join(format!("pf-plis-{}", std::process::id()));
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

#[test]
fn ca7_un_ancien_fichier_sans_pli_est_relu_et_reecrit_a_l_identique() {
    let (p, _s, _piece) = rect();
    let text = bytes(&p);
    assert_eq!(text, bytes(&persist::from_json(&text).unwrap()));
}

// ----- CA8 : marge de couture, reports ----------------------------------------------------------

#[test]
fn ca8_la_marge_de_couture_se_calcule_sur_le_contour_deplie() {
    let (mut p, _s, piece) = rect();
    p.apply(Command::SetSeamAllowance {
        piece,
        seam: Some(SeamAllowance {
            default_mm: 10.0,
            ..SeamAllowance::default()
        }),
    })
    .unwrap();
    p.apply(mid_pleat(piece)).unwrap();
    // Contour déplié 220 × 100, marge 10 tout autour : 240 × 120, aire 28 800, périmètre 720.
    let cut = p.piece_cut_contour(piece).unwrap();
    assert!((cut.area_mm2 - 28_800.0).abs() < 1e-6, "{}", cut.area_mm2);
    assert!((cut.perimeter_mm - 720.0).abs() < 1e-6);
    assert_eq!(
        p.piece(piece).unwrap().seam.as_ref().unwrap().default_mm,
        10.0
    );
}

#[test]
fn ca8_un_droit_fil_de_la_partie_deplacee_suit_celui_qui_traverse_est_signale() {
    let (mut p, s, piece) = rect();
    // Droit-fil vertical à x = 150 (partie droite) : il glisse de 20.
    let g = segment(&mut p, s, pt(150.0, 20.0), pt(150.0, 80.0));
    let outline = p.piece(piece).unwrap().outline.clone();
    p.apply(Command::SetPieceOutline {
        piece,
        outline,
        grainline: Some(g),
    })
    .unwrap();
    p.apply(mid_pleat(piece)).unwrap();
    let gl = p.piece(piece).unwrap().grainline.expect("droit-fil repris");
    match p.piece_sheet(piece).unwrap().shape(gl).unwrap() {
        Shape::Line(a, b) => {
            assert!(distance_mm(a, pt(170.0, 20.0)) < 1e-9, "{a:?}");
            assert!(distance_mm(b, pt(170.0, 80.0)) < 1e-9, "{b:?}");
        }
        other => panic!("{other:?}"),
    }
    // Un droit-fil horizontal qui traverse la ligne de pli n'est pas repris, avec remarque.
    let (mut q, qs, qpiece) = rect();
    let g = segment(&mut q, qs, pt(20.0, 50.0), pt(180.0, 50.0));
    let outline = q.piece(qpiece).unwrap().outline.clone();
    q.apply(Command::SetPieceOutline {
        piece: qpiece,
        outline,
        grainline: Some(g),
    })
    .unwrap();
    let out = q.apply(mid_pleat(qpiece)).unwrap();
    assert!(q.piece(qpiece).unwrap().grainline.is_none());
    assert!(
        out.notes.iter().any(|n| n.contains("ligne de pli")),
        "{:?}",
        out.notes
    );
}

#[test]
fn ca8_une_copie_de_feuille_garde_des_identifiants_de_pli_uniques() {
    let (mut p, _s, piece) = rect();
    p.apply(mid_pleat(piece)).unwrap();
    let sheet = p.piece_sheet(piece).unwrap().id;
    p.apply(Command::DuplicateSheet { sheet }).unwrap();
    let mut ids: Vec<u64> = p
        .pieces
        .iter()
        .flat_map(|q| q.pleats.iter().map(|l| l.id.0))
        .collect();
    let n = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), n, "identifiants de pli uniques");
    persist::from_json(&bytes(&p)).unwrap();
}

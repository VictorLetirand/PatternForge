//! US-06.1b — cran **en coin** : posé à un sommet de la couture, orienté selon la bissectrice de
//! l'angle du contour de la pièce. Position et direction **lues** (`Project::notch_corner`), jamais
//! stockées. Valeurs attendues calculées à la main (jamais relues du code testé). Un groupe de tests
//! par critère :
//! CC1 rectangle (4 sommets, début / fin de tracé, côtés), CC2 exemple du cahier, CC3 trapèze à
//! 60°, CC4 sommet lisse = cran ordinaire, CC5 sommet rentrant (pièce en L), CC6 sens horaire,
//! CC7 le cran suit le sommet déplacé, CC8 refus sans trace, CC9 sommet non identifiable =
//! contrôle et repli, CC10 une étape annulable et no-op, CC11 fichier (omission, aller-retour,
//! chargeur strict, `pf-validate`, `examples/`), CC12 copies, inversion et coupe d'un tracé.

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::{PointMm, Side};
use serde_json::Value;
use std::f64::consts::SQRT_2;
use std::process::Command as Proc;

const DEPTH: f64 = 5.0;
const TOL: f64 = 1e-9;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn close(a: PointMm, x: f64, y: f64) -> bool {
    (a.x - x).abs() <= TOL && (a.y - y).abs() <= TOL
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

/// Polygone fermé : projet, feuille, pièce, tracés (`edges[i]` : `pts[i]` → `pts[i+1]`).
fn poly(pts: &[(f64, f64)]) -> (Project, SheetId, PieceId, Vec<EntityId>) {
    let mut p = Project::new("Coins");
    let s = p.sheets[0].id;
    let n = pts.len();
    let edges: Vec<EntityId> = (0..n)
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
            source: PieceSource::Paths {
                entities: edges.clone(),
            },
            name: "Pièce".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    (p, s, piece, edges)
}

fn rect() -> (Project, SheetId, PieceId, Vec<EntityId>) {
    poly(&[(0.0, 0.0), (100.0, 0.0), (100.0, 60.0), (0.0, 60.0)])
}

fn ell() -> [(f64, f64); 6] {
    [
        (0.0, 0.0),
        (120.0, 0.0),
        (120.0, 40.0),
        (60.0, 40.0),
        (60.0, 80.0),
        (0.0, 80.0),
    ]
}

fn notch(p: &mut Project, s: SheetId, entity: EntityId, at: f64, side: Side) -> NotchId {
    p.apply(Command::AddNotch {
        sheet: s,
        entity,
        distance_mm: at,
        side,
        length_mm: DEPTH,
        style: NotchStyle::Single,
    })
    .unwrap()
    .notches[0]
}

fn corner_cmd(s: SheetId, n: NotchId, corner: bool) -> Command {
    Command::SetNotchCorner {
        sheet: s,
        notch: n,
        corner,
    }
}

fn len_of(p: &Project, s: SheetId, e: EntityId) -> f64 {
    p.sheet(s).unwrap().path_length_mm(e).unwrap()
}

/// Cran en coin posé à la **fin** de `edges[i]` ou au **début** de `edges[i]`.
fn corner_at(p: &mut Project, s: SheetId, e: EntityId, at_end: bool, side: Side) -> NotchId {
    let at = if at_end { len_of(p, s, e) } else { 0.0 };
    let n = notch(p, s, e, at, side);
    p.apply(corner_cmd(s, n, true)).unwrap();
    n
}

fn placed(p: &Project, s: SheetId, n: NotchId) -> Vec<(PointMm, PointMm)> {
    let notch = p.sheet(s).unwrap().notch(n).unwrap();
    p.notch_placed_marks(s, notch).unwrap().1
}

/// Un trait de `DEPTH` mm du sommet `v` vers `v + DEPTH * d`.
fn assert_single_mark(got: &[(PointMm, PointMm)], v: (f64, f64), d: (f64, f64), what: &str) {
    assert_eq!(got.len(), 1, "{what} : {got:?}");
    let tip = (v.0 + DEPTH * d.0, v.1 + DEPTH * d.1);
    assert!(
        close(got[0].0, v.0, v.1) && close(got[0].1, tip.0, tip.1),
        "{what} : obtenu {:?}, attendu {:?} → {:?}",
        got[0],
        v,
        tip
    );
}

fn assert_unit(v: pf_geometry::VectorMm) {
    assert!((v.norm() - 1.0).abs() < 1e-12, "non unitaire : {v:?}");
}

// ---------------------------------------------------------------- CC1

#[test]
fn cc1_rectangle_quatre_sommets_debut_fin_et_cotes() {
    let h = 1.0 / SQRT_2;
    // Sommets de (0,0),(100,0),(100,60),(0,60) et bissectrice extérieure à la main.
    let corners = [
        ((0.0, 0.0), (-h, -h)),
        ((100.0, 0.0), (h, -h)),
        ((100.0, 60.0), (h, h)),
        ((0.0, 60.0), (-h, h)),
    ];
    for (i, (v, out)) in corners.iter().enumerate() {
        for side in [Side::Right, Side::Left] {
            // Tracé parcouru dans le sens anti-horaire : la droite est l'extérieur.
            let want = if side == Side::Right {
                *out
            } else {
                (-out.0, -out.1)
            };
            // Le sommet i est le début de e[i] et la fin de e[i-1].
            for (edge, at_end) in [(i, false), ((i + 3) % 4, true)] {
                let (mut p, s, _, e) = rect();
                let n = corner_at(&mut p, s, e[edge], at_end, side);
                assert_single_mark(
                    &placed(&p, s, n),
                    *v,
                    want,
                    &format!("sommet {i}, tracé {edge}, fin {at_end}, {side:?}"),
                );
                let c = p.notch_corner(s, n).unwrap();
                assert!(close(c.vertex, v.0, v.1));
                assert_unit(c.outward);
                assert_unit(c.direction);
                assert!((c.outward.x - out.0).abs() < TOL && (c.outward.y - out.1).abs() < TOL);
                assert!(
                    (c.direction.x - want.0).abs() < TOL && (c.direction.y - want.1).abs() < TOL
                );
                assert!((c.interior_angle_deg - 90.0).abs() < 1e-9);
            }
        }
    }
}

// ---------------------------------------------------------------- CC2

#[test]
fn cc2_exemple_du_cahier_sommet_100_0() {
    let (mut p, s, _, e) = rect();
    let n = corner_at(&mut p, s, e[0], true, Side::Right);
    let marks = placed(&p, s, n);
    assert_eq!(marks.len(), 1);
    // Direction (1 ; −1)/√2, trait de 5 mm : extrémité (100 + 5/√2 ; −5/√2).
    assert!(close(marks[0].0, 100.0, 0.0));
    assert!(close(marks[0].1, 100.0 + 5.0 / SQRT_2, -5.0 / SQRT_2));
    // Le cran n'a stocké ni position ni direction : il est ancré au bout du tracé (proportion 1,
    // position lue = 100 mm), c'est tout.
    let sh = p.sheet(s).unwrap();
    let stored = sh.notch(n).unwrap();
    assert!(stored.corner && !stored.on_cut);
    assert_eq!(stored.anchor, Some(NotchAnchor::Ratio { ratio: 1.0 }));
    assert!((sh.notch_distance_mm(stored).unwrap() - 100.0).abs() < TOL);
}

// ---------------------------------------------------------------- CC3

#[test]
fn cc3_trapeze_a_60_degres() {
    // Base 100, côtés de 20 mm à 60° : C = B + 20 (cos 120°, sin 120°) = (90 ; 10√3).
    let hh = 10.0 * 3.0_f64.sqrt();
    let (mut p, s, _, e) = poly(&[(0.0, 0.0), (100.0, 0.0), (90.0, hh), (10.0, hh)]);
    // En B (100 ; 0) : angle intérieur 60°. Vecteurs unitaires vers A (−1 ; 0) et vers C
    // (−1/2 ; √3/2) : somme (−3/2 ; √3/2), norme √3 → bissectrice intérieure (−√3/2 ; 1/2),
    // extérieure (√3/2 ; −1/2).
    let n = corner_at(&mut p, s, e[1], false, Side::Right);
    let c = p.notch_corner(s, n).unwrap();
    assert!((c.interior_angle_deg - 60.0).abs() < 1e-9, "{c:?}");
    let k = 3.0_f64.sqrt() / 2.0;
    assert_single_mark(&placed(&p, s, n), (100.0, 0.0), (k, -0.5), "B extérieur");
    // Côté gauche (intérieur) : (−√3/2 ; 1/2).
    let m = corner_at(&mut p, s, e[0], true, Side::Left);
    assert_single_mark(&placed(&p, s, m), (100.0, 0.0), (-k, 0.5), "B intérieur");
    // En C (90 ; 10√3) : angle intérieur 120°. Unitaires vers B (1/2 ; −√3/2) et vers D (−1 ; 0) :
    // somme (−1/2 ; −√3/2), norme 1 → extérieure (1/2 ; √3/2).
    let c2 = corner_at(&mut p, s, e[2], false, Side::Right);
    let info = p.notch_corner(s, c2).unwrap();
    assert!((info.interior_angle_deg - 120.0).abs() < 1e-9, "{info:?}");
    let got = placed(&p, s, c2);
    assert_eq!(got.len(), 1);
    assert!(close(got[0].0, 90.0, hh));
    assert!(close(got[0].1, 90.0 + 2.5, hh + 2.5 * 3.0_f64.sqrt()));
}

// ---------------------------------------------------------------- CC4

#[test]
fn cc4_sommet_lisse_donne_la_normale_du_trace_comme_un_cran_ordinaire() {
    // Sommet (50 ; 0) entre deux tracés alignés : la bissectrice est la normale (0 ; −1).
    let (mut p, s, _, e) = poly(&[
        (0.0, 0.0),
        (50.0, 0.0),
        (100.0, 0.0),
        (100.0, 60.0),
        (0.0, 60.0),
    ]);
    for form in NotchForm::ALL {
        for side in [Side::Right, Side::Left] {
            let n = corner_at(&mut p, s, e[1], false, side);
            p.apply(Command::SetNotchShape {
                sheet: s,
                notch: n,
                form,
                width_mm: (!form.is_line()).then_some(4.0),
                on_cut: false,
            })
            .unwrap();
            // Cran ordinaire de même réglage au même point (distance 0 du tracé suivant).
            let o = notch(&mut p, s, e[1], 0.0, side);
            p.apply(Command::SetNotchShape {
                sheet: s,
                notch: o,
                form,
                width_mm: (!form.is_line()).then_some(4.0),
                on_cut: false,
            })
            .unwrap();
            let (a, b) = (placed(&p, s, n), placed(&p, s, o));
            assert_eq!(a.len(), b.len(), "{form:?} {side:?}");
            for (x, y) in a.iter().zip(&b) {
                assert!(
                    close(x.0, y.0.x, y.0.y) && close(x.1, y.1.x, y.1.y),
                    "{form:?} {side:?} : coin {x:?}, ordinaire {y:?}"
                );
            }
            let c = p.notch_corner(s, n).unwrap();
            assert!((c.interior_angle_deg - 180.0).abs() < 1e-9);
            assert!((c.outward.x).abs() < TOL && (c.outward.y + 1.0).abs() < TOL);
        }
    }
    // Cran double : deux traits parallèles écartés de 3 mm de part et d'autre du sommet.
    let n = corner_at(&mut p, s, e[1], false, Side::Right);
    p.apply(Command::SetNotch {
        sheet: s,
        notch: n,
        distance_mm: 0.0,
        side: Side::Right,
        length_mm: DEPTH,
        style: NotchStyle::Double,
    })
    .unwrap();
    let got = placed(&p, s, n);
    assert_eq!(got.len(), 2);
    let mut xs: Vec<f64> = got.iter().map(|g| g.0.x).collect();
    xs.sort_by(f64::total_cmp);
    assert!(
        (xs[0] - 48.5).abs() < TOL && (xs[1] - 51.5).abs() < TOL,
        "{got:?}"
    );
    for g in &got {
        assert!((g.0.y).abs() < TOL && (g.1.y + DEPTH).abs() < TOL && (g.1.x - g.0.x).abs() < TOL);
    }
}

// ---------------------------------------------------------------- CC5

#[test]
fn cc5_sommet_rentrant_piece_en_l() {
    let h = 1.0 / SQRT_2;
    let (mut p, s, _, e) = poly(&ell());
    // Sommet rentrant (60 ; 40) : début de e[3], fin de e[2]. Tracé entrant (−1 ; 0), sortant
    // (0 ; 1) : normales gauches (0 ; −1) et (−1 ; 0), somme (−1 ; −1), extérieure (1 ; 1)/√2 :
    // vers le coin vide du L, hors de la pièce. Angle intérieur 270°.
    for (edge, at_end) in [(3, false), (2, true)] {
        let n = corner_at(&mut p, s, e[edge], at_end, Side::Right);
        assert_single_mark(
            &placed(&p, s, n),
            (60.0, 40.0),
            (h, h),
            "rentrant extérieur",
        );
        let c = p.notch_corner(s, n).unwrap();
        assert!((c.interior_angle_deg - 270.0).abs() < 1e-9, "{c:?}");
        let m = corner_at(&mut p, s, e[edge], at_end, Side::Left);
        assert_single_mark(
            &placed(&p, s, m),
            (60.0, 40.0),
            (-h, -h),
            "rentrant intérieur",
        );
    }
    // Sommet saillant voisin (60 ; 80) : (0 ; 1) puis (−1 ; 0) → extérieure (1 ; 1)/√2, 90°.
    let n = corner_at(&mut p, s, e[4], false, Side::Right);
    assert_single_mark(&placed(&p, s, n), (60.0, 80.0), (h, h), "saillant");
    assert!((p.notch_corner(s, n).unwrap().interior_angle_deg - 90.0).abs() < 1e-9);
    // Sommet saillant (120 ; 40) : (1 ; 0)·(0 ; 1) → (1 ; 1)/√2.
    let n = corner_at(&mut p, s, e[2], false, Side::Right);
    assert_single_mark(
        &placed(&p, s, n),
        (120.0, 40.0),
        (h, h),
        "saillant (120 ; 40)",
    );
}

// ---------------------------------------------------------------- CC6

/// Remet le contour de la pièce dans l'ordre et le sens des tracés (horaire pour un polygone
/// donné dans le sens horaire) : `ExtractPiece` le range anti-horaire.
fn clockwise_outline(p: &mut Project, piece: PieceId, edges: &[EntityId]) {
    p.apply(Command::SetPieceOutline {
        piece,
        outline: edges
            .iter()
            .map(|&entity| PieceEdge {
                entity,
                reversed: false,
            })
            .collect(),
        grainline: None,
    })
    .unwrap();
    assert!(p.piece_contour(piece).unwrap().clockwise, "contour horaire");
}

#[test]
fn cc6_sens_horaire_et_anti_horaire_donnent_la_meme_bissectrice_exterieure() {
    let h = 1.0 / SQRT_2;
    // Même L parcouru dans le sens horaire : e[i] va de r[i] à r[i+1].
    let r = [
        (0.0, 80.0),
        (60.0, 80.0),
        (60.0, 40.0),
        (120.0, 40.0),
        (120.0, 0.0),
        (0.0, 0.0),
    ];
    let (mut p, s, piece, e) = poly(&r);
    clockwise_outline(&mut p, piece, &e);
    // Sommet rentrant (60 ; 40) = début de e[2], fin de e[1]. Parcours horaire : la **gauche** du
    // tracé est l'extérieur.
    let out = corner_at(&mut p, s, e[2], false, Side::Left);
    assert_single_mark(&placed(&p, s, out), (60.0, 40.0), (h, h), "horaire, gauche");
    let inn = corner_at(&mut p, s, e[1], true, Side::Right);
    assert_single_mark(
        &placed(&p, s, inn),
        (60.0, 40.0),
        (-h, -h),
        "horaire, droite",
    );
    // Sommet saillant (120 ; 0) = début de e[4] : extérieure (1 ; −1)/√2.
    let n = corner_at(&mut p, s, e[4], false, Side::Left);
    assert_single_mark(
        &placed(&p, s, n),
        (120.0, 0.0),
        (h, -h),
        "horaire, saillant",
    );
    let c = p.notch_corner(s, n).unwrap();
    assert!((c.interior_angle_deg - 90.0).abs() < 1e-9);
    // Rectangle horaire.
    let (mut q, qs, qpiece, qe) = poly(&[(0.0, 0.0), (0.0, 60.0), (100.0, 60.0), (100.0, 0.0)]);
    clockwise_outline(&mut q, qpiece, &qe);
    // Sommet (100 ; 0) = fin de qe[2] : (100 ; 60) → (100 ; 0).
    let n = corner_at(&mut q, qs, qe[2], true, Side::Left);
    assert_single_mark(
        &placed(&q, qs, n),
        (100.0, 0.0),
        (h, -h),
        "rectangle horaire",
    );
}

// ---------------------------------------------------------------- CC7

#[test]
fn cc7_le_cran_suit_le_sommet_deplace() {
    let (mut p, s, _, e) = rect();
    let n = corner_at(&mut p, s, e[1], false, Side::Right);
    let before = p.sheet(s).unwrap().notch(n).unwrap().clone();
    let vertex = p
        .sheet(s)
        .unwrap()
        .points
        .iter()
        .find(|q| close(p.sheet(s).unwrap().position(q.id).unwrap(), 100.0, 0.0))
        .unwrap()
        .id;
    // (100 ; 0) → (160 ; 0) : le bord bas devient (0 ; 0)→(160 ; 0) et le côté (160 ; 0)→(100 ; 60).
    // Angle intérieur 45° ; unitaires vers A (−1 ; 0) et vers C (−1 ; 1)/√2 : la bissectrice
    // intérieure fait 180° − 22,5°, l'extérieure (cos 22,5° ; −sin 22,5°).
    p.apply(Command::MovePoint {
        sheet: s,
        point: vertex,
        to: pt(160.0, 0.0),
    })
    .unwrap();
    let cos = (2.0 + SQRT_2).sqrt() / 2.0;
    let sin = (2.0 - SQRT_2).sqrt() / 2.0;
    assert_single_mark(
        &placed(&p, s, n),
        (160.0, 0.0),
        (cos, -sin),
        "sommet déplacé",
    );
    assert!((p.notch_corner(s, n).unwrap().interior_angle_deg - 45.0).abs() < 1e-9);
    // Le cran lui-même n'a pas changé : rien n'a été stocké.
    assert_eq!(*p.sheet(s).unwrap().notch(n).unwrap(), before);

    // Cran posé à la **fin** du tracé précédent : même sommet, même résultat (sa position est
    // ancrée au bout du tracé, la longueur qui change ne le fait pas sortir du bord).
    let (mut q, qs, _, qe) = rect();
    let m = corner_at(&mut q, qs, qe[0], true, Side::Right);
    let qv = q
        .sheet(qs)
        .unwrap()
        .points
        .iter()
        .find(|c| close(q.sheet(qs).unwrap().position(c.id).unwrap(), 100.0, 0.0))
        .unwrap()
        .id;
    q.apply(Command::MovePoint {
        sheet: qs,
        point: qv,
        to: pt(160.0, 0.0),
    })
    .unwrap();
    assert_single_mark(
        &placed(&q, qs, m),
        (160.0, 0.0),
        (cos, -sin),
        "fin de tracé, déplacé",
    );
    // Raccourcir le tracé (sommet vers (40 ; 0)) : le cran reste au sommet.
    q.apply(Command::MovePoint {
        sheet: qs,
        point: qv,
        to: pt(40.0, 0.0),
    })
    .unwrap();
    assert!(close(q.notch_corner(qs, m).unwrap().vertex, 40.0, 0.0));
}

// ---------------------------------------------------------------- CC8

#[test]
fn cc8_refus_sans_trace() {
    let (mut p, s, _, e) = rect();
    let mid = notch(&mut p, s, e[0], 30.0, Side::Right);
    let alone = segment(&mut p, s, pt(300.0, 0.0), pt(400.0, 0.0));
    let outside = notch(&mut p, s, alone, 0.0, Side::Right);
    let end = notch(&mut p, s, e[0], 100.0, Side::Right);
    let cut = notch(&mut p, s, e[0], 0.0, Side::Right);
    p.apply(Command::SetNotchShape {
        sheet: s,
        notch: cut,
        form: NotchForm::Line,
        width_mm: None,
        on_cut: true,
    })
    .unwrap();
    let before = bytes(&p);
    let refused = |p: &mut Project, c: Command, needle: &str| {
        let err = p.apply(c).unwrap_err().to_string();
        assert!(err.contains(needle), "{err}");
        assert_eq!(bytes(p), before, "refus sans trace : {needle}");
    };
    refused(&mut p, corner_cmd(s, mid, true), "extrémité");
    refused(&mut p, corner_cmd(s, outside, true), "aucune pièce");
    refused(&mut p, corner_cmd(s, cut, true), "sur la coupe");
    refused(&mut p, corner_cmd(s, NotchId(987_654), true), "");
    refused(&mut p, corner_cmd(SheetId(987_654), end, true), "");
    // Le cran en coin refuse ensuite « sur la coupe » (réglage de forme) : combinaison reportée.
    p.apply(corner_cmd(s, end, true)).unwrap();
    let before2 = bytes(&p);
    let err = p
        .apply(Command::SetNotchShape {
            sheet: s,
            notch: end,
            form: NotchForm::Line,
            width_mm: None,
            on_cut: true,
        })
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("sur la coupe") && err.contains("coin"),
        "{err}"
    );
    assert_eq!(bytes(&p), before2);
    // Le changement de forme d'un cran en coin, lui, est accepté.
    p.apply(Command::SetNotchShape {
        sheet: s,
        notch: end,
        form: NotchForm::V,
        width_mm: Some(4.0),
        on_cut: false,
    })
    .unwrap();
}

// ---------------------------------------------------------------- CC9

#[test]
fn cc9_sommet_non_identifiable_est_un_controle_avec_repli() {
    // (a) Le cran quitte le sommet (la commande de la mère passe) : contrôle + repli sur la
    // normale du tracé, jamais un refus ni un panic.
    let (mut p, s, piece, e) = rect();
    let n = corner_at(&mut p, s, e[0], true, Side::Right);
    assert!(!p
        .piece_issues(piece)
        .iter()
        .any(|i| matches!(i, PieceIssue::CornerNotchesFailed { .. })));
    p.apply(Command::SetNotchAnchor {
        sheet: s,
        notch: n,
        anchor: Some(NotchAnchor::Ratio { ratio: 0.3 }),
    })
    .unwrap();
    assert!(p.notch_corner(s, n).is_err());
    let issue = p
        .piece_issues(piece)
        .into_iter()
        .find(|i| matches!(i, PieceIssue::CornerNotchesFailed { .. }))
        .expect("contrôle CornerNotchesFailed");
    assert_eq!(issue.key(), "corner_notches_failed");
    // Repli : le cran ordinaire à 30 mm, côté droit = (30 ; 0) → (30 ; −5).
    assert_single_mark(
        &placed(&p, s, n),
        (30.0, 0.0),
        (0.0, -1.0),
        "repli sur la normale",
    );
    p.validate().unwrap();
    assert_eq!(persist::from_json(&bytes(&p)).unwrap(), p);

    // (b) Contour dégénéré (aire nulle) : le sommet n'est plus identifiable.
    let (mut p, s, piece, e) = poly(&[(0.0, 0.0), (100.0, 0.0), (50.0, 50.0)]);
    let n = corner_at(&mut p, s, e[1], true, Side::Right);
    let apex = p
        .sheet(s)
        .unwrap()
        .points
        .iter()
        .find(|q| close(p.sheet(s).unwrap().position(q.id).unwrap(), 50.0, 50.0))
        .unwrap()
        .id;
    p.apply(Command::MovePoint {
        sheet: s,
        point: apex,
        to: pt(50.0, 0.0),
    })
    .unwrap();
    assert!(p.notch_corner(s, n).is_err());
    assert!(p
        .piece_issues(piece)
        .iter()
        .any(|i| matches!(i, PieceIssue::CornerNotchesFailed { .. })));
    // Le cran reste dessiné (repli) : la fin de e[1] est (50 ; 0).
    let marks = placed(&p, s, n);
    assert!(!marks.is_empty() && close(marks[0].0, 50.0, 0.0));

    // (c) La pièce disparaît : plus de sommet, repli sans contrôle (aucune pièce à avertir).
    let (mut p, s, piece, e) = rect();
    let n = corner_at(&mut p, s, e[0], true, Side::Right);
    p.apply(Command::DeletePiece { piece }).unwrap();
    let err = p.notch_corner(s, n).unwrap_err().to_string();
    assert!(err.contains("aucune pièce"), "{err}");
    assert_single_mark(
        &placed(&p, s, n),
        (100.0, 0.0),
        (0.0, -1.0),
        "pièce supprimée",
    );
    p.validate().unwrap();
}

// ---------------------------------------------------------------- CC10

#[test]
fn cc10_une_etape_annulable_et_no_op() {
    let (p, s, _, e) = rect();
    let mut doc = Document::new(p);
    let n = doc
        .execute(Command::AddNotch {
            sheet: s,
            entity: e[0],
            distance_mm: 100.0,
            side: Side::Right,
            length_mm: DEPTH,
            style: NotchStyle::Single,
        })
        .unwrap()
        .notches[0];
    let before = bytes(doc.project());
    doc.execute(corner_cmd(s, n, true)).unwrap();
    let after = bytes(doc.project());
    assert_ne!(before, after);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before, "une seule annulation");
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), after);
    // Le même réglage ne crée aucune étape.
    doc.execute(corner_cmd(s, n, true)).unwrap();
    assert_eq!(bytes(doc.project()), after);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
    // Décocher un cran ordinaire : no-op aussi.
    doc.execute(corner_cmd(s, n, false)).unwrap();
    assert_eq!(bytes(doc.project()), before);
    assert!(doc.undo());
    assert!(
        doc.project().sheet(s).unwrap().notch(n).is_err(),
        "l'annulation suivante retire le cran : les no-op n'ont créé aucune étape"
    );
}

// ---------------------------------------------------------------- CC11

fn cli(path: &str) -> i32 {
    Proc::new(env!("CARGO_BIN_EXE_pf-validate"))
        .arg(path)
        .output()
        .expect("lancement de pf-validate")
        .status
        .code()
        .unwrap_or(-1)
}

fn tmp_file(name: &str, text: &str) -> String {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("notch_corner");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, text).unwrap();
    path.to_string_lossy().into_owned()
}

#[test]
fn cc11_omission_aller_retour_et_ancien_fichier() {
    let (mut p, s, _, e) = rect();
    let plain = notch(&mut p, s, e[0], 30.0, Side::Right);
    let old = bytes(&p);
    assert!(!old.contains("\"corner\""), "champ écrit sans nécessité");
    assert_eq!(bytes(&persist::from_json(&old).unwrap()), old);
    let c = corner_at(&mut p, s, e[1], false, Side::Left);
    let json = bytes(&p);
    let doc: Value = serde_json::from_str(&json).unwrap();
    let notches = &doc["sheets"][0]["notches"];
    assert!(notches[0].get("corner").is_none());
    assert_eq!(notches[1]["corner"], true);
    // Ni sommet, ni direction, ni angle ne sont écrits.
    for key in ["vertex", "direction", "bisector", "interior_angle"] {
        assert!(!json.contains(key), "{key} stocké");
    }
    let back = persist::from_json(&json).unwrap();
    assert_eq!(back, p);
    assert_eq!(bytes(&back), json);
    // Retour à l'ordinaire : plus aucun champ `corner` dans le fichier.
    p.apply(corner_cmd(s, c, false)).unwrap();
    assert!(!bytes(&p).contains("\"corner\""));
    assert_eq!(p.schema_version, 6);
    let _ = plain;
}

#[test]
fn cc11_chargeur_strict_et_pf_validate() {
    let (mut p, s, _, e) = rect();
    let n = corner_at(&mut p, s, e[1], false, Side::Right);
    let good = bytes(&p);
    assert_eq!(cli(&tmp_file("bon.pforge.json", &good)), 0);
    let bad = |from: &str, to: &str| {
        assert!(good.contains(from), "{from}");
        good.replacen(from, to, 1)
    };
    let mutants = [
        (
            "coin textuel",
            bad("\"corner\": true", "\"corner\": \"oui\""),
        ),
        ("coin numérique", bad("\"corner\": true", "\"corner\": 1")),
        (
            "clé inconnue",
            bad("\"corner\": true", "\"corner\": true,\n \"bissectrice\": 1"),
        ),
        (
            "coin et sur la coupe",
            bad("\"corner\": true", "\"corner\": true,\n \"on_cut\": true"),
        ),
    ];
    for (name, text) in mutants {
        assert!(persist::from_json(&text).is_err(), "{name} : chargeur");
        assert_eq!(
            cli(&tmp_file("mauvais.pforge.json", &text)),
            1,
            "{name} : pf-validate"
        );
    }
    // `corner: false` écrit à la main est accepté et réécrit sans le champ.
    let explicit = bad("\"corner\": true", "\"corner\": false");
    let loaded = persist::from_json(&explicit).unwrap();
    assert!(!loaded.sheet(s).unwrap().notch(n).unwrap().corner);
    assert!(!bytes(&loaded).contains("\"corner\""));
}

#[test]
fn cc11_les_exemples_ne_changent_pas() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut seen = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(".pforge.json") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n");
        assert!(!text.contains("\"corner\""), "{path:?}");
        let loaded = persist::from_json(&text).unwrap();
        assert_eq!(bytes(&loaded).replace("\r\n", "\n"), text, "{path:?}");
        seen += 1;
    }
    assert!(seen >= 2, "exemples trouvés : {seen}");
}

// ---------------------------------------------------------------- CC12

#[test]
fn cc12_copie_de_feuille_garde_le_coin_duplication_le_remet_ordinaire_avec_remarque() {
    let (mut p, s, _, e) = rect();
    let n = corner_at(&mut p, s, e[1], false, Side::Right);
    // Copie de feuille : les pièces suivent, le cran reste en coin et se lit pareil.
    let out = p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    let copy = out.sheet.expect("feuille copiée");
    let cn = p.sheet(copy).unwrap().notches[0].clone();
    assert!(cn.corner);
    let c = p.notch_corner(copy, cn.id).unwrap();
    assert!(close(c.vertex, 100.0, 0.0));
    assert_single_mark(
        &placed(&p, copy, cn.id),
        (100.0, 0.0),
        (1.0 / SQRT_2, -1.0 / SQRT_2),
        "copie",
    );
    // Duplication de tracés : la copie n'est dans aucune pièce, donc ordinaire, avec remarque.
    let out = p
        .apply(Command::Duplicate {
            sheet: s,
            entities: vec![e[1]],
            transform: pf_geometry::Transform::Translate {
                dx_mm: 0.0,
                dy_mm: 200.0,
            },
        })
        .unwrap();
    let dup = p
        .sheet(s)
        .unwrap()
        .notches
        .iter()
        .find(|c| out.notches.contains(&c.id))
        .expect("le cran est copié avec son tracé");
    assert!(!dup.corner);
    assert!(
        out.notes.iter().any(|t| t.contains("cran en coin")),
        "{:?}",
        out.notes
    );
    assert!(p.sheet(s).unwrap().notch(n).unwrap().corner);
    // Presse-papiers : même règle.
    let frag = p.copy_fragment(s, &[], &[e[1]], &[n], &[]).unwrap();
    assert!(
        frag.notes.iter().any(|t| t.contains("cran(s) en coin")),
        "{:?}",
        frag.notes
    );
    assert!(frag.notches.iter().all(|c| !c.corner));
}

#[test]
fn cc12_inversion_et_coupe_du_trace_gardent_le_meme_trait() {
    let (mut p, s, _, e) = rect();
    let n = corner_at(&mut p, s, e[1], false, Side::Right);
    let want = placed(&p, s, n);
    // Inverser le tracé : le cran passe à l'autre bout et change de côté, le trait reste le même.
    p.apply(Command::ReversePath {
        sheet: s,
        entity: e[1],
    })
    .unwrap();
    let got = placed(&p, s, n);
    assert_eq!(got.len(), want.len());
    assert!(close(got[0].0, want[0].0.x, want[0].0.y) && close(got[0].1, want[0].1.x, want[0].1.y));
    // Couper le tracé en son milieu : le cran garde son sommet.
    p.apply(Command::SplitPath {
        sheet: s,
        entity: e[1],
        at: pt(100.0, 30.0),
    })
    .unwrap();
    let got = placed(&p, s, n);
    assert!(close(got[0].0, want[0].0.x, want[0].0.y) && close(got[0].1, want[0].1.x, want[0].1.y));
}

//! PH-4b — perçages au pas le long d'un bord : `Command::AddPieceMarkingRun`.
//!
//! Toutes les valeurs attendues sont calculées **à la main** dans les tests (jamais relues du
//! code testé). CA1 valeurs exactes sur bords droits, CA2 courbes (arc, Bézier, chaîne),
//! CA3 fente de poinçon orientée, CA4 refus sans trace, CA5 une étape annulable et cohérence avec
//! les autres opérations, CA6 octets des fichiers existants et format. La campagne adverse
//! (oracle par échantillonnage dense) est dans `adverse_percages.rs`.

use pf_core::persist;
use pf_core::piece::PieceIssue;
use pf_core::*;
use pf_geometry::PointMm;
use serde_json::json;
use std::path::PathBuf;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn close(a: PointMm, x: f64, y: f64, tol: f64) -> bool {
    (a.x - x).abs() <= tol && (a.y - y).abs() <= tol
}

/// Refus : message ; le projet est identique (valeur et octets).
fn refused(p: &mut Project, c: Command) -> String {
    let (before, text) = (p.clone(), bytes(p));
    let err = p.apply(c).expect_err("la commande aurait dû être refusée");
    assert_eq!(*p, before, "refus sans trace (valeur)");
    assert_eq!(text, bytes(p), "refus sans trace (octets)");
    assert!(!err.to_string().is_empty());
    err.to_string()
}

struct Scene {
    p: Project,
    s: SheetId,
    piece: PieceId,
}

/// Rectangle 100 × 70 mm, pièce liée anti-horaire.
fn rect() -> Scene {
    let mut p = Project::new("Perçages au pas");
    let s = p.sheets[0].id;
    p.apply(Command::AddRectangle {
        sheet: s,
        corner: PointRef::New(pt(0.0, 0.0)),
        width_mm: 100.0,
        height_mm: 70.0,
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(50.0, 35.0) },
            name: "Fond".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    Scene { p, s, piece }
}

/// Segment de la feuille qui relie ces deux positions (dans un sens ou dans l'autre).
fn seg_between(p: &Project, s: SheetId, a: PointMm, b: PointMm) -> EntityId {
    let sheet = p.sheet(s).unwrap();
    sheet
        .entities
        .iter()
        .find(|e| {
            sheet.segment_ends(e.id).is_ok_and(|(x, y)| {
                (close(x, a.x, a.y, 1e-9) && close(y, b.x, b.y, 1e-9))
                    || (close(x, b.x, b.y, 1e-9) && close(y, a.x, a.y, 1e-9))
            })
        })
        .unwrap_or_else(|| panic!("segment {a:?} {b:?}"))
        .id
}

fn bottom(sc: &Scene) -> EntityId {
    seg_between(&sc.p, sc.s, pt(0.0, 0.0), pt(100.0, 0.0))
}

fn spec(edges: &[EntityId]) -> MarkingRunSpec {
    MarkingRunSpec {
        edges: edges.to_vec(),
        spacing: RunSpacing::Count { count: 3 },
        start_margin_mm: 0.0,
        end_margin_mm: 0.0,
        distance_mm: 3.0,
        side: RunSide::Inside,
        form: RunForm::Drill { diameter_mm: 1.2 },
    }
}

fn run_cmd(piece: PieceId, run: MarkingRunSpec) -> Command {
    Command::AddPieceMarkingRun { piece, run }
}

fn holes(p: &Project, piece: PieceId) -> Vec<(PointMm, MarkingShape)> {
    p.piece_markings(piece)
        .iter()
        .map(|r| (r.at, r.marking.shape))
        .collect()
}

fn xs(h: &[(PointMm, MarkingShape)]) -> Vec<f64> {
    h.iter().map(|(a, _)| a.x).collect()
}

fn ys(h: &[(PointMm, MarkingShape)]) -> Vec<f64> {
    h.iter().map(|(a, _)| a.y).collect()
}

fn all_close(got: &[f64], want: &[f64], tol: f64) {
    assert_eq!(got.len(), want.len(), "{got:?} / {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert!((g - w).abs() <= tol, "{got:?} / {want:?}");
    }
}

// ----- CA1 : valeurs calculées à la main sur un bord droit ----------------------------

#[test]
fn ca1_pas_4_sur_100_mm_donne_26_percages_a_4_mm_exact() {
    let mut sc = rect();
    let edge = bottom(&sc);
    let mut r = spec(&[edge]);
    r.spacing = RunSpacing::Step { step_mm: 4.0 };
    let preview = sc.p.marking_run_preview(sc.piece, &r).unwrap();
    // ⌈100 / 4⌉ = 25 intervalles, 26 positions, espacement réel 100 / 25 = 4 mm.
    assert_eq!(preview.holes.len(), 26);
    assert_eq!(preview.step_mm, 4.0);
    assert_eq!(preview.length_mm, 100.0);
    let o = sc.p.apply(run_cmd(sc.piece, r)).unwrap();
    assert_eq!(o.markings.len(), 26);
    let h = holes(&sc.p, sc.piece);
    all_close(
        &xs(&h),
        &(0..26).map(|i| 4.0 * i as f64).collect::<Vec<_>>(),
        1e-9,
    );
    all_close(&ys(&h), &[3.0; 26], 1e-9);
    assert!(h
        .iter()
        .all(|(_, s)| *s == MarkingShape::Drill { diameter_mm: 1.2 }));
    // L'aperçu donne les mêmes positions que la création.
    for (a, b) in preview.holes.iter().zip(&h) {
        assert!(close(a.position, b.0.x, b.0.y, 1e-12));
    }
    assert!(sc
        .p
        .piece_issues(sc.piece)
        .iter()
        .all(|i| !matches!(i, PieceIssue::MarkingOutsideOutline { .. })));
}

#[test]
fn ca1_pas_7_donne_15_intervalles_de_6_6667_mm_au_plus_7() {
    let mut sc = rect();
    let mut r = spec(&[bottom(&sc)]);
    r.spacing = RunSpacing::Step { step_mm: 7.0 };
    let pv = sc.p.marking_run_preview(sc.piece, &r).unwrap();
    // ⌈100 / 7⌉ = ⌈14,28…⌉ = 15 intervalles ; 100 / 15 = 6,6666… ≤ 7.
    assert_eq!(pv.holes.len(), 16);
    assert!((pv.step_mm - 100.0 / 15.0).abs() < 1e-12 && pv.step_mm <= 7.0);
    sc.p.apply(run_cmd(sc.piece, r)).unwrap();
    let x: Vec<f64> = (0..16).map(|i| 100.0 * i as f64 / 15.0).collect();
    all_close(&xs(&holes(&sc.p, sc.piece)), &x, 1e-9);
    // Le dernier perçage tombe exactement à la fin du bord.
    assert!((xs(&holes(&sc.p, sc.piece))[15] - 100.0).abs() < 1e-12);
}

#[test]
fn ca1_le_pas_exact_ne_gagne_pas_un_intervalle_par_arrondi() {
    // 2,1 / 0,7 vaut 3,0000000000000004 en flottant : ⌈·⌉ naïf donnerait 4 intervalles.
    let (span, step) = (std::hint::black_box(2.1_f64), std::hint::black_box(0.7_f64));
    assert!(span / step > 3.0);
    let mut p = Project::new("Court");
    let s = p.sheets[0].id;
    p.apply(Command::AddRectangle {
        sheet: s,
        corner: PointRef::New(pt(0.0, 0.0)),
        width_mm: 2.1,
        height_mm: 70.0,
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(1.0, 35.0) },
            name: "Court".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let edge = seg_between(&p, s, pt(0.0, 0.0), pt(2.1, 0.0));
    let mut r = spec(&[edge]);
    r.spacing = RunSpacing::Step { step_mm: 0.7 };
    r.distance_mm = 0.5;
    let pv = p.marking_run_preview(piece, &r).unwrap();
    assert_eq!(pv.holes.len(), 4, "3 intervalles de 0,7 mm");
    assert!((pv.step_mm - 0.7).abs() < 1e-9);
    // Pas 0,5 sur 2,1 mm : ⌈4,2⌉ = 5 intervalles de 0,42 mm.
    r.spacing = RunSpacing::Step { step_mm: 0.5 };
    let pv = p.marking_run_preview(piece, &r).unwrap();
    assert_eq!(pv.holes.len(), 6);
    assert!((pv.step_mm - 0.42).abs() < 1e-9 && pv.step_mm <= 0.5);
}

#[test]
fn ca1_nombre_5_donne_0_25_50_75_100() {
    let mut sc = rect();
    let mut r = spec(&[bottom(&sc)]);
    r.spacing = RunSpacing::Count { count: 5 };
    sc.p.apply(run_cmd(sc.piece, r)).unwrap();
    all_close(
        &xs(&holes(&sc.p, sc.piece)),
        &[0.0, 25.0, 50.0, 75.0, 100.0],
        1e-9,
    );
}

#[test]
fn ca1_marges_de_depart_et_de_fin() {
    let mut sc = rect();
    let edge = bottom(&sc);
    // Nombre 4, premier à 10, dernier à 100 − 20 = 80 : 10, 33,33…, 56,66…, 80.
    let mut r = spec(&[edge]);
    r.spacing = RunSpacing::Count { count: 4 };
    r.start_margin_mm = 10.0;
    r.end_margin_mm = 20.0;
    let pv = sc.p.marking_run_preview(sc.piece, &r).unwrap();
    assert!((pv.step_mm - 70.0 / 3.0).abs() < 1e-12);
    sc.p.apply(run_cmd(sc.piece, r)).unwrap();
    all_close(
        &xs(&holes(&sc.p, sc.piece)),
        &[10.0, 10.0 + 70.0 / 3.0, 10.0 + 140.0 / 3.0, 80.0],
        1e-9,
    );
    // Pas 10 avec marges 5 et 5 : intervalle utile 90 → 9 intervalles exacts, 10 perçages.
    let mut sc = rect();
    let mut r = spec(&[bottom(&sc)]);
    r.spacing = RunSpacing::Step { step_mm: 10.0 };
    r.start_margin_mm = 5.0;
    r.end_margin_mm = 5.0;
    sc.p.apply(run_cmd(sc.piece, r)).unwrap();
    let want: Vec<f64> = (0..10).map(|i| 5.0 + 10.0 * i as f64).collect();
    all_close(&xs(&holes(&sc.p, sc.piece)), &want, 1e-9);
}

#[test]
fn ca1_un_seul_percage_au_milieu_de_l_intervalle_utile() {
    let sc = rect();
    let mut r = spec(&[bottom(&sc)]);
    r.spacing = RunSpacing::Count { count: 1 };
    let pv = sc.p.marking_run_preview(sc.piece, &r).unwrap();
    assert_eq!(pv.step_mm, 0.0);
    assert!(close(pv.holes[0].position, 50.0, 3.0, 1e-12));
    // Marges 10 et 30 : intervalle [10 ; 70], milieu 40.
    r.start_margin_mm = 10.0;
    r.end_margin_mm = 30.0;
    assert!(close(
        sc.p.marking_run_preview(sc.piece, &r).unwrap().holes[0].position,
        40.0,
        3.0,
        1e-12
    ));
    // Marges qui se touchent (somme = longueur) : accepté pour un seul perçage.
    r.start_margin_mm = 60.0;
    r.end_margin_mm = 40.0;
    assert!(close(
        sc.p.marking_run_preview(sc.piece, &r).unwrap().holes[0].position,
        60.0,
        3.0,
        1e-12
    ));
}

#[test]
fn ca1_cote_exterieur_et_les_quatre_bords_du_rectangle() {
    let mut sc = rect();
    let edge = bottom(&sc);
    let mut r = spec(&[edge]);
    r.side = RunSide::Outside;
    sc.p.apply(run_cmd(sc.piece, r)).unwrap();
    let h = holes(&sc.p, sc.piece);
    all_close(&xs(&h), &[0.0, 50.0, 100.0], 1e-9);
    all_close(&ys(&h), &[-3.0, -3.0, -3.0], 1e-9);
    // Hors de la pièce : avertissement (jamais un refus), un par perçage.
    assert!(sc
        .p
        .piece_issues(sc.piece)
        .contains(&PieceIssue::MarkingOutsideOutline { count: 3 }));

    // Côté intérieur des quatre bords, dans le sens du contour anti-horaire.
    let mut sc = rect();
    let c = |a, b, c, d| seg_between(&sc.p, sc.s, pt(a, b), pt(c, d));
    let (b, rt, t, l) = (
        c(0.0, 0.0, 100.0, 0.0),
        c(100.0, 0.0, 100.0, 70.0),
        c(100.0, 70.0, 0.0, 70.0),
        c(0.0, 70.0, 0.0, 0.0),
    );
    let piece = sc.piece;
    for e in [b, rt, t, l] {
        sc.p.apply(run_cmd(piece, spec(&[e]))).unwrap();
    }
    let h = holes(&sc.p, piece);
    assert_eq!(h.len(), 12);
    let want = [
        (0.0, 3.0),
        (50.0, 3.0),
        (100.0, 3.0), // bas, vers +x
        (97.0, 0.0),
        (97.0, 35.0),
        (97.0, 70.0), // droite, vers +y
        (100.0, 67.0),
        (50.0, 67.0),
        (0.0, 67.0), // haut, vers −x
        (3.0, 70.0),
        (3.0, 35.0),
        (3.0, 0.0), // gauche, vers −y
    ];
    for ((got, _), (x, y)) in h.iter().zip(want) {
        assert!(close(*got, x, y, 1e-9), "{got:?} / ({x} ; {y})");
    }
}

/// Fond du porte-cartes : rectangle 100 × 70, congés de 5 mm (voir `scenario_porte_cartes`).
struct Wallet {
    p: Project,
    piece: PieceId,
    bottom: EntityId,
    a01: EntityId,
    right: EntityId,
}

fn wallet() -> Wallet {
    let mut p = Project::new("Porte-cartes");
    let sheet = p.sheets[0].id;
    let seg = |p: &mut Project, a: PointMm, b: PointMm| {
        p.apply(Command::AddSegment {
            sheet,
            from: PointRef::New(a),
            to: PointRef::New(b),
        })
        .unwrap()
        .entities[0]
    };
    let bottom = seg(&mut p, pt(0.0, 0.0), pt(100.0, 0.0));
    let right = seg(&mut p, pt(100.0, 0.0), pt(100.0, 70.0));
    let top = seg(&mut p, pt(100.0, 70.0), pt(0.0, 70.0));
    let left = seg(&mut p, pt(0.0, 70.0), pt(0.0, 0.0));
    let fillet = |p: &mut Project, first, second, a, b| {
        *p.apply(Command::FilletPaths {
            sheet,
            first,
            second,
            radius_mm: 5.0,
            pick_first: a,
            pick_second: b,
        })
        .unwrap()
        .entities
        .last()
        .unwrap()
    };
    let a01 = fillet(&mut p, bottom, right, pt(50.0, 0.0), pt(100.0, 35.0));
    let a12 = fillet(&mut p, right, top, pt(100.0, 35.0), pt(50.0, 70.0));
    let a23 = fillet(&mut p, top, left, pt(50.0, 70.0), pt(0.0, 35.0));
    let a30 = fillet(&mut p, left, bottom, pt(0.0, 35.0), pt(50.0, 0.0));
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths {
                entities: vec![bottom, a01, right, a12, top, a23, left, a30],
            },
            name: "Fond".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    Wallet {
        p,
        piece,
        bottom,
        a01,
        right,
    }
}

#[test]
fn ca1_reproduit_les_23_percages_du_porte_cartes() {
    let mut w = wallet();
    // Le bord bas mesure 90 mm après les congés (x de 5 à 95) ; le scénario pose 23 perçages à
    // 3 mm du bord, pas de 4 mm, de x = 6 à x = 94 : marges de 1 mm, 88 / 4 = 22 intervalles.
    let o = &w.p.piece(w.piece).unwrap().outline;
    assert!(o[0].entity == w.bottom && !o[0].reversed);
    let mut r = spec(&[w.bottom]);
    r.spacing = RunSpacing::Step { step_mm: 4.0 };
    r.start_margin_mm = 1.0;
    r.end_margin_mm = 1.0;
    r.form = RunForm::Drill { diameter_mm: 1.2 };
    let pv = w.p.marking_run_preview(w.piece, &r).unwrap();
    assert_eq!((pv.holes.len(), pv.step_mm, pv.length_mm), (23, 4.0, 90.0));
    let o = w.p.apply(run_cmd(w.piece, r)).unwrap();
    assert_eq!(o.markings.len(), 23);
    let h = holes(&w.p, w.piece);
    for (k, (at, shape)) in h.iter().enumerate() {
        assert!(close(*at, 6.0 + 4.0 * k as f64, 3.0, 1e-9), "{k} {at:?}");
        assert_eq!(*shape, MarkingShape::Drill { diameter_mm: 1.2 });
    }
}

#[test]
fn ca1_les_points_d_ancrage_sont_dans_un_calque_masque_et_comptes() {
    let mut sc = rect();
    let sheet = |p: &Project, s| p.sheet(s).unwrap().clone();
    let visible = |s: &Sheet| s.points.iter().filter(|p| s.layer_visible(p.layer)).count();
    let before = sheet(&sc.p, sc.s);
    let mut r = spec(&[bottom(&sc)]);
    r.spacing = RunSpacing::Step { step_mm: 4.0 };
    let o = sc.p.apply(run_cmd(sc.piece, r)).unwrap();
    let after = sheet(&sc.p, sc.s);
    // 26 points créés (aucun n'existait à ces endroits), un calque créé, aucun point visible de plus.
    assert_eq!(o.points.len(), 26);
    assert_eq!(after.points.len(), before.points.len() + 26);
    assert_eq!(visible(&after), visible(&before));
    assert_eq!(o.layers.len(), 1);
    let layer = after.layers.iter().find(|l| l.id == o.layers[0]).unwrap();
    assert_eq!(layer.name, MARKING_RUN_POINTS_LAYER_NAME);
    assert!(!layer.visible && !layer.locked);
    assert!(o
        .points
        .iter()
        .all(|id| after.point(*id).unwrap().layer == Some(layer.id)));
    assert!(
        o.notes.iter().any(|n| n.contains("26 point(s) d'ancrage")),
        "{:?}",
        o.notes
    );
    // Chaque marquage est ancré sur un de ces points.
    for m in &sc.p.piece(sc.piece).unwrap().markings {
        let MarkingAnchor::Point { point } = m.at else {
            panic!("ancrage point")
        };
        assert!(o.points.contains(&point));
    }
    // Une seconde rangée réutilise le calque (aucun calque de plus).
    let mut r2 = spec(&[bottom(&sc)]);
    r2.distance_mm = 6.0;
    let o2 = sc.p.apply(run_cmd(sc.piece, r2)).unwrap();
    assert!(o2.layers.is_empty());
    assert_eq!(sc.p.sheet(sc.s).unwrap().layers.len(), after.layers.len());
}

#[test]
fn ca1_un_point_deja_present_est_reutilise_sans_point_visible() {
    let mut sc = rect();
    // Distance 0 : les perçages d'extrémité tombent sur les coins (points existants, visibles).
    let mut r = spec(&[bottom(&sc)]);
    r.distance_mm = 0.0;
    let o = sc.p.apply(run_cmd(sc.piece, r)).unwrap();
    assert_eq!(o.markings.len(), 3);
    assert_eq!(o.points.len(), 1, "seul le perçage du milieu crée un point");
    let h = holes(&sc.p, sc.piece);
    all_close(&xs(&h), &[0.0, 50.0, 100.0], 1e-9);
    all_close(&ys(&h), &[0.0, 0.0, 0.0], 1e-9);
}

#[test]
fn ca1_selection_vers_bord_ordre_du_contour_et_refus() {
    let w = wallet();
    let ordered =
        w.p.marking_run_edges(w.piece, &[w.right, w.bottom, w.a01])
            .unwrap();
    assert_eq!(ordered, vec![w.bottom, w.a01, w.right]);
    assert!(w.p.marking_run_edges(w.piece, &[]).is_err());
    assert!(w
        .p
        .marking_run_edges(w.piece, &[EntityId(9_999_999)])
        .is_err());
    // Deux tracés qui ne se suivent pas.
    assert!(w
        .p
        .marking_run_edges(w.piece, &[w.bottom, w.right])
        .is_err());
}

// ----- CA2 : arc, Bézier, chaîne ----------------------------------------------------------

/// Quart de disque de rayon 50 : segment O→A, arc de A à B (90°, anti-horaire), segment B→O.
fn quarter() -> (Scene, EntityId, EntityId, EntityId) {
    let mut p = Project::new("Quart de disque");
    let s = p.sheets[0].id;
    let oa = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(0.0, 0.0)),
            to: PointRef::New(pt(50.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let arc = p
        .apply(Command::AddArc {
            sheet: s,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: 50.0,
            start_deg: 0.0,
            end_deg: 90.0,
            ccw: true,
        })
        .unwrap()
        .entities[0];
    let bo = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(0.0, 50.0)),
            to: PointRef::New(pt(0.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(10.0, 10.0) },
            name: "Quart".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    (Scene { p, s, piece }, oa, arc, bo)
}

#[test]
fn ca2_arc_de_rayon_50_sur_90_degres_abscisse_curviligne_et_normale() {
    let (mut sc, oa, arc, bo) = quarter();
    let o = &sc.p.piece(sc.piece).unwrap().outline;
    assert!(
        o.iter().all(|e| !e.reversed),
        "premisse : contour anti-horaire d'origine"
    );
    assert_eq!(
        o.iter().map(|e| e.entity).collect::<Vec<_>>(),
        vec![oa, arc, bo]
    );
    // N = 5 : longueur 25π = 78,5398…, perçages à 0, 22,5°, 45°, 67,5°, 90° (abscisses égales).
    let mut r = spec(&[arc]);
    r.spacing = RunSpacing::Count { count: 5 };
    let pv = sc.p.marking_run_preview(sc.piece, &r).unwrap();
    assert!((pv.length_mm - 25.0 * std::f64::consts::PI).abs() < 1e-9);
    assert!((pv.step_mm - 25.0 * std::f64::consts::PI / 4.0).abs() < 1e-9);
    sc.p.apply(run_cmd(sc.piece, r)).unwrap();
    for (i, (at, _)) in holes(&sc.p, sc.piece).iter().enumerate() {
        // Angle polaire autour du centre (0 ; 0) et rayon 50 − 3 = 47 (côté intérieur).
        let angle = at.y.atan2(at.x).to_degrees();
        let radius = at.x.hypot(at.y);
        assert!((angle - 22.5 * i as f64).abs() < 1e-9, "{i} : {angle}");
        assert!((radius - 47.0).abs() < 1e-9, "{i} : {radius}");
    }
    // La corde entre deux perçages (≈ 18,38 mm) n'est pas le pas curviligne (19,63 mm).
    let h = holes(&sc.p, sc.piece);
    let chord = (h[1].0.x - h[0].0.x).hypot(h[1].0.y - h[0].0.y);
    assert!((chord - 2.0 * 47.0 * (11.25f64).to_radians().sin()).abs() < 1e-9);
}

#[test]
fn ca2_arc_en_mode_pas_et_cote_exterieur() {
    let (mut sc, _, arc, _) = quarter();
    // Pas 10 : ⌈78,5398 / 10⌉ = 8 intervalles de 9,8175 mm, soit 11,25° chacun ; rayon 53.
    let mut r = spec(&[arc]);
    r.spacing = RunSpacing::Step { step_mm: 10.0 };
    r.side = RunSide::Outside;
    sc.p.apply(run_cmd(sc.piece, r)).unwrap();
    let h = holes(&sc.p, sc.piece);
    assert_eq!(h.len(), 9);
    for (i, (at, _)) in h.iter().enumerate() {
        assert!((at.y.atan2(at.x).to_degrees() - 11.25 * i as f64).abs() < 1e-9);
        assert!((at.x.hypot(at.y) - 53.0).abs() < 1e-9);
    }
}

/// Courbe de Bézier (0 ; 0) (20 ; 60) (80 ; 60) (100 ; 0), fermée par sa corde.
fn bezier_piece() -> (Scene, EntityId) {
    let mut p = Project::new("Bézier");
    let s = p.sheets[0].id;
    let chord = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(pt(0.0, 0.0)),
            to: PointRef::New(pt(100.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let curve = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(pt(0.0, 0.0)),
            c1: pt(20.0, 60.0),
            c2: pt(80.0, 60.0),
            end: PointRef::New(pt(100.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(50.0, 10.0) },
            name: "Bombé".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let _ = chord;
    (Scene { p, s, piece }, curve)
}

/// Point de la courbe du test (formule de Bernstein écrite ici, indépendante du code testé).
fn bezier(t: f64) -> (f64, f64) {
    let u = 1.0 - t;
    (
        3.0 * u * u * t * 20.0 + 3.0 * u * t * t * 80.0 + t * t * t * 100.0,
        3.0 * u * u * t * 60.0 + 3.0 * u * t * t * 60.0,
    )
}

#[test]
fn ca2_bezier_compare_a_un_echantillonnage_dense_independant() {
    let (mut sc, curve) = bezier_piece();
    let edge =
        sc.p.piece(sc.piece)
            .unwrap()
            .outline
            .iter()
            .find(|e| e.entity == curve)
            .copied()
            .unwrap();
    // Premisse : le contour anti-horaire parcourt la courbe de son arrivée vers son départ.
    assert!(edge.reversed);
    // Échantillonnage dense, dans le sens du contour (t décroissant) : 400 000 pas.
    let n = 400_000;
    let samples: Vec<(f64, f64)> = (0..=n).map(|i| bezier(1.0 - i as f64 / n as f64)).collect();
    let mut cum = vec![0.0];
    for w in samples.windows(2) {
        cum.push(cum.last().unwrap() + (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1));
    }
    let total = *cum.last().unwrap();
    let mut r = spec(&[curve]);
    r.spacing = RunSpacing::Count { count: 7 };
    r.distance_mm = 4.0;
    r.start_margin_mm = 5.0;
    r.end_margin_mm = 8.0;
    let pv = sc.p.marking_run_preview(sc.piece, &r).unwrap();
    assert!(
        (pv.length_mm - total).abs() < 1e-3,
        "{} / {total}",
        pv.length_mm
    );
    sc.p.apply(run_cmd(sc.piece, r)).unwrap();
    let h = holes(&sc.p, sc.piece);
    assert_eq!(h.len(), 7);
    for (i, (at, _)) in h.iter().enumerate() {
        let s = 5.0 + (total - 13.0) * i as f64 / 6.0;
        let k = cum.partition_point(|c| *c < s).clamp(1, n);
        let f = (s - cum[k - 1]) / (cum[k] - cum[k - 1]);
        let (a, b) = (samples[k - 1], samples[k]);
        let p = (a.0 + (b.0 - a.0) * f, a.1 + (b.1 - a.1) * f);
        // Tangente par différence centrée sur 2 pas, normale à gauche (intérieur, anti-horaire).
        let (c, d) = (samples[k.saturating_sub(5)], samples[(k + 5).min(n)]);
        let l = (d.0 - c.0).hypot(d.1 - c.1);
        let (tx, ty) = ((d.0 - c.0) / l, (d.1 - c.1) / l);
        let want = (p.0 - ty * 4.0, p.1 + tx * 4.0);
        assert!(close(*at, want.0, want.1, 1e-3), "{i} : {at:?} / {want:?}");
    }
}

#[test]
fn ca2_chaine_segment_arc_segment_abscisse_sur_la_longueur_totale() {
    let mut w = wallet();
    let chain =
        w.p.marking_run_edges(w.piece, &[w.right, w.bottom, w.a01])
            .unwrap();
    assert_eq!(chain, vec![w.bottom, w.a01, w.right]);
    // Longueur totale : 90 + 2,5π (arc de 5 mm sur 90°) + 60.
    let total = 150.0 + 2.5 * std::f64::consts::PI;
    let mut r = spec(&chain);
    r.spacing = RunSpacing::Count { count: 9 };
    let pv = w.p.marking_run_preview(w.piece, &r).unwrap();
    assert!((pv.length_mm - total).abs() < 1e-9);
    w.p.apply(run_cmd(w.piece, r)).unwrap();
    let h = holes(&w.p, w.piece);
    assert_eq!(h.len(), 9);
    for (i, (at, _)) in h.iter().enumerate() {
        let s = total * i as f64 / 8.0;
        // Formules à la main : bas (x = 5 + s), arc de centre (95 ; 5) de rayon 5 − 3 = 2
        // (angle de −90° à 0°), puis bord droit (x = 97, y = 5 + s').
        let (x, y) = if s <= 90.0 {
            (5.0 + s, 3.0)
        } else if s <= 90.0 + 2.5 * std::f64::consts::PI {
            let phi = -std::f64::consts::FRAC_PI_2 + (s - 90.0) / 5.0;
            (95.0 + 2.0 * phi.cos(), 5.0 + 2.0 * phi.sin())
        } else {
            (97.0, 5.0 + (s - 90.0 - 2.5 * std::f64::consts::PI))
        };
        assert!(close(*at, x, y, 1e-9), "{i} : {at:?} / ({x} ; {y})");
    }
}

// ----- CA3 : fente de poinçon orientée le long du bord --------------------------------------

fn slot(length_mm: f64, width_mm: f64) -> RunForm {
    RunForm::Slot {
        length_mm,
        width_mm,
    }
}

#[test]
fn ca3_fente_orientee_le_long_d_un_segment_oblique_3_4_5() {
    let mut p = Project::new("Oblique");
    let s = p.sheets[0].id;
    p.apply(Command::AddPolygon {
        sheet: s,
        vertices: [(0.0, 0.0), (40.0, 0.0), (70.0, 40.0), (30.0, 40.0)]
            .map(|(x, y)| PointRef::New(pt(x, y)))
            .to_vec(),
        closed: true,
        role: PathRole::default(),
        layer: None,
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(35.0, 20.0) },
            name: "Losange".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let oblique = seg_between(&p, s, pt(40.0, 0.0), pt(70.0, 40.0));
    let mut r = spec(&[oblique]);
    r.spacing = RunSpacing::Count { count: 2 };
    r.form = slot(6.0, 1.5);
    p.apply(run_cmd(piece, r)).unwrap();
    let h = holes(&p, piece);
    // Direction (30 ; 40) / 50 = (0,6 ; 0,8) : angle atan2(4, 3) = 53,13010235415598°.
    // Normale intérieure (gauche) (−0,8 ; 0,6) × 3 = (−2,4 ; 1,8).
    assert!(close(h[0].0, 40.0 - 2.4, 1.8, 1e-9) && close(h[1].0, 70.0 - 2.4, 40.0 + 1.8, 1e-9));
    for (_, shape) in &h {
        let MarkingShape::Slot {
            length_mm,
            width_mm,
            angle_deg,
        } = *shape
        else {
            panic!("fente attendue");
        };
        assert_eq!((length_mm, width_mm), (6.0, 1.5));
        assert!((angle_deg - 53.13010235415598).abs() < 1e-9, "{angle_deg}");
    }
    // Un autre bord : (70 ; 40) → (30 ; 40) vers −x = 180°, (30 ; 40) → (0 ; 0) = 233,13010…°.
    let top = seg_between(&p, s, pt(70.0, 40.0), pt(30.0, 40.0));
    let diag = seg_between(&p, s, pt(30.0, 40.0), pt(0.0, 0.0));
    for (edge, want) in [(top, 180.0), (diag, 180.0 + 53.13010235415598)] {
        let before = p.piece(piece).unwrap().markings.len();
        let mut r = spec(&[edge]);
        r.form = slot(4.0, 1.0);
        p.apply(run_cmd(piece, r)).unwrap();
        let m = p.piece(piece).unwrap().markings[before];
        let MarkingShape::Slot { angle_deg, .. } = m.shape else {
            panic!("fente")
        };
        assert!((angle_deg - want).abs() < 1e-9, "{angle_deg} / {want}");
    }
}

#[test]
fn ca3_fente_orientee_le_long_d_un_arc_tangente_locale() {
    let (mut sc, _, arc, _) = quarter();
    let mut r = spec(&[arc]);
    r.spacing = RunSpacing::Count { count: 5 };
    r.form = slot(8.0, 2.0);
    sc.p.apply(run_cmd(sc.piece, r)).unwrap();
    // Arc anti-horaire : la tangente à l'angle polaire φ est φ + 90°.
    for (i, (_, shape)) in holes(&sc.p, sc.piece).iter().enumerate() {
        let MarkingShape::Slot { angle_deg, .. } = *shape else {
            panic!("fente")
        };
        let want = (22.5 * i as f64 + 90.0) % 360.0;
        assert!(
            (angle_deg - want).abs() < 1e-9,
            "{i} : {angle_deg} / {want}"
        );
    }
}

#[test]
fn ca3_fente_bornes_validation_et_format() {
    let ok = MarkingShape::Slot {
        length_mm: 6.0,
        width_mm: 1.5,
        angle_deg: 12.0,
    };
    assert!(ok.validate().is_ok());
    for bad in [
        (0.5, 0.6, 0.0),    // longueur < largeur
        (6.0, 0.09, 0.0),   // largeur trop petite
        (6.0, 100.01, 0.0), // largeur trop grande
        (100.01, 2.0, 0.0), // longueur trop grande
        (6.0, f64::NAN, 0.0),
        (f64::INFINITY, 2.0, 0.0),
        (6.0, 2.0, 360.0),
        (6.0, 2.0, -0.1),
        (6.0, 2.0, f64::NAN),
    ] {
        let s = MarkingShape::Slot {
            length_mm: bad.0,
            width_mm: bad.1,
            angle_deg: bad.2,
        };
        assert!(s.validate().is_err(), "{bad:?}");
    }
    // Fente ronde (longueur = largeur) acceptée ; fente maximale acceptée.
    assert!(MarkingShape::Slot {
        length_mm: 2.0,
        width_mm: 2.0,
        angle_deg: 0.0
    }
    .validate()
    .is_ok());
    assert!(MarkingShape::Slot {
        length_mm: 100.0,
        width_mm: 100.0,
        angle_deg: 359.9
    }
    .validate()
    .is_ok());
    // Format : variante à champs, clés inconnues et champ manquant refusés.
    let text = serde_json::to_string(&ok).unwrap();
    assert_eq!(
        text,
        r#"{"type":"slot","length_mm":6.0,"width_mm":1.5,"angle_deg":12.0}"#
    );
    assert_eq!(serde_json::from_str::<MarkingShape>(&text).unwrap(), ok);
    assert!(serde_json::from_str::<MarkingShape>(
        r#"{"type":"slot","length_mm":6.0,"width_mm":1.5,"angle_deg":12.0,"z":1}"#
    )
    .is_err());
    assert!(serde_json::from_str::<MarkingShape>(
        r#"{"type":"slot","length_mm":6.0,"width_mm":1.5}"#
    )
    .is_err());
    assert!(serde_json::from_str::<MarkingShape>(r#"{"type":"slot"}"#).is_err());
    assert_eq!(ok.key(), "slot");
    assert_eq!(ok.size_mm(), None);
    // `rotated` : seul l'angle d'une fente change, ramené dans [0 ; 360[.
    let MarkingShape::Slot { angle_deg, .. } = ok.rotated(350.0) else {
        panic!()
    };
    assert!((angle_deg - 2.0).abs() < 1e-9);
    let MarkingShape::Slot { angle_deg, .. } = ok.rotated(-20.0) else {
        panic!()
    };
    assert!((angle_deg - 352.0).abs() < 1e-9);
    assert_eq!(MarkingShape::Punch.rotated(30.0), MarkingShape::Punch);
}

// ----- CA4 : refus sans trace -------------------------------------------------------------

#[test]
fn ca4_refus_sans_trace() {
    let mut sc = rect();
    let edge = bottom(&sc);
    let piece = sc.piece;
    let ok = spec(&[edge]);

    // Bord : absent, vide, hors contour, d'une autre pièce, inconnu, doublé, non consécutif.
    let mut other = ok.clone();
    other.edges = vec![];
    refused(&mut sc.p, run_cmd(piece, other));
    let mut other = ok.clone();
    other.edges = vec![EntityId(9_999_999)];
    refused(&mut sc.p, run_cmd(piece, other));
    let mut other = ok.clone();
    other.edges = vec![edge, edge];
    refused(&mut sc.p, run_cmd(piece, other));
    let right = seg_between(&sc.p, sc.s, pt(100.0, 0.0), pt(100.0, 70.0));
    let top = seg_between(&sc.p, sc.s, pt(100.0, 70.0), pt(0.0, 70.0));
    let mut other = ok.clone();
    other.edges = vec![edge, top];
    refused(&mut sc.p, run_cmd(piece, other));
    let mut other = ok.clone();
    other.edges = vec![right, edge]; // dans le mauvais ordre du contour
    refused(&mut sc.p, run_cmd(piece, other));
    // Tracé de la feuille qui n'est pas du contour.
    let free =
        sc.p.apply(Command::AddSegment {
            sheet: sc.s,
            from: PointRef::New(pt(10.0, 10.0)),
            to: PointRef::New(pt(20.0, 10.0)),
        })
        .unwrap()
        .entities[0];
    let mut other = ok.clone();
    other.edges = vec![free];
    assert!(refused(&mut sc.p, run_cmd(piece, other)).contains("contour"));
    // Pièce inconnue.
    refused(&mut sc.p, run_cmd(PieceId(9_999_999), ok.clone()));
    // Bord d'une autre pièce.
    sc.p.apply(Command::AddRectangle {
        sheet: sc.s,
        corner: PointRef::New(pt(300.0, 0.0)),
        width_mm: 50.0,
        height_mm: 50.0,
    })
    .unwrap();
    sc.p.apply(Command::ExtractPiece {
        sheet: sc.s,
        source: PieceSource::Click {
            at: pt(320.0, 20.0),
        },
        name: "Autre".into(),
        detached: false,
    })
    .unwrap();
    let foreign = seg_between(&sc.p, sc.s, pt(300.0, 0.0), pt(350.0, 0.0));
    let mut other = ok.clone();
    other.edges = vec![foreign];
    assert!(refused(&mut sc.p, run_cmd(piece, other)).contains("contour"));

    // Pas, nombre, distance, marges.
    let mut bad: Vec<MarkingRunSpec> = Vec::new();
    for step in [
        0.0,
        0.49,
        1000.01,
        -4.0,
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
    ] {
        let mut r = ok.clone();
        r.spacing = RunSpacing::Step { step_mm: step };
        bad.push(r);
    }
    for count in [0, 501, u32::MAX] {
        let mut r = ok.clone();
        r.spacing = RunSpacing::Count { count };
        bad.push(r);
    }
    for dist in [-0.001, 100.001, f64::NAN, f64::INFINITY, -f64::INFINITY] {
        let mut r = ok.clone();
        r.distance_mm = dist;
        bad.push(r);
    }
    for m in [-1.0, f64::NAN, f64::INFINITY] {
        let mut r = ok.clone();
        r.start_margin_mm = m;
        bad.push(r);
        let mut r = ok.clone();
        r.end_margin_mm = m;
        bad.push(r);
    }
    // Marges qui dépassent (ou égalent, pour plusieurs perçages) la longueur de 100 mm.
    for (a, b, spacing) in [
        (60.0, 50.0, RunSpacing::Count { count: 3 }),
        (60.0, 50.0, RunSpacing::Count { count: 1 }),
        (50.0, 50.0, RunSpacing::Count { count: 3 }),
        (50.0, 50.0, RunSpacing::Step { step_mm: 5.0 }),
        (100.0, 0.0, RunSpacing::Step { step_mm: 5.0 }),
        (200.0, 0.0, RunSpacing::Step { step_mm: 5.0 }),
    ] {
        let mut r = ok.clone();
        (r.start_margin_mm, r.end_margin_mm, r.spacing) = (a, b, spacing);
        bad.push(r);
    }
    // Dimensions : diamètre (US-06.3) et fente.
    for d in [0.0, 0.09, 100.01, f64::NAN, f64::INFINITY, -1.0] {
        let mut r = ok.clone();
        r.form = RunForm::Drill { diameter_mm: d };
        bad.push(r);
    }
    for (l, w) in [
        (0.5, 0.6),
        (6.0, 0.09),
        (6.0, 100.01),
        (100.01, 2.0),
        (f64::NAN, 2.0),
        (6.0, f64::NAN),
    ] {
        let mut r = ok.clone();
        r.form = slot(l, w);
        bad.push(r);
    }
    for r in bad {
        refused(&mut sc.p, run_cmd(piece, r.clone()));
        // Le même refus à l'aperçu, sans rien modifier.
        assert!(sc.p.marking_run_preview(piece, &r).is_err(), "{r:?}");
    }
    assert!(sc.p.piece(piece).unwrap().markings.is_empty());
}

#[test]
fn ca4_limites_de_500_par_rangee_et_de_1000_par_piece() {
    // Bord de 300 mm : pas 0,5 → 601 perçages (> 500) refusé ; pas 0,6 → 500 intervalles = 501.
    let mut p = Project::new("Long");
    let s = p.sheets[0].id;
    p.apply(Command::AddRectangle {
        sheet: s,
        corner: PointRef::New(pt(0.0, 0.0)),
        width_mm: 300.0,
        height_mm: 100.0,
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click {
                at: pt(150.0, 50.0),
            },
            name: "Long".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let edge = seg_between(&p, s, pt(0.0, 0.0), pt(300.0, 0.0));
    let mut r = spec(&[edge]);
    r.spacing = RunSpacing::Step { step_mm: 0.5 };
    refused(&mut p, run_cmd(piece, r.clone()));
    r.spacing = RunSpacing::Step { step_mm: 0.6 };
    refused(&mut p, run_cmd(piece, r.clone())); // 300 / 0,6 = 500 intervalles, 501 perçages
    r.spacing = RunSpacing::Step { step_mm: 0.6013 };
    assert_eq!(
        p.apply(run_cmd(piece, r.clone())).unwrap().markings.len(),
        500
    );
    // 500 de plus : 1000 au total, accepté ; un seul de plus : refusé (limite de la pièce).
    r.spacing = RunSpacing::Count { count: 500 };
    r.distance_mm = 6.0;
    assert_eq!(
        p.apply(run_cmd(piece, r.clone())).unwrap().markings.len(),
        500
    );
    assert_eq!(p.piece(piece).unwrap().markings.len(), 1000);
    r.spacing = RunSpacing::Count { count: 1 };
    let msg = refused(&mut p, run_cmd(piece, r.clone()));
    assert!(msg.contains("1000"), "{msg}");
    assert!(p.marking_run_preview(piece, &r).is_err());
}

#[test]
fn ca4_piece_sans_contour_ferme_et_piece_detachee() {
    // Pièce sans contour : fixture du dépôt.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let text = std::fs::read_to_string(dir.join("v2-piece-sans-contour.pforge.json")).unwrap();
    let mut p = persist::from_json(&text).unwrap();
    let piece = p.pieces[0].id;
    assert!(p.piece(piece).unwrap().outline.is_empty());
    let msg = refused(&mut p, run_cmd(piece, spec(&[EntityId(1)])));
    assert!(!msg.is_empty());
    // Contour ouvert : trois côtés d'un rectangle seulement, imposés par SetPieceOutline.
    let mut sc = rect();
    let full = sc.p.piece(sc.piece).unwrap().outline.clone();
    let open = full[..3].to_vec();
    if sc
        .p
        .apply(Command::SetPieceOutline {
            piece: sc.piece,
            outline: open,
            grainline: None,
        })
        .is_ok()
    {
        let edge = sc.p.piece(sc.piece).unwrap().outline[0].entity;
        refused(&mut sc.p, run_cmd(sc.piece, spec(&[edge])));
    }
    // Pièce détachée : le contour fermé suffit, les points vont sur sa feuille.
    let mut p = Project::new("Détachée");
    let s = p.sheets[0].id;
    p.apply(Command::AddRectangle {
        sheet: s,
        corner: PointRef::New(pt(0.0, 0.0)),
        width_mm: 100.0,
        height_mm: 70.0,
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(50.0, 35.0) },
            name: "Détachée".into(),
            detached: true,
        })
        .unwrap()
        .pieces[0];
    let sheet = p.piece_sheet(piece).unwrap().id;
    let edge = p.piece(piece).unwrap().outline[0].entity;
    let o = p.apply(run_cmd(piece, spec(&[edge]))).unwrap();
    assert_eq!(o.markings.len(), 3);
    assert_eq!(p.piece_markings(piece).len(), 3);
    assert!(p
        .sheet(sheet)
        .unwrap()
        .layers
        .iter()
        .any(|l| l.name == MARKING_RUN_POINTS_LAYER_NAME));
}

#[test]
fn ca4_calque_verrouille_refuse_les_points_d_ancrage() {
    let mut sc = rect();
    let edge = bottom(&sc);
    let o = sc.p.apply(run_cmd(sc.piece, spec(&[edge]))).unwrap();
    let layer = o.layers[0];
    sc.p.apply(Command::SetLayerLocked {
        sheet: sc.s,
        layer,
        locked: true,
    })
    .unwrap();
    let mut r = spec(&[edge]);
    r.distance_mm = 8.0;
    let before = sc.p.clone();
    let err = sc.p.apply(run_cmd(sc.piece, r.clone())).unwrap_err();
    assert!(matches!(err, CoreError::LayerLocked(_)), "{err:?}");
    assert_eq!(sc.p, before);
    assert!(sc.p.marking_run_preview(sc.piece, &r).is_err());
    // Déverrouillé : accepté.
    sc.p.apply(Command::SetLayerLocked {
        sheet: sc.s,
        layer,
        locked: false,
    })
    .unwrap();
    sc.p.apply(run_cmd(sc.piece, r)).unwrap();
}

#[test]
fn ca4_calque_verrouille_du_bord_n_est_pas_modifie_donc_pas_un_refus() {
    // Le bord reste intact : seuls des points d'ancrage et des marquages sont ajoutés.
    let mut p = Project::new("Bord verrouillé");
    let s = p.sheets[0].id;
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Contour".into(),
        })
        .unwrap()
        .layers[0];
    p.apply(Command::InLayer {
        sheet: s,
        layer,
        command: Box::new(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(pt(0.0, 0.0)),
            width_mm: 100.0,
            height_mm: 70.0,
        }),
    })
    .unwrap();
    let piece = p
        .apply(Command::ExtractPiece {
            sheet: s,
            source: PieceSource::Click { at: pt(50.0, 35.0) },
            name: "Fond".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: true,
    })
    .unwrap();
    let edge = p.piece(piece).unwrap().outline[0].entity;
    let o = p.apply(run_cmd(piece, spec(&[edge]))).unwrap();
    assert_eq!(o.markings.len(), 3);
}

#[test]
fn ca4_un_percage_hors_de_la_piece_est_un_avertissement_pas_un_refus() {
    let mut sc = rect();
    let edge = bottom(&sc);
    // 80 mm vers l'intérieur d'une pièce haute de 70 mm : centres hors du contour, accepté.
    let mut r = spec(&[edge]);
    r.distance_mm = 80.0;
    let o = sc.p.apply(run_cmd(sc.piece, r)).unwrap();
    assert_eq!(o.markings.len(), 3);
    assert!(sc
        .p
        .piece_issues(sc.piece)
        .contains(&PieceIssue::MarkingOutsideOutline { count: 3 }));
    // Distance 0 : sur le bord (compris dans la pièce) : aucun avertissement.
    let mut sc = rect();
    let mut r = spec(&[bottom(&sc)]);
    r.distance_mm = 0.0;
    sc.p.apply(run_cmd(sc.piece, r)).unwrap();
    assert!(sc
        .p
        .piece_issues(sc.piece)
        .iter()
        .all(|i| !matches!(i, PieceIssue::MarkingOutsideOutline { .. })));
}

#[test]
fn ca4_contour_entier_ferme_refuse_si_premier_et_dernier_se_confondent() {
    let mut sc = rect();
    let chain =
        sc.p.piece(sc.piece)
            .unwrap()
            .outline
            .iter()
            .map(|e| e.entity)
            .collect::<Vec<_>>();
    let mut r = spec(&chain);
    r.spacing = RunSpacing::Count { count: 10 };
    // Périmètre 340 : sans marge, le dernier perçage tomberait sur le premier.
    let msg = refused(&mut sc.p, run_cmd(sc.piece, r.clone()));
    assert!(msg.contains("premier"), "{msg}");
    // Avec une marge de fin d'un pas (340 / 10 = 34 mm) : 10 perçages à 34 mm l'un de l'autre.
    r.end_margin_mm = 340.0 / 10.0;
    sc.p.apply(run_cmd(sc.piece, r)).unwrap();
    assert_eq!(sc.p.piece_markings(sc.piece).len(), 10);
}

// ----- CA5 : une étape annulable, indépendance, cohérence -----------------------------------

#[test]
fn ca5_une_seule_etape_annulable_octets_identiques() {
    let sc = rect();
    let edge = bottom(&sc);
    let mut doc = Document::new(sc.p.clone());
    let original = bytes(doc.project());
    let mut r = spec(&[edge]);
    r.spacing = RunSpacing::Step { step_mm: 4.0 };
    doc.execute(run_cmd(sc.piece, r)).unwrap();
    let done = bytes(doc.project());
    assert_ne!(done, original);
    assert_eq!(doc.project().piece(sc.piece).unwrap().markings.len(), 26);
    assert!(doc.undo());
    assert_eq!(
        bytes(doc.project()),
        original,
        "annuler : octets identiques"
    );
    assert!(!doc.can_undo(), "une seule étape");
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), done, "rétablir : octets identiques");
    // Relecture JSON identique.
    let back = persist::from_json(&done).unwrap();
    assert_eq!(bytes(&back), done);
    assert_eq!(back, *doc.project());
    // Un refus n'ajoute aucune étape.
    let mut bad = spec(&[edge]);
    bad.distance_mm = -1.0;
    assert!(doc.execute(run_cmd(sc.piece, bad)).is_err());
    assert!(doc.undo());
    assert!(!doc.can_undo());
}

#[test]
fn ca5_les_percages_sont_independants_et_figes_sauf_sur_un_point_existant() {
    let mut sc = rect();
    let edge = bottom(&sc);
    // Distance 0 : le premier perçage est ancré au coin (0 ; 0) existant, les autres sur des
    // points libres créés (positions figées).
    let mut r = spec(&[edge]);
    r.distance_mm = 0.0;
    r.spacing = RunSpacing::Count { count: 5 };
    let o = sc.p.apply(run_cmd(sc.piece, r)).unwrap();
    let ids = o.markings.clone();
    let corner = {
        let m = sc.p.piece(sc.piece).unwrap().markings[0];
        let MarkingAnchor::Point { point } = m.at else {
            panic!()
        };
        point
    };
    assert!(
        !o.points.contains(&corner),
        "le coin existant est réutilisé"
    );
    sc.p.apply(Command::MovePoint {
        sheet: sc.s,
        point: corner,
        to: pt(-10.0, -5.0),
    })
    .unwrap();
    let h = holes(&sc.p, sc.piece);
    assert!(close(h[0].0, -10.0, -5.0, 1e-9), "suit le point déplacé");
    for (k, (at, _)) in h.iter().enumerate().skip(1) {
        assert!(
            close(*at, 25.0 * k as f64, 0.0, 1e-9),
            "{k} : positions figées"
        );
    }
    // Supprimer un perçage de la rangée n'affecte pas les autres.
    let before = holes(&sc.p, sc.piece);
    sc.p.apply(Command::RemovePieceMarking {
        piece: sc.piece,
        marking: ids[2],
    })
    .unwrap();
    let after = holes(&sc.p, sc.piece);
    assert_eq!(after.len(), 4);
    let mut expected = before.clone();
    expected.remove(2);
    assert_eq!(after, expected);
    // Chaque perçage reste un marquage ordinaire : modifiable (forme) comme les autres.
    let m = sc.p.piece(sc.piece).unwrap().markings[1];
    sc.p.apply(Command::SetPieceMarking {
        piece: sc.piece,
        marking: m.id,
        at: m.at,
        shape: MarkingShape::Cross { size_mm: 4.0 },
    })
    .unwrap();
    // Un identifiant par perçage, tous distincts.
    let mut sorted = ids.clone();
    sorted.sort_by_key(|i| i.0);
    sorted.dedup();
    assert_eq!(sorted.len(), 5);
}

#[test]
fn ca5_supprimer_un_point_d_ancrage_retire_le_percage_avec_remarque() {
    let mut sc = rect();
    let o = sc.p.apply(run_cmd(sc.piece, spec(&[bottom(&sc)]))).unwrap();
    let p0 = o.points[0];
    let r =
        sc.p.apply(Command::Delete {
            sheet: sc.s,
            points: vec![p0],
            entities: vec![],
        })
        .unwrap();
    assert_eq!(sc.p.piece_markings(sc.piece).len(), 2);
    assert!(
        r.notes.iter().any(|n| n.contains("marquage")),
        "{:?}",
        r.notes
    );
}

#[test]
fn ca5_copie_de_feuille_et_import_gardent_les_percages_et_les_fentes() {
    let mut sc = rect();
    let mut r = spec(&[bottom(&sc)]);
    r.spacing = RunSpacing::Count { count: 4 };
    r.form = slot(5.0, 1.0);
    sc.p.apply(run_cmd(sc.piece, r)).unwrap();
    let before = sc.p.clone();
    let o = sc.p.apply(Command::DuplicateSheet { sheet: sc.s }).unwrap();
    sc.p.validate().unwrap();
    let copy = o.pieces[0];
    assert_eq!(holes(&sc.p, copy), holes(&sc.p, sc.piece));
    assert_eq!(
        sc.p.piece(sc.piece).unwrap(),
        before.piece(sc.piece).unwrap()
    );
    // La copie a ses propres points, dans son propre calque masqué.
    let PieceLink::Linked { sheet: new_sheet } = sc.p.piece(copy).unwrap().link else {
        panic!()
    };
    let sheet = sc.p.sheet(new_sheet).unwrap();
    let layer = sheet
        .layers
        .iter()
        .find(|l| l.name == MARKING_RUN_POINTS_LAYER_NAME)
        .unwrap();
    assert!(!layer.visible);
    // Import dans un autre projet.
    let mut other = Project::new("Autre");
    let imported = other.apply(before.import_command(sc.s).unwrap()).unwrap();
    other.validate().unwrap();
    assert_eq!(holes(&other, imported.pieces[0]), holes(&before, sc.piece));
}

#[test]
fn ca5_presse_papiers_colle_les_points_d_ancrage_sans_toucher_aux_marquages() {
    let mut sc = rect();
    let o = sc.p.apply(run_cmd(sc.piece, spec(&[bottom(&sc)]))).unwrap();
    let h = holes(&sc.p, sc.piece);
    let fragment = sc.p.copy_fragment(sc.s, &o.points, &[], &[], &[]).unwrap();
    sc.p.apply(Command::PasteFragment {
        sheet: sc.s,
        fragment: Box::new(fragment),
        layer: None,
    })
    .unwrap();
    sc.p.validate().unwrap();
    assert_eq!(
        holes(&sc.p, sc.piece),
        h,
        "les marquages ne dépendent pas des copies"
    );
}

#[test]
fn ca5_separer_puis_assembler_conserve_positions_et_orientation_des_fentes() {
    let mut sc = rect();
    let mut r = spec(&[bottom(&sc)]);
    r.spacing = RunSpacing::Count { count: 4 };
    r.form = slot(5.0, 1.0);
    sc.p.apply(run_cmd(sc.piece, r)).unwrap();
    // Coupe verticale à x = 50 : x = 0 et 33,33 à gauche, 66,67 et 100 à droite.
    let o =
        sc.p.apply(Command::SplitPiece {
            piece: sc.piece,
            a: pt(50.0, -10.0),
            b: pt(50.0, 80.0),
        })
        .unwrap();
    sc.p.validate().unwrap();
    let (l, r2) = (o.pieces[0], o.pieces[1]);
    let (lh, rh) = (holes(&sc.p, l), holes(&sc.p, r2));
    assert_eq!((lh.len(), rh.len()), (2, 2), "{:?}", o.notes);
    assert!(close(lh[1].0, 100.0 / 3.0, 3.0, 1e-9) && close(rh[0].0, 200.0 / 3.0, 3.0, 1e-9));
    for (_, s) in lh.iter().chain(&rh) {
        let MarkingShape::Slot { angle_deg, .. } = *s else {
            panic!()
        };
        assert!(angle_deg.abs() < 1e-9);
    }
    let edge = |p: &Project, half: PieceId| p.piece(half).unwrap().outline.last().unwrap().entity;
    let j =
        sc.p.apply(Command::JoinPieces {
            first: l,
            first_edge: edge(&sc.p, l),
            second: r2,
            second_edge: edge(&sc.p, r2),
            tolerance_mm: 0.5,
        })
        .unwrap();
    sc.p.validate().unwrap();
    let merged = holes(&sc.p, j.pieces[0]);
    assert_eq!(merged.len(), 4);
    for (k, x) in [0.0, 100.0 / 3.0, 200.0 / 3.0, 100.0].iter().enumerate() {
        assert!(close(merged[k].0, *x, 3.0, 1e-6), "{k} : {:?}", merged[k].0);
    }
}

// ----- CA6 : fichiers existants et format ----------------------------------------------------

#[test]
fn ca6_fichiers_existants_inchanges_a_l_octet() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for name in [
        "v1-exercice.pforge.json",
        "v1-grande-piece.pforge.json",
        "v2-piece-contour-droit-fil.pforge.json",
        "v2-piece-sans-contour.pforge.json",
    ] {
        let first =
            bytes(&persist::from_json(&std::fs::read_to_string(dir.join(name)).unwrap()).unwrap());
        assert_eq!(first, bytes(&persist::from_json(&first).unwrap()), "{name}");
        assert!(!first.contains("\"slot\"") && !first.contains(MARKING_RUN_POINTS_LAYER_NAME));
    }
    for entry in
        std::fs::read_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples")).unwrap()
    {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(".pforge.json") {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n");
        assert_eq!(
            text,
            persist::to_json(&persist::from_json(&text).unwrap()).unwrap(),
            "{path:?}"
        );
    }
}

#[test]
fn ca6_la_commande_se_serialise_et_se_relit() {
    let sc = rect();
    let mut r = spec(&[bottom(&sc)]);
    r.form = slot(6.0, 1.5);
    let c = run_cmd(sc.piece, r);
    let v = serde_json::to_value(&c).unwrap();
    assert_eq!(v["type"], json!("add_piece_marking_run"));
    assert_eq!(v["run"]["form"]["type"], json!("slot"));
    let back: Command = serde_json::from_value(v).unwrap();
    assert_eq!(back, c);
}

//! US-03.3 — longueur développée imposée à une courbe de Bézier (03.3a) : calculs faits à la main,
//! refus sans trace (octets identiques), une seule étape annulable, coordination avec les
//! formules, constructions maintenues, calques verrouillés, copies, ouverture d'un fichier altéré.
//!
//! Sous-ensemble : courbe de Bézier ; inconnue = longueur d'une poignée ou extrémité libre qui
//! glisse le long d'une direction ; cible = nombre. Voir `docs/DECISIONS.md` 320.

use pf_core::persist;
use pf_core::*;
use pf_geometry::{PointMm, Side, Transform};
use serde_json::{json, Value};

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn mv(s: SheetId, point: PointId, x: f64, y: f64) -> Command {
    Command::MovePoint {
        sheet: s,
        point,
        to: PointMm::new(x, y),
    }
}

/// Commande refusée : projet **identique à l'octet** ; renvoie le message.
fn refused(p: &mut Project, cmd: Command) -> String {
    let before = bytes(p);
    let before_p = p.clone();
    let e = p.apply(cmd).expect_err("devrait être refusée").to_string();
    assert_eq!(bytes(p), before, "trace laissée par un refus : {e}");
    assert_eq!(*p, before_p);
    e
}

fn impose(s: SheetId, curve: EntityId, length_mm: f64, adjust: LengthAdjust) -> Command {
    Command::SetCurveLengthConstraint {
        sheet: s,
        curve,
        spec: Some(LengthSpec {
            length_mm,
            adjust,
            reference: None,
        }),
    }
}

fn slide(point: PointId, angle_deg: f64) -> LengthAdjust {
    LengthAdjust::Slide { point, angle_deg }
}

fn release(s: SheetId, curve: EntityId) -> Command {
    Command::SetCurveLengthConstraint {
        sheet: s,
        curve,
        spec: None,
    }
}

fn pos(p: &Project, s: SheetId, id: PointId) -> PointMm {
    p.sheet(s).unwrap().position(id).unwrap()
}

fn near(q: PointMm, x: f64, y: f64, tol: f64) {
    assert!(
        (q.x - x).abs() <= tol && (q.y - y).abs() <= tol,
        "position {q:?}, attendu ({x} ; {y})"
    );
}

fn free(p: &mut Project, s: SheetId, x: f64, y: f64) -> PointId {
    p.apply(Command::AddPoint {
        sheet: s,
        at: PointMm::new(x, y),
    })
    .unwrap()
    .points[0]
}

/// Oracle indépendant : longueur d'arc de la Bézier par Simpson sur la norme de la dérivée
/// (200 000 pas), sans rien appeler de `pf_geometry` ni du solveur.
fn oracle_len(p0: PointMm, c1: PointMm, c2: PointMm, p3: PointMm) -> f64 {
    let speed = |t: f64| {
        let u = 1.0 - t;
        let dx =
            3.0 * (u * u * (c1.x - p0.x) + 2.0 * u * t * (c2.x - c1.x) + t * t * (p3.x - c2.x));
        let dy =
            3.0 * (u * u * (c1.y - p0.y) + 2.0 * u * t * (c2.y - c1.y) + t * t * (p3.y - c2.y));
        (dx * dx + dy * dy).sqrt()
    };
    let n = 200_000usize;
    let h = 1.0 / n as f64;
    let mut sum = speed(0.0) + speed(1.0);
    for i in 1..n {
        sum += speed(i as f64 * h) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    sum * h / 3.0
}

fn curve_len(p: &Project, s: SheetId, curve: EntityId) -> f64 {
    let sheet = p.sheet(s).unwrap();
    let g = sheet.curve_geometry(curve).unwrap();
    oracle_len(g.p0, g.c1, g.c2, g.p3)
}

/// Courbe P0 (0 ; 0) → P3 (100 ; 0), poignées nulles : un segment parcouru à vitesse variable.
fn straight() -> (Project, SheetId, PointId, PointId, EntityId) {
    let mut p = Project::new("Longueur imposée");
    let s = p.sheets[0].id;
    let o = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(PointMm::new(0.0, 0.0)),
            c1: PointMm::new(0.0, 0.0),
            c2: PointMm::new(100.0, 0.0),
            end: PointRef::New(PointMm::new(100.0, 0.0)),
        })
        .unwrap();
    (p, s, o.points[0], o.points[1], o.entities[0])
}

/// Courbe de poignée de départ (150 ; 0) : longueur 116 mm (voir `poignee_*`).
fn handled() -> (Project, SheetId, PointId, PointId, EntityId) {
    let mut p = Project::new("Poignée");
    let s = p.sheets[0].id;
    let o = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(PointMm::new(0.0, 0.0)),
            c1: PointMm::new(150.0, 0.0),
            c2: PointMm::new(100.0, 0.0),
            end: PointRef::New(PointMm::new(100.0, 0.0)),
        })
        .unwrap();
    (p, s, o.points[0], o.points[1], o.entities[0])
}

fn ends(p: &Project, s: SheetId, curve: EntityId) -> (PointId, PointId) {
    match p.sheet(s).unwrap().entity(curve).unwrap().kind {
        EntityKind::Curve { start, end, .. } => (start, end),
        _ => unreachable!(),
    }
}

fn handle_start(p: &Project, s: SheetId, curve: EntityId) -> (f64, f64) {
    match p.sheet(s).unwrap().entity(curve).unwrap().kind {
        EntityKind::Curve { handle_start, .. } => (handle_start.dx, handle_start.dy),
        _ => unreachable!(),
    }
}

// ---------------------------------------------------------------------------------------------
// Calculs faits à la main
// ---------------------------------------------------------------------------------------------

#[test]
fn glisser_l_extremite_le_long_de_x_donne_250_puis_suit_le_parent() {
    let (mut p, s, a, b, c) = straight();
    p.apply(impose(s, c, 250.0, slide(b, 0.0))).unwrap();
    near(pos(&p, s, b), 250.0, 0.0, 1e-6);
    // A en (0 ; 30) : la courbe devient la droite de A à B, B glisse sur y = 0 : √(x² + 30²) = 250.
    p.apply(mv(s, a, 0.0, 30.0)).unwrap();
    near(pos(&p, s, b), 61600f64.sqrt(), 0.0, 1e-6);
    assert!((pos(&p, s, b).y).abs() < 1e-9);
    assert!((curve_len(&p, s, c) - 250.0).abs() < 1e-6);
}

#[test]
fn poignee_longueur_calculee_a_la_main() {
    // Poignée de départ s le long de +X, c2 = P3 = (100 ; 0) : pour s > 100 la courbe dépasse 100
    // en x(t*) avec t* = s / (3s − 200) puis revient : L = 2·x(t*) − 100.
    // s = 150 : t* = 0,6, x = 43,2 + 43,2 + 21,6 = 108, L = 116.
    // s = 300 : t* = 3/7, x = 56700 / 343, L = 113400 / 343 − 100 = 230,612244898…
    let (mut p, s, _a, _b, c) = handled();
    assert!((curve_len(&p, s, c) - 116.0).abs() < 1e-6);
    let start = ends(&p, s, c);
    let target = 113_400.0 / 343.0 - 100.0;
    p.apply(impose(
        s,
        c,
        target,
        LengthAdjust::Handle { point: start.0 },
    ))
    .unwrap();
    let (dx, dy) = handle_start(&p, s, c);
    assert!(
        (dx - 300.0).abs() < 1e-4 && dy.abs() < 1e-12,
        "({dx} ; {dy})"
    );
    assert!((curve_len(&p, s, c) - target).abs() < 1e-6);
    // La poignée garde sa direction ; réduire la cible la ramène : L = 116 → s = 150.
    p.apply(impose(s, c, 116.0, LengthAdjust::Handle { point: start.0 }))
        .unwrap();
    let (dx, dy) = handle_start(&p, s, c);
    assert!(
        (dx - 150.0).abs() < 1e-4 && dy.abs() < 1e-12,
        "({dx} ; {dy})"
    );
}

#[test]
fn poignee_cible_courante_ne_change_rien() {
    let (mut p, s, a, _b, c) = handled();
    let before = p.clone();
    p.apply(impose(s, c, 116.0, LengthAdjust::Handle { point: a }))
        .unwrap();
    // Seule la contrainte est enregistrée : la géométrie est identique à l'octet près.
    let mut expected = before;
    expected.sheets[0].length_constraints.push(CurveLength {
        curve: c,
        length_mm: 116.0,
        adjust: LengthAdjust::Handle { point: a },
        reference: None,
        expr: None,
        link: None,
    });
    assert_eq!(p, expected);
}

// ---------------------------------------------------------------------------------------------
// CA 1 : résolution à 1e-6 mm ou refus expliqué
// ---------------------------------------------------------------------------------------------

#[test]
fn ca1_cible_hors_plage_refusee_avec_les_bornes() {
    let (mut p, s, _a, b, c) = straight();
    // B glisse sur la verticale x = 100 : L = √(100² + y²) ≥ 100, jamais 50.
    let e = refused(&mut p, impose(s, c, 50.0, slide(b, 90.0)));
    assert!(e.contains("n'a pas de solution"), "{e}");
    assert!(e.contains("100,0"), "borne basse absente : {e}");
    assert!(e.contains("plage explorée"), "{e}");
    // Cible atteignable mais l'autre sens ne l'est pas : refus propre aussi pour 0 et négatif.
    for bad in [0.0, -5.0, f64::NAN, f64::INFINITY, 2.0e6] {
        refused(&mut p, impose(s, c, bad, slide(b, 0.0)));
    }
}

#[test]
fn ca1_precision_1e6_sur_des_courbes_courbes() {
    let mut p = Project::new("Précision");
    let s = p.sheets[0].id;
    let o = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(PointMm::new(0.0, 0.0)),
            c1: PointMm::new(40.0, 120.0),
            c2: PointMm::new(160.0, 120.0),
            end: PointRef::New(PointMm::new(200.0, 0.0)),
        })
        .unwrap();
    let (a, b, c) = (o.points[0], o.points[1], o.entities[0]);
    let natural = curve_len(&p, s, c);
    for (adjust, target) in [
        (LengthAdjust::Handle { point: a }, natural + 37.123456),
        (LengthAdjust::Handle { point: b }, natural + 11.5),
        (slide(b, 20.0), natural + 55.0),
        (slide(b, 160.0), natural + 3.25),
    ] {
        p.apply(impose(s, c, target, adjust)).unwrap();
        let got = curve_len(&p, s, c);
        assert!((got - target).abs() < 1e-6, "{adjust:?} : {got} ≠ {target}");
        p.apply(release(s, c)).unwrap();
    }
}

#[test]
fn deux_solutions_la_branche_la_plus_proche_de_la_position_courante_est_choisie() {
    // Poignée de départ le long de +X, poignée d'arrivée (0 ; 150) : la longueur diminue quand la
    // poignée s'allonge, puis augmente (minimum en s*). Une cible entre le minimum et la longueur
    // actuelle a deux solutions ; la contrainte prend celle du même côté du minimum que la poignée.
    let build = |s0: f64| {
        let mut p = Project::new("Deux solutions");
        let s = p.sheets[0].id;
        let o = p
            .apply(Command::AddCurve {
                sheet: s,
                start: PointRef::New(PointMm::new(0.0, 0.0)),
                c1: PointMm::new(s0, 0.0),
                c2: PointMm::new(100.0, 150.0),
                end: PointRef::New(PointMm::new(100.0, 0.0)),
            })
            .unwrap();
        (p, s, o.points[0], o.entities[0])
    };
    let f = |h: f64| {
        oracle_len(
            PointMm::new(0.0, 0.0),
            PointMm::new(h, 0.0),
            PointMm::new(100.0, 150.0),
            PointMm::new(100.0, 0.0),
        )
    };
    let (mut best, mut at) = (f64::INFINITY, 0.0);
    let mut h = 1.0;
    while h <= 600.0 {
        if f(h) < best {
            (best, at) = (f(h), h);
        }
        h += 1.0;
    }
    assert!(at > 5.0 && at < 500.0, "minimum à {at}");
    for (s0, right) in [(at + 80.0, true), (at - 4.0, false)] {
        let (mut p, s, a, c) = build(s0);
        let target = (f(s0) + best) / 2.0;
        p.apply(impose(s, c, target, LengthAdjust::Handle { point: a }))
            .unwrap();
        let (dx, _) = handle_start(&p, s, c);
        assert!((curve_len(&p, s, c) - target).abs() < 1e-6);
        assert_eq!(dx > at, right, "poignée {dx}, minimum vers {at}");
    }
    // Sous le minimum : refus avec les bornes.
    let (mut p, s, a, c) = build(at + 80.0);
    let e = refused(
        &mut p,
        impose(s, c, best - 5.0, LengthAdjust::Handle { point: a }),
    );
    assert!(e.contains("n'a pas de solution"), "{e}");
}

// ---------------------------------------------------------------------------------------------
// CA 2 : la contrainte persiste et se re-résout
// ---------------------------------------------------------------------------------------------

#[test]
fn ca2_parent_deplace_une_etape_annulable_exacte() {
    let (p, s, a, b, c) = straight();
    let mut doc = Document::new(p);
    doc.execute(impose(s, c, 250.0, slide(b, 0.0))).unwrap();
    let after_pose = bytes(doc.project());
    doc.execute(mv(s, a, 20.0, 40.0)).unwrap();
    let after_move = bytes(doc.project());
    near(
        pos(doc.project(), s, b),
        20.0 + (250.0f64 * 250.0 - 1600.0).sqrt(),
        0.0,
        1e-6,
    );
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), after_pose);
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), after_move);
    assert!(doc.undo() && doc.undo());
    assert!(!doc.can_undo());
    near(pos(doc.project(), s, b), 100.0, 0.0, 0.0);
}

#[test]
fn ca2_re_resolution_impossible_refuse_la_commande_mere() {
    let (mut p, s, a, b, c) = straight();
    p.apply(impose(s, c, 120.0, slide(b, 90.0))).unwrap();
    near(pos(&p, s, b), 100.0, 4400f64.sqrt(), 1e-6);
    // A à 400 mm de la verticale x = 100 : la plus courte courbe possible fait 400 mm.
    let e = refused(&mut p, mv(s, a, -300.0, 0.0));
    assert!(e.contains("n'a pas de solution"), "{e}");
    assert!(e.contains("400,0"), "{e}");
    // Un déplacement possible reste accepté, et la contrainte persiste.
    p.apply(mv(s, a, 60.0, 0.0)).unwrap();
    assert!((curve_len(&p, s, c) - 120.0).abs() < 1e-6);
}

#[test]
fn ca2_la_poignee_suit_un_parent_et_la_direction_est_conservee() {
    let (mut p, s, a, b, c) = handled();
    let target = 116.0;
    p.apply(impose(s, c, target, LengthAdjust::Handle { point: a }))
        .unwrap();
    // B recule à (90 ; 0) : la poignée s'ajuste pour garder 116 mm, sur la même direction.
    p.apply(mv(s, b, 90.0, 0.0)).unwrap();
    let (dx, dy) = handle_start(&p, s, c);
    assert!(dy.abs() < 1e-12 && dx > 0.0, "({dx} ; {dy})");
    assert!((curve_len(&p, s, c) - target).abs() < 1e-6);
}

#[test]
fn modification_directe_de_l_inconnue_la_contrainte_l_emporte() {
    let (mut p, s, a, b, c) = straight();
    p.apply(impose(s, c, 250.0, slide(b, 0.0))).unwrap();
    // B tiré en (300 ; 50) : il est ramené sur la droite y = 50 à la longueur imposée.
    p.apply(mv(s, b, 300.0, 50.0)).unwrap();
    let q = pos(&p, s, b);
    assert!((q.y - 50.0).abs() < 1e-9, "{q:?}");
    assert!(
        (q.x - (250.0f64 * 250.0 - 2500.0).sqrt()).abs() < 1e-6,
        "{q:?}"
    );
    // Poignée glissée : la longueur de la poignée est reprise.
    let (mut p2, s2, a2, _b2, c2) = handled();
    p2.apply(impose(s2, c2, 116.0, LengthAdjust::Handle { point: a2 }))
        .unwrap();
    p2.apply(Command::MoveCurveHandle {
        sheet: s2,
        curve: c2,
        handle: CurveHandle::Start,
        to: PointMm::new(0.0, 80.0),
    })
    .unwrap();
    assert!((curve_len(&p2, s2, c2) - 116.0).abs() < 1e-6);
    let _ = a;
}

// ---------------------------------------------------------------------------------------------
// CA 4 : contrainte désactivable
// ---------------------------------------------------------------------------------------------

#[test]
fn ca4_retirer_garde_la_geometrie_et_libere_les_parents() {
    let (p0, s, a, b, c) = straight();
    let mut doc = Document::new(p0);
    doc.execute(impose(s, c, 250.0, slide(b, 0.0))).unwrap();
    let imposed = bytes(doc.project());
    doc.execute(release(s, c)).unwrap();
    assert!(doc
        .project()
        .sheet(s)
        .unwrap()
        .length_constraints
        .is_empty());
    near(pos(doc.project(), s, b), 250.0, 0.0, 1e-6);
    // Libre : déplacer A ne bouge plus B.
    doc.execute(mv(s, a, 0.0, 30.0)).unwrap();
    near(pos(doc.project(), s, b), 250.0, 0.0, 1e-6);
    // Retirer deux fois : aucune étape.
    let steps = {
        let mut n = 0;
        let mut d = doc.clone();
        while d.undo() {
            n += 1;
        }
        n
    };
    doc.execute(release(s, c)).unwrap();
    let mut d = doc.clone();
    let mut n = 0;
    while d.undo() {
        n += 1;
    }
    assert_eq!(
        n, steps,
        "retirer une contrainte absente ne crée pas d'étape"
    );
    // Annuler le retrait rétablit la contrainte à l'octet.
    let mut doc2 = Document::new(Project::new("x"));
    let _ = &mut doc2;
    let mut d = Document::new(straight().0);
    d.execute(impose(s, c, 250.0, slide(b, 0.0))).unwrap();
    d.execute(release(s, c)).unwrap();
    assert!(d.undo());
    assert_eq!(bytes(d.project()), imposed);
}

#[test]
fn poser_deux_fois_la_meme_contrainte_n_est_pas_une_etape() {
    let (p, s, _a, b, c) = straight();
    let mut doc = Document::new(p);
    doc.execute(impose(s, c, 250.0, slide(b, 0.0))).unwrap();
    doc.execute(impose(s, c, 250.0, slide(b, 0.0))).unwrap();
    assert!(doc.undo());
    assert!(
        !doc.can_undo(),
        "la seconde pose ne doit pas être une étape"
    );
}

#[test]
fn modifier_la_cible_est_une_seule_etape() {
    let (p, s, _a, b, c) = straight();
    let mut doc = Document::new(p);
    doc.execute(impose(s, c, 250.0, slide(b, 0.0))).unwrap();
    doc.execute(impose(s, c, 300.0, slide(b, 0.0))).unwrap();
    near(pos(doc.project(), s, b), 300.0, 0.0, 1e-6);
    assert!(doc.undo());
    near(pos(doc.project(), s, b), 250.0, 0.0, 1e-6);
}

// ---------------------------------------------------------------------------------------------
// Refus de structure
// ---------------------------------------------------------------------------------------------

#[test]
fn refus_de_structure_sans_trace() {
    let (mut p, s, a, b, c) = straight();
    let other = free(&mut p, s, 40.0, 40.0);
    // Point qui n'est pas une extrémité de la courbe.
    refused(&mut p, impose(s, c, 120.0, slide(other, 0.0)));
    refused(
        &mut p,
        impose(s, c, 120.0, LengthAdjust::Handle { point: other }),
    );
    // Direction non finie.
    refused(&mut p, impose(s, c, 120.0, slide(b, f64::NAN)));
    // Poignée nulle : direction indéterminée.
    let e = refused(
        &mut p,
        impose(s, c, 120.0, LengthAdjust::Handle { point: a }),
    );
    assert!(e.contains("longueur nulle"), "{e}");
    // Tracé qui n'est pas une courbe de Bézier.
    let seg = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::Existing(other),
            to: PointRef::New(PointMm::new(80.0, 40.0)),
        })
        .unwrap()
        .entities[0];
    let e = refused(&mut p, impose(s, seg, 50.0, slide(other, 0.0)));
    assert!(e.contains("courbe de Bézier"), "{e}");
    // Cercle : autre mécanisme, refusé (la courbe lisse est admise depuis 03.3b : voir
    // `longueur_courbe_lisse.rs`).
    let circle = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(PointMm::new(300.0, 100.0)),
            radius_mm: 30.0,
        })
        .unwrap();
    let e = refused(
        &mut p,
        impose(s, circle.entities[0], 150.0, slide(circle.points[0], 0.0)),
    );
    assert!(e.contains("courbe de Bézier"), "{e}");
    // Tracé inconnu.
    refused(&mut p, impose(s, EntityId(987_654), 100.0, slide(b, 0.0)));
}

#[test]
fn extremite_construite_ou_d_arc_refusee() {
    let mut p = Project::new("Extrémités");
    let s = p.sheets[0].id;
    let a = free(&mut p, s, 0.0, 0.0);
    let z = free(&mut p, s, 0.0, 50.0);
    let d = p
        .apply(Command::AddPointAt {
            sheet: s,
            point: PointRef::Derived(PointDefinition::Offset {
                from: z,
                dx_mm: 100.0,
                dy_mm: 0.0,
            }),
        })
        .unwrap()
        .points[0];
    let c = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::Existing(a),
            c1: PointMm::new(0.0, 0.0),
            c2: PointMm::new(100.0, 50.0),
            end: PointRef::Existing(d),
        })
        .unwrap()
        .entities[0];
    let e = refused(&mut p, impose(s, c, 150.0, slide(d, 0.0)));
    assert!(e.contains("construit"), "{e}");
    // Le point libre, lui, est accepté : A glisse vers la gauche, √((100 + s)² + 50²) = 150.
    p.apply(impose(s, c, 150.0, slide(a, 180.0))).unwrap();
    near(pos(&p, s, a), 100.0 - 20000f64.sqrt(), 0.0, 1e-6);
    // Un point qui en entraîne un autre ne peut pas rendre la longueur variable : refus expliqué.
    let (mut q, sq, aq, bq, cq) = straight();
    let _ = aq;
    q.apply(Command::AddArc {
        sheet: sq,
        center: PointRef::Existing(bq),
        radius_mm: 10.0,
        start_deg: 0.0,
        end_deg: 90.0,
        ccw: true,
    })
    .unwrap();
    let e = refused(&mut q, impose(sq, cq, 150.0, slide(bq, 0.0)));
    assert!(e.contains("arc"), "{e}");
}

#[test]
fn un_point_ne_glisse_pas_pour_deux_contraintes() {
    let mut p = Project::new("Deux courbes");
    let s = p.sheets[0].id;
    let a = free(&mut p, s, 0.0, 0.0);
    let b = free(&mut p, s, 100.0, 0.0);
    let d = free(&mut p, s, 0.0, 100.0);
    let mk = |p: &mut Project, from: PointId, to: PointId, x: f64| {
        p.apply(Command::AddCurve {
            sheet: s,
            start: PointRef::Existing(from),
            c1: PointMm::new(x, 0.0),
            c2: PointMm::new(x, 0.0),
            end: PointRef::Existing(to),
        })
        .unwrap()
        .entities[0]
    };
    let c1 = mk(&mut p, a, b, 0.0);
    let c2 = mk(&mut p, d, b, 100.0);
    p.apply(impose(s, c1, 150.0, slide(b, 0.0))).unwrap();
    let e = refused(&mut p, impose(s, c2, 220.0, slide(b, 90.0)));
    assert!(e.contains("déjà déplacé"), "{e}");
}

// ---------------------------------------------------------------------------------------------
// Ordre, cycles, chaînes
// ---------------------------------------------------------------------------------------------

#[test]
fn chaine_de_longueurs_resolue_dans_l_ordre_des_dependances() {
    // C1 : A → B (B glisse sur y = 0), C2 : B → C (C glisse sur x = 100). Poignées nulles.
    let mut p = Project::new("Chaîne");
    let s = p.sheets[0].id;
    let o1 = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(PointMm::new(0.0, 0.0)),
            c1: PointMm::new(0.0, 0.0),
            c2: PointMm::new(100.0, 0.0),
            end: PointRef::New(PointMm::new(100.0, 0.0)),
        })
        .unwrap();
    let (a, b, c1) = (o1.points[0], o1.points[1], o1.entities[0]);
    let o2 = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::Existing(b),
            c1: PointMm::new(100.0, 0.0),
            c2: PointMm::new(100.0, 100.0),
            end: PointRef::New(PointMm::new(100.0, 100.0)),
        })
        .unwrap();
    let (cc, c2) = (o2.points[0], o2.entities[0]);
    p.apply(impose(s, c1, 150.0, slide(b, 0.0))).unwrap();
    p.apply(impose(s, c2, 120.0, slide(cc, 90.0))).unwrap();
    near(pos(&p, s, b), 150.0, 0.0, 1e-6);
    // C2 : de B (150 ; 0) à C (100 ; y) : √(50² + y²) = 120 → y = √11900.
    near(pos(&p, s, cc), 100.0, 11900f64.sqrt(), 1e-6);
    // Déplacer A : B glisse (C1), puis C re-glisse (C2 lit B) : les deux tiennent en une étape.
    let mut doc = Document::new(p);
    doc.execute(mv(s, a, 30.0, 40.0)).unwrap();
    let sheet = doc.project().sheet(s).unwrap();
    assert!((sheet.curve_developed_mm(c1).unwrap() - 150.0).abs() < 1e-6);
    assert!((sheet.curve_developed_mm(c2).unwrap() - 120.0).abs() < 1e-6);
    // B = (30 + √(150² − 40²) ; 0) : C2 a bougé avec lui.
    near(
        pos(doc.project(), s, b),
        30.0 + (150.0f64 * 150.0 - 1600.0).sqrt(),
        0.0,
        1e-6,
    );
    assert!(doc.undo());
    near(pos(doc.project(), s, b), 150.0, 0.0, 1e-6);
    near(pos(doc.project(), s, cc), 100.0, 11900f64.sqrt(), 1e-6);
}

#[test]
fn cycle_entre_deux_longueurs_refuse() {
    // C1 : A → B (B glisse), C2 : B → A (A glisse) : chacune écrit ce que l'autre lit.
    let mut p = Project::new("Cycle");
    let s = p.sheets[0].id;
    let o1 = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(PointMm::new(0.0, 0.0)),
            c1: PointMm::new(0.0, 0.0),
            c2: PointMm::new(100.0, 0.0),
            end: PointRef::New(PointMm::new(100.0, 0.0)),
        })
        .unwrap();
    let (a, b, c1) = (o1.points[0], o1.points[1], o1.entities[0]);
    let c2 = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::Existing(b),
            c1: PointMm::new(100.0, 60.0),
            c2: PointMm::new(0.0, 60.0),
            end: PointRef::Existing(a),
        })
        .unwrap()
        .entities[0];
    p.apply(impose(s, c1, 150.0, slide(b, 0.0))).unwrap();
    let e = refused(&mut p, impose(s, c2, 400.0, slide(a, 180.0)));
    assert!(e.contains("circulaire"), "{e}");
}

// ---------------------------------------------------------------------------------------------
// Coordination avec formules, constructions maintenues, calques verrouillés
// ---------------------------------------------------------------------------------------------

#[test]
fn une_variable_qui_deplace_un_parent_re_resout_dans_la_meme_etape() {
    let mut p = Project::new("Formule");
    let s = p.sheets[0].id;
    p.apply(Command::AddVariable {
        name: "L".into(),
        unit: VariableUnit::Mm,
        expr: "100".into(),
    })
    .unwrap();
    let a = free(&mut p, s, 0.0, 0.0);
    let b = p
        .apply(Command::AddPointAt {
            sheet: s,
            point: PointRef::Derived(PointDefinition::Offset {
                from: a,
                dx_mm: 1.0,
                dy_mm: 0.0,
            }),
        })
        .unwrap()
        .points[0];
    p.apply(Command::SetParameterExpr {
        target: ParamTarget {
            sheet: s,
            point: b,
            param: ParamKind::Dx,
        },
        expr: Some("L".into()),
    })
    .unwrap();
    let c = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::Existing(b),
            c1: PointMm::new(100.0, 0.0),
            c2: PointMm::new(300.0, 0.0),
            end: PointRef::New(PointMm::new(300.0, 0.0)),
        })
        .unwrap();
    let (e, curve) = (c.points[0], c.entities[0]);
    p.apply(impose(s, curve, 200.0, slide(e, 0.0))).unwrap();
    near(pos(&p, s, e), 300.0, 0.0, 1e-6);
    let mut doc = Document::new(p);
    doc.execute(Command::SetVariableExpr {
        name: "L".into(),
        expr: "150".into(),
    })
    .unwrap();
    near(pos(doc.project(), s, b), 150.0, 0.0, 0.0);
    near(pos(doc.project(), s, e), 350.0, 0.0, 1e-6);
    assert!(doc.undo());
    near(pos(doc.project(), s, e), 300.0, 0.0, 1e-6);
    assert!(!doc.can_undo() || doc.undo());
}

#[test]
fn construction_maintenue_et_longueur_imposee_ne_se_disputent_pas_un_point() {
    let mut p = Project::new("Maintenu");
    let s = p.sheets[0].id;
    let a = free(&mut p, s, 0.0, 0.0);
    let o = p
        .apply(Command::AddSegmentPolar {
            sheet: s,
            from: PointRef::Existing(a),
            length_mm: 100.0,
            angle_deg: 0.0,
        })
        .unwrap();
    let (b, seg) = (o.points[0], o.entities[0]);
    let c = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::Existing(a),
            c1: PointMm::new(0.0, 0.0),
            c2: PointMm::new(100.0, 0.0),
            end: PointRef::Existing(b),
        })
        .unwrap()
        .entities[0];
    // Lier le segment maintenu, puis imposer une longueur à la courbe en faisant glisser B : refusé.
    p.apply(Command::SetConstructionKept {
        sheet: s,
        segment: seg,
        kept: true,
    })
    .unwrap();
    let e = refused(&mut p, impose(s, c, 150.0, slide(b, 0.0)));
    assert!(e.contains("construction maintenue"), "{e}");
    // L'inverse : la contrainte d'abord, puis lier la construction est refusé.
    p.apply(Command::SetConstructionKept {
        sheet: s,
        segment: seg,
        kept: false,
    })
    .unwrap();
    p.apply(impose(s, c, 150.0, slide(b, 0.0))).unwrap();
    let e = refused(
        &mut p,
        Command::SetConstructionKept {
            sheet: s,
            segment: seg,
            kept: true,
        },
    );
    assert!(e.contains("longueur imposée"), "{e}");
}

#[test]
fn extremite_deplacee_parent_d_une_construction_maintenue_refusee_expliquee() {
    // A → B maintenu depuis P (libre) ; une courbe P → Q fait glisser P : le segment maintenu
    // dépendrait d'un point déplacé par la longueur imposée : refus expliqué, sans trace.
    let mut p = Project::new("Parent maintenu");
    let s = p.sheets[0].id;
    let q = free(&mut p, s, 100.0, 0.0);
    let pt = free(&mut p, s, 0.0, 0.0);
    let o = p
        .apply(Command::AddSegmentPolar {
            sheet: s,
            from: PointRef::Existing(pt),
            length_mm: 50.0,
            angle_deg: 90.0,
        })
        .unwrap();
    p.apply(Command::SetConstructionKept {
        sheet: s,
        segment: o.entities[0],
        kept: true,
    })
    .unwrap();
    let c = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::Existing(pt),
            c1: PointMm::new(0.0, 0.0),
            c2: PointMm::new(100.0, 0.0),
            end: PointRef::Existing(q),
        })
        .unwrap()
        .entities[0];
    let e = refused(&mut p, impose(s, c, 150.0, slide(pt, 180.0)));
    assert!(e.contains("construction maintenue"), "{e}");
}

#[test]
fn calque_verrouille_refuse_sans_trace() {
    let (mut p, s, a, b, c) = straight();
    p.apply(impose(s, c, 250.0, slide(b, 0.0))).unwrap();
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Fixe".into(),
        })
        .unwrap()
        .layers[0];
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![b],
        entities: vec![],
        layer: Some(layer),
    })
    .unwrap();
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: true,
    })
    .unwrap();
    // Déplacer A obligerait B (calque verrouillé) à glisser.
    let e = refused(&mut p, mv(s, a, 0.0, 30.0));
    assert!(e.to_lowercase().contains("verrouill"), "{e}");
}

// ---------------------------------------------------------------------------------------------
// Règle par opération
// ---------------------------------------------------------------------------------------------

#[test]
fn translation_et_rotation_de_toute_la_courbe_gardent_la_contrainte_sans_rien_ecraser() {
    let (mut p, s, a, b, c) = straight();
    p.apply(impose(s, c, 250.0, slide(b, 0.0))).unwrap();
    p.apply(Command::Transform {
        sheet: s,
        points: vec![a, b],
        transform: Transform::Translate {
            dx_mm: 10.0,
            dy_mm: -20.0,
        },
    })
    .unwrap();
    near(pos(&p, s, b), 260.0, -20.0, 1e-6);
    p.apply(Command::Transform {
        sheet: s,
        points: vec![a, b],
        transform: Transform::Rotate {
            center: PointMm::new(0.0, 0.0),
            angle_deg: 90.0,
        },
    })
    .unwrap();
    assert!((curve_len(&p, s, c) - 250.0).abs() < 1e-6);
    assert_eq!(p.sheet(s).unwrap().length_constraints.len(), 1);
}

#[test]
fn courbe_supprimee_retire_la_contrainte_avec_remarque_et_annule_exactement() {
    let (p0, s, _a, b, c) = straight();
    let mut doc = Document::new(p0);
    doc.execute(impose(s, c, 250.0, slide(b, 0.0))).unwrap();
    let with = bytes(doc.project());
    let o = doc
        .execute(Command::Delete {
            sheet: s,
            points: vec![],
            entities: vec![c],
        })
        .unwrap();
    assert!(
        o.notes.iter().any(|n| n.contains("longueur imposée")),
        "{:?}",
        o.notes
    );
    assert!(doc
        .project()
        .sheet(s)
        .unwrap()
        .length_constraints
        .is_empty());
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), with);
}

#[test]
fn couper_la_courbe_retire_la_contrainte_avec_remarque() {
    let (mut p, s, _a, b, c) = straight();
    p.apply(impose(s, c, 250.0, slide(b, 0.0))).unwrap();
    let o = p
        .apply(Command::SplitPath {
            sheet: s,
            entity: c,
            at: PointMm::new(120.0, 0.0),
        })
        .unwrap();
    assert!(
        o.notes.iter().any(|n| n.contains("longueur imposée")),
        "{:?}",
        o.notes
    );
    assert!(p.sheet(s).unwrap().length_constraints.is_empty());
}

#[test]
fn point_devenu_centre_d_arc_retire_la_contrainte_avec_remarque() {
    let (mut p, s, _a, b, c) = straight();
    p.apply(impose(s, c, 250.0, slide(b, 0.0))).unwrap();
    let o = p
        .apply(Command::AddArc {
            sheet: s,
            center: PointRef::Existing(b),
            radius_mm: 10.0,
            start_deg: 0.0,
            end_deg: 90.0,
            ccw: true,
        })
        .unwrap();
    assert!(
        o.notes.iter().any(|n| n.contains("longueur imposée")),
        "{:?}",
        o.notes
    );
    assert!(p.sheet(s).unwrap().length_constraints.is_empty());
}

#[test]
fn inverser_le_sens_garde_la_contrainte() {
    let (mut p, s, _a, b, c) = straight();
    p.apply(impose(s, c, 250.0, slide(b, 0.0))).unwrap();
    p.apply(Command::ReversePath {
        sheet: s,
        entity: c,
    })
    .unwrap();
    let sheet = p.sheet(s).unwrap();
    assert_eq!(sheet.length_constraints.len(), 1);
    assert!((sheet.curve_developed_mm(c).unwrap() - 250.0).abs() < 1e-6);
}

#[test]
fn copie_de_feuille_conserve_la_contrainte_aux_nouveaux_identifiants() {
    let (mut p, s, _a, b, c) = straight();
    p.apply(impose(s, c, 250.0, slide(b, 0.0))).unwrap();
    let o = p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    let copy = o.sheet.expect("feuille copiée");
    let src = p.sheet(s).unwrap();
    let dst = p.sheet(copy).unwrap();
    assert_eq!(dst.length_constraints.len(), 1);
    let cc = dst.length_constraints[0].clone();
    assert_ne!(cc.curve, c);
    assert_ne!(cc.adjust.point(), b);
    assert_eq!(cc.length_mm, 250.0);
    assert_eq!(src.length_constraints.len(), 1);
    assert!((dst.curve_developed_mm(cc.curve).unwrap() - 250.0).abs() < 1e-6);
    // La copie se re-résout seule.
    let a2 = ends(&p, copy, cc.curve).0;
    p.apply(mv(copy, a2, 0.0, 30.0)).unwrap();
    near(pos(&p, s, b), 250.0, 0.0, 1e-6);
    assert!((p.sheet(copy).unwrap().curve_developed_mm(cc.curve).unwrap() - 250.0).abs() < 1e-6);
}

#[test]
fn presse_papiers_copie_sans_contrainte_avec_remarque() {
    let (mut p, s, a, b, c) = straight();
    p.apply(impose(s, c, 250.0, slide(b, 0.0))).unwrap();
    let frag = p.copy_fragment(s, &[a, b], &[c], &[], &[]).unwrap();
    assert!(
        frag.notes.iter().any(|n| n.contains("longueur imposée")),
        "{:?}",
        frag.notes
    );
    let o = p
        .apply(Command::PasteFragment {
            sheet: s,
            fragment: Box::new(frag),
            layer: None,
        })
        .unwrap();
    assert_eq!(p.sheet(s).unwrap().length_constraints.len(), 1);
    assert!(!o.entities.is_empty());
}

#[test]
fn dupliquer_la_selection_ne_copie_pas_la_contrainte() {
    let (mut p, s, _a, b, c) = straight();
    p.apply(impose(s, c, 250.0, slide(b, 0.0))).unwrap();
    p.apply(Command::Duplicate {
        sheet: s,
        entities: vec![c],
        transform: Transform::Translate {
            dx_mm: 0.0,
            dy_mm: 50.0,
        },
    })
    .unwrap();
    assert_eq!(p.sheet(s).unwrap().length_constraints.len(), 1);
}

// ---------------------------------------------------------------------------------------------
// Persistance : format, ouverture d'un fichier altéré
// ---------------------------------------------------------------------------------------------

#[test]
fn projet_sans_contrainte_n_ecrit_aucune_cle() {
    let (p, _s, _a, _b, _c) = straight();
    assert!(!bytes(&p).contains("length_constraints"));
}

#[test]
fn aller_retour_octets_et_valeur() {
    let (mut p, s, _a, b, c) = straight();
    p.apply(impose(s, c, 250.0, slide(b, 0.0))).unwrap();
    let text = bytes(&p);
    assert!(text.contains("\"length_constraints\""), "{text}");
    assert!(text.contains("\"type\": \"slide\""), "{text}");
    let back = persist::from_json(&text).unwrap();
    assert_eq!(bytes(&back), text);
    assert_eq!(back, p);
}

fn corrupted(p: &Project, edit: impl FnOnce(&mut Value)) -> Result<Project, CoreError> {
    let mut v: Value = serde_json::from_str(&bytes(p)).unwrap();
    edit(&mut v["sheets"][0]["length_constraints"][0]);
    persist::from_json(&serde_json::to_string(&v).unwrap())
}

#[test]
fn ouverture_d_un_fichier_altere_refusee_avec_message_localise() {
    let (mut p, s, a, b, c) = straight();
    p.apply(impose(s, c, 250.0, slide(b, 0.0))).unwrap();
    assert!(corrupted(&p, |_| {}).is_ok());
    // Longueur écrite ≠ longueur imposée.
    let e = corrupted(&p, |v| v["length_mm"] = json!(251.0)).unwrap_err();
    assert!(e.to_string().contains("longueur imposée"), "{e}");
    assert!(e.to_string().contains("feuille"), "{e}");
    // Écart de 1e-5 : refusé (la tolérance est 1e-6).
    assert!(corrupted(&p, |v| v["length_mm"] = json!(250.00001)).is_err());
    // Courbe inconnue, point qui n'est pas une extrémité, cible invalide, champ inconnu.
    assert!(corrupted(&p, |v| v["curve"] = json!(999_999)).is_err());
    assert!(corrupted(&p, |v| v["adjust"]["point"] = json!(999_999)).is_err());
    assert!(corrupted(&p, |v| v["length_mm"] = json!(0.0)).is_err());
    assert!(corrupted(&p, |v| v["length_mm"] = json!(-250.0)).is_err());
    assert!(corrupted(&p, |v| v["extra"] = json!(1)).is_err());
    assert!(corrupted(&p, |v| v["adjust"]["type"] = json!("autre")).is_err());
    assert!(corrupted(&p, |v| v["adjust"]["angle_deg"] = json!("x")).is_err());
    let _ = a;
    // Deux contraintes pour la même courbe.
    let mut v: Value = serde_json::from_str(&bytes(&p)).unwrap();
    let dup = v["sheets"][0]["length_constraints"][0].clone();
    v["sheets"][0]["length_constraints"]
        .as_array_mut()
        .unwrap()
        .push(dup);
    assert!(persist::from_json(&serde_json::to_string(&v).unwrap()).is_err());
}

#[test]
fn ouverture_d_une_poignee_nulle_ou_d_un_point_construit_refusee() {
    let (mut p, s, a, _b, c) = handled();
    p.apply(impose(s, c, 116.0, LengthAdjust::Handle { point: a }))
        .unwrap();
    // Poignée écrite à zéro dans le fichier.
    let mut v: Value = serde_json::from_str(&bytes(&p)).unwrap();
    for e in v["sheets"][0]["entities"].as_array_mut().unwrap() {
        if e["kind"]["type"] == "curve" {
            e["kind"]["handle_start"] = json!({"dx": 0.0, "dy": 0.0});
        }
    }
    assert!(persist::from_json(&serde_json::to_string(&v).unwrap()).is_err());
}

#[test]
fn cote_perpendiculaire_et_orientation_ne_changent_pas_la_longueur_imposee() {
    // Une courbe coudée : la contrainte se tient aussi avec des poignées non nulles et un
    // décalage latéral du parent.
    let mut p = Project::new("Coudée");
    let s = p.sheets[0].id;
    let o = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(PointMm::new(0.0, 0.0)),
            c1: PointMm::new(30.0, 80.0),
            c2: PointMm::new(110.0, -70.0),
            end: PointRef::New(PointMm::new(150.0, 10.0)),
        })
        .unwrap();
    let (a, b, c) = (o.points[0], o.points[1], o.entities[0]);
    let target = curve_len(&p, s, c) + 40.0;
    p.apply(impose(s, c, target, slide(b, 15.0))).unwrap();
    for (x, y) in [(5.0, 5.0), (-20.0, 30.0), (10.0, -40.0)] {
        p.apply(mv(s, a, x, y)).unwrap();
        assert!((curve_len(&p, s, c) - target).abs() < 1e-6, "A ({x} ; {y})");
    }
    let _ = Side::Left;
}

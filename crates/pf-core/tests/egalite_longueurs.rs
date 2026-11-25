//! US-03.4 — égalité de longueurs (03.4a) : la longueur imposée d'une courbe de Bézier est celle d'un
//! autre tracé + un décalage signé (ex. tête de manche = emmanchure + embu). Calculs faits à la
//! main, un test par critère d'acceptation, refus sans trace (octets identiques), une seule étape
//! annulable, règle par opération, ouverture d'un fichier altéré. Voir `docs/DECISIONS.md` 321.

use pf_core::persist;
use pf_core::*;
use pf_geometry::{PointMm, Transform};
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

/// `curve` = `reference` + `offset_mm`, l'extrémité `point` glisse le long de +X.
fn equal(
    s: SheetId,
    curve: EntityId,
    reference: EntityId,
    offset_mm: f64,
    point: PointId,
) -> Command {
    equal_with(
        s,
        curve,
        reference,
        offset_mm,
        LengthAdjust::Slide {
            point,
            angle_deg: 0.0,
        },
    )
}

fn equal_with(
    s: SheetId,
    curve: EntityId,
    reference: EntityId,
    offset_mm: f64,
    adjust: LengthAdjust,
) -> Command {
    Command::SetCurveLengthConstraint {
        sheet: s,
        curve,
        spec: Some(LengthSpec {
            // Ignorée : la cible est recalculée depuis la référence.
            length_mm: 1.0,
            adjust,
            reference: Some(LengthRef {
                entity: reference,
                offset_mm,
            }),
        }),
    }
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

/// Oracle indépendant : longueur d'arc de la Bézier par Simpson (200 000 pas).
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
    let g = p.sheet(s).unwrap().curve_geometry(curve).unwrap();
    oracle_len(g.p0, g.c1, g.c2, g.p3)
}

fn add_segment(
    p: &mut Project,
    s: SheetId,
    a: (f64, f64),
    b: (f64, f64),
) -> (PointId, PointId, EntityId) {
    let o = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(PointMm::new(a.0, a.1)),
            to: PointRef::New(PointMm::new(b.0, b.1)),
        })
        .unwrap();
    (o.points[0], o.points[1], o.entities[0])
}

/// Courbe droite (poignées nulles) de (0 ; y) à (x ; y) : longueur x.
fn add_straight_curve(p: &mut Project, s: SheetId, y: f64, x: f64) -> (PointId, PointId, EntityId) {
    let o = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(PointMm::new(0.0, y)),
            c1: PointMm::new(0.0, y),
            c2: PointMm::new(x, y),
            end: PointRef::New(PointMm::new(x, y)),
        })
        .unwrap();
    (o.points[0], o.points[1], o.entities[0])
}

/// Référence R : segment (0 ; 0) → (100 ; 0) ; courbe C : (0 ; 60) → (150 ; 60), droite.
/// Renvoie `(projet, feuille, fin de R, R, fin de C, C)`.
fn basic() -> (Project, SheetId, PointId, EntityId, PointId, EntityId) {
    let mut p = Project::new("Égalité de longueurs");
    let s = p.sheets[0].id;
    let (_r0, r1, r) = add_segment(&mut p, s, (0.0, 0.0), (100.0, 0.0));
    let (_c0, c1, c) = add_straight_curve(&mut p, s, 60.0, 150.0);
    (p, s, r1, r, c1, c)
}

// ---------------------------------------------------------------------------------------------
// CA 1 : résolution à 1e-6 mm, calculs à la main
// ---------------------------------------------------------------------------------------------

#[test]
fn ca1_reference_100_plus_12_donne_112() {
    let (mut p, s, _r1, r, c1, c) = basic();
    p.apply(equal(s, c, r, 12.0, c1)).unwrap();
    // C part de (0 ; 60), l'extrémité glisse le long de X : 112 mm → (112 ; 60).
    let q = pos(&p, s, c1);
    assert!(
        (q.x - 112.0).abs() < 1e-6 && (q.y - 60.0).abs() < 1e-9,
        "{q:?}"
    );
    assert!((curve_len(&p, s, c) - 112.0).abs() < 1e-6);
    let sheet = p.sheet(s).unwrap();
    let k = sheet.length_constraint(c).unwrap();
    assert!((k.length_mm - 112.0).abs() < 1e-9, "{k:?}");
    assert_eq!(
        k.reference,
        Some(LengthRef {
            entity: r,
            offset_mm: 12.0
        })
    );
}

#[test]
fn decalage_negatif_et_nul() {
    let (mut p, s, _r1, r, c1, c) = basic();
    p.apply(equal(s, c, r, -7.5, c1)).unwrap();
    assert!((curve_len(&p, s, c) - 92.5).abs() < 1e-6);
    p.apply(equal(s, c, r, 0.0, c1)).unwrap();
    assert!((curve_len(&p, s, c) - 100.0).abs() < 1e-6);
}

#[test]
fn la_reference_peut_etre_une_bezier_un_arc_un_cercle_ou_une_courbe_lisse() {
    let (mut p, s, _r1, _r, c1, c) = basic();
    // Bézier coudée : longueur par l'oracle.
    let o = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(PointMm::new(0.0, -50.0)),
            c1: PointMm::new(30.0, 20.0),
            c2: PointMm::new(110.0, -120.0),
            end: PointRef::New(PointMm::new(150.0, -50.0)),
        })
        .unwrap();
    let bez = o.entities[0];
    p.apply(equal(s, c, bez, 12.0, c1)).unwrap();
    let want = curve_len(&p, s, bez) + 12.0;
    assert!((curve_len(&p, s, c) - want).abs() < 1e-6, "{want}");
    // Arc de rayon 50, 90° : 25 π.
    let arc = p
        .apply(Command::AddArc {
            sheet: s,
            center: PointRef::New(PointMm::new(300.0, 0.0)),
            radius_mm: 50.0,
            start_deg: 0.0,
            end_deg: 90.0,
            ccw: true,
        })
        .unwrap()
        .entities[0];
    p.apply(equal(s, c, arc, 2.0, c1)).unwrap();
    assert!((curve_len(&p, s, c) - (25.0 * std::f64::consts::PI + 2.0)).abs() < 1e-6);
    // Cercle de rayon 20 : 40 π.
    let circle = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(PointMm::new(300.0, 200.0)),
            radius_mm: 20.0,
        })
        .unwrap()
        .entities[0];
    p.apply(equal(s, c, circle, -5.0, c1)).unwrap();
    assert!((curve_len(&p, s, c) - (40.0 * std::f64::consts::PI - 5.0)).abs() < 1e-6);
    // Courbe lisse alignée (3 points sur une droite de 100 mm) : 100 mm.
    let spline = p
        .apply(Command::AddSpline {
            sheet: s,
            points: vec![
                PointRef::New(PointMm::new(0.0, 300.0)),
                PointRef::New(PointMm::new(50.0, 300.0)),
                PointRef::New(PointMm::new(100.0, 300.0)),
            ],
        })
        .unwrap()
        .entities[0];
    p.apply(equal(s, c, spline, 12.0, c1)).unwrap();
    assert!((curve_len(&p, s, c) - 112.0).abs() < 1e-6);
}

#[test]
fn la_poignee_peut_etre_l_inconnue() {
    let (mut p, s, r1, r, _c1, _c) = basic();
    let o = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(PointMm::new(0.0, 200.0)),
            c1: PointMm::new(150.0, 200.0),
            c2: PointMm::new(100.0, 200.0),
            end: PointRef::New(PointMm::new(100.0, 200.0)),
        })
        .unwrap();
    let (a, b, h) = (o.points[0], o.points[1], o.entities[0]);
    let _ = b;
    // Poignée de 300 mm : longueur 113400 / 343 − 100 (calcul à la main de `longueur_imposee.rs`).
    let offset = 113_400.0 / 343.0 - 200.0;
    p.apply(equal_with(
        s,
        h,
        r,
        offset,
        LengthAdjust::Handle { point: a },
    ))
    .unwrap();
    assert!((curve_len(&p, s, h) - (100.0 + offset)).abs() < 1e-6);
    let handle = match p.sheet(s).unwrap().entity(h).unwrap().kind {
        EntityKind::Curve { handle_start, .. } => handle_start,
        _ => unreachable!(),
    };
    assert!((handle.dx - 300.0).abs() < 1e-4 && handle.dy.abs() < 1e-12);
    // R = 130 : la poignée s'ajuste ; la longueur réelle suit la cible écrite.
    p.apply(mv(s, r1, 130.0, 0.0)).unwrap();
    let sheet = p.sheet(s).unwrap();
    let k = sheet.length_constraint(h).unwrap();
    assert!((k.length_mm - (130.0 + offset)).abs() < 1e-9);
    assert!((sheet.curve_developed_mm(h).unwrap() - k.length_mm).abs() < 1e-6);
}

// ---------------------------------------------------------------------------------------------
// CA 2 : la contrainte persiste, re-résolution dans la même étape annulable
// ---------------------------------------------------------------------------------------------

#[test]
fn ca2_deplacer_un_parent_de_la_reference_re_resout_dans_la_meme_etape() {
    let (p, s, r1, r, c1, c) = basic();
    let mut doc = Document::new(p);
    doc.execute(equal(s, c, r, 12.0, c1)).unwrap();
    let after_pose = bytes(doc.project());
    doc.execute(mv(s, r1, 130.0, 0.0)).unwrap();
    let after_move = bytes(doc.project());
    // R = 130 → C = 142.
    let q = pos(doc.project(), s, c1);
    assert!((q.x - 142.0).abs() < 1e-6, "{q:?}");
    assert!((curve_len(doc.project(), s, c) - 142.0).abs() < 1e-6);
    let k = doc
        .project()
        .sheet(s)
        .unwrap()
        .length_constraint(c)
        .unwrap()
        .clone();
    assert!((k.length_mm - 142.0).abs() < 1e-9);
    // Annuler : UNE étape suffit pour revenir exactement à la pose ; rétablir : idem.
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), after_pose);
    assert!(doc.redo());
    assert_eq!(bytes(doc.project()), after_move);
    // Un déplacement en diagonale (R = 5 · 13 = 65) : C = 77.
    doc.execute(mv(s, r1, 25.0, 60.0)).unwrap();
    let rr = (25.0f64 * 25.0 + 60.0 * 60.0).sqrt();
    assert!((curve_len(doc.project(), s, c) - (rr + 12.0)).abs() < 1e-6);
}

#[test]
fn ca2_pose_et_retrait_sont_une_seule_etape_annulable() {
    let (p, s, _r1, r, c1, c) = basic();
    let mut doc = Document::new(p);
    let before = bytes(doc.project());
    doc.execute(equal(s, c, r, 12.0, c1)).unwrap();
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
    assert!(!doc.can_undo());
}

#[test]
fn chaine_a_egale_b_plus_3_et_b_egale_c_plus_2() {
    let mut p = Project::new("Chaîne");
    let s = p.sheets[0].id;
    let (_c0, c_end, cc) = add_segment(&mut p, s, (0.0, 0.0), (100.0, 0.0));
    let (_b0, b_end, bb) = add_straight_curve(&mut p, s, 40.0, 150.0);
    let (_a0, a_end, aa) = add_straight_curve(&mut p, s, 80.0, 150.0);
    // Poser dans l'ordre inverse : A = B + 3 d'abord (B encore libre), puis B = C + 2.
    p.apply(equal(s, aa, bb, 3.0, a_end)).unwrap();
    p.apply(equal(s, bb, cc, 2.0, b_end)).unwrap();
    assert!((curve_len(&p, s, bb) - 102.0).abs() < 1e-6);
    assert!((curve_len(&p, s, aa) - 105.0).abs() < 1e-6);
    // Déplacer C : B puis A suivent dans la même étape.
    let mut doc = Document::new(p);
    let before = bytes(doc.project());
    doc.execute(mv(s, c_end, 200.0, 0.0)).unwrap();
    let q = doc.project();
    assert!((curve_len(q, s, bb) - 202.0).abs() < 1e-6);
    assert!((curve_len(q, s, aa) - 205.0).abs() < 1e-6);
    let sheet = q.sheet(s).unwrap();
    assert!((sheet.length_constraint(aa).unwrap().length_mm - 205.0).abs() < 1e-9);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
    assert!((curve_len(doc.project(), s, aa) - 105.0).abs() < 1e-6);
}

#[test]
fn reference_imposee_par_une_valeur_la_propagation_reste_correcte() {
    let (mut p, s, _r1, _r, c1, c) = basic();
    let (_b0, b_end, bb) = add_straight_curve(&mut p, s, 100.0, 90.0);
    // B = 80 (valeur), C = B + 12 = 92.
    p.apply(Command::SetCurveLengthConstraint {
        sheet: s,
        curve: bb,
        spec: Some(LengthSpec {
            length_mm: 80.0,
            adjust: LengthAdjust::Slide {
                point: b_end,
                angle_deg: 0.0,
            },
            reference: None,
        }),
    })
    .unwrap();
    p.apply(equal(s, c, bb, 12.0, c1)).unwrap();
    assert!((curve_len(&p, s, c) - 92.0).abs() < 1e-6);
    // Changer la valeur de B (une seule commande) : C suit dans la même étape.
    p.apply(Command::SetCurveLengthConstraint {
        sheet: s,
        curve: bb,
        spec: Some(LengthSpec {
            length_mm: 120.0,
            adjust: LengthAdjust::Slide {
                point: b_end,
                angle_deg: 0.0,
            },
            reference: None,
        }),
    })
    .unwrap();
    assert!((curve_len(&p, s, bb) - 120.0).abs() < 1e-6);
    assert!((curve_len(&p, s, c) - 132.0).abs() < 1e-6);
}

#[test]
fn re_resolution_impossible_refuse_la_commande_mere_avec_la_reference_et_le_decalage() {
    let (mut p, s, r1, r, c1, c) = basic();
    p.apply(equal(s, c, r, -90.0, c1)).unwrap();
    assert!((curve_len(&p, s, c) - 10.0).abs() < 1e-6);
    // R ramenée à 50 mm : cible = 50 − 90 < 0.
    let e = refused(&mut p, mv(s, r1, 50.0, 0.0));
    assert!(e.contains("-90"), "décalage absent : {e}");
    assert!(e.contains("50"), "longueur de référence absente : {e}");
    assert!(e.contains("n'a pas de solution"), "{e}");
    // Un déplacement qui reste dans la plage est accepté.
    p.apply(mv(s, r1, 95.0, 0.0)).unwrap();
    assert!((curve_len(&p, s, c) - 5.0).abs() < 1e-6);
}

// ---------------------------------------------------------------------------------------------
// Refus sans trace : octets identiques
// ---------------------------------------------------------------------------------------------

#[test]
fn refus_reference_a_soi_meme_inconnue_et_cible_non_positive() {
    let (mut p, s, _r1, r, c1, c) = basic();
    let e = refused(&mut p, equal(s, c, c, 3.0, c1));
    assert!(e.contains("soi-même"), "{e}");
    let e = refused(&mut p, equal(s, c, EntityId(999_999), 3.0, c1));
    assert!(e.contains("référence"), "{e}");
    // Cible = 100 − 100 = 0 puis 100 − 120 < 0 : refus, avec la longueur de référence.
    let e = refused(&mut p, equal(s, c, r, -100.0, c1));
    assert!(e.contains("100"), "{e}");
    let e = refused(&mut p, equal(s, c, r, -120.0, c1));
    assert!(e.contains("-120") && e.contains("100"), "{e}");
    // Décalage non fini ou démesuré.
    refused(&mut p, equal(s, c, r, f64::NAN, c1));
    refused(&mut p, equal(s, c, r, 1.0e9, c1));
    assert!(p.sheet(s).unwrap().length_constraints.is_empty());
}

#[test]
fn refus_hors_plage_de_l_inconnue_avec_les_bornes() {
    let (mut p, s, _r1, r, _c1, _c) = basic();
    // Extrémité qui glisse à la verticale : la courbe ne peut pas être plus courte que 150 mm
    // (distance de (0 ; 60) à la droite x = 150) ; cible 112 impossible.
    let (_h0, h1, h) = add_straight_curve(&mut p, s, 500.0, 150.0);
    let _ = h1;
    let c1 = match p.sheet(s).unwrap().entity(h).unwrap().kind {
        EntityKind::Curve { end, .. } => end,
        _ => unreachable!(),
    };
    let e = refused(
        &mut p,
        equal_with(
            s,
            h,
            r,
            12.0,
            LengthAdjust::Slide {
                point: c1,
                angle_deg: 90.0,
            },
        ),
    );
    assert!(e.contains("n'a pas de solution"), "{e}");
    assert!(e.contains("égale à") && e.contains("+12"), "{e}");
}

#[test]
fn cycles_refuses_a_la_pose() {
    let mut p = Project::new("Cycles");
    let s = p.sheets[0].id;
    let (_a0, a1, a) = add_straight_curve(&mut p, s, 0.0, 100.0);
    let (_b0, b1, b) = add_straight_curve(&mut p, s, 50.0, 100.0);
    p.apply(equal(s, a, b, 3.0, a1)).unwrap();
    // B = A refusé : A = B, B = A.
    let e = refused(&mut p, equal(s, b, a, 1.0, b1));
    assert!(e.contains("circulaire"), "{e}");
    // Chaîne A = B + 3, B = C + 2, puis C = A refusé.
    let (_c0, c1, cc) = add_straight_curve(&mut p, s, 90.0, 100.0);
    p.apply(equal(s, b, cc, 2.0, b1)).unwrap();
    let e = refused(&mut p, equal(s, cc, a, 0.0, c1));
    assert!(e.contains("circulaire"), "{e}");
    assert_eq!(p.sheet(s).unwrap().length_constraints.len(), 2);
}

#[test]
fn reference_qui_depend_de_ce_que_la_contrainte_modifie_refusee() {
    // R partage l'extrémité E de la courbe C : E glisse, donc R change quand C se résout.
    let mut p = Project::new("Dépendance");
    let s = p.sheets[0].id;
    let (_c0, e_pt, c) = add_straight_curve(&mut p, s, 0.0, 100.0);
    let r = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::Existing(e_pt),
            to: PointRef::New(PointMm::new(100.0, 80.0)),
        })
        .unwrap()
        .entities[0];
    let e = refused(&mut p, equal(s, c, r, 5.0, e_pt));
    assert!(e.contains("circulaire"), "{e}");
}

// ---------------------------------------------------------------------------------------------
// CA 3 : vocabulaire — l'égalité ne crée que des longueurs imposées sur des courbes de Bézier
// ---------------------------------------------------------------------------------------------

#[test]
fn ca3_seule_une_courbe_de_bezier_peut_etre_contrainte() {
    let (mut p, s, _r1, r, _c1, _c) = basic();
    let (_a, b, seg) = add_segment(&mut p, s, (0.0, 200.0), (50.0, 200.0));
    let e = refused(&mut p, equal(s, seg, r, 5.0, b));
    assert!(e.contains("courbe de Bézier"), "{e}");
}

// ---------------------------------------------------------------------------------------------
// CA 4 : désactivation
// ---------------------------------------------------------------------------------------------

#[test]
fn ca4_desactiver_garde_la_geometrie_et_delie_la_courbe() {
    let (mut p, s, r1, r, c1, c) = basic();
    p.apply(equal(s, c, r, 12.0, c1)).unwrap();
    let kept = pos(&p, s, c1);
    p.apply(release(s, c)).unwrap();
    assert!(p.sheet(s).unwrap().length_constraints.is_empty());
    assert_eq!(pos(&p, s, c1), kept);
    // La référence peut bouger sans entraîner la courbe.
    p.apply(mv(s, r1, 300.0, 0.0)).unwrap();
    assert_eq!(pos(&p, s, c1), kept);
    assert!(!bytes(&p).contains("\"reference\""));
}

#[test]
fn poser_deux_fois_la_meme_egalite_n_est_pas_une_etape() {
    let (p, s, _r1, r, c1, c) = basic();
    let mut doc = Document::new(p);
    doc.execute(equal(s, c, r, 12.0, c1)).unwrap();
    let before = bytes(doc.project());
    doc.execute(equal(s, c, r, 12.0, c1)).unwrap();
    assert_eq!(bytes(doc.project()), before);
    // Sans effet : aucune étape (le premier undo défait la pose, pas la répétition).
    // Changer le décalage : une étape.
    doc.execute(equal(s, c, r, 20.0, c1)).unwrap();
    assert!((curve_len(doc.project(), s, c) - 120.0).abs() < 1e-6);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
    // La répétition n'a pas laissé d'étape : un seul undo de plus retire la pose.
    assert!(doc.undo());
    assert!(!doc.can_undo());
    assert!(doc
        .project()
        .sheet(s)
        .unwrap()
        .length_constraints
        .is_empty());
    assert!(doc.redo() && doc.redo());
    // Passer d'une égalité à une valeur : une étape, plus de référence.
    doc.execute(Command::SetCurveLengthConstraint {
        sheet: s,
        curve: c,
        spec: Some(LengthSpec {
            length_mm: 90.0,
            adjust: LengthAdjust::Slide {
                point: c1,
                angle_deg: 0.0,
            },
            reference: None,
        }),
    })
    .unwrap();
    let k = doc
        .project()
        .sheet(s)
        .unwrap()
        .length_constraint(c)
        .unwrap();
    assert_eq!(k.reference, None);
    assert!((curve_len(doc.project(), s, c) - 90.0).abs() < 1e-6);
}

// ---------------------------------------------------------------------------------------------
// Règle par opération
// ---------------------------------------------------------------------------------------------

#[test]
fn reference_supprimee_retire_la_contrainte_avec_remarque_et_annule_exactement() {
    let (p0, s, _r1, r, c1, c) = basic();
    let mut doc = Document::new(p0);
    doc.execute(equal(s, c, r, 12.0, c1)).unwrap();
    let with = bytes(doc.project());
    let o = doc
        .execute(Command::Delete {
            sheet: s,
            points: vec![],
            entities: vec![r],
        })
        .unwrap();
    assert!(
        o.notes
            .iter()
            .any(|n| n.contains("longueur imposée") && n.contains("référence")),
        "{:?}",
        o.notes
    );
    assert!(doc
        .project()
        .sheet(s)
        .unwrap()
        .length_constraints
        .is_empty());
    // La géométrie de C est conservée.
    assert!((curve_len(doc.project(), s, c) - 112.0).abs() < 1e-6);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), with);
}

#[test]
fn reference_coupee_retire_la_contrainte_avec_remarque() {
    let (mut p, s, _r1, r, c1, c) = basic();
    p.apply(equal(s, c, r, 12.0, c1)).unwrap();
    let o = p
        .apply(Command::SplitPath {
            sheet: s,
            entity: r,
            at: PointMm::new(40.0, 0.0),
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
fn reference_inversee_garde_la_contrainte() {
    let (mut p, s, _r1, r, c1, c) = basic();
    p.apply(equal(s, c, r, 12.0, c1)).unwrap();
    p.apply(Command::ReversePath {
        sheet: s,
        entity: r,
    })
    .unwrap();
    assert_eq!(p.sheet(s).unwrap().length_constraints.len(), 1);
    assert!((curve_len(&p, s, c) - 112.0).abs() < 1e-6);
}

#[test]
fn courbe_contrainte_supprimee_retire_la_contrainte_et_garde_la_reference() {
    let (mut p, s, _r1, r, c1, c) = basic();
    p.apply(equal(s, c, r, 12.0, c1)).unwrap();
    let o = p
        .apply(Command::Delete {
            sheet: s,
            points: vec![],
            entities: vec![c],
        })
        .unwrap();
    assert!(o.notes.iter().any(|n| n.contains("longueur imposée")));
    assert!(p.sheet(s).unwrap().length_constraints.is_empty());
    assert!(p.sheet(s).unwrap().entity(r).is_ok());
}

#[test]
fn copie_de_feuille_remappe_la_reference() {
    let (mut p, s, _r1, r, c1, c) = basic();
    p.apply(equal(s, c, r, 12.0, c1)).unwrap();
    let copy = p
        .apply(Command::DuplicateSheet { sheet: s })
        .unwrap()
        .sheet
        .expect("feuille copiée");
    let dst = p.sheet(copy).unwrap();
    let k = dst.length_constraints[0].clone();
    let rf = k.reference.expect("référence reprise");
    assert_ne!(rf.entity, r);
    assert_ne!(k.curve, c);
    assert!(dst.entity(rf.entity).is_ok());
    assert_eq!(rf.offset_mm, 12.0);
    // La copie se re-résout seule : déplacer l'extrémité de sa référence.
    let r_end = dst.entity_points(rf.entity).unwrap().1;
    p.apply(mv(copy, r_end, 140.0, 0.0)).unwrap();
    let dst = p.sheet(copy).unwrap();
    assert!((dst.curve_developed_mm(k.curve).unwrap() - 152.0).abs() < 1e-6);
    // L'original n'a pas bougé.
    assert!((curve_len(&p, s, c) - 112.0).abs() < 1e-6);
}

#[test]
fn presse_papiers_copie_sans_contrainte_avec_remarque() {
    let (mut p, s, r1, r, c1, c) = basic();
    p.apply(equal(s, c, r, 12.0, c1)).unwrap();
    let c0 = match p.sheet(s).unwrap().entity(c).unwrap().kind {
        EntityKind::Curve { start, .. } => start,
        _ => unreachable!(),
    };
    // Copie de C seule (référence non copiée), puis des deux.
    for (points, entities) in [(vec![c0, c1], vec![c]), (vec![c0, c1, r1], vec![c, r])] {
        let frag = p.copy_fragment(s, &points, &entities, &[], &[]).unwrap();
        assert!(
            frag.notes.iter().any(|n| n.contains("longueur imposée")),
            "{:?}",
            frag.notes
        );
        p.apply(Command::PasteFragment {
            sheet: s,
            fragment: Box::new(frag),
            layer: None,
        })
        .unwrap();
        assert_eq!(p.sheet(s).unwrap().length_constraints.len(), 1);
    }
}

#[test]
fn dupliquer_la_selection_ne_copie_pas_la_contrainte() {
    let (mut p, s, _r1, r, c1, c) = basic();
    p.apply(equal(s, c, r, 12.0, c1)).unwrap();
    p.apply(Command::Duplicate {
        sheet: s,
        entities: vec![c, r],
        transform: Transform::Translate {
            dx_mm: 0.0,
            dy_mm: 50.0,
        },
    })
    .unwrap();
    assert_eq!(p.sheet(s).unwrap().length_constraints.len(), 1);
}

#[test]
fn transformer_une_extremite_de_la_reference_re_resout_la_courbe() {
    let (mut p, s, r1, r, c1, c) = basic();
    p.apply(equal(s, c, r, 12.0, c1)).unwrap();
    p.apply(Command::Transform {
        sheet: s,
        points: vec![r1],
        transform: Transform::Translate {
            dx_mm: 30.0,
            dy_mm: 0.0,
        },
    })
    .unwrap();
    assert!((curve_len(&p, s, c) - 142.0).abs() < 1e-6);
}

// ---------------------------------------------------------------------------------------------
// Persistance : format, ouverture d'un fichier altéré
// ---------------------------------------------------------------------------------------------

#[test]
fn aller_retour_octets_et_cle_reference() {
    let (mut p, s, _r1, r, c1, c) = basic();
    p.apply(equal(s, c, r, 12.0, c1)).unwrap();
    let text = bytes(&p);
    assert!(text.contains("\"reference\""), "{text}");
    assert!(text.contains("\"offset_mm\": 12.0"), "{text}");
    let back = persist::from_json(&text).unwrap();
    assert_eq!(bytes(&back), text);
    assert_eq!(back, p);
}

#[test]
fn sans_egalite_aucune_cle_reference_et_projets_existants_identiques() {
    let (mut p, s, _r1, _r, c1, c) = basic();
    p.apply(Command::SetCurveLengthConstraint {
        sheet: s,
        curve: c,
        spec: Some(LengthSpec {
            length_mm: 200.0,
            adjust: LengthAdjust::Slide {
                point: c1,
                angle_deg: 0.0,
            },
            reference: None,
        }),
    })
    .unwrap();
    assert!(!bytes(&p).contains("reference"));
}

fn corrupted(p: &Project, edit: impl FnOnce(&mut Value)) -> Result<Project, CoreError> {
    let mut v: Value = serde_json::from_str(&bytes(p)).unwrap();
    edit(&mut v["sheets"][0]["length_constraints"][0]);
    persist::from_json(&serde_json::to_string(&v).unwrap())
}

#[test]
fn ouverture_d_un_fichier_altere_refusee_avec_message_localise() {
    let (mut p, s, _r1, r, c1, c) = basic();
    p.apply(equal(s, c, r, 12.0, c1)).unwrap();
    assert!(corrupted(&p, |_| {}).is_ok());
    // Cible écrite ≠ référence + décalage (même si la courbe a bien cette longueur).
    let e = corrupted(&p, |v| v["reference"]["offset_mm"] = json!(13.0)).unwrap_err();
    assert!(e.to_string().contains("longueur imposée"), "{e}");
    assert!(corrupted(&p, |v| v["reference"]["offset_mm"] = json!(12.00001)).is_err());
    // Référence inconnue, à soi-même, décalage invalide, champ inconnu, longueur écrite fausse.
    let e = corrupted(&p, |v| v["reference"]["entity"] = json!(999_999)).unwrap_err();
    assert!(e.to_string().contains("référence"), "{e}");
    let own = c.0;
    assert!(corrupted(&p, |v| v["reference"]["entity"] = json!(own)).is_err());
    assert!(corrupted(&p, |v| v["reference"]["offset_mm"] = json!("x")).is_err());
    assert!(corrupted(&p, |v| v["reference"]["extra"] = json!(1)).is_err());
    assert!(corrupted(&p, |v| v["length_mm"] = json!(113.0)).is_err());
    assert!(corrupted(&p, |v| v["reference"] = json!({"entity": r.0})).is_err());
}

#[test]
fn ouverture_d_un_cycle_refusee() {
    let mut p = Project::new("Cycle");
    let s = p.sheets[0].id;
    let (_a0, a1, a) = add_straight_curve(&mut p, s, 0.0, 100.0);
    let (_b0, b1, b) = add_straight_curve(&mut p, s, 50.0, 100.0);
    p.apply(equal(s, a, b, 0.0, a1)).unwrap();
    p.apply(equal(s, b, a, 0.0, b1)).unwrap_err();
    // Fichier fabriqué : A = B et B = A (les deux longueurs valent 100).
    let mut v: Value = serde_json::from_str(&bytes(&p)).unwrap();
    let mut second = v["sheets"][0]["length_constraints"][0].clone();
    second["curve"] = json!(b.0);
    second["reference"]["entity"] = json!(a.0);
    second["adjust"]["point"] = json!(b1.0);
    v["sheets"][0]["length_constraints"]
        .as_array_mut()
        .unwrap()
        .push(second);
    let e = persist::from_json(&serde_json::to_string(&v).unwrap()).unwrap_err();
    assert!(e.to_string().contains("circulaire"), "{e}");
}

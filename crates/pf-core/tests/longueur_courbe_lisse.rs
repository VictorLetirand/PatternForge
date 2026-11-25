//! US-03.3b — longueur imposée à une **courbe lisse** (points de passage) et cible **maintenue par
//! une formule** (Bézier et courbe lisse) : valeurs calculées à la main, refus sans trace (octets
//! identiques), une seule étape annulable, copies, ouverture d'un fichier altéré.
//!
//! Courbe lisse : seule l'extrémité qui glisse peut être l'inconnue (pas de poignée) ; la racine
//! retenue est la plus proche de la position courante. Cible par formule : `CurveLength.expr`,
//! valeur évaluée laissée écrite, ré-évaluée et re-résolue dans la même étape qu'une variable.

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;
use serde_json::{json, Value};

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

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

fn expr(s: SheetId, curve: EntityId, text: &str) -> Command {
    Command::SetCurveLengthExpr {
        sheet: s,
        curve,
        expr: Some(text.into()),
    }
}

fn unlink(s: SheetId, curve: EntityId) -> Command {
    Command::SetCurveLengthExpr {
        sheet: s,
        curve,
        expr: None,
    }
}

fn var(p: &mut Project, name: &str, text: &str) {
    p.apply(Command::AddVariable {
        name: name.into(),
        unit: VariableUnit::Mm,
        expr: text.into(),
    })
    .unwrap();
}

fn set_var(name: &str, text: &str) -> Command {
    Command::SetVariableExpr {
        name: name.into(),
        expr: text.into(),
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

/// Oracle indépendant : longueur d'une courbe lisse par points de passage, tangentes de Catmull-Rom
/// `(suivant − précédent) / 2` aux points intérieurs, corde aux extrémités (réécrites ici), tronçon
/// d'Hermite converti en Bézier `c1 = P + m/3`, Simpson sur la norme de la dérivée (200 000 pas).
fn oracle_spline_len(pts: &[(f64, f64)]) -> f64 {
    let n = pts.len();
    let tangent = |i: usize| -> (f64, f64) {
        if i == 0 {
            (pts[1].0 - pts[0].0, pts[1].1 - pts[0].1)
        } else if i == n - 1 {
            (pts[n - 1].0 - pts[n - 2].0, pts[n - 1].1 - pts[n - 2].1)
        } else {
            (
                (pts[i + 1].0 - pts[i - 1].0) / 2.0,
                (pts[i + 1].1 - pts[i - 1].1) / 2.0,
            )
        }
    };
    let mut total = 0.0;
    for i in 0..n - 1 {
        let (m0, m1) = (tangent(i), tangent(i + 1));
        let p0 = pts[i];
        let p3 = pts[i + 1];
        let c1 = (p0.0 + m0.0 / 3.0, p0.1 + m0.1 / 3.0);
        let c2 = (p3.0 - m1.0 / 3.0, p3.1 - m1.1 / 3.0);
        let speed = |t: f64| {
            let u = 1.0 - t;
            let dx =
                3.0 * (u * u * (c1.0 - p0.0) + 2.0 * u * t * (c2.0 - c1.0) + t * t * (p3.0 - c2.0));
            let dy =
                3.0 * (u * u * (c1.1 - p0.1) + 2.0 * u * t * (c2.1 - c1.1) + t * t * (p3.1 - c2.1));
            dx.hypot(dy)
        };
        let steps = 200_000usize;
        let h = 1.0 / steps as f64;
        let mut sum = speed(0.0) + speed(1.0);
        for k in 1..steps {
            sum += speed(k as f64 * h) * if k % 2 == 1 { 4.0 } else { 2.0 };
        }
        total += sum * h / 3.0;
    }
    total
}

struct Smooth {
    p: Project,
    s: SheetId,
    spline: EntityId,
    pts: Vec<PointId>,
}

/// Courbe lisse à travers `coords` (points libres).
fn smooth(coords: &[(f64, f64)]) -> Smooth {
    let mut p = Project::new("Courbe lisse");
    let s = p.sheets[0].id;
    let o = p
        .apply(Command::AddSpline {
            sheet: s,
            points: coords
                .iter()
                .map(|&(x, y)| PointRef::New(PointMm::new(x, y)))
                .collect(),
        })
        .unwrap();
    Smooth {
        p,
        s,
        spline: o.entities[0],
        pts: o.points,
    }
}

fn spline_len(sm: &Smooth) -> f64 {
    let sheet = sm.p.sheet(sm.s).unwrap();
    let pts: Vec<(f64, f64)> = sm
        .pts
        .iter()
        .map(|&id| {
            let q = sheet.position(id).unwrap();
            (q.x, q.y)
        })
        .collect();
    oracle_spline_len(&pts)
}

// ---------------------------------------------------------------------------------------------
// Courbe lisse : calculs faits à la main
// ---------------------------------------------------------------------------------------------

#[test]
fn deux_noeuds_la_longueur_est_la_distance_et_l_extremite_glisse_exactement() {
    // (0 ; 0) → (100 ; 0), tangentes égales à la corde : segment parcouru à vitesse constante,
    // longueur = distance. Cible 150 le long de +X : extrémité en (150 ; 0). Cible 80 : (80 ; 0).
    let mut sm = smooth(&[(0.0, 0.0), (100.0, 0.0)]);
    let end = sm.pts[1];
    sm.p.apply(impose(sm.s, sm.spline, 150.0, slide(end, 0.0)))
        .unwrap();
    near(pos(&sm.p, sm.s, end), 150.0, 0.0, 1e-6);
    assert!((spline_len(&sm) - 150.0).abs() < 1e-6);
    sm.p.apply(impose(sm.s, sm.spline, 80.0, slide(end, 0.0)))
        .unwrap();
    near(pos(&sm.p, sm.s, end), 80.0, 0.0, 1e-6);
}

#[test]
fn direction_verticale_la_solution_la_plus_proche_est_retenue() {
    // Extrémité en (100 ; 10), glissement le long de Y : longueur √(100² + y²). Cible 130 :
    // y = ±√6900 = ±83,0662386…, la plus proche de y = 10 est +83,0662386 (distance 73 contre 93).
    let mut sm = smooth(&[(0.0, 0.0), (100.0, 10.0)]);
    // La longueur courante est √10100 = 100,498… : la cible est atteignable des deux côtés.
    let end = sm.pts[1];
    // Cible exactement la valeur du segment droit (0 ; 0) → (100 ; y) : √(100² + y²) = 130.
    sm.p.apply(impose(sm.s, sm.spline, 130.0, slide(end, 90.0)))
        .unwrap();
    near(pos(&sm.p, sm.s, end), 100.0, 6900f64.sqrt(), 1e-6);
}

#[test]
fn trois_points_alignes_la_longueur_est_l_abscisse_de_l_extremite() {
    // (0 ; 0), (50 ; 0), (x ; 0) : les dérivées x'(t) des deux tronçons restent > 0 (calcul à la
    // main : 150t² − 100t + 50 puis −150t² + 200t + 100 pour x = 200 ; discriminant négatif pour le
    // premier), la longueur est donc x. Cibles 200 puis 120.
    let mut sm = smooth(&[(0.0, 0.0), (50.0, 0.0), (100.0, 0.0)]);
    let end = sm.pts[2];
    sm.p.apply(impose(sm.s, sm.spline, 200.0, slide(end, 0.0)))
        .unwrap();
    near(pos(&sm.p, sm.s, end), 200.0, 0.0, 1e-6);
    sm.p.apply(impose(sm.s, sm.spline, 120.0, slide(end, 0.0)))
        .unwrap();
    near(pos(&sm.p, sm.s, end), 120.0, 0.0, 1e-6);
    assert!((spline_len(&sm) - 120.0).abs() < 1e-6);
}

#[test]
fn courbe_non_triviale_precision_1e6_contre_l_oracle() {
    let mut sm = smooth(&[(0.0, 0.0), (50.0, 40.0), (100.0, 0.0), (160.0, 30.0)]);
    let start_len = spline_len(&sm);
    let target = start_len * 1.25;
    let end = sm.pts[3];
    sm.p.apply(impose(sm.s, sm.spline, target, slide(end, 20.0)))
        .unwrap();
    // L'extrémité est restée sur la droite de direction 20° passant par (160 ; 30).
    let q = pos(&sm.p, sm.s, end);
    let cross = (q.x - 160.0) * 20f64.to_radians().sin() - (q.y - 30.0) * 20f64.to_radians().cos();
    assert!(cross.abs() < 1e-9, "écart à la droite : {cross}");
    assert!(
        (spline_len(&sm) - target).abs() < 1e-6,
        "{}",
        spline_len(&sm)
    );
}

#[test]
fn cible_hors_plage_refusee_avec_les_bornes_sans_trace() {
    // Le point intérieur (50 ; 40) reste fixe : toute longueur est au moins 2 × 40 = 80 mm.
    let mut sm = smooth(&[(0.0, 0.0), (50.0, 40.0), (100.0, 0.0)]);
    let end = sm.pts[2];
    let e = refused(&mut sm.p, impose(sm.s, sm.spline, 10.0, slide(end, 0.0)));
    assert!(e.contains("n'a pas de solution"), "{e}");
    assert!(e.contains("la longueur développée va de"), "{e}");
}

// ---------------------------------------------------------------------------------------------
// Refus de structure
// ---------------------------------------------------------------------------------------------

#[test]
fn poignee_point_interieur_et_point_construit_refuses_sans_trace() {
    let mut sm = smooth(&[(0.0, 0.0), (50.0, 40.0), (100.0, 0.0)]);
    let (a, mid, b) = (sm.pts[0], sm.pts[1], sm.pts[2]);
    let e = refused(
        &mut sm.p,
        impose(sm.s, sm.spline, 150.0, LengthAdjust::Handle { point: b }),
    );
    assert!(e.contains("pas de poignée"), "{e}");
    let e = refused(&mut sm.p, impose(sm.s, sm.spline, 150.0, slide(mid, 0.0)));
    assert!(e.contains("pas une extrémité"), "{e}");
    // Extrémité construite : seule une extrémité libre peut glisser.
    let off =
        sm.p.apply(Command::AddPointAt {
            sheet: sm.s,
            point: PointRef::Derived(PointDefinition::Offset {
                from: a,
                dx_mm: 30.0,
                dy_mm: -30.0,
            }),
        })
        .unwrap()
        .points[0];
    let sp2 =
        sm.p.apply(Command::AddSpline {
            sheet: sm.s,
            points: vec![
                PointRef::Existing(off),
                PointRef::New(PointMm::new(200.0, 0.0)),
            ],
        })
        .unwrap();
    let e = refused(
        &mut sm.p,
        impose(sm.s, sp2.entities[0], 100.0, slide(off, 0.0)),
    );
    assert!(e.contains("libre"), "{e}");
}

#[test]
fn un_segment_est_toujours_refuse_avec_le_nouveau_message() {
    let mut sm = smooth(&[(0.0, 0.0), (100.0, 0.0)]);
    let seg =
        sm.p.apply(Command::AddSegment {
            sheet: sm.s,
            from: PointRef::New(PointMm::new(0.0, 50.0)),
            to: PointRef::New(PointMm::new(80.0, 50.0)),
        })
        .unwrap();
    let e = refused(
        &mut sm.p,
        impose(sm.s, seg.entities[0], 50.0, slide(seg.points[1], 0.0)),
    );
    assert!(e.contains("courbe de Bézier ou une courbe lisse"), "{e}");
}

// ---------------------------------------------------------------------------------------------
// Re-résolution quand les parents bougent, suppression, retrait
// ---------------------------------------------------------------------------------------------

#[test]
fn parent_deplace_re_resout_dans_une_seule_etape_annulable() {
    // Droite (0 ; 0) → (x ; 0), longueur 150. A monte en (0 ; 30) : √(x² + 30²) = 150 →
    // x = √21600 = 60√6 = 146,9693846…
    let sm = smooth(&[(0.0, 0.0), (100.0, 0.0)]);
    let (s, spline, a, b) = (sm.s, sm.spline, sm.pts[0], sm.pts[1]);
    let mut doc = Document::new(sm.p);
    doc.execute(impose(s, spline, 150.0, slide(b, 0.0)))
        .unwrap();
    let before = bytes(doc.project());
    doc.execute(Command::MovePoint {
        sheet: s,
        point: a,
        to: PointMm::new(0.0, 30.0),
    })
    .unwrap();
    near(pos(doc.project(), s, b), 21600f64.sqrt(), 0.0, 1e-6);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before, "une seule annulation suffit");
}

#[test]
fn courbe_supprimee_retire_la_contrainte_avec_remarque_et_la_formule() {
    let mut sm = smooth(&[(0.0, 0.0), (100.0, 0.0)]);
    var(&mut sm.p, "w", "100");
    sm.p.apply(impose(sm.s, sm.spline, 150.0, slide(sm.pts[1], 0.0)))
        .unwrap();
    sm.p.apply(expr(sm.s, sm.spline, "w*3/2")).unwrap();
    let before = sm.p.clone();
    let o =
        sm.p.apply(Command::Delete {
            sheet: sm.s,
            points: vec![],
            entities: vec![sm.spline],
        })
        .unwrap();
    assert!(
        o.notes.iter().any(|n| n.contains("longueur imposée")),
        "{:?}",
        o.notes
    );
    assert!(sm.p.sheet(sm.s).unwrap().length_constraints.is_empty());
    assert!(!bytes(&sm.p).contains("\"expr\":\"w*3/2\""));
    assert_ne!(sm.p, before);
}

#[test]
fn retirer_garde_la_geometrie_et_delie_la_formule() {
    let mut sm = smooth(&[(0.0, 0.0), (100.0, 0.0)]);
    var(&mut sm.p, "w", "100");
    sm.p.apply(impose(sm.s, sm.spline, 150.0, slide(sm.pts[1], 0.0)))
        .unwrap();
    sm.p.apply(expr(sm.s, sm.spline, "w*3/2")).unwrap();
    sm.p.apply(Command::SetCurveLengthConstraint {
        sheet: sm.s,
        curve: sm.spline,
        spec: None,
    })
    .unwrap();
    near(pos(&sm.p, sm.s, sm.pts[1]), 150.0, 0.0, 1e-6);
    assert!(sm.p.sheet(sm.s).unwrap().length_constraints.is_empty());
    // Plus rien ne suit la variable.
    sm.p.apply(set_var("w", "200")).unwrap();
    near(pos(&sm.p, sm.s, sm.pts[1]), 150.0, 0.0, 1e-6);
}

// ---------------------------------------------------------------------------------------------
// Cible par formule
// ---------------------------------------------------------------------------------------------

/// Courbe lisse (0 ; 0) → (100 ; 0), variable w = 100, longueur imposée par « w*3/2 » = 150.
fn formula_world() -> (Smooth, Document) {
    let mut sm = smooth(&[(0.0, 0.0), (100.0, 0.0)]);
    var(&mut sm.p, "w", "100");
    sm.p.apply(impose(sm.s, sm.spline, 100.0, slide(sm.pts[1], 0.0)))
        .unwrap();
    sm.p.apply(expr(sm.s, sm.spline, "w*3/2")).unwrap();
    let doc = Document::new(sm.p.clone());
    (sm, doc)
}

#[test]
fn formule_ecrit_la_valeur_et_resout_dans_une_etape() {
    let sm = smooth(&[(0.0, 0.0), (100.0, 0.0)]);
    let (s, spline, b) = (sm.s, sm.spline, sm.pts[1]);
    let mut p = sm.p;
    var(&mut p, "w", "100");
    p.apply(impose(s, spline, 100.0, slide(b, 0.0))).unwrap();
    let mut doc = Document::new(p);
    let before = bytes(doc.project());
    doc.execute(expr(s, spline, "w*3/2")).unwrap();
    near(pos(doc.project(), s, b), 150.0, 0.0, 1e-6);
    let c = doc.project().sheet(s).unwrap().length_constraints[0].clone();
    assert_eq!(c.expr.as_deref(), Some("w*3/2"));
    assert!((c.length_mm - 150.0).abs() < 1e-12);
    assert!(
        bytes(doc.project()).contains("\"expr\": \"w*3/2\"")
            || bytes(doc.project()).contains("\"expr\":\"w*3/2\"")
    );
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
    assert!(!doc.can_undo() || doc.undo());
}

#[test]
fn changer_la_variable_re_resout_dans_la_meme_etape() {
    let (sm, _) = formula_world();
    let (s, b) = (sm.s, sm.pts[1]);
    let mut doc = Document::new(sm.p);
    let before = bytes(doc.project());
    doc.execute(set_var("w", "120")).unwrap();
    // 120 × 3 / 2 = 180.
    near(pos(doc.project(), s, b), 180.0, 0.0, 1e-6);
    assert!(
        (doc.project().sheet(s).unwrap().length_constraints[0].length_mm - 180.0).abs() < 1e-12
    );
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
}

#[test]
fn variable_qui_rendrait_la_cible_invalide_refusee_sans_trace() {
    let (mut sm, _) = formula_world();
    // w = 0 : la formule donne 0 mm (strictement positif exigé).
    let e = refused(&mut sm.p, set_var("w", "0"));
    assert!(e.contains("strictement positif"), "{e}");
    // w = 10 000 000 : 15 000 000 mm > 1 000 000.
    let e = refused(&mut sm.p, set_var("w", "10000000"));
    assert!(e.contains("au plus"), "{e}");
    // Variable supprimée alors que la formule la cite : refus nommant la longueur imposée.
    let e = refused(&mut sm.p, Command::RemoveVariable { name: "w".into() });
    assert!(
        e.contains("est utilisée par") && e.contains("longueur imposée de"),
        "{e}"
    );
}

#[test]
fn cible_hors_plage_par_variable_refuse_la_commande_de_la_variable() {
    // Point intérieur à 40 mm : longueur >= 80. Formule « v » avec v = 120 puis v = 10.
    let mut sm = smooth(&[(0.0, 0.0), (50.0, 40.0), (100.0, 0.0)]);
    var(&mut sm.p, "v", "120");
    sm.p.apply(impose(sm.s, sm.spline, 120.0, slide(sm.pts[2], 0.0)))
        .unwrap();
    sm.p.apply(expr(sm.s, sm.spline, "v")).unwrap();
    let e = refused(&mut sm.p, set_var("v", "10"));
    assert!(e.contains("n'a pas de solution"), "{e}");
}

#[test]
fn renommer_la_variable_reecrit_la_formule() {
    let (mut sm, _) = formula_world();
    sm.p.apply(Command::RenameVariable {
        from: "w".into(),
        to: "largeur".into(),
    })
    .unwrap();
    let c = &sm.p.sheet(sm.s).unwrap().length_constraints[0];
    assert_eq!(c.expr.as_deref(), Some("largeur*3/2"));
    // Elle suit toujours la variable.
    sm.p.apply(set_var("largeur", "60")).unwrap();
    near(pos(&sm.p, sm.s, sm.pts[1]), 90.0, 0.0, 1e-6);
}

#[test]
fn delier_garde_la_valeur_et_sans_formule_n_est_pas_une_etape() {
    let (sm, _) = formula_world();
    let (s, spline, b) = (sm.s, sm.spline, sm.pts[1]);
    let mut doc = Document::new(sm.p);
    doc.execute(unlink(s, spline)).unwrap();
    let c = doc.project().sheet(s).unwrap().length_constraints[0].clone();
    assert_eq!(c.expr, None);
    assert!((c.length_mm - 150.0).abs() < 1e-12);
    // La contrainte reste (nombre figé) : la variable n'a plus d'effet.
    doc.execute(set_var("w", "10")).unwrap();
    near(pos(doc.project(), s, b), 150.0, 0.0, 1e-6);
    // Délier sans formule : aucune étape.
    let before = bytes(doc.project());
    let steps = doc.can_undo();
    doc.execute(unlink(s, spline)).unwrap();
    assert_eq!(bytes(doc.project()), before);
    assert!(steps);
    assert!(doc.undo()); // annule seulement le changement de variable
    assert_eq!(
        doc.project().sheet(s).unwrap().length_constraints[0].expr,
        None
    );
}

#[test]
fn refus_sans_trace_formule_vide_inconnue_sans_contrainte_ou_avec_reference() {
    let (mut sm, _) = formula_world();
    let (s, spline) = (sm.s, sm.spline);
    let e = refused(&mut sm.p, expr(s, spline, "   "));
    assert!(e.contains("formule vide"), "{e}");
    let e = refused(&mut sm.p, expr(s, spline, "inconnue*2"));
    assert!(e.contains("ne peut pas être calculée"), "{e}");
    let e = refused(&mut sm.p, expr(s, spline, "w*"));
    assert!(e.contains("ne peut pas être calculée"), "{e}");
    let e = refused(&mut sm.p, expr(s, spline, "0-w"));
    assert!(e.contains("strictement positif"), "{e}");
    // Aucune longueur imposée sur une autre courbe.
    let other =
        sm.p.apply(Command::AddSpline {
            sheet: s,
            points: vec![
                PointRef::New(PointMm::new(0.0, 300.0)),
                PointRef::New(PointMm::new(100.0, 300.0)),
            ],
        })
        .unwrap();
    let e = refused(&mut sm.p, expr(s, other.entities[0], "w"));
    assert!(e.contains("aucune longueur imposée"), "{e}");
    // Égalité avec un autre tracé : pas de formule (formule comme décalage : 03.4b).
    sm.p.apply(Command::SetCurveLengthConstraint {
        sheet: s,
        curve: other.entities[0],
        spec: Some(LengthSpec {
            length_mm: 1.0,
            adjust: slide(other.points[1], 0.0),
            reference: Some(LengthRef {
                entity: spline,
                offset_mm: 5.0,
            }),
        }),
    })
    .unwrap();
    let e = refused(&mut sm.p, expr(s, other.entities[0], "w"));
    assert!(e.contains("autre tracé"), "{e}");
}

#[test]
fn nouvelle_demande_de_longueur_remplace_la_contrainte_et_sa_formule() {
    let (mut sm, _) = formula_world();
    sm.p.apply(impose(sm.s, sm.spline, 130.0, slide(sm.pts[1], 0.0)))
        .unwrap();
    let c = &sm.p.sheet(sm.s).unwrap().length_constraints[0];
    assert_eq!(c.expr, None);
    near(pos(&sm.p, sm.s, sm.pts[1]), 130.0, 0.0, 1e-6);
    sm.p.apply(set_var("w", "10")).unwrap();
    near(pos(&sm.p, sm.s, sm.pts[1]), 130.0, 0.0, 1e-6);
}

#[test]
fn batch_contrainte_et_formule_est_une_seule_etape() {
    // Ce que fait l'inspecteur : contrainte puis formule dans une étape annulable.
    let sm = smooth(&[(0.0, 0.0), (100.0, 0.0)]);
    let (s, spline, b) = (sm.s, sm.spline, sm.pts[1]);
    let mut p = sm.p;
    var(&mut p, "w", "100");
    let mut doc = Document::new(p);
    let before = bytes(doc.project());
    doc.execute(Command::Batch {
        commands: vec![
            impose(s, spline, 100.0, slide(b, 0.0)),
            expr(s, spline, "w+25"),
        ],
    })
    .unwrap();
    near(pos(doc.project(), s, b), 125.0, 0.0, 1e-6);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
    assert!(!doc.can_undo());
}

#[test]
fn formule_sur_une_courbe_de_bezier() {
    // Bézier à poignées nulles : longueur = distance. Cible « k*2+50 » avec k = 100 → 250, puis
    // k = 120 → 290 (extrémité en (290 ; 0)).
    let mut p = Project::new("Bézier");
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
    let (curve, b) = (o.entities[0], o.points[1]);
    var(&mut p, "k", "100");
    p.apply(impose(s, curve, 100.0, slide(b, 0.0))).unwrap();
    p.apply(expr(s, curve, "k*2+50")).unwrap();
    near(pos(&p, s, b), 250.0, 0.0, 1e-6);
    p.apply(set_var("k", "120")).unwrap();
    near(pos(&p, s, b), 290.0, 0.0, 1e-6);
}

// ---------------------------------------------------------------------------------------------
// Copies, diagnostic, dépendances, fichier
// ---------------------------------------------------------------------------------------------

#[test]
fn copie_de_feuille_garde_la_formule_et_l_import_sans_la_variable_la_delie() {
    let (sm, _) = formula_world();
    let s = sm.s;
    let mut p = sm.p;
    let o = p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    let copy = o.sheet.unwrap();
    let c = p.sheet(copy).unwrap().length_constraints[0].clone();
    assert_eq!(c.expr.as_deref(), Some("w*3/2"));
    // La copie suit la variable, la source aussi.
    p.apply(set_var("w", "80")).unwrap();
    assert!((p.sheet(copy).unwrap().length_constraints[0].length_mm - 120.0).abs() < 1e-12);
    assert!((p.sheet(s).unwrap().length_constraints[0].length_mm - 120.0).abs() < 1e-12);
    // Import dans un projet sans la variable : formule déliée avec remarque, valeur gardée.
    let mut other = Project::new("Autre");
    let sheet = p.sheet(s).unwrap().clone();
    let o = other
        .apply(Command::ImportSheet {
            sheet: Box::new(sheet),
            pieces: vec![],
        })
        .unwrap();
    assert!(
        o.notes.iter().any(|n| n.contains("formule(s) de longueur")),
        "{:?}",
        o.notes
    );
    let imported = other.sheet(o.sheet.unwrap()).unwrap();
    assert_eq!(imported.length_constraints[0].expr, None);
    assert!((imported.length_constraints[0].length_mm - 120.0).abs() < 1e-12);
}

#[test]
fn diagnostic_et_dependances_voient_la_formule() {
    let (sm, _) = formula_world();
    let diags = sm.p.link_diagnostics();
    let d = diags
        .iter()
        .find(|d| d.link == LinkType::ImposedLength)
        .expect("diagnostic de la longueur imposée");
    assert_eq!(d.kind, DiagnosticKind::Active);
    assert_eq!(d.text, "w*3/2");
    let edges = sm.p.dependency_edges();
    assert!(
        edges
            .iter()
            .any(|e| e.parent == DependencyObject::Variable("w".into())
                && e.child == DependencyObject::Entity(sm.spline)
                && e.nature == DependencyNature::FormulaReference),
        "{edges:?}"
    );
}

#[test]
fn aller_retour_octets_et_projet_sans_formule_sans_cle() {
    let (sm, _) = formula_world();
    let text = bytes(&sm.p);
    let back = persist::from_json(&text).unwrap();
    assert_eq!(bytes(&back), text);
    assert_eq!(back, sm.p);
    // Sans formule : aucune clé `expr` dans la contrainte.
    let mut sm2 = smooth(&[(0.0, 0.0), (100.0, 0.0)]);
    sm2.p
        .apply(impose(sm2.s, sm2.spline, 150.0, slide(sm2.pts[1], 0.0)))
        .unwrap();
    let v: Value = serde_json::from_str(&bytes(&sm2.p)).unwrap();
    assert!(v["sheets"][0]["length_constraints"][0]
        .get("expr")
        .is_none());
}

fn altered(p: &Project, edit: impl FnOnce(&mut Value)) -> Result<Project, CoreError> {
    let mut v: Value = serde_json::from_str(&bytes(p)).unwrap();
    edit(&mut v["sheets"][0]["length_constraints"][0]);
    persist::from_json(&serde_json::to_string(&v).unwrap())
}

#[test]
fn ouverture_d_un_fichier_altere_refusee_avec_message_localise() {
    let (sm, _) = formula_world();
    assert!(altered(&sm.p, |_| {}).is_ok());
    // Formule changée sans recalcul : la valeur écrite ne correspond plus.
    let e = altered(&sm.p, |c| c["expr"] = json!("w"))
        .unwrap_err()
        .to_string();
    assert!(e.contains("formule"), "{e}");
    assert!(e.contains("feuille"), "{e}");
    // Variable inconnue, formule vide, expression syntaxiquement fausse.
    assert!(altered(&sm.p, |c| c["expr"] = json!("inconnue")).is_err());
    assert!(altered(&sm.p, |c| c["expr"] = json!("")).is_err());
    assert!(altered(&sm.p, |c| c["expr"] = json!("w*")).is_err());
    // expr de type non texte, valeur écrite décalée de 1e-3.
    assert!(altered(&sm.p, |c| c["expr"] = json!(5)).is_err());
    assert!(altered(&sm.p, |c| c["length_mm"] = json!(150.001)).is_err());
    // Formule ET référence : refusé.
    let e = altered(&sm.p, |c| {
        c["reference"] = json!({ "entity": c["curve"].clone(), "offset_mm": 1.0 })
    })
    .unwrap_err()
    .to_string();
    assert!(e.contains("pas les deux"), "{e}");
    // Poignée sur une courbe lisse : refusé avec le motif.
    let e =
        altered(
            &sm.p,
            |c| c["adjust"] = json!({ "type": "handle", "point": c["adjust"]["point"].clone() }),
        )
        .unwrap_err()
        .to_string();
    assert!(e.contains("poignée"), "{e}");
}

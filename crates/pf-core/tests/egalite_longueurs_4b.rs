//! US-03.4b — égalité de longueurs étendue : cible = **somme** de tracés, de la même feuille ou
//! d'**autres feuilles**, + décalage en nombre ou **maintenu par une formule**. Valeurs calculées à
//! la main, un test par critère, refus sans trace (octets identiques), une seule étape annulable,
//! règles de suppression, copie / importation, lecture d'anciens fichiers à l'identique.

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;
use serde_json::{json, Value};

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

/// Une contrainte de longueur de ce texte JSON porte-t-elle une extension `link` ?
fn has_link_key(json: &str) -> bool {
    let v: Value = serde_json::from_str(json).unwrap();
    v["sheets"].as_array().unwrap().iter().any(|s| {
        s["length_constraints"]
            .as_array()
            .is_some_and(|a| a.iter().any(|c| c.get("link").is_some()))
    })
}

/// Commande refusée : projet identique à l'octet ; renvoie le message.
fn refused(p: &mut Project, cmd: Command) -> String {
    let before = bytes(p);
    let before_p = p.clone();
    let e = p.apply(cmd).expect_err("devrait être refusée").to_string();
    assert_eq!(bytes(p), before, "trace laissée par un refus : {e}");
    assert_eq!(*p, before_p);
    e
}

fn pos(p: &Project, s: SheetId, id: PointId) -> PointMm {
    p.sheet(s).unwrap().position(id).unwrap()
}

fn dev(p: &Project, s: SheetId, e: EntityId) -> f64 {
    p.sheet(s).unwrap().curve_developed_mm(e).unwrap()
}

fn mv(s: SheetId, point: PointId, x: f64, y: f64) -> Command {
    Command::MovePoint {
        sheet: s,
        point,
        to: PointMm::new(x, y),
    }
}

fn segment(
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

/// Courbe droite de (0 ; y) à (x ; y) : longueur x.
fn straight(p: &mut Project, s: SheetId, y: f64, x: f64) -> (PointId, PointId, EntityId) {
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

/// Longueur imposée par un nombre : l'extrémité `point` glisse le long de +X.
fn impose(s: SheetId, curve: EntityId, point: PointId, length_mm: f64) -> Command {
    Command::SetCurveLengthConstraint {
        sheet: s,
        curve,
        spec: Some(LengthSpec {
            length_mm,
            adjust: LengthAdjust::Slide {
                point,
                angle_deg: 0.0,
            },
            reference: None,
        }),
    }
}

fn term(entity: EntityId, sheet: Option<SheetId>) -> LengthTerm {
    LengthTerm { entity, sheet }
}

fn equality(
    s: SheetId,
    curve: EntityId,
    terms: Vec<LengthTerm>,
    offset_mm: f64,
    offset_expr: Option<&str>,
) -> Command {
    Command::SetCurveLengthEquality {
        sheet: s,
        curve,
        equality: Some(LengthEquality {
            terms,
            offset_mm,
            offset_expr: offset_expr.map(str::to_string),
        }),
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

struct World {
    p: Project,
    /// Feuille A : courbe C (150 mm au départ), segment L de 60 mm.
    a: SheetId,
    c: EntityId,
    c_end: PointId,
    l: EntityId,
    /// Feuille B : segment R de 100 mm, segment Q de 50 mm ((0 ; 10) → (30 ; 50)), courbe D de 80 mm.
    b: SheetId,
    r: EntityId,
    r_end: PointId,
    q: EntityId,
    d: EntityId,
    d_end: PointId,
}

fn world() -> World {
    let mut p = Project::new("Somme");
    let a = p.sheets[0].id;
    let b = p
        .apply(Command::CreateSheet {
            name: "Manche".into(),
        })
        .unwrap()
        .sheet
        .unwrap();
    let (_, _, l) = segment(&mut p, a, (0.0, 0.0), (60.0, 0.0));
    let (_, c_end, c) = straight(&mut p, a, 60.0, 150.0);
    let (_, r_end, r) = segment(&mut p, b, (0.0, 0.0), (100.0, 0.0));
    let (_, _, q) = segment(&mut p, b, (0.0, 10.0), (30.0, 50.0));
    let (_, d_end, d) = straight(&mut p, b, 30.0, 80.0);
    World {
        p,
        a,
        c,
        c_end,
        l,
        b,
        r,
        r_end,
        q,
        d,
        d_end,
    }
}

// ---------------------------------------------------------------------------------------------
// (1) Tracé d'une autre feuille
// ---------------------------------------------------------------------------------------------

#[test]
fn ca1_tracé_d_une_autre_feuille_100_plus_12_donne_112() {
    let mut w = world();
    w.p.apply(impose(w.a, w.c, w.c_end, 150.0)).unwrap();
    w.p.apply(equality(w.a, w.c, vec![term(w.r, Some(w.b))], 12.0, None))
        .unwrap();
    // C part de (0 ; 60), glisse le long de X : 100 + 12 = 112 mm.
    let q = pos(&w.p, w.a, w.c_end);
    assert!(
        (q.x - 112.0).abs() < 1e-6 && (q.y - 60.0).abs() < 1e-9,
        "{q:?}"
    );
    let k = w.p.sheet(w.a).unwrap().length_constraint(w.c).unwrap();
    assert!((k.length_mm - 112.0).abs() < 1e-9);
    assert_eq!(k.reference.unwrap().entity, w.r);
    assert_eq!(k.link.as_ref().unwrap().sheet, Some(w.b));
    assert!(k.link.as_ref().unwrap().more.is_empty());
}

#[test]
fn ca1_deplacer_la_reference_d_une_autre_feuille_re_resout_en_une_etape() {
    let mut w = world();
    w.p.apply(impose(w.a, w.c, w.c_end, 150.0)).unwrap();
    w.p.apply(equality(w.a, w.c, vec![term(w.r, Some(w.b))], 12.0, None))
        .unwrap();
    let mut doc = Document::new(w.p.clone());
    let before = bytes(doc.project());
    doc.execute(mv(w.b, w.r_end, 140.0, 0.0)).unwrap();
    // R mesure 140 : C = 152 ; l'extrémité de C est en (152 ; 60).
    assert!((pos(doc.project(), w.a, w.c_end).x - 152.0).abs() < 1e-6);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
    assert!(!doc.can_undo() || doc.undo());
}

// ---------------------------------------------------------------------------------------------
// (2) Somme de plusieurs tracés
// ---------------------------------------------------------------------------------------------

#[test]
fn ca2_somme_de_deux_tracés_de_l_autre_feuille_plus_5() {
    let mut w = world();
    w.p.apply(impose(w.a, w.c, w.c_end, 150.0)).unwrap();
    // R = 100, Q = hypot(30 ; 40) = 50 : 100 + 50 + 5 = 155.
    w.p.apply(equality(
        w.a,
        w.c,
        vec![term(w.r, Some(w.b)), term(w.q, Some(w.b))],
        5.0,
        None,
    ))
    .unwrap();
    assert!((pos(&w.p, w.a, w.c_end).x - 155.0).abs() < 1e-6);
    let k = w.p.sheet(w.a).unwrap().length_constraint(w.c).unwrap();
    assert_eq!(k.terms().len(), 2);
}

#[test]
fn ca2_somme_mixte_meme_feuille_et_autre_feuille_decalage_negatif() {
    let mut w = world();
    w.p.apply(impose(w.a, w.c, w.c_end, 150.0)).unwrap();
    // L (feuille A) = 60, R (feuille B) = 100, décalage -10 : 150 ; puis Q ajouté : 200.
    w.p.apply(equality(
        w.a,
        w.c,
        vec![term(w.l, None), term(w.r, Some(w.b))],
        -10.0,
        None,
    ))
    .unwrap();
    assert!((dev(&w.p, w.a, w.c) - 150.0).abs() < 1e-6);
    w.p.apply(equality(
        w.a,
        w.c,
        vec![term(w.l, None), term(w.r, Some(w.b)), term(w.q, Some(w.b))],
        -10.0,
        None,
    ))
    .unwrap();
    assert!((dev(&w.p, w.a, w.c) - 200.0).abs() < 1e-6);
    // Déplacer l'extrémité de L (feuille A) : 60 -> 80 : 220.
    let l_end = w.p.sheet(w.a).unwrap().entity_points(w.l).unwrap().1;
    w.p.apply(mv(w.a, l_end, 80.0, 0.0)).unwrap();
    assert!((dev(&w.p, w.a, w.c) - 220.0).abs() < 1e-6);
}

#[test]
fn ca2_la_somme_est_dans_l_ordre_donne_et_le_premier_tracé_reste_la_reference() {
    let mut w = world();
    w.p.apply(impose(w.a, w.c, w.c_end, 150.0)).unwrap();
    w.p.apply(equality(
        w.a,
        w.c,
        vec![term(w.q, Some(w.b)), term(w.l, None)],
        0.0,
        None,
    ))
    .unwrap();
    let k = w.p.sheet(w.a).unwrap().length_constraint(w.c).unwrap();
    assert_eq!(k.reference.unwrap().entity, w.q);
    assert_eq!(k.link.as_ref().unwrap().sheet, Some(w.b));
    assert_eq!(k.link.as_ref().unwrap().more, vec![term(w.l, None)]);
    // 50 + 60 = 110.
    assert!((dev(&w.p, w.a, w.c) - 110.0).abs() < 1e-6);
}

// ---------------------------------------------------------------------------------------------
// (3) Décalage par formule
// ---------------------------------------------------------------------------------------------

#[test]
fn ca3_decalage_par_formule_suit_la_variable_dans_la_meme_etape() {
    let mut w = world();
    var(&mut w.p, "embu", "7");
    w.p.apply(impose(w.a, w.c, w.c_end, 150.0)).unwrap();
    w.p.apply(equality(
        w.a,
        w.c,
        vec![term(w.r, Some(w.b))],
        0.0,
        Some("embu*2-1"),
    ))
    .unwrap();
    // embu = 7 : décalage 13, longueur 113 ; la valeur évaluée est écrite.
    let k = w.p.sheet(w.a).unwrap().length_constraint(w.c).unwrap();
    assert_eq!(k.offset_expr(), Some("embu*2-1"));
    assert!((k.reference.unwrap().offset_mm - 13.0).abs() < 1e-9);
    assert!((k.length_mm - 113.0).abs() < 1e-9);
    let mut doc = Document::new(w.p.clone());
    let before = bytes(doc.project());
    doc.execute(Command::SetVariableExpr {
        name: "embu".into(),
        expr: "9".into(),
    })
    .unwrap();
    // embu = 9 : décalage 17, longueur 117, dans la même étape que la variable.
    assert!((dev(doc.project(), w.a, w.c) - 117.0).abs() < 1e-6);
    let k = doc
        .project()
        .sheet(w.a)
        .unwrap()
        .length_constraint(w.c)
        .unwrap();
    assert!((k.reference.unwrap().offset_mm - 17.0).abs() < 1e-9);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
}

#[test]
fn ca3_formule_meme_feuille_et_somme_ensemble() {
    let mut w = world();
    var(&mut w.p, "e", "4");
    w.p.apply(impose(w.a, w.c, w.c_end, 150.0)).unwrap();
    // L (60) + Q (50) - e (4) = 106.
    w.p.apply(equality(
        w.a,
        w.c,
        vec![term(w.l, None), term(w.q, Some(w.b))],
        0.0,
        Some("-e"),
    ))
    .unwrap();
    assert!((dev(&w.p, w.a, w.c) - 106.0).abs() < 1e-6);
}

#[test]
fn ca3_renommer_et_supprimer_une_variable_du_decalage() {
    let mut w = world();
    var(&mut w.p, "embu", "7");
    w.p.apply(impose(w.a, w.c, w.c_end, 150.0)).unwrap();
    w.p.apply(equality(w.a, w.c, vec![term(w.r, None)], 0.0, Some("embu")))
        .ok();
    // R est sur la feuille B : la même feuille n'a pas ce tracé -> refus ; on vise la feuille B.
    w.p.apply(equality(
        w.a,
        w.c,
        vec![term(w.r, Some(w.b))],
        0.0,
        Some("embu"),
    ))
    .unwrap();
    let e = refused(
        &mut w.p,
        Command::RemoveVariable {
            name: "embu".into(),
        },
    );
    assert!(e.contains("décalage"), "{e}");
    w.p.apply(Command::RenameVariable {
        from: "embu".into(),
        to: "aisance".into(),
    })
    .unwrap();
    let k = w.p.sheet(w.a).unwrap().length_constraint(w.c).unwrap();
    assert_eq!(k.offset_expr(), Some("aisance"));
    assert!((dev(&w.p, w.a, w.c) - 107.0).abs() < 1e-6);
}

// ---------------------------------------------------------------------------------------------
// Ordre topologique global (toutes feuilles)
// ---------------------------------------------------------------------------------------------

#[test]
fn chaine_a_travers_les_feuilles_dans_les_deux_sens_d_index() {
    let mut w = world();
    // B : D = R + 3 (même feuille). A : C = D (feuille B) + 2. A est avant B dans la liste : un
    // ordre feuille par feuille calculerait C avec un D périmé.
    w.p.apply(impose(w.b, w.d, w.d_end, 80.0)).unwrap();
    w.p.apply(equality(w.b, w.d, vec![term(w.r, None)], 3.0, None))
        .unwrap();
    w.p.apply(impose(w.a, w.c, w.c_end, 150.0)).unwrap();
    w.p.apply(equality(w.a, w.c, vec![term(w.d, Some(w.b))], 2.0, None))
        .unwrap();
    assert!((dev(&w.p, w.b, w.d) - 103.0).abs() < 1e-6);
    assert!((dev(&w.p, w.a, w.c) - 105.0).abs() < 1e-6);
    // R passe à 120 : D = 123, C = 125, en une commande.
    w.p.apply(mv(w.b, w.r_end, 120.0, 0.0)).unwrap();
    assert!((dev(&w.p, w.b, w.d) - 123.0).abs() < 1e-6);
    assert!((dev(&w.p, w.a, w.c) - 125.0).abs() < 1e-6);
    // Sens inverse : on met la feuille B avant la feuille A.
    w.p.apply(Command::MoveSheet {
        sheet: w.b,
        new_index: 0,
    })
    .unwrap();
    w.p.apply(mv(w.b, w.r_end, 90.0, 0.0)).unwrap();
    assert!((dev(&w.p, w.b, w.d) - 93.0).abs() < 1e-6);
    assert!((dev(&w.p, w.a, w.c) - 95.0).abs() < 1e-6);
}

#[test]
fn cycle_entre_deux_feuilles_refuse_sans_trace() {
    let mut w = world();
    w.p.apply(impose(w.a, w.c, w.c_end, 150.0)).unwrap();
    w.p.apply(equality(w.a, w.c, vec![term(w.d, Some(w.b))], 0.0, None))
        .unwrap();
    // D est imposée par un nombre (rien de circulaire : C attend D) ; D = C ferme la boucle.
    w.p.apply(impose(w.b, w.d, w.d_end, 80.0)).unwrap();
    let mut p = w.p.clone();
    let e = refused(
        &mut p,
        equality(w.b, w.d, vec![term(w.c, Some(w.a))], 0.0, None),
    );
    assert!(e.contains("circulaire"), "{e}");
}

// ---------------------------------------------------------------------------------------------
// Refus propres, sans trace
// ---------------------------------------------------------------------------------------------

#[test]
fn refus_sans_trace() {
    let mut w = world();
    var(&mut w.p, "v", "5");
    // Sans longueur imposée.
    let e = refused(
        &mut w.p,
        equality(w.a, w.c, vec![term(w.r, Some(w.b))], 1.0, None),
    );
    assert!(e.contains("aucune longueur imposée"), "{e}");
    w.p.apply(impose(w.a, w.c, w.c_end, 150.0)).unwrap();
    // Aucun tracé.
    refused(&mut w.p, equality(w.a, w.c, vec![], 1.0, None));
    // Référence manquante : tracé inconnu (feuille de la courbe), sur l'autre feuille, feuille inconnue.
    let e = refused(
        &mut w.p,
        equality(w.a, w.c, vec![term(EntityId(999_999), None)], 1.0, None),
    );
    assert!(e.contains("n'existe pas"), "{e}");
    let e = refused(
        &mut w.p,
        equality(w.a, w.c, vec![term(w.l, Some(w.b))], 1.0, None),
    );
    assert!(e.contains("Manche"), "{e}");
    let e = refused(
        &mut w.p,
        equality(w.a, w.c, vec![term(w.r, Some(SheetId(888_888)))], 1.0, None),
    );
    assert!(e.contains("feuille"), "{e}");
    // Auto-référence et tracé cité deux fois.
    let e = refused(
        &mut w.p,
        equality(w.a, w.c, vec![term(w.c, None)], 1.0, None),
    );
    assert!(e.contains("soi-même"), "{e}");
    let e = refused(
        &mut w.p,
        equality(
            w.a,
            w.c,
            vec![term(w.r, Some(w.b)), term(w.r, Some(w.b))],
            1.0,
            None,
        ),
    );
    assert!(e.contains("qu'une fois"), "{e}");
    // Formule vide, non évaluable, inconnue.
    refused(
        &mut w.p,
        equality(w.a, w.c, vec![term(w.r, Some(w.b))], 1.0, Some("  ")),
    );
    refused(
        &mut w.p,
        equality(
            w.a,
            w.c,
            vec![term(w.r, Some(w.b))],
            1.0,
            Some("inconnue+1"),
        ),
    );
    refused(
        &mut w.p,
        equality(w.a, w.c, vec![term(w.r, Some(w.b))], 1.0, Some("1/0")),
    );
    // Cible <= 0 : 100 - 100 = 0, par nombre et par formule (100 - 105).
    let e = refused(
        &mut w.p,
        equality(w.a, w.c, vec![term(w.r, Some(w.b))], -100.0, None),
    );
    assert!(e.contains("pas de solution"), "{e}");
    refused(
        &mut w.p,
        equality(w.a, w.c, vec![term(w.r, Some(w.b))], 0.0, Some("-v-100")),
    );
    // Décalage non fini ou hors borne.
    refused(
        &mut w.p,
        equality(w.a, w.c, vec![term(w.r, Some(w.b))], f64::NAN, None),
    );
    refused(
        &mut w.p,
        equality(w.a, w.c, vec![term(w.r, Some(w.b))], 2.0e6, None),
    );
    // Courbe inconnue.
    refused(
        &mut w.p,
        equality(
            w.a,
            EntityId(777_777),
            vec![term(w.r, Some(w.b))],
            1.0,
            None,
        ),
    );
}

#[test]
fn tracé_de_longueur_nulle_refuse_si_la_cible_n_est_pas_positive() {
    let mut w = world();
    // Courbe de longueur nulle sur la feuille B : départ et arrivée au même endroit.
    let zero = w.p.apply(Command::AddCurve {
        sheet: w.b,
        start: PointRef::New(PointMm::new(5.0, 5.0)),
        c1: PointMm::new(5.0, 5.0),
        c2: PointMm::new(5.0, 5.0),
        end: PointRef::New(PointMm::new(5.0, 5.0)),
    });
    let Ok(o) = zero else {
        // Création refusée : un tracé dégénéré n'existe pas dans le modèle, rien à référencer.
        return;
    };
    let z = o.entities[0];
    w.p.apply(impose(w.a, w.c, w.c_end, 150.0)).unwrap();
    // Zéro + décalage nul : cible nulle, refusée ; zéro + R (100) accepté (la somme est positive).
    refused(
        &mut w.p,
        equality(w.a, w.c, vec![term(z, Some(w.b))], 0.0, None),
    );
    w.p.apply(equality(
        w.a,
        w.c,
        vec![term(z, Some(w.b)), term(w.r, Some(w.b))],
        0.0,
        None,
    ))
    .unwrap();
    assert!((dev(&w.p, w.a, w.c) - 100.0).abs() < 1e-6);
}

#[test]
fn re_resolution_impossible_refuse_la_commande_mere_avec_la_somme() {
    let mut w = world();
    w.p.apply(impose(w.a, w.c, w.c_end, 150.0)).unwrap();
    w.p.apply(equality(
        w.a,
        w.c,
        vec![term(w.r, Some(w.b)), term(w.q, Some(w.b))],
        -140.0,
        None,
    ))
    .unwrap();
    // Cible 100 + 50 - 140 = 10 ; ramener R à 20 (20 + 50 - 140 < 0) est refusé.
    let e = refused(&mut w.p, mv(w.b, w.r_end, 20.0, 0.0));
    assert!(
        e.contains("égale à") || e.contains("pas de solution"),
        "{e}"
    );
}

// ---------------------------------------------------------------------------------------------
// Suppression / modification de ce qui est référencé (règles de 03.4a)
// ---------------------------------------------------------------------------------------------

fn sum_world() -> World {
    let mut w = world();
    w.p.apply(impose(w.a, w.c, w.c_end, 150.0)).unwrap();
    w.p.apply(equality(
        w.a,
        w.c,
        vec![term(w.r, Some(w.b)), term(w.q, Some(w.b))],
        5.0,
        None,
    ))
    .unwrap();
    w
}

#[test]
fn feuille_de_reference_supprimee_retire_la_contrainte_et_annule_exactement() {
    let w = sum_world();
    let mut doc = Document::new(w.p.clone());
    let before = bytes(doc.project());
    let o = doc.execute(Command::DeleteSheet { sheet: w.b }).unwrap();
    assert!(
        o.notes
            .iter()
            .any(|n| n.contains("longueur imposée") && n.contains("feuille")),
        "{:?}",
        o.notes
    );
    assert!(doc
        .project()
        .sheet(w.a)
        .unwrap()
        .length_constraints
        .is_empty());
    // La géométrie de C est conservée.
    assert!((dev(doc.project(), w.a, w.c) - 155.0).abs() < 1e-6);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
}

#[test]
fn un_seul_tracé_de_la_somme_supprime_retire_toute_la_contrainte() {
    let mut w = sum_world();
    let o =
        w.p.apply(Command::Delete {
            sheet: w.b,
            points: vec![],
            entities: vec![w.q],
        })
        .unwrap();
    assert!(
        o.notes.iter().any(|n| n.contains("référence")),
        "{:?}",
        o.notes
    );
    assert!(w.p.sheet(w.a).unwrap().length_constraints.is_empty());
    assert!((dev(&w.p, w.a, w.c) - 155.0).abs() < 1e-6);
}

#[test]
fn tracé_de_la_somme_coupe_sur_l_autre_feuille_retire_la_contrainte() {
    let mut w = sum_world();
    let o =
        w.p.apply(Command::SplitPath {
            sheet: w.b,
            entity: w.r,
            at: PointMm::new(40.0, 0.0),
        })
        .unwrap();
    assert!(o.notes.iter().any(|n| n.contains("longueur imposée")));
    assert!(w.p.sheet(w.a).unwrap().length_constraints.is_empty());
}

#[test]
fn courbe_contrainte_supprimee_ne_touche_pas_les_references() {
    let mut w = sum_world();
    w.p.apply(Command::Delete {
        sheet: w.a,
        points: vec![],
        entities: vec![w.c],
    })
    .unwrap();
    assert!(w.p.sheet(w.a).unwrap().length_constraints.is_empty());
    assert!(w.p.sheet(w.b).unwrap().entity(w.r).is_ok());
}

// ---------------------------------------------------------------------------------------------
// Forme canonique, une étape, déliage
// ---------------------------------------------------------------------------------------------

#[test]
fn egalite_simple_meme_octets_par_les_deux_commandes() {
    let mut w1 = world();
    w1.p.apply(impose(w1.a, w1.c, w1.c_end, 150.0)).unwrap();
    w1.p.apply(Command::SetCurveLengthConstraint {
        sheet: w1.a,
        curve: w1.c,
        spec: Some(LengthSpec {
            length_mm: 1.0,
            adjust: LengthAdjust::Slide {
                point: w1.c_end,
                angle_deg: 0.0,
            },
            reference: Some(LengthRef {
                entity: w1.l,
                offset_mm: 12.0,
            }),
        }),
    })
    .unwrap();
    let mut w2 = world();
    w2.p.apply(impose(w2.a, w2.c, w2.c_end, 150.0)).unwrap();
    // La feuille de la courbe écrite explicitement est ramenée à la forme canonique.
    w2.p.apply(equality(
        w2.a,
        w2.c,
        vec![term(w2.l, Some(w2.a))],
        12.0,
        None,
    ))
    .unwrap();
    assert_eq!(bytes(&w1.p), bytes(&w2.p));
    assert!(!has_link_key(&bytes(&w2.p)));
    assert!((dev(&w2.p, w2.a, w2.c) - 72.0).abs() < 1e-6);
}

#[test]
fn pose_repetee_n_est_pas_une_etape_et_delier_garde_la_geometrie() {
    let w = world();
    let mut doc = Document::new(w.p.clone());
    doc.execute(impose(w.a, w.c, w.c_end, 150.0)).unwrap();
    let eq = equality(w.a, w.c, vec![term(w.r, Some(w.b))], 12.0, None);
    doc.execute(eq.clone()).unwrap();
    let with = bytes(doc.project());
    doc.execute(eq).unwrap();
    assert_eq!(bytes(doc.project()), with);
    // Délier : la valeur 112 est gardée en nombre, plus de référence ni d'extension.
    doc.execute(Command::SetCurveLengthEquality {
        sheet: w.a,
        curve: w.c,
        equality: None,
    })
    .unwrap();
    let k = doc
        .project()
        .sheet(w.a)
        .unwrap()
        .length_constraint(w.c)
        .unwrap();
    assert!(k.reference.is_none() && k.link.is_none());
    assert!((k.length_mm - 112.0).abs() < 1e-9);
    // Délier sans égalité : aucune étape.
    let unlinked = bytes(doc.project());
    doc.execute(Command::SetCurveLengthEquality {
        sheet: w.a,
        curve: w.c,
        equality: None,
    })
    .unwrap();
    assert_eq!(bytes(doc.project()), unlinked);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), with);
    assert!(doc.undo());
    assert!(doc.undo());
    assert!(!doc.can_undo());
}

#[test]
fn une_egalite_remplace_une_formule_de_cible() {
    let mut w = world();
    var(&mut w.p, "t", "130");
    w.p.apply(impose(w.a, w.c, w.c_end, 150.0)).unwrap();
    w.p.apply(Command::SetCurveLengthExpr {
        sheet: w.a,
        curve: w.c,
        expr: Some("t".into()),
    })
    .unwrap();
    w.p.apply(equality(w.a, w.c, vec![term(w.r, Some(w.b))], 1.0, None))
        .unwrap();
    let k = w.p.sheet(w.a).unwrap().length_constraint(w.c).unwrap();
    assert!(k.expr.is_none());
    assert!((k.length_mm - 101.0).abs() < 1e-9);
    // Et inversement : la formule de cible reste refusée tant qu'il y a une égalité.
    refused(
        &mut w.p,
        Command::SetCurveLengthExpr {
            sheet: w.a,
            curve: w.c,
            expr: Some("t".into()),
        },
    );
}

// ---------------------------------------------------------------------------------------------
// Copie de feuille, importation
// ---------------------------------------------------------------------------------------------

#[test]
fn dupliquer_la_feuille_garde_les_references_d_autre_feuille_et_remappe_les_siennes() {
    let mut w = world();
    w.p.apply(impose(w.a, w.c, w.c_end, 150.0)).unwrap();
    // Somme : L (même feuille, remappé) + R (feuille B, identifiant gardé).
    w.p.apply(equality(
        w.a,
        w.c,
        vec![term(w.l, None), term(w.r, Some(w.b))],
        0.0,
        None,
    ))
    .unwrap();
    let copy =
        w.p.apply(Command::DuplicateSheet { sheet: w.a })
            .unwrap()
            .sheet
            .unwrap();
    let dst = w.p.sheet(copy).unwrap();
    let k = dst.length_constraints[0].clone();
    let terms = k.terms();
    assert_ne!(terms[0].entity, w.l);
    assert!(dst.entity(terms[0].entity).is_ok());
    assert_eq!(terms[1], term(w.r, Some(w.b)));
    assert!((dst.curve_developed_mm(k.curve).unwrap() - 160.0).abs() < 1e-6);
    // R passe à 120 : l'original et la copie suivent (180 chacune).
    w.p.apply(mv(w.b, w.r_end, 120.0, 0.0)).unwrap();
    assert!((dev(&w.p, w.a, w.c) - 180.0).abs() < 1e-6);
    assert!((dev(&w.p, copy, k.curve) - 180.0).abs() < 1e-6);
}

#[test]
fn importer_une_feuille_d_un_autre_projet_delie_les_references_d_autre_feuille() {
    let mut w = world();
    w.p.apply(impose(w.a, w.c, w.c_end, 150.0)).unwrap();
    w.p.apply(equality(w.a, w.c, vec![term(w.r, Some(w.b))], 12.0, None))
        .unwrap();
    let mut other = Project::new("Autre");
    let o = other
        .apply(Command::ImportSheet {
            sheet: Box::new(w.p.sheet(w.a).unwrap().clone()),
            pieces: vec![],
        })
        .unwrap();
    assert!(
        o.notes.iter().any(|n| n.contains("autre feuille")),
        "{:?}",
        o.notes
    );
    let s = other.sheet(o.sheet.unwrap()).unwrap();
    let k = &s.length_constraints[0];
    assert!(k.reference.is_none() && k.link.is_none());
    assert!((k.length_mm - 112.0).abs() < 1e-9);
    assert!(!has_link_key(&bytes(&other)));
}

// ---------------------------------------------------------------------------------------------
// Format : ancien fichier lu et réécrit à l'identique, fichier altéré refusé
// ---------------------------------------------------------------------------------------------

fn read_dir_files(dir: &str, suffix: &str) -> Vec<std::path::PathBuf> {
    let mut out: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("{dir} : {e}"))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.to_string_lossy().ends_with(suffix))
        .collect();
    out.sort();
    out
}

#[test]
fn les_exemples_se_relisent_et_se_reecrivent_a_l_identique_sans_cle_link() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");
    let files = read_dir_files(root, ".pforge.json");
    assert!(files.len() >= 4, "{files:?}");
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        let p = persist::from_json(&text).unwrap_or_else(|e| panic!("{f:?} : {e}"));
        assert_eq!(
            bytes(&p).replace('\r', ""),
            text.replace('\r', ""),
            "{f:?} : relu puis réécrit, le fichier change"
        );
        assert!(!has_link_key(&text), "{f:?}");
    }
}

#[test]
fn les_fixtures_anciennes_gardent_leurs_egalites_sans_extension() {
    let base = concat!(env!("CARGO_MANIFEST_DIR"), "/tests");
    let mut seen = 0;
    for dir in ["fixtures", "fixtures_v3", "fixtures_v4"] {
        for f in read_dir_files(&format!("{base}/{dir}"), ".pforge.json") {
            let text = std::fs::read_to_string(&f).unwrap();
            let p = persist::from_json(&text).unwrap_or_else(|e| panic!("{f:?} : {e}"));
            let out = bytes(&p);
            assert!(!has_link_key(&out), "{f:?}");
            // La réécriture est stable (relire la sortie donne la même sortie).
            assert_eq!(bytes(&persist::from_json(&out).unwrap()), out, "{f:?}");
            seen += p
                .sheets
                .iter()
                .flat_map(|s| &s.length_constraints)
                .filter(|c| c.reference.is_some())
                .count();
        }
    }
    // Le fichier v4 porte une égalité 03.4a (référence 189 + 12) : relue sans extension.
    assert!(seen >= 1, "aucune fixture ne porte d'égalité de longueurs");
    let text = std::fs::read_to_string(format!(
        "{base}/fixtures_v4/v4-gamme-ecarts-variables-formules-pieces.pforge.json"
    ))
    .unwrap();
    let old: Value = serde_json::from_str(&text).unwrap();
    let new: Value = serde_json::from_str(&bytes(&persist::from_json(&text).unwrap())).unwrap();
    let find = |v: &Value| -> Value {
        v["sheets"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| {
                s["length_constraints"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
            })
            .find(|c| c.get("reference").is_some())
            .unwrap()
    };
    assert_eq!(
        find(&old)["reference"],
        json!({"entity": 189, "offset_mm": 12.0})
    );
    assert_eq!(find(&old), find(&new));
}

fn corrupted(p: &Project, edit: impl FnOnce(&mut Value)) -> Result<Project, CoreError> {
    let mut v: Value = serde_json::from_str(&bytes(p)).unwrap();
    edit(&mut v["sheets"][0]["length_constraints"][0]);
    persist::from_json(&serde_json::to_string(&v).unwrap())
}

#[test]
fn aller_retour_octets_d_une_somme_avec_formule_et_fichier_altere_refuse() {
    let mut w = world();
    var(&mut w.p, "embu", "7");
    w.p.apply(impose(w.a, w.c, w.c_end, 150.0)).unwrap();
    w.p.apply(equality(
        w.a,
        w.c,
        vec![term(w.r, Some(w.b)), term(w.l, None)],
        0.0,
        Some("embu"),
    ))
    .unwrap();
    let text = bytes(&w.p);
    assert!(
        text.contains("\"link\"") && text.contains("\"offset_expr\": \"embu\""),
        "{text}"
    );
    let back = persist::from_json(&text).unwrap();
    assert_eq!(bytes(&back), text);
    assert_eq!(back, w.p);
    assert!(corrupted(&w.p, |_| {}).is_ok());
    let (b, r, l) = (w.b.0, w.r.0, w.l.0);
    // Extension vide, feuille de la courbe écrite, tracé inconnu, doublon, champ inconnu.
    assert!(corrupted(&w.p, |v| v["link"] = json!({})).is_err());
    let a = w.a.0;
    assert!(corrupted(&w.p, |v| v["link"]["sheet"] = json!(a)).is_err());
    assert!(corrupted(&w.p, |v| v["link"]["sheet"] = json!(888_888)).is_err());
    assert!(corrupted(&w.p, |v| v["link"]["more"][0]["entity"] = json!(999_999)).is_err());
    assert!(corrupted(&w.p, |v| v["link"]["more"][0]["entity"] = json!(r)).is_err());
    assert!(corrupted(&w.p, |v| v["link"]["extra"] = json!(1)).is_err());
    assert!(corrupted(&w.p, |v| v["link"]["more"][0]["extra"] = json!(1)).is_err());
    // Formule du décalage fausse, vide, ou valeur écrite qui ne la suit pas.
    assert!(corrupted(&w.p, |v| v["link"]["offset_expr"] = json!("embu+1")).is_err());
    assert!(corrupted(&w.p, |v| v["link"]["offset_expr"] = json!("  ")).is_err());
    assert!(corrupted(&w.p, |v| v["reference"]["offset_mm"] = json!(8.0)).is_err());
    // Extension sans égalité.
    assert!(corrupted(&w.p, |v| {
        v.as_object_mut().unwrap().remove("reference");
    })
    .is_err());
    // Le tracé de la somme n'est pas celui de la feuille indiquée.
    assert!(corrupted(&w.p, |v| v["link"]["more"][0]["sheet"] = json!(b)).is_err());
    let _ = l;
}

#[test]
fn ouverture_d_un_cycle_entre_deux_feuilles_refusee() {
    let mut w = world();
    w.p.apply(impose(w.a, w.c, w.c_end, 150.0)).unwrap();
    w.p.apply(equality(w.a, w.c, vec![term(w.d, Some(w.b))], 0.0, None))
        .unwrap();
    // Fichier fabriqué : D (feuille B) = C (feuille A) + 0 ; les deux valent 80.
    let mut v: Value = serde_json::from_str(&bytes(&w.p)).unwrap();
    let mut first = v["sheets"][0]["length_constraints"][0].clone();
    first["curve"] = json!(w.d.0);
    first["adjust"]["point"] = json!(w.d_end.0);
    first["reference"]["entity"] = json!(w.c.0);
    first["link"]["sheet"] = json!(w.a.0);
    v["sheets"][1]["length_constraints"] = json!([first]);
    let e = persist::from_json(&serde_json::to_string(&v).unwrap()).unwrap_err();
    assert!(e.to_string().contains("circulaire"), "{e}");
}

// ---------------------------------------------------------------------------------------------
// Diagnostic et dépendances
// ---------------------------------------------------------------------------------------------

#[test]
fn diagnostic_et_dependances_voient_la_somme_et_la_formule() {
    let mut w = world();
    var(&mut w.p, "embu", "7");
    w.p.apply(impose(w.a, w.c, w.c_end, 150.0)).unwrap();
    w.p.apply(equality(
        w.a,
        w.c,
        vec![term(w.r, Some(w.b)), term(w.l, None)],
        0.0,
        Some("embu"),
    ))
    .unwrap();
    let diags = w.p.link_diagnostics();
    let d = diags
        .iter()
        .find(|d| d.link == LinkType::LengthEquality)
        .expect("égalité diagnostiquée");
    assert_eq!(d.kind, DiagnosticKind::Active);
    assert_eq!(d.text, "embu");
    assert!((d.offset_mm.unwrap() - 7.0).abs() < 1e-9);
    // La variable est utilisée (par le décalage) : pas signalée inutilisée.
    assert!(!diags
        .iter()
        .any(|d| d.kind == DiagnosticKind::UnusedVariable));
    // L'arbre : R (feuille B) est un parent de C.
    let tree =
        w.p.dependency_tree(&DependencyObject::Entity(w.c), Some(1))
            .unwrap();
    let parents: Vec<DependencyObject> = tree.parents.iter().map(|e| e.object.clone()).collect();
    assert!(
        parents.contains(&DependencyObject::Entity(w.r)),
        "{parents:?}"
    );
    assert!(
        parents.contains(&DependencyObject::Entity(w.l)),
        "{parents:?}"
    );
    assert!(parents.contains(&DependencyObject::Variable("embu".into())));
}

// ---------------------------------------------------------------------------------------------
// Schéma JSON publié
// ---------------------------------------------------------------------------------------------

#[path = "format_ouvert/validateur.rs"]
mod validateur;

#[test]
fn le_schema_json_valide_l_extension_et_refuse_une_cle_inconnue() {
    let mut w = world();
    var(&mut w.p, "embu", "7");
    w.p.apply(impose(w.a, w.c, w.c_end, 150.0)).unwrap();
    w.p.apply(equality(
        w.a,
        w.c,
        vec![term(w.r, Some(w.b)), term(w.l, None)],
        0.0,
        Some("embu"),
    ))
    .unwrap();
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../docs/pforge-v6.schema.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let mut doc: Value = serde_json::from_str(&bytes(&w.p)).unwrap();
    assert!(validateur::validate(&schema, &doc).is_empty());
    let c = "length_constraints";
    doc["sheets"][0][c][0]["link"]["extra"] = json!(1);
    assert!(!validateur::validate(&schema, &doc).is_empty());
    // Une extension vide n'est refusée que par le chargeur (le schéma n'a pas de minProperties).
    doc["sheets"][0][c][0]["link"] = json!({});
    assert!(persist::from_json(&doc.to_string()).is_err());
    doc["sheets"][0][c][0]["link"] = json!({"more": [{"sheet": 3}]});
    assert!(!validateur::validate(&schema, &doc).is_empty());
}

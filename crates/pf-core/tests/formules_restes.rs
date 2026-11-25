//! US-03.1 CA 3, restes (tranche 118) — formules sur la distance d'un point sur un segment, la
//! proportion sur un segment, le `t` d'un point sur une courbe, la fraction d'une division, et
//! formule **à la création** (`AddPointWithFormulas`).
//!
//! Les valeurs attendues sont calculées à la main (commentaires). Le segment `S` va de (0 ; 0) à
//! (1000 ; 0). La courbe de Bézier va de (0 ; 0) à (300 ; 0) avec les poignées (0 ; 300) et
//! (300 ; 300) : B(t) = 3(1-t)²t·(0 ; 300) + 3(1-t)t²·(300 ; 300) + t³·(300 ; 0).

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;
use serde_json::{json, Value};

const TOL: f64 = 1e-9;

struct F {
    p: Project,
    s: SheetId,
    seg: EntityId,
    ps: PointId, // OnSegment, 10 mm depuis le départ
    pr: PointId, // OnSegmentRatio, 0,1
    pc: PointId, // OnCurve, t = 0,25
    pd: PointId, // Division, 0,5
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn var(p: &mut Project, name: &str, expr: &str) {
    p.apply(Command::AddVariable {
        name: name.into(),
        unit: VariableUnit::Mm,
        expr: expr.into(),
    })
    .unwrap();
}

fn set_var(name: &str, expr: &str) -> Command {
    Command::SetVariableExpr {
        name: name.into(),
        expr: expr.into(),
    }
}

fn derived(p: &mut Project, s: SheetId, d: PointDefinition) -> PointId {
    p.apply(Command::AddPointAt {
        sheet: s,
        point: PointRef::Derived(d),
    })
    .unwrap()
    .points[0]
}

fn target(s: SheetId, point: PointId, param: ParamKind) -> ParamTarget {
    ParamTarget {
        sheet: s,
        point,
        param,
    }
}

fn bind(p: &mut Project, s: SheetId, point: PointId, param: ParamKind, expr: &str) {
    p.apply(Command::SetParameterExpr {
        target: target(s, point, param),
        expr: Some(expr.into()),
    })
    .unwrap();
}

fn fixture() -> F {
    let mut p = Project::new("Formules restes");
    let s = p.sheets[0].id;
    let seg = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(PointMm::new(0.0, 0.0)),
            to: PointRef::New(PointMm::new(1000.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let curve = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(PointMm::new(0.0, 0.0)),
            c1: PointMm::new(0.0, 300.0),
            c2: PointMm::new(300.0, 300.0),
            end: PointRef::New(PointMm::new(300.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let ps = derived(
        &mut p,
        s,
        PointDefinition::OnSegment {
            segment: seg,
            distance_mm: 10.0,
            origin: SegmentEnd::Start,
        },
    );
    let pr = derived(
        &mut p,
        s,
        PointDefinition::OnSegmentRatio {
            segment: seg,
            ratio: 0.1,
        },
    );
    let pc = derived(&mut p, s, PointDefinition::OnCurve { curve, t: 0.25 });
    let pd = p
        .apply(Command::DivideTrace {
            sheet: s,
            trace: seg,
            spec: DivisionSpec::Equal { parts: 2 },
            start: None,
        })
        .unwrap()
        .points[0];
    F {
        p,
        s,
        seg,
        ps,
        pr,
        pc,
        pd,
    }
}

fn at(p: &Project, s: SheetId, id: PointId, x: f64, y: f64) {
    let q = p.sheet(s).unwrap().position(id).unwrap();
    assert!(
        (q.x - x).abs() < TOL && (q.y - y).abs() < TOL,
        "position ({} ; {}), attendu ({x} ; {y})",
        q.x,
        q.y
    );
}

fn value(p: &Project, s: SheetId, id: PointId, k: ParamKind) -> f64 {
    p.sheet(s).unwrap().parameter_value(id, k).unwrap()
}

fn refused(p: &mut Project, c: Command) -> String {
    let before = bytes(p);
    let e = p.apply(c).expect_err("devrait être refusé").to_string();
    assert_eq!(bytes(p), before, "refus : le projet doit rester identique");
    e
}

// ---------------------------------------------------------------------------------------------
// CA 3 : les quatre paramètres sont liables et maintenus
// ---------------------------------------------------------------------------------------------

#[test]
fn distance_sur_segment_formule_maintenue() {
    let mut f = fixture();
    var(&mut f.p, "L", "250");
    bind(&mut f.p, f.s, f.ps, ParamKind::SegmentDistance, "L/2+10");
    // 250 / 2 + 10 = 135
    assert!((value(&f.p, f.s, f.ps, ParamKind::SegmentDistance) - 135.0).abs() < TOL);
    at(&f.p, f.s, f.ps, 135.0, 0.0);
    // Une seule étape annulable pour la variable ; 300 / 2 + 10 = 160.
    let mut doc = Document::new(f.p.clone());
    let before = bytes(doc.project());
    doc.execute(set_var("L", "300")).unwrap();
    at(doc.project(), f.s, f.ps, 160.0, 0.0);
    assert!(doc.undo() && !doc.undo());
    assert_eq!(bytes(doc.project()), before);
}

#[test]
fn proportion_sur_segment_formule_maintenue() {
    let mut f = fixture();
    var(&mut f.p, "p", "0,6");
    bind(&mut f.p, f.s, f.pr, ParamKind::SegmentRatio, "p/2");
    // 0,6 / 2 = 0,3 -> 1000 x 0,3 = 300
    at(&f.p, f.s, f.pr, 300.0, 0.0);
    f.p.apply(set_var("p", "0,8")).unwrap();
    // 0,8 / 2 = 0,4 -> 400
    at(&f.p, f.s, f.pr, 400.0, 0.0);
    // Fraction littérale : 1/4 -> 250.
    bind(&mut f.p, f.s, f.pr, ParamKind::SegmentRatio, "1/4");
    at(&f.p, f.s, f.pr, 250.0, 0.0);
}

#[test]
fn t_sur_courbe_formule_maintenue() {
    let mut f = fixture();
    var(&mut f.p, "q", "1");
    bind(&mut f.p, f.s, f.pc, ParamKind::CurveRatio, "q/2");
    // t = 0,5 : x = 0,375 x 0 + 0,375 x 300 + 0,125 x 300 = 150 ; y = (0,375 + 0,375) x 300 = 225.
    at(&f.p, f.s, f.pc, 150.0, 225.0);
    f.p.apply(set_var("q", "0,5")).unwrap();
    // t = 0,25 : b1 = 0,421875, b2 = 0,140625, b3 = 0,015625.
    // x = 0,140625 x 300 + 0,015625 x 300 = 46,875 ; y = (0,421875 + 0,140625) x 300 = 168,75.
    at(&f.p, f.s, f.pc, 46.875, 168.75);
}

#[test]
fn fraction_de_division_formule_maintenue() {
    let mut f = fixture();
    var(&mut f.p, "D", "800");
    bind(&mut f.p, f.s, f.pd, ParamKind::DivisionRatio, "D/1000");
    // 800 / 1000 = 0,8 -> 800 mm sur un segment de 1000.
    at(&f.p, f.s, f.pd, 800.0, 0.0);
    f.p.apply(set_var("D", "250")).unwrap();
    at(&f.p, f.s, f.pd, 250.0, 0.0);
    // Les bornes sont exclues : 0 et 1 refusés, projet identique.
    let e = refused(&mut f.p, set_var("D", "0"));
    assert!(e.contains("division_ratio"), "{e}");
    refused(&mut f.p, set_var("D", "1000"));
    assert_eq!(value(&f.p, f.s, f.pd, ParamKind::DivisionRatio), 0.25);
}

#[test]
fn valeur_hors_plage_refusee_sans_trace() {
    let mut f = fixture();
    var(&mut f.p, "L", "250");
    // 250 x 10 = 2500 > 1000 mm : refusé à la liaison.
    refused(
        &mut f.p,
        Command::SetParameterExpr {
            target: target(f.s, f.ps, ParamKind::SegmentDistance),
            expr: Some("L*10".into()),
        },
    );
    // Proportion 3/2 > 1 ; proportion négative ; t > 1.
    for (point, kind, text) in [
        (f.pr, ParamKind::SegmentRatio, "3/2"),
        (f.pr, ParamKind::SegmentRatio, "0-1/2"),
        (f.pc, ParamKind::CurveRatio, "L/100"),
    ] {
        refused(
            &mut f.p,
            Command::SetParameterExpr {
                target: target(f.s, point, kind),
                expr: Some(text.into()),
            },
        );
    }
    // Une variable qui rend une formule impossible est refusée, la variable ne change pas.
    bind(&mut f.p, f.s, f.ps, ParamKind::SegmentDistance, "L*3");
    at(&f.p, f.s, f.ps, 750.0, 0.0);
    let e = refused(&mut f.p, set_var("L", "400"));
    assert!(e.contains("segment_distance") || e.contains("1200"), "{e}");
    assert_eq!(f.p.variables[0].expr, "250");
}

// ---------------------------------------------------------------------------------------------
// Mécanique 03.1b : délier, remarques, suppression, renommage
// ---------------------------------------------------------------------------------------------

#[test]
fn valeur_modifiee_directement_delie_avec_remarque() {
    let mut f = fixture();
    var(&mut f.p, "L", "250");
    bind(&mut f.p, f.s, f.ps, ParamKind::SegmentDistance, "L/2");
    bind(&mut f.p, f.s, f.pr, ParamKind::SegmentRatio, "L/1000");
    bind(&mut f.p, f.s, f.pc, ParamKind::CurveRatio, "L/1000");
    bind(&mut f.p, f.s, f.pd, ParamKind::DivisionRatio, "L/500");
    let out =
        f.p.apply(Command::SetPointDistance {
            sheet: f.s,
            point: f.ps,
            distance_mm: 77.0,
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("déliée")),
        "{:?}",
        out.notes
    );
    assert!(f
        .p
        .sheet(f.s)
        .unwrap()
        .formula(f.ps, ParamKind::SegmentDistance)
        .is_none());
    assert_eq!(value(&f.p, f.s, f.ps, ParamKind::SegmentDistance), 77.0);
    for (point, kind) in [
        (f.pr, ParamKind::SegmentRatio),
        (f.pc, ParamKind::CurveRatio),
        (f.pd, ParamKind::DivisionRatio),
    ] {
        let out =
            f.p.apply(Command::SetPointParameter {
                sheet: f.s,
                point,
                value: 0.4,
            })
            .unwrap();
        assert!(out.notes.iter().any(|n| n.contains("déliée")), "{kind:?}");
        assert!(f.p.sheet(f.s).unwrap().formula(point, kind).is_none());
        assert_eq!(value(&f.p, f.s, point, kind), 0.4);
    }
    assert!(f.p.sheet(f.s).unwrap().formulas.is_empty());
    // Réécrire la même valeur sur un paramètre lié garde le lien et ne crée aucune étape.
    bind(&mut f.p, f.s, f.ps, ParamKind::SegmentDistance, "L/2");
    let mut doc = Document::new(f.p.clone());
    doc.execute(Command::SetPointDistance {
        sheet: f.s,
        point: f.ps,
        distance_mm: 125.0,
    })
    .unwrap();
    assert!(!doc.can_undo());
    assert!(doc
        .project()
        .sheet(f.s)
        .unwrap()
        .formula(f.ps, ParamKind::SegmentDistance)
        .is_some());
}

#[test]
fn fraction_de_division_modifiable_directement() {
    let mut f = fixture();
    f.p.apply(Command::SetPointParameter {
        sheet: f.s,
        point: f.pd,
        value: 0.25,
    })
    .unwrap();
    at(&f.p, f.s, f.pd, 250.0, 0.0);
    for bad in [0.0, 1.0, -0.1, 1.5] {
        refused(
            &mut f.p,
            Command::SetPointParameter {
                sheet: f.s,
                point: f.pd,
                value: bad,
            },
        );
    }
}

#[test]
fn variable_utilisee_non_supprimable_renommage_reecrit() {
    let mut f = fixture();
    var(&mut f.p, "L", "250");
    bind(&mut f.p, f.s, f.ps, ParamKind::SegmentDistance, "L/2+10");
    bind(&mut f.p, f.s, f.pd, ParamKind::DivisionRatio, "L/500");
    let e = refused(&mut f.p, Command::RemoveVariable { name: "L".into() });
    assert!(
        e.contains("segment_distance") && e.contains("division_ratio"),
        "{e}"
    );
    f.p.apply(Command::RenameVariable {
        from: "L".into(),
        to: "Long".into(),
    })
    .unwrap();
    let sheet = f.p.sheet(f.s).unwrap();
    assert_eq!(
        sheet
            .formula(f.ps, ParamKind::SegmentDistance)
            .unwrap()
            .expr,
        "Long/2+10"
    );
    assert_eq!(
        sheet.formula(f.pd, ParamKind::DivisionRatio).unwrap().expr,
        "Long/500"
    );
    f.p.apply(set_var("Long", "300")).unwrap();
    at(&f.p, f.s, f.ps, 160.0, 0.0);
    // 300 / 500 = 0,6 -> 600
    at(&f.p, f.s, f.pd, 600.0, 0.0);
}

#[test]
fn point_supprime_la_formule_est_retiree_avec_remarque() {
    let mut f = fixture();
    var(&mut f.p, "L", "250");
    bind(&mut f.p, f.s, f.pr, ParamKind::SegmentRatio, "L/1000");
    let out =
        f.p.apply(Command::Delete {
            sheet: f.s,
            points: vec![f.pr],
            entities: vec![],
        })
        .unwrap();
    assert!(
        out.notes.iter().any(|n| n.contains("retirée")),
        "{:?}",
        out.notes
    );
    assert!(f.p.sheet(f.s).unwrap().formulas.is_empty());
    f.p.validate().unwrap();
}

#[test]
fn copie_de_feuille_le_lien_suit() {
    let mut f = fixture();
    var(&mut f.p, "L", "250");
    bind(&mut f.p, f.s, f.pr, ParamKind::SegmentRatio, "L/1000");
    let out = f.p.apply(Command::DuplicateSheet { sheet: f.s }).unwrap();
    let copy = out.sheet.unwrap();
    let cs = f.p.sheet(copy).unwrap();
    assert_eq!(cs.formulas.len(), 1);
    let np = cs.formulas[0].point;
    assert_ne!(np, f.pr);
    f.p.apply(set_var("L", "400")).unwrap();
    // 400 / 1000 = 0,4 -> 400 mm sur les deux feuilles.
    at(&f.p, f.s, f.pr, 400.0, 0.0);
    at(&f.p, copy, np, 400.0, 0.0);
}

// ---------------------------------------------------------------------------------------------
// Formule à la création : AddPointWithFormulas
// ---------------------------------------------------------------------------------------------

fn on_segment(segment: EntityId, origin: SegmentEnd) -> PointDefinition {
    PointDefinition::OnSegment {
        segment,
        distance_mm: 1.0, // valeur provisoire : la formule la remplace
        origin,
    }
}

fn expr(param: ParamKind, text: &str) -> ParamExpr {
    ParamExpr {
        param,
        expr: text.into(),
    }
}

#[test]
fn creation_avec_formule_une_etape_et_lien_pose() {
    let mut f = fixture();
    var(&mut f.p, "L", "250");
    let mut doc = Document::new(f.p.clone());
    let before = bytes(doc.project());
    let out = doc
        .execute(Command::AddPointWithFormulas {
            sheet: f.s,
            definition: on_segment(f.seg, SegmentEnd::Start),
            formulas: vec![expr(ParamKind::SegmentDistance, " L/2+10 ")],
        })
        .unwrap();
    let id = out.points[0];
    // 250 / 2 + 10 = 135 ; le texte est rogné.
    at(doc.project(), f.s, id, 135.0, 0.0);
    let sheet = doc.project().sheet(f.s).unwrap();
    assert_eq!(
        sheet.formula(id, ParamKind::SegmentDistance).unwrap().expr,
        "L/2+10"
    );
    // Maintenu : la variable déplace le point.
    doc.execute(set_var("L", "300")).unwrap();
    at(doc.project(), f.s, id, 160.0, 0.0);
    // Une seule étape pour la création : deux annulations ramènent à l'état d'avant.
    assert!(doc.undo() && doc.undo() && !doc.undo());
    assert_eq!(bytes(doc.project()), before);
}

#[test]
fn creation_depuis_l_arrivee_et_deux_formules() {
    let mut f = fixture();
    var(&mut f.p, "L", "250");
    var(&mut f.p, "A", "90");
    let id =
        f.p.apply(Command::AddPointWithFormulas {
            sheet: f.s,
            definition: on_segment(f.seg, SegmentEnd::End),
            formulas: vec![expr(ParamKind::SegmentDistance, "L/2+10")],
        })
        .unwrap()
        .points[0];
    // Depuis l'arrivée : 1000 - 135 = 865.
    at(&f.p, f.s, id, 865.0, 0.0);
    let free =
        f.p.apply(Command::AddPoint {
            sheet: f.s,
            at: PointMm::new(500.0, 500.0),
        })
        .unwrap()
        .points[0];
    let pol =
        f.p.apply(Command::AddPointWithFormulas {
            sheet: f.s,
            definition: PointDefinition::Polar {
                from: free,
                distance_mm: 1.0,
                angle_deg: 1.0,
                reference: None,
            },
            formulas: vec![
                expr(ParamKind::Distance, "L/2"),
                expr(ParamKind::Angle, "A"),
            ],
        })
        .unwrap()
        .points[0];
    // 250 / 2 = 125 à 90 degres : (500 ; 625).
    at(&f.p, f.s, pol, 500.0, 625.0);
    f.p.apply(set_var("A", "0")).unwrap();
    at(&f.p, f.s, pol, 625.0, 500.0);
    // Un point de division et une proportion se créent aussi avec leur formule.
    let pr =
        f.p.apply(Command::AddPointWithFormulas {
            sheet: f.s,
            definition: PointDefinition::OnSegmentRatio {
                segment: f.seg,
                ratio: 0.9,
            },
            formulas: vec![expr(ParamKind::SegmentRatio, "L/1000")],
        })
        .unwrap()
        .points[0];
    // 250 / 1000 = 0,25 -> 250.
    at(&f.p, f.s, pr, 250.0, 0.0);
    f.p.validate().unwrap();
}

#[test]
fn creation_refus_sans_trace() {
    let mut f = fixture();
    var(&mut f.p, "L", "250");
    let cases: Vec<(Vec<ParamExpr>, &str)> = vec![
        (vec![expr(ParamKind::SegmentDistance, "  ")], "vide"),
        (vec![expr(ParamKind::SegmentDistance, "Z")], "inconnue"),
        (vec![expr(ParamKind::SegmentDistance, "L/0")], "division"),
        (vec![expr(ParamKind::SegmentDistance, "L*10")], "segment"),
        (
            vec![
                expr(ParamKind::SegmentDistance, "L"),
                expr(ParamKind::SegmentDistance, "L+1"),
            ],
            "deux formules",
        ),
        (vec![expr(ParamKind::RadiusA, "L")], "radius_a"),
    ];
    for (formulas, needle) in cases {
        let e = refused(
            &mut f.p,
            Command::AddPointWithFormulas {
                sheet: f.s,
                definition: on_segment(f.seg, SegmentEnd::Start),
                formulas,
            },
        );
        assert!(e.to_lowercase().contains(needle), "{needle} : {e}");
    }
    // Un point existe déjà à cet endroit (le point 10 mm, via 10 = L/25) : rien à maintenir.
    let e = refused(
        &mut f.p,
        Command::AddPointWithFormulas {
            sheet: f.s,
            definition: on_segment(f.seg, SegmentEnd::Start),
            formulas: vec![expr(ParamKind::SegmentDistance, "L/25")],
        },
    );
    assert!(e.contains("existe déjà"), "{e}");
    // Feuille inconnue.
    refused(
        &mut f.p,
        Command::AddPointWithFormulas {
            sheet: SheetId(999_999),
            definition: on_segment(f.seg, SegmentEnd::Start),
            formulas: vec![expr(ParamKind::SegmentDistance, "L")],
        },
    );
}

// ---------------------------------------------------------------------------------------------
// Tailles et fichiers
// ---------------------------------------------------------------------------------------------

#[test]
fn valeurs_par_taille_pilotent_les_nouveaux_parametres() {
    let mut f = fixture();
    var(&mut f.p, "L", "250");
    var(&mut f.p, "p", "0,4");
    bind(&mut f.p, f.s, f.ps, ParamKind::SegmentDistance, "L/2+10");
    bind(&mut f.p, f.s, f.pr, ParamKind::SegmentRatio, "p/2");
    f.p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: vec!["S".into(), "M".into(), "L".into()],
        base: 1,
    })
    .unwrap();
    let small = f.p.sizes[0].sizes[0].id;
    for (name, expr) in [("L", "100"), ("p", "0,2")] {
        f.p.apply(Command::SetVariableSizeExpr {
            name: name.into(),
            size: small,
            expr: expr.into(),
        })
        .unwrap();
    }
    let r = f.p.derived_for_size(f.s, Some(small)).unwrap();
    let pos = |id: PointId| r.points.iter().find(|(p, _)| *p == id).unwrap().1;
    // Taille S : 100 / 2 + 10 = 60 ; 0,2 / 2 = 0,1 -> 100 mm.
    assert!((pos(f.ps).x - 60.0).abs() < TOL);
    assert!((pos(f.pr).x - 100.0).abs() < TOL);
    // Taille de base : 135 et 0,2 -> 200.
    at(&f.p, f.s, f.ps, 135.0, 0.0);
    at(&f.p, f.s, f.pr, 200.0, 0.0);
}

fn all_bound() -> F {
    let mut f = fixture();
    var(&mut f.p, "L", "250");
    var(&mut f.p, "p", "0,6");
    bind(&mut f.p, f.s, f.ps, ParamKind::SegmentDistance, "L/2+10");
    bind(&mut f.p, f.s, f.pr, ParamKind::SegmentRatio, "p/2");
    bind(&mut f.p, f.s, f.pc, ParamKind::CurveRatio, "p/2");
    bind(&mut f.p, f.s, f.pd, ParamKind::DivisionRatio, "p");
    f
}

fn open_err(v: &Value) -> String {
    persist::from_json(&serde_json::to_string(v).unwrap())
        .expect_err("devrait être refusé")
        .to_string()
}

fn point_index(v: &Value, id: PointId) -> usize {
    v["sheets"][0]["points"]
        .as_array()
        .unwrap()
        .iter()
        .position(|p| p["id"] == json!(id.0))
        .unwrap()
}

#[test]
fn fichier_relu_a_l_identique_et_noms_des_parametres() {
    let f = all_bound();
    let text = bytes(&f.p);
    assert_eq!(bytes(&persist::from_json(&text).unwrap()), text);
    let v: Value = serde_json::from_str(&text).unwrap();
    let params: Vec<&str> = v["sheets"][0]["formulas"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["param"].as_str().unwrap())
        .collect();
    assert_eq!(
        params,
        [
            "segment_distance",
            "segment_ratio",
            "curve_ratio",
            "division_ratio"
        ]
    );
    // Un projet sans formule ne porte aucune clé `formulas` (octets inchangés).
    assert!(!bytes(&fixture().p).contains("formulas"));
}

#[test]
fn ouverture_detecte_l_ecart_valeur_formule() {
    let f = all_bound();
    let ok: Value = serde_json::from_str(&bytes(&f.p)).unwrap();
    // Valeur écrite modifiée à la main : fichier refusé, message qui nomme le paramètre.
    let mut v = ok.clone();
    let i = point_index(&v, f.pr);
    v["sheets"][0]["points"][i]["definition"]["ratio"] = json!(0.31);
    let e = open_err(&v);
    assert!(
        e.contains("ne correspond pas") && e.contains("segment_ratio"),
        "{e}"
    );
    let mut v = ok.clone();
    let i = point_index(&v, f.ps);
    v["sheets"][0]["points"][i]["definition"]["distance_mm"] = json!(136.0);
    assert!(open_err(&v).contains("segment_distance"));
    let mut v = ok.clone();
    let i = point_index(&v, f.pc);
    v["sheets"][0]["points"][i]["definition"]["t"] = json!(0.5);
    assert!(open_err(&v).contains("curve_ratio"));
    // Tolérance 1e-9 : 5e-10 passe, 2e-9 non.
    let mut v = ok.clone();
    let i = point_index(&v, f.pr);
    v["sheets"][0]["points"][i]["definition"]["ratio"] = json!(0.3 + 5e-10);
    persist::from_json(&serde_json::to_string(&v).unwrap()).unwrap();
    v["sheets"][0]["points"][i]["definition"]["ratio"] = json!(0.3 + 2e-9);
    assert!(open_err(&v).contains("ne correspond pas"));
    // Formule sur un paramètre que le point n'a pas : refusée.
    let mut v = ok.clone();
    v["sheets"][0]["formulas"][0]["param"] = json!("segment_ratio");
    assert!(!open_err(&v).is_empty());
    // Nom de paramètre inconnu : refusé à la lecture.
    let mut v = ok;
    v["sheets"][0]["formulas"][0]["param"] = json!("ratio_inconnu");
    assert!(!open_err(&v).is_empty());
}

fn find_param_enum(v: &Value) -> Option<Vec<String>> {
    match v {
        Value::Object(o) => {
            if let Some(e) = o
                .get("properties")
                .and_then(|p| p.get("param"))
                .and_then(|p| p.get("enum"))
            {
                return Some(
                    e.as_array()
                        .unwrap()
                        .iter()
                        .map(|x| x.as_str().unwrap().to_string())
                        .collect(),
                );
            }
            o.values().find_map(find_param_enum)
        }
        Value::Array(a) => a.iter().find_map(find_param_enum),
        _ => None,
    }
}

#[test]
fn schema_json_liste_les_douze_parametres() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/pforge-v6.schema.json");
    let schema: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut got = find_param_enum(&schema).expect("énumération des paramètres");
    got.sort();
    let mut want: Vec<String> = [
        "distance",
        "angle",
        "dx",
        "dy",
        "along_distance",
        "bisector_distance",
        "radius_a",
        "radius_b",
        "segment_distance",
        "segment_ratio",
        "curve_ratio",
        "division_ratio",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    want.sort();
    assert_eq!(got, want);
}

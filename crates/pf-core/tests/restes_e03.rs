//! Restes d'E03 (tranche 107) : US-03.1 CA 3 (quatre paramètres de plus acceptent une formule :
//! distance sur un tracé, distance sur une bissectrice, rayons du compas) et US-03.2b (rectangle
//! créé avec ses constructions, optionnellement maintenu).
//!
//! Toutes les valeurs attendues sont calculées à la main (commentaires) ; un refus laisse le
//! projet identique à l'octet ; un geste = une étape annulable.

use pf_core::persist;
use pf_core::*;
use pf_geometry::{PointMm, Side};

const TOL: f64 = 1e-9;

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

fn free(p: &mut Project, s: SheetId, x: f64, y: f64) -> PointId {
    p.apply(Command::AddPoint {
        sheet: s,
        at: PointMm::new(x, y),
    })
    .unwrap()
    .points[0]
}

fn derived(p: &mut Project, s: SheetId, d: PointDefinition) -> PointId {
    p.apply(Command::AddPointAt {
        sheet: s,
        point: PointRef::Derived(d),
    })
    .unwrap()
    .points[0]
}

fn bind(s: SheetId, point: PointId, param: ParamKind, expr: &str) -> Command {
    Command::SetParameterExpr {
        target: ParamTarget {
            sheet: s,
            point,
            param,
        },
        expr: Some(expr.into()),
    }
}

fn pos(p: &Project, s: SheetId, id: PointId) -> PointMm {
    p.sheet(s).unwrap().position(id).unwrap()
}

fn at(p: &Project, s: SheetId, id: PointId, x: f64, y: f64) {
    let q = pos(p, s, id);
    assert!(
        (q.x - x).abs() < TOL && (q.y - y).abs() < TOL,
        "position {q:?}, attendu ({x} ; {y})"
    );
}

fn value(p: &Project, s: SheetId, id: PointId, k: ParamKind) -> f64 {
    p.sheet(s).unwrap().parameter_value(id, k).unwrap()
}

fn refused(p: &mut Project, cmd: Command) -> String {
    let before = bytes(p);
    let before_p = p.clone();
    let e = p.apply(cmd).expect_err("devrait être refusée").to_string();
    assert_eq!(bytes(p), before, "trace laissée par un refus : {e}");
    assert_eq!(*p, before_p);
    e
}

// ---------------------------------------------------------------------------------------------
// US-03.1 CA 3 : quatre paramètres de plus
// ---------------------------------------------------------------------------------------------

#[test]
fn distance_sur_un_trace_suit_la_variable() {
    let mut p = Project::new("Along");
    let s = p.sheets[0].id;
    var(&mut p, "a", "20");
    let seg = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(PointMm::new(0.0, 0.0)),
            to: PointRef::New(PointMm::new(100.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let q = derived(
        &mut p,
        s,
        PointDefinition::AlongCurve {
            curve: seg,
            distance_mm: 30.0,
            origin: SegmentEnd::Start,
        },
    );
    at(&p, s, q, 30.0, 0.0);
    // a = 20 : 20 + 15 = 35.
    p.apply(bind(s, q, ParamKind::AlongDistance, "a+15"))
        .unwrap();
    assert_eq!(value(&p, s, q, ParamKind::AlongDistance), 35.0);
    at(&p, s, q, 35.0, 0.0);
    let mut doc = Document::new(p);
    let before = bytes(doc.project());
    // a = 30 : 45, dans la même étape que la variable.
    doc.execute(set_var("a", "30")).unwrap();
    at(doc.project(), s, q, 45.0, 0.0);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before);
    assert!(!doc.undo() || doc.project().variables.len() == 1);
}

#[test]
fn distance_sur_une_bissectrice() {
    let mut p = Project::new("Bissectrice");
    let s = p.sheets[0].id;
    var(&mut p, "d", "25");
    let v = free(&mut p, s, 0.0, 0.0);
    let f = free(&mut p, s, 100.0, 0.0);
    let g = free(&mut p, s, 0.0, 100.0);
    let b = derived(
        &mut p,
        s,
        PointDefinition::Bisector {
            vertex: v,
            first: f,
            second: g,
            distance_mm: 10.0,
        },
    );
    // d = 25 : 2 * 25 = 50 ; direction (1 ; 1) / racine(2) : 50 / racine(2) = 35,35533905932738.
    p.apply(bind(s, b, ParamKind::BisectorDistance, "d*2"))
        .unwrap();
    at(&p, s, b, 35.35533905932738, 35.35533905932738);
    // d = 10 : 20 mm, soit 14,142135623730951 sur chaque axe.
    p.apply(set_var("d", "10")).unwrap();
    at(&p, s, b, 14.142135623730951, 14.142135623730951);
}

#[test]
fn rayons_du_compas_et_refus_hors_portee() {
    let mut p = Project::new("Compas");
    let s = p.sheets[0].id;
    var(&mut p, "r", "60");
    let a = free(&mut p, s, 0.0, 0.0);
    let b = free(&mut p, s, 100.0, 0.0);
    let c = derived(
        &mut p,
        s,
        PointDefinition::Compass {
            center_a: a,
            center_b: b,
            radius_a_mm: 10.0,
            radius_b_mm: 95.0,
            near: AimMm { x: 36.0, y: 48.0 },
        },
    );
    p.apply(bind(s, c, ParamKind::RadiusA, "r")).unwrap();
    p.apply(bind(s, c, ParamKind::RadiusB, "r+20")).unwrap();
    // r = 60 : rayons 60 et 80 ; x = (100² + 60² − 80²) / 200 = 36 ; y = racine(3600 − 1296) = 48.
    at(&p, s, c, 36.0, 48.0);
    // r = 50 : rayons 50 et 70 ; x = (10000 + 2500 − 4900) / 200 = 38 ; y = racine(1056).
    p.apply(set_var("r", "50")).unwrap();
    at(&p, s, c, 38.0, 32.49615361854384);
    // r = 10 : rayons 10 et 30, les cercles ne se coupent plus (10 + 30 < 100) : refus sans trace.
    let e = refused(&mut p, set_var("r", "10"));
    assert!(!e.is_empty());
    at(&p, s, c, 38.0, 32.49615361854384);
}

#[test]
fn delier_et_valeur_directe_pour_les_nouveaux_parametres() {
    let mut p = Project::new("Delier");
    let s = p.sheets[0].id;
    var(&mut p, "r", "60");
    let a = free(&mut p, s, 0.0, 0.0);
    let b = free(&mut p, s, 100.0, 0.0);
    let c = derived(
        &mut p,
        s,
        PointDefinition::Compass {
            center_a: a,
            center_b: b,
            radius_a_mm: 60.0,
            radius_b_mm: 80.0,
            near: AimMm { x: 36.0, y: 48.0 },
        },
    );
    p.apply(bind(s, c, ParamKind::RadiusA, "r")).unwrap();
    // Valeur écrite directement : la formule est déliée avec remarque, la valeur est gardée.
    let o = p
        .apply(Command::SetPointValues {
            sheet: s,
            point: c,
            values: vec![50.0, 70.0],
        })
        .unwrap();
    assert!(!o.notes.is_empty(), "remarque attendue : {:?}", o.notes);
    assert!(p
        .parameter_formula(ParamTarget {
            sheet: s,
            point: c,
            param: ParamKind::RadiusA
        })
        .is_none());
    at(&p, s, c, 38.0, 32.49615361854384);
    // Un paramètre qui n'existe pas sur ce type de point est refusé.
    let e = refused(&mut p, bind(s, c, ParamKind::AlongDistance, "5"));
    assert!(e.contains("paramètre"), "{e}");
}

#[test]
fn formule_non_evaluable_refusee_sans_trace_pour_un_nouveau_parametre() {
    let mut p = Project::new("Refus");
    let s = p.sheets[0].id;
    let v = free(&mut p, s, 0.0, 0.0);
    let f = free(&mut p, s, 100.0, 0.0);
    let g = free(&mut p, s, 0.0, 100.0);
    let b = derived(
        &mut p,
        s,
        PointDefinition::Bisector {
            vertex: v,
            first: f,
            second: g,
            distance_mm: 10.0,
        },
    );
    refused(
        &mut p,
        bind(s, b, ParamKind::BisectorDistance, "inconnue*2"),
    );
    refused(&mut p, bind(s, b, ParamKind::BisectorDistance, "   "));
}

// ---------------------------------------------------------------------------------------------
// US-03.2b : rectangle maintenu
// ---------------------------------------------------------------------------------------------

fn rect(
    p: &mut Project,
    s: SheetId,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    kept: bool,
) -> Result<Outcome, CoreError> {
    p.apply(Command::AddConstructedRectangle {
        sheet: s,
        corner: PointRef::New(PointMm::new(x, y)),
        width_mm: w,
        height_mm: h,
        kept,
    })
}

fn construction(p: &Project, s: SheetId, e: EntityId) -> Option<Construction> {
    match p.sheet(s).unwrap().entity(e).unwrap().kind {
        EntityKind::Segment { construction, .. } => construction,
        _ => panic!("pas un segment"),
    }
}

#[test]
fn rectangle_maintenu_positions_et_constructions_a_la_main() {
    let mut p = Project::new("Rect");
    let s = p.sheets[0].id;
    // Coin (10 ; 20), 100 × 60 : (110 ; 20), (110 ; 80), (10 ; 80).
    let o = rect(&mut p, s, 10.0, 20.0, 100.0, 60.0, true).unwrap();
    assert_eq!((o.points.len(), o.entities.len()), (4, 4));
    let [p0, p1, p3, p2] = o.points[..] else {
        panic!("4 points")
    };
    at(&p, s, p0, 10.0, 20.0);
    at(&p, s, p1, 110.0, 20.0);
    at(&p, s, p2, 110.0, 80.0);
    at(&p, s, p3, 10.0, 80.0);
    assert_eq!(
        construction(&p, s, o.entities[0]),
        Some(Construction::Polar {
            length_mm: 100.0,
            angle_deg: 0.0,
            kept: true
        })
    );
    assert_eq!(construction(&p, s, o.entities[1]), None);
    assert_eq!(
        construction(&p, s, o.entities[2]),
        Some(Construction::Parallel {
            reference: o.entities[0],
            distance_mm: 60.0,
            side: Side::Left,
            kept: true
        })
    );
    assert_eq!(construction(&p, s, o.entities[3]), None);
    let sheet = p.sheet(s).unwrap();
    assert!(sheet.is_kept_segment(o.entities[0]) && sheet.is_kept_segment(o.entities[2]));
}

#[test]
fn deplacer_le_coin_deplace_le_rectangle_en_une_etape() {
    let mut p = Project::new("Rect");
    let s = p.sheets[0].id;
    let o = rect(&mut p, s, 10.0, 20.0, 100.0, 60.0, true).unwrap();
    let [p0, p1, p3, p2] = o.points[..] else {
        panic!()
    };
    let mut doc = Document::new(p);
    let before = bytes(doc.project());
    doc.execute(Command::MovePoint {
        sheet: s,
        point: p0,
        to: PointMm::new(30.0, 5.0),
    })
    .unwrap();
    // Le rectangle garde 100 × 60 : (130 ; 5), (130 ; 65), (30 ; 65).
    let q = doc.project();
    at(q, s, p0, 30.0, 5.0);
    at(q, s, p1, 130.0, 5.0);
    at(q, s, p2, 130.0, 65.0);
    at(q, s, p3, 30.0, 65.0);
    assert!(doc.undo());
    assert_eq!(bytes(doc.project()), before, "une seule étape");
    assert!(!doc.undo() || doc.project().sheets[0].points.is_empty());
}

#[test]
fn signes_de_la_largeur_et_de_la_hauteur() {
    let cases = [
        // (w, h, p1, p2, p3) depuis (0 ; 0)
        (80.0, -50.0, (80.0, 0.0), (80.0, -50.0), (0.0, -50.0)),
        (-80.0, 50.0, (-80.0, 0.0), (-80.0, 50.0), (0.0, 50.0)),
        (-80.0, -50.0, (-80.0, 0.0), (-80.0, -50.0), (0.0, -50.0)),
    ];
    for (w, h, a, b, c) in cases {
        let mut p = Project::new("Signes");
        let s = p.sheets[0].id;
        let o = rect(&mut p, s, 0.0, 0.0, w, h, true).unwrap();
        let [_, p1, p3, p2] = o.points[..] else {
            panic!()
        };
        at(&p, s, p1, a.0, a.1);
        at(&p, s, p2, b.0, b.1);
        at(&p, s, p3, c.0, c.1);
        // Les mêmes positions que l'ancien rectangle.
        let mut q = Project::new("Ancien");
        let sq = q.sheets[0].id;
        let oq = q
            .apply(Command::AddRectangle {
                sheet: sq,
                corner: PointRef::New(PointMm::new(0.0, 0.0)),
                width_mm: w,
                height_mm: h,
            })
            .unwrap();
        for (i, j) in [(1, 1), (2, 3), (3, 2)] {
            let (x, y) = (pos(&q, sq, oq.points[i]), pos(&p, s, o.points[j]));
            assert!((x.x - y.x).abs() < TOL && (x.y - y.y).abs() < TOL);
        }
        // Déplacer le coin garde la forme.
        p.apply(Command::MovePoint {
            sheet: s,
            point: o.points[0],
            to: PointMm::new(7.0, 9.0),
        })
        .unwrap();
        at(&p, s, p2, 7.0 + b.0, 9.0 + b.1);
    }
}

#[test]
fn non_maintenu_memorise_seulement_et_cote_lie_ensuite() {
    let mut p = Project::new("Rect");
    let s = p.sheets[0].id;
    let o = rect(&mut p, s, 0.0, 0.0, 40.0, 30.0, false).unwrap();
    let [p0, p1, ..] = o.points[..] else { panic!() };
    // Non maintenu : déplacer le coin ne déplace que lui.
    p.apply(Command::MovePoint {
        sheet: s,
        point: p0,
        to: PointMm::new(5.0, 5.0),
    })
    .unwrap();
    at(&p, s, p1, 40.0, 0.0);
    // Le côté de départ ne correspond plus à sa construction : lier est refusé (03.2a).
    let e = refused(
        &mut p,
        Command::SetConstructionKept {
            sheet: s,
            segment: o.entities[0],
            kept: true,
        },
    );
    assert!(e.contains("ne correspond plus"), "{e}");
    // Un rectangle intact se lie ensuite, côté par côté.
    let mut q = Project::new("Rect2");
    let sq = q.sheets[0].id;
    let oq = rect(&mut q, sq, 0.0, 0.0, 40.0, 30.0, false).unwrap();
    for e in [oq.entities[0], oq.entities[2]] {
        q.apply(Command::SetConstructionKept {
            sheet: sq,
            segment: e,
            kept: true,
        })
        .unwrap();
    }
    q.apply(Command::MovePoint {
        sheet: sq,
        point: oq.points[0],
        to: PointMm::new(1.0, 2.0),
    })
    .unwrap();
    at(&q, sq, oq.points[2], 1.0, 32.0);
}

#[test]
fn deplacer_directement_un_coin_suivi_delie_avec_remarque() {
    let mut p = Project::new("Rect");
    let s = p.sheets[0].id;
    let o = rect(&mut p, s, 0.0, 0.0, 100.0, 60.0, true).unwrap();
    let [p0, p1, p3, p2] = o.points[..] else {
        panic!()
    };
    // Déplacer p1 : le côté de départ est délié ; la parallèle suit toujours p0 → p1 :
    // p1 = (100 ; 10) donne une direction (100 ; 10), normale gauche (−10 ; 100) / |d|.
    let out = p
        .apply(Command::MovePoint {
            sheet: s,
            point: p1,
            to: PointMm::new(100.0, 10.0),
        })
        .unwrap();
    assert!(out.notes.iter().any(|n| n.contains("n'est plus maintenu")));
    assert!(!p.sheet(s).unwrap().is_kept_segment(o.entities[0]));
    assert!(p.sheet(s).unwrap().is_kept_segment(o.entities[2]));
    // |d| = racine(10100) = 100,498756211209 ; k = 60 / |d| ; p3 = (−10 k ; 100 k).
    let norm = 10100f64.sqrt();
    let k = 60.0 / norm;
    at(&p, s, p3, -10.0 * k, 100.0 * k);
    at(&p, s, p2, 100.0 - 10.0 * k, 10.0 + 100.0 * k);
    at(&p, s, p0, 0.0, 0.0);
}

#[test]
fn refus_sans_trace_dimensions_invalides_et_point_deja_suivi() {
    let mut p = Project::new("Rect");
    let s = p.sheets[0].id;
    for (w, h) in [
        (0.0, 10.0),
        (10.0, 0.0),
        (1e-7, 10.0),
        (f64::NAN, 10.0),
        (10.0, f64::INFINITY),
    ] {
        refused(
            &mut p,
            Command::AddConstructedRectangle {
                sheet: s,
                corner: PointRef::New(PointMm::new(0.0, 0.0)),
                width_mm: w,
                height_mm: h,
                kept: true,
            },
        );
    }
    // Deux rectangles maintenus qui partageraient un coin suivi (même position) : le second est
    // refusé (un point ne suit qu'une construction), sans trace.
    rect(&mut p, s, 0.0, 0.0, 100.0, 60.0, true).unwrap();
    let e = refused(
        &mut p,
        Command::AddConstructedRectangle {
            sheet: s,
            corner: PointRef::New(PointMm::new(0.0, 60.0)),
            width_mm: 100.0,
            height_mm: -60.0,
            kept: true,
        },
    );
    assert!(!e.is_empty());
    // Même rectangle non maintenu : accepté (aucune construction à écrire).
    rect(&mut p, s, 200.0, 0.0, 10.0, 10.0, false).unwrap();
}

#[test]
fn le_rectangle_survit_a_la_serialisation_et_a_la_copie_de_feuille() {
    let mut p = Project::new("Rect");
    let s = p.sheets[0].id;
    let o = rect(&mut p, s, 0.0, 0.0, 100.0, 60.0, true).unwrap();
    let json = bytes(&p);
    let back = persist::from_json(&json).unwrap();
    assert_eq!(back, p);
    assert_eq!(bytes(&back), json);
    // Aucun champ nouveau : les clés du fichier sont celles des segments ordinaires.
    assert!(!json.contains("rectangle"));
    let d = p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    let copy = d.sheet.unwrap();
    let c = p.sheet(copy).unwrap();
    assert_eq!(
        c.kept_segment_ids().len(),
        2,
        "liens conservés dans la copie"
    );
    let q0 = c
        .points
        .iter()
        .find(|q| matches!(q.definition, PointDefinition::Free { x, y } if x == 0.0 && y == 0.0))
        .unwrap()
        .id;
    p.apply(Command::MovePoint {
        sheet: copy,
        point: q0,
        to: PointMm::new(3.0, 4.0),
    })
    .unwrap();
    assert!(p
        .sheet(copy)
        .unwrap()
        .points
        .iter()
        .any(|q| (pos(&p, copy, q.id).x - 103.0).abs() < TOL
            && (pos(&p, copy, q.id).y - 64.0).abs() < TOL));
    let _ = o;
}

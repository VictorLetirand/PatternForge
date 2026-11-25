//! US-02.6 — divisions : N parts égales ou proportions, en longueur d'arc, par
//! des points construits maintenus ; une seule étape annulable ; refus sans trace.

use pf_core::*;
use pf_geometry::{distance_mm, PointMm};

const TOL: f64 = 1e-9;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn setup() -> (Project, SheetId) {
    let p = Project::new("Divisions");
    let s = p.sheets[0].id;
    (p, s)
}

fn snapshot(p: &Project) -> String {
    serde_json::to_string(p).unwrap()
}

fn pos(p: &Project, s: SheetId, id: PointId) -> PointMm {
    p.sheet(s).unwrap().position(id).unwrap()
}

fn seg(p: &mut Project, s: SheetId, a: PointMm, b: PointMm) -> (EntityId, PointId, PointId) {
    let o = p
        .apply(Command::AddSegment {
            sheet: s,
            from: PointRef::New(a),
            to: PointRef::New(b),
        })
        .unwrap();
    (o.entities[0], o.points[0], o.points[1])
}

fn s_curve(p: &mut Project, s: SheetId) -> EntityId {
    p.apply(Command::AddCurve {
        sheet: s,
        start: PointRef::New(pt(0.0, 0.0)),
        c1: pt(80.0, 60.0),
        c2: pt(20.0, -50.0),
        end: PointRef::New(pt(100.0, 10.0)),
    })
    .unwrap()
    .entities[0]
}

fn spline(p: &mut Project, s: SheetId) -> EntityId {
    p.apply(Command::AddSpline {
        sheet: s,
        points: vec![
            PointRef::New(pt(0.0, 0.0)),
            PointRef::New(pt(30.0, 25.0)),
            PointRef::New(pt(70.0, -10.0)),
            PointRef::New(pt(120.0, 15.0)),
        ],
    })
    .unwrap()
    .entities[0]
}

fn divide(p: &mut Project, s: SheetId, trace: EntityId, spec: DivisionSpec) -> Vec<PointId> {
    p.apply(Command::DivideTrace {
        sheet: s,
        trace,
        spec,
        start: None,
    })
    .unwrap()
    .points
}

fn equal(parts: usize) -> DivisionSpec {
    DivisionSpec::Equal { parts }
}

fn props(values: &[f64]) -> DivisionSpec {
    DivisionSpec::Proportions {
        values: values.to_vec(),
    }
}

/// Polyligne très fine du tracé (référence indépendante de `length_mm`) et
/// longueurs cumulées.
fn dense(shape: &Shape) -> (Vec<PointMm>, Vec<f64>) {
    const N: usize = 60_000;
    let beziers = match shape {
        Shape::Cubic(c) => vec![*c],
        Shape::Spline(sp) => sp.segments().to_vec(),
        _ => unreachable!(),
    };
    let mut poly = Vec::new();
    for c in &beziers {
        for i in 0..=N {
            poly.push(c.point_at(i as f64 / N as f64));
        }
    }
    let mut cum = vec![0.0];
    for w in poly.windows(2) {
        let last = *cum.last().unwrap();
        cum.push(last + distance_mm(w[0], w[1]));
    }
    (poly, cum)
}

fn arc_position(poly: &[PointMm], cum: &[f64], q: PointMm) -> f64 {
    let mut best = (f64::MAX, 0.0);
    for i in 0..poly.len() - 1 {
        let (a, b) = (poly[i], poly[i + 1]);
        let ab = b - a;
        let l2 = ab.x * ab.x + ab.y * ab.y;
        if l2 == 0.0 {
            continue;
        }
        let aq = q - a;
        let t = ((aq.x * ab.x + aq.y * ab.y) / l2).clamp(0.0, 1.0);
        let c = pt(a.x + t * ab.x, a.y + t * ab.y);
        let d = distance_mm(c, q);
        if d < best.0 {
            best = (d, cum[i] + t * l2.sqrt());
        }
    }
    best.1
}

/// Longueurs des tronçons mesurées sur la polyligne fine.
fn sections(p: &Project, s: SheetId, trace: EntityId, points: &[PointId]) -> (Vec<f64>, f64) {
    let shape = p.sheet(s).unwrap().shape(trace).unwrap();
    let (poly, cum) = dense(&shape);
    let total = *cum.last().unwrap();
    let mut at: Vec<f64> = vec![0.0];
    at.extend(
        points
            .iter()
            .map(|id| arc_position(&poly, &cum, pos(p, s, *id))),
    );
    at.push(total);
    (at.windows(2).map(|w| w[1] - w[0]).collect(), total)
}

fn assert_sections(got: &[f64], total: f64, ratios: &[f64]) {
    assert_eq!(got.len(), ratios.len());
    let sum: f64 = ratios.iter().sum();
    for (g, r) in got.iter().zip(ratios) {
        assert!(
            (g - total * r / sum).abs() < 1e-6,
            "tronçon {g} au lieu de {}",
            total * r / sum
        );
    }
}

// --- CA : points construits, parts égales, proportions -------------------------

#[test]
fn segment_parts_egales_points_construits_maintenus() {
    let (mut p, s) = setup();
    let (e, _, _) = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let ids = divide(&mut p, s, e, equal(4));
    assert_eq!(ids.len(), 3);
    for (i, id) in ids.iter().enumerate() {
        let point = p.sheet(s).unwrap().point(*id).unwrap();
        assert!(matches!(
            point.definition,
            PointDefinition::Division { trace, .. } if trace == e
        ));
        assert_eq!(point.role, PointRole::Construction);
        assert!(distance_mm(pos(&p, s, *id), pt(25.0 * (i + 1) as f64, 0.0)) < TOL);
    }
}

#[test]
fn courbe_de_bezier_parts_egales_en_longueur_d_arc() {
    let (mut p, s) = setup();
    let c = s_curve(&mut p, s);
    for n in [2usize, 5, 9] {
        let mut q = p.clone();
        let ids = divide(&mut q, s, c, equal(n));
        assert_eq!(ids.len(), n - 1);
        let (got, total) = sections(&q, s, c, &ids);
        assert_sections(&got, total, &vec![1.0; n]);
    }
}

#[test]
fn courbe_lisse_parts_egales_et_proportions() {
    let (mut p, s) = setup();
    let sp = spline(&mut p, s);
    let mut q = p.clone();
    let ids = divide(&mut q, s, sp, equal(7));
    let (got, total) = sections(&q, s, sp, &ids);
    assert_sections(&got, total, &[1.0; 7]);
    let ids = divide(&mut p, s, sp, props(&[1.0, 2.0, 1.0]));
    let (got, total) = sections(&p, s, sp, &ids);
    assert_sections(&got, total, &[1.0, 2.0, 1.0]);
}

#[test]
fn proportions_liste_et_fractions_saisies() {
    let (mut p, s) = setup();
    let (e, _, _) = seg(&mut p, s, pt(0.0, 0.0), pt(80.0, 0.0));
    let spec = input::parse_division("1:2:1").unwrap();
    assert_eq!(spec, props(&[1.0, 2.0, 1.0]));
    let ids = divide(&mut p, s, e, spec);
    let xs: Vec<f64> = ids.iter().map(|i| pos(&p, s, *i).x).collect();
    assert!((xs[0] - 20.0).abs() < TOL && (xs[1] - 60.0).abs() < TOL);
    let spec = input::parse_division("1/4; 1/2; 1/4").unwrap();
    assert_eq!(spec, props(&[0.25, 0.5, 0.25]));
    assert_eq!(input::parse_division("5").unwrap(), equal(5));
    assert_eq!(
        input::parse_division("0,5 1,5").unwrap(),
        props(&[0.5, 1.5])
    );
    for bad in ["", "abc", "1/0 1", "1:x", "2.5", "1:"] {
        // « 1: » = un seul élément avec séparateur : une proportion seule est refusée plus loin
        let r = input::parse_division(bad);
        if let Ok(spec) = r {
            assert!(spec.ratios().is_err(), "{bad}");
        }
    }
}

#[test]
fn arc_parts_egales_en_angle_exact() {
    let (mut p, s) = setup();
    let o = p
        .apply(Command::AddArc {
            sheet: s,
            center: PointRef::New(pt(10.0, 20.0)),
            radius_mm: 50.0,
            start_deg: 20.0,
            end_deg: 140.0,
            ccw: true,
        })
        .unwrap();
    let arc = o.entities[0];
    let ids = divide(&mut p, s, arc, equal(4));
    for (i, id) in ids.iter().enumerate() {
        let a = (20.0 + 30.0 * (i + 1) as f64).to_radians();
        let want = pt(10.0 + 50.0 * a.cos(), 20.0 + 50.0 * a.sin());
        assert!(distance_mm(pos(&p, s, *id), want) < TOL, "point {i}");
    }
}

fn circle(p: &mut Project, s: SheetId) -> (EntityId, PointId) {
    let o = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: 40.0,
        })
        .unwrap();
    let e = o.entities[0];
    let EntityKind::Circle { center, .. } = p.sheet(s).unwrap().entity(e).unwrap().kind else {
        unreachable!()
    };
    (e, center)
}

#[test]
fn cercle_parts_egales_en_angle_depuis_un_point_de_depart_donne() {
    let (mut p, s) = setup();
    let (c, center) = circle(&mut p, s);
    // Sans point de départ : départ propre du cercle (0°).
    let mut q = p.clone();
    let ids = divide(&mut q, s, c, equal(4));
    assert_eq!(ids.len(), 3);
    assert!(distance_mm(pos(&q, s, ids[0]), pt(0.0, 40.0)) < TOL);
    // Avec un point de départ à 30° (hors cercle : seule la direction compte).
    let a = 30f64.to_radians();
    let start = p
        .apply(Command::AddPoint {
            sheet: s,
            at: pt(100.0 * a.cos(), 100.0 * a.sin()),
        })
        .unwrap()
        .points[0];
    let ids = p
        .apply(Command::DivideTrace {
            sheet: s,
            trace: c,
            spec: equal(6),
            start: Some(start),
        })
        .unwrap()
        .points;
    assert_eq!(ids.len(), 5);
    for (i, id) in ids.iter().enumerate() {
        let b = (30.0 + 60.0 * (i + 1) as f64).to_radians();
        assert!(
            distance_mm(pos(&p, s, *id), pt(40.0 * b.cos(), 40.0 * b.sin())) < TOL,
            "point {i}"
        );
    }
    // Le point de départ est maintenu : le déplacer fait tourner la division.
    p.apply(Command::MovePoint {
        sheet: s,
        point: start,
        to: pt(0.0, 50.0),
    })
    .unwrap();
    let b = 150f64.to_radians();
    assert!(distance_mm(pos(&p, s, ids[0]), pt(40.0 * b.cos(), 40.0 * b.sin())) < 1e-6);
    // Déplacer le centre : le cercle et la direction centre → départ suivent.
    p.apply(Command::MovePoint {
        sheet: s,
        point: center,
        to: pt(5.0, 7.0),
    })
    .unwrap();
    let b = ((50.0_f64 - 7.0).atan2(-5.0)).to_degrees() + 120.0;
    let b = b.to_radians();
    assert!(
        distance_mm(
            pos(&p, s, ids[1]),
            pt(5.0 + 40.0 * b.cos(), 7.0 + 40.0 * b.sin())
        ) < 1e-6
    );
}

// --- bornes et refus sans trace -------------------------------------------------

#[test]
fn bornes_de_n_et_proportions_invalides_refusees_sans_trace() {
    let (mut p, s) = setup();
    let (e, _, _) = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let before = snapshot(&p);
    let bad = [
        equal(0),
        equal(1),
        equal(1001),
        equal(usize::MAX),
        props(&[]),
        props(&[1.0]),
        props(&[1.0, 0.0]),
        props(&[1.0, -1.0]),
        props(&[1.0, f64::NAN]),
        props(&[1.0, f64::INFINITY]),
        props(&[f64::MAX, f64::MAX]),
        props(&[1e-300, 1e300]),
        props(&vec![1.0; 1001]),
    ];
    for spec in bad {
        let r = p.apply(Command::DivideTrace {
            sheet: s,
            trace: e,
            spec: spec.clone(),
            start: None,
        });
        assert!(r.is_err(), "{spec:?} doit être refusé");
        assert_eq!(snapshot(&p), before, "{spec:?} : aucune trace");
    }
    // Bornes acceptées : 2 et 1000.
    assert_eq!(divide(&mut p, s, e, equal(2)).len(), 1);
    assert_eq!(divide(&mut p, s, e, equal(1000)).len(), 999);
}

#[test]
fn trace_absente_verrouillee_ou_depart_invalide_refuses_sans_trace() {
    let (mut p, s) = setup();
    let (e, a, _) = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let before = snapshot(&p);
    let attempts = [
        (EntityId(99_999), None),
        (e, Some(PointId(99_999))),
        // un point de départ n'a de sens que pour un cercle
        (e, Some(a)),
    ];
    for (trace, start) in attempts {
        let r = p.apply(Command::DivideTrace {
            sheet: s,
            trace,
            spec: equal(3),
            start,
        });
        assert!(r.is_err(), "{trace:?} {start:?}");
        assert_eq!(snapshot(&p), before);
    }
    assert!(p
        .apply(Command::DivideTrace {
            sheet: SheetId(99_999),
            trace: e,
            spec: equal(3),
            start: None,
        })
        .is_err());
    assert_eq!(snapshot(&p), before);

    // Calque verrouillé.
    let l = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Base".into(),
        })
        .unwrap()
        .layers[0];
    let o = p
        .apply(Command::InLayer {
            sheet: s,
            layer: l,
            command: Box::new(Command::AddSegment {
                sheet: s,
                from: PointRef::New(pt(0.0, 50.0)),
                to: PointRef::New(pt(60.0, 50.0)),
            }),
        })
        .unwrap();
    let locked_trace = o.entities[0];
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer: l,
        locked: true,
    })
    .unwrap();
    let before = snapshot(&p);
    let err = p
        .apply(Command::DivideTrace {
            sheet: s,
            trace: locked_trace,
            spec: equal(3),
            start: None,
        })
        .unwrap_err();
    assert!(matches!(err, CoreError::LayerLocked(_)), "{err}");
    assert_eq!(snapshot(&p), before);
    // Un tracé libre reste divisible pendant qu'un autre calque est verrouillé.
    assert_eq!(divide(&mut p, s, e, equal(3)).len(), 2);
}

// --- suivi du tracé --------------------------------------------------------------

#[test]
fn les_points_suivent_le_trace_quand_il_change() {
    let (mut p, s) = setup();
    let (e, a, b) = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let ids = divide(&mut p, s, e, props(&[1.0, 3.0]));
    assert!((pos(&p, s, ids[0]).x - 25.0).abs() < TOL);
    p.apply(Command::MovePoint {
        sheet: s,
        point: b,
        to: pt(0.0, 200.0),
    })
    .unwrap();
    assert!(distance_mm(pos(&p, s, ids[0]), pt(0.0, 50.0)) < TOL);
    p.apply(Command::MovePoint {
        sheet: s,
        point: a,
        to: pt(0.0, 100.0),
    })
    .unwrap();
    assert!(distance_mm(pos(&p, s, ids[0]), pt(0.0, 125.0)) < TOL);
}

#[test]
fn les_parts_restent_egales_apres_deplacement_d_une_poignee_de_courbe() {
    let (mut p, s) = setup();
    let c = s_curve(&mut p, s);
    let ids = divide(&mut p, s, c, equal(5));
    p.apply(Command::MoveCurveHandle {
        sheet: s,
        curve: c,
        handle: CurveHandle::Start,
        to: pt(10.0, 90.0),
    })
    .unwrap();
    let (got, total) = sections(&p, s, c, &ids);
    assert_sections(&got, total, &[1.0; 5]);
}

#[test]
fn trace_qui_devient_degenere_ou_supprime_est_refuse_ou_fige() {
    let (mut p, s) = setup();
    let (e, _, b) = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let ids = divide(&mut p, s, e, equal(3));
    // Rendre le segment de longueur nulle est refusé, rien ne bouge.
    let before = snapshot(&p);
    assert!(p
        .apply(Command::MovePoint {
            sheet: s,
            point: b,
            to: pt(0.0, 0.0),
        })
        .is_err());
    assert_eq!(snapshot(&p), before);
    // Supprimer le tracé : les points sont figés ou la suppression est refusée,
    // jamais un point à une position fausse.
    let r = p.apply(Command::Delete {
        sheet: s,
        points: vec![],
        entities: vec![e],
    });
    if r.is_ok() {
        for id in &ids {
            assert!(p.sheet(s).unwrap().position(*id).is_ok());
        }
    }
}

// --- annulation ------------------------------------------------------------------

#[test]
fn une_division_est_une_seule_etape_annulable() {
    let (mut p, s) = setup();
    let (e, _, _) = seg(&mut p, s, pt(0.0, 0.0), pt(90.0, 0.0));
    let mut doc = Document::new(p.clone());
    let n_before = doc.project().sheet(s).unwrap().points.len();
    doc.execute(Command::DivideTrace {
        sheet: s,
        trace: e,
        spec: equal(10),
        start: None,
    })
    .unwrap();
    assert_eq!(doc.project().sheet(s).unwrap().points.len(), n_before + 9);
    assert!(doc.undo());
    assert_eq!(snapshot(doc.project()), snapshot(&p));
    assert!(doc.redo());
    assert_eq!(doc.project().sheet(s).unwrap().points.len(), n_before + 9);
}

// --- fichiers, copies ---------------------------------------------------------------

#[test]
fn fichiers_inchanges_sans_division_et_relecture_exacte() {
    let (mut p, s) = setup();
    let (e, _, _) = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let (c, _) = circle(&mut p, s);
    let plain = persist::to_json(&p).unwrap();
    assert!(plain.contains("\"schema_version\": 6"));
    assert!(!plain.contains("\"type\": \"division\""));
    divide(&mut p, s, e, equal(3));
    let start = p
        .apply(Command::AddPoint {
            sheet: s,
            at: pt(40.0, 0.0),
        })
        .unwrap()
        .points[0];
    p.apply(Command::DivideTrace {
        sheet: s,
        trace: c,
        spec: equal(3),
        start: Some(start),
    })
    .unwrap();
    let json = persist::to_json(&p).unwrap();
    assert!(json.contains("\"schema_version\": 6"));
    assert!(json.contains("\"type\": \"division\""));
    let back = persist::from_json(&json).unwrap();
    assert_eq!(back, p);
    assert_eq!(persist::to_json(&back).unwrap(), json);
    // Division sur un segment : le champ « start » est omis.
    let first = json.find("\"type\": \"division\"").unwrap();
    let chunk = &json[first..first + 120];
    assert!(!chunk.contains("start"), "{chunk}");
    // Champ inconnu : refusé ; fraction hors de ]0 ; 1[ : refusée à la lecture.
    assert!(persist::from_json(&json.replacen("\"ratio\"", "\"ratioo\"", 1)).is_err());
    let bad = json.replacen("\"ratio\": 0.3333333333333333", "\"ratio\": 1.5", 1);
    assert_ne!(bad, json);
    assert!(persist::from_json(&bad).is_err());
}

#[test]
fn copie_de_feuille_et_presse_papiers_reecrivent_les_ids() {
    let (mut p, s) = setup();
    let (e, _, _) = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let ids = divide(&mut p, s, e, equal(4));
    p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    let copy = p.sheets[1].clone();
    let copied: Vec<&Point> = copy
        .points
        .iter()
        .filter(|q| matches!(q.definition, PointDefinition::Division { .. }))
        .collect();
    assert_eq!(copied.len(), 3);
    for q in copied {
        let PointDefinition::Division { trace, .. } = q.definition else {
            unreachable!()
        };
        assert_ne!(trace, e, "la copie suit son propre tracé");
        assert!(copy.entity(trace).is_ok());
        assert!(!ids.contains(&q.id));
    }
    // Presse-papiers : tracé et points copiés, la construction est maintenue.
    let sheet = p.sheet(s).unwrap();
    let all_points: Vec<PointId> = sheet.points.iter().map(|q| q.id).collect();
    let frag = p.copy_fragment(s, &all_points, &[e], &[], &[]).unwrap();
    frag.validate().unwrap();
    p.apply(Command::PasteFragment {
        sheet: s,
        fragment: Box::new(frag),
        layer: None,
    })
    .unwrap();
    let n = p
        .sheet(s)
        .unwrap()
        .points
        .iter()
        .filter(|q| matches!(q.definition, PointDefinition::Division { .. }))
        .count();
    assert_eq!(n, 6);
    // Tracé non copié : le point devient libre dans le fragment.
    let frag = p.copy_fragment(s, &[ids[0]], &[], &[], &[]).unwrap();
    assert!(frag
        .points
        .iter()
        .find(|q| q.id == ids[0])
        .unwrap()
        .definition
        .is_free());
}

//! US-02.4 — intersections générales : point construit à l'intersection de deux
//! tracés quelconques (segment, Bézier, courbe lisse, arc, cercle), qui suit ses
//! parents ; refus sans trace quand l'intersection disparaît ou n'existe pas.

use pf_core::*;
use pf_geometry::{distance_mm, PointMm};

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn setup() -> (Project, SheetId) {
    let p = Project::new("Intersections");
    let s = p.sheets[0].id;
    (p, s)
}

fn snapshot(p: &Project) -> String {
    serde_json::to_string(p).unwrap()
}

fn seg(p: &mut Project, s: SheetId, a: PointMm, b: PointMm) -> Outcome {
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::New(a),
        to: PointRef::New(b),
    })
    .unwrap()
}

/// Arche : sommet (5 ; 7,5), y(t) = 30 t (1 − t).
fn arch(p: &mut Project, s: SheetId) -> Outcome {
    p.apply(Command::AddCurve {
        sheet: s,
        start: PointRef::New(pt(0.0, 0.0)),
        c1: pt(0.0, 10.0),
        c2: pt(10.0, 10.0),
        end: PointRef::New(pt(10.0, 0.0)),
    })
    .unwrap()
}

fn circle(p: &mut Project, s: SheetId, c: PointMm, r: f64) -> EntityId {
    p.apply(Command::AddCircle {
        sheet: s,
        center: PointRef::New(c),
        radius_mm: r,
    })
    .unwrap()
    .entities[0]
}

fn spline(p: &mut Project, s: SheetId, pts: &[PointMm]) -> Outcome {
    p.apply(Command::AddSpline {
        sheet: s,
        points: pts.iter().map(|q| PointRef::New(*q)).collect(),
    })
    .unwrap()
}

fn inter(first: EntityId, second: EntityId, near: PointMm) -> PointRef {
    PointRef::Derived(PointDefinition::TraceIntersection {
        first,
        second,
        near: near.into(),
    })
}

fn add_inter(
    p: &mut Project,
    s: SheetId,
    first: EntityId,
    second: EntityId,
    near: PointMm,
) -> Result<PointId, CoreError> {
    p.apply(Command::AddPointAt {
        sheet: s,
        point: inter(first, second, near),
    })
    .map(|o| o.points[0])
}

fn pos(p: &Project, s: SheetId, id: PointId) -> PointMm {
    p.sheet(s).unwrap().position(id).unwrap()
}

// --- CA : couples manquants, solutions exactes ----------------------------------

#[test]
fn courbe_segment_deux_solutions_choisies_par_proximite() {
    let (mut p, s) = setup();
    let c = arch(&mut p, s).entities[0];
    let l = seg(&mut p, s, pt(-5.0, 3.0), pt(15.0, 3.0)).entities[0];
    let hits = p.sheet(s).unwrap().trace_intersections(c, l).unwrap();
    assert_eq!(hits.len(), 2);
    let disc = (1.0_f64 - 4.0 * 3.0 / 30.0).sqrt();
    let xs: Vec<f64> = [(1.0 - disc) / 2.0, (1.0 + disc) / 2.0]
        .iter()
        .map(|t| {
            let u = 1.0 - t;
            30.0 * u * t * t + 10.0 * t * t * t
        })
        .collect();
    for (h, x) in hits.iter().zip(&xs) {
        assert!(distance_mm(h.point, pt(*x, 3.0)) < 1e-9, "{h:?}");
    }
    // `near` choisit la solution : près de la gauche, puis près de la droite.
    let left = add_inter(&mut p, s, c, l, pt(0.0, 3.0)).unwrap();
    assert!(distance_mm(pos(&p, s, left), pt(xs[0], 3.0)) < 1e-9);
    let right = add_inter(&mut p, s, c, l, pt(10.0, 3.0)).unwrap();
    assert!(distance_mm(pos(&p, s, right), pt(xs[1], 3.0)) < 1e-9);
    assert_ne!(left, right);
}

#[test]
fn courbe_courbe_courbe_cercle_courbe_arc_et_spline_tout() {
    let (mut p, s) = setup();
    let c1 = arch(&mut p, s).entities[0];
    let c2 = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(pt(0.0, 12.0)),
            c1: pt(0.0, 2.0),
            c2: pt(10.0, 2.0),
            end: PointRef::New(pt(10.0, 12.0)),
        })
        .unwrap()
        .entities[0];
    let ring = circle(&mut p, s, pt(5.0, 0.0), 6.0);
    let arc = p
        .apply(Command::AddArc {
            sheet: s,
            center: PointRef::New(pt(5.0, 0.0)),
            radius_mm: 6.0,
            start_deg: 0.0,
            end_deg: 90.0,
            ccw: true,
        })
        .unwrap()
        .entities[0];
    let sp = spline(&mut p, s, &[pt(-3.0, 4.0), pt(5.0, 6.0), pt(13.0, 4.0)]).entities[0];
    let line = seg(&mut p, s, pt(-10.0, 5.0), pt(20.0, 5.0)).entities[0];

    let sh = p.sheet(s).unwrap();
    // courbe/courbe : deux croisements.
    assert_eq!(sh.trace_intersections(c1, c2).unwrap().len(), 2);
    // courbe/cercle : deux croisements ; courbe/arc (quart de cercle) : un seul.
    assert_eq!(sh.trace_intersections(c1, ring).unwrap().len(), 2);
    assert_eq!(sh.trace_intersections(c1, arc).unwrap().len(), 1);
    // spline avec segment, courbe, cercle, arc.
    for other in [line, c1, ring, arc] {
        let hits = sh.trace_intersections(sp, other).unwrap();
        assert!(!hits.is_empty(), "spline / {other:?}");
        // symétrique : mêmes points dans l'autre ordre
        let back = sh.trace_intersections(other, sp).unwrap();
        assert_eq!(hits.len(), back.len());
        for h in &hits {
            assert!(back.iter().any(|b| distance_mm(b.point, h.point) < 1e-9));
        }
    }
    // Chaque couple donne un point construit valide.
    for (a, b) in [
        (c1, c2),
        (c1, ring),
        (c1, arc),
        (sp, line),
        (sp, c1),
        (sp, ring),
    ] {
        let near = p.sheet(s).unwrap().trace_intersections(a, b).unwrap()[0].point;
        let def = PointDefinition::TraceIntersection {
            first: a,
            second: b,
            near: near.into(),
        };
        assert!(distance_mm(p.sheet(s).unwrap().evaluate(&def).unwrap(), near) < 1e-9);
        p.apply(Command::AddPointAt {
            sheet: s,
            point: PointRef::Derived(def),
        })
        .unwrap();
    }
}

#[test]
fn tangence_aucune_et_solutions_nombreuses() {
    let (mut p, s) = setup();
    let c = arch(&mut p, s).entities[0];
    let tangent = seg(&mut p, s, pt(-5.0, 7.5), pt(15.0, 7.5)).entities[0];
    let above = seg(&mut p, s, pt(-5.0, 7.6), pt(15.0, 7.6)).entities[0];
    let sh = p.sheet(s).unwrap();
    let t = sh.trace_intersections(c, tangent).unwrap();
    assert_eq!(t.len(), 1);
    assert!(distance_mm(t[0].point, pt(5.0, 7.5)) < 1e-9);
    assert!(sh.trace_intersections(c, above).unwrap().is_empty());
    // Courbe lisse en vague : beaucoup de croisements avec une droite.
    let wave: Vec<PointMm> = (0..9)
        .map(|i| pt(i as f64 * 10.0, if i % 2 == 0 { 0.0 } else { 10.0 }))
        .collect();
    let w = spline(&mut p, s, &wave).entities[0];
    let mid = seg(&mut p, s, pt(-5.0, 5.0), pt(85.0, 5.0)).entities[0];
    let hits = p.sheet(s).unwrap().trace_intersections(w, mid).unwrap();
    assert_eq!(hits.len(), 8, "{hits:?}");
    for h in &hits {
        assert!((h.point.y - 5.0).abs() < 1e-9);
    }
}

// --- Le point suit ses parents ----------------------------------------------------

#[test]
fn le_point_suit_ses_parents_et_garde_sa_solution() {
    let (mut p, s) = setup();
    let o = arch(&mut p, s);
    let (c, start) = (o.entities[0], o.points[0]);
    let l = seg(&mut p, s, pt(-5.0, 3.0), pt(15.0, 3.0));
    let (line, line_start) = (l.entities[0], l.points[0]);
    let id = add_inter(&mut p, s, c, line, pt(10.0, 3.0)).unwrap();
    let before = pos(&p, s, id);
    // Déplacer la droite vers le haut : le point reste sur la branche de droite.
    p.apply(Command::MovePoint {
        sheet: s,
        point: line_start,
        to: pt(-5.0, 5.0),
    })
    .unwrap();
    let after = pos(&p, s, id);
    assert!(
        after.x > 5.0 && after.y > 3.0 && after.y < 5.0 + 1e-9,
        "{after:?}"
    );
    assert!(before.x > 5.0);
    // Il est toujours exactement sur les deux tracés.
    let sh = p.sheet(s).unwrap();
    let hits = sh.trace_intersections(c, line).unwrap();
    assert!(hits.iter().any(|h| distance_mm(h.point, after) < 1e-9));
    // Déplacer l'extrémité de la courbe : le point est recalculé.
    p.apply(Command::MovePoint {
        sheet: s,
        point: start,
        to: pt(-2.0, 0.0),
    })
    .unwrap();
    let sh = p.sheet(s).unwrap();
    let again = sh.position(id).unwrap();
    let hits = sh.trace_intersections(c, line).unwrap();
    assert!(hits.iter().any(|h| distance_mm(h.point, again) < 1e-9));
    assert!(
        distance_mm(again, after) > 1e-6,
        "le point aurait dû bouger"
    );
}

#[test]
fn intersection_disparue_apres_deplacement_refus_explique_sans_trace() {
    let (mut p, s) = setup();
    let c = arch(&mut p, s).entities[0];
    let l = seg(&mut p, s, pt(-5.0, 3.0), pt(15.0, 3.0));
    let (line, line_start, line_end) = (l.entities[0], l.points[0], l.points[1]);
    let id = add_inter(&mut p, s, c, line, pt(0.0, 3.0)).unwrap();
    let before = snapshot(&p);
    let pos_before = pos(&p, s, id);
    // La droite passe au-dessus du sommet : plus de solution → la commande est refusée.
    for (point, to) in [(line_start, pt(-5.0, 30.0)), (line_end, pt(15.0, 30.0))] {
        let err = p
            .apply(Command::MovePoint {
                sheet: s,
                point,
                to,
            })
            .unwrap_err()
            .to_string();
        assert!(err.contains("ne se coupent plus"), "{err}");
        assert_eq!(snapshot(&p), before, "aucune trace après refus");
        assert_eq!(pos(&p, s, id), pos_before);
    }
    // Un déplacement qui garde une solution reste permis.
    p.apply(Command::MovePoint {
        sheet: s,
        point: line_start,
        to: pt(-5.0, 4.0),
    })
    .unwrap();
}

#[test]
fn refus_sans_trace() {
    let (mut p, s) = setup();
    let c = arch(&mut p, s).entities[0];
    let same = seg(&mut p, s, pt(0.0, 20.0), pt(10.0, 20.0)).entities[0];
    let apart = seg(&mut p, s, pt(0.0, 30.0), pt(10.0, 30.0)).entities[0];
    let overlap = seg(&mut p, s, pt(5.0, 20.0), pt(15.0, 20.0)).entities[0];
    let before = snapshot(&p);
    let cases: Vec<(PointRef, &str)> = vec![
        (inter(c, apart, pt(0.0, 0.0)), "ne se coupent pas"),
        (inter(c, c, pt(0.0, 0.0)), "tracés différents"),
        (inter(same, overlap, pt(5.0, 20.0)), "superposent"),
        (inter(c, EntityId(9999), pt(0.0, 0.0)), "absent"),
        (inter(EntityId(9999), c, pt(0.0, 0.0)), "absent"),
        (inter(c, same, pt(f64::NAN, 0.0)), "NaN"),
        (inter(c, same, pt(f64::INFINITY, 0.0)), "infini"),
        (inter(c, same, pt(1e12, 0.0)), "hors espace"),
    ];
    for (point, what) in cases {
        let r = p.apply(Command::AddPointAt { sheet: s, point });
        assert!(r.is_err(), "{what} aurait dû être refusé");
        assert_eq!(snapshot(&p), before, "{what} : trace laissée");
    }
    // Un tracé dégénéré (courbe aplatie sur la droite) : erreur propre, pas de panique.
    let flat = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(pt(0.0, 40.0)),
            c1: pt(3.0, 40.0),
            c2: pt(6.0, 40.0),
            end: PointRef::New(pt(9.0, 40.0)),
        })
        .unwrap()
        .entities[0];
    let line40 = seg(&mut p, s, pt(-1.0, 40.0), pt(20.0, 40.0)).entities[0];
    let before = snapshot(&p);
    assert!(add_inter(&mut p, s, flat, line40, pt(5.0, 40.0)).is_err());
    assert_eq!(snapshot(&p), before);
}

// --- Verrou, annulation, copie, fichier -------------------------------------------

#[test]
fn calque_verrouille_et_annulation() {
    let (mut p, s) = setup();
    let o = arch(&mut p, s);
    let c = o.entities[0];
    let l = seg(&mut p, s, pt(-5.0, 3.0), pt(15.0, 3.0));
    let (line, line_start) = (l.entities[0], l.points[0]);
    let id = add_inter(&mut p, s, c, line, pt(10.0, 3.0)).unwrap();
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Fixe".into(),
        })
        .unwrap()
        .layers[0];
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![id],
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
    // Le point verrouillé ne peut pas être entraîné par un parent libre.
    let before = snapshot(&p);
    let err = p
        .apply(Command::MovePoint {
            sheet: s,
            point: line_start,
            to: pt(-5.0, 4.0),
        })
        .unwrap_err()
        .to_string();
    assert!(err.contains("verrouill"), "{err}");
    assert_eq!(snapshot(&p), before);
    // Créer un point dans le calque verrouillé : pas possible non plus (le point
    // créé arrive dans le calque par défaut, mais la commande ne modifie aucun objet verrouillé).
    let mut doc = Document::new(p.clone());
    let n = doc.project().sheet(s).unwrap().points.len();
    doc.execute(Command::AddPointAt {
        sheet: s,
        point: inter(c, line, pt(0.0, 3.0)),
    })
    .unwrap();
    assert_eq!(doc.project().sheet(s).unwrap().points.len(), n + 1);
    assert!(doc.undo());
    assert_eq!(doc.project().sheet(s).unwrap().points.len(), n);
    assert!(doc.redo());
    assert_eq!(doc.project().sheet(s).unwrap().points.len(), n + 1);
}

#[test]
fn fichier_copie_de_feuille_et_octets_inchanges_sans_ces_points() {
    let (mut p, s) = setup();
    let c = arch(&mut p, s).entities[0];
    let line = seg(&mut p, s, pt(-5.0, 3.0), pt(15.0, 3.0)).entities[0];
    let plain = persist::to_json(&p).unwrap();
    assert!(!plain.contains("trace_intersection"));
    assert!(plain.contains("\"schema_version\": 6"));
    let id = add_inter(&mut p, s, c, line, pt(10.0, 3.0)).unwrap();
    let json = persist::to_json(&p).unwrap();
    assert!(json.contains("\"type\": \"trace_intersection\""), "{json}");
    let back = persist::from_json(&json).unwrap();
    assert_eq!(back, p);
    assert!(distance_mm(pos(&back, s, id), pos(&p, s, id)) < 1e-12);
    // Fichier dont l'intersection a disparu : refusé à la lecture, message clair.
    fn raise(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(m) => {
                if m.get("type").and_then(|t| t.as_str()) == Some("free")
                    && m.get("y").and_then(|y| y.as_f64()) == Some(3.0)
                {
                    m.insert("y".into(), serde_json::json!(30.0));
                }
                m.values_mut().for_each(raise);
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(raise),
            _ => {}
        }
    }
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    raise(&mut value);
    let err = persist::from_json(&value.to_string())
        .unwrap_err()
        .to_string();
    assert!(err.contains("ne se coupent plus"), "{err}");
    // Copie de feuille : le point suit les tracés copiés, pas les originaux.
    p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    let copy = p.sheets[1].clone();
    let q = copy
        .points
        .iter()
        .find(|q| matches!(q.definition, PointDefinition::TraceIntersection { .. }))
        .expect("point copié");
    let PointDefinition::TraceIntersection { first, second, .. } = q.definition else {
        unreachable!()
    };
    assert!(copy.entity(first).is_ok() && copy.entity(second).is_ok());
    assert_ne!(first, c);
    assert!(distance_mm(copy.position(q.id).unwrap(), pos(&p, s, id)) < 1e-9);
}

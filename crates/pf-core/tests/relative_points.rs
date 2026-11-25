//! US-02.5 — points relatifs maintenus : distance le long d'une courbe, dx/dy,
//! milieu, projection orthogonale, bissectrice, direction + distance, compas.
//! Chacun suit ses parents, refuse les cycles et les cas impossibles sans rien
//! modifier, se modifie par `SetPointValues` et s'annule.

use pf_core::*;
use pf_geometry::{distance_mm, PointMm};

const TOL: f64 = 1e-9;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn setup() -> (Project, SheetId) {
    let p = Project::new("Relatifs");
    let s = p.sheets[0].id;
    (p, s)
}

fn snapshot(p: &Project) -> String {
    serde_json::to_string(p).unwrap()
}

fn pos(p: &Project, s: SheetId, id: PointId) -> PointMm {
    p.sheet(s).unwrap().position(id).unwrap()
}

fn free(p: &mut Project, s: SheetId, x: f64, y: f64) -> PointId {
    p.apply(Command::AddPoint {
        sheet: s,
        at: pt(x, y),
    })
    .unwrap()
    .points[0]
}

fn add(p: &mut Project, s: SheetId, d: PointDefinition) -> Result<PointId, CoreError> {
    p.apply(Command::AddPointAt {
        sheet: s,
        point: PointRef::Derived(d),
    })
    .map(|o| o.points[0])
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

fn mv(p: &mut Project, s: SheetId, id: PointId, to: PointMm) -> Result<Outcome, CoreError> {
    p.apply(Command::MovePoint {
        sheet: s,
        point: id,
        to,
    })
}

fn values(p: &mut Project, s: SheetId, id: PointId, v: &[f64]) -> Result<Outcome, CoreError> {
    p.apply(Command::SetPointValues {
        sheet: s,
        point: id,
        values: v.to_vec(),
    })
}

/// Arc de centre (0 ; 0), rayon 50, de 0° à 90°.
fn quarter(p: &mut Project, s: SheetId) -> (EntityId, PointId) {
    let o = p
        .apply(Command::AddArc {
            sheet: s,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: 50.0,
            start_deg: 0.0,
            end_deg: 90.0,
            ccw: true,
        })
        .unwrap();
    let arc = o.entities[0];
    let center = match p.sheet(s).unwrap().entity(arc).unwrap().kind {
        EntityKind::Arc { center, .. } => center,
        _ => unreachable!(),
    };
    (arc, center)
}

/// Bézier colinéaire d'abscisse x(t) croissante : la longueur d'arc jusqu'à un
/// point est exactement son abscisse.
fn flat_curve(p: &mut Project, s: SheetId) -> EntityId {
    p.apply(Command::AddCurve {
        sheet: s,
        start: PointRef::New(pt(0.0, 0.0)),
        c1: pt(30.0, 0.0),
        c2: pt(60.0, 0.0),
        end: PointRef::New(pt(90.0, 0.0)),
    })
    .unwrap()
    .entities[0]
}

// --- distance le long d'une courbe ------------------------------------------------

#[test]
fn distance_le_long_d_un_arc_valeur_exacte_depuis_les_deux_bouts() {
    let (mut p, s) = setup();
    let (arc, _) = quarter(&mut p, s);
    let from_start = add(
        &mut p,
        s,
        PointDefinition::AlongCurve {
            curve: arc,
            distance_mm: 20.0,
            origin: SegmentEnd::Start,
        },
    )
    .unwrap();
    let a = 20.0_f64 / 50.0;
    assert!(distance_mm(pos(&p, s, from_start), pt(50.0 * a.cos(), 50.0 * a.sin())) < TOL);
    let from_end = add(
        &mut p,
        s,
        PointDefinition::AlongCurve {
            curve: arc,
            distance_mm: 20.0,
            origin: SegmentEnd::End,
        },
    )
    .unwrap();
    let b = std::f64::consts::FRAC_PI_2 - 20.0 / 50.0;
    assert!(distance_mm(pos(&p, s, from_end), pt(50.0 * b.cos(), 50.0 * b.sin())) < TOL);
}

#[test]
fn distance_le_long_d_une_bezier_et_d_une_courbe_lisse() {
    let (mut p, s) = setup();
    let c = flat_curve(&mut p, s);
    let id = add(
        &mut p,
        s,
        PointDefinition::AlongCurve {
            curve: c,
            distance_mm: 37.25,
            origin: SegmentEnd::Start,
        },
    )
    .unwrap();
    assert!(distance_mm(pos(&p, s, id), pt(37.25, 0.0)) < TOL);
    let sp = p
        .apply(Command::AddSpline {
            sheet: s,
            points: vec![
                PointRef::New(pt(0.0, 20.0)),
                PointRef::New(pt(50.0, 20.0)),
                PointRef::New(pt(100.0, 20.0)),
            ],
        })
        .unwrap()
        .entities[0];
    let id = add(
        &mut p,
        s,
        PointDefinition::AlongCurve {
            curve: sp,
            distance_mm: 70.0,
            origin: SegmentEnd::Start,
        },
    )
    .unwrap();
    assert!(distance_mm(pos(&p, s, id), pt(70.0, 20.0)) < TOL);
}

#[test]
fn distance_suit_la_courbe_et_est_modifiable_et_annulable() {
    let (mut p, s) = setup();
    let (arc, center) = quarter(&mut p, s);
    let id = add(
        &mut p,
        s,
        PointDefinition::AlongCurve {
            curve: arc,
            distance_mm: 25.0,
            origin: SegmentEnd::Start,
        },
    )
    .unwrap();
    let before = pos(&p, s, id);
    mv(&mut p, s, center, pt(10.0, 5.0)).unwrap();
    let after = pos(&p, s, id);
    assert!(distance_mm(after, pt(before.x + 10.0, before.y + 5.0)) < 1e-6);
    // Paramètre modifiable puis annulable.
    let mut doc = Document::new(p.clone());
    doc.execute(Command::SetPointValues {
        sheet: s,
        point: id,
        values: vec![40.0],
    })
    .unwrap();
    let r: f64 = 50.0;
    let c = pos(doc.project(), s, center);
    let a = 40.0 / r;
    assert!(
        distance_mm(
            pos(doc.project(), s, id),
            pt(c.x + r * a.cos(), c.y + r * a.sin())
        ) < TOL
    );
    assert!(doc.undo());
    assert_eq!(snapshot(doc.project()), snapshot(&p));
}

#[test]
fn distance_au_dela_de_la_longueur_refusee_sans_rien_modifier() {
    let (mut p, s) = setup();
    let (arc, _) = quarter(&mut p, s);
    let before = snapshot(&p);
    let len = 25.0 * std::f64::consts::PI;
    let err = add(
        &mut p,
        s,
        PointDefinition::AlongCurve {
            curve: arc,
            distance_mm: len + 0.01,
            origin: SegmentEnd::Start,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("dépasse la longueur"), "{err}");
    assert_eq!(snapshot(&p), before);
    let id = add(
        &mut p,
        s,
        PointDefinition::AlongCurve {
            curve: arc,
            distance_mm: 10.0,
            origin: SegmentEnd::Start,
        },
    )
    .unwrap();
    let before = snapshot(&p);
    assert!(values(&mut p, s, id, &[len + 1.0]).is_err());
    assert!(values(&mut p, s, id, &[-1.0]).is_err());
    assert!(values(&mut p, s, id, &[f64::NAN]).is_err());
    assert!(values(&mut p, s, id, &[1.0, 2.0]).is_err());
    assert_eq!(snapshot(&p), before);
}

// --- dx / dy, milieu ----------------------------------------------------------------

#[test]
fn decalage_dx_dy_exact_suit_son_parent_et_se_modifie() {
    let (mut p, s) = setup();
    let a = free(&mut p, s, 10.0, 20.0);
    let id = add(
        &mut p,
        s,
        PointDefinition::Offset {
            from: a,
            dx_mm: 3.5,
            dy_mm: -7.25,
        },
    )
    .unwrap();
    assert_eq!(pos(&p, s, id), pt(13.5, 12.75));
    mv(&mut p, s, a, pt(-4.0, 1.0)).unwrap();
    assert_eq!(pos(&p, s, id), pt(-0.5, -6.25));
    values(&mut p, s, id, &[1.0, 2.0]).unwrap();
    assert_eq!(pos(&p, s, id), pt(-3.0, 3.0));
    let before = snapshot(&p);
    assert!(values(&mut p, s, id, &[1.0]).is_err());
    assert!(values(&mut p, s, id, &[f64::INFINITY, 0.0]).is_err());
    assert!(values(&mut p, s, id, &[2.0e6, 0.0]).is_err());
    assert_eq!(snapshot(&p), before);
}

#[test]
fn milieu_de_deux_points_suit_ses_parents() {
    let (mut p, s) = setup();
    let a = free(&mut p, s, 0.0, 0.0);
    let b = free(&mut p, s, 10.0, 6.0);
    let m = add(
        &mut p,
        s,
        PointDefinition::Midpoint {
            first: a,
            second: b,
        },
    )
    .unwrap();
    assert_eq!(pos(&p, s, m), pt(5.0, 3.0));
    mv(&mut p, s, b, pt(20.0, -4.0)).unwrap();
    assert_eq!(pos(&p, s, m), pt(10.0, -2.0));
    let before = snapshot(&p);
    assert!(add(
        &mut p,
        s,
        PointDefinition::Midpoint {
            first: a,
            second: a
        }
    )
    .is_err());
    assert_eq!(snapshot(&p), before);
}

// --- projection ---------------------------------------------------------------------

#[test]
fn projection_sur_droite_cercle_arc_courbe_et_suivi() {
    let (mut p, s) = setup();
    let (line, a, _b) = seg(&mut p, s, pt(0.0, 0.0), pt(10.0, 0.0));
    let q = free(&mut p, s, 30.0, 4.0);
    let id = add(
        &mut p,
        s,
        PointDefinition::Projection {
            point: q,
            trace: line,
        },
    )
    .unwrap();
    // la droite porteuse est prolongée
    assert!(distance_mm(pos(&p, s, id), pt(30.0, 0.0)) < TOL);
    mv(&mut p, s, a, pt(0.0, -3.0)).unwrap();
    let (u, v) = (pt(10.0, 0.0) - pt(0.0, -3.0), pt(30.0, 4.0) - pt(0.0, -3.0));
    let k = (u.x * v.x + u.y * v.y) / (u.x * u.x + u.y * u.y);
    assert!(distance_mm(pos(&p, s, id), pt(k * u.x, -3.0 + k * u.y)) < TOL);

    let (mut p, s) = setup();
    let c = p
        .apply(Command::AddCircle {
            sheet: s,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: 5.0,
        })
        .unwrap()
        .entities[0];
    let q = free(&mut p, s, 0.0, 20.0);
    let id = add(
        &mut p,
        s,
        PointDefinition::Projection { point: q, trace: c },
    )
    .unwrap();
    assert!(distance_mm(pos(&p, s, id), pt(0.0, 5.0)) < TOL);

    let (mut p, s) = setup();
    let (arc, _) = quarter(&mut p, s);
    let q = free(&mut p, s, 60.0, 60.0);
    let id = add(
        &mut p,
        s,
        PointDefinition::Projection {
            point: q,
            trace: arc,
        },
    )
    .unwrap();
    let h = 50.0 / 2.0_f64.sqrt();
    assert!(distance_mm(pos(&p, s, id), pt(h, h)) < TOL);
    // pied hors de l'arc : refus, rien de modifié
    let before = snapshot(&p);
    let out = free(&mut p, s, -10.0, -10.0);
    let before_out = snapshot(&p);
    let err = add(
        &mut p,
        s,
        PointDefinition::Projection {
            point: out,
            trace: arc,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("hors de l'arc"), "{err}");
    assert_eq!(snapshot(&p), before_out);
    assert_ne!(before, before_out);

    let (mut p, s) = setup();
    let c = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(pt(0.0, 0.0)),
            c1: pt(0.0, 10.0),
            c2: pt(10.0, 10.0),
            end: PointRef::New(pt(10.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let q = free(&mut p, s, 5.0, 20.0);
    let id = add(
        &mut p,
        s,
        PointDefinition::Projection { point: q, trace: c },
    )
    .unwrap();
    assert!(distance_mm(pos(&p, s, id), pt(5.0, 7.5)) < TOL);
    let sp = p
        .apply(Command::AddSpline {
            sheet: s,
            points: vec![
                PointRef::New(pt(0.0, -20.0)),
                PointRef::New(pt(50.0, -20.0)),
                PointRef::New(pt(100.0, -20.0)),
            ],
        })
        .unwrap()
        .entities[0];
    let q = free(&mut p, s, 30.0, -5.0);
    let id = add(
        &mut p,
        s,
        PointDefinition::Projection {
            point: q,
            trace: sp,
        },
    )
    .unwrap();
    assert!(distance_mm(pos(&p, s, id), pt(30.0, -20.0)) < TOL);
}

// --- bissectrice ----------------------------------------------------------------------

#[test]
fn bissectrice_valeur_exacte_suivi_et_refus() {
    let (mut p, s) = setup();
    let v = free(&mut p, s, 0.0, 0.0);
    let a = free(&mut p, s, 10.0, 0.0);
    let b = free(&mut p, s, 0.0, 10.0);
    let id = add(
        &mut p,
        s,
        PointDefinition::Bisector {
            vertex: v,
            first: a,
            second: b,
            distance_mm: 2.0_f64.sqrt() * 3.0,
        },
    )
    .unwrap();
    assert!(distance_mm(pos(&p, s, id), pt(3.0, 3.0)) < TOL);
    // le parent bouge : la bissectrice suit (angle de 60°)
    mv(&mut p, s, b, pt(5.0, 5.0 * 3.0_f64.sqrt())).unwrap();
    let t = 30.0_f64.to_radians();
    let d = 2.0_f64.sqrt() * 3.0;
    assert!(distance_mm(pos(&p, s, id), pt(d * t.cos(), d * t.sin())) < TOL);
    // points confondus : refus, état inchangé
    let before = snapshot(&p);
    assert!(mv(&mut p, s, b, pt(0.0, 0.0)).is_err());
    assert_eq!(snapshot(&p), before);
    // angle plat : refus
    assert!(mv(&mut p, s, b, pt(-10.0, 0.0)).is_err());
    assert_eq!(snapshot(&p), before);
    // création avec un point confondu avec le sommet
    let same = add(
        &mut p,
        s,
        PointDefinition::Bisector {
            vertex: v,
            first: v,
            second: b,
            distance_mm: 1.0,
        },
    )
    .unwrap_err()
    .to_string();
    assert!(same.contains("confondu"), "{same}");
    assert_eq!(snapshot(&p), before);
    values(&mut p, s, id, &[4.0]).unwrap();
    assert!(distance_mm(pos(&p, s, id), pt(4.0 * t.cos(), 4.0 * t.sin())) < TOL);
    assert!(values(&mut p, s, id, &[0.0]).is_err());
}

// --- direction + distance ------------------------------------------------------------

#[test]
fn direction_absolue_et_relative_a_un_trace_avec_suivi() {
    let (mut p, s) = setup();
    let o = free(&mut p, s, 5.0, 5.0);
    let abs = add(
        &mut p,
        s,
        PointDefinition::Polar {
            from: o,
            distance_mm: 10.0,
            angle_deg: 90.0,
            reference: None,
        },
    )
    .unwrap();
    assert_eq!(pos(&p, s, abs), pt(5.0, 15.0));
    let (line, _, end) = seg(&mut p, s, pt(0.0, 30.0), pt(20.0, 30.0));
    let rel = add(
        &mut p,
        s,
        PointDefinition::Polar {
            from: o,
            distance_mm: 10.0,
            angle_deg: 30.0,
            reference: Some(line),
        },
    )
    .unwrap();
    let t = 30.0_f64.to_radians();
    assert!(
        distance_mm(
            pos(&p, s, rel),
            pt(5.0 + 10.0 * t.cos(), 5.0 + 10.0 * t.sin())
        ) < TOL
    );
    // le tracé de référence tourne de 90° : le point relatif suit
    mv(&mut p, s, end, pt(0.0, 50.0)).unwrap();
    let t = 120.0_f64.to_radians();
    assert!(
        distance_mm(
            pos(&p, s, rel),
            pt(5.0 + 10.0 * t.cos(), 5.0 + 10.0 * t.sin())
        ) < TOL
    );
    values(&mut p, s, rel, &[4.0, -90.0]).unwrap();
    assert!(distance_mm(pos(&p, s, rel), pt(5.0 + 4.0, 5.0)) < TOL);
    let before = snapshot(&p);
    assert!(values(&mut p, s, rel, &[0.0, 10.0]).is_err());
    assert!(values(&mut p, s, rel, &[1.0, f64::NAN]).is_err());
    assert_eq!(snapshot(&p), before);
    // référence qui n'est pas un segment : refus
    let (arc, _) = quarter(&mut p, s);
    assert!(add(
        &mut p,
        s,
        PointDefinition::Polar {
            from: o,
            distance_mm: 1.0,
            angle_deg: 0.0,
            reference: Some(arc),
        },
    )
    .is_err());
}

// --- compas ----------------------------------------------------------------------------

#[test]
fn compas_deux_solutions_choix_par_proximite_suivi_et_refus() {
    let (mut p, s) = setup();
    let a = free(&mut p, s, 0.0, 0.0);
    let b = free(&mut p, s, 6.0, 0.0);
    let compass = |near: PointMm| PointDefinition::Compass {
        center_a: a,
        center_b: b,
        radius_a_mm: 5.0,
        radius_b_mm: 5.0,
        near: near.into(),
    };
    let up = add(&mut p, s, compass(pt(3.0, 10.0))).unwrap();
    let down = add(&mut p, s, compass(pt(3.0, -10.0))).unwrap();
    assert!(distance_mm(pos(&p, s, up), pt(3.0, 4.0)) < TOL);
    assert!(distance_mm(pos(&p, s, down), pt(3.0, -4.0)) < TOL);
    // B s'éloigne : les deux points suivent (même côté)
    mv(&mut p, s, b, pt(8.0, 0.0)).unwrap();
    assert!(distance_mm(pos(&p, s, up), pt(4.0, 3.0)) < TOL);
    assert!(distance_mm(pos(&p, s, down), pt(4.0, -3.0)) < TOL);
    // rayons modifiables
    values(&mut p, s, up, &[5.0, 5.0]).unwrap();
    // B trop loin : les arcs ne se coupent plus, déplacement refusé sans rien changer
    let before = snapshot(&p);
    let err = mv(&mut p, s, b, pt(30.0, 0.0)).unwrap_err().to_string();
    assert!(err.contains("ne se coupent pas"), "{err}");
    assert_eq!(snapshot(&p), before);
    assert!(values(&mut p, s, up, &[1.0, 1.0]).is_err());
    assert!(values(&mut p, s, up, &[-1.0, 5.0]).is_err());
    assert_eq!(snapshot(&p), before);
    // création impossible
    let far = PointDefinition::Compass {
        center_a: a,
        center_b: b,
        radius_a_mm: 1.0,
        radius_b_mm: 1.0,
        near: pt(0.0, 0.0).into(),
    };
    assert!(add(&mut p, s, far).is_err());
    assert_eq!(snapshot(&p), before);
    // centres confondus
    let same = PointDefinition::Compass {
        center_a: a,
        center_b: a,
        radius_a_mm: 1.0,
        radius_b_mm: 2.0,
        near: pt(0.0, 0.0).into(),
    };
    assert!(add(&mut p, s, same)
        .unwrap_err()
        .to_string()
        .contains("confondus"));
    assert_eq!(snapshot(&p), before);
}

#[test]
fn compas_tangent_donne_une_solution_exacte() {
    let (mut p, s) = setup();
    let a = free(&mut p, s, 0.0, 0.0);
    let b = free(&mut p, s, 10.0, 0.0);
    let id = add(
        &mut p,
        s,
        PointDefinition::Compass {
            center_a: a,
            center_b: b,
            radius_a_mm: 4.0,
            radius_b_mm: 6.0,
            near: pt(4.0, 1.0).into(),
        },
    )
    .unwrap();
    assert!(distance_mm(pos(&p, s, id), pt(4.0, 0.0)) < TOL);
}

// --- cycles ----------------------------------------------------------------------------

/// Change un champ d'identifiant de la définition d'un point dans le JSON.
fn forge(json: &str, point: PointId, field: &str, to: u64) -> String {
    fn walk(v: &mut serde_json::Value, point: u64, field: &str, to: u64, hit: &mut bool) {
        match v {
            serde_json::Value::Object(m) => {
                if m.get("id").and_then(|i| i.as_u64()) == Some(point) {
                    if let Some(serde_json::Value::Object(d)) = m.get_mut("definition") {
                        d.insert(field.into(), serde_json::json!(to));
                        *hit = true;
                    }
                }
                m.values_mut().for_each(|c| walk(c, point, field, to, hit));
            }
            serde_json::Value::Array(a) => {
                a.iter_mut().for_each(|c| walk(c, point, field, to, hit))
            }
            _ => {}
        }
    }
    let mut value: serde_json::Value = serde_json::from_str(json).unwrap();
    let mut hit = false;
    walk(&mut value, point.0, field, to, &mut hit);
    assert!(hit);
    serde_json::to_string(&value).unwrap()
}

#[test]
fn cycles_refuses_pour_chaque_type_de_point_relatif() {
    let (mut p, s) = setup();
    let a = free(&mut p, s, 0.0, 0.0);
    let b = free(&mut p, s, 10.0, 0.0);
    let c = free(&mut p, s, 0.0, 10.0);
    let (line, _, _) = seg(&mut p, s, pt(40.0, 40.0), pt(60.0, 40.0));
    let (arc, _) = quarter(&mut p, s);
    let defs: Vec<(PointDefinition, &str)> = vec![
        (
            PointDefinition::Offset {
                from: a,
                dx_mm: 1.0,
                dy_mm: 2.0,
            },
            "from",
        ),
        (
            PointDefinition::Midpoint {
                first: a,
                second: b,
            },
            "first",
        ),
        (
            PointDefinition::Midpoint {
                first: a,
                second: b,
            },
            "second",
        ),
        (
            PointDefinition::Projection {
                point: c,
                trace: line,
            },
            "point",
        ),
        (
            PointDefinition::Bisector {
                vertex: a,
                first: b,
                second: c,
                distance_mm: 2.0,
            },
            "vertex",
        ),
        (
            PointDefinition::Bisector {
                vertex: a,
                first: b,
                second: c,
                distance_mm: 2.0,
            },
            "first",
        ),
        (
            PointDefinition::Polar {
                from: a,
                distance_mm: 3.0,
                angle_deg: 10.0,
                reference: None,
            },
            "from",
        ),
        (
            PointDefinition::Compass {
                center_a: a,
                center_b: b,
                radius_a_mm: 6.0,
                radius_b_mm: 6.0,
                near: pt(5.0, 5.0).into(),
            },
            "center_a",
        ),
        (
            PointDefinition::Compass {
                center_a: a,
                center_b: b,
                radius_a_mm: 6.0,
                radius_b_mm: 6.0,
                near: pt(5.0, 5.0).into(),
            },
            "center_b",
        ),
    ];
    for (d, field) in defs {
        let mut q = p.clone();
        let id = add(&mut q, s, d).unwrap();
        let json = persist::to_json(&q).unwrap();
        persist::from_json(&json).unwrap();
        // un parent devient le point lui-même
        let cyclic = forge(&json, id, field, id.0);
        let err = persist::from_json(&cyclic).unwrap_err();
        assert!(
            matches!(err, CoreError::CyclicReference(_)),
            "{field} : {err:?}"
        );
    }
    // cycle indirect : le segment de référence part du point qu'il sert à définir
    let o = free(&mut p, s, 5.0, 5.0);
    let (rseg, rs, _) = seg(&mut p, s, pt(70.0, 0.0), pt(80.0, 0.0));
    let id = add(
        &mut p,
        s,
        PointDefinition::Polar {
            from: o,
            distance_mm: 3.0,
            angle_deg: 10.0,
            reference: Some(rseg),
        },
    )
    .unwrap();
    let json = persist::to_json(&p).unwrap();
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    fn swap(v: &mut serde_json::Value, from: u64, to: u64) {
        match v {
            serde_json::Value::Object(m) => {
                if m.get("from").and_then(|x| x.as_u64()) == Some(from) && m.contains_key("to") {
                    m.insert("from".into(), serde_json::json!(to));
                }
                m.values_mut().for_each(|c| swap(c, from, to));
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(|c| swap(c, from, to)),
            _ => {}
        }
    }
    swap(&mut value, rs.0, id.0);
    let err = persist::from_json(&value.to_string()).unwrap_err();
    assert!(matches!(err, CoreError::CyclicReference(_)), "{err:?}");
    let _ = arc;
}

// --- suppression, copies, fichiers -------------------------------------------------------

#[test]
fn supprimer_un_parent_est_refuse_supprimer_le_tracé_fige_le_point() {
    let (mut p, s) = setup();
    let a = free(&mut p, s, 0.0, 0.0);
    let m = add(
        &mut p,
        s,
        PointDefinition::Offset {
            from: a,
            dx_mm: 3.0,
            dy_mm: 4.0,
        },
    )
    .unwrap();
    let before = snapshot(&p);
    let err = p
        .apply(Command::Delete {
            sheet: s,
            points: vec![a],
            entities: vec![],
        })
        .unwrap_err()
        .to_string();
    assert!(err.contains("sert de parent"), "{err}");
    assert_eq!(snapshot(&p), before);
    // en supprimant les deux, c'est accepté
    p.apply(Command::Delete {
        sheet: s,
        points: vec![a, m],
        entities: vec![],
    })
    .unwrap();
    assert!(p.sheet(s).unwrap().points.is_empty());

    // tracé supprimé : le point devient libre à sa position actuelle
    let (mut p, s) = setup();
    let (arc, _) = quarter(&mut p, s);
    let id = add(
        &mut p,
        s,
        PointDefinition::AlongCurve {
            curve: arc,
            distance_mm: 12.0,
            origin: SegmentEnd::Start,
        },
    )
    .unwrap();
    let at = pos(&p, s, id);
    p.apply(Command::Delete {
        sheet: s,
        points: vec![],
        entities: vec![arc],
    })
    .unwrap();
    assert!(p.sheet(s).unwrap().point(id).unwrap().definition.is_free());
    assert!(distance_mm(pos(&p, s, id), at) < TOL);
}

#[test]
fn fichiers_octets_inchanges_sans_ces_points_et_relecture_exacte() {
    let (mut p, s) = setup();
    let a = free(&mut p, s, 0.0, 0.0);
    let b = free(&mut p, s, 10.0, 0.0);
    let (arc, _) = quarter(&mut p, s);
    let plain = persist::to_json(&p).unwrap();
    assert!(plain.contains("\"schema_version\": 6"));
    for t in [
        "along_curve",
        "offset",
        "midpoint",
        "projection",
        "bisector",
        "polar",
        "compass",
    ] {
        assert!(!plain.contains(&format!("\"type\": \"{t}\"")), "{t}");
    }
    let c = free(&mut p, s, 20.0, 30.0);
    add(
        &mut p,
        s,
        PointDefinition::AlongCurve {
            curve: arc,
            distance_mm: 7.0,
            origin: SegmentEnd::End,
        },
    )
    .unwrap();
    add(
        &mut p,
        s,
        PointDefinition::Offset {
            from: a,
            dx_mm: 1.5,
            dy_mm: 2.5,
        },
    )
    .unwrap();
    add(
        &mut p,
        s,
        PointDefinition::Midpoint {
            first: a,
            second: b,
        },
    )
    .unwrap();
    add(
        &mut p,
        s,
        PointDefinition::Projection {
            point: c,
            trace: arc,
        },
    )
    .unwrap();
    add(
        &mut p,
        s,
        PointDefinition::Bisector {
            vertex: a,
            first: b,
            second: c,
            distance_mm: 3.0,
        },
    )
    .unwrap();
    add(
        &mut p,
        s,
        PointDefinition::Polar {
            from: b,
            distance_mm: 2.0,
            angle_deg: 45.0,
            reference: None,
        },
    )
    .unwrap();
    add(
        &mut p,
        s,
        PointDefinition::Compass {
            center_a: a,
            center_b: b,
            radius_a_mm: 7.0,
            radius_b_mm: 7.0,
            near: pt(5.0, 5.0).into(),
        },
    )
    .unwrap();
    let json = persist::to_json(&p).unwrap();
    assert!(json.contains("\"schema_version\": 6"));
    for t in [
        "along_curve",
        "offset",
        "midpoint",
        "projection",
        "bisector",
        "polar",
        "compass",
    ] {
        assert!(json.contains(&format!("\"type\": \"{t}\"")), "{t}");
    }
    let back = persist::from_json(&json).unwrap();
    assert_eq!(back, p);
    assert_eq!(persist::to_json(&back).unwrap(), json);
    // champ inconnu dans une définition : refusé
    let bad = json.replacen("\"dx_mm\"", "\"dx_mmm\"", 1);
    assert!(persist::from_json(&bad).is_err());
}

#[test]
fn copie_de_feuille_et_presse_papiers_conservent_la_construction() {
    let (mut p, s) = setup();
    let (_, a, b) = seg(&mut p, s, pt(0.0, 0.0), pt(10.0, 0.0));
    let m = add(
        &mut p,
        s,
        PointDefinition::Midpoint {
            first: a,
            second: b,
        },
    )
    .unwrap();
    let o = add(
        &mut p,
        s,
        PointDefinition::Offset {
            from: m,
            dx_mm: 0.0,
            dy_mm: 5.0,
        },
    )
    .unwrap();
    p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    let copy = p.sheets[1].clone();
    let q = copy
        .points
        .iter()
        .find(|q| matches!(q.definition, PointDefinition::Offset { .. }))
        .unwrap();
    let PointDefinition::Offset { from, .. } = q.definition else {
        unreachable!()
    };
    assert_ne!(from, m, "la copie suit ses propres parents");
    assert!(copy.point(from).is_ok());
    assert!(distance_mm(copy.position(q.id).unwrap(), pt(5.0, 5.0)) < TOL);

    // presse-papiers : tout copié, la construction est maintenue
    let sheet = p.sheet(s).unwrap();
    let ents: Vec<EntityId> = sheet.entities.iter().map(|e| e.id).collect();
    let frag = p.copy_fragment(s, &[m, o], &ents, &[], &[]).unwrap();
    frag.validate().unwrap();
    p.apply(Command::PasteFragment {
        sheet: s,
        fragment: Box::new(frag),
        layer: None,
    })
    .unwrap();
    let sheet = p.sheet(s).unwrap();
    assert_eq!(
        sheet
            .points
            .iter()
            .filter(|q| matches!(q.definition, PointDefinition::Offset { .. }))
            .count(),
        2
    );
    // parents non copiés : le point devient libre dans le fragment
    let frag = p.copy_fragment(s, &[o], &[], &[], &[]).unwrap();
    let fp = frag.points.iter().find(|q| q.id == o).unwrap();
    assert!(fp.definition.is_free());
    // fragment forgé avec un parent absent : refusé
    let json = serde_json::to_string(&p.copy_fragment(s, &[m], &ents, &[], &[]).unwrap()).unwrap();
    let broken = forge_fragment(&json, m, a.0 + 9999);
    assert!(Fragment::from_json(&broken).is_err());
}

fn forge_fragment(json: &str, point: PointId, to: u64) -> String {
    fn walk(v: &mut serde_json::Value, point: u64, to: u64) {
        match v {
            serde_json::Value::Object(m) => {
                if m.get("id").and_then(|i| i.as_u64()) == Some(point) {
                    if let Some(serde_json::Value::Object(d)) = m.get_mut("definition") {
                        d.insert("first".into(), serde_json::json!(to));
                    }
                }
                m.values_mut().for_each(|c| walk(c, point, to));
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(|c| walk(c, point, to)),
            _ => {}
        }
    }
    let mut value: serde_json::Value = serde_json::from_str(json).unwrap();
    walk(&mut value, point.0, to);
    serde_json::to_string(&value).unwrap()
}

#[test]
fn refus_sans_trace_sur_references_absentes() {
    let (mut p, s) = setup();
    let a = free(&mut p, s, 0.0, 0.0);
    let before = snapshot(&p);
    for d in [
        PointDefinition::Offset {
            from: PointId(9999),
            dx_mm: 1.0,
            dy_mm: 1.0,
        },
        PointDefinition::Midpoint {
            first: a,
            second: PointId(9999),
        },
        PointDefinition::Projection {
            point: a,
            trace: EntityId(9999),
        },
        PointDefinition::AlongCurve {
            curve: EntityId(9999),
            distance_mm: 1.0,
            origin: SegmentEnd::Start,
        },
    ] {
        assert!(add(&mut p, s, d).is_err());
        assert_eq!(snapshot(&p), before);
    }
}

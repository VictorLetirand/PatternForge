//! US-02.7 — modifier un tracé : prolonger / raccourcir, couper, fusionner,
//! inverser. Un test par critère d'acceptation et par type de tracé, les refus
//! sans trace, le verrou de calque, l'annulation en une étape, les crans, les
//! cotations, les groupes, les pièces et la sérialisation.

use pf_core::contour::{closed_contours, measure_contour};
use pf_core::*;
use pf_geometry::{distance_mm, PointMm, Side};

const LEN_TOL: f64 = 1e-9;
const POS_TOL: f64 = 1e-6;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn setup() -> (Project, SheetId) {
    let p = Project::new("Modifier un tracé");
    let s = p.sheets[0].id;
    (p, s)
}

fn snapshot(p: &Project) -> String {
    serde_json::to_string(p).unwrap()
}

fn seg(p: &mut Project, s: SheetId, a: PointMm, b: PointMm) -> EntityId {
    p.apply(Command::AddSegment {
        sheet: s,
        from: PointRef::New(a),
        to: PointRef::New(b),
    })
    .unwrap()
    .entities[0]
}

fn arc(p: &mut Project, s: SheetId, c: PointMm, r: f64, a0: f64, a1: f64) -> EntityId {
    p.apply(Command::AddArc {
        sheet: s,
        center: PointRef::New(c),
        radius_mm: r,
        start_deg: a0,
        end_deg: a1,
        ccw: true,
    })
    .unwrap()
    .entities[0]
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

fn bezier(p: &mut Project, s: SheetId) -> EntityId {
    p.apply(Command::AddCurve {
        sheet: s,
        start: PointRef::New(pt(0.0, 0.0)),
        c1: pt(20.0, 60.0),
        c2: pt(70.0, -40.0),
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
            PointRef::New(pt(60.0, -10.0)),
            PointRef::New(pt(100.0, 20.0)),
        ],
    })
    .unwrap()
    .entities[0]
}

fn notch(p: &mut Project, s: SheetId, e: EntityId, d: f64, side: Side) -> NotchId {
    p.apply(Command::AddNotch {
        sheet: s,
        entity: e,
        distance_mm: d,
        side,
        length_mm: 5.0,
        style: NotchStyle::Single,
    })
    .unwrap()
    .notches[0]
}

fn len(p: &Project, s: SheetId, e: EntityId) -> f64 {
    p.sheet(s).unwrap().path_length_mm(e).unwrap()
}

/// Traits de tous les crans (pied, pointe), triés : position géométrique des crans.
fn marks(p: &Project, s: SheetId) -> Vec<[f64; 4]> {
    let sh = p.sheet(s).unwrap();
    let mut v: Vec<[f64; 4]> = sh
        .notches
        .iter()
        .flat_map(|n| sh.notch_marks(n).unwrap())
        .map(|(a, b)| [a.x, a.y, b.x, b.y])
        .collect();
    v.sort_by(|a, b| {
        a.iter()
            .zip(b)
            .map(|(x, y)| x.total_cmp(y))
            .find(|o| o.is_ne())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    v
}

fn assert_same_marks(a: &[[f64; 4]], b: &[[f64; 4]], tol: f64) {
    assert_eq!(a.len(), b.len(), "nombre de traits de crans");
    for (x, y) in a.iter().zip(b) {
        for k in 0..4 {
            assert!((x[k] - y[k]).abs() <= tol, "cran déplacé : {x:?} -> {y:?}");
        }
    }
}

fn pos(p: &Project, s: SheetId, id: PointId) -> PointMm {
    p.sheet(s).unwrap().position(id).unwrap()
}

fn ends(p: &Project, s: SheetId, e: EntityId) -> (PointMm, PointMm) {
    let sh = p.sheet(s).unwrap();
    let (a, b) = sh.entity_points(e).unwrap();
    (sh.position(a).unwrap(), sh.position(b).unwrap())
}

/// Échantillon dense d'un tracé (pour comparer deux formes).
fn dense(p: &Project, s: SheetId, e: EntityId, n: usize) -> Vec<PointMm> {
    let sh = p.sheet(s).unwrap();
    let l = sh.path_length_mm(e).unwrap();
    (0..=n)
        .map(|i| sh.path_point_at(e, l * i as f64 / n as f64).unwrap().0)
        .collect()
}

/// Distance maximale de chaque point de `pts` à la forme du tracé `e`.
fn max_gap(p: &Project, s: SheetId, e: EntityId, pts: &[PointMm]) -> f64 {
    let sh = p.sheet(s).unwrap();
    let shape = sh.shape(e).unwrap();
    // `distance_to_point` des courbes passe par une polyligne grossière : on en
    // prend une très fine (erreur de flèche < 1e-6 mm sur ces tracés).
    let fine: Option<Vec<PointMm>> = match &shape {
        Shape::Cubic(_) | Shape::Spline(_) => Some(shape.sample(20_000)),
        _ => None,
    };
    pts.iter()
        .map(|q| match &fine {
            Some(poly) => poly
                .windows(2)
                .map(|w| pf_geometry::project_on_segment(*q, w[0], w[1]).unwrap().1)
                .fold(f64::INFINITY, f64::min),
            None => shape.distance_to_point(*q),
        })
        .fold(0.0, f64::max)
}

/// Tolérance de forme : exacte pour segments, arcs, cercles ; polyligne fine sinon.
fn shape_tol(label: &str) -> f64 {
    if label == "bézier" || label == "courbe lisse" {
        2e-6
    } else {
        1e-9
    }
}

fn lock_layer_of(p: &mut Project, s: SheetId, e: EntityId) {
    let l = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Verrouillé".into(),
        })
        .unwrap()
        .layers[0];
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![],
        entities: vec![e],
        layer: Some(l),
    })
    .unwrap();
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer: l,
        locked: true,
    })
    .unwrap();
}

fn refused(p: &mut Project, cmd: Command, what: &str) -> String {
    let before = snapshot(p);
    let err = p.apply(cmd).expect_err(what).to_string();
    assert_eq!(
        snapshot(p),
        before,
        "{what} : le refus doit être sans trace"
    );
    assert!(!err.is_empty());
    err
}

// ------------------------------------------------------------------ prolonger / raccourcir

#[test]
fn prolonger_un_segment_par_chaque_extremite_et_recaler_les_crans() {
    for end in [SegmentEnd::Start, SegmentEnd::End] {
        let (mut p, s) = setup();
        let e = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
        notch(&mut p, s, e, 30.0, Side::Left);
        notch(&mut p, s, e, 80.0, Side::Right);
        let marks_before = marks(&p, s);
        let out = p
            .apply(Command::ExtendPath {
                sheet: s,
                entity: e,
                end,
                delta_mm: 25.0,
            })
            .unwrap();
        assert!(out.notes.iter().all(|n| !n.contains("supprimé")));
        assert!((len(&p, s, e) - 125.0).abs() < LEN_TOL);
        let (a, b) = ends(&p, s, e);
        match end {
            SegmentEnd::End => {
                assert!(distance_mm(a, pt(0.0, 0.0)) < LEN_TOL);
                assert!(distance_mm(b, pt(125.0, 0.0)) < LEN_TOL);
            }
            SegmentEnd::Start => {
                assert!(distance_mm(a, pt(-25.0, 0.0)) < LEN_TOL);
                assert!(distance_mm(b, pt(100.0, 0.0)) < LEN_TOL);
            }
        }
        // CA : crans à la même position géométrique (1e-6 mm)
        assert_same_marks(&marks_before, &marks(&p, s), POS_TOL);
    }
}

#[test]
fn raccourcir_un_segment_supprime_les_crans_perdus_avec_message() {
    let (mut p, s) = setup();
    let e = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    notch(&mut p, s, e, 20.0, Side::Left);
    notch(&mut p, s, e, 90.0, Side::Left);
    let out = p
        .apply(Command::ExtendPath {
            sheet: s,
            entity: e,
            end: SegmentEnd::End,
            delta_mm: -30.0,
        })
        .unwrap();
    assert!((len(&p, s, e) - 70.0).abs() < LEN_TOL);
    assert_eq!(p.sheet(s).unwrap().notches.len(), 1);
    assert!(
        out.notes
            .iter()
            .any(|n| n.contains("cran") && n.contains("supprimé")),
        "{:?}",
        out.notes
    );
    // par le départ : les crans restants sont recalés
    let (mut p, s) = setup();
    let e = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    notch(&mut p, s, e, 60.0, Side::Left);
    let before = marks(&p, s);
    p.apply(Command::ExtendPath {
        sheet: s,
        entity: e,
        end: SegmentEnd::Start,
        delta_mm: -40.0,
    })
    .unwrap();
    assert!((len(&p, s, e) - 60.0).abs() < LEN_TOL);
    assert_same_marks(&before, &marks(&p, s), POS_TOL);
}

#[test]
fn prolonger_et_raccourcir_un_arc() {
    for end in [SegmentEnd::Start, SegmentEnd::End] {
        for delta in [12.0, -12.0] {
            let (mut p, s) = setup();
            let e = arc(&mut p, s, pt(10.0, 5.0), 50.0, 20.0, 110.0);
            let l0 = len(&p, s, e);
            notch(&mut p, s, e, l0 / 2.0, Side::Left);
            let before = marks(&p, s);
            let base = dense(&p, s, e, 20);
            p.apply(Command::ExtendPath {
                sheet: s,
                entity: e,
                end,
                delta_mm: delta,
            })
            .unwrap();
            assert!(
                (len(&p, s, e) - (l0 + delta)).abs() < LEN_TOL,
                "{end:?} {delta}"
            );
            let g = p.sheet(s).unwrap().arc_geometry(e).unwrap();
            assert!((g.radius_mm - 50.0).abs() < LEN_TOL);
            assert_same_marks(&before, &marks(&p, s), POS_TOL);
            if delta < 0.0 {
                // le reste de l'arc est sur le même cercle
                let (a, b) = ends(&p, s, e);
                assert!((distance_mm(a, pt(10.0, 5.0)) - 50.0).abs() < LEN_TOL);
                assert!((distance_mm(b, pt(10.0, 5.0)) - 50.0).abs() < LEN_TOL);
            } else {
                assert!(
                    max_gap(&p, s, e, &base) < LEN_TOL,
                    "l'arc prolonge l'ancien"
                );
            }
        }
    }
}

#[test]
fn prolonger_refus_sans_trace() {
    let (mut p, s) = setup();
    let e = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let a = arc(&mut p, s, pt(0.0, 50.0), 20.0, 0.0, 90.0);
    let c = bezier(&mut p, s);
    let sp = spline(&mut p, s);
    let ci = circle(&mut p, s, pt(200.0, 0.0), 10.0);
    let ext = |entity, delta_mm| Command::ExtendPath {
        sheet: s,
        entity,
        end: SegmentEnd::End,
        delta_mm,
    };
    for d in [0.0, f64::NAN, f64::INFINITY, -100.0, -250.0, 1e300, -1e300] {
        refused(&mut p, ext(e, d), &format!("segment {d}"));
    }
    // l'arc ne devient pas un cercle ; ni ne disparaît
    let circ = 2.0 * std::f64::consts::PI * 20.0;
    for d in [circ, circ * 2.0, -circ, f64::NAN] {
        refused(&mut p, ext(a, d), &format!("arc {d}"));
    }
    // courbes et cercles : refus documenté
    for t in [c, sp, ci] {
        let msg = refused(&mut p, ext(t, 5.0), "courbe ou cercle");
        assert!(msg.contains("segments et les arcs"), "{msg}");
    }
    refused(&mut p, ext(EntityId(9_999), 1.0), "tracé absent");
}

#[test]
fn prolonger_jusqu_a_un_autre_trace_segment_et_arc() {
    // segment prolongé jusqu'à une verticale
    let (mut p, s) = setup();
    let e = seg(&mut p, s, pt(0.0, 0.0), pt(40.0, 0.0));
    let wall = seg(&mut p, s, pt(100.0, -50.0), pt(100.0, 50.0));
    notch(&mut p, s, e, 10.0, Side::Left);
    let before = marks(&p, s);
    p.apply(Command::TrimPathTo {
        sheet: s,
        entity: e,
        end: SegmentEnd::End,
        target: wall,
        pick: pt(90.0, 0.0),
    })
    .unwrap();
    let (a, b) = ends(&p, s, e);
    assert!(distance_mm(a, pt(0.0, 0.0)) < LEN_TOL);
    assert!(distance_mm(b, pt(100.0, 0.0)) < LEN_TOL, "{b:?}");
    assert_same_marks(&before, &marks(&p, s), POS_TOL);

    // raccourci jusqu'à la même verticale (le segment la dépasse)
    let (mut p, s) = setup();
    let e = seg(&mut p, s, pt(0.0, 0.0), pt(160.0, 0.0));
    let wall = seg(&mut p, s, pt(100.0, -50.0), pt(100.0, 50.0));
    p.apply(Command::TrimPathTo {
        sheet: s,
        entity: e,
        end: SegmentEnd::End,
        target: wall,
        pick: pt(100.0, 0.0),
    })
    .unwrap();
    assert!((len(&p, s, e) - 100.0).abs() < LEN_TOL);

    // par le départ
    let (mut p, s) = setup();
    let e = seg(&mut p, s, pt(50.0, 0.0), pt(160.0, 0.0));
    let wall = seg(&mut p, s, pt(0.0, -50.0), pt(0.0, 50.0));
    p.apply(Command::TrimPathTo {
        sheet: s,
        entity: e,
        end: SegmentEnd::Start,
        target: wall,
        pick: pt(0.0, 0.0),
    })
    .unwrap();
    let (a, _) = ends(&p, s, e);
    assert!(distance_mm(a, pt(0.0, 0.0)) < LEN_TOL);
    assert!((len(&p, s, e) - 160.0).abs() < LEN_TOL);

    // segment prolongé jusqu'à un cercle : deux solutions, la plus proche du clic
    let (mut p, s) = setup();
    let e = seg(&mut p, s, pt(0.0, 0.0), pt(10.0, 0.0));
    let c = circle(&mut p, s, pt(100.0, 0.0), 30.0);
    p.apply(Command::TrimPathTo {
        sheet: s,
        entity: e,
        end: SegmentEnd::End,
        target: c,
        pick: pt(70.0, 0.0),
    })
    .unwrap();
    assert!(distance_mm(ends(&p, s, e).1, pt(70.0, 0.0)) < LEN_TOL);

    // arc prolongé jusqu'à un segment (par son support : le cercle)
    let (mut p, s) = setup();
    let a = arc(&mut p, s, pt(0.0, 0.0), 50.0, 0.0, 40.0);
    let wall = seg(&mut p, s, pt(-100.0, 35.0), pt(100.0, 35.0));
    p.apply(Command::TrimPathTo {
        sheet: s,
        entity: a,
        end: SegmentEnd::End,
        target: wall,
        pick: pt(10.0, 35.0),
    })
    .unwrap();
    let (_, b) = ends(&p, s, a);
    assert!((b.y - 35.0).abs() < LEN_TOL, "{b:?}");
    assert!((distance_mm(b, pt(0.0, 0.0)) - 50.0).abs() < LEN_TOL);
    assert!(b.x > 0.0, "solution du côté du clic");
}

#[test]
fn prolonger_jusqu_a_un_autre_trace_refus_sans_trace() {
    let (mut p, s) = setup();
    let e = seg(&mut p, s, pt(0.0, 0.0), pt(40.0, 0.0));
    let far = seg(&mut p, s, pt(100.0, 10.0), pt(100.0, 50.0)); // ne coupe pas la droite
    let par = seg(&mut p, s, pt(0.0, 20.0), pt(40.0, 20.0)); // parallèle
    let on = seg(&mut p, s, pt(40.0, -10.0), pt(40.0, 10.0)); // s'arrête déjà dessus
    let sp = spline(&mut p, s);
    let cmd = |target, pick| Command::TrimPathTo {
        sheet: s,
        entity: e,
        end: SegmentEnd::End,
        target,
        pick,
    };
    refused(&mut p, cmd(far, pt(100.0, 0.0)), "pas d'intersection");
    refused(&mut p, cmd(par, pt(20.0, 20.0)), "parallèle");
    refused(&mut p, cmd(on, pt(40.0, 0.0)), "déjà à l'intersection");
    refused(&mut p, cmd(e, pt(0.0, 0.0)), "même tracé");
    refused(&mut p, cmd(EntityId(9_999), pt(0.0, 0.0)), "tracé absent");
    refused(&mut p, cmd(on, pt(f64::NAN, 0.0)), "NaN");
    refused(
        &mut p,
        Command::TrimPathTo {
            sheet: s,
            entity: sp,
            end: SegmentEnd::End,
            target: on,
            pick: pt(0.0, 0.0),
        },
        "courbe",
    );
}

#[test]
fn prolonger_fige_les_points_construits_et_suit_les_cotations() {
    let (mut p, s) = setup();
    let e = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let mid = p
        .apply(Command::AddPointOnSegment {
            sheet: s,
            segment: e,
            distance_mm: 30.0,
            origin: SegmentEnd::Start,
        })
        .unwrap()
        .points[0];
    let (pa, pb) = {
        let (a, b) = p.sheet(s).unwrap().entity_points(e).unwrap();
        (a, b)
    };
    p.apply(Command::AddDimension {
        sheet: s,
        from: pa,
        to: pb,
        offset_mm: 10.0,
    })
    .unwrap();
    let out = p
        .apply(Command::ExtendPath {
            sheet: s,
            entity: e,
            end: SegmentEnd::Start,
            delta_mm: 20.0,
        })
        .unwrap();
    let sh = p.sheet(s).unwrap();
    assert!(sh.point(mid).unwrap().definition.is_free(), "figé");
    assert!(
        distance_mm(pos(&p, s, mid), pt(30.0, 0.0)) < LEN_TOL,
        "à sa position"
    );
    assert!(
        out.notes.iter().any(|n| n.contains("libre")),
        "{:?}",
        out.notes
    );
    // la cotation du tracé mesure maintenant le tracé prolongé
    let d = &sh.dimensions[0];
    let (a, b) = (pos(&p, s, d.from), pos(&p, s, d.to));
    assert!((distance_mm(a, b) - 120.0).abs() < LEN_TOL);
}

// ------------------------------------------------------------------ couper

fn dressed_segment(p: &mut Project, s: SheetId) -> (EntityId, Vec<NotchId>) {
    let e = seg(p, s, pt(0.0, 0.0), pt(100.0, 40.0));
    let l = len(p, s, e);
    let n = vec![
        notch(p, s, e, l * 0.2, Side::Left),
        notch(p, s, e, l * 0.5, Side::Right),
        notch(p, s, e, l * 0.9, Side::Left),
    ];
    (e, n)
}

#[test]
fn couper_un_segment_longueur_crans_groupe_nom_role_calque() {
    let (mut p, s) = setup();
    let (e, _) = dressed_segment(&mut p, s);
    let other = seg(&mut p, s, pt(0.0, 100.0), pt(50.0, 100.0));
    p.apply(Command::AddLayer {
        sheet: s,
        name: "Coupe".into(),
    })
    .unwrap();
    let layer = p.sheet(s).unwrap().layers[0].id;
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![],
        entities: vec![e],
        layer: Some(layer),
    })
    .unwrap();
    p.apply(Command::SetPathRole {
        sheet: s,
        entity: e,
        role: PathRole::CuttingContour,
    })
    .unwrap();
    p.apply(Command::SetEntityName {
        sheet: s,
        entity: e,
        name: Some("Côté".into()),
    })
    .unwrap();
    p.apply(Command::CreateGroup {
        sheet: s,
        entities: vec![e, other],
    })
    .unwrap();
    let l0 = len(&p, s, e);
    let marks_before = marks(&p, s);
    let out = p
        .apply(Command::SplitPath {
            sheet: s,
            entity: e,
            at: pt(37.0, 14.8), // sur la droite y = 0,4 x
        })
        .unwrap();
    let new = out.entities[0];
    let sh = p.sheet(s).unwrap();
    // CA : longueur totale conservée (1e-9)
    assert!((len(&p, s, e) + len(&p, s, new) - l0).abs() < LEN_TOL);
    // CA : crans à la même position géométrique (1e-6), chacun sur le bon tronçon
    assert_same_marks(&marks_before, &marks(&p, s), POS_TOL);
    assert!(sh.notches.iter().any(|n| n.entity == e));
    assert!(sh.notches.iter().any(|n| n.entity == new));
    for n in &sh.notches {
        assert!(n.distance_mm <= len(&p, s, n.entity) + 1e-9);
    }
    // groupe, nom, rôle, calque
    let (e1, e2) = (sh.entity(e).unwrap(), sh.entity(new).unwrap());
    assert_eq!(
        (e1.role, e2.role),
        (PathRole::CuttingContour, PathRole::CuttingContour)
    );
    assert_eq!((e1.layer, e2.layer), (Some(layer), Some(layer)));
    assert_eq!(e1.name.as_deref(), Some("Côté"));
    assert_eq!(e2.name.as_deref(), Some("Côté (2)"));
    let g = sh.group_of(e).unwrap();
    assert!(g.members.contains(&new) && g.members.contains(&other));
    // continuité : le nouveau point est partagé
    let (_, end1) = sh.entity_points(e).unwrap();
    let (start2, _) = sh.entity_points(new).unwrap();
    assert_eq!(end1, start2);
    assert!(distance_mm(pos(&p, s, end1), pt(37.0, 14.8)) < 1e-9);
}

#[test]
fn couper_un_arc_et_un_bezier_et_une_courbe_lisse() {
    // arc
    let (mut p, s) = setup();
    let a = arc(&mut p, s, pt(5.0, 5.0), 40.0, 10.0, 200.0);
    let l0 = len(&p, s, a);
    notch(&mut p, s, a, l0 * 0.25, Side::Left);
    notch(&mut p, s, a, l0 * 0.75, Side::Right);
    let before = marks(&p, s);
    let base = dense(&p, s, a, 40);
    let at = p.sheet(s).unwrap().path_point_at(a, l0 * 0.4).unwrap().0;
    let new = p
        .apply(Command::SplitPath {
            sheet: s,
            entity: a,
            at,
        })
        .unwrap()
        .entities[0];
    assert!((len(&p, s, a) + len(&p, s, new) - l0).abs() < LEN_TOL);
    assert!((len(&p, s, a) - l0 * 0.4).abs() < 1e-7);
    assert_same_marks(&before, &marks(&p, s), POS_TOL);
    assert!(max_gap(&p, s, a, &base[..10]) < LEN_TOL);

    // Bézier : la forme est celle d'avant ; longueurs conservées à la tolérance
    // de la quadrature (1e-6 mm), pas 1e-9
    let (mut p, s) = setup();
    let c = bezier(&mut p, s);
    let l0 = len(&p, s, c);
    notch(&mut p, s, c, l0 * 0.3, Side::Left);
    notch(&mut p, s, c, l0 * 0.8, Side::Left);
    let before = marks(&p, s);
    let base = dense(&p, s, c, 60);
    let at = base[24];
    let new = p
        .apply(Command::SplitPath {
            sheet: s,
            entity: c,
            at,
        })
        .unwrap()
        .entities[0];
    let sum = len(&p, s, c) + len(&p, s, new);
    assert!(
        (sum - l0).abs() < 1e-6,
        "Bézier : écart de longueur {}",
        (sum - l0).abs()
    );
    assert_same_marks(&before, &marks(&p, s), 1e-5);
    let max = base
        .iter()
        .map(|q| max_gap(&p, s, c, &[*q]).min(max_gap(&p, s, new, &[*q])))
        .fold(0.0, f64::max);
    assert!(max < 2e-6, "la forme change de {max} mm");

    // courbe lisse : à un point de passage, forme et longueur exactes
    let (mut p, s) = setup();
    let sp = spline(&mut p, s);
    let l0 = len(&p, s, sp);
    notch(&mut p, s, sp, l0 * 0.2, Side::Left);
    notch(&mut p, s, sp, l0 * 0.9, Side::Right);
    let before = marks(&p, s);
    let base = dense(&p, s, sp, 80);
    let new = p
        .apply(Command::SplitPath {
            sheet: s,
            entity: sp,
            at: pt(30.0, 25.0),
        })
        .unwrap()
        .entities[0];
    let sum = len(&p, s, sp) + len(&p, s, new);
    assert!(
        (sum - l0).abs() < LEN_TOL,
        "courbe lisse : écart {}",
        (sum - l0).abs()
    );
    assert_same_marks(&before, &marks(&p, s), POS_TOL);
    let max = base
        .iter()
        .map(|q| max_gap(&p, s, sp, &[*q]).min(max_gap(&p, s, new, &[*q])))
        .fold(0.0, f64::max);
    assert!(max < 2e-6, "la forme change de {max} mm");
}

#[test]
fn couper_refus_sans_trace() {
    let (mut p, s) = setup();
    let e = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let a = arc(&mut p, s, pt(0.0, 200.0), 30.0, 0.0, 90.0);
    let c = circle(&mut p, s, pt(300.0, 0.0), 10.0);
    let sp = spline(&mut p, s);
    let cu = bezier(&mut p, s);
    let split = |entity, at| Command::SplitPath {
        sheet: s,
        entity,
        at,
    };
    let m = refused(&mut p, split(e, pt(50.0, 1.0)), "hors du tracé");
    assert!(m.contains("du tracé"), "{m}");
    refused(&mut p, split(e, pt(150.0, 0.0)), "au-delà de l'extrémité");
    refused(&mut p, split(e, pt(0.0, 0.0)), "extrémité de départ");
    refused(&mut p, split(e, pt(100.0, 0.0)), "extrémité d'arrivée");
    refused(&mut p, split(e, pt(1e-7, 0.0)), "trop près de l'extrémité");
    refused(&mut p, split(e, pt(f64::NAN, 0.0)), "NaN");
    refused(&mut p, split(e, pt(f64::INFINITY, 0.0)), "infini");
    refused(&mut p, split(a, pt(0.0, 200.0)), "arc : centre");
    refused(&mut p, split(c, pt(310.0, 0.0)), "cercle");
    refused(
        &mut p,
        split(sp, pt(15.0, 12.0)),
        "courbe lisse hors point de passage",
    );
    refused(&mut p, split(sp, pt(0.0, 0.0)), "courbe lisse : extrémité");
    refused(&mut p, split(cu, pt(50.0, 50.0)), "Bézier hors courbe");
    refused(&mut p, split(EntityId(9_999), pt(0.0, 0.0)), "tracé absent");
}

#[test]
fn couper_un_contour_ferme_met_a_jour_la_piece() {
    for reversed in [false, true] {
        let (mut p, s) = setup();
        let rect = p
            .apply(Command::AddRectangle {
                sheet: s,
                corner: PointRef::New(pt(0.0, 0.0)),
                width_mm: 100.0,
                height_mm: 60.0,
            })
            .unwrap();
        let piece = p
            .apply(Command::AddPiece {
                sheet: s,
                name: "Rabat".into(),
                quantity: 1,
                material: None,
                mirrored_pair: false,
            })
            .unwrap()
            .pieces[0];
        let edges: Vec<PieceEdge> = rect
            .entities
            .iter()
            .map(|&entity| PieceEdge {
                entity,
                reversed: false,
            })
            .collect();
        let mut edges = edges;
        edges[0].reversed = reversed;
        p.apply(Command::SetPieceOutline {
            piece,
            outline: edges,
            grainline: None,
        })
        .unwrap();
        let c0 = &closed_contours(p.sheet(s).unwrap())[0];
        let m0 = measure_contour(p.sheet(s).unwrap(), c0).unwrap();
        let new = p
            .apply(Command::SplitPath {
                sheet: s,
                entity: rect.entities[0],
                at: pt(40.0, 0.0),
            })
            .unwrap()
            .entities[0];
        let outline = &p.piece(piece).unwrap().outline;
        assert_eq!(outline.len(), 5);
        let i = outline.iter().position(|x| x.entity == new).unwrap();
        let j = outline
            .iter()
            .position(|x| x.entity == rect.entities[0])
            .unwrap();
        assert_eq!(outline[i].reversed, reversed);
        assert_eq!(i, if reversed { j - 1 } else { j + 1 });
        // le contour reste fermé, même périmètre et même aire
        let contours = closed_contours(p.sheet(s).unwrap());
        assert_eq!(contours.len(), 1);
        assert_eq!(contours[0].edges.len(), 5);
        let m1 = measure_contour(p.sheet(s).unwrap(), &contours[0]).unwrap();
        assert!((m1.perimeter_mm - m0.perimeter_mm).abs() < LEN_TOL);
        assert!((m1.area_mm2 - m0.area_mm2).abs() < 1e-7);
    }
}

// ------------------------------------------------------------------ fusionner

#[test]
fn couper_puis_fusionner_redonne_le_tracee_d_origine_segment_arc_bezier_courbe() {
    type Make = fn(&mut Project, SheetId) -> EntityId;
    let makes: [(&str, Make, f64, f64); 4] = [
        (
            "segment",
            |p, s| seg(p, s, pt(0.0, 0.0), pt(100.0, 40.0)),
            LEN_TOL,
            POS_TOL,
        ),
        (
            "arc",
            |p, s| arc(p, s, pt(5.0, 5.0), 40.0, 10.0, 200.0),
            LEN_TOL,
            POS_TOL,
        ),
        ("bézier", bezier, 1e-6, 1e-5),
        ("courbe lisse", spline, LEN_TOL, POS_TOL),
    ];
    for (label, make, len_tol, mark_tol) in makes {
        for split_first in [false, true] {
            let (mut p, s) = setup();
            let e = make(&mut p, s);
            let l0 = len(&p, s, e);
            notch(&mut p, s, e, l0 * 0.15, Side::Left);
            notch(&mut p, s, e, l0 * 0.55, Side::Right);
            notch(&mut p, s, e, l0 * 0.95, Side::Left);
            let marks0 = marks(&p, s);
            let base = dense(&p, s, e, 50);
            let at = if label == "courbe lisse" {
                pt(30.0, 25.0)
            } else {
                base[20]
            };
            let new = p
                .apply(Command::SplitPath {
                    sheet: s,
                    entity: e,
                    at,
                })
                .unwrap()
                .entities[0];
            let (first, second) = if split_first { (e, new) } else { (new, e) };
            let out = p
                .apply(Command::MergePaths {
                    sheet: s,
                    first,
                    second,
                })
                .unwrap();
            let survivor = out.entities[0];
            let sh = p.sheet(s).unwrap();
            assert_eq!(sh.entities.len(), 1, "{label} : un seul tracé après fusion");
            assert!(
                (len(&p, s, survivor) - l0).abs() < len_tol,
                "{label} : longueur {} / {l0}",
                len(&p, s, survivor)
            );
            assert_same_marks(&marks0, &marks(&p, s), mark_tol);
            let gap = max_gap(&p, s, survivor, &base);
            assert!(
                gap < shape_tol(label),
                "{label} : forme changée de {gap} mm"
            );
            assert_eq!(sh.notches.len(), 3);
            // le sens du tracé survivant est celui de `first`
            let (a, b) = ends(&p, s, survivor);
            let (oa, ob) = (base[0], base[50]);
            if split_first {
                assert!(
                    distance_mm(a, oa) < 1e-9 && distance_mm(b, ob) < 1e-9,
                    "{label}"
                );
            } else {
                // `first` est le second tronçon : le tracé fusionné garde son sens
                assert!(distance_mm(a, ob).min(distance_mm(a, oa)) < 1e-9, "{label}");
            }
        }
    }
}

#[test]
fn fusionner_segments_colineaires_sens_opposes_crans_et_cotations() {
    // deux segments dont le second est saisi à l'envers : A->P et B->P
    let (mut p, s) = setup();
    let e1 = seg(&mut p, s, pt(0.0, 0.0), pt(60.0, 0.0));
    let e2 = seg(&mut p, s, pt(100.0, 0.0), pt(60.0, 0.0));
    notch(&mut p, s, e1, 10.0, Side::Left);
    notch(&mut p, s, e2, 10.0, Side::Left); // à 90 mm de l'origine, côté bas
    let before = marks(&p, s);
    let out = p
        .apply(Command::MergePaths {
            sheet: s,
            first: e1,
            second: e2,
        })
        .unwrap();
    assert_eq!(out.entities, vec![e1]);
    assert!((len(&p, s, e1) - 100.0).abs() < LEN_TOL);
    assert_same_marks(&before, &marks(&p, s), POS_TOL);
    // le point de raccord inutile est supprimé
    assert_eq!(p.sheet(s).unwrap().points.len(), 2);
}

#[test]
fn fusionner_arcs_meme_cercle_et_refus() {
    let (mut p, s) = setup();
    let a1 = arc(&mut p, s, pt(0.0, 0.0), 50.0, 0.0, 60.0);
    let a2 = arc(&mut p, s, pt(0.0, 0.0), 50.0, 90.0, 130.0);
    // les deux arcs d'un même cercle ne se touchent pas
    let m = refused(
        &mut p,
        Command::MergePaths {
            sheet: s,
            first: a1,
            second: a2,
        },
        "non adjacents",
    );
    assert!(m.contains("adjacents"), "{m}");
    // construits par coupe : adjacents, mais de rayon différent → pas de fusion
    let (mut p, s) = setup();
    let a1 = arc(&mut p, s, pt(0.0, 0.0), 50.0, 0.0, 60.0);
    let (_, e1) = ends(&p, s, a1);
    let big = p
        .apply(Command::AddArcThreePoints {
            sheet: s,
            start: PointRef::New(e1),
            through: PointRef::New(pt(10.0, 70.0)),
            end: PointRef::New(pt(-30.0, 60.0)),
        })
        .unwrap()
        .entities[0];
    let m = refused(
        &mut p,
        Command::MergePaths {
            sheet: s,
            first: a1,
            second: big,
        },
        "cercles différents",
    );
    assert!(m.contains("même cercle"), "{m}");
    // arcs d'un même cercle mais de sens contraire
    let (mut p, s) = setup();
    let a = arc(&mut p, s, pt(0.0, 0.0), 50.0, 0.0, 120.0);
    let new = p
        .apply(Command::SplitPath {
            sheet: s,
            entity: a,
            at: pt(
                50.0 * 60f64.to_radians().cos(),
                50.0 * 60f64.to_radians().sin(),
            ),
        })
        .unwrap()
        .entities[0];
    p.apply(Command::ReversePath {
        sheet: s,
        entity: new,
    })
    .unwrap();
    // inversé, le second arc tourne dans l'autre sens mais reste adjacent : fusion
    // acceptée (le sens du premier est conservé) et l'arc obtenu est le même
    let l0 = len(&p, s, a) + len(&p, s, new);
    p.apply(Command::MergePaths {
        sheet: s,
        first: a,
        second: new,
    })
    .unwrap();
    assert!((len(&p, s, a) - l0).abs() < LEN_TOL);
}

#[test]
fn fusionner_refus_sans_trace() {
    let (mut p, s) = setup();
    let e1 = seg(&mut p, s, pt(0.0, 0.0), pt(50.0, 0.0));
    let e2 = seg(&mut p, s, pt(50.0, 0.0), pt(100.0, 10.0)); // adjacent, non colinéaire
    let back = seg(&mut p, s, pt(50.0, 0.0), pt(20.0, 0.0)); // se replie sur e1
    let lone = seg(&mut p, s, pt(300.0, 0.0), pt(350.0, 0.0)); // non adjacent
    let a = arc(&mut p, s, pt(0.0, 100.0), 10.0, 0.0, 90.0);
    let sp = spline(&mut p, s);
    let c = circle(&mut p, s, pt(500.0, 0.0), 5.0);
    let merge = |first, second| Command::MergePaths {
        sheet: s,
        first,
        second,
    };
    let m = refused(&mut p, merge(e1, e2), "non colinéaires");
    assert!(m.contains("colinéaires"), "{m}");
    let m = refused(&mut p, merge(e1, back), "repli");
    assert!(m.contains("replient"), "{m}");
    let m = refused(&mut p, merge(e1, lone), "non adjacents");
    assert!(m.contains("adjacents"), "{m}");
    let m = refused(&mut p, merge(e1, a), "types incompatibles");
    assert!(m.contains("types incompatibles"), "{m}");
    refused(&mut p, merge(sp, e1), "types incompatibles");
    refused(&mut p, merge(c, c), "même tracé");
    refused(&mut p, merge(e1, e1), "même tracé");
    refused(&mut p, merge(e1, EntityId(9_999)), "tracé absent");
    // Bézier non issues d'une même courbe
    let c1 = bezier(&mut p, s);
    let end = p.sheet(s).unwrap().entity_points(c1).unwrap().1;
    let c2 = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::Existing(end),
            c1: pt(100.0, 90.0),
            c2: pt(150.0, 20.0),
            end: PointRef::New(pt(200.0, 10.0)),
        })
        .unwrap()
        .entities[0];
    let m = refused(&mut p, merge(c1, c2), "Bézier non prolongées");
    assert!(m.contains("Bézier"), "{m}");
    // rôles et calques différents, groupes différents
    let (mut p, s) = setup();
    let f = seg(&mut p, s, pt(0.0, 0.0), pt(50.0, 0.0));
    let g = seg(&mut p, s, pt(50.0, 0.0), pt(100.0, 0.0));
    p.apply(Command::SetPathRole {
        sheet: s,
        entity: g,
        role: PathRole::CuttingContour,
    })
    .unwrap();
    let m = refused(
        &mut p,
        Command::MergePaths {
            sheet: s,
            first: f,
            second: g,
        },
        "rôles",
    );
    assert!(m.contains("rôles"), "{m}");
    p.apply(Command::SetPathRole {
        sheet: s,
        entity: g,
        role: PathRole::Construction,
    })
    .unwrap();
    let lay = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Autre".into(),
        })
        .unwrap()
        .layers[0];
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![],
        entities: vec![g],
        layer: Some(lay),
    })
    .unwrap();
    let m = refused(
        &mut p,
        Command::MergePaths {
            sheet: s,
            first: f,
            second: g,
        },
        "calques",
    );
    assert!(m.contains("calques"), "{m}");
}

#[test]
fn fusionner_groupes_noms_et_pieces() {
    let (mut p, s) = setup();
    let e = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let other = seg(&mut p, s, pt(0.0, 50.0), pt(10.0, 50.0));
    p.apply(Command::SetEntityName {
        sheet: s,
        entity: e,
        name: Some("Bord".into()),
    })
    .unwrap();
    p.apply(Command::CreateGroup {
        sheet: s,
        entities: vec![e, other],
    })
    .unwrap();
    let piece = p
        .apply(Command::AddPiece {
            sheet: s,
            name: "P".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    p.apply(Command::SetPieceOutline {
        piece,
        outline: vec![PieceEdge {
            entity: e,
            reversed: false,
        }],
        grainline: None,
    })
    .unwrap();
    let new = p
        .apply(Command::SplitPath {
            sheet: s,
            entity: e,
            at: pt(40.0, 0.0),
        })
        .unwrap()
        .entities[0];
    assert_eq!(p.piece(piece).unwrap().outline.len(), 2);
    let out = p
        .apply(Command::MergePaths {
            sheet: s,
            first: e,
            second: new,
        })
        .unwrap();
    let sh = p.sheet(s).unwrap();
    // le second tracé disparaît partout, sans référence morte
    assert!(sh.entity(new).is_err());
    assert_eq!(p.piece(piece).unwrap().outline.len(), 1);
    assert_eq!(p.piece(piece).unwrap().outline[0].entity, e);
    let g = sh.group_of(e).unwrap();
    assert!(!g.members.contains(&new));
    assert_eq!(sh.entity(e).unwrap().name.as_deref(), Some("Bord"));
    // le nom « Bord (2) » du second tronçon disparaît : c'est dit
    assert!(
        out.notes.iter().any(|n| n.contains("Bord (2)")),
        "{:?}",
        out.notes
    );
    p.validate().unwrap();
}

// ------------------------------------------------------------------ inverser

#[test]
fn inverser_chaque_type_de_trace_garde_la_forme_et_les_crans() {
    type Make = fn(&mut Project, SheetId) -> EntityId;
    let makes: [(&str, Make); 5] = [
        ("segment", |p, s| seg(p, s, pt(0.0, 0.0), pt(100.0, 40.0))),
        ("arc", |p, s| arc(p, s, pt(5.0, 5.0), 40.0, 10.0, 200.0)),
        ("bézier", bezier),
        ("courbe lisse", spline),
        ("cercle", |p, s| circle(p, s, pt(5.0, 5.0), 40.0)),
    ];
    for (label, make) in makes {
        let (mut p, s) = setup();
        let e = make(&mut p, s);
        let l0 = len(&p, s, e);
        notch(&mut p, s, e, l0 * 0.2, Side::Left);
        notch(&mut p, s, e, l0 * 0.7, Side::Right);
        let marks0 = marks(&p, s);
        let base = dense(&p, s, e, 60);
        let (a0, b0) = ends(&p, s, e);
        let mut doc = Document::new(p);
        doc.execute(Command::ReversePath {
            sheet: s,
            entity: e,
        })
        .unwrap();
        let p = doc.project().clone();
        // CA : crans à la même position géométrique, côté géré
        assert_same_marks(&marks0, &marks(&p, s), POS_TOL);
        assert!((len(&p, s, e) - l0).abs() < 1e-9, "{label}");
        let gap = max_gap(&p, s, e, &base);
        assert!(gap < shape_tol(label), "{label} : forme changée de {gap}");
        if label != "cercle" {
            // départ et arrivée échangés
            let (a1, b1) = ends(&p, s, e);
            assert!(
                distance_mm(a1, b0) < 1e-9 && distance_mm(b1, a0) < 1e-9,
                "{label}"
            );
            let tail = p.sheet(s).unwrap().path_point_at(e, 0.0).unwrap().0;
            assert!(distance_mm(tail, b0) < 1e-9, "{label}");
        }
        // une seule étape annulable
        assert!(doc.undo());
        assert!(!doc.undo());
        // inverser deux fois : même géométrie
        let mut p2 = p.clone();
        p2.apply(Command::ReversePath {
            sheet: s,
            entity: e,
        })
        .unwrap();
        assert_same_marks(&marks0, &marks(&p2, s), POS_TOL);
        assert!(max_gap(&p2, s, e, &base) < shape_tol(label));
        let (a2, b2) = ends(&p2, s, e);
        if label != "cercle" {
            assert!(distance_mm(a2, a0) < 1e-9 && distance_mm(b2, b0) < 1e-9);
        }
    }
}

#[test]
fn inverser_suit_les_points_construits_les_pieces_et_la_mesure_du_contour() {
    let (mut p, s) = setup();
    let rect = p
        .apply(Command::AddRectangle {
            sheet: s,
            corner: PointRef::New(pt(0.0, 0.0)),
            width_mm: 100.0,
            height_mm: 60.0,
        })
        .unwrap();
    let e = rect.entities[0];
    let on_start = p
        .apply(Command::AddPointOnSegment {
            sheet: s,
            segment: e,
            distance_mm: 30.0,
            origin: SegmentEnd::Start,
        })
        .unwrap()
        .points[0];
    let ratio = p
        .apply(Command::AddPointAt {
            sheet: s,
            point: PointRef::Derived(PointDefinition::OnSegmentRatio {
                segment: e,
                ratio: 0.25,
            }),
        })
        .unwrap()
        .points[0];
    let along = p
        .apply(Command::AddPointAt {
            sheet: s,
            point: PointRef::Derived(PointDefinition::AlongCurve {
                curve: e,
                distance_mm: 10.0,
                origin: SegmentEnd::End,
            }),
        })
        .unwrap()
        .points[0];
    let piece = p
        .apply(Command::AddPiece {
            sheet: s,
            name: "Rabat".into(),
            quantity: 1,
            material: None,
            mirrored_pair: false,
        })
        .unwrap()
        .pieces[0];
    p.apply(Command::SetPieceOutline {
        piece,
        outline: rect
            .entities
            .iter()
            .map(|&entity| PieceEdge {
                entity,
                reversed: false,
            })
            .collect(),
        grainline: None,
    })
    .unwrap();
    let before: Vec<PointMm> = [on_start, ratio, along]
        .iter()
        .map(|q| pos(&p, s, *q))
        .collect();
    let c0 = closed_contours(p.sheet(s).unwrap());
    let m0 = measure_contour(p.sheet(s).unwrap(), &c0[0]).unwrap();
    p.apply(Command::ReversePath {
        sheet: s,
        entity: e,
    })
    .unwrap();
    // points construits : même position, toujours maintenus (pas figés)
    for (q, b) in [on_start, ratio, along].iter().zip(&before) {
        assert!(distance_mm(pos(&p, s, *q), *b) < 1e-9);
        assert!(!p.sheet(s).unwrap().point(*q).unwrap().definition.is_free());
    }
    // contour de pièce : le tracé est parcouru dans l'autre sens, donc l'indicateur bascule
    assert!(p.piece(piece).unwrap().outline[0].reversed);
    // le contour fermé garde périmètre et aire
    let c1 = closed_contours(p.sheet(s).unwrap());
    assert_eq!(c1.len(), 1);
    let m1 = measure_contour(p.sheet(s).unwrap(), &c1[0]).unwrap();
    assert!((m1.perimeter_mm - m0.perimeter_mm).abs() < LEN_TOL);
    assert!((m1.area_mm2 - m0.area_mm2).abs() < 1e-7);
    // un point construit continue de suivre le tracé quand on le modifie ensuite
    p.apply(Command::SetPointDistance {
        sheet: s,
        point: on_start,
        distance_mm: 40.0,
    })
    .unwrap();
    p.validate().unwrap();
}

// ------------------------------------------------------------------ verrou, annulation, fichier

#[test]
fn calque_verrouille_refuse_chaque_operation_sans_trace() {
    let ops = |s: SheetId, e: EntityId, f: EntityId| {
        vec![
            Command::ExtendPath {
                sheet: s,
                entity: e,
                end: SegmentEnd::End,
                delta_mm: 5.0,
            },
            Command::TrimPathTo {
                sheet: s,
                entity: e,
                end: SegmentEnd::End,
                target: f,
                pick: pt(100.0, 0.0),
            },
            Command::SplitPath {
                sheet: s,
                entity: e,
                at: pt(30.0, 0.0),
            },
            Command::MergePaths {
                sheet: s,
                first: e,
                second: f,
            },
            Command::MergePaths {
                sheet: s,
                first: f,
                second: e,
            },
            Command::ReversePath {
                sheet: s,
                entity: e,
            },
        ]
    };
    for which in 0..6 {
        let (mut p, s) = setup();
        let e = seg(&mut p, s, pt(0.0, 0.0), pt(50.0, 0.0));
        let f = seg(&mut p, s, pt(50.0, 0.0), pt(80.0, 0.0));
        let _wall = seg(&mut p, s, pt(100.0, -10.0), pt(100.0, 10.0));
        // pour la fusion, f est dans le calque verrouillé
        lock_layer_of(&mut p, s, if which == 4 { f } else { e });
        let mut cmd = ops(s, e, f).remove(which);
        if which == 1 {
            if let Command::TrimPathTo { target, .. } = &mut cmd {
                *target = _wall;
            }
        }
        // la fusion de calques différents échoue aussi (calque), c'est un refus de plus
        let m = refused(
            &mut p,
            cmd,
            &format!("opération {which} sur calque verrouillé"),
        );
        assert!(!m.is_empty());
    }
}

#[test]
fn chaque_operation_est_une_seule_etape_annulable_et_le_fichier_relit() {
    let (mut p, s) = setup();
    let e = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let w = seg(&mut p, s, pt(150.0, -10.0), pt(150.0, 10.0));
    notch(&mut p, s, e, 70.0, Side::Left);
    let cmds = [
        Command::ExtendPath {
            sheet: s,
            entity: e,
            end: SegmentEnd::End,
            delta_mm: -20.0,
        },
        Command::TrimPathTo {
            sheet: s,
            entity: e,
            end: SegmentEnd::End,
            target: w,
            pick: pt(150.0, 0.0),
        },
        Command::SplitPath {
            sheet: s,
            entity: e,
            at: pt(40.0, 0.0),
        },
        Command::ReversePath {
            sheet: s,
            entity: e,
        },
    ];
    let mut doc = Document::new(p);
    for cmd in cmds {
        let before = snapshot(doc.project());
        doc.execute(cmd).unwrap();
        let after = snapshot(doc.project());
        assert_ne!(before, after);
        let text = persist::to_json(doc.project()).unwrap();
        let back = persist::from_json(&text).unwrap();
        assert_eq!(back.schema_version, 6);
        assert_eq!(persist::to_json(&back).unwrap(), text);
        assert!(doc.undo());
        assert_eq!(snapshot(doc.project()), before, "annuler = état d'avant");
        assert!(doc.redo());
        assert_eq!(snapshot(doc.project()), after);
    }
    // les fichiers qui n'utilisent pas ces opérations ne sont pas touchés
    let (p0, _) = setup();
    assert_eq!(p0.schema_version, 6);
}

#[test]
fn feuille_dupliquee_apres_coupe_garde_des_ids_coherents() {
    let (mut p, s) = setup();
    let (e, _) = dressed_segment(&mut p, s);
    p.apply(Command::SplitPath {
        sheet: s,
        entity: e,
        at: pt(50.0, 20.0),
    })
    .unwrap();
    p.apply(Command::DuplicateSheet { sheet: s }).unwrap();
    p.validate().unwrap();
    assert_eq!(p.sheets[0].entities.len(), p.sheets[1].entities.len());
    assert_eq!(p.sheets[0].notches.len(), p.sheets[1].notches.len());
}

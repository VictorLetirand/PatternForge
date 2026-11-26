//! US-02.3 — tangence et raccords : tangente à une courbe, droite tangente à un
//! cercle, congé, chanfrein. Un test par critère d'acceptation, plus les
//! combinaisons de tracés, les refus sans trace, les crans, groupes, calques,
//! pièces, la sérialisation et l'annulation.

use pf_core::contour::closed_contours;
use pf_core::*;
use pf_geometry::{distance_mm, PointMm, VectorMm};
use std::f64::consts::PI;

const ANGLE_TOL: f64 = 1e-9;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn setup() -> (Project, SheetId) {
    let p = Project::new("Tangence");
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

fn circle(p: &mut Project, s: SheetId, c: PointMm, r: f64) -> EntityId {
    p.apply(Command::AddCircle {
        sheet: s,
        center: PointRef::New(c),
        radius_mm: r,
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

fn fillet(
    first: EntityId,
    second: EntityId,
    s: SheetId,
    r: f64,
    pa: PointMm,
    pb: PointMm,
) -> Command {
    Command::FilletPaths {
        sheet: s,
        first,
        second,
        radius_mm: r,
        pick_first: pa,
        pick_second: pb,
    }
}

/// Tangente unitaire (non orientée) d'un tracé en un point de ce tracé.
fn tangent_at(sh: &Sheet, e: EntityId, at: PointMm) -> VectorMm {
    match sh.shape(e).unwrap() {
        Shape::Line(a, b) => (b - a).unit().unwrap(),
        Shape::Arc(a) => (at - a.center).unit().unwrap().rotated_left(),
        Shape::Circle(c) => (at - c.circle.center).unit().unwrap().rotated_left(),
        _ => panic!("tracé sans tangente simple"),
    }
}

/// Angle (rad) entre les tangentes de deux tracés au point `at`.
fn tangent_angle(sh: &Sheet, e1: EntityId, e2: EntityId, at: PointMm) -> f64 {
    let (u, v) = (tangent_at(sh, e1, at), tangent_at(sh, e2, at));
    u.cross(v).abs().atan2(u.dot(v).abs())
}

/// Vérifie le raccord : l'arc `joint` est tangent à `first` et `second` en ses
/// extrémités (< 1e-9 rad), ses extrémités sont sur les tracés, son rayon est `r`.
fn assert_tangent_joint(
    p: &Project,
    s: SheetId,
    first: EntityId,
    second: EntityId,
    joint: EntityId,
    r: f64,
) {
    let sh = p.sheet(s).unwrap();
    let EntityKind::Arc { start, end, .. } = sh.entity(joint).unwrap().kind else {
        panic!("le raccord n'est pas un arc");
    };
    let g = sh.arc_geometry(joint).unwrap();
    assert!((g.radius_mm - r).abs() < 1e-9, "rayon {}", g.radius_mm);
    for (e, pid) in [(first, start), (second, end)] {
        let t = sh.position(pid).unwrap();
        let on = sh.shape(e).unwrap().distance_to_point(t);
        assert!(on < 1e-9, "point de tangence à {on} mm du tracé");
        let a = tangent_angle(sh, e, joint, t);
        assert!(a < ANGLE_TOL, "angle entre tangentes {a} rad");
    }
}

fn joint_of(out: &Outcome) -> EntityId {
    *out.entities.last().expect("raccord créé")
}

fn length(p: &Project, s: SheetId, e: EntityId) -> f64 {
    p.sheet(s).unwrap().path_length_mm(e).unwrap()
}

// --- CA 1 : angle entre tangentes au raccord < 1e-9 rad, par combinaison -----

#[test]
fn ca1_conge_segment_segment_plusieurs_angles() {
    for deg in [20.0_f64, 45.0, 90.0, 120.0, 160.0] {
        let (mut p, s) = setup();
        let rad = deg.to_radians();
        let a = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
        let b = seg(
            &mut p,
            s,
            pt(0.0, 0.0),
            pt(100.0 * rad.cos(), 100.0 * rad.sin()),
        );
        let r = 6.0;
        let out = p
            .apply(fillet(
                a,
                b,
                s,
                r,
                pt(50.0, 0.0),
                pt(50.0 * rad.cos(), 50.0 * rad.sin()),
            ))
            .unwrap();
        let j = joint_of(&out);
        assert_tangent_joint(&p, s, a, b, j, r);
        // Distance coin → point de tangence : r / tan(θ/2) ; segments raccourcis d'autant.
        let d = r / (rad / 2.0).tan();
        assert!((length(&p, s, a) - (100.0 - d)).abs() < 1e-9, "{deg}°");
        assert!((length(&p, s, b) - (100.0 - d)).abs() < 1e-9, "{deg}°");
        // Arc de balayage π − θ.
        let g = p.sheet(s).unwrap().arc_geometry(j).unwrap();
        assert!((g.sweep_rad.abs() - (PI - rad)).abs() < 1e-9, "{deg}°");
    }
}

#[test]
fn ca1_conge_segment_arc_et_arc_segment() {
    for swapped in [false, true] {
        let (mut p, s) = setup();
        let a = arc(&mut p, s, pt(0.0, 0.0), 50.0, 0.0, 180.0);
        let l = seg(&mut p, s, pt(0.0, 20.0), pt(0.0, 100.0));
        let (first, second, pa, pb) = if swapped {
            (a, l, pt(30.0, 40.0), pt(0.0, 80.0))
        } else {
            (l, a, pt(0.0, 80.0), pt(30.0, 40.0))
        };
        let out = p.apply(fillet(first, second, s, 10.0, pa, pb)).unwrap();
        let j = joint_of(&out);
        assert_tangent_joint(&p, s, first, second, j, 10.0);
        // Le segment est raccourci au-dessus du coin (0, 50), l'arc coupé à droite.
        let sh = p.sheet(s).unwrap();
        let (sa, sb) = sh.segment_ends(l).unwrap();
        assert!(sa.y > 50.0 && (sb.y - 100.0).abs() < 1e-9, "{sa:?}");
        let g = sh.arc_geometry(a).unwrap();
        assert!(
            g.sweep_deg() < 90.0 && g.sweep_deg() > 0.0,
            "{}",
            g.sweep_deg()
        );
    }
}

#[test]
fn ca1_conge_arc_arc() {
    let (mut p, s) = setup();
    let a1 = arc(&mut p, s, pt(0.0, 0.0), 50.0, 0.0, 180.0);
    let a2 = arc(&mut p, s, pt(60.0, 0.0), 50.0, 0.0, 180.0);
    let out = p
        .apply(fillet(a1, a2, s, 8.0, pt(-8.7, 49.2), pt(68.7, 49.2)))
        .unwrap();
    assert_tangent_joint(&p, s, a1, a2, joint_of(&out), 8.0);
    // Les deux arcs ont été coupés (plus courts qu'un demi-cercle).
    assert!(length(&p, s, a1) < 50.0 * PI - 1.0);
    assert!(length(&p, s, a2) < 50.0 * PI - 1.0);
}

#[test]
fn ca1_conge_segment_cercle_le_cercle_n_est_pas_coupe() {
    let (mut p, s) = setup();
    let c = circle(&mut p, s, pt(0.0, 0.0), 30.0);
    let l = seg(&mut p, s, pt(-100.0, 10.0), pt(100.0, 10.0));
    let before_circle = p.sheet(s).unwrap().entity(c).unwrap().clone();
    let out = p
        .apply(fillet(l, c, s, 5.0, pt(60.0, 10.0), pt(25.0, 16.6)))
        .unwrap();
    assert_tangent_joint(&p, s, l, c, joint_of(&out), 5.0);
    assert_eq!(p.sheet(s).unwrap().entity(c).unwrap(), &before_circle);
    assert!((length(&p, s, l) - (100.0 - 31.622776601683793)).abs() < 1e-6);
}

#[test]
fn ca1_conge_cercle_cercle() {
    let (mut p, s) = setup();
    let c1 = circle(&mut p, s, pt(0.0, 0.0), 30.0);
    let c2 = circle(&mut p, s, pt(40.0, 0.0), 30.0);
    let out = p
        .apply(fillet(c1, c2, s, 5.0, pt(19.0, 24.0), pt(21.0, 24.0)))
        .unwrap();
    assert_tangent_joint(&p, s, c1, c2, joint_of(&out), 5.0);
}

#[test]
fn ca1_la_pointe_designe_le_coin_garde() {
    // Deux segments qui se croisent en X : les points visés choisissent le quadrant.
    for (pa, pb, cx, cy) in [
        (pt(30.0, 0.0), pt(0.0, 30.0), 1.0, 1.0),
        (pt(-30.0, 0.0), pt(0.0, 30.0), -1.0, 1.0),
        (pt(-30.0, 0.0), pt(0.0, -30.0), -1.0, -1.0),
        (pt(30.0, 0.0), pt(0.0, -30.0), 1.0, -1.0),
    ] {
        let (mut p, s) = setup();
        let a = seg(&mut p, s, pt(-50.0, 0.0), pt(50.0, 0.0));
        let b = seg(&mut p, s, pt(0.0, -50.0), pt(0.0, 50.0));
        let out = p.apply(fillet(a, b, s, 5.0, pa, pb)).unwrap();
        let g = p.sheet(s).unwrap().arc_geometry(joint_of(&out)).unwrap();
        assert!(
            distance_mm(g.center, pt(5.0 * cx, 5.0 * cy)) < 1e-9,
            "{:?}",
            g.center
        );
    }
}

// --- CA 2 : cas impossibles refusés, projet inchangé --------------------------

fn refus(p: &mut Project, cmd: Command, what: &str) -> String {
    let before = snapshot(p);
    let err = p.apply(cmd).expect_err(what).to_string();
    assert!(!err.trim().is_empty(), "{what} : message vide");
    assert_eq!(snapshot(p), before, "{what} : le projet a changé");
    p.validate().unwrap();
    err
}

#[test]
fn ca2_cas_impossibles_refuses_sans_trace() {
    let (mut p, s) = setup();
    let a = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let b = seg(&mut p, s, pt(0.0, 0.0), pt(0.0, 30.0));
    let par = seg(&mut p, s, pt(0.0, 10.0), pt(100.0, 10.0));
    let far = circle(&mut p, s, pt(0.0, 300.0), 5.0);
    let curve = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(pt(0.0, 50.0)),
            c1: pt(10.0, 60.0),
            c2: pt(20.0, 60.0),
            end: PointRef::New(pt(30.0, 50.0)),
        })
        .unwrap()
        .entities[0];
    let spline = p
        .apply(Command::AddSpline {
            sheet: s,
            points: vec![PointRef::New(pt(0.0, 70.0)), PointRef::New(pt(40.0, 80.0))],
        })
        .unwrap()
        .entities[0];
    let (pa, pb) = (pt(50.0, 0.0), pt(0.0, 20.0));

    let too_big = refus(&mut p, fillet(a, b, s, 40.0, pa, pb), "rayon trop grand");
    assert!(too_big.contains("trop grand"), "{too_big}");
    for (r, what) in [
        (0.0, "rayon nul"),
        (-3.0, "rayon négatif"),
        (f64::NAN, "rayon NaN"),
        (f64::INFINITY, "rayon infini"),
        (1e9, "rayon hors espace"),
    ] {
        refus(&mut p, fillet(a, b, s, r, pa, pb), what);
    }
    let par_err = refus(
        &mut p,
        fillet(a, par, s, 2.0, pa, pt(50.0, 10.0)),
        "parallèles",
    );
    assert!(par_err.contains("parallèles"), "{par_err}");
    let no_corner = refus(
        &mut p,
        fillet(a, far, s, 2.0, pa, pt(0.0, 305.0)),
        "sans sécante",
    );
    assert!(no_corner.contains("ne se coupent pas"), "{no_corner}");
    refus(&mut p, fillet(a, a, s, 2.0, pa, pa), "même tracé");
    refus(
        &mut p,
        fillet(a, curve, s, 2.0, pa, pt(10.0, 55.0)),
        "Bézier",
    );
    refus(
        &mut p,
        fillet(spline, b, s, 2.0, pt(10.0, 72.0), pb),
        "courbe lisse",
    );
    refus(
        &mut p,
        fillet(a, EntityId(99_999), s, 2.0, pa, pb),
        "tracé inconnu",
    );
    refus(
        &mut p,
        fillet(a, b, SheetId(99_999), 2.0, pa, pb),
        "feuille inconnue",
    );
    refus(
        &mut p,
        fillet(a, b, s, 2.0, pt(f64::NAN, 0.0), pb),
        "point visé NaN",
    );
    // Points visés confondus avec le coin : le côté gardé est indéterminé.
    refus(
        &mut p,
        fillet(a, b, s, 2.0, pt(0.0, 0.0), pb),
        "pointe sur le coin",
    );

    // Chanfrein.
    refus(
        &mut p,
        Command::ChamferPaths {
            sheet: s,
            first: a,
            second: b,
            distance_first_mm: 10.0,
            distance_second_mm: 40.0,
            pick_first: pa,
            pick_second: pb,
        },
        "chanfrein trop long",
    );
    for (d1, d2, what) in [
        (0.0, 5.0, "distance nulle"),
        (-1.0, 5.0, "distance négative"),
        (5.0, f64::NAN, "distance NaN"),
    ] {
        refus(
            &mut p,
            Command::ChamferPaths {
                sheet: s,
                first: a,
                second: b,
                distance_first_mm: d1,
                distance_second_mm: d2,
                pick_first: pa,
                pick_second: pb,
            },
            what,
        );
    }
    refus(
        &mut p,
        Command::ChamferPaths {
            sheet: s,
            first: a,
            second: par,
            distance_first_mm: 3.0,
            distance_second_mm: 3.0,
            pick_first: pa,
            pick_second: pt(50.0, 10.0),
        },
        "chanfrein parallèles",
    );
    refus(
        &mut p,
        Command::ChamferPaths {
            sheet: s,
            first: a,
            second: far,
            distance_first_mm: 3.0,
            distance_second_mm: 3.0,
            pick_first: pa,
            pick_second: pt(0.0, 305.0),
        },
        "chanfrein avec un cercle",
    );
}

#[test]
fn ca2_calque_verrouille_respecte() {
    let (mut p, s) = setup();
    let a = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let b = seg(&mut p, s, pt(0.0, 0.0), pt(0.0, 100.0));
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Fixe".into(),
        })
        .unwrap()
        .layers[0];
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![],
        entities: vec![b],
        layer: Some(layer),
    })
    .unwrap();
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: true,
    })
    .unwrap();
    let err = refus(
        &mut p,
        fillet(a, b, s, 5.0, pt(50.0, 0.0), pt(0.0, 50.0)),
        "congé verrouillé",
    );
    assert!(err.contains("verrouillé"), "{err}");
    refus(
        &mut p,
        Command::ChamferPaths {
            sheet: s,
            first: a,
            second: b,
            distance_first_mm: 5.0,
            distance_second_mm: 5.0,
            pick_first: pt(50.0, 0.0),
            pick_second: pt(0.0, 50.0),
        },
        "chanfrein verrouillé",
    );
}

#[test]
fn cercle_verrouille_non_modifie_par_un_conge_permis() {
    let (mut p, s) = setup();
    let c = circle(&mut p, s, pt(0.0, 0.0), 30.0);
    let l = seg(&mut p, s, pt(-100.0, 10.0), pt(100.0, 10.0));
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Fixe".into(),
        })
        .unwrap()
        .layers[0];
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![],
        entities: vec![c],
        layer: Some(layer),
    })
    .unwrap();
    p.apply(Command::SetLayerLocked {
        sheet: s,
        layer,
        locked: true,
    })
    .unwrap();
    // Le segment (déverrouillé) est coupé ; le cercle verrouillé reste intact.
    p.apply(fillet(l, c, s, 5.0, pt(60.0, 10.0), pt(25.0, 16.6)))
        .unwrap();
    // Dans l'autre ordre, l'arc irait dans le calque verrouillé : refusé.
    let (mut q, t) = setup();
    let c2 = circle(&mut q, t, pt(0.0, 0.0), 30.0);
    let l2 = seg(&mut q, t, pt(-100.0, 10.0), pt(100.0, 10.0));
    let lay = q
        .apply(Command::AddLayer {
            sheet: t,
            name: "Fixe".into(),
        })
        .unwrap()
        .layers[0];
    q.apply(Command::SetObjectsLayer {
        sheet: t,
        points: vec![],
        entities: vec![c2],
        layer: Some(lay),
    })
    .unwrap();
    q.apply(Command::SetLayerLocked {
        sheet: t,
        layer: lay,
        locked: true,
    })
    .unwrap();
    let err = refus(
        &mut q,
        fillet(c2, l2, t, 5.0, pt(25.0, 16.6), pt(60.0, 10.0)),
        "arc en calque verrouillé",
    );
    assert!(err.contains("verrouillé"), "{err}");
}

// --- Annulation, une seule étape ---------------------------------------------

#[test]
fn conge_et_chanfrein_sont_une_seule_etape_annulable() {
    for chamfer in [false, true] {
        let mut p = Project::new("U");
        let s = p.sheets[0].id;
        let a = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
        let b = seg(&mut p, s, pt(0.0, 0.0), pt(0.0, 100.0));
        let mut doc = Document::new(p);
        let before = snapshot(doc.project());
        let cmd = if chamfer {
            Command::ChamferPaths {
                sheet: s,
                first: a,
                second: b,
                distance_first_mm: 10.0,
                distance_second_mm: 10.0,
                pick_first: pt(50.0, 0.0),
                pick_second: pt(0.0, 50.0),
            }
        } else {
            fillet(a, b, s, 10.0, pt(50.0, 0.0), pt(0.0, 50.0))
        };
        doc.execute(cmd).unwrap();
        let after = snapshot(doc.project());
        assert_ne!(after, before);
        assert!(doc.undo());
        assert_eq!(snapshot(doc.project()), before);
        assert!(!doc.undo(), "une seule étape d'historique");
        assert!(doc.redo());
        assert_eq!(snapshot(doc.project()), after);
    }
}

// --- Crans, cotations, groupes, calques, rôles, noms ---------------------------

#[test]
fn crans_recales_ou_supprimes_avec_message() {
    let (mut p, s) = setup();
    let a = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let b = seg(&mut p, s, pt(0.0, 0.0), pt(0.0, 100.0));
    for d in [10.0, 60.0] {
        p.apply(Command::AddNotch {
            sheet: s,
            entity: a,
            distance_mm: d,
            side: pf_geometry::Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap();
    }
    let kept_pos = p.sheet(s).unwrap().path_point_at(a, 60.0).unwrap().0;
    let out = p
        .apply(fillet(a, b, s, 20.0, pt(50.0, 0.0), pt(0.0, 50.0)))
        .unwrap();
    let sh = p.sheet(s).unwrap();
    // Le cran à 10 mm tombait dans les 20 mm retirés : supprimé, avec un message.
    assert_eq!(sh.notches.len(), 1);
    assert!(
        out.notes
            .iter()
            .any(|n| n.contains("cran") && n.contains("supprimé")),
        "{:?}",
        out.notes
    );
    // Le cran à 60 mm reste au même endroit, recalé à 40 mm du nouveau départ.
    let n = &sh.notches[0];
    assert!((n.distance_mm - 40.0).abs() < 1e-9);
    assert!(distance_mm(sh.path_point_at(a, n.distance_mm).unwrap().0, kept_pos) < 1e-9);
}

#[test]
fn crans_extremite_arrivee_coupee() {
    let (mut p, s) = setup();
    // Segment orienté vers le coin : c'est l'arrivée qui est coupée.
    let a = seg(&mut p, s, pt(100.0, 0.0), pt(0.0, 0.0));
    let b = seg(&mut p, s, pt(0.0, 0.0), pt(0.0, 100.0));
    for d in [40.0, 90.0] {
        p.apply(Command::AddNotch {
            sheet: s,
            entity: a,
            distance_mm: d,
            side: pf_geometry::Side::Left,
            length_mm: 5.0,
            style: NotchStyle::Single,
        })
        .unwrap();
    }
    let out = p
        .apply(fillet(a, b, s, 20.0, pt(50.0, 0.0), pt(0.0, 50.0)))
        .unwrap();
    let sh = p.sheet(s).unwrap();
    assert_eq!(sh.notches.len(), 1);
    assert!((sh.notches[0].distance_mm - 40.0).abs() < 1e-9);
    assert!(out.notes.iter().any(|n| n.contains("cran")));
}

#[test]
fn cotation_sur_le_coin_conservee_et_point_construit_fige() {
    let (mut p, s) = setup();
    let a = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let b = seg(&mut p, s, pt(0.0, 0.0), pt(0.0, 100.0));
    let sh = p.sheet(s).unwrap();
    let (corner, far) = sh.segment_point_ids(a).unwrap();
    let derived = p
        .apply(Command::AddPointOnSegment {
            sheet: s,
            segment: a,
            distance_mm: 80.0,
            origin: SegmentEnd::Start,
        })
        .unwrap()
        .points[0];
    p.apply(Command::AddDimension {
        sheet: s,
        from: corner,
        to: far,
        offset_mm: 10.0,
    })
    .unwrap();
    let out = p
        .apply(fillet(a, b, s, 20.0, pt(50.0, 0.0), pt(0.0, 50.0)))
        .unwrap();
    let sh = p.sheet(s).unwrap();
    // Le coin, coté, est conservé.
    assert!(sh.point(corner).is_ok());
    assert_eq!(sh.dimensions.len(), 1);
    // Le point à 80 mm du départ est figé : même position, plus de dépendance.
    let q = sh.point(derived).unwrap();
    assert!(q.definition.is_free());
    assert!(distance_mm(sh.position(derived).unwrap(), pt(80.0, 0.0)) < 1e-9);
    assert!(
        out.notes.iter().any(|n| n.contains("libre")),
        "{:?}",
        out.notes
    );
}

#[test]
fn calque_groupe_nom_role_des_tracés_conserves() {
    let (mut p, s) = setup();
    let a = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let b = seg(&mut p, s, pt(0.0, 0.0), pt(0.0, 100.0));
    let layer = p
        .apply(Command::AddLayer {
            sheet: s,
            name: "Coupe".into(),
        })
        .unwrap()
        .layers[0];
    p.apply(Command::SetObjectsLayer {
        sheet: s,
        points: vec![],
        entities: vec![a, b],
        layer: Some(layer),
    })
    .unwrap();
    for e in [a, b] {
        p.apply(Command::SetPathRole {
            sheet: s,
            entity: e,
            role: PathRole::CuttingContour,
        })
        .unwrap();
    }
    p.apply(Command::SetEntityName {
        sheet: s,
        entity: a,
        name: Some("Devant".into()),
    })
    .unwrap();
    p.apply(Command::CreateGroup {
        sheet: s,
        entities: vec![a, b],
    })
    .unwrap();
    let out = p
        .apply(fillet(a, b, s, 10.0, pt(50.0, 0.0), pt(0.0, 50.0)))
        .unwrap();
    let j = joint_of(&out);
    let sh = p.sheet(s).unwrap();
    assert_eq!(sh.entity(a).unwrap().layer, Some(layer));
    assert_eq!(sh.entity(a).unwrap().name.as_deref(), Some("Devant"));
    assert_eq!(sh.entity(j).unwrap().layer, Some(layer));
    assert_eq!(sh.entity(j).unwrap().role, PathRole::CuttingContour);
    assert_eq!(sh.group_of(j).map(|g| g.id), sh.group_of(a).map(|g| g.id));
    assert!(sh.group_of(j).is_some());
    // Les nouveaux points sont dans le même calque.
    for id in &out.points {
        assert_eq!(sh.point(*id).unwrap().layer, Some(layer));
    }
}

#[test]
fn roles_differents_donnent_un_raccord_de_construction() {
    let (mut p, s) = setup();
    let a = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let b = seg(&mut p, s, pt(0.0, 0.0), pt(0.0, 100.0));
    p.apply(Command::SetPathRole {
        sheet: s,
        entity: a,
        role: PathRole::SewingContour,
    })
    .unwrap();
    let out = p
        .apply(fillet(a, b, s, 10.0, pt(50.0, 0.0), pt(0.0, 50.0)))
        .unwrap();
    let sh = p.sheet(s).unwrap();
    assert_eq!(
        sh.entity(joint_of(&out)).unwrap().role,
        PathRole::Construction
    );
    assert_eq!(sh.entity(a).unwrap().role, PathRole::SewingContour);
}

// --- Contours fermés et pièces -------------------------------------------------

fn rectangle_with_piece(p: &mut Project, s: SheetId) -> (Vec<EntityId>, PieceId) {
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
    (rect.entities, piece)
}

#[test]
fn contour_ferme_reste_ferme_perimetre_exact_et_piece_mise_a_jour() {
    for swapped in [false, true] {
        let (mut p, s) = setup();
        let (e, piece) = rectangle_with_piece(&mut p, s);
        let before = closed_contours(p.sheet(s).unwrap());
        assert_eq!(before.len(), 1);
        let (first, second, pa, pb) = if swapped {
            (e[1], e[0], pt(100.0, 30.0), pt(50.0, 0.0))
        } else {
            (e[0], e[1], pt(50.0, 0.0), pt(100.0, 30.0))
        };
        let r = 12.0;
        let out = p.apply(fillet(first, second, s, r, pa, pb)).unwrap();
        let j = joint_of(&out);
        let contours = closed_contours(p.sheet(s).unwrap());
        assert_eq!(contours.len(), 1, "le contour reste fermé");
        assert_eq!(contours[0].edges.len(), 5);
        let perimeter = 2.0 * (100.0 + 60.0) - 2.0 * r + PI * r / 2.0;
        let m = pf_core::contour::measure_contour(p.sheet(s).unwrap(), &contours[0]).unwrap();
        assert!(
            (m.perimeter_mm - perimeter).abs() < 1e-6,
            "{}",
            m.perimeter_mm
        );
        // La pièce reçoit l'arc entre les deux tracés, dans le bon sens.
        let outline = &p.piece(piece).unwrap().outline;
        assert_eq!(outline.len(), 5);
        let i = outline.iter().position(|x| x.entity == j).unwrap();
        assert_eq!(outline[i].reversed, swapped);
        assert!(out.notes.is_empty(), "{:?}", out.notes);
    }
}

#[test]
fn piece_dont_le_contour_ne_chaine_pas_les_tracés_est_videe_avec_remarque() {
    let (mut p, s) = setup();
    let (e, piece) = rectangle_with_piece(&mut p, s);
    // e[0] et e[2] sont opposés : on les raccorde ailleurs pour forcer le cas ;
    // ici : contour réordonné [e0, e2, e1, e3] puis congé entre e0 et e1 (non voisins).
    p.apply(Command::SetPieceOutline {
        piece,
        outline: [e[0], e[2], e[1], e[3]]
            .iter()
            .map(|&entity| PieceEdge {
                entity,
                reversed: false,
            })
            .collect(),
        grainline: None,
    })
    .unwrap();
    let out = p
        .apply(fillet(e[0], e[1], s, 10.0, pt(50.0, 0.0), pt(100.0, 30.0)))
        .unwrap();
    assert!(p.piece(piece).unwrap().outline.is_empty());
    assert!(
        out.notes.iter().any(|n| n.contains("Rabat")),
        "{:?}",
        out.notes
    );
}

// --- Chanfrein -------------------------------------------------------------------

#[test]
fn chanfrein_distances_donnees_contour_et_longueurs() {
    let (mut p, s) = setup();
    let (e, piece) = rectangle_with_piece(&mut p, s);
    let out = p
        .apply(Command::ChamferPaths {
            sheet: s,
            first: e[0],
            second: e[1],
            distance_first_mm: 10.0,
            distance_second_mm: 20.0,
            pick_first: pt(50.0, 0.0),
            pick_second: pt(100.0, 30.0),
        })
        .unwrap();
    let j = joint_of(&out);
    let sh = p.sheet(s).unwrap();
    assert!((length(&p, s, e[0]) - 90.0).abs() < 1e-9);
    assert!((length(&p, s, e[1]) - 40.0).abs() < 1e-9);
    assert!((length(&p, s, j) - (10.0_f64.hypot(20.0))).abs() < 1e-9);
    let (a, b) = sh.segment_ends(j).unwrap();
    assert!(distance_mm(a, pt(90.0, 0.0)) < 1e-9 && distance_mm(b, pt(100.0, 20.0)) < 1e-9);
    let contours = closed_contours(sh);
    assert_eq!(contours.len(), 1);
    assert_eq!(contours[0].edges.len(), 5);
    assert_eq!(p.piece(piece).unwrap().outline.len(), 5);
}

// --- Droite tangente à un cercle ------------------------------------------------

#[test]
fn droite_tangente_depuis_un_point_exterieur_choix_par_proximite() {
    let (mut p, s) = setup();
    let c = circle(&mut p, s, pt(0.0, 0.0), 20.0);
    let from = pt(50.0, 0.0);
    let mut seen = Vec::new();
    for pick in [pt(30.0, 25.0), pt(30.0, -25.0)] {
        let out = p
            .apply(Command::AddTangentLine {
                sheet: s,
                from: PointRef::New(from),
                target: c,
                pick,
                length_mm: None,
            })
            .unwrap();
        let sh = p.sheet(s).unwrap();
        let (a, t) = sh.segment_ends(out.entities[0]).unwrap();
        assert!(distance_mm(a, from) < 1e-12);
        // Tangente : le rayon au point de contact est perpendiculaire à la droite.
        assert!((distance_mm(t, pt(0.0, 0.0)) - 20.0).abs() < 1e-9);
        assert!(
            (t - pt(0.0, 0.0))
                .unit()
                .unwrap()
                .dot((t - a).unit().unwrap())
                .abs()
                < ANGLE_TOL
        );
        // Longueur de la tangente : √(50² − 20²).
        assert!((distance_mm(a, t) - (2500.0_f64 - 400.0).sqrt()).abs() < 1e-9);
        // Le point visé est du même côté que la solution choisie.
        assert!(t.y * pick.y > 0.0);
        seen.push(t);
    }
    assert!(
        distance_mm(seen[0], seen[1]) > 1.0,
        "deux solutions distinctes"
    );
}

#[test]
fn droite_tangente_point_sur_le_cercle_et_refus_interieur() {
    let (mut p, s) = setup();
    let c = circle(&mut p, s, pt(0.0, 0.0), 20.0);
    let cmd = |from: PointMm, pick: PointMm, length_mm: Option<f64>| Command::AddTangentLine {
        sheet: s,
        from: PointRef::New(from),
        target: c,
        pick,
        length_mm,
    };
    // Point sur le cercle : une seule tangente, de la longueur demandée, du côté visé.
    let out = p
        .apply(cmd(pt(20.0, 0.0), pt(20.0, 10.0), Some(30.0)))
        .unwrap();
    let (a, b) = p.sheet(s).unwrap().segment_ends(out.entities[0]).unwrap();
    assert!(distance_mm(a, pt(20.0, 0.0)) < 1e-9 && distance_mm(b, pt(20.0, 30.0)) < 1e-9);
    let err = refus(
        &mut p,
        cmd(pt(0.0, 20.0), pt(5.0, 20.0), None),
        "longueur manquante",
    );
    assert!(err.contains("longueur"), "{err}");
    let err = refus(
        &mut p,
        cmd(pt(3.0, 4.0), pt(5.0, 20.0), Some(10.0)),
        "point intérieur",
    );
    assert!(err.contains("intérieur"), "{err}");
    refus(
        &mut p,
        cmd(pt(f64::NAN, 4.0), pt(5.0, 20.0), Some(10.0)),
        "point NaN",
    );
    refus(
        &mut p,
        cmd(pt(50.0, 0.0), pt(f64::NAN, 0.0), None),
        "pointe NaN",
    );
    refus(
        &mut p,
        cmd(pt(20.0, 0.0), pt(20.0, 5.0), Some(-3.0)),
        "longueur négative",
    );
    // Cible qui n'est pas un cercle.
    let l = seg(&mut p, s, pt(100.0, 0.0), pt(150.0, 0.0));
    refus(
        &mut p,
        Command::AddTangentLine {
            sheet: s,
            from: PointRef::New(pt(0.0, 90.0)),
            target: l,
            pick: pt(0.0, 0.0),
            length_mm: None,
        },
        "cible segment",
    );
}

#[test]
fn droite_tangente_a_un_arc_seules_les_solutions_sur_l_arc() {
    let (mut p, s) = setup();
    // Quart de cercle haut-droit (0° à 90°).
    let a = arc(&mut p, s, pt(0.0, 0.0), 20.0, 0.0, 90.0);
    let from = pt(50.0, 0.0);
    // Les deux tangentes touchent le cercle en y > 0 et y < 0 ; seule la première est sur l'arc.
    let out = p
        .apply(Command::AddTangentLine {
            sheet: s,
            from: PointRef::New(from),
            target: a,
            pick: pt(30.0, -25.0), // visé du mauvais côté : la solution sur l'arc est choisie quand même
            length_mm: None,
        })
        .unwrap();
    let (_, t) = p.sheet(s).unwrap().segment_ends(out.entities[0]).unwrap();
    assert!(
        t.y > 0.0
            && p.sheet(s)
                .unwrap()
                .arc_geometry(a)
                .unwrap()
                .contains_angle_of(t)
    );
    // Point d'où aucune tangente ne touche l'arc.
    refus(
        &mut p,
        Command::AddTangentLine {
            sheet: s,
            from: PointRef::New(pt(-50.0, 0.0)),
            target: a,
            pick: pt(0.0, 0.0),
            length_mm: None,
        },
        "aucune solution sur l'arc",
    );
}

// --- Tangente à une courbe ---------------------------------------------------------

/// Direction de la courbe en distance `d` par différence finie centrée.
fn finite_difference(sh: &Sheet, e: EntityId, d: f64, h: f64) -> VectorMm {
    let a = sh.path_point_at(e, d - h).unwrap().0;
    let b = sh.path_point_at(e, d + h).unwrap().0;
    (b - a).unit().unwrap()
}

fn tangent_segment(
    p: &mut Project,
    s: SheetId,
    e: EntityId,
    at: TangentAt,
    backwards: bool,
) -> (PointMm, PointMm) {
    let out = p
        .apply(Command::AddTangentAt {
            sheet: s,
            entity: e,
            at,
            length_mm: 25.0,
            backwards,
        })
        .unwrap();
    p.sheet(s).unwrap().segment_ends(out.entities[0]).unwrap()
}

#[test]
fn tangente_a_une_courbe_de_bezier_et_a_une_courbe_lisse() {
    let (mut p, s) = setup();
    let bez = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(pt(0.0, 0.0)),
            c1: pt(30.0, 60.0),
            c2: pt(70.0, 60.0),
            end: PointRef::New(pt(100.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let spl = p
        .apply(Command::AddSpline {
            sheet: s,
            points: vec![
                PointRef::New(pt(0.0, 100.0)),
                PointRef::New(pt(40.0, 140.0)),
                PointRef::New(pt(90.0, 110.0)),
                PointRef::New(pt(130.0, 150.0)),
            ],
        })
        .unwrap()
        .entities[0];
    for e in [bez, spl] {
        let len = length(&p, s, e);
        for f in [0.0, 0.2, 0.5, 0.83, 1.0] {
            let (a, b) = tangent_segment(&mut p, s, e, TangentAt::Fraction(f), false);
            let sh = p.sheet(s).unwrap();
            let d = f * len;
            let contact = sh.path_point_at(e, d).unwrap().0;
            assert!(distance_mm(a, contact) < 1e-9, "contact");
            assert!((distance_mm(a, b) - 25.0).abs() < 1e-9);
            // Direction de la courbe, par différence finie (indépendante du calcul de la tangente).
            let h = 1e-3;
            let fd = finite_difference(sh, e, d.clamp(h, len - h), h);
            let u = (b - a).unit().unwrap();
            // Différence finie sur ±1e-3 mm : tolérance 3e-3 (courbure aux extrémités).
            assert!(
                u.cross(fd).abs() < 3e-3 && u.dot(fd) > 0.0,
                "f={f} u={u:?} fd={fd:?}"
            );
        }
        // À l'envers : sens opposé.
        let (a, b) = tangent_segment(&mut p, s, e, TangentAt::Fraction(0.5), true);
        let fd = finite_difference(p.sheet(s).unwrap(), e, 0.5 * len, 1e-3);
        assert!((b - a).unit().unwrap().dot(fd) < 0.0);
        // Par un point proche de la courbe.
        let near = p.sheet(s).unwrap().path_point_at(e, 0.3 * len).unwrap().0;
        let (a, _) = tangent_segment(
            &mut p,
            s,
            e,
            TangentAt::Near(pt(near.x + 0.01, near.y + 0.01)),
            false,
        );
        assert!(distance_mm(a, near) < 0.05);
    }
}

#[test]
fn tangente_a_un_arc_et_a_un_cercle_exacte() {
    let (mut p, s) = setup();
    let c = circle(&mut p, s, pt(10.0, 10.0), 30.0);
    let a = arc(&mut p, s, pt(-80.0, 0.0), 40.0, 30.0, 200.0);
    for e in [c, a] {
        for f in [0.1, 0.5, 0.9] {
            let (pa, pb) = tangent_segment(&mut p, s, e, TangentAt::Fraction(f), false);
            let sh = p.sheet(s).unwrap();
            let center = match sh.shape(e).unwrap() {
                Shape::Circle(c) => c.circle.center,
                Shape::Arc(a) => a.center,
                _ => unreachable!(),
            };
            // Le contact est sur le tracé, et le rayon y est perpendiculaire à la tangente.
            assert!(sh.shape(e).unwrap().distance_to_point(pa) < 1e-9);
            let radial = (pa - center).unit().unwrap();
            assert!(
                radial.dot((pb - pa).unit().unwrap()).abs() < ANGLE_TOL,
                "f={f}"
            );
        }
    }
    // Sens du tracé : cercle anti-horaire, départ à 0° → tangente vers +y au départ.
    let (pa, pb) = tangent_segment(&mut p, s, c, TangentAt::Fraction(0.0), false);
    assert!(pb.y > pa.y && (pb.x - pa.x).abs() < 1e-9);
}

#[test]
fn tangente_refus_sans_trace() {
    let (mut p, s) = setup();
    let l = seg(&mut p, s, pt(0.0, 0.0), pt(10.0, 0.0));
    let c = circle(&mut p, s, pt(50.0, 50.0), 10.0);
    let cmd = |entity: EntityId, at: TangentAt, length_mm: f64| Command::AddTangentAt {
        sheet: s,
        entity,
        at,
        length_mm,
        backwards: false,
    };
    refus(
        &mut p,
        cmd(l, TangentAt::Fraction(0.5), 5.0),
        "un segment est sa tangente",
    );
    for f in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
        refus(
            &mut p,
            cmd(c, TangentAt::Fraction(f), 5.0),
            "fraction invalide",
        );
    }
    for len in [0.0, -2.0, f64::NAN, f64::INFINITY, 1e9] {
        refus(
            &mut p,
            cmd(c, TangentAt::Fraction(0.5), len),
            "longueur invalide",
        );
    }
    refus(
        &mut p,
        cmd(c, TangentAt::Near(pt(f64::NAN, 0.0)), 5.0),
        "point NaN",
    );
    refus(
        &mut p,
        cmd(EntityId(77_777), TangentAt::Fraction(0.5), 5.0),
        "tracé inconnu",
    );
}

// --- Fichiers : schéma 3 sans montée de version ------------------------------------

#[test]
fn fichier_sans_ces_objets_inchange_et_avec_relu_a_l_identique() {
    let (mut p, s) = setup();
    let a = seg(&mut p, s, pt(0.0, 0.0), pt(100.0, 0.0));
    let b = seg(&mut p, s, pt(0.0, 0.0), pt(0.0, 100.0));
    let before = persist::to_json(&p).unwrap();
    assert!(before.contains("\"schema_version\": 6"));
    // Aucun champ nouveau n'apparaît tant qu'on ne crée pas de raccord.
    for key in ["tangent", "fillet", "chamfer", "joint"] {
        assert!(!before.contains(key), "{key}");
    }
    p.apply(fillet(a, b, s, 10.0, pt(50.0, 0.0), pt(0.0, 50.0)))
        .unwrap();
    let json = persist::to_json(&p).unwrap();
    assert!(json.contains("\"schema_version\": 6"));
    let back = persist::from_json(&json).unwrap();
    assert_eq!(back, p);
    assert_eq!(persist::to_json(&back).unwrap(), json);
}

#[test]
fn fichiers_d_exemple_relus_a_l_octet() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    let mut n = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if !(path.to_string_lossy().ends_with(".pforge")
            || path.to_string_lossy().ends_with(".pforge.json"))
        {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let p = persist::from_json(&text).unwrap();
        assert_eq!(p.schema_version, 6, "{path:?}");
        assert_eq!(persist::to_json(&p).unwrap(), text, "{path:?}");
        n += 1;
    }
    assert!(n > 0);
}

#[test]
fn commandes_serialisables() {
    let cmds = vec![
        fillet(
            EntityId(1),
            EntityId(2),
            SheetId(3),
            5.0,
            pt(1.0, 2.0),
            pt(3.0, 4.0),
        ),
        Command::ChamferPaths {
            sheet: SheetId(3),
            first: EntityId(1),
            second: EntityId(2),
            distance_first_mm: 4.0,
            distance_second_mm: 5.0,
            pick_first: pt(1.0, 2.0),
            pick_second: pt(3.0, 4.0),
        },
        Command::AddTangentLine {
            sheet: SheetId(3),
            from: PointRef::New(pt(0.0, 0.0)),
            target: EntityId(1),
            pick: pt(1.0, 1.0),
            length_mm: Some(3.0),
        },
        Command::AddTangentAt {
            sheet: SheetId(3),
            entity: EntityId(1),
            at: TangentAt::Near(pt(1.0, 1.0)),
            length_mm: 3.0,
            backwards: true,
        },
    ];
    for c in cmds {
        let json = serde_json::to_string(&c).unwrap();
        let back: Command = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c, "{json}");
    }
}

// --- Campagne aléatoire : tangence exacte ou refus propre ------------------------------

struct Lcg(u64);
impl Lcg {
    fn unit(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[test]
fn campagne_conges_segment_segment_tangence_ou_refus() {
    let mut r = Lcg(42);
    let (mut ok, mut refused) = (0, 0);
    for _ in 0..400 {
        let (mut p, s) = setup();
        let ang = (3.0 + r.unit() * 174.0).to_radians();
        let (la, lb) = (5.0 + r.unit() * 200.0, 5.0 + r.unit() * 200.0);
        let a = seg(&mut p, s, pt(0.0, 0.0), pt(la, 0.0));
        let b = seg(&mut p, s, pt(0.0, 0.0), pt(lb * ang.cos(), lb * ang.sin()));
        let rad = 0.5 + r.unit() * 120.0;
        let before = snapshot(&p);
        match p.apply(fillet(
            a,
            b,
            s,
            rad,
            pt(la / 2.0, 0.0),
            pt(lb / 2.0 * ang.cos(), lb / 2.0 * ang.sin()),
        )) {
            Ok(out) => {
                ok += 1;
                assert_tangent_joint(&p, s, a, b, joint_of(&out), rad);
                let d = rad / (ang / 2.0).tan();
                assert!(d < la.min(lb) + 1e-9);
            }
            Err(_) => {
                refused += 1;
                assert_eq!(snapshot(&p), before);
                // Un refus est justifié : le point de tangence sortirait d'un segment.
                let d = rad / (ang / 2.0).tan();
                assert!(
                    d > la.min(lb) - 1e-6,
                    "refus injustifié d={d} la={la} lb={lb}"
                );
            }
        }
    }
    assert!(ok > 50 && refused > 20, "ok={ok} refused={refused}");
}

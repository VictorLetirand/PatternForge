//! US-02.9 — aligner, répartir, mettre à l'horizontale / à la verticale.
//! Un test par critère d'acceptation et par option, refus sans trace, annulation
//! en une étape, et une campagne aléatoire (propriétés) sur le plan du cœur.

use pf_core::align::{AlignMode, DistributeMode};
use pf_core::*;
use pf_geometry::{distance_mm, PointMm};

const TOL: f64 = 1e-9;

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn setup() -> (Document, SheetId) {
    let p = Project::new("Aligner");
    let s = p.sheets[0].id;
    (Document::new(p), s)
}

fn add(doc: &mut Document, s: SheetId, x: f64, y: f64) -> PointId {
    doc.execute(Command::AddPoint {
        sheet: s,
        at: pt(x, y),
    })
    .unwrap()
    .points[0]
}

fn pos(doc: &Document, s: SheetId, p: PointId) -> PointMm {
    doc.project().sheet(s).unwrap().position(p).unwrap()
}

fn seg(doc: &mut Document, s: SheetId, a: PointMm, b: PointMm) -> (EntityId, PointId, PointId) {
    let o = doc
        .execute(Command::AddSegment {
            sheet: s,
            from: PointRef::New(a),
            to: PointRef::New(b),
        })
        .unwrap();
    (o.entities[0], o.points[0], o.points[1])
}

fn four(doc: &mut Document, s: SheetId) -> Vec<PointId> {
    [(10.0, 5.0), (30.0, 25.0), (-4.0, 12.5), (22.0, -7.0)]
        .iter()
        .map(|&(x, y)| add(doc, s, x, y))
        .collect()
}

/// Exécute le plan : une étape d'historique, annulation exacte.
fn run(doc: &mut Document, plan: Result<Option<Command>, CoreError>) {
    let before = doc.project().clone();
    let cmd = plan.unwrap().expect("un plan");
    doc.execute(cmd).unwrap();
    assert!(doc.undo());
    assert_eq!(*doc.project(), before, "annulation exacte en une étape");
    assert!(doc.redo());
}

#[test]
fn alignement_six_options_exact() {
    let (mut doc, s) = setup();
    let ids = four(&mut doc, s);
    type Case = (AlignMode, fn(PointMm) -> f64, f64, bool);
    let cases: [Case; 6] = [
        (AlignMode::Left, |p| p.x, -4.0, true),
        (AlignMode::Right, |p| p.x, 30.0, true),
        (AlignMode::Top, |p| p.y, 25.0, true),
        (AlignMode::Bottom, |p| p.y, -7.0, true),
        (AlignMode::CenterHorizontal, |p| p.y, 9.0, true),
        (AlignMode::CenterVertical, |p| p.x, 13.0, true),
    ];
    for (mode, coord, want, _) in cases {
        let (mut d, s) = (Document::new(doc.project().clone()), s);
        let plan = d.project().plan_align(s, &ids, mode);
        run(&mut d, plan);
        for &i in &ids {
            assert!((coord(pos(&d, s, i)) - want).abs() < TOL, "{mode:?}");
        }
        // L'autre coordonnée est conservée.
        let other = if matches!(
            mode,
            AlignMode::Left | AlignMode::Right | AlignMode::CenterVertical
        ) {
            |p: PointMm| p.y
        } else {
            |p: PointMm| p.x
        };
        for &i in &ids {
            assert_eq!(other(pos(&d, s, i)), other(pos(&doc, s, i)));
        }
    }
}

#[test]
fn alignement_sur_une_droite_de_reference() {
    let (mut doc, s) = setup();
    let ids = four(&mut doc, s);
    // Droite y = x + 1 (45°, passant par (0, 1)).
    let mode = AlignMode::OnLine {
        origin: pt(0.0, 1.0),
        angle_deg: 45.0,
    };
    let plan = doc.project().plan_align(s, &ids, mode);
    run(&mut doc, plan);
    for &i in &ids {
        let p = pos(&doc, s, i);
        assert!((p.y - p.x - 1.0).abs() < TOL, "{p:?}");
    }
}

#[test]
fn repartition_horizontale_verticale_et_le_long_d_une_droite() {
    let (mut doc, s) = setup();
    let ids: Vec<_> = [
        (0.0, 0.0),
        (3.0, 7.0),
        (9.0, -2.0),
        (10.0, 4.0),
        (20.0, 10.0),
    ]
    .iter()
    .map(|&(x, y)| add(&mut doc, s, x, y))
    .collect();
    let plan = doc
        .project()
        .plan_distribute(s, &ids, DistributeMode::Horizontal);
    let mut d = Document::new(doc.project().clone());
    run(&mut d, plan);
    for (k, &i) in ids.iter().enumerate() {
        assert!((pos(&d, s, i).x - 5.0 * k as f64).abs() < TOL);
        assert_eq!(pos(&d, s, i).y, pos(&doc, s, i).y);
    }
    // Vertical : extrêmes y = -2 (ids[2]) et 10 (ids[4]), pas de 3.
    let plan = doc
        .project()
        .plan_distribute(s, &ids, DistributeMode::Vertical);
    let mut d = Document::new(doc.project().clone());
    run(&mut d, plan);
    let mut ys: Vec<f64> = ids.iter().map(|&i| pos(&d, s, i).y).collect();
    ys.sort_by(f64::total_cmp);
    for (k, y) in ys.iter().enumerate() {
        assert!((y - (-2.0 + 3.0 * k as f64)).abs() < TOL, "{ys:?}");
    }
    // Le long de la droite entre les deux points les plus éloignés.
    let plan = doc
        .project()
        .plan_distribute(s, &ids, DistributeMode::AlongLine);
    let mut d = Document::new(doc.project().clone());
    run(&mut d, plan);
    let (a, b) = (pos(&d, s, ids[0]), pos(&d, s, ids[4]));
    assert_eq!((a, b), (pt(0.0, 0.0), pt(20.0, 10.0)));
    let mut ds: Vec<f64> = ids.iter().map(|&i| distance_mm(a, pos(&d, s, i))).collect();
    ds.sort_by(f64::total_cmp);
    let step = distance_mm(a, b) / 4.0;
    for (k, v) in ds.iter().enumerate() {
        assert!((v - step * k as f64).abs() < TOL, "{ds:?}");
    }
}

#[test]
fn rien_a_faire_est_un_no_op_sans_etape() {
    let (mut doc, s) = setup();
    let ids = four(&mut doc, s);
    let steps_before = doc.can_undo();
    let p = doc.project();
    assert_eq!(p.plan_align(s, &ids[..1], AlignMode::Left).unwrap(), None);
    assert_eq!(p.plan_align(s, &[], AlignMode::Top).unwrap(), None);
    assert_eq!(
        p.plan_align(s, &[ids[0], ids[0]], AlignMode::Top).unwrap(),
        None
    );
    assert_eq!(
        p.plan_distribute(s, &ids[..2], DistributeMode::Horizontal)
            .unwrap(),
        None
    );
    // Déjà alignés.
    let (mut d2, s2) = setup();
    let a = add(&mut d2, s2, 1.0, 5.0);
    let b = add(&mut d2, s2, 9.0, 5.0);
    assert_eq!(
        d2.project()
            .plan_align(s2, &[a, b], AlignMode::CenterHorizontal)
            .unwrap(),
        None
    );
    // Déjà répartis.
    let c = add(&mut d2, s2, 5.0, 8.0);
    assert_eq!(
        d2.project()
            .plan_distribute(s2, &[a, b, c], DistributeMode::Horizontal)
            .unwrap(),
        None
    );
    assert_eq!(doc.can_undo(), steps_before);
}

#[test]
fn point_construit_refuse_avec_liste_et_sans_deplacement_partiel() {
    let (mut doc, s) = setup();
    let (e, a, b) = seg(&mut doc, s, pt(0.0, 0.0), pt(40.0, 0.0));
    let mid = doc
        .execute(Command::AddPointOnSegment {
            sheet: s,
            segment: e,
            distance_mm: 10.0,
            origin: SegmentEnd::Start,
        })
        .unwrap()
        .points[0];
    let free = add(&mut doc, s, 7.0, 30.0);
    let label = doc.project().sheet(s).unwrap().label(mid).to_string();
    let before = doc.project().clone();
    let err = doc
        .project()
        .plan_align(s, &[a, b, mid, free], AlignMode::Top)
        .unwrap_err();
    assert!(err.to_string().contains(&label), "{err}");
    // Aucune trace : le plan ne touche pas au projet.
    assert_eq!(*doc.project(), before);
    // Le point construit en référence immobile (déjà sur la cible) est accepté.
    let ok = doc.project().plan_align(s, &[mid, free], AlignMode::Bottom);
    assert!(ok.is_ok(), "{ok:?}");
}

#[test]
fn point_verrouille_refuse_sans_trace() {
    let (mut doc, s) = setup();
    let l = doc
        .execute(Command::AddLayer {
            sheet: s,
            name: "Gelé".into(),
        })
        .unwrap()
        .layers[0];
    let locked = doc
        .execute(Command::InLayer {
            sheet: s,
            layer: l,
            command: Box::new(Command::AddPoint {
                sheet: s,
                at: pt(50.0, 50.0),
            }),
        })
        .unwrap()
        .points[0];
    let free = add(&mut doc, s, 0.0, 0.0);
    doc.execute(Command::SetLayerLocked {
        sheet: s,
        layer: l,
        locked: true,
    })
    .unwrap();
    let before = doc.project().clone();
    let err = doc
        .project()
        .plan_align(s, &[locked, free], AlignMode::Left)
        .unwrap_err();
    assert!(matches!(err, CoreError::LayerLocked(_)), "{err}");
    assert_eq!(*doc.project(), before);
}

#[test]
fn nan_et_infini_refuses() {
    let (mut doc, s) = setup();
    let ids = four(&mut doc, s);
    for angle in [f64::NAN, f64::INFINITY] {
        let mode = AlignMode::OnLine {
            origin: pt(0.0, 0.0),
            angle_deg: angle,
        };
        assert!(doc.project().plan_align(s, &ids, mode).is_err());
    }
    let mode = AlignMode::OnLine {
        origin: pt(f64::NAN, 0.0),
        angle_deg: 0.0,
    };
    assert!(doc.project().plan_align(s, &ids, mode).is_err());
    let (e, ..) = seg(&mut doc, s, pt(0.0, 0.0), pt(10.0, 3.0));
    assert!(doc
        .project()
        .plan_level(s, &[e], true, Some(pt(f64::INFINITY, 0.0)), None)
        .is_err());
}

#[test]
fn mise_a_l_horizontale_et_a_la_verticale_avec_tout_ce_qui_est_attache() {
    for horizontal in [true, false] {
        let (mut doc, s) = setup();
        let (e, a, b) = seg(&mut doc, s, pt(10.0, 10.0), pt(40.0, 50.0));
        // Un second tracé attaché en b, et un point construit sur e.
        let c = doc
            .execute(Command::AddSegment {
                sheet: s,
                from: PointRef::Existing(b),
                to: PointRef::New(pt(60.0, 20.0)),
            })
            .unwrap()
            .points[0];
        let on = doc
            .execute(Command::AddPointOnSegment {
                sheet: s,
                segment: e,
                distance_mm: 10.0,
                origin: SegmentEnd::Start,
            })
            .unwrap()
            .points[0];
        let loose = add(&mut doc, s, 100.0, 100.0);
        let len0 = distance_mm(pos(&doc, s, a), pos(&doc, s, b));
        let bc0 = distance_mm(pos(&doc, s, b), pos(&doc, s, c));
        let loose0 = pos(&doc, s, loose);
        let plan = doc.project().plan_level(s, &[e], horizontal, None, None);
        run(&mut doc, plan);
        let (pa, pb) = (pos(&doc, s, a), pos(&doc, s, b));
        let off = if horizontal { pb.y - pa.y } else { pb.x - pa.x };
        assert!(off.abs() < TOL, "{off}");
        assert_eq!(pa, pt(10.0, 10.0), "pivot = départ");
        assert!((distance_mm(pa, pb) - len0).abs() < TOL);
        assert!((distance_mm(pb, pos(&doc, s, c)) - bc0).abs() < TOL);
        assert!((distance_mm(pa, pos(&doc, s, on)) - 10.0).abs() < TOL);
        assert_eq!(pos(&doc, s, loose), loose0, "le point isolé ne bouge pas");
    }
}

#[test]
fn mise_a_l_horizontale_pivot_choisi_deux_segments_et_refus() {
    let (mut doc, s) = setup();
    let (e1, a1, b1) = seg(&mut doc, s, pt(0.0, 0.0), pt(30.0, 10.0));
    let (e2, a2, b2) = seg(&mut doc, s, pt(100.0, 0.0), pt(100.0, 40.0));
    // Pivot choisi (milieu de e1) : le segment reste centré.
    let plan = doc
        .project()
        .plan_level(s, &[e1], true, Some(pt(15.0, 5.0)), None);
    let mut d = Document::new(doc.project().clone());
    run(&mut d, plan);
    let (p, q) = (pos(&d, s, a1), pos(&d, s, b1));
    assert!((p.y - q.y).abs() < TOL);
    assert!(distance_mm(pt((p.x + q.x) / 2.0, p.y), pt(15.0, 5.0)) < TOL);
    // Deux segments indépendants : tous deux à l'horizontale en une étape.
    let plan = doc.project().plan_level(s, &[e1, e2], true, None, None);
    run(&mut doc, plan);
    assert!((pos(&doc, s, a1).y - pos(&doc, s, b1).y).abs() < TOL);
    assert!((pos(&doc, s, a2).y - pos(&doc, s, b2).y).abs() < TOL);
    // Déjà d'aplomb : no-op. Non-segment : refus.
    assert_eq!(
        doc.project()
            .plan_level(s, &[e1, e2], true, None, None)
            .unwrap(),
        None
    );
    assert!(doc.project().plan_level(s, &[], true, None, None).is_err());
}

#[test]
fn segment_degenere_et_segments_lies_refuses() {
    let (mut doc, s) = setup();
    // Deux segments qui partagent un sommet : un seul ne peut pas obéir aux deux.
    let (e1, _, b1) = seg(&mut doc, s, pt(0.0, 0.0), pt(10.0, 10.0));
    let e2 = doc
        .execute(Command::AddSegment {
            sheet: s,
            from: PointRef::Existing(b1),
            to: PointRef::New(pt(10.0, 30.0)),
        })
        .unwrap()
        .entities[0];
    let before = doc.project().clone();
    let err = doc
        .project()
        .plan_level(s, &[e1, e2], true, None, None)
        .unwrap_err();
    assert!(err.to_string().contains("lié"), "{err}");
    assert_eq!(*doc.project(), before);
}

#[test]
fn campagne_aleatoire_proprietes() {
    // Générateur congruentiel (sans dépendance) ; 200 tirages.
    let mut seed = 0x2545_F491_4F6C_DD1Du64;
    let mut rnd = move || {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((seed >> 11) as f64 / (1u64 << 53) as f64) * 400.0 - 200.0
    };
    for round in 0..200 {
        let (mut doc, s) = setup();
        let n = 2 + (round % 6);
        let ids: Vec<_> = (0..n).map(|_| add(&mut doc, s, rnd(), rnd())).collect();
        let modes = [
            AlignMode::Left,
            AlignMode::Right,
            AlignMode::Top,
            AlignMode::Bottom,
            AlignMode::CenterHorizontal,
            AlignMode::CenterVertical,
            AlignMode::OnLine {
                origin: pt(rnd(), rnd()),
                angle_deg: rnd(),
            },
        ];
        let mode = modes[round % modes.len()];
        let before = doc.project().clone();
        let plan = doc
            .project()
            .plan_align(s, &ids, mode)
            .unwrap()
            .expect("plan");
        doc.execute(plan).unwrap();
        let pts: Vec<PointMm> = ids.iter().map(|&i| pos(&doc, s, i)).collect();
        match mode {
            AlignMode::Left | AlignMode::Right | AlignMode::CenterVertical => {
                assert!(pts.iter().all(|p| (p.x - pts[0].x).abs() < TOL))
            }
            AlignMode::Top | AlignMode::Bottom | AlignMode::CenterHorizontal => {
                assert!(pts.iter().all(|p| (p.y - pts[0].y).abs() < TOL))
            }
            AlignMode::OnLine { origin, angle_deg } => {
                let (sn, cs) = pf_geometry::sin_cos_deg(angle_deg);
                for p in &pts {
                    let d = (cs * (p.y - origin.y) - sn * (p.x - origin.x)).abs();
                    assert!(d < 1e-7, "{d}");
                }
            }
        }
        // Idempotence : un second passage est un no-op (sauf droite oblique : tolérance).
        if !matches!(mode, AlignMode::OnLine { .. }) {
            assert_eq!(doc.project().plan_align(s, &ids, mode).unwrap(), None);
        }
        assert!(doc.undo());
        assert_eq!(*doc.project(), before);
        // Répartition : pas égaux.
        if n >= 3 {
            let plan = doc
                .project()
                .plan_distribute(s, &ids, DistributeMode::Horizontal)
                .unwrap();
            if let Some(plan) = plan {
                doc.execute(plan).unwrap();
                let mut xs: Vec<f64> = ids.iter().map(|&i| pos(&doc, s, i).x).collect();
                xs.sort_by(f64::total_cmp);
                let step = (xs[xs.len() - 1] - xs[0]) / (xs.len() - 1) as f64;
                for (k, x) in xs.iter().enumerate() {
                    assert!((x - (xs[0] + step * k as f64)).abs() < 1e-7);
                }
            }
        }
    }
}

#[test]
fn extremites_d_arc_jamais_de_resultat_inexact() {
    let (mut doc, s) = setup();
    let e = doc
        .execute(Command::AddArc {
            sheet: s,
            center: PointRef::New(pt(0.0, 0.0)),
            radius_mm: 10.0,
            start_deg: 20.0,
            end_deg: 100.0,
            ccw: true,
        })
        .unwrap()
        .entities[0];
    let pts = doc
        .project()
        .sheet(s)
        .unwrap()
        .entity_all_points(e)
        .unwrap();
    let before = doc.project().clone();
    for mode in [AlignMode::Left, AlignMode::Top, AlignMode::CenterVertical] {
        if let Ok(Some(cmd)) = doc.project().plan_align(s, &pts, mode) {
            // Un plan rendu est exact : chaque point atteint sa cible.
            let mut d = Document::new(before.clone());
            d.execute(cmd).unwrap();
            let xs: Vec<f64> = pts.iter().map(|&p| pos(&d, s, p).x).collect();
            let ys: Vec<f64> = pts.iter().map(|&p| pos(&d, s, p).y).collect();
            let all_eq = |v: &[f64]| v.iter().all(|a| (a - v[0]).abs() < TOL);
            assert!(all_eq(&xs) || all_eq(&ys), "{mode:?} {xs:?} {ys:?}");
        }
        assert_eq!(*doc.project(), before);
    }
}

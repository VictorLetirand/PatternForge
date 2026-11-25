//! US-03.1 CA 3, restes (tranche 118) — campagne adverse : 160 séquences à graine fixe de 40 pas.
//!
//! Quatre familles de paramètres liables à une formule : distance d'un point sur un segment
//! (`SegmentDistance`, depuis le départ ou l'arrivée), proportion sur un segment (`SegmentRatio`),
//! position `t` sur une courbe de Bézier (`CurveRatio`) et fraction de division (`DivisionRatio`),
//! pilotées par deux variables `a` et `b` ; des points sont aussi **créés avec leur formule**
//! (`AddPointWithFormulas`).
//!
//! **Oracle indépendant** : tenu ici sans appeler `pf_core::formula` ni `pf_core::expr`. Les
//! expressions sont évaluées par des fermetures écrites à la main ; les positions viennent de
//! formules fermées (le segment fait 1000 mm sur l'axe X, la courbe de Bézier est évaluée par ses
//! polynômes de Bernstein). Pour chaque commande l'oracle prédit accepté / refusé et l'état
//! résultant ; un refus laisse les octets identiques, un geste accepté qui change l'état fait
//! exactement une étape annulable (annuler = octets d'avant, rétablir = octets d'après).
//! Des compteurs de couverture minimaux prouvent que les cas difficiles sont exercés.

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;

const SEQUENCES: u64 = 160;
const STEPS: usize = 40;
const TOL: f64 = 1e-6;
const MAX_POINTS: usize = 14;

struct Rng(u64);

impl Rng {
    fn step(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.step() % n
    }
    fn val(&mut self, lo: f64, hi: f64) -> f64 {
        let n = ((hi - lo) * 100.0) as u64;
        lo + self.below(n + 1) as f64 / 100.0
    }
}

#[derive(Default, Debug)]
struct Counters {
    var_ok: u32,
    var_refused: u32,
    bind_ok: u32,
    bind_refused: u32,
    bind_wrong_kind: u32,
    unbind: u32,
    direct_unlink: u32,
    direct_same_keeps: u32,
    direct_refused: u32,
    created: [u32; 4],
    created_from_end: u32,
    create_range_refused: u32,
    create_dup_refused: u32,
    remove_refused: u32,
    noop: u32,
    undo_checks: u32,
    reload_checks: u32,
}

/// Expressions (les mêmes textes que le cœur lit) et leur évaluation écrite à part.
const EXPR_D: [&str; 7] = [
    "a",
    "b",
    "a+b",
    "a*4-b",
    "min(a;b)",
    "max(a;b)*3",
    "abs(a-b)+1",
];
const EXPR_R: [&str; 7] = [
    "a/200",
    "b/(a+b)",
    "1/3",
    "(a-b)/400+0,5",
    "min(a;b)/max(a;b)",
    "a/b/2",
    "(a+b)/800",
];

fn eval_d(i: usize, a: f64, b: f64) -> f64 {
    match i {
        0 => a,
        1 => b,
        2 => a + b,
        3 => a * 4.0 - b,
        4 => a.min(b),
        5 => a.max(b) * 3.0,
        _ => (a - b).abs() + 1.0,
    }
}

fn eval_r(i: usize, a: f64, b: f64) -> f64 {
    match i {
        0 => a / 200.0,
        1 => b / (a + b),
        2 => 1.0 / 3.0,
        3 => (a - b) / 400.0 + 0.5,
        4 => a.min(b) / a.max(b),
        5 => a / b / 2.0,
        _ => (a + b) / 800.0,
    }
}

/// 0 distance sur segment, 1 proportion sur segment, 2 t sur courbe, 3 fraction de division.
const KINDS: [ParamKind; 4] = [
    ParamKind::SegmentDistance,
    ParamKind::SegmentRatio,
    ParamKind::CurveRatio,
    ParamKind::DivisionRatio,
];

fn eval_kind(k: usize, e: usize, a: f64, b: f64) -> f64 {
    if k == 0 {
        eval_d(e, a, b)
    } else {
        eval_r(e, a, b)
    }
}

fn expr_text(k: usize, e: usize) -> &'static str {
    if k == 0 {
        EXPR_D[e]
    } else {
        EXPR_R[e]
    }
}

/// Valeur acceptée par la géométrie : segment de 1000 mm ; `t` dans [0 ; 1] ; division dans ]0 ; 1[.
fn valid(k: usize, v: f64) -> bool {
    match k {
        0 => (0.0..=1000.0).contains(&v),
        1 | 2 => (0.0..=1.0).contains(&v),
        _ => v > 0.0 && v < 1.0,
    }
}

/// Courbe de Bézier (0;500) (200;900) (600;100) (800;500) : polynômes de Bernstein.
fn bezier(t: f64) -> (f64, f64) {
    let u = 1.0 - t;
    let (b0, b1, b2, b3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
    (
        b1 * 200.0 + b2 * 600.0 + b3 * 800.0,
        b0 * 500.0 + b1 * 900.0 + b2 * 100.0 + b3 * 500.0,
    )
}

#[derive(Clone)]
struct Tp {
    id: PointId,
    kind: usize,
    from_end: bool,
    val: f64,
    bound: Option<usize>,
}

impl Tp {
    fn pos(&self) -> (f64, f64) {
        match self.kind {
            0 if self.from_end => (1000.0 - self.val, 0.0),
            0 => (self.val, 0.0),
            1 | 3 => (1000.0 * self.val, 0.0),
            _ => bezier(self.val),
        }
    }
}

const FREE: [(f64, f64); 4] = [(0.0, 0.0), (1000.0, 0.0), (0.0, 500.0), (800.0, 500.0)];

fn pos_of(kind: usize, from_end: bool, val: f64) -> (f64, f64) {
    Tp {
        id: PointId(0),
        kind,
        from_end,
        val,
        bound: None,
    }
    .pos()
}

fn near(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

struct World {
    p: Project,
    s: SheetId,
    seg: EntityId,
    curve: EntityId,
    a: f64,
    b: f64,
    pts: Vec<Tp>,
}

fn build_world(rng: &mut Rng) -> World {
    let mut p = Project::new("Adverse formules restes");
    let s = p.sheets[0].id;
    let (a, b) = (rng.val(1.0, 400.0), rng.val(1.0, 400.0));
    for (n, v) in [("a", a), ("b", b)] {
        p.apply(Command::AddVariable {
            name: n.into(),
            unit: VariableUnit::Mm,
            expr: format!("{v}"),
        })
        .unwrap();
    }
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
            start: PointRef::New(PointMm::new(0.0, 500.0)),
            c1: PointMm::new(200.0, 900.0),
            c2: PointMm::new(600.0, 100.0),
            end: PointRef::New(PointMm::new(800.0, 500.0)),
        })
        .unwrap()
        .entities[0];
    let mut add = |d: PointDefinition| {
        p.apply(Command::AddPointAt {
            sheet: s,
            point: PointRef::Derived(d),
        })
        .unwrap()
        .points[0]
    };
    let id0 = add(PointDefinition::OnSegment {
        segment: seg,
        distance_mm: 10.0,
        origin: SegmentEnd::Start,
    });
    let id1 = add(PointDefinition::OnSegmentRatio {
        segment: seg,
        ratio: 0.02,
    });
    let id2 = add(PointDefinition::OnCurve { curve, t: 0.3 });
    let id3 = p
        .apply(Command::DivideTrace {
            sheet: s,
            trace: seg,
            spec: DivisionSpec::Equal { parts: 2 },
            start: None,
        })
        .unwrap()
        .points[0];
    let pts = vec![
        Tp {
            id: id0,
            kind: 0,
            from_end: false,
            val: 10.0,
            bound: None,
        },
        Tp {
            id: id1,
            kind: 1,
            from_end: false,
            val: 0.02,
            bound: None,
        },
        Tp {
            id: id2,
            kind: 2,
            from_end: false,
            val: 0.3,
            bound: None,
        },
        Tp {
            id: id3,
            kind: 3,
            from_end: false,
            val: 0.5,
            bound: None,
        },
    ];
    World {
        p,
        s,
        seg,
        curve,
        a,
        b,
        pts,
    }
}

fn check(w: &World) {
    let sheet = w.p.sheet(w.s).unwrap();
    let mut bound_count = 0;
    for t in &w.pts {
        let got = sheet.parameter_value(t.id, KINDS[t.kind]).unwrap();
        assert!(
            (got - t.val).abs() < 1e-9,
            "point {:?} : paramètre {got}, oracle {}",
            t.id,
            t.val
        );
        match (t.bound, sheet.formula(t.id, KINDS[t.kind])) {
            (Some(e), Some(f)) => {
                assert_eq!(f.expr, expr_text(t.kind, e), "texte de la formule");
                bound_count += 1;
            }
            (None, None) => {}
            (o, f) => panic!("formule : oracle {o:?}, projet {f:?}"),
        }
        let q = sheet.position(t.id).unwrap();
        assert!(
            near((q.x, q.y), t.pos()) < TOL,
            "position {:?} : ({} ; {}), oracle {:?}",
            t.id,
            q.x,
            q.y,
            t.pos()
        );
    }
    assert_eq!(sheet.formulas.len(), bound_count, "formules en trop");
}

/// Exécute `cmd` ; `expect_ok` vient de l'oracle. Renvoie le résultat si accepté.
fn run(w: &mut World, cmd: Command, expect_ok: bool, c: &mut Counters) -> Option<Outcome> {
    let before = persist::to_json(&w.p).unwrap();
    let mut doc = Document::new(w.p.clone());
    match (doc.execute(cmd), expect_ok) {
        (Ok(out), true) => {
            let after = persist::to_json(doc.project()).unwrap();
            if after == before {
                assert!(!doc.can_undo(), "étape créée sans changement");
                c.noop += 1;
            } else {
                assert!(doc.undo(), "étape annulable");
                assert_eq!(persist::to_json(doc.project()).unwrap(), before);
                assert!(!doc.undo(), "une seule étape");
                assert!(doc.redo());
                assert_eq!(persist::to_json(doc.project()).unwrap(), after);
                c.undo_checks += 1;
            }
            w.p = doc.project().clone();
            Some(out)
        }
        (Err(e), false) => {
            assert_eq!(
                persist::to_json(doc.project()).unwrap(),
                before,
                "refus : {e}"
            );
            assert!(!doc.can_undo());
            None
        }
        (Ok(_), false) => panic!("accepté alors que l'oracle refuse"),
        (Err(e), true) => panic!("refusé alors que l'oracle accepte : {e}"),
    }
}

fn recompute(w: &mut World) -> bool {
    let (a, b) = (w.a, w.b);
    let mut ok = true;
    for t in &mut w.pts {
        if let Some(e) = t.bound {
            t.val = eval_kind(t.kind, e, a, b);
            ok &= valid(t.kind, t.val);
        }
    }
    ok
}

fn step(w: &mut World, rng: &mut Rng, c: &mut Counters) {
    let s = w.s;
    match rng.below(11) {
        // 0..=1 : changer une variable
        0 | 1 => {
            let is_a = rng.below(2) == 0;
            let val = rng.val(1.0, 400.0);
            let saved = w.pts.clone();
            let (sa, sb) = (w.a, w.b);
            if is_a {
                w.a = val;
            } else {
                w.b = val;
            }
            let ok = recompute(w);
            let cmd = Command::SetVariableExpr {
                name: if is_a { "a" } else { "b" }.into(),
                expr: format!("{val}"),
            };
            let (na, nb, npts) = (w.a, w.b, w.pts.clone());
            w.a = sa;
            w.b = sb;
            w.pts = saved;
            if run(w, cmd, ok, c).is_some() {
                w.a = na;
                w.b = nb;
                w.pts = npts;
                c.var_ok += 1;
            } else {
                c.var_refused += 1;
            }
        }
        // 2..=3 : lier une formule
        2 | 3 => {
            let i = rng.below(w.pts.len() as u64) as usize;
            let t = w.pts[i].clone();
            let e = rng.below(7) as usize;
            let v = eval_kind(t.kind, e, w.a, w.b);
            let cmd = Command::SetParameterExpr {
                target: ParamTarget {
                    sheet: s,
                    point: t.id,
                    param: KINDS[t.kind],
                },
                expr: Some(expr_text(t.kind, e).into()),
            };
            if run(w, cmd, valid(t.kind, v), c).is_some() {
                w.pts[i].val = v;
                w.pts[i].bound = Some(e);
                c.bind_ok += 1;
            } else {
                c.bind_refused += 1;
            }
        }
        // 4 : délier, ou lier un paramètre d'un autre type (refusé)
        4 => {
            let i = rng.below(w.pts.len() as u64) as usize;
            let t = w.pts[i].clone();
            if rng.below(3) == 0 {
                let other = (t.kind + 1 + rng.below(3) as usize) % 4;
                let cmd = Command::SetParameterExpr {
                    target: ParamTarget {
                        sheet: s,
                        point: t.id,
                        param: KINDS[other],
                    },
                    expr: Some("a".into()),
                };
                assert!(run(w, cmd, false, c).is_none());
                c.bind_wrong_kind += 1;
            } else {
                let cmd = Command::SetParameterExpr {
                    target: ParamTarget {
                        sheet: s,
                        point: t.id,
                        param: KINDS[t.kind],
                    },
                    expr: None,
                };
                c.unbind += t.bound.is_some() as u32;
                assert!(run(w, cmd, true, c).is_some());
                w.pts[i].bound = None;
            }
        }
        // 5..=6 : valeur écrite directement (délie ce qui change)
        5 | 6 => {
            let i = rng.below(w.pts.len() as u64) as usize;
            let t = w.pts[i].clone();
            let v = if rng.below(5) == 0 {
                t.val
            } else if t.kind == 0 {
                rng.val(-50.0, 1100.0)
            } else {
                rng.val(-20.0, 120.0) / 100.0
            };
            let cmd = if t.kind == 0 {
                Command::SetPointDistance {
                    sheet: s,
                    point: t.id,
                    distance_mm: v,
                }
            } else {
                Command::SetPointParameter {
                    sheet: s,
                    point: t.id,
                    value: v,
                }
            };
            if run(w, cmd, valid(t.kind, v), c).is_some() {
                if v != t.val && t.bound.is_some() {
                    w.pts[i].bound = None;
                    c.direct_unlink += 1;
                }
                if v == t.val && t.bound.is_some() {
                    c.direct_same_keeps += 1;
                }
                w.pts[i].val = v;
            } else {
                c.direct_refused += 1;
            }
        }
        // 7..=9 : créer un point avec sa formule
        7..=9 if w.pts.len() < MAX_POINTS => {
            let k = rng.below(4) as usize;
            let e = rng.below(7) as usize;
            let from_end = k == 0 && rng.below(2) == 0;
            let v = eval_kind(k, e, w.a, w.b);
            let definition = match k {
                0 => PointDefinition::OnSegment {
                    segment: w.seg,
                    distance_mm: 1.0,
                    origin: if from_end {
                        SegmentEnd::End
                    } else {
                        SegmentEnd::Start
                    },
                },
                1 => PointDefinition::OnSegmentRatio {
                    segment: w.seg,
                    ratio: 0.77,
                },
                2 => PointDefinition::OnCurve {
                    curve: w.curve,
                    t: 0.77,
                },
                _ => PointDefinition::Division {
                    trace: w.seg,
                    ratio: 0.77,
                    start: None,
                },
            };
            let cmd = Command::AddPointWithFormulas {
                sheet: s,
                definition,
                formulas: vec![ParamExpr {
                    param: KINDS[k],
                    expr: format!(" {} ", expr_text(k, e)),
                }],
            };
            let here = pos_of(k, from_end, v);
            let in_range = valid(k, v);
            let clash = FREE
                .iter()
                .copied()
                .chain(w.pts.iter().map(Tp::pos))
                .any(|q| near(q, here) <= 1e-6);
            let ok = in_range && !clash;
            match run(w, cmd, ok, c) {
                Some(out) => {
                    assert_eq!(out.points.len(), 1);
                    w.pts.push(Tp {
                        id: out.points[0],
                        kind: k,
                        from_end,
                        val: v,
                        bound: Some(e),
                    });
                    c.created[k] += 1;
                    c.created_from_end += from_end as u32;
                }
                None if !in_range => c.create_range_refused += 1,
                None => c.create_dup_refused += 1,
            }
        }
        // 10 : supprimer une variable citée (refusé) ; sinon rien
        _ => {
            let cites = |name: &str| {
                w.pts
                    .iter()
                    .filter_map(|t| t.bound.map(|e| expr_text(t.kind, e)))
                    .any(|x| x.contains(name))
            };
            let name = if rng.below(2) == 0 { "a" } else { "b" };
            if cites(name) {
                let cmd = Command::RemoveVariable { name: name.into() };
                assert!(run(w, cmd, false, c).is_none());
                c.remove_refused += 1;
            }
        }
    }
}

#[test]
fn campagne_adverse_formules_restes() {
    let mut c = Counters::default();
    let mut commands = 0u64;
    for seq in 0..SEQUENCES {
        let mut rng = Rng(0xA5A5_1234_9E37_79B9 ^ (seq + 1).wrapping_mul(0xD1B5_4A32_D192_ED03));
        for _ in 0..4 {
            rng.step();
        }
        let mut w = build_world(&mut rng);
        check(&w);
        for n in 0..STEPS {
            step(&mut w, &mut rng, &mut c);
            check(&w);
            commands += 1;
            if n % 8 == 0 {
                // Relecture : valeur et octets identiques, formules revérifiées à l'ouverture.
                let json = persist::to_json(&w.p).unwrap();
                let back = persist::from_json(&json).expect("relecture");
                assert_eq!(back, w.p);
                assert_eq!(persist::to_json(&back).unwrap(), json);
                c.reload_checks += 1;
            }
        }
    }
    eprintln!("{SEQUENCES} séquences, {commands} commandes : {c:?}");
    assert!(c.var_ok >= 400 && c.var_refused >= 100, "{c:?}");
    assert!(c.bind_ok >= 300 && c.bind_refused >= 100, "{c:?}");
    assert!(c.bind_wrong_kind >= 40 && c.unbind >= 60, "{c:?}");
    assert!(c.direct_unlink >= 100 && c.direct_refused >= 100, "{c:?}");
    assert!(c.direct_same_keeps >= 10, "{c:?}");
    assert!(c.created.iter().all(|n| *n >= 100), "{c:?}");
    assert!(c.created_from_end >= 50, "{c:?}");
    assert!(
        c.create_range_refused >= 100 && c.create_dup_refused >= 20,
        "{c:?}"
    );
    assert!(c.remove_refused >= 100, "{c:?}");
    assert!(c.undo_checks >= 1500, "{c:?}");
}

//! Restes d'E03 (tranche 107) — campagne adverse : 160 séquences à graine fixe de 45 pas.
//!
//! Deux mondes par séquence :
//! - des **rectangles** créés par `AddConstructedRectangle` (maintenus ou non), déplacés par coin,
//!   liés / déliés côté par côté (`SetConstructionKept`) ;
//! - quatre **paramètres à formule** (distance sur un tracé, distance sur une bissectrice,
//!   rayons du compas) pilotés par deux variables, liés / déliés, ou écrits directement.
//!
//! **Oracle indépendant** : tenu ici, sans appeler `pf_core::kept` ni `pf_core::formula`. Les
//! positions des rectangles sont recalculées avec cos / sin et la normale `(-dy ; dx) / |d|` ;
//! les positions des points construits avec leurs formules fermées (`d / racine(2)`,
//! intersection de deux cercles). Pour chaque commande l'oracle prédit accepté / refusé et l'état
//! résultant ; un refus laisse les octets identiques, un geste accepté qui change l'état fait
//! exactement une étape annulable (annuler = octets d'avant, rétablir = octets d'après).
//! Des compteurs de couverture minimaux prouvent que les cas difficiles sont exercés.

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;

const SEQUENCES: u64 = 160;
const STEPS: usize = 45;
const TOL: f64 = 1e-6;

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
    rect_added: u32,
    rect_added_kept: u32,
    move_corner_linked: u32,
    move_p1_unlinks: u32,
    move_p23_unlinks: u32,
    link_ok: u32,
    link_refused: u32,
    unlink_ok: u32,
    noop: u32,
    var_ok: u32,
    var_refused: u32,
    rebind_ok: u32,
    rebind_refused: u32,
    direct_ok: u32,
    direct_refused: u32,
    unbind: u32,
    undo_checks: u32,
    reload_checks: u32,
}

// ---------------------------------------------------------------------------------------------
// Oracle des rectangles
// ---------------------------------------------------------------------------------------------

#[derive(Clone)]
struct Rect {
    ids: [PointId; 4], // p0, p1, p2, p3
    segs: [EntityId; 2],
    w: f64,
    k: f64, // hauteur algébrique le long de la normale gauche de p0 → p1
    ang: f64,
    pos: [(f64, f64); 4],
    link0: bool,
    link2: bool,
}

impl Rect {
    fn expected_p1(&self) -> (f64, f64) {
        let a = self.ang.to_radians();
        (
            self.pos[0].0 + self.w.abs() * a.cos(),
            self.pos[0].1 + self.w.abs() * a.sin(),
        )
    }
    /// (p2, p3) d'après p0 et p1 courants.
    fn expected_23(&self) -> ((f64, f64), (f64, f64)) {
        let (p0, p1) = (self.pos[0], self.pos[1]);
        let (dx, dy) = (p1.0 - p0.0, p1.1 - p0.1);
        let n = (dx * dx + dy * dy).sqrt();
        let (nx, ny) = (-dy / n * self.k, dx / n * self.k);
        ((p1.0 + nx, p1.1 + ny), (p0.0 + nx, p0.1 + ny))
    }
    fn propagate(&mut self) {
        if self.link0 {
            self.pos[1] = self.expected_p1();
        }
        if self.link2 {
            let (p2, p3) = self.expected_23();
            self.pos[2] = p2;
            self.pos[3] = p3;
        }
    }
}

fn near(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

// ---------------------------------------------------------------------------------------------
// Oracle des formules
// ---------------------------------------------------------------------------------------------

const EXPRS: [&str; 7] = [
    "a",
    "b",
    "a+b",
    "a*2+b",
    "min(a;b)",
    "max(a;b)",
    "abs(a-b)+1",
];

fn eval(i: usize, a: f64, b: f64) -> f64 {
    match i {
        0 => a,
        1 => b,
        2 => a + b,
        3 => a * 2.0 + b,
        4 => a.min(b),
        5 => a.max(b),
        _ => (a - b).abs() + 1.0,
    }
}

/// Paramètres : 0 along, 1 bissectrice, 2 rayon A, 3 rayon B.
#[derive(Clone)]
struct Formulas {
    a: f64,
    b: f64,
    vals: [f64; 4],
    bound: [Option<usize>; 4],
}

/// Valide les 4 valeurs : le tracé fait 1000 mm, les centres sont à 100 mm.
fn geometry_ok(v: &[f64; 4]) -> bool {
    let along = (0.0..=1000.0).contains(&v[0]);
    let bis = v[1] > 0.0;
    let (ra, rb) = (v[2], v[3]);
    let reach = ra > 0.0 && rb > 0.0 && ra + rb >= 100.0 && (ra - rb).abs() <= 100.0;
    along && bis && reach
}

fn compass_pos(ra: f64, rb: f64) -> (f64, f64) {
    let x = (100.0 * 100.0 + ra * ra - rb * rb) / 200.0;
    (x, (ra * ra - x * x).max(0.0).sqrt())
}

/// Vrai si les cercles sont presque tangents (la position dépend alors d'un arrondi).
fn tangent(v: &[f64; 4]) -> bool {
    let (ra, rb) = (v[2], v[3]);
    (ra + rb - 100.0).abs() < 1e-6 || ((ra - rb).abs() - 100.0).abs() < 1e-6
}

struct World {
    p: Project,
    s: SheetId,
    rects: Vec<Rect>,
    f: Formulas,
    pts: [PointId; 3], // along, bisector, compass
}

fn kinds() -> [ParamKind; 4] {
    [
        ParamKind::AlongDistance,
        ParamKind::BisectorDistance,
        ParamKind::RadiusA,
        ParamKind::RadiusB,
    ]
}

fn derived(p: &mut Project, s: SheetId, d: PointDefinition) -> PointId {
    p.apply(Command::AddPointAt {
        sheet: s,
        point: PointRef::Derived(d),
    })
    .unwrap()
    .points[0]
}

fn free(p: &mut Project, s: SheetId, x: f64, y: f64) -> PointId {
    p.apply(Command::AddPoint {
        sheet: s,
        at: PointMm::new(x, y),
    })
    .unwrap()
    .points[0]
}

fn build_world(rng: &mut Rng) -> World {
    let mut p = Project::new("Adverse E03");
    let s = p.sheets[0].id;
    let a = rng.val(60.0, 100.0);
    let b = rng.val(60.0, 100.0);
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
    let along = derived(
        &mut p,
        s,
        PointDefinition::AlongCurve {
            curve: seg,
            distance_mm: 10.0,
            origin: SegmentEnd::Start,
        },
    );
    let (v, f1, f2) = (
        free(&mut p, s, 0.0, 2000.0),
        free(&mut p, s, 100.0, 2000.0),
        free(&mut p, s, 0.0, 2100.0),
    );
    let bis = derived(
        &mut p,
        s,
        PointDefinition::Bisector {
            vertex: v,
            first: f1,
            second: f2,
            distance_mm: 10.0,
        },
    );
    let (ca, cb) = (
        free(&mut p, s, 0.0, -1000.0),
        free(&mut p, s, 100.0, -1000.0),
    );
    let comp = derived(
        &mut p,
        s,
        PointDefinition::Compass {
            center_a: ca,
            center_b: cb,
            radius_a_mm: a,
            radius_b_mm: b,
            near: AimMm { x: 50.0, y: -950.0 },
        },
    );
    let pts = [along, bis, comp];
    let mut w = World {
        p,
        s,
        rects: Vec::new(),
        f: Formulas {
            a,
            b,
            vals: [10.0, 10.0, a, b],
            bound: [None; 4],
        },
        pts,
    };
    // Liaisons initiales : along = a*2 ... choisies parmi les expressions de l'oracle.
    for (i, e) in [(0, 0usize), (1, 1), (2, 0), (3, 1)] {
        let val = eval(e, a, b);
        w.p.apply(Command::SetParameterExpr {
            target: ParamTarget {
                sheet: s,
                point: w.pts[if i < 2 { i } else { 2 }],
                param: kinds()[i],
            },
            expr: Some(EXPRS[e].into()),
        })
        .unwrap();
        w.f.vals[i] = val;
        w.f.bound[i] = Some(e);
    }
    w
}

fn point_of(w: &World, i: usize) -> PointId {
    w.pts[if i < 2 { i } else { 2 }]
}

fn check(w: &World, c: &mut Counters) {
    let p = &w.p;
    let sheet = p.sheet(w.s).unwrap();
    for r in &w.rects {
        for i in 0..4 {
            let q = sheet.position(r.ids[i]).unwrap();
            assert!(
                near((q.x, q.y), r.pos[i]) < TOL,
                "rectangle : point {i} à ({} ; {}), oracle {:?}",
                q.x,
                q.y,
                r.pos[i]
            );
        }
        assert_eq!(sheet.is_kept_segment(r.segs[0]), r.link0, "état du côté 0");
        assert_eq!(sheet.is_kept_segment(r.segs[1]), r.link2, "état du côté 2");
    }
    let v = &w.f.vals;
    for (i, k) in kinds().iter().enumerate() {
        let got = sheet.parameter_value(point_of(w, i), *k).unwrap();
        assert!(
            (got - v[i]).abs() < 1e-9,
            "paramètre {i} : {got}, oracle {}",
            v[i]
        );
        let has = sheet.formula(point_of(w, i), *k).is_some();
        assert_eq!(has, w.f.bound[i].is_some(), "formule {i}");
    }
    let q = sheet.position(w.pts[0]).unwrap();
    assert!(near((q.x, q.y), (v[0], 0.0)) < TOL);
    let q = sheet.position(w.pts[1]).unwrap();
    let h = v[1] / 2f64.sqrt();
    assert!(near((q.x, q.y), (h, 2000.0 + h)) < TOL, "bissectrice");
    if !tangent(v) {
        let q = sheet.position(w.pts[2]).unwrap();
        let (x, y) = compass_pos(v[2], v[3]);
        assert!(near((q.x, q.y), (x, -1000.0 + y)) < TOL, "compas");
    }
    c.undo_checks += 0;
}

/// Exécute `cmd` ; l'oracle donne l'état attendu si accepté (`Some`), `None` si refusé.
fn run(
    w: &mut World,
    doc_cmd: Command,
    expect: Option<(Vec<Rect>, Formulas)>,
    c: &mut Counters,
) -> bool {
    let before = persist::to_json(&w.p).unwrap();
    let mut doc = Document::new(w.p.clone());
    match (doc.execute(doc_cmd), expect) {
        (Ok(_), Some((rects, f))) => {
            let after = persist::to_json(doc.project()).unwrap();
            if after == before {
                // aucun changement : aucune étape
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
            w.rects = rects;
            w.f = f;
            check(w, c);
            true
        }
        (Err(e), None) => {
            assert_eq!(
                persist::to_json(doc.project()).unwrap(),
                before,
                "refus : {e}"
            );
            assert!(!doc.can_undo());
            false
        }
        (Ok(_), None) => panic!("accepté alors que l'oracle refuse"),
        (Err(e), Some(_)) => panic!("refusé alors que l'oracle accepte : {e}"),
    }
}

fn step(w: &mut World, rng: &mut Rng, c: &mut Counters) {
    let s = w.s;
    match rng.below(14) {
        // 0..=1 : nouveau rectangle
        0 | 1 if w.rects.len() < 6 => {
            let i = w.rects.len() as f64;
            let (x, y) = (3000.0 + 400.0 * i, 5000.0 + rng.val(-50.0, 50.0));
            let mut wd = rng.val(5.0, 120.0);
            let mut ht = rng.val(5.0, 120.0);
            if rng.below(2) == 0 {
                wd = -wd;
            }
            if rng.below(2) == 0 {
                ht = -ht;
            }
            let kept = rng.below(2) == 0;
            let before = persist::to_json(&w.p).unwrap();
            let cmd = Command::AddConstructedRectangle {
                sheet: s,
                corner: PointRef::New(PointMm::new(x, y)),
                width_mm: wd,
                height_mm: ht,
                kept,
            };
            let out = w.p.apply(cmd).expect("rectangle accepté");
            assert_ne!(persist::to_json(&w.p).unwrap(), before);
            let [p0, p1, p3, p2] = out.points[..] else {
                panic!("4 points")
            };
            let ang = if wd > 0.0 { 0.0 } else { 180.0 };
            // Normale gauche de la direction (cos ang ; sin ang) = (−sin ; cos) = (0 ; ±1).
            let k = ht.abs() * if (ht > 0.0) == (wd > 0.0) { 1.0 } else { -1.0 };
            let mut r = Rect {
                ids: [p0, p1, p2, p3],
                segs: [out.entities[0], out.entities[2]],
                w: wd,
                k,
                ang,
                pos: [(x, y), (x + wd, y), (x + wd, y + ht), (x, y + ht)],
                link0: kept,
                link2: kept,
            };
            // Contrôle de l'oracle lui-même contre les positions d'AddRectangle.
            let probe = r.clone();
            r.propagate();
            for i in 0..4 {
                assert!(near(r.pos[i], probe.pos[i]) < TOL, "oracle de création");
            }
            w.rects.push(r);
            c.rect_added += 1;
            c.rect_added_kept += kept as u32;
            check(w, c);
        }
        // 2..=4 : déplacer p0
        2..=4 if !w.rects.is_empty() => {
            let ri = rng.below(w.rects.len() as u64) as usize;
            let mut r = w.rects[ri].clone();
            let to = (
                3000.0 + 400.0 * ri as f64 + rng.val(-50.0, 50.0),
                5000.0 + rng.val(-80.0, 80.0),
            );
            r.pos[0] = to;
            r.propagate();
            c.move_corner_linked += (r.link0 || r.link2) as u32;
            let mut rects = w.rects.clone();
            rects[ri] = r;
            let cmd = Command::MovePoint {
                sheet: s,
                point: w.rects[ri].ids[0],
                to: PointMm::new(to.0, to.1),
            };
            let f = w.f.clone();
            run(w, cmd, Some((rects, f)), c);
        }
        // 5..=6 : déplacer p1 directement
        5 | 6 if !w.rects.is_empty() => {
            let ri = rng.below(w.rects.len() as u64) as usize;
            let mut r = w.rects[ri].clone();
            let to = (
                3000.0 + 400.0 * ri as f64 + rng.val(60.0, 200.0),
                5000.0 + rng.val(-90.0, 90.0),
            );
            if near(to, r.pos[0]) < 1.0 {
                return;
            }
            r.pos[1] = to;
            c.move_p1_unlinks += r.link0 as u32;
            r.link0 = false;
            r.propagate();
            let mut rects = w.rects.clone();
            rects[ri] = r;
            let cmd = Command::MovePoint {
                sheet: s,
                point: w.rects[ri].ids[1],
                to: PointMm::new(to.0, to.1),
            };
            let f = w.f.clone();
            run(w, cmd, Some((rects, f)), c);
        }
        // 7 : déplacer p2 ou p3 directement
        7 if !w.rects.is_empty() => {
            let ri = rng.below(w.rects.len() as u64) as usize;
            let which = 2 + rng.below(2) as usize;
            let mut r = w.rects[ri].clone();
            let to = (
                3000.0 + 400.0 * ri as f64 + rng.val(-120.0, 250.0),
                5000.0 + rng.val(-150.0, 150.0),
            );
            if (0..4).any(|i| near(to, r.pos[i]) < 1.0) {
                return;
            }
            r.pos[which] = to;
            c.move_p23_unlinks += r.link2 as u32;
            r.link2 = false;
            let mut rects = w.rects.clone();
            rects[ri] = r;
            let cmd = Command::MovePoint {
                sheet: s,
                point: w.rects[ri].ids[which],
                to: PointMm::new(to.0, to.1),
            };
            let f = w.f.clone();
            run(w, cmd, Some((rects, f)), c);
        }
        // 8..=9 : lier / délier un côté
        8 | 9 if !w.rects.is_empty() => {
            let ri = rng.below(w.rects.len() as u64) as usize;
            let side = rng.below(2) as usize;
            let want = rng.below(2) == 0;
            let r = &w.rects[ri];
            let cur = if side == 0 { r.link0 } else { r.link2 };
            let cmd = Command::SetConstructionKept {
                sheet: s,
                segment: r.segs[side],
                kept: want,
            };
            let f = w.f.clone();
            if cur == want {
                run(w, cmd, Some((w.rects.clone(), f)), c);
                return;
            }
            if !want {
                let mut rects = w.rects.clone();
                if side == 0 {
                    rects[ri].link0 = false;
                } else {
                    rects[ri].link2 = false;
                }
                assert!(run(w, cmd, Some((rects, f)), c));
                c.unlink_ok += 1;
                return;
            }
            // Lier : accepté seulement si les positions écrites correspondent à la construction.
            let fits = if side == 0 {
                near(r.pos[1], r.expected_p1()) <= 1e-6
            } else {
                let (p2, p3) = r.expected_23();
                near(r.pos[2], p2) <= 1e-6 && near(r.pos[3], p3) <= 1e-6
            };
            if fits {
                let mut rects = w.rects.clone();
                if side == 0 {
                    rects[ri].link0 = true;
                } else {
                    rects[ri].link2 = true;
                }
                assert!(run(w, cmd, Some((rects, f)), c));
                c.link_ok += 1;
            } else {
                assert!(!run(w, cmd, None, c));
                c.link_refused += 1;
            }
        }
        // 10 : changer une variable
        10 | 11 => {
            let is_a = rng.below(2) == 0;
            let val = if is_a {
                rng.val(1.0, 400.0)
            } else {
                rng.val(1.0, 150.0)
            };
            let mut f = w.f.clone();
            if is_a {
                f.a = val;
            } else {
                f.b = val;
            }
            for i in 0..4 {
                if let Some(e) = f.bound[i] {
                    f.vals[i] = eval(e, f.a, f.b);
                }
            }
            let cmd = Command::SetVariableExpr {
                name: if is_a { "a" } else { "b" }.into(),
                expr: format!("{val}"),
            };
            if geometry_ok(&f.vals) {
                let rects = w.rects.clone();
                assert!(run(w, cmd, Some((rects, f)), c));
                c.var_ok += 1;
            } else {
                assert!(!run(w, cmd, None, c));
                c.var_refused += 1;
            }
        }
        // 12 : lier / délier une formule
        12 => {
            let i = rng.below(4) as usize;
            let target = ParamTarget {
                sheet: s,
                point: point_of(w, i),
                param: kinds()[i],
            };
            if rng.below(3) == 0 {
                let mut f = w.f.clone();
                c.unbind += f.bound[i].is_some() as u32;
                f.bound[i] = None;
                let cmd = Command::SetParameterExpr { target, expr: None };
                let rects = w.rects.clone();
                run(w, cmd, Some((rects, f)), c);
            } else {
                let e = rng.below(EXPRS.len() as u64) as usize;
                let mut f = w.f.clone();
                f.vals[i] = eval(e, f.a, f.b);
                f.bound[i] = Some(e);
                let cmd = Command::SetParameterExpr {
                    target,
                    expr: Some(EXPRS[e].into()),
                };
                if geometry_ok(&f.vals) {
                    let rects = w.rects.clone();
                    assert!(run(w, cmd, Some((rects, f)), c));
                    c.rebind_ok += 1;
                } else {
                    assert!(!run(w, cmd, None, c));
                    c.rebind_refused += 1;
                }
            }
        }
        // 13 : valeur écrite directement (délie ce qui change)
        _ => {
            let which = rng.below(3) as usize;
            let mut f = w.f.clone();
            let values = match which {
                0 => vec![rng.val(0.0, 1200.0)],
                1 => vec![rng.val(1.0, 200.0)],
                _ => vec![rng.val(1.0, 150.0), rng.val(1.0, 150.0)],
            };
            let first = [0usize, 1, 2][which];
            for (j, v) in values.iter().enumerate() {
                let i = first + j;
                if f.vals[i] != *v {
                    f.bound[i] = None;
                }
                f.vals[i] = *v;
            }
            let cmd = Command::SetPointValues {
                sheet: s,
                point: w.pts[which],
                values,
            };
            if geometry_ok(&f.vals) {
                let rects = w.rects.clone();
                assert!(run(w, cmd, Some((rects, f)), c));
                c.direct_ok += 1;
            } else {
                assert!(!run(w, cmd, None, c));
                c.direct_refused += 1;
            }
        }
    }
}

#[test]
fn campagne_adverse_restes_e03() {
    let mut c = Counters::default();
    let mut commands = 0u64;
    for seq in 0..SEQUENCES {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ (seq + 1).wrapping_mul(0xD1B5_4A32_D192_ED03));
        for _ in 0..4 {
            rng.step();
        }
        let mut w = build_world(&mut rng);
        check(&w, &mut c);
        for n in 0..STEPS {
            step(&mut w, &mut rng, &mut c);
            commands += 1;
            if n % 9 == 0 {
                // Relecture : valeur et octets identiques.
                let json = persist::to_json(&w.p).unwrap();
                let back = persist::from_json(&json).expect("relecture");
                assert_eq!(back, w.p);
                assert_eq!(persist::to_json(&back).unwrap(), json);
                c.reload_checks += 1;
            }
        }
    }
    eprintln!("{SEQUENCES} séquences, {commands} commandes : {c:?}");
    assert!(c.rect_added >= 400 && c.rect_added_kept >= 150, "{c:?}");
    assert!(c.move_corner_linked >= 300, "{c:?}");
    assert!(c.move_p1_unlinks >= 40 && c.move_p23_unlinks >= 40, "{c:?}");
    assert!(
        c.link_ok >= 30 && c.link_refused >= 30 && c.unlink_ok >= 30,
        "{c:?}"
    );
    assert!(c.var_ok >= 400 && c.var_refused >= 100, "{c:?}");
    assert!(c.rebind_ok >= 100 && c.rebind_refused >= 20, "{c:?}");
    assert!(c.direct_ok >= 100 && c.direct_refused >= 20, "{c:?}");
    assert!(c.unbind >= 20, "{c:?}");
    assert!(c.undo_checks >= 1500, "{c:?}");
}

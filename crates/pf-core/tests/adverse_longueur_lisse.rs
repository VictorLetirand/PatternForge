//! US-03.3b — campagne adverse « longueur imposée d'une courbe lisse et cible par formule » :
//! 160 séquences à graine fixe de 50 pas sur deux courbes lisses (3 à 5 points de passage, puis 2) et
//! une courbe de Bézier : imposer une valeur (extrémité qui glisse le long d'une direction
//! quelconque, ou poignée pour la Bézier), lier / délier une formule, changer les variables,
//! déplacer des points, supprimer une courbe, retirer, annuler / rétablir, relire le fichier.
//!
//! **Oracle indépendant** (aucun appel à `pf_core::length_constraint` ni à `pf_geometry`) :
//! - longueur d'une courbe lisse = Simpson sur la norme de la dérivée de chaque tronçon d'Hermite
//!   (tangentes de Catmull-Rom, corde aux extrémités, réécrites ici), d'une Bézier idem ;
//! - valeur d'une formule = petit évaluateur écrit ici pour cinq formules fixes, variables tenues à
//!   part dans le test ;
//! - prédiction de l'acceptation d'une valeur imposée à une courbe lisse : marche d'échantillonnage
//!   indépendante (mêmes 128 pas que le contrat documenté du solveur, `span` = 20 × (cible + corde +
//!   1)), avec marge de 0,5 mm pour ne rien affirmer sur les cas limites ; la racine retenue doit
//!   être dans l'intervalle de la **plus proche** racine franche.
//!
//! Invariants après chaque pas : refus => octets identiques ; accepté => pour chaque contrainte, la
//! longueur de l'oracle égale la cible écrite à 1e-6 mm et, avec formule, la valeur de l'oracle
//! égale la cible à 1e-9 ; aucun point hors des extrémités glissantes et du point déplacé n'a bougé ;
//! une extrémité imposée reste sur sa droite ; relecture identique ; annuler puis rétablir exacts à
//! l'octet. Compteurs de couverture minimaux. Mutations vérifiées : voir `docs/JOURNAL.md`.

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;

const SEQUENCES: u64 = 160;
const STEPS: usize = 50;
const MARGIN_MM: f64 = 0.5;

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
    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
    fn val(&mut self, lo: f64, hi: f64) -> f64 {
        let n = ((hi - lo) * 100.0) as u64;
        lo + self.below(n + 1) as f64 / 100.0
    }
    fn pick(&mut self, n: usize) -> usize {
        self.below(n as u64) as usize
    }
}

type Xy = (f64, f64);

// ---------------------------------------------------------------------------------------------
// Oracle
// ---------------------------------------------------------------------------------------------

/// Simpson raffiné (pas x4) jusqu'à ce que deux niveaux s'accordent à 2e-9 mm quand `n >= 6000` ;
/// `n` pas simples en dessous (prédictions grossières).
fn simpson_bezier(g: &[Xy; 4], n: usize) -> f64 {
    if n < 6000 {
        return simpson_once(g, n);
    }
    let mut n = n;
    let mut prev = simpson_once(g, n);
    loop {
        n *= 4;
        let cur = simpson_once(g, n);
        if (cur - prev).abs() <= 2e-9 || n >= 1_500_000 {
            return cur;
        }
        prev = cur;
    }
}

fn simpson_once(g: &[Xy; 4], n: usize) -> f64 {
    let speed = |t: f64| {
        let u = 1.0 - t;
        let dx = 3.0
            * (u * u * (g[1].0 - g[0].0)
                + 2.0 * u * t * (g[2].0 - g[1].0)
                + t * t * (g[3].0 - g[2].0));
        let dy = 3.0
            * (u * u * (g[1].1 - g[0].1)
                + 2.0 * u * t * (g[2].1 - g[1].1)
                + t * t * (g[3].1 - g[2].1));
        dx.hypot(dy)
    };
    let h = 1.0 / n as f64;
    let mut sum = speed(0.0) + speed(1.0);
    for i in 1..n {
        sum += speed(i as f64 * h) * if i % 2 == 1 { 4.0 } else { 2.0 };
    }
    sum * h / 3.0
}

/// Longueur d'une courbe lisse par points de passage ; `None` si deux points consécutifs sont
/// (presque) confondus (courbe invalide).
fn spline_len(pts: &[Xy], steps: usize) -> Option<f64> {
    let n = pts.len();
    if pts
        .windows(2)
        .any(|w| (w[0].0 - w[1].0).hypot(w[0].1 - w[1].1) < 1e-4)
    {
        return None;
    }
    let tangent = |i: usize| -> Xy {
        if i == 0 {
            (pts[1].0 - pts[0].0, pts[1].1 - pts[0].1)
        } else if i == n - 1 {
            (pts[n - 1].0 - pts[n - 2].0, pts[n - 1].1 - pts[n - 2].1)
        } else {
            (
                (pts[i + 1].0 - pts[i - 1].0) / 2.0,
                (pts[i + 1].1 - pts[i - 1].1) / 2.0,
            )
        }
    };
    let mut total = 0.0;
    for i in 0..n - 1 {
        let (m0, m1) = (tangent(i), tangent(i + 1));
        let (p0, p3) = (pts[i], pts[i + 1]);
        let c1 = (p0.0 + m0.0 / 3.0, p0.1 + m0.1 / 3.0);
        let c2 = (p3.0 - m1.0 / 3.0, p3.1 - m1.1 / 3.0);
        total += simpson_bezier(&[p0, c1, c2, p3], steps);
    }
    Some(total)
}

/// Longueur d'une courbe du projet lue **sans** le code de contrainte.
fn oracle_len(sheet: &Sheet, curve: EntityId, steps: usize) -> f64 {
    match &sheet.entity(curve).unwrap().kind {
        EntityKind::Spline { nodes } => {
            let pts: Vec<Xy> = nodes
                .iter()
                .map(|n| {
                    let q = sheet.position(n.point).unwrap();
                    (q.x, q.y)
                })
                .collect();
            spline_len(&pts, steps).expect("courbe lisse valide")
        }
        EntityKind::Curve {
            start,
            end,
            handle_start,
            handle_end,
        } => {
            let (a, b) = (
                sheet.position(*start).unwrap(),
                sheet.position(*end).unwrap(),
            );
            let g = [
                (a.x, a.y),
                (a.x + handle_start.dx, a.y + handle_start.dy),
                (b.x + handle_end.dx, b.y + handle_end.dy),
                (b.x, b.y),
            ];
            simpson_bezier(&g, steps)
        }
        _ => unreachable!(),
    }
}

/// Les cinq formules de la campagne et leur valeur pour (a, b).
const FORMULAS: [&str; 5] = ["a", "a+b", "a*2-b", "b*7", "(a+b)/2"];

fn formula_value(text: &str, a: f64, b: f64) -> f64 {
    match text {
        "a" => a,
        "a+b" => a + b,
        "a*2-b" => a * 2.0 - b,
        "b*7" => b * 7.0,
        "(a+b)/2" => (a + b) / 2.0,
        other => panic!("formule inconnue de l'oracle : {other}"),
    }
}

/// Résultat de la marche de prédiction sur un côté.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Side {
    /// Aucune traversée, tous les échantillons à plus de la marge de la cible.
    CleanNone,
    /// Première traversée franche, intervalle `[lo ; hi]` (abscisses signées).
    Robust(f64, f64),
    /// Traversée ou échantillon trop proche de la cible : on n'affirme rien.
    Ambiguous,
}

/// Marche d'échantillonnage de l'oracle sur la droite `origin + x·dir`, 128 pas jusqu'à `end`
/// (abscisse signée), fonction `f` = longueur de la courbe lisse avec l'extrémité en `x`.
fn walk(f: &dyn Fn(f64) -> Option<f64>, start_value: f64, end: f64, target: f64) -> Side {
    let (mut prev_x, mut prev_g) = (0.0_f64, start_value - target);
    let mut clean = prev_g.abs() > MARGIN_MM;
    for i in 1..=128usize {
        let x = end * i as f64 / 128.0;
        let Some(v) = f(x) else {
            return if clean {
                Side::CleanNone
            } else {
                Side::Ambiguous
            };
        };
        let g = v - target;
        if (g > 0.0) != (prev_g > 0.0) || g == 0.0 {
            return if clean && g.abs() > MARGIN_MM {
                Side::Robust(prev_x.min(x), prev_x.max(x))
            } else {
                Side::Ambiguous
            };
        }
        if g.abs() <= MARGIN_MM {
            clean = false;
        }
        prev_x = x;
        prev_g = g;
    }
    if clean {
        Side::CleanNone
    } else {
        Side::Ambiguous
    }
}

// ---------------------------------------------------------------------------------------------
// Monde
// ---------------------------------------------------------------------------------------------

#[derive(Default, Debug)]
struct Counters {
    steps: usize,
    accepted: usize,
    refused: usize,
    impose_smooth_accepted: usize,
    impose_smooth_predicted_accept: usize,
    impose_smooth_predicted_refuse: usize,
    nearest_checked: usize,
    unexpected_refusals: usize,
    formula_set: usize,
    formula_resolved_by_variable: usize,
    formula_refused_predicted: usize,
    unlinked: usize,
    released: usize,
    moved: usize,
    deleted_with_constraint: usize,
    undo_checked: usize,
    reload_checked: usize,
    bezier_impose: usize,
    last_error: String,
    unexpected_messages: Vec<String>,
}

struct World {
    doc: Document,
    sheet: SheetId,
    curves: Vec<EntityId>,
    points: Vec<PointId>,
    vars: (f64, f64),
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn build(rng: &mut Rng) -> World {
    let mut p = Project::new("Campagne");
    let s = p.sheets[0].id;
    for (name, v) in [("a", "100"), ("b", "20")] {
        p.apply(Command::AddVariable {
            name: name.into(),
            unit: VariableUnit::Mm,
            expr: v.into(),
        })
        .unwrap();
    }
    let mut curves = Vec::new();
    let mut points = Vec::new();
    let n = 3 + rng.pick(3);
    let pts: Vec<PointRef> = (0..n)
        .map(|i| {
            PointRef::New(PointMm::new(
                i as f64 * 60.0 + rng.val(-15.0, 15.0),
                rng.val(0.0, 120.0),
            ))
        })
        .collect();
    let o = p
        .apply(Command::AddSpline {
            sheet: s,
            points: pts,
        })
        .unwrap();
    curves.push(o.entities[0]);
    points.extend(o.points);
    let o = p
        .apply(Command::AddSpline {
            sheet: s,
            points: vec![
                PointRef::New(PointMm::new(rng.val(0.0, 50.0), 400.0 + rng.val(0.0, 40.0))),
                PointRef::New(PointMm::new(
                    150.0 + rng.val(0.0, 80.0),
                    400.0 + rng.val(0.0, 40.0),
                )),
            ],
        })
        .unwrap();
    curves.push(o.entities[0]);
    points.extend(o.points);
    let x0 = rng.val(0.0, 40.0);
    let x1 = 150.0 + rng.val(0.0, 60.0);
    let o = p
        .apply(Command::AddCurve {
            sheet: s,
            start: PointRef::New(PointMm::new(x0, 700.0)),
            c1: PointMm::new(x0 + rng.val(10.0, 80.0), 700.0 + rng.val(-50.0, 50.0)),
            c2: PointMm::new(x1 - rng.val(10.0, 80.0), 700.0 + rng.val(-50.0, 50.0)),
            end: PointRef::New(PointMm::new(x1, 700.0 + rng.val(-20.0, 20.0))),
        })
        .unwrap();
    curves.push(o.entities[0]);
    points.extend(o.points);
    World {
        doc: Document::new(p),
        sheet: s,
        curves,
        points,
        vars: (100.0, 20.0),
    }
}

fn sheet(w: &World) -> &Sheet {
    w.doc.project().sheet(w.sheet).unwrap()
}

fn all_positions(w: &World) -> Vec<(PointId, Xy)> {
    w.points
        .iter()
        .filter_map(|&id| sheet(w).position(id).ok().map(|q| (id, (q.x, q.y))))
        .collect()
}

/// Invariants vrais après **chaque** pas (accepté ou refusé).
fn check_invariants(w: &World, c: &mut Counters) {
    let proj = w.doc.project();
    let sh = sheet(w);
    proj.validate().expect("projet valide");
    for k in &sh.length_constraints {
        let len = oracle_len(sh, k.curve, 6000);
        assert!(
            (len - k.length_mm).abs() <= 1e-6 + 1e-9 * len,
            "longueur de l'oracle {len} ≠ cible écrite {} ({:?})",
            k.length_mm,
            k.adjust
        );
        if let Some(text) = &k.expr {
            let (a, b) = read_vars(proj);
            let v = formula_value(text, a, b);
            assert!(
                (v - k.length_mm).abs() <= 1e-9,
                "formule « {text} » = {v}, cible écrite {}",
                k.length_mm
            );
        }
    }
    // Relecture identique (une fois sur quatre : coûteux).
    if c.steps % 4 == 0 {
        let text = bytes(proj);
        let back = persist::from_json(&text).expect("relecture");
        assert_eq!(bytes(&back), text);
        assert_eq!(&back, proj);
        c.reload_checked += 1;
    }
}

enum Op {
    Impose {
        curve: EntityId,
        target: f64,
        adjust: LengthAdjust,
    },
    Expr {
        curve: EntityId,
        text: Option<&'static str>,
    },
    Release(EntityId),
    SetVar {
        name: &'static str,
        value: f64,
    },
    Move {
        point: PointId,
        to: Xy,
    },
    Delete(EntityId),
    Undo,
    Redo,
}

fn random_op(rng: &mut Rng, w: &World) -> Op {
    let constrained: Vec<EntityId> = sheet(w)
        .length_constraints
        .iter()
        .map(|k| k.curve)
        .collect();
    match rng.below(100) {
        0..=29 => {
            let curve = w.curves[rng.pick(w.curves.len())];
            let (first, last) = sheet(w).entity_points(curve).unwrap();
            let end = if rng.chance(50) { first } else { last };
            let bezier = matches!(
                sheet(w).entity(curve).unwrap().kind,
                EntityKind::Curve { .. }
            );
            let adjust = if bezier && rng.chance(30) {
                LengthAdjust::Handle { point: end }
            } else {
                let angle = match rng.below(5) {
                    0 => 0.0,
                    1 => 90.0,
                    2 => 45.0,
                    3 => 135.0,
                    _ => rng.val(0.0, 359.0),
                };
                LengthAdjust::Slide {
                    point: end,
                    angle_deg: angle,
                }
            };
            let len = oracle_len(sheet(w), curve, 2000);
            let target = len * rng.val(0.6, 1.9);
            Op::Impose {
                curve,
                target,
                adjust,
            }
        }
        30..=44 if !constrained.is_empty() => Op::Expr {
            curve: constrained[rng.pick(constrained.len())],
            text: Some(FORMULAS[rng.pick(FORMULAS.len())]),
        },
        45..=52 if !constrained.is_empty() => Op::Expr {
            curve: constrained[rng.pick(constrained.len())],
            text: None,
        },
        53..=58 if !constrained.is_empty() => Op::Release(constrained[rng.pick(constrained.len())]),
        59..=73 => {
            let values = [
                0.0, 5.0, 10.0, 40.0, 60.0, 80.0, 100.0, 130.0, 160.0, 200.0, 260.0,
            ];
            Op::SetVar {
                name: if rng.chance(50) { "a" } else { "b" },
                value: values[rng.pick(values.len())],
            }
        }
        74..=88 => Op::Move {
            point: w.points[rng.pick(w.points.len())],
            to: (rng.val(0.0, 300.0), rng.val(0.0, 800.0)),
        },
        89..=91 if w.curves.len() > 1 => Op::Delete(w.curves[rng.pick(w.curves.len())]),
        92..=95 => Op::Undo,
        96..=97 => Op::Redo,
        _ => Op::Release(w.curves[rng.pick(w.curves.len())]),
    }
}

fn run_op(w: &mut World, op: Op, rng: &mut Rng, c: &mut Counters) {
    let before = bytes(w.doc.project());
    let positions_before = all_positions(w);
    let s = w.sheet;
    c.steps += 1;
    match op {
        Op::Undo | Op::Redo => {
            let did = if matches!(op, Op::Undo) {
                w.doc.undo()
            } else {
                w.doc.redo()
            };
            if !did {
                assert_eq!(bytes(w.doc.project()), before);
            }
            // Les variables sont lues du projet restauré.
            w.vars = read_vars(w.doc.project());
            check_invariants(w, c);
        }
        Op::Impose {
            curve,
            target,
            adjust,
        } => {
            let LengthAdjust::Slide { point, angle_deg } = adjust else {
                c.bezier_impose += 1;
                let cmd = Command::SetCurveLengthConstraint {
                    sheet: s,
                    curve,
                    spec: Some(LengthSpec {
                        length_mm: target,
                        adjust,
                        reference: None,
                    }),
                };
                finish(w, cmd, &before, rng, c, &positions_before, &[]);
                return;
            };
            let smooth = matches!(
                sheet(w).entity(curve).unwrap().kind,
                EntityKind::Spline { .. }
            );
            // Prédiction (courbe lisse seulement) : l'oracle marche à partir de la position courante.
            let mut expected: Option<Option<(f64, f64)>> = None; // Some(None) = refus prédit
            let origin = sheet(w).position(point).unwrap();
            let (sin, cos) = angle_deg.to_radians().sin_cos();
            if smooth {
                let nodes: Vec<PointId> = match &sheet(w).entity(curve).unwrap().kind {
                    EntityKind::Spline { nodes } => nodes.iter().map(|n| n.point).collect(),
                    _ => unreachable!(),
                };
                let base: Vec<Xy> = nodes
                    .iter()
                    .map(|&id| {
                        let q = sheet(w).position(id).unwrap();
                        (q.x, q.y)
                    })
                    .collect();
                let idx = nodes.iter().position(|&n| n == point).unwrap();
                let f = |x: f64| -> Option<f64> {
                    let mut pts = base.clone();
                    pts[idx] = (origin.x + cos * x, origin.y + sin * x);
                    spline_len(&pts, 200)
                };
                let chord =
                    (base[0].0 - base[base.len() - 1].0).hypot(base[0].1 - base[base.len() - 1].1);
                let span = 20.0 * (target + chord + 1.0);
                let v0 = f(0.0).expect("courbe courante valide");
                let (fwd, back) = (walk(&f, v0, span, target), walk(&f, v0, -span, target));
                expected = match (fwd, back) {
                    (Side::CleanNone, Side::CleanNone) => Some(None),
                    (Side::Robust(a, b), Side::CleanNone)
                    | (Side::CleanNone, Side::Robust(a, b)) => Some(Some((a, b))),
                    (Side::Robust(a, b), Side::Robust(c2, d)) => {
                        let da = a.abs().min(b.abs());
                        let dc = c2.abs().min(d.abs());
                        Some(Some(if da <= dc { (a, b) } else { (c2, d) }))
                    }
                    _ => None,
                };
            }
            let cmd = Command::SetCurveLengthConstraint {
                sheet: s,
                curve,
                spec: Some(LengthSpec {
                    length_mm: target,
                    adjust,
                    reference: None,
                }),
            };
            let accepted = finish(w, cmd, &before, rng, c, &positions_before, &[point]);
            if smooth {
                match (&expected, accepted) {
                    (Some(None), true) => {
                        panic!("refus prédit, commande acceptée (cible {target})")
                    }
                    (Some(None), false) => c.impose_smooth_predicted_refuse += 1,
                    (Some(Some(_)), false) => {
                        c.unexpected_refusals += 1;
                        c.unexpected_messages.push(c.last_error.clone());
                    }
                    (Some(Some((lo, hi))), true) => {
                        c.impose_smooth_predicted_accept += 1;
                        let q = sheet(w).position(point).unwrap();
                        let along = (q.x - origin.x) * cos + (q.y - origin.y) * sin;
                        assert!(
                            along >= lo - 1e-5 && along <= hi + 1e-5,
                            "racine {along} hors de l'intervalle de la plus proche racine [{lo} ; {hi}]"
                        );
                        c.nearest_checked += 1;
                    }
                    _ => {}
                }
                if accepted {
                    c.impose_smooth_accepted += 1;
                }
            }
        }
        Op::Expr { curve, text } => {
            let cmd = Command::SetCurveLengthExpr {
                sheet: s,
                curve,
                expr: text.map(String::from),
            };
            let refusal_predicted = text.is_some_and(|t| {
                let v = formula_value(t, w.vars.0, w.vars.1);
                v <= 0.0
            });
            let had = sheet(w).length_constraint(curve).map(|k| k.expr.is_some());
            let slid: Vec<PointId> = sheet(w)
                .length_constraints
                .iter()
                .filter_map(|k| match k.adjust {
                    LengthAdjust::Slide { point, .. } => Some(point),
                    _ => None,
                })
                .collect();
            let ok = finish(w, cmd, &before, rng, c, &positions_before, &slid);
            if refusal_predicted {
                assert!(!ok, "formule de valeur <= 0 acceptée");
                c.formula_refused_predicted += 1;
            }
            if ok {
                match text {
                    Some(_) => c.formula_set += 1,
                    None => {
                        c.unlinked += 1;
                        let k = sheet(w)
                            .length_constraint(curve)
                            .expect("contrainte gardée");
                        assert!(k.expr.is_none());
                        let _ = had;
                    }
                }
            }
        }
        Op::Release(curve) => {
            let cmd = Command::SetCurveLengthConstraint {
                sheet: s,
                curve,
                spec: None,
            };
            let ok = finish(w, cmd, &before, rng, c, &positions_before, &[]);
            assert!(ok);
            assert!(sheet(w).length_constraint(curve).is_none());
            c.released += 1;
        }
        Op::SetVar { name, value } => {
            let new = if name == "a" {
                (value, w.vars.1)
            } else {
                (w.vars.0, value)
            };
            let any_formula = sheet(w).length_constraints.iter().any(|k| k.expr.is_some());
            let bad = sheet(w).length_constraints.iter().any(|k| {
                k.expr
                    .as_deref()
                    .is_some_and(|t| formula_value(t, new.0, new.1) <= 0.0)
            });
            let cmd = Command::SetVariableExpr {
                name: name.into(),
                expr: format!("{value}"),
            };
            // Points susceptibles de bouger : toutes les extrémités glissantes.
            let slid: Vec<PointId> = sheet(w)
                .length_constraints
                .iter()
                .filter_map(|k| match k.adjust {
                    LengthAdjust::Slide { point, .. } => Some(point),
                    _ => None,
                })
                .collect();
            let ok = finish(w, cmd, &before, rng, c, &positions_before, &slid);
            if bad {
                assert!(!ok, "variable rendant une cible <= 0 acceptée");
                c.formula_refused_predicted += 1;
            }
            if ok {
                w.vars = new;
                if any_formula {
                    c.formula_resolved_by_variable += 1;
                }
                assert_eq!(read_vars(w.doc.project()), w.vars);
            }
        }
        Op::Move { point, to } => {
            let slid: Vec<PointId> = sheet(w)
                .length_constraints
                .iter()
                .filter_map(|k| match k.adjust {
                    LengthAdjust::Slide { point, .. } => Some(point),
                    _ => None,
                })
                .collect();
            let mut moved = slid;
            moved.push(point);
            let cmd = Command::MovePoint {
                sheet: s,
                point,
                to: PointMm::new(to.0, to.1),
            };
            if finish(w, cmd, &before, rng, c, &positions_before, &moved) {
                c.moved += 1;
            }
        }
        Op::Delete(curve) => {
            let had = sheet(w).length_constraint(curve).is_some();
            let cmd = Command::Delete {
                sheet: s,
                points: vec![],
                entities: vec![curve],
            };
            let n_before = sheet(w).length_constraints.len();
            if finish(w, cmd, &before, rng, c, &positions_before, &[]) {
                w.curves.retain(|&e| e != curve);
                assert_eq!(
                    sheet(w).length_constraints.len(),
                    n_before - usize::from(had),
                    "contrainte de la courbe supprimée retirée, les autres gardées"
                );
                if had {
                    c.deleted_with_constraint += 1;
                }
            }
        }
    }
}

fn read_vars(p: &Project) -> (f64, f64) {
    let get = |n: &str| -> f64 {
        let v = p.variables.iter().find(|v| v.name == n).unwrap();
        v.expr.parse::<f64>().unwrap()
    };
    (get("a"), get("b"))
}

/// Exécute une commande et vérifie les invariants ; renvoie `true` si acceptée.
fn finish(
    w: &mut World,
    cmd: Command,
    before: &str,
    rng: &mut Rng,
    c: &mut Counters,
    positions_before: &[(PointId, Xy)],
    may_move: &[PointId],
) -> bool {
    let result = w.doc.execute(cmd);
    match result {
        Err(e) => {
            assert_eq!(
                bytes(w.doc.project()),
                before,
                "trace laissée par un refus : {e}"
            );
            c.refused += 1;
            c.last_error = e.to_string();
            check_invariants(w, c);
            false
        }
        Ok(_) => {
            c.accepted += 1;
            let after = bytes(w.doc.project());
            // Aucun point en dehors de ceux qui peuvent bouger n'a changé.
            for (id, old) in positions_before {
                if may_move.contains(id) {
                    continue;
                }
                let new = sheet(w).position(*id).map(|q| (q.x, q.y));
                if let Ok(new) = new {
                    assert!(
                        (new.0 - old.0).abs() <= 1e-9 && (new.1 - old.1).abs() <= 1e-9,
                        "point {id:?} déplacé sans raison : {old:?} -> {new:?}"
                    );
                }
            }
            // Annuler puis rétablir : exacts à l'octet (une fois sur trois).
            if after != before && rng.chance(33) {
                assert!(w.doc.undo(), "une étape à annuler");
                assert_eq!(bytes(w.doc.project()), before, "annulation inexacte");
                assert!(w.doc.redo());
                assert_eq!(bytes(w.doc.project()), after, "rétablissement inexact");
                c.undo_checked += 1;
            }
            check_invariants(w, c);
            true
        }
    }
}

#[test]
fn campagne_adverse_longueur_courbe_lisse_et_formule() {
    let mut c = Counters::default();
    for seq in 0..SEQUENCES {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ (seq + 1).wrapping_mul(0xD6E8_FEB8_6659_FD93));
        let mut w = build(&mut rng);
        for _ in 0..STEPS {
            let op = random_op(&mut rng, &w);
            run_op(&mut w, op, &mut rng, &mut c);
        }
    }
    eprintln!("{c:#?}");
    assert!(c.steps >= 150 * 40, "{c:?}");
    assert!(c.accepted > 1500 && c.refused > 500, "{c:?}");
    assert!(c.impose_smooth_accepted > 300, "{c:?}");
    assert!(c.impose_smooth_predicted_accept > 150, "{c:?}");
    assert!(c.impose_smooth_predicted_refuse > 50, "{c:?}");
    assert!(c.nearest_checked > 150, "{c:?}");
    assert!(c.formula_set > 150, "{c:?}");
    assert!(c.formula_resolved_by_variable > 80, "{c:?}");
    assert!(c.formula_refused_predicted > 20, "{c:?}");
    assert!(c.unlinked > 40 && c.released > 40 && c.moved > 300, "{c:?}");
    assert!(c.deleted_with_constraint > 10, "{c:?}");
    assert!(c.undo_checked > 300 && c.reload_checked > 1000, "{c:?}");
    assert!(c.bezier_impose > 100, "{c:?}");
    // Aucun refus à tort : une racine franche prédite par l'oracle est toujours trouvée (un extremum
    // dans un pas d'échantillonnage ne refuse rien : voir `solve_nearest`).
    assert_eq!(c.unexpected_refusals, 0, "{:?}", c.unexpected_messages);
}

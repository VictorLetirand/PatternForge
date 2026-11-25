//! Plans de coupe, métrage (US-15.3) — campagne adverse : 180 séquences à graine fixe. Chaque
//! séquence pose des pièces (rectangle, carré, triangle, L) sur une grille de cases de 105 mm,
//! avec rotations de 90°, miroirs et tailles au hasard, injecte des défauts (chevauchement,
//! début de l'étoffe, bord, pli), les répare, supprime tout, annule et rétablit.
//!
//! **Oracle indépendant** : il connaît les sommets locaux de chaque pièce, calcule l'aire par sa
//! propre formule du lacet, pose ses contours avec `cos` / `sin`, décide seul de la validité
//! (bandes de la zone utile, pli, début, recoupement des boîtes : les cases sont disjointes, donc
//! deux boîtes qui se recoupent sont deux poses de la même case, dont les coins coïncident), de
//! la longueur (plus grand `y`), de la surface, du rendement et des lignes par taille (taille de
//! base d'abord, puis l'ordre de la gamme). Comparé au cœur après **chaque** pas ; un plan
//! invalide doit ne porter **aucun** chiffre. Des compteurs prouvent l'exercice de chaque
//! famille. Mutations : voir `docs/JOURNAL.md` (tranche 126).

use pf_core::cutting_plan::{CuttingPlanSpec, FabricFold};
use pf_core::cutting_yield::YieldStatus;
use pf_core::*;
use pf_geometry::PointMm;

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
    fn pick<T: Clone>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len() as u64) as usize].clone()
    }
}

type P = (f64, f64);

fn shapes() -> Vec<Vec<P>> {
    vec![
        vec![(0.0, 0.0), (100.0, 0.0), (100.0, 100.0), (0.0, 100.0)],
        vec![(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)],
        vec![(0.0, 0.0), (80.0, 0.0), (0.0, 40.0)],
        vec![
            (0.0, 0.0),
            (80.0, 0.0),
            (80.0, 30.0),
            (30.0, 30.0),
            (30.0, 70.0),
            (0.0, 70.0),
        ],
    ]
}

fn lace(v: &[P]) -> f64 {
    let n = v.len();
    (0..n)
        .map(|i| v[i].0 * v[(i + 1) % n].1 - v[(i + 1) % n].0 * v[i].1)
        .sum::<f64>()
        .abs()
        / 2.0
}

#[derive(Clone, Copy)]
struct OPl {
    shape: usize,
    size: Option<usize>,
    x: f64,
    y: f64,
    rot: f64,
    mir: bool,
}

/// Les rotations sont des multiples de 90° : sinus et cosinus arrondis à 0 ou ±1.
fn o_poly(shapes: &[Vec<P>], pl: &OPl) -> Vec<P> {
    let (s, c) = pl.rot.to_radians().sin_cos();
    let (s, c) = (s.round(), c.round());
    shapes[pl.shape]
        .iter()
        .map(|&(x, y)| {
            let x = if pl.mir { -x } else { x };
            (c * x - s * y + pl.x, s * x + c * y + pl.y)
        })
        .collect()
}

fn bbox(v: &[P]) -> (f64, f64, f64, f64) {
    let mut b = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for &(x, y) in v {
        b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
    }
    b
}

#[derive(Clone)]
struct OPlan {
    w: f64,
    sel: f64,
    fold: bool,
}

fn o_valid(shapes: &[Vec<P>], plan: &OPlan, state: &[OPl]) -> bool {
    let bands: Vec<(f64, f64)> = if plan.fold {
        vec![(plan.sel, plan.w / 2.0), (plan.w / 2.0, plan.w - plan.sel)]
    } else {
        vec![(plan.sel, plan.w - plan.sel)]
    };
    let boxes: Vec<_> = state.iter().map(|o| bbox(&o_poly(shapes, o))).collect();
    for b in &boxes {
        if !bands
            .iter()
            .any(|&(lo, hi)| b.0 >= lo - 1e-6 && b.2 <= hi + 1e-6)
            || b.1 < -1e-6
        {
            return false;
        }
    }
    for i in 0..state.len() {
        for j in (i + 1)..state.len() {
            let (a, b) = (boxes[i], boxes[j]);
            if a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3 {
                return false;
            }
        }
    }
    true
}

#[derive(Default)]
struct Cov {
    valid: u32,
    invalid_overlap: u32,
    invalid_before: u32,
    invalid_edge: u32,
    invalid_fold: u32,
    empty: u32,
    multi_size: u32,
    undo_redo: u32,
    repaired: u32,
    folded_valid: u32,
}

fn build() -> (Project, Vec<PieceId>, Vec<SizeId>) {
    let defs = shapes();
    let mut p = Project::new("Adverse metrage");
    let s = p.sheets[0].id;
    let mut ids = Vec::new();
    for (k, d) in defs.iter().enumerate() {
        let ox = 400.0 * (k as f64 + 1.0);
        let poly = p
            .apply(Command::AddPolygon {
                sheet: s,
                vertices: d
                    .iter()
                    .map(|v| PointRef::New(PointMm::new(ox + v.0, 30.0 + v.1)))
                    .collect(),
                closed: true,
                role: PathRole::default(),
                layer: None,
            })
            .unwrap();
        let id = p
            .apply(Command::ExtractPiece {
                sheet: s,
                source: PieceSource::Paths {
                    entities: poly.entities,
                },
                name: format!("F{k}"),
                detached: false,
            })
            .unwrap()
            .pieces[0];
        ids.push(id);
    }
    p.apply(Command::AddSizeRange {
        name: "G".into(),
        kind: SizeKind::Alphanumeric,
        labels: vec!["S".into(), "M".into(), "L".into()],
        base: 1,
    })
    .unwrap();
    let sizes = p.sizes[0].sizes.iter().map(|z| z.id).collect();
    (p, ids, sizes)
}

type Rows = Vec<(Option<usize>, usize)>;

/// `None` = vide ; `Some(None)` = invalide ; `Some(Some(..))` = (longueur, surface, rendement, lignes).
type Expect = Option<Option<(f64, f64, f64, Rows)>>;

fn expected(shapes: &[Vec<P>], plan: &OPlan, state: &[OPl]) -> Expect {
    if state.is_empty() {
        return None;
    }
    if !o_valid(shapes, plan, state) {
        return Some(None);
    }
    let length = state
        .iter()
        .map(|o| bbox(&o_poly(shapes, o)).3)
        .fold(0.0_f64, f64::max);
    let area: f64 = state.iter().map(|o| lace(&shapes[o.shape])).sum();
    let useful = plan.w - 2.0 * plan.sel;
    let mut rows: Rows = Vec::new();
    for o in state {
        match rows.iter_mut().find(|r| r.0 == o.size) {
            Some(r) => r.1 += 1,
            None => rows.push((o.size, 1)),
        }
    }
    rows.sort_by_key(|r| r.0.map_or(0, |i| i + 1));
    Some(Some((length, area, 100.0 * area / (useful * length), rows)))
}

struct Ctx<'a> {
    plan_id: CuttingPlanId,
    sizes: &'a [SizeId],
    shapes: &'a [Vec<P>],
    plan: &'a OPlan,
}

fn check(p: &Project, c: &Ctx, state: &[OPl], cov: &mut Cov, what: &str) {
    let y = p.cutting_plan_yield(c.plan_id).unwrap();
    match expected(c.shapes, c.plan, state) {
        None => {
            assert_eq!(y.status, YieldStatus::Empty, "{what}");
            assert!(y.figures.is_none(), "{what}");
            cov.empty += 1;
        }
        Some(None) => {
            assert_eq!(y.status, YieldStatus::Invalid, "{what}");
            assert!(
                y.figures.is_none(),
                "{what} : un plan invalide porte un chiffre"
            );
        }
        Some(Some((length, area, percent, rows))) => {
            assert_eq!(y.status, YieldStatus::Valid, "{what}");
            let g = y.figures.expect("chiffres d'un plan valide");
            assert!((g.length_mm - length).abs() < 1e-6, "{what} longueur");
            assert!((g.used_area_mm2 - area).abs() < 1e-3, "{what} surface");
            assert!(
                (g.useful_width_mm - (c.plan.w - 2.0 * c.plan.sel)).abs() < 1e-9,
                "{what}"
            );
            assert!((g.yield_percent - percent).abs() < 1e-6, "{what} rendement");
            let got: Rows = g
                .by_size
                .iter()
                .map(|r| {
                    (
                        r.size
                            .map(|id| c.sizes.iter().position(|s| *s == id).unwrap()),
                        r.pieces,
                    )
                })
                .collect();
            assert_eq!(got, rows, "{what} lignes par taille");
            let sum: f64 = g.by_size.iter().map(|r| r.share_percent).sum();
            assert!(
                (sum - g.yield_percent).abs() < 1e-9,
                "{what} somme des parts"
            );
            cov.valid += 1;
            cov.multi_size += (rows.len() > 1) as u32;
            cov.folded_valid += c.plan.fold as u32;
        }
    }
}

type Hist = Vec<(Project, Vec<(PlacementId, OPl)>)>;

fn add(
    doc: &mut Document,
    plan_id: CuttingPlanId,
    ids: &[PieceId],
    sizes: &[SizeId],
    o: &OPl,
) -> PlacementId {
    doc.execute(Command::AddPlacement {
        plan: plan_id,
        piece: ids[o.shape],
        size: o.size.map(|i| sizes[i]),
        x_mm: o.x,
        y_mm: o.y,
        rotation_deg: o.rot,
        mirrored: o.mir,
    })
    .unwrap()
    .placements[0]
}

#[test]
fn campagne_adverse_metrage() {
    let defs = shapes();
    let mut cov = Cov::default();
    let mut r = Rng(0x00C0_FFEE_2026_1126);
    for seq in 0..180u32 {
        let (p0, ids, sizes) = build();
        let plan = OPlan {
            w: r.pick(&[640.0, 840.0, 1000.0]),
            sel: r.pick(&[0.0, 10.0, 20.0]),
            fold: r.chance(35),
        };
        let mut doc = Document::new(p0);
        let plan_id = doc
            .execute(Command::AddCuttingPlan {
                spec: CuttingPlanSpec {
                    name: format!("M{seq}"),
                    material: None,
                    fabric_width_mm: plan.w,
                    fold: if plan.fold {
                        FabricFold::Folded
                    } else {
                        FabricFold::Flat
                    },
                    selvedge_mm: plan.sel,
                    min_gap_mm: 5.0,
                    grain_direction: None,
                },
            })
            .unwrap()
            .cutting_plans[0];
        let ctx = Ctx {
            plan_id,
            sizes: &sizes,
            shapes: &defs,
            plan: &plan,
        };
        let per_row = (((plan.w - 2.0 * plan.sel) / 105.0).floor() as u64).max(1);
        let mut state: Vec<(PlacementId, OPl)> = Vec::new();
        let mut hist: Hist = vec![(doc.project().clone(), vec![])];
        let mut redo: Hist = Vec::new();
        let mut next_cell = 0u64;
        let plain = |s: &[(PlacementId, OPl)]| s.iter().map(|x| x.1).collect::<Vec<_>>();
        for step in 0..(8 + r.below(8)) {
            let what = format!("séquence {seq} pas {step}");
            match r.below(10) {
                0..=4 => {
                    let shape = r.below(defs.len() as u64) as usize;
                    let rot = r.pick(&[0.0, 90.0, 180.0, 270.0]);
                    let mir = r.chance(30);
                    let size = r.pick(&[None, None, Some(0usize), Some(1), Some(2)]);
                    let (cx, cy) = (next_cell % per_row, next_cell / per_row);
                    next_cell += 1;
                    let probe = OPl {
                        shape,
                        size,
                        x: 0.0,
                        y: 0.0,
                        rot,
                        mir,
                    };
                    let b0 = bbox(&o_poly(&defs, &probe));
                    let (tx, ty) = (plan.sel + 105.0 * cx as f64, 105.0 * cy as f64);
                    let o = OPl {
                        x: tx - b0.0,
                        y: ty - b0.1,
                        ..probe
                    };
                    let id = add(&mut doc, plan_id, &ids, &sizes, &o);
                    state.push((id, o));
                    hist.push((doc.project().clone(), state.clone()));
                    redo.clear();
                }
                5 if !state.is_empty() => {
                    // Défaut injecté puis réparé : le chiffre disparaît, puis revient.
                    let k = r.below(state.len() as u64) as usize;
                    let mut o = state[k].1;
                    match r.below(3) {
                        0 => cov.invalid_overlap += 1,
                        1 => {
                            o.y = -3.0;
                            cov.invalid_before += 1;
                        }
                        _ => {
                            o.x = plan.w - 20.0;
                            cov.invalid_edge += 1;
                        }
                    }
                    let id = add(&mut doc, plan_id, &ids, &sizes, &o);
                    state.push((id, o));
                    check(
                        doc.project(),
                        &ctx,
                        &plain(&state),
                        &mut cov,
                        &format!("{what} défaut"),
                    );
                    doc.execute(Command::RemovePlacement {
                        plan: plan_id,
                        placement: id,
                    })
                    .unwrap();
                    state.pop();
                    hist.push((doc.project().clone(), state.clone()));
                    redo.clear();
                    cov.repaired += 1;
                }
                6 if !state.is_empty() => {
                    let k = r.below(state.len() as u64) as usize;
                    doc.execute(Command::RemovePlacement {
                        plan: plan_id,
                        placement: state[k].0,
                    })
                    .unwrap();
                    state.remove(k);
                    hist.push((doc.project().clone(), state.clone()));
                    redo.clear();
                }
                7 if r.chance(40) => {
                    for (id, _) in std::mem::take(&mut state) {
                        doc.execute(Command::RemovePlacement {
                            plan: plan_id,
                            placement: id,
                        })
                        .unwrap();
                    }
                    hist.push((doc.project().clone(), state.clone()));
                    redo.clear();
                }
                8 | 9 => {
                    if r.chance(50) && hist.len() > 1 {
                        redo.push(hist.pop().unwrap());
                        let (pj, st) = hist.last().unwrap();
                        // Une étape mémorisée peut regrouper plusieurs étapes du document.
                        while doc.project() != pj && doc.undo() {}
                        assert_eq!(doc.project(), pj, "{what} annulation");
                        state = st.clone();
                        cov.undo_redo += 1;
                    } else if let Some(next) = redo.pop() {
                        while doc.project() != &next.0 && doc.redo() {}
                        assert_eq!(*doc.project(), next.0, "{what} rétablissement");
                        state = next.1.clone();
                        hist.push(next);
                        cov.undo_redo += 1;
                    }
                }
                _ => {}
            }
            let pl = plain(&state);
            check(doc.project(), &ctx, &pl, &mut cov, &what);
            if plan.fold && !pl.is_empty() && !o_valid(&defs, &plan, &pl) {
                cov.invalid_fold += 1;
            }
        }
    }
    assert!(cov.valid >= 300, "valides {}", cov.valid);
    assert!(
        cov.invalid_overlap >= 20,
        "chevauchements {}",
        cov.invalid_overlap
    );
    assert!(cov.invalid_before >= 20, "début {}", cov.invalid_before);
    assert!(cov.invalid_edge >= 20, "bord {}", cov.invalid_edge);
    assert!(cov.invalid_fold >= 10, "pli {}", cov.invalid_fold);
    assert!(cov.empty >= 30, "vides {}", cov.empty);
    assert!(cov.multi_size >= 60, "plusieurs tailles {}", cov.multi_size);
    assert!(cov.undo_redo >= 60, "annulations {}", cov.undo_redo);
    assert!(cov.repaired >= 40, "réparés {}", cov.repaired);
    assert!(cov.folded_valid >= 40, "pliés valides {}", cov.folded_valid);
}

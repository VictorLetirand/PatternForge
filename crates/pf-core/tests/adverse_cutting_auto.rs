//! Plans de coupe, proposition de placement automatique (US-15.4) : campagne adverse de 180 jeux
//! de pièces aléatoires à graine fixe (rectangle à droit-fil, carré en paire, triangle, L concave,
//! disque, bande large), plans plats ou pliés, laizes serrées, écart et lisières variables,
//! rotations autorisées ou non, droit-fil du plan ou non.
//!
//! **Oracle indépendant du cœur** : il connaît les formes locales, pose ses propres contours avec
//! une table de rotations à 4 valeurs (jamais `place_point`), cherche les chevauchements par
//! **échantillonnage** de points (point dans un polygone par lancer de rayon, point dans un
//! disque), mesure les écarts par échantillonnage dense des bords, décide seul des débordements
//! (zone de pose, pli, début), des rotations permises (droit-fil), des pièces non posables et des
//! quantités, et recalcule la longueur et le rendement. Propriétés : jamais de chevauchement ni de
//! débordement, quantité exacte, niveau 2 jamais pire que le niveau 1, déterminisme (deux
//! exécutions identiques), temps borné, aucun panic. Des compteurs prouvent la couverture.
//! Mutations : voir `docs/JOURNAL.md` (tranche 127).

use pf_core::cutting_auto::{AutoError, AutoItem, AutoOptions, UnplaceableReason};
use pf_core::cutting_plan::{CuttingPlanSpec, FabricFold};
use pf_core::*;
use pf_geometry::PointMm;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

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

#[derive(Clone)]
enum Sh {
    Poly(Vec<P>),
    Disk(f64),
}

struct Def {
    shape: Sh,
    /// Largeur et hauteur de la boîte locale.
    wh: P,
    vertical_grain: bool,
    pair: bool,
}

fn defs() -> Vec<Def> {
    vec![
        Def {
            shape: Sh::Poly(vec![(0.0, 0.0), (100.0, 0.0), (100.0, 50.0), (0.0, 50.0)]),
            wh: (100.0, 50.0),
            vertical_grain: true,
            pair: false,
        },
        Def {
            shape: Sh::Poly(vec![(0.0, 0.0), (60.0, 0.0), (60.0, 60.0), (0.0, 60.0)]),
            wh: (60.0, 60.0),
            vertical_grain: false,
            pair: true,
        },
        Def {
            shape: Sh::Poly(vec![(0.0, 0.0), (80.0, 0.0), (0.0, 40.0)]),
            wh: (80.0, 40.0),
            vertical_grain: false,
            pair: false,
        },
        Def {
            shape: Sh::Poly(vec![
                (0.0, 0.0),
                (80.0, 0.0),
                (80.0, 30.0),
                (30.0, 30.0),
                (30.0, 70.0),
                (0.0, 70.0),
            ]),
            wh: (80.0, 70.0),
            vertical_grain: false,
            pair: false,
        },
        Def {
            shape: Sh::Disk(40.0),
            wh: (80.0, 80.0),
            vertical_grain: false,
            pair: false,
        },
        Def {
            shape: Sh::Poly(vec![(0.0, 0.0), (300.0, 0.0), (300.0, 30.0), (0.0, 30.0)]),
            wh: (300.0, 30.0),
            vertical_grain: false,
            pair: false,
        },
        Def {
            shape: Sh::Poly(vec![(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)]),
            wh: (100.0, 100.0),
            vertical_grain: false,
            pair: false,
        },
        Def {
            shape: Sh::Poly(vec![(0.0, 0.0), (50.0, 0.0), (50.0, 50.0), (0.0, 50.0)]),
            wh: (50.0, 50.0),
            vertical_grain: false,
            pair: false,
        },
        Def {
            shape: Sh::Poly(vec![
                (0.0, 0.0),
                (90.0, 0.0),
                (90.0, 20.0),
                (45.0, 60.0),
                (0.0, 20.0),
            ]),
            wh: (90.0, 60.0),
            vertical_grain: false,
            pair: false,
        },
    ]
}

fn build(defs: &[Def]) -> (Project, Vec<PieceId>) {
    let mut p = Project::new("Adverse auto");
    let s = p.sheets[0].id;
    let mut ids = Vec::new();
    for (k, d) in defs.iter().enumerate() {
        let (ox, oy) = (400.0 * (k as f64 + 1.0), 40.0 + 13.0 * k as f64);
        let e = match &d.shape {
            Sh::Poly(v) => p
                .apply(Command::AddPolygon {
                    sheet: s,
                    vertices: v
                        .iter()
                        .map(|q| PointRef::New(PointMm::new(ox + q.0, oy + q.1)))
                        .collect(),
                    closed: true,
                    role: PathRole::default(),
                    layer: None,
                })
                .unwrap(),
            Sh::Disk(r) => p
                .apply(Command::AddCircle {
                    sheet: s,
                    center: PointRef::New(PointMm::new(ox + r, oy + r)),
                    radius_mm: *r,
                })
                .unwrap(),
        };
        let id = p
            .apply(Command::ExtractPiece {
                sheet: s,
                source: PieceSource::Paths {
                    entities: e.entities,
                },
                name: format!("P{k}"),
                detached: false,
            })
            .unwrap()
            .pieces[0];
        p.apply(Command::SetPieceInfo {
            piece: id,
            name: format!("P{k}"),
            quantity: 1,
            material: None,
            mirrored_pair: d.pair,
        })
        .unwrap();
        if d.vertical_grain {
            let g = p
                .apply(Command::AddSegment {
                    sheet: s,
                    from: PointRef::New(PointMm::new(ox + 50.0, oy + 10.0)),
                    to: PointRef::New(PointMm::new(ox + 50.0, oy + 40.0)),
                })
                .unwrap();
            p.apply(Command::SetPathRole {
                sheet: s,
                entity: g.entities[0],
                role: PathRole::Grainline,
            })
            .unwrap();
            let outline = p.piece(id).unwrap().outline.clone();
            p.apply(Command::SetPieceOutline {
                piece: id,
                outline,
                grainline: Some(g.entities[0]),
            })
            .unwrap();
        }
        ids.push(id);
    }
    (p, ids)
}

// ---------------------------------------------------------------------------
// Oracle
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum Posed {
    Poly(Vec<P>),
    Disk(P, f64),
}

fn cs(rot: f64) -> (f64, f64) {
    match rot as i64 {
        0 => (1.0, 0.0),
        90 => (0.0, 1.0),
        180 => (-1.0, 0.0),
        270 => (0.0, -1.0),
        r => panic!("rotation inattendue {r}"),
    }
}

fn pose(sh: &Sh, x: f64, y: f64, rot: f64, mirrored: bool) -> Posed {
    let (c, s) = cs(rot);
    let t = |q: P| {
        let q = if mirrored { (-q.0, q.1) } else { q };
        (c * q.0 - s * q.1 + x, s * q.0 + c * q.1 + y)
    };
    match sh {
        Sh::Poly(v) => Posed::Poly(v.iter().map(|q| t(*q)).collect()),
        Sh::Disk(r) => Posed::Disk(t((*r, *r)), *r),
    }
}

fn bbox(p: &Posed) -> (f64, f64, f64, f64) {
    match p {
        Posed::Poly(v) => v
            .iter()
            .fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |b, q| {
                (b.0.min(q.0), b.1.min(q.1), b.2.max(q.0), b.3.max(q.1))
            }),
        Posed::Disk(c, r) => (c.0 - r, c.1 - r, c.0 + r, c.1 + r),
    }
}

fn area(sh: &Sh) -> f64 {
    match sh {
        Sh::Poly(v) => {
            let n = v.len();
            (0..n)
                .map(|i| v[i].0 * v[(i + 1) % n].1 - v[(i + 1) % n].0 * v[i].1)
                .sum::<f64>()
                .abs()
                / 2.0
        }
        Sh::Disk(r) => std::f64::consts::PI * r * r,
    }
}

fn inside(p: &Posed, q: P) -> bool {
    match p {
        Posed::Disk(c, r) => ((q.0 - c.0).powi(2) + (q.1 - c.1).powi(2)).sqrt() < r - 0.1,
        Posed::Poly(v) => {
            let mut inside = false;
            let n = v.len();
            for i in 0..n {
                let (a, b) = (v[i], v[(i + 1) % n]);
                if (a.1 > q.1) != (b.1 > q.1) && q.0 < a.0 + (q.1 - a.1) / (b.1 - a.1) * (b.0 - a.0)
                {
                    inside = !inside;
                }
            }
            inside
        }
    }
}

fn seg_dist(p: P, a: P, b: P) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0)
    };
    ((p.0 - a.0 - t * dx).powi(2) + (p.1 - a.1 - t * dy).powi(2)).sqrt()
}

fn dist_to(p: P, s: &Posed) -> f64 {
    match s {
        Posed::Disk(c, r) => (((p.0 - c.0).powi(2) + (p.1 - c.1).powi(2)).sqrt() - r).max(0.0),
        Posed::Poly(v) => {
            let n = v.len();
            (0..n)
                .map(|i| seg_dist(p, v[i], v[(i + 1) % n]))
                .fold(f64::MAX, f64::min)
        }
    }
}

fn boundary(s: &Posed) -> Vec<P> {
    match s {
        Posed::Disk(c, r) => (0..720)
            .map(|i| {
                let a = f64::from(i) * std::f64::consts::PI / 360.0;
                (c.0 + r * a.cos(), c.1 + r * a.sin())
            })
            .collect(),
        Posed::Poly(v) => {
            let n = v.len();
            let mut out = Vec::new();
            for i in 0..n {
                let (a, b) = (v[i], v[(i + 1) % n]);
                let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
                let k = (len / 1.0).ceil().max(1.0) as usize;
                for j in 0..k {
                    let t = j as f64 / k as f64;
                    out.push((a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1)));
                }
            }
            out
        }
    }
}

fn distance(a: &Posed, b: &Posed) -> f64 {
    let m = boundary(a)
        .into_iter()
        .map(|p| dist_to(p, b))
        .fold(f64::MAX, f64::min);
    boundary(b)
        .into_iter()
        .map(|p| dist_to(p, a))
        .fold(m, f64::min)
}

fn sampled_overlap(a: &Posed, b: &Posed) -> usize {
    let (ba, bb) = (bbox(a), bbox(b));
    let (x0, y0, x1, y1) = (
        ba.0.max(bb.0),
        ba.1.max(bb.1),
        ba.2.min(bb.2),
        ba.3.min(bb.3),
    );
    if x0 >= x1 || y0 >= y1 {
        return 0;
    }
    let mut hits = 0;
    let mut x = x0 + 0.37;
    while x < x1 {
        let mut y = y0 + 0.41;
        while y < y1 {
            if inside(a, (x, y)) && inside(b, (x, y)) {
                hits += 1;
            }
            y += 2.0;
        }
        x += 2.0;
    }
    hits
}

#[derive(Default)]
struct Cov {
    ok: usize,
    refused: usize,
    refined: usize,
    folded_ok: usize,
    grain_ok: usize,
    mirrored: usize,
    rotated: usize,
    gaps: usize,
    selvedge: usize,
    disks: usize,
    time_limited: usize,
    lt_level1: usize,
}

#[test]
fn campagne_adverse_placement_automatique() {
    let defs = defs();
    let (base, ids) = build(&defs);
    let mut rng = Rng(0x5EED_A070_0127_0001);
    let mut cov = Cov::default();
    let cancel = AtomicBool::new(false);
    for case in 0..180 {
        let width = rng.pick(&[150.0, 220.0, 300.0, 400.0, 600.0]);
        let sel = rng.pick(&[0.0, 10.0, 20.0]);
        let gap = rng.pick(&[0.0, 2.0, 8.0]);
        let folded = rng.chance(30);
        let grain = rng.chance(40).then_some(90.0);
        let allow = rng.chance(60);
        let mut p = base.clone();
        let plan = p
            .apply(Command::AddCuttingPlan {
                spec: CuttingPlanSpec {
                    name: "P".into(),
                    material: None,
                    fabric_width_mm: width,
                    fold: if folded {
                        FabricFold::Folded
                    } else {
                        FabricFold::Flat
                    },
                    selvedge_mm: sel,
                    min_gap_mm: gap,
                    grain_direction: grain,
                },
            })
            .unwrap()
            .cutting_plans[0];
        let (lo, hi) = (sel, if folded { width / 2.0 } else { width - sel });
        let span = hi - lo;
        let mut kinds: Vec<usize> = Vec::new();
        for _ in 0..(1 + rng.below(4)) {
            let k = rng.below(defs.len() as u64) as usize;
            if !kinds.contains(&k) {
                kinds.push(k);
            }
        }
        let mut items = Vec::new();
        for &k in &kinds {
            let q = 1 + rng.below(4) as u32;
            let m = if defs[k].pair { rng.below(3) as u32 } else { 0 };
            items.push(AutoItem {
                piece: ids[k],
                size: None,
                quantity: q,
                mirrored_quantity: m,
            });
        }
        // Pose d'un plan sans placement existant, ou avec quelques placements à remplacer.
        if rng.chance(30) {
            p.apply(Command::AddPlacement {
                plan,
                piece: ids[1],
                size: None,
                x_mm: 0.0,
                y_mm: 0.0,
                rotation_deg: 0.0,
                mirrored: false,
            })
            .unwrap();
        }
        let opts = AutoOptions {
            allow_rotation: allow,
            time_limit_ms: 10_000,
        };
        // Oracle : rotations permises et pièces non posables.
        let grain_blocks_quarter = |k: usize| grain.is_some() && defs[k].vertical_grain;
        let min_width = |k: usize| {
            let (w, h) = defs[k].wh;
            if allow && !grain_blocks_quarter(k) {
                w.min(h)
            } else {
                w
            }
        };
        let expect_refused: Vec<PieceId> = kinds
            .iter()
            .filter(|k| min_width(**k) > span + 1e-6)
            .map(|k| ids[*k])
            .collect();
        let before = p.clone();
        let t0 = Instant::now();
        let first = p.propose_cutting_layout(plan, &items, &opts, &cancel);
        // Le calcul rend la main dans le budget demandé ; la marge n'absorbe qu'une machine
        // chargée, un calcul qui ignorerait son budget échoue toujours.
        let spent = t0.elapsed();
        let limit = std::time::Duration::from_millis(opts.time_limit_ms)
            + std::time::Duration::from_secs(20);
        assert!(
            spent < limit,
            "cas {case} trop long : {spent:?} (limite {limit:?})"
        );
        assert_eq!(p, before, "cas {case} : lecture pure");
        let second = p.propose_cutting_layout(plan, &items, &opts, &cancel);
        assert_eq!(
            format!("{first:?}"),
            format!("{second:?}"),
            "cas {case} : déterminisme"
        );
        if !expect_refused.is_empty() {
            match first {
                Err(AutoError::Unplaceable(v)) => {
                    let mut got: Vec<PieceId> = v.iter().map(|u| u.piece).collect();
                    let mut want = expect_refused.clone();
                    got.sort();
                    want.sort();
                    assert_eq!(got, want, "cas {case}");
                    assert!(v
                        .iter()
                        .all(|u| matches!(u.reason, UnplaceableReason::TooWide { .. })));
                    cov.refused += 1;
                }
                other => panic!("cas {case} : refus attendu, obtenu {other:?}"),
            }
            continue;
        }
        let prop = first.unwrap_or_else(|e| panic!("cas {case} : {e:?}"));
        cov.ok += 1;
        cov.refined += usize::from(prop.refined);
        cov.time_limited += usize::from(prop.time_limited);
        cov.folded_ok += usize::from(folded);
        cov.grain_ok += usize::from(grain.is_some());
        cov.gaps += usize::from(gap > 0.0);
        cov.selvedge += usize::from(sel > 0.0);
        assert!(
            prop.proposed.yield_percent >= prop.level1.yield_percent - 1e-9,
            "cas {case} : niveau 2 pire que niveau 1"
        );
        cov.lt_level1 +=
            usize::from(prop.proposed.yield_percent > prop.level1.yield_percent + 1e-9);
        // Quantités exactes.
        let expected_total: usize = items
            .iter()
            .map(|i| (i.quantity + i.mirrored_quantity) as usize)
            .sum();
        assert_eq!(prop.poses.len(), expected_total, "cas {case}");
        for it in &items {
            let n = prop
                .poses
                .iter()
                .filter(|q| q.piece == it.piece && !q.mirrored)
                .count();
            let m = prop
                .poses
                .iter()
                .filter(|q| q.piece == it.piece && q.mirrored)
                .count();
            assert_eq!(
                (n, m),
                (it.quantity as usize, it.mirrored_quantity as usize),
                "cas {case}"
            );
        }
        // Pose par l'oracle.
        let kind_of = |piece: PieceId| ids.iter().position(|i| *i == piece).unwrap();
        let posed: Vec<(usize, Posed, f64)> = prop
            .poses
            .iter()
            .map(|q| {
                let k = kind_of(q.piece);
                cov.mirrored += usize::from(q.mirrored);
                cov.rotated += usize::from(q.rotation_deg != 0.0 && q.rotation_deg != 180.0);
                cov.disks += usize::from(k == 4);
                // Rotation permise ?
                assert!(
                    allow || q.rotation_deg == 0.0 || q.rotation_deg == 180.0,
                    "cas {case} : rotation non autorisée"
                );
                assert!(
                    !(grain_blocks_quarter(k)
                        && (q.rotation_deg == 90.0 || q.rotation_deg == 270.0)),
                    "cas {case} : droit-fil violé"
                );
                (
                    k,
                    pose(&defs[k].shape, q.x_mm, q.y_mm, q.rotation_deg, q.mirrored),
                    area(&defs[k].shape),
                )
            })
            .collect();
        let mut top = 0.0_f64;
        for (i, (_, a, _)) in posed.iter().enumerate() {
            let b = bbox(a);
            assert!(
                b.0 >= lo - 1e-6 && b.2 <= hi + 1e-6 && b.1 >= -1e-6,
                "cas {case} : débordement {b:?}"
            );
            top = top.max(b.3);
            for (j, (_, c, _)) in posed.iter().enumerate().skip(i + 1) {
                assert_eq!(
                    sampled_overlap(a, c),
                    0,
                    "cas {case} : chevauchement {i}/{j}"
                );
                let (bc, ba) = (bbox(c), bbox(a));
                let far = ba.0 > bc.2 + gap + 5.0
                    || bc.0 > ba.2 + gap + 5.0
                    || ba.1 > bc.3 + gap + 5.0
                    || bc.1 > ba.3 + gap + 5.0;
                if !far && gap > 0.0 {
                    let d = distance(a, c);
                    assert!(d >= gap - 0.4, "cas {case} : écart {d} < {gap} ({i}/{j})");
                }
            }
        }
        // Longueur et rendement recalculés.
        let used: f64 = posed.iter().map(|(_, _, a)| *a).sum();
        let want_yield = 100.0 * used / ((width - 2.0 * sel) * top);
        assert!(
            (prop.proposed.length_mm - top).abs() <= 0.1,
            "cas {case} : longueur"
        );
        assert!(
            (prop.proposed.yield_percent - want_yield).abs() <= 0.2,
            "cas {case} : rendement {} / {want_yield}",
            prop.proposed.yield_percent
        );
        // Application : une étape, plan valide.
        let mut doc = Document::new(p.clone());
        doc.execute(prop.command.clone()).unwrap();
        assert!(
            doc.project().analyze_cutting_plan(plan).unwrap().is_valid(),
            "cas {case}"
        );
        assert!(doc.undo());
        assert_eq!(doc.project(), &p, "cas {case} : annulation en un pas");
    }
    assert!(cov.refined >= 3, "niveau 2 retenu {}", cov.refined);
    assert!(cov.lt_level1 >= 1, "niveau 2 meilleur {}", cov.lt_level1);
    assert!(cov.ok >= 100, "ok {}", cov.ok);
    assert!(cov.refused >= 10, "refus {}", cov.refused);
    assert!(cov.folded_ok >= 15, "pliés {}", cov.folded_ok);
    assert!(cov.grain_ok >= 30, "droit-fil {}", cov.grain_ok);
    assert!(cov.mirrored >= 10, "miroirs {}", cov.mirrored);
    assert!(cov.rotated >= 10, "tournées {}", cov.rotated);
    assert!(cov.gaps >= 30 && cov.selvedge >= 30, "écarts / lisières");
    assert!(cov.disks >= 10, "disques {}", cov.disks);
    assert_eq!(cov.time_limited, 0);
}

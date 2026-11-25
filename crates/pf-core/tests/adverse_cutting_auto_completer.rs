//! Plans de coupe, placement automatique en mode « compléter » (tranche 133) : campagne adverse
//! de 230 tirages à graine fixe. Un plan existant valide est d'abord obtenu par « tout replacer »,
//! puis l'oracle le malmène (retraits, quantités relevées ou abaissées, doublon, débordement) et
//! décide seul du résultat attendu.
//!
//! **Oracle indépendant du cœur** : formes locales connues, rotations à 4 valeurs (jamais
//! `place_point`), chevauchements par échantillonnage de points, écarts par échantillonnage des
//! bords, décision seule des zones de pose / pli / début, des rotations permises et des quantités.
//! Propriétés : l'existant n'est JAMAIS modifié (mêmes placements, mêmes identifiants, même
//! ordre), quantités exactes, zéro chevauchement ni débordement, écart respecté, déterminisme,
//! lecture pure, une seule étape annulable, refus nommés. Des compteurs prouvent la couverture.

use pf_core::cutting_auto::{AutoError, AutoItem, AutoMode, AutoOptions};
use pf_core::cutting_plan::{CuttingPlanSpec, FabricFold};
use pf_core::*;
use pf_geometry::PointMm;
use std::collections::BTreeSet;
use std::sync::atomic::AtomicBool;

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

struct Def {
    shape: Vec<P>,
    wh: P,
    vertical_grain: bool,
    pair: bool,
}

fn defs() -> Vec<Def> {
    let r = |w: f64, h: f64| vec![(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)];
    vec![
        Def {
            shape: r(100.0, 50.0),
            wh: (100.0, 50.0),
            vertical_grain: true,
            pair: false,
        },
        Def {
            shape: r(60.0, 60.0),
            wh: (60.0, 60.0),
            vertical_grain: false,
            pair: true,
        },
        Def {
            shape: vec![(0.0, 0.0), (80.0, 0.0), (0.0, 40.0)],
            wh: (80.0, 40.0),
            vertical_grain: false,
            pair: false,
        },
        Def {
            shape: vec![
                (0.0, 0.0),
                (80.0, 0.0),
                (80.0, 30.0),
                (30.0, 30.0),
                (30.0, 70.0),
                (0.0, 70.0),
            ],
            wh: (80.0, 70.0),
            vertical_grain: false,
            pair: false,
        },
        Def {
            shape: r(300.0, 30.0),
            wh: (300.0, 30.0),
            vertical_grain: false,
            pair: false,
        },
        Def {
            shape: vec![(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)],
            wh: (100.0, 100.0),
            vertical_grain: false,
            pair: false,
        },
        Def {
            shape: r(50.0, 50.0),
            wh: (50.0, 50.0),
            vertical_grain: false,
            pair: false,
        },
        Def {
            shape: vec![
                (0.0, 0.0),
                (90.0, 0.0),
                (90.0, 20.0),
                (45.0, 60.0),
                (0.0, 20.0),
            ],
            wh: (90.0, 60.0),
            vertical_grain: false,
            pair: false,
        },
    ]
}

fn build(defs: &[Def]) -> (Project, Vec<PieceId>) {
    let mut p = Project::new("Adverse compléter");
    let s = p.sheets[0].id;
    let mut ids = Vec::new();
    for (k, d) in defs.iter().enumerate() {
        let (ox, oy) = (400.0 * (k as f64 + 1.0), 40.0 + 13.0 * k as f64);
        let e = p
            .apply(Command::AddPolygon {
                sheet: s,
                vertices: d
                    .shape
                    .iter()
                    .map(|q| PointRef::New(PointMm::new(ox + q.0, oy + q.1)))
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

fn cs(rot: f64) -> (f64, f64) {
    match rot as i64 {
        0 => (1.0, 0.0),
        90 => (0.0, 1.0),
        180 => (-1.0, 0.0),
        270 => (0.0, -1.0),
        r => panic!("rotation inattendue {r}"),
    }
}

fn pose(sh: &[P], x: f64, y: f64, rot: f64, mirrored: bool) -> Vec<P> {
    let (c, s) = cs(rot);
    sh.iter()
        .map(|q| {
            let q = if mirrored { (-q.0, q.1) } else { *q };
            (c * q.0 - s * q.1 + x, s * q.0 + c * q.1 + y)
        })
        .collect()
}

fn bbox(v: &[P]) -> (f64, f64, f64, f64) {
    v.iter()
        .fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |b, q| {
            (b.0.min(q.0), b.1.min(q.1), b.2.max(q.0), b.3.max(q.1))
        })
}

fn area(v: &[P]) -> f64 {
    let n = v.len();
    (0..n)
        .map(|i| v[i].0 * v[(i + 1) % n].1 - v[(i + 1) % n].0 * v[i].1)
        .sum::<f64>()
        .abs()
        / 2.0
}

fn inside(v: &[P], q: P) -> bool {
    let mut inside = false;
    let n = v.len();
    for i in 0..n {
        let (a, b) = (v[i], v[(i + 1) % n]);
        if (a.1 > q.1) != (b.1 > q.1) && q.0 < a.0 + (q.1 - a.1) / (b.1 - a.1) * (b.0 - a.0) {
            inside = !inside;
        }
    }
    inside
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

fn dist_to(p: P, v: &[P]) -> f64 {
    let n = v.len();
    (0..n)
        .map(|i| seg_dist(p, v[i], v[(i + 1) % n]))
        .fold(f64::MAX, f64::min)
}

fn boundary(v: &[P]) -> Vec<P> {
    let n = v.len();
    let mut out = Vec::new();
    for i in 0..n {
        let (a, b) = (v[i], v[(i + 1) % n]);
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        let k = len.ceil().max(1.0) as usize;
        for j in 0..k {
            let t = j as f64 / k as f64;
            out.push((a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1)));
        }
    }
    out
}

fn distance(a: &[P], b: &[P]) -> f64 {
    let m = boundary(a)
        .into_iter()
        .map(|p| dist_to(p, b))
        .fold(f64::MAX, f64::min);
    boundary(b)
        .into_iter()
        .map(|p| dist_to(p, a))
        .fold(m, f64::min)
}

fn sampled_overlap(a: &[P], b: &[P]) -> usize {
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
    cases: usize,
    completed: usize,
    nothing: usize,
    exceeds: usize,
    overlap_refused: usize,
    overflow_refused: usize,
    folded_ok: usize,
    grain_ok: usize,
    mirrored_new: usize,
    rotated_new: usize,
    gaps: usize,
    refined: usize,
    new_poses: usize,
    kept_total: usize,
}

type Key = (usize, bool);

fn counts(ids: &[PieceId], pls: &[CuttingPlacement]) -> std::collections::BTreeMap<Key, usize> {
    let mut m = std::collections::BTreeMap::new();
    for pl in pls {
        let k = ids.iter().position(|i| *i == pl.piece).unwrap();
        *m.entry((k, pl.mirrored)).or_insert(0) += 1;
    }
    m
}

#[test]
fn campagne_adverse_placement_automatique_completer() {
    let defs = defs();
    let (base, ids) = build(&defs);
    let mut rng = Rng(0x5EED_A070_0133_0001);
    let mut cov = Cov::default();
    let cancel = AtomicBool::new(false);
    for case in 0..230 {
        let width = rng.pick(&[220.0, 300.0, 400.0, 600.0]);
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
        let grain_blocks_quarter = |k: usize| grain.is_some() && defs[k].vertical_grain;
        let min_width = |k: usize| {
            let (w, h) = defs[k].wh;
            if allow && !grain_blocks_quarter(k) {
                w.min(h)
            } else {
                w
            }
        };
        let mut kinds: Vec<usize> = Vec::new();
        for _ in 0..(1 + rng.below(4)) {
            let k = rng.below(defs.len() as u64) as usize;
            if !kinds.contains(&k) && min_width(k) <= span + 1e-6 {
                kinds.push(k);
            }
        }
        if kinds.is_empty() {
            continue;
        }
        let mut items: Vec<AutoItem> = kinds
            .iter()
            .map(|&k| AutoItem {
                piece: ids[k],
                size: None,
                quantity: 1 + rng.below(3) as u32,
                mirrored_quantity: if defs[k].pair { rng.below(3) as u32 } else { 0 },
            })
            .collect();
        let opts = AutoOptions {
            allow_rotation: allow,
            time_limit_ms: 10_000,
        };
        // Existant valide, obtenu par « tout replacer » (mode d'avant, inchangé).
        let Ok(first) = p.propose_cutting_layout(plan, &items, &opts, &cancel) else {
            continue;
        };
        p.apply(first.command).unwrap();
        cov.cases += 1;

        // Scénario choisi par l'oracle.
        #[derive(PartialEq, Clone, Copy)]
        enum Sc {
            Nothing,
            Exceeds,
            Overlap,
            Overflow,
            Missing,
        }
        let roll = rng.below(100);
        let mut sc = match roll {
            0..=11 => Sc::Nothing,
            12..=23 => Sc::Exceeds,
            24..=33 => Sc::Overlap,
            34..=43 => Sc::Overflow,
            _ => Sc::Missing,
        };
        if sc == Sc::Exceeds {
            // Abaisse une quantité sans tout annuler.
            let i = rng.below(items.len() as u64) as usize;
            let total: u32 = items.iter().map(|x| x.quantity + x.mirrored_quantity).sum();
            if total >= 2 && items[i].quantity >= 1 {
                items[i].quantity -= 1;
            } else {
                sc = Sc::Missing;
            }
        }
        let before = p.cutting_plan(plan).unwrap().placements.clone();
        match sc {
            Sc::Overlap => {
                let d = before[rng.below(before.len() as u64) as usize];
                p.apply(Command::AddPlacement {
                    plan,
                    piece: d.piece,
                    size: d.size,
                    x_mm: d.x_mm,
                    y_mm: d.y_mm,
                    rotation_deg: d.rotation_deg,
                    mirrored: d.mirrored,
                })
                .unwrap();
            }
            Sc::Overflow => {
                let d = before[rng.below(before.len() as u64) as usize];
                p.apply(Command::UpdatePlacement {
                    plan,
                    placement: d.id,
                    size: d.size,
                    x_mm: width + 20.0,
                    y_mm: d.y_mm,
                    rotation_deg: d.rotation_deg,
                    mirrored: d.mirrored,
                })
                .unwrap();
            }
            Sc::Missing => {
                let k = (1 + rng.below(3) as usize).min(before.len());
                for _ in 0..k {
                    let live = p.cutting_plan(plan).unwrap().placements.clone();
                    let victim = live[rng.below(live.len() as u64) as usize];
                    p.apply(Command::RemovePlacement {
                        plan,
                        placement: victim.id,
                    })
                    .unwrap();
                }
                if rng.chance(30) {
                    items[0].quantity += 1 + rng.below(2) as u32;
                }
            }
            Sc::Nothing | Sc::Exceeds => {}
        }
        let existing = p.cutting_plan(plan).unwrap().placements.clone();
        let snapshot = p.clone();
        let first =
            p.propose_cutting_layout_mode(plan, &items, &opts, AutoMode::CompleteMissing, &cancel);
        assert_eq!(p, snapshot, "cas {case} : lecture pure");
        let second =
            p.propose_cutting_layout_mode(plan, &items, &opts, AutoMode::CompleteMissing, &cancel);
        assert_eq!(
            format!("{first:?}"),
            format!("{second:?}"),
            "cas {case} : déterminisme"
        );

        // Quantités demandées et posées, comptées par l'oracle.
        let have = counts(&ids, &existing);
        let mut want: std::collections::BTreeMap<Key, usize> = Default::default();
        for it in &items {
            let k = ids.iter().position(|i| *i == it.piece).unwrap();
            *want.entry((k, false)).or_insert(0) += it.quantity as usize;
            *want.entry((k, true)).or_insert(0) += it.mirrored_quantity as usize;
        }
        let over: BTreeSet<Key> = want
            .iter()
            .filter(|(k, w)| have.get(k).copied().unwrap_or(0) > **w)
            .map(|(k, _)| *k)
            .collect();
        let missing_total: usize = want
            .iter()
            .map(|(k, w)| w.saturating_sub(have.get(k).copied().unwrap_or(0)))
            .sum();

        match sc {
            Sc::Overlap | Sc::Overflow => {
                match first {
                    Err(AutoError::ExistingInvalid(e)) => {
                        if sc == Sc::Overlap {
                            assert!(e.overlaps >= 1, "cas {case} : {e:?}");
                            cov.overlap_refused += 1;
                        } else {
                            assert!(e.overflows >= 1, "cas {case} : {e:?}");
                            cov.overflow_refused += 1;
                        }
                    }
                    other => panic!("cas {case} : existant invalide attendu, obtenu {other:?}"),
                }
                continue;
            }
            Sc::Exceeds => {
                match first {
                    Err(AutoError::ExistingExceeds(v)) => {
                        let got: BTreeSet<Key> = v
                            .iter()
                            .map(|o| (ids.iter().position(|i| *i == o.piece).unwrap(), o.mirrored))
                            .collect();
                        assert_eq!(got, over, "cas {case}");
                        cov.exceeds += 1;
                    }
                    other => panic!("cas {case} : excès attendu, obtenu {other:?}"),
                }
                continue;
            }
            Sc::Nothing | Sc::Missing => {}
        }
        assert!(over.is_empty());
        if missing_total == 0 {
            assert_eq!(
                first.unwrap_err(),
                AutoError::NothingToComplete,
                "cas {case}"
            );
            cov.nothing += 1;
            continue;
        }
        let prop = first.unwrap_or_else(|e| panic!("cas {case} : {e:?}"));
        cov.completed += 1;
        cov.refined += usize::from(prop.refined);
        cov.folded_ok += usize::from(folded);
        cov.grain_ok += usize::from(grain.is_some());
        cov.gaps += usize::from(gap > 0.0);
        cov.kept_total += existing.len();
        assert_eq!(prop.poses.len(), missing_total, "cas {case}");
        cov.new_poses += prop.poses.len();
        assert!(
            prop.proposed.yield_percent >= prop.level1.yield_percent - 1e-9,
            "cas {case} : niveau 2 pire que niveau 1"
        );

        // Application en une étape.
        let mut doc = Document::new(p.clone());
        doc.execute(prop.command.clone()).unwrap();
        let after = doc.project().cutting_plan(plan).unwrap().placements.clone();
        assert_eq!(after.len(), existing.len() + missing_total, "cas {case}");
        // L'existant n'est jamais modifié : mêmes placements, mêmes identifiants, même ordre.
        assert_eq!(&after[..existing.len()], &existing[..], "cas {case}");
        // Quantités exactes : cible pour les pièces demandées, inchangé pour les autres.
        let got = counts(&ids, &after);
        for (k, n) in &got {
            let expect = want
                .get(k)
                .copied()
                .map_or(*n, |w| w.max(have.get(k).copied().unwrap_or(0)));
            assert_eq!(*n, expect, "cas {case} : quantité {k:?}");
        }
        for (k, w) in &want {
            assert_eq!(
                got.get(k).copied().unwrap_or(0),
                *w,
                "cas {case} : cible {k:?}"
            );
        }
        // Contours posés par l'oracle (existant et nouveau).
        let kind_of = |piece: PieceId| ids.iter().position(|i| *i == piece).unwrap();
        for q in &after[existing.len()..] {
            let k = kind_of(q.piece);
            cov.mirrored_new += usize::from(q.mirrored);
            cov.rotated_new += usize::from(q.rotation_deg != 0.0 && q.rotation_deg != 180.0);
            assert!(
                allow || q.rotation_deg == 0.0 || q.rotation_deg == 180.0,
                "cas {case} : rotation non autorisée"
            );
            assert!(
                !(grain_blocks_quarter(k) && (q.rotation_deg == 90.0 || q.rotation_deg == 270.0)),
                "cas {case} : droit-fil violé"
            );
        }
        let posed: Vec<(Vec<P>, f64)> = after
            .iter()
            .map(|q| {
                let k = kind_of(q.piece);
                (
                    pose(&defs[k].shape, q.x_mm, q.y_mm, q.rotation_deg, q.mirrored),
                    area(&defs[k].shape),
                )
            })
            .collect();
        let mut top = 0.0_f64;
        for (i, (a, _)) in posed.iter().enumerate() {
            let b = bbox(a);
            assert!(
                b.0 >= lo - 1e-6 && b.2 <= hi + 1e-6 && b.1 >= -1e-6,
                "cas {case} : débordement {b:?}"
            );
            top = top.max(b.3);
            for (j, (c, _)) in posed.iter().enumerate().skip(i + 1) {
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
        let used: f64 = posed.iter().map(|(_, a)| *a).sum();
        let want_yield = 100.0 * used / ((width - 2.0 * sel) * top);
        assert!(
            (prop.proposed.length_mm - top).abs() <= 0.1,
            "cas {case} : longueur {} / {top}",
            prop.proposed.length_mm
        );
        // Le rendement du cœur est celui de la matière utile entière (laize - 2 lisières) ; en
        // pli, la comparaison reste valable (même formule des deux côtés).
        if !folded {
            assert!(
                (prop.proposed.yield_percent - want_yield).abs() <= 0.2,
                "cas {case} : rendement {} / {want_yield}",
                prop.proposed.yield_percent
            );
        }
        // Une seule étape annulable, retour exact.
        assert!(doc.undo());
        assert_eq!(doc.project(), &p, "cas {case} : annulation en un pas");
        assert!(!doc.undo(), "cas {case} : une seule étape");
    }
    assert!(cov.cases >= 150, "tirages exploitables {}", cov.cases);
    assert!(cov.completed >= 80, "complétés {}", cov.completed);
    assert!(cov.nothing >= 8, "rien à compléter {}", cov.nothing);
    assert!(cov.exceeds >= 8, "excès {}", cov.exceeds);
    assert!(
        cov.overlap_refused >= 8,
        "chevauchants {}",
        cov.overlap_refused
    );
    assert!(
        cov.overflow_refused >= 8,
        "débordants {}",
        cov.overflow_refused
    );
    assert!(cov.folded_ok >= 10, "pliés {}", cov.folded_ok);
    assert!(cov.grain_ok >= 20, "droit-fil {}", cov.grain_ok);
    assert!(cov.gaps >= 30, "écarts {}", cov.gaps);
    assert!(cov.mirrored_new >= 3, "miroirs posés {}", cov.mirrored_new);
    assert!(cov.new_poses >= 100, "poses {}", cov.new_poses);
    assert!(
        cov.kept_total >= 100,
        "placements conservés {}",
        cov.kept_total
    );
    assert!(cov.rotated_new >= 5, "poses tournées {}", cov.rotated_new);
    assert!(cov.refined >= 1, "niveau 2 retenu {}", cov.refined);
}

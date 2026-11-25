//! US-09.9 — campagne adverse « contrôle de forme » : 160 séquences à graine fixe de 14 pas sur 1 à
//! 3 Béziers cubiques (3 à 6 tailles, base variable) : écarts posés sur le départ ou l'arrivée,
//! effacés, changements de base, lectures avec gamme ou feuille inconnues. Un tiers des courbes
//! est une « arche » symétrique (`c1 = p0 + (a ; b)`, `c2 = p3 + (−a ; b)`) dont l'arrivée est
//! envoyée au voisinage du rebroussement `p3 = p0 + (a ; 0)`, pour que des tailles le franchissent.
//!
//! **Oracle indépendant** : le modèle (`Model`) ne garde que la **position absolue du départ et de
//! l'arrivée de chaque courbe dans chaque taille** (base + écart, jamais relue du cœur) et les deux
//! poignées relatives. Il évalue la courbe en **base de puissance** (`B′ = 3a·t² + 2b·t + c`,
//! `B″ = 6a·t + 2b`, formule réécrite ici), la courbure `κ = (x′y″ − y′x″) / (x′² + y′²)^{3/2}`,
//! compte à la main les changements de signe de `κ` et les rebroussements (vitesse faible ou
//! tangente retournée, angle par `atan2`), et déduit les signalements attendus par ses propres
//! règles. Les seuils sont écrits **ici** en littéraux (jamais relus des constantes du cœur).
//! Les décisions à moins de 1e-6 près d'un seuil ne sont pas comparées (comptées : au plus 3 %).
//!
//! Invariants à chaque pas : lecture sans effet (octets identiques) ; une entrée par courbe, une
//! lecture par taille dans l'ordre de la gamme ; rebroussements, inflexions, courbure maximale
//! (quand elle est bien définie) et signalements = oracle ; un changement de base ne change aucune
//! lecture par taille ni aucun saut de courbure ; gamme et feuille inconnues refusées.
//!
//! Mutations vérifiées à la main : voir `docs/JOURNAL.md` (tranche 100).

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;

const SEQUENCES: u64 = 160;
const STEPS: usize = 14;
const GHOST: u64 = 987_654;
const POOL: [&str; 6] = ["XXS", "XS", "S", "M", "L", "XL"];

// Seuils de l'oracle (littéraux : convention d'atelier, écrite indépendamment du cœur).
const N: usize = 256;
const DENSE: usize = 4096;
const SLOW: f64 = 1e-3;
const TURN_DEG: f64 = 150.0;
const K_ZERO: f64 = 1e-5;
const JUMP: f64 = 0.5;
const TIE: f64 = 1e-6;

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
    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.below(100) < percent
    }
}

type V = (f64, f64);

fn bytes(p: &Project) -> String {
    persist::to_json(p).expect("projet valide")
}

// ---------------------------------------------------------------------------------- oracle

#[derive(Debug, Clone, Copy)]
struct Reading {
    degenerate: bool,
    cusps: usize,
    inflections: usize,
    kmax: f64,
    /// Les décisions (rebroussement, signe de κ) ne sont pas à un seuil près.
    sure: bool,
    /// `kmax` est bien défini : pas de rebroussement, vitesse minimale franche partout.
    kmax_defined: bool,
}

/// Évaluation d'une Bézier `[p0, c1, c2, p3]` en base de puissance.
struct Poly {
    a: V,
    b: V,
    c: V,
}

impl Poly {
    fn new(q: [V; 4]) -> Poly {
        let k = |f: fn(V) -> f64| {
            let (p0, c1, c2, p3) = (f(q[0]), f(q[1]), f(q[2]), f(q[3]));
            (
                -p0 + 3.0 * c1 - 3.0 * c2 + p3,
                3.0 * p0 - 6.0 * c1 + 3.0 * c2,
                3.0 * (c1 - p0),
            )
        };
        let (ax, bx, cx) = k(|v| v.0);
        let (ay, by, cy) = k(|v| v.1);
        Poly {
            a: (ax, ay),
            b: (bx, by),
            c: (cx, cy),
        }
    }
    fn d1(&self, t: f64) -> V {
        (
            3.0 * self.a.0 * t * t + 2.0 * self.b.0 * t + self.c.0,
            3.0 * self.a.1 * t * t + 2.0 * self.b.1 * t + self.c.1,
        )
    }
    fn d2(&self, t: f64) -> V {
        (
            6.0 * self.a.0 * t + 2.0 * self.b.0,
            6.0 * self.a.1 * t + 2.0 * self.b.1,
        )
    }
}

fn norm(v: V) -> f64 {
    v.0.hypot(v.1)
}

fn kappa(poly: &Poly, t: f64) -> f64 {
    let (v, a) = (poly.d1(t), poly.d2(t));
    let s2 = v.0 * v.0 + v.1 * v.1;
    (v.0 * a.1 - v.1 * a.0) / s2.powf(1.5)
}

fn read(q: [V; 4]) -> Reading {
    let spread = norm((q[1].0 - q[0].0, q[1].1 - q[0].1))
        + norm((q[2].0 - q[1].0, q[2].1 - q[1].1))
        + norm((q[3].0 - q[2].0, q[3].1 - q[2].1));
    if spread <= 1e-9 {
        return Reading {
            degenerate: true,
            cusps: 0,
            inflections: 0,
            kmax: 0.0,
            sure: true,
            kmax_defined: false,
        };
    }
    let poly = Poly::new(q);
    let ts: Vec<f64> = (0..=N).map(|j| j as f64 / N as f64).collect();
    let speeds: Vec<f64> = ts.iter().map(|&t| norm(poly.d1(t))).collect();
    let mean = speeds.iter().sum::<f64>() / speeds.len() as f64;
    let slow = SLOW * mean;
    let mut sure = true;
    // Rebroussements.
    let mut flagged = vec![false; ts.len()];
    for j in 1..N {
        if speeds[j] < slow {
            flagged[j] = true;
        }
        if ((speeds[j] / slow) - 1.0).abs() < TIE {
            sure = false;
        }
    }
    for j in 0..N {
        let (u, w) = (poly.d1(ts[j]), poly.d1(ts[j + 1]));
        if speeds[j] > mean * 1e-12 && speeds[j + 1] > mean * 1e-12 {
            let ang = (u.0 * w.1 - u.1 * w.0).abs().atan2(u.0 * w.0 + u.1 * w.1);
            let lim = TURN_DEG.to_radians();
            if ang > lim {
                flagged[j] = true;
                flagged[j + 1] = true;
            }
            if (ang / lim - 1.0).abs() < TIE {
                sure = false;
            }
        }
    }
    let mut cusps = 0;
    let mut prev = false;
    for &f in &flagged {
        if f && !prev {
            cusps += 1;
        }
        prev = f;
    }
    // Inflexions : signe de κ entre échantillons significatifs.
    let mut inflections = 0;
    let mut last = 0.0_f64;
    for (j, &t) in ts.iter().enumerate() {
        if speeds[j] <= slow {
            continue;
        }
        let k = kappa(&poly, t);
        if (k.abs() / K_ZERO - 1.0).abs() < TIE {
            sure = false;
        }
        if k.abs() < K_ZERO {
            continue;
        }
        if last != 0.0 && (k > 0.0) != (last > 0.0) {
            inflections += 1;
        }
        last = k;
    }
    // Courbure maximale sur une grille dense, hors zone de rebroussement.
    let mut kmax = 0.0_f64;
    let mut min_ratio = f64::INFINITY;
    for j in 0..=DENSE {
        let t = j as f64 / DENSE as f64;
        let s = norm(poly.d1(t));
        min_ratio = min_ratio.min(s / mean);
        if s > slow {
            kmax = kmax.max(kappa(&poly, t).abs());
        }
    }
    Reading {
        degenerate: false,
        cusps,
        inflections,
        kmax,
        sure,
        kmax_defined: cusps == 0 && min_ratio > 0.05,
    }
}

/// Saut de courbure attendu de `k1` à `k2` (règle réécrite) et décision incertaine ?
fn jump(k1: f64, k2: f64) -> (bool, bool) {
    if k1 < K_ZERO {
        return (k2 >= K_ZERO, false);
    }
    let r = (k2 - k1).abs() / k1;
    (r > JUMP, (r / JUMP - 1.0).abs() < 1e-3)
}

// ---------------------------------------------------------------------------------- modèle

struct CurveModel {
    h0: V,
    h1: V,
    /// Départ et arrivée absolus dans chaque taille.
    p0: Vec<V>,
    p3: Vec<V>,
    /// Pour une arche : demi-largeur de poignée `a` (cible du rebroussement).
    arch: Option<f64>,
}

struct Model {
    curves: Vec<CurveModel>,
    base: usize,
}

impl Model {
    fn quad(&self, c: usize, s: usize) -> [V; 4] {
        let cv = &self.curves[c];
        let (p0, p3) = (cv.p0[s], cv.p3[s]);
        [
            p0,
            (p0.0 + cv.h0.0, p0.1 + cv.h0.1),
            (p3.0 + cv.h1.0, p3.1 + cv.h1.1),
            p3,
        ]
    }
}

struct Ids {
    sheet: SheetId,
    range: SizeRangeId,
    sizes: Vec<SizeId>,
    curves: Vec<EntityId>,
    /// (départ, arrivée) de chaque courbe.
    ends: Vec<(PointId, PointId)>,
}

fn build(rng: &mut Rng) -> (Project, Ids, Model) {
    let n = 3 + rng.below(4) as usize;
    let base = rng.below(n as u64) as usize;
    let mut p = Project::new("Forme adverse");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddSizeRange {
        name: "Gamme".into(),
        kind: SizeKind::Alphanumeric,
        labels: POOL[..n].iter().map(|s| s.to_string()).collect(),
        base,
    })
    .unwrap();
    let range = p.sizes[0].id;
    let sizes: Vec<SizeId> = p.sizes[0].sizes.iter().map(|s| s.id).collect();
    let count = 1 + rng.below(3) as usize;
    let (mut ids, mut curves, mut ends) = (Vec::new(), Vec::new(), Vec::new());
    for k in 0..count {
        let ox = 1000.0 * k as f64;
        let (h0, h1, d, arch) = if rng.chance(35) {
            let (a, b) = (rng.range(30, 120) as f64, rng.range(20, 80) as f64);
            let d = rng.range(40, 160) as f64;
            ((a, b), (-a, b), (d, 0.0), Some(a))
        } else {
            let h0 = (rng.range(-120, 120) as f64, rng.range(-80, 80) as f64);
            let h1 = (rng.range(-120, 120) as f64, rng.range(-80, 80) as f64);
            let d = (rng.range(30, 160) as f64, rng.range(-60, 60) as f64);
            (nonzero(h0), nonzero(h1), d, None)
        };
        let (p0, p3) = ((ox, 0.0), (ox + d.0, d.1));
        let pm = |v: V| PointMm::new(v.0, v.1);
        let out = p
            .apply(Command::AddCurve {
                sheet,
                start: PointRef::New(pm(p0)),
                c1: pm((p0.0 + h0.0, p0.1 + h0.1)),
                c2: pm((p3.0 + h1.0, p3.1 + h1.1)),
                end: PointRef::New(pm(p3)),
            })
            .unwrap();
        ids.push(out.entities[0]);
        let find = |at: V| {
            let s = p.sheet(sheet).unwrap();
            s.points
                .iter()
                .find(|q| pf_geometry::distance_mm(s.position(q.id).unwrap(), pm(at)) < 1e-9)
                .unwrap()
                .id
        };
        ends.push((find(p0), find(p3)));
        curves.push(CurveModel {
            h0,
            h1,
            p0: vec![p0; n],
            p3: vec![p3; n],
            arch,
        });
    }
    (
        p,
        Ids {
            sheet,
            range,
            sizes,
            curves: ids,
            ends,
        },
        Model { curves, base },
    )
}

/// Une poignée jamais nulle (pas de vitesse nulle aux extrémités : κ y est mal défini).
fn nonzero(h: V) -> V {
    if norm(h) < 5.0 {
        (h.0 + 20.0, h.1 + 10.0)
    } else {
        h
    }
}

// ---------------------------------------------------------------------------------- vérification

#[derive(Debug, Clone, PartialEq)]
enum Tag {
    Cusp(usize, usize),
    Infl(usize, usize, usize),
    Jump(usize, usize),
    Degenerate(usize),
}

fn tag_of(k: ShapeSignalKind, size_idx: usize, ids: &Ids) -> Tag {
    match k {
        ShapeSignalKind::CuspIntroduced { cusps } => Tag::Cusp(size_idx, cusps),
        ShapeSignalKind::InflectionChanged { base, found } => Tag::Infl(size_idx, base, found),
        ShapeSignalKind::CurvatureJump { from, .. } => {
            Tag::Jump(size_idx, ids.sizes.iter().position(|s| *s == from).unwrap())
        }
        ShapeSignalKind::Degenerate { .. } => Tag::Degenerate(size_idx),
    }
}

#[derive(Default)]
struct Stats {
    compared: usize,
    skipped: usize,
    cusps_seen: usize,
    cusp_signals: usize,
    inflection_signals: usize,
    jump_signals: usize,
    degenerate_signals: usize,
}

fn check(p: &Project, ids: &Ids, m: &Model, ctx: &str, st: &mut Stats) -> ShapeControl {
    let before = bytes(p);
    let c = p.shape_control(ids.sheet, ids.range).expect("lecture");
    assert_eq!(bytes(p), before, "{ctx} : lecture sans effet");
    assert_eq!(c.sizes, ids.sizes, "{ctx} : ordre de la gamme");
    assert_eq!(c.base, ids.sizes[m.base], "{ctx} : base");
    let got: Vec<EntityId> = c.curves.iter().map(|x| x.entity).collect();
    assert_eq!(got, ids.curves, "{ctx} : une entrée par courbe");
    for (ci, cv) in c.curves.iter().enumerate() {
        assert_eq!(cv.kind, CurveKind::Bezier);
        assert_eq!(cv.sizes.len(), ids.sizes.len());
        assert_eq!(cv.steps.len(), ids.sizes.len() - 1);
        let reads: Vec<Reading> = (0..ids.sizes.len()).map(|s| read(m.quad(ci, s))).collect();
        let mut sure = true;
        for (s, (sz, r)) in cv.sizes.iter().zip(&reads).enumerate() {
            let ctx = format!("{ctx} courbe {ci} taille {s}");
            assert_eq!(sz.size, ids.sizes[s], "{ctx}");
            match (&sz.reading, r.degenerate) {
                (ShapeReading::Degenerate(_), true) => {}
                (ShapeReading::Metrics(_), false) => {}
                (got, want) => panic!("{ctx} : dégénérée attendue = {want}, lu {got:?}"),
            }
            let ShapeReading::Metrics(mt) = sz.reading else {
                continue;
            };
            if !r.sure {
                sure = false;
                continue;
            }
            assert_eq!(mt.cusps, r.cusps, "{ctx} : rebroussements");
            assert_eq!(mt.inflections, r.inflections, "{ctx} : inflexions");
            st.cusps_seen += r.cusps;
            if r.kmax_defined {
                let tol = 1e-3 * r.kmax + 1e-9;
                assert!(
                    (mt.max_curvature_per_mm - r.kmax).abs() <= tol,
                    "{ctx} : courbure max {} ≠ {}",
                    mt.max_curvature_per_mm,
                    r.kmax
                );
                if r.kmax >= K_ZERO {
                    let rad = mt.min_radius_mm.expect("rayon");
                    assert!((rad * mt.max_curvature_per_mm - 1.0).abs() < 1e-9, "{ctx}");
                }
            }
        }
        st.compared += reads.len();
        if !sure {
            st.skipped += reads.len();
            continue;
        }
        // Signalements attendus (règles de l'oracle).
        let mut want: Vec<Tag> = Vec::new();
        let base = &reads[m.base];
        for (s, r) in reads.iter().enumerate() {
            if r.degenerate {
                want.push(Tag::Degenerate(s));
                continue;
            }
            if !base.degenerate && s != m.base {
                if base.cusps == 0 && r.cusps > 0 {
                    want.push(Tag::Cusp(s, r.cusps));
                }
                if r.inflections != base.inflections {
                    want.push(Tag::Infl(s, base.inflections, r.inflections));
                }
            }
        }
        let mut undefined: Vec<usize> = Vec::new();
        for s in 1..reads.len() {
            let (a, b) = (&reads[s - 1], &reads[s]);
            if a.degenerate || b.degenerate || a.cusps > 0 || b.cusps > 0 {
                continue;
            }
            if !a.kmax_defined || !b.kmax_defined {
                undefined.push(s);
                continue;
            }
            let (j, tie) = jump(a.kmax, b.kmax);
            if tie {
                undefined.push(s);
            } else if j {
                want.push(Tag::Jump(s, s - 1));
            }
        }
        let mut have: Vec<Tag> = cv
            .signals
            .iter()
            .map(|sg| {
                let s = ids.sizes.iter().position(|x| *x == sg.size).unwrap();
                tag_of(sg.kind, s, ids)
            })
            .filter(|t| !matches!(t, Tag::Jump(s, _) if undefined.contains(s)))
            .collect();
        for t in &have {
            match t {
                Tag::Cusp(..) => st.cusp_signals += 1,
                Tag::Infl(..) => st.inflection_signals += 1,
                Tag::Jump(..) => st.jump_signals += 1,
                Tag::Degenerate(_) => st.degenerate_signals += 1,
            }
        }
        let key = |t: &Tag| format!("{t:?}");
        want.sort_by_key(key);
        have.sort_by_key(key);
        assert_eq!(have, want, "{ctx} courbe {ci} : signalements");
        // Ordre des signalements : par taille de la gamme (jamais un tri par gravité global).
        let order: Vec<usize> = cv
            .signals
            .iter()
            .map(|sg| ids.sizes.iter().position(|x| *x == sg.size).unwrap())
            .collect();
        assert!(order.windows(2).all(|w| w[0] <= w[1]), "{ctx} : ordre");
    }
    c
}

/// Lectures par taille sans la référence (base) : sert à comparer avant / après un changement de
/// base.
fn per_size(c: &ShapeControl) -> Vec<Vec<(usize, usize, f64)>> {
    c.curves
        .iter()
        .map(|cv| {
            cv.sizes
                .iter()
                .map(|s| match s.reading {
                    ShapeReading::Metrics(m) => (m.cusps, m.inflections, m.max_curvature_per_mm),
                    ShapeReading::Degenerate(_) => (usize::MAX, 0, 0.0),
                })
                .collect()
        })
        .collect()
}

fn jumps(c: &ShapeControl) -> Vec<Vec<(SizeId, SizeId)>> {
    c.curves
        .iter()
        .map(|cv| {
            cv.signals
                .iter()
                .filter_map(|s| match s.kind {
                    ShapeSignalKind::CurvatureJump { from, .. } => Some((s.size, from)),
                    _ => None,
                })
                .collect()
        })
        .collect()
}

// ---------------------------------------------------------------------------------- campagne

fn random_delta(rng: &mut Rng, m: &Model, c: usize, which_end: bool, s: usize) -> V {
    let cv = &m.curves[c];
    let basepos = if which_end {
        cv.p3[m.base]
    } else {
        cv.p0[m.base]
    };
    if let Some(a) = cv.arch.filter(|_| which_end) {
        if rng.chance(45) {
            // Au voisinage du rebroussement : arrivée à p0 + (a + e ; f).
            let jit = [0, 0, 1, -1, 2, -2, 5, -5];
            let e = jit[rng.below(jit.len() as u64) as usize] as f64;
            let f = jit[rng.below(jit.len() as u64) as usize] as f64 * 0.5;
            let target = (cv.p0[s].0 + a + e, cv.p0[s].1 + f);
            return (target.0 - basepos.0, target.1 - basepos.1);
        }
    }
    if rng.chance(20) {
        (rng.range(-150, 150) as f64, rng.range(-150, 150) as f64)
    } else {
        (rng.range(-25, 25) as f64, rng.range(-25, 25) as f64)
    }
}

fn run(seed: u64, st: &mut Stats) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let (mut p, ids, mut m) = build(&mut rng);
    let n = ids.sizes.len();
    check(&p, &ids, &m, &format!("seed {seed} initial"), st);
    for step in 0..STEPS {
        let ctx = format!("seed {seed} pas {step}");
        match rng.below(100) {
            0..=54 => {
                let c = rng.below(ids.curves.len() as u64) as usize;
                let end = rng.chance(60);
                let s = rng.below(n as u64) as usize;
                if s == m.base {
                    // Une taille de base ne reçoit jamais d'écart : refusé sans trace.
                    let before = bytes(&p);
                    let point = if end { ids.ends[c].1 } else { ids.ends[c].0 };
                    let r = p.apply(Command::SetPointGrading {
                        sheet: ids.sheet,
                        point,
                        size: ids.sizes[s],
                        dx_mm: 3.0,
                        dy_mm: 4.0,
                    });
                    assert!(r.is_err(), "{ctx} : base refusée");
                    assert_eq!(bytes(&p), before, "{ctx}");
                    continue;
                }
                let d = random_delta(&mut rng, &m, c, end, s);
                let point = if end { ids.ends[c].1 } else { ids.ends[c].0 };
                p.apply(Command::SetPointGrading {
                    sheet: ids.sheet,
                    point,
                    size: ids.sizes[s],
                    dx_mm: d.0,
                    dy_mm: d.1,
                })
                .unwrap_or_else(|e| panic!("{ctx} : {e}"));
                let cv = &mut m.curves[c];
                let b = if end { cv.p3[m.base] } else { cv.p0[m.base] };
                let v = (b.0 + d.0, b.1 + d.1);
                if end {
                    cv.p3[s] = v;
                } else {
                    cv.p0[s] = v;
                }
            }
            55..=69 => {
                let c = rng.below(ids.curves.len() as u64) as usize;
                let end = rng.chance(50);
                let point = if end { ids.ends[c].1 } else { ids.ends[c].0 };
                p.apply(Command::ClearPointGrading {
                    sheet: ids.sheet,
                    point,
                })
                .unwrap_or_else(|e| panic!("{ctx} : {e}"));
                let cv = &mut m.curves[c];
                let v = if end { &mut cv.p3 } else { &mut cv.p0 };
                let b = v[m.base];
                v.iter_mut().for_each(|x| *x = b);
            }
            70..=84 => {
                let s = rng.below(n as u64) as usize;
                let before = bytes(&p);
                let degenerate_there = (0..ids.curves.len()).any(|c| read(m.quad(c, s)).degenerate);
                let r = p.apply(Command::SetBaseSize { size: ids.sizes[s] });
                if degenerate_there {
                    assert!(r.is_err(), "{ctx} : base dégénérée refusée");
                    assert_eq!(bytes(&p), before, "{ctx}");
                } else {
                    r.unwrap_or_else(|e| panic!("{ctx} : {e}"));
                    check_after_base_change(&p, &ids, &m, s);
                    m.base = s;
                }
            }
            85..=92 => {
                assert_eq!(
                    p.shape_control(ids.sheet, SizeRangeId(GHOST)),
                    Err(ShapeControlError::UnknownRange(SizeRangeId(GHOST))),
                    "{ctx}"
                );
                assert_eq!(
                    p.shape_control(SheetId(GHOST), ids.range),
                    Err(ShapeControlError::UnknownSheet(SheetId(GHOST))),
                    "{ctx}"
                );
            }
            _ => {}
        }
        check(&p, &ids, &m, &ctx, st);
    }
}

/// Après un `SetBaseSize` accepté (le projet a déjà la nouvelle base) : la géométrie évaluée de
/// chaque taille est celle du modèle (positions absolues inchangées), donc les lectures aussi.
fn check_after_base_change(after: &Project, ids: &Ids, m: &Model, new_base: usize) {
    let c = after.shape_control(ids.sheet, ids.range).unwrap();
    assert_eq!(c.base, ids.sizes[new_base]);
    for (ci, cv) in c.curves.iter().enumerate() {
        for (s, sz) in cv.sizes.iter().enumerate() {
            let r = read(m.quad(ci, s));
            match sz.reading {
                ShapeReading::Metrics(mt) => {
                    assert!(!r.degenerate);
                    if r.sure {
                        assert_eq!((mt.cusps, mt.inflections), (r.cusps, r.inflections));
                    }
                }
                ShapeReading::Degenerate(_) => assert!(r.degenerate),
            }
        }
    }
}

#[test]
fn adverse_shape_control_campaign() {
    let mut st = Stats::default();
    for seed in 1..=SEQUENCES {
        run(seed, &mut st);
    }
    // La campagne doit réellement exercer chaque signalement et ne pas tout esquiver.
    assert!(st.compared > 5_000, "{}", st.compared);
    assert!(
        st.skipped * 100 <= st.compared * 3,
        "trop de décisions à un seuil près : {} / {}",
        st.skipped,
        st.compared
    );
    assert!(st.cusps_seen > 50, "rebroussements vus : {}", st.cusps_seen);
    assert!(st.cusp_signals > 20, "CuspIntroduced : {}", st.cusp_signals);
    assert!(st.inflection_signals > 20, "{}", st.inflection_signals);
    assert!(st.jump_signals > 20, "{}", st.jump_signals);
}

/// Un changement de base re-exprime les écarts mais ne change aucune lecture par taille : on
/// compare les lectures du cœur avant et après, pour chaque nouvelle base possible.
#[test]
fn adverse_base_change_keeps_readings() {
    let mut checked = 0;
    for seed in 1..=60u64 {
        let mut rng = Rng(seed.wrapping_mul(0xD6E8_FEB8_6659_FD93) | 1);
        let (mut p, ids, mut m) = build(&mut rng);
        let n = ids.sizes.len();
        for s in 0..n {
            if s == m.base {
                continue;
            }
            for c in 0..ids.curves.len() {
                let d = random_delta(&mut rng, &m, c, true, s);
                p.apply(Command::SetPointGrading {
                    sheet: ids.sheet,
                    point: ids.ends[c].1,
                    size: ids.sizes[s],
                    dx_mm: d.0,
                    dy_mm: d.1,
                })
                .unwrap();
                let b = m.curves[c].p3[m.base];
                m.curves[c].p3[s] = (b.0 + d.0, b.1 + d.1);
            }
        }
        let before = p.shape_control(ids.sheet, ids.range).unwrap();
        for s in 0..n {
            if (0..ids.curves.len()).any(|c| read(m.quad(c, s)).degenerate) {
                continue;
            }
            let mut q = p.clone();
            q.apply(Command::SetBaseSize { size: ids.sizes[s] })
                .unwrap();
            let after = q.shape_control(ids.sheet, ids.range).unwrap();
            let (a, b) = (per_size(&before), per_size(&after));
            assert_eq!(a.len(), b.len());
            for (ra, rb) in a.iter().zip(&b) {
                for (x, y) in ra.iter().zip(rb) {
                    assert_eq!((x.0, x.1), (y.0, y.1), "seed {seed} base {s}");
                    assert!((x.2 - y.2).abs() <= 1e-9 * x.2.max(1.0), "seed {seed}");
                }
            }
            assert_eq!(jumps(&before), jumps(&after), "seed {seed} base {s}");
            checked += 1;
        }
    }
    assert!(checked > 100, "{checked}");
}

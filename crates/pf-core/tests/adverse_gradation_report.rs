//! US-09.3a — campagne adverse « report et interpolation de gradation » : 160 séquences à graine
//! fixe de 60 pas (poses d'écarts valides, nuls et invalides ; reports dans les quatre modes ;
//! interpolations ; listes de cibles vides, en double, contenant la source ou une extrémité, des
//! points inconnus, construits, sur calque verrouillé, suivis par une construction maintenue, ou
//! trop près des bornes de l'espace de travail ; extrémités confondues ; annuler, rétablir,
//! écrire-relire).
//!
//! **Oracle indépendant** : l'état attendu (`Model`) est une table `(point, taille) → (dx, dy)`
//! réécrite ici avec ses propres règles. Les symétries sont réécrites (`mirror`), l'interpolation
//! est écrite sous la forme `A + t·(B − A)` (le code testé écrit `(1 − t)·A + t·B`) avec `t` borné
//! par `max` / `min`. Les valeurs de départ sont des multiples de 0,25 mm ; les résultats
//! d'interpolation sont comparés à 1e-9 mm près. Pour un refus, l'oracle liste **toutes** les
//! classes de refus qu'il constate (sans supposer l'ordre des contrôles) : la classe du refus du
//! code doit en faire partie.
//!
//! Invariants après chaque pas : projet valide ; refus = octets identiques et aucune étape ; étape
//! si et seulement si l'état change ; contrôle typé d'accord avec `apply` ; annuler / rétablir
//! exacts à l'octet ; relecture identique ; écarts = ceux de l'oracle (clés rangées, aucun écart
//! nul) ; position évaluée de chaque point libre dans chaque taille = base + écart.
//!
//! Mutations vérifiées à la main : voir `docs/JOURNAL.md` (tranche 96).

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;
use std::collections::BTreeMap;

const SEQUENCES: u64 = 160;
const STEPS: usize = 60;
const GHOST: u64 = 987_654;
const TOL: f64 = 1e-9;

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
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len() as u64) as usize]
    }
}

// ---------------------------------------------------------------------------------------------
// Oracle
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Plain,
    Locked,
    Followed,
    Constructed,
}

#[derive(Clone, Copy, Debug)]
struct Pt {
    id: u64,
    kind: Kind,
    base: (f64, f64),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Cls {
    UnknownPoint,
    UnknownSize,
    Base,
    Number,
    NotFree,
    Locked,
    Followed,
    Workspace,
    Empty,
    Duplicate,
    Source,
    Degenerate,
}

fn class_of(e: &GradingError) -> Cls {
    match e {
        GradingError::UnknownSheet(_) => panic!("feuille inconnue non générée"),
        GradingError::UnknownPoint(_) => Cls::UnknownPoint,
        GradingError::UnknownSize(_) => Cls::UnknownSize,
        GradingError::BaseSize { .. } => Cls::Base,
        GradingError::NotFinite | GradingError::OutOfBounds => Cls::Number,
        GradingError::NotFree { .. } => Cls::NotFree,
        GradingError::Locked { .. } => Cls::Locked,
        GradingError::Followed { .. } => Cls::Followed,
        GradingError::OutOfWorkspace { .. } => Cls::Workspace,
        GradingError::NoTargets => Cls::Empty,
        GradingError::DuplicateTarget { .. } => Cls::Duplicate,
        GradingError::TargetIsSource { .. } => Cls::Source,
        GradingError::DegenerateSegment { .. } => Cls::Degenerate,
        GradingError::NotAGradingCommand => panic!("commande non générée"),
    }
}

/// Nouveaux écarts d'un point : (taille, (dx, dy)).
type Entries = Vec<(u64, (f64, f64))>;

/// Écarts attendus : (point, taille) → (dx, dy), jamais nuls.
type Table = BTreeMap<(u64, u64), (f64, f64)>;

struct World {
    pts: Vec<Pt>,
    /// (taille, est la base).
    sizes: Vec<(u64, bool)>,
}

fn in_range(v: f64) -> bool {
    v.is_finite() && v.abs() <= 1.0e6
}

fn mirror(mode: GradingCopyMode, d: (f64, f64)) -> (f64, f64) {
    let flip = |v: f64| 0.0 - v;
    let (mut x, mut y) = d;
    match mode {
        GradingCopyMode::Same => {}
        GradingCopyMode::MirrorX => x = flip(x),
        GradingCopyMode::MirrorY => y = flip(y),
        GradingCopyMode::Invert => {
            x = flip(x);
            y = flip(y);
        }
    }
    // 0 − 0 = +0 : jamais de zéro négatif.
    (x, y)
}

impl World {
    fn pt(&self, id: u64) -> Option<&Pt> {
        self.pts.iter().find(|p| p.id == id)
    }

    fn size(&self, id: u64) -> Option<bool> {
        self.sizes.iter().find(|s| s.0 == id).map(|s| s.1)
    }

    fn is_free(&self, id: u64) -> bool {
        self.pt(id).is_some_and(|p| p.kind != Kind::Constructed)
    }

    /// Classes de refus d'un point cible qui recevrait ces nouvelles entrées.
    fn target_classes(&self, t: u64, entries: &[(u64, (f64, f64))], out: &mut Vec<Cls>) {
        let Some(p) = self.pt(t) else {
            out.push(Cls::UnknownPoint);
            return;
        };
        match p.kind {
            Kind::Constructed => out.push(Cls::NotFree),
            Kind::Locked => out.push(Cls::Locked),
            Kind::Followed => out.push(Cls::Followed),
            Kind::Plain => {}
        }
        if p.kind != Kind::Constructed
            && entries
                .iter()
                .any(|(_, d)| !in_range(p.base.0 + d.0) || !in_range(p.base.1 + d.1))
        {
            out.push(Cls::Workspace);
        }
    }

    /// Commun au report et à l'interpolation : classes de refus des cibles ; `entries_of` donne les
    /// nouvelles entrées d'une cible.
    fn targets_plan(
        &self,
        targets: &[u64],
        reserved: &[u64],
        entries_of: &dyn Fn(&Pt) -> Entries,
        classes: &mut Vec<Cls>,
    ) -> Vec<(u64, Entries)> {
        let mut plan = Vec::new();
        for (i, t) in targets.iter().enumerate() {
            if targets[..i].contains(t) {
                classes.push(Cls::Duplicate);
            }
            if reserved.contains(t) {
                classes.push(Cls::Source);
            }
            let entries = self.pt(*t).map(entries_of).unwrap_or_default();
            self.target_classes(*t, &entries, classes);
            plan.push((*t, entries));
        }
        plan
    }

    /// `Ok(nouvelle table)` ou `Err(classes de refus constatées)`.
    fn predict(&self, e: &Table, cmd: &Command) -> Result<Table, Vec<Cls>> {
        let mut m = e.clone();
        let mut classes: Vec<Cls> = Vec::new();
        match cmd {
            Command::SetPointGrading {
                point,
                size,
                dx_mm,
                dy_mm,
                ..
            } => {
                let pt = self.pt(point.0);
                if pt.is_none() {
                    classes.push(Cls::UnknownPoint);
                }
                match self.size(size.0) {
                    None => classes.push(Cls::UnknownSize),
                    Some(true) => classes.push(Cls::Base),
                    Some(false) => {}
                }
                if !in_range(*dx_mm) || !in_range(*dy_mm) {
                    classes.push(Cls::Number);
                }
                let zero = *dx_mm == 0.0 && *dy_mm == 0.0;
                if let (Some(p), false) = (pt, zero) {
                    self.target_classes(p.id, &[(size.0, (*dx_mm, *dy_mm))], &mut classes);
                }
                if !classes.is_empty() {
                    return Err(classes);
                }
                if zero {
                    m.remove(&(point.0, size.0));
                } else {
                    m.insert((point.0, size.0), (*dx_mm, *dy_mm));
                }
            }
            Command::ClearPointGrading { point, .. } => {
                if self.pt(point.0).is_none() {
                    return Err(vec![Cls::UnknownPoint]);
                }
                m.retain(|(p, _), _| *p != point.0);
            }
            Command::CopyPointGrading {
                source,
                targets,
                mode,
                ..
            } => {
                let t: Vec<u64> = targets.iter().map(|p| p.0).collect();
                match self.pt(source.0) {
                    None => classes.push(Cls::UnknownPoint),
                    Some(p) if p.kind == Kind::Constructed => classes.push(Cls::NotFree),
                    Some(_) => {}
                }
                if t.is_empty() {
                    classes.push(Cls::Empty);
                }
                let rule: Entries = e
                    .iter()
                    .filter(|((p, _), _)| *p == source.0)
                    .map(|((_, s), d)| (*s, mirror(*mode, *d)))
                    .collect();
                let plan = self.targets_plan(&t, &[source.0], &|_| rule.clone(), &mut classes);
                if !classes.is_empty() {
                    return Err(classes);
                }
                for (p, entries) in plan {
                    m.retain(|(q, _), _| *q != p);
                    for (s, d) in entries {
                        m.insert((p, s), d);
                    }
                }
            }
            Command::InterpolatePointGrading {
                from, to, points, ..
            } => {
                let t: Vec<u64> = points.iter().map(|p| p.0).collect();
                let mut ends = Vec::new();
                for end in [from.0, to.0] {
                    match self.pt(end) {
                        None => classes.push(Cls::UnknownPoint),
                        Some(p) if p.kind == Kind::Constructed => classes.push(Cls::NotFree),
                        Some(p) => ends.push(*p),
                    }
                }
                if t.is_empty() {
                    classes.push(Cls::Empty);
                }
                let mut line = None;
                if ends.len() == 2 {
                    let (a, b) = (ends[0].base, ends[1].base);
                    let (ux, uy) = (b.0 - a.0, b.1 - a.1);
                    let len2 = ux * ux + uy * uy;
                    if from.0 == to.0 || len2.sqrt() < 1e-9 {
                        classes.push(Cls::Degenerate);
                    } else {
                        line = Some((a, (ux, uy), len2));
                    }
                }
                let all_sizes: Vec<u64> = self
                    .sizes
                    .iter()
                    .map(|s| s.0)
                    .filter(|s| e.contains_key(&(from.0, *s)) || e.contains_key(&(to.0, *s)))
                    .collect();
                let entries_of = |p: &Pt| -> Entries {
                    let Some((a, u, len2)) = line else {
                        return Vec::new();
                    };
                    // Projection orthogonale de la base du point sur A → B, bornée à [0 ; 1].
                    let t =
                        (((p.base.0 - a.0) * u.0 + (p.base.1 - a.1) * u.1) / len2).clamp(0.0, 1.0);
                    all_sizes
                        .iter()
                        .filter_map(|s| {
                            let da = e.get(&(from.0, *s)).copied().unwrap_or((0.0, 0.0));
                            let db = e.get(&(to.0, *s)).copied().unwrap_or((0.0, 0.0));
                            let snap = |v: f64| if v.abs() < 1e-9 { 0.0 } else { v };
                            let d = (
                                snap(da.0 + t * (db.0 - da.0)),
                                snap(da.1 + t * (db.1 - da.1)),
                            );
                            (d != (0.0, 0.0)).then_some((*s, d))
                        })
                        .collect()
                };
                let plan = self.targets_plan(&t, &[from.0, to.0], &entries_of, &mut classes);
                if !classes.is_empty() {
                    return Err(classes);
                }
                for (p, entries) in plan {
                    m.retain(|(q, _), _| *q != p);
                    for (s, d) in entries {
                        m.insert((p, s), d);
                    }
                }
            }
            _ => unreachable!("commande non générée"),
        }
        Ok(m)
    }
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).expect("projet valide")
}

// ---------------------------------------------------------------------------------------------
// Mise en place et générateurs
// ---------------------------------------------------------------------------------------------

fn setup(rng: &mut Rng) -> (Project, World, SheetId) {
    let mut p = Project::new("Adverse report");
    let sheet = p.sheets[0].id;
    let mut sizes = Vec::new();
    for (name, labels, base) in [
        ("Femme", vec!["XS", "S", "M", "L", "XL"], 2usize),
        ("Tour", vec!["34", "36", "38"], 1usize),
    ] {
        let out = p
            .apply(Command::AddSizeRange {
                name: name.into(),
                kind: SizeKind::Alphanumeric,
                labels: labels.iter().map(|s| s.to_string()).collect(),
                base,
            })
            .unwrap();
        for (i, s) in out.sizes.iter().enumerate() {
            sizes.push((s.0, i == base));
        }
    }
    let mut pts = Vec::new();
    let add = |p: &mut Project, x: f64, y: f64| {
        p.apply(Command::AddPoint {
            sheet,
            at: PointMm::new(x, y),
        })
        .unwrap()
        .points[0]
    };
    for i in 0..8u64 {
        let (x, y) = match i {
            6 => (990_000.0, 100.0),
            7 => (-990_000.25, 300.5),
            _ => (
                i as f64 * 57.25 + rng.below(40) as f64 * 0.25,
                rng.below(2000) as f64 * 0.25 - 200.0,
            ),
        };
        let id = add(&mut p, x, y);
        pts.push(Pt {
            id: id.0,
            kind: Kind::Plain,
            base: (x, y),
        });
    }
    // Un point libre confondu (par déplacement) avec le premier : extrémités de même position.
    let twin = add(&mut p, 1234.5, 77.0);
    p.apply(Command::MovePoint {
        sheet,
        point: twin,
        to: PointMm::new(pts[0].base.0, pts[0].base.1),
    })
    .unwrap();
    pts.push(Pt {
        id: twin.0,
        kind: Kind::Plain,
        base: pts[0].base,
    });
    // Un point libre sur un calque verrouillé.
    let layer = p
        .apply(Command::AddLayer {
            sheet,
            name: "Verrou".into(),
        })
        .unwrap()
        .layers[0];
    let locked = p
        .apply(Command::InLayer {
            sheet,
            layer,
            command: Box::new(Command::AddPoint {
                sheet,
                at: PointMm::new(3000.0, 3000.0),
            }),
        })
        .unwrap()
        .points[0];
    p.apply(Command::SetLayerLocked {
        sheet,
        layer,
        locked: true,
    })
    .unwrap();
    pts.push(Pt {
        id: locked.0,
        kind: Kind::Locked,
        base: (3000.0, 3000.0),
    });
    // Un point libre suivi par une construction maintenue.
    let seg = p
        .apply(Command::AddSegmentPolar {
            sheet,
            from: PointRef::New(PointMm::new(4000.0, 4000.0)),
            length_mm: 80.0,
            angle_deg: 30.0,
        })
        .unwrap();
    let followed = *seg.points.last().unwrap();
    p.apply(Command::SetConstructionKept {
        sheet,
        segment: seg.entities[0],
        kept: true,
    })
    .unwrap();
    let PointDefinition::Free { x, y } =
        p.sheet(sheet).unwrap().point(followed).unwrap().definition
    else {
        panic!("point suivi libre")
    };
    pts.push(Pt {
        id: followed.0,
        kind: Kind::Followed,
        base: (x, y),
    });
    // Un point construit (décalé du premier).
    let off = p
        .apply(Command::AddPointAt {
            sheet,
            point: PointRef::Derived(PointDefinition::Offset {
                from: PointId(pts[0].id),
                dx_mm: 12.5,
                dy_mm: -4.0,
            }),
        })
        .unwrap()
        .points[0];
    pts.push(Pt {
        id: off.0,
        kind: Kind::Constructed,
        base: (0.0, 0.0),
    });
    (p, World { pts, sizes }, sheet)
}

fn gen_value(rng: &mut Rng) -> f64 {
    if rng.chance(80) {
        return (rng.below(81) as f64 - 40.0) * 0.25 * if rng.chance(30) { 8.0 } else { 1.0 };
    }
    match rng.below(9) {
        0 => 0.0,
        1 => f64::NAN,
        2 => f64::INFINITY,
        3 => 1_000_000.25,
        4 => (rng.below(41) as f64 - 20.0) * 2500.0,
        5 => (rng.below(41) as f64 - 20.0) * 500.0,
        _ => 0.0,
    }
}

fn gen_size(rng: &mut Rng, w: &World) -> SizeId {
    if rng.chance(4) {
        SizeId(GHOST)
    } else {
        SizeId(rng.pick(&w.sizes).0)
    }
}

fn gen_point(rng: &mut Rng, w: &World) -> PointId {
    if rng.chance(4) {
        return PointId(GHOST);
    }
    // Surtout des points ordinaires : les reports doivent aboutir assez souvent.
    let plain: Vec<u64> = w
        .pts
        .iter()
        .filter(|p| p.kind == Kind::Plain)
        .map(|p| p.id)
        .collect();
    if rng.chance(10) {
        // Près des bornes de l'espace de travail.
        PointId(w.pts[6 + rng.below(2) as usize].id)
    } else if rng.chance(86) {
        PointId(*rng.pick(&plain))
    } else {
        PointId(rng.pick(&w.pts).id)
    }
}

fn gen_points(rng: &mut Rng, w: &World, special: &[PointId]) -> Vec<PointId> {
    if rng.chance(3) {
        return Vec::new();
    }
    let n = 1 + rng.below(4) as usize;
    let mut out: Vec<PointId> = (0..n).map(|_| gen_point(rng, w)).collect();
    // Doublon, ou source / extrémité dans la liste.
    if rng.chance(6) {
        out.push(out[0]);
    }
    if rng.chance(6) {
        out.push(*rng.pick(special));
    }
    out
}

fn gen_command(rng: &mut Rng, w: &World, sheet: SheetId) -> Command {
    match rng.below(100) {
        0..=29 => Command::SetPointGrading {
            sheet,
            point: gen_point(rng, w),
            size: gen_size(rng, w),
            dx_mm: gen_value(rng),
            dy_mm: gen_value(rng),
        },
        30..=33 => Command::ClearPointGrading {
            sheet,
            point: gen_point(rng, w),
        },
        34..=61 => {
            let source = gen_point(rng, w);
            Command::CopyPointGrading {
                sheet,
                source,
                targets: gen_points(rng, w, &[source]),
                mode: *rng.pick(&GradingCopyMode::ALL),
            }
        }
        _ => {
            let from = gen_point(rng, w);
            let to = if rng.chance(4) {
                from
            } else if rng.chance(8) {
                // L'extrémité confondue avec la première (position de base identique).
                PointId(w.pts[8].id)
            } else {
                gen_point(rng, w)
            };
            Command::InterpolatePointGrading {
                sheet,
                from,
                to,
                points: gen_points(rng, w, &[from, to]),
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Invariants
// ---------------------------------------------------------------------------------------------

fn check_state(p: &Project, w: &World, sheet: SheetId, expected: &Table, ctx: &str) {
    p.validate().unwrap_or_else(|e| panic!("{ctx} : {e}"));
    let s = p.sheet(sheet).unwrap();
    assert_eq!(s.gradings.len(), expected.len(), "{ctx} : nombre d'écarts");
    let keys: Vec<(u64, u64)> = s.gradings.iter().map(|g| (g.point.0, g.size.0)).collect();
    let mut sorted = keys.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(keys, sorted, "{ctx} : écarts non rangés ou en double");
    for g in &s.gradings {
        let want = expected
            .get(&(g.point.0, g.size.0))
            .unwrap_or_else(|| panic!("{ctx} : écart inattendu {:?}", (g.point.0, g.size.0)));
        assert!(
            (g.dx_mm - want.0).abs() < TOL && (g.dy_mm - want.1).abs() < TOL,
            "{ctx} : écart ({}, {}) {:?} != {:?}",
            g.point.0,
            g.size.0,
            (g.dx_mm, g.dy_mm),
            want
        );
        assert!(g.dx_mm != 0.0 || g.dy_mm != 0.0, "{ctx} : écart nul stocké");
        assert!(
            g.dx_mm.is_sign_positive() || g.dx_mm != 0.0,
            "{ctx} : zéro négatif"
        );
        assert!(
            g.dy_mm.is_sign_positive() || g.dy_mm != 0.0,
            "{ctx} : zéro négatif"
        );
        assert_eq!(w.size(g.size.0), Some(false), "{ctx} : écart sur une base");
        assert!(w.is_free(g.point.0), "{ctx} : écart sur un point construit");
    }
    // Position évaluée d'un point libre dans chaque taille = base + écart.
    for (size, _) in &w.sizes {
        let ev = p.evaluate_sheet(sheet, Some(SizeId(*size))).unwrap();
        for pt in w.pts.iter().filter(|q| q.kind != Kind::Constructed) {
            let d = expected.get(&(pt.id, *size)).copied().unwrap_or((0.0, 0.0));
            let got = ev.points.iter().find(|(i, _)| i.0 == pt.id);
            // Un point suivi par une construction maintenue n'est jamais gradué : base seule.
            let got = got
                .unwrap_or_else(|| panic!("{ctx} : point {} absent", pt.id))
                .1;
            assert!(
                (got.x - (pt.base.0 + d.0)).abs() < TOL && (got.y - (pt.base.1 + d.1)).abs() < TOL,
                "{ctx} : point {} en taille {size}",
                pt.id
            );
        }
    }
    let text = bytes(p);
    let back = persist::from_json(&text).unwrap_or_else(|e| panic!("{ctx} : relecture : {e}"));
    assert_eq!(&back, p, "{ctx} : relecture différente");
    assert_eq!(bytes(&back), text, "{ctx} : réécriture différente");
}

#[derive(Default)]
struct Counters {
    steps: usize,
    accepted: usize,
    refused: usize,
    noops: usize,
    copies_ok: usize,
    copies_by_mode: [usize; 4],
    copies_multi: usize,
    interps_ok: usize,
    interps_clamped_low: usize,
    interps_clamped_high: usize,
    interps_inside: usize,
    interps_zero_omitted: usize,
    refusals: BTreeMap<Cls, usize>,
    undos: usize,
    redos: usize,
    reloads: usize,
    max_entries: usize,
}

fn run_sequence(seed: u64, c: &mut Counters) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let (start, w, sheet) = setup(&mut rng);
    let mut doc = Document::new(start);
    let mut model: Table = Table::new();
    let mut history: Vec<String> = vec![bytes(doc.project())];
    let mut models: Vec<Table> = vec![model.clone()];
    let mut cursor = 0usize;
    check_state(doc.project(), &w, sheet, &model, "départ");

    for step in 0..STEPS {
        c.steps += 1;
        let ctx = format!("séquence {seed}, pas {step}");
        let before = doc.project().clone();
        let before_bytes = bytes(&before);
        match rng.below(100) {
            0..=7 => {
                let expect = cursor > 0;
                assert_eq!(doc.undo(), expect, "{ctx} : annuler");
                if expect {
                    cursor -= 1;
                    c.undos += 1;
                }
                assert_eq!(bytes(doc.project()), history[cursor], "{ctx} : annuler");
                model = models[cursor].clone();
            }
            8..=13 => {
                let expect = cursor + 1 < history.len();
                assert_eq!(doc.redo(), expect, "{ctx} : rétablir");
                if expect {
                    cursor += 1;
                    c.redos += 1;
                }
                assert_eq!(bytes(doc.project()), history[cursor], "{ctx} : rétablir");
                model = models[cursor].clone();
            }
            14..=17 => {
                let back = persist::from_json(&before_bytes).expect("relecture");
                assert_eq!(back, before, "{ctx} : relecture");
                c.reloads += 1;
            }
            _ => {
                let cmd = gen_command(&mut rng, &w, sheet);
                let predicted = w.predict(&model, &cmd);
                let typed = before.check_grading_command(&cmd);
                let result = doc.execute(cmd.clone());
                let after_bytes = bytes(doc.project());
                match (predicted, &result) {
                    (Err(classes), Err(e)) => {
                        c.refused += 1;
                        let typed = typed.expect_err(&format!("{ctx} : contrôle typé accepte"));
                        assert_eq!(e.to_string(), typed.to_string(), "{ctx}");
                        let class = class_of(&typed);
                        assert!(
                            classes.contains(&class),
                            "{ctx} : refus {class:?} hors de {classes:?} pour {cmd:?}"
                        );
                        *c.refusals.entry(class).or_default() += 1;
                        assert_eq!(after_bytes, before_bytes, "{ctx} : refus avec trace");
                    }
                    (Ok(next), Ok(_)) => {
                        c.accepted += 1;
                        assert!(typed.is_ok(), "{ctx} : contrôle typé refuse {cmd:?}");
                        match &cmd {
                            Command::CopyPointGrading { mode, targets, .. } => {
                                c.copies_ok += 1;
                                c.copies_by_mode[GradingCopyMode::ALL
                                    .iter()
                                    .position(|m| m == mode)
                                    .unwrap()] += 1;
                                if targets.len() > 1 {
                                    c.copies_multi += 1;
                                }
                            }
                            Command::InterpolatePointGrading {
                                from, to, points, ..
                            } => {
                                c.interps_ok += 1;
                                let (a, b) = (w.pt(from.0).unwrap().base, w.pt(to.0).unwrap().base);
                                for p in points {
                                    let q = w.pt(p.0).unwrap().base;
                                    let (ux, uy) = (b.0 - a.0, b.1 - a.1);
                                    let raw =
                                        ((q.0 - a.0) * ux + (q.1 - a.1) * uy) / (ux * ux + uy * uy);
                                    if raw < 0.0 {
                                        c.interps_clamped_low += 1;
                                    } else if raw > 1.0 {
                                        c.interps_clamped_high += 1;
                                    } else {
                                        c.interps_inside += 1;
                                    }
                                    let has_input =
                                        model.keys().any(|(pt, _)| *pt == from.0 || *pt == to.0);
                                    let got = next.keys().filter(|(pt, _)| *pt == p.0).count();
                                    let inputs = w
                                        .sizes
                                        .iter()
                                        .filter(|(s, _)| {
                                            model.contains_key(&(from.0, *s))
                                                || model.contains_key(&(to.0, *s))
                                        })
                                        .count();
                                    if has_input && got < inputs {
                                        c.interps_zero_omitted += 1;
                                    }
                                }
                            }
                            _ => {}
                        }
                        model = next;
                        if after_bytes == before_bytes {
                            c.noops += 1;
                        } else {
                            history.truncate(cursor + 1);
                            models.truncate(cursor + 1);
                            history.push(after_bytes);
                            models.push(model.clone());
                            cursor += 1;
                        }
                    }
                    (p, r) => panic!("{ctx} : oracle {p:?} / code {r:?} pour {cmd:?}"),
                }
            }
        }
        check_state(doc.project(), &w, sheet, &model, &ctx);
        assert_eq!(doc.can_undo(), cursor > 0, "{ctx} : can_undo");
        assert_eq!(
            doc.can_redo(),
            cursor + 1 < history.len(),
            "{ctx} : can_redo"
        );
        c.max_entries = c.max_entries.max(model.len());
    }
    while cursor > 0 {
        assert!(doc.undo());
        cursor -= 1;
        assert_eq!(bytes(doc.project()), history[cursor]);
    }
    assert!(!doc.can_undo());
    while cursor + 1 < history.len() {
        assert!(doc.redo());
        cursor += 1;
        assert_eq!(bytes(doc.project()), history[cursor]);
    }
}

#[test]
fn campagne_adverse_report_et_interpolation_de_gradation() {
    let mut c = Counters::default();
    for seed in 1..=SEQUENCES {
        run_sequence(seed, &mut c);
    }
    eprintln!(
        "report : {} pas, {} acceptés ({} sans effet), {} refusés ; {} reports (modes {:?}, {} \
         multi-cibles) ; {} interpolations (t<0 : {}, t>1 : {}, dedans : {}, avec écart nul omis \
         : {}) ; refus {:?} ; {} annuler, {} rétablir, {} relectures, max {} écarts",
        c.steps,
        c.accepted,
        c.noops,
        c.refused,
        c.copies_ok,
        c.copies_by_mode,
        c.copies_multi,
        c.interps_ok,
        c.interps_clamped_low,
        c.interps_clamped_high,
        c.interps_inside,
        c.interps_zero_omitted,
        c.refusals,
        c.undos,
        c.redos,
        c.reloads,
        c.max_entries
    );
    assert!(c.accepted > 1500, "trop peu d'acceptés : {}", c.accepted);
    assert!(c.refused > 800, "trop peu de refus : {}", c.refused);
    assert!(
        c.noops > 100,
        "trop peu de commandes sans effet : {}",
        c.noops
    );
    assert!(c.copies_ok > 300, "reports acceptés : {}", c.copies_ok);
    assert!(
        c.copies_by_mode.iter().all(|n| *n > 50),
        "modes : {:?}",
        c.copies_by_mode
    );
    assert!(
        c.copies_multi > 100,
        "reports multi-cibles : {}",
        c.copies_multi
    );
    assert!(c.interps_ok > 300, "interpolations : {}", c.interps_ok);
    assert!(c.interps_clamped_low > 50 && c.interps_clamped_high > 50);
    assert!(c.interps_inside > 100, "t dedans : {}", c.interps_inside);
    for class in [
        Cls::UnknownPoint,
        Cls::NotFree,
        Cls::Locked,
        Cls::Followed,
        Cls::Workspace,
        Cls::Empty,
        Cls::Duplicate,
        Cls::Source,
        Cls::Degenerate,
        Cls::Base,
        Cls::Number,
        Cls::UnknownSize,
    ] {
        assert!(
            c.refusals.get(&class).copied().unwrap_or(0) > 5,
            "classe de refus {class:?} peu exercée : {:?}",
            c.refusals
        );
    }
    assert!(c.undos > 200 && c.redos > 50 && c.reloads > 200);
    assert!(c.max_entries >= 10);
}

//! Campagne adverse US-10.3 / US-10.4 : 200 séquences à graine fixe de 30 pas (commandes valides,
//! invalides, fantômes, suppression de mesure / couture / cote, unités incohérentes, valeurs non
//! finies ou hors bornes).
//!
//! L'oracle est un modèle de données écrit ici ; il ne lit ni le code des commandes ni celui des
//! lectures. Les valeurs mesurées sont des **formules fermées** du rectangle gradué (largeur
//! `400 + dx`, hauteur `600 + dy`, angle 90°, segment collé en première taille) ; le barème, les
//! retouches (somme des deltas, bornes 0 à 1 000 000), les écarts signés et les statuts sont
//! recalculés par force brute, **tous les nombres étant exactement représentables** (entiers et
//! demis) : l'oracle compare sans tolérance. Un refus laisse les octets identiques ; les
//! compteurs prouvent que chaque contrôle fort s'est exécuté.

#![allow(clippy::needless_range_loop)]

use pf_core::persist;
use pf_core::*;
use pf_geometry::{distance_mm, PointMm};
use std::collections::{BTreeMap, BTreeSet};

const N: usize = 5;
const SIZES: [&str; N] = ["XS", "S", "M", "L", "XL"];

struct R(u64);
impl R {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        ((self.next() >> 11) as usize) % n
    }
    fn chance(&mut self, pct: usize) -> bool {
        self.below(100) < pct
    }
    fn int(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as usize) as i64
    }
}

/// Ce que mesure une cote de l'oracle.
#[derive(Clone, Copy, Debug, PartialEq)]
enum DimKind {
    /// Longueur du tracé AB = largeur.
    Width,
    /// Horizontale B → A : ΔX = −largeur, lue en valeur absolue.
    HRev,
    /// Verticale D → A : ΔY = −hauteur, lue en valeur absolue.
    VRev,
    /// Angle en A (de B vers D) = 90°.
    Angle,
    /// Segment de 100 mm réduit à un point en première taille.
    Short,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Src {
    Seam(u64),
    Dim(u64),
}

#[derive(Clone, Debug)]
struct OM {
    id: u64,
    name: String,
    deg: bool,
    vals: BTreeMap<usize, f64>,
}
#[derive(Clone, Debug)]
struct OA {
    id: u64,
    name: String,
    target: u64,
    delta: f64,
}
#[derive(Clone, Debug)]
struct OF {
    id: u64,
    name: String,
    m: u64,
    src: Src,
    tm: f64,
    tp: f64,
}

#[derive(Clone, Debug)]
struct Oracle {
    ms: Vec<OM>,
    alts: Vec<OA>,
    fins: Vec<OF>,
    dims: Vec<(u64, DimKind)>,
    seam: Option<u64>,
    w: [f64; N],
    h: [f64; N],
    sizes: Vec<u64>,
    ever: BTreeSet<u64>,
}

#[derive(Default, Debug)]
struct Counters {
    accepted: usize,
    refused: usize,
    bytes_checked: usize,
    alt_adds: usize,
    alt_edits: usize,
    fin_adds: usize,
    fin_sets: usize,
    cascade_measure: usize,
    cascade_source: usize,
    unit_refused: usize,
    ghost_refused: usize,
    bad_values: usize,
    readings: usize,
    conforming: usize,
    out: usize,
    boundary: usize,
    na_reference: usize,
    na_source: usize,
    na_alteration: usize,
    with_alt_differs: usize,
    altered_issue_nobase: usize,
    altered_issue_range: usize,
    copies_checked: usize,
}

fn trimmed(n: &str) -> Option<String> {
    let t = n.trim();
    (!t.is_empty()).then(|| t.to_string())
}

fn good_value(v: f64) -> bool {
    v.is_finite() && (0.0..=1_000_000.0).contains(&v)
}

fn good_delta(v: f64) -> bool {
    v.is_finite() && v.abs() <= 1_000_000.0
}

const NAMES: [&str; 12] = [
    "Largeur", "largeur", "LARGEUR", "Hauteur", "Angle", "Manche", "Épaule", "  Cou  ", "", "   ",
    "Bras", "Jambe",
];
const TOLS: [f64; 11] = [
    0.0,
    0.5,
    1.0,
    2.0,
    3.0,
    5.0,
    -1.0,
    f64::NAN,
    f64::INFINITY,
    1_000_000.0,
    1_000_001.0,
];
const DELTAS: [f64; 14] = [
    0.0,
    1.0,
    -1.0,
    2.5,
    -2.0,
    15.0,
    -15.0,
    -400.0,
    400.0,
    1_000_000.0,
    -1_000_000.0,
    1_000_001.0,
    f64::NAN,
    f64::INFINITY,
];

fn ghost(r: &mut R) -> u64 {
    9_000_000 + r.below(1000) as u64
}

fn pt(x: f64, y: f64) -> PointMm {
    PointMm::new(x, y)
}

fn point_at(p: &Project, sheet: SheetId, at: PointMm) -> PointId {
    let s = p.sheet(sheet).unwrap();
    s.points
        .iter()
        .find(|q| distance_mm(s.position(q.id).unwrap(), at) < 1e-9)
        .unwrap()
        .id
}

fn seg(p: &mut Project, sheet: SheetId, a: PointMm, b: PointMm) -> EntityId {
    p.apply(Command::AddSegment {
        sheet,
        from: PointRef::New(a),
        to: PointRef::New(b),
    })
    .unwrap()
    .entities[0]
}

/// Géométrie commune : tout ce qu'il faut pour recréer couture et cotes pendant la séquence.
struct Geo {
    sheet: SheetId,
    ab: EntityId,
    short: EntityId,
    ia: PointId,
    ib: PointId,
    id: PointId,
    piece: PieceId,
    bc: EntityId,
    dos: PieceId,
    vertical: EntityId,
}

fn dim_kind(g: &Geo, k: DimKind) -> FullDimensionKind {
    match k {
        DimKind::Width => FullDimensionKind::PathLength {
            entity: g.ab,
            offset_mm: 10.0,
        },
        DimKind::HRev => FullDimensionKind::Horizontal {
            from: g.ib,
            to: g.ia,
            offset_mm: 10.0,
        },
        DimKind::VRev => FullDimensionKind::Vertical {
            from: g.id,
            to: g.ia,
            offset_mm: 10.0,
        },
        DimKind::Angle => FullDimensionKind::Angle {
            vertex: g.ia,
            first: g.ib,
            second: g.id,
            radius_mm: 20.0,
        },
        DimKind::Short => FullDimensionKind::PathLength {
            entity: g.short,
            offset_mm: 5.0,
        },
    }
}

fn add_dim(p: &mut Project, g: &Geo, k: DimKind) -> u64 {
    p.apply(Command::AddFullDimension {
        sheet: g.sheet,
        kind: dim_kind(g, k),
        label: None,
        text_shift_mm: 0.0,
    })
    .unwrap()
    .full_dimensions[0]
        .0
}

fn new_project(r: &mut R) -> (Project, Oracle, Geo) {
    let mut p = Project::new("Adverse retouches");
    let sheet = p.sheets[0].id;
    p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: SIZES.iter().map(|s| s.to_string()).collect(),
        base: 2,
    })
    .unwrap();
    let sizes: Vec<SizeId> = p.sizes[0].sizes.iter().map(|s| s.id).collect();
    let (a, b, c, d) = (
        pt(0.0, 0.0),
        pt(400.0, 0.0),
        pt(400.0, 600.0),
        pt(0.0, 600.0),
    );
    let ab = seg(&mut p, sheet, a, b);
    let bc = seg(&mut p, sheet, b, c);
    let cd = seg(&mut p, sheet, c, d);
    let da = seg(&mut p, sheet, d, a);
    let piece = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths {
                entities: vec![ab, bc, cd, da],
            },
            name: "Devant".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let (ia, ib, ic, id) = (
        point_at(&p, sheet, a),
        point_at(&p, sheet, b),
        point_at(&p, sheet, c),
        point_at(&p, sheet, d),
    );
    let mut w = [400.0; N];
    let mut h = [600.0; N];
    for s in 0..N {
        if s == 2 {
            continue;
        }
        let (dx, dy) = (r.int(-20, 20), r.int(-20, 20));
        w[s] += dx as f64;
        h[s] += dy as f64;
        for (point, x, y) in [(ib, dx, 0), (ic, dx, dy), (id, 0, dy)] {
            p.apply(Command::SetPointGrading {
                sheet,
                point,
                size: sizes[s],
                dx_mm: x as f64,
                dy_mm: y as f64,
            })
            .unwrap();
        }
    }
    let o = p
        .apply(Command::AddRectangle {
            sheet,
            corner: PointRef::New(pt(1000.0, 0.0)),
            width_mm: 400.0,
            height_mm: 600.0,
        })
        .unwrap();
    let dos = p
        .apply(Command::ExtractPiece {
            sheet,
            source: PieceSource::Paths {
                entities: o.entities.clone(),
            },
            name: "Dos".into(),
            detached: false,
        })
        .unwrap()
        .pieces[0];
    let vertical = o
        .entities
        .iter()
        .copied()
        .find(|e| {
            p.sheet(sheet)
                .unwrap()
                .segment_ends(*e)
                .is_ok_and(|(u, v)| (u.x - v.x).abs() < 1e-9 && (u.y - v.y).abs() > 1.0)
        })
        .unwrap();
    // Segment de 100 mm collé en première taille.
    let short = seg(&mut p, sheet, pt(2000.0, 0.0), pt(2100.0, 0.0));
    let q = point_at(&p, sheet, pt(2100.0, 0.0));
    p.apply(Command::SetPointGrading {
        sheet,
        point: q,
        size: sizes[0],
        dx_mm: -100.0,
        dy_mm: 0.0,
    })
    .unwrap();
    let g = Geo {
        sheet,
        ab,
        short,
        ia,
        ib,
        id,
        piece,
        bc,
        dos,
        vertical,
    };
    let mut o = Oracle {
        ms: vec![],
        alts: vec![],
        fins: vec![],
        dims: vec![],
        seam: None,
        w,
        h,
        sizes: sizes.iter().map(|s| s.0).collect(),
        ever: BTreeSet::new(),
    };
    for k in [
        DimKind::Width,
        DimKind::HRev,
        DimKind::VRev,
        DimKind::Angle,
        DimKind::Short,
    ] {
        let id = add_dim(&mut p, &g, k);
        o.dims.push((id, k));
    }
    o.seam = Some(add_seam(&mut p, &g).expect("couture").0);
    // Trois mesures de départ (valeurs proches des mesurées pour que les deux statuts arrivent).
    for (name, deg) in [("Largeur", false), ("Hauteur", false), ("Angle", true)] {
        let id = p
            .apply(Command::AddMeasurement {
                name: name.into(),
                unit: if deg {
                    MeasurementUnit::Deg
                } else {
                    MeasurementUnit::Mm
                },
            })
            .unwrap()
            .measurements[0]
            .0;
        o.ever.insert(id);
        let mut vals = BTreeMap::new();
        for s in 0..N {
            let base = if deg {
                90.0
            } else if name == "Largeur" {
                w[s]
            } else {
                h[s]
            };
            let v = base + r.int(-6, 6) as f64;
            vals.insert(s, v);
            p.apply(Command::SetMeasurementValue {
                measurement: MeasurementId(id),
                size: sizes[s],
                value: v,
            })
            .unwrap();
        }
        o.ms.push(OM {
            id,
            name: name.into(),
            deg,
            vals,
        });
    }
    (p, o, g)
}

fn add_seam(p: &mut Project, g: &Geo) -> Result<SeamPairId, CoreError> {
    let a = p.seam_side_range(g.piece, g.bc, 1)?;
    let b = p.seam_side_range(g.dos, g.vertical, 1)?;
    let out = p.apply(Command::AddSeamPair {
        a,
        b,
        embu_mm: None,
        tolerance_mm: None,
    })?;
    Ok(out.seam_pairs[0])
}

impl Oracle {
    fn dup(names: Vec<(u64, &str)>, name: &str, except: Option<u64>) -> bool {
        names
            .into_iter()
            .any(|(id, n)| Some(id) != except && n.to_lowercase() == name.to_lowercase())
    }
    fn m(&self, id: u64) -> Option<&OM> {
        self.ms.iter().find(|m| m.id == id)
    }
    fn dim_kind_of(&self, id: u64) -> Option<DimKind> {
        self.dims.iter().find(|d| d.0 == id).map(|d| d.1)
    }
    fn src_alive(&self, s: Src) -> bool {
        match s {
            Src::Seam(id) => self.seam == Some(id),
            Src::Dim(id) => self.dim_kind_of(id).is_some(),
        }
    }
    fn src_deg(&self, s: Src) -> bool {
        matches!(s, Src::Dim(id) if self.dim_kind_of(id) == Some(DimKind::Angle))
    }
    /// Valeur mesurée par la source dans la taille `s` (`None` = dégénérée).
    fn measured(&self, src: Src, s: usize) -> Option<f64> {
        match src {
            Src::Seam(_) => Some(self.h[s]),
            Src::Dim(id) => match self.dim_kind_of(id).unwrap() {
                DimKind::Width | DimKind::HRev => Some(self.w[s]),
                DimKind::VRev => Some(self.h[s]),
                DimKind::Angle => Some(90.0),
                DimKind::Short => (s != 0).then_some(100.0),
            },
        }
    }
    fn fin_ok(&self, name: &str, m: u64, src: Src, tm: f64, tp: f64, except: Option<u64>) -> bool {
        trimmed(name).is_some_and(|n| {
            !Oracle::dup(
                self.fins.iter().map(|f| (f.id, f.name.as_str())).collect(),
                &n,
                except,
            )
        }) && self.m(m).is_some_and(|om| om.deg == self.src_deg(src))
            && self.src_alive(src)
            && good_value(tm)
            && good_value(tp)
    }
    /// (base, retouché, nombre, issue : 1 = sans valeur, 2 = hors bornes) pour la mesure `m`.
    fn altered(&self, m: u64, s: usize) -> (Option<f64>, Option<f64>, usize, f64, u8) {
        let base = self.m(m).unwrap().vals.get(&s).copied();
        let mut delta = 0.0;
        let mut count = 0;
        for a in self.alts.iter().filter(|a| a.target == m) {
            delta += a.delta;
            count += 1;
        }
        if count == 0 {
            return (base, base, 0, 0.0, 0);
        }
        match base {
            None => (None, None, count, delta, 1),
            Some(b) => {
                let v = b + delta;
                if (0.0..=1_000_000.0).contains(&v) {
                    (Some(b), Some(v), count, delta, 0)
                } else {
                    (Some(b), None, count, delta, 2)
                }
            }
        }
    }
}

type Update = Box<dyn FnOnce(&mut Oracle, &Outcome, &mut Counters)>;

fn pick_m(r: &mut R, o: &Oracle) -> u64 {
    if o.ms.is_empty() || r.chance(8) {
        ghost(r)
    } else {
        o.ms[r.below(o.ms.len())].id
    }
}

fn pick_a(r: &mut R, o: &Oracle) -> u64 {
    if o.alts.is_empty() || r.chance(8) {
        ghost(r)
    } else {
        o.alts[r.below(o.alts.len())].id
    }
}

fn pick_f(r: &mut R, o: &Oracle) -> u64 {
    if o.fins.is_empty() || r.chance(8) {
        ghost(r)
    } else {
        o.fins[r.below(o.fins.len())].id
    }
}

fn pick_src(r: &mut R, o: &Oracle) -> Src {
    if r.chance(10) {
        return if r.chance(50) {
            Src::Seam(ghost(r))
        } else {
            Src::Dim(ghost(r))
        };
    }
    let mut all: Vec<Src> = o.dims.iter().map(|d| Src::Dim(d.0)).collect();
    if let Some(s) = o.seam {
        all.push(Src::Seam(s));
    }
    if all.is_empty() {
        return Src::Dim(ghost(r));
    }
    all[r.below(all.len())]
}

fn to_source(g: &Geo, s: Src) -> FinishedSource {
    match s {
        Src::Seam(id) => FinishedSource::Seam {
            seam: SeamPairId(id),
        },
        Src::Dim(id) => FinishedSource::FullDimension {
            sheet: g.sheet,
            dimension: FullDimensionId(id),
        },
    }
}

fn value_for(r: &mut R, o: &Oracle, deg: bool, s: usize) -> f64 {
    if r.chance(15) {
        return [
            0.0,
            1_000_000.0,
            1_000_000.5,
            -1.0,
            f64::NAN,
            f64::INFINITY,
            0.5,
        ][r.below(7)];
    }
    let base = if deg {
        90.0
    } else if r.chance(50) {
        o.w[s]
    } else {
        o.h[s]
    };
    base + r.int(-6, 6) as f64
}

fn step(p: &mut Project, o: &mut Oracle, g: &Geo, r: &mut R, c: &mut Counters) {
    let op = r.below(32);
    let before = persist::to_json(p).unwrap();
    let after: Update;
    let cmd: Command;
    let expect: bool;
    match op {
        0..=1 => {
            let name = NAMES[r.below(NAMES.len())].to_string();
            let deg = r.chance(25);
            expect = trimmed(&name).is_some_and(|n| {
                !Oracle::dup(
                    o.ms.iter().map(|m| (m.id, m.name.as_str())).collect(),
                    &n,
                    None,
                )
            });
            cmd = Command::AddMeasurement {
                name: name.clone(),
                unit: if deg {
                    MeasurementUnit::Deg
                } else {
                    MeasurementUnit::Mm
                },
            };
            after = Box::new(move |o, out, _| {
                let id = out.measurements[0].0;
                assert!(o.ever.insert(id));
                o.ms.push(OM {
                    id,
                    name: name.trim().to_string(),
                    deg,
                    vals: BTreeMap::new(),
                });
            });
        }
        2..=4 => {
            let id = pick_m(r, o);
            let s = r.below(N);
            let deg = o.m(id).is_some_and(|m| m.deg);
            let v = value_for(r, o, deg, s);
            if !good_value(v) {
                c.bad_values += 1;
            }
            expect = o.m(id).is_some() && good_value(v);
            cmd = Command::SetMeasurementValue {
                measurement: MeasurementId(id),
                size: SizeId(o.sizes[s]),
                value: v,
            };
            after = Box::new(move |o, _, _| {
                o.ms.iter_mut()
                    .find(|m| m.id == id)
                    .unwrap()
                    .vals
                    .insert(s, v);
            });
        }
        5 => {
            let id = pick_m(r, o);
            let s = r.below(N);
            expect = o.m(id).is_some();
            cmd = Command::ClearMeasurementValue {
                measurement: MeasurementId(id),
                size: SizeId(o.sizes[s]),
            };
            after = Box::new(move |o, _, _| {
                o.ms.iter_mut()
                    .find(|m| m.id == id)
                    .unwrap()
                    .vals
                    .remove(&s);
            });
        }
        6 => {
            let id = pick_m(r, o);
            expect = o.m(id).is_some();
            cmd = Command::RemoveMeasurement {
                measurement: MeasurementId(id),
            };
            after = Box::new(move |o, _, c| {
                o.ms.retain(|m| m.id != id);
                let (na, nf) = (o.alts.len(), o.fins.len());
                o.alts.retain(|a| a.target != id);
                o.fins.retain(|f| f.m != id);
                if na + nf > o.alts.len() + o.fins.len() {
                    c.cascade_measure += 1;
                }
            });
        }
        7..=10 => {
            let name = NAMES[r.below(NAMES.len())].to_string();
            let t = pick_m(r, o);
            let d = DELTAS[r.below(DELTAS.len())];
            if !good_delta(d) {
                c.bad_values += 1;
            }
            expect = trimmed(&name).is_some_and(|n| {
                !Oracle::dup(
                    o.alts.iter().map(|a| (a.id, a.name.as_str())).collect(),
                    &n,
                    None,
                )
            }) && o.m(t).is_some()
                && good_delta(d);
            cmd = Command::AddAlteration {
                name: name.clone(),
                target: MeasurementId(t),
                delta: d,
            };
            after = Box::new(move |o, out, c| {
                let id = out.alterations[0].0;
                assert!(o.ever.insert(id), "id jamais réutilisé");
                o.alts.push(OA {
                    id,
                    name: name.trim().to_string(),
                    target: t,
                    delta: d,
                });
                c.alt_adds += 1;
            });
        }
        11 => {
            let id = pick_a(r, o);
            let name = NAMES[r.below(NAMES.len())].to_string();
            expect = o.alts.iter().any(|a| a.id == id)
                && trimmed(&name).is_some_and(|n| {
                    !Oracle::dup(
                        o.alts.iter().map(|a| (a.id, a.name.as_str())).collect(),
                        &n,
                        Some(id),
                    )
                });
            cmd = Command::RenameAlteration {
                alteration: AlterationId(id),
                name: name.clone(),
            };
            after = Box::new(move |o, _, c| {
                o.alts.iter_mut().find(|a| a.id == id).unwrap().name = name.trim().to_string();
                c.alt_edits += 1;
            });
        }
        12 => {
            let id = pick_a(r, o);
            let t = pick_m(r, o);
            expect = o.alts.iter().any(|a| a.id == id) && o.m(t).is_some();
            cmd = Command::SetAlterationTarget {
                alteration: AlterationId(id),
                target: MeasurementId(t),
            };
            after = Box::new(move |o, _, c| {
                o.alts.iter_mut().find(|a| a.id == id).unwrap().target = t;
                c.alt_edits += 1;
            });
        }
        13..=14 => {
            let id = pick_a(r, o);
            let d = DELTAS[r.below(DELTAS.len())];
            expect = o.alts.iter().any(|a| a.id == id) && good_delta(d);
            cmd = Command::SetAlterationDelta {
                alteration: AlterationId(id),
                delta: d,
            };
            after = Box::new(move |o, _, c| {
                o.alts.iter_mut().find(|a| a.id == id).unwrap().delta = d;
                c.alt_edits += 1;
            });
        }
        15 => {
            let id = pick_a(r, o);
            expect = o.alts.iter().any(|a| a.id == id);
            cmd = Command::RemoveAlteration {
                alteration: AlterationId(id),
            };
            after = Box::new(move |o, _, _| o.alts.retain(|a| a.id != id));
        }
        16..=21 => {
            let name = NAMES[r.below(NAMES.len())].to_string();
            let m = pick_m(r, o);
            let src = pick_src(r, o);
            let (tm, tp) = (TOLS[r.below(TOLS.len())], TOLS[r.below(TOLS.len())]);
            expect = o.fin_ok(&name, m, src, tm, tp, None);
            if o.m(m).is_some_and(|om| om.deg != o.src_deg(src)) && o.src_alive(src) {
                c.unit_refused += 1;
            }
            if !o.src_alive(src) || o.m(m).is_none() {
                c.ghost_refused += 1;
            }
            if !good_value(tm) || !good_value(tp) {
                c.bad_values += 1;
            }
            cmd = Command::AddFinishedMeasure {
                name: name.clone(),
                measurement: MeasurementId(m),
                source: to_source(g, src),
                tolerance_minus: tm,
                tolerance_plus: tp,
            };
            after = Box::new(move |o, out, c| {
                let id = out.finished_measures[0].0;
                assert!(o.ever.insert(id));
                o.fins.push(OF {
                    id,
                    name: name.trim().to_string(),
                    m,
                    src,
                    tm,
                    tp,
                });
                c.fin_adds += 1;
            });
        }
        22..=23 => {
            let id = pick_f(r, o);
            let name = NAMES[r.below(NAMES.len())].to_string();
            let m = pick_m(r, o);
            let src = pick_src(r, o);
            let (tm, tp) = (TOLS[r.below(TOLS.len())], TOLS[r.below(TOLS.len())]);
            expect = o.fins.iter().any(|f| f.id == id) && o.fin_ok(&name, m, src, tm, tp, Some(id));
            cmd = Command::SetFinishedMeasure {
                finished: FinishedMeasureId(id),
                name: name.clone(),
                measurement: MeasurementId(m),
                source: to_source(g, src),
                tolerance_minus: tm,
                tolerance_plus: tp,
            };
            after = Box::new(move |o, _, c| {
                let f = o.fins.iter_mut().find(|f| f.id == id).unwrap();
                *f = OF {
                    id,
                    name: name.trim().to_string(),
                    m,
                    src,
                    tm,
                    tp,
                };
                c.fin_sets += 1;
            });
        }
        24 => {
            let id = pick_f(r, o);
            expect = o.fins.iter().any(|f| f.id == id);
            cmd = Command::RemoveFinishedMeasure {
                finished: FinishedMeasureId(id),
            };
            after = Box::new(move |o, _, _| o.fins.retain(|f| f.id != id));
        }
        25..=26 => {
            // Suppression d'une cote vivante : retire les mesures finies qui la citent.
            if o.dims.is_empty() {
                return;
            }
            let (id, _) = o.dims[r.below(o.dims.len())];
            expect = true;
            cmd = Command::DeleteFullDimension {
                sheet: g.sheet,
                dimension: FullDimensionId(id),
            };
            after = Box::new(move |o, _, c| {
                o.dims.retain(|d| d.0 != id);
                let n = o.fins.len();
                o.fins.retain(|f| f.src != Src::Dim(id));
                if o.fins.len() < n {
                    c.cascade_source += 1;
                }
            });
        }
        27 => {
            let Some(id) = o.seam else {
                return;
            };
            expect = true;
            cmd = Command::RemoveSeamPair { id: SeamPairId(id) };
            after = Box::new(move |o, _, c| {
                o.seam = None;
                let n = o.fins.len();
                o.fins.retain(|f| f.src != Src::Seam(id));
                if o.fins.len() < n {
                    c.cascade_source += 1;
                }
            });
        }
        28 => {
            // Recrée la couture : refusée si elle existe déjà (une arête = un couple).
            let a = p.seam_side_range(g.piece, g.bc, 1).unwrap();
            let b = p.seam_side_range(g.dos, g.vertical, 1).unwrap();
            expect = o.seam.is_none();
            cmd = Command::AddSeamPair {
                a,
                b,
                embu_mm: None,
                tolerance_mm: None,
            };
            after = Box::new(move |o, out, _| {
                let id = out.seam_pairs[0].0;
                assert!(o.ever.insert(id));
                o.seam = Some(id);
            });
        }
        _ => {
            let kinds = [
                DimKind::Width,
                DimKind::HRev,
                DimKind::VRev,
                DimKind::Angle,
                DimKind::Short,
            ];
            let k = kinds[r.below(kinds.len())];
            expect = true;
            cmd = Command::AddFullDimension {
                sheet: g.sheet,
                kind: dim_kind(g, k),
                label: None,
                text_shift_mm: 0.0,
            };
            after = Box::new(move |o, out, _| {
                let id = out.full_dimensions[0].0;
                assert!(o.ever.insert(id));
                o.dims.push((id, k));
            });
        }
    }
    match p.apply(cmd.clone()) {
        Ok(out) => {
            assert!(expect, "accepté alors que l'oracle refuse : {cmd:?}");
            after(o, &out, c);
            c.accepted += 1;
        }
        Err(e) => {
            assert!(!expect, "refusé alors que l'oracle accepte : {cmd:?} : {e}");
            assert_eq!(persist::to_json(p).unwrap(), before, "refus sans trace");
            c.refused += 1;
            c.bytes_checked += 1;
        }
    }
}

fn state_matches(p: &Project, o: &Oracle) {
    assert_eq!(p.measurements.len(), o.ms.len());
    for (pm, om) in p.measurements.iter().zip(&o.ms) {
        assert_eq!((pm.id.0, &pm.name), (om.id, &om.name));
        assert_eq!(pm.unit == MeasurementUnit::Deg, om.deg);
        let got: BTreeMap<usize, f64> = pm
            .values
            .iter()
            .map(|v| {
                (
                    o.sizes.iter().position(|s| *s == v.size.0).unwrap(),
                    v.value,
                )
            })
            .collect();
        assert_eq!(got, om.vals, "valeurs de {}", om.name);
    }
    assert_eq!(p.alterations.len(), o.alts.len(), "retouches");
    for (pa, oa) in p.alterations.iter().zip(&o.alts) {
        assert_eq!(
            (pa.id.0, &pa.name, pa.target.0, pa.delta),
            (oa.id, &oa.name, oa.target, oa.delta)
        );
    }
    assert_eq!(p.finished_measures.len(), o.fins.len(), "mesures finies");
    for (pf, of) in p.finished_measures.iter().zip(&o.fins) {
        assert_eq!(
            (pf.id.0, &pf.name, pf.measurement.0),
            (of.id, &of.name, of.m)
        );
        assert_eq!((pf.tolerance_minus, pf.tolerance_plus), (of.tm, of.tp));
        match (pf.source, of.src) {
            (FinishedSource::Seam { seam }, Src::Seam(id)) => assert_eq!(seam.0, id),
            (FinishedSource::FullDimension { dimension, .. }, Src::Dim(id)) => {
                assert_eq!(dimension.0, id)
            }
            (a, b) => panic!("source {a:?} ≠ {b:?}"),
        }
    }
    assert_eq!(p.seams.len(), usize::from(o.seam.is_some()));
    let text = persist::to_json(p).expect("valide");
    assert_eq!(&persist::from_json(&text).expect("relu"), p);
}

fn check_readings(p: &Project, o: &Oracle, r: &mut R, c: &mut Counters) {
    for with_alt in [false, true] {
        let t = p
            .finished_measure_table(p.sizes[0].id, with_alt)
            .expect("tableau");
        assert_eq!(t.columns.len(), N);
        for s in 0..N {
            assert_eq!(t.sizes[s].0, o.sizes[s]);
            assert_eq!(t.columns[s].len(), o.fins.len());
            for (rd, f) in t.columns[s].iter().zip(&o.fins) {
                c.readings += 1;
                let measured = o.measured(f.src, s);
                let (base, alt, count, delta, issue) = o.altered(f.m, s);
                let reference = if with_alt { alt } else { base };
                let reason = if measured.is_none() {
                    Some(FinishedReason::SourceDegenerate)
                } else if with_alt && issue == 2 {
                    Some(FinishedReason::AlterationOutOfRange)
                } else if reference.is_none() {
                    Some(FinishedReason::NoReferenceValue)
                } else {
                    None
                };
                assert_eq!(rd.id.0, f.id);
                assert_eq!(rd.measured, measured, "mesuré {} {}", f.name, SIZES[s]);
                assert_eq!(rd.reference, reference, "référence {} {}", f.name, SIZES[s]);
                assert_eq!(rd.reason, reason, "raison {} {}", f.name, SIZES[s]);
                let (gap, status) = match (reason, measured, reference) {
                    (None, Some(m), Some(rf)) => {
                        let gap = m - rf;
                        let ok = gap >= -f.tm && gap <= f.tp;
                        if gap == -f.tm || gap == f.tp {
                            c.boundary += 1;
                        }
                        (
                            Some(gap),
                            if ok {
                                FinishedStatus::Conforming
                            } else {
                                FinishedStatus::OutOfTolerance
                            },
                        )
                    }
                    _ => (None, FinishedStatus::NotComparable),
                };
                assert_eq!(rd.gap, gap, "écart {} {}", f.name, SIZES[s]);
                assert_eq!(rd.status, status, "statut {} {}", f.name, SIZES[s]);
                let want_delta = if with_alt && count > 0 && alt.is_some() {
                    delta
                } else {
                    0.0
                };
                assert_eq!(rd.alteration_delta, want_delta);
                match status {
                    FinishedStatus::Conforming => c.conforming += 1,
                    FinishedStatus::OutOfTolerance => c.out += 1,
                    FinishedStatus::NotComparable => match reason {
                        Some(FinishedReason::SourceDegenerate) => c.na_source += 1,
                        Some(FinishedReason::AlterationOutOfRange) => c.na_alteration += 1,
                        _ => c.na_reference += 1,
                    },
                }
            }
        }
    }
    // Valeurs retouchées, rapport par retouche et copie lue.
    let s = r.below(N);
    let size = SizeId(o.sizes[s]);
    for m in &o.ms {
        let (base, alt, count, delta, issue) = o.altered(m.id, s);
        let v = p
            .altered_measurement_value(MeasurementId(m.id), size)
            .unwrap();
        assert_eq!(
            (v.base, v.altered, v.count, v.delta),
            (base, alt, count, delta)
        );
        let want = match issue {
            1 => Some(AlterationIssue::NoBaseValue),
            2 => Some(AlterationIssue::OutOfRange),
            _ => None,
        };
        assert_eq!(v.issue, want);
        match issue {
            1 => c.altered_issue_nobase += 1,
            2 => c.altered_issue_range += 1,
            _ => {}
        }
        if count > 0 && alt != base {
            c.with_alt_differs += 1;
        }
    }
    let rep = p.alteration_report(size).unwrap();
    assert_eq!(rep.len(), o.alts.len());
    for (rd, a) in rep.iter().zip(&o.alts) {
        let (base, alt, _, _, issue) = o.altered(a.target, s);
        assert_eq!((rd.base, rd.altered), (base, alt));
        assert_eq!(
            rd.status,
            match issue {
                0 => AlterationStatus::Applied,
                1 => AlterationStatus::NoBaseValue,
                _ => AlterationStatus::OutOfRange,
            }
        );
    }
    // Copie lue : valeurs retouchées (ou retirées), géométrie identique à celle sans retouche.
    let g = p.graded_project(Some(size)).unwrap();
    let mut plain = p.clone();
    plain.alterations.clear();
    let gp = plain.graded_project(Some(size)).unwrap();
    assert_eq!(g.sheets, gp.sheets, "retouches : géométrie intacte");
    for m in &o.ms {
        let (base, alt, count, _, _) = o.altered(m.id, s);
        let got = g
            .measurement(MeasurementId(m.id))
            .unwrap()
            .values
            .iter()
            .find(|v| v.size == size)
            .map(|v| v.value);
        let want = if count > 0 && base.is_some() {
            alt
        } else {
            base
        };
        assert_eq!(got, want, "copie lue : {}", m.name);
    }
    c.copies_checked += 1;
}

#[test]
fn campagne_retouches_et_mesures_finies() {
    let mut c = Counters::default();
    let mut seqs = 0;
    for seed in 1..=200u64 {
        let mut r = R(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let (mut p, mut o, g) = new_project(&mut r);
        let initial = p.sheets[0].entities.clone();
        for _ in 0..30 {
            step(&mut p, &mut o, &g, &mut r, &mut c);
            state_matches(&p, &o);
            check_readings(&p, &o, &mut r, &mut c);
            assert_eq!(
                p.sheets[0].entities, initial,
                "tracés de base jamais modifiés"
            );
        }
        seqs += 1;
    }
    eprintln!("{c:#?}");
    assert_eq!(seqs, 200);
    assert!(c.accepted > 2000 && c.refused > 1500, "{c:?}");
    assert!(c.bytes_checked == c.refused, "{c:?}");
    assert!(c.alt_adds > 300 && c.alt_edits > 150, "{c:?}");
    assert!(c.fin_adds > 150 && c.fin_sets > 10, "{c:?}");
    assert!(c.cascade_measure > 20 && c.cascade_source > 10, "{c:?}");
    assert!(
        c.unit_refused > 50 && c.ghost_refused > 100 && c.bad_values > 300,
        "{c:?}"
    );
    assert!(c.readings > 20_000, "{c:?}");
    assert!(c.conforming > 3000 && c.out > 3000, "{c:?}");
    assert!(c.boundary > 100, "{c:?}");
    assert!(
        c.na_reference > 100 && c.na_source > 50 && c.na_alteration > 20,
        "{c:?}"
    );
    assert!(c.with_alt_differs > 500, "{c:?}");
    assert!(
        c.altered_issue_nobase > 50 && c.altered_issue_range > 20,
        "{c:?}"
    );
    assert!(c.copies_checked == 6000, "{c:?}");
}

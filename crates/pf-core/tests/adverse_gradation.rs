//! US-09.2a — campagne adverse « gradation » : 160 séquences à graine fixe de 60 pas (points libres
//! et décalés, plusieurs feuilles, gammes et tailles, écarts valides, nuls, hors bornes, non finis,
//! sur la base, sur un point construit, sur des identifiants inconnus ; changements de base ;
//! retraits de tailles et de gammes ; suppressions de points ; déplacements ; copies de feuille ;
//! annuler, rétablir, écrire-relire).
//!
//! **Oracle indépendant** : le modèle (`Model`) est réécrit ici avec ses propres règles. Il stocke
//! la position de base de chaque point libre et les écarts, et **calcule les positions évaluées**
//! (`eval`) : base + écart, un point décalé = position évaluée de son parent + décalage. Le
//! changement de base y est formulé par l'invariant : la nouvelle position de base est la position
//! évaluée de la nouvelle base et chaque écart est la **différence des positions évaluées** (pas la
//! formule du code testé). Les valeurs sont des multiples de 0,25 mm : les sommes sont exactes.
//!
//! Invariants vérifiés après chaque pas : projet valide ; refus = octets identiques et aucune
//! étape ; étape si et seulement si l'état change ; annuler / rétablir exacts à l'octet ;
//! relecture identique ; `evaluate_sheet(None)` = géométrie écrite = évaluation de la base de
//! chaque gamme ; **les positions évaluées de toutes les tailles sont celles de l'oracle** ; un
//! changement de base laisse toutes les positions évaluées inchangées ; retirer une taille ne
//! change aucune autre taille ; aucune taille inconnue n'est acceptée.
//!
//! Mutations vérifiées à la main : voir `docs/JOURNAL.md` (tranche 95).

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

#[derive(Clone, Debug, PartialEq)]
enum ODef {
    Free(f64, f64),
    Offset { from: u64, dx: f64, dy: f64 },
}

#[derive(Clone, Debug, PartialEq)]
struct OPoint {
    id: u64,
    def: ODef,
}

#[derive(Clone, Debug, PartialEq)]
struct OSheet {
    id: u64,
    points: Vec<OPoint>,
    /// Écarts : (point, taille) → (dx, dy), jamais nuls.
    e: BTreeMap<(u64, u64), (f64, f64)>,
}

#[derive(Clone, Debug, PartialEq)]
struct ORange {
    id: u64,
    sizes: Vec<u64>,
    base: u64,
}

#[derive(Clone, Debug, PartialEq)]
struct Model {
    sheets: Vec<OSheet>,
    ranges: Vec<ORange>,
}

fn in_range(v: f64) -> bool {
    v.is_finite() && v.abs() <= 1.0e6
}

/// (feuille, point, taille) → position évaluée (`None` : incalculable).
type Snapshot = BTreeMap<(u64, u64, Option<u64>), Option<(f64, f64)>>;

impl OSheet {
    fn point(&self, id: u64) -> Option<&OPoint> {
        self.points.iter().find(|p| p.id == id)
    }

    /// Position évaluée d'un point dans une taille (`size` = `None` : géométrie écrite) ; `None`
    /// si la position sortirait de l'espace de travail (un point décalé d'un parent gradué).
    fn eval(&self, id: u64, size: Option<u64>) -> Option<(f64, f64)> {
        let at = match &self.point(id).expect("point").def {
            ODef::Free(x, y) => {
                let (dx, dy) = size
                    .and_then(|s| self.e.get(&(id, s)))
                    .copied()
                    .unwrap_or((0.0, 0.0));
                (x + dx, y + dy)
            }
            ODef::Offset { from, dx, dy } => {
                let (x, y) = self.eval(*from, size)?;
                (x + dx, y + dy)
            }
        };
        (in_range(at.0) && in_range(at.1)).then_some(at)
    }
}

impl Model {
    fn all_sizes(&self) -> Vec<u64> {
        self.ranges.iter().flat_map(|r| r.sizes.clone()).collect()
    }

    fn range_of(&self, size: u64) -> Option<usize> {
        self.ranges.iter().position(|r| r.sizes.contains(&size))
    }

    fn is_base(&self, size: u64) -> bool {
        self.ranges.iter().any(|r| r.base == size)
    }

    fn sheet(&self, id: u64) -> Option<usize> {
        self.sheets.iter().position(|s| s.id == id)
    }

    /// Les positions évaluées de tous les points dans toutes les tailles (et `None`).
    fn snapshot(&self) -> Snapshot {
        let mut out = BTreeMap::new();
        for s in &self.sheets {
            for p in &s.points {
                out.insert((s.id, p.id, None), s.eval(p.id, None));
                for size in self.all_sizes() {
                    out.insert((s.id, p.id, Some(size)), s.eval(p.id, Some(size)));
                }
            }
        }
        out
    }

    /// Toutes les positions graduées d'un point libre restent dans l'espace de travail, et la
    /// géométrie écrite de la feuille est calculable.
    fn graded_ok(&self, sheet: &OSheet, base: (f64, f64), point: u64) -> bool {
        sheet
            .e
            .iter()
            .filter(|((p, _), _)| *p == point)
            .all(|(_, (dx, dy))| in_range(base.0 + dx) && in_range(base.1 + dy))
            && sheet
                .points
                .iter()
                .all(|q| sheet.eval(q.id, None).is_some())
    }

    /// Prédit l'effet d'une commande : `None` = refus, `Some(nouvel état)` sinon. Les identifiants
    /// créés par la commande ne sont pas connus ici : `adopt` les reprend du projet.
    fn predict(&self, cmd: &Command) -> Option<Model> {
        let mut m = self.clone();
        match cmd {
            Command::SetPointGrading {
                sheet,
                point,
                size,
                dx_mm,
                dy_mm,
            } => {
                let si = m.sheet(sheet.0)?;
                m.sheets[si].point(point.0)?;
                m.range_of(size.0)?;
                if m.is_base(size.0) {
                    return None;
                }
                if !in_range(*dx_mm) || !in_range(*dy_mm) {
                    return None;
                }
                let zero = *dx_mm == 0.0 && *dy_mm == 0.0;
                if !zero {
                    let ODef::Free(x, y) = m.sheets[si].point(point.0)?.def else {
                        return None;
                    };
                    if !in_range(x + dx_mm) || !in_range(y + dy_mm) {
                        return None;
                    }
                    m.sheets[si].e.insert((point.0, size.0), (*dx_mm, *dy_mm));
                } else {
                    m.sheets[si].e.remove(&(point.0, size.0));
                }
            }
            Command::ClearPointGrading { sheet, point } => {
                let si = m.sheet(sheet.0)?;
                m.sheets[si].point(point.0)?;
                m.sheets[si].e.retain(|(p, _), _| *p != point.0);
            }
            Command::AddSizeRange { labels, base, .. } => {
                if m.ranges.len() >= 20
                    || labels.is_empty()
                    || labels.len() > 60
                    || *base >= labels.len()
                {
                    return None;
                }
                // Identifiants adoptés ensuite (ils viennent du compteur du projet).
                m.ranges.push(ORange {
                    id: 0,
                    sizes: vec![0; labels.len()],
                    base: u64::MAX - *base as u64,
                });
            }
            Command::AddSize { range, .. } => {
                let ri = m.ranges.iter().position(|r| r.id == range.0)?;
                if m.ranges[ri].sizes.len() >= 60 {
                    return None;
                }
                m.ranges[ri].sizes.push(0);
            }
            Command::RemoveSize { size } => {
                let ri = m.range_of(size.0)?;
                if m.ranges[ri].sizes.len() == 1 || m.ranges[ri].base == size.0 {
                    return None;
                }
                m.ranges[ri].sizes.retain(|s| *s != size.0);
                for s in &mut m.sheets {
                    s.e.retain(|(_, sz), _| *sz != size.0);
                }
            }
            Command::RemoveSizeRange { range } => {
                let ri = m.ranges.iter().position(|r| r.id == range.0)?;
                let gone = m.ranges.remove(ri);
                for s in &mut m.sheets {
                    s.e.retain(|(_, sz), _| !gone.sizes.contains(sz));
                }
            }
            Command::SetSizeBreak { size, .. } => {
                m.range_of(size.0)?;
            }
            Command::SetBaseSize { size } => {
                let ri = m.range_of(size.0)?;
                if m.ranges[ri].base == size.0 {
                    return Some(m);
                }
                let before = self.snapshot();
                let shifts = m
                    .sheets
                    .iter()
                    .any(|s| s.e.keys().any(|(_, sz)| *sz == size.0));
                if shifts && m.ranges.len() > 1 {
                    return None;
                }
                // La géométrie écrite de la nouvelle base doit être calculable partout.
                if before
                    .iter()
                    .any(|((_, _, sz), v)| *sz == Some(size.0) && v.is_none())
                {
                    return None;
                }
                m.ranges[ri].base = size.0;
                // Nouvelle position de base = position évaluée de la nouvelle base ; chaque écart
                // = (position évaluée avant) − (nouvelle position de base).
                let sizes = self.ranges[ri].sizes.clone();
                for (si, s) in m.sheets.iter_mut().enumerate() {
                    let old = &self.sheets[si];
                    let free: Vec<u64> = old
                        .points
                        .iter()
                        .filter(|p| matches!(p.def, ODef::Free(..)))
                        .map(|p| p.id)
                        .collect();
                    for pid in free {
                        let new_base = before[&(old.id, pid, Some(size.0))].expect("calculable");
                        for p in &mut s.points {
                            if p.id == pid {
                                p.def = ODef::Free(new_base.0, new_base.1);
                            }
                        }
                        s.e.retain(|(p, sz), _| !(*p == pid && sizes.contains(sz)));
                        for &sz in &sizes {
                            if sz == size.0 {
                                continue;
                            }
                            let ev = before[&(old.id, pid, Some(sz))].expect("calculable");
                            let e = (ev.0 - new_base.0, ev.1 - new_base.1);
                            // Un écart re-exprimé au-delà de ±1 000 000 mm est refusé.
                            if !in_range(e.0) || !in_range(e.1) {
                                return None;
                            }
                            if e != (0.0, 0.0) {
                                s.e.insert((pid, sz), e);
                            }
                        }
                    }
                }
            }
            Command::AddPoint { sheet, .. } => {
                let si = m.sheet(sheet.0)?;
                // Un point libre de plus (identifiant adopté) — ou une fusion (voir `adopt`).
                m.sheets[si].points.push(OPoint {
                    id: 0,
                    def: ODef::Free(0.0, 0.0),
                });
            }
            Command::MovePoint { sheet, point, to } => {
                let si = m.sheet(sheet.0)?;
                let ODef::Free(..) = m.sheets[si].point(point.0)?.def else {
                    return None;
                };
                if !in_range(to.x) || !in_range(to.y) {
                    return None;
                }
                for p in &mut m.sheets[si].points {
                    if p.id == point.0 {
                        p.def = ODef::Free(to.x, to.y);
                    }
                }
                if !m.graded_ok(&m.sheets[si], (to.x, to.y), point.0) {
                    return None;
                }
            }
            Command::Delete {
                sheet,
                points,
                entities,
            } => {
                assert!(entities.is_empty());
                let si = m.sheet(sheet.0)?;
                for p in points {
                    m.sheets[si].point(p.0)?;
                    let has_child = m.sheets[si]
                        .points
                        .iter()
                        .any(|q| matches!(q.def, ODef::Offset { from, .. } if from == p.0));
                    if has_child {
                        return None;
                    }
                    m.sheets[si].points.retain(|q| q.id != p.0);
                    m.sheets[si].e.retain(|(q, _), _| *q != p.0);
                }
            }
            Command::AddPointAt { sheet, point } => {
                let si = m.sheet(sheet.0)?;
                let PointRef::Derived(PointDefinition::Offset { from, dx_mm, dy_mm }) = point
                else {
                    unreachable!("seuls les points décalés sont générés")
                };
                m.sheets[si].point(from.0)?;
                m.sheets[si].points.push(OPoint {
                    id: 0,
                    def: ODef::Offset {
                        from: from.0,
                        dx: *dx_mm,
                        dy: *dy_mm,
                    },
                });
                // La géométrie écrite du nouveau point doit être dans l'espace de travail.
                m.sheets[si].eval(0, None)?;
            }
            Command::DuplicateSheet { sheet } => {
                let si = m.sheet(sheet.0)?;
                let copy = m.sheets[si].clone();
                m.sheets.insert(si + 1, copy);
            }
            _ => unreachable!("commande non générée"),
        }
        Some(m)
    }

    /// Reprend dans le modèle prédit les identifiants créés par la commande (lus dans le projet).
    fn adopt(&mut self, cmd: &Command, before: &Model, after: &Project, out: &Outcome) {
        let _ = after;
        match cmd {
            Command::AddSizeRange { base, .. } => {
                let r = self.ranges.last_mut().expect("gamme");
                r.id = out.size_ranges[0].0;
                r.sizes = out.sizes.iter().map(|s| s.0).collect();
                r.base = r.sizes[*base];
            }
            Command::AddSize { range, .. } => {
                let ri = self.ranges.iter().position(|r| r.id == range.0).unwrap();
                *self.ranges[ri].sizes.last_mut().unwrap() = out.sizes[0].0;
            }
            Command::AddPoint { sheet, at } => {
                let si = self.sheet(sheet.0).unwrap();
                match out.points.first() {
                    Some(id) => {
                        self.sheets[si].points.last_mut().unwrap().id = id.0;
                        self.sheets[si].points.last_mut().unwrap().def = ODef::Free(at.x, at.y);
                    }
                    // Fusion avec un point existant au même endroit : rien n'est créé.
                    None => self.sheets[si].points.pop().map(|_| ()).unwrap_or(()),
                }
            }
            Command::AddPointAt { sheet, .. } => {
                let si = self.sheet(sheet.0).unwrap();
                match out.points.first() {
                    Some(id) => self.sheets[si].points.last_mut().unwrap().id = id.0,
                    None => {
                        self.sheets[si].points.pop();
                    }
                }
            }
            Command::DuplicateSheet { sheet } => {
                let si = before.sheet(sheet.0).unwrap();
                let new = after.sheet(out.sheet.expect("feuille copiée")).unwrap();
                let old = &before.sheets[si];
                let map: BTreeMap<u64, u64> = old
                    .points
                    .iter()
                    .zip(new.points.iter())
                    .map(|(o, n)| (o.id, n.id.0))
                    .collect();
                assert_eq!(old.points.len(), new.points.len());
                let copy = &mut self.sheets[si + 1];
                copy.id = new.id.0;
                for p in &mut copy.points {
                    p.id = map[&p.id];
                    if let ODef::Offset { from, .. } = &mut p.def {
                        *from = map[from];
                    }
                }
                copy.e = old.e.iter().map(|((p, s), v)| ((map[p], *s), *v)).collect();
            }
            _ => {}
        }
    }
}

/// Modèle lu dans le projet (utilisé au départ et après annuler / rétablir pour l'historique :
/// ici seul le modèle est recopié, jamais relu du code testé).
fn bytes(p: &Project) -> String {
    persist::to_json(p).expect("projet valide")
}

// ---------------------------------------------------------------------------------------------
// Générateurs
// ---------------------------------------------------------------------------------------------

/// Valeurs d'écart : multiples de 0,25, avec des bornes et des valeurs invalides.
fn gen_value(rng: &mut Rng) -> f64 {
    if rng.chance(82) {
        return (rng.below(81) as f64 - 40.0) * 0.25 * if rng.chance(30) { 8.0 } else { 1.0 };
    }
    match rng.below(9) {
        0 => 0.0,
        1 => f64::NAN,
        2 => f64::INFINITY,
        3 => 1_000_000.0,
        4 => 1_000_000.25,
        5 => -999_999.75,
        6 => 999_900.0,
        7 => -5000.0,
        _ => 0.0,
    }
}

fn gen_size(rng: &mut Rng, m: &Model) -> SizeId {
    let all = m.all_sizes();
    if all.is_empty() || rng.chance(4) {
        SizeId(GHOST)
    } else if rng.chance(8) {
        // La taille de base d'une gamme : refus attendu.
        SizeId(m.ranges[rng.below(m.ranges.len() as u64) as usize].base)
    } else if rng.chance(40) {
        // Une taille qui porte déjà des écarts : retraits et changements de base intéressants.
        let used: Vec<u64> = m
            .sheets
            .iter()
            .flat_map(|s| s.e.keys().map(|(_, sz)| *sz))
            .collect();
        if used.is_empty() {
            SizeId(*rng.pick(&all))
        } else {
            SizeId(*rng.pick(&used))
        }
    } else {
        SizeId(*rng.pick(&all))
    }
}

fn gen_point(rng: &mut Rng, m: &Model, si: usize) -> PointId {
    let s = &m.sheets[si];
    if s.points.is_empty() || rng.chance(5) {
        PointId(GHOST)
    } else {
        PointId(rng.pick(&s.points).id)
    }
}

struct Counters {
    steps: usize,
    accepted: usize,
    refused: usize,
    noops: usize,
    graded: usize,
    rebases: usize,
    rebases_shifting: usize,
    rebase_refused: usize,
    size_removals_with_entries: usize,
    deletions_with_entries: usize,
    duplicates_with_entries: usize,
    undos: usize,
    redos: usize,
    reloads: usize,
    next_label: u64,
    max_entries: usize,
}

fn gen_command(rng: &mut Rng, m: &Model, c: &mut Counters) -> Command {
    let si = rng.below(m.sheets.len() as u64) as usize;
    let sheet = SheetId(m.sheets[si].id);
    match rng.below(100) {
        0..=34 => Command::SetPointGrading {
            sheet,
            point: gen_point(rng, m, si),
            size: gen_size(rng, m),
            dx_mm: gen_value(rng),
            dy_mm: gen_value(rng),
        },
        35..=38 => Command::ClearPointGrading {
            sheet,
            point: gen_point(rng, m, si),
        },
        39..=42 => {
            c.next_label += 1;
            let n = 1 + rng.below(5) as usize;
            Command::AddSizeRange {
                name: format!("Gamme {}", c.next_label),
                kind: SizeKind::Alphanumeric,
                labels: (0..n).map(|i| format!("t{}_{i}", c.next_label)).collect(),
                base: rng.below(n as u64) as usize,
            }
        }
        43..=48 => {
            c.next_label += 1;
            let range = if m.ranges.is_empty() || rng.chance(6) {
                SizeRangeId(GHOST)
            } else {
                SizeRangeId(rng.pick(&m.ranges).id)
            };
            Command::AddSize {
                range,
                label: format!("u{}", c.next_label),
                at: None,
            }
        }
        49..=53 => Command::RemoveSize {
            size: gen_size(rng, m),
        },
        54 => Command::RemoveSizeRange {
            range: if m.ranges.is_empty() || rng.chance(5) {
                SizeRangeId(GHOST)
            } else {
                SizeRangeId(rng.pick(&m.ranges).id)
            },
        },
        55 => Command::SetSizeBreak {
            size: gen_size(rng, m),
            break_size: rng.chance(50),
        },
        56..=64 => Command::SetBaseSize {
            size: gen_size(rng, m),
        },
        65..=70 => c_add_point(rng, sheet, c),
        71..=74 => {
            let from = gen_point(rng, m, si);
            Command::AddPointAt {
                sheet,
                point: PointRef::Derived(PointDefinition::Offset {
                    from,
                    dx_mm: (rng.below(40) as f64 + 1.0) * 0.5,
                    dy_mm: -(rng.below(40) as f64) * 0.25,
                }),
            }
        }
        75..=80 => Command::MovePoint {
            sheet,
            point: gen_point(rng, m, si),
            to: PointMm::new(
                if rng.chance(5) {
                    999_999.0
                } else {
                    rng.below(2000) as f64 * 0.25
                },
                rng.below(2000) as f64 * 0.25,
            ),
        },
        81..=85 => Command::Delete {
            sheet,
            points: vec![gen_point(rng, m, si)],
            entities: vec![],
        },
        _ => Command::DuplicateSheet { sheet },
    }
}

fn c_add_point(rng: &mut Rng, sheet: SheetId, c: &mut Counters) -> Command {
    c.next_label += 1;
    // Positions distinctes (aucune fusion sauf coïncidence, tolérée par l'oracle).
    Command::AddPoint {
        sheet,
        at: PointMm::new(
            (c.next_label * 13 % 3000) as f64 + rng.below(4) as f64 * 0.25,
            (c.next_label * 29 % 3000) as f64,
        ),
    }
}

// ---------------------------------------------------------------------------------------------
// Invariants
// ---------------------------------------------------------------------------------------------

fn check_state(p: &Project, expected: &Model, ctx: &str) {
    p.validate().unwrap_or_else(|e| panic!("{ctx} : {e}"));
    // Structure : mêmes feuilles, mêmes points, mêmes gammes.
    assert_eq!(p.sheets.len(), expected.sheets.len(), "{ctx} : feuilles");
    assert_eq!(p.sizes.len(), expected.ranges.len(), "{ctx} : gammes");
    for (r, o) in p.sizes.iter().zip(&expected.ranges) {
        assert_eq!(r.id.0, o.id, "{ctx} : gamme");
        assert_eq!(r.base.0, o.base, "{ctx} : base");
        let mut got: Vec<u64> = r.sizes.iter().map(|s| s.id.0).collect();
        let mut want = o.sizes.clone();
        got.sort_unstable();
        want.sort_unstable();
        assert_eq!(got, want, "{ctx} : tailles de la gamme");
    }
    let all: Vec<u64> = expected.all_sizes();
    let mut total_entries = 0;
    for (s, o) in p.sheets.iter().zip(&expected.sheets) {
        assert_eq!(s.id.0, o.id, "{ctx} : id de feuille");
        let got_ids: Vec<u64> = s.points.iter().map(|q| q.id.0).collect();
        let want_ids: Vec<u64> = o.points.iter().map(|q| q.id).collect();
        assert_eq!(got_ids, want_ids, "{ctx} : points de la feuille {}", s.id.0);
        // Écarts stockés = écarts de l'oracle (à 1e-9), rangés par (point, taille), sans doublon.
        assert_eq!(s.gradings.len(), o.e.len(), "{ctx} : nombre d'écarts");
        let keys: Vec<(u64, u64)> = s.gradings.iter().map(|g| (g.point.0, g.size.0)).collect();
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(keys, sorted, "{ctx} : écarts non rangés ou en double");
        for g in &s.gradings {
            let want = o
                .e
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
                !expected.is_base(g.size.0),
                "{ctx} : écart sur une taille de base"
            );
        }
        total_entries += s.gradings.len();
        // Positions évaluées : oracle = code, pour la base écrite et pour chaque taille.
        let base = p.evaluate_sheet(s.id, None).expect("évaluation");
        assert!(base.issues.is_empty(), "{ctx} : {:?}", base.issues);
        for q in &o.points {
            let got = base.points.iter().find(|(i, _)| i.0 == q.id).unwrap().1;
            let want = o.eval(q.id, None).expect("la base est calculable");
            assert!(
                (got.x - want.0).abs() < TOL && (got.y - want.1).abs() < TOL,
                "{ctx} : base du point {}",
                q.id
            );
        }
        for &size in &all {
            let ev = p
                .evaluate_sheet(s.id, Some(SizeId(size)))
                .unwrap_or_else(|e| panic!("{ctx} : taille {size} : {e}"));
            let mut uncomputable = 0;
            for q in &o.points {
                let got = ev.points.iter().find(|(i, _)| i.0 == q.id).map(|(_, v)| *v);
                match (got, o.eval(q.id, Some(size))) {
                    (Some(got), Some(want)) => assert!(
                        (got.x - want.0).abs() < TOL && (got.y - want.1).abs() < TOL,
                        "{ctx} : point {} en taille {size} : ({}, {}) != {:?}",
                        q.id,
                        got.x,
                        got.y,
                        want
                    ),
                    // Jamais de silence : un point hors de l'espace de travail est absent et dit.
                    (None, None) => uncomputable += 1,
                    (g, w) => panic!("{ctx} : point {} en taille {size} : {g:?} / {w:?}", q.id),
                }
            }
            assert_eq!(ev.points.len() + uncomputable, o.points.len(), "{ctx}");
            assert_eq!(
                ev.issues.is_empty(),
                uncomputable == 0,
                "{ctx} : {:?}",
                ev.issues
            );
        }
        // La base de chaque gamme est la géométrie écrite.
        for r in &p.sizes {
            let ev = p.evaluate_sheet(s.id, Some(r.base)).unwrap();
            assert_eq!(ev.points, base.points, "{ctx} : base ≠ None");
        }
        assert!(
            p.evaluate_sheet(s.id, Some(SizeId(555_555_555))).is_err(),
            "{ctx} : taille inconnue acceptée"
        );
    }
    let _ = total_entries;
    // Relecture identique, octet pour octet.
    let text = bytes(p);
    let back = persist::from_json(&text).unwrap_or_else(|e| panic!("{ctx} : relecture : {e}"));
    assert_eq!(&back, p, "{ctx} : relecture différente");
    assert_eq!(bytes(&back), text, "{ctx} : réécriture différente");
}

fn initial() -> (Project, Model) {
    let mut p = Project::new("Adverse gradation");
    let sheet = p.sheets[0].id;
    let a = p
        .apply(Command::AddPoint {
            sheet,
            at: PointMm::new(100.0, 200.0),
        })
        .unwrap()
        .points[0];
    let b = p
        .apply(Command::AddPoint {
            sheet,
            at: PointMm::new(400.0, 50.0),
        })
        .unwrap()
        .points[0];
    // Une gamme de départ (XS S M L XL, base M) : les écarts ont de quoi s'attacher.
    let out = p
        .apply(Command::AddSizeRange {
            name: "Départ".into(),
            kind: SizeKind::Alphanumeric,
            labels: ["XS", "S", "M", "L", "XL"].map(String::from).to_vec(),
            base: 2,
        })
        .unwrap();
    let sizes: Vec<u64> = out.sizes.iter().map(|s| s.0).collect();
    let model = Model {
        sheets: vec![OSheet {
            id: sheet.0,
            points: vec![
                OPoint {
                    id: a.0,
                    def: ODef::Free(100.0, 200.0),
                },
                OPoint {
                    id: b.0,
                    def: ODef::Free(400.0, 50.0),
                },
            ],
            e: BTreeMap::new(),
        }],
        ranges: vec![ORange {
            id: out.size_ranges[0].0,
            base: sizes[2],
            sizes,
        }],
    };
    (p, model)
}

fn run_sequence(seed: u64, c: &mut Counters) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let (start, start_model) = initial();
    let mut doc = Document::new(start);
    let mut model = start_model;
    let mut history: Vec<String> = vec![bytes(doc.project())];
    let mut models: Vec<Model> = vec![model.clone()];
    let mut cursor = 0usize;

    for step in 0..STEPS {
        c.steps += 1;
        let ctx = format!("séquence {seed}, pas {step}");
        let before = doc.project().clone();
        let before_bytes = bytes(&before);
        match rng.below(100) {
            // Annuler.
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
            // Rétablir.
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
            // Écrire puis relire.
            14..=17 => {
                let back = persist::from_json(&before_bytes).expect("relecture");
                assert_eq!(back, before, "{ctx} : relecture");
                c.reloads += 1;
            }
            _ => {
                let cmd = gen_command(&mut rng, &model, c);
                let predicted = model.predict(&cmd);
                let result = doc.execute(cmd.clone());
                let after = doc.project().clone();
                let after_bytes = bytes(&after);
                match (predicted, &result) {
                    (None, Err(e)) => {
                        c.refused += 1;
                        assert!(!e.to_string().is_empty(), "{ctx}");
                        assert_eq!(
                            after_bytes, before_bytes,
                            "{ctx} : refus avec trace : {cmd:?}"
                        );
                        if matches!(
                            cmd,
                            Command::SetPointGrading { .. } | Command::ClearPointGrading { .. }
                        ) {
                            assert!(
                                before.check_grading_command(&cmd).is_err(),
                                "{ctx} : le contrôle typé accepte {cmd:?}"
                            );
                        }
                        if matches!(cmd, Command::SetBaseSize { .. }) {
                            c.rebase_refused += 1;
                        }
                    }
                    (Some(mut m), Ok(out)) => {
                        c.accepted += 1;
                        m.adopt(&cmd, &model, &after, out);
                        if matches!(
                            cmd,
                            Command::SetPointGrading { .. } | Command::ClearPointGrading { .. }
                        ) {
                            assert!(
                                before.check_grading_command(&cmd).is_ok(),
                                "{ctx} : le contrôle typé refuse {cmd:?}"
                            );
                            c.graded += 1;
                        }
                        // Invariants propres aux opérations sur les tailles.
                        match &cmd {
                            Command::SetBaseSize { size } => {
                                let snap_before = model.snapshot();
                                let snap_after = m.snapshot();
                                let changed = model
                                    .ranges
                                    .iter()
                                    .find(|r| r.sizes.contains(&size.0))
                                    .is_some_and(|r| r.base != size.0);
                                if changed {
                                    c.rebases += 1;
                                    if model
                                        .sheets
                                        .iter()
                                        .any(|s| s.e.keys().any(|(_, sz)| *sz == size.0))
                                    {
                                        c.rebases_shifting += 1;
                                    }
                                }
                                // Toutes les positions de TOUTES les tailles sont identiques
                                // (la clé `None` change de sens : c'est la nouvelle base).
                                for ((sh, pt, sz), v) in &snap_before {
                                    if sz.is_none() {
                                        continue;
                                    }
                                    let w = snap_after[&(*sh, *pt, *sz)];
                                    match (v, w) {
                                        (Some(v), Some(w)) => assert!(
                                            (v.0 - w.0).abs() < TOL && (v.1 - w.1).abs() < TOL,
                                            "{ctx} : le changement de base déplace {pt} en {sz:?}"
                                        ),
                                        (None, None) => {}
                                        _ => panic!("{ctx} : calculabilité changée pour {pt}"),
                                    }
                                }
                                for s in &after.sheets {
                                    for q in &s.points {
                                        let sz = Some(*size);
                                        let ev_before = before
                                            .evaluate_sheet(s.id, sz)
                                            .unwrap()
                                            .points
                                            .iter()
                                            .find(|(i, _)| *i == q.id)
                                            .copied();
                                        let ev_after = after
                                            .evaluate_sheet(s.id, sz)
                                            .unwrap()
                                            .points
                                            .iter()
                                            .find(|(i, _)| *i == q.id)
                                            .copied();
                                        assert_eq!(ev_before, ev_after, "{ctx} : code, base");
                                    }
                                }
                            }
                            Command::RemoveSize { size } => {
                                let had = before.size_dependents(*size).len();
                                if had > 0 {
                                    c.size_removals_with_entries += 1;
                                }
                                assert!(after.size_dependents(*size).is_empty(), "{ctx}");
                                // Aucune autre taille n'a bougé.
                                for r in &before.sizes {
                                    for sz in r.sizes.iter().filter(|s| s.id != *size) {
                                        for s in &before.sheets {
                                            let b =
                                                before.evaluate_sheet(s.id, Some(sz.id)).unwrap();
                                            let a =
                                                after.evaluate_sheet(s.id, Some(sz.id)).unwrap();
                                            assert_eq!(
                                                a.points, b.points,
                                                "{ctx} : taille {}",
                                                sz.label
                                            );
                                        }
                                    }
                                }
                                if had > 0 {
                                    assert!(
                                        out.notes.iter().any(|n| n.contains("écart")),
                                        "{ctx} : retrait sans remarque"
                                    );
                                }
                            }
                            Command::Delete { sheet, points, .. } => {
                                let had = before
                                    .sheet(*sheet)
                                    .unwrap()
                                    .gradings
                                    .iter()
                                    .any(|g| points.contains(&g.point));
                                if had {
                                    c.deletions_with_entries += 1;
                                    assert!(
                                        out.notes.iter().any(|n| n.contains("gradation")),
                                        "{ctx} : suppression sans remarque"
                                    );
                                }
                            }
                            Command::DuplicateSheet { sheet }
                                if !before.sheet(*sheet).unwrap().gradings.is_empty() =>
                            {
                                c.duplicates_with_entries += 1;
                            }
                            _ => {}
                        }
                        model = m;
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
        check_state(doc.project(), &model, &ctx);
        assert_eq!(doc.can_undo(), cursor > 0, "{ctx} : can_undo");
        assert_eq!(
            doc.can_redo(),
            cursor + 1 < history.len(),
            "{ctx} : can_redo"
        );
        c.max_entries = c
            .max_entries
            .max(doc.project().sheets.iter().map(|s| s.gradings.len()).sum());
    }
    // Tout défaire puis tout refaire : états exacts à l'octet.
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
fn campagne_adverse_gradation() {
    let mut c = Counters {
        steps: 0,
        accepted: 0,
        refused: 0,
        noops: 0,
        graded: 0,
        rebases: 0,
        rebases_shifting: 0,
        rebase_refused: 0,
        size_removals_with_entries: 0,
        deletions_with_entries: 0,
        duplicates_with_entries: 0,
        undos: 0,
        redos: 0,
        reloads: 0,
        next_label: 0,
        max_entries: 0,
    };
    for seed in 1..=SEQUENCES {
        run_sequence(seed, &mut c);
    }
    eprintln!(
        "gradation : {} pas, {} acceptés ({} sans effet), {} refusés, {} gradations acceptées, \
         {} changements de base ({} avec déplacement, {} refusés), {} retraits de taille avec écarts, \
         {} suppressions de point avec écarts, {} copies avec écarts, {} annuler, {} rétablir, \
         {} relectures, max {} écarts",
        c.steps,
        c.accepted,
        c.noops,
        c.refused,
        c.graded,
        c.rebases,
        c.rebases_shifting,
        c.rebase_refused,
        c.size_removals_with_entries,
        c.deletions_with_entries,
        c.duplicates_with_entries,
        c.undos,
        c.redos,
        c.reloads,
        c.max_entries
    );
    // La campagne doit réellement exercer chaque famille.
    assert!(c.accepted > 1500, "trop peu d'acceptés : {}", c.accepted);
    assert!(c.refused > 1000, "trop peu de refus : {}", c.refused);
    assert!(c.graded > 800, "trop peu de gradations : {}", c.graded);
    assert!(
        c.rebases > 150,
        "trop peu de changements de base : {}",
        c.rebases
    );
    assert!(
        c.rebases_shifting > 40,
        "trop peu de changements de base qui déplacent : {}",
        c.rebases_shifting
    );
    assert!(c.rebase_refused > 5, "refus de base : {}", c.rebase_refused);
    assert!(
        c.size_removals_with_entries > 20,
        "{}",
        c.size_removals_with_entries
    );
    assert!(c.deletions_with_entries > 5, "{}", c.deletions_with_entries);
    assert!(
        c.duplicates_with_entries > 5,
        "{}",
        c.duplicates_with_entries
    );
    assert!(c.undos > 200 && c.redos > 50 && c.reloads > 200);
    assert!(c.max_entries >= 10);
}

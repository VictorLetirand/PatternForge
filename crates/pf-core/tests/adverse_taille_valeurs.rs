//! US-09.5c et US-09.7 — campagne adverse « cran proportionnel et valeurs de variables par taille » :
//! 160 séquences à graine fixe de 18 pas sur un segment AB (cinq tailles XS S M L XL, base
//! variable), trois variables P, Q, R, un point C polaire (distance liée à P) et un point D décalé
//! (dy lié à R), des crans à distance fixe, proportionnels et ancrés en proportion.
//!
//! **Oracle indépendant** : le modèle (`Model`) est réécrit ici. Il stocke les expressions sous une
//! forme structurée (`Ex`), les évalue lui-même (récursion avec détection de cycle, division par
//! zéro), tient les positions de base et les écarts de A et de B, calcule la longueur de AB et la
//! position de chaque cran par taille (fixe : sa distance ; proportionnel : `d ÷ longueur en base ×
//! longueur dans la taille` ; ancré : `r × longueur`), et formule le changement de base par
//! l'invariant « les positions évaluées ne changent pas » (la distance écrite d'un cran
//! proportionnel suit). Il décide seul si une commande doit être acceptée ou refusée.
//!
//! Invariants à chaque pas : accepté ⇔ accepté par l'oracle ; refus = octets identiques ; table de
//! variables, positions de A, B, C, D, longueur de AB et distance de chaque cran **dans chaque
//! taille** (1e-6) ; la taille de base se lit comme le projet (`graded_project(base) == projet`) ;
//! relecture JSON identique ; une étape annulable par commande effective (contrôlée en cours de
//! séquence, puis jusqu'au projet initial).
//!
//! Mutations vérifiées à la main : voir `docs/JOURNAL.md` (tranche 104).

use pf_core::persist;
use pf_core::*;
use pf_geometry::{PointMm, Side};
use std::collections::BTreeMap;

const SEQUENCES: u64 = 160;
const STEPS: usize = 18;
const TOL: f64 = 1e-6;
const LABELS: [&str; 5] = ["XS", "S", "M", "L", "XL"];
const NAMES: [&str; 7] = ["P", "Q", "R", "Pa", "pa", "Qa", "Ra"];

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
}

// ---------------------------------------------------------------------------------------------
// Oracle
// ---------------------------------------------------------------------------------------------

/// Expression structurée (le texte en est dérivé, jamais l'inverse).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ex {
    Num(i64),
    /// `<var> * k + c`
    Lin {
        var: usize,
        k: f64,
        c: i64,
    },
    /// `<var> / k` (k = 0 : division par zéro)
    Div {
        var: usize,
        k: i64,
    },
}

impl Ex {
    fn cites(&self, v: usize) -> bool {
        match *self {
            Ex::Num(_) => false,
            Ex::Lin { var, .. } | Ex::Div { var, .. } => var == v,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Fail {
    Cycle,
    Div0,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Nt {
    Fixed(f64),
    Prop(f64),
    Ratio(f64),
}

#[derive(Clone, Debug)]
struct Model {
    labels: Vec<String>,
    base: usize,
    /// Abscisse de base de A et de B (y = 0).
    base_x: [f64; 2],
    /// Écart cumulé (libellé, point) → dx, jamais nul, jamais sur la base.
    delta: BTreeMap<(String, usize), f64>,
    names: [String; 3],
    alive: [bool; 3],
    base_expr: [Ex; 3],
    sized: BTreeMap<(usize, String), Ex>,
    notches: Vec<(NotchId, Nt)>,
    next_label: usize,
}

impl Model {
    fn expr(&self, v: usize, label: Option<&str>) -> Ex {
        label
            .and_then(|l| self.sized.get(&(v, l.to_string())))
            .copied()
            .unwrap_or(self.base_expr[v])
    }

    fn eval_var(
        &self,
        v: usize,
        label: Option<&str>,
        memo: &mut [Option<f64>; 3],
        visiting: &mut [bool; 3],
    ) -> Result<f64, Fail> {
        if let Some(x) = memo[v] {
            return Ok(x);
        }
        if visiting[v] {
            return Err(Fail::Cycle);
        }
        visiting[v] = true;
        let x = match self.expr(v, label) {
            Ex::Num(n) => n as f64,
            Ex::Lin { var, k, c } => self.eval_var(var, label, memo, visiting)? * k + c as f64,
            Ex::Div { var, k } => {
                let x = self.eval_var(var, label, memo, visiting)?;
                if k == 0 {
                    return Err(Fail::Div0);
                }
                x / k as f64
            }
        };
        visiting[v] = false;
        memo[v] = Some(x);
        Ok(x)
    }

    /// Table complète (variables vivantes, dans l'ordre) dans une taille (`None` = base).
    fn table(&self, label: Option<&str>) -> Result<[Option<f64>; 3], Fail> {
        let mut memo = [None; 3];
        let mut out = [None; 3];
        for v in (0..3).filter(|v| self.alive[*v]) {
            out[v] = Some(self.eval_var(v, label, &mut memo, &mut [false; 3])?);
        }
        Ok(out)
    }

    fn labels_with_entries(&self) -> Vec<String> {
        let mut l: Vec<String> = self.sized.keys().map(|(_, l)| l.clone()).collect();
        l.sort();
        l.dedup();
        l
    }

    /// La table est évaluable en base et dans toute taille qui a une valeur propre, et la distance
    /// polaire de C (variable P) y reste > 0 (sinon C ne se calcule pas dans la taille : la valeur
    /// par taille créerait une erreur de géométrie, refusée ; en base : géométrie invalide).
    fn all_tables_ok(&self) -> bool {
        let good = |l: Option<&str>| self.table(l).is_ok_and(|t| t[0].is_some_and(|p| p > 0.0));
        good(None) && self.labels_with_entries().iter().all(|l| good(Some(l)))
    }

    fn x_of(&self, label: &str, pt: usize) -> f64 {
        self.base_x[pt]
            + self
                .delta
                .get(&(label.to_string(), pt))
                .copied()
                .unwrap_or(0.0)
    }

    fn len_of(&self, label: &str) -> f64 {
        self.x_of(label, 1) - self.x_of(label, 0)
    }

    fn base_label(&self) -> String {
        self.labels[self.base].clone()
    }

    fn notch_in(&self, nt: Nt, label: &str) -> f64 {
        let len = self.len_of(label);
        match nt {
            Nt::Fixed(d) => d,
            Nt::Prop(d) => d / self.len_of(&self.base_label()) * len,
            Nt::Ratio(r) => r * len,
        }
    }

    fn text(&self, e: Ex) -> String {
        match e {
            Ex::Num(n) => n.to_string(),
            Ex::Lin { var, k, c } => format!(
                "{} * {} {} {}",
                self.names[var],
                k,
                if c < 0 { "-" } else { "+" },
                c.abs()
            ),
            Ex::Div { var, k } => format!("{} / {}", self.names[var], k),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Banc
// ---------------------------------------------------------------------------------------------

#[derive(Default, Debug)]
struct Counters {
    set_ok: usize,
    set_refused_cycle: usize,
    set_refused_div0: usize,
    set_refused_base: usize,
    set_refused_geometry: usize,
    clear_refused_geometry: usize,
    base_expr_ok: usize,
    base_expr_refused: usize,
    clear: usize,
    grade_ok: usize,
    grade_refused_base: usize,
    prop_set: usize,
    prop_refused_anchor: usize,
    notch_added: usize,
    rebase_ok: usize,
    rebase_with_prop: usize,
    rebase_refused_values: usize,
    size_removed: usize,
    size_removed_with_values: usize,
    size_added: usize,
    rename_ok: usize,
    rename_rewrote_sized: usize,
    rename_refused: usize,
    remove_var_ok: usize,
    remove_var_refused: usize,
    refusals_bytes_checked: usize,
    prop_moved_ge_1mm: usize,
    sized_positions_checked: usize,
    undo_checks: usize,
    steps_effective: usize,
}

struct Ids {
    sheet: SheetId,
    ab: EntityId,
    pts: [PointId; 4], // A, B, C, D
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).expect("projet valide")
}

fn sid(p: &Project, label: &str) -> SizeId {
    p.sizes
        .iter()
        .flat_map(|r| r.sizes.iter())
        .find(|s| s.label == label)
        .unwrap_or_else(|| panic!("taille {label}"))
        .id
}

fn point_at(p: &Project, sheet: SheetId, x: f64, y: f64) -> PointId {
    let s = p.sheet(sheet).unwrap();
    s.points
        .iter()
        .find(|q| {
            let pos = s.position(q.id).unwrap();
            (pos.x - x).abs() < 1e-9 && (pos.y - y).abs() < 1e-9
        })
        .unwrap()
        .id
}

fn build(rng: &mut Rng) -> (Project, Model, Ids) {
    let mut p = Project::new("campagne");
    let sheet = p.sheets[0].id;
    let base = rng.below(5) as usize;
    p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: LABELS.iter().map(|s| s.to_string()).collect(),
        base,
    })
    .unwrap();
    let ab = p
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(PointMm::new(0.0, 0.0)),
            to: PointRef::New(PointMm::new(100.0, 0.0)),
        })
        .unwrap()
        .entities[0];
    let (a, b) = (
        point_at(&p, sheet, 0.0, 0.0),
        point_at(&p, sheet, 100.0, 0.0),
    );
    let m = Model {
        labels: LABELS.iter().map(|s| s.to_string()).collect(),
        base,
        base_x: [0.0, 100.0],
        delta: BTreeMap::new(),
        names: ["P".into(), "Q".into(), "R".into()],
        alive: [true; 3],
        base_expr: [
            Ex::Num(100),
            Ex::Lin {
                var: 0,
                k: 0.5,
                c: 0,
            },
            Ex::Lin {
                var: 1,
                k: 2.0,
                c: -5,
            },
        ],
        sized: BTreeMap::new(),
        notches: Vec::new(),
        next_label: 0,
    };
    for v in 0..3 {
        p.apply(Command::AddVariable {
            name: m.names[v].clone(),
            unit: VariableUnit::Mm,
            expr: m.text(m.base_expr[v]),
        })
        .unwrap();
    }
    let c = p
        .apply(Command::AddPointAt {
            sheet,
            point: PointRef::Derived(PointDefinition::Polar {
                from: a,
                distance_mm: 1.0,
                angle_deg: 90.0,
                reference: None,
            }),
        })
        .unwrap()
        .points[0];
    let d = p
        .apply(Command::AddPointAt {
            sheet,
            point: PointRef::Derived(PointDefinition::Offset {
                from: b,
                dx_mm: 0.0,
                dy_mm: 1.0,
            }),
        })
        .unwrap()
        .points[0];
    for (point, param, expr) in [(c, ParamKind::Distance, "P"), (d, ParamKind::Dy, "R")] {
        p.apply(Command::SetParameterExpr {
            target: ParamTarget {
                sheet,
                point,
                param,
            },
            expr: Some(expr.into()),
        })
        .unwrap();
    }
    (
        p,
        m,
        Ids {
            sheet,
            ab,
            pts: [a, b, c, d],
        },
    )
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= TOL
}

/// Vérifie le projet contre l'oracle, dans toutes les tailles.
fn check(p: &Project, m: &Model, ids: &Ids, c: &mut Counters, ctx: &str) {
    p.validate()
        .unwrap_or_else(|e| panic!("{ctx} : invalide : {e}"));
    let text = bytes(p);
    assert_eq!(persist::from_json(&text).unwrap(), *p, "{ctx} : relecture");
    let base_id = sid(p, &m.base_label());
    assert_eq!(
        p.graded_project(Some(base_id)).unwrap(),
        *p,
        "{ctx} : la base se lit comme le projet"
    );
    assert_eq!(
        p.derived_for_size(ids.sheet, None).unwrap().points,
        p.derived_for_size(ids.sheet, Some(base_id)).unwrap().points,
        "{ctx} : None = base"
    );
    for label in &m.labels {
        let size = sid(p, label);
        let t = m
            .table(Some(label))
            .unwrap_or_else(|f| panic!("{ctx} : l'oracle n'évalue pas {label} : {f:?}"));
        let got = p
            .variable_values_for_size(Some(size))
            .unwrap_or_else(|e| panic!("{ctx} : table {label} : {e}"));
        let want: Vec<(String, f64)> = (0..3)
            .filter(|v| m.alive[*v])
            .map(|v| (m.names[v].clone(), t[v].unwrap()))
            .collect();
        assert_eq!(got.len(), want.len(), "{ctx} : {label} : taille de table");
        for (g, w) in got.iter().zip(&want) {
            assert!(
                g.0 == w.0 && near(g.1, w.1),
                "{ctx} : {label} : {g:?} ≠ {w:?}"
            );
        }
        let (xa, xb) = (m.x_of(label, 0), m.x_of(label, 1));
        let (vp, vr) = (t[0].unwrap(), t[2].unwrap());
        let r = p
            .derived_for_size(ids.sheet, Some(size))
            .unwrap_or_else(|e| panic!("{ctx} : lecture {label} : {e}"));
        let at = |id: PointId| {
            r.points
                .iter()
                .find(|q| q.0 == id)
                .unwrap_or_else(|| {
                    panic!(
                        "{ctx} : {label} : point {id:?} non calculé ; table {t:?} ; {:?}",
                        r.issues
                    )
                })
                .1
        };
        for (pt, (x, y)) in [
            (ids.pts[0], (xa, 0.0)),
            (ids.pts[1], (xb, 0.0)),
            (ids.pts[2], (xa, vp)),
            (ids.pts[3], (xb, vr)),
        ] {
            let q = at(pt);
            assert!(
                near(q.x, x) && near(q.y, y),
                "{ctx} : {label} : point {pt:?} ({} ; {}) ≠ ({x} ; {y})",
                q.x,
                q.y
            );
            c.sized_positions_checked += 1;
        }
        let len = r
            .path_lengths
            .iter()
            .find(|l| l.0 == ids.ab)
            .unwrap()
            .1
            .clone()
            .unwrap();
        assert!(near(len, xb - xa), "{ctx} : {label} : longueur {len}");
        for (nid, nt) in &m.notches {
            let got = r
                .notches
                .iter()
                .find(|n| n.notch == *nid)
                .unwrap()
                .distance_mm
                .clone()
                .unwrap();
            let want = m.notch_in(*nt, label);
            assert!(
                near(got, want),
                "{ctx} : {label} : cran {nid:?} {got} ≠ {want}"
            );
            if let Nt::Prop(d) = nt {
                if (want - d).abs() > 1.0 {
                    c.prop_moved_ge_1mm += 1;
                }
            }
        }
    }
}

enum Cmd {
    SetSized(usize, String, Ex),
    Clear(usize, String),
    SetBaseExpr(usize, Ex),
    Grade(usize, String, i64),
    SetProp(usize, bool),
    AddFixed(f64),
    AddRatio(f64),
    SetBase(String),
    RemoveSize(String),
    AddSize(String),
    Rename(usize, String),
    RemoveVar(usize),
}

fn pick_alive(rng: &mut Rng, m: &Model) -> usize {
    loop {
        let v = rng.below(3) as usize;
        if m.alive[v] {
            return v;
        }
    }
}

fn gen_ex(rng: &mut Rng, m: &Model) -> Ex {
    match rng.below(8) {
        0 | 1 => Ex::Num(rng.range(-20, 200)),
        2..=4 => Ex::Lin {
            var: pick_alive(rng, m),
            k: [0.5, 1.0, 2.0, 3.0][rng.below(4) as usize],
            c: rng.range(-40, 20),
        },
        5 | 6 => Ex::Div {
            var: pick_alive(rng, m),
            k: [2, 4, 0, 0][rng.below(4) as usize],
        },
        _ => Ex::Lin {
            var: pick_alive(rng, m),
            k: 1.0,
            c: 0,
        },
    }
}

fn gen(rng: &mut Rng, m: &Model) -> Cmd {
    let label = |rng: &mut Rng| m.labels[rng.below(m.labels.len() as u64) as usize].clone();
    match rng.below(34) {
        0..=6 => Cmd::SetSized(pick_alive(rng, m), label(rng), gen_ex(rng, m)),
        7 | 8 => Cmd::Clear(pick_alive(rng, m), label(rng)),
        9..=11 => Cmd::SetBaseExpr(pick_alive(rng, m), gen_ex(rng, m)),
        12..=16 => Cmd::Grade(rng.below(2) as usize, label(rng), rng.range(-15, 15)),
        17..=20 if !m.notches.is_empty() => Cmd::SetProp(
            rng.below(m.notches.len() as u64) as usize,
            rng.below(4) != 0,
        ),
        21 | 22 if m.notches.len() < 6 => Cmd::AddFixed(rng.range(5, 60) as f64),
        23 if m.notches.len() < 6 => Cmd::AddRatio([0.25, 0.5, 0.75][rng.below(3) as usize]),
        24..=26 => Cmd::SetBase(label(rng)),
        27 => Cmd::RemoveSize(label(rng)),
        28 => Cmd::AddSize(format!("N{}", m.next_label)),
        29..=31 => Cmd::Rename(
            pick_alive(rng, m),
            NAMES[rng.below(NAMES.len() as u64) as usize].to_string(),
        ),
        32 => Cmd::RemoveVar(1),
        _ if m.notches.len() < 6 => Cmd::AddFixed(rng.range(5, 60) as f64),
        _ => Cmd::Clear(pick_alive(rng, m), label(rng)),
    }
}

/// Oracle d'une commande : (commande du cœur, modèle suivant ou `None` si refus attendu). Met à
/// jour les compteurs d'intention.
fn expect(
    cmd: &Cmd,
    m: &Model,
    p: &Project,
    ids: &Ids,
    c: &mut Counters,
) -> (Command, Option<Model>) {
    let mut n = m.clone();
    match cmd {
        Cmd::SetSized(v, label, ex) => {
            let core = Command::SetVariableSizeExpr {
                name: m.names[*v].clone(),
                size: sid(p, label),
                expr: m.text(*ex),
            };
            if *label == m.base_label() {
                c.set_refused_base += 1;
                return (core, None);
            }
            n.sized.insert((*v, label.clone()), *ex);
            match n.table(Some(label)) {
                Ok(_) if !n.all_tables_ok() => {
                    c.set_refused_geometry += 1;
                    (core, None)
                }
                Ok(_) => {
                    c.set_ok += 1;
                    (core, Some(n))
                }
                Err(Fail::Cycle) => {
                    c.set_refused_cycle += 1;
                    (core, None)
                }
                Err(Fail::Div0) => {
                    c.set_refused_div0 += 1;
                    (core, None)
                }
            }
        }
        Cmd::Clear(v, label) => {
            n.sized.remove(&(*v, label.clone()));
            let core = Command::ClearVariableSizeExpr {
                name: m.names[*v].clone(),
                size: sid(p, label),
            };
            // Retirer une valeur peut rendre la taille impossible (P dépend d'une autre valeur).
            if n.all_tables_ok() {
                c.clear += 1;
                (core, Some(n))
            } else {
                c.clear_refused_geometry += 1;
                (core, None)
            }
        }
        Cmd::SetBaseExpr(v, ex) => {
            n.base_expr[*v] = *ex;
            let core = Command::SetVariableExpr {
                name: m.names[*v].clone(),
                expr: m.text(*ex),
            };
            if n.all_tables_ok() {
                c.base_expr_ok += 1;
                (core, Some(n))
            } else {
                c.base_expr_refused += 1;
                (core, None)
            }
        }
        Cmd::Grade(pt, label, dx) => {
            let core = Command::SetPointGrading {
                sheet: ids.sheet,
                point: ids.pts[*pt],
                size: sid(p, label),
                dx_mm: *dx as f64,
                dy_mm: 0.0,
            };
            if *label == m.base_label() {
                c.grade_refused_base += 1;
                return (core, None);
            }
            c.grade_ok += 1;
            if *dx == 0 {
                n.delta.remove(&(label.clone(), *pt));
            } else {
                n.delta.insert((label.clone(), *pt), *dx as f64);
            }
            (core, Some(n))
        }
        Cmd::SetProp(i, on) => {
            let (nid, nt) = m.notches[*i];
            let core = Command::SetNotchProportional {
                sheet: ids.sheet,
                notch: nid,
                proportional: *on,
            };
            match (nt, *on) {
                (Nt::Ratio(_), true) => {
                    c.prop_refused_anchor += 1;
                    (core, None)
                }
                (Nt::Ratio(_), false) => (core, Some(n)),
                (Nt::Fixed(d) | Nt::Prop(d), on) => {
                    c.prop_set += 1;
                    n.notches[*i].1 = if on { Nt::Prop(d) } else { Nt::Fixed(d) };
                    (core, Some(n))
                }
            }
        }
        Cmd::AddFixed(d) => (
            Command::AddNotch {
                sheet: ids.sheet,
                entity: ids.ab,
                distance_mm: *d,
                side: Side::Left,
                length_mm: 5.0,
                style: NotchStyle::Single,
            },
            // L'identifiant est connu après coup : `n.notches` reçoit une entrée provisoire.
            Some({
                n.notches.push((NotchId(0), Nt::Fixed(*d)));
                n
            }),
        ),
        Cmd::AddRatio(r) => (
            Command::AddAnchoredNotch {
                sheet: ids.sheet,
                entity: ids.ab,
                anchor: NotchAnchor::Ratio { ratio: *r },
                side: Side::Left,
                length_mm: 5.0,
                style: NotchStyle::Single,
            },
            Some({
                n.notches.push((NotchId(0), Nt::Ratio(*r)));
                n
            }),
        ),
        Cmd::SetBase(label) => {
            let core = Command::SetBaseSize {
                size: sid(p, label),
            };
            let new = m.labels.iter().position(|l| l == label).unwrap();
            if new == m.base {
                return (core, Some(n));
            }
            if !m.sized.is_empty() {
                c.rebase_refused_values += 1;
                return (core, None);
            }
            c.rebase_ok += 1;
            let old_len = m.len_of(&m.base_label());
            let new_len = m.len_of(label);
            if n.notches.iter().any(|(_, k)| matches!(k, Nt::Prop(_))) {
                c.rebase_with_prop += 1;
            }
            for (_, k) in &mut n.notches {
                if let Nt::Prop(d) = k {
                    *d = *d / old_len * new_len;
                }
            }
            let nb = [m.x_of(label, 0), m.x_of(label, 1)];
            let mut delta = BTreeMap::new();
            for l in m.labels.iter().filter(|l| *l != label) {
                for (pt, base_x) in nb.iter().enumerate() {
                    let d = m.x_of(l, pt) - base_x;
                    if d != 0.0 {
                        delta.insert((l.clone(), pt), d);
                    }
                }
            }
            n.base_x = nb;
            n.delta = delta;
            n.base = new;
            (core, Some(n))
        }
        Cmd::RemoveSize(label) => {
            let core = Command::RemoveSize {
                size: sid(p, label),
            };
            if *label == m.base_label() || m.labels.len() == 1 {
                return (core, None);
            }
            c.size_removed += 1;
            if m.sized.keys().any(|(_, l)| l == label) {
                c.size_removed_with_values += 1;
            }
            n.labels.retain(|l| l != label);
            n.base = n.labels.iter().position(|l| *l == m.base_label()).unwrap();
            n.delta.retain(|(l, _), _| l != label);
            n.sized.retain(|(_, l), _| l != label);
            (core, Some(n))
        }
        Cmd::AddSize(label) => {
            c.size_added += 1;
            n.labels.push(label.clone());
            n.next_label += 1;
            (
                Command::AddSize {
                    range: p.sizes[0].id,
                    label: label.clone(),
                    at: None,
                },
                Some(n),
            )
        }
        Cmd::Rename(v, to) => {
            let core = Command::RenameVariable {
                from: m.names[*v].clone(),
                to: to.clone(),
            };
            let clash =
                (0..3).any(|k| k != *v && m.alive[k] && m.names[k].eq_ignore_ascii_case(to));
            if clash {
                c.rename_refused += 1;
                return (core, None);
            }
            if m.names[*v] != *to {
                c.rename_ok += 1;
                if m.sized.values().any(|e| (0..3).any(|k| e.cites(k))) {
                    c.rename_rewrote_sized += 1;
                }
            }
            n.names[*v] = to.clone();
            (core, Some(n))
        }
        Cmd::RemoveVar(v) => {
            let core = Command::RemoveVariable {
                name: m.names[*v].clone(),
            };
            if !m.alive[*v] {
                return (core, None);
            }
            let cited = (0..3).filter(|k| k != v && m.alive[*k]).any(|k| {
                m.base_expr[k].cites(*v)
                    || m.sized.iter().any(|((sv, _), e)| *sv == k && e.cites(*v))
            });
            // P et R sont aussi cités par les formules liées de C et de D : jamais supprimables.
            if cited || *v != 1 {
                c.remove_var_refused += 1;
                return (core, None);
            }
            c.remove_var_ok += 1;
            n.alive[*v] = false;
            n.sized.retain(|(sv, _), _| sv != v);
            (core, Some(n))
        }
    }
}

fn sequence(seed: u64, c: &mut Counters) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    for _ in 0..4 {
        rng.step();
    }
    let (p0, mut m, ids) = build(&mut rng);
    let mut doc = Document::new(p0.clone());
    check(doc.project(), &m, &ids, c, &format!("seed {seed} initial"));
    let mut effective = 0usize;
    for step in 0..STEPS {
        let cmd = gen(&mut rng, &m);
        let (core, want) = expect(&cmd, &m, doc.project(), &ids, c);
        let ctx = format!("seed {seed} pas {step} {core:?}");
        let before = doc.project().clone();
        let before_bytes = bytes(&before);
        let got = doc.execute(core);
        assert_eq!(
            got.is_ok(),
            want.is_some(),
            "{ctx} : cœur {:?}, oracle {}",
            got.as_ref().err().map(|e| e.to_string()),
            if want.is_some() { "accepte" } else { "refuse" }
        );
        match (got, want) {
            (Ok(out), Some(mut next)) => {
                // Cran ajouté : l'identifiant vient du cœur.
                if matches!(cmd, Cmd::AddFixed(_) | Cmd::AddRatio(_)) {
                    c.notch_added += 1;
                    next.notches.last_mut().unwrap().0 = out.notches[0];
                }
                m = next;
                if *doc.project() != before {
                    effective += 1;
                    c.steps_effective += 1;
                    if step % 3 == 0 {
                        let after = doc.project().clone();
                        assert!(doc.undo(), "{ctx} : annuler");
                        assert_eq!(*doc.project(), before, "{ctx} : annuler = état d'avant");
                        assert!(doc.redo(), "{ctx} : rétablir");
                        assert_eq!(*doc.project(), after, "{ctx} : rétablir");
                        c.undo_checks += 1;
                    }
                }
            }
            (Err(_), None) => {
                assert_eq!(*doc.project(), before, "{ctx} : refus avec trace");
                assert_eq!(bytes(doc.project()), before_bytes, "{ctx} : octets");
                c.refusals_bytes_checked += 1;
            }
            _ => unreachable!(),
        }
        check(doc.project(), &m, &ids, c, &ctx);
    }
    // Une étape annulable par commande effective, puis le projet initial.
    for _ in 0..effective {
        assert!(doc.undo(), "seed {seed} : annuler jusqu'au début");
    }
    assert!(!doc.undo(), "seed {seed} : trop d'étapes");
    assert_eq!(*doc.project(), p0, "seed {seed} : retour au projet initial");
}

#[test]
fn campagne_adverse_taille_valeurs() {
    let mut c = Counters::default();
    for seed in 1..=SEQUENCES {
        sequence(seed, &mut c);
    }
    // Compteurs : les contrôles forts ont réellement été exercés.
    assert!(
        c.set_refused_geometry >= 1,
        "géométrie {}",
        c.set_refused_geometry
    );
    assert!(c.set_ok >= 150, "set_ok {}", c.set_ok);
    assert!(c.set_refused_cycle >= 10, "cycle {}", c.set_refused_cycle);
    assert!(c.set_refused_div0 >= 10, "div0 {}", c.set_refused_div0);
    assert!(c.set_refused_base >= 20, "base {}", c.set_refused_base);
    assert!(c.base_expr_ok >= 50, "base_expr_ok {}", c.base_expr_ok);
    assert!(
        c.base_expr_refused >= 10,
        "base_expr_refused {}",
        c.base_expr_refused
    );
    assert!(c.clear >= 40, "clear {}", c.clear);
    assert!(c.grade_ok >= 200, "grade_ok {}", c.grade_ok);
    assert!(
        c.grade_refused_base >= 20,
        "grade base {}",
        c.grade_refused_base
    );
    assert!(c.prop_set >= 100, "prop_set {}", c.prop_set);
    assert!(
        c.prop_refused_anchor >= 10,
        "prop anchor {}",
        c.prop_refused_anchor
    );
    assert!(c.notch_added >= 150, "notches {}", c.notch_added);
    assert!(c.rebase_ok >= 30, "rebase {}", c.rebase_ok);
    assert!(
        c.rebase_with_prop >= 10,
        "rebase prop {}",
        c.rebase_with_prop
    );
    assert!(
        c.rebase_refused_values >= 10,
        "rebase refused {}",
        c.rebase_refused_values
    );
    assert!(c.size_removed >= 20, "size removed {}", c.size_removed);
    assert!(
        c.size_removed_with_values >= 3,
        "removed with values {}",
        c.size_removed_with_values
    );
    assert!(c.size_added >= 20, "size added {}", c.size_added);
    assert!(c.rename_ok >= 40, "rename {}", c.rename_ok);
    assert!(
        c.rename_rewrote_sized >= 10,
        "rename sized {}",
        c.rename_rewrote_sized
    );
    assert!(
        c.rename_refused >= 10,
        "rename refused {}",
        c.rename_refused
    );
    assert!(c.remove_var_ok >= 3, "remove ok {}", c.remove_var_ok);
    assert!(
        c.remove_var_refused >= 10,
        "remove refused {}",
        c.remove_var_refused
    );
    assert!(
        c.refusals_bytes_checked >= 150,
        "refusals {}",
        c.refusals_bytes_checked
    );
    assert!(
        c.prop_moved_ge_1mm >= 500,
        "prop moved {}",
        c.prop_moved_ge_1mm
    );
    assert!(
        c.sized_positions_checked >= 20_000,
        "positions {}",
        c.sized_positions_checked
    );
    assert!(c.undo_checks >= 150, "undo {}", c.undo_checks);
    assert!(c.steps_effective >= 1000, "effective {}", c.steps_effective);
    eprintln!(
        "adverse_taille_valeurs : {SEQUENCES} séquences × {STEPS} pas ; \
         valeurs posées {} (refus cycle {}, div0 {}, base {}) ; expr de base {} / refus {} ; \
         écarts {} ; proportionnels {} (déplacés ≥ 1 mm : {}) ; changements de base {} (dont {} avec \
         proportionnel, {} refusés) ; tailles retirées {} ; renommages {} ; refus vérifiés {} ; \
         positions {} ; annulations {}",
        c.set_ok,
        c.set_refused_cycle,
        c.set_refused_div0,
        c.set_refused_base,
        c.base_expr_ok,
        c.base_expr_refused,
        c.grade_ok,
        c.prop_set,
        c.prop_moved_ge_1mm,
        c.rebase_ok,
        c.rebase_with_prop,
        c.rebase_refused_values,
        c.size_removed,
        c.rename_ok,
        c.refusals_bytes_checked,
        c.sized_positions_checked,
        c.undo_checks
    );
}

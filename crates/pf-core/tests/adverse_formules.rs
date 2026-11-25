//! US-03.1b — campagne adverse « valeurs maintenues » : 160 séquences à graine fixe de 60 pas
//! (ajouts de variables et de points construits, liage / déliage, changements de variables,
//! renommages, suppressions, valeurs modifiées directement, annuler, rétablir, sérialiser-relire).
//!
//! **Oracle indépendant** : les formules sont générées comme des **arbres** (`T`), rendues en texte
//! entièrement parenthésé pour la commande, et évaluées par un évaluateur récursif écrit ici —
//! aucun appel à `pf_core::expr` ni à `evaluate_variables`. L'oracle tient sa propre table de
//! variables (arbres), ses points (libres, décalés, en direction), ses liens, et recalcule
//! directement les positions attendues (somme des décalages, cos / sin). Pour chaque commande
//! l'oracle prédit d'abord accepté / refusé et l'état résultant (variables valides, aucune formule
//! non évaluable, géométrie non dégénérée, 1e6 mm), le code doit être d'accord.
//!
//! Invariants après chaque pas : projet valide, tableau de variables = oracle, liste des formules
//! = oracle (texte, ordre), valeur de chaque paramètre = oracle **à l'égalité exacte**, valeur d'un
//! paramètre lié = évaluation de sa formule, positions = oracle (1e-6 mm), refus = octets
//! identiques, étape créée si et seulement si l'état change, annuler / rétablir exacts à l'octet,
//! relecture identique (octets et valeur).
//!
//! Mutations vérifiées à la main : voir `docs/JOURNAL.md` (tranche 88).

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;
use std::collections::HashMap;

const SEQUENCES: u64 = 160;
const STEPS: usize = 60;

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
// Expressions : arbre, rendu, évaluation récursive
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum T {
    N(String),
    Id(String),
    Neg(Box<T>),
    Bin(char, Box<T>, Box<T>),
    Min(Box<T>, Box<T>),
    Max(Box<T>, Box<T>),
    Abs(Box<T>),
    Sqrt(Box<T>),
}

fn render(t: &T) -> String {
    match t {
        T::N(s) | T::Id(s) => s.clone(),
        T::Neg(x) => format!("-({})", render(x)),
        T::Bin(op, l, r) => format!("({}{}{})", render(l), op, render(r)),
        T::Min(l, r) => format!("min({};{})", render(l), render(r)),
        T::Max(l, r) => format!("max({};{})", render(l), render(r)),
        T::Abs(x) => format!("abs({})", render(x)),
        T::Sqrt(x) => format!("sqrt({})", render(x)),
    }
}

fn rename_ids(t: &mut T, from: &str, to: &str) {
    match t {
        T::N(_) => {}
        T::Id(s) => {
            if s == from {
                *s = to.to_string();
            }
        }
        T::Neg(x) | T::Abs(x) | T::Sqrt(x) => rename_ids(x, from, to),
        T::Bin(_, l, r) | T::Min(l, r) | T::Max(l, r) => {
            rename_ids(l, from, to);
            rename_ids(r, from, to);
        }
    }
}

fn uses(t: &T, name: &str) -> bool {
    match t {
        T::N(_) => false,
        T::Id(s) => s == name,
        T::Neg(x) | T::Abs(x) | T::Sqrt(x) => uses(x, name),
        T::Bin(_, l, r) | T::Min(l, r) | T::Max(l, r) => uses(l, name) || uses(r, name),
    }
}

/// Évaluation récursive ; `None` = refusée (variable inconnue, division par zéro, racine d'un
/// négatif, valeur non finie).
fn eval(t: &T, lookup: &dyn Fn(&str) -> Option<f64>) -> Option<f64> {
    let fin = |v: f64| if v.is_finite() { Some(v) } else { None };
    match t {
        T::N(s) => s.replace(',', ".").parse::<f64>().ok().and_then(fin),
        T::Id(s) => lookup(s),
        T::Neg(x) => fin(-eval(x, lookup)?),
        T::Bin(op, l, r) => {
            let (a, b) = (eval(l, lookup)?, eval(r, lookup)?);
            fin(match op {
                '+' => a + b,
                '-' => a - b,
                '*' => a * b,
                _ => {
                    if b == 0.0 {
                        return None;
                    }
                    a / b
                }
            })
        }
        T::Min(l, r) => Some(eval(l, lookup)?.min(eval(r, lookup)?)),
        T::Max(l, r) => Some(eval(l, lookup)?.max(eval(r, lookup)?)),
        T::Abs(x) => Some(eval(x, lookup)?.abs()),
        T::Sqrt(x) => {
            let v = eval(x, lookup)?;
            if v < 0.0 {
                None
            } else {
                Some(v.sqrt())
            }
        }
    }
}

const NUMBERS: [&str; 16] = [
    "1", "2", "3", "4", "5", "10", "12", "0,5", "2,5", "7.5", "100", "0", "0", "1000", "30", "90",
];

/// Arbre aléatoire ; `id_pct` = chance qu'une feuille soit un nom de variable.
fn gen(rng: &mut Rng, names: &[String], depth: usize, id_pct: u64) -> T {
    if depth == 0 || rng.chance(30) {
        return if names.is_empty() || !rng.chance(id_pct) {
            T::N(rng.pick(&NUMBERS).to_string())
        } else if rng.chance(4) {
            T::Id("zz".into())
        } else {
            T::Id(rng.pick(names).clone())
        };
    }
    let sub = |rng: &mut Rng| Box::new(gen(rng, names, depth - 1, id_pct));
    match rng.below(9) {
        0 => T::Bin('+', sub(rng), sub(rng)),
        1 => T::Bin('-', sub(rng), sub(rng)),
        2 => T::Bin('*', sub(rng), sub(rng)),
        3 => T::Bin('/', sub(rng), sub(rng)),
        4 => T::Neg(sub(rng)),
        5 => T::Min(sub(rng), sub(rng)),
        6 => T::Max(sub(rng), sub(rng)),
        7 => T::Abs(sub(rng)),
        _ => T::Sqrt(sub(rng)),
    }
}

// ---------------------------------------------------------------------------------------------
// Oracle : variables, points, liens
// ---------------------------------------------------------------------------------------------

const RESERVED: [&str; 5] = ["min", "max", "abs", "sqrt", "pi"];

fn valid_name(n: &str) -> bool {
    let c: Vec<char> = n.chars().collect();
    !c.is_empty()
        && c.len() <= 40
        && (c[0].is_ascii_alphabetic() || c[0] == '_')
        && c.iter().all(|x| x.is_ascii_alphanumeric() || *x == '_')
        && !RESERVED.iter().any(|w| w.eq_ignore_ascii_case(n))
}

#[derive(Clone, Debug, PartialEq)]
enum Def {
    Free(f64, f64),
    Offset { from: PointId, dx: f64, dy: f64 },
    Polar { from: PointId, d: f64, a: f64 },
}

#[derive(Clone, Debug, PartialEq)]
struct OPoint {
    id: PointId,
    def: Def,
}

#[derive(Clone, Debug, PartialEq, Default)]
struct Oracle {
    vars: Vec<(String, T)>,
    pts: Vec<OPoint>,
    binds: Vec<(PointId, ParamKind, T)>,
}

fn param_of(def: &Def, k: ParamKind) -> Option<f64> {
    match (def, k) {
        (Def::Polar { d, .. }, ParamKind::Distance) => Some(*d),
        (Def::Polar { a, .. }, ParamKind::Angle) => Some(*a),
        (Def::Offset { dx, .. }, ParamKind::Dx) => Some(*dx),
        (Def::Offset { dy, .. }, ParamKind::Dy) => Some(*dy),
        _ => None,
    }
}

fn set_param(def: &mut Def, k: ParamKind, v: f64) {
    match (def, k) {
        (Def::Polar { d, .. }, ParamKind::Distance) => *d = v,
        (Def::Polar { a, .. }, ParamKind::Angle) => *a = v,
        (Def::Offset { dx, .. }, ParamKind::Dx) => *dx = v,
        (Def::Offset { dy, .. }, ParamKind::Dy) => *dy = v,
        _ => {}
    }
}

impl Oracle {
    /// Valeurs des variables par récursion mémoïsée ; `None` si cycle, inconnue ou non évaluable.
    fn var_values(&self) -> Option<HashMap<String, f64>> {
        fn go(
            o: &Oracle,
            name: &str,
            memo: &mut HashMap<String, f64>,
            visiting: &mut Vec<String>,
        ) -> Option<f64> {
            if let Some(v) = memo.get(name) {
                return Some(*v);
            }
            if visiting.iter().any(|v| v == name) {
                return None;
            }
            let (_, tree) = o.vars.iter().find(|(n, _)| n == name)?;
            visiting.push(name.to_string());
            let v = {
                // L'évaluation d'un nom appelle `go` de façon récursive.
                let cell = std::cell::RefCell::new((&mut *memo, &mut *visiting));
                eval(tree, &|n| {
                    let mut g = cell.borrow_mut();
                    let (m, v) = &mut *g;
                    go(o, n, m, v)
                })
            };
            visiting.pop();
            let v = v?;
            memo.insert(name.to_string(), v);
            Some(v)
        }
        let mut memo = HashMap::new();
        for (n, _) in &self.vars {
            go(self, n, &mut memo, &mut Vec::new())?;
        }
        Some(memo)
    }

    /// Positions attendues ; `None` si une géométrie est dégénérée ou hors de ±1e6 mm.
    fn positions(&self) -> Option<HashMap<PointId, (f64, f64)>> {
        fn pos(
            o: &Oracle,
            id: PointId,
            memo: &mut HashMap<PointId, (f64, f64)>,
        ) -> Option<(f64, f64)> {
            if let Some(v) = memo.get(&id) {
                return Some(*v);
            }
            let p = o.pts.iter().find(|p| p.id == id)?;
            let r = match &p.def {
                Def::Free(x, y) => (*x, *y),
                Def::Offset { from, dx, dy } => {
                    let f = pos(o, *from, memo)?;
                    (f.0 + dx, f.1 + dy)
                }
                Def::Polar { from, d, a } => {
                    if !d.is_finite() || *d <= 1e-9 || !a.is_finite() {
                        return None;
                    }
                    let f = pos(o, *from, memo)?;
                    let (s, c) = a.to_radians().sin_cos();
                    (f.0 + d * c, f.1 + d * s)
                }
            };
            if !(r.0.is_finite() && r.1.is_finite() && r.0.abs() <= 1e6 && r.1.abs() <= 1e6) {
                return None;
            }
            memo.insert(id, r);
            Some(r)
        }
        let mut memo = HashMap::new();
        for p in &self.pts {
            pos(self, p.id, &mut memo)?;
        }
        Some(memo)
    }

    /// Réévalue tous les liens (variables valides, formules évaluables) puis vérifie la géométrie.
    fn realize(&mut self) -> bool {
        let Some(vals) = self.var_values() else {
            return false;
        };
        let binds = self.binds.clone();
        for (pt, k, tree) in &binds {
            let Some(v) = eval(tree, &|n| vals.get(n).copied()) else {
                return false;
            };
            if let Some(p) = self.pts.iter_mut().find(|p| p.id == *pt) {
                set_param(&mut p.def, *k, v);
            }
        }
        self.positions().is_some()
    }

    fn name_taken(&self, name: &str, except: Option<&str>) -> bool {
        self.vars
            .iter()
            .any(|(n, _)| Some(n.as_str()) != except && n.eq_ignore_ascii_case(name))
    }

    fn constructed(&self) -> Vec<(PointId, bool)> {
        self.pts
            .iter()
            .filter_map(|p| match p.def {
                Def::Free(..) => None,
                Def::Offset { .. } => Some((p.id, true)),
                Def::Polar { .. } => Some((p.id, false)),
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------------------------
// Exécution
// ---------------------------------------------------------------------------------------------

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

#[derive(Default)]
struct Stats {
    accepted: u64,
    refused: u64,
    /// Changement de variable accepté qui a déplacé au moins un paramètre lié, par paramètre.
    moved: HashMap<&'static str, u64>,
    /// Changement de variable refusé alors que la table seule restait valide (formule ou
    /// géométrie en cause).
    refused_by_formula: u64,
    released: u64,
    undone: u64,
    redone: u64,
    reloaded: u64,
    binds_ok: u64,
}

struct Run {
    doc: Document,
    sheet: SheetId,
    o: Oracle,
    hist: Vec<(Oracle, String)>,
    redo: Vec<(Oracle, String)>,
}

enum Pred {
    Accept(Oracle),
    /// Création d'un point : toujours acceptée ; fusionnée avec un point existant si
    /// `Outcome::points` est vide (rien ne change alors).
    Create(Def),
    Refuse,
}

impl Run {
    fn new() -> Run {
        let mut p = Project::new("Campagne");
        let s = p.sheets[0].id;
        let a = p
            .apply(Command::AddPoint {
                sheet: s,
                at: PointMm::new(0.0, 0.0),
            })
            .unwrap()
            .points[0];
        let o = Oracle {
            pts: vec![OPoint {
                id: a,
                def: Def::Free(0.0, 0.0),
            }],
            ..Oracle::default()
        };
        Run {
            doc: Document::new(p),
            sheet: s,
            o,
            hist: Vec::new(),
            redo: Vec::new(),
        }
    }

    fn names(&self) -> Vec<String> {
        self.o.vars.iter().map(|(n, _)| n.clone()).collect()
    }

    /// Exécute, compare à la prédiction, met l'historique de l'oracle à jour.
    fn exec(&mut self, what: &str, cmd: Command, pred: Pred, st: &mut Stats) -> Option<Outcome> {
        let before = bytes(self.doc.project());
        let before_p = self.doc.project().clone();
        let result = self.doc.execute(cmd);
        match (result, pred) {
            (Ok(out), pred @ (Pred::Accept(_) | Pred::Create(_))) => {
                let next = match pred {
                    Pred::Accept(next) => next,
                    Pred::Create(def) => {
                        let mut next = self.o.clone();
                        if let Some(&id) = out.points.first() {
                            next.pts.push(OPoint { id, def });
                        }
                        next
                    }
                    Pred::Refuse => unreachable!(),
                };
                let after = bytes(self.doc.project());
                let changed = after != before;
                assert_eq!(
                    changed,
                    next != self.o,
                    "{what} : étape créée = {changed}, l'oracle prédit {}",
                    next != self.o
                );
                if changed {
                    self.hist.push((self.o.clone(), before));
                    self.redo.clear();
                }
                self.o = next;
                st.accepted += 1;
                Some(out)
            }
            (Err(_), Pred::Refuse) => {
                assert_eq!(
                    bytes(self.doc.project()),
                    before,
                    "{what} : trace d'un refus"
                );
                assert_eq!(*self.doc.project(), before_p, "{what} : trace d'un refus");
                st.refused += 1;
                None
            }
            (Ok(_), Pred::Refuse) => panic!("{what} : acceptée alors que l'oracle la refuse"),
            (Err(e), _) => panic!("{what} : refusée à tort : {e}"),
        }
    }

    fn check(&self, what: &str) {
        let p = self.doc.project();
        p.validate()
            .unwrap_or_else(|e| panic!("{what} : projet invalide : {e}"));
        // Variables : texte et ordre.
        let got: Vec<(String, String)> = p
            .variables
            .iter()
            .map(|v| (v.name.clone(), v.expr.clone()))
            .collect();
        let want: Vec<(String, String)> = self
            .o
            .vars
            .iter()
            .map(|(n, t)| (n.clone(), render(t)))
            .collect();
        assert_eq!(got, want, "{what} : table de variables");
        // Formules : texte et ordre.
        let sheet = p.sheet(self.sheet).unwrap();
        let got: Vec<(PointId, ParamKind, String)> = sheet
            .formulas
            .iter()
            .map(|f| (f.point, f.param, f.expr.clone()))
            .collect();
        let want: Vec<(PointId, ParamKind, String)> = self
            .o
            .binds
            .iter()
            .map(|(pt, k, t)| (*pt, *k, render(t)))
            .collect();
        assert_eq!(got, want, "{what} : formules");
        // Valeurs : égalité exacte ; valeur liée = évaluation de la formule.
        let vals = self.o.var_values().expect("variables de l'oracle");
        assert_eq!(sheet.points.len(), self.o.pts.len(), "{what} : points");
        for op in &self.o.pts {
            for k in [
                ParamKind::Distance,
                ParamKind::Angle,
                ParamKind::Dx,
                ParamKind::Dy,
            ] {
                assert_eq!(
                    sheet.parameter_value(op.id, k),
                    param_of(&op.def, k),
                    "{what} : valeur de {:?} {:?}",
                    op.id,
                    k
                );
            }
        }
        for (pt, k, tree) in &self.o.binds {
            let want = eval(tree, &|n| vals.get(n).copied()).expect("formule évaluable");
            assert_eq!(
                sheet.parameter_value(*pt, *k),
                Some(want),
                "{what} : valeur liée {:?} {:?} = {}",
                pt,
                k,
                render(tree)
            );
        }
        // Positions.
        let want = self.o.positions().expect("géométrie de l'oracle");
        for op in &self.o.pts {
            let got = sheet.position(op.id).unwrap();
            let w = want[&op.id];
            assert!(
                (got.x - w.0).abs() < 1e-6 && (got.y - w.1).abs() < 1e-6,
                "{what} : position {:?} = ({} ; {}), attendu {:?}",
                op.id,
                got.x,
                got.y,
                w
            );
        }
        // Relecture identique.
        let text = bytes(p);
        let again =
            persist::from_json(&text).unwrap_or_else(|e| panic!("{what} : relecture : {e}"));
        assert_eq!(bytes(&again), text, "{what} : relecture non identique");
        assert_eq!(again, *p, "{what} : relecture différente");
        assert_eq!(
            self.doc.can_undo(),
            !self.hist.is_empty(),
            "{what} : annuler"
        );
        assert_eq!(
            self.doc.can_redo(),
            !self.redo.is_empty(),
            "{what} : rétablir"
        );
    }

    fn step(&mut self, rng: &mut Rng, st: &mut Stats) {
        let s = self.sheet;
        let names = self.names();
        let constructed = self.o.constructed();
        let roll = rng.below(100);
        // Début de séquence : de quoi lier.
        let roll = if !self.redo.is_empty() && roll < 14 {
            97
        } else if names.is_empty() && roll < 70 {
            0
        } else if constructed.is_empty() && roll < 70 {
            44
        } else {
            roll
        };
        match roll {
            // AddVariable.
            0..=9 => {
                let name = if rng.chance(12) {
                    rng.pick(&["1x", "a b", "min", "PI", "", "é"]).to_string()
                } else {
                    rng.pick(&["a", "b", "c", "d", "L", "W", "H", "k", "A", "l"])
                        .to_string()
                };
                let tree = gen(rng, &names, 2, 40);
                let pad = if rng.chance(20) { "  " } else { "" };
                let cmd = Command::AddVariable {
                    name: name.clone(),
                    unit: VariableUnit::Mm,
                    expr: format!("{pad}{}{pad}", render(&tree)),
                };
                let mut next = self.o.clone();
                let pred = if valid_name(&name) && !self.o.name_taken(&name, None) {
                    next.vars.push((name, tree));
                    if next.realize() {
                        Pred::Accept(next)
                    } else {
                        Pred::Refuse
                    }
                } else {
                    Pred::Refuse
                };
                self.exec("AddVariable", cmd, pred, st);
            }
            // SetVariableExpr.
            10..=26 => {
                let name = if rng.chance(4) {
                    "nope".to_string()
                } else {
                    rng.pick(&names).clone()
                };
                let tree = gen(rng, &names, 2, 40);
                let cmd = Command::SetVariableExpr {
                    name: name.clone(),
                    expr: format!(" {} ", render(&tree)),
                };
                self.var_change("SetVariableExpr", cmd, rng, st, |o| {
                    match o.vars.iter_mut().find(|(n, _)| *n == name) {
                        Some(v) => {
                            v.1 = tree.clone();
                            true
                        }
                        None => false,
                    }
                });
            }
            // RenameVariable.
            27..=31 => {
                let from = if rng.chance(4) {
                    "nope".to_string()
                } else {
                    rng.pick(&names).clone()
                };
                let to = rng
                    .pick(&[
                        "e", "f", "g", "LL", "l", "a", "A", "Largeur", "min", "1z", "L",
                    ])
                    .to_string();
                let cmd = Command::RenameVariable {
                    from: from.clone(),
                    to: to.clone(),
                };
                self.var_change("RenameVariable", cmd, rng, st, |o| {
                    if !o.vars.iter().any(|(n, _)| *n == from)
                        || !valid_name(&to)
                        || o.name_taken(&to, Some(&from))
                    {
                        return false;
                    }
                    for (n, t) in o.vars.iter_mut() {
                        rename_ids(t, &from, &to);
                        if *n == from {
                            *n = to.clone();
                        }
                    }
                    for (_, _, t) in o.binds.iter_mut() {
                        rename_ids(t, &from, &to);
                    }
                    true
                });
            }
            // RemoveVariable.
            32..=36 => {
                let name = if rng.chance(5) {
                    "nope".to_string()
                } else {
                    rng.pick(&names).clone()
                };
                let cmd = Command::RemoveVariable { name: name.clone() };
                self.var_change("RemoveVariable", cmd, rng, st, |o| {
                    let used = o.vars.iter().any(|(_, t)| uses(t, &name))
                        || o.binds.iter().any(|(_, _, t)| uses(t, &name));
                    if !o.vars.iter().any(|(n, _)| *n == name) || used {
                        return false;
                    }
                    o.vars.retain(|(n, _)| *n != name);
                    true
                });
            }
            // Points : libre, décalé, en direction.
            37..=39 => {
                let (x, y) = (rng.below(101) as f64 - 50.0, rng.below(101) as f64 - 50.0);
                let cmd = Command::AddPoint {
                    sheet: s,
                    at: PointMm::new(x, y),
                };
                self.exec("AddPoint", cmd, Pred::Create(Def::Free(x, y)), st);
            }
            40..=47 => {
                let from = rng.pick(&self.o.pts).id;
                let (dx, dy) = (rng.below(81) as f64 - 40.0, rng.below(81) as f64 - 40.0);
                let cmd = Command::AddPointAt {
                    sheet: s,
                    point: PointRef::Derived(PointDefinition::Offset {
                        from,
                        dx_mm: dx,
                        dy_mm: dy,
                    }),
                };
                self.exec(
                    "AddOffset",
                    cmd,
                    Pred::Create(Def::Offset { from, dx, dy }),
                    st,
                );
            }
            48..=55 => {
                let from = rng.pick(&self.o.pts).id;
                let d = *rng.pick(&[10.0, 20.0, 30.0, 40.0, 50.0]);
                let a = *rng.pick(&[0.0, 30.0, 45.0, 60.0, 90.0, 135.0, 180.0, 270.0]);
                let cmd = Command::AddPointAt {
                    sheet: s,
                    point: PointRef::Derived(PointDefinition::Polar {
                        from,
                        distance_mm: d,
                        angle_deg: a,
                        reference: None,
                    }),
                };
                self.exec("AddPolar", cmd, Pred::Create(Def::Polar { from, d, a }), st);
            }
            // Lier une formule.
            56..=78 => {
                let unknown_point = rng.chance(3);
                let polars: Vec<(PointId, bool)> =
                    constructed.iter().copied().filter(|c| !c.1).collect();
                let (pt, is_offset) = if unknown_point || constructed.is_empty() {
                    (PointId(999_999), true)
                } else if !polars.is_empty() && rng.chance(50) {
                    *rng.pick(&polars)
                } else {
                    *rng.pick(&constructed)
                };
                let wrong = rng.chance(12);
                let k = match (is_offset, wrong) {
                    (true, false) | (false, true) => *rng.pick(&[ParamKind::Dx, ParamKind::Dy]),
                    _ => *rng.pick(&[ParamKind::Distance, ParamKind::Angle]),
                };
                let tree = gen(rng, &names, 2, 80);
                let cmd = Command::SetParameterExpr {
                    target: ParamTarget {
                        sheet: s,
                        point: pt,
                        param: k,
                    },
                    expr: Some(format!(" {} ", render(&tree))),
                };
                let mut next = self.o.clone();
                let valid_target = self
                    .o
                    .pts
                    .iter()
                    .find(|p| p.id == pt)
                    .is_some_and(|p| param_of(&p.def, k).is_some());
                let pred = if valid_target {
                    match next.binds.iter_mut().find(|b| b.0 == pt && b.1 == k) {
                        Some(b) => b.2 = tree.clone(),
                        None => next.binds.push((pt, k, tree.clone())),
                    }
                    if next.realize() {
                        Pred::Accept(next)
                    } else {
                        Pred::Refuse
                    }
                } else {
                    Pred::Refuse
                };
                if self.exec("Bind", cmd, pred, st).is_some() {
                    st.binds_ok += 1;
                }
            }
            // Délier.
            79..=82 => {
                let (pt, is_offset) = if constructed.is_empty() || rng.chance(10) {
                    (PointId(999_999), true)
                } else if self.o.binds.is_empty() || rng.chance(25) {
                    *rng.pick(&constructed)
                } else {
                    let b = rng.pick(&self.o.binds).clone();
                    (b.0, matches!(b.1, ParamKind::Dx | ParamKind::Dy))
                };
                let k = match (is_offset, rng.chance(10)) {
                    (true, false) | (false, true) => *rng.pick(&[ParamKind::Dx, ParamKind::Dy]),
                    _ => *rng.pick(&[ParamKind::Distance, ParamKind::Angle]),
                };
                let cmd = Command::SetParameterExpr {
                    target: ParamTarget {
                        sheet: s,
                        point: pt,
                        param: k,
                    },
                    expr: None,
                };
                let mut next = self.o.clone();
                let valid_target = self
                    .o
                    .pts
                    .iter()
                    .find(|p| p.id == pt)
                    .is_some_and(|p| param_of(&p.def, k).is_some());
                let pred = if valid_target {
                    next.binds.retain(|b| !(b.0 == pt && b.1 == k));
                    Pred::Accept(next)
                } else {
                    Pred::Refuse
                };
                self.exec("Unbind", cmd, pred, st);
            }
            // Valeurs modifiées directement : délie les paramètres liés dont la valeur change.
            83..=88 => {
                if constructed.is_empty() {
                    return;
                }
                let (pt, is_offset) = *rng.pick(&constructed);
                let cur = self.o.pts.iter().find(|p| p.id == pt).unwrap().def.clone();
                let keep = rng.chance(35);
                let (values, new_def) = if is_offset {
                    let Def::Offset { from, dx, dy } = cur else {
                        unreachable!()
                    };
                    let (nx, ny) = if keep {
                        (dx, dy + 1.0)
                    } else {
                        (rng.below(61) as f64 - 30.0, rng.below(61) as f64 - 30.0)
                    };
                    (
                        vec![nx, ny],
                        Def::Offset {
                            from,
                            dx: nx,
                            dy: ny,
                        },
                    )
                } else {
                    let Def::Polar { from, d, a } = cur else {
                        unreachable!()
                    };
                    let (nd, na) = if keep {
                        (d, a + 15.0)
                    } else {
                        (
                            *rng.pick(&[0.0, 5.0, 12.0, 25.0, -3.0]),
                            *rng.pick(&[0.0, 20.0, 100.0, 359.0]),
                        )
                    };
                    (vec![nd, na], Def::Polar { from, d: nd, a: na })
                };
                let cmd = Command::SetPointValues {
                    sheet: s,
                    point: pt,
                    values,
                };
                let mut next = self.o.clone();
                let old = self.o.pts.iter().find(|p| p.id == pt).unwrap().def.clone();
                next.pts.iter_mut().find(|p| p.id == pt).unwrap().def = new_def.clone();
                let kinds: &[ParamKind] = if is_offset {
                    &[ParamKind::Dx, ParamKind::Dy]
                } else {
                    &[ParamKind::Distance, ParamKind::Angle]
                };
                let before = next.binds.len();
                next.binds.retain(|b| {
                    !(b.0 == pt
                        && kinds.contains(&b.1)
                        && param_of(&old, b.1) != param_of(&new_def, b.1))
                });
                let released = before - next.binds.len();
                let pred = if next.realize() {
                    Pred::Accept(next)
                } else {
                    Pred::Refuse
                };
                if self.exec("SetPointValues", cmd, pred, st).is_some() {
                    st.released += released as u64;
                }
            }
            // Déplacer un point libre.
            89..=90 => {
                let frees: Vec<PointId> = self
                    .o
                    .pts
                    .iter()
                    .filter(|p| matches!(p.def, Def::Free(..)))
                    .map(|p| p.id)
                    .collect();
                if frees.is_empty() {
                    return;
                }
                let pt = *rng.pick(&frees);
                let (x, y) = (rng.below(101) as f64 - 50.0, rng.below(101) as f64 - 50.0);
                let cmd = Command::MovePoint {
                    sheet: s,
                    point: pt,
                    to: PointMm::new(x, y),
                };
                let mut next = self.o.clone();
                next.pts.iter_mut().find(|p| p.id == pt).unwrap().def = Def::Free(x, y);
                let pred = if next.realize() {
                    Pred::Accept(next)
                } else {
                    Pred::Refuse
                };
                self.exec("MovePoint", cmd, pred, st);
            }
            // Supprimer un point sans enfant.
            91..=92 => {
                let leaves: Vec<PointId> = self
                    .o
                    .pts
                    .iter()
                    .filter(|p| {
                        !self.o.pts.iter().any(|q| match q.def {
                            Def::Offset { from, .. } | Def::Polar { from, .. } => from == p.id,
                            Def::Free(..) => false,
                        })
                    })
                    .map(|p| p.id)
                    .collect();
                if leaves.len() < 2 {
                    return;
                }
                let pt = *rng.pick(&leaves);
                let cmd = Command::Delete {
                    sheet: s,
                    points: vec![pt],
                    entities: vec![],
                };
                let mut next = self.o.clone();
                next.pts.retain(|p| p.id != pt);
                next.binds.retain(|b| b.0 != pt);
                self.exec("Delete", cmd, Pred::Accept(next), st);
            }
            // Annuler / rétablir.
            93..=96 => {
                let cur = (self.o.clone(), bytes(self.doc.project()));
                let ok = self.doc.undo();
                assert_eq!(ok, !self.hist.is_empty(), "annuler : disponibilité");
                if let Some((o, b)) = self.hist.pop() {
                    assert_eq!(bytes(self.doc.project()), b, "annuler : octets");
                    self.o = o;
                    self.redo.push(cur);
                    st.undone += 1;
                }
            }
            97..=98 => {
                let cur = (self.o.clone(), bytes(self.doc.project()));
                let ok = self.doc.redo();
                assert_eq!(ok, !self.redo.is_empty(), "rétablir : disponibilité");
                if let Some((o, b)) = self.redo.pop() {
                    assert_eq!(bytes(self.doc.project()), b, "rétablir : octets");
                    self.o = o;
                    self.hist.push(cur);
                    st.redone += 1;
                }
            }
            // Fermer et rouvrir : la relecture continue la séquence (historique repartant de zéro).
            _ => {
                let text = bytes(self.doc.project());
                let again = persist::from_json(&text).unwrap();
                assert_eq!(bytes(&again), text);
                self.doc = Document::new(again);
                self.hist.clear();
                self.redo.clear();
                st.reloaded += 1;
            }
        }
    }

    /// Commande qui modifie la table de variables : prédiction par `edit` (variables) puis
    /// réévaluation de tous les liens ; compte les déplacements et les refus dus aux formules.
    fn var_change(
        &mut self,
        what: &str,
        cmd: Command,
        _rng: &mut Rng,
        st: &mut Stats,
        edit: impl FnOnce(&mut Oracle) -> bool,
    ) {
        let mut next = self.o.clone();
        let edited = edit(&mut next);
        let table_ok = edited && next.var_values().is_some();
        let before = self.o.clone();
        let pred = if table_ok && next.realize() {
            Pred::Accept(next.clone())
        } else {
            Pred::Refuse
        };
        if table_ok && matches!(pred, Pred::Refuse) {
            st.refused_by_formula += 1;
        }
        if self.exec(what, cmd, pred, st).is_some() {
            for (pt, k, _) in &next.binds {
                let was = before
                    .pts
                    .iter()
                    .find(|p| p.id == *pt)
                    .and_then(|p| param_of(&p.def, *k));
                let now = next
                    .pts
                    .iter()
                    .find(|p| p.id == *pt)
                    .and_then(|p| param_of(&p.def, *k));
                if was != now {
                    *st.moved.entry(kind_name(*k)).or_default() += 1;
                }
            }
        }
    }
}

fn kind_name(k: ParamKind) -> &'static str {
    k.name()
}

#[test]
fn campagne_adverse_des_valeurs_maintenues() {
    let mut st = Stats::default();
    for seq in 0..SEQUENCES {
        let mut rng = Rng(0x5EED_0310_0000_0001 ^ (seq.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1));
        let mut run = Run::new();
        run.check("départ");
        for step in 0..STEPS {
            run.step(&mut rng, &mut st);
            run.check(&format!("séquence {seq}, pas {step}"));
        }
    }
    eprintln!(
        "acceptées {} refusées {} dont par formule {} ; liages {} ; déplacés {:?} ; déliés {} ; \
         annulées {} rétablies {} ; relectures {}",
        st.accepted,
        st.refused,
        st.refused_by_formula,
        st.binds_ok,
        st.moved,
        st.released,
        st.undone,
        st.redone,
        st.reloaded
    );
    assert!(st.accepted > 2500 && st.refused > 800);
    assert!(st.refused_by_formula >= 40, "refus par formule trop rares");
    assert!(st.binds_ok >= 600);
    assert!(st.released >= 40);
    assert!(st.undone >= 100 && st.redone >= 40 && st.reloaded >= 40);
    for k in ["distance", "angle", "dx", "dy"] {
        assert!(
            st.moved.get(k).copied().unwrap_or(0) >= 60,
            "paramètre « {k} » trop peu déplacé par un changement de variable : {:?}",
            st.moved
        );
    }
}

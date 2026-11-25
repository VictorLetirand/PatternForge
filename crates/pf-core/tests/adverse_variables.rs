//! US-03.1a — campagne adverse « variables » : 160 séquences à graine fixe de 40 pas
//! (ajouts, modifications d'expression et d'unité, renommages, suppressions, annuler, rétablir,
//! sérialiser-relire), noms valides et invalides, expressions bien formées, corrompues,
//! profondes, démesurées, avec division par zéro, racine négative, débordement, cycles.
//!
//! **Oracle indépendant** : un petit analyseur par *précédence montante* (le code testé est un
//! descendant récursif par niveaux), un évaluateur d'arbre, une réécriture de références par
//! balayage de caractères, un détecteur de cycle par **élimination des nœuds sans dépendance**
//! (le code testé fait un parcours en profondeur) et une validation de noms écrite ici. Aucun
//! appel à `pf_core::expr` ni à `evaluate_variables`. Pour chaque commande, l'oracle prédit
//! d'abord accepté / refusé et le tableau résultant ; le code doit être d'accord, valeur à valeur
//! (égalité exacte des `f64`). Invariants après chaque pas : aucune valeur non finie, aucun
//! cycle, noms uniques (casse comprise), annuler / rétablir exacts à l'octet, valeurs
//! identiques après relecture, refus sans trace (octets identiques).
//!
//! Mutations vérifiées à la main : voir `docs/JOURNAL.md` (tranche 87).

use pf_core::persist;
use pf_core::*;

const SEQUENCES: u64 = 160;
const STEPS: usize = 40;

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
struct V {
    name: String,
    unit: VariableUnit,
    expr: String,
}

#[derive(Debug)]
enum T {
    N(f64),
    Id(String),
    Neg(Box<T>),
    Bin(char, Box<T>, Box<T>),
    F(String, Vec<T>),
}

#[derive(Debug, Clone, PartialEq)]
enum Tk {
    Num(f64),
    Id(String),
    Sym(char),
}

fn lex(s: &str) -> Option<Vec<Tk>> {
    let c: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if ch.is_whitespace() {
            i += 1;
        } else if "+-*/();".contains(ch) {
            out.push(Tk::Sym(ch));
            i += 1;
        } else if ch.is_ascii_digit()
            || ((ch == '.' || ch == ',') && i + 1 < c.len() && c[i + 1].is_ascii_digit())
        {
            let mut int = String::new();
            while i < c.len() && c[i].is_ascii_digit() {
                int.push(c[i]);
                i += 1;
            }
            let mut frac = String::new();
            if i + 1 < c.len() && (c[i] == '.' || c[i] == ',') && c[i + 1].is_ascii_digit() {
                i += 1;
                while i < c.len() && c[i].is_ascii_digit() {
                    frac.push(c[i]);
                    i += 1;
                }
            }
            let text = format!(
                "{}.{}",
                if int.is_empty() { "0" } else { &int },
                if frac.is_empty() { "0" } else { &frac }
            );
            let v: f64 = text.parse().ok()?;
            if !v.is_finite() {
                return None;
            }
            out.push(Tk::Num(v));
        } else if ch.is_ascii_alphabetic() || ch == '_' {
            let mut id = String::new();
            while i < c.len() && (c[i].is_ascii_alphanumeric() || c[i] == '_') {
                id.push(c[i]);
                i += 1;
            }
            out.push(Tk::Id(id));
        } else {
            return None;
        }
    }
    Some(out)
}

struct P {
    t: Vec<Tk>,
    i: usize,
}

impl P {
    fn peek(&self) -> Option<&Tk> {
        self.t.get(self.i)
    }
    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(&Tk::Sym(c)) {
            self.i += 1;
            true
        } else {
            false
        }
    }
    /// Précédence montante : `+ -` = 1, `* /` = 2, tous associatifs à gauche.
    fn expr(&mut self, min: u8, depth: usize) -> Option<T> {
        let mut lhs = self.atom(depth)?;
        loop {
            let (op, prec) = match self.peek() {
                Some(Tk::Sym(c @ ('+' | '-'))) => (*c, 1),
                Some(Tk::Sym(c @ ('*' | '/'))) => (*c, 2),
                _ => break,
            };
            if prec < min {
                break;
            }
            self.i += 1;
            let rhs = self.expr(prec + 1, depth)?;
            lhs = T::Bin(op, Box::new(lhs), Box::new(rhs));
        }
        Some(lhs)
    }
    fn atom(&mut self, depth: usize) -> Option<T> {
        let tk = self.peek()?.clone();
        self.i += 1;
        match tk {
            Tk::Num(v) => Some(T::N(v)),
            Tk::Sym('-') => {
                if depth + 1 > 32 {
                    return None;
                }
                Some(T::Neg(Box::new(self.atom(depth + 1)?)))
            }
            Tk::Sym('(') => {
                if depth + 1 > 32 {
                    return None;
                }
                let e = self.expr(1, depth + 1)?;
                self.eat(')').then_some(e)
            }
            Tk::Id(id) => {
                if self.peek() == Some(&Tk::Sym('(')) {
                    let (lo, hi) = match id.as_str() {
                        "min" | "max" => (2, 8),
                        "abs" | "sqrt" => (1, 1),
                        _ => return None,
                    };
                    if depth + 1 > 32 {
                        return None;
                    }
                    self.i += 1;
                    let mut args = Vec::new();
                    if self.peek() != Some(&Tk::Sym(')')) {
                        loop {
                            args.push(self.expr(1, depth + 1)?);
                            if self.eat(';') {
                                continue;
                            }
                            break;
                        }
                    }
                    if !self.eat(')') || args.len() < lo || args.len() > hi {
                        return None;
                    }
                    Some(T::F(id, args))
                } else if id == "pi" {
                    Some(T::N(std::f64::consts::PI))
                } else if ["min", "max", "abs", "sqrt"].contains(&id.as_str()) {
                    None
                } else {
                    Some(T::Id(id))
                }
            }
            Tk::Sym(_) => None,
        }
    }
}

fn parse(s: &str) -> Option<T> {
    if s.chars().count() > 500 {
        return None;
    }
    let toks = lex(s)?;
    let mut p = P { t: toks, i: 0 };
    let t = p.expr(1, 0)?;
    (p.i == p.t.len()).then_some(t)
}

fn deps(t: &T, out: &mut Vec<String>) {
    match t {
        T::N(_) => {}
        T::Id(n) => out.push(n.clone()),
        T::Neg(a) => deps(a, out),
        T::Bin(_, a, b) => {
            deps(a, out);
            deps(b, out);
        }
        T::F(_, args) => args.iter().for_each(|a| deps(a, out)),
    }
}

fn fin(v: f64) -> Option<f64> {
    v.is_finite().then_some(if v == 0.0 { 0.0 } else { v })
}

fn eval(t: &T, env: &dyn Fn(&str) -> f64) -> Option<f64> {
    match t {
        T::N(v) => Some(*v),
        T::Id(n) => Some(env(n)),
        T::Neg(a) => fin(-eval(a, env)?),
        T::Bin(op, a, b) => {
            let (x, y) = (eval(a, env)?, eval(b, env)?);
            fin(match op {
                '+' => x + y,
                '-' => x - y,
                '*' => x * y,
                _ => {
                    if y == 0.0 {
                        return None;
                    }
                    x / y
                }
            })
        }
        T::F(f, args) => {
            let xs: Option<Vec<f64>> = args.iter().map(|a| eval(a, env)).collect();
            let xs = xs?;
            fin(match f.as_str() {
                "abs" => xs[0].abs(),
                "sqrt" => {
                    if xs[0] < 0.0 {
                        return None;
                    }
                    xs[0].sqrt()
                }
                "min" => xs.iter().copied().fold(f64::INFINITY, f64::min),
                _ => xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            })
        }
    }
}

fn valid_name(n: &str) -> bool {
    let c: Vec<char> = n.chars().collect();
    !c.is_empty()
        && c.len() <= 40
        && (c[0].is_ascii_alphabetic() || c[0] == '_')
        && c.iter().all(|x| x.is_ascii_alphanumeric() || *x == '_')
        && !["min", "max", "abs", "sqrt", "pi"].contains(&n.to_lowercase().as_str())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rej {
    TooMany,
    Name,
    Duplicate,
    Syntax,
    Unknown,
    Cycle,
    Eval,
}

/// Valeurs d'un tableau, ou la raison du refus.
fn oracle(vars: &[V]) -> Result<Vec<f64>, Rej> {
    if vars.len() > 500 {
        return Err(Rej::TooMany);
    }
    for (i, v) in vars.iter().enumerate() {
        if !valid_name(&v.name) {
            return Err(Rej::Name);
        }
        if vars[..i]
            .iter()
            .any(|w| w.name.to_lowercase() == v.name.to_lowercase())
        {
            return Err(Rej::Duplicate);
        }
    }
    let trees: Vec<T> = vars
        .iter()
        .map(|v| parse(&v.expr).ok_or(Rej::Syntax))
        .collect::<Result<_, _>>()?;
    let mut edges: Vec<Vec<usize>> = Vec::new();
    for t in &trees {
        let mut names = Vec::new();
        deps(t, &mut names);
        let mut e = Vec::new();
        for n in names {
            e.push(vars.iter().position(|v| v.name == n).ok_or(Rej::Unknown)?);
        }
        edges.push(e);
    }
    // Élimination : retirer à tour de rôle les variables dont toutes les dépendances sont
    // déjà retirées ; ce qui reste à la fin appartient à un cycle ou en dépend.
    let mut done: Vec<Option<f64>> = vec![None; vars.len()];
    let mut removed = vec![false; vars.len()];
    loop {
        let mut progress = false;
        for i in 0..vars.len() {
            if !removed[i] && edges[i].iter().all(|&j| removed[j]) {
                let env = |n: &str| {
                    let j = vars.iter().position(|v| v.name == n).unwrap();
                    done[j].unwrap()
                };
                done[i] = Some(eval(&trees[i], &env).ok_or(Rej::Eval)?);
                removed[i] = true;
                progress = true;
            }
        }
        if !progress {
            break;
        }
    }
    if removed.iter().any(|r| !r) {
        return Err(Rej::Cycle);
    }
    Ok(done.into_iter().map(Option::unwrap).collect())
}

fn rewrite(text: &str, from: &str, to: &str) -> String {
    let c: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i].is_ascii_alphabetic() || c[i] == '_' {
            let mut run = String::new();
            while i < c.len() && (c[i].is_ascii_alphanumeric() || c[i] == '_') {
                run.push(c[i]);
                i += 1;
            }
            out.push_str(if run == from { to } else { &run });
        } else {
            out.push(c[i]);
            i += 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Génération
// ---------------------------------------------------------------------------------------------

const VALID_NAMES: [&str; 17] = [
    "A", "B", "C", "D", "E", "a", "b", "Tour", "tour", "B2", "B22", "AB", "x_1", "_y", "pi2",
    "minimum", "Aisance",
];
const BAD_NAMES: [&str; 9] = ["min", "pi", "Max", "SQRT", "é", "1x", "", "a b", "a-b"];

fn gen_name(rng: &mut Rng) -> String {
    if rng.chance(86) {
        rng.pick(&VALID_NAMES).to_string()
    } else if rng.chance(15) {
        "n".repeat(41)
    } else {
        rng.pick(&BAD_NAMES).to_string()
    }
}

fn sp(rng: &mut Rng) -> &'static str {
    ["", " ", "  "][rng.below(3) as usize]
}

fn gen_expr(rng: &mut Rng, names: &[String], depth: u32) -> String {
    let roll = rng.below(100);
    if depth >= 4 || roll < 40 {
        return match rng.below(100) {
            0..=44 => rng
                .pick(&[
                    "0", "1", "2", "5", "10", "12", "100", "920", "0,5", ".5", "2.5", "3,75", "40",
                    "1000",
                ])
                .to_string(),
            45..=87 if !names.is_empty() => {
                if rng.chance(92) {
                    rng.pick(names).clone()
                } else {
                    rng.pick(&["Q", "zz", "a_b"]).to_string()
                }
            }
            88..=92 => "pi".into(),
            _ => "7".into(),
        };
    }
    let l = gen_expr(rng, names, depth + 1);
    match roll {
        40..=74 => {
            let op = *rng.pick(&["+", "-", "*", "/", "-", "+"]);
            let r = gen_expr(rng, names, depth + 1);
            format!("{l}{}{op}{}{r}", sp(rng), sp(rng))
        }
        75..=82 => format!("({l})"),
        83..=86 => format!("-{l}"),
        87..=90 => format!("{l}/({l}-{l})"), // division par zéro (sauf valeur dégénérée)
        91..=93 => format!("sqrt({l})"),
        94..=95 => format!("abs({l})"),
        _ => {
            let f = *rng.pick(&["min", "max"]);
            let n = if rng.chance(5) {
                1
            } else {
                2 + rng.below(3) as usize
            };
            let mut args = vec![l];
            for _ in 1..n {
                args.push(gen_expr(rng, names, depth + 1));
            }
            format!("{f}({})", args.join(*rng.pick(&[";", "; "])))
        }
    }
}

fn corrupt(rng: &mut Rng, s: &str) -> String {
    let mut c: Vec<char> = s.chars().collect();
    let junk = [
        '(', ')', ';', ',', '+', '*', ' ', 'e', '$', 'é', '1', 'A', '-', '/', '.',
    ];
    let at = if c.is_empty() {
        0
    } else {
        rng.below(c.len() as u64) as usize
    };
    match rng.below(3) {
        0 if !c.is_empty() => {
            c.remove(at);
        }
        1 => c.insert(at.min(c.len()), *rng.pick(&junk)),
        _ if !c.is_empty() => c[at] = *rng.pick(&junk),
        _ => {}
    }
    c.into_iter().collect()
}

fn gen_text(rng: &mut Rng, names: &[String]) -> String {
    let roll = rng.below(100);
    if roll < 2 {
        let n = 28 + rng.below(9) as usize;
        return format!("{}1{}", "(".repeat(n), ")".repeat(n));
    }
    if roll < 3 {
        let n = 29 + rng.below(6) as usize;
        return format!("{}1", "-".repeat(n));
    }
    if roll < 5 {
        let d = "9".repeat(150);
        return format!("{d}*{d}*{}", if rng.chance(50) { &d } else { "2" });
    }
    if roll < 6 {
        return "1+".repeat(260);
    }
    let e = gen_expr(rng, names, 0);
    if rng.chance(10) {
        corrupt(rng, &e)
    } else {
        e
    }
}

// ---------------------------------------------------------------------------------------------
// Séquences
// ---------------------------------------------------------------------------------------------

#[derive(Default, Debug)]
struct Counters {
    steps: usize,
    accepted: usize,
    noops: usize,
    refused: usize,
    cycles: usize,
    unknown: usize,
    eval: usize,
    syntax: usize,
    names: usize,
    duplicates: usize,
    renames_rewriting: usize,
    removes: usize,
    removes_refused_in_use: usize,
    undos: usize,
    redos: usize,
    reloads: usize,
    max_table: usize,
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

fn mirror_of(p: &Project) -> Vec<V> {
    p.variables
        .iter()
        .map(|v| V {
            name: v.name.clone(),
            unit: v.unit,
            expr: v.expr.clone(),
        })
        .collect()
}

fn check_state(p: &Project, mirror: &[V], c: &mut Counters) {
    assert_eq!(mirror_of(p), mirror, "table différente du miroir");
    c.max_table = c.max_table.max(mirror.len());
    // Noms uniques sans tenir compte de la casse.
    let mut lower: Vec<String> = mirror.iter().map(|v| v.name.to_lowercase()).collect();
    lower.sort();
    lower.dedup();
    assert_eq!(lower.len(), mirror.len(), "noms en double");
    // Valeurs : oracle (qui exclut tout cycle) = code, exactement, toutes finies.
    let expected = oracle(mirror).expect("l'état du projet doit toujours être valide");
    let got = p.variable_values().expect("variable_values");
    assert_eq!(got.len(), expected.len());
    for ((name, v), (w, m)) in got.iter().zip(expected.iter().zip(mirror)) {
        assert_eq!(name, &m.name);
        assert!(v.is_finite(), "{name} non fini");
        assert_eq!(v, w, "valeur de {name} ({})", m.expr);
        if *v == 0.0 {
            assert!(v.is_sign_positive(), "{name} = -0");
        }
    }
}

fn count_rej(c: &mut Counters, r: Rej) {
    match r {
        Rej::Cycle => c.cycles += 1,
        Rej::Unknown => c.unknown += 1,
        Rej::Eval => c.eval += 1,
        Rej::Syntax => c.syntax += 1,
        Rej::Name => c.names += 1,
        Rej::Duplicate => c.duplicates += 1,
        Rej::TooMany => {}
    }
}

fn run_sequence(seed: u64, c: &mut Counters) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut doc = Document::new(Project::new("Adverse"));
    let mut mirror: Vec<V> = Vec::new();
    let mut hist: Vec<(Vec<V>, String)> = Vec::new();
    let mut redo: Vec<(Vec<V>, String)> = Vec::new();
    for step in 0..STEPS {
        c.steps += 1;
        let names: Vec<String> = mirror.iter().map(|v| v.name.clone()).collect();
        let before = bytes(doc.project());
        let before_p = doc.project().clone();
        let roll = rng.below(100);
        // (commande, tableau candidat selon l'oracle ; None = refus certain)
        let planned: Option<(Command, Option<Vec<V>>, bool)> = match roll {
            0..=24 => {
                let name = gen_name(&mut rng);
                let unit =
                    *rng.pick(&[VariableUnit::Mm, VariableUnit::Deg, VariableUnit::Unitless]);
                let expr = gen_text(&mut rng, &names);
                let mut cand = mirror.clone();
                cand.push(V {
                    name: name.clone(),
                    unit,
                    expr: expr.trim().to_string(),
                });
                Some((Command::AddVariable { name, unit, expr }, Some(cand), false))
            }
            25..=48 if !mirror.is_empty() => {
                let target = rng.pick(&names).clone();
                let expr = if rng.chance(4) {
                    mirror
                        .iter()
                        .find(|v| v.name == target)
                        .unwrap()
                        .expr
                        .clone()
                } else {
                    gen_text(&mut rng, &names)
                };
                let mut cand = mirror.clone();
                for v in &mut cand {
                    if v.name == target {
                        v.expr = expr.trim().to_string();
                    }
                }
                Some((
                    Command::SetVariableExpr { name: target, expr },
                    Some(cand),
                    false,
                ))
            }
            49..=61 if !mirror.is_empty() => {
                let from = if rng.chance(4) {
                    "Inconnue".to_string()
                } else {
                    rng.pick(&names).clone()
                };
                let to = if rng.chance(8) {
                    from.clone()
                } else {
                    gen_name(&mut rng)
                };
                let cmd = Command::RenameVariable {
                    from: from.clone(),
                    to: to.clone(),
                };
                let exists = names.contains(&from);
                let cand = (exists
                    && valid_name(&to)
                    && !mirror
                        .iter()
                        .any(|v| v.name != from && v.name.to_lowercase() == to.to_lowercase()))
                .then(|| {
                    mirror
                        .iter()
                        .map(|v| V {
                            name: if v.name == from {
                                to.clone()
                            } else {
                                v.name.clone()
                            },
                            unit: v.unit,
                            expr: rewrite(&v.expr, &from, &to),
                        })
                        .collect::<Vec<_>>()
                });
                let rewriting = cand.as_ref().is_some_and(|k| {
                    from != to && k.iter().zip(&mirror).any(|(a, b)| a.expr != b.expr)
                });
                Some((cmd, cand, rewriting))
            }
            62..=71 if !mirror.is_empty() => {
                let target = if rng.chance(3) {
                    "Inconnue".to_string()
                } else {
                    rng.pick(&names).clone()
                };
                let cmd = Command::RemoveVariable {
                    name: target.clone(),
                };
                let exists = names.contains(&target);
                let used = mirror.iter().any(|v| {
                    v.name != target && {
                        let mut d = Vec::new();
                        deps(&parse(&v.expr).unwrap(), &mut d);
                        d.contains(&target)
                    }
                });
                if exists && used {
                    c.removes_refused_in_use += 1;
                }
                let cand = (exists && !used).then(|| {
                    mirror
                        .iter()
                        .filter(|v| v.name != target)
                        .cloned()
                        .collect()
                });
                Some((cmd, cand, false))
            }
            72..=75 if !mirror.is_empty() => {
                let target = rng.pick(&names).clone();
                let unit =
                    *rng.pick(&[VariableUnit::Mm, VariableUnit::Deg, VariableUnit::Unitless]);
                let mut cand = mirror.clone();
                for v in &mut cand {
                    if v.name == target {
                        v.unit = unit;
                    }
                }
                Some((
                    Command::SetVariableUnit { name: target, unit },
                    Some(cand),
                    false,
                ))
            }
            _ => None,
        };
        match planned {
            Some((cmd, cand, rewriting)) => {
                let verdict = cand.as_ref().map(|k| oracle(k));
                let accepted = matches!(verdict, Some(Ok(_)));
                let is_remove = matches!(cmd, Command::RemoveVariable { .. });
                let result = doc.execute(cmd);
                assert_eq!(
                    result.is_ok(),
                    accepted,
                    "seed {seed} pas {step} : verdict {verdict:?} / code {result:?}"
                );
                if accepted {
                    let cand = cand.unwrap();
                    c.accepted += 1;
                    if is_remove {
                        c.removes += 1;
                    }
                    if cand == mirror {
                        assert_eq!(bytes(doc.project()), before, "sans effet : octets changés");
                        c.noops += 1;
                    } else {
                        hist.push((mirror.clone(), before));
                        redo.clear();
                        if rewriting {
                            c.renames_rewriting += 1;
                        }
                        mirror = cand;
                    }
                } else {
                    c.refused += 1;
                    if let Some(Err(r)) = verdict {
                        count_rej(c, r);
                        if r == Rej::Cycle {
                            let m = result.unwrap_err().to_string();
                            assert!(m.contains(" → "), "chemin du cycle absent : {m}");
                        }
                    }
                    assert_eq!(bytes(doc.project()), before, "refus avec trace");
                    assert_eq!(*doc.project(), before_p);
                }
            }
            None => match rng.below(20) {
                0..=8 => {
                    if let Some((m, b)) = hist.pop() {
                        assert!(doc.can_undo());
                        assert!(doc.undo());
                        assert_eq!(bytes(doc.project()), b, "annuler inexact");
                        redo.push((mirror.clone(), before));
                        mirror = m;
                        c.undos += 1;
                    } else {
                        assert!(!doc.can_undo());
                    }
                }
                9..=13 => {
                    if let Some((m, b)) = redo.pop() {
                        assert!(doc.can_redo());
                        assert!(doc.redo());
                        assert_eq!(bytes(doc.project()), b, "rétablir inexact");
                        hist.push((mirror.clone(), before));
                        mirror = m;
                        c.redos += 1;
                    } else {
                        assert!(!doc.can_redo());
                    }
                }
                _ => {
                    let again = persist::from_json(&before).expect("relecture");
                    assert_eq!(again, *doc.project());
                    assert_eq!(bytes(&again), before);
                    assert_eq!(again.variable_values(), doc.project().variable_values());
                    c.reloads += 1;
                }
            },
        }
        check_state(doc.project(), &mirror, c);
    }
}

#[test]
fn campagne_adverse_variables() {
    let mut c = Counters::default();
    for seed in 1..=SEQUENCES {
        run_sequence(seed, &mut c);
    }
    eprintln!("{c:#?}");
    assert_eq!(c.steps, SEQUENCES as usize * STEPS);
    // Les contrôles forts ont réellement été exercés.
    assert!(c.accepted > 800, "{c:?}");
    assert!(c.noops >= 10, "{c:?}");
    assert!(c.cycles >= 30, "{c:?}");
    assert!(c.unknown >= 30, "{c:?}");
    assert!(c.eval >= 30, "{c:?}");
    assert!(c.syntax >= 30, "{c:?}");
    assert!(c.names >= 30, "{c:?}");
    assert!(c.duplicates >= 10, "{c:?}");
    assert!(c.renames_rewriting >= 40, "{c:?}");
    assert!(c.removes >= 20, "{c:?}");
    assert!(c.removes_refused_in_use >= 20, "{c:?}");
    assert!(c.undos >= 100, "{c:?}");
    assert!(c.redos >= 40, "{c:?}");
    assert!(c.reloads >= 100, "{c:?}");
    assert!(c.max_table >= 6, "{c:?}");
}

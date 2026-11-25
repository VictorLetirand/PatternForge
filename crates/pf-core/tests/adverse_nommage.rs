//! US-11.5 conventions de nommage (tranche 130) — campagne adverse : 180 séquences à graine fixe de
//! 14 à 30 pas : création / modification / suppression de règles (une valeur valide ou **une seule**
//! valeur invalide par pas), création de pièces, feuilles, variantes et codes avec le prochain nom
//! de la règle ou un nom ordinaire, noms « leurres » posés à la main (casse, zéros de tête, autre
//! préfixe), renommages en lot (aperçu comparé, puis commande), annulations et rétablissements
//! exacts à l'octet.
//!
//! **Oracle indépendant** : un miroir (`Model`) tient règles, noms de pièces / codes / feuilles /
//! variantes et le compteur d'identifiants ; il décide seul (règles du cahier écrites ici) si une
//! opération est acceptée, refusée ou sans effet, calcule lui-même le prochain nom (numéros relus
//! caractère par caractère, plus grand numéro + pas, saut des noms pris sans tenir compte de la
//! casse) et écrit le JSON attendu des règles. Comparé au projet après **chaque** pas. Des
//! **compteurs** prouvent que chaque contrôle fort s'exécute. Mutations vérifiées à la main : voir
//! `docs/JOURNAL.md` (tranche 130).

use pf_core::persist;
use pf_core::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;

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

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

const TARGETS: [NamingTarget; 4] = [
    NamingTarget::Piece,
    NamingTarget::PieceCode,
    NamingTarget::Sheet,
    NamingTarget::Variant,
];
const TARGET_KEYS: [&str; 4] = ["piece", "piece_code", "sheet", "variant"];

// ---------------------------------------------------------------------------
// Miroir
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
struct Spec {
    name: String,
    target: usize,
    prefix: String,
    sep: String,
    suffix: String,
    counter: Option<(u32, u32, u8)>,
}

#[derive(Clone, Debug, PartialEq)]
struct MRule {
    id: u64,
    spec: Spec,
}

#[derive(Clone, Debug, PartialEq)]
struct Model {
    rules: Vec<MRule>,
    /// (id, nom, code)
    pieces: Vec<(u64, String, Option<String>)>,
    sheets: Vec<(u64, String)>,
    variants: Vec<(u64, String)>,
    next_id: u64,
}

#[derive(Debug)]
struct Next {
    name: String,
    skipped: usize,
    used_max: bool,
}

#[derive(Clone, Debug)]
enum Op {
    AddRule(Spec),
    UpdRule(u64, Spec),
    RemRule(u64),
    /// (cible, avec la règle ?, nom ordinaire)
    Create(usize, bool, String),
    /// (cible, rang de l'objet, nom leurre)
    Decoy(usize, usize, String),
    Batch(u64),
}

#[derive(Debug, PartialEq)]
enum Verdict {
    Ok,
    Refused(&'static str),
}

fn low(s: &str) -> String {
    s.to_lowercase()
}

/// Nom produit pour un numéro (écrit ici, indépendamment du cœur).
fn fmt(s: &Spec, n: Option<u64>) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !s.prefix.is_empty() {
        parts.push(s.prefix.clone());
    }
    if let Some(n) = n {
        let w = s.counter.map(|c| c.2 as usize).unwrap_or(0);
        let mut digits = n.to_string();
        while digits.len() < w {
            digits.insert(0, '0');
        }
        parts.push(digits);
    }
    if !s.suffix.is_empty() {
        parts.push(s.suffix.clone());
    }
    parts.join(&s.sep)
}

/// Numéro d'un nom qui suit le motif (relu caractère par caractère).
fn digits_of(s: &Spec, name: &str) -> Option<u64> {
    let head: Vec<char> = if s.prefix.is_empty() {
        vec![]
    } else {
        format!("{}{}", s.prefix, s.sep).chars().collect()
    };
    let tail: Vec<char> = if s.suffix.is_empty() {
        vec![]
    } else {
        format!("{}{}", s.sep, s.suffix).chars().collect()
    };
    let cs: Vec<char> = name.chars().collect();
    if cs.len() < head.len() + tail.len() + 1 {
        return None;
    }
    if cs[..head.len()] != head[..] || cs[cs.len() - tail.len()..] != tail[..] {
        return None;
    }
    let mid = &cs[head.len()..cs.len() - tail.len()];
    if mid.len() > 9 || !mid.iter().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(
        mid.iter()
            .fold(0u64, |a, c| a * 10 + (*c as u64 - '0' as u64)),
    )
}

fn spec_error(s: &Spec) -> Option<&'static str> {
    let t = s.name.trim();
    if t.is_empty() {
        return Some("nom vide");
    }
    if t.chars().count() > 120 {
        return Some("nom trop long");
    }
    if t.chars().any(|c| c.is_control()) {
        return Some("nom caractère de contrôle");
    }
    for (p, label) in [(&s.prefix, "préfixe"), (&s.suffix, "suffixe")] {
        if p.chars().count() > 40 {
            return Some(if label == "préfixe" {
                "préfixe trop long"
            } else {
                "suffixe trop long"
            });
        }
        if p.chars().any(|c| c.is_control()) {
            return Some("caractère de contrôle");
        }
        if p != p.trim() {
            return Some("espaces de bord");
        }
    }
    if s.sep.chars().count() > 4 {
        return Some("séparateur trop long");
    }
    if s.sep.chars().any(|c| c.is_control()) {
        return Some("caractère de contrôle");
    }
    match s.counter {
        Some((start, step, width)) => {
            if start > 999_999 {
                return Some("début hors bornes");
            }
            if !(1..=1000).contains(&step) {
                return Some("pas hors bornes");
            }
            if width > 9 {
                return Some("largeur hors bornes");
            }
        }
        None => {
            if s.prefix.is_empty() {
                return Some("préfixe vide sans compteur");
            }
        }
    }
    None
}

impl Model {
    fn find(&self, id: u64) -> Option<usize> {
        self.rules.iter().position(|r| r.id == id)
    }

    fn rule_for(&self, target: usize) -> Option<&MRule> {
        self.rules.iter().find(|r| r.spec.target == target)
    }

    fn existing(&self, target: usize) -> Vec<(u64, String)> {
        match target {
            0 => self.pieces.iter().map(|p| (p.0, p.1.clone())).collect(),
            1 => self
                .pieces
                .iter()
                .filter_map(|p| p.2.clone().map(|c| (p.0, c)))
                .collect(),
            2 => self.sheets.clone(),
            _ => self.variants.clone(),
        }
    }

    /// Prochain nom de la règle (lecture seule).
    fn next(&self, r: &MRule) -> Result<Next, &'static str> {
        let names = self.existing(r.spec.target);
        let taken = |s: &str| names.iter().any(|(_, n)| low(n) == low(s));
        let Some((start, step, _)) = r.spec.counter else {
            let name = fmt(&r.spec, None);
            return if taken(&name) {
                Err("nom pris sans compteur")
            } else {
                Ok(Next {
                    name,
                    skipped: 0,
                    used_max: false,
                })
            };
        };
        let max = names
            .iter()
            .filter_map(|(_, n)| digits_of(&r.spec, n))
            .max();
        let mut n = match max {
            Some(m) => (m + step as u64).max(start as u64),
            None => start as u64,
        };
        let mut skipped = 0;
        loop {
            if n > 999_999_999 {
                return Err("compteur dépassé");
            }
            let name = fmt(&r.spec, Some(n));
            if !taken(&name) {
                return Ok(Next {
                    name,
                    skipped,
                    used_max: max.is_some(),
                });
            }
            skipped += 1;
            n += step as u64;
        }
    }

    /// Aperçu du lot : (id, ancien, nouveau).
    fn batch(&self, r: &MRule) -> Result<Vec<(u64, String, String)>, &'static str> {
        let Some((start, step, _)) = r.spec.counter else {
            return Err("lot sans compteur");
        };
        Ok(self
            .existing(r.spec.target)
            .into_iter()
            .enumerate()
            .map(|(i, (id, old))| {
                let n = start as u64 + i as u64 * step as u64;
                (id, old, fmt(&r.spec, Some(n)))
            })
            .collect())
    }

    fn check_spec(&self, s: &Spec, except: Option<u64>) -> Option<&'static str> {
        if let Some(e) = spec_error(s) {
            return Some(e);
        }
        for r in &self.rules {
            if Some(r.id) == except {
                continue;
            }
            if low(&r.spec.name) == low(s.name.trim()) {
                return Some("nom de règle pris");
            }
            if r.spec.target == s.target {
                return Some("cible prise");
            }
        }
        None
    }

    fn stored(s: &Spec) -> Spec {
        Spec {
            name: s.name.trim().to_string(),
            ..s.clone()
        }
    }

    fn apply(&mut self, op: &Op) -> Verdict {
        match op {
            Op::AddRule(s) => {
                let id = self.next_id;
                if let Some(e) = self.check_spec(s, None) {
                    return Verdict::Refused(e);
                }
                self.next_id += 1;
                self.rules.push(MRule {
                    id,
                    spec: Self::stored(s),
                });
                Verdict::Ok
            }
            Op::UpdRule(id, s) => {
                let Some(i) = self.find(*id) else {
                    return Verdict::Refused("règle inconnue");
                };
                if let Some(e) = self.check_spec(s, Some(*id)) {
                    return Verdict::Refused(e);
                }
                self.rules[i].spec = Self::stored(s);
                Verdict::Ok
            }
            Op::RemRule(id) => {
                let Some(i) = self.find(*id) else {
                    return Verdict::Refused("règle inconnue");
                };
                self.rules.remove(i);
                Verdict::Ok
            }
            Op::Create(t, _, name) => {
                let name = name.trim().to_string();
                match t {
                    0 => {
                        self.pieces.push((self.next_id, name, None));
                        self.next_id += 1;
                    }
                    1 => {
                        // Code : sur la première pièce sans code, sinon la dernière.
                        if self.pieces.is_empty() {
                            return Verdict::Ok;
                        }
                        let i = self
                            .pieces
                            .iter()
                            .position(|p| p.2.is_none())
                            .unwrap_or(self.pieces.len() - 1);
                        self.pieces[i].2 = Some(name);
                    }
                    2 => {
                        self.sheets.push((self.next_id, name));
                        self.next_id += 1;
                    }
                    _ => {
                        if self.variants.iter().any(|v| low(&v.1) == low(&name)) {
                            return Verdict::Refused("variante : nom pris");
                        }
                        if self.variants.len() >= 50 {
                            return Verdict::Refused("trop de variantes");
                        }
                        self.variants.push((self.next_id, name));
                        self.next_id += 1;
                    }
                }
                Verdict::Ok
            }
            Op::Decoy(t, i, name) => {
                let n = name.trim().to_string();
                if *t == 1 && n.is_empty() {
                    // Un code vide efface le code.
                    self.pieces[*i].2 = None;
                    return Verdict::Ok;
                }
                if n.is_empty() {
                    return Verdict::Refused("nom vide");
                }
                match t {
                    0 => self.pieces[*i].1 = n,
                    1 => self.pieces[*i].2 = Some(n),
                    2 => self.sheets[*i].1 = n,
                    _ => {
                        if self
                            .variants
                            .iter()
                            .enumerate()
                            .any(|(j, v)| j != *i && low(&v.1) == low(&n))
                        {
                            return Verdict::Refused("variante : nom pris");
                        }
                        self.variants[*i].1 = n;
                    }
                }
                Verdict::Ok
            }
            Op::Batch(id) => {
                let Some(i) = self.find(*id) else {
                    return Verdict::Refused("règle inconnue");
                };
                let r = self.rules[i].clone();
                let changes = match self.batch(&r) {
                    Ok(c) => c,
                    Err(e) => return Verdict::Refused(e),
                };
                for (id, _, new) in changes {
                    match r.spec.target {
                        0 => self.pieces.iter_mut().find(|p| p.0 == id).unwrap().1 = new,
                        1 => self.pieces.iter_mut().find(|p| p.0 == id).unwrap().2 = Some(new),
                        2 => self.sheets.iter_mut().find(|s| s.0 == id).unwrap().1 = new,
                        _ => self.variants.iter_mut().find(|v| v.0 == id).unwrap().1 = new,
                    }
                }
                Verdict::Ok
            }
        }
    }

    fn rules_json(&self) -> Value {
        let list: Vec<Value> = self
            .rules
            .iter()
            .map(|r| {
                let mut o = serde_json::Map::new();
                o.insert("id".into(), json!(r.id));
                o.insert("name".into(), json!(r.spec.name));
                o.insert("target".into(), json!(TARGET_KEYS[r.spec.target]));
                if !r.spec.prefix.is_empty() {
                    o.insert("prefix".into(), json!(r.spec.prefix));
                }
                if !r.spec.sep.is_empty() {
                    o.insert("separator".into(), json!(r.spec.sep));
                }
                if !r.spec.suffix.is_empty() {
                    o.insert("suffix".into(), json!(r.spec.suffix));
                }
                if let Some((start, step, width)) = r.spec.counter {
                    o.insert(
                        "counter".into(),
                        json!({"start": start, "step": step, "width": width}),
                    );
                }
                Value::Object(o)
            })
            .collect();
        Value::Array(list)
    }
}

// ---------------------------------------------------------------------------
// Générateur
// ---------------------------------------------------------------------------

const LEURRES: [&str; 22] = [
    "P-007", "p-003", "P-0012", "Q-004", "P-xx", "Pièce 5", "F-10", "F-20", "V-2", "3", "12 mm",
    "Dos", "P-1", "V-05", "p-001", "p-002", "f-10", "f-20", "v-01", "v-02", "P-004", "p-005",
];

fn gen_spec(rng: &mut Rng, model: &Model, flaw: bool) -> Spec {
    let target = rng.below(4) as usize;
    let mut s = Spec {
        name: rng
            .pick(&[
                "Pièces",
                "Feuilles",
                "Codes",
                "Variantes",
                "Règle A",
                "Règle B",
            ])
            .into(),
        target,
        prefix: if rng.chance(75) {
            rng.pick(&["P", "F", "V"]).to_string()
        } else {
            rng.pick(&["Pièce", "Dos", "", "X"]).to_string()
        },
        sep: rng.pick(&["-", "-", "_", " ", "", ".", "/"]).to_string(),
        suffix: rng.pick(&["", "", "", "mm", "B"]).to_string(),
        counter: if rng.chance(88) {
            Some((
                rng.pick(&[0, 1, 1, 5, 10]),
                rng.pick(&[1, 1, 2, 5]),
                rng.pick(&[0, 2, 3, 3]),
            ))
        } else {
            None
        },
    };
    if !flaw {
        return s;
    }
    // Une seule valeur invalide.
    match rng.below(14) {
        0 => s.name = rng.pick(&["", "   "]).to_string(),
        1 => s.name = "N".repeat(121),
        2 => s.name = "Règle\u{7}".into(),
        3 => s.prefix = format!("P{}", '\u{1}'),
        4 => s.prefix = "P".repeat(41),
        5 => s.prefix = " P".into(),
        6 => s.suffix = "mm ".into(),
        7 => s.sep = "-----".into(),
        8 => s.sep = "\t".into(),
        9 => {
            s.prefix = String::new();
            s.counter = None;
        }
        10 => s.counter = Some((1_000_000, 1, 2)),
        11 => s.counter = Some((1, rng.pick(&[0, 1001]), 2)),
        12 => s.counter = Some((1, 1, 10)),
        _ => {
            // Nom ou cible déjà pris par une autre règle (casse changée pour le nom).
            if let Some(r) = model.rules.first() {
                if rng.chance(50) {
                    s.name = r.spec.name.to_uppercase();
                } else {
                    s.target = r.spec.target;
                }
            }
        }
    }
    s
}

fn gen_op(rng: &mut Rng, model: &Model, ghost: u64) -> Op {
    let rule_id = |rng: &mut Rng| {
        if model.rules.is_empty() || rng.chance(6) {
            ghost
        } else {
            model.rules[rng.below(model.rules.len() as u64) as usize].id
        }
    };
    match rng.below(100) {
        0..=17 => {
            let flaw = rng.chance(35);
            Op::AddRule(gen_spec(rng, model, flaw))
        }
        18..=26 => {
            let id = rule_id(rng);
            // Moitié : mêmes valeurs (aucune étape), moitié : nouvelles valeurs.
            let s = match model.find(id) {
                Some(i) if rng.chance(30) => model.rules[i].spec.clone(),
                _ => {
                    let flaw = rng.chance(30);
                    let mut s = gen_spec(rng, model, flaw);
                    if let Some(i) = model.find(id) {
                        if rng.chance(60) {
                            s.target = model.rules[i].spec.target;
                        }
                    }
                    s
                }
            };
            Op::UpdRule(id, s)
        }
        27..=31 => Op::RemRule(rule_id(rng)),
        32..=66 => {
            let t = rng.below(4) as usize;
            let use_rule = model.rule_for(t).is_some() && rng.chance(55);
            let name = if rng.chance(50) {
                rng.pick(&LEURRES).to_string()
            } else {
                format!("Nom {}", rng.below(5))
            };
            // Piège : un nom en minuscules égal au prochain nom (non relu comme numéro, mais pris
            // sans tenir compte de la casse) : la règle devra le sauter.
            let trap = model
                .rule_for(t)
                .filter(|_| !use_rule && rng.chance(70))
                .and_then(|r| model.next(r).ok())
                .map(|n| n.name.to_lowercase())
                .filter(|n| model.rule_for(t).is_some_and(|r| *n != fmt(&r.spec, None)));
            Op::Create(t, use_rule, trap.unwrap_or(name))
        }
        67..=84 => {
            let t = rng.below(4) as usize;
            let count = model.existing(t).len();
            let name = if rng.chance(8) {
                String::new()
            } else {
                rng.pick(&LEURRES).to_string()
            };
            // Rang parmi les objets (codes : rang parmi les pièces, la commande pose le nom).
            let n = if t == 1 { model.pieces.len() } else { count };
            if n == 0 {
                let name = if name.is_empty() {
                    "Dos".to_string()
                } else {
                    name
                };
                return Op::Create(t, false, name);
            }
            Op::Decoy(t, rng.below(n as u64) as usize, name)
        }
        _ => Op::Batch(rule_id(rng)),
    }
}

// ---------------------------------------------------------------------------
// Exécution sur le vrai projet
// ---------------------------------------------------------------------------

#[derive(Default, Debug)]
struct Counters {
    ok: usize,
    noop: usize,
    undo: usize,
    redo: usize,
    refused: BTreeMap<&'static str, usize>,
    created_by_rule: [usize; 4],
    created_plain: usize,
    next_err: usize,
    skipped_taken: usize,
    from_max: usize,
    from_start: usize,
    suffix_names: usize,
    padded: usize,
    batch_applied: usize,
    batch_objects: usize,
    rules_added: usize,
    rules_removed: usize,
    rules_updated: usize,
    max_rules: usize,
    previews: usize,
    next_checks: usize,
}

fn run_op(
    doc: &mut Document,
    model: &Model,
    op: &Op,
    sheet: SheetId,
    c: &mut Counters,
) -> Option<Result<Outcome, CoreError>> {
    // Contrôles du calcul du prochain nom avant toute création avec la règle.
    let resolve = |t: usize, use_rule: bool, plain: &str, c: &mut Counters| -> Option<String> {
        if !use_rule {
            return Some(plain.to_string());
        }
        let r = model.rule_for(t).expect("règle voulue");
        let got = doc.project().next_name(NamingRuleId(r.id));
        match (model.next(r), got) {
            (Ok(n), Ok(g)) => {
                assert_eq!(g, n.name, "prochain nom de la règle {}", r.spec.name);
                c.created_by_rule[t] += 1;
                c.skipped_taken += n.skipped;
                if n.used_max {
                    c.from_max += 1;
                } else {
                    c.from_start += 1;
                }
                if !r.spec.suffix.is_empty() {
                    c.suffix_names += 1;
                }
                if n.name.contains('0') && r.spec.counter.is_some_and(|k| k.2 > 1) {
                    c.padded += 1;
                }
                Some(g)
            }
            (Err(_), Err(_)) => {
                c.next_err += 1;
                None
            }
            (a, b) => panic!("prochain nom : oracle {a:?} / cœur {b:?}"),
        }
    };
    let spec = |s: &Spec| NamingRuleSpec {
        name: s.name.clone(),
        target: TARGETS[s.target],
        prefix: s.prefix.clone(),
        separator: s.sep.clone(),
        suffix: s.suffix.clone(),
        counter: s
            .counter
            .map(|(start, step, width)| NamingCounter { start, step, width }),
    };
    let card = |p: &Piece, name: String, code: Option<String>| Command::SetPieceCard {
        piece: p.id,
        name,
        code,
        quantity: p.quantity,
        mirrored_pair: p.mirrored_pair,
        material: p.material.clone(),
        category: p.category,
        comment: p.comment.clone(),
    };
    let cmd = match op {
        Op::AddRule(s) => Command::AddNamingRule { spec: spec(s) },
        Op::UpdRule(id, s) => Command::UpdateNamingRule {
            rule: NamingRuleId(*id),
            spec: spec(s),
        },
        Op::RemRule(id) => Command::RemoveNamingRule {
            rule: NamingRuleId(*id),
        },
        Op::Create(t, use_rule, plain) => {
            if *t == 1 && doc.project().pieces.is_empty() {
                return None;
            }
            let name = resolve(*t, *use_rule, plain, c)?;
            if !*use_rule {
                c.created_plain += 1;
            }
            match t {
                0 => Command::AddPiece {
                    sheet,
                    name,
                    quantity: 1,
                    material: None,
                    mirrored_pair: false,
                },
                1 => {
                    let ps = &doc.project().pieces;
                    if ps.is_empty() {
                        return None;
                    }
                    let p = ps
                        .iter()
                        .find(|p| p.code.is_none())
                        .unwrap_or(ps.last().unwrap());
                    card(p, p.name.clone(), Some(name))
                }
                2 => Command::CreateSheet { name },
                _ => Command::AddVariant { name },
            }
        }
        Op::Decoy(t, i, name) => match t {
            0 => {
                let p = &doc.project().pieces[*i];
                card(p, name.clone(), p.code.clone())
            }
            1 => {
                let p = &doc.project().pieces[*i];
                card(p, p.name.clone(), Some(name.clone()))
            }
            2 => Command::RenameSheet {
                sheet: doc.project().sheets[*i].id,
                name: name.clone(),
            },
            _ => Command::RenameVariant {
                variant: doc.project().variants[*i].id,
                name: name.clone(),
            },
        },
        Op::Batch(id) => {
            // Aperçu du cœur = aperçu de l'oracle.
            if let Some(i) = model.find(*id) {
                let r = &model.rules[i];
                match (
                    model.batch(r),
                    doc.project().naming_preview(NamingRuleId(*id)),
                ) {
                    (Ok(want), Ok(got)) => {
                        let got: Vec<_> = got.into_iter().map(|g| (g.id, g.old, g.new)).collect();
                        assert_eq!(got, want, "aperçu du lot");
                        c.previews += 1;
                        c.batch_objects += want.len();
                    }
                    (Err(_), Err(_)) => {}
                    (a, b) => panic!("aperçu : oracle {a:?} / cœur {b:?}"),
                }
            }
            Command::ApplyNamingRule {
                rule: NamingRuleId(*id),
            }
        }
    };
    Some(doc.execute(cmd))
}

fn check_state(doc: &Document, m: &Model, c: &mut Counters) {
    let p = doc.project();
    p.validate().unwrap();
    assert_eq!(
        serde_json::to_value(&p.naming_rules).unwrap(),
        m.rules_json(),
        "règles ≠ oracle"
    );
    let pieces: Vec<_> = p
        .pieces
        .iter()
        .map(|x| (x.id.0, x.name.clone(), x.code.clone()))
        .collect();
    assert_eq!(pieces, m.pieces, "pièces ≠ oracle");
    let sheets: Vec<_> = p.sheets.iter().map(|s| (s.id.0, s.name.clone())).collect();
    assert_eq!(sheets, m.sheets, "feuilles ≠ oracle");
    let variants: Vec<_> = p
        .variants
        .iter()
        .map(|v| (v.id.0, v.name.clone()))
        .collect();
    assert_eq!(variants, m.variants, "variantes ≠ oracle");
    assert_eq!(p.next_id, m.next_id, "compteur d'identifiants");
    // Le prochain nom de chaque règle vient de l'état seul (aucun compteur stocké).
    for r in &m.rules {
        match (m.next(r), p.next_name(NamingRuleId(r.id))) {
            (Ok(n), Ok(g)) => assert_eq!(g, n.name),
            (Err(_), Err(_)) => {}
            (a, b) => panic!("prochain nom : oracle {a:?} / cœur {b:?}"),
        }
        c.next_checks += 1;
    }
    // Clé omise quand la liste est vide.
    let text = bytes(p);
    assert_eq!(text.contains("\"naming_rules\""), !m.rules.is_empty());
    assert_eq!(bytes(&persist::from_json(&text).unwrap()), text);
}

#[test]
fn campagne_adverse_nommage() {
    let base = Project::new("Campagne nommage");
    let sheet = base.sheets[0].id;
    let first_sheet = (sheet.0, base.sheets[0].name.clone());
    let ghost = base.next_id + 10_000;
    let mut c = Counters::default();
    let sequences = 180;
    for s in 0..sequences {
        let mut rng = Rng(0x5EED_1300_u64 + 977 * s as u64 + 1);
        let mut model = Model {
            rules: vec![],
            pieces: vec![],
            sheets: vec![first_sheet.clone()],
            variants: vec![],
            next_id: base.next_id,
        };
        let mut doc = Document::new(base.clone());
        let mut undo: Vec<(Model, String)> = Vec::new();
        let mut redo: Vec<(Model, String)> = Vec::new();
        let mut just_undone = false;
        let steps = 14 + rng.below(17);
        for _ in 0..steps {
            let history = rng.chance(12) || (just_undone && rng.chance(50));
            if history {
                let want_undo = if just_undone {
                    rng.chance(25)
                } else {
                    rng.chance(50)
                };
                just_undone = false;
                if want_undo {
                    assert_eq!(doc.can_undo(), !undo.is_empty());
                    if let Some((prev_model, prev_bytes)) = undo.pop() {
                        redo.push((model.clone(), bytes(doc.project())));
                        assert!(doc.undo());
                        model = prev_model;
                        assert_eq!(bytes(doc.project()), prev_bytes, "annulation inexacte");
                        c.undo += 1;
                        just_undone = true;
                    } else {
                        assert!(!doc.undo());
                    }
                } else if let Some((next_model, next_bytes)) = redo.pop() {
                    undo.push((model.clone(), bytes(doc.project())));
                    assert!(doc.redo());
                    model = next_model;
                    assert_eq!(bytes(doc.project()), next_bytes, "rétablissement inexact");
                    c.redo += 1;
                } else {
                    assert!(!doc.redo());
                }
                check_state(&doc, &model, &mut c);
                continue;
            }
            let op = gen_op(&mut rng, &model, ghost);
            let before_model = model.clone();
            let before_bytes = bytes(doc.project());
            // Un « Create » avec la règle dont le prochain nom est impossible n'a pas lieu.
            let skip_create = matches!(&op, Op::Create(t, true, _)
                if model.rule_for(*t).is_none_or(|r| model.next(r).is_err()));
            let skip_code = matches!(&op, Op::Create(1, _, _) if model.pieces.is_empty());
            let result = run_op(&mut doc, &before_model, &op, sheet, &mut c);
            if skip_create || skip_code {
                assert!(result.is_none());
                assert_eq!(bytes(doc.project()), before_bytes);
                check_state(&doc, &model, &mut c);
                continue;
            }
            let result = result.expect("opération exécutée");
            // Création avec la règle : l'oracle applique le nom qu'il a calculé lui-même.
            let model_op = match &op {
                Op::Create(t, true, _) => Op::Create(
                    *t,
                    true,
                    model.next(model.rule_for(*t).unwrap()).unwrap().name,
                ),
                other => other.clone(),
            };
            let verdict = model.apply(&model_op);
            match verdict {
                Verdict::Refused(why) => {
                    let e = result.expect_err(&format!("{op:?} devait être refusé ({why})"));
                    model = before_model;
                    assert_eq!(
                        bytes(doc.project()),
                        before_bytes,
                        "refus avec trace : {op:?} ({e})"
                    );
                    *c.refused.entry(why).or_default() += 1;
                }
                Verdict::Ok => {
                    result.unwrap_or_else(|e| panic!("{op:?} devait passer : {e}"));
                    if model == before_model {
                        assert_eq!(
                            bytes(doc.project()),
                            before_bytes,
                            "no-op avec trace : {op:?}"
                        );
                        assert_eq!(doc.can_undo(), !undo.is_empty(), "no-op : un pas de trop");
                        c.noop += 1;
                    } else {
                        undo.push((before_model, before_bytes));
                        redo.clear();
                        c.ok += 1;
                        match &op {
                            Op::AddRule(_) => c.rules_added += 1,
                            Op::RemRule(_) => c.rules_removed += 1,
                            Op::UpdRule(..) => c.rules_updated += 1,
                            Op::Batch(_) => c.batch_applied += 1,
                            _ => {}
                        }
                    }
                }
            }
            c.max_rules = c.max_rules.max(model.rules.len());
            check_state(&doc, &model, &mut c);
        }
    }
    // Compteurs : chaque contrôle fort a réellement été exécuté.
    assert!(c.ok > 1200, "{c:?}");
    assert!(c.noop > 10, "{c:?}");
    assert!(c.undo > 100 && c.redo > 20, "{c:?}");
    assert!(
        c.rules_added > 200 && c.rules_removed > 20 && c.rules_updated > 40,
        "{c:?}"
    );
    assert!(c.max_rules >= 4, "{c:?}");
    assert!(c.created_by_rule.iter().all(|n| *n > 8), "{c:?}");
    assert!(c.created_plain > 100, "{c:?}");
    assert!(c.next_err >= 2, "{c:?}");
    assert!(c.skipped_taken >= 5, "{c:?}");
    assert!(c.from_max > 40 && c.from_start > 30, "{c:?}");
    assert!(c.suffix_names > 20 && c.padded > 30, "{c:?}");
    assert!(
        c.batch_applied > 20 && c.batch_objects > 60 && c.previews > 40,
        "{c:?}"
    );
    assert!(c.next_checks > 2000, "{c:?}");
    for why in [
        "nom vide",
        "nom trop long",
        "nom caractère de contrôle",
        "caractère de contrôle",
        "préfixe trop long",
        "espaces de bord",
        "séparateur trop long",
        "préfixe vide sans compteur",
        "début hors bornes",
        "pas hors bornes",
        "largeur hors bornes",
        "nom de règle pris",
        "cible prise",
        "règle inconnue",
        "lot sans compteur",
        "variante : nom pris",
    ] {
        assert!(
            c.refused.get(why).copied().unwrap_or(0) >= 3,
            "refus « {why} » trop rare : {c:?}"
        );
    }
}

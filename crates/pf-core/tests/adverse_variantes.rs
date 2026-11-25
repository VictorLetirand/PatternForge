//! US-11.1 variantes (tranche 119) — campagne adverse : 180 séquences à graine fixe de 14 à 30
//! pas sur un projet de quatre pièces : ajout / renommage / suppression / duplication de
//! variantes, options (ajout, retrait, activation), entrées (ajout, modification, retrait),
//! suppression de pièces (refusée quand une variante les cite), annulations et rétablissements
//! exacts à l'octet. Chaque pas est une valeur valide ou **une seule** valeur invalide.
//!
//! **Oracle indépendant** : un miroir (`Model`) tient variantes, options, entrées, pièces vivantes
//! et le compteur d'identifiants ; il décide seul (règles du cahier écrites ici : bornes 1 à 999,
//! noms rognés et uniques sans la casse, options de la variante, entrée = (pièce, option)) si une
//! opération est acceptée, refusée ou sans effet, écrit lui-même le JSON attendu (clés omises
//! quand vides) et **calcule la nomenclature** (découpes et surface nette par matière) à partir
//! des aires connues des quatre rectangles (largeur × hauteur). Comparé au projet après **chaque**
//! pas. Des **compteurs** prouvent que chaque contrôle fort s'exécute. Mutations vérifiées à la
//! main : voir `docs/JOURNAL.md` (tranche 119).

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;
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

// ---------------------------------------------------------------------------
// Fixture : quatre rectangles, aire = largeur × hauteur (écrite ici)
// ---------------------------------------------------------------------------

/// (nom, hauteur, quantité de la fiche, paire, matière) ; largeur 100 partout.
const PIECES: [(&str, f64, u32, bool, Option<&str>); 4] = [
    ("Dos", 50.0, 1, false, Some("Coton")),
    ("Poche", 20.0, 2, false, Some("Coton")),
    ("Manche", 30.0, 1, true, Some("Lin")),
    ("Col", 10.0, 3, false, None),
];

fn fixture() -> (Project, Vec<PieceId>) {
    let mut p = Project::new("Campagne variantes");
    let s = p.sheets[0].id;
    let mut ids = Vec::new();
    for (i, (name, h, qty, pair, mat)) in PIECES.iter().enumerate() {
        let r = p
            .apply(Command::AddRectangle {
                sheet: s,
                corner: PointRef::New(PointMm::new(200.0 * i as f64, 0.0)),
                width_mm: 100.0,
                height_mm: *h,
            })
            .unwrap();
        let id = p
            .apply(Command::ExtractPiece {
                sheet: s,
                source: PieceSource::Paths {
                    entities: r.entities,
                },
                name: (*name).into(),
                detached: false,
            })
            .unwrap()
            .pieces[0];
        p.apply(Command::SetPieceCard {
            piece: id,
            name: (*name).into(),
            code: None,
            quantity: *qty,
            mirrored_pair: *pair,
            material: mat.map(str::to_string),
            category: None,
            comment: None,
        })
        .unwrap();
        ids.push(id);
    }
    (p, ids)
}

// ---------------------------------------------------------------------------
// Miroir
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
struct ME {
    /// Identifiant d'entrée (schéma 6) : compteur du projet, jamais réutilisé.
    id: u64,
    piece: usize,
    qty: u32,
    mat: Option<String>,
    opt: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
struct MV {
    id: u64,
    name: String,
    options: Vec<String>,
    active: Vec<String>,
    entries: Vec<ME>,
}

#[derive(Clone, Debug, PartialEq)]
struct Model {
    variants: Vec<MV>,
    live: [bool; 4],
    next_id: u64,
}

#[derive(Clone, Debug)]
enum Op {
    AddV(String),
    Rename(u64, String),
    RemoveV(u64),
    Dup(u64, String),
    AddOpt(u64, String),
    RemOpt(u64, String),
    Active(u64, String, bool),
    AddE(u64, usize, u32, Option<String>, Option<String>),
    UpdE(
        u64,
        usize,
        Option<String>,
        u32,
        Option<String>,
        Option<String>,
    ),
    RemE(u64, usize, Option<String>),
    DelPiece(usize),
}

#[derive(Debug, PartialEq)]
enum Verdict {
    Ok,
    Refused(&'static str),
}

fn trimmed_name(raw: &str) -> Result<String, &'static str> {
    let t = raw.trim().to_string();
    if t.is_empty() {
        return Err("nom vide");
    }
    if t.chars().count() > 120 {
        return Err("nom trop long");
    }
    if t.chars().any(|c| c.is_control()) {
        return Err("nom caractère de contrôle");
    }
    Ok(t)
}

fn low(s: &str) -> String {
    s.to_lowercase()
}

impl Model {
    fn find(&self, id: u64) -> Option<usize> {
        self.variants.iter().position(|v| v.id == id)
    }

    /// Option stockée correspondant à la saisie (casse ignorée, espaces de bord retirés).
    fn resolve(&self, vi: usize, raw: &str) -> Option<String> {
        self.variants[vi]
            .options
            .iter()
            .find(|o| low(o) == low(raw.trim()))
            .cloned()
    }

    fn apply(&mut self, op: &Op) -> Verdict {
        match op {
            Op::AddV(name) => {
                let n = match trimmed_name(name) {
                    Ok(n) => n,
                    Err(e) => return Verdict::Refused(e),
                };
                if self.variants.iter().any(|v| low(&v.name) == low(&n)) {
                    return Verdict::Refused("nom pris");
                }
                if self.variants.len() >= 50 {
                    return Verdict::Refused("trop de variantes");
                }
                self.variants.push(MV {
                    id: self.next_id,
                    name: n,
                    options: vec![],
                    active: vec![],
                    entries: vec![],
                });
                self.next_id += 1;
                Verdict::Ok
            }
            Op::Rename(id, name) => {
                let Some(vi) = self.find(*id) else {
                    return Verdict::Refused("variante inconnue");
                };
                let n = match trimmed_name(name) {
                    Ok(n) => n,
                    Err(e) => return Verdict::Refused(e),
                };
                if self
                    .variants
                    .iter()
                    .enumerate()
                    .any(|(i, v)| i != vi && low(&v.name) == low(&n))
                {
                    return Verdict::Refused("nom pris");
                }
                self.variants[vi].name = n;
                Verdict::Ok
            }
            Op::RemoveV(id) => {
                let Some(vi) = self.find(*id) else {
                    return Verdict::Refused("variante inconnue");
                };
                self.variants.remove(vi);
                Verdict::Ok
            }
            Op::Dup(id, name) => {
                let Some(vi) = self.find(*id) else {
                    return Verdict::Refused("variante inconnue");
                };
                let n = match trimmed_name(name) {
                    Ok(n) => n,
                    Err(e) => return Verdict::Refused(e),
                };
                if self.variants.iter().any(|v| low(&v.name) == low(&n)) {
                    return Verdict::Refused("nom pris");
                }
                if self.variants.len() >= 50 {
                    return Verdict::Refused("trop de variantes");
                }
                let mut c = self.variants[vi].clone();
                c.id = self.next_id;
                c.name = n;
                self.next_id += 1;
                // Les entrées copiées reçoivent des identifiants neufs, dans l'ordre.
                for e in &mut c.entries {
                    e.id = self.next_id;
                    self.next_id += 1;
                }
                self.variants.push(c);
                Verdict::Ok
            }
            Op::AddOpt(id, name) => {
                let Some(vi) = self.find(*id) else {
                    return Verdict::Refused("variante inconnue");
                };
                let n = match trimmed_name(name) {
                    Ok(n) => n,
                    Err(e) => return Verdict::Refused(e),
                };
                if self.variants[vi].options.iter().any(|o| low(o) == low(&n)) {
                    return Verdict::Refused("option prise");
                }
                if self.variants[vi].options.len() >= 20 {
                    return Verdict::Refused("trop d'options");
                }
                self.variants[vi].options.push(n);
                Verdict::Ok
            }
            Op::RemOpt(id, name) => {
                let Some(vi) = self.find(*id) else {
                    return Verdict::Refused("variante inconnue");
                };
                let Some(o) = self.resolve(vi, name) else {
                    return Verdict::Refused("option inconnue");
                };
                if self.variants[vi]
                    .entries
                    .iter()
                    .any(|e| e.opt.as_ref() == Some(&o))
                {
                    return Verdict::Refused("option utilisée");
                }
                let v = &mut self.variants[vi];
                v.options.retain(|x| *x != o);
                v.active.retain(|x| *x != o);
                Verdict::Ok
            }
            Op::Active(id, name, on) => {
                let Some(vi) = self.find(*id) else {
                    return Verdict::Refused("variante inconnue");
                };
                let Some(o) = self.resolve(vi, name) else {
                    return Verdict::Refused("option inconnue");
                };
                let v = &mut self.variants[vi];
                let is = v.active.contains(&o);
                if *on && !is {
                    v.active.push(o);
                } else if !*on && is {
                    v.active.retain(|x| *x != o);
                }
                Verdict::Ok
            }
            Op::AddE(id, piece, qty, mat, opt) => {
                let Some(vi) = self.find(*id) else {
                    return Verdict::Refused("variante inconnue");
                };
                if !self.live.get(*piece).copied().unwrap_or(false) {
                    return Verdict::Refused("pièce inconnue");
                }
                if !(1..=999).contains(qty) {
                    return Verdict::Refused("quantité");
                }
                let mat = match mat.as_deref().map(trimmed_name).transpose() {
                    Ok(m) => m,
                    Err(_) => return Verdict::Refused("matière"),
                };
                let opt = match opt.as_deref() {
                    None => None,
                    Some(o) => match self.resolve(vi, o) {
                        Some(o) => Some(o),
                        None => return Verdict::Refused("option inconnue"),
                    },
                };
                let v = &mut self.variants[vi];
                if v.entries.iter().any(|e| e.piece == *piece && e.opt == opt) {
                    return Verdict::Refused("entrée dupliquée");
                }
                if v.entries.len() >= 500 {
                    return Verdict::Refused("trop d'entrées");
                }
                v.entries.push(ME {
                    id: self.next_id,
                    piece: *piece,
                    qty: *qty,
                    mat,
                    opt,
                });
                self.next_id += 1;
                Verdict::Ok
            }
            Op::UpdE(id, piece, opt, qty, mat, new_opt) => {
                let Some(vi) = self.find(*id) else {
                    return Verdict::Refused("variante inconnue");
                };
                let old = match opt.as_deref() {
                    None => None,
                    Some(o) => match self.resolve(vi, o) {
                        Some(o) => Some(o),
                        None => return Verdict::Refused("option inconnue"),
                    },
                };
                let Some(at) = self.variants[vi]
                    .entries
                    .iter()
                    .position(|e| e.piece == *piece && e.opt == old)
                else {
                    return Verdict::Refused("entrée introuvable");
                };
                if !(1..=999).contains(qty) {
                    return Verdict::Refused("quantité");
                }
                let mat = match mat.as_deref().map(trimmed_name).transpose() {
                    Ok(m) => m,
                    Err(_) => return Verdict::Refused("matière"),
                };
                let new_opt = match new_opt.as_deref() {
                    None => None,
                    Some(o) => match self.resolve(vi, o) {
                        Some(o) => Some(o),
                        None => return Verdict::Refused("option inconnue"),
                    },
                };
                let v = &mut self.variants[vi];
                if v.entries
                    .iter()
                    .enumerate()
                    .any(|(i, e)| i != at && e.piece == *piece && e.opt == new_opt)
                {
                    return Verdict::Refused("entrée dupliquée");
                }
                v.entries[at] = ME {
                    id: v.entries[at].id,
                    piece: *piece,
                    qty: *qty,
                    mat,
                    opt: new_opt,
                };
                Verdict::Ok
            }
            Op::RemE(id, piece, opt) => {
                let Some(vi) = self.find(*id) else {
                    return Verdict::Refused("variante inconnue");
                };
                let old = match opt.as_deref() {
                    None => None,
                    Some(o) => match self.resolve(vi, o) {
                        Some(o) => Some(o),
                        None => return Verdict::Refused("option inconnue"),
                    },
                };
                let Some(at) = self.variants[vi]
                    .entries
                    .iter()
                    .position(|e| e.piece == *piece && e.opt == old)
                else {
                    return Verdict::Refused("entrée introuvable");
                };
                self.variants[vi].entries.remove(at);
                Verdict::Ok
            }
            Op::DelPiece(piece) => {
                if !self.live[*piece] {
                    return Verdict::Refused("pièce inconnue");
                }
                if self
                    .variants
                    .iter()
                    .any(|v| v.entries.iter().any(|e| e.piece == *piece))
                {
                    return Verdict::Refused("pièce citée");
                }
                self.live[*piece] = false;
                Verdict::Ok
            }
        }
    }

    /// JSON attendu de `variants` : clés omises quand vides, ordre `id, name, options,
    /// options_active, entries` ; entrée `id, piece, quantity, material?, option?`.
    fn json(&self, ids: &[PieceId]) -> Value {
        let list: Vec<Value> = self
            .variants
            .iter()
            .map(|v| {
                let mut o = serde_json::Map::new();
                o.insert("id".into(), json!(v.id));
                o.insert("name".into(), json!(v.name));
                if !v.options.is_empty() {
                    o.insert("options".into(), json!(v.options));
                }
                if !v.active.is_empty() {
                    o.insert("options_active".into(), json!(v.active));
                }
                if !v.entries.is_empty() {
                    let es: Vec<Value> = v
                        .entries
                        .iter()
                        .map(|e| {
                            let mut m = serde_json::Map::new();
                            m.insert("id".into(), json!(e.id));
                            m.insert("piece".into(), json!(ids[e.piece].0));
                            m.insert("quantity".into(), json!(e.qty));
                            if let Some(x) = &e.mat {
                                m.insert("material".into(), json!(x));
                            }
                            if let Some(x) = &e.opt {
                                m.insert("option".into(), json!(x));
                            }
                            Value::Object(m)
                        })
                        .collect();
                    o.insert("entries".into(), Value::Array(es));
                }
                Value::Object(o)
            })
            .collect();
        Value::Array(list)
    }

    /// Nomenclature attendue pour une variante (`None` = tout le projet) : matière en minuscules
    /// → (lignes, découpes, surface nette).
    fn bom(&self, variant: Option<usize>) -> BTreeMap<Option<String>, (usize, u32, f64)> {
        // (pièce, quantité, matière de l'entrée ou de la pièce)
        let mut rows: Vec<(usize, u32, Option<String>)> = Vec::new();
        match variant {
            None => {
                for (i, (_, _, qty, _, mat)) in PIECES.iter().enumerate() {
                    if self.live[i] {
                        rows.push((i, *qty, mat.map(str::to_string)));
                    }
                }
            }
            Some(vi) => {
                let v = &self.variants[vi];
                for e in &v.entries {
                    let counts = match &e.opt {
                        None => true,
                        Some(o) => v.active.contains(o),
                    };
                    if counts {
                        rows.push((
                            e.piece,
                            e.qty,
                            e.mat.clone().or(PIECES[e.piece].4.map(str::to_string)),
                        ));
                    }
                }
            }
        }
        let mut out: BTreeMap<Option<String>, (usize, u32, f64)> = BTreeMap::new();
        for (piece, qty, mat) in rows {
            let (_, h, _, pair, _) = PIECES[piece];
            let cuts = qty * if pair { 2 } else { 1 };
            let area = 100.0 * h;
            let key = mat.map(|m| low(m.trim()));
            let slot = out.entry(key).or_insert((0, 0, 0.0));
            slot.0 += 1;
            slot.1 += cuts;
            slot.2 += area * f64::from(cuts);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Commande du cœur pour une opération du miroir
// ---------------------------------------------------------------------------

fn command(op: &Op, ids: &[PieceId], ghost: PieceId) -> Command {
    let v = |id: &u64| VariantId(*id);
    // Une pièce « fantôme » (jamais créée) sert aux références inconnues.
    let pid = |i: &usize| if *i == usize::MAX { ghost } else { ids[*i] };
    match op {
        Op::AddV(n) => Command::AddVariant { name: n.clone() },
        Op::Rename(i, n) => Command::RenameVariant {
            variant: v(i),
            name: n.clone(),
        },
        Op::RemoveV(i) => Command::RemoveVariant { variant: v(i) },
        Op::Dup(i, n) => Command::DuplicateVariant {
            variant: v(i),
            name: n.clone(),
        },
        Op::AddOpt(i, n) => Command::AddVariantOption {
            variant: v(i),
            option: n.clone(),
        },
        Op::RemOpt(i, n) => Command::RemoveVariantOption {
            variant: v(i),
            option: n.clone(),
        },
        Op::Active(i, n, a) => Command::SetVariantOptionActive {
            variant: v(i),
            option: n.clone(),
            active: *a,
        },
        Op::AddE(i, p, q, m, o) => Command::AddVariantEntry {
            variant: v(i),
            piece: pid(p),
            quantity: *q,
            material: m.clone(),
            option: o.clone(),
        },
        Op::UpdE(i, p, o, q, m, n) => Command::UpdateVariantEntry {
            variant: v(i),
            piece: pid(p),
            option: o.clone(),
            quantity: *q,
            material: m.clone(),
            new_option: n.clone(),
        },
        Op::RemE(i, p, o) => Command::RemoveVariantEntry {
            variant: v(i),
            piece: pid(p),
            option: o.clone(),
        },
        Op::DelPiece(p) => Command::DeletePiece { piece: ids[*p] },
    }
}

// ---------------------------------------------------------------------------
// Générateur : valeur valide ou une seule valeur invalide
// ---------------------------------------------------------------------------

const NAMES: [&str; 8] = [
    "Chemise", "chemise", "Robe", "ROBE", "Été", "été", "Manteau", " Gilet ",
];
const OPTS: [&str; 6] = [
    "avec poche",
    "AVEC POCHE",
    "col",
    "Col",
    "doublé",
    " fente ",
];
const MATS: [&str; 5] = ["Coton", "coton", " Lin ", "Cuir", "Soie"];

#[derive(Default, Debug)]
struct Counters {
    ok: usize,
    noop: usize,
    refused: BTreeMap<&'static str, usize>,
    undo: usize,
    redo: usize,
    bom_variant_checks: usize,
    bom_excluded_entries: usize,
    bom_override: usize,
    max_variants: usize,
    max_entries: usize,
    dup: usize,
    deleted_pieces: usize,
}

fn pick_variant(rng: &mut Rng, m: &Model) -> u64 {
    if m.variants.is_empty() || rng.chance(4) {
        return 999_999; // variante inconnue
    }
    rng.pick(&m.variants).id
}

fn pick_piece(rng: &mut Rng, m: &Model) -> usize {
    let live: Vec<usize> = (0..4).filter(|i| m.live[*i]).collect();
    if live.is_empty() || rng.chance(3) {
        return usize::MAX; // pièce inconnue
    }
    rng.pick(&live)
}

fn gen_op(rng: &mut Rng, m: &Model) -> Op {
    // Sans variante, presque tout serait « variante inconnue » : on en crée d'abord une.
    if m.variants.is_empty() && rng.chance(85) {
        return Op::AddV(rng.pick(&NAMES).to_string());
    }
    match rng.below(100) {
        0..=11 => {
            let n = if rng.chance(15) {
                rng.pick(&["", "   ", "a\tb"]).to_string()
            } else {
                rng.pick(&NAMES).to_string()
            };
            Op::AddV(n)
        }
        12..=17 => {
            let n = if rng.chance(15) {
                rng.pick(&["", "  "]).to_string()
            } else {
                rng.pick(&NAMES).to_string()
            };
            Op::Rename(pick_variant(rng, m), n)
        }
        18..=22 => Op::RemoveV(pick_variant(rng, m)),
        23..=27 => Op::Dup(pick_variant(rng, m), rng.pick(&NAMES).to_string()),
        28..=36 => {
            let n = if rng.chance(8) {
                "".to_string()
            } else {
                rng.pick(&OPTS).to_string()
            };
            Op::AddOpt(pick_variant(rng, m), n)
        }
        37..=41 => {
            let v = pick_variant(rng, m);
            let name = match m.find(v) {
                Some(vi) if !m.variants[vi].options.is_empty() && rng.chance(85) => {
                    rng.pick(&m.variants[vi].options)
                }
                _ => "fantôme".to_string(),
            };
            Op::RemOpt(v, name)
        }
        42..=49 => {
            let v = pick_variant(rng, m);
            let name = match m.find(v) {
                Some(vi) if !m.variants[vi].options.is_empty() && rng.chance(90) => {
                    let o = rng.pick(&m.variants[vi].options);
                    if rng.chance(30) {
                        o.to_uppercase()
                    } else {
                        o
                    }
                }
                _ => "fantôme".to_string(),
            };
            Op::Active(v, name, rng.chance(60))
        }
        50..=71 => {
            let v = pick_variant(rng, m);
            let q = match rng.below(12) {
                0 => 0,
                1 => 1000,
                2 => 999,
                3 => 1,
                _ => 1 + rng.below(8) as u32,
            };
            let mat = match rng.below(10) {
                0 => Some("  ".to_string()),
                1..=3 => Some(rng.pick(&MATS).to_string()),
                _ => None,
            };
            let opt = entry_opt(rng, m, v);
            Op::AddE(v, pick_piece(rng, m), q, mat, opt)
        }
        72..=84 => {
            let v = pick_variant(rng, m);
            // Entrée existante la plupart du temps.
            let existing = m.find(v).and_then(|vi| {
                let es = &m.variants[vi].entries;
                (!es.is_empty()).then(|| rng.pick(es))
            });
            match existing {
                Some(e) if rng.chance(90) => {
                    let q = if rng.chance(10) {
                        rng.pick(&[0u32, 1000])
                    } else if rng.chance(30) {
                        e.qty
                    } else {
                        1 + rng.below(9) as u32
                    };
                    let mat = if rng.chance(40) {
                        e.mat.clone()
                    } else if rng.chance(8) {
                        Some(" ".to_string())
                    } else if rng.chance(50) {
                        Some(rng.pick(&MATS).to_string())
                    } else {
                        None
                    };
                    let new_opt = if rng.chance(40) {
                        e.opt.clone()
                    } else {
                        entry_opt(rng, m, v)
                    };
                    Op::UpdE(v, e.piece, e.opt.clone(), q, mat, new_opt)
                }
                _ => Op::UpdE(v, pick_piece(rng, m), entry_opt(rng, m, v), 1, None, None),
            }
        }
        85..=96 => {
            let v = pick_variant(rng, m);
            let existing = m.find(v).and_then(|vi| {
                let es = &m.variants[vi].entries;
                (!es.is_empty()).then(|| rng.pick(es))
            });
            match existing {
                Some(e) if rng.chance(90) => Op::RemE(v, e.piece, e.opt.clone()),
                _ => Op::RemE(v, pick_piece(rng, m), entry_opt(rng, m, v)),
            }
        }
        _ => Op::DelPiece(rng.below(4) as usize),
    }
}

fn entry_opt(rng: &mut Rng, m: &Model, v: u64) -> Option<String> {
    let opts = m
        .find(v)
        .map(|vi| m.variants[vi].options.clone())
        .unwrap_or_default();
    match rng.below(10) {
        0..=3 => None,
        4 if rng.chance(50) => Some("fantôme".to_string()),
        _ if !opts.is_empty() => {
            let o = rng.pick(&opts);
            Some(if rng.chance(30) { o.to_uppercase() } else { o })
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Comparaisons
// ---------------------------------------------------------------------------

fn check_bom(p: &Project, m: &Model, rng: &mut Rng, c: &mut Counters) {
    // Tout le projet : les pièces vivantes, avec leur propre quantité.
    let all = p.bill_of_materials(None).unwrap();
    compare_bom(&all, &m.bom(None), "projet entier");
    for (vi, v) in m.variants.iter().enumerate() {
        if rng.chance(50) {
            continue;
        }
        let bom = p
            .bill_of_materials_variant(None, Some(VariantId(v.id)))
            .unwrap();
        compare_bom(&bom, &m.bom(Some(vi)), &format!("variante {}", v.name));
        c.bom_variant_checks += 1;
        let counted = v
            .entries
            .iter()
            .filter(|e| e.opt.as_ref().is_none_or(|o| v.active.contains(o)))
            .count();
        assert_eq!(bom.lines.len(), counted);
        c.bom_excluded_entries += v.entries.len() - counted;
        c.bom_override += v.entries.iter().filter(|e| e.mat.is_some()).count();
    }
}

fn compare_bom(bom: &Bom, expected: &BTreeMap<Option<String>, (usize, u32, f64)>, what: &str) {
    let mut got: BTreeMap<Option<String>, (usize, u32, f64)> = BTreeMap::new();
    for m in &bom.materials {
        let key = m.material.as_ref().map(|x| x.to_lowercase());
        let area = m.net_area_mm2[0].expect("aire lisible");
        assert!(
            got.insert(key, (m.pieces, m.cuts, area)).is_none(),
            "{what}"
        );
    }
    assert_eq!(
        got.len(),
        expected.len(),
        "{what} : matières {got:?} / {expected:?}"
    );
    for (k, (n, cuts, area)) in expected {
        let g = got
            .get(k)
            .unwrap_or_else(|| panic!("{what} : matière {k:?} absente"));
        assert_eq!((g.0, g.1), (*n, *cuts), "{what} : {k:?}");
        assert!(
            (g.2 - area).abs() < 1e-6,
            "{what} : {k:?} aire {} / {area}",
            g.2
        );
    }
}

fn check_state(doc: &Document, m: &Model, ids: &[PieceId]) {
    let p = doc.project();
    p.validate().unwrap();
    let got = serde_json::to_value(&p.variants).unwrap();
    assert_eq!(got, m.json(ids), "variantes ≠ oracle");
    assert_eq!(p.next_id, m.next_id, "compteur d'identifiants");
    for (i, id) in ids.iter().enumerate() {
        assert_eq!(p.piece(*id).is_ok(), m.live[i], "pièce {i} vivante");
    }
    // Clé omise quand la liste est vide.
    assert_eq!(bytes(p).contains("\"variants\""), !m.variants.is_empty());
    // Relecture idempotente.
    let text = bytes(p);
    assert_eq!(bytes(&persist::from_json(&text).unwrap()), text);
}

// ---------------------------------------------------------------------------
// Campagne
// ---------------------------------------------------------------------------

#[test]
fn campagne_adverse_variantes() {
    let (base, ids) = fixture();
    let ghost = PieceId(base.next_id + 10_000);
    let mut c = Counters::default();
    let sequences = 180;
    for s in 0..sequences {
        let mut rng = Rng(0x5EED_1100_u64 + 977 * s as u64 + 1);
        let mut model = Model {
            variants: vec![],
            live: [true; 4],
            next_id: base.next_id,
        };
        let mut doc = Document::new(base.clone());
        // Piles de l'oracle : miroir + octets.
        let mut undo: Vec<(Model, String)> = Vec::new();
        let mut redo: Vec<(Model, String)> = Vec::new();
        let mut just_undone = false;
        let steps = 14 + rng.below(17);
        for _ in 0..steps {
            // Annuler / rétablir : 8 % des pas.
            let history = rng.chance(12) || (just_undone && rng.chance(50));
            if history {
                let want_undo = if just_undone {
                    rng.chance(25)
                } else {
                    rng.chance(50)
                };
                just_undone = false;
                if want_undo {
                    let had = !undo.is_empty();
                    assert_eq!(doc.can_undo(), had);
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
                check_state(&doc, &model, &ids);
                continue;
            }
            let op = gen_op(&mut rng, &model);
            let before_model = model.clone();
            let before_bytes = bytes(doc.project());
            let verdict = model.apply(&op);
            let result = doc.execute(command(&op, &ids, ghost));
            match verdict {
                Verdict::Refused(why) => {
                    let e = result.expect_err(&format!("{op:?} devait être refusé ({why})"));
                    model = before_model;
                    assert_eq!(
                        bytes(doc.project()),
                        before_bytes,
                        "refus avec trace : {op:?}"
                    );
                    *c.refused.entry(why).or_default() += 1;
                    let msg = e.to_string();
                    match (why, &op) {
                        ("pièce citée", Op::DelPiece(_)) => {
                            let named = model
                                .variants
                                .iter()
                                .any(|v| msg.contains(&format!("« {} »", v.name)));
                            assert!(named, "le refus ne nomme pas la variante : {msg}");
                        }
                        ("option utilisée", Op::RemOpt(..)) => {
                            let named =
                                PIECES.iter().any(|p| msg.contains(&format!("« {} »", p.0)));
                            assert!(named, "le refus ne nomme pas l'entrée : {msg}");
                        }
                        _ => {}
                    }
                }
                Verdict::Ok => {
                    result.unwrap_or_else(|e| panic!("{op:?} devait passer : {e}"));
                    if model == before_model {
                        // Même valeur : aucun pas, aucun octet changé.
                        assert_eq!(
                            bytes(doc.project()),
                            before_bytes,
                            "no-op avec trace : {op:?}"
                        );
                        c.noop += 1;
                    } else {
                        undo.push((before_model, before_bytes));
                        redo.clear();
                        c.ok += 1;
                        if matches!(op, Op::Dup(..)) {
                            c.dup += 1;
                        }
                        if matches!(op, Op::DelPiece(_)) {
                            c.deleted_pieces += 1;
                        }
                    }
                }
            }
            c.max_variants = c.max_variants.max(model.variants.len());
            c.max_entries = c.max_entries.max(
                model
                    .variants
                    .iter()
                    .map(|v| v.entries.len())
                    .max()
                    .unwrap_or(0),
            );
            check_state(&doc, &model, &ids);
            check_bom(doc.project(), &model, &mut rng, &mut c);
        }
    }
    // Compteurs : chaque contrôle fort a réellement été exécuté.
    assert!(c.ok > 1500, "{c:?}");
    assert!(c.noop > 20, "{c:?}");
    assert!(c.undo > 100 && c.redo > 20, "{c:?}");
    assert!(c.dup > 40, "{c:?}");
    assert!(c.deleted_pieces > 10, "{c:?}");
    assert!(c.bom_variant_checks > 1500, "{c:?}");
    assert!(c.bom_excluded_entries > 100, "{c:?}");
    assert!(c.bom_override > 100, "{c:?}");
    assert!(c.max_variants >= 4 && c.max_entries >= 4, "{c:?}");
    for why in [
        "nom vide",
        "nom caractère de contrôle",
        "nom pris",
        "variante inconnue",
        "option prise",
        "option inconnue",
        "option utilisée",
        "quantité",
        "matière",
        "pièce inconnue",
        "pièce citée",
        "entrée dupliquée",
        "entrée introuvable",
    ] {
        assert!(
            c.refused.get(why).copied().unwrap_or(0) >= 5,
            "refus « {why} » trop rare : {c:?}"
        );
    }
}

//! Décisions 455 à 459 — campagnes adverses du schéma 6 (graine fixe).
//!
//! 1. `campagne_adverse_commandes_v6` (160 séquences de 36 pas) : fiches client citées par des
//!    tailles, renommage / déplacement d'options, entrées de variante à identifiant stable,
//!    annulations et rétablissements. **Oracle indépendant** : un modèle écrit ici (fiches, tailles
//!    et leur fiche d'origine, variantes, options, entrées et le compteur d'identifiants) décide seul
//!    accepté / refusé et prédit l'état complet après **chaque** pas ; une annulation redonne les
//!    octets d'avant, un rétablissement ceux d'après. Chaque séquence relabellise aussi son
//!    résultat en v5 (clés v6 retirées, compteur recalculé par le test) et vérifie la migration :
//!    identifiants d'entrée attribués dans l'ordre du fichier.
//! 2. `campagne_adverse_fichiers_v6_abimes` (160 séquences) : un fichier v6 valide aléatoire (fond,
//!    peau, défauts, zones) est relu à l'octet, puis chaque corruption (chemin absolu, `..`,
//!    empreinte fausse, polygone auto-croisé, défaut hors contour, clé v6 dans un fichier v5…) est
//!    refusée avec la raison attendue.
//!
//! Des **compteurs** assertés prouvent que chaque contrôle fort s'est exécuté.

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;
use serde_json::{json, Value};
use std::collections::BTreeMap;

const SEQUENCES: usize = 160;
const STEPS: usize = 36;

struct Rng(u64);

impl Rng {
    fn step(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.step() >> 11) as usize % n
    }
    fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }
    /// Multiple de 0,5 dans `[lo ; hi]` (exact en binaire, relu à l'identique).
    fn half(&mut self, lo: i64, hi: i64) -> f64 {
        lo as f64 + self.below(((hi - lo) * 2 + 1) as usize) as f64 / 2.0
    }
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).expect("projet valide")
}

// ---------------------------------------------------------------------------
// Modèle (oracle)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
struct ME {
    id: u64,
    piece: u64,
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

/// Taille de l'oracle : (id, libellé, fiche citée).
type MS = (u64, String, Option<u64>);

#[derive(Clone, Debug, PartialEq)]
struct M {
    clients: Vec<(u64, String)>,
    /// (id de gamme, tailles (id, libellé, fiche d'origine)) ; la première taille est la base.
    ranges: Vec<(u64, Vec<MS>)>,
    variants: Vec<MV>,
    pieces: Vec<u64>,
    next_id: u64,
}

fn low(s: &str) -> String {
    s.to_lowercase()
}

#[derive(Default, Debug)]
struct C {
    sequences: usize,
    steps: usize,
    gen_new: usize,
    gen_replace_changed: usize,
    gen_replace_same: usize,
    gen_refused_dup: usize,
    client_removed: usize,
    client_refused_cited: usize,
    client_refused_ghost: usize,
    size_removed_with_client: usize,
    size_refused_base: usize,
    range_removed_with_clients: usize,
    rename_client: usize,
    rename_size: usize,
    entry_added: usize,
    entry_refused: usize,
    entry_updated: usize,
    entry_removed: usize,
    opt_added: usize,
    opt_removed: usize,
    opt_remove_refused: usize,
    rename_ok: usize,
    rename_case_only: usize,
    rename_same: usize,
    rename_refused_empty: usize,
    rename_refused_dup: usize,
    rename_refused_unknown: usize,
    rename_rewrote_entries: usize,
    rename_rewrote_active: usize,
    move_ok: usize,
    move_same: usize,
    move_refused_range: usize,
    move_refused_unknown: usize,
    dup_variant: usize,
    undo_exact: usize,
    redo_exact: usize,
    migrations: usize,
    migrated_entries: usize,
    roundtrips: usize,
}

// ---------------------------------------------------------------------------
// Fixture
// ---------------------------------------------------------------------------

fn fixture() -> (Project, M) {
    let mut p = Project::new("Campagne v6");
    let s = p.sheets[0].id;
    let mut pieces = Vec::new();
    for (i, name) in ["Dos", "Poche", "Col"].into_iter().enumerate() {
        let r = p
            .apply(Command::AddRectangle {
                sheet: s,
                corner: PointRef::New(PointMm::new(i as f64 * 200.0, 0.0)),
                width_mm: 100.0,
                height_mm: 40.0,
            })
            .unwrap();
        let id = p
            .apply(Command::ExtractPiece {
                sheet: s,
                source: PieceSource::Paths {
                    entities: r.entities,
                },
                name: name.into(),
                detached: false,
            })
            .unwrap()
            .pieces[0];
        pieces.push(id.0);
    }
    for (name, labels) in [
        ("A", vec!["S", "M", "L"]),
        ("B", vec!["T0", "T1", "T2", "T3"]),
    ] {
        p.apply(Command::AddSizeRange {
            name: name.into(),
            kind: SizeKind::Alphanumeric,
            labels: labels.into_iter().map(String::from).collect(),
            base: 0,
        })
        .unwrap();
    }
    let mut clients = Vec::new();
    for name in ["Alice", "Bob", "Chloe"] {
        let id = p
            .apply(Command::AddClient { name: name.into() })
            .unwrap()
            .clients[0];
        clients.push((id.0, name.to_string()));
    }
    let mut variants = Vec::new();
    for name in ["Robe", "Veste"] {
        let id = p
            .apply(Command::AddVariant { name: name.into() })
            .unwrap()
            .variants[0];
        variants.push(MV {
            id: id.0,
            name: name.into(),
            options: vec![],
            active: vec![],
            entries: vec![],
        });
    }
    let ranges = p
        .sizes
        .iter()
        .map(|r| {
            (
                r.id.0,
                r.sizes
                    .iter()
                    .map(|z| (z.id.0, z.label.clone(), None))
                    .collect(),
            )
        })
        .collect();
    let m = M {
        clients,
        ranges,
        variants,
        pieces,
        next_id: p.next_id,
    };
    (p, m)
}

fn check_state(p: &Project, m: &M) {
    assert_eq!(p.next_id, m.next_id, "compteur d'identifiants");
    assert_eq!(p.clients.len(), m.clients.len());
    for (c, (id, name)) in p.clients.iter().zip(&m.clients) {
        assert_eq!((c.id.0, &c.name), (*id, name));
    }
    assert_eq!(p.sizes.len(), m.ranges.len());
    for (r, (id, sizes)) in p.sizes.iter().zip(&m.ranges) {
        assert_eq!(r.id.0, *id);
        let got: Vec<MS> = r
            .sizes
            .iter()
            .map(|s| (s.id.0, s.label.clone(), s.client.map(|c| c.0)))
            .collect();
        assert_eq!(&got, sizes, "gamme {id}");
    }
    assert_eq!(p.variants.len(), m.variants.len());
    for (v, mv) in p.variants.iter().zip(&m.variants) {
        assert_eq!((v.id.0, &v.name), (mv.id, &mv.name));
        assert_eq!(v.options, mv.options, "options de {}", mv.name);
        assert_eq!(
            v.options_active, mv.active,
            "options actives de {}",
            mv.name
        );
        let got: Vec<ME> = v
            .entries
            .iter()
            .map(|e| ME {
                id: e.id.0,
                piece: e.piece.0,
                opt: e.option.clone(),
            })
            .collect();
        assert_eq!(got, mv.entries, "entrées de {}", mv.name);
    }
}

// ---------------------------------------------------------------------------
// Pas
// ---------------------------------------------------------------------------

enum Verdict {
    Ok,
    Refused(&'static str),
}

const LABELS: [&str; 8] = ["T38", "T40", "t40", "T42", "S", "m", "T3", "X1"];
const OPTS: [&str; 6] = ["poche", "col", "manche", "ceinture", "Poche", "doublure"];

/// Un pas : commande, verdict et mise à jour de l'oracle.
fn pick_step(r: &mut Rng, m: &mut M, c: &mut C) -> (Command, Verdict, Option<String>) {
    let ghost = 900_000 + r.below(5) as u64;
    if m.clients.is_empty() {
        // Plus aucune fiche : on en crée une (identifiant = compteur).
        let name = format!("N{}", r.below(1000));
        let id = m.next_id;
        m.next_id += 1;
        m.clients.push((id, name.clone()));
        return (Command::AddClient { name }, Verdict::Ok, None);
    }
    let k = r.below(100);
    match k {
        0..=14 => {
            // Génération d'une taille depuis une fiche.
            let ci = r.below(m.clients.len());
            let ri = r.below(m.ranges.len());
            let label = LABELS[r.below(LABELS.len())];
            let replace = r.chance(50);
            let cmd = Command::GenerateClientSize {
                client: ClientId(m.clients[ci].0),
                range: SizeRangeId(m.ranges[ri].0),
                label: label.into(),
                replace,
            };
            let found = m.ranges[ri].1.iter().position(|s| low(&s.1) == low(label));
            match found {
                Some(_) if !replace => {
                    c.gen_refused_dup += 1;
                    (cmd, Verdict::Refused("dup"), None)
                }
                Some(i) => {
                    let new = Some(m.clients[ci].0);
                    if m.ranges[ri].1[i].2 == new {
                        c.gen_replace_same += 1;
                    } else {
                        c.gen_replace_changed += 1;
                    }
                    m.ranges[ri].1[i].2 = new;
                    (cmd, Verdict::Ok, None)
                }
                None => {
                    let id = m.next_id;
                    m.next_id += 1;
                    m.ranges[ri]
                        .1
                        .push((id, label.into(), Some(m.clients[ci].0)));
                    c.gen_new += 1;
                    (cmd, Verdict::Ok, None)
                }
            }
        }
        15..=24 => {
            // Suppression d'une fiche (parfois fantôme).
            if r.chance(8) {
                c.client_refused_ghost += 1;
                return (
                    Command::RemoveClient {
                        client: ClientId(ghost),
                    },
                    Verdict::Refused("fantôme"),
                    None,
                );
            }
            let ci = r.below(m.clients.len());
            let id = m.clients[ci].0;
            let cmd = Command::RemoveClient {
                client: ClientId(id),
            };
            let cited = m
                .ranges
                .iter()
                .flat_map(|g| g.1.iter())
                .find(|s| s.2 == Some(id))
                .map(|s| s.1.clone());
            match cited {
                Some(label) => {
                    c.client_refused_cited += 1;
                    (cmd, Verdict::Refused("citée"), Some(label))
                }
                None => {
                    m.clients.remove(ci);
                    c.client_removed += 1;
                    (cmd, Verdict::Ok, None)
                }
            }
        }
        25..=29 => {
            // Suppression d'une taille (la base est refusée).
            let ri = r.below(m.ranges.len());
            let si = r.below(m.ranges[ri].1.len());
            let (id, _, client) = m.ranges[ri].1[si].clone();
            let cmd = Command::RemoveSize { size: SizeId(id) };
            if si == 0 {
                c.size_refused_base += 1;
                return (cmd, Verdict::Refused("base"), None);
            }
            if client.is_some() {
                c.size_removed_with_client += 1;
            }
            m.ranges[ri].1.remove(si);
            (cmd, Verdict::Ok, None)
        }
        30..=31 => {
            // Suppression d'une gamme entière (rare : on garde au moins une gamme).
            if m.ranges.len() < 2 {
                let ci = r.below(m.clients.len());
                m.clients[ci].1 = format!("Cl{}", r.below(1000));
                let name = m.clients[ci].1.clone();
                c.rename_client += 1;
                return (
                    Command::RenameClient {
                        client: ClientId(m.clients[ci].0),
                        name,
                    },
                    Verdict::Ok,
                    None,
                );
            }
            let ri = r.below(m.ranges.len());
            if m.ranges[ri].1.iter().any(|s| s.2.is_some()) {
                c.range_removed_with_clients += 1;
            }
            let id = m.ranges[ri].0;
            m.ranges.remove(ri);
            (
                Command::RemoveSizeRange {
                    range: SizeRangeId(id),
                },
                Verdict::Ok,
                None,
            )
        }
        32..=34 => {
            // Renommer une fiche ou une taille : la référence suit l'id.
            if r.chance(50) {
                let ci = r.below(m.clients.len());
                let name = format!("Cl{}-{}", r.below(1000), ci);
                if m.clients.iter().any(|x| low(&x.1) == low(&name)) {
                    c.rename_client += 0;
                    return (
                        Command::RenameClient {
                            client: ClientId(m.clients[ci].0),
                            name,
                        },
                        Verdict::Refused("nom pris"),
                        None,
                    );
                }
                m.clients[ci].1 = name.clone();
                c.rename_client += 1;
                (
                    Command::RenameClient {
                        client: ClientId(m.clients[ci].0),
                        name,
                    },
                    Verdict::Ok,
                    None,
                )
            } else {
                let ri = r.below(m.ranges.len());
                let si = r.below(m.ranges[ri].1.len());
                let label = format!("R{}", r.below(10_000));
                let id = m.ranges[ri].1[si].0;
                if m.ranges[ri].1.iter().any(|s| low(&s.1) == low(&label)) {
                    return (
                        Command::RenameSize {
                            size: SizeId(id),
                            label,
                        },
                        Verdict::Refused("libellé pris"),
                        None,
                    );
                }
                m.ranges[ri].1[si].1 = label.clone();
                c.rename_size += 1;
                (
                    Command::RenameSize {
                        size: SizeId(id),
                        label,
                    },
                    Verdict::Ok,
                    None,
                )
            }
        }
        35..=42 => {
            // Ajout d'une option (nom unique sans la casse, 20 au plus).
            let vi = r.below(m.variants.len());
            let name = OPTS[r.below(OPTS.len())];
            let v = &mut m.variants[vi];
            let cmd = Command::AddVariantOption {
                variant: VariantId(v.id),
                option: name.into(),
            };
            if v.options.iter().any(|o| low(o) == low(name)) {
                return (cmd, Verdict::Refused("option existante"), None);
            }
            v.options.push(name.into());
            c.opt_added += 1;
            (cmd, Verdict::Ok, None)
        }
        43..=47 => {
            // Activer / désactiver une option.
            let vi = r.below(m.variants.len());
            let v = &mut m.variants[vi];
            if v.options.is_empty() {
                let cmd = Command::SetVariantOptionActive {
                    variant: VariantId(v.id),
                    option: "rien".into(),
                    active: true,
                };
                return (cmd, Verdict::Refused("option inconnue"), None);
            }
            let o = v.options[r.below(v.options.len())].clone();
            let active = r.chance(60);
            let cmd = Command::SetVariantOptionActive {
                variant: VariantId(v.id),
                option: o.clone(),
                active,
            };
            let is = v.active.contains(&o);
            if active && !is {
                v.active.push(o);
            } else if !active && is {
                v.active.retain(|x| *x != o);
            }
            (cmd, Verdict::Ok, None)
        }
        48..=60 => {
            // Ajout d'une entrée (pièce, option) : identifiant = compteur.
            let vi = r.below(m.variants.len());
            let piece = m.pieces[r.below(m.pieces.len())];
            let opt = if m.variants[vi].options.is_empty() || r.chance(30) {
                None
            } else {
                let v = &m.variants[vi];
                Some(v.options[r.below(v.options.len())].clone())
            };
            let (vid, dup) = {
                let v = &m.variants[vi];
                (
                    v.id,
                    v.entries.iter().any(|e| e.piece == piece && e.opt == opt),
                )
            };
            let cmd = Command::AddVariantEntry {
                variant: VariantId(vid),
                piece: PieceId(piece),
                quantity: 1,
                material: None,
                option: opt.clone(),
            };
            if dup {
                c.entry_refused += 1;
                return (cmd, Verdict::Refused("entrée dupliquée"), None);
            }
            let id = m.next_id;
            m.next_id += 1;
            m.variants[vi].entries.push(ME { id, piece, opt });
            c.entry_added += 1;
            (cmd, Verdict::Ok, None)
        }
        61..=64 => {
            // Modification d'une entrée : l'identifiant ne change pas.
            let vi = r.below(m.variants.len());
            if m.variants[vi].entries.is_empty() {
                return (
                    Command::RemoveVariantEntry {
                        variant: VariantId(m.variants[vi].id),
                        piece: PieceId(m.pieces[0]),
                        option: None,
                    },
                    Verdict::Refused("entrée introuvable"),
                    None,
                );
            }
            let at = r.below(m.variants[vi].entries.len());
            let e = m.variants[vi].entries[at].clone();
            let new_opt = if m.variants[vi].options.is_empty() || r.chance(30) {
                None
            } else {
                let v = &m.variants[vi];
                Some(v.options[r.below(v.options.len())].clone())
            };
            let cmd = Command::UpdateVariantEntry {
                variant: VariantId(m.variants[vi].id),
                piece: PieceId(e.piece),
                option: e.opt.clone(),
                quantity: 1,
                material: None,
                new_option: new_opt.clone(),
            };
            let dup = m.variants[vi]
                .entries
                .iter()
                .enumerate()
                .any(|(i, x)| i != at && x.piece == e.piece && x.opt == new_opt);
            if dup {
                c.entry_refused += 1;
                return (cmd, Verdict::Refused("entrée dupliquée"), None);
            }
            m.variants[vi].entries[at].opt = new_opt;
            c.entry_updated += 1;
            (cmd, Verdict::Ok, None)
        }
        65..=68 => {
            // Retrait d'une entrée.
            let vi = r.below(m.variants.len());
            if m.variants[vi].entries.is_empty() {
                return (
                    Command::RemoveVariantEntry {
                        variant: VariantId(m.variants[vi].id),
                        piece: PieceId(m.pieces[1]),
                        option: None,
                    },
                    Verdict::Refused("entrée introuvable"),
                    None,
                );
            }
            let at = r.below(m.variants[vi].entries.len());
            let e = m.variants[vi].entries.remove(at);
            c.entry_removed += 1;
            (
                Command::RemoveVariantEntry {
                    variant: VariantId(m.variants[vi].id),
                    piece: PieceId(e.piece),
                    option: e.opt,
                },
                Verdict::Ok,
                None,
            )
        }
        69..=72 => {
            // Retrait d'une option (refusé si une entrée la cite).
            let vi = r.below(m.variants.len());
            let v = &mut m.variants[vi];
            if v.options.is_empty() {
                return (
                    Command::RemoveVariantOption {
                        variant: VariantId(v.id),
                        option: "rien".into(),
                    },
                    Verdict::Refused("option inconnue"),
                    None,
                );
            }
            let o = v.options[r.below(v.options.len())].clone();
            let cmd = Command::RemoveVariantOption {
                variant: VariantId(v.id),
                option: o.clone(),
            };
            if v.entries.iter().any(|e| e.opt.as_ref() == Some(&o)) {
                c.opt_remove_refused += 1;
                return (cmd, Verdict::Refused("utilisée"), None);
            }
            v.options.retain(|x| *x != o);
            v.active.retain(|x| *x != o);
            c.opt_removed += 1;
            (cmd, Verdict::Ok, None)
        }
        73..=88 => rename_option_step(r, m, c),
        89..=95 => move_option_step(r, m, c),
        _ => {
            // Duplication d'une variante : identifiants neufs pour la variante puis ses entrées.
            let vi = r.below(m.variants.len());
            let name = format!("Copie{}", r.below(100_000));
            let src = m.variants[vi].clone();
            let cmd = Command::DuplicateVariant {
                variant: VariantId(src.id),
                name: name.clone(),
            };
            if m.variants.iter().any(|v| low(&v.name) == low(&name)) || m.variants.len() >= 50 {
                return (cmd, Verdict::Refused("nom pris"), None);
            }
            let mut copy = src;
            copy.id = m.next_id;
            m.next_id += 1;
            copy.name = name;
            for e in &mut copy.entries {
                e.id = m.next_id;
                m.next_id += 1;
            }
            m.variants.push(copy);
            c.dup_variant += 1;
            (cmd, Verdict::Ok, None)
        }
    }
}

/// Variante sans option visée par un pas d'option : on lui en crée une (pas de refus gratuit).
fn add_first_option(m: &mut M, vi: usize, c: &mut C) -> (Command, Verdict, Option<String>) {
    let v = &mut m.variants[vi];
    let name = format!("opt{}", v.id);
    v.options.push(name.clone());
    c.opt_added += 1;
    (
        Command::AddVariantOption {
            variant: VariantId(v.id),
            option: name,
        },
        Verdict::Ok,
        None,
    )
}

/// Variante visée par un pas d'option : le plus souvent une variante qui en a (sinon au hasard).
fn pick_variant_for_option(r: &mut Rng, m: &M) -> usize {
    let holders: Vec<usize> = (0..m.variants.len())
        .filter(|i| !m.variants[*i].options.is_empty())
        .collect();
    if !holders.is_empty() && r.chance(92) {
        holders[r.below(holders.len())]
    } else {
        r.below(m.variants.len())
    }
}

fn rename_option_step(r: &mut Rng, m: &mut M, c: &mut C) -> (Command, Verdict, Option<String>) {
    let vi = pick_variant_for_option(r, m);
    let vid = m.variants[vi].id;
    let mk = |option: &str, name: &str| Command::RenameVariantOption {
        variant: VariantId(vid),
        option: option.into(),
        name: name.into(),
    };
    if m.variants[vi].options.is_empty() {
        return add_first_option(m, vi, c);
    }
    if r.chance(8) {
        c.rename_refused_unknown += 1;
        return (
            mk("inexistante", "x"),
            Verdict::Refused("option inconnue"),
            None,
        );
    }
    let old = m.variants[vi].options[r.below(m.variants[vi].options.len())].clone();
    // Graphie de la référence : la casse est ignorée à la résolution.
    let reference = if r.chance(40) {
        old.to_uppercase()
    } else {
        old.clone()
    };
    let others: Vec<String> = m.variants[vi]
        .options
        .iter()
        .filter(|o| **o != old)
        .cloned()
        .collect();
    let (name, kind) = match r.below(7) {
        0 => (String::new(), "vide"),
        1 => ("   ".to_string(), "vide"),
        2 if !others.is_empty() => {
            let o = &others[r.below(others.len())];
            (
                if r.chance(50) {
                    o.to_uppercase()
                } else {
                    format!(" {o} ")
                },
                "doublon",
            )
        }
        3 => (old.clone(), "même"),
        4 => (old.to_uppercase(), "casse"),
        _ => (format!("{}-{}", old, r.below(1000)), "neuf"),
    };
    let trimmed = name.trim().to_string();
    let cmd = mk(&reference, &name);
    if trimmed.is_empty() {
        c.rename_refused_empty += 1;
        return (cmd, Verdict::Refused("vide"), None);
    }
    if trimmed.chars().count() > 120 {
        return (cmd, Verdict::Refused("trop long"), None);
    }
    if others.iter().any(|o| low(o) == low(&trimmed)) {
        c.rename_refused_dup += 1;
        let _ = kind;
        return (cmd, Verdict::Refused("doublon"), None);
    }
    // Accepté : réécriture de l'option, de l'état actif et des entrées.
    let v = &mut m.variants[vi];
    if trimmed == old {
        c.rename_same += 1;
    } else if low(&trimmed) == low(&old) {
        c.rename_case_only += 1;
    }
    c.rename_ok += 1;
    for o in v.options.iter_mut().chain(v.active.iter_mut()) {
        if *o == old {
            *o = trimmed.clone();
            if trimmed != old {
                // compté plus bas selon ce qui a réellement été réécrit
            }
        }
    }
    if trimmed != old {
        if v.entries.iter().any(|e| e.opt.as_ref() == Some(&old)) {
            c.rename_rewrote_entries += 1;
        }
        if v.active.contains(&trimmed) {
            c.rename_rewrote_active += 1;
        }
    }
    for e in &mut v.entries {
        if e.opt.as_ref() == Some(&old) {
            e.opt = Some(trimmed.clone());
        }
    }
    (cmd, Verdict::Ok, None)
}

fn move_option_step(r: &mut Rng, m: &mut M, c: &mut C) -> (Command, Verdict, Option<String>) {
    let vi = pick_variant_for_option(r, m);
    let vid = m.variants[vi].id;
    let mk = |option: &str, to: usize| Command::MoveVariantOption {
        variant: VariantId(vid),
        option: option.into(),
        to,
    };
    if m.variants[vi].options.is_empty() {
        return add_first_option(m, vi, c);
    }
    if r.chance(8) {
        c.move_refused_unknown += 1;
        return (
            mk("inexistante", 0),
            Verdict::Refused("option inconnue"),
            None,
        );
    }
    let len = m.variants[vi].options.len();
    let from = r.below(len);
    let o = m.variants[vi].options[from].clone();
    let to = if r.chance(15) {
        len + r.below(3)
    } else if len > 1 && r.chance(85) {
        (from + 1 + r.below(len - 1)) % len
    } else {
        r.below(len)
    };
    let cmd = mk(&o.to_uppercase(), to);
    if to >= len {
        c.move_refused_range += 1;
        return (cmd, Verdict::Refused("position"), None);
    }
    if to == from {
        c.move_same += 1;
    } else {
        c.move_ok += 1;
    }
    let v = &mut m.variants[vi];
    let moved = v.options.remove(from);
    v.options.insert(to, moved);
    (cmd, Verdict::Ok, None)
}

// ---------------------------------------------------------------------------
// Migration relabellisée (oracle indépendant du chargeur)
// ---------------------------------------------------------------------------

/// Texte v5 déduit d'un texte v6 : version 5, `client` des tailles et `id` des entrées retirés,
/// compteur = plus grand identifiant restant + 1. Retourne aussi les identifiants attendus pour
/// les entrées (dans l'ordre du fichier) et le compteur v6 attendu.
fn to_v5(text: &str) -> (String, Vec<u64>, u64) {
    let mut v: Value = serde_json::from_str(text).unwrap();
    v["schema_version"] = json!(5);
    let mut entries = 0usize;
    if let Some(ranges) = v.get_mut("sizes").and_then(Value::as_array_mut) {
        for r in ranges {
            for s in r["sizes"].as_array_mut().unwrap() {
                s.as_object_mut().unwrap().remove("client");
            }
        }
    }
    if let Some(variants) = v.get_mut("variants").and_then(Value::as_array_mut) {
        for var in variants {
            if let Some(es) = var.get_mut("entries").and_then(Value::as_array_mut) {
                for e in es {
                    e.as_object_mut().unwrap().remove("id");
                    entries += 1;
                }
            }
        }
    }
    fn max_id(v: &Value, best: &mut u64) {
        match v {
            Value::Object(o) => {
                if let Some(id) = o.get("id").and_then(Value::as_u64) {
                    *best = (*best).max(id);
                }
                o.values().for_each(|x| max_id(x, best));
            }
            Value::Array(a) => a.iter().for_each(|x| max_id(x, best)),
            _ => {}
        }
    }
    let mut best = 0;
    max_id(&v, &mut best);
    let next = best + 1;
    v["next_id"] = json!(next);
    let ids: Vec<u64> = (next..next + entries as u64).collect();
    (
        serde_json::to_string_pretty(&v).unwrap() + "\n",
        ids,
        next + entries as u64,
    )
}

fn check_migration(p: &Project, c: &mut C) {
    let text = bytes(p);
    let (v5, ids, next) = to_v5(&text);
    let migrated = persist::from_json(&v5).unwrap_or_else(|e| panic!("v5 relabellisé : {e}"));
    assert_eq!(migrated.schema_version, 6);
    assert_eq!(migrated.next_id, next, "compteur après migration");
    let got: Vec<u64> = migrated
        .variants
        .iter()
        .flat_map(|v| v.entries.iter().map(|e| e.id.0))
        .collect();
    assert_eq!(got, ids, "ids dans l'ordre du fichier");
    assert!(migrated
        .sizes
        .iter()
        .all(|r| r.sizes.iter().all(|s| s.client.is_none())));
    // Tout le reste est conservé : à identifiants d'entrée, compteur et fiches d'origine près.
    let again = bytes(&migrated);
    assert_eq!(bytes(&persist::from_json(&again).unwrap()), again);
    c.migrations += 1;
    c.migrated_entries += ids.len();
}

#[test]
fn campagne_adverse_commandes_v6() {
    let mut c = C::default();
    for seq in 0..SEQUENCES {
        c.sequences += 1;
        let mut r = Rng(0x9E37_79B9_7F4A_7C15u64.wrapping_mul(seq as u64 + 1) ^ 0x6A09_E667);
        let (p, mut m) = fixture();
        check_state(&p, &m);
        let mut doc = Document::new(p);
        // Piles d'annulation de l'oracle : (modèle, octets) avant chaque pas effectif.
        let mut undo: Vec<(M, String)> = Vec::new();
        let mut redo: Vec<(M, String)> = Vec::new();
        for _ in 0..STEPS {
            c.steps += 1;
            if r.chance(12) && !undo.is_empty() {
                // Annuler : octets et modèle d'avant.
                let (pm, pb) = undo.pop().unwrap();
                redo.push((m.clone(), bytes(doc.project())));
                assert!(doc.undo());
                m = pm;
                assert_eq!(bytes(doc.project()), pb, "annulation exacte à l'octet");
                check_state(doc.project(), &m);
                c.undo_exact += 1;
                continue;
            }
            if r.chance(8) && !redo.is_empty() {
                let (nm, nb) = redo.pop().unwrap();
                undo.push((m.clone(), bytes(doc.project())));
                assert!(doc.redo());
                m = nm;
                assert_eq!(bytes(doc.project()), nb, "rétablissement exact à l'octet");
                check_state(doc.project(), &m);
                c.redo_exact += 1;
                continue;
            }
            let before_m = m.clone();
            let before_bytes = bytes(doc.project());
            let (cmd, verdict, cited_label) = pick_step(&mut r, &mut m, &mut c);
            let res = doc.execute(cmd.clone());
            match verdict {
                Verdict::Refused(why) => {
                    let err = res.expect_err(&format!("{cmd:?} doit être refusé ({why})"));
                    assert_eq!(
                        bytes(doc.project()),
                        before_bytes,
                        "refus sans trace : {why}"
                    );
                    m = before_m;
                    if let Some(label) = cited_label {
                        let msg = err.to_string();
                        assert!(
                            msg.contains(&format!("par la taille « {label} » de la gamme")),
                            "{msg}"
                        );
                    }
                }
                Verdict::Ok => {
                    res.unwrap_or_else(|e| panic!("{cmd:?} doit être accepté : {e}"));
                    let changed = bytes(doc.project()) != before_bytes;
                    assert_eq!(changed, m != before_m, "{cmd:?} : octets ⇔ oracle");
                    if changed {
                        undo.push((before_m, before_bytes));
                        redo.clear();
                    }
                }
            }
            check_state(doc.project(), &m);
            let text = bytes(doc.project());
            assert_eq!(
                bytes(&persist::from_json(&text).expect("relecture")),
                text,
                "aller-retour octet"
            );
            c.roundtrips += 1;
        }
        check_migration(doc.project(), &mut c);
        // Tout annuler redonne l'état de départ.
        while doc.undo() {}
        let (p0, m0) = fixture();
        assert_eq!(doc.project(), &p0);
        check_state(doc.project(), &m0);
    }
    eprintln!("{c:?}");
    assert_eq!(c.sequences, SEQUENCES);
    // Seuils = environ la moitié des comptes observés (graine fixe : reproductibles).
    let checks: [(&str, usize, usize); 38] = [
        ("gen_new", c.gen_new, 250),
        ("gen_replace_changed", c.gen_replace_changed, 40),
        ("gen_replace_same", c.gen_replace_same, 15),
        ("gen_refused_dup", c.gen_refused_dup, 50),
        ("client_removed", c.client_removed, 100),
        ("client_refused_cited", c.client_refused_cited, 100),
        ("client_refused_ghost", c.client_refused_ghost, 15),
        ("size_removed_with_client", c.size_removed_with_client, 15),
        ("size_refused_base", c.size_refused_base, 30),
        (
            "range_removed_with_clients",
            c.range_removed_with_clients,
            15,
        ),
        ("rename_client", c.rename_client, 40),
        ("rename_size", c.rename_size, 30),
        ("entry_added", c.entry_added, 250),
        ("entry_refused", c.entry_refused, 40),
        ("entry_updated", c.entry_updated, 40),
        ("entry_removed", c.entry_removed, 40),
        ("opt_added", c.opt_added, 250),
        ("opt_removed", c.opt_removed, 40),
        ("opt_remove_refused", c.opt_remove_refused, 15),
        ("rename_ok", c.rename_ok, 150),
        ("rename_case_only", c.rename_case_only, 30),
        ("rename_same", c.rename_same, 30),
        ("rename_refused_empty", c.rename_refused_empty, 70),
        ("rename_refused_dup", c.rename_refused_dup, 10),
        ("rename_refused_unknown", c.rename_refused_unknown, 25),
        ("rename_rewrote_entries", c.rename_rewrote_entries, 25),
        ("rename_rewrote_active", c.rename_rewrote_active, 12),
        ("move_ok", c.move_ok, 25),
        ("move_same", c.move_same, 80),
        ("move_refused_range", c.move_refused_range, 20),
        ("move_refused_unknown", c.move_refused_unknown, 8),
        ("dup_variant", c.dup_variant, 80),
        ("undo_exact", c.undo_exact, 300),
        ("redo_exact", c.redo_exact, 30),
        ("roundtrips", c.roundtrips, 4000),
        ("migrations", c.migrations, SEQUENCES),
        ("migrated_entries", c.migrated_entries, 200),
        ("steps", c.steps, SEQUENCES * STEPS),
    ];
    for (name, got, min) in checks {
        assert!(got >= min, "compteur {name} : {got} < {min} ({c:?})");
    }
}

// ---------------------------------------------------------------------------
// Fichiers v6 abîmés
// ---------------------------------------------------------------------------

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Value {
    json!([{"x": x0, "y": y0}, {"x": x1, "y": y0}, {"x": x1, "y": y1}, {"x": x0, "y": y1}])
}

fn hex64(r: &mut Rng) -> String {
    (0..64)
        .map(|_| char::from_digit(r.below(16) as u32, 16).unwrap())
        .collect()
}

/// Fichier v6 valide aléatoire : un fond (deux ou quatre points), une peau avec défauts et zones.
/// Les identifiants sont ceux de l'écriture à la main ci-dessous (compteur 100).
fn random_valid(r: &mut Rng) -> Value {
    let (w, h) = (1000.0 + r.half(0, 500), 800.0 + r.half(0, 400));
    let n_defects = 1 + r.below(4);
    let mut defects = Vec::new();
    for i in 0..n_defects {
        let x = 50.0 + i as f64 * 150.0 + r.half(0, 20);
        let y = 50.0 + r.half(0, 100);
        let kind = ["hole", "scar", "stain", "thin", "other"][r.below(5)];
        defects.push(json!({
            "id": 10 + i,
            "kind": kind,
            "polygon": [{"x": x, "y": y}, {"x": x + 40.0, "y": y}, {"x": x + 20.0, "y": y + 30.0}]
        }));
    }
    let calibration = if r.chance(50) {
        json!({"kind": "two_points",
            "points_px": [{"x": 10.0, "y": 20.0}, {"x": 10.0 + r.half(10, 500), "y": 20.0}],
            "known_distance_mm": r.half(1, 900), "residual_mm": r.half(0, 3)})
    } else {
        json!({"kind": "four_points",
            "points_px": [{"x": 0.0, "y": 0.0}, {"x": 400.0, "y": 0.0}, {"x": 400.0, "y": 300.0}, {"x": 0.0, "y": 300.0}],
            "known_width_mm": r.half(1, 500), "known_height_mm": r.half(1, 500), "residual_mm": 0.0})
    };
    let mut doc = json!({
        "format": "patternforge", "schema_version": 6, "name": "Abîmé", "next_id": 100,
        "sheets": [{"id": 1, "name": "Feuille 1", "points": [], "entities": [], "background": {
            "image": format!("fonds/scan{}.png", r.below(100)), "sha256": hex64(r),
            "width_px": 100 + r.below(5000), "height_px": 100 + r.below(5000),
            "calibration": calibration,
            "opacity": r.half(0, 2) / 2.0, "visible": r.chance(50), "locked": r.chance(50)
        }}],
        "cutting_plans": [{"id": 2, "name": "Peau", "fabric_width_mm": 1500.0, "fold": "flat",
            "selvedge_mm": 0.0, "min_gap_mm": 0.0,
            "hide": {"contour": rect(0.0, 0.0, w, h), "defects": defects,
                "quality_zones": [{"id": 30, "grade": 1 + r.below(3),
                    "polygon": [{"x": 0.0, "y": 0.0}, {"x": 300.0, "y": 0.0}, {"x": 150.0, "y": 200.0}]}]}}],
        "clients": [{"id": 3, "name": "Alice"}],
        "sizes": [{"id": 4, "name": "G", "kind": "numeric", "base": 5,
            "sizes": [{"id": 5, "label": "38"}, {"id": 6, "label": "sur-mesure", "client": 3}]}]
    });
    if r.chance(30) {
        // Sans zone : la clé est omise (jamais écrite vide).
        doc["cutting_plans"][0]["hide"]
            .as_object_mut()
            .unwrap()
            .remove("quality_zones");
    }
    doc
}

/// Premier chemin où deux documents divergent (message d'échec lisible).
fn first_delta(path: &str, a: &Value, b: &Value) -> String {
    match (a, b) {
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys()) {
                match (x.get(k), y.get(k)) {
                    (Some(u), Some(v)) if u != v => {
                        return first_delta(&format!("{path}/{k}"), u, v)
                    }
                    (Some(_), None) | (None, Some(_)) => {
                        return format!("{path}/{k} absent d'un côté")
                    }
                    _ => {}
                }
            }
            String::new()
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (u, v)) in x.iter().zip(y).enumerate() {
                if u != v {
                    return first_delta(&format!("{path}/{i}"), u, v);
                }
            }
            String::new()
        }
        _ => format!("{path} : {a} ≠ {b}"),
    }
}

#[derive(Default, Debug)]
struct F {
    valid_roundtrip: usize,
    by_kind: BTreeMap<&'static str, usize>,
    cli_checked: usize,
}

fn set(doc: &mut Value, pointer: &str, v: Value) {
    *doc.pointer_mut(pointer).unwrap() = v;
}

/// Applique la corruption `k` ; retourne (nom, fragment attendu du message).
fn corrupt(r: &mut Rng, doc: &mut Value, k: usize) -> (&'static str, &'static str) {
    let bg = "/sheets/0/background";
    let hide = "/cutting_plans/0/hide";
    match k {
        0 => {
            set(
                doc,
                &format!("{bg}/image"),
                json!(format!("/home/u/{}.png", r.below(99))),
            );
            ("chemin absolu", "chemin absolu")
        }
        1 => {
            set(
                doc,
                &format!("{bg}/image"),
                json!(format!("C:/fonds/{}.png", r.below(99))),
            );
            ("lecteur", "« : » refusé")
        }
        2 => {
            let p = ["../a.png", "x/../../a.png", "fonds/..", ".."][r.below(4)];
            set(doc, &format!("{bg}/image"), json!(p));
            ("point-point", "segment « .. » refusé")
        }
        3 => {
            let bad = match r.below(4) {
                0 => "0".repeat(63),
                1 => "0".repeat(65),
                2 => "g".repeat(64),
                _ => "F".repeat(64),
            };
            set(doc, &format!("{bg}/sha256"), json!(bad));
            ("empreinte fausse", "l'empreinte doit avoir 64 chiffres")
        }
        4 => {
            let bad = [-0.5, 1.5, 2.0][r.below(3)];
            set(doc, &format!("{bg}/opacity"), json!(bad));
            ("opacité", "l'opacité doit être comprise entre 0 et 1")
        }
        5 => {
            set(doc, &format!("{bg}/width_px"), json!(0));
            ("largeur nulle", "largeur en pixels hors de")
        }
        6 => {
            let kind = doc
                .pointer(&format!("{bg}/calibration/kind"))
                .unwrap()
                .clone();
            let field = if kind == json!("two_points") {
                "known_distance_mm"
            } else {
                "known_width_mm"
            };
            let bad = [0.0, -1.0][r.below(2)];
            set(doc, &format!("{bg}/calibration/{field}"), json!(bad));
            ("distance nulle", "doit être > 0")
        }
        7 => {
            // Contour auto-croisé (papillon).
            set(
                doc,
                &format!("{hide}/contour"),
                json!([{"x": 0.0, "y": 0.0}, {"x": 500.0, "y": 500.0}, {"x": 500.0, "y": 0.0}, {"x": 0.0, "y": 500.0}]),
            );
            doc.pointer_mut(hide)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove("defects");
            ("polygone auto-croisé", "le polygone n'est pas simple")
        }
        8 => {
            // Défaut hors contour : sommet très à droite.
            set(
                doc,
                &format!("{hide}/defects/0/polygon"),
                json!([{"x": 90000.0, "y": 10.0}, {"x": 90040.0, "y": 10.0}, {"x": 90020.0, "y": 40.0}]),
            );
            (
                "défaut hors contour",
                "le défaut sort du contour de la peau",
            )
        }
        9 => {
            set(doc, &format!("{hide}/defects/0/kind"), json!("tear"));
            ("genre de défaut", "unknown variant")
        }
        10 => {
            doc["schema_version"] = json!(5);
            ("clé v6 dans un fichier v5", "réservée au format version 6")
        }
        11 => {
            set(doc, "/sizes/0/sizes/1/client", json!(900));
            ("fiche inexistante", "n'existe plus")
        }
        12 => {
            doc.pointer_mut(bg).unwrap()["zz"] = json!(1);
            ("clé en trop", "unknown field")
        }
        13 => {
            // Identifiant de défaut en double avec celui du plan.
            set(doc, &format!("{hide}/defects/0/id"), json!(2));
            (
                "identifiant en double",
                "identifiant en double dans le projet (2)",
            )
        }
        14 => {
            // Zone de qualité hors note.
            doc.pointer_mut(hide).unwrap()["quality_zones"] = json!([{"id": 30, "grade": 7,
                "polygon": [{"x": 0.0, "y": 0.0}, {"x": 5.0, "y": 0.0}, {"x": 0.0, "y": 5.0}]}]);
            ("note de qualité", "la note de qualité doit être 1, 2 ou 3")
        }
        _ => {
            // Défaut dont un côté traverse le contour (contour en U).
            set(
                doc,
                &format!("{hide}/contour"),
                json!([{"x": 0.0, "y": 0.0}, {"x": 300.0, "y": 0.0}, {"x": 300.0, "y": 300.0}, {"x": 200.0, "y": 300.0},
                       {"x": 200.0, "y": 100.0}, {"x": 100.0, "y": 100.0}, {"x": 100.0, "y": 300.0}, {"x": 0.0, "y": 300.0}]),
            );
            let h = doc.pointer_mut(hide).unwrap().as_object_mut().unwrap();
            h.remove("quality_zones");
            h.insert(
                "defects".into(),
                json!([{"id": 10, "kind": "hole",
                    "polygon": [{"x": 50.0, "y": 250.0}, {"x": 250.0, "y": 250.0}, {"x": 250.0, "y": 280.0}, {"x": 50.0, "y": 280.0}]}]),
            );
            (
                "défaut qui traverse le contour",
                "le défaut sort du contour de la peau",
            )
        }
    }
}

#[test]
fn campagne_adverse_fichiers_v6_abimes() {
    let mut f = F::default();
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "adverse_v6-{}",
        format!("{:?}", std::thread::current().id()).replace(['(', ')'], "")
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    for seq in 0..SEQUENCES {
        let mut r = Rng(0xD1B5_4A32_D192_ED03u64.wrapping_mul(seq as u64 + 1) ^ 0x3C6E_F372);
        let good = random_valid(&mut r);
        // Le fichier valide est accepté et relu à l'octet.
        let p = persist::from_json(&good.to_string()).unwrap_or_else(|e| panic!("#{seq} : {e}"));
        let text = bytes(&p);
        assert_eq!(bytes(&persist::from_json(&text).unwrap()), text, "#{seq}");
        let written: Value = serde_json::from_str(&text).unwrap();
        if written != good {
            panic!(
                "#{seq} : oracle JSON : {}",
                first_delta("", &written, &good)
            );
        }
        f.valid_roundtrip += 1;
        // Une corruption au hasard, plus la corruption (seq mod 16) pour tout couvrir.
        for k in [r.below(16), seq % 16] {
            let mut doc = good.clone();
            let (name, needle) = corrupt(&mut r, &mut doc, k);
            let bad = doc.to_string();
            let err = persist::from_json(&bad).expect_err(&format!("#{seq} {name} accepté"));
            let msg = err.to_string();
            assert!(
                msg.contains(needle),
                "#{seq} {name} : « {msg} » sans « {needle} »"
            );
            *f.by_kind.entry(name).or_default() += 1;
            // pf-validate refuse aussi (code 1) : un échantillon, un lancement coûte.
            if seq % 20 == 0 {
                let path = dir.join(format!("{seq}-{k}.pforge.json"));
                std::fs::write(&path, &bad).unwrap();
                let out = std::process::Command::new(env!("CARGO_BIN_EXE_pf-validate"))
                    .arg(&path)
                    .output()
                    .unwrap();
                assert_eq!(out.status.code(), Some(1), "#{seq} {name}");
                f.cli_checked += 1;
            }
        }
    }
    eprintln!("{f:?}");
    assert_eq!(f.valid_roundtrip, SEQUENCES);
    assert_eq!(
        f.by_kind.len(),
        16,
        "toutes les corruptions : {:?}",
        f.by_kind
    );
    assert!(f.by_kind.values().all(|n| *n >= 10), "{:?}", f.by_kind);
    assert!(f.cli_checked >= 12);
}

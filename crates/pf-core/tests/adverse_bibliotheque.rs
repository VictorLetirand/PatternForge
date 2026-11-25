//! US-11.4 bibliothèque de bases (tranche 129) — campagne adverse : 160 séquences à graine fixe
//! de 14 à 29 pas qui mêlent enregistrement (pièce, bloc, modèle ; noms valides et refusés),
//! renommage, suppression, fichiers abîmés déposés dans le dossier, insertion (avec ou sans
//! l'option « renommer ») dans un projet dont les variables sont tantôt absentes, tantôt
//! identiques, tantôt en conflit (valeur, unité ou casse), modification des variables du projet,
//! annulations et rétablissements.
//!
//! **Oracle indépendant** : un miroir décide seul (règles écrites ici) de l'acceptation de chaque
//! pas, tient la table des fichiers (numéros réservés, entrées, fichiers abîmés), la table de
//! variables du projet et la liste des **repères suivis** (feuille, point, variable, décalage) dont
//! la position attendue est `(valeur de la variable + dx ; dy)`, avec `dx = dy = 5 × n` pour la
//! n-ième insertion de bloc dans la feuille (décalage du collage). Le dossier est relu **à la main**
//! (`serde_json::Value`), jamais par `Library`. Des compteurs prouvent que chaque contrôle fort
//! s'exécute. Mutations vérifiées à la main : voir `docs/JOURNAL.md` (tranche 129).

mod bibliotheque_support;

use bibliotheque_support::*;
use pf_core::*;
use serde_json::Value;
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

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Piece,
    Block,
    Model,
}

fn kind_name(k: Kind) -> &'static str {
    match k {
        Kind::Piece => "piece",
        Kind::Block => "block",
        Kind::Model => "model",
    }
}

#[derive(Clone)]
struct MEntry {
    name: String,
    kind: Kind,
    /// Valeur de `L` dans l'entrée.
    l: i64,
}

enum MFile {
    Entry(MEntry),
    Junk,
}

#[derive(Clone)]
struct MVar {
    name: String,
    mm: bool,
    value: i64,
}

#[derive(Clone)]
struct Tracked {
    sheet: SheetId,
    point: PointId,
    var: String,
    d: f64,
}

/// Miroir du projet cible.
#[derive(Clone)]
struct MTarget {
    vars: Vec<MVar>,
    tracked: Vec<Tracked>,
    sheets: usize,
    pieces: usize,
    points0: usize,
    blocks: usize,
}

#[derive(Default)]
struct Cov {
    saved: BTreeMap<&'static str, usize>,
    name_refused: BTreeMap<&'static str, usize>,
    renamed: usize,
    rename_refused: usize,
    deleted: usize,
    junk: usize,
    inserted: BTreeMap<&'static str, usize>,
    conflict_refused: usize,
    conflict_renamed: usize,
    reused: usize,
    added: usize,
    undone: usize,
    redone: usize,
    var_changed: usize,
    shifted_blocks: usize,
    checks_positions: usize,
    listings: usize,
}

fn norm(name: &str) -> String {
    name.trim().to_string()
}

fn name_error(name: &str) -> Option<&'static str> {
    let t = name.trim();
    if t.is_empty() {
        Some("vide")
    } else if t.chars().any(|c| (c as u32) < 32 || c as u32 == 127) {
        Some("controle")
    } else if t.chars().count() > 80 {
        Some("long")
    } else {
        None
    }
}

fn expr_for(v: i64, rng: &mut Rng) -> String {
    if v % 2 == 0 && rng.chance(50) {
        format!("{}*2", v / 2)
    } else {
        v.to_string()
    }
}

/// Lecture indépendante du dossier : (numéro, nom, genre) des fichiers valides au premier niveau,
/// et nombre de fichiers qui ne se lisent pas comme une entrée (JSON + clés attendues).
fn raw_dir(dir: &std::path::Path) -> (BTreeMap<u64, (String, String)>, usize) {
    let mut ok = BTreeMap::new();
    let mut bad = 0;
    let Ok(rd) = std::fs::read_dir(dir) else {
        return (ok, 0);
    };
    for e in rd.flatten() {
        let name = e.file_name().into_string().unwrap();
        let Some(num) = name
            .strip_prefix("lib-")
            .and_then(|s| s.strip_suffix(".pforge-lib.json"))
        else {
            continue;
        };
        let num: u64 = num.parse().unwrap();
        let parsed = std::fs::read_to_string(e.path())
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok());
        match parsed {
            Some(v)
                if v["format"] == "patternforge-library"
                    && v["id"].as_u64() == Some(num)
                    && v["name"].is_string()
                    && v["content"]["kind"].is_string() =>
            {
                ok.insert(
                    num,
                    (
                        v["name"].as_str().unwrap().to_string(),
                        v["content"]["kind"].as_str().unwrap().to_string(),
                    ),
                );
            }
            _ => bad += 1,
        }
    }
    (ok, bad)
}

fn run(seed: u64, cov: &mut Cov) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let t = Tmp::new(&format!("adv{seed}"));
    let lib = Library::new(t.path());
    let sources = [(50_i64, fixture("50")), (70_i64, fixture("70"))];
    let mut files: BTreeMap<u64, MFile> = BTreeMap::new();

    let mut doc = Document::new(Project::new("Cible"));
    let sheet0 = doc.project().sheets[0].id;
    let mut mt = MTarget {
        vars: Vec::new(),
        tracked: Vec::new(),
        sheets: 1,
        pieces: 0,
        points0: 0,
        blocks: 0,
    };
    let mut undo: Vec<(MTarget, String)> = Vec::new();
    let mut redo: Vec<(MTarget, String)> = Vec::new();
    let mut now_bytes = bytes(doc.project());

    let steps = 14 + rng.below(16);
    for step in 0..steps {
        let ctx = format!("graine {seed}, pas {step}");
        let next_num = files.keys().max().map_or(1, |m| m + 1);
        match rng.below(100) {
            // --- enregistrer ---
            0..=24 => {
                let (l, fx) = rng.pick(&sources.iter().collect::<Vec<_>>());
                let kind = rng.pick(&[Kind::Piece, Kind::Block, Kind::Model]);
                let content = match kind {
                    Kind::Piece => fx.p.library_piece(fx.piece).unwrap(),
                    Kind::Block => {
                        fx.p.library_block(fx.sheet, &[fx.point], &[fx.base], &[])
                            .unwrap()
                    }
                    Kind::Model => fx.p.library_model().unwrap(),
                };
                let name = if rng.chance(30) {
                    rng.pick(&[
                        String::new(),
                        "   ".to_string(),
                        "a\u{1}b".to_string(),
                        "x".repeat(81),
                        "Rabat".to_string(),
                        "RABAT".to_string(),
                    ])
                } else {
                    rng.pick(&[
                        "Rabat".to_string(),
                        " Poche".to_string(),
                        "Col ".to_string(),
                        "Dos".to_string(),
                        "Manche".to_string(),
                        "x".repeat(80),
                    ])
                };
                let dup = files.values().any(|f| {
                    matches!(f, MFile::Entry(e) if e.name.to_lowercase() == norm(&name).to_lowercase())
                });
                let r = lib.save(&name, content);
                match (name_error(&name), dup) {
                    (Some(why), _) => {
                        let e = r.unwrap_err();
                        let want = match why {
                            "vide" => LibraryError::NameEmpty,
                            "controle" => LibraryError::NameControl,
                            _ => LibraryError::NameTooLong { max: 80 },
                        };
                        assert_eq!(e, want, "{ctx}");
                        *cov.name_refused.entry(why).or_default() += 1;
                    }
                    (None, true) => {
                        assert!(
                            matches!(r, Err(LibraryError::NameDuplicate { .. })),
                            "{ctx}"
                        );
                        *cov.name_refused.entry("doublon").or_default() += 1;
                    }
                    (None, false) => {
                        let e = r.unwrap_or_else(|e| panic!("{ctx} : {e}"));
                        assert_eq!(e.id, next_num, "{ctx} : numéro de fichier");
                        files.insert(
                            next_num,
                            MFile::Entry(MEntry {
                                name: norm(&name),
                                kind,
                                l: *l,
                            }),
                        );
                        *cov.saved.entry(kind_name(kind)).or_default() += 1;
                    }
                }
            }
            // --- renommer ---
            25..=34 => {
                let id = if rng.chance(15) {
                    next_num + 3
                } else {
                    rng.below(next_num + 1)
                };
                let name = rng.pick(&[
                    "Alpha".to_string(),
                    "rabat".to_string(),
                    "Dos".to_string(),
                    String::new(),
                    "\u{7}".to_string(),
                    "Beta".to_string(),
                ]);
                let r = lib.rename(id, &name);
                match files.get(&id) {
                    Some(MFile::Entry(e)) => {
                        let dup = files.iter().any(|(k, f)| {
                            *k != id
                                && matches!(f, MFile::Entry(o) if o.name.to_lowercase() == norm(&name).to_lowercase())
                        });
                        if name_error(&name).is_some() || dup {
                            assert!(r.is_err(), "{ctx}");
                            cov.rename_refused += 1;
                        } else {
                            let got = r.unwrap_or_else(|e| panic!("{ctx} : {e}"));
                            assert_eq!(got.id, id, "{ctx}");
                            let mut e = e.clone();
                            e.name = norm(&name);
                            files.insert(id, MFile::Entry(e));
                            cov.renamed += 1;
                        }
                    }
                    Some(MFile::Junk) => {
                        if name_error(&name).is_some() {
                            assert!(r.is_err(), "{ctx}");
                        } else {
                            assert!(matches!(r, Err(LibraryError::Corrupt { .. })), "{ctx}");
                        }
                        cov.rename_refused += 1;
                    }
                    None => {
                        // Un nom invalide est refusé avant la recherche du fichier.
                        assert!(r.is_err(), "{ctx}");
                        cov.rename_refused += 1;
                    }
                }
            }
            // --- supprimer ---
            35..=42 => {
                let id = rng.below(next_num + 1);
                let r = lib.delete(id);
                match files.get(&id) {
                    Some(MFile::Entry(_)) => {
                        r.unwrap_or_else(|e| panic!("{ctx} : {e}"));
                        files.remove(&id);
                        cov.deleted += 1;
                    }
                    Some(MFile::Junk) => {
                        assert!(matches!(r, Err(LibraryError::Corrupt { .. })), "{ctx}");
                    }
                    None => assert_eq!(r, Err(LibraryError::NotFound { id }), "{ctx}"),
                }
            }
            // --- fichier abîmé déposé ---
            43..=50 => {
                let data: Vec<u8> = match rng.below(4) {
                    0 => b"{".to_vec(),
                    1 => vec![0xff, 0x00, 0x41],
                    2 => br#"{"format":"patternforge-library","version":1}"#.to_vec(),
                    _ => {
                        let good = files.iter().find(|(_, f)| matches!(f, MFile::Entry(_)));
                        match good {
                            Some((id, _)) => {
                                let b = std::fs::read(t.file(*id)).unwrap();
                                b[..b.len() / 3].to_vec()
                            }
                            None => b"[]".to_vec(),
                        }
                    }
                };
                std::fs::create_dir_all(t.path()).unwrap();
                std::fs::write(t.file(next_num), &data).unwrap();
                files.insert(next_num, MFile::Junk);
                cov.junk += 1;
            }
            // --- insérer ---
            51..=78 => {
                let entries: Vec<u64> = files
                    .iter()
                    .filter(|(_, f)| matches!(f, MFile::Entry(_)))
                    .map(|(k, _)| *k)
                    .collect();
                if entries.is_empty() {
                    continue;
                }
                let id = rng.pick(&entries);
                let MFile::Entry(me) = &files[&id] else {
                    unreachable!()
                };
                let entry = lib.load(id).unwrap_or_else(|e| panic!("{ctx} : {e}"));
                assert_eq!(
                    (entry.name.as_str(), entry.id),
                    (me.name.as_str(), id),
                    "{ctx}"
                );
                let rename = rng.chance(40);
                let (sheet, kind) = ((me.kind == Kind::Block).then_some(sheet0), me.kind);
                // Oracle : conflit = même nom (casse ignorée) mais autre nom exact, autre unité
                // ou autre valeur.
                let conflict = mt.vars.iter().any(|v| {
                    v.name.eq_ignore_ascii_case("L") && (v.name != "L" || !v.mm || v.value != me.l)
                });
                let before = bytes(doc.project());
                assert_eq!(before, now_bytes, "{ctx}");
                let r = doc.execute(Command::InsertLibrary {
                    content: Box::new(entry.content.clone()),
                    sheet,
                    layer: None,
                    rename_variables: rename,
                });
                if conflict && !rename {
                    assert!(r.is_err(), "{ctx} : conflit accepté");
                    assert_eq!(bytes(doc.project()), before, "{ctx} : trace d'un refus");
                    cov.conflict_refused += 1;
                } else {
                    let out = r.unwrap_or_else(|e| panic!("{ctx} : {e}"));
                    undo.push((mt.clone(), before));
                    redo.clear();
                    // Variable attendue.
                    let var_name = if conflict {
                        let n = (2..)
                            .map(|i| format!("L_{i}"))
                            .find(|n| !mt.vars.iter().any(|v| v.name.eq_ignore_ascii_case(n)))
                            .unwrap();
                        mt.vars.push(MVar {
                            name: n.clone(),
                            mm: true,
                            value: me.l,
                        });
                        cov.conflict_renamed += 1;
                        n
                    } else if mt.vars.iter().any(|v| v.name == "L") {
                        cov.reused += 1;
                        "L".to_string()
                    } else {
                        mt.vars.push(MVar {
                            name: "L".into(),
                            mm: true,
                            value: me.l,
                        });
                        cov.added += 1;
                        "L".to_string()
                    };
                    let p = doc.project();
                    match kind {
                        Kind::Piece | Kind::Model => {
                            mt.sheets += 1;
                            mt.pieces += 1;
                            let sid = out.sheet.unwrap();
                            let s = p.sheet(sid).unwrap();
                            assert_eq!(s.formulas.len(), 1, "{ctx}");
                            assert_eq!(s.points.len(), 7, "{ctx}");
                            assert_eq!(s.formulas[0].expr, var_name, "{ctx}");
                            mt.tracked.push(Tracked {
                                sheet: sid,
                                point: s.formulas[0].point,
                                var: var_name,
                                d: 0.0,
                            });
                        }
                        Kind::Block => {
                            let d = 5.0 * mt.blocks as f64;
                            if mt.blocks > 0 {
                                cov.shifted_blocks += 1;
                            }
                            mt.blocks += 1;
                            mt.points0 += 3;
                            let s = p.sheet(sheet0).unwrap();
                            assert_eq!(s.points.len(), mt.points0, "{ctx}");
                            let f = s.formulas.last().unwrap();
                            assert_eq!(f.expr, var_name, "{ctx}");
                            mt.tracked.push(Tracked {
                                sheet: sheet0,
                                point: f.point,
                                var: var_name,
                                d,
                            });
                        }
                    }
                    *cov.inserted.entry(kind_name(kind)).or_default() += 1;
                    now_bytes = bytes(doc.project());
                }
            }
            // --- ajouter une variable au projet ---
            79..=84 => {
                let name = rng.pick(&["L", "l", "L_2"]);
                let mm = rng.chance(75);
                let value = rng.pick(&[50_i64, 70, 90]);
                let before = bytes(doc.project());
                let expr = expr_for(value, &mut rng);
                let r = doc.execute(Command::AddVariable {
                    name: name.to_string(),
                    unit: if mm {
                        VariableUnit::Mm
                    } else {
                        VariableUnit::Deg
                    },
                    expr,
                });
                if mt.vars.iter().any(|v| v.name.eq_ignore_ascii_case(name)) {
                    assert!(r.is_err(), "{ctx}");
                    assert_eq!(bytes(doc.project()), before, "{ctx}");
                } else {
                    r.unwrap_or_else(|e| panic!("{ctx} : {e}"));
                    undo.push((mt.clone(), before));
                    redo.clear();
                    mt.vars.push(MVar {
                        name: name.into(),
                        mm,
                        value,
                    });
                    now_bytes = bytes(doc.project());
                }
            }
            // --- changer une variable du projet ---
            85..=91 => {
                if mt.vars.is_empty() {
                    continue;
                }
                let i = rng.below(mt.vars.len() as u64) as usize;
                let value = rng.pick(&[50_i64, 70, 90]);
                // Un conflit d'unité n'est pas modifié : seule la valeur change.
                let before = bytes(doc.project());
                let expr = expr_for(value, &mut rng);
                doc.execute(Command::SetVariableExpr {
                    name: mt.vars[i].name.clone(),
                    expr,
                })
                .unwrap_or_else(|e| panic!("{ctx} : {e}"));
                if before != bytes(doc.project()) {
                    undo.push((mt.clone(), before));
                    redo.clear();
                    now_bytes = bytes(doc.project());
                }
                mt.vars[i].value = value;
                cov.var_changed += 1;
            }
            // --- annuler / rétablir ---
            _ => {
                if rng.chance(60) {
                    let ok = doc.undo();
                    assert_eq!(ok, !undo.is_empty(), "{ctx}");
                    if let Some((m, b)) = undo.pop() {
                        redo.push((mt.clone(), now_bytes.clone()));
                        mt = m;
                        now_bytes = b;
                        assert_eq!(bytes(doc.project()), now_bytes, "{ctx} : annulation exacte");
                        cov.undone += 1;
                    }
                } else {
                    let ok = doc.redo();
                    assert_eq!(ok, !redo.is_empty(), "{ctx}");
                    if let Some((m, b)) = redo.pop() {
                        undo.push((mt.clone(), now_bytes.clone()));
                        mt = m;
                        now_bytes = b;
                        assert_eq!(bytes(doc.project()), now_bytes, "{ctx} : rétablissement");
                        cov.redone += 1;
                    }
                }
            }
        }

        // --- contrôles après chaque pas ---
        // Dossier : relu à la main, comparé au miroir.
        let (raw, bad) = raw_dir(t.path());
        let want: BTreeMap<u64, (String, String)> = files
            .iter()
            .filter_map(|(k, f)| match f {
                MFile::Entry(e) => Some((*k, (e.name.clone(), kind_name(e.kind).to_string()))),
                MFile::Junk => None,
            })
            .collect();
        let want_kinds: BTreeMap<u64, (String, String)> = want
            .iter()
            .map(|(k, (n, kind))| (*k, (n.clone(), kind.clone())))
            .collect();
        assert_eq!(raw, want_kinds, "{ctx} : dossier");
        let junk = files.values().filter(|f| matches!(f, MFile::Junk)).count();
        assert_eq!(bad, junk, "{ctx} : fichiers abîmés");
        let listing = lib.list().unwrap();
        assert_eq!(listing.entries.len(), want.len(), "{ctx}");
        assert_eq!(listing.problems.len(), junk, "{ctx}");
        cov.listings += 1;
        // Projet : variables, comptes, repères suivis.
        let p = doc.project();
        assert_eq!(p.sheets.len(), mt.sheets, "{ctx}");
        assert_eq!(p.pieces.len(), mt.pieces, "{ctx}");
        assert_eq!(p.sheet(sheet0).unwrap().points.len(), mt.points0, "{ctx}");
        assert_eq!(p.variables.len(), mt.vars.len(), "{ctx}");
        let values: BTreeMap<String, f64> = p.variable_values().unwrap().into_iter().collect();
        for v in &mt.vars {
            let pv = p.variables.iter().find(|x| x.name == v.name).unwrap();
            assert_eq!(
                pv.unit == VariableUnit::Mm,
                v.mm,
                "{ctx} : unité de {}",
                v.name
            );
            assert_eq!(
                values[&v.name], v.value as f64,
                "{ctx} : valeur de {}",
                v.name
            );
        }
        for tr in &mt.tracked {
            let v = mt.vars.iter().find(|v| v.name == tr.var).unwrap();
            let got = pos(p, tr.sheet, tr.point);
            assert_eq!(got, (v.value as f64 + tr.d, tr.d), "{ctx} : repère suivi");
            cov.checks_positions += 1;
        }
        p.validate().unwrap_or_else(|e| panic!("{ctx} : {e}"));
    }
}

#[test]
fn campagne_adverse_bibliotheque_160_sequences() {
    let mut cov = Cov::default();
    for seed in 1..=160 {
        run(seed, &mut cov);
    }
    // Compteurs : chaque contrôle fort s'est exécuté.
    for k in ["piece", "block", "model"] {
        assert!(cov.saved[k] >= 20, "enregistrements {k} : {:?}", cov.saved);
        assert!(cov.inserted[k] >= 20, "insertions {k} : {:?}", cov.inserted);
    }
    for k in ["vide", "controle", "long", "doublon"] {
        assert!(
            cov.name_refused[k] >= 8,
            "refus {k} : {:?}",
            cov.name_refused
        );
    }
    assert!(cov.renamed >= 20, "renommages {}", cov.renamed);
    assert!(
        cov.rename_refused >= 10,
        "renommages refusés {}",
        cov.rename_refused
    );
    assert!(cov.deleted >= 20, "suppressions {}", cov.deleted);
    assert!(cov.junk >= 50, "fichiers abîmés {}", cov.junk);
    assert!(
        cov.conflict_refused >= 30,
        "conflits refusés {}",
        cov.conflict_refused
    );
    assert!(
        cov.conflict_renamed >= 20,
        "conflits renommés {}",
        cov.conflict_renamed
    );
    assert!(cov.reused >= 30, "variables réutilisées {}", cov.reused);
    assert!(cov.added >= 30, "variables ajoutées {}", cov.added);
    assert!(
        cov.undone >= 20 && cov.redone >= 5,
        "{} {}",
        cov.undone,
        cov.redone
    );
    assert!(
        cov.var_changed >= 40,
        "variables modifiées {}",
        cov.var_changed
    );
    assert!(
        cov.shifted_blocks >= 10,
        "blocs décalés {}",
        cov.shifted_blocks
    );
    assert!(
        cov.checks_positions >= 500,
        "positions {}",
        cov.checks_positions
    );
    assert!(cov.listings >= 2000, "relectures {}", cov.listings);
    eprintln!(
        "adverse_bibliotheque : enreg. {:?}, insertions {:?}, refus nom {:?}, conflits refusés {} / renommés {}, \
         réutilisées {}, ajoutées {}, abîmés {}, positions {}",
        cov.saved,
        cov.inserted,
        cov.name_refused,
        cov.conflict_refused,
        cov.conflict_renamed,
        cov.reused,
        cov.added,
        cov.junk,
        cov.checks_positions
    );
}

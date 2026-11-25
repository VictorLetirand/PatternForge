//! US-17.3 : campagne adverse du verrou et de la détection de modification externe : 200
//! séquences à graine fixe de 5 à 24 actions sur de vrais fichiers temporaires, avec deux
//! « instances » (deux `Document`, deux `FileLock`).
//!
//! Oracle **indépendant** : l'état attendu du disque est un simple nom de projet (chaque écriture
//! a un nom unique ou rejoue un nom déjà vu) ; une instance « a vu » un nom ; l'enregistrement
//! protégé est attendu refusé exactement quand le disque n'a plus le nom vu (ou n'existe plus),
//! jamais d'après une empreinte ou une date. Pour les verrous : un seul propriétaire attendu,
//! périmé à 45 s (valeur écrite ici, pas lue dans le code testé).
//!
//! Propriétés : un refus laisse les octets du disque identiques et ne crée aucun temporaire ; un
//! accord écrit le projet de l'instance ; la révision suit chaque écriture ; compteurs de
//! couverture assertés.

use pf_core::file_guard::{self, FileLock, LockError};
use pf_core::{persist, Command, Document, ExternalChange, Project, SaveError};
use std::path::{Path, PathBuf};

const SEQUENCES: usize = 200;

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

#[derive(Default, Debug)]
struct Counters {
    sequences: usize,
    accepted: usize,
    refused_modified: usize,
    refused_removed: usize,
    reverted_unchanged: usize,
    identical_rewrites: usize,
    forced: usize,
    lock_ok: usize,
    lock_held: usize,
    lock_stale_taken: usize,
    lock_stolen_drop_kept: usize,
}

fn disk_name(path: &Path) -> Option<String> {
    persist::load(path).ok().map(|p| p.name)
}

fn dir_entries(dir: &Path) -> Vec<String> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

struct Inst {
    doc: Document,
    seen: String,
    lock: Option<FileLock>,
}

fn run_sequence(seed: u64, c: &mut Counters) {
    let mut rng = Rng(seed);
    let dir: PathBuf =
        std::env::temp_dir().join(format!("pf-adverse-verrou-{}-{seed}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("p.pforge.json");
    let mut serial = 0u32;
    let mut fresh = || {
        serial += 1;
        format!("v{serial}")
    };
    let start = fresh();
    persist::save(&path, &Project::new(&start)).unwrap();
    let mut disk: Option<String> = Some(start.clone());
    let mut inst: Vec<Inst> = (0..2)
        .map(|_| Inst {
            doc: Document::open(&path).unwrap(),
            seen: start.clone(),
            lock: None,
        })
        .collect();
    // Verrou attendu : (propriétaire, heure) ; fichier de verrou attendu présent ou non.
    let mut owner: Option<(usize, u64)> = None;
    let mut now = 10_000u64;
    let steps = 5 + rng.below(20);
    for _ in 0..steps {
        let who = rng.below(2);
        match rng.below(16) {
            0 | 1 => {
                let name = fresh();
                inst[who]
                    .doc
                    .execute(Command::RenameProject { name: name.clone() })
                    .unwrap();
            }
            2..=4 => {
                let before = std::fs::read(&path).ok();
                let entries_before = dir_entries(&dir);
                let mine = inst[who].doc.project().name.clone();
                let result = inst[who].doc.save_checked(&path);
                let expected = match &disk {
                    None => Some(ExternalChange::Removed),
                    Some(d) if *d != inst[who].seen => Some(ExternalChange::Modified),
                    Some(_) => None,
                };
                match expected {
                    None => {
                        assert_eq!(result, Ok(()), "seed {seed}");
                        disk = Some(mine.clone());
                        inst[who].seen = mine.clone();
                        assert_eq!(disk_name(&path), disk, "seed {seed}");
                        c.accepted += 1;
                    }
                    Some(kind) => {
                        assert_eq!(result, Err(SaveError::External(kind)), "seed {seed}");
                        assert_eq!(std::fs::read(&path).ok(), before, "seed {seed}");
                        assert_eq!(dir_entries(&dir), entries_before, "seed {seed}");
                        match kind {
                            ExternalChange::Removed => c.refused_removed += 1,
                            _ => c.refused_modified += 1,
                        }
                    }
                }
            }
            5 => {
                // Écriture externe d'un contenu nouveau.
                let name = fresh();
                persist::save(&path, &Project::new(&name)).unwrap();
                disk = Some(name);
            }
            6 => {
                // Retour à ce que l'instance a vu : plus aucun changement attendu.
                let seen = inst[who].seen.clone();
                persist::save(&path, &Project::new(&seen)).unwrap();
                disk = Some(seen);
                assert_eq!(
                    inst[who].doc.external_change().unwrap(),
                    ExternalChange::Unchanged,
                    "seed {seed}"
                );
                c.reverted_unchanged += 1;
            }
            7 => {
                if let Ok(bytes) = std::fs::read(&path) {
                    std::fs::write(&path, bytes).unwrap();
                    c.identical_rewrites += 1;
                }
            }
            8 => {
                if rng.below(2) == 0 {
                    let _ = std::fs::remove_file(&path);
                    disk = None;
                } else {
                    let mine = inst[who].doc.project().name.clone();
                    inst[who].doc.save_as(&path).unwrap();
                    disk = Some(mine.clone());
                    inst[who].seen = mine;
                    c.forced += 1;
                }
            }
            _ => {
                // Verrou : acquisition, rafraîchissement ou libération.
                now += rng.below(40) as u64;
                match rng.below(3) {
                    0 => {
                        let result = FileLock::acquire(&path, now);
                        let held = owner.filter(|&(_, since)| now - since < 45);
                        match (held, result) {
                            (None, Ok(lock)) => {
                                if owner.is_some() {
                                    c.lock_stale_taken += 1;
                                }
                                inst[who].lock = Some(lock);
                                owner = Some((who, now));
                                c.lock_ok += 1;
                            }
                            (Some((_, since)), Err(LockError::Held(h))) => {
                                assert_eq!(
                                    (h.pid, h.since),
                                    (std::process::id(), since),
                                    "seed {seed}"
                                );
                                c.lock_held += 1;
                            }
                            (e, r) => {
                                panic!("seed {seed} : attendu {e:?}, obtenu {:?}", r.map(|_| ()))
                            }
                        }
                    }
                    1 => {
                        if let Some(l) = &inst[who].lock {
                            l.refresh(now).unwrap();
                            // Fichier absent : le rafraîchissement le recrée ; sinon seul
                            // le propriétaire change l'heure.
                            if owner.is_none() || owner.map(|o| o.0) == Some(who) {
                                owner = Some((who, now));
                            }
                        }
                    }
                    _ => {
                        if inst[who].lock.take().is_some() {
                            if owner.map(|o| o.0) == Some(who) {
                                owner = None;
                            } else {
                                c.lock_stolen_drop_kept += 1;
                            }
                        }
                    }
                }
                assert_eq!(
                    file_guard::lock_path(&path).exists(),
                    owner.is_some(),
                    "seed {seed} : présence du fichier de verrou"
                );
            }
        }
    }
    drop(inst);
    assert!(
        !file_guard::lock_path(&path).exists() || owner.is_some(),
        "seed {seed}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    c.sequences += 1;
}

#[test]
fn campagne_adverse_verrou_et_modification_externe() {
    let mut c = Counters::default();
    for i in 0..SEQUENCES {
        run_sequence(0xA11C_E000 + i as u64, &mut c);
    }
    eprintln!("{c:?}");
    assert_eq!(c.sequences, SEQUENCES);
    assert!(c.accepted >= 100, "{c:?}");
    assert!(c.refused_modified >= 60, "{c:?}");
    assert!(c.refused_removed >= 20, "{c:?}");
    assert!(c.reverted_unchanged >= 40, "{c:?}");
    assert!(c.identical_rewrites >= 40, "{c:?}");
    assert!(c.forced >= 30, "{c:?}");
    assert!(c.lock_ok >= 50 && c.lock_held >= 30, "{c:?}");
    assert!(c.lock_stale_taken >= 5, "{c:?}");
    assert!(c.lock_stolen_drop_kept >= 3, "{c:?}");
}

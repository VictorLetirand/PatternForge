//! US-17.2 - campagne adverse des versions : 160 séquences à graine fixe créer / modifier /
//! comparer / restaurer / supprimer, avec corruption du dossier (index absent, illisible ou faux,
//! fichier tronqué ou supprimé). **Oracle indépendant** : un modèle en mémoire (fichiers attendus,
//! états = nombre de points, validité des noms par une liste écrite ici) ; il ne lit jamais les
//! constantes ni les fonctions du module testé. Invariants : aucun panic, **jamais de perte** (tout
//! fichier de version non supprimé par l'action reste présent, octet pour octet), liste conforme
//! au modèle, index reconstruit, restauration annulable en un pas.

use pf_core::version_diff::diff_projects;
use pf_core::versions::*;
use pf_core::*;
use pf_geometry::PointMm;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// État du projet = k points alignés ; les états forment une lignée (préfixes).
fn state(k: u32) -> Project {
    let mut p = Project::new("Adverse");
    let s = p.sheets[0].id;
    for i in 0..k {
        p.apply(Command::AddPoint {
            sheet: s,
            at: PointMm::new(f64::from(i) * 7.0, 3.0),
        })
        .unwrap();
    }
    p
}

/// Oracle de nom : écrit ici, indépendamment du module testé.
fn name_ok(raw: &str) -> bool {
    let t = raw.trim();
    if t.is_empty() || t.chars().count() > 60 || t.ends_with('.') {
        return false;
    }
    if t.chars()
        .any(|c| "<>:\"/\\|?*".contains(c) || (c as u32) < 32)
    {
        return false;
    }
    let base = t.split('.').next().unwrap().to_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&base.as_str())
        || (base.len() == 4
            && (base.starts_with("COM") || base.starts_with("LPT"))
            && base.as_bytes()[3].is_ascii_digit()
            && base.as_bytes()[3] != b'0');
    !reserved
}

const NAMES: &[&str] = &[
    "v1",
    "Avant jupe",
    "  espace  ",
    "",
    "a/b",
    "CON",
    "nul.txt",
    "x?",
    "fin.",
    "Épaule été",
    "COM0",
    "LPT3",
    "a:b",
    "dix",
    "avec  doubles   espaces",
];

#[derive(Clone)]
struct Known {
    points: u32,
    bytes: Vec<u8>,
    truncated: bool,
}

#[derive(Default)]
struct Cover {
    created: u32,
    refused_names: u32,
    truncated: u32,
    index_removed: u32,
    index_garbage: u32,
    index_wrong: u32,
    file_removed: u32,
    restored: u32,
    deleted: u32,
    compared: u32,
    rebuilt: u32,
    damaged_seen: u32,
    refused_damaged: u32,
}

fn dir_for(seq: usize) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target")
        .join(format!("us172-adverse/s{seq:03}"));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d.canonicalize().unwrap()
}

fn check_listing(pp: &Path, model: &BTreeMap<String, Known>, cover: &mut Cover) {
    let l = list(pp).expect("liste");
    if l.index_rebuilt {
        cover.rebuilt += 1;
    }
    let files: Vec<&str> = l.versions.iter().map(|v| v.file.as_str()).collect();
    let expected: Vec<&str> = model.keys().map(String::as_str).collect();
    assert_eq!(
        files, expected,
        "liste = fichiers attendus, ordre chronologique"
    );
    for v in &l.versions {
        let k = &model[&v.file];
        assert_eq!(v.damaged, k.truncated, "{}", v.file);
        if v.damaged {
            cover.damaged_seen += 1;
        }
        // Jamais de perte : octets intacts (ceux de l'oracle).
        assert_eq!(fs::read(versions_dir(pp).join(&v.file)).unwrap(), k.bytes);
    }
    // Idempotence : une seconde lecture ne reconstruit rien.
    assert!(
        !list(pp).unwrap().index_rebuilt,
        "index stable après lecture"
    );
}

fn run(seq: usize, rng: &mut Rng, cover: &mut Cover) {
    let dir = dir_for(seq);
    let pp = dir.join("p.pforge.json");
    let vdir = versions_dir(&pp);
    let mut points = rng.below(4) as u32;
    let mut doc = Document::new(state(points));
    let mut model: BTreeMap<String, Known> = BTreeMap::new();
    let mut now = 1_700_000_000 + seq as u64 * 10_000;
    for _ in 0..(12 + rng.below(12)) {
        now += 1 + rng.below(3);
        match rng.below(13) {
            0..=2 => {
                let raw = NAMES[rng.below(NAMES.len() as u64) as usize];
                let r = create(&pp, doc.project(), raw, now);
                if name_ok(raw) {
                    let v = r.unwrap_or_else(|e| panic!("création {raw:?} : {e}"));
                    let bytes = fs::read(vdir.join(&v.file)).unwrap();
                    assert!(model
                        .insert(
                            v.file.clone(),
                            Known {
                                points,
                                bytes,
                                truncated: false
                            }
                        )
                        .is_none());
                    cover.created += 1;
                } else {
                    assert!(matches!(r, Err(VersionError::Name(_))), "{raw:?}");
                    cover.refused_names += 1;
                }
            }
            3 => {
                let s = doc.project().sheets[0].id;
                doc.execute(Command::AddPoint {
                    sheet: s,
                    at: PointMm::new(f64::from(points) * 7.0, 3.0),
                })
                .unwrap();
                points += 1;
            }
            4 | 10 | 11 => {
                // Corruption : l'une des cinq.
                match rng.below(5) {
                    0 => {
                        let healthy: Vec<String> = model
                            .iter()
                            .filter(|(_, k)| !k.truncated)
                            .map(|(f, _)| f.clone())
                            .collect();
                        if !healthy.is_empty() {
                            let f = &healthy[rng.below(healthy.len() as u64) as usize];
                            let k = model.get_mut(f).unwrap();
                            let cut = k.bytes.len() / 2;
                            k.bytes.truncate(cut);
                            k.truncated = true;
                            fs::write(vdir.join(f), &k.bytes).unwrap();
                            cover.truncated += 1;
                        }
                    }
                    1 => {
                        let _ = fs::remove_file(vdir.join("index.json"));
                        cover.index_removed += 1;
                    }
                    2 => {
                        if vdir.exists() {
                            fs::write(vdir.join("index.json"), "{ pas du json ").unwrap();
                            cover.index_garbage += 1;
                        }
                    }
                    3 => {
                        if vdir.exists() {
                            fs::write(
                                vdir.join("index.json"),
                                r#"{"format":"patternforge-versions","version":1,"entries":[{"file":"20000101T000000Z-x.pforge.json","name":"x","created":"c","revision":"sha256:0","size":9}]}"#,
                            )
                            .unwrap();
                            cover.index_wrong += 1;
                        }
                    }
                    _ => {
                        if let Some(f) = model.keys().next().cloned() {
                            fs::remove_file(vdir.join(&f)).unwrap();
                            model.remove(&f);
                            cover.file_removed += 1;
                        }
                    }
                }
            }
            5 => check_listing(&pp, &model, cover),
            6 => {
                // Comparaison de deux versions saines : oracle = différence des nombres de points.
                let healthy: Vec<(&String, &Known)> =
                    model.iter().filter(|(_, k)| !k.truncated).collect();
                if healthy.len() >= 2 {
                    let a = healthy[rng.below(healthy.len() as u64) as usize];
                    let b = healthy[rng.below(healthy.len() as u64) as usize];
                    let pa = read(&pp, a.0).unwrap();
                    let pb = read(&pp, b.0).unwrap();
                    let d = diff_projects(&pa, &pb);
                    let (added, removed) = (
                        b.1.points.saturating_sub(a.1.points) as usize,
                        a.1.points.saturating_sub(b.1.points) as usize,
                    );
                    let (da, dr, dm) = d.sheets.first().map_or((0, 0, 0), |s| {
                        (
                            s.points_added.len(),
                            s.points_removed.len(),
                            s.points_moved.len(),
                        )
                    });
                    assert_eq!((da, dr, dm), (added, removed, 0));
                    cover.compared += 1;
                }
                if let Some((f, _)) = model.iter().find(|(_, k)| k.truncated) {
                    assert!(matches!(read(&pp, f), Err(VersionError::Damaged(_))));
                    cover.refused_damaged += 1;
                }
            }
            7 => {
                // Restauration d'une version saine, comme le fait l'application.
                let healthy: Vec<(&String, &Known)> =
                    model.iter().filter(|(_, k)| !k.truncated).collect();
                if !healthy.is_empty() {
                    let (f, k) = healthy[rng.below(healthy.len() as u64) as usize];
                    let (f, k) = (f.clone(), k.clone());
                    let before = doc.project().clone();
                    let target = read(&pp, &f).unwrap();
                    let snap = create(&pp, &before, "avant restauration", now).unwrap();
                    model.insert(
                        snap.file.clone(),
                        Known {
                            points,
                            bytes: fs::read(vdir.join(&snap.file)).unwrap(),
                            truncated: false,
                        },
                    );
                    let can_undo_before = doc.can_undo();
                    doc.restore_project(target).unwrap();
                    assert_eq!(doc.project().sheets[0].points.len() as u32, k.points);
                    if k.points != points {
                        assert!(doc.undo(), "un pas d'annulation");
                        assert_eq!(doc.project(), &before, "un seul undo suffit");
                        assert!(doc.redo());
                    }
                    let _ = can_undo_before;
                    points = k.points;
                    cover.restored += 1;
                }
            }
            8 => {
                // Suppression d'une version (saine ou abîmée).
                if !model.is_empty() {
                    let n = rng.below(model.len() as u64) as usize;
                    let f = model.keys().nth(n).unwrap().clone();
                    delete(&pp, &f, now).unwrap();
                    model.remove(&f);
                    assert!(!vdir.join(&f).exists());
                    cover.deleted += 1;
                }
            }
            _ => {
                // Suppression d'un nom inconnu ou hors dossier : refusée, rien ne bouge.
                for bad in ["../p.pforge.json", "index.json", "zz.pforge.json"] {
                    assert!(matches!(
                        delete(&pp, bad, now),
                        Err(VersionError::Unknown(_))
                    ));
                }
            }
        }
        // Après chaque pas : aucun fichier attendu n'a disparu ni changé.
        for (f, k) in &model {
            assert_eq!(&fs::read(vdir.join(f)).unwrap(), &k.bytes, "perte de {f}");
        }
    }
    check_listing(&pp, &model, cover);
}

#[test]
fn campagne_adverse_versions_160_sequences() {
    let mut rng = Rng(0x1720_0001);
    let mut cover = Cover::default();
    for seq in 0..160 {
        run(seq, &mut rng, &mut cover);
    }
    eprintln!("couverture : cree {} refus {} tronque {} index-retire {} index-illisible {} index-faux {} fichier-retire {} restaure {} supprime {} compare {} reconstruit {} abimees {} lectures-refusees {}", cover.created, cover.refused_names, cover.truncated, cover.index_removed, cover.index_garbage, cover.index_wrong, cover.file_removed, cover.restored, cover.deleted, cover.compared, cover.rebuilt, cover.damaged_seen, cover.refused_damaged);
    // Compteurs de couverture : chaque famille d'événements a réellement eu lieu.
    assert!(cover.created >= 150, "créations {}", cover.created);
    assert!(
        cover.refused_names >= 100,
        "noms refusés {}",
        cover.refused_names
    );
    assert!(cover.truncated >= 20, "tronqués {}", cover.truncated);
    assert!(
        cover.index_removed >= 20,
        "index retirés {}",
        cover.index_removed
    );
    assert!(
        cover.index_garbage >= 15,
        "index illisibles {}",
        cover.index_garbage
    );
    assert!(cover.index_wrong >= 15, "index faux {}", cover.index_wrong);
    assert!(
        cover.file_removed >= 10,
        "fichiers retirés {}",
        cover.file_removed
    );
    assert!(cover.restored >= 50, "restaurations {}", cover.restored);
    assert!(cover.deleted >= 50, "suppressions {}", cover.deleted);
    assert!(cover.compared >= 30, "comparaisons {}", cover.compared);
    assert!(cover.rebuilt >= 20, "index reconstruits {}", cover.rebuilt);
    assert!(
        cover.damaged_seen >= 20,
        "abîmées vues {}",
        cover.damaged_seen
    );
    assert!(
        cover.refused_damaged >= 10,
        "lectures refusées {}",
        cover.refused_damaged
    );
}

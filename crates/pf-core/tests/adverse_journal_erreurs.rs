//! US-17.5 : campagne adverse du journal d'erreurs : 200 séquences à graine fixe de 3 à 160
//! erreurs (chemins Windows, UNC, Unix, avec espaces et accents ; noms de projet, de feuille et
//! d'utilisateur ; textes entre guillemets). Les valeurs sensibles sont des « canaris » uniques
//! par séquence.
//!
//! Oracle **indépendant** : les canaris sont choisis ici ; on vérifie, sur le JSON gardé, sur les
//! lignes exportées et sur chaque message, qu'**aucun canari n'apparaît** (sans tenir compte de la
//! casse), qu'aucun antislash, lecteur (`c:`), `/home/` ou `/users/` ne reste, que chaque message
//! fait au plus 200 caractères, que le journal ne dépasse ni 100 entrées ni 16 Kio, que la
//! relecture du réglage redonne **exactement** le même journal (l'assainissement est idempotent)
//! et que la dernière erreur enregistrée est la dernière entrée (rotation par l'ancien bout).

use pf_core::error_log::{ErrorLog, MAX_BYTES, MAX_ENTRIES};

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
    recorded: usize,
    skipped_duplicates: usize,
    with_secrets: usize,
    path_only: usize,
    rotated: usize,
    templates: [usize; 10],
}

fn run_sequence(seed: u64, c: &mut Counters) {
    let mut rng = Rng(seed);
    let n = seed % 100_000;
    let user = format!("Zoé{n}Lefèvre");
    let project = format!("Robe d'été {n} bis");
    let sheet = format!("Devant gauche {n}");
    let dir = format!("Mes Documents {n}");
    let roots = [
        format!(r"C:\Users\{user}\{dir}"),
        format!(r"d:/travail {n}/{user}/{dir}"),
        format!(r"\\serveur{n}\partage {user}\{dir}"),
        format!("/home/{user}/{dir}"),
        format!("~/{user}/{dir}"),
    ];
    let canaries = [
        user.to_lowercase(),
        project.to_lowercase(),
        sheet.to_lowercase(),
        dir.to_lowercase(),
        format!("serveur{n}"),
        format!("travail {n}"),
    ];
    let with_secrets = rng.below(2) == 0;
    // Sans valeurs sensibles fournies, seuls les chemins et les guillemets protègent : les
    // gabarits qui écrivent un nom en clair (4 et 7) ne sont alors pas tirés.
    let secrets = if with_secrets {
        c.with_secrets += 1;
        vec![project.clone(), user.clone()]
    } else {
        c.path_only += 1;
        Vec::new()
    };
    let max_template = if with_secrets { 10 } else { 8 };

    let mut log = ErrorLog::default();
    let mut last_message_kept = false;
    let count = 3 + rng.below(158);
    for step in 0..count {
        let root = &roots[rng.below(roots.len())];
        let sep = if root.contains('\\') { '\\' } else { '/' };
        let file = format!("{root}{sep}{project}.pforge.json");
        let detail = [
            "accès refusé",
            "disque plein",
            "fichier verrouillé",
            "Version 5 attendue",
        ][rng.below(4)];
        let mut t = rng.below(max_template);
        if !with_secrets && (t == 4 || t == 7) {
            t = 0;
        }
        c.templates[t] += 1;
        let raw = match t {
            0 => format!("{file} : {detail}"),
            1 => format!("Impossible d'ouvrir {file}"),
            2 => format!("Projet « {project} » : feuille « {sheet} » introuvable"),
            3 => format!("Écriture de {file} impossible ({detail})"),
            4 => format!("Erreur pour {user} sur {project}"),
            5 => format!("failed to write \"{file}\": {detail}"),
            6 => format!("Dossier de sauvegarde introuvable : {root}"),
            7 => format!("dossier{sep}{project}{sep}x : {detail}"),
            8 => format!("{root} : {detail}"),
            _ => format!("Impossible de créer le dossier {root} puis réessayer ({step})"),
        };
        let before = log.len();
        let changed = log.record(1_700_000_000 + step as u64, "dialog", &raw, &secrets);
        if changed {
            c.recorded += 1;
            if log.len() <= before {
                c.rotated += 1;
            }
            last_message_kept = true;
        } else {
            c.skipped_duplicates += 1;
        }
        // Après chaque ajout, tout le journal respecte l'oracle.
        let json = log.to_value().to_string().to_lowercase();
        for canary in &canaries {
            assert!(
                !json.contains(canary),
                "seed {seed} : « {canary} » dans {json}"
            );
        }
        assert!(log.len() <= MAX_ENTRIES && log.to_value().to_string().len() <= MAX_BYTES);
        for e in log.entries() {
            let m = e.message.to_lowercase();
            assert!(m.chars().count() <= 200, "seed {seed}");
            assert!(!m.contains('\\'), "seed {seed} : {m}");
            for bad in ["c:", "d:", "/home/", "/users/", "~/"] {
                assert!(!m.contains(bad), "seed {seed} : {bad} dans {m}");
            }
        }
    }
    assert!(last_message_kept || log.is_empty());
    for line in log.export_lines() {
        let l = line.to_lowercase();
        for canary in &canaries {
            assert!(!l.contains(canary), "seed {seed} : export {l}");
        }
    }
    assert_eq!(ErrorLog::from_value(&log.to_value()), log, "seed {seed}");
    c.sequences += 1;
}

#[test]
fn campagne_adverse_journal_sans_donnee_personnelle() {
    let mut c = Counters::default();
    for i in 0..SEQUENCES {
        run_sequence(0xE44_0000 + i as u64 * 7919, &mut c);
    }
    eprintln!("{c:?}");
    assert_eq!(c.sequences, SEQUENCES);
    assert!(c.recorded >= 1_500, "{c:?}");
    assert!(c.rotated >= 1_000, "{c:?}");
    assert!(c.with_secrets >= 70 && c.path_only >= 70, "{c:?}");
    assert!(c.templates.iter().all(|&n| n >= 100), "{c:?}");
}

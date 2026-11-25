//! US-09.1 — campagne adverse « tailles » : 160 séquences à graine fixe de 60 pas (création,
//! renommage et suppression de gammes ; ajout, retrait, renommage, déplacement de tailles ;
//! rupture ; changement de base ; quelques commandes d'autres domaines ; annuler, rétablir,
//! écrire-relire), avec libellés et noms valides, vides, en double (casse différente), trop
//! longs, à caractère de contrôle, identifiants inconnus ou appartenant à un autre objet.
//!
//! **Oracle indépendant** : le modèle des gammes est réécrit ici (`Model`, simples vecteurs) avec
//! ses propres règles de validation et d'allocation d'identifiants. Pour chaque commande, l'oracle
//! prédit accepté / refusé et le nouvel état ; le code testé doit être d'accord, gamme par gamme,
//! taille par taille (rang, libellé, rupture, base, identifiant).
//!
//! Invariants vérifiés après chaque pas : projet valide ; refus = octets identiques et aucune
//! étape d'historique ; étape d'historique si et seulement si l'état change ; annuler / rétablir
//! exacts à l'octet ; relecture identique ; une seule base par gamme et appartenant à la gamme ;
//! identifiants uniques ; ordre conservé ; une commande de tailles ne touche aucun autre objet ;
//! une commande d'un autre domaine ne touche aucune gamme ; `evaluate_sheet(None)` = géométrie de
//! base = `evaluate_sheet(Some(t))` pour toute taille t du projet.
//!
//! Mutations vérifiées à la main : voir `docs/JOURNAL.md` (tranche 94).

use pf_core::persist;
use pf_core::*;
use pf_geometry::PointMm;

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
// Oracle : le modèle des gammes, réécrit
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
struct OSize {
    id: u64,
    label: String,
    brk: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct ORange {
    id: u64,
    name: String,
    numeric: bool,
    sizes: Vec<OSize>,
    base: u64,
}

#[derive(Clone, Debug, PartialEq)]
struct Model {
    ranges: Vec<ORange>,
    next: u64,
}

fn text_ok(s: &str, max: usize) -> bool {
    !s.trim().is_empty() && !s.chars().any(|c| c.is_control()) && s.chars().count() <= max
}

fn ci_eq(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

impl Model {
    fn locate(&self, size: u64) -> Option<(usize, usize)> {
        for (ri, r) in self.ranges.iter().enumerate() {
            if let Some(si) = r.sizes.iter().position(|s| s.id == size) {
                return Some((ri, si));
            }
        }
        None
    }

    fn range_idx(&self, id: u64) -> Option<usize> {
        self.ranges.iter().position(|r| r.id == id)
    }

    /// Prédit l'effet d'une commande : `None` = refus, `Some(nouvel état)` sinon (peut être égal).
    fn predict(&self, cmd: &Command) -> Option<Model> {
        let mut m = self.clone();
        match cmd {
            Command::AddSizeRange {
                name,
                kind,
                labels,
                base,
            } => {
                let name = name.trim().to_string();
                if m.ranges.len() >= 20
                    || !text_ok(&name, 120)
                    || m.ranges.iter().any(|r| ci_eq(&r.name, &name))
                    || labels.is_empty()
                    || labels.len() > 60
                    || *base >= labels.len()
                {
                    return None;
                }
                let labels: Vec<String> = labels.iter().map(|l| l.trim().to_string()).collect();
                for (i, l) in labels.iter().enumerate() {
                    if !text_ok(l, 20) || labels[..i].iter().any(|o| ci_eq(o, l)) {
                        return None;
                    }
                }
                let id = m.next;
                m.next += 1;
                let mut sizes = Vec::new();
                for l in labels {
                    sizes.push(OSize {
                        id: m.next,
                        label: l,
                        brk: false,
                    });
                    m.next += 1;
                }
                let base = sizes[*base].id;
                m.ranges.push(ORange {
                    id,
                    name,
                    numeric: *kind == SizeKind::Numeric,
                    sizes,
                    base,
                });
            }
            Command::RenameSizeRange { range, name } => {
                let i = m.range_idx(range.0)?;
                let name = name.trim().to_string();
                if !text_ok(&name, 120)
                    || m.ranges
                        .iter()
                        .enumerate()
                        .any(|(k, r)| k != i && ci_eq(&r.name, &name))
                {
                    return None;
                }
                m.ranges[i].name = name;
            }
            Command::RemoveSizeRange { range } => {
                let i = m.range_idx(range.0)?;
                m.ranges.remove(i);
            }
            Command::AddSize { range, label, at } => {
                let i = m.range_idx(range.0)?;
                let label = label.trim().to_string();
                let r = &mut m.ranges[i];
                if r.sizes.len() >= 60
                    || !text_ok(&label, 20)
                    || r.sizes.iter().any(|s| ci_eq(&s.label, &label))
                {
                    return None;
                }
                let at = at.unwrap_or(r.sizes.len());
                if at > r.sizes.len() {
                    return None;
                }
                r.sizes.insert(
                    at,
                    OSize {
                        id: m.next,
                        label,
                        brk: false,
                    },
                );
                m.next += 1;
            }
            Command::RemoveSize { size } => {
                let (ri, si) = m.locate(size.0)?;
                let r = &mut m.ranges[ri];
                if r.sizes.len() == 1 || r.base == size.0 {
                    return None;
                }
                r.sizes.remove(si);
            }
            Command::RenameSize { size, label } => {
                let (ri, si) = m.locate(size.0)?;
                let label = label.trim().to_string();
                let r = &mut m.ranges[ri];
                if !text_ok(&label, 20)
                    || r.sizes
                        .iter()
                        .enumerate()
                        .any(|(k, s)| k != si && ci_eq(&s.label, &label))
                {
                    return None;
                }
                r.sizes[si].label = label;
            }
            Command::MoveSize { size, to } => {
                let (ri, si) = m.locate(size.0)?;
                let r = &mut m.ranges[ri];
                if *to >= r.sizes.len() {
                    return None;
                }
                let s = r.sizes.remove(si);
                r.sizes.insert(*to, s);
            }
            Command::SetSizeBreak { size, break_size } => {
                let (ri, si) = m.locate(size.0)?;
                m.ranges[ri].sizes[si].brk = *break_size;
            }
            Command::SetBaseSize { size } => {
                let (ri, _) = m.locate(size.0)?;
                m.ranges[ri].base = size.0;
            }
            _ => unreachable!("commande étrangère aux tailles"),
        }
        Some(m)
    }
}

fn model_of(p: &Project) -> Model {
    Model {
        ranges: p
            .sizes
            .iter()
            .map(|r| ORange {
                id: r.id.0,
                name: r.name.clone(),
                numeric: r.kind == SizeKind::Numeric,
                sizes: r
                    .sizes
                    .iter()
                    .map(|s| OSize {
                        id: s.id.0,
                        label: s.label.clone(),
                        brk: s.break_size,
                    })
                    .collect(),
                base: r.base.0,
            })
            .collect(),
        next: p.next_id,
    }
}

// ---------------------------------------------------------------------------------------------
// Générateurs
// ---------------------------------------------------------------------------------------------

const NAMES: [&str; 10] = [
    "Femme", "Homme", "Stature", "Tour", "femme", "FEMME", " Enfant ", "", "  ", "Ado",
];
const LABELS: [&str; 18] = [
    "XS", "S", "M", "L", "XL", "2XL", "34", "36", "38", "40", "xs", "m", " M ", "", "  ", "a\tb",
    "S ", "10",
];

fn gen_label(rng: &mut Rng) -> String {
    if rng.chance(4) {
        "x".repeat(21 + rng.below(3) as usize)
    } else if rng.chance(15) {
        format!("T{}", rng.below(40))
    } else {
        rng.pick(&LABELS).to_string()
    }
}

fn gen_name(rng: &mut Rng) -> String {
    if rng.chance(3) {
        "n".repeat(121)
    } else if rng.chance(20) {
        format!("Gamme {}", rng.below(30))
    } else {
        rng.pick(&NAMES).to_string()
    }
}

fn ghost(rng: &mut Rng, p: &Project) -> u64 {
    match rng.below(3) {
        0 => 987_654,
        1 => p.sheets[0].id.0,
        _ => p.sheets[0].points.first().map_or(1, |pt| pt.id.0),
    }
}

fn gen_size_id(rng: &mut Rng, m: &Model, p: &Project) -> SizeId {
    let all: Vec<u64> = m
        .ranges
        .iter()
        .flat_map(|r| r.sizes.iter().map(|s| s.id))
        .collect();
    if all.is_empty() || rng.chance(7) {
        SizeId(ghost(rng, p))
    } else {
        SizeId(*rng.pick(&all))
    }
}

fn gen_range_id(rng: &mut Rng, m: &Model, p: &Project) -> SizeRangeId {
    if m.ranges.is_empty() || rng.chance(7) {
        SizeRangeId(ghost(rng, p))
    } else {
        SizeRangeId(rng.pick(&m.ranges).id)
    }
}

fn gen_size_command(rng: &mut Rng, m: &Model, p: &Project) -> Command {
    match rng.below(20) {
        0..=3 => {
            let n = if rng.chance(5) {
                0
            } else if rng.chance(3) {
                61
            } else {
                1 + rng.below(6) as usize
            };
            let labels: Vec<String> = (0..n).map(|_| gen_label(rng)).collect();
            // Souvent des libellés propres : une gamme valide de temps en temps.
            let labels = if rng.chance(60) {
                (0..n.min(8)).map(|i| format!("{}", 30 + 2 * i)).collect()
            } else {
                labels
            };
            let base = if rng.chance(8) {
                labels.len() + rng.below(2) as usize
            } else {
                rng.below(labels.len().max(1) as u64) as usize
            };
            Command::AddSizeRange {
                name: gen_name(rng),
                kind: if rng.chance(50) {
                    SizeKind::Numeric
                } else {
                    SizeKind::Alphanumeric
                },
                labels,
                base,
            }
        }
        4 => Command::RenameSizeRange {
            range: gen_range_id(rng, m, p),
            name: gen_name(rng),
        },
        5 => Command::RemoveSizeRange {
            range: gen_range_id(rng, m, p),
        },
        6..=8 => {
            let range = gen_range_id(rng, m, p);
            let len = m.range_idx(range.0).map_or(0, |i| m.ranges[i].sizes.len());
            let at = if rng.chance(40) {
                None
            } else {
                Some(rng.below(len as u64 + 2) as usize)
            };
            Command::AddSize {
                range,
                label: gen_label(rng),
                at,
            }
        }
        9..=10 => Command::RemoveSize {
            size: gen_size_id(rng, m, p),
        },
        11 => Command::RenameSize {
            size: gen_size_id(rng, m, p),
            label: gen_label(rng),
        },
        12..=13 => {
            let size = gen_size_id(rng, m, p);
            let len = m
                .locate(size.0)
                .map_or(3, |(ri, _)| m.ranges[ri].sizes.len());
            Command::MoveSize {
                size,
                to: rng.below(len as u64 + 1) as usize,
            }
        }
        14..=15 => Command::SetSizeBreak {
            size: gen_size_id(rng, m, p),
            break_size: rng.chance(60),
        },
        _ => Command::SetBaseSize {
            size: gen_size_id(rng, m, p),
        },
    }
}

fn gen_other_command(rng: &mut Rng, p: &Project) -> Command {
    let sheet = p.sheets[rng.below(p.sheets.len() as u64) as usize].id;
    match rng.below(5) {
        0 => Command::AddSegment {
            sheet,
            from: PointRef::New(PointMm::new(rng.below(200) as f64, rng.below(200) as f64)),
            to: PointRef::New(PointMm::new(
                200.0 + rng.below(100) as f64,
                rng.below(200) as f64,
            )),
        },
        1 => Command::RenameSheet {
            sheet,
            name: format!("Feuille {}", rng.below(9)),
        },
        2 => Command::CreateSheet {
            name: format!("Autre {}", rng.below(9)),
        },
        3 => Command::AddVariable {
            name: format!("v{}", rng.below(6)),
            unit: VariableUnit::Mm,
            expr: format!("{}", rng.below(300)),
        },
        _ => Command::RenameProject {
            name: format!("Projet {}", rng.below(9)),
        },
    }
}

fn is_size_command(c: &Command) -> bool {
    matches!(
        c,
        Command::AddSizeRange { .. }
            | Command::RenameSizeRange { .. }
            | Command::RemoveSizeRange { .. }
            | Command::AddSize { .. }
            | Command::RemoveSize { .. }
            | Command::RenameSize { .. }
            | Command::MoveSize { .. }
            | Command::SetSizeBreak { .. }
            | Command::SetBaseSize { .. }
    )
}

// ---------------------------------------------------------------------------------------------
// Invariants
// ---------------------------------------------------------------------------------------------

fn bytes(p: &Project) -> String {
    persist::to_json(p).expect("projet valide")
}

#[derive(Default)]
struct Counters {
    steps: usize,
    accepted: usize,
    refused: usize,
    noops: usize,
    other: usize,
    undos: usize,
    redos: usize,
    reloads: usize,
    max_ranges: usize,
    max_sizes: usize,
}

fn check_state(p: &Project, expected: &Model, ctx: &str) {
    p.validate().unwrap_or_else(|e| panic!("{ctx} : {e}"));
    let got = model_of(p);
    assert_eq!(got.ranges, expected.ranges, "{ctx} : gammes");
    assert_eq!(got.next, expected.next, "{ctx} : compteur d'identifiants");
    let mut ids = Vec::new();
    for r in &p.sizes {
        // Une seule base, appartenant à la gamme.
        assert_eq!(
            r.sizes.iter().filter(|s| s.id == r.base).count(),
            1,
            "{ctx} : base de « {} »",
            r.name
        );
        assert!(!r.sizes.is_empty(), "{ctx} : gamme vide");
        ids.push(r.id.0);
        ids.extend(r.sizes.iter().map(|s| s.id.0));
    }
    let n = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), n, "{ctx} : identifiants en double");
    // Relecture identique, octet pour octet.
    let text = bytes(p);
    let back = persist::from_json(&text).unwrap_or_else(|e| panic!("{ctx} : relecture : {e}"));
    assert_eq!(&back, p, "{ctx} : relecture différente");
    assert_eq!(bytes(&back), text, "{ctx} : réécriture différente");
    // `evaluate_sheet(None)` = géométrie de base = toute taille.
    for sheet in &p.sheets {
        let base = p.evaluate_sheet(sheet.id, None).expect("évaluation");
        for pt in &sheet.points {
            let pos = sheet.position(pt.id).unwrap();
            assert!(
                base.points.iter().any(|(i, q)| *i == pt.id && *q == pos),
                "{ctx} : point {} absent de l'évaluation",
                pt.id.0
            );
        }
        assert_eq!(base.points.len(), sheet.points.len(), "{ctx}");
        for r in &p.sizes {
            for s in &r.sizes {
                let e = p
                    .evaluate_sheet(sheet.id, Some(s.id))
                    .expect("taille connue");
                assert_eq!(e.points, base.points, "{ctx} : taille {}", s.label);
            }
        }
    }
    assert!(
        p.evaluate_sheet(p.sheets[0].id, Some(SizeId(555_555_555)))
            .is_err(),
        "{ctx} : taille inconnue acceptée"
    );
}

fn rest(p: &Project) -> Project {
    let mut q = p.clone();
    q.sizes.clear();
    q.next_id = 0;
    q
}

fn run_sequence(seed: u64, c: &mut Counters) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    // Un peu de géométrie pour que l'évaluation ait de quoi dire ; le document repart ensuite
    // d'un historique vide (la géométrie n'est jamais défaite ici).
    let mut start = Project::new("Adverse tailles");
    let sheet = start.sheets[0].id;
    start
        .apply(Command::AddSegment {
            sheet,
            from: PointRef::New(PointMm::new(0.0, 0.0)),
            to: PointRef::New(PointMm::new(120.0, 35.0)),
        })
        .unwrap();
    let mut doc = Document::new(start);
    let mut model = model_of(doc.project());
    // Historique de référence : états successifs en octets, `cursor` = état courant.
    let mut history: Vec<String> = vec![bytes(doc.project())];
    let mut cursor = 0usize;

    for step in 0..STEPS {
        c.steps += 1;
        let ctx = format!("séquence {seed}, pas {step}");
        let before = doc.project().clone();
        let before_bytes = bytes(&before);
        match rng.below(100) {
            // Annuler.
            0..=7 => {
                let expect = cursor > 0;
                assert_eq!(doc.undo(), expect, "{ctx} : annuler");
                if expect {
                    cursor -= 1;
                    c.undos += 1;
                }
                assert_eq!(bytes(doc.project()), history[cursor], "{ctx} : annuler");
                model = model_of(doc.project());
            }
            // Rétablir.
            8..=13 => {
                let expect = cursor + 1 < history.len();
                assert_eq!(doc.redo(), expect, "{ctx} : rétablir");
                if expect {
                    cursor += 1;
                    c.redos += 1;
                }
                assert_eq!(bytes(doc.project()), history[cursor], "{ctx} : rétablir");
                model = model_of(doc.project());
            }
            // Écrire puis relire : le même projet.
            14..=17 => {
                let back = persist::from_json(&before_bytes).expect("relecture");
                assert_eq!(back, before, "{ctx} : relecture");
                c.reloads += 1;
            }
            // Commande d'un autre domaine : aucune gamme touchée.
            18..=27 => {
                let cmd = gen_other_command(&mut rng, doc.project());
                let outcome = doc.execute(cmd.clone());
                c.other += 1;
                let after = doc.project().clone();
                assert_eq!(
                    after.sizes, before.sizes,
                    "{ctx} : {cmd:?} touche les gammes"
                );
                let after_bytes = bytes(&after);
                if outcome.is_err() {
                    assert_eq!(after_bytes, before_bytes, "{ctx} : refus avec trace");
                } else if after_bytes != before_bytes {
                    history.truncate(cursor + 1);
                    history.push(after_bytes);
                    cursor += 1;
                }
                model = model_of(doc.project());
            }
            // Commande de tailles.
            _ => {
                let cmd = gen_size_command(&mut rng, &model, doc.project());
                assert!(is_size_command(&cmd));
                let predicted = model.predict(&cmd);
                let result = doc.execute(cmd.clone());
                let after = doc.project().clone();
                let after_bytes = bytes(&after);
                match (&predicted, &result) {
                    (None, Err(e)) => {
                        c.refused += 1;
                        assert!(!e.to_string().is_empty(), "{ctx}");
                        assert_eq!(
                            after_bytes, before_bytes,
                            "{ctx} : refus avec trace : {cmd:?}"
                        );
                        assert!(
                            before.check_size_command(&cmd).is_err(),
                            "{ctx} : le contrôle typé accepte {cmd:?}"
                        );
                    }
                    (Some(m), Ok(_)) => {
                        c.accepted += 1;
                        assert!(
                            before.check_size_command(&cmd).is_ok(),
                            "{ctx} : le contrôle typé refuse {cmd:?}"
                        );
                        assert_eq!(&model_of(&after), m, "{ctx} : état après {cmd:?}");
                        // Rien d'autre n'a bougé.
                        assert_eq!(
                            rest(&after),
                            rest(&before),
                            "{ctx} : {cmd:?} touche le reste"
                        );
                        if after_bytes == before_bytes {
                            c.noops += 1;
                        } else {
                            history.truncate(cursor + 1);
                            history.push(after_bytes);
                            cursor += 1;
                        }
                    }
                    _ => panic!("{ctx} : oracle {predicted:?} / code {result:?} pour {cmd:?}"),
                }
                model = model_of(doc.project());
            }
        }
        check_state(doc.project(), &model, &ctx);
        // L'historique du document est exactement celui de l'oracle (étape ssi état change).
        assert_eq!(doc.can_undo(), cursor > 0, "{ctx} : can_undo");
        assert_eq!(
            doc.can_redo(),
            cursor + 1 < history.len(),
            "{ctx} : can_redo"
        );
        c.max_ranges = c.max_ranges.max(doc.project().sizes.len());
        c.max_sizes = c
            .max_sizes
            .max(doc.project().sizes.iter().map(|r| r.sizes.len()).sum());
    }
    // Tout défaire puis tout refaire : états exacts à l'octet.
    while cursor > 0 {
        assert!(doc.undo());
        cursor -= 1;
        assert_eq!(bytes(doc.project()), history[cursor]);
    }
    assert!(!doc.can_undo());
    while cursor + 1 < history.len() {
        assert!(doc.redo());
        cursor += 1;
        assert_eq!(bytes(doc.project()), history[cursor]);
    }
}

#[test]
fn campagne_adverse_tailles() {
    let mut c = Counters::default();
    for seed in 1..=SEQUENCES {
        run_sequence(seed, &mut c);
    }
    eprintln!(
        "tailles : {} pas, {} acceptés ({} sans effet), {} refusés, {} autres commandes, \
         {} annuler, {} rétablir, {} relectures, max {} gammes / {} tailles",
        c.steps,
        c.accepted,
        c.noops,
        c.refused,
        c.other,
        c.undos,
        c.redos,
        c.reloads,
        c.max_ranges,
        c.max_sizes
    );
    // La campagne doit réellement exercer chaque famille.
    assert!(c.accepted > 1500, "trop peu d'acceptés : {}", c.accepted);
    assert!(c.refused > 1500, "trop peu de refus : {}", c.refused);
    assert!(
        c.noops > 20,
        "trop peu de commandes sans effet : {}",
        c.noops
    );
    assert!(c.other > 500 && c.undos > 200 && c.redos > 50 && c.reloads > 200);
    assert!(c.max_ranges >= 3 && c.max_sizes >= 10);
}

//! US-10.2 (reste) — campagne adverse « taille sur-mesure générée depuis la fiche client » :
//! 200 séquences à graine fixe de 40 pas (génération, remplacement, refus, fiches, barème,
//! suppressions), deux gammes dont une de 59 tailles pour atteindre la limite de 60.
//!
//! **Oracle indépendant** : un modèle de données écrit ici (colonnes du barème et valeurs des fiches
//! en cartes triées, libellés par gamme) décide seul accepté / refusé (fiche ou gamme fantôme,
//! libellé vide / de plus de 20 caractères / à caractère de contrôle / doublon casse ignorée sans
//! confirmation, limite de 60 tailles) et prédit l'état complet : la colonne générée est **exactement**
//! la fiche, une cellule sans valeur dans la fiche est vide (même si une valeur existait avant un
//! remplacement), les autres colonnes et la fiche ne bougent jamais, rien ne « suit » ensuite.
//!
//! Invariants à chaque pas : accepté ⇔ accepté par l'oracle ; refus = octets identiques ; état
//! complet identique à l'oracle ; remarque qui nomme chaque mesure manquante ; relecture JSON ;
//! nombre d'étapes annulables = nombre de pas qui changent l'état ; tout annuler redonne le projet
//! initial. Les compteurs prouvent que chaque contrôle fort s'est exécuté.

use pf_core::persist;
use pf_core::*;
use std::collections::BTreeMap;

const SEQUENCES: u64 = 200;
const STEPS: usize = 40;

struct R(u64);
impl R {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        ((self.next() >> 11) as usize) % n
    }
}

type Col = BTreeMap<u64, f64>;

#[derive(Clone, Debug, PartialEq)]
struct O {
    /// (id, nom, colonne : taille → valeur)
    ms: Vec<(u64, String, Col)>,
    /// (id, nom, valeurs : mesure → valeur)
    cs: Vec<(u64, String, Col)>,
    /// (id de gamme, tailles (id, libellé))
    ranges: Vec<(u64, Vec<(u64, String)>)>,
    /// Schéma 6 : taille → fiche dont elle a été générée (jamais retirée en silence).
    origin: BTreeMap<u64, u64>,
}

#[derive(Default, Debug)]
struct Counters {
    generated_new: usize,
    replaced_changed: usize,
    replaced_noop: usize,
    refused_client: usize,
    refused_range: usize,
    refused_blank: usize,
    refused_long: usize,
    refused_control: usize,
    refused_dup: usize,
    refused_limit: usize,
    missing_cells: usize,
    stale_cleared: usize,
    full_cards: usize,
    refused_cited: usize,
    removed_uncited: usize,
    other_ops: usize,
    refused_other: usize,
    undo_checked: usize,
}

fn good(v: f64) -> bool {
    v.is_finite() && (0.0..=1_000_000.0).contains(&v)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).expect("projet valide")
}

fn check_state(p: &Project, o: &O) {
    assert_eq!(p.measurements.len(), o.ms.len());
    for (m, (id, name, col)) in p.measurements.iter().zip(&o.ms) {
        assert_eq!((m.id.0, &m.name), (*id, name));
        let got: Col = m.values.iter().map(|v| (v.size.0, v.value)).collect();
        assert_eq!(&got, col, "colonne de {name}");
    }
    assert_eq!(p.clients.len(), o.cs.len());
    for (c, (id, name, vals)) in p.clients.iter().zip(&o.cs) {
        assert_eq!((c.id.0, &c.name), (*id, name));
        let got: Col = c
            .values
            .iter()
            .map(|v| (v.measurement.0, v.value))
            .collect();
        assert_eq!(&got, vals, "fiche {name}");
    }
    assert_eq!(p.sizes.len(), o.ranges.len());
    for (r, (id, labels)) in p.sizes.iter().zip(&o.ranges) {
        assert_eq!(r.id.0, *id);
        let got: Vec<(u64, String)> = r.sizes.iter().map(|s| (s.id.0, s.label.clone())).collect();
        assert_eq!(&got, labels);
        for s in &r.sizes {
            assert_eq!(s.client.map(|c| c.0), o.origin.get(&s.id.0).copied());
        }
    }
}

fn fixture() -> (Project, O) {
    let mut p = Project::new("Adverse sur-mesure");
    let mut o = O {
        ms: Vec::new(),
        cs: Vec::new(),
        ranges: Vec::new(),
        origin: BTreeMap::new(),
    };
    for (name, labels) in [
        ("A", vec!["S".to_string(), "M".to_string(), "L".to_string()]),
        ("B", (0..59).map(|i| format!("T{i}")).collect()),
    ] {
        p.apply(Command::AddSizeRange {
            name: name.into(),
            kind: SizeKind::Alphanumeric,
            labels,
            base: 0,
        })
        .unwrap();
    }
    for r in &p.sizes {
        o.ranges.push((
            r.id.0,
            r.sizes.iter().map(|s| (s.id.0, s.label.clone())).collect(),
        ));
    }
    (p, o)
}

fn pick_label(r: &mut R, o: &O, range: usize, n: &mut usize) -> String {
    let existing: Vec<String> = o.ranges[range].1.iter().map(|s| s.1.clone()).collect();
    match r.below(12) {
        0 => String::new(),
        1 => "   ".into(),
        2 => "x".repeat(21),
        3 => "a\tb".into(),
        4 | 5 => {
            // Libellé existant, parfois changé de casse ou entouré d'espaces.
            let e = &existing[r.below(existing.len())];
            match r.below(3) {
                0 => e.to_lowercase(),
                1 => format!("  {e} "),
                _ => e.clone(),
            }
        }
        6 => "x".repeat(20),
        _ => {
            *n += 1;
            format!("N{n}")
        }
    }
}

fn step(doc: &mut Document, o: &mut O, r: &mut R, c: &mut Counters, n: &mut usize) -> bool {
    let before_o = o.clone();
    let before_bytes = bytes(doc.project());
    let k = r.below(100);
    let ghost = 900_000 + r.below(5) as u64;
    // Commande + prédiction de l'oracle (None = refus).
    let (cmd, accepted): (Command, bool);
    let mut expect_new: Option<(usize, String)> = None; // (gamme, libellé créé)
    let mut gen_info: Option<(u64, usize, u64)> = None; // (fiche, gamme, taille cible)
    if k < 40 {
        let ci = if o.cs.is_empty() || r.below(12) == 0 {
            None
        } else {
            Some(r.below(o.cs.len()))
        };
        let ri = if r.below(14) == 0 {
            None
        } else {
            Some(r.below(o.ranges.len()))
        };
        let label = pick_label(r, o, ri.unwrap_or(0), n);
        let replace = r.below(2) == 0;
        let client = ci.map_or(ClientId(ghost), |i| ClientId(o.cs[i].0));
        let range = ri.map_or(SizeRangeId(ghost), |i| SizeRangeId(o.ranges[i].0));
        cmd = Command::GenerateClientSize {
            client,
            range,
            label: label.clone(),
            replace,
        };
        let t = label.trim().to_string();
        accepted = if ci.is_none() {
            c.refused_client += 1;
            false
        } else if ri.is_none() {
            c.refused_range += 1;
            false
        } else if t.is_empty() {
            c.refused_blank += 1;
            false
        } else if t.chars().count() > 20 {
            c.refused_long += 1;
            false
        } else if t.chars().any(|ch| ch.is_control()) {
            c.refused_control += 1;
            false
        } else {
            let (Some(ci), Some(ri)) = (ci, ri) else {
                unreachable!("fiche et gamme présentes")
            };
            let found = o.ranges[ri]
                .1
                .iter()
                .find(|s| s.1.to_lowercase() == t.to_lowercase())
                .map(|s| s.0);
            match found {
                Some(_) if !replace => {
                    c.refused_dup += 1;
                    false
                }
                None if o.ranges[ri].1.len() >= 60 => {
                    c.refused_limit += 1;
                    false
                }
                _ => {
                    let target = match found {
                        Some(id) => id,
                        None => {
                            expect_new = Some((ri, t.clone()));
                            0
                        }
                    };
                    gen_info = Some((o.cs[ci].0, ri, target));
                    true
                }
            }
        };
    } else if k < 48 {
        *n += 1;
        let unit = if r.below(4) == 0 {
            MeasurementUnit::Deg
        } else {
            MeasurementUnit::Mm
        };
        cmd = Command::AddMeasurement {
            name: format!("M{n}"),
            unit,
        };
        accepted = true;
    } else if k < 56 {
        *n += 1;
        cmd = Command::AddClient {
            name: format!("C{n}"),
        };
        accepted = true;
    } else if k < 72 {
        // Valeur de fiche, parfois invalide ou sur mesure / fiche fantôme.
        let vals = [0.0, 1.0, 408.0, 930.5, 1_000_000.0];
        let bad = [-1.0, f64::NAN, 1_000_001.0, f64::INFINITY];
        let invalid = r.below(5) == 0;
        let v = if invalid {
            bad[r.below(bad.len())]
        } else {
            vals[r.below(vals.len())]
        };
        if o.cs.is_empty() || o.ms.is_empty() {
            *n += 1;
            cmd = Command::AddClient {
                name: format!("C{n}"),
            };
            accepted = true;
        } else {
            let (ci, mi) = (r.below(o.cs.len()), r.below(o.ms.len()));
            cmd = Command::SetClientValue {
                client: ClientId(o.cs[ci].0),
                measurement: MeasurementId(o.ms[mi].0),
                value: v,
            };
            accepted = good(v);
            if accepted {
                let m = o.ms[mi].0;
                o.cs[ci].2.insert(m, v);
            }
        }
    } else if k < 77 {
        if o.cs.is_empty() || o.ms.is_empty() {
            *n += 1;
            cmd = Command::AddClient {
                name: format!("C{n}"),
            };
            accepted = true;
        } else {
            let (ci, mi) = (r.below(o.cs.len()), r.below(o.ms.len()));
            cmd = Command::ClearClientValue {
                client: ClientId(o.cs[ci].0),
                measurement: MeasurementId(o.ms[mi].0),
            };
            accepted = true;
            let m = o.ms[mi].0;
            o.cs[ci].2.remove(&m);
        }
    } else if k < 89 {
        if o.ms.is_empty() {
            *n += 1;
            cmd = Command::AddMeasurement {
                name: format!("M{n}"),
                unit: MeasurementUnit::Mm,
            };
            accepted = true;
        } else {
            let mi = r.below(o.ms.len());
            let ri = r.below(o.ranges.len());
            let (si, _) = o.ranges[ri].1[r.below(o.ranges[ri].1.len())].clone();
            let v = [0.0, 100.0, 500.5, 999.0][r.below(4)];
            cmd = Command::SetMeasurementValue {
                measurement: MeasurementId(o.ms[mi].0),
                size: SizeId(si),
                value: v,
            };
            accepted = true;
            o.ms[mi].2.insert(si, v);
        }
    } else if k < 95 {
        if o.cs.is_empty() {
            *n += 1;
            cmd = Command::AddClient {
                name: format!("C{n}"),
            };
            accepted = true;
        } else {
            let ci = r.below(o.cs.len());
            cmd = Command::RemoveClient {
                client: ClientId(o.cs[ci].0),
            };
            // Schéma 6 : refusé tant qu'une taille générée depuis la fiche la cite.
            accepted = !o.origin.values().any(|c| *c == o.cs[ci].0);
            if accepted {
                c.removed_uncited += 1;
                o.cs.remove(ci);
            }
        }
    } else if o.ms.is_empty() {
        *n += 1;
        cmd = Command::AddMeasurement {
            name: format!("M{n}"),
            unit: MeasurementUnit::Mm,
        };
        accepted = true;
    } else {
        let mi = r.below(o.ms.len());
        let id = o.ms[mi].0;
        cmd = Command::RemoveMeasurement {
            measurement: MeasurementId(id),
        };
        accepted = true;
        o.ms.remove(mi);
        for cl in &mut o.cs {
            cl.2.remove(&id);
        }
    }
    let is_gen = matches!(cmd, Command::GenerateClientSize { .. });
    let is_add = matches!(
        cmd,
        Command::AddMeasurement { .. } | Command::AddClient { .. }
    );
    let res = doc.execute(cmd.clone());
    assert_eq!(
        res.is_ok(),
        accepted,
        "accepté ⇔ oracle ({cmd:?}) : {res:?}"
    );
    if !accepted {
        assert_eq!(bytes(doc.project()), before_bytes, "refus sans trace");
        if is_gen {
            // Le compteur de refus de génération est déjà tenu plus haut.
        } else if let Command::RemoveClient { client } = &cmd {
            // Refus nommé : l'erreur cite la taille (libellé) et sa gamme, rien n'est retiré.
            // La première taille (ordre des gammes puis des tailles) qui cite la fiche.
            let label = &o
                .ranges
                .iter()
                .flat_map(|r| r.1.iter())
                .find(|s| o.origin.get(&s.0) == Some(&client.0))
                .unwrap()
                .1;
            let msg = res.unwrap_err().to_string();
            assert!(msg.contains(&format!("« {label} »")), "{msg}");
            c.refused_cited += 1;
        } else {
            c.refused_other += 1;
        }
        *o = before_o;
        check_state(doc.project(), o);
        return false;
    }
    let out = res.unwrap();
    if is_add {
        // Ajouts : l'oracle apprend l'identifiant attribué.
        match &cmd {
            Command::AddMeasurement { name, .. } => {
                o.ms.push((out.measurements[0].0, name.clone(), Col::new()))
            }
            Command::AddClient { name } => o.cs.push((out.clients[0].0, name.clone(), Col::new())),
            _ => unreachable!(),
        }
    }
    if let Some((cid, ri, target)) = gen_info {
        let t = match &cmd {
            Command::GenerateClientSize { label, .. } => label.trim().to_string(),
            _ => unreachable!(),
        };
        let size = match &expect_new {
            Some((_, label)) => {
                assert_eq!(out.sizes.len(), 1);
                let id = out.sizes[0].0;
                assert!(o.ranges.iter().all(|r| r.1.iter().all(|s| s.0 != id)));
                o.ranges[ri].1.push((id, label.clone()));
                c.generated_new += 1;
                id
            }
            None => {
                assert!(out.sizes.is_empty(), "remplacement : aucune taille créée");
                target
            }
        };
        o.origin.insert(size, cid);
        let card = o.cs.iter().find(|x| x.0 == cid).unwrap().2.clone();
        let mut missing = Vec::new();
        for m in &mut o.ms {
            match card.get(&m.0) {
                Some(v) => {
                    m.2.insert(size, *v);
                }
                None => {
                    if m.2.remove(&size).is_some() {
                        c.stale_cleared += 1;
                    }
                    missing.push(m.1.clone());
                    c.missing_cells += 1;
                }
            }
        }
        if missing.is_empty() && !o.ms.is_empty() {
            c.full_cards += 1;
        }
        let filled = o.ms.len() - missing.len();
        let note = out.notes.last().expect("remarque de génération");
        assert!(note.contains(&format!("{filled} mesure(s) renseignée(s)")));
        assert!(note.contains(&t));
        for name in &missing {
            assert!(note.contains(name.as_str()), "{name} nommée dans {note}");
        }
        if expect_new.is_none() {
            if *o == before_o {
                c.replaced_noop += 1;
            } else {
                c.replaced_changed += 1;
            }
        }
    } else {
        c.other_ops += 1;
    }
    check_state(doc.project(), o);
    persist::from_json(&bytes(doc.project())).expect("relecture");
    // Le projet est identique à l'avant-pas si et seulement si l'oracle l'est.
    assert_eq!(
        bytes(doc.project()) == before_bytes,
        *o == before_o,
        "{cmd:?}"
    );
    *o != before_o
}

#[test]
fn campagne_taille_client() {
    let mut c = Counters::default();
    for seq in 0..SEQUENCES {
        let mut r = R(0x9E37_79B9_7F4A_7C15u64.wrapping_mul(seq + 1) ^ 0xA5A5_1234);
        let (p, mut o) = fixture();
        let initial = p.clone();
        let mut doc = Document::new(p);
        let mut n = 0usize;
        let mut effective = 0usize;
        for _ in 0..STEPS {
            if step(&mut doc, &mut o, &mut r, &mut c, &mut n) {
                effective += 1;
            }
        }
        let mut undone = 0usize;
        while doc.undo() {
            undone += 1;
        }
        assert_eq!(undone, effective, "séquence {seq} : une étape par effet");
        assert_eq!(*doc.project(), initial, "tout annuler = projet initial");
        c.undo_checked += 1;
    }
    eprintln!("{c:?}");
    assert_eq!(c.undo_checked, SEQUENCES as usize);
    assert!(c.generated_new >= 300, "{c:?}");
    assert!(c.replaced_changed >= 40, "{c:?}");
    assert!(c.replaced_noop >= 15, "{c:?}");
    assert!(c.refused_client >= 20, "{c:?}");
    assert!(c.refused_range >= 20, "{c:?}");
    assert!(c.refused_blank >= 20, "{c:?}");
    assert!(c.refused_long >= 10, "{c:?}");
    assert!(c.refused_control >= 10, "{c:?}");
    assert!(c.refused_dup >= 20, "{c:?}");
    assert!(c.refused_limit >= 5, "{c:?}");
    assert!(c.missing_cells >= 300, "{c:?}");
    assert!(c.stale_cleared >= 10, "{c:?}");
    assert!(c.full_cards >= 10, "{c:?}");
    assert!(c.refused_cited >= 20, "{c:?}");
    assert!(c.removed_uncited >= 100, "{c:?}");
    assert!(c.other_ops >= 1000, "{c:?}");
    assert!(c.refused_other >= 30, "{c:?}");
}

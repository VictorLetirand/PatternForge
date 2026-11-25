//! US-03.5 — campagne adverse « diagnostic des liens » : 160 séquences à graine fixe de 60 pas
//! (variables, points construits, formules, segments longueur-angle / perpendiculaires /
//! parallèles, liage et déliage, courbes, longueurs imposées et égalités, déplacements,
//! suppressions, calques et verrous, annuler, rétablir).
//!
//! **Oracle indépendant** : les comptes attendus sont recalculés ici **depuis le JSON** du projet
//! (`serde_json::to_value`), sans appeler `pf_core::diagnostic` ni les validations : formules,
//! segments dont `construction.kept` est vrai (points suivis = `to`, ou `from` et `to` pour une
//! parallèle), contraintes de longueur avec ou sans `reference` (extrémité glissante = point
//! suivi), calques verrouillés lus sur les objets **écrits** par chaque lien, variables inutilisées
//! par un découpage en identifiants du texte des expressions (pas l'analyseur du cœur).
//!
//! Invariants après **chaque** pas (accepté, refusé, annuler, rétablir) : le diagnostic est une
//! fonction pure (octets du projet inchangés, deux appels égaux) ; aucune erreur sur un projet
//! produit par des commandes acceptées ; comptes par type et par gravité = oracle, points suivis =
//! oracle ; tri par (gravité, type) ; relecture du fichier = même liste. Et sur un **clone
//! altéré** (formule dont la variable disparaît, point suivi déplacé, longueur écrite décalée) :
//! erreurs attendues, `validate()` refuse, et pour chaque type `actifs + erreurs = liens`.
//!
//! Mutation vérifiée à la main : voir `docs/JOURNAL.md` (tranche 92).

use pf_core::persist;
use pf_core::*;
use pf_geometry::{PointMm, Side};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

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
    fn pick<T: Clone>(&mut self, xs: &[T]) -> Option<T> {
        if xs.is_empty() {
            None
        } else {
            Some(xs[self.below(xs.len() as u64) as usize].clone())
        }
    }
    /// Nombre à deux décimales dans [lo ; hi].
    fn val(&mut self, lo: f64, hi: f64) -> f64 {
        let n = ((hi - lo) * 100.0) as u64;
        lo + self.below(n + 1) as f64 / 100.0
    }
}

// ---------------------------------------------------------------------------------------------
// Oracle (lecture du JSON)
// ---------------------------------------------------------------------------------------------

const LINKS: [LinkType; 4] = [
    LinkType::Formula,
    LinkType::KeptConstruction,
    LinkType::ImposedLength,
    LinkType::LengthEquality,
];

#[derive(Default, Debug, PartialEq)]
struct Oracle {
    links: HashMap<LinkType, usize>,
    followed: HashMap<LinkType, usize>,
    locked: HashMap<LinkType, usize>,
    unused: usize,
}

fn arr(v: &Value) -> impl Iterator<Item = &Value> {
    v.as_array().into_iter().flatten()
}

fn idents(text: &str, out: &mut HashSet<String>) {
    let cs: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < cs.len() {
        let c = cs[i];
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < cs.len() && (cs[i].is_ascii_alphanumeric() || cs[i] == '_') {
                i += 1;
            }
            out.insert(cs[start..i].iter().collect());
        } else if c.is_ascii_digit() {
            while i < cs.len() && (cs[i].is_ascii_alphanumeric() || cs[i] == ',' || cs[i] == '.') {
                i += 1;
            }
        } else {
            i += 1;
        }
    }
}

fn oracle(p: &Project) -> Oracle {
    let v = serde_json::to_value(p).unwrap();
    let mut o = Oracle::default();
    let mut cited = HashSet::new();
    for var in arr(&v["variables"]) {
        idents(var["expr"].as_str().unwrap(), &mut cited);
    }
    for s in arr(&v["sheets"]) {
        let locked_layers: HashSet<u64> = arr(&s["layers"])
            .filter(|l| l["locked"] == Value::Bool(true))
            .map(|l| l["id"].as_u64().unwrap())
            .collect();
        let locked = |item: &Value| {
            item.get("layer")
                .and_then(Value::as_u64)
                .is_some_and(|l| locked_layers.contains(&l))
        };
        let points: HashMap<u64, &Value> = arr(&s["points"])
            .map(|p| (p["id"].as_u64().unwrap(), p))
            .collect();
        let entities: HashMap<u64, &Value> = arr(&s["entities"])
            .map(|e| (e["id"].as_u64().unwrap(), e))
            .collect();
        let point_locked = |id: u64| points.get(&id).is_some_and(|p| locked(p));
        let entity_locked = |id: u64| entities.get(&id).is_some_and(|e| locked(e));
        let bump = |m: &mut HashMap<LinkType, usize>, k: LinkType, n: usize| {
            if n > 0 {
                *m.entry(k).or_default() += n;
            }
        };
        for f in arr(&s["formulas"]) {
            idents(f["expr"].as_str().unwrap(), &mut cited);
            bump(&mut o.links, LinkType::Formula, 1);
            bump(&mut o.followed, LinkType::Formula, 1);
            if point_locked(f["point"].as_u64().unwrap()) {
                bump(&mut o.locked, LinkType::Formula, 1);
            }
        }
        for e in arr(&s["entities"]) {
            let kind = &e["kind"];
            let kept =
                kind["type"] == "segment" && kind["construction"]["kept"] == Value::Bool(true);
            if !kept {
                continue;
            }
            let mut deps = vec![kind["to"].as_u64().unwrap()];
            if kind["construction"]["type"] == "parallel" {
                deps.push(kind["from"].as_u64().unwrap());
            }
            bump(&mut o.links, LinkType::KeptConstruction, 1);
            bump(&mut o.followed, LinkType::KeptConstruction, deps.len());
            if locked(e) || deps.iter().any(|d| point_locked(*d)) {
                bump(&mut o.locked, LinkType::KeptConstruction, 1);
            }
        }
        for c in arr(&s["length_constraints"]) {
            let link = if c.get("reference").is_some() {
                LinkType::LengthEquality
            } else {
                LinkType::ImposedLength
            };
            bump(&mut o.links, link, 1);
            let slid =
                (c["adjust"]["type"] == "slide").then(|| c["adjust"]["point"].as_u64().unwrap());
            bump(&mut o.followed, link, usize::from(slid.is_some()));
            if entity_locked(c["curve"].as_u64().unwrap()) || slid.is_some_and(point_locked) {
                bump(&mut o.locked, link, 1);
            }
        }
    }
    o.unused = arr(&v["variables"])
        .filter(|var| !cited.contains(var["name"].as_str().unwrap()))
        .count();
    o
}

/// Comptes lus dans le diagnostic : `(actifs, erreurs, verrous, suivis des actifs)` par type.
#[derive(Default, Debug)]
struct Seen {
    active: HashMap<LinkType, usize>,
    broken: HashMap<LinkType, usize>,
    locked: HashMap<LinkType, usize>,
    followed: HashMap<LinkType, usize>,
    unused: usize,
}

fn seen(diags: &[LinkDiagnostic]) -> Seen {
    let mut s = Seen::default();
    for d in diags {
        match d.kind {
            DiagnosticKind::Active => {
                *s.active.entry(d.link).or_default() += 1;
                *s.followed.entry(d.link).or_default() += d.followed_points().count();
            }
            DiagnosticKind::Broken => *s.broken.entry(d.link).or_default() += 1,
            DiagnosticKind::LockedLayer => *s.locked.entry(d.link).or_default() += 1,
            DiagnosticKind::UnusedVariable => s.unused += 1,
            // Aucune gradation dans cette campagne (US-09.2a : voir `adverse_gradation.rs`).
            DiagnosticKind::GradedAndFollowed => {
                panic!("conflit de gradation inattendu : {d:?}")
            }
        }
    }
    s
}

fn get(m: &HashMap<LinkType, usize>, k: LinkType) -> usize {
    m.get(&k).copied().unwrap_or(0)
}

fn bytes(p: &Project) -> String {
    persist::to_json(p).unwrap()
}

// ---------------------------------------------------------------------------------------------
// Génération de commandes
// ---------------------------------------------------------------------------------------------

#[derive(Default)]
struct Cov {
    steps: u64,
    accepted: u64,
    refused: u64,
    undone: u64,
    redone: u64,
    max_links: [u64; 4],
    locked_seen: u64,
    unused_seen: u64,
    two_sheets: u64,
    tamper_formula: u64,
    tamper_kept: u64,
    tamper_length: u64,
    equality_tamper: u64,
}

fn random_command(rng: &mut Rng, p: &Project, next_var: &mut u64) -> Command {
    let sheet = &p.sheets[rng.below(p.sheets.len() as u64) as usize];
    let sid = sheet.id;
    let points: Vec<PointId> = sheet.points.iter().map(|x| x.id).collect();
    let segments: Vec<EntityId> = sheet
        .entities
        .iter()
        .filter(|e| matches!(e.kind, EntityKind::Segment { .. }))
        .map(|e| e.id)
        .collect();
    let constructed: Vec<EntityId> = sheet
        .entities
        .iter()
        .filter(|e| {
            matches!(
                e.kind,
                EntityKind::Segment {
                    construction: Some(_),
                    ..
                }
            )
        })
        .map(|e| e.id)
        .collect();
    let curves: Vec<EntityId> = sheet
        .entities
        .iter()
        .filter(|e| matches!(e.kind, EntityKind::Curve { .. }))
        .map(|e| e.id)
        .collect();
    let all_entities: Vec<EntityId> = sheet.entities.iter().map(|e| e.id).collect();
    let vars: Vec<String> = p.variables.iter().map(|v| v.name.clone()).collect();
    let free = |rng: &mut Rng| PointMm::new(rng.val(-200.0, 200.0), rng.val(-200.0, 200.0));
    let side = |rng: &mut Rng| {
        if rng.chance(50) {
            Side::Left
        } else {
            Side::Right
        }
    };
    let roll = rng.below(100);
    let fallback = |rng: &mut Rng| Command::AddPoint {
        sheet: sid,
        at: PointMm::new(rng.val(-200.0, 200.0), rng.val(-200.0, 200.0)),
    };
    match roll {
        0..=7 => {
            *next_var += 1;
            let name = format!("v{next_var}");
            let expr = match rng.pick(&vars) {
                Some(other) if rng.chance(50) => format!("{other}*{}", 1 + rng.below(3)),
                _ => format!("{},{}", 5 + rng.below(80), rng.below(10)),
            };
            Command::AddVariable {
                name,
                unit: VariableUnit::Mm,
                expr,
            }
        }
        8..=11 => match rng.pick(&vars) {
            Some(name) => Command::RemoveVariable { name },
            None => fallback(rng),
        },
        12..=15 => match rng.pick(&vars) {
            Some(name) => Command::SetVariableExpr {
                name,
                expr: format!("{}", 5 + rng.below(90)),
            },
            None => fallback(rng),
        },
        16..=22 => fallback(rng),
        23..=30 => match rng.pick(&points) {
            Some(from) => {
                let def = if rng.chance(50) {
                    PointDefinition::Polar {
                        from,
                        distance_mm: rng.val(10.0, 120.0),
                        angle_deg: rng.val(0.0, 350.0),
                        reference: None,
                    }
                } else {
                    PointDefinition::Offset {
                        from,
                        dx_mm: rng.val(-80.0, 80.0),
                        dy_mm: rng.val(-80.0, 80.0),
                    }
                };
                Command::AddPointAt {
                    sheet: sid,
                    point: PointRef::Derived(def),
                }
            }
            None => fallback(rng),
        },
        31..=42 => {
            let targets: Vec<(PointId, bool)> = sheet
                .points
                .iter()
                .filter_map(|x| match x.definition {
                    PointDefinition::Polar { .. } => Some((x.id, true)),
                    PointDefinition::Offset { .. } => Some((x.id, false)),
                    _ => None,
                })
                .collect();
            match rng.pick(&targets) {
                Some((point, polar)) => {
                    let param = match (polar, rng.chance(50)) {
                        (true, true) => ParamKind::Distance,
                        (true, false) => ParamKind::Angle,
                        (false, true) => ParamKind::Dx,
                        (false, false) => ParamKind::Dy,
                    };
                    let expr = if rng.chance(20) {
                        None
                    } else {
                        Some(match rng.pick(&vars) {
                            Some(v) => format!("{v}/2+{}", 1 + rng.below(30)),
                            None => format!("{}", 5 + rng.below(40)),
                        })
                    };
                    Command::SetParameterExpr {
                        target: ParamTarget {
                            sheet: sid,
                            point,
                            param,
                        },
                        expr,
                    }
                }
                None => fallback(rng),
            }
        }
        43..=48 => Command::AddSegmentPolar {
            sheet: sid,
            from: match rng.pick(&points) {
                Some(f) if rng.chance(70) => PointRef::Existing(f),
                _ => PointRef::New(free(rng)),
            },
            length_mm: rng.val(20.0, 150.0),
            angle_deg: rng.val(0.0, 350.0),
        },
        49..=52 => match (rng.pick(&segments), rng.pick(&points)) {
            (Some(reference), Some(from)) => Command::AddPerpendicular {
                sheet: sid,
                reference,
                from: PointRef::Existing(from),
                length_mm: rng.val(20.0, 100.0),
                side: side(rng),
            },
            _ => fallback(rng),
        },
        53..=55 => match rng.pick(&segments) {
            Some(reference) => Command::AddParallel {
                sheet: sid,
                reference,
                distance_mm: rng.val(10.0, 60.0),
                side: side(rng),
            },
            None => fallback(rng),
        },
        56..=64 => match rng.pick(&constructed) {
            Some(segment) => Command::SetConstructionKept {
                sheet: sid,
                segment,
                kept: rng.chance(75),
            },
            None => fallback(rng),
        },
        65..=68 => {
            let (x, y) = (rng.val(-200.0, 100.0), rng.val(-200.0, 200.0));
            Command::AddCurve {
                sheet: sid,
                start: PointRef::New(PointMm::new(x, y)),
                c1: PointMm::new(x + 40.0, y + rng.val(20.0, 90.0)),
                c2: PointMm::new(x + 150.0, y + rng.val(20.0, 90.0)),
                end: PointRef::New(PointMm::new(x + rng.val(150.0, 200.0), y)),
            }
        }
        69..=78 => match rng.pick(&curves) {
            Some(curve) => {
                let (start, end) = match sheet.entity(curve).map(|e| e.kind.clone()) {
                    Ok(EntityKind::Curve { start, end, .. }) => (start, end),
                    _ => return fallback(rng),
                };
                if rng.chance(15) {
                    return Command::SetCurveLengthConstraint {
                        sheet: sid,
                        curve,
                        spec: None,
                    };
                }
                let current = sheet.curve_developed_mm(curve).unwrap_or(200.0);
                let point = if rng.chance(50) { start } else { end };
                let adjust = if rng.chance(70) {
                    LengthAdjust::Slide {
                        point,
                        angle_deg: [0.0, 45.0, 90.0][rng.below(3) as usize],
                    }
                } else {
                    LengthAdjust::Handle { point }
                };
                let reference = if rng.chance(50) {
                    // Surtout une autre courbe (longueur voisine : la cible est atteignable).
                    let pool = if rng.chance(75) {
                        &curves
                    } else {
                        &all_entities
                    };
                    let others: Vec<EntityId> =
                        pool.iter().copied().filter(|e| *e != curve).collect();
                    rng.pick(&others).map(|entity| LengthRef {
                        entity,
                        offset_mm: rng.val(-20.0, 30.0),
                    })
                } else {
                    None
                };
                Command::SetCurveLengthConstraint {
                    sheet: sid,
                    curve,
                    spec: Some(LengthSpec {
                        length_mm: current * (0.9 + rng.below(40) as f64 / 100.0),
                        adjust,
                        reference,
                    }),
                }
            }
            None => fallback(rng),
        },
        79..=86 => match rng.pick(&points) {
            Some(point) => {
                let to = match sheet.position(point) {
                    Ok(at) if rng.chance(70) => {
                        PointMm::new(at.x + rng.val(-25.0, 25.0), at.y + rng.val(-25.0, 25.0))
                    }
                    _ => free(rng),
                };
                Command::MovePoint {
                    sheet: sid,
                    point,
                    to,
                }
            }
            None => fallback(rng),
        },
        87..=89 => {
            if rng.chance(50) {
                match rng.pick(&all_entities) {
                    Some(e) => Command::Delete {
                        sheet: sid,
                        points: vec![],
                        entities: vec![e],
                    },
                    None => fallback(rng),
                }
            } else {
                match rng.pick(&points) {
                    Some(pt) => Command::Delete {
                        sheet: sid,
                        points: vec![pt],
                        entities: vec![],
                    },
                    None => fallback(rng),
                }
            }
        }
        _ => {
            // Calques : créer, ranger des objets, verrouiller / déverrouiller.
            let layers: Vec<LayerId> = sheet.layers.iter().map(|l| l.id).collect();
            match (rng.below(3), rng.pick(&layers)) {
                (0, _) if layers.len() < 2 => Command::AddLayer {
                    sheet: sid,
                    name: format!("Calque {}", layers.len() + 1),
                },
                (1, Some(layer)) => {
                    let pts: Vec<PointId> =
                        points.iter().copied().filter(|_| rng.chance(35)).collect();
                    let ents: Vec<EntityId> = all_entities
                        .iter()
                        .copied()
                        .filter(|_| rng.chance(35))
                        .collect();
                    Command::SetObjectsLayer {
                        sheet: sid,
                        points: pts,
                        entities: ents,
                        layer: Some(layer),
                    }
                }
                (_, Some(layer)) => Command::SetLayerLocked {
                    sheet: sid,
                    layer,
                    locked: rng.chance(60),
                },
                _ => Command::AddLayer {
                    sheet: sid,
                    name: "Calque 1".into(),
                },
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Invariants
// ---------------------------------------------------------------------------------------------

fn check_valid(label: &str, p: &Project, cov: &mut Cov) {
    let before = bytes(p);
    let diags = p.link_diagnostics();
    assert_eq!(
        bytes(p),
        before,
        "{label} : le diagnostic a modifié le projet"
    );
    assert_eq!(
        diags,
        p.link_diagnostics(),
        "{label} : deux appels différents"
    );
    assert!(
        diags
            .iter()
            .all(|d| d.severity != DiagnosticSeverity::Error && d.kind != DiagnosticKind::Broken),
        "{label} : erreur sur un projet valide : {diags:#?}"
    );
    let o = oracle(p);
    let s = seen(&diags);
    for link in LINKS {
        assert_eq!(
            get(&s.active, link),
            get(&o.links, link),
            "{label} : liens {link:?}"
        );
        assert_eq!(
            get(&s.followed, link),
            get(&o.followed, link),
            "{label} : points suivis {link:?}"
        );
        assert_eq!(
            get(&s.locked, link),
            get(&o.locked, link),
            "{label} : verrous {link:?}"
        );
        assert_eq!(get(&s.broken, link), 0, "{label}");
    }
    assert_eq!(s.unused, o.unused, "{label} : variables inutilisées");
    let expected_total: usize = LINKS.iter().map(|l| get(&o.links, *l)).sum::<usize>()
        + LINKS.iter().map(|l| get(&o.locked, *l)).sum::<usize>()
        + o.unused;
    assert_eq!(
        diags.len(),
        expected_total,
        "{label} : lignes en trop ou en moins"
    );
    // Tri : gravité puis type ; les feuilles suivent l'ordre du projet à gravité et type égaux.
    let keys: Vec<_> = diags.iter().map(|d| (d.severity, d.link)).collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "{label} : tri");
    let order: HashMap<SheetId, usize> = p
        .sheets
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id, i))
        .collect();
    for w in diags.windows(2) {
        if (w[0].severity, w[0].link) == (w[1].severity, w[1].link) {
            let (a, b) = (
                w[0].sheet.and_then(|s| order.get(&s)).copied(),
                w[1].sheet.and_then(|s| order.get(&s)).copied(),
            );
            assert!(a <= b, "{label} : feuilles dans le désordre");
        }
    }
    // Chaque objet désigné existe dans sa feuille.
    for d in &diags {
        for obj in &d.objects {
            match (obj, d.sheet.and_then(|s| p.sheet(s).ok())) {
                (DiagnosticObject::Point(id), Some(sh)) => {
                    assert!(sh.point(*id).is_ok(), "{label}")
                }
                (DiagnosticObject::Entity(id), Some(sh)) => {
                    assert!(sh.entity(*id).is_ok(), "{label}")
                }
                (DiagnosticObject::Variable(n), None) => {
                    assert!(p.variables.iter().any(|v| &v.name == n), "{label}")
                }
                other => panic!("{label} : objet sans feuille : {other:?}"),
            }
        }
    }
    for (i, l) in LINKS.iter().enumerate() {
        cov.max_links[i] = cov.max_links[i].max(get(&o.links, *l) as u64);
    }
    cov.locked_seen += o.locked.values().sum::<usize>() as u64;
    cov.unused_seen += o.unused as u64;
    cov.two_sheets += u64::from(p.sheets.len() > 1);
}

/// Altère un **clone** (formule, point suivi ou longueur écrite) et vérifie les erreurs.
fn check_tampered(label: &str, p: &Project, rng: &mut Rng, cov: &mut Cov) {
    let o = oracle(p);
    let mut q = p.clone();
    let choice = rng.below(3);
    let (tampered, exactly_one): (LinkType, bool) = match choice {
        0 => {
            let Some((si, fi)) = q
                .sheets
                .iter()
                .enumerate()
                .flat_map(|(si, s)| (0..s.formulas.len()).map(move |fi| (si, fi)))
                .next()
            else {
                return;
            };
            q.sheets[si].formulas[fi].expr = "zz_absente+1".into();
            cov.tamper_formula += 1;
            (LinkType::Formula, true)
        }
        1 => {
            let Some((si, seg)) = q
                .sheets
                .iter()
                .enumerate()
                .flat_map(|(si, s)| s.kept_segment_ids().into_iter().map(move |g| (si, g)))
                .next()
            else {
                return;
            };
            let to = match q.sheets[si].entity(seg).map(|e| e.kind.clone()) {
                Ok(EntityKind::Segment { to, .. }) => to,
                _ => return,
            };
            let pt = q.sheets[si].points.iter_mut().find(|x| x.id == to).unwrap();
            let PointDefinition::Free { x, .. } = &mut pt.definition else {
                return;
            };
            *x += 0.5;
            cov.tamper_kept += 1;
            (LinkType::KeptConstruction, false)
        }
        _ => {
            let Some((si, ci)) = q
                .sheets
                .iter()
                .enumerate()
                .flat_map(|(si, s)| (0..s.length_constraints.len()).map(move |ci| (si, ci)))
                .next()
            else {
                return;
            };
            let c = &mut q.sheets[si].length_constraints[ci];
            c.length_mm += 3.0;
            cov.tamper_length += 1;
            let link = if c.reference.is_some() {
                cov.equality_tamper += 1;
                LinkType::LengthEquality
            } else {
                LinkType::ImposedLength
            };
            (link, true)
        }
    };
    assert!(
        q.validate().is_err(),
        "{label} : l'ouverture aurait dû refuser"
    );
    let diags = q.link_diagnostics();
    let s = seen(&diags);
    let errors = get(&s.broken, tampered);
    assert!(
        errors >= 1,
        "{label} : aucune erreur {tampered:?} : {diags:#?}"
    );
    if exactly_one {
        assert_eq!(errors, 1, "{label} : {tampered:?}");
    }
    for link in LINKS {
        assert_eq!(
            get(&s.active, link) + get(&s.broken, link),
            get(&o.links, link),
            "{label} : partition {link:?}"
        );
        if link != tampered && link != LinkType::Formula {
            assert_eq!(
                get(&s.broken, link),
                0,
                "{label} : erreur parasite {link:?}"
            );
        }
        assert_eq!(get(&s.locked, link), get(&o.locked, link), "{label}");
    }
    // Les erreurs viennent avant le reste.
    assert_eq!(diags[0].severity, DiagnosticSeverity::Error, "{label}");
}

fn run(seed: u64, cov: &mut Cov) {
    let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut doc = Document::new(Project::new("Adverse diagnostic"));
    let mut next_var = 0u64;
    if seed % 2 == 0 {
        doc.execute(Command::CreateSheet {
            name: "Deux".into(),
        })
        .unwrap();
    }
    for step in 0..STEPS {
        let label = format!("graine {seed}, pas {step}");
        cov.steps += 1;
        let before = bytes(doc.project());
        let roll = rng.below(100);
        if roll < 5 {
            if doc.undo() {
                cov.undone += 1;
            }
        } else if roll < 9 {
            if doc.redo() {
                cov.redone += 1;
            }
        } else {
            let cmd = random_command(&mut rng, doc.project(), &mut next_var);
            match doc.execute(cmd) {
                Ok(_) => cov.accepted += 1,
                Err(_) => {
                    cov.refused += 1;
                    assert_eq!(bytes(doc.project()), before, "{label} : refus avec trace");
                }
            }
        }
        let p = doc.project().clone();
        check_valid(&label, &p, cov);
        if step % 5 == 4 {
            let again = persist::from_json(&bytes(&p)).unwrap();
            assert_eq!(
                again.link_diagnostics(),
                p.link_diagnostics(),
                "{label} : relecture"
            );
        }
        if step % 3 == 2 {
            check_tampered(&label, &p, &mut rng, cov);
        }
    }
}

#[test]
fn campagne_adverse_diagnostic() {
    let mut cov = Cov::default();
    for seed in 1..=SEQUENCES {
        run(seed, &mut cov);
    }
    eprintln!(
        "couverture : acceptées {}, refusées {}, annulées {}, rétablies {}, liens max {:?}, \
         verrous {}, inutilisées {}, deux feuilles {}, altérations formule {} / suivi {} / \
         longueur {} (dont égalité {})",
        cov.accepted,
        cov.refused,
        cov.undone,
        cov.redone,
        cov.max_links,
        cov.locked_seen,
        cov.unused_seen,
        cov.two_sheets,
        cov.tamper_formula,
        cov.tamper_kept,
        cov.tamper_length,
        cov.equality_tamper
    );
    // La campagne exerce réellement les cas difficiles.
    assert_eq!(cov.steps, SEQUENCES * STEPS as u64);
    assert!(
        cov.accepted > 2000 && cov.refused > 200,
        "{:?}",
        (cov.accepted, cov.refused)
    );
    assert!(
        cov.undone > 100 && cov.redone > 20,
        "{:?}",
        (cov.undone, cov.redone)
    );
    for (i, l) in LINKS.iter().enumerate() {
        assert!(
            cov.max_links[i] >= 2,
            "{l:?} jamais exercé : {:?}",
            cov.max_links
        );
    }
    assert!(cov.locked_seen > 50, "verrous : {}", cov.locked_seen);
    assert!(
        cov.unused_seen > 200,
        "variables inutilisées : {}",
        cov.unused_seen
    );
    assert!(cov.two_sheets > 1000);
    assert!(cov.tamper_formula > 50, "{}", cov.tamper_formula);
    assert!(cov.tamper_kept > 50, "{}", cov.tamper_kept);
    assert!(cov.tamper_length > 30, "{}", cov.tamper_length);
    assert!(cov.equality_tamper > 5, "{}", cov.equality_tamper);
}

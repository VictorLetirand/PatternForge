//! Décision 334 — campagne adverse du schéma v5 (graine fixe, 170 séquences).
//!
//! Chaque séquence construit un projet aléatoire (rectangles, points, crans, variables, gammes,
//! écarts), l'écrit, le relabellise en v4 et vérifie : migration sans perte, octets canoniques
//! (une seule ligne change), idempotence, aller-retour par `serde_json::Value`. Elle pose ensuite
//! des objets v5 aléatoires (valeurs écrites en demi-millimètres, exactes en binaire) dont
//! l'oracle JSON est construit **dans le test** à partir des mêmes tirages, puis applique toutes
//! les corruptions applicables (référence cassée, doublon d'id, clé en trop, version future, objet
//! v5 dans un fichier v4…) qui doivent être refusées. Des compteurs prouvent que les contrôles
//! forts ont réellement été exécutés.

use pf_core::persist;
use pf_core::*;
use pf_geometry::{PointMm, Side};
use serde_json::{json, Value};

const SEQUENCES: usize = 170;

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
    /// Multiple de 0,5 dans `[lo ; hi]` (exact en binaire, relu à l'identique).
    fn half(&mut self, lo: i64, hi: i64) -> f64 {
        lo as f64 + self.below(((hi - lo) * 2 + 1) as u64) as f64 / 2.0
    }
}

#[derive(Default)]
struct Counters {
    sequences: usize,
    migrated: usize,
    bytes_checked: usize,
    one_line_diff: usize,
    idempotent: usize,
    value_roundtrip: usize,
    with_sizes: usize,
    with_gradings: usize,
    with_variables: usize,
    v5_objects: usize,
    v5_oracle_checked: usize,
    per_size_written: usize,
    proportional_written: usize,
    finished_seam_or_dim: usize,
    corruptions: std::collections::BTreeMap<&'static str, usize>,
}

fn v4_text(v5: &str) -> String {
    assert_eq!(v5.matches("\"schema_version\": 6,").count(), 1);
    v5.replacen("\"schema_version\": 6,", "\"schema_version\": 4,", 1)
}

/// Lignes qui diffèrent entre deux textes de même nombre de lignes (oracle de « une seule ligne »).
fn differing_lines<'a>(a: &'a str, b: &'a str) -> Vec<(&'a str, &'a str)> {
    let (la, lb): (Vec<&str>, Vec<&str>) = (a.lines().collect(), b.lines().collect());
    assert_eq!(la.len(), lb.len(), "nombre de lignes");
    la.into_iter().zip(lb).filter(|(x, y)| x != y).collect()
}

fn random_project(rng: &mut Rng, n: usize) -> Project {
    let mut p = Project::new(&format!("Projet adverse {n}"));
    let s = p.sheets[0].id;
    for _ in 0..1 + rng.below(3) {
        let out = p
            .apply(Command::AddRectangle {
                sheet: s,
                corner: PointRef::New(PointMm::new(rng.half(0, 500), rng.half(0, 500))),
                width_mm: 20.0 + rng.half(0, 200),
                height_mm: 20.0 + rng.half(0, 200),
            })
            .unwrap();
        if rng.chance(70) {
            let e = out.entities[rng.below(out.entities.len() as u64) as usize];
            let _ = p.apply(Command::AddNotch {
                sheet: s,
                entity: e,
                distance_mm: 5.0 + rng.half(0, 10),
                side: if rng.chance(50) {
                    Side::Left
                } else {
                    Side::Right
                },
                length_mm: 5.0,
                style: NotchStyle::Single,
            });
        }
    }
    for _ in 0..rng.below(3) {
        let _ = p.apply(Command::AddPoint {
            sheet: s,
            at: PointMm::new(1000.0 + rng.half(0, 300), 1000.0 + rng.half(0, 300)),
        });
    }
    for i in 0..rng.below(5) {
        let expr = if i > 0 && rng.chance(50) {
            format!("v{} * 2", rng.below(i))
        } else {
            format!("{}", 1 + rng.below(900))
        };
        p.apply(Command::AddVariable {
            name: format!("v{i}"),
            unit: VariableUnit::Mm,
            expr,
        })
        .unwrap();
    }
    let ranges = if rng.chance(85) { 1 + rng.below(2) } else { 0 };
    for r in 0..ranges {
        let count = 2 + rng.below(5) as usize;
        let labels: Vec<String> = (0..count).map(|k| format!("T{r}{k}")).collect();
        p.apply(Command::AddSizeRange {
            name: format!("Gamme {r}"),
            kind: SizeKind::Alphanumeric,
            labels,
            base: rng.below(count as u64) as usize,
        })
        .unwrap();
    }
    // Écarts sur les points libres (certains refus sont normaux : taille de base, etc.).
    let points: Vec<PointId> = p.sheets[0].points.iter().map(|q| q.id).collect();
    let sizes: Vec<SizeId> = p
        .sizes
        .iter()
        .flat_map(|r| r.sizes.iter().map(|z| z.id))
        .collect();
    for &pt in &points {
        if sizes.is_empty() || !rng.chance(40) {
            continue;
        }
        for _ in 0..1 + rng.below(3) {
            let size = sizes[rng.below(sizes.len() as u64) as usize];
            let _ = p.apply(Command::SetPointGrading {
                sheet: s,
                point: pt,
                size,
                dx_mm: rng.half(-20, 20),
                dy_mm: rng.half(-20, 20),
            });
        }
    }
    p
}

/// Sous-ensemble aléatoire non vide de `0..n` (dans l'ordre).
fn subset(rng: &mut Rng, n: usize) -> Vec<usize> {
    let mut v: Vec<usize> = (0..n).filter(|_| rng.chance(60)).collect();
    if v.is_empty() && n > 0 {
        v.push(rng.below(n as u64) as usize);
    }
    v
}

/// Pose des objets v5 aléatoires ; renvoie l'oracle JSON (clés de premier niveau et colonnes
/// modifiées), construit à la main à partir des mêmes tirages.
fn add_v5_objects(rng: &mut Rng, p: &mut Project, c: &mut Counters) -> Value {
    let mut oracle = serde_json::Map::new();
    let sizes: Vec<SizeId> = p
        .sizes
        .iter()
        .flat_map(|r| r.sizes.iter().map(|z| z.id))
        .collect();
    let mut next = p.next_id;
    fn alloc(next: &mut u64) -> u64 {
        *next += 1;
        *next - 1
    }
    // Règles nommées.
    if !sizes.is_empty() && rng.chance(70) {
        let (mut rules, mut js) = (Vec::new(), Vec::new());
        for i in 0..1 + rng.below(3) {
            let rid = alloc(&mut next);
            let modes = [
                (GradingCopyMode::Same, None),
                (GradingCopyMode::MirrorX, Some("mirror_x")),
                (GradingCopyMode::MirrorY, Some("mirror_y")),
                (GradingCopyMode::Invert, Some("invert")),
            ];
            let (mode, mode_json) = modes[rng.below(4) as usize];
            let (mut deltas, mut jd) = (Vec::new(), Vec::new());
            for k in subset(rng, sizes.len()) {
                let (dx, dy) = (rng.half(-50, 50), rng.half(-50, 50));
                deltas.push(GradingRuleDelta {
                    size: sizes[k],
                    dx_mm: dx,
                    dy_mm: dy,
                });
                jd.push(json!({"size": sizes[k].0, "dx_mm": dx, "dy_mm": dy}));
            }
            let name = format!("Règle {i}");
            let mut j = json!({"id": rid, "name": name, "deltas": jd});
            if let Some(m) = mode_json {
                j["mode"] = json!(m);
            }
            rules.push(GradingRule {
                id: GradingRuleId(rid),
                name,
                mode,
                deltas,
            });
            js.push(j);
        }
        p.grading_rules = rules;
        oracle.insert("grading_rules".into(), json!(js));
    }
    // Barème, fiches, retouches.
    let mut measures: Vec<MeasurementId> = Vec::new();
    if rng.chance(75) {
        let (mut ms, mut js) = (Vec::new(), Vec::new());
        for i in 0..1 + rng.below(4) {
            let mid = alloc(&mut next);
            let deg = rng.chance(25);
            let (mut values, mut jv) = (Vec::new(), Vec::new());
            if !sizes.is_empty() && rng.chance(80) {
                for k in subset(rng, sizes.len()) {
                    let v = rng.half(0, 900) / 2.0;
                    values.push(MeasurementValue {
                        size: sizes[k],
                        value: v,
                    });
                    jv.push(json!({"size": sizes[k].0, "value": v}));
                }
            }
            let name = format!("Mesure {i}");
            let mut j = json!({"id": mid, "name": name, "unit": if deg { "deg" } else { "mm" }});
            if !jv.is_empty() {
                j["values"] = json!(jv);
            }
            ms.push(Measurement {
                id: MeasurementId(mid),
                name,
                unit: if deg {
                    MeasurementUnit::Deg
                } else {
                    MeasurementUnit::Mm
                },
                values,
            });
            measures.push(MeasurementId(mid));
            js.push(j);
        }
        p.measurements = ms;
        oracle.insert("measurements".into(), json!(js));
        if rng.chance(70) {
            let (mut cs, mut jc) = (Vec::new(), Vec::new());
            for i in 0..1 + rng.below(3) {
                let cid = alloc(&mut next);
                let (mut values, mut jv) = (Vec::new(), Vec::new());
                // Mesures manquantes : seulement un sous-ensemble (peut être vide).
                for &measurement in &measures {
                    if rng.chance(55) {
                        let v = rng.half(0, 900) / 2.0;
                        values.push(ClientValue {
                            measurement,
                            value: v,
                        });
                        jv.push(json!({"measurement": measurement.0, "value": v}));
                    }
                }
                let name = format!("Fiche {i}");
                let mut j = json!({"id": cid, "name": name});
                if !jv.is_empty() {
                    j["values"] = json!(jv);
                }
                cs.push(Client {
                    id: ClientId(cid),
                    name,
                    values,
                });
                jc.push(j);
            }
            p.clients = cs;
            oracle.insert("clients".into(), json!(jc));
        }
        if rng.chance(60) {
            let (mut al, mut ja) = (Vec::new(), Vec::new());
            for i in 0..1 + rng.below(3) {
                let aid = alloc(&mut next);
                let target = measures[rng.below(measures.len() as u64) as usize];
                let delta = rng.half(-60, 60);
                let name = format!("Retouche {i}");
                ja.push(json!({"id": aid, "name": name, "target": target.0, "delta": delta}));
                al.push(Alteration {
                    id: AlterationId(aid),
                    name,
                    target,
                    delta,
                });
            }
            p.alterations = al;
            oracle.insert("alterations".into(), json!(ja));
        }
        // Mesures finies : cote complète de la feuille si on peut en poser une.
        if rng.chance(60) {
            let s = p.sheets[0].id;
            let pts: Vec<PointId> = p.sheets[0].points.iter().map(|q| q.id).collect();
            if pts.len() >= 2 {
                p.next_id = next;
                let applied = p
                    .apply(Command::AddFullDimension {
                        sheet: s,
                        kind: FullDimensionKind::Horizontal {
                            from: pts[0],
                            to: pts[1],
                            offset_mm: 5.0,
                        },
                        label: None,
                        text_shift_mm: 0.0,
                    })
                    .is_ok();
                // L'identifiant de la cote vient du compteur du projet : on repart de lui.
                next = p.next_id;
                if applied {
                    let dim = p.sheets[0].full_dimensions.last().unwrap().id;
                    let fid = alloc(&mut next);
                    let target = measures[rng.below(measures.len() as u64) as usize];
                    let (tm, tp) = (rng.half(0, 20), rng.half(0, 20));
                    let name = "Finie 0".to_string();
                    p.finished_measures = vec![FinishedMeasure {
                        id: FinishedMeasureId(fid),
                        name: name.clone(),
                        measurement: target,
                        source: FinishedSource::FullDimension {
                            sheet: s,
                            dimension: dim,
                        },
                        tolerance_minus: tm,
                        tolerance_plus: tp,
                    }];
                    oracle.insert(
                        "finished_measures".into(),
                        json!([{"id": fid, "name": name, "measurement": target.0,
                                "source": {"type": "full_dimension", "sheet": s.0, "dimension": dim.0},
                                "tolerance_minus": tm, "tolerance_plus": tp}]),
                    );
                    c.finished_seam_or_dim += 1;
                }
            }
        }
    }
    // Valeurs de variable par taille (expressions entières toujours valides).
    if !sizes.is_empty() && !p.variables.is_empty() && rng.chance(60) {
        let vi = rng.below(p.variables.len() as u64) as usize;
        let mut entries = Vec::new();
        for k in subset(rng, sizes.len()) {
            entries.push(VariableSizeExpr {
                size: sizes[k],
                expr: format!("{}", 1 + rng.below(99)),
            });
        }
        c.per_size_written += entries.len();
        p.variables[vi].per_size = entries;
    }
    // Cran proportionnel : premier cran sans ancrage.
    if rng.chance(50) {
        if let Some(n) = p.sheets[0].notches.iter_mut().find(|n| n.anchor.is_none()) {
            n.proportional = true;
            c.proportional_written += 1;
        }
    }
    p.next_id = p.next_id.max(next);
    p.validate()
        .unwrap_or_else(|e| panic!("projet v5 généré invalide : {e}"));
    Value::Object(oracle)
}

fn expect_refused(doc: &Value, what: &'static str, c: &mut Counters) {
    assert!(
        persist::from_json(&doc.to_string()).is_err(),
        "corruption « {what} » acceptée"
    );
    *c.corruptions.entry(what).or_default() += 1;
}

/// Applique chaque corruption applicable à `good` et vérifie le refus.
fn corruptions(rng: &mut Rng, good: &Value, c: &mut Counters) {
    let has = |k: &str| {
        good.get(k)
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty())
    };
    // Version future et zéro.
    let mut d = good.clone();
    d["schema_version"] = json!(7);
    expect_refused(&d, "version future", c);
    let mut d = good.clone();
    d["schema_version"] = json!(0);
    expect_refused(&d, "version zéro", c);
    // Clé en trop sur un objet v5 au hasard.
    for key in [
        "grading_rules",
        "measurements",
        "clients",
        "alterations",
        "finished_measures",
    ] {
        if has(key) {
            let mut d = good.clone();
            let n = d[key].as_array().unwrap().len();
            d[key][rng.below(n as u64) as usize]["zz"] = json!(1);
            expect_refused(&d, "clé en trop", c);
            // Objet v5 dans un fichier déclaré v4 / v3.
            for v in [4, 3] {
                let mut d = good.clone();
                d["schema_version"] = json!(v);
                expect_refused(&d, "objet v5 dans un fichier plus ancien", c);
            }
            // Identifiant en double avec la feuille.
            let mut d = good.clone();
            d[key][0]["id"] = good["sheets"][0]["id"].clone();
            expect_refused(&d, "identifiant en double", c);
            // Identifiant hors compteur.
            let mut d = good.clone();
            d[key][0]["id"] = good["next_id"].clone();
            expect_refused(&d, "identifiant hors compteur", c);
            // Nom vidé.
            let mut d = good.clone();
            d[key][0]["name"] = json!("");
            expect_refused(&d, "nom vide", c);
        }
    }
    if has("grading_rules") {
        let mut d = good.clone();
        d["grading_rules"][0]["deltas"][0]["size"] = json!(u32::MAX);
        expect_refused(&d, "taille inconnue (règle)", c);
        let mut d = good.clone();
        d["grading_rules"][0]["deltas"][0]["dx_mm"] = json!(2e6);
        expect_refused(&d, "écart hors bornes", c);
        let mut d = good.clone();
        let first = d["grading_rules"][0]["deltas"][0].clone();
        d["grading_rules"][0]["deltas"]
            .as_array_mut()
            .unwrap()
            .push(first);
        expect_refused(&d, "taille en double (règle)", c);
    }
    if has("measurements") {
        let mi = good["measurements"]
            .as_array()
            .unwrap()
            .iter()
            .position(|m| m.get("values").is_some());
        if let Some(mi) = mi {
            let mut d = good.clone();
            d["measurements"][mi]["values"][0]["size"] = json!(u32::MAX);
            expect_refused(&d, "taille inconnue (mesure)", c);
            let mut d = good.clone();
            d["measurements"][mi]["values"][0]["value"] = json!(-1.0);
            expect_refused(&d, "valeur négative", c);
            let mut d = good.clone();
            d["measurements"][mi]["values"][0]["value"] = json!("x");
            expect_refused(&d, "type erroné", c);
        }
        let mut d = good.clone();
        d["measurements"][0]["unit"] = json!("cm");
        expect_refused(&d, "unité inconnue", c);
    }
    if has("clients") {
        if let Some(ci) = good["clients"]
            .as_array()
            .unwrap()
            .iter()
            .position(|m| m.get("values").is_some())
        {
            let mut d = good.clone();
            d["clients"][ci]["values"][0]["measurement"] = json!(u32::MAX);
            expect_refused(&d, "mesure inconnue (fiche)", c);
        }
    }
    if has("alterations") {
        let mut d = good.clone();
        d["alterations"][0]["target"] = json!(u32::MAX);
        expect_refused(&d, "mesure inconnue (retouche)", c);
    }
    if has("finished_measures") {
        let mut d = good.clone();
        d["finished_measures"][0]["measurement"] = json!(u32::MAX);
        expect_refused(&d, "mesure inconnue (mesure finie)", c);
        let mut d = good.clone();
        d["finished_measures"][0]["source"]["dimension"] = json!(u32::MAX);
        expect_refused(&d, "cote inconnue", c);
        let mut d = good.clone();
        d["finished_measures"][0]["tolerance_plus"] = json!(-0.5);
        expect_refused(&d, "tolérance négative", c);
    }
    // per_size.
    let vi = good["variables"]
        .as_array()
        .into_iter()
        .flatten()
        .position(|v| v.get("per_size").is_some());
    if let Some(vi) = vi {
        let mut d = good.clone();
        d["variables"][vi]["per_size"][0]["size"] = json!(u32::MAX);
        expect_refused(&d, "taille inconnue (per_size)", c);
        let mut d = good.clone();
        d["variables"][vi]["per_size"][0]["expr"] = json!("1 / 0");
        expect_refused(&d, "expression invalide (per_size)", c);
        let mut d = good.clone();
        d["variables"][vi]["per_size"][0]["zz"] = json!(1);
        expect_refused(&d, "clé en trop (per_size)", c);
        let mut d = good.clone();
        d["schema_version"] = json!(4);
        expect_refused(&d, "per_size dans un fichier v4", c);
    }
    // proportional.
    let notch = good["sheets"][0]["notches"]
        .as_array()
        .into_iter()
        .flatten()
        .position(|n| n.get("proportional").is_some());
    if let Some(ni) = notch {
        let mut d = good.clone();
        d["sheets"][0]["notches"][ni]["anchor"] = json!({"type": "midpoint"});
        expect_refused(&d, "proportional avec ancrage", c);
        let mut d = good.clone();
        d["sheets"][0]["notches"][ni]["proportional"] = json!(1);
        expect_refused(&d, "proportional non booléen", c);
        let mut d = good.clone();
        d["schema_version"] = json!(4);
        expect_refused(&d, "proportional dans un fichier v4", c);
    }
}

#[test]
fn campagne_adverse_schema_v5_graine_fixe() {
    let mut rng = Rng(0x5EED_0334_0005_0001);
    let mut c = Counters::default();
    for n in 0..SEQUENCES {
        c.sequences += 1;
        let p = random_project(&mut rng, n);
        let v5 = persist::to_json(&p).unwrap();
        let v4 = v4_text(&v5);
        // I1 : aucune perte (structure identique au projet de départ).
        let migrated = persist::from_json(&v4).unwrap_or_else(|e| panic!("#{n} : {e}"));
        assert_eq!(migrated, p, "#{n} : perte à la migration");
        assert_eq!(migrated.schema_version, 6);
        c.migrated += 1;
        c.with_sizes += usize::from(!p.sizes.is_empty());
        c.with_gradings += usize::from(!p.sheets[0].gradings.is_empty());
        c.with_variables += usize::from(!p.variables.is_empty());
        // I2 : mêmes octets, une seule ligne change.
        let out = persist::to_json(&migrated).unwrap();
        assert_eq!(out, v5, "#{n} : octets");
        c.bytes_checked += out.len();
        let diff = differing_lines(&v4, &out);
        assert_eq!(
            diff,
            vec![("  \"schema_version\": 4,", "  \"schema_version\": 6,")],
            "#{n}"
        );
        c.one_line_diff += 1;
        // Aucune clé v5 sans donnée v5.
        for key in [
            "grading_rules",
            "measurements",
            "clients",
            "alterations",
            "finished_measures",
            "per_size",
            "proportional",
        ] {
            assert!(!out.contains(key), "#{n} : « {key} » écrit sans donnée");
        }
        // I3 : idempotence (migrer le résultat ne change rien, même relabellisé v4).
        let again = persist::to_json(&persist::from_json(&out).unwrap()).unwrap();
        assert_eq!(again, out, "#{n} : idempotence");
        assert_eq!(
            persist::to_json(&persist::from_json(&v4_text(&out)).unwrap()).unwrap(),
            out
        );
        c.idempotent += 1;
        // I4 : v5 -> Value -> v5 (même projet ; octets recalculés identiques).
        let value: Value = serde_json::from_str(&out).unwrap();
        let back = persist::from_json(&value.to_string()).unwrap();
        assert_eq!(back, p, "#{n} : aller-retour par Value");
        assert_eq!(persist::to_json(&back).unwrap(), out);
        c.value_roundtrip += 1;

        // Objets v5 : écriture, oracle indépendant, relecture identique.
        let mut q = p.clone();
        let oracle = add_v5_objects(&mut rng, &mut q, &mut c);
        let text = persist::to_json(&q).unwrap();
        let doc: Value = serde_json::from_str(&text).unwrap();
        for (key, expected) in oracle.as_object().unwrap() {
            assert_eq!(&doc[key.as_str()], expected, "#{n} : oracle « {key} »");
            c.v5_oracle_checked += 1;
        }
        for key in [
            "grading_rules",
            "measurements",
            "clients",
            "alterations",
            "finished_measures",
        ] {
            assert_eq!(
                doc.get(key).is_some(),
                oracle.get(key).is_some(),
                "#{n} : « {key} » écrit sans donnée ou perdu"
            );
        }
        let reread = persist::from_json(&text).unwrap();
        assert_eq!(reread, q, "#{n} : relecture v5");
        assert_eq!(persist::to_json(&reread).unwrap(), text, "#{n} : octets v5");
        c.v5_objects += oracle.as_object().unwrap().len();
        corruptions(&mut rng, &doc, &mut c);
    }
    println!(
        "campagne v5 : {} séquences, {} migrées, {} octets comparés, {} objets v5 contrôlés, corruptions {:?}",
        c.sequences, c.migrated, c.bytes_checked, c.v5_oracle_checked, c.corruptions
    );
    // Les contrôles forts ont réellement tourné.
    assert!(c.sequences >= 150);
    assert_eq!(c.migrated, SEQUENCES);
    assert_eq!(c.one_line_diff, SEQUENCES);
    assert_eq!(c.idempotent, SEQUENCES);
    assert_eq!(c.value_roundtrip, SEQUENCES);
    assert!(c.with_sizes >= 100, "gammes : {}", c.with_sizes);
    assert!(c.with_gradings >= 20, "écarts : {}", c.with_gradings);
    assert!(c.with_variables >= 100, "variables : {}", c.with_variables);
    assert!(
        c.v5_oracle_checked >= 200,
        "oracle : {}",
        c.v5_oracle_checked
    );
    assert!(
        c.per_size_written >= 40,
        "per_size : {}",
        c.per_size_written
    );
    assert!(
        c.proportional_written >= 20,
        "proportional : {}",
        c.proportional_written
    );
    assert!(
        c.finished_seam_or_dim >= 10,
        "mesures finies : {}",
        c.finished_seam_or_dim
    );
    let total: usize = c.corruptions.values().sum();
    assert!(total >= 1500, "corruptions refusées : {total}");
    for kind in [
        "version future",
        "clé en trop",
        "objet v5 dans un fichier plus ancien",
        "identifiant en double",
        "identifiant hors compteur",
        "nom vide",
        "taille inconnue (règle)",
        "taille inconnue (mesure)",
        "mesure inconnue (fiche)",
        "mesure inconnue (retouche)",
        "mesure inconnue (mesure finie)",
        "cote inconnue",
        "taille inconnue (per_size)",
        "proportional avec ancrage",
        "per_size dans un fichier v4",
        "proportional dans un fichier v4",
        "valeur négative",
        "écart hors bornes",
    ] {
        assert!(
            c.corruptions.get(kind).copied().unwrap_or(0) >= 5,
            "corruption « {kind} » trop rare : {:?}",
            c.corruptions.get(kind)
        );
    }
}

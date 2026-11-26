//! Décision 334 — schéma v5 : migration unique v4 → v5 et stockage posé (règles de gradation
//! nommées, barème, fiches client, retouches, mesures finies, valeurs de variable par taille, cran
//! proportionnel). Types, validation et format seulement : aucune commande.
//!
//! Les valeurs attendues sont écrites à la main ; le JSON est relu par `serde_json::Value` et par
//! un découpage de lignes, jamais par le code de sérialisation testé.

use pf_core::persist;
use pf_core::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn tests_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests")
}

/// (nom, texte) de tous les fichiers de `fixtures_v4/`.
fn fixtures_v4() -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = std::fs::read_dir(tests_dir().join("fixtures_v4"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .map(|p| {
            (
                p.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read_to_string(&p).unwrap(),
            )
        })
        .collect();
    v.sort();
    v
}

/// Texte v4 -> texte v5 attendu : **la seule** ligne `"schema_version": 4` change.
fn expected_v5(v4: &str) -> String {
    assert_eq!(v4.matches("\"schema_version\": 4,").count(), 1);
    v4.replacen("\"schema_version\": 4,", "\"schema_version\": 6,", 1)
}

/// Clés de premier niveau d'un fichier, dans l'ordre d'écriture (lecture des lignes indentées de
/// deux espaces, sans passer par le sérialiseur).
fn top_level_keys(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.strip_prefix("  \""))
        .filter_map(|l| l.split_once("\":").map(|(k, _)| k.to_string()))
        .collect()
}

// ---------------------------------------------------------------------------
// Migration des fichiers v4 du dépôt
// ---------------------------------------------------------------------------

#[test]
fn ca1_chaque_fichier_v4_migre_sans_perte_avec_les_octets_v4_et_la_version_changee() {
    let files = fixtures_v4();
    assert!(files.len() >= 6, "les fixtures v4 ont disparu");
    for (name, text) in &files {
        let text = text.replace("\r\n", "\n");
        let p = persist::from_json(&text).unwrap_or_else(|e| panic!("{name} : {e}"));
        assert_eq!(p.schema_version, 6, "{name}");
        let out = persist::to_json(&p).unwrap();
        assert_eq!(out, expected_v5(&text), "{name} : octets attendus");
        // Lecteur indépendant : mêmes données sauf `schema_version`.
        let (a, b): (Value, Value) = (
            serde_json::from_str(&text).unwrap(),
            serde_json::from_str(&out).unwrap(),
        );
        let (mut a, b) = (a, b);
        a["schema_version"] = json!(6);
        assert_eq!(a, b, "{name} : perte ou ajout");
        // Aucun objet v5 n'apparaît.
        for key in [
            "grading_rules",
            "measurements",
            "clients",
            "alterations",
            "finished_measures",
            "per_size",
            "proportional",
        ] {
            assert!(!out.contains(key), "{name} : « {key} » apparu");
        }
    }
}

#[test]
fn ca1_la_fixture_riche_porte_bien_gamme_ecarts_variables_formules_et_pieces() {
    let (_, text) = fixtures_v4()
        .into_iter()
        .find(|(n, _)| n.contains("gamme-ecarts-variables-formules-pieces"))
        .expect("fixture riche");
    let v: Value = serde_json::from_str(&text).unwrap();
    assert!(v["sizes"][0]["sizes"].as_array().unwrap().len() >= 4);
    assert!(!v["variables"].as_array().unwrap().is_empty());
    assert!(!v["pieces"].as_array().unwrap().is_empty());
    let s0 = &v["sheets"][0];
    assert!(!s0["gradings"].as_array().unwrap().is_empty());
    assert!(!s0["formulas"].as_array().unwrap().is_empty());
}

#[test]
fn ca2_les_exemples_v5_sont_les_fichiers_v4_avec_la_seule_version_changee() {
    let mut n = 0;
    for entry in std::fs::read_dir(tests_dir().join("../../../examples")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !(name.ends_with(".pforge") || name.ends_with(".pforge.json")) {
            continue;
        }
        n += 1;
        let v5 = std::fs::read_to_string(&path).unwrap();
        let v4 = std::fs::read_to_string(tests_dir().join(format!("fixtures_v4/v4-{name}")))
            .unwrap_or_else(|_| panic!("{name} : fixture v4 absente"));
        assert_eq!(v5, expected_v5(&v4), "{name}");
    }
    assert_eq!(n, 5, "cinq exemples attendus");
}

#[test]
fn ca3_migration_idempotente_et_aller_retour() {
    for (name, text) in fixtures_v4() {
        let p = persist::from_json(&text).unwrap();
        let b1 = persist::to_json(&p).unwrap();
        let p2 = persist::from_json(&b1).unwrap();
        assert_eq!(p2, p, "{name} : relecture");
        assert_eq!(persist::to_json(&p2).unwrap(), b1, "{name} : idempotence");
        // v5 -> étiquette v4 -> relu : même projet (la migration ne change rien d'autre).
        let relabeled = b1.replacen("\"schema_version\": 6,", "\"schema_version\": 4,", 1);
        assert_eq!(persist::from_json(&relabeled).unwrap(), p, "{name}");
    }
}

#[test]
fn ca4_projet_v5_sans_objet_nouveau_ecrit_les_memes_octets_qu_en_v4_hors_version() {
    let mut p = Project::new("Gamme");
    p.apply(Command::AddSizeRange {
        name: "Femme".into(),
        kind: SizeKind::Alphanumeric,
        labels: ["S", "M", "L"].map(String::from).to_vec(),
        base: 1,
    })
    .unwrap();
    p.apply(Command::AddVariable {
        name: "tour".into(),
        unit: VariableUnit::Mm,
        expr: "700".into(),
    })
    .unwrap();
    let v5 = persist::to_json(&p).unwrap();
    let as_v4 = v5.replacen("\"schema_version\": 6,", "\"schema_version\": 4,", 1);
    assert_ne!(v5, as_v4);
    let reopened = persist::from_json(&as_v4).unwrap();
    assert_eq!(persist::to_json(&reopened).unwrap(), v5);
    for key in ["per_size", "grading_rules", "measurements", "clients"] {
        assert!(!v5.contains(key), "{key}");
    }
    // Clés de premier niveau : aucune clé v5 n'est écrite quand elle est vide.
    assert_eq!(
        top_level_keys(&v5),
        [
            "format",
            "schema_version",
            "name",
            "next_id",
            "sheets",
            "variables",
            "sizes"
        ]
    );
}

// ---------------------------------------------------------------------------
// Objets v5 : lecture exacte, relecture identique, ordre d'écriture
// ---------------------------------------------------------------------------

/// Document v5 écrit à la main, tous objets présents. Ids : feuille 1, points 2-3, tracé 4,
/// cran 5, cote 16, gamme 6 (tailles 7 S, 8 M base, 9 L), règles 10-11, mesures 12-13, fiche 14,
/// retouche 15, mesures finies 17-18 ; `next_id` 30.
fn doc() -> Value {
    json!({
        "format": "patternforge",
        "schema_version": 5,
        "name": "Projet v5",
        "next_id": 30,
        "sheets": [{
            "id": 1,
            "name": "Feuille 1",
            "points": [
                {"id": 2, "label": "A", "definition": {"type": "free", "x": 0.0, "y": 0.0}, "role": "construction"},
                {"id": 3, "label": "B", "definition": {"type": "free", "x": 100.0, "y": 0.0}, "role": "construction"}
            ],
            "entities": [
                {"id": 4, "role": "cutting_contour", "kind": {"type": "segment", "from": 2, "to": 3}}
            ],
            "notches": [
                {"id": 5, "entity": 4, "distance_mm": 10.0, "side": "right", "length_mm": 5.0,
                 "style": "single", "proportional": true}
            ],
            "full_dimensions": [
                {"id": 16, "kind": {"type": "horizontal", "from": 2, "to": 3, "offset_mm": 10.0}}
            ]
        }],
        "variables": [
            {"name": "A", "unit": "mm", "expr": "10",
             "per_size": [{"size": 7, "expr": "8"}, {"size": 9, "expr": "12"}]},
            {"name": "B", "unit": "mm", "expr": "A * 2"}
        ],
        "sizes": [{
            "id": 6, "name": "Femme", "kind": "alphanumeric", "base": 8,
            "sizes": [{"id": 7, "label": "S"}, {"id": 8, "label": "M"}, {"id": 9, "label": "L"}]
        }],
        "grading_rules": [
            {"id": 10, "name": "Hanche", "mode": "mirror_x",
             "deltas": [{"size": 7, "dx_mm": -5.0, "dy_mm": -10.0}, {"size": 9, "dx_mm": 5.0, "dy_mm": 10.0}]},
            {"id": 11, "name": "Épaule", "deltas": [{"size": 9, "dx_mm": 2.5, "dy_mm": 0.0}]}
        ],
        "measurements": [
            {"id": 12, "name": "Tour de taille", "unit": "mm",
             "values": [{"size": 7, "value": 640.0}, {"size": 8, "value": 680.0}, {"size": 9, "value": 720.0}]},
            {"id": 13, "name": "Pente d'épaule", "unit": "deg"}
        ],
        "clients": [
            {"id": 14, "name": "Fiche A", "values": [{"measurement": 12, "value": 701.5}]}
        ],
        "alterations": [
            {"id": 15, "name": "Manche +15", "target": 12, "delta": 15.0}
        ],
        "finished_measures": [
            {"id": 17, "name": "Largeur finie", "measurement": 12,
             "source": {"type": "full_dimension", "sheet": 1, "dimension": 16},
             "tolerance_minus": 5.0, "tolerance_plus": 10.0}
        ]
    })
}

fn load(doc: &Value) -> Result<Project, CoreError> {
    persist::from_json(&doc.to_string())
}

fn refused(doc: &Value, fragment: &str) {
    let err = load(doc).expect_err(fragment).to_string();
    assert!(
        err.contains(fragment),
        "message attendu avec « {fragment} », obtenu « {err} »"
    );
}

#[test]
fn ca5_objets_v5_lus_a_la_valeur_pres() {
    let p = load(&doc()).unwrap();
    assert_eq!(p.schema_version, 6);
    assert_eq!(p.grading_rules.len(), 2);
    let r = &p.grading_rules[0];
    assert_eq!(
        (r.id, r.name.as_str(), r.mode),
        (GradingRuleId(10), "Hanche", GradingCopyMode::MirrorX)
    );
    assert_eq!(
        r.deltas,
        vec![
            GradingRuleDelta {
                size: SizeId(7),
                dx_mm: -5.0,
                dy_mm: -10.0
            },
            GradingRuleDelta {
                size: SizeId(9),
                dx_mm: 5.0,
                dy_mm: 10.0
            },
        ]
    );
    assert_eq!(p.grading_rules[1].mode, GradingCopyMode::Same);
    assert_eq!(p.measurements[0].unit, MeasurementUnit::Mm);
    assert_eq!(
        p.measurements[0].values[2],
        MeasurementValue {
            size: SizeId(9),
            value: 720.0
        }
    );
    assert_eq!(p.measurements[1].unit, MeasurementUnit::Deg);
    assert!(p.measurements[1].values.is_empty(), "mesure non renseignée");
    assert_eq!(
        p.clients[0].values,
        vec![ClientValue {
            measurement: MeasurementId(12),
            value: 701.5
        }]
    );
    assert_eq!(
        (p.alterations[0].target, p.alterations[0].delta),
        (MeasurementId(12), 15.0)
    );
    let f = &p.finished_measures[0];
    assert_eq!(
        f.source,
        FinishedSource::FullDimension {
            sheet: SheetId(1),
            dimension: FullDimensionId(16)
        }
    );
    assert_eq!((f.tolerance_minus, f.tolerance_plus), (5.0, 10.0));
    assert_eq!(
        p.variables[0].per_size,
        vec![
            VariableSizeExpr {
                size: SizeId(7),
                expr: "8".into()
            },
            VariableSizeExpr {
                size: SizeId(9),
                expr: "12".into()
            },
        ]
    );
    assert!(p.variables[1].per_size.is_empty());
    assert!(p.sheets[0].notches[0].proportional);
}

#[test]
fn ca5_objets_v5_relus_identiques_et_ecrits_avec_l_ordre_attendu() {
    let input = doc();
    let p = load(&input).unwrap();
    let text = persist::to_json(&p).unwrap();
    // Lecteur indépendant : les données écrites sont exactement celles du document d'entrée.
    let mut expected = input.clone();
    expected["schema_version"] = json!(6); // le document d'entrée est en v5 : relu puis écrit en v6
    assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), expected);
    assert_eq!(load(&serde_json::from_str(&text).unwrap()).unwrap(), p);
    assert_eq!(persist::to_json(&p).unwrap(), text, "déterminisme");
    // Ordre des clés de premier niveau : v4 d'abord, puis les objets v5 dans l'ordre du § 4.10.
    assert_eq!(
        top_level_keys(&text),
        [
            "format",
            "schema_version",
            "name",
            "next_id",
            "sheets",
            "variables",
            "sizes",
            "grading_rules",
            "measurements",
            "clients",
            "alterations",
            "finished_measures"
        ]
    );
}

#[test]
fn ca5_une_mesure_sans_valeur_et_une_fiche_sans_valeur_restent_absentes_a_l_ecriture() {
    let mut d = doc();
    d["clients"][0].as_object_mut().unwrap().remove("values");
    let p = load(&d).unwrap();
    assert!(p.clients[0].values.is_empty());
    let text = persist::to_json(&p).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    assert!(
        v["clients"][0].get("values").is_none(),
        "jamais de valeur devinée ni de clé vide"
    );
    assert!(v["measurements"][1].get("values").is_none());
}

#[test]
fn ca5_les_cles_inconnues_du_projet_sont_toujours_conservees() {
    let mut d = doc();
    d["zz_projet"] = json!({"a": [1, 2]});
    let p = load(&d).unwrap();
    let out: Value = serde_json::from_str(&persist::to_json(&p).unwrap()).unwrap();
    assert_eq!(out["zz_projet"], d["zz_projet"]);
}

// ---------------------------------------------------------------------------
// Versions
// ---------------------------------------------------------------------------

#[test]
fn ca6_version_future_refusee_avec_message_clair() {
    let mut d = doc();
    d["schema_version"] = json!(7);
    let err = load(&d).unwrap_err();
    assert_eq!(
        err,
        CoreError::UnsupportedVersion {
            found: 7,
            supported: 6
        }
    );
    let msg = err.to_string();
    assert!(
        msg.contains("version 7") && msg.contains("plus récent"),
        "{msg}"
    );
    for bad in [
        json!(0),
        json!(8),
        json!(u32::MAX),
        json!("5"),
        json!(5.5),
        json!(-1),
        Value::Null,
    ] {
        let mut d = doc();
        d["schema_version"] = bad.clone();
        assert!(load(&d).is_err(), "{bad}");
    }
}

#[test]
fn ca6_objets_v5_dans_un_fichier_declare_v4_ou_plus_ancien_sont_refuses() {
    let good = doc();
    let mut plain = good.clone();
    for k in [
        "grading_rules",
        "measurements",
        "clients",
        "alterations",
        "finished_measures",
    ] {
        plain.as_object_mut().unwrap().remove(k);
    }
    plain["variables"][0]
        .as_object_mut()
        .unwrap()
        .remove("per_size");
    plain["sheets"][0]["notches"][0]
        .as_object_mut()
        .unwrap()
        .remove("proportional");
    plain["schema_version"] = json!(4);
    load(&plain).expect("sans objet v5, le fichier v4 s'ouvre");
    let mut carriers: Vec<(&str, Value)> = Vec::new();
    for k in [
        "grading_rules",
        "measurements",
        "clients",
        "alterations",
        "finished_measures",
    ] {
        let mut d = plain.clone();
        d[k] = good[k].clone();
        carriers.push((k, d));
    }
    let mut d = plain.clone();
    d["variables"][0]["per_size"] = good["variables"][0]["per_size"].clone();
    carriers.push(("per_size", d));
    let mut d = plain.clone();
    d["sheets"][0]["notches"][0]["proportional"] = json!(true);
    carriers.push(("proportional", d));
    for (key, d) in carriers {
        refused(
            &d,
            &format!("la clé « {key} » est réservée au format version 5"),
        );
        // Même vide (tableau vide) pour les clés de projet : rien n'est jeté en silence.
        let mut older = d.clone();
        older["schema_version"] = json!(3);
        older.as_object_mut().unwrap().remove("sizes");
        refused(&older, "réservée");
    }
    let mut empty = plain.clone();
    empty["clients"] = json!([]);
    refused(&empty, "« clients » est réservée");
}

// ---------------------------------------------------------------------------
// Chargeur strict
// ---------------------------------------------------------------------------

#[test]
fn ca7_cle_en_trop_refusee_sur_chaque_objet_v5() {
    for (path, what) in [
        (vec!["grading_rules", "0"], "règle"),
        (vec!["grading_rules", "0", "deltas", "0"], "écart de règle"),
        (vec!["measurements", "0"], "mesure"),
        (vec!["measurements", "0", "values", "0"], "valeur de mesure"),
        (vec!["clients", "0"], "fiche"),
        (vec!["clients", "0", "values", "0"], "valeur de fiche"),
        (vec!["alterations", "0"], "retouche"),
        (vec!["finished_measures", "0"], "mesure finie"),
        (vec!["finished_measures", "0", "source"], "source"),
        (vec!["variables", "0", "per_size", "0"], "valeur par taille"),
        (vec!["sheets", "0", "notches", "0"], "cran"),
    ] {
        let mut d = doc();
        let mut at = &mut d;
        for step in &path {
            at = match step.parse::<usize>() {
                Ok(i) => &mut at[i],
                Err(_) => &mut at[*step],
            };
        }
        at["zz"] = json!(1);
        refused(&d, "unknown field `zz`");
        let _ = what;
    }
}

#[test]
fn ca7_type_errone_refuse() {
    type Case = (&'static str, Box<dyn Fn(&mut Value)>, &'static str);
    let cases: Vec<Case> = vec![
        (
            "valeur texte",
            Box::new(|d| d["measurements"][0]["values"][0]["value"] = json!("640")),
            "invalid type",
        ),
        (
            "nom nombre",
            Box::new(|d| d["grading_rules"][0]["name"] = json!(5)),
            "invalid type",
        ),
        (
            "deltas objet",
            Box::new(|d| d["grading_rules"][0]["deltas"] = json!({})),
            "invalid type",
        ),
        (
            "taille texte",
            Box::new(|d| d["grading_rules"][0]["deltas"][0]["size"] = json!("7")),
            "invalid type",
        ),
        (
            "mode inconnu",
            Box::new(|d| d["grading_rules"][0]["mode"] = json!("rotation")),
            "unknown variant",
        ),
        (
            "unité inconnue",
            Box::new(|d| d["measurements"][0]["unit"] = json!("cm")),
            "unknown variant",
        ),
        (
            "source inconnue",
            Box::new(|d| d["finished_measures"][0]["source"]["type"] = json!("piece")),
            "unknown variant",
        ),
        (
            "proportional texte",
            Box::new(|d| d["sheets"][0]["notches"][0]["proportional"] = json!("oui")),
            "invalid type",
        ),
        (
            "per_size texte",
            Box::new(|d| d["variables"][0]["per_size"] = json!("8")),
            "invalid type",
        ),
        (
            "tolérance texte",
            Box::new(|d| d["finished_measures"][0]["tolerance_plus"] = json!("1")),
            "invalid type",
        ),
        (
            "champ requis absent",
            Box::new(|d| {
                d["alterations"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("target");
            }),
            "missing field",
        ),
        (
            "deltas absent",
            Box::new(|d| {
                d["grading_rules"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("deltas");
            }),
            "missing field",
        ),
        (
            "source absente",
            Box::new(|d| {
                d["finished_measures"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("source");
            }),
            "missing field",
        ),
    ];
    for (name, edit, fragment) in cases {
        let mut d = doc();
        edit(&mut d);
        let err = load(&d).expect_err(name).to_string();
        assert!(err.contains(fragment), "{name} : {err}");
    }
}

#[test]
fn ca7_references_inconnues_refusees_avec_message() {
    type Case = (&'static str, Box<dyn Fn(&mut Value)>, &'static str);
    let cases: Vec<Case> = vec![
        (
            "taille de règle",
            Box::new(|d| d["grading_rules"][0]["deltas"][0]["size"] = json!(99)),
            "taille absente",
        ),
        (
            "taille de mesure",
            Box::new(|d| d["measurements"][0]["values"][0]["size"] = json!(99)),
            "taille absente",
        ),
        (
            "taille de per_size",
            Box::new(|d| d["variables"][0]["per_size"][0]["size"] = json!(99)),
            "taille absente",
        ),
        (
            "taille = id de gamme",
            Box::new(|d| d["variables"][0]["per_size"][0]["size"] = json!(6)),
            "taille absente",
        ),
        (
            "taille = id de point",
            Box::new(|d| d["measurements"][0]["values"][0]["size"] = json!(2)),
            "taille absente",
        ),
        (
            "mesure de fiche",
            Box::new(|d| d["clients"][0]["values"][0]["measurement"] = json!(99)),
            "mesure absente",
        ),
        (
            "mesure = id de taille",
            Box::new(|d| d["clients"][0]["values"][0]["measurement"] = json!(7)),
            "mesure absente",
        ),
        (
            "cible de retouche",
            Box::new(|d| d["alterations"][0]["target"] = json!(99)),
            "mesure absente",
        ),
        (
            "mesure finie",
            Box::new(|d| d["finished_measures"][0]["measurement"] = json!(99)),
            "mesure absente",
        ),
        (
            "couture",
            Box::new(|d| d["finished_measures"][0]["source"] = json!({"type": "seam", "seam": 99})),
            "couture absente",
        ),
        (
            "cote",
            Box::new(|d| d["finished_measures"][0]["source"]["dimension"] = json!(99)),
            "cote absente",
        ),
        (
            "feuille de la cote",
            Box::new(|d| d["finished_measures"][0]["source"]["sheet"] = json!(99)),
            "cote absente",
        ),
    ];
    for (name, edit, fragment) in cases {
        let mut d = doc();
        edit(&mut d);
        let err = load(&d).expect_err(name);
        assert!(
            matches!(err, CoreError::DeadReference(_)),
            "{name} : {err:?}"
        );
        assert!(err.to_string().contains(fragment), "{name} : {err}");
    }
    // Rangs : une taille est désignée par son id, jamais par son rang (0, 1, 2 n'existent pas).
    for rank in [0, 1, 2] {
        let mut d = doc();
        d["measurements"][0]["values"][0]["size"] = json!(rank);
        assert!(load(&d).is_err(), "rang {rank}");
    }
}

#[test]
fn ca7_identifiants_doublons_et_compteur() {
    for (a, b) in [
        (["alterations", "0"], ["clients", "0"]),
        (["grading_rules", "1"], ["grading_rules", "0"]),
        (["measurements", "1"], ["measurements", "0"]),
        (["finished_measures", "0"], ["sizes", "0"]),
    ] {
        let mut d = doc();
        let id = d[b[0]][b[1].parse::<usize>().unwrap()]["id"].clone();
        d[a[0]][a[1].parse::<usize>().unwrap()]["id"] = id;
        let err = load(&d).expect_err(&format!("{a:?}"));
        assert!(matches!(err, CoreError::DuplicateId(_)), "{a:?} : {err:?}");
    }
    // Identifiant au-delà du compteur, ou nul.
    for bad in [30, 31, 0] {
        let mut d = doc();
        d["alterations"][0]["id"] = json!(bad);
        refused(&d, "identifiant");
    }
}

#[test]
fn ca7_doublons_de_contenu_refuses() {
    type Case = (&'static str, Box<dyn Fn(&mut Value)>, &'static str);
    let cases: Vec<Case> = vec![
        (
            "taille deux fois dans une règle",
            Box::new(|d| d["grading_rules"][0]["deltas"][1]["size"] = json!(7)),
            "deux écarts",
        ),
        (
            "taille deux fois dans une mesure",
            Box::new(|d| d["measurements"][0]["values"][1]["size"] = json!(7)),
            "deux valeurs",
        ),
        (
            "mesure deux fois dans une fiche",
            Box::new(|d| {
                d["clients"][0]["values"] =
                    json!([{"measurement": 12, "value": 1.0}, {"measurement": 12, "value": 2.0}])
            }),
            "deux valeurs",
        ),
        (
            "taille deux fois dans per_size",
            Box::new(|d| d["variables"][0]["per_size"][1]["size"] = json!(7)),
            "deux valeurs",
        ),
        (
            "nom de règle en double",
            Box::new(|d| d["grading_rules"][1]["name"] = json!("hanche")),
            "casse ignorée",
        ),
        (
            "nom de mesure en double",
            Box::new(|d| d["measurements"][1]["name"] = json!("TOUR DE TAILLE")),
            "casse ignorée",
        ),
    ];
    for (name, edit, fragment) in cases {
        let mut d = doc();
        edit(&mut d);
        refused_named(&d, name, fragment);
    }
}

fn refused_named(d: &Value, name: &str, fragment: &str) {
    let err = load(d).expect_err(name).to_string();
    assert!(err.contains(fragment), "{name} : {err}");
}

#[test]
fn ca7_valeurs_non_finies_hors_bornes_ou_negatives_refusees() {
    // JSON ne porte ni NaN ni infini : on les pose en Rust, la validation et l'écriture refusent.
    let base = load(&doc()).unwrap();
    type Edit = Box<dyn Fn(&mut Project, f64)>;
    let edits: Vec<(&str, Edit)> = vec![
        (
            "écart de règle",
            Box::new(|p, v| p.grading_rules[0].deltas[0].dx_mm = v),
        ),
        (
            "valeur de mesure",
            Box::new(|p, v| p.measurements[0].values[0].value = v),
        ),
        (
            "valeur de fiche",
            Box::new(|p, v| p.clients[0].values[0].value = v),
        ),
        (
            "delta de retouche",
            Box::new(|p, v| p.alterations[0].delta = v),
        ),
        (
            "tolérance moins",
            Box::new(|p, v| p.finished_measures[0].tolerance_minus = v),
        ),
        (
            "tolérance plus",
            Box::new(|p, v| p.finished_measures[0].tolerance_plus = v),
        ),
    ];
    for (name, edit) in &edits {
        for v in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut p = base.clone();
            edit(&mut p, v);
            assert!(p.validate().is_err(), "{name} {v}");
            assert!(persist::to_json(&p).is_err(), "{name} {v} écrit");
        }
    }
    // Hors bornes et négatives, par le fichier.
    type Case = (&'static str, Box<dyn Fn(&mut Value)>);
    let cases: Vec<Case> = vec![
        (
            "écart 1e300",
            Box::new(|d| d["grading_rules"][0]["deltas"][0]["dx_mm"] = json!(1e300)),
        ),
        (
            "écart 1000001",
            Box::new(|d| d["grading_rules"][0]["deltas"][0]["dy_mm"] = json!(1_000_001.0)),
        ),
        (
            "mesure négative",
            Box::new(|d| d["measurements"][0]["values"][0]["value"] = json!(-1.0)),
        ),
        (
            "mesure 1e300",
            Box::new(|d| d["measurements"][0]["values"][0]["value"] = json!(1e300)),
        ),
        (
            "fiche négative",
            Box::new(|d| d["clients"][0]["values"][0]["value"] = json!(-0.5)),
        ),
        (
            "delta 1e300",
            Box::new(|d| d["alterations"][0]["delta"] = json!(1e300)),
        ),
        (
            "tolérance négative",
            Box::new(|d| d["finished_measures"][0]["tolerance_minus"] = json!(-1.0)),
        ),
    ];
    for (name, edit) in cases {
        let mut d = doc();
        edit(&mut d);
        assert!(load(&d).is_err(), "{name}");
    }
    // Bornes exactes acceptées ; un delta négatif de retouche l'est aussi.
    let mut d = doc();
    d["grading_rules"][0]["deltas"][0]["dx_mm"] = json!(-1_000_000.0);
    d["measurements"][0]["values"][0]["value"] = json!(0.0);
    d["alterations"][0]["delta"] = json!(-15.0);
    assert!(load(&d).is_ok());
}

#[test]
fn ca7_noms_invalides_refuses() {
    for bad in ["", "   ", " x", "x ", "a\u{0}b", "a\tb", &"n".repeat(121)] {
        for path in [
            "grading_rules",
            "measurements",
            "clients",
            "alterations",
            "finished_measures",
        ] {
            let mut d = doc();
            d[path][0]["name"] = json!(bad);
            assert!(load(&d).is_err(), "{path} nom « {bad:?} »");
        }
    }
    let mut d = doc();
    d["grading_rules"][0]["name"] = json!("n".repeat(120));
    assert!(load(&d).is_ok(), "120 caractères acceptés");
}

#[test]
fn ca7_regle_sans_ecart_et_limites_de_nombre_refusees() {
    let mut d = doc();
    d["grading_rules"][0]["deltas"] = json!([]);
    refused(&d, "aucun écart");
    // Trop de règles (201) : refusé.
    let rules: Vec<Value> = (0..201)
        .map(|i| json!({"id": 100 + i, "name": format!("r{i}"), "deltas": [{"size": 7, "dx_mm": 1.0, "dy_mm": 1.0}]}))
        .collect();
    let mut d = doc();
    d["grading_rules"] = json!(rules);
    d["next_id"] = json!(1000);
    refused(&d, "trop de règles");
    // 200 : accepté.
    let mut d = doc();
    d["grading_rules"] = json!(&rules[..200]);
    d["next_id"] = json!(1000);
    assert!(load(&d).is_ok());
}

#[test]
fn ca7_cran_proportionnel_refuse_avec_un_ancrage() {
    let mut d = doc();
    d["sheets"][0]["notches"][0]["anchor"] = json!({"type": "midpoint"});
    refused(&d, "proportional");
    let mut ok = doc();
    ok["sheets"][0]["notches"][0]
        .as_object_mut()
        .unwrap()
        .remove("proportional");
    ok["sheets"][0]["notches"][0]["anchor"] = json!({"type": "midpoint"});
    assert!(load(&ok).is_ok(), "ancré sans proportional : accepté");
    // `false` explicite relu, mais écrit sans la clé (octets canoniques).
    let mut f = doc();
    f["sheets"][0]["notches"][0]["proportional"] = json!(false);
    let text = persist::to_json(&load(&f).unwrap()).unwrap();
    assert!(!text.contains("proportional"));
}

#[test]
fn ca7_valeurs_par_taille_de_variable_validees_par_taille() {
    type Case = (&'static str, Box<dyn Fn(&mut Value)>);
    let cases: Vec<Case> = vec![
        (
            "syntaxe",
            Box::new(|d| d["variables"][0]["per_size"][0]["expr"] = json!("1 +")),
        ),
        (
            "division par zéro",
            Box::new(|d| d["variables"][0]["per_size"][0]["expr"] = json!("1 / 0")),
        ),
        (
            "variable inconnue",
            Box::new(|d| d["variables"][0]["per_size"][0]["expr"] = json!("inconnue + 1")),
        ),
        (
            "auto-référence",
            Box::new(|d| d["variables"][0]["per_size"][0]["expr"] = json!("A + 1")),
        ),
        (
            "cycle dans une taille",
            Box::new(|d| {
                d["variables"][0]["per_size"][0]["expr"] = json!("B");
            }),
        ),
        (
            "vide",
            Box::new(|d| d["variables"][0]["per_size"][0]["expr"] = json!("")),
        ),
        (
            "trop long",
            Box::new(|d| d["variables"][0]["per_size"][0]["expr"] = json!("1+".repeat(300) + "1")),
        ),
    ];
    for (name, edit) in cases {
        let mut d = doc();
        edit(&mut d);
        assert!(load(&d).is_err(), "{name}");
    }
    // Une formule valide dans une taille seulement, citant une autre variable : acceptée.
    let mut d = doc();
    d["variables"][0]["per_size"][0]["expr"] = json!("B / 2 + 1");
    d["variables"][1]["expr"] = json!("7");
    assert!(load(&d).is_ok());
    // Valeur de base inchangée par `per_size`.
    let p = load(&doc()).unwrap();
    assert_eq!(p.variable_values().unwrap()[0], ("A".to_string(), 10.0));
}

#[test]
fn ca8_limite_declaree_les_commandes_existantes_refusent_sans_trace_ce_que_v5_reference() {
    let mut d = doc();
    d["variables"][0]["per_size"][0]["expr"] = json!("B / 2 + 1");
    d["variables"][1]["expr"] = json!("7");
    let mut p = load(&d).unwrap();
    let before = p.clone();
    // Renommer une variable citée par un `per_size` : depuis la tranche 104 (US-09.7), la
    // référence est réécrite dans la même étape (la limite déclarée en tranche 103 est levée).
    p.apply(Command::RenameVariable {
        from: "B".into(),
        to: "D".into(),
    })
    .expect("renommage");
    assert_eq!(p.variable_size_expr("A", SizeId(7)), Some("D / 2 + 1"));
    assert_ne!(p, before);
    // Retirer une taille citée par une règle nommée (09.3b : pas encore de commande qui la
    // réécrive) reste refusé sans trace ; les valeurs par taille et celles du barème (E10,
    // tranche 105) suivent la taille.
    let before = p.clone();
    let err = p
        .apply(Command::RemoveSize { size: SizeId(9) })
        .expect_err("retrait de taille");
    assert!(err.to_string().contains("taille absente"), "{err}");
    assert_eq!(p, before, "refus sans trace");
}

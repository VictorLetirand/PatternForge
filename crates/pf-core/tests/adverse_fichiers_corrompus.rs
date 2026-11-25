//! US-17.6 - fichiers corrompus à l'ouverture.

mod adverse_support;
use adverse_support::*;

use pf_core::command::Command;
use pf_core::{persist, Project};
use pf_geometry::PointMm;
use serde_json::{json, Value};
use std::panic::{catch_unwind, AssertUnwindSafe};

// ---------------------------------------------------------------------------
// 1. Fichiers corrompus
// ---------------------------------------------------------------------------

#[test]
fn json_tronque_a_chaque_position_ne_panique_pas() {
    let text = persist::to_json(&rich_project()).unwrap();
    let full: Project = persist::from_json(&text).unwrap();
    // chaque début d'octet UTF-8, tous les 7 octets pour rester rapide
    let mut cut = 0;
    while cut < text.len() {
        if text.is_char_boundary(cut) {
            let part = &text[..cut];
            let r = catch_unwind(AssertUnwindSafe(|| persist::from_json(part)));
            match r {
                Err(_) => panic!("panic à la troncature {cut}"),
                Ok(Ok(p)) => assert_eq!(p, full, "troncature {cut} acceptée avec perte"),
                Ok(Err(e)) => assert!(!e.to_string().is_empty()),
            }
        }
        cut += 7;
    }
}

#[test]
fn mutations_structurelles_du_projet_riche() {
    let base: Value = serde_json::from_str(&persist::to_json(&rich_project()).unwrap()).unwrap();
    let (ok, refused) = run_mutations(&base, "v3");
    eprintln!("mutations v3 : {ok} ouvertures valides, {refused} refus");
    assert!(
        refused > ok / 4,
        "le contrôle des fichiers semble trop laxiste"
    );
}

#[test]
fn mutations_structurelles_du_projet_avec_cercles_et_arcs() {
    let base: Value =
        serde_json::from_str(&persist::to_json(&rich_project_with_arcs()).unwrap()).unwrap();
    let (ok, refused) = run_mutations(&base, "arcs");
    eprintln!("mutations arcs : {ok} ouvertures valides, {refused} refus");
    assert!(
        refused > ok / 4,
        "le contrôle des fichiers semble trop laxiste"
    );
}

#[test]
fn mutations_structurelles_du_projet_avec_courbes_lisses() {
    let base: Value =
        serde_json::from_str(&persist::to_json(&rich_project_with_splines()).unwrap()).unwrap();
    let (ok, refused) = run_mutations(&base, "splines");
    eprintln!("mutations splines : {ok} ouvertures valides, {refused} refus");
    assert!(
        refused > ok / 4,
        "le contrôle des fichiers semble trop laxiste"
    );
}

#[test]
fn mutations_structurelles_de_la_migration_v2() {
    let mut base: Value =
        serde_json::from_str(&persist::to_json(&rich_project()).unwrap()).unwrap();
    let obj = base.as_object_mut().unwrap();
    obj.insert("schema_version".into(), json!(2));
    obj.remove("pieces");
    let sheets = obj.get_mut("sheets").unwrap().as_array_mut().unwrap();
    sheets[0].as_object_mut().unwrap().insert(
        "piece".into(),
        json!({"name": "Ancienne", "quantity": 2, "material": "Coton"}),
    );
    assert!(persist::from_json(&serde_json::to_string(&base).unwrap()).is_ok());
    run_mutations(&base, "v2");
}

#[test]
fn schema_version_futur_zero_absent_ou_faux() {
    let mut base: Value =
        serde_json::from_str(&persist::to_json(&rich_project()).unwrap()).unwrap();
    let cases: Vec<(&str, Option<Value>)> = vec![
        ("absent", None),
        ("zero", Some(json!(0))),
        ("futur", Some(json!(7))),
        ("énorme", Some(json!(u64::MAX))),
        ("négatif", Some(json!(-3))),
        ("chaine", Some(json!("3"))),
        ("flottant", Some(json!(3.5))),
        ("flottant entier", Some(json!(3.0))),
        ("null", Some(Value::Null)),
    ];
    for (name, v) in cases {
        match v {
            None => {
                base.as_object_mut().unwrap().remove("schema_version");
            }
            Some(v) => {
                base["schema_version"] = v;
            }
        }
        let text = serde_json::to_string(&base).unwrap();
        let r = catch_unwind(AssertUnwindSafe(|| persist::from_json(&text)))
            .unwrap_or_else(|_| panic!("panic pour schema_version {name}"));
        let err = r.expect_err(&format!("schema_version {name} doit être refusé"));
        assert!(!err.to_string().is_empty());
    }
}

#[test]
fn fichiers_vides_binaires_et_non_projets() {
    for text in [
        "",
        " ",
        "\n",
        "null",
        "[]",
        "42",
        "\"x\"",
        "{}",
        "{\"format\":\"autre\"}",
        "{{{",
        "\u{0}",
        "\u{feff}",
    ] {
        let r = catch_unwind(AssertUnwindSafe(|| persist::from_json(text)))
            .unwrap_or_else(|_| panic!("panic sur {text:?}"));
        assert!(r.is_err(), "{text:?} ne doit pas s'ouvrir");
    }
    // nesting profond : ne doit pas faire déborder la pile
    let deep = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
    let r = catch_unwind(AssertUnwindSafe(|| persist::from_json(&deep)));
    assert!(r.unwrap().is_err());
    let deep_obj = format!("{}1{}", "{\"a\":".repeat(50_000), "}".repeat(50_000));
    assert!(
        catch_unwind(AssertUnwindSafe(|| persist::from_json(&deep_obj)))
            .unwrap()
            .is_err()
    );
}

#[test]
fn ouverture_de_fichiers_sur_disque_vide_binaire_utf8_invalide() {
    let dir = std::env::temp_dir().join(format!("pf-adverse-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("vide.pforge.json", vec![]),
        (
            "binaire.pforge.json",
            (0..=255u8).cycle().take(4096).collect(),
        ),
        ("utf8_invalide.pforge.json", vec![b'{', 0xff, 0xfe, b'}']),
        ("pdf.pforge.json", b"%PDF-1.4\n\x00\x01\x02".to_vec()),
    ];
    for (name, bytes) in cases {
        let path = dir.join(name);
        std::fs::write(&path, &bytes).unwrap();
        let r = catch_unwind(AssertUnwindSafe(|| persist::load(&path)))
            .unwrap_or_else(|_| panic!("panic sur {name}"));
        let e = r.expect_err(name);
        assert!(!e.to_string().is_empty());
        // le fichier refusé n'est jamais modifié
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    assert!(persist::load(&dir.join("absent.pforge.json")).is_err());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn champs_inconnus_de_projet_feuille_piece_conserves() {
    let mut v: Value = serde_json::from_str(&persist::to_json(&rich_project()).unwrap()).unwrap();
    v["gradation"] = json!({"tailles": [36, 38]});
    v["sheets"][0]["futur"] = json!("garde-moi");
    v["pieces"][0]["variante"] = json!([1, 2, 3]);
    let p = persist::from_json(&serde_json::to_string(&v).unwrap()).unwrap();
    let out: Value = serde_json::from_str(&persist::to_json(&p).unwrap()).unwrap();
    assert_eq!(out["gradation"], v["gradation"]);
    assert_eq!(out["sheets"][0]["futur"], v["sheets"][0]["futur"]);
    assert_eq!(out["pieces"][0]["variante"], v["pieces"][0]["variante"]);
}

#[test]
fn coordonnees_non_finies_ou_gigantesques_a_l_ouverture() {
    let base: Value = serde_json::from_str(&persist::to_json(&rich_project()).unwrap()).unwrap();
    for bad in [
        "1e308", "-1e308", "1e999", "-1e999", "NaN", "Infinity", "1e-400",
    ] {
        let text = serde_json::to_string(&base).unwrap();
        // remplace littéralement la première coordonnée x d'un point libre
        let marker = "\"x\":";
        let at = text.find(marker).expect("point libre");
        let end = at + marker.len() + text[at + marker.len()..].find([',', '}']).unwrap();
        let mutated = format!("{}\"x\":{}{}", &text[..at], bad, &text[end..]);
        let r = catch_unwind(AssertUnwindSafe(|| check_open(&mutated)))
            .unwrap_or_else(|_| panic!("panic avec x = {bad}"));
        r.unwrap_or_else(|m| panic!("x = {bad} : {m}"));
        if matches!(bad, "1e308" | "-1e308") {
            assert!(
                persist::from_json(&mutated).is_err(),
                "x = {bad} est hors de l'espace de travail et doit être refusé"
            );
        }
    }
}

#[test]
fn next_id_maximal_ne_fait_pas_paniquer_les_commandes() {
    let mut v: Value = serde_json::from_str(&persist::to_json(&rich_project()).unwrap()).unwrap();
    v["next_id"] = json!(u64::MAX);
    let text = serde_json::to_string(&v).unwrap();
    let Ok(mut p) = persist::from_json(&text) else {
        return; // refusé à l'ouverture : acceptable
    };
    let sheet = p.sheets[0].id;
    let r = catch_unwind(AssertUnwindSafe(|| {
        p.apply(Command::AddPoint {
            sheet,
            at: PointMm::new(1.0, 1.0),
        })
    }));
    assert!(r.is_ok(), "panic : débordement du compteur d'identifiants");
}

#[test]
fn mutations_structurelles_du_projet_avec_lignes_d_aide() {
    let base: Value =
        serde_json::from_str(&persist::to_json(&rich_project_with_guides()).unwrap()).unwrap();
    let (ok, refused) = run_mutations(&base, "v3 + lignes d'aide");
    eprintln!("mutations lignes d'aide : {ok} ouvertures valides, {refused} refus");
    assert!(
        refused > ok / 4,
        "le contrôle des fichiers semble trop laxiste"
    );
}

#[test]
fn cles_dupliquees_dans_le_json_ne_sont_pas_acceptees_silencieusement() {
    let text = persist::to_json(&rich_project()).unwrap();
    // deuxième « name » au niveau projet : la dernière valeur ne doit pas gagner en silence
    let dup = text.replacen(
        "{
",
        "{
  \"name\": \"Premier\",
",
        1,
    );
    match persist::from_json(&dup) {
        Err(_) => {}
        Ok(p) => panic!("clé dupliquée acceptée, nom lu : {:?}", p.name),
    }
}

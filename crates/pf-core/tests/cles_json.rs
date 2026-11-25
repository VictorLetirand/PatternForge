//! D-02 : une clé JSON dupliquée est refusée (chargeur, `pf-validate`, presse-papiers).

use pf_core::clipboard::Fragment;
use pf_core::persist;

const BASE: &str = include_str!("../../../examples/piece-crans-droit-fil.pforge.json");

fn refus(text: &str) -> String {
    match persist::from_json(text) {
        Ok(p) => panic!("clé dupliquée acceptée, projet « {} »", p.name),
        Err(e) => e.to_string(),
    }
}

fn dupliquer(from: &str, to: &str) -> String {
    assert!(BASE.contains(from), "motif absent : {from}");
    BASE.replacen(from, to, 1)
}

#[test]
fn base_sans_doublon_est_lue() {
    persist::from_json(BASE).expect("l'exemple est valide");
}

#[test]
fn doublon_au_niveau_du_projet_refuse_et_nomme_la_cle() {
    let msg = refus(&dupliquer(
        "\"name\": \"Pièce avec crans et droit-fil\",",
        "\"name\": \"Premier\",\n  \"name\": \"Pièce avec crans et droit-fil\",",
    ));
    assert!(msg.contains("« name »"), "{msg}");
    assert!(msg.contains("line 5"), "ligne attendue : {msg}");
}

#[test]
fn doublon_de_valeur_identique_refuse_aussi() {
    let msg = refus(&dupliquer(
        "\"schema_version\": 6,",
        "\"schema_version\": 6,\n  \"schema_version\": 6,",
    ));
    assert!(msg.contains("« schema_version »"), "{msg}");
}

#[test]
fn doublon_imbrique_refuse() {
    let msg = refus(&dupliquer(
        "\"x\": 0.0,",
        "\"x\": 0.0,\n            \"x\": 1.0,",
    ));
    assert!(msg.contains("« x »"), "{msg}");
}

#[test]
fn doublon_dans_un_tableau_d_objets_refuse() {
    // deuxième point de la liste : « label » deux fois
    let msg = refus(&dupliquer(
        "\"label\": \"B\",",
        "\"label\": \"B\",\n          \"label\": \"B2\",",
    ));
    assert!(msg.contains("« label »"), "{msg}");
}

#[test]
fn cles_differant_par_la_casse_sont_distinctes() {
    assert!(persist::reject_duplicate_keys(r#"{"a":1,"A":2,"b":{"a":1,"A":2}}"#).is_ok());
    // dans un projet : « Schema_Version » est une clé distincte, jamais lue comme doublon
    let text = dupliquer(
        "\"schema_version\": 6,",
        "\"schema_version\": 6,
  \"Schema_Version\": 4,",
    );
    assert!(persist::reject_duplicate_keys(&text).is_ok());
    if let Err(e) = persist::from_json(&text) {
        assert!(!e.to_string().contains("dupliquée"), "{e}");
    }
}

#[test]
fn memes_cles_dans_des_objets_differents_sont_permises() {
    assert!(
        persist::reject_duplicate_keys(r#"{"a":{"k":1},"b":{"k":2},"c":[{"k":1},{"k":2}]}"#)
            .is_ok()
    );
}

#[test]
fn json_invalide_reste_signale_comme_avant() {
    let msg = refus("{\"a\": ");
    assert!(msg.contains("JSON invalide"), "{msg}");
}

#[test]
fn chargement_refuse_sans_trace() {
    let dir = std::env::temp_dir().join(format!("pf-cles-json-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("dup.pforge.json");
    let text = dupliquer(
        "\"schema_version\": 6,",
        "\"schema_version\": 6,\n  \"schema_version\": 6,",
    );
    std::fs::write(&file, &text).unwrap();
    let err = persist::load(&file)
        .expect_err("doublon refusé")
        .to_string();
    assert!(err.contains("« schema_version »"), "{err}");
    assert_eq!(std::fs::read(&file).unwrap(), text.as_bytes());
    let names: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
    assert_eq!(names.len(), 1, "aucun fichier créé");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn pf_validate_signale_le_doublon_avec_sa_position() {
    let dir = std::env::temp_dir().join(format!("pf-cles-json-val-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("dup.pforge.json");
    std::fs::write(
        &file,
        dupliquer(
            "\"name\": \"Pièce avec crans et droit-fil\",",
            "\"name\": \"Premier\",\n  \"name\": \"Second\",",
        ),
    )
    .unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_pf-validate"))
        .arg(&file)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert_eq!(out.status.code(), Some(1), "{text}");
    assert!(
        text.contains("« name »") && text.contains("(ligne 5"),
        "{text}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn fragment_du_presse_papiers_avec_doublon_refuse() {
    let err = Fragment::from_json(r#"{"format":"patternforge-clipboard","format":"x"}"#)
        .expect_err("doublon refusé")
        .to_string();
    assert!(err.contains("« format »"), "{err}");
}

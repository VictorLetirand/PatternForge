//! US-17.5 : journal d'erreurs local, assaini et borné. Valeurs attendues écrites à la main.

use pf_core::error_log::{
    format_utc, sanitize, ErrorLog, MAX_BYTES, MAX_ENTRIES, MAX_KIND_CHARS, MAX_MESSAGE_CHARS,
};
use serde_json::json;

#[test]
fn horodatage_utc() {
    assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
    // 11016 jours après 1970 : 29 février 2000 (année bissextile divisible par 400).
    assert_eq!(format_utc(951_782_400), "2000-02-29T00:00:00Z");
    assert_eq!(
        format_utc(951_782_400 + 3_600 * 23 + 60 * 59 + 59),
        "2000-02-29T23:59:59Z"
    );
    assert_eq!(format_utc(1_700_000_000), "2023-11-14T22:13:20Z");
    // 1er mars 2100 : 2100 n'est pas bissextile (4 106 400 000 s / 86 400 = 47 541 jours).
    assert_eq!(format_utc(47_541 * 86_400), "2100-03-01T00:00:00Z");
}

#[test]
fn chemins_masques_meme_avec_des_espaces() {
    assert_eq!(
        sanitize(
            r"C:\Users\Jean Dupont\Docs\Veste Alice.pforge.json : accès refusé",
            &[]
        ),
        "<chemin> : accès refusé"
    );
    assert_eq!(
        sanitize(
            "Impossible d'ouvrir /home/jean/projets/x.pforge.json introuvable",
            &[]
        ),
        "Impossible d'ouvrir <chemin> introuvable"
    );
    assert_eq!(
        sanitize(r"\\serveur\partage\mon plan.pdf : Erreur", &[]),
        "<chemin> : Erreur"
    );
    assert_eq!(
        sanitize(r"échec dans dossier\sous\x ici", &[]),
        "échec dans <chemin> ici"
    );
    assert_eq!(sanitize("lu : D:/data/a b/c.svg", &[]), "lu : <chemin>");
    assert_eq!(sanitize("voir ~/perso/notes", &[]), "voir <chemin>");
}

#[test]
fn texte_ordinaire_inchange() {
    for text in [
        "Refusé : pas de feuille active",
        "et/ou 1/2 de la longueur, rapport 3:1",
        "Point introuvable (id 12)",
        "Version 5 attendue",
    ] {
        assert_eq!(sanitize(text, &[]), text);
    }
}

#[test]
fn noms_entre_guillemets_et_valeurs_sensibles_masques() {
    assert_eq!(
        sanitize("Feuille « Devant » introuvable", &[]),
        "Feuille «…» introuvable"
    );
    assert_eq!(
        sanitize("Pièce \"Manche gauche\" vide", &[]),
        "Pièce \"…\" vide"
    );
    assert_eq!(
        sanitize("Pièce « jamais refermée et le reste", &[]),
        "Pièce «…"
    );
    let secrets = [
        "Alice Martin".to_string(),
        " jdupont ".to_string(),
        String::new(),
    ];
    assert_eq!(
        sanitize("Dossier de JDUPONT, projet alice martin", &secrets),
        "Dossier de <…>, projet <…>"
    );
}

#[test]
fn espaces_longueur_et_controles() {
    assert_eq!(sanitize("a\n\tb \u{0}  c", &[]), "a b c");
    let long = "é".repeat(500);
    let out = sanitize(&long, &[]);
    assert_eq!(out.chars().count(), MAX_MESSAGE_CHARS);
    assert!(out.ends_with('…'));
    assert!(out.starts_with("éé"));
    assert_eq!(sanitize("   \n ", &[]), "");
}

#[test]
fn enregistrement_doublons_et_genre() {
    let mut log = ErrorLog::default();
    assert!(log.record(10, "Save", "échec", &[]));
    assert!(!log.record(11, "save", "échec", &[]), "doublon consécutif");
    assert!(log.record(12, "open", "échec", &[]), "autre genre");
    assert!(log.record(13, "open", "autre", &[]));
    assert!(!log.record(14, "open", "  \n", &[]), "message vide");
    assert!(log.record(15, "chemin\\evil: ../x!", "m", &[]));
    let kinds: Vec<&str> = log.entries().iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(kinds, vec!["save", "open", "open", "cheminevil..x"]);
    assert!("k".repeat(80).chars().count() > MAX_KIND_CHARS);
    assert!(log.record(16, &"k".repeat(80), "m", &[]));
    assert_eq!(
        log.entries().last().unwrap().kind.chars().count(),
        MAX_KIND_CHARS
    );
}

#[test]
fn rotation_par_nombre() {
    let mut log = ErrorLog::default();
    for i in 0..(MAX_ENTRIES + 30) {
        assert!(log.record(i as u64, "t", &format!("erreur numero {i}"), &[]));
    }
    assert_eq!(log.len(), MAX_ENTRIES);
    // Les 30 plus anciennes (0 à 29) sont parties.
    assert_eq!(log.entries()[0].message, "erreur numero 30");
    assert_eq!(log.entries()[MAX_ENTRIES - 1].message, "erreur numero 129");
}

#[test]
fn rotation_par_taille() {
    let mut log = ErrorLog::default();
    for i in 0..MAX_ENTRIES {
        // Messages de la longueur maximale : 100 entrées dépasseraient la borne en octets.
        let mut msg = format!("{i:03} ");
        msg.push_str(&"x".repeat(MAX_MESSAGE_CHARS));
        log.record(1_700_000_000 + i as u64, "longtemps", &msg, &[]);
        assert!(log.to_value().to_string().len() <= MAX_BYTES, "après {i}");
    }
    assert!(
        log.len() < MAX_ENTRIES,
        "la borne de taille a retiré des entrées"
    );
    let last = log.entries().last().unwrap();
    assert!(last.message.starts_with("099 "), "la plus récente reste");
    assert!(
        !log.entries()[0].message.starts_with("000 "),
        "la plus ancienne est partie"
    );
}

#[test]
fn aller_retour_et_relecture_tolerante() {
    let mut log = ErrorLog::default();
    log.record(5, "save", "échec A", &[]);
    log.record(6, "open", "échec B", &[]);
    assert_eq!(ErrorLog::from_value(&log.to_value()), log);

    let hostile = json!([
        {"t": 1, "k": "ok", "m": "bon"},
        {"t": "x", "k": "ok", "m": "temps invalide"},
        {"t": 2, "k": 7, "m": "genre invalide"},
        {"t": 3, "k": "a b", "m": "   "},
        {"t": 4, "k": "ok", "m": r"C:\Users\Zoe Dupont\a.pforge.json : refusé"},
        42,
        null
    ]);
    let read = ErrorLog::from_value(&hostile);
    let messages: Vec<&str> = read.entries().iter().map(|e| e.message.as_str()).collect();
    assert_eq!(messages, vec!["bon", "<chemin> : refusé"]);
    assert_eq!(ErrorLog::from_value(&json!({"t": 1})), ErrorLog::default());
    assert_eq!(ErrorLog::from_value(&json!("texte")), ErrorLog::default());

    // Un réglage édité à la main avec 500 entrées est borné à la lecture.
    let many: Vec<_> = (0..500)
        .map(|i| json!({"t": i, "k": "t", "m": format!("e{i}")}))
        .collect();
    let read = ErrorLog::from_value(&json!(many));
    assert_eq!(read.len(), MAX_ENTRIES);
    assert_eq!(read.entries()[0].message, "e400");
}

#[test]
fn export_une_ligne_par_entree() {
    let mut log = ErrorLog::default();
    log.record(0, "save", "premier", &[]);
    log.record(1_700_000_000, "open", "second", &[]);
    assert_eq!(
        log.export_lines(),
        vec![
            "1970-01-01T00:00:00Z [save] premier".to_string(),
            "2023-11-14T22:13:20Z [open] second".to_string()
        ]
    );
    log.clear();
    assert!(log.is_empty() && log.export_lines().is_empty());
}

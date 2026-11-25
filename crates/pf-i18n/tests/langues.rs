//! US-19.3 : fichiers de langue chargés sans recompiler. Valeurs écrites à la main ; l'oracle de la
//! campagne adverse ne lit aucune table du code testé.

use pf_i18n::langfile::{self, LangFileError};
use pf_i18n::{lookup, set_lang, tr, tr_args, Lang};
use std::path::PathBuf;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pf_langues_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn doc(code: &str, messages: &str) -> String {
    format!(
        r#"{{"format":"patternforge-lang","version":1,"code":"{code}","name":"Prueba","messages":{{{messages}}}}}"#
    )
}

#[test]
fn le_gabarit_est_complet_a_jour_et_se_recharge() {
    let generated = langfile::template_json();
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/langues/gabarit.lang.json");
    let on_disk = std::fs::read_to_string(&path).expect("gabarit versionné");
    assert_eq!(
        on_disk.replace("\r\n", "\n"),
        generated,
        "relancer `cargo run -q -p pf-i18n --example generer_gabarit`"
    );
    let lang = langfile::parse(&generated).expect("le gabarit est un fichier valide");
    assert_eq!(lang.code, "xx");
    assert_eq!(
        lang.messages.len(),
        Lang::Fr.catalog().len(),
        "toutes les clés"
    );
    assert_eq!(lang.messages["menu.file.title"], "File");
}

#[test]
fn les_cles_absentes_retombent_sur_l_anglais_puis_le_francais() {
    let dir = temp("repli");
    std::fs::write(
        dir.join("es.lang.json"),
        doc("es", r#""menu.file.title":"Archivo""#),
    )
    .unwrap();
    let report = langfile::load_dir(&dir);
    assert!(report.refused.is_empty());
    langfile::set_available(report.loaded);
    assert_eq!(langfile::available(), vec![("es".into(), "Prueba".into())]);
    assert!(langfile::activate("es"));
    assert_eq!(tr("menu.file.title"), "Archivo");
    assert_eq!(tr("menu.file.save"), "Save", "repli anglais");
    assert_eq!(
        tr_args("dialog.unsaved.body", &[("name", "X")]),
        "The project \"X\" has been modified. Do you want to save it?"
    );
    // Rien en anglais : repli français (clé inventée ajoutée seulement au français n'existe pas :
    // on vérifie la chaîne par `lookup`), jamais la clé brute pour une clé du catalogue.
    for (key, _) in Lang::Fr.catalog() {
        assert_ne!(tr(key), *key, "clé brute à l'écran : {key}");
    }
    assert_eq!(
        lookup(Lang::En, "menu.file.title"),
        "File",
        "catalogue intact"
    );
    set_lang(Lang::Fr);
    assert_eq!(
        tr("menu.file.title"),
        "Fichier",
        "choisir le français désactive la langue"
    );
    assert_eq!(langfile::active_code(), None);
    assert!(!langfile::activate("zz"), "code inconnu refusé sans effet");
    assert_eq!(tr("menu.file.title"), "Fichier");
}

#[test]
fn chaque_refus_a_un_code_et_un_message_dans_les_deux_langues() {
    let cases: Vec<(String, &str)> = vec![
        ("pas du json".into(), "not_json"),
        ("[1]".into(), "not_an_object"),
        (r#"{"format":"x"}"#.into(), "wrong_format"),
        (
            r#"{"format":"patternforge-lang","version":2}"#.into(),
            "bad_version",
        ),
        (
            r#"{"format":"patternforge-lang","version":1,"code":"es","name":"a","messages":{},"x":1}"#
                .into(),
            "unknown_field",
        ),
        (
            r#"{"format":"patternforge-lang","version":1,"name":"a","messages":{}}"#.into(),
            "missing_field",
        ),
        (doc("e", ""), "bad_code"),
        (doc("fr", ""), "reserved_code"),
        (doc("EN", ""), "reserved_code"),
        (
            r#"{"format":"patternforge-lang","version":1,"code":"es","name":"  ","messages":{}}"#
                .into(),
            "bad_name",
        ),
        (
            r#"{"format":"patternforge-lang","version":1,"code":"es","name":"a","messages":[]}"#
                .into(),
            "bad_messages",
        ),
        (doc("es", r#""cle.inconnue":"x""#), "unknown_key"),
        (doc("es", r#""menu.file.title":""#.to_string().as_str()), "not_json"),
        (doc("es", r#""menu.file.title":"""#), "bad_text"),
        (doc("es", r#""menu.file.title":3"#), "bad_text"),
        (doc("es", r#""dialog.unsaved.body":"sin marca""#), "placeholder_mismatch"),
        (
            doc("es", r#""dialog.unsaved.body":"{name} {autre}""#),
            "placeholder_mismatch",
        ),
    ];
    for (text, expected) in cases {
        let err = langfile::parse(&text).expect_err(&text);
        assert_eq!(err.code(), expected, "{text}");
        for lang in Lang::ALL {
            set_lang(lang);
            let msg = err.message();
            assert!(!msg.is_empty() && !msg.contains("lang.err."), "{msg}");
            assert!(!msg.contains('{'), "marque non remplie : {msg}");
        }
    }
    set_lang(Lang::Fr);
    assert_eq!(
        langfile::parse(&"x".repeat(3 * 1024 * 1024)).unwrap_err(),
        LangFileError::TooBig
    );
}

#[test]
fn un_fichier_invalide_ne_charge_rien_et_les_autres_restent() {
    let dir = temp("melange");
    std::fs::write(
        dir.join("es.lang.json"),
        doc("es", r#""menu.file.title":"A""#),
    )
    .unwrap();
    std::fs::write(dir.join("it.lang.json"), doc("it", r#""cle.inconnue":"B""#)).unwrap();
    std::fs::write(dir.join("de.lang.json"), doc("es", "")).unwrap(); // code ≠ nom
    std::fs::write(dir.join("notes.txt"), "ignoré").unwrap();
    let report = langfile::load_dir(&dir);
    let loaded: Vec<&str> = report.loaded.iter().map(|l| l.code.as_str()).collect();
    assert_eq!(loaded, vec!["es"]);
    let refused: Vec<(&str, &str)> = report
        .refused
        .iter()
        .map(|(f, e)| (f.as_str(), e.code()))
        .collect();
    assert_eq!(
        refused,
        vec![
            ("de.lang.json", "code_file_mismatch"),
            ("it.lang.json", "unknown_key")
        ]
    );
    assert!(langfile::load_dir(&dir.join("absent")).loaded.is_empty());
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Campagne adverse : 240 séquences (graine fixe) écriture d'un fichier corrompu ou valide →
/// chargement → activation → lecture. L'oracle est la liste des corruptions : chacune a un
/// verdict connu d'avance.
#[test]
fn campagne_adverse_fichiers_de_langue() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let (mut accepted, mut refused_count) = (0, 0);
    let mut seen_codes = std::collections::BTreeSet::new();
    for n in 0..240 {
        let dir = temp(&format!("adv{}", n % 4));
        let code = ["es", "pt-BR", "de", "it", "nl", "zh-Hant"][rng.below(6)];
        let valid = doc(
            code,
            r#""menu.file.title":"Titulo","menu.file.save":"Guardar""#,
        );
        // (contenu, verdict attendu : None = accepté, Some(code) = refus)
        let (content, expected): (Vec<u8>, Option<&str>) = match rng.below(14) {
            0 => (valid.clone().into_bytes(), None),
            1 => {
                let cut = 1 + rng.below(valid.len() - 1);
                (valid.as_bytes()[..cut].to_vec(), Some("not_json"))
            }
            2 => (
                valid.replace("patternforge-lang", "autre").into_bytes(),
                Some("wrong_format"),
            ),
            3 => (
                valid.replace("\"version\":1", "\"version\":2").into_bytes(),
                Some("bad_version"),
            ),
            4 => (
                valid
                    .replace("\"version\":1", "\"version\":\"1\"")
                    .into_bytes(),
                Some("bad_version"),
            ),
            5 => (
                valid
                    .replace(&format!("\"code\":\"{code}\""), "\"code\":\"x y\"")
                    .into_bytes(),
                Some("bad_code"),
            ),
            6 => (
                valid
                    .replace(&format!("\"code\":\"{code}\""), "\"code\":\"fr\"")
                    .into_bytes(),
                Some("reserved_code"),
            ),
            7 => (
                valid
                    .replace("menu.file.title", "menu.nexiste.pas")
                    .into_bytes(),
                Some("unknown_key"),
            ),
            8 => (
                valid.replace("\"Titulo\"", "null").into_bytes(),
                Some("bad_text"),
            ),
            9 => (
                format!("{},\"extra\":1}}", &valid[..valid.len() - 1]).into_bytes(),
                Some("unknown_field"),
            ),
            10 => {
                let mut bytes = valid.clone().into_bytes();
                let at = rng.below(bytes.len());
                bytes[at] = 0xFF; // octet invalide en UTF-8 : lecture refusée ou JSON invalide
                (bytes, Some("any"))
            }
            11 => (
                valid
                    .replace("\"name\":\"Prueba\"", "\"name\":\"\"")
                    .into_bytes(),
                Some("bad_name"),
            ),
            12 => (vec![b'['; 5000], Some("not_json")),
            _ => (
                vec![b' '; (langfile::MAX_BYTES as usize) + 1 + rng.below(100)],
                Some("too_big"),
            ),
        };
        std::fs::write(dir.join(format!("{code}.lang.json")), &content).unwrap();
        let report = langfile::load_dir(&dir);
        match expected {
            None => {
                accepted += 1;
                assert_eq!(report.loaded.len(), 1, "n={n}");
                assert!(report.refused.is_empty(), "n={n}");
                langfile::set_available(report.loaded);
                assert!(langfile::activate(code));
                assert_eq!(tr("menu.file.title"), "Titulo");
                assert_eq!(tr("menu.file.save"), "Guardar");
                assert_eq!(tr("menu.edit.title"), "Edit");
                set_lang(Lang::Fr);
                assert_eq!(tr("menu.edit.title"), "Édition");
            }
            Some(want) => {
                refused_count += 1;
                assert!(report.loaded.is_empty(), "langue à moitié chargée, n={n}");
                assert_eq!(report.refused.len(), 1, "n={n}");
                let got = report.refused[0].1.code();
                seen_codes.insert(got);
                if want != "any" {
                    assert_eq!(got, want, "n={n}");
                }
                // Rien n'a changé pour l'utilisateur.
                langfile::set_available(report.loaded);
                assert_eq!(langfile::available(), vec![]);
                assert_eq!(langfile::active_code(), None);
                assert!(!report.refused[0].1.message().is_empty());
            }
        }
    }
    assert!(
        accepted >= 8 && refused_count >= 150,
        "{accepted}/{refused_count}"
    );
    for want in [
        "not_json",
        "wrong_format",
        "bad_version",
        "bad_code",
        "reserved_code",
        "unknown_key",
        "bad_text",
        "unknown_field",
        "bad_name",
        "too_big",
    ] {
        assert!(seen_codes.contains(want), "refus jamais couvert : {want}");
    }
}
